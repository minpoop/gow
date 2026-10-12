Thirty-fifth alpha, from your 0.1.33 test ("the hud looks almost perfect", "sometimes when i cast the runic attacks they come out twice", "the weapon for the chaos blades is still a fist weapon").

- **Runics come out once.** The mod pressed the skill button in a way the game kept queued, so skills with a follow-up (Thunderbolt on the spear) struck a second time about two seconds later. It now presses once, and while the skill plays no further skill-button press reaches it.
- **The Blades of Chaos are a whip on your existing character too.** Elden Ring only hands out the kit when a character is made, so your character kept the old Caestus. That weapon is now turned into the whip (moveset, look and skill), so you don't need a new character. This also fixes the "kit equipped" check that failed in the test.
- **Every runic's animation is recognised.** Hoarfrost Stomp played on the axe but was counted as a miss (its animations are numbered differently from the others); each skill's own animation number is now read from the game.
- **Kratos's skeleton for the animation research.** The probe folder now saves Kratos's own skeleton (last time it saved a few props'). Your 0.1.33 probe already gave his main animation pack and 308 named animation sets to study.

Still an alpha: finishers, throws and recalls have no animation yet; Spartan Rage's roar is untested.
