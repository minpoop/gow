//! Enemy stun meters, stunned state and finishers.

use crate::data::Data;
use crate::engine::Event;
use crate::sheets::EnemyClassDef;
use crate::types::*;
use std::collections::HashMap;

pub struct StunState {
    pub meter: f32,
    pub class: &'static EnemyClassDef,
    pub last_hit: f64,
    pub stunned_until: f64,
    pub alive: bool,
    /// When the player last damaged this enemy (for kill credit).
    pub player_hit_at: f64,
    pub boss: bool,
    /// Already staggered for being worn down (once per enemy).
    pub wounded_done: bool,
}

#[derive(Default)]
pub struct StunTracker {
    pub enemies: HashMap<EnemyId, StunState>,
}

impl StunTracker {
    /// Keep the table in step with the enemies the host reports; emits kill events.
    pub fn sync(&mut self, d: &'static Data, f: &Frame, now: f64, ev: &mut Vec<Event>) {
        for e in &f.enemies {
            let st = self.enemies.entry(e.id).or_insert_with(|| StunState {
                meter: 0.0,
                class: d.enemy_class(e.max_hp, e.boss),
                last_hit: f64::NEG_INFINITY,
                stunned_until: f64::NEG_INFINITY,
                alive: e.alive && e.hp > 0.0,
                player_hit_at: f64::NEG_INFINITY,
                boss: e.boss,
                wounded_done: false,
            });
            // A boss bar can appear after the character was first seen.
            if e.boss && !st.boss {
                st.boss = true;
                st.class = d.enemy_class(e.max_hp, true);
            }
            let alive_now = e.alive && e.hp > 0.0;
            if st.alive && !alive_now {
                st.alive = false;
                if now - st.player_hit_at <= 6.0 {
                    ev.push(Event::Kill { enemy: e.id, boss: st.boss });
                }
            }
        }
        // Forget enemies that left the scan (but keep the dead until they vanish).
        let live: std::collections::HashSet<EnemyId> = f.enemies.iter().map(|e| e.id).collect();
        self.enemies.retain(|id, _| live.contains(id));
    }

    pub fn note_player_hit(&mut self, enemy: EnemyId, now: f64) {
        if let Some(s) = self.enemies.get_mut(&enemy) {
            s.player_hit_at = now;
        }
    }

    /// Add stun points. Returns true when this hit filled the meter.
    pub fn add(&mut self, d: &'static Data, enemy: EnemyId, points: f32, now: f64, out: &mut Vec<Command>, ev: &mut Vec<Event>) -> bool {
        let Some(s) = self.enemies.get_mut(&enemy) else { return false };
        if !s.alive || now < s.stunned_until {
            return false;
        }
        s.meter += points;
        s.last_hit = now;
        if s.meter >= s.class.stun_threshold {
            s.meter = s.class.stun_threshold;
            s.stunned_until = now + s.class.stunned_s as f64;
            out.push(Command::Stagger { target: enemy, seconds: s.class.stunned_s });
            out.push(Command::Sfx(s.class.stun_sfx));
            ev.push(Event::StunFull { enemy });
            let _ = d;
            return true;
        }
        false
    }

    /// A regular enemy the player has worn down to the tuning sheet's wounded level is staggered once, as if its
    /// stun meter had filled, so a normal fight ends with a finisher on offer rather than only a kill.
    pub fn wounded_check(&mut self, d: &'static Data, f: &Frame, now: f64, out: &mut Vec<Command>, ev: &mut Vec<Event>) {
        let level = d.tuning.wounded_hp_pct / 100.0;
        let ready: Vec<EnemyId> = f
            .enemies
            .iter()
            .filter(|e| e.alive && !e.boss && e.max_hp > 0.0 && e.hp / e.max_hp <= level)
            .filter(|e| {
                self.enemies
                    .get(&e.id)
                    .map(|s| s.alive && !s.boss && !s.wounded_done && now - s.player_hit_at <= 6.0 && now >= s.stunned_until)
                    .unwrap_or(false)
            })
            .map(|e| e.id)
            .collect();
        for id in ready {
            if let Some(s) = self.enemies.get_mut(&id) {
                s.wounded_done = true;
            }
            self.add(d, id, 1.0e6, now, out, ev);
        }
    }

