Twenty-eighth alpha, from your play-test of 0.1.24 ("hud art is transparent", "boss is good") and your Ragnarök screenshot. It also carries 0.1.25 and 0.1.26, which were never played.

- **The HUD looks like Ragnarök's.** Kratos' HUD now sits together in the bottom-left corner, placed and coloured from your Ragnarök screenshot:
  - **Shield:** on a green octagon, where Ragnarök shows the relic.
  - **Runic attacks:** two blue diamond plates, light on top and heavy below. Each shows its own white symbol. A plate on cooldown goes dark and refills from the bottom up.
  - **Weapon:** a white icon on a grey disc, with a thin line and stud leading to the bars.
  - **Health:** a dark green bar that brightens toward its end.
  - **Spartan Rage:** under health, in four segments of red. The segment still filling burns orange.
  - **Prompt:** the flaming L3 + R3 buttons appear under the bars when Rage is ready.

  The weapon's name shows for a moment when you switch, and Mimir's subtitles sit above the bars. `hud_scale` in `settings.json` makes the whole cluster bigger or smaller.
- **Your Ragnarök pictures.** The bar pictures from your install are plain white bar shapes, so the HUD now draws the bars itself in Ragnarök's style; a full-colour picture you pick for a bar still replaces the drawing. The shield keeps Ragnarök's runic-armour icon. `ragnarok_hud.json` now also lists the six runic icons, so you can put Ragnarök's own runic pictures on the plates from the gallery. Your existing choices are kept.
- **Enemies stay tough after dying or resting.** Elden Ring puts an enemy back to its normal health when it respawns (after a rest at a Site of Grace or a reload). The mod now notices this and gives it the tripled health again.

Unchanged: summons (20,000 health, 3.5x damage) and Spartan Rage. Still an alpha: Elden Ring's own animations play for every move.
