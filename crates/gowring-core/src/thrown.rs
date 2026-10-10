//! The Leviathan Axe (throw, stick, recall) and Draupnir Spear (throw up to three, detonate).
//! Both are simulated here; the host only draws them and turns `Damage` commands into hits.

use crate::data::Data;
use crate::engine::{Event, Gains};
use crate::input::Inputs;
use crate::sheets::{ProjectileDef, WeaponDef, WeaponKind, PROJECTILES};
use crate::types::*;
use std::collections::HashSet;

pub const TAG_AXE: u32 = 1;
pub const TAG_SPEAR_BASE: u32 = 100;

#[derive(Clone, Debug)]
pub enum AxeState {
    InHand,
    Flying { pos: Vec3, dir: Vec3, traveled: f32 },
    Stuck { pos: Vec3, on: Option<EnemyId> },
    Returning { pos: Vec3, speed: f32, hit: HashSet<EnemyId> },
}

#[derive(Clone, Debug)]
pub enum SpearState {
    Flying { pos: Vec3, dir: Vec3, traveled: f32 },
    Stuck { pos: Vec3, on: Option<EnemyId> },
}

#[derive(Clone, Debug)]
pub struct Spear {
    pub id: u32,
    pub state: SpearState,
}

pub struct Thrown {
    d: &'static Data,
    axe_def: &'static ProjectileDef,
    spear_def: &'static ProjectileDef,
    pub axe: AxeState,
    pub spears: Vec<Spear>,
    next_spear: u32,
    spear_ready_at: f64,
    pub aiming: bool,
}

/// First enemy a segment passes through (smallest parameter), as (index, t in 0..=1).
fn segment_hit(a: Vec3, b: Vec3, enemies: &[EnemyView], radius: f32, skip: &HashSet<EnemyId>) -> Option<(usize, f32)> {
    let ab = b - a;
    let len2 = ab.dot(ab).max(1e-9);
    let mut best: Option<(usize, f32)> = None;
    for (i, e) in enemies.iter().enumerate() {
        if !e.alive || skip.contains(&e.id) {
            continue;
        }
        let centre = e.pos + Vec3::new(0.0, 1.0, 0.0);
        let t = ((centre - a).dot(ab) / len2).clamp(0.0, 1.0);
        let p = a + ab * t;
        if p.dist(centre) <= radius + e.radius + 0.4 && best.map(|(_, bt)| t < bt).unwrap_or(true) {
            best = Some((i, t));
        }
    }
    best
}

impl Thrown {
    pub fn new(d: &'static Data) -> Self {
        let find = |w: &str| PROJECTILES.iter().find(|p| p.weapon == w).expect("projectile row");
        Thrown {
            d,
            axe_def: find("leviathan_axe"),
            spear_def: find("draupnir_spear"),
            axe: AxeState::InHand,
            spears: Vec::new(),
            next_spear: 0,
            spear_ready_at: 0.0,
            aiming: false,
        }
    }

    pub fn axe_in_hand(&self) -> bool {
        matches!(self.axe, AxeState::InHand)
    }
    pub fn axe_out(&self) -> bool {
        !self.axe_in_hand()
    }
    pub fn stuck_spears(&self) -> usize {
        self.spears.iter().filter(|s| matches!(s.state, SpearState::Stuck { .. })).count()
    }
    pub fn recall_available(&self, weapon: &WeaponDef) -> bool {
        match weapon.kind {
            WeaponKind::Axe => self.axe_out(),
            WeaponKind::Spear => self.stuck_spears() > 0,
            _ => false,
        }
    }
    pub fn reset(&mut self) {
        self.axe = AxeState::InHand;
        self.spears.clear();
    }

    /// Where the thrown weapons are, for the host to draw.
    pub fn positions(&self) -> Vec<(&'static str, Vec3)> {
        let mut v = Vec::new();
        match &self.axe {
            AxeState::InHand => {}
            AxeState::Flying { pos, .. } | AxeState::Stuck { pos, .. } | AxeState::Returning { pos, .. } => v.push(("leviathan_axe", *pos)),
        }
        for s in &self.spears {
            match &s.state {
                SpearState::Flying { pos, .. } | SpearState::Stuck { pos, .. } => v.push(("draupnir_spear", *pos)),
            }
        }
        v
    }

