Thirty-sixth alpha, from your note before the next test: "can we make a debug key to give max rage", and D-pad up/down "still swapping my potions/spells around so i cant heal", where "in gow normally heals would come from enemies drops".

- **F10 fills Spartan Rage.** Press F10 on the keyboard at any time to fill the meter, then L3 + R3 to set it off. A message confirms it. The key can be changed in `settings.json` (`debug_rage_key`).
- **D-pad up and down no longer change your spell or quick item.** They cast your runics, but Elden Ring was also cycling your spells and items on the same press, which moved the flask out of the use-item slot. The mod now puts your selection back after each runic press. Changes you make in the menu still stick, and the flask stays on Square / X (use item).
- **Health stones.** As in Ragnarök, enemies you kill can drop a glowing green stone (about 3 in 5 do; a boss drops three). Walk over one to heal 12 % of your health. Stones are only picked up when you are hurt, last 30 seconds and blink before they go. Set `health_stones` to false in `settings.json` to turn them off.

Also in this build, from your 0.1.33 test and not yet tried in play:

- **Runics come out once.** Thunderbolt and other skills with a follow-up no longer strike a second time.
- **The Blades of Chaos are a whip on your existing character.** The old Caestus is turned into the whip, so you don't need a new character, and the "kit equipped" check passes again.
- **Hoarfrost Stomp's animation is recognised**, and the probe folder saves Kratos's own skeleton.

Still an alpha: finishers, throws and recalls have no animation yet; Spartan Rage's roar is untested.
