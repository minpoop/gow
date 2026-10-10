# Kratos in the Lands Between

*God of War Ragnarök × Elden Ring.* Post-game Kratos — Leviathan Axe and Guardian Shield, Blades of Chaos, Draupnir
Spear, light and heavy runic attacks, Spartan Rage, finishers, and Mimir on his belt — loose in Elden Ring's open
world, with the Ragnarök HUD. Solo, offline, on its own save.

> **Status: alpha, untested in the games.** See [docs/STATUS.md](docs/STATUS.md) for exactly what has and has not been proven.

## What you need
* **Elden Ring** (Steam, PC, version 2.7.1.0–2.7.1.1) — the game you play in. Started offline through ModEngine2 by
  [Melty](https://melty.gg), which installs ModEngine2 for you.
* **God of War Ragnarök** (Steam, PC) — read from your own copy at *Play*; nothing from it is shipped or uploaded.

## How it plays
* Controls are Elden Ring's, mapped to Ragnarök's feel (see `design/bindings.json`):
  R1/R2 attack · L1 guard (tap just before a hit to parry) · **L2 hold = aim, + R1 = throw** (Axe, Spear) ·
  **Triangle (two-hand) = recall / detonate** · **D-pad right = swap Axe → Blades → Spear** · **D-pad up / down = light / heavy runic** ·
  **L3 + R3 = Spartan Rage** (when the bar is full) · **R3 on a stunned enemy = finisher**.
* A new character on this mashup's own save starts as Kratos, whatever class you pick.
* Elden Ring's HUD is hidden; the Ragnarök one is drawn instead.

## Layout
| Path | What |
|---|---|
| `design/*.json` | The design sheets — source of truth for every number and reference |
| `crates/gowring-sheets` | Sheet loader, preflight (`--bin preflight`) and code generator |
| `crates/gowring-core` | Ragnarök's rules as a host-independent engine, plus the simulated host used by tests |
| `crates/gowring-gow` | Reader for your God of War Ragnarök folder (+ `gow-fixture`, a synthetic install for tests) |
| `crates/gowring-launcher` | `GowRing.exe` — `--prepare`, `--play`, `--diagnose` |
| `crates/gowring-dll` | `gowring.dll` — the Elden Ring bridge, overlay and audio (Windows target) |
| `tools/` | Wine end-to-end script and stand-ins used to test the launcher and DLL |
| `packaging/` | Build and zip the release |
| `melty/` | The Melty listing text and install recipe |

## Build
```
cargo run -p gowring-sheets --bin preflight        # design sheets must be CLEAN
cargo test --workspace                            # rules, reader, launcher
packaging/build.sh                                # cross-builds GowRing.exe + gowring.dll and zips dist/GowRing-<ver>.zip
```
Needs Rust, `cargo-xwin` and the `x86_64-pc-windows-msvc` target. `tools/wine-e2e.sh` needs Wine.

## Credits and licences
**Licence of this project's own code: not chosen yet** (the author decides; until then all rights reserved). Built with AI (Claude). Uses [`eldenring`/fromsoftware-rs](https://github.com/vswarte/eldenring-rs) (MIT/Apache-2.0),
[hudhook](https://github.com/veeenu/hudhook) (MIT), [retour](https://github.com/Hpmason/retour-rs), rodio, ImGui, and ships
[vgmstream](https://github.com/vgmstream/vgmstream) (ISC) to convert Wwise audio. The Ragnarök audio-pack layout follows
[gowr_soundtool](https://github.com/iArtorias/gowr_soundtool) (MIT); the WAD layout follows the community GOWTool.
Elden Ring and God of War Ragnarök belong to their owners; this is an unofficial fan project and contains no game files.
