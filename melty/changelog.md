Sixteenth alpha: fights now last long enough to stun and finish enemies, from your play-test of 0.1.14 (which passed Melty's two 10-minute runs).

- **Regular enemies are tougher.** Elden Ring's weakest enemies die in one hit, so nothing lived long enough to stun. Regular enemies now get three times their normal health when they first come into range. Bosses and mini-bosses are left alone. `enemy_health_multiplier` in `settings.json` changes it (1.0 = Elden Ring's own health).
- **Wounded enemies can be finished.** A regular enemy you have worn down to a quarter of its health is staggered once, as if its stun bar had filled, so a normal fight ends with an R3 finisher on offer. `finisher_when_wounded` turns it off.
- **Spartan Rage keeps the speed you tested.** Your 0.1.14 test ran with the Rage multiplier at 3 and you said it felt right, so that pace is now the built-in one and the multiplier is back at 1.0. A `settings.json` from 0.1.14 is converted for you (the old file is kept as `settings.v1.json`) so Rage plays the same as it did.
- **Fewer enemy bars through walls.** Health bars now show for enemies within 22 metres (was 35) unless you have just hit them.
- **Better log.** The log records the size of your first sixty melee hits against each enemy's health, every enemy whose health was raised, and Rage gained from damage you take, so tuning can be done from numbers instead of memory.

Still an alpha: Elden Ring's own animations play for every move, the HUD is the mod's own drawing, and nothing from Ragnarök's own art or sound is in use yet.
