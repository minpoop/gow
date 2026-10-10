//! Light and heavy runic attacks: cooldowns, wind-up and area resolution.

use crate::data::Data;
use crate::engine::Event;
use crate::sheets::{RunicDef, RunicShape, RunicStatus, RunicTier};
use crate::types::*;
use std::collections::HashMap;

struct Pending {
    runic: &'static RunicDef,
    fire_at: f64,
}

pub struct Runics {
    d: &'static Data,
    cd: HashMap<&'static str, f32>,
    pending: Vec<Pending>,
}

/// Lateral half-width of a `line` runic.
const LINE_HALF_WIDTH: f32 = 1.6;
/// Half-angle of a `cone` runic.
const CONE_HALF_ANGLE: f32 = 45.0;
/// Radius around the chosen target of a `column` runic.
const COLUMN_RADIUS: f32 = 4.5;

impl Runics {
    pub fn new(d: &'static Data) -> Self {
        Runics { d, cd: HashMap::new(), pending: Vec::new() }
    }

    pub fn remaining(&self, r: &RunicDef) -> f32 {
        self.cd.get(r.id).copied().unwrap_or(0.0)
    }

    pub fn ready(&self, r: &RunicDef) -> bool {
        self.remaining(r) <= 0.0 && !self.pending.iter().any(|p| p.runic.id == r.id)
    }

    /// 1.0 = just used, 0.0 = ready.
    pub fn cooldown_frac(&self, r: &RunicDef) -> f32 {
        (self.remaining(r) / r.cooldown_s).clamp(0.0, 1.0)
    }

    pub fn reset_all(&mut self) {
        self.cd.clear();
        self.pending.clear();
    }

