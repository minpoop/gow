//! The HUD as data. `HudState` holds what to show; `draw_list` lays it out from `hud.json` and
//! returns plain draw commands the renderer (ImGui in the DLL, a rasteriser in tests) executes.

use crate::data::Data;
use crate::sheets::*;
use crate::types::*;
use crate::vector::{self, Prim};

#[derive(Clone, Debug, Default)]
pub struct EnemyBar {
    pub id: EnemyId,
    pub pos: Vec3,
    pub hp_frac: f32,
    pub stun_frac: f32,
    pub stunned: bool,
    pub boss: bool,
    /// The frost / burn build-up from weapon hits (0-1) and the colour it is drawn in (0 = none).
    pub status_frac: f32,
    pub status_color: u32,
    /// "BURNING" / "FROZEN" while a full meter's status lasts.
    pub status_label: Option<&'static str>,
}

#[derive(Clone, Debug, Default)]
pub struct RunicHud {
    pub id: &'static str,
    pub name: &'static str,
    /// 1.0 = just used … 0.0 = ready.
    pub remaining_frac: f32,
    pub ready: bool,
    /// The plate's colours from the runic's weapon (0 = the HUD sheet's): face, lit tone, soft halo (0..1).
    pub color: u32,
    pub light: u32,
    pub glow: f32,
}

#[derive(Clone, Debug, Default)]
pub struct HudState {
    pub hp_frac: f32,
    pub hp_trail_frac: f32,
    pub rage_frac: f32,
    pub rage_ready: bool,
    pub rage_active: bool,
    pub runic_light: Option<RunicHud>,
    pub runic_heavy: Option<RunicHud>,
    pub weapon_icon: Option<&'static str>,
    pub weapon_name: &'static str,
    pub shield_drawn: bool,
    pub aiming: bool,
    pub recall_available: bool,
    pub recall_label: &'static str,
    pub enemies: Vec<EnemyBar>,
    pub boss: Option<EnemyBar>,
    pub finisher_target: Option<EnemyId>,
    pub subtitle: Option<String>,
    pub mimir_speaking: bool,
    pub rage_overlay: f32,
    pub low_health_overlay: f32,
    pub in_combat: bool,
    /// Pulse phase 0..1 for "ready" glows.
    pub pulse: f32,
    /// HUD size multiplier from the player's settings (0 or unset = 1.0). Elements scale about their own anchor corner.
    pub ui_scale: f32,
    /// Seconds on a running clock; drives the HUD's moving parts (the fire around the Rage prompt).
    pub time_s: f32,
    /// The runic picker, shown for a moment after L1 + D-pad changes a slot.
    pub runic_picker: Option<RunicPicker>,
}

