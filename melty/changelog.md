Forty-second alpha: Kratos's detailed model, read straight from your Ragnarök install.

- **Found it.** Kratos's detailed model is in your install's shared level-of-detail pack (`root.lodpack`), stored as one small piece per part of him. The earlier searches looked for one big block of data that is never stored that way. The mod now reads the pack's table of contents and takes his pieces directly.
- **What it writes.** About three minutes into play, the background check reads the most detailed version of each of Kratos's 281 parts (about 60 MB of your pack, a few seconds) and writes a compact copy to `%LOCALAPPDATA%\GowRing\kratos_model` (about 21 MB): `detail.grkm` (shapes only), `parts.tsv` (a list of his parts) and a few whole pieces under `samples`. It runs once. Nothing leaves your PC unless you send it.
- The search through your install's files and the copying of the packs' tables of contents are gone, since they are no longer needed.

Nothing changes in game in this build.
