# MODLOG — Kratos in the Lands Between (God of War Ragnarök × Elden Ring)

Journal for the build. Anything not here is lost at the next context compaction.

## Brief (from the player, 2026-10-10)
- Host (role `primary`): **Elden Ring** (slug `elden-ring`, ModEngine2, offline, EAC off — Melty starts it).
- Companion (role `companion`): **God of War Ragnarök** (slug `custom-god-of-war-ragnarok`, Steam app 2322010).
  Melty hands the mod the folder as `{game:custom-god-of-war-ragnarok}`. Real GoW content is read from the
  player's own copy at run time; nothing from either game is shipped.
- Idea in one sentence: *post-game Kratos, with Mimir on his belt, loose in the Lands Between — full Ragnarök HUD
  and fighting kit (Leviathan Axe + Guardian Shield, Blades of Chaos, Draupnir Spear, light/heavy runic attacks,
  Spartan Rage).*
- Solo only. No Atreus, no co-op. Open sandbox start (all weapons unlocked, roam Limgrave; Tree Sentinel is
  the first big fight).
- First playable must have: all three weapons + shield, runic attacks, full HUD, Mimir.

## Route (and why)
`native-hook` Rust DLL loaded by ModEngine2 (`external_dlls`), built on `eldenring` (fromsoftware-rs 0.14, exe
2.7.1.0 RVA table = what Melty requires, `>=2.7.1.0 <=2.7.1.1`) plus a launcher exe (`GowRing.exe`) that
`--prepare`s (converts GoW assets from the player's copy) and `--play`s (writes the ME2 config, starts ME2).
Closest prior art on Melty: *ER Strike* (Elden Ring + CS2 companion, same launcher shape). Nothing on Melty
does Kratos/God of War.
Pattern from `universal-modder` `mashup-mods`: *Pattern 1 — port the content* (rules rebuilt on the host's own
engine) + *converter on the user's own install* for assets. Rejected: running GoWR alongside (no Windows/
Proton oracle, PC port is online-account-adjacent, and we only need its files).

Engine-native first: Kratos' weapons are Elden Ring weapon rows (so ER does animations, hit detection, enemy
reactions); the DLL layers Ragnarök's rules on top (stances, throw/recall, runics, rage, stun/finisher,
camera, HUD, Mimir). Rules are tested headless in `gowring-core` against a simulated host.

## Environment facts
- Dev box is a Linux cloud container: **no Elden Ring, no God of War Ragnarök, no GPU**. Cross-build with
  `cargo xwin` (`x86_64-pc-windows-msvc`), Wine 9 runs the launcher/probe exes for oracles.
- Therefore: nothing here has been run in the real games. `docs/STATUS.md` is the honest matrix.

## Hard rules honoured
- Never touch EAC; Melty/ME2 starts ER offline. DLL refuses to run if `EOS`/`start_protected_game` session
  detected (see `hooks.json: boot_guard`).
- No game files in the package. GoW assets are converted into `{localappdata}/GowRing/cache` at `--prepare`.
- Separate save (`ER0000.gowring.sl2`) via file redirect so the player's normal save is never touched.

## Log
- 2026-10-10 Intake done (5 questions). Connected to Melty (token kept in scratchpad only). `search_mashups`
  for elden-ring+GoWR: nothing close (27 ER mashups, none Kratos).
- 2026-10-10 Toolchain: cargo-xwin MSVC target works; wine64 runs test exe. `eldenring` 0.14.0 crate vendored from crates.io.
- 2026-10-10 Research: GoW Ragnarök PC assets — community tools exist for WAD/mesh (GOWTool, PC build),
  textures (DDS) and audio (`.audiopack` RIFF → `.wem`, `gowr_soundtool`). **No PC animation extractor found.**
  => Kratos' *motions* come from Elden Ring's own animation sets for v0.1; GoW rules/HUD/voice/models are read.
- 2026-10-10 Built all of it (sheets 16, core 51 tests, gow reader 19 tests, launcher + DLL cross-built, Wine checks).
  Melty draft created: modId `d005edc0-ea0c-4b98-9df5-99ea3834a624` (githubRepo minpoop/gow, remix OFF, licence unset: the
  author has not chosen). Release 0.1.0 submitted as DRAFT (sha256 5d4d19a1…), one_click_check = yes.
  Not published: no real gameplay screenshot exists and nothing has run in Elden Ring.
- 2026-10-10 Resubmitted release 0.1.0 (draft, replaces the earlier upload): sha256 8ae77c00…; listing text updated.
  Open: real gameplay screenshot (needs the player to run the Melty Test build), real GoW asset mapping (needs
  `--diagnose` from a real install), first in-game run of every `unverified` hook. See docs/STATUS.md.