/// The runics a weapon can put in the slot just changed, with the chosen one marked.
#[derive(Clone, Debug, Default)]
pub struct RunicPicker {
    pub heavy: bool,
    pub weapon_name: &'static str,
    /// (name, icon, what it does), in the sheet's order.
    pub options: Vec<(&'static str, Option<&'static str>, &'static str)>,
    /// The weapon's plate colours: face, lit tone, soft halo.
    pub colors: (u32, u32, f32),
    pub chosen: usize,
    /// 1 while shown, fading to 0 at the end.
    pub alpha: f32,
}

#[derive(Clone, Copy, Debug)]
pub struct Screen {
    pub w: f32,
    pub h: f32,
}

impl Screen {
    /// Uniform scale and letterbox offset of the 1920x1080 virtual canvas.
    pub fn fit(&self) -> (f32, f32, f32) {
        let s = (self.w / 1920.0).min(self.h / 1080.0);
        (s, (self.w - 1920.0 * s) / 2.0, (self.h - 1080.0 * s) / 2.0)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Align {
    Left,
    Center,
}

/// A rectangle in real screen pixels.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

#[derive(Clone, Debug)]
pub enum DrawCmd {
    /// `color` fills the bar, glowing to `lead` at the leading edge; `segments` > 1 splits it (the Rage meter).
    Bar { rect: Rect, frac: f32, trail: f32, frame: Option<&'static str>, fill: Option<&'static str>, color: u32, lead: u32, segments: u32, alpha: f32 },
    /// A runic plate `rx` by `ry` from its centre: `color` its face, `light` its lit tone, `aura` a soft halo (0..1).
    Ring { cx: f32, cy: f32, rx: f32, ry: f32, remaining: f32, ready: bool, frame: Option<&'static str>, glow: Option<&'static str>, icon: Option<&'static str>, color: u32, light: u32, aura: f32, alpha: f32 },
    /// `mono`: drawn as a white silhouette (Ragnarök's glyph style) when there is no picture for it.
    Icon { rect: Rect, asset: Option<&'static str>, color: u32, alpha: f32, mono: bool },
    Text { rect: Rect, px: f32, text: String, color: u32, alpha: f32, align: Align, plate: Option<&'static str>, portrait: Option<&'static str> },
    Prompt { rect: Rect, text: Option<String>, glyph: Option<&'static str>, backing: Option<&'static str>, color: u32, alpha: f32 },
    Overlay { color: u32, alpha: f32 },
    /// Plain vector shapes drawn as they are (plates behind icons, small hints).
    Shapes { prims: Vec<Prim> },
    /// A line of text, `px` tall, with its top at `y` and starting at `x` (or centred on `x`).
    Label { x: f32, y: f32, px: f32, text: String, color: u32, alpha: f32, align: Align },
}

fn visible(v: HudVisibleWhen, s: &HudState) -> bool {
    match v {
        HudVisibleWhen::Always => true,
        HudVisibleWhen::InCombat => s.in_combat || !s.enemies.is_empty(),
        HudVisibleWhen::Aiming => s.aiming,
        HudVisibleWhen::EnemyStunned => s.enemies.iter().any(|e| e.stunned),
        HudVisibleWhen::RageReady => s.rage_ready,
        HudVisibleWhen::RageActive => s.rage_active,
        HudVisibleWhen::MimirSpeaking => s.mimir_speaking && s.subtitle.is_some(),
        HudVisibleWhen::BossEngaged => s.boss.is_some(),
        HudVisibleWhen::LowHealth => s.low_health_overlay > 0.01,
        HudVisibleWhen::FinisherAvailable => s.finisher_target.is_some(),
        HudVisibleWhen::RecallAvailable => s.recall_available,
    }
}

/// Where an element sits, in virtual 1920x1080 pixels (top-left of its rectangle).
fn anchored(h: &HudElementDef) -> (f32, f32) {
    let (x, y, w, hh) = (h.x as f32, h.y as f32, h.w as f32, h.h as f32);
    match h.anchor {
        HudAnchor::TopLeft => (x, y),
        HudAnchor::TopCenter => (960.0 - w / 2.0 + x, y),
        HudAnchor::TopRight => (1920.0 - w - x, y),
        HudAnchor::BottomLeft => (x, 1080.0 - hh - y),
        HudAnchor::BottomCenter => (960.0 - w / 2.0 + x, 1080.0 - hh - y),
        HudAnchor::BottomRight => (1920.0 - w - x, 1080.0 - hh - y),
        HudAnchor::Center => (960.0 - w / 2.0 + x, 540.0 - hh / 2.0 + y),
        HudAnchor::World => (x, y),
    }
}

/// The point of the 1920x1080 canvas an anchor scales about.
fn anchor_ref(a: HudAnchor) -> (f32, f32) {
    match a {
        HudAnchor::TopLeft | HudAnchor::World => (0.0, 0.0),
        HudAnchor::TopCenter => (960.0, 0.0),
        HudAnchor::TopRight => (1920.0, 0.0),
        HudAnchor::BottomLeft => (0.0, 1080.0),
        HudAnchor::BottomCenter => (960.0, 1080.0),
        HudAnchor::BottomRight => (1920.0, 1080.0),
        HudAnchor::Center => (960.0, 540.0),
    }
}

/// Lay the HUD out for `screen`. `project` maps a world point to real screen pixels (None when
/// behind the camera).
pub fn draw_list(d: &Data, s: &HudState, screen: Screen, project: &dyn Fn(Vec3) -> Option<(f32, f32)>) -> Vec<DrawCmd> {
    let (k, ox, oy) = screen.fit();
    let mut out = Vec::new();
    let us = if s.ui_scale > 0.05 { s.ui_scale.clamp(0.4, 3.0) } else { 1.0 };
    let rect_at = |h: &HudElementDef| {
        let (vx, vy) = anchored(h);
        // scale about the element's anchor point so corner elements stay in their corners
        let (rx, ry) = anchor_ref(h.anchor);
        let (nx, ny) = (rx + (vx - rx) * us, ry + (vy - ry) * us);
        Rect { x: ox + nx * k, y: oy + ny * k, w: h.w as f32 * k * us, h: h.h as f32 * k * us }
    };
    let pulse_alpha = 0.65 + 0.35 * (s.pulse * std::f32::consts::TAU).sin().abs();

    for h in HUD {
        if !visible(h.visible_when, s) {
            continue;
        }
        let color = h.color;
        let (lead, segments) = (h.lead, h.segments.max(1) as u32);
        match h.bind {
            HudBind::PlayerHp => out.push(DrawCmd::Bar {
                rect: rect_at(h),
                frac: s.hp_frac,
                trail: s.hp_trail_frac,
                frame: h.frame,
                fill: h.fill,
                color,
                lead,
                segments,
                alpha: 1.0,
            }),
            HudBind::PlayerRage => out.push(DrawCmd::Bar {
                rect: rect_at(h),
                frac: s.rage_frac,
                trail: s.rage_frac,
                frame: h.frame,
                fill: h.fill,
                color,
                lead,
                segments,
                alpha: if s.rage_active { pulse_alpha } else { 1.0 },
            }),
            // Ragnarök's prompt: the two stick buttons, on fire
            HudBind::RageReady => out.push(DrawCmd::Shapes { prims: vector::rage_prompt(&rect_at(h), s.time_s, 1.0) }),
            HudBind::RunicLightCd | HudBind::RunicHeavyCd => {
                let r = if h.bind == HudBind::RunicLightCd { &s.runic_light } else { &s.runic_heavy };
                let Some(r) = r else { continue };
                let rc = rect_at(h);
                // light on top, heavy below, like Ragnarök's pair, each with its own glyph; the sheet's box includes the plate's outer silver line
                let inset = 4.0 * k * us;
                out.push(DrawCmd::Ring {
                    cx: rc.x + rc.w / 2.0,
                    cy: rc.y + rc.h / 2.0,
                    rx: rc.w / 2.0 - inset,
                    ry: rc.h / 2.0 - inset,
                    remaining: r.remaining_frac,
                    ready: r.ready,
                    frame: h.frame,
                    glow: h.fill,
                    icon: d.runic(r.id).icon.or(s.weapon_icon),
                    // the runic's weapon colours its plate (frozen blue, burning red-orange, sandy yellow)
                    color: if r.color != 0 { r.color } else { color },
                    light: if r.light != 0 { r.light } else { lead },
                    aura: r.glow,
                    alpha: 1.0,
                });
            }
            HudBind::WeaponCurrent | HudBind::ShieldState => {
                // Ragnarök's cluster: the weapon on a round plate next to the bars, the shield on an octagon where
                // Ragnarök keeps the relic
                let rc = rect_at(h);
                let (cx, cy) = (rc.x + rc.w / 2.0, rc.y + rc.h / 2.0);
                if h.bind == HudBind::WeaponCurrent {
                    let r = rc.w.min(rc.h) / 2.0;
                    let mut prims = vector::plate(vector::PlateShape::Disc, cx, cy, r, r, vector::shade(lead, 0.5), lead, 1.0, 1.0);
                    // the fine line that runs from the plate between the health and Rage bars, with its stud
                    let len = 330.0 * k * us;
                    for i in 0..8 {
                        let (a, b) = (cx + r + len * i as f32 / 8.0, cx + r + len * (i + 1) as f32 / 8.0);
                        prims.push(vector::Prim::Line { a: (a, cy), b: (b, cy), thick: (1.2 * k * us).max(1.0), color: 0xD0CCD4, alpha: 0.7 * (1.0 - i as f32 / 8.0) });
                    }
                    let d = (3.2 * k * us).max(2.0);
                    let stud = vec![(cx + r, cy - d), (cx + r + d, cy), (cx + r, cy + d), (cx + r - d, cy)];
                    prims.push(vector::Prim::Poly { pts: stud.clone(), color: 0x1A181C, alpha: 0.95 });
                    for i in 0..4 {
                        prims.push(vector::Prim::Line { a: stud[i], b: stud[(i + 1) % 4], thick: (1.0 * k * us).max(1.0), color: 0xE8E4EC, alpha: 0.9 });
                    }
                    out.push(DrawCmd::Shapes { prims });
                    let inner = r * 0.62;
                    out.push(DrawCmd::Icon { rect: Rect { x: cx - inner, y: cy - inner, w: inner * 2.0, h: inner * 2.0 }, asset: s.weapon_icon, color, alpha: 1.0, mono: true });
                } else {
                    let (rx, ry) = (rc.w / 2.0 - 4.0 * k * us, rc.h / 2.0 - 4.0 * k * us);
                    out.push(DrawCmd::Shapes { prims: vector::plate(vector::PlateShape::Octagon, cx, cy, rx, ry, vector::shade(lead, 0.35), lead, 1.0, 1.0) });
                    let inner = ry * 0.66;
                    let alpha = if s.shield_drawn { 1.0 } else { 0.55 };
                    out.push(DrawCmd::Icon { rect: Rect { x: cx - inner, y: cy - inner, w: inner * 2.0, h: inner * 2.0 }, asset: h.glyph, color, alpha, mono: true });
                }
            }
            HudBind::ThrowAim => out.push(DrawCmd::Icon { rect: rect_at(h), asset: h.glyph, color, alpha: 0.95, mono: false }),
            HudBind::RecallState => out.push(DrawCmd::Prompt { rect: rect_at(h), text: Some(s.recall_label.to_string()), glyph: h.glyph, backing: None, color, alpha: 1.0 }),
            HudBind::Finisher => out.push(DrawCmd::Prompt { rect: rect_at(h), text: Some("Finish".into()), glyph: h.glyph, backing: h.frame, color, alpha: pulse_alpha }),
            HudBind::EnemyHp | HudBind::EnemyStun | HudBind::EnemyStunned => {
                for e in s.enemies.iter().filter(|e| !e.boss) {
                    let Some((px, py)) = project(e.pos + Vec3::new(0.0, 2.3, 0.0)) else { continue };
                    let (w, hh) = (h.w as f32 * k * us, h.h as f32 * k * us);
                    let rc = Rect { x: px - w / 2.0 + h.x as f32 * k * 0.0, y: py + h.y as f32 * k, w, h: hh };
                    match h.bind {
                        HudBind::EnemyHp => out.push(DrawCmd::Bar { rect: rc, frac: e.hp_frac, trail: e.hp_frac, frame: h.frame, fill: h.fill, color, lead, segments, alpha: 0.95 }),
                        HudBind::EnemyStun => {
                            out.push(DrawCmd::Bar { rect: rc, frac: e.stun_frac, trail: e.stun_frac, frame: h.frame, fill: h.fill, color, lead, segments, alpha: 0.95 });
                            if e.status_frac > 0.01 && e.status_color != 0 {
                                let thin = Rect { y: rc.y + rc.h + 2.0 * k * us, h: (rc.h * 0.5).max(2.0), ..rc };
                                out.push(DrawCmd::Bar { rect: thin, frac: e.status_frac, trail: e.status_frac, frame: None, fill: None, color: e.status_color, lead: vector::lighten(e.status_color, 0.4), segments: 1, alpha: 0.95 });
                            }
                            if let Some(label) = e.status_label {
                                let tag = Rect { y: rc.y + rc.h * 2.0 + 6.0 * k * us, h: 16.0 * k * us, ..rc };
                                let tint = if label == "FROZEN" { 0x7FD8FF } else { 0xFF8A3A };
                                out.push(DrawCmd::Label { x: tag.x + tag.w / 2.0, y: tag.y, px: 14.0 * k * us, text: label.to_string(), color: tint, alpha: 1.0, align: Align::Center });
                            }
                        }
                        _ if e.stunned => out.push(DrawCmd::Icon { rect: rc, asset: h.glyph, color, alpha: pulse_alpha, mono: false }),
                        _ => {}
                    }
                }
            }
            HudBind::BossHp => {
                if let Some(b) = &s.boss {
                    out.push(DrawCmd::Bar { rect: rect_at(h), frac: b.hp_frac, trail: b.hp_frac, frame: h.frame, fill: h.fill, color, lead, segments, alpha: 1.0 });
                }
            }
            HudBind::BossStun => {
                if let Some(b) = &s.boss {
                    out.push(DrawCmd::Bar { rect: rect_at(h), frac: b.stun_frac, trail: b.stun_frac, frame: h.frame, fill: h.fill, color, lead, segments, alpha: 1.0 });
                }
            }
            HudBind::MimirSubtitle => {
                if let Some(text) = &s.subtitle {
                    let mut rc = rect_at(h);
                    rc.h = rc.h.max(d.mimir.subtitle_px as f32 * k * us * 2.2);
                    out.push(DrawCmd::Text {
                        rect: rc,
                        px: d.mimir.subtitle_px as f32 * k * us,
                        text: text.clone(),
                        color,
                        alpha: 1.0,
                        align: Align::Center,
                        plate: h.frame,
                        portrait: h.glyph,
                    });
                }
            }
            HudBind::RageOverlay => out.push(DrawCmd::Overlay { color, alpha: s.rage_overlay * d.rage.tint_strength }),
            HudBind::LowHealthOverlay => out.push(DrawCmd::Overlay { color, alpha: s.low_health_overlay * 0.5 }),
        }
    }
    if let Some(p) = &s.runic_picker {
        picker(p, &mut out, (k, ox, oy), us);
    }
    out
}

/// The runic picker: a panel above the runic plates listing the weapon's runics for the slot just changed, the
/// chosen one lit with what it does. Laid out on the 1920x1080 canvas from the bottom-left corner, like the cluster.
fn picker(p: &RunicPicker, out: &mut Vec<DrawCmd>, (k, ox, oy): (f32, f32, f32), us: f32) {
    let a = p.alpha.clamp(0.0, 1.0);
    if a <= 0.01 || p.options.is_empty() {
        return;
    }
    let at = |x: f32, y: f32| (ox + x * us * k, oy + (1080.0 + (y - 1080.0) * us) * k);
    let sz = |v: f32| v * k * us;
    let (x0, w) = (36.0, 480.0);
    let row_h = 54.0;
    let desc_px = 17.0;
    let per_line = ((w - 40.0) / (desc_px * 0.52)) as usize;
    let desc = wrap_chars(p.options.get(p.chosen).map(|o| o.2).unwrap_or(""), per_line.max(10));
    let h = 46.0 + row_h * p.options.len() as f32 + 10.0 + 24.0 * desc.len() as f32 + 34.0;
    let bottom = 872.0;
    let top = bottom - h;
    // the panel
    let (px0, py0) = at(x0, top);
    let (px1, py1) = at(x0 + w, bottom);
    let frame = vec![(px0, py0), (px1, py0), (px1, py1), (px0, py1)];
    let mut bg = vec![Prim::Poly { pts: frame.clone(), color: 0x0B0D12, alpha: 0.78 * a }];
    for i in 0..4 {
        bg.push(Prim::Line { a: frame[i], b: frame[(i + 1) % 4], thick: sz(1.5).max(1.0), color: 0xB8BEC6, alpha: 0.55 * a });
    }
    out.push(DrawCmd::Shapes { prims: bg });
    let (tx, ty) = at(x0 + 20.0, top + 12.0);
    let title = format!("{} RUNIC  -  {}", if p.heavy { "HEAVY" } else { "LIGHT" }, p.weapon_name);
    out.push(DrawCmd::Label { x: tx, y: ty, px: sz(20.0), text: title, color: 0xFFD27A, alpha: a, align: Align::Left });
    for (i, (name, icon, _)) in p.options.iter().enumerate() {
        let chosen = i == p.chosen;
        let cy = top + 46.0 + row_h * (i as f32 + 0.5);
        let mut prims = Vec::new();
        if chosen {
            let (hx0, hy0) = at(x0 + 8.0, cy - row_h / 2.0 + 3.0);
            let (hx1, hy1) = at(x0 + w - 8.0, cy + row_h / 2.0 - 3.0);
            prims.push(Prim::Poly { pts: vec![(hx0, hy0), (hx1, hy0), (hx1, hy1), (hx0, hy1)], color: 0xFFFFFF, alpha: 0.08 * a });
            let (bx1, _) = at(x0 + 12.0, cy);
            prims.push(Prim::Poly { pts: vec![(hx0, hy0), (bx1, hy0), (bx1, hy1), (hx0, hy1)], color: 0xFFD27A, alpha: 0.9 * a });
        }
        let (cx, ccy) = at(x0 + 46.0, cy);
        let (face, lit, aura) = p.colors;
        prims.extend(vector::runic_plate(cx, ccy, sz(22.0), sz(19.0), face, lit, if chosen { 1.0 } else { 0.0 }, if chosen { aura } else { 0.0 }, a));
        out.push(DrawCmd::Shapes { prims });
        let g = sz(13.0);
        out.push(DrawCmd::Icon { rect: Rect { x: cx - g, y: ccy - g, w: g * 2.0, h: g * 2.0 }, asset: *icon, color: 0xFFFFFF, alpha: if chosen { a } else { 0.55 * a }, mono: true });
        let (nx, ny) = at(x0 + 80.0, cy - 11.0);
        out.push(DrawCmd::Label { x: nx, y: ny, px: sz(22.0), text: name.to_string(), color: if chosen { 0xFFFFFF } else { 0x9AA0A8 }, alpha: a, align: Align::Left });
    }
    let mut y = top + 46.0 + row_h * p.options.len() as f32 + 10.0;
    for line in desc {
        let (lx, ly) = at(x0 + 20.0, y);
        out.push(DrawCmd::Label { x: lx, y: ly, px: sz(desc_px), text: line, color: 0xC8CCD2, alpha: a, align: Align::Left });
        y += 24.0;
    }
    let (hx, hy) = at(x0 + 20.0, y + 6.0);
    out.push(DrawCmd::Label { x: hx, y: hy, px: sz(15.0), text: "Hold L1, press D-pad up / down: next light / heavy runic".into(), color: 0x8A9098, alpha: a, align: Align::Left });
}

/// Greedy word wrap by character count (the HUD has no font metrics here).
fn wrap_chars(text: &str, per_line: usize) -> Vec<String> {
    let mut lines: Vec<String> = Vec::new();
    for word in text.split_whitespace() {
        match lines.last_mut() {
            Some(l) if l.len() + 1 + word.len() <= per_line => {
                l.push(' ');
                l.push_str(word);
            }
            _ => lines.push(word.to_string()),
        }
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::*;

    fn project(p: Vec3) -> Option<(f32, f32)> {
        Some((960.0 + p.x * 20.0, 540.0 - p.y * 20.0))
    }

    fn base_state() -> HudState {
        let d = Data::get();
        HudState {
            hp_frac: 0.8,
            hp_trail_frac: 0.9,
            rage_frac: 0.4,
            runic_light: Some(RunicHud { id: "axe_runic_light", name: "Frost Shockwave", remaining_frac: 0.5, ready: false, ..Default::default() }),
            runic_heavy: Some(RunicHud { id: "axe_runic_heavy", name: "Glacial Cleave", remaining_frac: 0.0, ready: true, ..Default::default() }),
            weapon_icon: d.weapon("leviathan_axe").icon,
            weapon_name: "Leviathan Axe",
            shield_drawn: true,
            ..Default::default()
        }
    }

    #[test]
    fn idle_hud_has_vitals_runics_and_weapon_only() {
        let d = Data::get();
        let cmds = draw_list(d, &base_state(), Screen { w: 1920.0, h: 1080.0 }, &project);
        let bars = cmds.iter().filter(|c| matches!(c, DrawCmd::Bar { .. })).count();
        let rings = cmds.iter().filter(|c| matches!(c, DrawCmd::Ring { .. })).count();
        assert_eq!(bars, 2, "health + rage");
        assert_eq!(rings, 2);
        assert!(!cmds.iter().any(|c| matches!(c, DrawCmd::Overlay { .. } | DrawCmd::Text { .. })));
        // health bar sits where the sheet says (virtual == real at 1080p)
        let DrawCmd::Bar { rect, frac, .. } = &cmds[0] else { panic!() };
        assert_eq!((rect.x, rect.y, rect.w, rect.h), (243.0, 939.0, 392.0, 19.0));
        assert_eq!(*frac, 0.8);
    }

    #[test]
    fn ui_scale_grows_elements_about_their_own_corner() {
        let d = Data::get();
        let mut s = base_state();
        s.ui_scale = 2.0;
        let cmds = draw_list(d, &s, Screen { w: 1920.0, h: 1080.0 }, &project);
        let DrawCmd::Bar { rect, .. } = &cmds[0] else { panic!() };
        // bottom-left anchored: distance from the corner and size both double
        assert_eq!((rect.x, 1080.0 - (rect.y + rect.h), rect.w, rect.h), (486.0, 244.0, 784.0, 38.0));
        // the weapon icon's centre keeps its distance from the bottom-left corner times the scale
        let icon = cmds.iter().find_map(|c| if let DrawCmd::Icon { rect, .. } = c { Some(*rect) } else { None }).unwrap();
        let (cx, cy) = (icon.x + icon.w / 2.0, icon.y + icon.h / 2.0);
        assert!((cx - 423.0).abs() < 1e-3 && ((1080.0 - cy) - 237.0).abs() < 1e-3, "{icon:?}");
    }

    #[test]
    fn kratos_pieces_sit_together_in_the_bottom_left_like_ragnarok() {
        let d = Data::get();
        let mut s = base_state();
        s.rage_ready = true;
        s.shield_drawn = false;
        let cmds = draw_list(d, &s, Screen { w: 1920.0, h: 1080.0 }, &project);
        let mut rects = Vec::new();
        for c in &cmds {
            match c {
                DrawCmd::Bar { rect, .. } | DrawCmd::Icon { rect, .. } | DrawCmd::Prompt { rect, .. } => rects.push(*rect),
                DrawCmd::Ring { cx, cy, rx, ry, .. } => rects.push(Rect { x: cx - rx, y: cy - ry, w: rx * 2.0, h: ry * 2.0 }),
                DrawCmd::Shapes { prims } => {
                    let pts: Vec<(f32, f32)> = prims
                        .iter()
                        .flat_map(|p| match p {
                            vector::Prim::Tri { p, .. } => p.to_vec(),
                            vector::Prim::Line { a, b, .. } => vec![*a, *b],
                            vector::Prim::Circle { c, r, .. } => vec![(c.0 - r, c.1 - r), (c.0 + r, c.1 + r)],
                            vector::Prim::Poly { pts, .. } => pts.clone(),
                            vector::Prim::Grad { x0, y0, x1, y1, .. } => vec![(*x0, *y0), (*x1, *y1)],
                        })
                        .collect();
                    let (x0, x1) = (pts.iter().map(|q| q.0).fold(f32::MAX, f32::min), pts.iter().map(|q| q.0).fold(f32::MIN, f32::max));
                    let (y0, y1) = (pts.iter().map(|q| q.1).fold(f32::MAX, f32::min), pts.iter().map(|q| q.1).fold(f32::MIN, f32::max));
                    rects.push(Rect { x: x0, y: y0, w: x1 - x0, h: y1 - y0 });
                }
                _ => {}
            }
        }
        // health, rage, two runics, weapon (plate + glyph), shield (plate + glyph), the L3 + R3 prompt: all inside
        // the corner, all on screen
        assert_eq!(rects.len(), 9);
        for r in &rects {
            assert!(r.x >= 0.0 && r.x + r.w < 700.0 && r.y > 850.0 && r.y + r.h <= 1080.0, "{r:?}");
        }
        let ring_y: Vec<f32> = cmds.iter().filter_map(|c| if let DrawCmd::Ring { cy, .. } = c { Some(*cy) } else { None }).collect();
        assert!(ring_y[0] < ring_y[1], "light runic above heavy");
        // each runic shows its own glyph, not the weapon's
        let icons: Vec<_> = cmds.iter().filter_map(|c| if let DrawCmd::Ring { icon, .. } = c { *icon } else { None }).collect();
        assert_eq!(icons, vec!["hud_icon_runic_frost_wave", "hud_icon_runic_glacial"]);
        // the weapon and the shield sit on plates; the prompt is drawn as shapes
        assert_eq!(cmds.iter().filter(|c| matches!(c, DrawCmd::Shapes { .. })).count(), 3);
        // Rage sits under health, the prompt under Rage
        let DrawCmd::Bar { rect: hp, .. } = &cmds[0] else { panic!() };
        let DrawCmd::Bar { rect: rage, .. } = &cmds[1] else { panic!() };
        assert!(rage.y > hp.y + hp.h && (rage.x - hp.x).abs() < 8.0);
    }

    #[test]
    fn layout_scales_and_letterboxes() {
        let d = Data::get();
        let cmds = draw_list(d, &base_state(), Screen { w: 2560.0, h: 1080.0 }, &project);
        let DrawCmd::Bar { rect, .. } = &cmds[0] else { panic!() };
        // 21:9: scale 1.0, 320 px of side bars
        assert!((rect.x - (320.0 + 243.0)).abs() < 1e-3);
        let cmds = draw_list(d, &base_state(), Screen { w: 3840.0, h: 2160.0 }, &project);
        let DrawCmd::Bar { rect, .. } = &cmds[0] else { panic!() };
        assert!((rect.x - 486.0).abs() < 1e-3 && (rect.w - 784.0).abs() < 1e-3);
    }

    #[test]
    fn combat_elements_follow_state() {
        let d = Data::get();
        let mut s = base_state();
        s.aiming = true;
        s.recall_available = true;
        s.recall_label = "Recall";
        s.in_combat = true;
        s.enemies = vec![
            EnemyBar { id: 1, pos: Vec3::new(2.0, 0.0, 5.0), hp_frac: 0.5, stun_frac: 0.7, stunned: false, boss: false, status_frac: 0.0, status_color: 0, status_label: None },
            EnemyBar { id: 2, pos: Vec3::new(-2.0, 0.0, 5.0), hp_frac: 1.0, stun_frac: 1.0, stunned: true, boss: false, status_frac: 0.0, status_color: 0, status_label: None },
        ];
        s.finisher_target = Some(2);
        s.rage_ready = true;
        s.rage_frac = 1.0;
        let cmds = draw_list(d, &s, Screen { w: 1920.0, h: 1080.0 }, &project);
        // 2 enemies x (hp + stun) + health + rage = 6 bars; one stunned marker; crosshair + weapon + shield icons
        assert_eq!(cmds.iter().filter(|c| matches!(c, DrawCmd::Bar { .. })).count(), 6);
        let icons = cmds.iter().filter(|c| matches!(c, DrawCmd::Icon { .. })).count();
        assert_eq!(icons, 4 + 0, "weapon, shield, crosshair, one stunned marker");
        let prompts: Vec<_> = cmds.iter().filter_map(|c| if let DrawCmd::Prompt { text: Some(t), .. } = c { Some(t.clone()) } else { None }).collect();
        assert!(prompts.contains(&"Recall".to_string()) && prompts.contains(&"Finish".to_string()));
    }

    #[test]
    fn boss_bar_replaces_floating_bar_and_subtitles_appear() {
        let d = Data::get();
        let mut s = base_state();
        s.in_combat = true;
        s.boss = Some(EnemyBar { id: 9, pos: Vec3::new(0.0, 0.0, 8.0), hp_frac: 0.6, stun_frac: 0.2, stunned: false, boss: true, status_frac: 0.0, status_color: 0, status_label: None });
        s.enemies = vec![s.boss.clone().unwrap()];
        s.subtitle = Some("Boy, mind the left.".into());
        s.mimir_speaking = true;
        let cmds = draw_list(d, &s, Screen { w: 1920.0, h: 1080.0 }, &project);
        // health, rage, boss hp, boss stun; no floating bars for a boss
        assert_eq!(cmds.iter().filter(|c| matches!(c, DrawCmd::Bar { .. })).count(), 4);
        assert!(cmds.iter().any(|c| matches!(c, DrawCmd::Text { text, .. } if text == "Boy, mind the left.")));
    }

    #[test]
    fn rage_and_low_health_overlays_are_scaled() {
        let d = Data::get();
        let mut s = base_state();
        s.rage_active = true;
        s.rage_overlay = 1.0;
        s.low_health_overlay = 0.8;
        let cmds = draw_list(d, &s, Screen { w: 1920.0, h: 1080.0 }, &project);
        let alphas: Vec<f32> = cmds.iter().filter_map(|c| if let DrawCmd::Overlay { alpha, .. } = c { Some(*alpha) } else { None }).collect();
        assert_eq!(alphas.len(), 2);
        assert!(alphas.contains(&d.rage.tint_strength));
    }

    #[test]
    fn every_hud_row_is_laid_out_somewhere_in_a_busy_scene() {
        // All `bind`s that exist in the sheet must be reachable by some state.
        let d = Data::get();
        let mut s = base_state();
        s.aiming = true;
        s.recall_available = true;
        s.recall_label = "Recall";
        s.in_combat = true;
        s.rage_ready = true;
        s.rage_active = true;
        s.rage_overlay = 1.0;
        s.low_health_overlay = 1.0;
        s.mimir_speaking = true;
        s.subtitle = Some("x".into());
        s.finisher_target = Some(1);
        let boss = EnemyBar { id: 9, pos: Vec3::new(0.0, 0.0, 8.0), hp_frac: 0.6, stun_frac: 0.2, stunned: false, boss: true, status_frac: 0.0, status_color: 0, status_label: None };
        s.boss = Some(boss);
        s.enemies = vec![EnemyBar { id: 1, pos: Vec3::new(1.0, 0.0, 4.0), hp_frac: 0.5, stun_frac: 1.0, stunned: true, boss: false, status_frac: 0.0, status_color: 0, status_label: None }];
        let cmds = draw_list(d, &s, Screen { w: 1920.0, h: 1080.0 }, &project);
        // 18 hud rows -> each yields at least one command (world rows yield per enemy)
        assert!(cmds.len() >= HUD.len(), "{} cmds for {} rows", cmds.len(), HUD.len());
    }
}
