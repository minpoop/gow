Fourth alpha. The last test got Elden Ring to start with the mod loaded; it failed one self-check, and the cause is a version mismatch:

- **Elden Ring 2.7.1.0 is now supported.** The memory map library the mod is built on only knew 2.6.2, so the mod could not register its per-frame tasks on the current Steam build and never patched the game's parameter tables. It now builds on the library's newer source, which includes 2.7.1.0 (and 2.7.1.1).
- If a self-check still fails, it now reports what the mod was doing (start-up stage, attempts, last error and last log lines) so the next fix needs no log files.
- The setup report (`%LOCALAPPDATA%\GowRing\diagnose.txt`) can now read the Ragnarök PC `.wad` files (they are LZ4 frames) and lists the resource names inside them.

Still an alpha that has never been played inside the real games — see STATUS.md.
