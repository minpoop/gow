Forty-first alpha: another look at your Ragnarök install for Kratos's detailed model.

- **Searching the right files first.** Last time the search for Kratos's detailed model spent its time on the big texture packs and stopped before reaching the level-of-detail packs (`.lodpack`), where his detailed model most likely is. It now searches `root.lodpack` first, then the other level-of-detail packs, and skips textures, shaders, video and sound.
- **The packs' contents lists.** It also copies the small contents lists of the packs (`.toc` files, about 5 MB together), two small index files from the install, and the first 4 MB of the two shared level-of-detail packs into `%LOCALAPPDATA%\GowRing\kratos_probe\packs`, so the pack format can be worked out. Nothing leaves your PC unless you send it.
- Your last run listed all 1,052 animation sets in Kratos's main animation file: they are mostly cutscene, story and traversal animations; his combat moves are kept elsewhere.

Nothing changes in game in this build.
