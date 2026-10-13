Forty-fifth alpha: finding out why Kratos's body did not show.

In 0.1.43 the armour that should have shown Kratos hid your torso but drew nothing, and the game later crashed (most likely when it unloaded that model). This version is a test to find out what Elden Ring accepts:

- **Three test pieces on a new character.** The body shows Kratos's whole default outfit as before; the legs slot shows only his skin (head, torso and arms) standing about a metre to your right; the arms slot shows a small red cube a metre to your left at chest height. Which ones appear tells us what to fix.
- **Closer to the game's own files.** The pieces are now compressed the way Elden Ring's own files are, name their material the way working community imports do, and come with the low-detail copies the game may ask for.
- **Skeleton reading fixed.** It looked 8 bytes too early in Havok's skeleton; it now uses Havok 2018's layout (and logs what it sees if that still fails).

All pieces are rigid statues in plain colours for now. Characters made before this version keep their current look.
