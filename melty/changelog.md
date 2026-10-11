Twenty-third alpha, from your play-test of 0.1.21 (F9, finishers, kill credit and the Ragnarök art export all worked).

- **Summoned enemies count now.** The game keeps characters made by its debug creator out of the list the mod reads, so a summoned enemy had no health bar, your hits on it built no Rage, stun or burn, and the mod never set its health. The mod now tracks summons directly: they get a health bar, every hit counts, and they really have the health from `settings.json`.
- **Summons are tougher.** 8,000 health instead of 3,000, so a summon outlasts a Spartan Rage. Your settings file is updated unless you had changed the value yourself.
- **Rage takes longer again.** Last test it filled mostly from damage you took, so damage taken now gives a third less Rage, and every gain is a little smaller. `rage_gain_multiplier` still speeds it up for testing.
- **Burn and freeze happen sooner and show it.** About 6 Blades hits burn an enemy (was 10) and about 5 axe hits freeze one (was 8). While it lasts, the enemy's bar is labelled BURNING or FROZEN. Burning now takes about a fifth of the enemy's health over 5 seconds (was about a twelfth).
- **Ragnarök art export fixed and finished.** Button prompts, letters and other one-colour shapes now come out white on transparency instead of red. The 11 HDR textures that failed now decode, and the export no longer stops at 1,500 pictures. On your PC the last export decoded 1,489 textures, every one exactly the expected size, so reading Ragnarök's files works.
- The background check now looks at the right file for Kratos's model (`r_heroa00.wad`).

Still an alpha: Elden Ring's own animations play for every move, and the HUD keeps the mod's drawing until you pick Ragnarök pictures in `ragnarok_hud.json`.
