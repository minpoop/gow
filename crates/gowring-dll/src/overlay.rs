//! hooks.overlay — draws the HUD (from `gowring-core`'s draw list) with ImGui on top of Elden Ring's
//! D3D12 frame, using the Ragnarök art converted by `--prepare`.

use crate::assets::Store;
use gowring_core::hud::{self, Align, DrawCmd, HudState, Rect, Screen};
use gowring_core::{Data, Vec3};
use hudhook::imgui::{self, FontConfig, FontId, FontSource, TextureId, Ui};
use hudhook::{ImguiRenderLoop, RenderContext};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

/// Everything the overlay needs for one frame; written by the game-thread tick, read by the render thread.
#[derive(Clone, Default)]
pub struct Snapshot {
    pub valid: bool,
    pub hud: HudState,
    pub cam_pos: Vec3,
    pub cam_right: Vec3,
    pub cam_up: Vec3,
    pub cam_fwd: Vec3,
    pub fov_rad: f32,
    pub banner: String,
    pub projectiles: Vec<(&'static str, Vec3)>,
    /// Short-lived effect rings (runic shockwaves, impacts, the spear blast, Rage burst), see `FxMark`.
    pub fx: Vec<FxMark>,
    /// Settings: overall HUD opacity (0..1) and the controls panel's current visibility (0..1).
    pub opacity: f32,
    pub hint: f32,
}

/// One effect: where it happened, how far it reaches (metres) and how old it is (seconds).
#[derive(Clone)]
pub struct FxMark {
    pub kind: gowring_core::sheets::FxKind,
    pub pos: Vec3,
    pub radius: f32,
    pub age: f32,
}

/// How long an effect ring lives.
pub const FX_LIFE_S: f32 = 0.9;

pub type Shared = Arc<Mutex<Snapshot>>;

pub struct Overlay {
    shared: Shared,
    store: Arc<Store>,
    textures: HashMap<&'static str, (TextureId, f32, f32)>,
    /// Textures that are white shapes (tinted by the HUD colour when drawn).
    masks: std::collections::HashSet<&'static str>,
    big_font: Option<SendFont>,
}

/// `FontId` wraps a raw pointer into ImGui's atlas; it is only ever used on the render thread.
struct SendFont(FontId);
unsafe impl Send for SendFont {}
unsafe impl Sync for SendFont {}


impl Overlay {
    pub fn new(shared: Shared, store: Arc<Store>) -> Self {
        Overlay { shared, store, textures: HashMap::new(), masks: Default::default(), big_font: None }
    }
}

/// Overall HUD opacity from the settings (f32 bits; 1.0 until the first snapshot).
static TOAST: Mutex<Option<(String, std::time::Instant)>> = Mutex::new(None);

/// Show a short line near the top of the screen for a few seconds (debug summons and the like).
pub fn toast(msg: &str) {
    if let Ok(mut g) = TOAST.lock() {
        *g = Some((msg.to_string(), std::time::Instant::now()));
    }
}

static OPACITY: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0x3F80_0000);

fn col(rgb: u32, alpha: f32) -> [f32; 4] {
    let alpha = alpha * f32::from_bits(OPACITY.load(std::sync::atomic::Ordering::Relaxed));
    [((rgb >> 16) & 0xFF) as f32 / 255.0, ((rgb >> 8) & 0xFF) as f32 / 255.0, (rgb & 0xFF) as f32 / 255.0, alpha.clamp(0.0, 1.0)]
}