    pub fn update(&mut self, d: &'static Data, dt: f32, now: f64) {
        let hold = d.tuning.stun_hold_s as f64;
        for s in self.enemies.values_mut() {
            if now < s.stunned_until {
                continue;
            }
            if s.meter >= s.class.stun_threshold && now >= s.stunned_until {
                s.meter = 0.0; // stun over: start fresh
            }
            if now - s.last_hit > hold {
                s.meter = (s.meter - s.class.decay_per_s * dt).max(0.0);
            }
        }
    }

    pub fn frac(&self, enemy: EnemyId) -> f32 {
        self.enemies.get(&enemy).map(|s| (s.meter / s.class.stun_threshold).clamp(0.0, 1.0)).unwrap_or(0.0)
    }

    pub fn is_stunned(&self, enemy: EnemyId, now: f64) -> bool {
        self.enemies.get(&enemy).map(|s| s.alive && now < s.stunned_until).unwrap_or(false)
    }

    /// The stunned enemy the player could execute right now (nearest inside reach and arc).
    pub fn finisher_target(&self, d: &'static Data, f: &Frame, now: f64) -> Option<EnemyId> {
        let t = d.tuning;
        let half_arc = t.finisher_arc_deg / 2.0;
        f.enemies
            .iter()
            .filter(|e| e.alive && self.is_stunned(e.id, now))
            .filter(|e| e.pos.dist(f.player.pos) <= t.finisher_reach_m + e.radius)
            .filter(|e| {
                let to = (e.pos - f.player.pos).flat();
                to.len() < 0.01 || f.player.facing.angle_deg(to) <= half_arc
            })
            .min_by(|a, b| {
                a.pos.dist(f.player.pos).partial_cmp(&b.pos.dist(f.player.pos)).unwrap_or(std::cmp::Ordering::Equal)
            })
            .map(|e| e.id)
    }

    /// Damage a finisher deals to `e` (a full kill for small enemies, a slice of max HP otherwise).
    pub fn finisher_damage(&self, e: &EnemyView, rage: bool) -> f32 {
        let pct = self.enemies.get(&e.id).map(|s| s.class.finisher_pct_max_hp).unwrap_or(1.0);
        let base = if pct >= 0.999 { e.hp } else { (e.max_hp * pct).min(e.hp) };
        if rage && pct < 0.999 {
            (base * 1.5).min(e.hp)
        } else {
            base
        }
    }

