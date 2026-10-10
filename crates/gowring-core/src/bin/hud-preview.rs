//! Dev tool: rasterises `gowring-core`'s HUD draw list with flat placeholder shapes so the layout can be
//! judged without the game. It is a LAYOUT PREVIEW — not gameplay, not Ragnarök art.
//!
//!   cargo run -p gowring-core --features preview --bin hud-preview -- out.png [width height]

use gowring_core::hud::{self, Align, DrawCmd, EnemyBar, HudState, RunicHud, Screen};
use gowring_core::{Data, Vec3};

struct Canvas {
    w: usize,
    h: usize,
    px: Vec<[f32; 3]>,
    font: fontdue::Font,
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
    let mut cv = Canvas { w, h, px: vec![[0.0; 3]; w * h], font };
    // backdrop: a dusk-coloured gradient so translucent HUD parts read like they would over a scene
    for y in 0..h {
        for x in 0..w {
            let t = y as f32 / h as f32;
            cv.px[y * w + x] = [0.16 + 0.10 * t, 0.17 + 0.12 * t, 0.22 + 0.06 * t];
        }
    }
    let d = Data::get();
    let state = HudState {
        hp_frac: 0.72,
        hp_trail_frac: 0.84,
        rage_frac: 1.0,
        rage_ready: true,
        runic_light: Some(RunicHud { id: "axe_runic_light", name: "Frost Shockwave", remaining_frac: 0.45, ready: false }),
        runic_heavy: Some(RunicHud { id: "axe_runic_heavy", name: "Glacial Cleave", remaining_frac: 0.0, ready: true }),
        weapon_icon: d.weapon("leviathan_axe").icon,
        weapon_name: "Leviathan Axe",
        shield_drawn: true,
        aiming: true,
        recall_available: true,
        recall_label: "Recall",
        in_combat: true,
        enemies: vec![
            EnemyBar { id: 1, pos: Vec3::new(-3.0, 0.0, 9.0), hp_frac: 0.55, stun_frac: 0.7, stunned: false, boss: false },
            EnemyBar { id: 2, pos: Vec3::new(2.5, 0.0, 7.0), hp_frac: 0.9, stun_frac: 1.0, stunned: true, boss: false },
            EnemyBar { id: 3, pos: Vec3::new(7.0, 0.0, 14.0), hp_frac: 0.3, stun_frac: 0.1, stunned: false, boss: false },
        ],
        finisher_target: Some(2),
        subtitle: Some("(Mimir's subtitle appears here, from Ragnarök's own text)".into()),
        mimir_speaking: true,
        pulse: 0.2,
        ..Default::default()
    };
    let project = |p: Vec3| -> Option<(f32, f32)> {
        if p.z <= 0.5 {
            return None;
        }
        let t = (60f32.to_radians() * 0.5).tan();
        Some(((p.x / (p.z * t * (w as f32 / h as f32)) * 0.5 + 0.5) * w as f32, (0.5 + 0.18 - 0.5 * (1.0 / (p.z * t)) * 0.9) * h as f32 * 0.0 + h as f32 * (0.5 + 0.35 / (p.z * t) * 0.9)))
    };
    for c in hud::draw_list(d, &state, Screen { w: w as f32, h: h as f32 }, &project) {
        match c {
            DrawCmd::Overlay { color, alpha } => cv.rect(0.0, 0.0, w as f32, h as f32, color, alpha),
            DrawCmd::Bar { rect, frac, trail, color, alpha, .. } => {
                cv.rect(rect.x, rect.y, rect.w, rect.h, 0x101010, 0.65 * alpha);
                if trail > frac {
                    cv.rect(rect.x, rect.y, rect.w * trail, rect.h, 0xE8E2C8, 0.45 * alpha);
                }
                cv.rect(rect.x, rect.y, rect.w * frac, rect.h, color, alpha);
                cv.outline(rect.x, rect.y, rect.w, rect.h, 0xBBBBBB, 0.5 * alpha);
            }
            DrawCmd::Ring { cx, cy, r, remaining, ready, color, alpha, .. } => {
                cv.disc(cx, cy, r, 0.0, 0x101010, 0.55 * alpha, None);
                cv.disc(cx, cy, r, r - 3.0, color, alpha, None);
                if remaining > 0.001 {
                    cv.disc(cx, cy, r - 3.0, 0.0, 0x000000, 0.62, Some(remaining));
                } else if ready {
                    cv.disc(cx, cy, r * 1.08, r * 1.0, color, alpha, None);
                }
                cv.text(cx - 14.0, cy - 12.0, 24.0, "AXE", 0xFFFFFF, 0.7);
            }
            DrawCmd::Icon { rect, asset, color, alpha } => {
                cv.outline(rect.x, rect.y, rect.w, rect.h, color, 0.6 * alpha);
                if let Some(id) = asset {
                    cv.text(rect.x + 4.0, rect.y + 4.0, 16.0, &id.rsplit('_').next().unwrap_or(id).to_ascii_uppercase(), color, 0.8 * alpha);
                }
            }
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
    let banner = "LAYOUT PREVIEW - flat placeholder shapes, not gameplay and not Ragnarok art";
    cv.text(24.0, h as f32 - 34.0, 22.0, banner, 0xFFD27A, 0.95);
    let mut buf = Vec::with_capacity(w * h * 3);
    for p in &cv.px {
        for c in p {
            buf.push((c.clamp(0.0, 1.0) * 255.0) as u8);
        }
    }
    let file = std::fs::File::create(&out).expect("create png");
    let mut enc = png::Encoder::new(std::io::BufWriter::new(file), w as u32, h as u32);
    enc.set_color(png::ColorType::Rgb);
    enc.set_depth(png::BitDepth::Eight);
    enc.write_header().unwrap().write_image_data(&buf).unwrap();
    println!("wrote {out} ({w}x{h})");
}
