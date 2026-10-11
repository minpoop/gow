//! Placeholder Mimir: when the recorded Ragnarök lines could not be read from the player's install, Mimir still
//! speaks in subtitles. The lines below were written for this mod (they are not Ragnarök's dialogue) and are
//! shown only while no real recording exists for a voice set.

/// Candidate lines for a voice set id from `mimir_triggers.json`, or an empty slice for an unknown set.
pub fn lines(set: &str) -> &'static [&'static str] {
    match set {
        "mimir_greeting" => &[
            "Well. The Lands Between. Gloomy, golden, and full of things that want you dead. Feels like home.",
            "Right then. New realm, same arrangement: you swing, I narrate.",
            "Don't mind me. Just a head on a belt with excellent opinions.",
        ],
        "mimir_combat_start" => &["Company. Wonderful.", "They've seen you. They always see you. It's the shouting.", "Eyes up. I count enough of them to be rude."],
        "mimir_kill" => &["That one won't be telling stories.", "Tidy.", "Another for the ledger, I suppose."],
        "mimir_kill_streak" => &["Is this a conquest or a harvest? Either way, impressive.", "Do leave one standing. For witnesses."],
        "mimir_rage" => &["Oh, there it is. Everyone, step back!", "That's the look. Remind them why they were afraid."],
        "mimir_low_health" => &["You're bleeding rather a lot. I'd prefer you didn't.", "Easy now. Dying is a poor use of your talents."],
        "mimir_parry" => &["Beautifully timed!", "Their blade, your shield, my admiration."],
        "mimir_boss_sight" => &["Now that is a large problem.", "Stay calm. Watch the feet, not the face."],
        "mimir_boss_down" => &["Down it goes. A fine kill. I'll tell it better than it happened.", "That's the end of that. Magnificent, and slightly horrifying."],
        "mimir_death" => &["Ah. That's... not ideal.", "Rest a moment. Then, again."],
        "mimir_new_area" => &["I don't recognise the stars here. Rather exciting.", "New ground. Mind the unfriendly parts."],
        "mimir_idle" => &["Quiet. Too quiet. I'd say more, but I already am.", "Walking is fine. Hitting things is better."],
        "mimir_axe_throw" => &["Throw it like you mean it!", "Careful with that. It knows the way home."],
        "mimir_runic" => &["Now that's a show!", "Frost, fire, and flair. Lovely."],
        _ => &[],
    }
}

/// How long a text-only line stays on screen.
pub fn hold_seconds(line: &str) -> f32 {
    (2.2 + line.chars().count() as f32 * 0.045).clamp(2.5, 7.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_voice_set_in_the_sheet_has_lines() {
        for t in gowring_core::sheets::MIMIR_TRIGGERS {
            let l = lines(t.voice);
            assert!(!l.is_empty(), "no stand-in lines for {}", t.voice);
            for line in l {
                assert!(line.chars().count() < 110, "{line}");
            }
        }
        assert!(lines("mimir_nothing").is_empty());
        assert!(hold_seconds("Tidy.") >= 2.5 && hold_seconds(&"x".repeat(500)) <= 7.0);
    }
}