    pub fn clear(&mut self, enemy: EnemyId) {
        if let Some(s) = self.enemies.get_mut(&enemy) {
            s.meter = 0.0;
            s.stunned_until = f64::NEG_INFINITY;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn enemy(id: EnemyId, hp: f32, pos: Vec3) -> EnemyView {
        EnemyView { id, pos, hp, max_hp: hp, alive: true, boss: false, engaged: true, radius: 0.6, staggered: false }
    }

    #[test]
    fn meter_fills_stuns_and_decays() {
        let d = Data::get();
        let mut st = StunTracker::default();
        let mut f = Frame::default();
        f.enemies.push(enemy(1, 900.0, Vec3::new(0.0, 0.0, 2.0)));
        let (mut out, mut ev) = (vec![], vec![]);
        st.sync(d, &f, 0.0, &mut ev);
        // soldier: threshold 60
        assert!(!st.add(d, 1, 30.0, 0.0, &mut out, &mut ev));
        assert!((st.frac(1) - 0.5).abs() < 1e-3);
        // decays after the hold time
        for i in 1..=20 {
            st.update(d, 0.1, 0.0 + i as f64 * 0.1 + 1.0);
        }
        assert!(st.frac(1) < 0.5);
        // fill it
        assert!(st.add(d, 1, 100.0, 3.0, &mut out, &mut ev));
        assert!(st.is_stunned(1, 3.5));
        assert!(!st.is_stunned(1, 3.0 + 6.5));
        assert!(out.iter().any(|c| matches!(c, Command::Stagger { target: 1, .. })));
        assert!(ev.iter().any(|e| matches!(e, Event::StunFull { enemy: 1 })));
        // stunned enemies take no more stun
        assert!(!st.add(d, 1, 100.0, 3.5, &mut out, &mut ev));
    }

    #[test]
    fn finisher_needs_reach_and_facing() {
        let d = Data::get();
        let mut st = StunTracker::default();
        let mut f = Frame::default();
        f.player.facing = Vec3::new(0.0, 0.0, 1.0);
        f.enemies.push(enemy(1, 900.0, Vec3::new(0.0, 0.0, 2.0)));
        f.enemies.push(enemy(2, 900.0, Vec3::new(0.0, 0.0, -2.0))); // behind
        f.enemies.push(enemy(3, 900.0, Vec3::new(0.0, 0.0, 12.0))); // far
        let (mut out, mut ev) = (vec![], vec![]);
        st.sync(d, &f, 0.0, &mut ev);
        for id in 1..=3 {
            st.add(d, id, 1000.0, 0.0, &mut out, &mut ev);
        }
        assert_eq!(st.finisher_target(d, &f, 0.5), Some(1));
        f.enemies.remove(0);
        assert_eq!(st.finisher_target(d, &f, 0.5), None);
    }

    #[test]
    fn finisher_damage_by_class() {
        let d = Data::get();
        let mut st = StunTracker::default();
        let mut f = Frame::default();
        let boss = EnemyView { boss: true, ..enemy(9, 20000.0, Vec3::ZERO) };
        let soldier = enemy(1, 900.0, Vec3::ZERO);
        f.enemies = vec![boss.clone(), soldier.clone()];
        let mut ev = vec![];
        st.sync(d, &f, 0.0, &mut ev);
        assert_eq!(st.finisher_damage(&soldier, false), 900.0);
        let b = st.finisher_damage(&boss, false);
        assert!((b - 20000.0 * 0.18).abs() < 1.0);
        assert!(st.finisher_damage(&boss, true) > b);
    }

    #[test]
    fn a_worn_down_regular_enemy_is_staggered_once_and_bosses_never() {
        let d = Data::get();
        let mut st = StunTracker::default();
        let mut f = Frame::default();
        f.enemies.push(EnemyView { max_hp: 400.0, ..enemy(1, 400.0, Vec3::new(0.0, 0.0, 2.0)) });
        f.enemies.push(EnemyView { boss: true, max_hp: 400.0, ..enemy(2, 400.0, Vec3::new(2.0, 0.0, 2.0)) });
        f.enemies.push(EnemyView { max_hp: 400.0, ..enemy(3, 400.0, Vec3::new(-2.0, 0.0, 2.0)) });
        let (mut out, mut ev) = (vec![], vec![]);
        st.sync(d, &f, 0.0, &mut ev);
        for id in 1..=3 {
            st.note_player_hit(id, 0.0);
        }
        // healthy: nothing happens
        st.wounded_check(d, &f, 0.1, &mut out, &mut ev);
        assert!(out.is_empty());
        // 1 and 2 are down to 20% of their max, 3 was never hit by the player (poison, a fall, an ally)
        f.enemies[0].hp = 80.0;
        f.enemies[1].hp = 80.0;
        f.enemies[2].hp = 80.0;
        st.enemies.get_mut(&3).unwrap().player_hit_at = f64::NEG_INFINITY;
        st.wounded_check(d, &f, 1.0, &mut out, &mut ev);
        assert!(st.is_stunned(1, 1.5), "the worn-down enemy is staggered");
        assert!(!st.is_stunned(2, 1.5), "a boss is left to its stun bar");
        assert!(!st.is_stunned(3, 1.5), "an enemy the player did not hit is left alone");
        assert_eq!(ev.iter().filter(|e| matches!(e, Event::StunFull { enemy: 1 })).count(), 1);
        // once only, even after the stun runs out
        out.clear();
        st.wounded_check(d, &f, 20.0, &mut out, &mut ev);
        st.note_player_hit(1, 19.5);
        st.wounded_check(d, &f, 20.0, &mut out, &mut ev);
        assert!(out.is_empty(), "no second stagger");
    }

    #[test]
    fn kills_are_credited_only_after_a_player_hit() {
        let d = Data::get();
        let mut st = StunTracker::default();
        let mut f = Frame::default();
        f.enemies.push(enemy(1, 900.0, Vec3::ZERO));
        f.enemies.push(enemy(2, 900.0, Vec3::ZERO));
        let mut ev = vec![];
        st.sync(d, &f, 0.0, &mut ev);
        st.note_player_hit(1, 1.0);
        f.enemies[0].hp = 0.0;
        f.enemies[0].alive = false;
        f.enemies[1].hp = 0.0;
        f.enemies[1].alive = false;
        st.sync(d, &f, 2.0, &mut ev);
        let kills: Vec<_> = ev.iter().filter_map(|e| if let Event::Kill { enemy, .. } = e { Some(*enemy) } else { None }).collect();
        assert_eq!(kills, vec![1]);
    }
}
