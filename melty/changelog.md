Seventh alpha (builds on the fifth and sixth: a game closing on its own now explains itself). The last tests ran Elden Ring 2.7.1.0 with the start-up checks passing (the mod loads, patches the parameter tables and hooks the HUD), but the game closed by itself before the run ended. This build records who asks the game to exit, and starts checking the game itself:

- **Start a new character.** The mod now plays on a fresh save file (`ER0000.gowring2.sl2`), so you get a new character creation. The starting classes are rewritten, so a new character starts with the Leviathan Axe, Blades of Chaos, Draupnir Spear and Guardian Shield and no armour. Your real saves are never touched, and the character from earlier alphas stays in the old mod save file.
- New self-checks report to Melty's test: `player-found`, `kit-equipped` (all four weapons are equipped), `rules-ran` (the combat rules ran for a few seconds with you in the world) and `no-fault` (the game did not die of a crash inside it). Each failure says what the mod saw.
- If the game closes before the run ends, the checks that had not been reported yet now say why: the mod's stage, its last log lines and which code called ExitProcess (the call is recorded and passed on unchanged; nothing is blocked).
- The mod's log now has a heartbeat every few seconds (is the game's main loop running, how far has the mod got), and the parameter patch no longer raises a panic every frame while the game is still loading its tables (your log showed ~60 of them in the first minute).
- The setup report no longer reads your Ragnarök folder while Elden Ring is starting; it waits three minutes.
- The HUD banner showed an old version number; it now shows the real one.
- The setup report (`%LOCALAPPDATA%\GowRing\diagnose.txt`) now reads the table of the Ragnarök UI and Mimir `.wad` files by record count, which is what the HUD art and Mimir's voice mapping needs.

Still an alpha: the HUD art is plain placeholder shapes until the Ragnarök art is mapped.