## 0.1.2 — why the game never appeared (2026-10-10)
- Evidence: the player's `launcher.log` from test ad1f7313 showed `ModEngine2 is not installed` — no `modengine2_launcher.exe` in `{managed}` or its parent. Melty installs ModEngine2 itself only for a mod that uses Melty's own game launch; a mod with an exe launcher must carry its loader (ER Mario and Minecraft Ring bundle me3).
- Fix: ship the official ModEngine2 2.1.0 (MIT) in `GowRing/modengine2/` (no ScyllaHide, no debug assets), plus the Microsoft VC++ runtime DLLs beside it (Melty's package check flagged msvcp140_atomic_wait.dll etc.; taken from Microsoft's vc_redist.x64.exe, sha256 in THIRD-PARTY.txt). The launcher also puts the ModEngine2 folders on the game's PATH.
- Also: setup notes capped at 40 lines; `diagnose.txt` is written in the background while the game loads, now including hex heads of the Ragnarök WADs (the PC layout is not the 2018 one: `exec/wad/pc_le/` holds 1654 WADs, e.g. `r_ui.wad`, `r_mimir00.wad`).
- Next: read `%LOCALAPPDATA%\GowRing\diagnose.txt` + `logs\launcher.log` + `logs\me2-launcher.log` from the next test; decode the WAD container from the hex heads.

## 0.1.3 — Elden Ring 2.7.1.0 (2026-10-10)
- Melty test of 0.1.2 (fa63c337, game 2.7.1.0): game started, dll-loaded / save-redirected / overlay-hooked passed, no crash for 5:00; `params-patched` failed.
- Cause: the `eldenring` 0.14.0 crate on crates.io only has RVA tables for 2.6.2.0 (WW) / 2.6.2.1 (JP); `rva::get()` panics on 2.7.1.0, which killed the boot thread before any task was registered. Upstream fromsoftware-rs main (59fbd3b, 2026-09-20) has Ww2710 / Jp2711. Switched gowring-dll to that rev (git dependency); no source changes needed to compile.
- Failing self-checks now carry the mod's stage, retry count, last param error and last log lines, so a test result alone says what happened.
- The Ragnarök PC `.wad` files are LZ4 frames (magic 04 22 4d 18, 64 KiB independent blocks, content size set); the decompressed stream starts `WTOC` v2 and a table of 0x90-byte records from 0x40 (0xaa-filled id at +8, name at +0x18: WAD_R_UI, TXRX_R_UI, WPTX_R_UI, DLGX_R_UI ...). `lz4.rs` reads the frame; `--diagnose` now dumps the table of r_ui / r_mimir00 and searches every WAD for the sheets' words. Next: read that output and map HUD textures / Mimir.
- Audio: 38 audiopack TOCs, all `wwise-vorbis`, english(us) has 27297 media; no .bnk files on disk (banks live inside the WADs).

## 0.1.4 — first real in-game evidence (2026-10-11)
- Melty test 12a9e307 of 0.1.3 (Elden Ring 2.7.1.0): all four self-checks passed (dll-loaded, save-redirected, params-patched, overlay-hooked); the run ended at 4:00 of 5:00 "closed by itself" (played 2:05; not known whether the player quit). The upstream-rev fix for 2.7.1.0 worked.
- Player screenshot (from that run): the Kratos HUD overlay renders in the real game on top of ER (health bar, rage bar + "Spartan Rage", two runic circles, AXE / SHIELD slots as placeholder boxes, banner "Ragnarök art 0/47"), and Elden Ring's own HUD is hidden. The character is an old one (cloak, sword, round shield) from the earlier unpatched save, so it does not show the kit.
- 0.1.4: new save file `ER0000.gowring2.sl2` so the patched starting classes apply (a new character); self-checks `player-found`, `kit-equipped`, `rules-ran`, `no-fault` (vectored exception watch + exit note); banner version fixed.
- WAD table: `u32 @8` of the decompressed `WTOC` header is the record count (31 for r_summonbird, 250 r_mimir00, 31568 r_ui); records are 0x90 bytes from 0x40; names like WAD_R_*, TXRX_R_*, WPTX_R_*, DLGX_R_*, BTRX_R_*, CXT_R_*, ANMX_R_*, LGTX_R_*, SCRX_R_*, BHVX_R_*, MDLX_R_*, MATX_R_*, PRTX_R_* are per-WAD resource-type sections; their tails hold (offset?) values stepping by 0x18. `--diagnose` now reads the whole table by count and dumps r_ui / r_mimir00 names that match the sheets' words, plus the bytes after the table of two tiny WADs.

## 0.1.5 — the game closes by itself (2026-10-11)
- Test ef25e686 of 0.1.4: dll-loaded / overlay-hooked / params-patched / no-fault passed; save-redirected, player-found, kit-equipped, rules-ran never reported; "played 0:00", "its window couldn't be seen", the game closed by itself at 1:33 (a graceful exit: `no-fault` is written from DLL_PROCESS_DETACH, no fatal exception). The player may simply have been away and closed it, or Melty stopped it; unknown. 0.1.3's run also closed at 4:00 of 5:00 after 2:05 played; 0.1.2 ran the full 5:00.
- 0.1.5: record-only hooks on ExitProcess / TerminateProcess(self) log the caller chain (module+offset); `process_exit` reports unreported checks with stage, recent log and the exit call. Next: read those from the test result (or gowring.log) and see whether the exit is the game's own, ModEngine2's, Melty's, or ours.
