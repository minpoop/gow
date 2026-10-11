Twenty-fifth alpha, from your play-test of 0.1.23 ("the boss is still a bit easy", and the Ragnarök pictures you picked from the gallery).

- **Ragnarök's own HUD art, first pieces.** The health bar, the Spartan Rage bar and the enemy bars now take Ragnarök's own bar shapes, read from your install, and the shield icon is Ragnarök's runic-armour shield. Ragnarök keeps these as white shapes and colours them in game, so the HUD does the same: a dark body in Ragnarök's shape, filled with the bar's colour. They were filled into `ragnarok_hud.json` for you once; change or empty any of them and they stay as you leave them.
- **Summons are real bosses.** 20,000 health (was 12,000) and they hit 3.5 times as hard as the enemy they copy (was 2.5), so one summon is enough to fill and use Spartan Rage. A value you set yourself in `settings.json` is kept.
- **Bosses resist burn and freeze.** Their status meters fill at half speed and the effect lasts half as long, so a boss can no longer be kept frozen.

Still an alpha: Elden Ring's own animations play for every move, and the runic rings keep the mod's drawing for now.
