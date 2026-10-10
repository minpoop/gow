Third alpha. Fixes the real reason the game never appeared in the last two test runs:

- **ModEngine2 is now bundled.** Melty only installs ModEngine2 for mods that use its own game launch; this mashup has its own launcher, so 0.1.0 and 0.1.1 looked for a ModEngine2 that was never there. The official ModEngine2 2.1.0 release (MIT licence, no anti-cheat bypass, ScyllaHide not included) now ships in the package under `GowRing/modengine2/` with its licence, together with the Microsoft Visual C++ runtime DLLs it needs (taken from Microsoft's own redistributable, shipped beside it so no install is required).
- The setup step now writes `%LOCALAPPDATA%\GowRing\diagnose.txt` while the game loads — a plain-text report of what the reader sees in your God of War Ragnarök folder. It only reads; nothing is changed or uploaded.
- The setup log no longer lists every file it could not read (it was thousands of lines).

Still an alpha that has never run inside the real games — see STATUS.md.
