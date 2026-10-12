//! Spartan Rage.

use crate::data::Data;
use crate::engine::Event;
use crate::sheets::MoveDef;
use crate::types::*;

pub struct Rage {
    pub meter: f32,
    pub active: bool,
    pub time_left: f32,
    /// Multiplier on every point of Rage earned (the player's `rage_gain_multiplier` setting).
    pub gain_mult: f32,
    d: &'static Data,
}

impl Rage {
    pub fn new(d: &'static Data) -> Self {
        Rage { meter: d.loadout.rage_start_value(), active: false, time_left: 0.0, gain_mult: 1.0, d }
    }

    pub fn max(&self) -> f32 {
        self.d.rage.meter_max
    }
    /// Fill the meter at once (the debug key); nothing while Rage is burning.
    pub fn fill(&mut self) {
        if !self.active {
            self.meter = self.max();
        }
    }

    pub fn ready(&self) -> bool {
        !self.active && self.meter >= self.max() - 1e-3
    }
    pub fn frac(&self) -> f32 {
        (self.meter / self.max()).clamp(0.0, 1.0)
    }
    pub fn damage_mult(&self) -> f32 {
        if self.active { self.d.rage.damage_mult } else { 1.0 }
    }
    pub fn taken_mult(&self) -> f32 {
        if self.active { self.d.rage.damage_taken_mult } else { 1.0 }
    }
    pub fn stun_mult(&self) -> f32 {
        if self.active { self.d.rage.stun_mult } else { 1.0 }
    }

    /// Meter gain; none while Rage is burning.
    pub fn gain(&mut self, v: f32) {
        if !self.active && v > 0.0 {
            self.meter = (self.meter + v * self.gain_mult).min(self.max());
        }
    }

    pub fn gain_from_hp_loss(&mut self, lost: f32, max_hp: f32) {
        if max_hp > 0.0 {
            self.gain(self.d.rage.gain_per_pct_hp_lost * (lost / max_hp * 100.0));
        }
    }

    pub fn activate(&mut self, f: &Frame, base_damage: f32, burst: &'static MoveDef, out: &mut Vec<Command>, ev: &mut Vec<Event>) {
        self.active = true;
        self.time_left = self.d.rage.duration_s;
        out.push(Command::Sfx(self.d.rage.sfx_on));
        out.push(Command::AnimSpeed(self.d.rage.anim_speed));
        out.push(Command::Shake(1.0));
        let r = self.d.rage;
        if r.er_anim > 0 {
            // Kratos roars as Rage takes hold
            out.push(Command::PlayAnim(r.er_anim));
        }
        if f.player.alive && r.activation_heal_pct > 0.0 {
            out.push(Command::Heal { amount: f.player.max_hp * r.activation_heal_pct / 100.0 });
        }
        // Burst: everything close is hurt and knocked down.
        let radius = r.burst_radius_m;
        for e in f.enemies.iter().filter(|e| e.alive && e.pos.dist(f.player.pos) <= radius + e.radius) {
            out.push(Command::Damage { target: e.id, amount: base_damage * burst.motion_value, source: DamageSource::RageBurst });
            if r.burst_stagger_s > 0.0 {
                out.push(Command::Stagger { target: e.id, seconds: r.burst_stagger_s });
            }
        }
        out.push(Command::Fx { kind: crate::sheets::FxKind::Blast, pos: f.player.pos, dir: f.player.facing, radius });
        ev.push(Event::RageOn);
    }

    /// A kill while raging buys more time (never past a full bar).
    pub fn extend(&mut self, seconds: f32) {
        if self.active {
            self.time_left = (self.time_left + seconds).min(self.d.rage.duration_s);
        }
    }

    pub fn update(&mut self, f: &Frame, out: &mut Vec<Command>, ev: &mut Vec<Event>) {
        if !self.active {
            return;
        }
        self.time_left -= f.dt;
        self.meter = self.max() * (self.time_left / self.d.rage.duration_s).clamp(0.0, 1.0);
        if f.player.alive {
            let heal = f.player.max_hp * self.d.rage.regen_pct_per_s / 100.0 * f.dt;
            if f.player.hp < f.player.max_hp && heal > 0.0 {
                out.push(Command::Heal { amount: heal });
            }
        }
        if self.time_left <= 0.0 || !f.player.alive {
            self.active = false;
            self.meter = 0.0;
            self.time_left = 0.0;
            out.push(Command::AnimSpeed(1.0));
            out.push(Command::Sfx(self.d.rage.sfx_off));
            ev.push(Event::RageOff);
        }
    }
}

impl crate::sheets::LoadoutDef {
    fn rage_start_value(&self) -> f32 {
        self.rage_start
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rage() -> Rage {
        Rage::new(Data::get())
    }

    #[test]
    fn fills_then_activates_then_drains() {
        let d = Data::get();
        let mut r = rage();
        assert!(!r.ready());
        r.gain(150.0);
        assert!(r.ready());
        let mut f = Frame::default();
        f.dt = 0.5;
        f.enemies.push(EnemyView { id: 1, pos: Vec3::new(2.0, 0.0, 0.0), hp: 900.0, max_hp: 900.0, alive: true, boss: false, engaged: true, radius: 0.6, staggered: false });
        f.enemies.push(EnemyView { id: 2, pos: Vec3::new(30.0, 0.0, 0.0), hp: 900.0, max_hp: 900.0, alive: true, boss: false, engaged: true, radius: 0.6, staggered: false });
        let (mut out, mut ev) = (vec![], vec![]);
        r.activate(&f, 420.0, d.mv("rage_burst"), &mut out, &mut ev);
        assert!(r.active);
        // only the near enemy is hit by the burst
        let hits: Vec<_> = out.iter().filter(|c| matches!(c, Command::Damage { .. })).collect();
        assert_eq!(hits.len(), 1);
        assert!(matches!(ev[0], Event::RageOn));
        // no forced animation: the override one sent in 0.1.35 left Kratos stuck in it
        assert_eq!(d.rage.er_anim, 0);
        assert!(!out.iter().any(|c| matches!(c, Command::PlayAnim(_))));
        assert_eq!(r.damage_mult(), d.rage.damage_mult);
        // gains are ignored while burning
        let before = r.meter;
        r.gain(50.0);
        assert_eq!(r.meter, before);
        // drains linearly and ends at the configured duration
        let mut t = 0.0;
        while r.active {
            out.clear();
            ev.clear();
            r.update(&f, &mut out, &mut ev);
            t += f.dt;
            assert!(t < 100.0);
        }
        assert!((t - d.rage.duration_s).abs() <= 0.6, "ended after {t}s");
        assert!(matches!(ev.last(), Some(Event::RageOff)));
        assert!(out.iter().any(|c| matches!(c, Command::AnimSpeed(s) if (*s - 1.0).abs() < 1e-6)));
        assert_eq!(r.meter, 0.0);
    }

    #[test]
    fn damage_taken_fills_the_meter() {
        let mut r = rage();
        r.gain_from_hp_loss(250.0, 2500.0); // 10% of max HP
        let want = 10.0 * Data::get().rage.gain_per_pct_hp_lost * r.gain_mult;
        assert!(want > 0.0 && (r.meter - want).abs() < 1e-3, "{} vs {want}", r.meter);
    }
}
