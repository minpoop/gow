//! Dev tool: rasterises `gowring-core`'s HUD draw list with flat placeholder shapes so the layout can be
//! judged without the game. It is a LAYOUT PREVIEW — not gameplay, not Ragnarök art.
//!
//!   cargo run -p gowring-core --features preview --bin hud-preview -- out.png [width height] [--match backdrop.png]
//!
//! `--match` draws only the player's corner cluster in the state of a reference screenshot (full health, Rage 89 %
//! and ready, both runics ready) over that screenshot (an RGB PNG of the same size), to check the layout against it.
//!
//! `--art DIR` loads the Ragnarök pictures `assets.json` picks (`ragnarok_pick`) from DIR — the player's own exported
//! `ragnarok_ui` folder, never shipped — so the HUD is drawn with Ragnarök's pieces as the overlay draws it in game.

use gowring_core::hud::{self, Align, DrawCmd, EnemyBar, HudState, RunicHud, Screen};
use gowring_core::vector::{self, Prim};
use gowring_core::{Data, Vec3};

struct Canvas {
    w: usize,
    h: usize,
    px: Vec<[f32; 3]>,
    font: fontdue::Font,
    /// Ragnarök pictures by HUD asset id: (width, height, RGBA).
    tex: std::sync::Arc<std::collections::HashMap<&'static str, (usize, usize, Vec<u8>)>>,
}

fn load_png(path: &std::path::Path) -> Option<(usize, usize, Vec<u8>)> {
    let mut dec = png::Decoder::new(std::io::BufReader::new(std::fs::File::open(path).ok()?));
    dec.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
    let mut r = dec.read_info().ok()?;
    let mut buf = vec![0; r.output_buffer_size()];
    let info = r.next_frame(&mut buf).ok()?;
    let rgba = match info.color_type {
        png::ColorType::Rgba => buf[..info.buffer_size()].to_vec(),
        png::ColorType::Rgb => buf[..info.buffer_size()].chunks_exact(3).flat_map(|p| [p[0], p[1], p[2], 255]).collect(),
        png::ColorType::GrayscaleAlpha => buf[..info.buffer_size()].chunks_exact(2).flat_map(|p| [p[0], p[0], p[0], p[1]]).collect(),
        png::ColorType::Grayscale => buf[..info.buffer_size()].iter().flat_map(|g| [*g, *g, *g, 255]).collect(),
        _ => return None,
    };
    Some((info.width as usize, info.height as usize, rgba))
}

fn rgb(c: u32) -> [f32; 3] {
    [((c >> 16) & 255) as f32 / 255.0, ((c >> 8) & 255) as f32 / 255.0, (c & 255) as f32 / 255.0]
}

