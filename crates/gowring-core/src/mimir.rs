//! Mimir: decides when he speaks. He never writes lines — the host plays Ragnarök's own recordings.

use crate::data::Data;
use crate::sheets::{MimirTriggerEvent, MimirTriggerRequires};
use crate::types::*;
use std::collections::HashMap;

pub struct Mimir {
    d: &'static Data,
    last_fired: HashMap<&'static str, f64>,
    last_line_at: f64,
    current_priority: i32,
    rng: u32,
    /// Multiplier on every trigger's chance (the player's `mimir_frequency` setting); 0 silences Mimir.
    pub chance_mult: f32,
}

#[derive(Clone, Copy, Debug)]
pub struct Context {
    pub in_combat: bool,
    pub voice_playing: bool,
}

impl Mimir {
    pub fn new(d: &'static Data) -> Self {
        Mimir { d, last_fired: HashMap::new(), last_line_at: f64::NEG_INFINITY, current_priority: 0, rng: (d.tuning.rng_seed as u32).max(1), chance_mult: 1.0 }
    }

    fn roll(&mut self) -> f32 {
        // xorshift32
        let mut x = self.rng;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.rng = x;
        (x % 10_000) as f32 / 100.0
    }

    /// An event happened; maybe Mimir reacts. Returns true when a line was requested.
    pub fn event(&mut self, ev: MimirTriggerEvent, now: f64, ctx: Context, out: &mut Vec<Command>) -> bool {
        let settings = self.d.mimir;
        // Highest priority trigger first.
        let mut triggers: Vec<_> = self.d.triggers(ev).to_vec();
        triggers.sort_by_key(|t| std::cmp::Reverse(t.priority));
        for t in triggers {
            if let Some(last) = self.last_fired.get(t.id) {
                if now - last < t.cooldown_s as f64 {
                    continue;
                }
            }
            match t.requires {
                MimirTriggerRequires::InCombat if !ctx.in_combat => continue,
                MimirTriggerRequires::OutOfCombat if ctx.in_combat => continue,
                _ => {}
            }
            let busy = ctx.voice_playing;
            if busy && !(t.interrupts && t.priority > self.current_priority) {
                continue;
            }
            if !busy && !t.interrupts && now - self.last_line_at < settings.min_gap_s as f64 {
                continue;
            }
            if self.roll() >= t.chance_pct * self.chance_mult {
                // A failed roll still burns half the cooldown so it does not re-roll every frame.
                self.last_fired.insert(t.id, now - t.cooldown_s as f64 * 0.5);
                continue;
            }
            self.last_fired.insert(t.id, now);
            self.last_line_at = now;
            self.current_priority = t.priority;
            out.push(Command::Voice { set: t.voice, no_repeat: settings.no_repeat_window as usize, priority: t.priority, interrupt: t.interrupts });
            return true;
        }
        false
    }

    /// Voice finished: forget the priority so the next line is judged fresh.
    pub fn idle(&mut self) {
        self.current_priority = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx(c: bool, v: bool) -> Context {
        Context { in_combat: c, voice_playing: v }
    }

    #[test]
    fn greeting_once_then_cooldown() {
        let d = Data::get();
        let mut m = Mimir::new(d);
        let mut out = vec![];
        assert!(m.event(MimirTriggerEvent::GameStart, 5.0, ctx(false, false), &mut out));
        assert!(matches!(out[0], Command::Voice { set: "mimir_greeting", .. }));
        assert!(!m.event(MimirTriggerEvent::GameStart, 600.0, ctx(false, false), &mut out));
    }

    #[test]
    fn requires_combat_state() {
        let d = Data::get();
        let mut m = Mimir::new(d);
        let mut out = vec![];
        // idle chatter needs peace
        let mut got = false;
        for i in 0..50 {
            got |= m.event(MimirTriggerEvent::Idle, 1000.0 + i as f64 * 200.0, ctx(true, false), &mut out);
        }
        assert!(!got);
        // mt_kill_streak needs combat; a peaceful call never fires
        assert!(!m.event(MimirTriggerEvent::KillStreak, 3000.0, ctx(false, false), &mut out));
    }

    #[test]
    fn high_priority_interrupts_but_low_does_not() {
        let d = Data::get();
        let mut m = Mimir::new(d);
        let mut out = vec![];
        // Rage (priority 6, interrupts) while a low-priority line plays
        m.current_priority = 2;
        let mut fired = false;
        for i in 0..20 {
            fired |= m.event(MimirTriggerEvent::RageOn, 100.0 + i as f64 * 100.0, ctx(true, true), &mut out);
            if fired {
                break;
            }
        }
        assert!(fired, "rage interrupts a priority-2 line");
        // a kill (priority 2, no interrupt) never talks over someone
        out.clear();
        for i in 0..100 {
            assert!(!m.event(MimirTriggerEvent::Kill, 5000.0 + i as f64 * 100.0, ctx(true, true), &mut out));
        }
        assert!(out.is_empty());
    }

    #[test]
    fn chance_is_respected_roughly() {
        let d = Data::get();
        let mut m = Mimir::new(d);
        let mut out = vec![];
        // mt_kill: chance 25%, cooldown 30 s. Fire an event every 300 s (cooldown never blocks).
        let mut n = 0;
        for i in 0..400 {
            if m.event(MimirTriggerEvent::Kill, 1000.0 + i as f64 * 300.0, ctx(false, false), &mut out) {
                n += 1;
            }
        }
        // failed rolls burn half a cooldown but 300s spacing dwarfs it
        assert!((60..140).contains(&n), "{n} of 400 at 25%");
    }
}
