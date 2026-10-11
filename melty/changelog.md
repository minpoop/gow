Seventeenth alpha: Spartan Rage becomes a tide-turner, from your play-test of 0.1.15 (stuns, wounded staggers and finishers all worked).

- **Rage takes longer to earn.** A full bar now takes roughly twice as long as in 0.1.15 (about 3 minutes of fighting instead of about 85 seconds). For quick testing, set `rage_gain_multiplier` to 2 in `settings.json` to get the old speed back.
- **Turning Rage on swings the fight.** It heals 20% of your health at once and knocks down every enemy within 10 metres for 3 seconds (was 8 metres, 1.5 seconds).
- **Rage is much stronger while it lasts.** Double damage (was 1.75x), you take 35% of incoming damage (was 50%), 3% of your health back every second (was 1.5%), and enemies' stun bars fill three times as fast (was twice), so finishers chain.
- **Kills feed it.** Every kill while raging adds 2 seconds, up to a full bar.
- **Finishers land harder.** The camera punches in and the screen shakes when a finisher connects, a little more during Rage.
- **Fix:** an enemy killed by a single blow did not count as your kill, so Mimir's kill lines and kill streaks missed them. It counts now.

Still an alpha: Elden Ring's own animations play for every move, the HUD is the mod's own drawing, and nothing from Ragnarök's own art or sound is in use yet.