impl Canvas {
    fn blend(&mut self, x: i64, y: i64, c: [f32; 3], a: f32) {
        if x < 0 || y < 0 || x >= self.w as i64 || y >= self.h as i64 {
            return;
        }
        let p = &mut self.px[y as usize * self.w + x as usize];
        for i in 0..3 {
            p[i] = p[i] * (1.0 - a) + c[i] * a;
        }
    }
    fn rect(&mut self, x: f32, y: f32, w: f32, h: f32, c: u32, a: f32) {
        for yy in y.floor() as i64..(y + h).ceil() as i64 {
            for xx in x.floor() as i64..(x + w).ceil() as i64 {
                self.blend(xx, yy, rgb(c), a);
            }
        }
    }
    fn outline(&mut self, x: f32, y: f32, w: f32, h: f32, c: u32, a: f32) {
        self.rect(x, y, w, 2.0, c, a);
        self.rect(x, y + h - 2.0, w, 2.0, c, a);
        self.rect(x, y, 2.0, h, c, a);
        self.rect(x + w - 2.0, y, 2.0, h, c, a);
    }
    fn disc(&mut self, cx: f32, cy: f32, r: f32, r_in: f32, c: u32, a: f32, sweep_from_top: Option<f32>) {
        for yy in (cy - r).floor() as i64..(cy + r).ceil() as i64 {
            for xx in (cx - r).floor() as i64..(cx + r).ceil() as i64 {
                let (dx, dy) = (xx as f32 + 0.5 - cx, yy as f32 + 0.5 - cy);
                let d = (dx * dx + dy * dy).sqrt();
                if d > r || d < r_in {
                    continue;
                }
                if let Some(frac) = sweep_from_top {
                    let ang = (dy.atan2(dx) + std::f32::consts::FRAC_PI_2).rem_euclid(std::f32::consts::TAU) / std::f32::consts::TAU;
                    if ang > frac {
                        continue;
                    }
                }
                self.blend(xx, yy, rgb(c), a);
            }
        }
    }
    /// Fill a triangle with 3x3 supersampling at the edges.
    fn tri(&mut self, p: [(f32, f32); 3], c: u32, a: f32) {
        let (min_x, max_x) = (p.iter().map(|q| q.0).fold(f32::MAX, f32::min).floor() as i64, p.iter().map(|q| q.0).fold(f32::MIN, f32::max).ceil() as i64);
        let (min_y, max_y) = (p.iter().map(|q| q.1).fold(f32::MAX, f32::min).floor() as i64, p.iter().map(|q| q.1).fold(f32::MIN, f32::max).ceil() as i64);
        let area = (p[1].0 - p[0].0) * (p[2].1 - p[0].1) - (p[2].0 - p[0].0) * (p[1].1 - p[0].1);
        if area.abs() < 1e-6 {
            return;
        }
        let inside = |x: f32, y: f32| {
            let w0 = ((p[1].0 - x) * (p[2].1 - y) - (p[2].0 - x) * (p[1].1 - y)) / area;
            let w1 = ((p[2].0 - x) * (p[0].1 - y) - (p[0].0 - x) * (p[2].1 - y)) / area;
            let w2 = 1.0 - w0 - w1;
            w0 >= 0.0 && w1 >= 0.0 && w2 >= 0.0
        };
        for yy in min_y..=max_y {
            for xx in min_x..=max_x {
                let mut hit = 0;
                for sy in 0..3 {
                    for sx in 0..3 {
                        if inside(xx as f32 + (sx as f32 + 0.5) / 3.0, yy as f32 + (sy as f32 + 0.5) / 3.0) {
                            hit += 1;
                        }
                    }
                }
                if hit > 0 {
                    self.blend(xx, yy, rgb(c), a * hit as f32 / 9.0);
                }
            }
        }
    }
    fn prims(&mut self, v: &[Prim]) {
        for p in v {
            match p {
                Prim::Tri { p, color, alpha } => self.tri(*p, *color, *alpha),
                Prim::Line { a, b, thick, color, alpha } => {
                    let (dx, dy) = (b.0 - a.0, b.1 - a.1);
                    let len = (dx * dx + dy * dy).sqrt().max(1e-4);
                    let (nx, ny) = (-dy / len * thick / 2.0, dx / len * thick / 2.0);
                    let q = [(a.0 + nx, a.1 + ny), (b.0 + nx, b.1 + ny), (b.0 - nx, b.1 - ny), (a.0 - nx, a.1 - ny)];
                    self.tri([q[0], q[1], q[2]], *color, *alpha);
                    self.tri([q[0], q[2], q[3]], *color, *alpha);
                }
                Prim::Circle { c, r, fill, thick, color, alpha } => {
                    if *fill {
                        self.disc(c.0, c.1, *r, 0.0, *color, *alpha, None);
                    } else {
                        self.disc(c.0, c.1, *r + thick / 2.0, (*r - thick / 2.0).max(0.0), *color, *alpha, None);
                    }
                }
                Prim::Poly { pts, color, alpha } => self.poly(pts, *color, *alpha),
                Prim::Tex { asset, pts, uv, color, alpha } => self.quad(asset, pts, uv, *color, *alpha),
                Prim::Grad { x0, y0, x1, y1, left, right, alpha } => {
                    let (l, r) = (rgb(*left), rgb(*right));
                    for yy in y0.floor() as i64..y1.ceil() as i64 {
                        let cy = (y1.min(yy as f32 + 1.0) - y0.max(yy as f32)).clamp(0.0, 1.0);
                        for xx in x0.floor() as i64..x1.ceil() as i64 {
                            let cx = (x1.min(xx as f32 + 1.0) - x0.max(xx as f32)).clamp(0.0, 1.0);
                            let t = ((xx as f32 + 0.5 - x0) / (x1 - x0).max(1e-3)).clamp(0.0, 1.0);
                            self.blend(xx, yy, [l[0] + (r[0] - l[0]) * t, l[1] + (r[1] - l[1]) * t, l[2] + (r[2] - l[2]) * t], alpha * cx * cy);
                        }
                    }
                }
            }
        }
    }
    /// A Ragnarök picture on a quad (a parallelogram), sampled bilinearly with wrapping like the game's sampler, one
    /// sample at each pixel's centre with half-open edges as a GPU fills triangles (so quads that share an edge, like
    /// the plates' four turned quarters, meet without a seam); the picture's white is tinted `c`.
    fn quad(&mut self, asset: &str, pts: &[(f32, f32); 4], uv: &[(f32, f32); 4], c: u32, a: f32) {
        let tex = self.tex.clone();
        let Some((tw, th, rgba)) = tex.get(asset) else { return };
        let (e1, e2) = ((pts[1].0 - pts[0].0, pts[1].1 - pts[0].1), (pts[3].0 - pts[0].0, pts[3].1 - pts[0].1));
        let det = e1.0 * e2.1 - e1.1 * e2.0;
        if det.abs() < 1e-6 {
            return;
        }
        let sample = |u: f32, v: f32| -> [f32; 4] {
            let (x, y) = (u * *tw as f32 - 0.5, v * *th as f32 - 0.5);
            let (x0, y0) = (x.floor(), y.floor());
            let (fx, fy) = (x - x0, y - y0);
            let px = |xi: i64, yi: i64| {
                let (xi, yi) = (xi.rem_euclid(*tw as i64) as usize, yi.rem_euclid(*th as i64) as usize);
                let i = (yi * tw + xi) * 4;
                [rgba[i] as f32 / 255.0, rgba[i + 1] as f32 / 255.0, rgba[i + 2] as f32 / 255.0, rgba[i + 3] as f32 / 255.0]
            };
            let (a, b, cc, d) = (px(x0 as i64, y0 as i64), px(x0 as i64 + 1, y0 as i64), px(x0 as i64, y0 as i64 + 1), px(x0 as i64 + 1, y0 as i64 + 1));
            let mut o = [0.0; 4];
            for k in 0..4 {
                o[k] = (a[k] * (1.0 - fx) + b[k] * fx) * (1.0 - fy) + (cc[k] * (1.0 - fx) + d[k] * fx) * fy;
            }
            o
        };
        let (min_x, max_x) = (pts.iter().map(|q| q.0).fold(f32::MAX, f32::min).floor() as i64, pts.iter().map(|q| q.0).fold(f32::MIN, f32::max).ceil() as i64);
        let (min_y, max_y) = (pts.iter().map(|q| q.1).fold(f32::MAX, f32::min).floor() as i64, pts.iter().map(|q| q.1).fold(f32::MIN, f32::max).ceil() as i64);
        let tint = rgb(c);
        for yy in min_y..=max_y {
            for xx in min_x..=max_x {
                let (dx, dy) = (xx as f32 + 0.5 - pts[0].0, yy as f32 + 0.5 - pts[0].1);
                let s = (dx * e2.1 - dy * e2.0) / det;
                let t = (e1.0 * dy - e1.1 * dx) / det;
                if !(0.0..1.0).contains(&s) || !(0.0..1.0).contains(&t) {
                    continue;
                }
                let u = uv[0].0 + s * (uv[1].0 - uv[0].0) + t * (uv[3].0 - uv[0].0);
                let v = uv[0].1 + s * (uv[1].1 - uv[0].1) + t * (uv[3].1 - uv[0].1);
                let p = sample(u, v);
                if p[3] > 0.0 {
                    self.blend(xx, yy, [p[0] * tint[0], p[1] * tint[1], p[2] * tint[2]], a * p[3]);
                }
            }
        }
    }
    /// Fill a convex polygon, 3x3 supersampled.
    fn poly(&mut self, pts: &[(f32, f32)], c: u32, a: f32) {
        if pts.len() < 3 {
            return;
        }
        let (min_x, max_x) = (pts.iter().map(|q| q.0).fold(f32::MAX, f32::min).floor() as i64, pts.iter().map(|q| q.0).fold(f32::MIN, f32::max).ceil() as i64);
        let (min_y, max_y) = (pts.iter().map(|q| q.1).fold(f32::MAX, f32::min).floor() as i64, pts.iter().map(|q| q.1).fold(f32::MIN, f32::max).ceil() as i64);
        let n = pts.len();
        let inside = |x: f32, y: f32| {
            let (mut pos, mut neg) = (false, false);
            for i in 0..n {
                let (a, b) = (pts[i], pts[(i + 1) % n]);
                let cr = (b.0 - a.0) * (y - a.1) - (b.1 - a.1) * (x - a.0);
                pos |= cr > 0.0;
                neg |= cr < 0.0;
            }
            !(pos && neg)
        };
        for yy in min_y..=max_y {
            for xx in min_x..=max_x {
                let mut hit = 0;
                for sy in 0..3 {
                    for sx in 0..3 {
                        if inside(xx as f32 + (sx as f32 + 0.5) / 3.0, yy as f32 + (sy as f32 + 0.5) / 3.0) {
                            hit += 1;
                        }
                    }
                }
                if hit > 0 {
                    self.blend(xx, yy, rgb(c), a * hit as f32 / 9.0);
                }
            }
        }
    }
    fn text(&mut self, x: f32, y: f32, px: f32, s: &str, c: u32, a: f32) -> f32 {
        let mut pen = x;
        for ch in s.chars() {
            let (m, bm) = self.font.rasterize(ch, px);
            for row in 0..m.height {
                for col in 0..m.width {
                    let cov = bm[row * m.width + col] as f32 / 255.0;
                    if cov > 0.0 {
                        let gx = pen as i64 + col as i64 + m.xmin as i64;
                        let gy = (y + px * 0.8) as i64 - m.ymin as i64 - m.height as i64 + row as i64;
                        self.blend(gx, gy, rgb(c), cov * a);
                    }
                }
            }
            pen += m.advance_width;
        }
        pen - x
    }
    fn text_w(&self, s: &str, px: f32) -> f32 {
        s.chars().map(|c| self.font.metrics(c, px).advance_width).sum()
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let out = args.get(1).cloned().unwrap_or_else(|| "hud-layout-preview.png".into());
    let (w, h) = (args.get(2).and_then(|s| s.parse().ok()).unwrap_or(1920usize), args.get(3).and_then(|s| s.parse().ok()).unwrap_or(1080usize));
    let font_path = std::env::var("HUD_PREVIEW_FONT").unwrap_or_else(|_| "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf".into());
    let font = fontdue::Font::from_bytes(std::fs::read(&font_path).expect("font"), fontdue::FontSettings::default()).expect("font parse");
    // `--art DIR`: Ragnarök's own pictures, as picked in assets.json
    let mut tex = std::collections::HashMap::new();
    if let Some(dir) = args.iter().position(|a| a == "--art").and_then(|i| args.get(i + 1)) {
        for a in gowring_core::sheets::ASSETS.iter().filter(|a| a.ragnarok_pick != "none") {
            match load_png(&std::path::Path::new(dir).join(format!("{}.png", a.ragnarok_pick))) {
                Some(t) => {
                    tex.insert(a.id, t);
                }
                None => eprintln!("{}: {} not found in {dir}", a.id, a.ragnarok_pick),
            }
        }
    }
    let art = vector::art_flags(|id| tex.contains_key(id));
    let mut cv = Canvas { w, h, px: vec![[0.0; 3]; w * h], font, tex: std::sync::Arc::new(tex) };
    let backdrop = args.iter().position(|a| a == "--match").and_then(|i| args.get(i + 1)).cloned();
    match &backdrop {
        Some(path) => {
            let dec = png::Decoder::new(std::io::BufReader::new(std::fs::File::open(path).expect("backdrop")));
            let mut r = dec.read_info().expect("backdrop png");
            let mut buf = vec![0; r.output_buffer_size()];
            let info = r.next_frame(&mut buf).expect("backdrop frame");
            let ch = info.color_type.samples();
            assert_eq!((info.width as usize, info.height as usize), (w, h), "backdrop must be {w}x{h}");
            for i in 0..w * h {
                cv.px[i] = [buf[i * ch] as f32 / 255.0, buf[i * ch + 1] as f32 / 255.0, buf[i * ch + 2] as f32 / 255.0];
            }
        }
        None => {
            // backdrop: a dusk-coloured gradient so translucent HUD parts read like they would over a scene
            for y in 0..h {
                for x in 0..w {
                    let t = y as f32 / h as f32;
                    cv.px[y * w + x] = [0.16 + 0.10 * t, 0.17 + 0.12 * t, 0.22 + 0.06 * t];
                }
            }
        }
    }
    let d = Data::get();
    if let Some(dir) = args.iter().position(|a| a == "--demo").and_then(|i| args.get(i + 1)) {
        demo(&cv, dir, art);
        return;
    }
    if args.iter().any(|a| a == "--runics") {
        // every runic on its plate, one row per weapon: light ones, then heavy ones
        for (row, w) in ["leviathan_axe", "blades_of_chaos", "draupnir_spear"].iter().enumerate() {
            let y = 150.0 + row as f32 * 300.0;
            cv.text(60.0, y - 120.0, 30.0, d.weapon(w).name, 0xFFD27A, 1.0);
            let list: Vec<_> = d.all_runics().filter(|r| r.weapon == *w).collect();
            for (i, r) in list.iter().enumerate() {
                let x = 150.0 + i as f32 * 290.0 + if r.tier == gowring_core::sheets::RunicTier::Heavy { 60.0 } else { 0.0 };
                let (rx, ry) = (58.0, 51.0);
                let w = d.weapon(r.weapon);
                cv.prims(&vector::runic_plate(x, y, rx, ry, w.runic_color, w.runic_light, 1.0, w.runic_glow, 1.0, art));
                if let Some(v) = r.icon.and_then(|id| vector::mono_icon(id, &hud::Rect { x: x - 32.0, y: y - 32.0, w: 64.0, h: 64.0 }, 0xFFFFFF, 1.0)) {
                    cv.prims(&v);
                }
                let tier = if r.tier == gowring_core::sheets::RunicTier::Heavy { "heavy" } else { "light" };
                let tw = cv.text_w(r.name, 24.0);
                cv.text(x - tw / 2.0, y + 66.0, 24.0, r.name, 0xF4EFE4, 1.0);
                let tw = cv.text_w(tier, 18.0);
                cv.text(x - tw / 2.0, y + 96.0, 18.0, tier, 0xB8BEC6, 1.0);
            }
        }
        save(&cv, &out);
        return;
    }
    let matching = HudState {
        hp_frac: 1.0,
        hp_trail_frac: 1.0,
        rage_frac: 0.89,
        rage_ready: true,
        runic_light: Some(RunicHud { id: "axe_runic_light", name: "Frost Shockwave", remaining_frac: 0.0, ready: true, ..Default::default() }),
        runic_heavy: Some(RunicHud { id: "axe_runic_heavy", name: "Glacial Cleave", remaining_frac: 0.0, ready: true, ..Default::default() }),
        weapon_icon: d.weapon("leviathan_axe").icon,
        weapon_name: "Leviathan Axe",
        shield_drawn: true,
        pulse: 0.2,
        ..Default::default()
    };
    let busy = HudState {
        hp_frac: 0.72,
        hp_trail_frac: 0.84,
        rage_frac: 1.0,
        rage_ready: true,
        runic_light: Some(RunicHud { id: "axe_runic_light", name: "Frost Shockwave", remaining_frac: 0.45, ready: false, ..Default::default() }),
        runic_heavy: Some(RunicHud { id: "axe_runic_heavy", name: "Glacial Cleave", remaining_frac: 0.0, ready: true, ..Default::default() }),
        weapon_icon: d.weapon("leviathan_axe").icon,
        weapon_name: "Leviathan Axe",
        shield_drawn: true,
        aiming: true,
        recall_available: true,
        recall_label: "Recall",
        in_combat: true,
        enemies: vec![
            EnemyBar { id: 1, pos: Vec3::new(-3.0, 0.0, 9.0), hp_frac: 0.55, stun_frac: 0.7, stunned: false, boss: false, status_frac: 0.6, status_color: 0x7FD8FF, status_label: None },
            EnemyBar { id: 2, pos: Vec3::new(2.5, 0.0, 7.0), hp_frac: 0.9, stun_frac: 1.0, stunned: true, boss: false, status_frac: 0.0, status_color: 0, status_label: None },
            EnemyBar { id: 3, pos: Vec3::new(7.0, 0.0, 14.0), hp_frac: 0.3, stun_frac: 0.1, stunned: false, boss: false, status_frac: 1.0, status_color: 0xFF8A3A, status_label: Some("BURNING") },
        ],
        finisher_target: Some(2),
        subtitle: Some("(Mimir's subtitle appears here, from Ragnarök's own text)".into()),
        mimir_speaking: true,
        pulse: 0.2,
        ..Default::default()
    };
    let mut state = if backdrop.is_some() { matching } else { busy };
    state.art = art;
    // `--time T`: the HUD clock (the fire around L3 + R3 moves with it); `--picker`: show the light runic picker
    if let Some(t) = args.iter().position(|a| a == "--time").and_then(|i| args.get(i + 1)).and_then(|t| t.parse().ok()) {
        state.time_s = t;
    }
    if let Some(wi) = args.iter().position(|a| a == "--weapon").and_then(|i| args.get(i + 1)).and_then(|t| t.parse::<u8>().ok()) {
        let w = d.right_weapon(wi);
        let slot = |id: Option<&'static str>, cd: f32| {
            id.map(|id| {
                let r = d.runic(id);
                RunicHud { id: r.id, name: r.name, remaining_frac: cd, ready: cd <= 0.0, color: w.runic_color, light: w.runic_light, glow: w.runic_glow, flash: 0.0 }
            })
        };
        state.weapon_icon = w.icon;
        state.weapon_name = w.name;
        state.weapon_disc = w.disc_color;
        let cd = state.runic_light.as_ref().map(|r| r.remaining_frac).unwrap_or(0.0);
        state.runic_light = slot(w.light_runic, cd);
        state.runic_heavy = slot(w.heavy_runic, 0.0);
    }
    // `--bars HP,TRAIL,RAGE[,HIT_AGE[,RAGE_ON]]`: set the bars (for frame-by-frame motion previews); RAGE_ON > 0 shows
    // Spartan Rage burning with that much of its start flash left
    if let Some(v) = args.iter().position(|a| a == "--bars").and_then(|i| args.get(i + 1)) {
        let n: Vec<f32> = v.split(',').filter_map(|x| x.parse().ok()).collect();
        if n.len() >= 3 {
            (state.hp_frac, state.hp_trail_frac, state.rage_frac) = (n[0], n[1], n[2]);
            state.rage_ready = n[2] >= 0.999;
            state.hp_hit_age = n.get(3).copied().unwrap_or(100.0);
            if let Some(r) = n.get(4).filter(|r| **r > 0.0) {
                state.rage_active = true;
                state.rage_ready = false;
                state.rage_flash = (*r - 1.0).clamp(0.0, 1.0);
            }
        }
    }
    // `--cooling L,H`: the light and heavy runics' remaining cooldown (0..1), `--flash L,H`: their flash
    for (key, cool) in [("--cooling", true), ("--flash", false)] {
        if let Some(v) = args.iter().position(|a| a == key).and_then(|i| args.get(i + 1)) {
            let n: Vec<f32> = v.split(',').filter_map(|x| x.parse().ok()).collect();
            for (slot, val) in [&mut state.runic_light, &mut state.runic_heavy].into_iter().zip(n) {
                if let Some(r) = slot {
                    if cool {
                        r.remaining_frac = val;
                        r.ready = val <= 0.0;
                    } else {
                        r.flash = val;
                    }
                }
            }
        }
    }
    // `--thrown`: the weapon held is out of Kratos' hand (axe) or has spears out
    if args.iter().any(|a| a == "--thrown") {
        state.recall_available = true;
        state.recall_label = if state.weapon_name == "Draupnir Spear" { "Detonate" } else { "Recall" };
    }
    if args.iter().any(|a| a == "--picker") {
        let opts: Vec<_> = d.all_runics().filter(|r| r.weapon == "leviathan_axe" && r.tier == gowring_core::sheets::RunicTier::Light).collect();
        state.runic_light = Some(RunicHud { id: opts[1].id, name: opts[1].name, remaining_frac: 0.0, ready: true, ..Default::default() });
        state.runic_picker = Some(gowring_core::hud::RunicPicker {
            heavy: false,
            weapon_name: "Leviathan Axe",
            options: opts.iter().map(|r| (r.name, r.icon, r.effect)).collect(),
            chosen: 1,
            colors: (0x4A7FB4, 0x8AB4DC, 0.0),
            alpha: 1.0,
        });
    }
    let project = |p: Vec3| -> Option<(f32, f32)> {
        if p.z <= 0.5 {
            return None;
        }
        let t = (60f32.to_radians() * 0.5).tan();
        Some(((p.x / (p.z * t * (w as f32 / h as f32)) * 0.5 + 0.5) * w as f32, (0.5 + 0.18 - 0.5 * (1.0 / (p.z * t)) * 0.9) * h as f32 * 0.0 + h as f32 * (0.5 + 0.35 / (p.z * t) * 0.9)))
    };
    for c in hud::draw_list(d, &state, Screen { w: w as f32, h: h as f32 }, &project) {
        paint(&mut cv, c);
    }
    if backdrop.is_some() {
        save(&cv, &out);
        return;
    }
    // an icon sheet: every HUD drawing at 110 px, on the same backdrop
    for (i, id) in ["hud_icon_axe", "hud_icon_blades", "hud_icon_spear", "hud_icon_shield", "hud_crosshair_throw", "hud_enemy_stunned"].iter().enumerate() {
        let r = gowring_core::hud::Rect { x: 1120.0 + i as f32 * 128.0, y: 40.0, w: 110.0, h: 110.0 };
        if let Some(v) = vector::icon_prims(id, &r, 0xFFFFFF, 1.0) {
            cv.prims(&v);
        }
    }
    let banner = "LAYOUT PREVIEW - original vector HUD art on a stand-in backdrop; not gameplay and not Ragnarok art";
    cv.text(24.0, 14.0, 22.0, banner, 0xFFD27A, 0.95);
    save(&cv, &out);
}

