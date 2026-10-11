Ninth alpha. The last run (0.1.7) got through every in-game check on Elden Ring 2.7.1.0: the mod loads, plays on its own save, patches the parameter tables, hooks the HUD, equips all four weapons on a new character and runs the combat rules. This build fixes what that run's log showed:

- **Kratos' health.** Elden Ring recalculates max HP from Vigor every frame and discarded the mod's direct write (it showed up as 1978 "damage taken" a few seconds after spawning, which would have charged the Rage meter). The starting classes now start at Vigor 99 instead, which is how the game itself gives a character more health.
- The mod's log now records how long the combat rules take each frame, and how close the nearest living enemy is, so a frame-rate dip or "no enemies found" can be read from the log.
- Everything from earlier builds stays: record-only exit trace, heartbeat, delayed setup report, new character on the mod's own save (`ER0000.gowring2.sl2`).

Still an alpha: the HUD art is plain placeholder shapes until the Ragnarök art is mapped, and combat has only been exercised in simulation and one minute of walking around.
