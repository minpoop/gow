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
| Everything compiles for `x86_64-pc-windows-msvc` against `eldenring` from upstream fromsoftware-rs commit 09a4208 (Elden Ring 2.7.2.0 / 1.17.2; MIT NON-AI licence from 2026-10-12) (the crates.io 0.14.0 release only knows exe 2.6.2.0; 2.7.2.0 — the current Steam build — is in the upstream source) | `cargo xwin build --release` |
| Melty test 7881e51e of 0.1.9 on a player's PC (Elden Ring 2.7.1.0): 3 of 3 five-minute runs passed, all 8 self-checks each (dll-loaded, overlay-hooked, params-patched, save-redirected, player-found, kit-equipped, rules-ran, no-fault); a new character equips all four carrier weapons (shield 30060000, axe 14000000, blades 21000000, spear 16000000); the rules tick costs ~25 µs a frame; a real perfect parry fired and refunded the damage (0.1.8 log); ER's own HUD is hidden and the overlay draws | Melty `test_result` 7881e51e; player gowring.log |
| Melty test 31a0ae86 of 0.1.11 (3 of 3 runs, all 8 checks) plus the player's run-1 log: L2 stays visible while masked (30 of 30 frames, repeatedly), axe throw → recall on Triangle/Y → `axe caught` with no weapon swap, kills by thrown-axe damage orders (672 each), 16 hostiles tracked | Melty `test_result` 31a0ae86; gowring.2.log |
| Real Melty test of 0.1.2 on a player's PC (Elden Ring 2.7.1.0): the game starts through the bundled ModEngine2, gowring.dll loads, the save is redirected, the overlay hooks, no crash over 5:00; the param patch did not run (the library had no 2.7.1.0 table) | Melty `test_result` fa63c337 |

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
* **Ragnarök's animations.** No public decoder exists for Ragnarök's animation sets (`.as` files, Santa Monica's own codec); every swing is Elden Ring's. Since 0.1.33 the background report writes Kratos's skeleton and animation samples to `kratos_probe/` for study.
* **Execution animations** for finishers, and throw / recall animations (their effects and damage happen; the character does not animate for them). Since 0.1.33 runics play an Elden Ring skill's animation (unverified in play) and Spartan Rage starts with a roar gesture.
* **Renaming items** (the inventory shows the vanilla names; the HUD shows Kratos' names).
* **Atreus, co-op, multiplayer** — out of scope by design.

## Next steps (in order)
1. Player runs `GowRing.exe --diagnose` (Play once, or run it from the mod folder) and shares `%LOCALAPPDATA%/GowRing/diagnose.txt`.
2. Fill `assets_map.json` / fix the name searches from that report; confirm Mimir's lines.
3. First in-game session: read `%LOCALAPPDATA%/GowRing/logs/gowring.log` — each hook logs its first success or failure.
4. Flip `CHANNEL` to `release` (strict assets) only after every `assets` row converts on a real install.