impl ImguiRenderLoop for Overlay {
    fn initialize<'a>(&'a mut self, ctx: &mut imgui::Context, render_context: &'a mut dyn RenderContext) {
        self.big_font = Some(SendFont(ctx.fonts().add_font(&[FontSource::DefaultFontData { config: Some(FontConfig { size_pixels: 30.0, oversample_h: 2, ..FontConfig::default() }) }])));
        let mut ok = 0;
        for (id, t) in &self.store.textures {
            match render_context.load_texture(&t.rgba, t.w, t.h) {
                Ok(tid) => {
                    self.textures.insert(*id, (tid, t.w as f32, t.h as f32));
                    if t.mask {
                        self.masks.insert(*id);
                    }
                    ok += 1;
                }
                Err(e) => crate::glog!("overlay", "texture {id}: {e:?}"),
            }
        }
        crate::glog!("overlay", "initialised: {ok}/{} textures uploaded ({} of them white shapes the HUD colours: {:?})", self.store.textures.len(), self.masks.len(), self.masks);
    }

    fn render(&mut self, ui: &mut Ui) {
        let snap = match self.shared.lock() {
            Ok(g) => g.clone(),
            Err(_) => return,
        };
        let [w, h] = ui.io().display_size;
        if w < 16.0 || h < 16.0 {
            return;
        }
        let dl = ui.get_background_draw_list();
        let alpha_mode = self.store.channel_alpha;
        OPACITY.store((if snap.valid { snap.opacity } else { 1.0 }).clamp(0.0, 1.0).to_bits(), std::sync::atomic::Ordering::Relaxed);

        if snap.valid {
            let (right, up, fwd, pos) = (snap.cam_right, snap.cam_up, snap.cam_fwd, snap.cam_pos);
            let tan = (snap.fov_rad.clamp(0.2, 2.5) * 0.5).tan();
            let aspect = w / h;
            let project = move |p: Vec3| -> Option<(f32, f32)> {
                let d = p - pos;
                let z = d.dot(fwd);
                if z <= 0.1 {
                    return None;
                }
                let nx = d.dot(right) / (z * tan * aspect);
                let ny = d.dot(up) / (z * tan);
                if nx.abs() > 1.3 || ny.abs() > 1.3 {
                    return None;
                }
                Some(((nx * 0.5 + 0.5) * w, (0.5 - ny * 0.5) * h))
            };
            let cmds = hud::draw_list(Data::get(), &snap.hud, Screen { w, h }, &project);
            let tex = |id: Option<&'static str>| id.and_then(|i| self.textures.get(i).copied());
            let image = |t: (TextureId, f32, f32), r: &Rect, a: f32, tint: u32| {
                dl.add_image(t.0, [r.x, r.y], [r.x + r.w, r.y + r.h]).col(col(tint, a)).build();
            };
            let mut ring_no = 0;
            for c in &cmds {
                match c {
                    DrawCmd::Overlay { color, alpha } => {
                        if *alpha > 0.002 {
                            dl.add_rect([0.0, 0.0], [w, h], col(*color, *alpha)).filled(true).build();
                        }
                    }
                    DrawCmd::Bar { rect, frac, trail, frame, fill, color, alpha } if tex(*frame).is_none() && tex(*fill).is_none() => {
                        draw_prims(&dl, &gowring_core::vector::bar(rect, *frac, *trail, *color, *alpha));
                    }
                    DrawCmd::Bar { rect, frac, trail, frame, fill, color, alpha } => {
                        let (x0, y0, x1, y1) = (rect.x, rect.y, rect.x + rect.w, rect.y + rect.h);
                        let is_mask = |id: Option<&'static str>| id.map(|i| self.masks.contains(i)).unwrap_or(false);
                        let frame_mask = is_mask(*frame);
                        // a picture clipped to the left `f` of the bar, tinted
                        let clipped = |t: (TextureId, f32, f32), f: f32, tint: u32, a: f32| {
                            dl.add_image(t.0, [x0, y0], [x0 + rect.w * f, y1]).uv_min([0.0, 0.0]).uv_max([f, 1.0]).col(col(tint, a)).build();
                        };
                        match tex(*frame) {
                            // Ragnarök's housing shapes are white masks: a dark body in that shape, the fill in the same shape
                            Some(t) if frame_mask => image(t, rect, 0.82 * alpha, 0x17140F),
                            Some(t) => image(t, rect, *alpha, 0xFFFFFF),
                            None => {
                                dl.add_rect([x0, y0], [x1, y1], col(0x101010, 0.65 * alpha)).filled(true).build();
                            }
                        }
                        // the lagging "recent damage" trail, then the fill
                        if *trail > *frac {
                            match tex(*frame) {
                                Some(t) if frame_mask => clipped(t, *trail, 0xE8E2C8, 0.45 * alpha),
                                _ => {
                                    dl.add_rect([x0, y0], [x0 + rect.w * trail, y1], col(0xE8E2C8, 0.45 * alpha)).filled(true).build();
                                }
                            }
                        }
                        let fill_t = tex(*fill).or(if frame_mask { tex(*frame) } else { None });
                        match fill_t {
                            Some(t) => {
                                let tinted = is_mask(*fill) || tex(*fill).is_none();
                                clipped(t, *frac, if tinted { *color } else { 0xFFFFFF }, *alpha);
                            }
                            None => {
                                dl.add_rect([x0, y0], [x0 + rect.w * frac, y1], col(*color, *alpha)).filled(true).build();
                            }
                        }
                    }
                    DrawCmd::Ring { cx, cy, r, remaining, ready, frame, icon, color, alpha, .. } if tex(*frame).is_none() => {
                        draw_prims(&dl, &gowring_core::vector::ring(*cx, *cy, *r, *remaining, *ready, *color, *alpha, *icon));
                        // which runic this ring is (the sheet lists the light one first)
                        let label = if ring_no == 0 { "LIGHT" } else { "HEAVY" };
                        ring_no += 1;
                        let sz = ui.calc_text_size(label);
                        dl.add_text([cx - sz[0] / 2.0, cy + r + 6.0], col(0xE8E2C8, 0.75 * alpha), label);
                    }
                    DrawCmd::Ring { cx, cy, r, remaining, ready, frame, glow, icon, color, alpha } => {
                        let rect = Rect { x: cx - r, y: cy - r, w: r * 2.0, h: r * 2.0 };
                        match tex(*frame) {
                            Some(t) => image(t, &rect, *alpha, 0xFFFFFF),
                            None => {
                                dl.add_circle([*cx, *cy], *r, col(0x101010, 0.55 * alpha)).filled(true).num_segments(48).build();
                                dl.add_circle([*cx, *cy], *r, col(*color, *alpha)).thickness(3.0).num_segments(48).build();
                            }
                        }
                        if let Some(t) = tex(*icon) {
                            let inner = Rect { x: cx - r * 0.62, y: cy - r * 0.62, w: r * 1.24, h: r * 1.24 };
                            image(t, &inner, *alpha, 0xFFFFFF);
                        }
                        if *remaining > 0.001 {
                            // dark pie for the part of the cooldown still to run, clockwise from 12 o'clock
                            let n = 48;
                            let start = -std::f32::consts::FRAC_PI_2;
                            let sweep = std::f32::consts::TAU * remaining;
                            let steps = ((n as f32 * remaining).ceil() as i32).max(1);
                            for i in 0..steps {
                                let a0 = start + sweep * (i as f32 / steps as f32);
                                let a1 = start + sweep * ((i + 1) as f32 / steps as f32);
                                dl.add_triangle([*cx, *cy], [cx + r * a0.cos(), cy + r * a0.sin()], [cx + r * a1.cos(), cy + r * a1.sin()], col(0x000000, 0.62)).filled(true).build();
                            }
                        } else if *ready {
                            match tex(*glow) {
                                Some(t) => image(t, &Rect { x: cx - r * 1.15, y: cy - r * 1.15, w: r * 2.3, h: r * 2.3 }, *alpha, 0xFFFFFF),
                                None => {
                                    dl.add_circle([*cx, *cy], *r * 1.05, col(*color, *alpha)).thickness(4.0).num_segments(48).build();
                                }
                            }
                        }
                    }
                    DrawCmd::Icon { rect, asset, color, alpha } => {
                        if let Some(t) = tex(*asset) {
                            image(t, rect, *alpha, *color);
                        } else if let Some(v) = asset.and_then(|id| gowring_core::vector::icon_prims(id, rect, *color, *alpha)) {
                            draw_prims(&dl, &v);
                            if matches!(*asset, Some("hud_icon_axe" | "hud_icon_blades" | "hud_icon_spear")) && !snap.hud.weapon_name.is_empty() {
                                let _f = self.big_font.as_ref().map(|f| ui.push_font(f.0));
                                dl.add_text([rect.x, rect.y + rect.h + 8.0], col(0xE8E2C8, 0.9 * alpha), snap.hud.weapon_name);
                            }
                        } else {
                            dl.add_rect([rect.x, rect.y], [rect.x + rect.w, rect.y + rect.h], col(*color, 0.5 * alpha)).thickness(2.0).build();
                            if let Some(id) = asset {
                                // stand-in label until the Ragnarök art is converted (alpha channel only)
                                let label = id.rsplit('_').next().unwrap_or(id).to_ascii_uppercase();
                                dl.add_text([rect.x + 4.0, rect.y + 4.0], col(*color, 0.7 * alpha), &label);
                            }
                        }
                    }
                    DrawCmd::Prompt { rect, text, glyph, backing, color, alpha } => {
                        match tex(*backing) {
                            Some(t) => image(t, rect, *alpha, 0xFFFFFF),
                            None => {
                                dl.add_rect([rect.x, rect.y], [rect.x + rect.w, rect.y + rect.h], col(0x000000, 0.45 * alpha)).filled(true).rounding(6.0).build();
                            }
                        }
                        let mut tx = rect.x + 10.0;
                        if let Some(t) = tex(*glyph) {
                            let side = rect.h * 0.8;
                            image(t, &Rect { x: rect.x + 6.0, y: rect.y + rect.h * 0.1, w: side, h: side }, *alpha, 0xFFFFFF);
                            tx = rect.x + 6.0 + side + 8.0;
                        }
                        if let Some(s) = text {
                            let _f = self.big_font.as_ref().map(|f| ui.push_font(f.0));
                            let sz = ui.calc_text_size(s);
                            dl.add_text([tx, rect.y + (rect.h - sz[1]) / 2.0], col(*color, *alpha), s);
                        }
                    }
                    DrawCmd::Text { rect, text, color, alpha, align, plate, portrait, .. } => {
                        match tex(*plate) {
                            Some(t) => image(t, rect, *alpha, 0xFFFFFF),
                            None => {
                                dl.add_rect([rect.x, rect.y], [rect.x + rect.w, rect.y + rect.h], col(0x000000, 0.55 * alpha)).filled(true).rounding(8.0).build();
                            }
                        }
                        dl.add_text([rect.x + 14.0, rect.y + 6.0], col(0xC9BB8E, 0.9 * alpha), "MIMIR");
                        let mut left = rect.x + 16.0;
                        if let Some(t) = tex(*portrait) {
                            let side = rect.h * 0.9;
                            image(t, &Rect { x: rect.x + 8.0, y: rect.y + rect.h * 0.05, w: side, h: side }, *alpha, 0xFFFFFF);
                            left = rect.x + 8.0 + side + 12.0;
                        }
                        let _f = self.big_font.as_ref().map(|f| ui.push_font(f.0));
                        let avail = rect.x + rect.w - 16.0 - left;
                        let lines = wrap(ui, text, avail);
                        let line_h = ui.calc_text_size("Ag")[1];
                        let total = line_h * lines.len() as f32;
                        for (i, line) in lines.iter().enumerate() {
                            let sz = ui.calc_text_size(line);
                            let x = if *align == Align::Center { left + (avail - sz[0]) / 2.0 } else { left };
                            dl.add_text([x, rect.y + (rect.h - total) / 2.0 + line_h * i as f32], col(*color, *alpha), line);
                        }
                    }
                }
            }
            // Thrown weapons: the weapon's own icon where the axe / spears are, spinning (axe) or pointing along the
            // throw (spear), with a soft halo. There is no flying model, so this is what the player sees in flight.
            let px_per_m = h / (2.0 * tan.max(0.05));
            for (id, p) in &snap.projectiles {
                let Some((sx, sy)) = project(*p) else { continue };
                let z = (*p - pos).dot(fwd).max(0.5);
                let size = (px_per_m * 0.9 / z).clamp(30.0, 150.0);
                let rect = Rect { x: sx - size / 2.0, y: sy - size / 2.0, w: size, h: size };
                dl.add_circle([sx, sy], size * 0.5, col(0x9FE3FF, 0.16)).filled(true).num_segments(24).build();
                let (asset, angle) = if *id == "leviathan_axe" { ("hud_icon_axe", snap.hud.pulse * std::f32::consts::TAU * 3.0) } else { ("hud_icon_spear", 0.0) };
                if let Some(v) = gowring_core::vector::icon_prims(asset, &rect, 0xFFFFFF, 1.0) {
                    draw_prims(&dl, &gowring_core::vector::rotated(&v, (sx, sy), angle));
                } else {
                    dl.add_circle([sx, sy], 9.0, col(0x9FE3FF, 0.95)).filled(true).num_segments(16).build();
                }
            }
        }
        if snap.valid {
            let (right, up, fwd, pos) = (snap.cam_right, snap.cam_up, snap.cam_fwd, snap.cam_pos);
            let tan = (snap.fov_rad.clamp(0.2, 2.5) * 0.5).tan();
            let aspect = w / h;
            let project = move |p: Vec3| -> Option<(f32, f32)> {
                let d = p - pos;
                let z = d.dot(fwd);
                if z <= 0.1 {
                    return None;
                }
                let nx = d.dot(right) / (z * tan * aspect);
                let ny = d.dot(up) / (z * tan);
                if nx.abs() > 1.6 || ny.abs() > 1.6 {
                    return None;
                }
                Some(((nx * 0.5 + 0.5) * w, (0.5 - ny * 0.5) * h))
            };
            for f in &snap.fx {
                let (Some((cx, cy)), Some((ex, ey))) = (project(f.pos), project(f.pos + right * f.radius)) else { continue };
                let reach = ((ex - cx).powi(2) + (ey - cy).powi(2)).sqrt().clamp(14.0, w * 0.6);
                let t = (f.age / FX_LIFE_S).clamp(0.0, 1.0);
                let grow = 1.0 - (1.0 - t).powi(3);
                let a = (1.0 - t).powf(1.4);
                let (main, light) = match f.kind {
                    gowring_core::sheets::FxKind::Frost => (0x7FD8FF, 0xE6F8FF),
                    gowring_core::sheets::FxKind::Fire => (0xFF8A3A, 0xFFD9A0),
                    gowring_core::sheets::FxKind::Blast => (0xFFD27A, 0xFFFFFF),
                };
                let r = reach * (0.2 + 0.8 * grow);
                dl.add_circle([cx, cy], r, col(main, 0.9 * a)).thickness(2.0 + 5.0 * (1.0 - t)).num_segments(48).build();
                dl.add_circle([cx, cy], r * 0.82, col(light, 0.55 * a)).thickness(1.5).num_segments(48).build();
                dl.add_circle([cx, cy], r * 0.6, col(main, 0.18 * a)).filled(true).num_segments(32).build();
                for k in 0..10 {
                    let ang = k as f32 * std::f32::consts::TAU / 10.0 + 0.3;
                    let (c0, s0) = (ang.cos(), ang.sin());
                    dl.add_line([cx + c0 * r * 0.9, cy + s0 * r * 0.9], [cx + c0 * r * (1.06 + 0.25 * grow), cy + s0 * r * (1.06 + 0.25 * grow)], col(light, 0.8 * a)).thickness(2.0).build();
                }
            }
        }
        if snap.valid && snap.hint > 0.01 {
            let st = crate::settings::get();
            let mut lines: Vec<String> = [
                "L2 hold: aim     L2 + R1: throw",
                "Triangle: recall axe / detonate spears",
                "D-pad right: Axe > Blades > Spear",
                "L1: block (tap just before a hit: parry)",
                "D-pad up / down: light / heavy runic",
                "L3 + R3: Spartan Rage (full bar)",
                "R3 near a staggered enemy: finisher (wear them down first)",
                "Tweak things live: settings.json",
            ]
            .iter()
            .map(|l| l.to_string())
            .collect();
            if st.debug_tools {
                lines.push(format!("F{} (keyboard): summon a tough enemy", st.debug_spawn_key));
            }
            let lines = &lines;
            let _f = self.big_font.as_ref().map(|f| ui.push_font(f.0));
            let line_h = ui.calc_text_size("Ag")[1] + 4.0;
            let width = lines.iter().map(|l| ui.calc_text_size(l)[0]).fold(0.0f32, f32::max).max(ui.calc_text_size("KRATOS CONTROLS")[0]) + 44.0;
            let height = line_h * (lines.len() as f32 + 1.4) + 28.0;
            let (x0, y0) = (w - width - 36.0, h * 0.09);
            let a = snap.hint;
            dl.add_rect([x0, y0], [x0 + width, y0 + height], col(0x000000, 0.58 * a)).filled(true).rounding(8.0).build();
            dl.add_rect([x0, y0], [x0 + width, y0 + height], col(0xC9BB8E, 0.5 * a)).rounding(8.0).thickness(1.5).build();
            dl.add_text([x0 + 22.0, y0 + 12.0], col(0xFFD27A, a), "KRATOS CONTROLS");
            for (i, l) in lines.iter().enumerate() {
                dl.add_text([x0 + 22.0, y0 + 14.0 + line_h * (i as f32 + 1.4)], col(0xF1ECD8, 0.95 * a), l);
            }
        }
        let toast = TOAST.lock().ok().and_then(|g| g.clone());
        if let Some((msg, t0)) = toast {
            let age = t0.elapsed().as_secs_f32();
            if age < 4.0 {
                let a = if age > 3.3 { (4.0 - age) / 0.7 } else { 1.0 };
                let _f = self.big_font.as_ref().map(|f| ui.push_font(f.0));
                let sz = ui.calc_text_size(&msg);
                let (x0, y0) = ((w - sz[0]) / 2.0 - 18.0, h * 0.16);
                dl.add_rect([x0, y0], [x0 + sz[0] + 36.0, y0 + sz[1] + 18.0], col(0x000000, 0.6 * a)).filled(true).rounding(8.0).build();
                dl.add_rect([x0, y0], [x0 + sz[0] + 36.0, y0 + sz[1] + 18.0], col(0xC9BB8E, 0.5 * a)).rounding(8.0).thickness(1.5).build();
                dl.add_text([x0 + 18.0, y0 + 9.0], col(0xFFD27A, a), &msg);
            }
        }
        if alpha_mode && !snap.banner.is_empty() {
            let sz = ui.calc_text_size(&snap.banner);
            dl.add_text([w - sz[0] - 14.0, h - sz[1] - 10.0], col(0xFFD27A, 0.9), &snap.banner);
        }
    }
}