fn save(cv: &Canvas, out: &str) {
    let (w, h) = (cv.w, cv.h);
    let mut buf = Vec::with_capacity(w * h * 3);
    for p in &cv.px {
        for c in p {
            buf.push((c.clamp(0.0, 1.0) * 255.0) as u8);
        }
    }
    let file = std::fs::File::create(out).expect("create png");
    let mut enc = png::Encoder::new(std::io::BufWriter::new(file), w as u32, h as u32);
    enc.set_color(png::ColorType::Rgb);
    enc.set_depth(png::BitDepth::Eight);
    enc.write_header().unwrap().write_image_data(&buf).unwrap();
    println!("wrote {out} ({w}x{h})");
}

/// Draw one HUD command with the preview's rasteriser.
fn paint(cv: &mut Canvas, c: DrawCmd) {
    let (w, h) = (cv.w, cv.h);
    match c {
        DrawCmd::Overlay { color, alpha } => cv.rect(0.0, 0.0, w as f32, h as f32, color, alpha),
        DrawCmd::Shapes { prims } => cv.prims(&prims),
        DrawCmd::Label { x, y, px, text, color, alpha, align } => {
            let x = if align == Align::Center { x - cv.text_w(&text, px) / 2.0 } else { x };
            cv.text(x, y, px, &text, color, alpha);
        }
        DrawCmd::Bar { rect, frac, trail, trail_color, trail_alpha, color, lead, segments, flash, alpha, live, .. } => cv.prims(&vector::rbar_fx(&rect, frac, trail, trail_color, trail_alpha, color, lead, segments, flash, alpha, live)),
        DrawCmd::Ring { cx, cy, rx, ry, remaining, icon, color, light, aura, flash, alpha, art, .. } => {
            cv.prims(&vector::runic_plate_fx(cx, cy, rx, ry, color, light, 1.0 - remaining, aura, flash, alpha, art));
            let s = ry * 0.62;
            if let Some(v) = icon.and_then(|id| vector::mono_icon(id, &hud::Rect { x: cx - s, y: cy - s, w: s * 2.0, h: s * 2.0 }, 0xFFFFFF, alpha)) {
                cv.prims(&v);
            }
        }
        DrawCmd::Icon { rect, asset, color, alpha, mono } => match asset.and_then(|id| if mono { vector::mono_icon(id, &rect, color, alpha) } else { vector::icon_prims(id, &rect, color, alpha) }) {
            Some(v) => cv.prims(&v),
            None => {
                cv.outline(rect.x, rect.y, rect.w, rect.h, color, 0.6 * alpha);
                if let Some(id) = asset {
                    cv.text(rect.x + 4.0, rect.y + 4.0, 16.0, &id.rsplit('_').next().unwrap_or(id).to_ascii_uppercase(), color, 0.8 * alpha);
                }
            }
        },
        DrawCmd::Prompt { rect, text, color, alpha, .. } => {
            cv.rect(rect.x, rect.y, rect.w, rect.h, 0x000000, 0.45 * alpha);
            if let Some(t) = text {
                cv.text(rect.x + 12.0, rect.y + rect.h * 0.18, rect.h * 0.55, &t, color, alpha);
            }
        }
        DrawCmd::Text { rect, px, text, color, alpha, align, .. } => {
            cv.rect(rect.x, rect.y, rect.w, rect.h, 0x000000, 0.55 * alpha);
            let tw = cv.text_w(&text, px);
            let x = if align == Align::Center { rect.x + (rect.w - tw) / 2.0 } else { rect.x + 16.0 };
            cv.text(x, rect.y + (rect.h - px) / 2.0, px, &text, color, alpha);
        }
    }
}

