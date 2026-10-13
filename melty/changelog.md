Forty-sixth alpha: the likely reason Kratos's body never showed.

In 0.1.44 none of the three test pieces appeared, not even the small cube. Comparing our model files field by field with Elden Ring's own turned up the cause: each mesh carries a box the game uses to decide whether it is on screen, and we wrote that box in the format the modding tools document, which is not the one Elden Ring actually reads. To the game, every piece looked like an empty box at your feet, so it never drew them.

- **The box is fixed**, along with four smaller details that now match the game's own files exactly.
- **The same three test pieces.** The body shows Kratos's whole default outfit; the legs slot his skin only, about a metre to your right; the arms slot a small red cube near you at chest height. The body and legs pieces now carry Elden Ring's own skeleton (read from your game last session), as the game's armour does.
- **Better logs.** The log now records whether the game opened our files and whether it finished loading them.

Your character from 0.1.44 can be used as is: it already wears the three test pieces. They are still rigid statues in plain colours.
