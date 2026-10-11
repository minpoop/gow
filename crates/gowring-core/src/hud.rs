//! The HUD as data. `HudState` holds what to show; `draw_list` lays it out from `hud.json` and
//! returns plain draw commands the renderer (ImGui in the DLL, a rasteriser in tests) executes.

use crate::data::Data;
use crate::sheets::*;
use crate::types::*;

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

#[derive(Clone, Debug)]
pub struct RunicHud {
    pub id: &'static str,
    pub name: &'static str,
    /// 1.0 = just used … 0.0 = ready.
    pub remaining_frac: f32,
    pub ready: bool,
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
    Bar { rect: Rect, frac: f32, trail: f32, frame: Option<&'static str>, fill: Option<&'static str>, color: u32, alpha: f32 },
    Ring { cx: f32, cy: f32, r: f32, remaining: f32, ready: bool, frame: Option<&'static str>, glow: Option<&'static str>, icon: Option<&'static str>, color: u32, alpha: f32 },
    Icon { rect: Rect, asset: Option<&'static str>, color: u32, alpha: f32 },
    Text { rect: Rect, px: f32, text: String, color: u32, alpha: f32, align: Align, plate: Option<&'static str>, portrait: Option<&'static str> },
    Prompt { rect: Rect, text: Option<String>, glyph: Option<&'static str>, backing: Option<&'static str>, color: u32, alpha: f32 },
    Overlay { color: u32, alpha: f32 },
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
        match h.bind {
            HudBind::PlayerHp => out.push(DrawCmd::Bar {
                rect: rect_at(h),
                frac: s.hp_frac,
                trail: s.hp_trail_frac,
                frame: h.frame,
                fill: h.fill,
                color,
                alpha: 1.0,
            }),
            HudBind::PlayerRage => out.push(DrawCmd::Bar {
                rect: rect_at(h),
                frac: s.rage_frac,
                trail: s.rage_frac,
                frame: h.frame,
                fill: h.fill,
                color,
                alpha: if s.rage_ready || s.rage_active { pulse_alpha } else { 1.0 },
            }),
            HudBind::RageReady => out.push(DrawCmd::Prompt {
                rect: rect_at(h),
                text: Some("Spartan Rage".into()),
                glyph: h.glyph,
                backing: h.frame,
                color,
                alpha: pulse_alpha,
            }),
            HudBind::RunicLightCd | HudBind::RunicHeavyCd => {
                let r = if h.bind == HudBind::RunicLightCd { &s.runic_light } else { &s.runic_heavy };
                let Some(r) = r else { continue };
                let rc = rect_at(h);
                out.push(DrawCmd::Ring {
                    cx: rc.x + rc.w / 2.0,
                    cy: rc.y + rc.h / 2.0,
                    r: rc.w.min(rc.h) / 2.0,
                    remaining: r.remaining_frac,
                    ready: r.ready,
                    frame: h.frame,
                    glow: h.fill,
                    icon: s.weapon_icon,
                    color,
                    alpha: if r.ready { pulse_alpha } else { 0.9 },
                });
            }
            HudBind::WeaponCurrent => out.push(DrawCmd::Icon { rect: rect_at(h), asset: s.weapon_icon, color, alpha: 1.0 }),
            HudBind::ShieldState => out.push(DrawCmd::Icon { rect: rect_at(h), asset: h.glyph, color, alpha: if s.shield_drawn { 1.0 } else { 0.35 } }),
            HudBind::ThrowAim => out.push(DrawCmd::Icon { rect: rect_at(h), asset: h.glyph, color, alpha: 0.95 }),
            HudBind::RecallState => out.push(DrawCmd::Prompt { rect: rect_at(h), text: Some(s.recall_label.to_string()), glyph: h.glyph, backing: None, color, alpha: 1.0 }),
            HudBind::Finisher => out.push(DrawCmd::Prompt { rect: rect_at(h), text: Some("Finish".into()), glyph: h.glyph, backing: h.frame, color, alpha: pulse_alpha }),
            HudBind::EnemyHp | HudBind::EnemyStun | HudBind::EnemyStunned => {
                for e in s.enemies.iter().filter(|e| !e.boss) {
                    let Some((px, py)) = project(e.pos + Vec3::new(0.0, 2.3, 0.0)) else { continue };
                    let (w, hh) = (h.w as f32 * k * us, h.h as f32 * k * us);
                    let rc = Rect { x: px - w / 2.0 + h.x as f32 * k * 0.0, y: py + h.y as f32 * k, w, h: hh };
                    match h.bind {
                        HudBind::EnemyHp => out.push(DrawCmd::Bar { rect: rc, frac: e.hp_frac, trail: e.hp_frac, frame: h.frame, fill: h.fill, color, alpha: 0.95 }),
                        HudBind::EnemyStun => {
                            out.push(DrawCmd::Bar { rect: rc, frac: e.stun_frac, trail: e.stun_frac, frame: h.frame, fill: h.fill, color, alpha: 0.95 });
                            if e.status_frac > 0.01 && e.status_color != 0 {
                                let thin = Rect { y: rc.y + rc.h + 2.0 * k * us, h: (rc.h * 0.5).max(2.0), ..rc };
                                out.push(DrawCmd::Bar { rect: thin, frac: e.status_frac, trail: e.status_frac, frame: None, fill: None, color: e.status_color, alpha: 0.95 });
                            }
                            if let Some(label) = e.status_label {
                                let tag = Rect { y: rc.y + rc.h * 2.0 + 6.0 * k * us, h: 16.0 * k * us, ..rc };
                                let tint = if label == "FROZEN" { 0x7FD8FF } else { 0xFF8A3A };
                                out.push(DrawCmd::Text { rect: tag, px: 14.0 * k * us, text: label.to_string(), color: tint, alpha: 1.0, align: Align::Center, plate: None, portrait: None });
                            }
                        }
                        _ if e.stunned => out.push(DrawCmd::Icon { rect: rc, asset: h.glyph, color, alpha: pulse_alpha }),
                        _ => {}
                    }
                }
            }
            HudBind::BossHp => {
                if let Some(b) = &s.boss {
                    out.push(DrawCmd::Bar { rect: rect_at(h), frac: b.hp_frac, trail: b.hp_frac, frame: h.frame, fill: h.fill, color, alpha: 1.0 });
                }
            }
            HudBind::BossStun => {
                if let Some(b) = &s.boss {
                    out.push(DrawCmd::Bar { rect: rect_at(h), frac: b.stun_frac, trail: b.stun_frac, frame: h.frame, fill: h.fill, color, alpha: 1.0 });
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
    out
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
            runic_light: Some(RunicHud { id: "axe_runic_light", name: "Frost Shockwave", remaining_frac: 0.5, ready: false }),
            runic_heavy: Some(RunicHud { id: "axe_runic_heavy", name: "Glacial Cleave", remaining_frac: 0.0, ready: true }),
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
        assert_eq!((rect.x, rect.y, rect.w, rect.h), (96.0, 64.0, 560.0, 26.0));
        assert_eq!(*frac, 0.8);
    }

    #[test]
    fn ui_scale_grows_elements_about_their_own_corner() {
        let d = Data::get();
        let mut s = base_state();
        s.ui_scale = 2.0;
        let cmds = draw_list(d, &s, Screen { w: 1920.0, h: 1080.0 }, &project);
        let DrawCmd::Bar { rect, .. } = &cmds[0] else { panic!() };
        // top-left anchored: position and size both double
        assert_eq!((rect.x, rect.y, rect.w, rect.h), (192.0, 128.0, 1120.0, 52.0));
        // bottom-left anchored weapon icon keeps its distance from the bottom-left corner times the scale
        let icon = cmds.iter().find_map(|c| if let DrawCmd::Icon { rect, .. } = c { Some(*rect) } else { None }).unwrap();
        assert!((icon.x - 192.0).abs() < 1e-3 && ((1080.0 - (icon.y + icon.h)) - 192.0).abs() < 1e-3, "{icon:?}");
        assert!((icon.w - 168.0).abs() < 1e-3);
    }

    #[test]
    fn layout_scales_and_letterboxes() {
        let d = Data::get();
        let cmds = draw_list(d, &base_state(), Screen { w: 2560.0, h: 1080.0 }, &project);
        let DrawCmd::Bar { rect, .. } = &cmds[0] else { panic!() };
        // 21:9: scale 1.0, 320 px of side bars
        assert!((rect.x - (320.0 + 96.0)).abs() < 1e-3);
        let cmds = draw_list(d, &base_state(), Screen { w: 3840.0, h: 2160.0 }, &project);
        let DrawCmd::Bar { rect, .. } = &cmds[0] else { panic!() };
        assert!((rect.x - 192.0).abs() < 1e-3 && (rect.w - 1120.0).abs() < 1e-3);
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
        assert!(prompts.contains(&"Recall".to_string()) && prompts.contains(&"Finish".to_string()) && prompts.contains(&"Spartan Rage".to_string()));
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
