# Status — what has and has not been proven (v0.1.0-alpha)

**No part of this mod has run inside Elden Ring or against a real God of War Ragnarök install.** It was built in
a Linux container that has neither game. This page says exactly what was checked, and how.

## Proven here
| Piece | How it was checked |
|---|---|
| Design sheets: 16 sheets, every cell filled, every reference resolves, hooks cover every sheet | `cargo run -p gowring-sheets --bin preflight` → CLEAN (0 errors, 0 warnings) |
| Rules engine (combos, throw/recall, spears, runics, Rage, stun/finisher, parry, Mimir timing, HUD layout, camera) | 51 unit/scenario tests against a simulated host (`cargo test -p gowring-core`) |
| God of War file reader on *synthetic* fixtures (audiopack+TOC, WAD, Wwise bank, DDS→PNG, conversion, diagnose report) | 19 tests (`cargo test -p gowring-gow`) — fixtures are written by the tests, no game files |
| Launcher `--diagnose` / `--prepare` / `--play`, ModEngine2 config, env passing, game-wait | `tools/wine-e2e.sh` under Wine with stand-ins for ModEngine2's launcher and `eldenring.exe` |
| `gowring.dll` loads, boots, logs, and redirects the save (CreateFileW, GetFileAttributes(Ex)W, MoveFileExW, CopyFileW, DeleteFileW) | `dll_probe.exe` under Wine: only `ER0000.gowring.sl2[.bak]` ever appears on disk |
| Everything compiles for `x86_64-pc-windows-msvc` against `eldenring` 0.14.0 (exe 2.7.1.0 table) | `cargo xwin build --release` |

## Not proven (every row is `unverified` in `design/hooks.json`)
| Piece | Why it is unproven |
|---|---|
| Reading the player, enemies, actions, equipment out of Elden Ring | needs the running game (field offsets come from the `eldenring` crate) |
| Hostile-character detection | the ER team-type numbers are a seeded guess (6, 7) that the DLL extends from who actually hurts the player; it logs a histogram |
| Param rewrite: carrier weapons + the ten starting classes become the Kratos kit | needs the game; weapon categories use the usual `wep_type` numbers |
| Damage/stagger/freeze applied to enemies | direct HP writes + animation-speed freeze; no engine bullets yet |
| Over-the-shoulder camera, HUD hide, stamina/HP upkeep | needs the game |
| The D3D12 HUD overlay (hudhook) | cannot create a device here (Wine has no GPU/display); HUD *layout* is tested, rendering is not |
| Audio playback | no sound device here |
| **Every God of War asset mapping** | the reader follows the community formats, but no real install was available. HUD textures are located by name in GoW WADs; Wwise media have numeric ids only, so Mimir's lines and the sound effects need `assets_map.json` filled from a `--diagnose` report of a real install |

## Not in this version at all
* **Kratos' body and weapon models.** Needs Elden Ring's skeleton/FLVER pipeline and Ragnarök's PC mesh/skeleton
  formats; the Tarnished (bare-chested, no armour) stands in. Weapons look like the vanilla Elden Ring weapons they borrow movesets from.
* **Ragnarök's animations.** No PC animation extractor exists in the community tooling; every swing is Elden Ring's.
* **Cast/execution animations** for runics and finishers (their effects and damage happen; the character does not animate for them).
* **Renaming items** (the inventory shows the vanilla names; the HUD shows Kratos' names).
* **Atreus, co-op, multiplayer** — out of scope by design.

## Next steps (in order)
1. Player runs `GowRing.exe --diagnose` (Play once, or run it from the mod folder) and shares `%LOCALAPPDATA%/GowRing/diagnose.txt`.
2. Fill `assets_map.json` / fix the name searches from that report; confirm Mimir's lines.
3. First in-game session: read `%LOCALAPPDATA%/GowRing/logs/gowring.log` — each hook logs its first success or failure.
4. Flip `CHANNEL` to `release` (strict assets) only after every `assets` row converts on a real install.
