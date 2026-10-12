Thirty-seventh alpha, from your 0.1.35 test: "Upon using rage my character got stuck in an animation which I then couldn't do anything so I closed the game."

- **Spartan Rage no longer locks Kratos in place.** As Rage started, the mod forced Elden Ring's Rallying Cry roar through a scripted-event shortcut that skips the game's own way of ending an animation, so Kratos stayed in it. The forced roar is gone: Rage now starts straight away, with its burst, heal and speed-up. A roar may come back later through the game's normal skill system, the way runic animations already play and end cleanly.

Not yet tried in play, because Rage stopped the last test. These are the same as in 0.1.35:

- **F10 fills Spartan Rage.** Press F10 on the keyboard at any time to fill the meter, then L3 + R3 to set it off. A message confirms it. The key can be changed in `settings.json` (`debug_rage_key`).
- **D-pad up and down no longer change your spell or quick item.** They cast your runics, but Elden Ring was also cycling your spells and items on the same press, which moved the flask out of the use-item slot. The mod now puts your selection back after each runic press. Changes you make in the menu still stick, and the flask stays on Square / X (use item).
- **Health stones.** As in Ragnarök, enemies you kill can drop a glowing green stone (about 3 in 5 do; a boss drops three). Walk over one to heal 12 % of your health. Stones are only picked up when you are hurt, last 30 seconds and blink before they go. Set `health_stones` to false in `settings.json` to turn them off.

Also in this build, from your 0.1.33 test and not yet tried in play:

- **Runics come out once.** Thunderbolt and other skills with a follow-up no longer strike a second time.
- **The Blades of Chaos are a whip on your existing character.** The old Caestus is turned into the whip, so you don't need a new character, and the "kit equipped" check passes again.
- **Hoarfrost Stomp's animation is recognised**, and the probe folder saves Kratos's own skeleton.

Still an alpha: finishers, throws and recalls have no animation yet, and Rage has no roar for now.
