//! Elemental build-up from ordinary weapon hits, as in Ragnarök: Leviathan Axe hits chill an enemy until it freezes,
//! Blades of Chaos hits heat it until it burns. Each enemy has one meter (0-100) per element; a full meter applies the
//! status and starts again, and a meter left alone drains after a short pause.

use crate::sheets::WeaponStatus;
use crate::types::EnemyId;
use std::collections::{HashMap, HashSet};

pub const FULL: f32 = 100.0;

#[derive(Clone, Copy, Debug)]
struct Meter {
    kind: WeaponStatus,
    pts: f32,
    last: f64,
}

#[derive(Default)]
pub struct StatusMeters {
    m: HashMap<EnemyId, Meter>,
}

impl StatusMeters {
    /// Add `pts` of `kind` to an enemy. Returns true when this filled the meter (it then starts again from zero).
    /// A hit of the other element replaces the build-up (an enemy is chilled or heated, not both).
    pub fn add(&mut self, enemy: EnemyId, kind: WeaponStatus, pts: f32, now: f64) -> bool {
        if kind == WeaponStatus::None || pts <= 0.0 {
            return false;
        }
        let m = self.m.entry(enemy).or_insert(Meter { kind, pts: 0.0, last: now });
        if m.kind != kind {
            *m = Meter { kind, pts: 0.0, last: now };
        }
        m.pts += pts;
        m.last = now;
        if m.pts >= FULL {
            m.pts = 0.0;
            return true;
        }
        false
    }

    /// Drain meters nobody has added to for `after` seconds.
    pub fn update(&mut self, dt: f32, now: f64, after: f32, per_s: f32) {
        for m in self.m.values_mut() {
            if now - m.last > after as f64 {
                m.pts = (m.pts - per_s * dt).max(0.0);
            }
        }
        self.m.retain(|_, m| m.pts > 0.0 || now - m.last <= after as f64);
    }

    /// How full an enemy's meter is (0-1) and of what.
    pub fn frac(&self, enemy: EnemyId) -> (f32, WeaponStatus) {
        self.m.get(&enemy).map(|m| ((m.pts / FULL).clamp(0.0, 1.0), m.kind)).unwrap_or((0.0, WeaponStatus::None))
    }

    /// Forget enemies that are gone.
    pub fn keep_only(&mut self, live: &HashSet<EnemyId>) {
        self.m.retain(|id, _| live.contains(id));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fills_resets_switches_and_drains() {
        let mut s = StatusMeters::default();
        for i in 0..9 {
            assert!(!s.add(1, WeaponStatus::Burning, 10.0, i as f64 * 0.1));
        }
        assert!((s.frac(1).0 - 0.9).abs() < 1e-4);
        assert!(s.add(1, WeaponStatus::Burning, 10.0, 1.0), "the tenth hit fills it");
        assert_eq!(s.frac(1).0, 0.0, "and it starts again");
        s.add(1, WeaponStatus::Burning, 50.0, 1.1);
        s.add(1, WeaponStatus::Frozen, 12.0, 1.2);
        assert_eq!(s.frac(1), (0.12, WeaponStatus::Frozen), "frost replaces the heat");
        s.update(0.5, 2.0, 2.5, 25.0);
        assert!((s.frac(1).0 - 0.12).abs() < 1e-4, "no drain during the pause");
        s.update(0.2, 4.0, 2.5, 25.0);
        assert!(s.frac(1).0 < 0.12, "drains after it");
        assert!(!s.add(2, WeaponStatus::None, 50.0, 0.0));
        assert_eq!(s.frac(2).0, 0.0);
    }
}