    /// Start a runic (windup begins now, effect lands after `windup_s`).
    pub fn press(&mut self, tier: RunicTier, weapon_id: &str, now: f64, out: &mut Vec<Command>) -> Option<&'static RunicDef> {
        let w = self.d.weapon(weapon_id);
        let id = match tier {
            RunicTier::Light => w.light_runic,
            RunicTier::Heavy => w.heavy_runic,
        }?;
        let r = self.d.runic(id);
        if !self.ready(r) {
            return None;
        }
        self.cd.insert(r.id, r.cooldown_s);
        self.pending.push(Pending { runic: r, fire_at: now + r.windup_s as f64 });
        out.push(Command::Cast { runic: r.id });
        Some(r)
    }

    /// Tick cooldowns and resolve any runic whose windup has finished.
    pub fn update(&mut self, f: &Frame, now: f64, dmg_mult: f32, stun_mult: f32, out: &mut Vec<Command>, ev: &mut Vec<Event>) -> Vec<(EnemyId, f32)> {
        let mut ready_now = vec![];
        for (id, left) in self.cd.iter_mut() {
            if *left > 0.0 {
                *left -= f.dt;
                if *left <= 0.0 {
                    *left = 0.0;
                    ready_now.push(*id);
                }
            }
        }
        for id in ready_now {
            out.push(Command::Sfx(self.d.runic(id).sfx.unwrap_or("sfx_runic_ready")));
        }
        let mut stun_adds = Vec::new();
        let due: Vec<Pending> = {
            let (due, keep): (Vec<_>, Vec<_>) = self.pending.drain(..).partition(|p| p.fire_at <= now);
            self.pending = keep;
            due
        };
        for p in due {
            self.resolve(p.runic, f, dmg_mult, stun_mult, out, ev, &mut stun_adds);
        }
        stun_adds
    }

    fn resolve(
        &mut self,
        r: &'static RunicDef,
        f: &Frame,
        dmg_mult: f32,
        stun_mult: f32,
        out: &mut Vec<Command>,
        ev: &mut Vec<Event>,
        stun_adds: &mut Vec<(EnemyId, f32)>,
    ) {
        let w = self.d.weapon(r.weapon);
        let origin = f.player.pos;
        let dir = self.aim_dir(f, r);
        let targets = Self::select(r, f, origin, dir);
        let amount = w.base_damage as f32 * r.damage_mult * dmg_mult;
        for e in &targets {
            out.push(Command::Damage { target: e.id, amount, source: DamageSource::Runic });
            if r.status != RunicStatus::None {
                out.push(Command::Status { target: e.id, status: r.status, seconds: r.status_s });
            }
            stun_adds.push((e.id, r.stun * w.stun_rate * stun_mult));
        }
        out.push(Command::Fx { kind: r.fx, pos: origin, dir, radius: r.radius_m });
        if let Some(s) = r.sfx {
            out.push(Command::Sfx(s));
        }
        ev.push(Event::RunicUsed { runic: r, hit: targets.len(), damage: amount });
    }

    /// Face the nearest engaged enemy that is roughly in front, otherwise the character's facing.
    fn aim_dir(&self, f: &Frame, r: &RunicDef) -> Vec3 {
        let facing = f.player.facing.flat().normalized();
        let best = f
            .enemies
            .iter()
            .filter(|e| e.alive && e.pos.dist(f.player.pos) <= r.radius_m + 4.0)
            .filter(|e| facing.angle_deg((e.pos - f.player.pos).flat()) <= 35.0)
            .min_by(|a, b| a.pos.dist(f.player.pos).partial_cmp(&b.pos.dist(f.player.pos)).unwrap_or(std::cmp::Ordering::Equal));
        match best {
            Some(e) => (e.pos - f.player.pos).flat().normalized(),
            None => facing,
        }
    }

    fn select<'a>(r: &RunicDef, f: &'a Frame, origin: Vec3, dir: Vec3) -> Vec<&'a EnemyView> {
        let alive = f.enemies.iter().filter(|e| e.alive);
        match r.shape {
            RunicShape::Ring => alive.filter(|e| e.pos.dist(origin) <= r.radius_m + e.radius).collect(),
            RunicShape::Cone => alive
                .filter(|e| e.pos.dist(origin) <= r.radius_m + e.radius)
                .filter(|e| dir.angle_deg((e.pos - origin).flat()) <= CONE_HALF_ANGLE)
                .collect(),
            RunicShape::Line | RunicShape::Lunge => alive
                .filter(|e| {
                    let rel = (e.pos - origin).flat();
                    let along = rel.dot(dir);
                    let lateral = (rel - dir * along).len();
                    along >= -e.radius && along <= r.radius_m + e.radius && lateral <= LINE_HALF_WIDTH + e.radius
                })
                .collect(),
            RunicShape::Column => {
                // Centre on the nearest enemy in front (or a point ahead of Kratos).
                let centre = f
                    .enemies
                    .iter()
                    .filter(|e| e.alive && dir.angle_deg((e.pos - origin).flat()) <= 60.0 && e.pos.dist(origin) <= r.radius_m + 2.0)
                    .min_by(|a, b| a.pos.dist(origin).partial_cmp(&b.pos.dist(origin)).unwrap_or(std::cmp::Ordering::Equal))
                    .map(|e| e.pos)
                    .unwrap_or(origin + dir * (r.radius_m * 0.5));
                f.enemies.iter().filter(|e| e.alive && e.pos.dist(centre) <= COLUMN_RADIUS + e.radius).collect()
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn enemy(id: EnemyId, x: f32, z: f32) -> EnemyView {
        EnemyView { id, pos: Vec3::new(x, 0.0, z), hp: 900.0, max_hp: 900.0, alive: true, boss: false, engaged: true, radius: 0.6, staggered: false }
    }

    fn run(r: &mut Runics, f: &Frame, t0: f64, secs: f64) -> (Vec<Command>, Vec<Event>, Vec<(EnemyId, f32)>) {
        let (mut out, mut ev, mut stun) = (vec![], vec![], vec![]);
        let mut t = t0;
        while t < t0 + secs {
            let mut fr = f.clone();
            fr.dt = 1.0 / 60.0;
            stun.extend(r.update(&fr, t, 1.0, 1.0, &mut out, &mut ev));
            t += 1.0 / 60.0;
        }
        (out, ev, stun)
    }

    #[test]
    fn cone_hits_front_not_back_and_cools_down() {
        let d = Data::get();
        let mut r = Runics::new(d);
        let mut f = Frame::default();
        f.enemies = vec![enemy(1, 0.0, 3.0), enemy(2, 0.0, -3.0), enemy(3, 6.0, 3.0)];
        let mut out = vec![];
        let run1 = r.press(RunicTier::Light, "leviathan_axe", 0.0, &mut out).unwrap();
        assert_eq!(run1.id, "axe_runic_light");
        assert!(!r.ready(run1));
        // second press during cooldown does nothing
        assert!(r.press(RunicTier::Light, "leviathan_axe", 0.1, &mut out).is_none());
        let (cmds, ev, stun) = run(&mut r, &f, 0.0, 1.0);
        let hit: Vec<_> = cmds
            .iter()
            .filter_map(|c| if let Command::Damage { target, amount, source: DamageSource::Runic } = c { Some((*target, *amount)) } else { None })
            .collect();
        assert_eq!(hit.len(), 1, "{hit:?}");
        assert_eq!(hit[0].0, 1);
        assert!((hit[0].1 - 420.0 * 2.0).abs() < 1e-3);
        assert!(cmds.iter().any(|c| matches!(c, Command::Status { target: 1, status: RunicStatus::Frozen, .. })));
        assert!(ev.iter().any(|e| matches!(e, Event::RunicUsed { hit: 1, .. })));
        assert_eq!(stun.len(), 1);
        // cools down to ready, with a chime, after cooldown_s
        let (cmds, _, _) = run(&mut r, &Frame::default(), 1.0, 12.5);
        assert!(r.ready(run1));
        assert!(cmds.iter().any(|c| matches!(c, Command::Sfx("sfx_runic_ready"))));
    }

    #[test]
    fn ring_hits_all_around_and_line_is_narrow() {
        let d = Data::get();
        let mut f = Frame::default();
        f.enemies = vec![enemy(1, 4.0, 0.0), enemy(2, -4.0, 0.0), enemy(3, 0.0, 4.0), enemy(4, 0.0, 30.0)];
        let mut r = Runics::new(d);
        let mut out = vec![];
        r.press(RunicTier::Light, "blades_of_chaos", 0.0, &mut out).unwrap();
        let (cmds, ev, _) = run(&mut r, &f, 0.0, 0.5);
        let n = cmds.iter().filter(|c| matches!(c, Command::Damage { .. })).count();
        assert_eq!(n, 3); // radius 7
        assert!(ev.iter().any(|e| matches!(e, Event::RunicUsed { hit: 3, .. })));

        // heavy axe runic is a line: only the enemy straight ahead
        f.enemies = vec![enemy(1, 0.0, 6.0), enemy(2, 5.0, 6.0), enemy(3, 0.0, -6.0)];
        let mut r = Runics::new(d);
        r.press(RunicTier::Heavy, "leviathan_axe", 0.0, &mut out).unwrap();
        let (cmds, _, _) = run(&mut r, &f, 0.0, 1.0);
        let ids: Vec<_> = cmds.iter().filter_map(|c| if let Command::Damage { target, .. } = c { Some(*target) } else { None }).collect();
        assert_eq!(ids, vec![1]);
    }

    #[test]
    fn rage_multiplies_runic_damage() {
        let d = Data::get();
        let mut r = Runics::new(d);
        let mut f = Frame::default();
        f.enemies = vec![enemy(1, 0.0, 2.0)];
        let mut out = vec![];
        r.press(RunicTier::Light, "draupnir_spear", 0.0, &mut out).unwrap();
        let (mut cmds, mut ev) = (vec![], vec![]);
        let mut t = 0.0;
        while t < 0.6 {
            f.dt = 1.0 / 60.0;
            r.update(&f, t, 1.75, 2.0, &mut cmds, &mut ev);
            t += 1.0 / 60.0;
        }
        let amount = cmds.iter().find_map(|c| if let Command::Damage { amount, .. } = c { Some(*amount) } else { None }).unwrap();
        assert!((amount - 360.0 * 1.8 * 1.75).abs() < 0.01);
    }

    #[test]
    fn shield_has_no_runics() {
        let d = Data::get();
        let mut r = Runics::new(d);
        let mut out = vec![];
        assert!(r.press(RunicTier::Light, "guardian_shield", 0.0, &mut out).is_none());
    }
}
