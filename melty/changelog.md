Forty-seventh alpha: Kratos is drawn, and the freeze is fixed.

0.1.45 worked: Kratos's whole default outfit now draws on your character, at your height and facing forward. He is still a rigid statue in a plain colour, and the two test pieces (his bare upper body beside you and a small red cube) are still there.

- **No more freeze on entering the world.** The mod reads Elden Ring's player skeleton a few seconds after you arrive. That read used a slow safety check thousands of times on the game's own thread: in 0.1.45 it froze the game, and in 0.1.44 it caused a 10-second hitch. It now uses fast, safe reads on a thread of its own, so it can no longer hold the game up.
- **Your own face no longer shows through Kratos's head.** The armour that shows Kratos now hides your character's head the way a full helm does.

Making him move with you, and his own colours, come next.
