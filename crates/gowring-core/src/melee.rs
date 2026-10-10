//! Combo tracking for Elden Ring's native moveset.
//!
//! ER plays the animation and resolves the hit; this module only keeps track of which Kratos
//! move that swing corresponds to, so its stun and rage numbers (from `moves.json`) can be
//! applied when the host reports damage dealt.

use crate::data::Data;
use crate::sheets::{MoveDef, MoveKind, WeaponDef};
use crate::types::{EnemyId, Frame};
use std::collections::HashMap;

#[derive(Clone, Copy, Debug)]
pub struct Active {
    pub mv: &'static MoveDef,
    pub started: f64,
}

#[derive(Default)]
pub struct Melee {
    last: Option<Active>,
    /// Hits already attributed to the active move (so a 2-hit move stops at 2).
    hits_taken: u32,
}

/// Seconds after `buffer_close_s` during which a press still continues the string.
const GRACE_S: f64 = 0.35;

impl Melee {
    pub fn current(&self) -> Option<Active> {
        self.last
    }

    pub fn reset(&mut self) {
        self.last = None;
        self.hits_taken = 0;
    }

    fn alive(&self, now: f64) -> Option<Active> {
        self.last.filter(|a| now - a.started <= a.mv.buffer_close_s as f64 + GRACE_S + a.mv.recovery_s as f64 * 0.0)
    }

    fn find_kind(d: &Data, w: &WeaponDef, kind: MoveKind) -> Option<&'static MoveDef> {
        w.lights.iter().chain(w.heavies).map(|id| d.mv(id)).find(|m| m.kind == kind)
    }

    /// A light-attack press: pick the move ER is about to play.
    pub fn light_pressed(&mut self, d: &Data, w: &WeaponDef, f: &Frame, now: f64) -> Option<&'static MoveDef> {
        let mv = if !f.player.grounded {
            Self::find_kind(d, w, MoveKind::Aerial)
        } else if f.player.sprinting {
            Self::find_kind(d, w, MoveKind::Sprint)
        } else {
            None
        };
        let mv = mv.or_else(|| {
            let first = w.lights.iter().map(|id| d.mv(id)).find(|m| m.kind == MoveKind::Light && m.chain_pos == 1);
            match self.alive(now) {
                Some(a) if a.mv.weapon == w.id => match a.mv.next_light {
                    Some(n) => Some(d.mv(n)),
                    None => first,
                },
                _ => first,
            }
        })?;
        self.start(mv, now);
        Some(mv)
    }

    /// A heavy press: continue a heavy chain or branch off a light string.
    pub fn heavy_pressed(&mut self, d: &Data, w: &WeaponDef, now: f64) -> Option<&'static MoveDef> {
        let first = w.heavies.iter().map(|id| d.mv(id)).find(|m| m.chain_pos == 1);
        let mv = match self.alive(now) {
            Some(a) if a.mv.weapon == w.id => match a.mv.next_heavy {
                Some(n) => Some(d.mv(n)),
                None => first,
            },
            _ => first,
        }?;
        self.start(mv, now);
        Some(mv)
    }

    pub fn start(&mut self, mv: &'static MoveDef, now: f64) {
        self.last = Some(Active { mv, started: now });
        self.hits_taken = 0;
    }

    /// Which move a damage event belongs to: the active swing while it can still connect.
    pub fn attribute(&mut self, now: f64) -> Option<&'static MoveDef> {
        let a = self.last?;
        let window = a.mv.buffer_close_s as f64 + a.mv.recovery_s as f64 + 0.35;
        if now - a.started > window {
            return None;
        }
        if self.hits_taken >= a.mv.hits.max(1) as u32 * 4 {
            return None;
        }
        self.hits_taken += 1;
        Some(a.mv)
    }
}

/// Damage the engine itself commanded; the host will echo it back as `Dealt` events a frame or
/// two later and those must not be counted as melee a second time.
#[derive(Default)]
pub struct Ledger {
    owed: HashMap<EnemyId, (f32, f64)>,
}

impl Ledger {
    pub fn expect(&mut self, enemy: EnemyId, amount: f32, now: f64) {
        let e = self.owed.entry(enemy).or_insert((0.0, now));
        e.0 += amount;
        e.1 = now;
    }

    /// Take `amount` off what is owed for `enemy`; returns the part that is NOT ours (melee).
    pub fn settle(&mut self, enemy: EnemyId, amount: f32, now: f64) -> f32 {
        let Some(e) = self.owed.get_mut(&enemy) else { return amount };
        if now - e.1 > 1.5 {
            self.owed.remove(&enemy);
            return amount;
        }
        let ours = e.0.min(amount);
        e.0 -= ours;
        if e.0 <= 0.5 {
            self.owed.remove(&enemy);
        }
        amount - ours
    }

    pub fn expire(&mut self, now: f64) {
        self.owed.retain(|_, (_, t)| now - *t <= 1.5);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn axe() -> &'static WeaponDef {
        Data::get().weapon("leviathan_axe")
    }

    #[test]
    fn light_string_advances_then_wraps() {
        let d = Data::get();
        let mut m = Melee::default();
        let f = Frame::default();
        let mut t = 0.0;
        let mut seen = vec![];
        for _ in 0..5 {
            let mv = m.light_pressed(d, axe(), &f, t).unwrap();
            seen.push(mv.id);
            t += 0.5;
        }
        assert_eq!(seen, ["axe_l1", "axe_l2", "axe_l3", "axe_l4", "axe_l1"]);
    }

    #[test]
    fn late_press_restarts_the_string() {
        let d = Data::get();
        let mut m = Melee::default();
        let f = Frame::default();
        assert_eq!(m.light_pressed(d, axe(), &f, 0.0).unwrap().id, "axe_l1");
        assert_eq!(m.light_pressed(d, axe(), &f, 3.0).unwrap().id, "axe_l1");
    }

    #[test]
    fn heavy_branches_off_a_light_string() {
        let d = Data::get();
        let mut m = Melee::default();
        let f = Frame::default();
        m.light_pressed(d, axe(), &f, 0.0);
        assert_eq!(m.heavy_pressed(d, axe(), 0.4).unwrap().id, "axe_h_finish");
        assert_eq!(m.heavy_pressed(d, axe(), 5.0).unwrap().id, "axe_h1");
        assert_eq!(m.heavy_pressed(d, axe(), 5.4).unwrap().id, "axe_h2");
    }

    #[test]
    fn sprint_and_air_variants() {
        let d = Data::get();
        let mut m = Melee::default();
        let mut f = Frame::default();
        f.player.sprinting = true;
        assert_eq!(m.light_pressed(d, axe(), &f, 0.0).unwrap().id, "axe_sprint");
        f.player.sprinting = false;
        f.player.grounded = false;
        assert_eq!(m.light_pressed(d, axe(), &f, 1.0).unwrap().id, "axe_aerial");
    }

    #[test]
    fn ledger_does_not_double_count() {
        let mut l = Ledger::default();
        l.expect(1, 500.0, 0.0);
        // host echoes 500 of ours plus 120 of real melee
        assert_eq!(l.settle(1, 620.0, 0.1), 120.0);
        assert_eq!(l.settle(1, 50.0, 0.2), 50.0);
    }
}
