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
}

pub type Shared = Arc<Mutex<Snapshot>>;

pub struct Overlay {
    shared: Shared,
    store: Arc<Store>,
    textures: HashMap<&'static str, (TextureId, f32, f32)>,
    big_font: Option<SendFont>,
}

/// `FontId` wraps a raw pointer into ImGui's atlas; it is only ever used on the render thread.
struct SendFont(FontId);
unsafe impl Send for SendFont {}
unsafe impl Sync for SendFont {}


impl Overlay {
    pub fn new(shared: Shared, store: Arc<Store>) -> Self {
        Overlay { shared, store, textures: HashMap::new(), big_font: None }
    }
}

fn col(rgb: u32, alpha: f32) -> [f32; 4] {
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
                    ok += 1;
                }
                Err(e) => crate::glog!("overlay", "texture {id}: {e:?}"),
            }
        }
        crate::glog!("overlay", "initialised: {ok}/{} textures uploaded", self.store.textures.len());
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
            for c in &cmds {
                match c {
                    DrawCmd::Overlay { color, alpha } => {
                        if *alpha > 0.002 {
                            dl.add_rect([0.0, 0.0], [w, h], col(*color, *alpha)).filled(true).build();
                        }
                    }
                    DrawCmd::Bar { rect, frac, trail, frame, fill, color, alpha } => {
                        let (x0, y0, x1, y1) = (rect.x, rect.y, rect.x + rect.w, rect.y + rect.h);
                        match tex(*frame) {
                            Some(t) => image(t, rect, *alpha, 0xFFFFFF),
                            None => {
                                dl.add_rect([x0, y0], [x1, y1], col(0x101010, 0.65 * alpha)).filled(true).build();
                            }
                        }
                        // the lagging "recent damage" trail, then the fill
                        if *trail > *frac {
                            dl.add_rect([x0, y0], [x0 + rect.w * trail, y1], col(0xE8E2C8, 0.45 * alpha)).filled(true).build();
                        }
                        match tex(*fill) {
                            Some(t) => {
                                dl.add_image(t.0, [x0, y0], [x0 + rect.w * frac, y1]).uv_min([0.0, 0.0]).uv_max([*frac, 1.0]).col(col(0xFFFFFF, *alpha)).build();
                            }
                            None => {
                                dl.add_rect([x0, y0], [x0 + rect.w * frac, y1], col(*color, *alpha)).filled(true).build();
                            }
                        }
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
                    DrawCmd::Icon { rect, asset, color, alpha } => match tex(*asset) {
                        Some(t) => image(t, rect, *alpha, *color),
                        None => {
                            dl.add_rect([rect.x, rect.y], [rect.x + rect.w, rect.y + rect.h], col(*color, 0.5 * alpha)).thickness(2.0).build();
                        }
                    },
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
            // thrown weapons: a bright marker where the axe / spears are (no model in v0.1)
            for (_, p) in &snap.projectiles {
                if let Some((sx, sy)) = project(*p) {
                    dl.add_circle([sx, sy], 9.0, col(0x9FE3FF, 0.95)).filled(true).num_segments(16).build();
                    dl.add_circle([sx, sy], 14.0, col(0xFFFFFF, 0.6)).thickness(2.0).num_segments(16).build();
                }
            }
        }
        if alpha_mode && !snap.banner.is_empty() {
            let sz = ui.calc_text_size(&snap.banner);
            dl.add_text([w - sz[0] - 14.0, h - sz[1] - 10.0], col(0xFFD27A, 0.9), &snap.banner);
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
