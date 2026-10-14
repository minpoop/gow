Fifty-fourth alpha: fixes from the 0.1.52 character-creator screenshots, and a closer look at the crash on closing.

0.1.52 brought the full beard (it shows!) and fitted Kratos to your character's build. Your screenshots showed what still needed work:

- **Hair on top of Kratos's head.** Kratos's eyes sit lower in his skull than an Elden Ring character's, so lining up the eyes left your character's crown and hair just above his. His head now sits 1.5 cm higher, so both crowns and chins line up. The helmet and body also hide every hair section now (all ten hair flags), including in the character creator.
- **The short beard under the long one.** Kratos's model also has a short goatee; it's now left out whenever the long beard is drawn.
- **Shoulder through the cloak.** Kratos's shoulder skin followed Elden Ring's upper-arm "twist" bones, which its animations roll against the arm, while the cloak over it followed the arm itself. Now both follow the arm, so they turn together.
- **The crash on closing.** It's the same crash each time, in the game's own shutdown, finding its memory damaged. Test runs now check the game's memory every 30 seconds and log when the damage first appears, to find what causes it.
- **Rebuilds Kratos once.** The first start after updating rebuilds Kratos's model from your Ragnarök files.
