Fifteenth alpha: a batch release. It carries the Rage fixes from the last build (not play-tested yet) plus a settings file and several HUD additions, so one session can check a lot at once.

- **Settings you can change while playing.** The first launch writes `%LOCALAPPDATA%\GowRing\settings.json`. Save it with a text editor while the game is running and the change applies within a second or two: Rage gain speed, damage the mod deals, HUD size and opacity, sound volume, Mimir's chattiness and subtitles, screen shake, effect rings, how long the controls panel stays up. Every key has a plain-language note beside it.
- **Controls panel.** For the first 25 seconds after you enter the world, a "KRATOS CONTROLS" card lists the buttons (aim, throw, recall, weapon swap, block and parry, runics, Rage, finisher).
- **HUD additions.** The weapon you are holding is named under the weapon icons, the runic slots are labelled LIGHT and HEAVY, and Mimir's subtitle carries a MIMIR tag.
- **Rage fixes from 0.1.13.** Detonating spears into empty air no longer earns Rage, ordinary swings now count toward Rage and stun (Elden Ring does not record who hit an enemy, so the mod attributes a nearby hit shortly after your own attack), and the meter is back to 100 points.
- **Rage log.** Every Rage gain is written to `gowring.log` with its size and the meter after it, so how fast it fills can be read straight off the log.
- **More Mimir lines** for kills, fights starting, axe throws, parries and runics, so he repeats himself less.

Still an alpha: the HUD is the mod's own drawing, runics and finishers have no animation of their own, and nothing from Ragnarök's own art or sound is in use yet.
