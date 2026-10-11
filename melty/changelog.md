Fifth alpha. The last test ran Elden Ring 2.7.1.0 with all four start-up checks passing (the mod loads, redirects the save, patches the parameter tables and hooks the HUD). This build starts checking the game itself:

- **Start a new character.** The mod now plays on a fresh save file (`ER0000.gowring2.sl2`), so you get a new character creation. The starting classes are rewritten, so a new character starts with the Leviathan Axe, Blades of Chaos, Draupnir Spear and Guardian Shield and no armour. Your real saves are never touched, and the character from earlier alphas stays in the old mod save file.
- New self-checks report to Melty's test: `player-found`, `kit-equipped` (all four weapons are equipped), `rules-ran` (the combat rules ran for a few seconds with you in the world) and `no-fault` (the game did not die of a crash inside it). Each failure says what the mod saw.
- The HUD banner showed an old version number; it now shows the real one.
- The setup report (`%LOCALAPPDATA%\GowRing\diagnose.txt`) now reads the table of the Ragnarök UI and Mimir `.wad` files by record count, which is what the HUD art and Mimir's voice mapping needs.

Still an alpha: the HUD art is plain placeholder shapes until the Ragnarök art is mapped.