/// `--demo DIR`: play a scripted fight through the real rules engine (`SimWorld`) and save every third frame (20 per
/// second) of the HUD's corner, with a caption, as DIR/00000.png ... — the frames of an animated preview.
fn demo(base: &Canvas, dir: &str, art: u32) {
    use gowring_core::sheets::ErAction::*;
    use gowring_core::sim::{SimEnemy, SimWorld};
    let d = Data::get();
    std::fs::create_dir_all(dir).expect("demo dir");
    let mut w = SimWorld::new();
    // fast Rage for the demo, no Mimir lines over the HUD
    w.engine.set_tuning(gowring_core::Tuning { rage_gain: 14.0, mimir_chance: 0.0, ..Default::default() });
    let max = w.player.max_hp;
    w.add_enemy(SimEnemy::new(7, 0.0, 1.8, 90000.0));
    let (sw, sh) = (base.w as f32, base.h as f32);
    let project = |_p: Vec3| -> Option<(f32, f32)> { None };
    // (from, to, caption)
    let captions = [
        (0.0, 1.0, "Leviathan Axe"),
        (1.0, 2.6, "Taking a hit: the lost chunk flashes, turns red, fades"),
        (2.6, 4.4, "Light runic used: white flash, grey, refills from the bottom"),
        (4.4, 5.3, "Healing fills smoothly"),
        (5.3, 7.0, "Hold L1 + D-pad up: change the light runic"),
        (7.0, 9.0, "Axe thrown: the Triangle sits in the disc (recall)"),
        (9.0, 10.8, "Swap to the Blades of Chaos"),
        (10.8, 12.9, "Draupnir Spear: spears out, Triangle on the disc's edge"),
        (12.9, 14.9, "Hits fill Spartan Rage; L3 + R3 burn when it is full"),
        (14.9, 18.5, "Spartan Rage: white-hot flash, runics fade, Rage's mark"),
    ];
    let mut t = 0.0f64;
    let mut frame = 0usize;
    let mut tick = 0usize;
    let (mut hit_done, mut heal_done) = (false, false);
    let mut rage_pressed = false;
    while t < 18.5 {
        let at = |a: f64| t >= a && t < a + 0.016;
        let within = |a: f64, b: f64| t >= a && t < b;
        let mut pressed = Vec::new();
        let mut hold = Vec::new();
        if t >= 1.0 && !hit_done {
            w.player.hp = max * 0.62;
            hit_done = true;
        }
        if t >= 4.4 && !heal_done {
            w.player.hp = max * 0.9;
            heal_done = true;
        }
        if at(2.6) {
            pressed.push(SwitchForm);
        }
        if within(5.3, 5.9) {
            hold.push(L1);
        }
        if at(5.45) {
            pressed.push(SwitchForm);
        }
        if within(7.0, 7.6) {
            hold.push(L2);
        }
        if at(7.45) {
            pressed.push(R1);
        }
        if at(8.6) {
            pressed.push(Action);
        }
        if at(9.0) {
            w.right_slot = 1;
        }
        if at(9.8) {
            pressed.push(ChangeItem);
        }
        if at(10.8) {
            w.right_slot = 2;
        }
        if within(11.2, 11.8) {
            hold.push(L2);
        }
        if at(11.65) {
            pressed.push(R1);
        }
        if at(12.9) {
            w.right_slot = 0;
        }
        if within(13.0, 14.7) && ((t - 13.0) / 0.32).fract() < 0.05 {
            pressed.push(R1);
        }
        if t >= 14.9 && !rage_pressed && w.engine.rage().ready() {
            pressed.push(L3);
            pressed.push(R3);
            rage_pressed = true;
        }
        let out = w.step(&pressed, &hold);
        t += w.dt as f64;
        tick += 1;
        if tick % 3 != 0 {
            continue;
        }
        let mut cv = Canvas { w: base.w, h: base.h, px: base.px.clone(), font: base.font.clone(), tex: base.tex.clone() };
        let mut hud = out.hud.clone();
        hud.ui_scale = 1.0;
        hud.art = art;
        for c in hud::draw_list(d, &hud, Screen { w: sw, h: sh }, &project) {
            paint(&mut cv, c);
        }
        let cap = captions.iter().find(|(a, b, _)| t >= *a && t < *b).map(|c| c.2).unwrap_or("");
        let (x0, y0, cw, ch) = (0usize, 500usize, 760usize, 580usize);
        cv.rect(x0 as f32, y0 as f32, cw as f32, 40.0, 0x000000, 0.65);
        cv.text(x0 as f32 + 14.0, y0 as f32 + 8.0, 22.0, cap, 0xFFD27A, 1.0);
        save_region(&cv, &format!("{dir}/{frame:05}.png"), x0, y0, cw, ch);
        frame += 1;
    }
    println!("wrote {frame} frames to {dir}");
}

fn save_region(cv: &Canvas, out: &str, x0: usize, y0: usize, w: usize, h: usize) {
    let mut buf = Vec::with_capacity(w * h * 3);
    for y in y0..y0 + h {
        for x in x0..x0 + w {
            for c in cv.px[y * cv.w + x] {
                buf.push((c.clamp(0.0, 1.0) * 255.0) as u8);
            }
        }
    }
    let file = std::fs::File::create(out).expect("create png");
    let mut enc = png::Encoder::new(std::io::BufWriter::new(file), w as u32, h as u32);
    enc.set_color(png::ColorType::Rgb);
    enc.set_depth(png::BitDepth::Eight);
    enc.write_header().unwrap().write_image_data(&buf).unwrap();
}
