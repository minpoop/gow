Forty-third alpha: Kratos's textures and default outfit, read from your Ragnarök install.

- **Default outfit picked.** Kratos's model carries every armour set on top of each other; the mod now knows which pieces make his default look (the starting leather straps, bracers, belt, cape and fur, plus his head, beard and skin).
- **His textures.** About three minutes into play, the background check reads the textures of that outfit from your texture packs, decodes them, and writes small previews (at most 512 pixels a side) to `%LOCALAPPDATA%\GowRing\kratos_tex`, with each material's texture list and a few raw pieces for checking the decoding.
- **His default outfit's 3D data.** The same check copies the default outfit's pieces from `root.lodpack` (about 12 MB) to `%LOCALAPPDATA%\GowRing\kratos_default`, for building his Elden Ring version.
- **Elden Ring's install.** It also lists Elden Ring's files (names and sizes only) into `kratos_tex\er_install.tsv`, to plan how his body part gets loaded.
- Kratos's torso, head and arms now decode too (they store their vertices differently).

Everything stays on your PC unless you send it. Nothing changes in game in this build.
