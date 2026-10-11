Twenty-fourth alpha, from your play-test of 0.1.22 (summons tracked, burn and freeze, white button masks all worked).

- **Summons are bosses now.** An F9 summon gets the big boss health bar, a boss stun bar, and finishers that take a slice of its health instead of killing it outright, so the fight builds through several stuns. It has 12,000 health (was 8,000) and hits 2.5 times as hard as the enemy it copies; the extra damage never kills you on its own. `debug_enemy_health` and the new `debug_enemy_damage_multiplier` in `settings.json` change both, and a value you set yourself is kept.
- **Rage fills more slowly.** In the last test it went from empty to 87 in about 30 seconds against a summon. Every gain is now a third smaller, and damage taken gives a quarter less again. Set `rage_gain_multiplier` to 2 for quick testing.

Still an alpha: Elden Ring's own animations play for every move, and the HUD keeps the mod's drawing until you pick Ragnarök pictures in `ragnarok_hud.json`.