/// Draw the shared vector art (see `gowring_core::vector`) with ImGui's draw list.
fn draw_prims(dl: &imgui::DrawListMut<'_>, prims: &[gowring_core::vector::Prim]) {
    use gowring_core::vector::Prim;
    for p in prims {
        match p {
            Prim::Tri { p, color, alpha } => {
                dl.add_triangle([p[0].0, p[0].1], [p[1].0, p[1].1], [p[2].0, p[2].1], col(*color, *alpha)).filled(true).build();
            }
            Prim::Line { a, b, thick, color, alpha } => {
                dl.add_line([a.0, a.1], [b.0, b.1], col(*color, *alpha)).thickness(*thick).build();
            }
            Prim::Circle { c, r, fill, thick, color, alpha } => {
                let b = dl.add_circle([c.0, c.1], *r, col(*color, *alpha)).num_segments(48);
                if *fill {
                    b.filled(true).build();
                } else {
                    b.thickness(*thick).build();
                }
            }
        }
    }
}

/// Greedy word wrap into at most three lines.
fn wrap(ui: &Ui, text: &str, max_w: f32) -> Vec<String> {
    let mut lines: Vec<String> = Vec::new();
    let mut cur = String::new();
    for word in text.split_whitespace() {
        let trial = if cur.is_empty() { word.to_string() } else { format!("{cur} {word}") };
        if ui.calc_text_size(&trial)[0] > max_w && !cur.is_empty() {
            lines.push(std::mem::take(&mut cur));
            cur = word.to_string();
        } else {
            cur = trial;
        }
    }
    if !cur.is_empty() {
        lines.push(cur);
    }
    lines.truncate(3);
    lines
}