    fn hand(&self, f: &Frame, def: &ProjectileDef) -> Vec3 {
        f.player.pos + Vec3::new(0.0, def.hand_height_m, 0.0)
    }

    /// Throw direction with a little aim assist toward the enemy nearest the crosshair.
    fn throw_dir(&self, f: &Frame, def: &ProjectileDef, start: Vec3) -> Vec3 {
        let aim = f.aim_dir.normalized();
        let aim = if aim == Vec3::ZERO { f.player.facing } else { aim };
        let best = f
            .enemies
            .iter()
            .filter(|e| e.alive && e.pos.dist(start) <= def.max_range_m)
            .map(|e| {
                let chest = e.pos + Vec3::new(0.0, 1.2, 0.0);
                (aim.angle_deg(chest - f.camera_pos.max_or(start)), chest)
            })
            .filter(|(a, _)| *a <= def.aim_assist_deg)
            .min_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
        match best {
            Some((_, chest)) => (chest - start).normalized(),
            None => aim,
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub fn update(
        &mut self,
        f: &Frame,
        inp: &Inputs,
        weapon: &'static WeaponDef,
        now: f64,
        dmg_mult: f32,
        stun_mult: f32,
        out: &mut Vec<Command>,
        ev: &mut Vec<Event>,
        gains: &mut Gains,
    ) {
        let d = self.d;
        let can_throw_weapon = matches!(weapon.kind, WeaponKind::Axe | WeaponKind::Spear);
        self.aiming = inp.aim_held && can_throw_weapon && f.player.controllable;

        // --- swapping away from the axe calls it back --------------------------------------
        if weapon.kind != WeaponKind::Axe && matches!(self.axe, AxeState::Stuck { .. } | AxeState::Flying { .. }) {
            self.start_return();
        }

        // --- ray results from last frame ------------------------------------------------------
        for r in &f.rays {
            if r.tag == TAG_AXE {
                if let (AxeState::Flying { .. }, Some(p)) = (&self.axe, r.hit) {
                    self.axe = AxeState::Stuck { pos: p, on: None };
                    out.push(Command::Fx { kind: self.axe_def.fx, pos: p, dir: Vec3::ZERO, radius: 1.5 });
                }
            } else if r.tag >= TAG_SPEAR_BASE {
                let id = r.tag - TAG_SPEAR_BASE;
                if let (Some(s), Some(p)) = (self.spears.iter_mut().find(|s| s.id == id), r.hit) {
                    if matches!(s.state, SpearState::Flying { .. }) {
                        s.state = SpearState::Stuck { pos: p, on: None };
                    }
                }
            }
        }

        // --- axe ------------------------------------------------------------------------------------
        if weapon.kind == WeaponKind::Axe {
            if self.aiming && inp.throw_pressed && self.axe_in_hand() {
                let def = self.axe_def;
                let start = self.hand(f, def);
                let dir = self.throw_dir(f, def, start);
                self.axe = AxeState::Flying { pos: start + dir * 0.8, dir, traveled: 0.0 };
                out.push(Command::Sfx(d.mv(def.throw_move).sfx.unwrap_or("sfx_axe_throw")));
                ev.push(Event::AxeThrown);
            } else if inp.recall_pressed && self.axe_out() {
                self.start_return();
            }
        }
        self.step_axe(f, now, dmg_mult, stun_mult, out, ev, gains);

        // --- spears ---------------------------------------------------------------------------------
        if weapon.kind == WeaponKind::Spear {
            let def = self.spear_def;
            if self.aiming && inp.throw_pressed && self.spears.len() < def.max_out as usize && now >= self.spear_ready_at {
                let start = self.hand(f, def);
                let dir = self.throw_dir(f, def, start);
                self.next_spear += 1;
                self.spears.push(Spear { id: self.next_spear, state: SpearState::Flying { pos: start + dir * 0.8, dir, traveled: 0.0 } });
                out.push(Command::Sfx(d.mv(def.throw_move).sfx.unwrap_or("sfx_spear_throw")));
                ev.push(Event::SpearThrown);
            } else if inp.recall_pressed && self.stuck_spears() > 0 {
                self.detonate(f, now, dmg_mult, stun_mult, out, ev, gains);
            }
        }
        self.step_spears(f, now, dmg_mult, stun_mult, out, ev, gains);
    }

    fn start_return(&mut self) {
        let pos = match &self.axe {
            AxeState::Flying { pos, .. } | AxeState::Stuck { pos, .. } => *pos,
            _ => return,
        };
        self.axe = AxeState::Returning { pos, speed: self.axe_def.return_speed_mps * 0.5, hit: HashSet::new() };
    }

    #[allow(clippy::too_many_arguments)]
    fn step_axe(&mut self, f: &Frame, now: f64, dmg_mult: f32, stun_mult: f32, out: &mut Vec<Command>, ev: &mut Vec<Event>, gains: &mut Gains) {
        let def = self.axe_def;
        let d = self.d;
        let w = d.weapon(def.weapon);
        let state = std::mem::replace(&mut self.axe, AxeState::InHand);
        self.axe = match state {
            AxeState::InHand => AxeState::InHand,
            AxeState::Flying { pos, dir, traveled } => {
                let step = def.speed_mps * f.dt;
                let next = pos + dir * step;
                if let Some((i, t)) = segment_hit(pos, next, &f.enemies, def.hit_radius_m, &HashSet::new()) {
                    let e = &f.enemies[i];
                    let mv = d.mv(def.throw_move);
                    out.push(Command::Damage { target: e.id, amount: w.base_damage as f32 * mv.motion_value * dmg_mult, source: DamageSource::Thrown });
                    gains.stun.push((e.id, mv.stun * w.stun_rate * stun_mult));
                    gains.rage += mv.rage_gain;
                    gains.player_hit.push(e.id);
                    out.push(Command::Fx { kind: def.fx, pos: pos + dir * (step * t), dir, radius: 1.5 });
                    AxeState::Stuck { pos: pos + dir * (step * t), on: Some(e.id) }
                } else if traveled + step >= def.max_range_m {
                    AxeState::Stuck { pos: next, on: None }
                } else {
                    out.push(Command::Raycast { tag: TAG_AXE, from: pos, to: next });
                    AxeState::Flying { pos: next, dir, traveled: traveled + step }
                }
            }
            AxeState::Stuck { pos, on } => {
                // ride along with the enemy it is stuck in
                let pos = on.and_then(|id| f.enemies.iter().find(|e| e.id == id && e.alive)).map(|e| e.pos + Vec3::new(0.0, 1.2, 0.0)).unwrap_or(pos);
                if pos.dist(f.player.pos) > def.auto_recall_dist_m {
                    AxeState::Returning { pos, speed: def.return_speed_mps * 0.5, hit: HashSet::new() }
                } else {
                    AxeState::Stuck { pos, on }
                }
            }
            AxeState::Returning { pos, speed, mut hit } => {
                let target = self.hand(f, def);
                let speed = (speed + def.return_accel_mps2 * f.dt).min(def.return_speed_mps.max(1.0));
                let to = target - pos;
                let dist = to.len();
                let step = (speed * f.dt).min(dist);
                let next = if dist < 1e-4 { target } else { pos + to * (step / dist) };
                let mv = d.mv(def.recall_move);
                // hits everything on the way back, once each
                while let Some((i, _)) = segment_hit(pos, next, &f.enemies, def.hit_radius_m, &hit) {
                    let e = &f.enemies[i];
                    hit.insert(e.id);
                    out.push(Command::Damage { target: e.id, amount: w.base_damage as f32 * mv.motion_value * dmg_mult, source: DamageSource::Thrown });
                    gains.stun.push((e.id, mv.stun * w.stun_rate * stun_mult));
                    gains.rage += mv.rage_gain;
                    gains.player_hit.push(e.id);
                }
                if next.dist(target) <= def.catch_dist_m {
                    if let Some(s) = mv.sfx_end {
                        out.push(Command::Sfx(s));
                    }
                    ev.push(Event::AxeCaught);
                    AxeState::InHand
                } else {
                    AxeState::Returning { pos: next, speed, hit }
                }
            }
        };
        let _ = now;
    }

    #[allow(clippy::too_many_arguments)]
    fn step_spears(&mut self, f: &Frame, now: f64, dmg_mult: f32, stun_mult: f32, out: &mut Vec<Command>, _ev: &mut Vec<Event>, gains: &mut Gains) {
        let def = self.spear_def;
        let d = self.d;
        let w = d.weapon(def.weapon);
        let mv = d.mv(def.throw_move);
        for s in &mut self.spears {
            s.state = match std::mem::replace(&mut s.state, SpearState::Stuck { pos: Vec3::ZERO, on: None }) {
                SpearState::Flying { pos, dir, traveled } => {
                    let step = def.speed_mps * f.dt;
                    let next = pos + dir * step;
                    if let Some((i, t)) = segment_hit(pos, next, &f.enemies, def.hit_radius_m, &HashSet::new()) {
                        let e = &f.enemies[i];
                        out.push(Command::Damage { target: e.id, amount: w.base_damage as f32 * mv.motion_value * dmg_mult, source: DamageSource::Thrown });
                        gains.stun.push((e.id, mv.stun * w.stun_rate * stun_mult));
                        gains.rage += mv.rage_gain;
                        gains.player_hit.push(e.id);
                        SpearState::Stuck { pos: pos + dir * (step * t), on: Some(e.id) }
                    } else if traveled + step >= def.max_range_m {
                        SpearState::Stuck { pos: next, on: None }
                    } else {
                        out.push(Command::Raycast { tag: TAG_SPEAR_BASE + s.id, from: pos, to: next });
                        SpearState::Flying { pos: next, dir, traveled: traveled + step }
                    }
                }
                SpearState::Stuck { pos, on } => {
                    let pos = on.and_then(|id| f.enemies.iter().find(|e| e.id == id && e.alive)).map(|e| e.pos + Vec3::new(0.0, 1.2, 0.0)).unwrap_or(pos);
                    SpearState::Stuck { pos, on }
                }
            };
        }
        let _ = now;
    }

    #[allow(clippy::too_many_arguments)]
    fn detonate(&mut self, f: &Frame, now: f64, dmg_mult: f32, stun_mult: f32, out: &mut Vec<Command>, ev: &mut Vec<Event>, gains: &mut Gains) {
        let def = self.spear_def;
        let d = self.d;
        let w = d.weapon(def.weapon);
        let mv = d.mv(def.recall_move);
        let mut total_hit = 0;
        let (stuck, flying): (Vec<_>, Vec<_>) = self.spears.drain(..).partition(|s| matches!(s.state, SpearState::Stuck { .. }));
        for s in &stuck {
            let SpearState::Stuck { pos, .. } = s.state else { continue };
            for e in f.enemies.iter().filter(|e| e.alive && e.pos.dist(pos) <= def.blast_radius_m + e.radius) {
                out.push(Command::Damage { target: e.id, amount: w.base_damage as f32 * mv.motion_value * dmg_mult, source: DamageSource::Blast });
                gains.stun.push((e.id, mv.stun * w.stun_rate * stun_mult));
                gains.player_hit.push(e.id);
                total_hit += 1;
            }
            out.push(Command::Fx { kind: def.fx, pos, dir: Vec3::ZERO, radius: def.blast_radius_m });
        }
        if let Some(sfx) = mv.sfx {
            out.push(Command::Sfx(sfx));
        }
        gains.rage += mv.rage_gain * stuck.len() as f32;
        out.push(Command::Shake(0.5));
        self.spears = flying;
        self.spear_ready_at = now + def.reready_s as f64;
        ev.push(Event::SpearsDetonated { spears: stuck.len(), hit: total_hit });
    }
}

trait MaxOr {
    fn max_or(self, fallback: Vec3) -> Vec3;
}
impl MaxOr for Vec3 {
    /// Camera position if the host provided one (non-zero), else the throw start.
    fn max_or(self, fallback: Vec3) -> Vec3 {
        if self == Vec3::ZERO { fallback } else { self }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sheets::ErAction;

    fn enemy(id: EnemyId, x: f32, z: f32) -> EnemyView {
        EnemyView { id, pos: Vec3::new(x, 0.0, z), hp: 5000.0, max_hp: 5000.0, alive: true, boss: false, engaged: true, radius: 0.6, staggered: false }
    }

    fn aim_frame(enemies: Vec<EnemyView>) -> Frame {
        let mut f = Frame { dt: 1.0 / 60.0, aim_dir: Vec3::new(0.0, 0.0, 1.0), enemies, ..Default::default() };
        f.player.facing = Vec3::new(0.0, 0.0, 1.0);
        f
    }

    fn inputs_throw() -> Inputs {
        Inputs { aim_held: true, throw_pressed: true, ..Default::default() }
    }

    fn run_frames(t: &mut Thrown, f: &Frame, inp: &Inputs, w: &'static WeaponDef, n: usize, now0: f64) -> (Vec<Command>, Vec<Event>, Gains) {
        let (mut out, mut ev, mut g) = (vec![], vec![], Gains::default());
        for i in 0..n {
            t.update(f, inp, w, now0 + i as f64 / 60.0, 1.0, 1.0, &mut out, &mut ev, &mut g);
        }
        (out, ev, g)
    }

    #[test]
    fn axe_hits_sticks_then_recall_returns_and_hits_on_the_way_back() {
        let d = Data::get();
        let w = d.weapon("leviathan_axe");
        let mut t = Thrown::new(d);
        let mut f = aim_frame(vec![enemy(1, 0.0, 10.0)]);
        // throw
        let (_, ev, _) = run_frames(&mut t, &f, &inputs_throw(), w, 1, 0.0);
        assert!(ev.iter().any(|e| matches!(e, Event::AxeThrown)));
        assert!(!t.axe_in_hand());
        // flies into the enemy
        let idle = Inputs { aim_held: true, ..Default::default() };
        let (out, _, g) = run_frames(&mut t, &f, &idle, w, 40, 0.02);
        let dmg: Vec<_> = out.iter().filter_map(|c| if let Command::Damage { amount, source: DamageSource::Thrown, .. } = c { Some(*amount) } else { None }).collect();
        assert_eq!(dmg.len(), 1, "one thrown hit");
        assert!((dmg[0] - 420.0 * 1.6).abs() < 0.01);
        assert!(g.stun.len() == 1 && g.rage > 0.0);
        assert!(matches!(t.axe, AxeState::Stuck { on: Some(1), .. }));
        // cannot throw again while it is out
        let (_, ev, _) = run_frames(&mut t, &f, &inputs_throw(), w, 1, 1.0);
        assert!(!ev.iter().any(|e| matches!(e, Event::AxeThrown)));
        // enemy dies: the axe stays where it was; recall brings it home hitting a second enemy
        f.enemies = vec![enemy(2, 0.0, 6.0)];
        let recall = Inputs { recall_pressed: true, ..Default::default() };
        let (out, ev, _) = run_frames(&mut t, &f, &recall, w, 1, 2.0);
        let _ = (out, ev);
        let (out, ev, _) = run_frames(&mut t, &f, &Inputs::default(), w, 240, 2.1);
        assert!(t.axe_in_hand(), "caught: {:?}", t.axe);
        assert!(ev.iter().any(|e| matches!(e, Event::AxeCaught)));
        let back: Vec<_> = out.iter().filter(|c| matches!(c, Command::Damage { target: 2, .. })).collect();
        assert_eq!(back.len(), 1, "return pass hits an enemy once");
        assert!(out.iter().any(|c| matches!(c, Command::Sfx("sfx_axe_catch"))));
    }

    #[test]
    fn world_hit_sticks_the_axe_via_raycast_result() {
        let d = Data::get();
        let w = d.weapon("leviathan_axe");
        let mut t = Thrown::new(d);
        let mut f = aim_frame(vec![]);
        run_frames(&mut t, &f, &inputs_throw(), w, 1, 0.0);
        let idle = Inputs { aim_held: true, ..Default::default() };
        let (out, _, _) = run_frames(&mut t, &f, &idle, w, 1, 0.02);
        let tag = out.iter().find_map(|c| if let Command::Raycast { tag, .. } = c { Some(*tag) } else { None });
        assert_eq!(tag, Some(TAG_AXE));
        f.rays = vec![RayResult { tag: TAG_AXE, hit: Some(Vec3::new(0.0, 1.0, 5.0)) }];
        run_frames(&mut t, &f, &idle, w, 1, 0.04);
        match t.axe {
            AxeState::Stuck { pos, on: None } => assert!((pos.z - 5.0).abs() < 1e-3),
            ref s => panic!("{s:?}"),
        }
    }

    #[test]
    fn swapping_away_recalls_the_axe() {
        let d = Data::get();
        let axe = d.weapon("leviathan_axe");
        let blades = d.weapon("blades_of_chaos");
        let mut t = Thrown::new(d);
        let f = aim_frame(vec![]);
        run_frames(&mut t, &f, &inputs_throw(), axe, 1, 0.0);
        run_frames(&mut t, &f, &Inputs::default(), axe, 30, 0.02);
        assert!(!t.axe_in_hand());
        run_frames(&mut t, &f, &Inputs::default(), blades, 400, 1.0);
        assert!(t.axe_in_hand());
    }

    #[test]
    fn aim_assist_pulls_toward_a_near_crosshair_target() {
        let d = Data::get();
        let w = d.weapon("leviathan_axe");
        let mut t = Thrown::new(d);
        // enemy 4 degrees off the aim line, 12 m out
        let ang = 4.0f32.to_radians();
        let f = aim_frame(vec![enemy(1, 12.0 * ang.sin(), 12.0 * ang.cos())]);
        run_frames(&mut t, &f, &inputs_throw(), w, 1, 0.0);
        let AxeState::Flying { dir, .. } = t.axe else { panic!() };
        assert!(dir.x > 0.02, "pulled toward the enemy: {dir:?}");
    }

    #[test]
    fn spears_stack_to_three_and_detonate_together() {
        let d = Data::get();
        let w = d.weapon("draupnir_spear");
        let mut t = Thrown::new(d);
        let f = aim_frame(vec![enemy(1, 0.0, 8.0)]);
        let mut now = 0.0;
        for _ in 0..5 {
            run_frames(&mut t, &f, &inputs_throw(), w, 1, now);
            now += 1.0;
        }
        assert_eq!(t.spears.len(), 3, "max_out");
        // let them fly into the enemy
        let (out, _, _) = run_frames(&mut t, &f, &Inputs { aim_held: true, ..Default::default() }, w, 60, now);
        let _ = out;
        assert!(t.stuck_spears() >= 1);
        let n = t.stuck_spears();
        let (out, ev, _) = run_frames(&mut t, &f, &Inputs { recall_pressed: true, ..Default::default() }, w, 1, now + 2.0);
        let blasts = out.iter().filter(|c| matches!(c, Command::Damage { source: DamageSource::Blast, .. })).count();
        assert_eq!(blasts, n);
        assert!(ev.iter().any(|e| matches!(e, Event::SpearsDetonated { .. })));
        assert_eq!(t.stuck_spears(), 0);
        // re-ready delay blocks an immediate rethrow
        let (_, ev, _) = run_frames(&mut t, &f, &inputs_throw(), w, 1, now + 2.1);
        assert!(!ev.iter().any(|e| matches!(e, Event::SpearThrown)));
        let (_, ev, _) = run_frames(&mut t, &f, &inputs_throw(), w, 1, now + 4.0);
        assert!(ev.iter().any(|e| matches!(e, Event::SpearThrown)));
        let _ = ErAction::R1;
    }
}
