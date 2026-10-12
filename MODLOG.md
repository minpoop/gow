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

## 0.1.6 — logs from the 0.1.4 run (2026-10-11)
- gowring.log (0.1.4 run, 85.9 s): boot fine (overlay hooked 5.7 s, tasks registered 5.7 s, first FrameBegin 7.6 s). `SoloParamRepository` holders were not populated until **66 s** (`Expected param holder to have exactly one res cap` panicked ~58 times, in bursts: the game's main loop ran ~60 fps for 1 s, then ~0.3 fps until 66 s). Params patched at 66.085 s (weapon rows 14000000 / 21000000 / 16000000 / 30060000, wep_type 17 / 35 / 25 / 67; 10 classes). The game never opened the save in 85 s. Exit at 85.867 s via ExitProcess (DETACH ran, 0 fatal exceptions). The launcher log shows the same pattern for an un-tested Play: `Elden Ring closed` 71 s after launch (0.1.3), and the earlier 0.1.3 test run ended at 240 s. 0.1.2 (no params patched) ran 5:00.
- Unknown: why the game exits ~5-20 s after the param patch / ~85 s after launch. Candidates not yet separated: the player closing it, an ER error dialog, the param patch itself, the harness. 0.1.5/0.1.6 record the ExitProcess caller chain, a heartbeat (frames since last beat, save/player state), and the exit time vs param patch.
- 0.1.6: param readiness probe (no panics while the tables load), heartbeat every 5 s (30 s after 2 min), background diagnose waits 180 s after launch (it read the Ragnarök folder for 108 s during one game start).
- diagnose (0.1.4): r_ui.wad has 31568 records. After the section list (WAD_R_UI, TXRX_R_UI, ...), resources come as pairs: a descriptor record (`a=001d b=80a2 c=00001200|00002100`, name `TX_<name>_<16 hex>`) and a data record (`a=0001 b=0022 c=000000c8`, same name). Tail fields hold stream offsets (descriptor: small, e.g. 0xb700; data record: large, e.g. 0x030d8320). UI texture names present: TX_sonhud_meter, TX_sonhud_meterglow, TX_sonhud_icon_*, TX_stunicon_small, TX_stunicon_housing_*, TX_toggleblockicon(+backing), TX_toggleaimicon(+backing), TX_tutorialsplash_ragedrain, TX_xbox_controller_buttonmasks, TX_worldinteract_*button. Next: dump descriptor + data heads for these to decode the texture layout.

## 0.1.8 — first clean in-game run (2026-10-11)
- 0.1.7 run (stopped by the player at 1:43, "everything worked fine"): params patched at 11 s (startup was fast this time), save opened/redirected at 24.9 s, the player appeared at 55.7 s, `kit: all 4 carrier weapons equipped [30060000, 14000000, 110000, 21000000, 110000, 16000000]`, ER's own HUD hidden, rules ticking (~1800 ticks), facing calibrator picked mapping 3 (score 1.0), ER action bits read (0x4, 0x1000). Checks reported: dll-loaded, overlay-hooked, params-patched, save-redirected, player-found, kit-equipped, rules-ran. `no-fault` was not reached (stopped before 200 s). No exit from the game itself in this run, so the earlier early exits were most likely the player closing it.
- Bug seen: the upkeep wrote `max_hp = 2500`, ER reset max HP (and clamped HP) to 522 each frame, so a 1978 "damage taken" appeared at 64.7 s and the Rage gain from HP loss would have fired. Fix: starting Vigor 99 on all ten classes (sheet column `vigor` replaces `max_hp`), no direct HP write.
- Frame rate in the log fell from 60 to ~21 fps after the player spawned (frames_since_last 300 → 107 per 5 s); unknown whether it is the game, Melty's capture, or the mod. 0.1.8 logs the rules-tick cost per heartbeat.
- `enemy_scan`: 0 hostile in range with 15 team_type-6 characters near (hostile set {6,7}); 0.1.8 logs the nearest living hostile-team distance and the scan range.

## 0.1.9 — second clean in-game run (2026-10-11)
- 0.1.8 test 1e3e6d1b: all 8 self-checks passed; Melty's `ran-to-end` failed because the player quit the game at 4:50 (the log's exit trace: `ExitProcess(0)` from `gowring.dll` hook <- OWClient.dll <- eldenring.exe+0x254454d (the game's own quit path) <- modengine2.dll — a normal quit; no fault). The player did not see Melty's timer.
- From gowring.log: rules tick ~25 µs avg (max ~0.4 ms), so the 60→30 fps drop is not the mod. The character loaded was the one from the 0.1.7 run (same spawn point, 522/522 HP): class params only apply to new characters, hence a fresh save name `ER0000.gowring3.sl2` so Vigor 99 takes effect.
- First live combat evidence: at 116 s a hostile came within 3.1 m (the Chapel's Grafted Scion); the player pressed L1 (ER bit 0x4) and the rules logged `perfect parry` with a 233 damage `Taken`, issuing the refund; HP reached 1/522 at 138 s and was 522/522 again at 191 s (died and respawned). `dealt: 0`, `damage_orders: 0`: no mod attack on an enemy yet. ER bits seen: 0x1 R1, 0x20 SpMove, 0x10 Action, 0x40 Jump, 0x1000 R3, 0x4 L1 — they agree with the sheet's ChrActions bit table.
- 0.1.9: log names the action bits; fresh save name.

## 0.1.9 passed Melty's test (2026-10-11)
- Test 7881e51e-46af-4fa9-a39f-3c3d8c1f1a2d: passed, graduates: true. 3 of 3 runs ran the full 5:00 with all 8 self-checks (installed, started, no-crash, responding, picture, ran-to-end, stray-loaders, loader-errors, self-checks all pass; `loaded` cannot be checked by Melty for this kind of mod). Played 1:30 / 0:55 / 1:05.
- Not done, and needs the user: publish decision, licence, remix, credits, committing melty.json, and a current real screenshot (the only real one so far is from 0.1.3: placeholder HUD, a pre-kit character).
- Still unproven in the real game: axe/spear throws and recall, runics, Rage, finishers, stagger/damage orders, Mimir, any converted Ragnarök art or sound (0/47).

## 0.1.10 (2026-10-11)
- User decisions: go with the suggested live combat session before publishing; licence unset; remix left off ("don't care" — kept at the restrictive default); credits per my judgement (added to the listing); melty.json committed to the repo root (on the working branch; Melty reads the linked repo, presumably its default branch).
- User screenshot of 0.1.9 on a new character (bare-chested, Leviathan-axe carrier in hand, shield on the back; HUD: health bar, empty rage bar, two runic rings, AXE/SHIELD boxes, banner `GowRing alpha 0.1.9 - Ragnarok art 0/47`) added to the draft listing (media 8493c638...).
- 0.1.10 adds log lines for axe thrown/caught, spear thrown, weapon swaps, kills and the first damage order per enemy.

## 0.1.11 — first live combat read (2026-10-11)
- 15-minute session (test e331f54e, 0.1.10, played 10:45): HP 2100/2100 at spawn (Vigor 99 works on a fresh save). Live: axe thrown ×≥12, `axe caught` (only after weapon swaps), weapon swaps D-pad (Axe→Blades→Spear→Axe), spear thrown ×3 twice, runics (Frost Shockwave 840, Glacial Cleave 1890, Inferno Whirl 1500, Spear Storm 1980, Spark Volley 648, Flame Lash 600; "hit 0" = no enemy in range at the time), enemy damage orders (672 from the axe throw) that took the Chapel boss from 2596 to 0 (`kill ... (boss)`), further kills of 27-hp soldiers, 1 parry earlier. Player took 354 and 115 damage; died once (hp 0/2100 at 344 s) and respawned. Rage never reached full in the session as far as the log shows (no RageOn).
- Player report: recall never worked; the throw crosshair "goes in and out" when held.
- Cause 1: `b_recall` was `change_style_r` (ER bit 28), which never appeared in `action_read` on the player's pad; their Triangle/Y is `Action` (0x10), seen at 71 s and with L2 at 140 s. Fixed: `b_recall` = `action`, masked only while `thrown.recall_available`, so interact still works with the axe in hand. Side effect found: while the axe was stuck, R1/R2 were masked (`masks` counter 4668 at 344 s), so a failed recall also blocked attacks; the same fix removes that.
- Cause 2: L2 was in the `disabled_action_inputs` mask; a disabled input disappears from `action_requests`, so the engine read "L2 released", unmasked, read it again... a flicker at the frame rate. Fixed: L2 is never disabled (its queued/pending copies are still cleared) and the engine keeps `aim_held` for 0.12 s after the last frame it saw it. The log reports whether L2 stays visible while masked. Not verified in play yet; the weapon art on L2 is the thing to watch.
- Log rotation keeps gowring.log, .1, .2, .3.

## 0.1.11 verified in play (2026-10-11)
- Test 31a0ae86 passed (3 of 3, graduates: true). Player: "it all worked". Run-1 log: `L2 was masked for 30 frames and still visible in 30 of them` (×4), axe thrown → Triangle (Action) → `axe caught` (49.9 s, 55.5 s, 195.9 s, 201.6 s) without a swap, thrown-axe damage orders of 672 killing 27-55 hp enemies, 16 hostiles in range at 254 s, hostile team learned from lock-on (48), first parry/damage numbers as before. Not seen: Rage (no RageOn), finisher, Blades' own moves, spear detonation.
- Waiting on the user's explicit go-ahead to publish. The listing was updated to say what has and has not been seen live.

## 0.1.12 — polish (2026-10-11)
- User: "keep this mod cooking, don't post until it's more polished". Publishing is on hold; 0.1.11 passed Melty's 3-run test and stays a draft.
- HUD: new `gowring-core/src/vector.rs` — original vector drawing (framed bars with caps/notches/lit fill/trail, cooldown rings with ready glow, axe / blades / spear / shield icons, aim reticle, stunned marker) as triangles, lines and circles; the overlay and `hud-preview` both draw the same primitives (`docs/hud-layout-preview.png` regenerated). Fills are fans grown by 0.6 px so no seams show; icons keep their own metal palette (the sheet tints them white).
- Thrown weapons: the overlay draws the axe icon spinning / the spear icon in flight (size from distance) with a halo; FX commands (runic shockwaves, spear blast, Rage burst, impacts) become expanding rings by kind (frost blue / fire orange / blast gold); `Command::Shake` shakes the camera (decaying wobble applied after ER's camera, also while no camera request is active).
- Sound: `synth.rs` generates a stand-in for each of the 14 `sfx_*` assets (filtered noise sweeps, sine sweeps, bell partials; each 0.3-1.5 s, peak <= 0.8, finite, no end clicks — unit-tested, not listened to) used only when the converted Ragnarök file is missing.
- Mimir: `mimir_text.rs` has original placeholder lines for all 14 voice sets; when no recording exists the subtitle shows for 2.5-7 s (no audio). These are not Ragnarök dialogue.
- Rage: `meter_max` 100 -> 60 (gain per hit unchanged) so it fills in an ER fight; it never reached full in ~25 min of play.
- Everything above is unverified in the real game until the next play session; the Wine probe only shows the DLL still loads and redirects the save.

## Ragnarök WAD textures: what the 0.1.11 diagnose showed (2026-10-11)
- The "UI textures" dump read the bytes at `table_end + tail_offset` for the `TX_sonhud_*`, `TX_stunicon_*`, `TX_toggle*icon*` records of r_ui.wad. They are not texture data: `TX_toggleaimicon` points at `42 43 c0 de` (LLVM / DXIL bitcode magic), others carry bit-packed shader bytecode and ASCII constant names (`...HeightFieldDetailsMask...`). The shader records that follow the section list (`transp_vs_*`, `*_ps_*`, `MTL_*`) already use offsets 0x430, 0x3f10, 0x78b0, 0xb4c0, 0x11cf0 with sizes 0x3ad4, 0x3998, ... (offset[i+1] = align16(offset[i] + size[i])), so those offsets run in one data region from 0; the TX records' offsets (0xb700 ...) fall inside that same range, so they are NOT in the same address space (probably another stream/section, or offsets scaled differently). The assumed mapping is wrong.
- Public docs: none found for the Ragnarök PC WAD/WTOC layout (mogaika's god_of_war_tools/browser cover GoW1 only). The decode is research work with no reference; the vector HUD is the shipped look until it is done.
- Next evidence to ask for if this is resumed: the whole record table of one small WAD that has a TX_ record, the decompressed bytes of the whole of that WAD, and the section-list tails (each section's first/last offset) — enough to see where the texture bytes live.

## 0.1.13 — Rage (2026-10-11)
- 0.1.12 play-test (test a8052b12, 10 min, played 6:30, all 8 self-checks passed): the player says the stand-in sounds are all good (HUD screenshot shows the new bars, rune rings with weapon icons, spear/shield icons; effect FX visible). Complaint: Rage fills too fast and only on kills, not on hits.
- Log (06b0147d gowring.1.log): `Spartan Rage ON` at 115.7 s with `dealt: 0, taken: 0, damage_orders: 0` up to that point, after ~15 spear volleys that all detonated with `0 hits`. Cause 1: `detonate()` granted `rage_gain * stuck.len()` regardless of hits (4.0 per spear → 12 per volley on air); with meter_max 60 that was ~5 volleys. Cause 2: native swings never produced `DamageEvent::Dealt`, because the bridge required `enemy.last_hit_by == player` and ER does not set it for ordinary hits (also why the player's own `Taken` events had `attacker=None`); only mod-ordered damage, echoed through `last_hit_by` written by the mod itself, registered, hence "only on kills".
- Fix: spear detonation earns Rage only per enemy hit (capped by the spears stuck); the bridge counts hp loss of a hostile within 8 m and 1.4 s of an R1/R2 press as the player's (mod orders are still netted off by the ledger); meter_max back to 100. Tests: empty-air detonation earns 0, hitting spears earn > 0.
- Unverified until the next session: that melee `Dealt` events now fire (log: `dealt` counter and `first damage dealt` early in a fight) and that Rage takes about a fight to fill.

## 0.1.14 — batch release (2026-10-11)
- User: "make bigger changes every test ... add or adjust more than one thing at a time so I can give batch criticisms". 0.1.13 was never play-tested, so 0.1.14 carries its Rage fixes plus a batch of additions; the player tests them together against a checklist.
- `settings.rs`: `%LOCALAPPDATA%\GowRing\settings.json`, created with documented defaults on first run and re-read when its mtime changes (polled every 1 s). Keys: rage_gain_multiplier, mod_damage_multiplier, hud_scale, hud_opacity, sfx_volume, stand_in_sounds, mimir_subtitles, mimir_frequency, camera_shake, effect_rings, controls_hint_seconds. Values are clamped, wrong-typed keys keep defaults, a file that does not parse keeps the last good settings and logs why. Unit-tested (throwaway crate) and the Wine probe created and loaded the file.
- Core: `Tuning` (rage gain, mod damage, Mimir chance) via `Engine::set_tuning`; `HudState.ui_scale` scales elements about their own corner. Mod damage is scaled before the ledger so netting stays exact.
- Overlay: KRATOS CONTROLS panel for `controls_hint_seconds` after entering the world, weapon name under the icons, LIGHT/HEAVY runic labels, MIMIR tag on subtitles, opacity.
- Log: every Rage gain writes `rage +X (xM) -> meter/max` so Rage speed can be read from gowring.log.
- Mimir: 14 voice sets keep original stand-in text; kills, combat start, axe throws, parries and runics got more lines.
- Unverified in real play: everything above plus the 0.1.13 Rage fixes.

## 0.1.14 verified in play, 0.1.15 (2026-10-11)
- Test 0009e11d (2 runs of 10 min) passed; Melty says it can be published (not published: the user wants polish first). Run 2 played 2:35, run 1 5:35 (the rest was idle). Player's answers to the checklist: controls card yes, HUD additions yes, Rage speed "feels ok / feels right" (the run was at rage_gain_multiplier 3.0), "things do die too fast to stun", "all animations are the default ones", live settings work, Mimir fine.
- Log: Rage went 7 -> 50 on one 316-hp hit taken (hp-loss gain, unlogged until now); `dealt: 20` and Rage ON at 158 s, boss kill at 164 s; the 55-hp Chapel enemies die to one mod hit (600), so no stun bar ever filled except on a heavy runic that also killed. After 201 s nothing happened (idle).
- Fix for "die too fast to stun": `Command::ScaleEnemy` (engine emits it once per regular enemy, `enemy_health_multiplier` default 3, ceiling `tuning.enemy_scale_hp_ceiling` = 1000 base hp, never bosses); the host scales hp, max_hp, max_uncapped_hp and base_hp once per handle id and logs `enemy_health` (and `max hp is back to ...` if the game resets it — unverified: Elden Ring may recompute enemy max HP). `StunTracker::wounded_check`: a non-boss enemy the player hit in the last 6 s and now at or below `wounded_hp_pct` (25) of max HP is staggered once via the normal stun path (finisher on offer); `finisher_when_wounded` setting.
- Rage baseline: `rage.gain_scale` 3.0 in the sheet (play-tested pace); settings_version 2; a 0.1.14 settings.json is migrated (rage multiplier / 3, backup settings.v1.json).
- HUD: enemy bars within `bar_range_m` 22 (was engage 35) unless recently hit; controls panel finisher line reworded.
- Log: first 60 native hits (`hit N on id (hp a/b)`), `rage +X from losing N hp`, rage line shows the combined multiplier.
- Unverified in real play: that writing enemy hp/max_hp sticks, how many native hits a 3x enemy takes, whether wounded stagger feels right, bar range.

## 0.1.15 verified in play, 0.1.16 — Rage as a tide-turner (2026-10-11)
- Test 5e605073 (1 run, stopped early by the player after ~4.5 min, as asked). Player: "everything works"; Rage fill "should take longer in the full release but for testing it works"; "rage should be a tide turner in a fight". Also asked for a roadmap: a Claude Docs doc "Kratos Mod Roadmap" (milestones M0-M4, research tracks R1-R2, tracker table, 0.1.16 checklist, open decisions).
- Log: settings migrated (rage 3.0 -> 1.0, settings.v1.json kept); enemy hp scaling held (no `max hp is back` lines; e.g. 198 -> 594, 55 -> 165, 466 -> 1398); wounded stagger at 146/595 (24.5%) then R3 finisher x2, a 2169-hp enemy took 8 hits (219-247 each) to its stagger at 340 then finisher; Rage 0 -> 100 between 79 s and 164 s (hits +2.2 each at x3, finishers +30, hp loss +12.1/+9.5/+21.6); Rage ON at 192.7 s: hits 241 + rage extra 181, two kills and a finisher inside 2 s.
- 0.1.16: rage.gain_scale 3.0 -> 1.5 (release pace); damage_mult 1.75 -> 2.0, damage_taken_mult 0.5 -> 0.35, regen 1.5 -> 3 %/s, stun_mult 2 -> 3; new sheet columns burst_radius_m 10 (was hard-coded 8), burst_stagger_s 3 (was 1.5), activation_heal_pct 20, kill_extend_s 2 (`Rage::extend`, capped at duration). Finisher: `Command::Shake` 1.2 (1.6 in Rage) and a camera punch-in (fov -9 deg, distance -0.9 m, 0.12 s in, 0.35 s ease out, `Camera::update(.., punch)`).
- Bug found by the new Rage test: `StunTracker::sync` ran before the frame's damage was noted, so a single blow that killed a fresh enemy was never credited (no Kill event: no Mimir kill line, no streak, no Rage extension). `sync` now also counts this frame's `Dealt` events.
- Unverified in real play: Rage pace at 1.5, activation heal/knockdown range, kill extension, camera punch-in visibility (the DLL applies fov and distance only while the mod camera is active).

## 0.1.17 — debug summon, roadmap goal (2026-10-11)
- 0.1.16 was uploaded but never tested; 0.1.17 carries it plus the user's next asks: "a debug button to spawn in a tough enemy so I'm not looking for a fight" (the first Rage took long because of time spent finding fights) and "I want at some point to have Kratos imported from his game so it looks and feels like playing Kratos from GoWR in Elden Ring" (roadmap: research chain R1 art and sound -> R2 Kratos's model -> R3 Kratos's animations, ending at that goal; the animations decision is now "until R3 lands, stopgap or not").
- Summon: F9 (settings `debug_spawn_key`, focus-checked `GetAsyncKeyState`) -> `Bridge::debug_spawn`: template = the regular hostile with the highest max HP among loaded characters within 250 m (not a boss bar, not a previous summon), cloned by `character_id`, `npc_param_id`, `EnemyIns::npc_think_param`, data `chara_init_param_id`; `WorldChrMan::spawn_debug_character` at player + facing x 5 m (+0.5 m up, Havok physics coordinates — unverified that the creator uses the same space; the log prints where it landed and the toast warns past 25 m). `last_created_chr` change = created; health set to `debug_enemy_health` (3000) and held for 2 s while the game finishes setting it up; marked in `scaled` so ScaleEnemy skips it. 5 s with no new character -> logged and toasted. Overlay `toast()`.
- Settings version 3 (new keys only): migration re-renders the file keeping values (Rage divided only for version 1 files); backup `settings.v<old>.json`.
- `debug_tools` defaults to true during the alpha; M3 (release polish) must decide whether players get it.
- Unverified in real play: that the debug creator spawns in retail offline at the requested place, the template choice, the health hold.

## 0.1.18 — weapon elements (2026-10-11, user away; asked for 30 min of progress)
- M2 "Blades identity": `status.rs` StatusMeters (one meter per enemy, 0-100, element replaces element, drains after `status_decay_after_s` 2.5 at `status_decay_per_s` 25). Sheet weapons.json gains `status` (none/frozen/burning), `status_per_hit`, `status_s`: axe frozen 12/hit 2.5 s, Blades burning 10/hit 5 s, spear/shield/hands none. Native hits add per Dealt event; thrown/recalled axe hits add 2.5x (`THROWN_STATUS_HITS`) through `Gains.status`. Full -> `Command::Status` (host: Frozen = freeze up to 8 s, Burning = max_hp/120 every 0.5 s) + a Frost/Fire FX ring + log line. HUD: `EnemyBar.status_frac/status_color`, a thin bar under the stun bar.
- Tests: status meter unit test; scenario "Blades hits set enemies burning and axe hits freeze them" (Blades burn on hit >= 5, HUD shows the build-up, spear none).
- Research started in parallel: Ragnarök PC asset formats, and how Elden Ring could take a custom player model and animations (for roadmap R1-R3).
- Unverified in real play: everything in 0.1.16-0.1.18.

## 0.1.19 — WAD arena layout (2026-10-11, research while the user was away)
- Research (two background agents, one reported so far): public Ragnarök PC tools now exist — smpack (iArtorias, GPL-3.0, Oct 2026: WAD/texpack/lodpack/.as/shaderpack/audiopack; DDS + glTF/FBX with skin; no animation export), GoWToolkit (JeanxPereira, no licence file: GNF + RDNA2 detile, mesh, material, lodpack, format notes), GOWTool gowr-pc branch, GodOfWarTool, id-daemon's PS4 model tool. Kratos = r_heroa00.wad (goProtoheroa00, MESH_heroa00_0, MG_heroa00_0); bone names not stored; animations are Santa Monica's own anm (ANM_ chunks, .as sets), not Havok, undecoded publicly. PC texpacks raw, pixels PS5/AGC tiled.
- Our earlier TX_ reading was wrong because `mem_off` (entry +0x78, u64) is an offset inside one of 9 memory arenas; the data is a run of load blocks (entry bits & 1 ends one), each laying out every arena in order; id 25 chunks are padding markers (length added after the block). New `gowring-gow/src/wtoc.rs` (own implementation from the documented rule; smpack is GPL so no code taken) with a synthetic two-block, two-arena, aligned, padded test.
- diagnose: `layout_check` on the two smallest WADs (laid-out end vs real size: "EXACT MATCH" validates the rule on real data), `ui_textures` (TX_ chunk kinds by id/server/arena, GNF magic at +0x68 of parm chunks), `kratos_probe` (r_heroa00 chunk kinds, MESH/MG/goProto/ANM entries with offsets and first bytes).
- Roadmap doc: new "Research notes" tab.
- Second research report (Elden Ring side, not yet checked by me): the player is an invisible c0000 rig dressed in parts (fc/fg/hr + hd/bd/am/lg); a custom body = replacement armour parts (FLVER2 0x2001A weighted to c0000 bones) with invisible flags hiding the base body (EquipParamProtector, exposed by the eldenring crate). Writers: Firelink (C++, MIT; FLVER/BND/TPF/DCX), SoulsFormatsNEXT (C#, GPL), soulstruct (Python, GPL); fstools-rs (Rust) is read-only. DCX: DFLT/zstd avoid Oodle. ModEngine2 mod folders override `parts/...` (files must exist before first lookup). ERSkeletonMan (C++, MIT) rescales bones at runtime. Animations: Havok 2018 spline-compressed anibnd; authoring needs Havok Content Tools 2018 (unavailable); open writers crash ER (DSAnimStudio issue 75, Oct 2026) -> plan R3 as a runtime bone-pose override from our DLL. Beard/cloth sway needs Havok cloth: blocked (cosmetic).

## 0.1.20 — Ragnarök UI textures to PNG (2026-10-11, second 30-min block while the user was away)
- `gowring-gow/src/wadtex.rs`: texture pairs in a WTOC (parm chunk id 1/server 25/client 1 with "GNF " at +0x68 and the 8-word T# at +0x78; GPU chunk id 29/server 25 of the same name, usually just before it). T# fields: format = w1>>20 & 0x1FF, width = ((w1>>30)&3 | (w2&0xFFF)<<2)+1, height = (w2>>14 & 0x3FFF)+1, last mip = w3>>16 & 0xF, tile = w3>>20 & 0x1F, type = w3>>28. WAD-resident pixels are linear (the reference tool forces linear for WAD chunks): each mip has an allocation with rows padded to 256 bytes, smallest mip first, full-size image last, rows packed tightly inside. Formats decoded: BC1/2/3/4/5/7 and RGBA8 (AGC ids 169-182, 56, 130) via texture2ddecoder (`dds::decode_raw`, refactored out of `dds::decode`).
- Launcher background diagnose: after the report, `export_ui_textures` finds r_ui.wad, decodes up to 1500 textures in at most 90 s into `%LOCALAPPDATA%\GowRing\ragnarok_ui\*.png` and appends a section to diagnose.txt (summary: pairs / written / not decoded / GPU chunk exactly the laid-out size — the last count validates the layout rule on real data).
- Tests: T# field round trip; two-mip BC1 picks the full-size image after the small one; unsupported/short inputs are errors; a synthetic one-texture WAD exports one PNG with an exact layout size.
- Unverified on real data until the player's next run (diagnose runs once per version, 3 min after launch).

## 0.1.21 — gallery and ragnarok_hud.json (2026-10-11, third block while the user was away)
- `wadtex::export` also writes `ragnarok_ui/index.html`: every exported picture with its name and size, a filter box, checkerboard behind transparency.
- `gowring-gow/src/hudmap.rs` + launcher: after the export, `%LOCALAPPDATA%\GowRing\ragnarok_hud.json` is created (every hud_* piece, empty, with its description). Before every Play, `apply_hud_choices` copies each chosen picture (`ragnarok_ui/<name>.png`) to the piece's cache path (`cache/hud/...png`), where the overlay's asset store already looks; pieces no longer chosen get their copied picture removed (tracked in `cache/hud/from_ragnarok_ui.txt`, so prepared art is never touched). Unknown pieces and missing pictures are logged (`ragnarok_hud:` lines in launcher.log).
- Tests: template/parse/apply/undo/missing; gallery content in the export test. Wine: the real `GowRing.exe --play` with a mapping file copied the picture and logged the missing one.

## 0.1.21 verified in play, 0.1.22 (2026-10-11)
- Test b322f42f (1 run, 10 min, passed; played 5:25). Player: F9 works; summon "works as intended besides the boss did not have a bar above their head"; Rage "feels good, still a bit fast; sometimes I kill the boss too fast to use it"; wounded stagger/finisher fine; burn/freeze meters fill but "don't know if they are doing anything"; kill credit fine; Ragnarök art "looks like the art but all inputs like up on dpad look red".
- Log: summons c4660 (npc 46606010, think 46600000, 5974 hp) and c4980 (3442 hp) created 0.0 m from the requested point (Havok coordinates are right) but never appeared in `hit`/scan lines: debug-created characters are not in `WorldChrMan::chr_inses_by_distance`, so no bar, no Dealt events, and the 2 s health hold never found them (they kept template health). Rage came almost entirely from damage taken (+10.2 per 158 hp at x1.5). No `burning`/`frozen` line: no meter reached 100.
- diagnose: wtoc layout EXACT MATCH on the small WADs; r_ui.wad (89,970,272 bytes): 1709 texture pairs, 1489 PNGs, 11 failed (fmt 179 BC6H), 1489 with GPU chunk exactly the laid-out size — the arena layout and the linear WAD texture layout are confirmed on real data. Cap of 1500 hit. Red buttons = BC4 (fmt 175) masks decoded into R only. The Kratos probe matched char_heroa00.wad by suffix (wrong file).
- 0.1.22: summoned handles kept and scanned directly (`summoned_handles`, merged into the scan candidates; logged once); health hold sets the exact value. Settings v4: debug_enemy_health 3000 -> 8000 (unchanged values migrated). rage.gain_scale 1.5 -> 1.25, gain_per_pct_hp_lost 0.9 -> 0.6. Axe status 12 -> 20/hit, Blades 10 -> 17/hit; host burn tick max_hp/120 -> /50; `StatusMeters::set_active/active` + `EnemyBar.status_label` (BURNING/FROZEN text under the bars). wadtex: BC4 -> white with alpha = value; BC6H (179/180) via texture2ddecoder::decode_bc6; export cap 4000 / 150 s. Kratos probe matches the file name exactly.

## 0.1.22 verified in play, 0.1.23 (2026-10-11)
- Test ff69e75b (1 run, stopped early by the player at ~4.5 min). Player: summon "works"; "it's not that strong"; Rage "feels the same"; burn/freeze "works"; art "looks right" (BC4 masks white).
- Log: summon c4600 scanned directly ("not in the game's by-distance list"), max read back as 2169 once, then hits show hp x/9128 (the hold took; the game scales base hp by something ~1.14). 22 hits of 192-423, frozen x2 (88 s, 97 s), burning 5 s at 105 s, stun full at 102.5 s, finisher at 106.6 s killed it (non-boss class: finisher = all remaining hp). The player took 4 hits in 31 s. Rage 0 -> 87 in ~31 s (+2.5 per axe hit at x1.25, +6.7 per 188 hp taken); never activated before the summon died.
- 0.1.23: summons count as bosses in the scan (`boss: ... || summoned`), so boss bar, boss stun threshold 160, finisher 18% (27% in Rage), no wounded stagger; a summon within 10 m multiplies the player's hp loss by `debug_enemy_damage_multiplier` (2.5; the extra is written in `apply`, never below 1 hp, and counts as damage taken for Rage). Settings v5: debug_enemy_health 8000 -> 12000 (unchanged values migrated), new multiplier key. rage.gain_scale 1.25 -> 0.8, gain_per_pct_hp_lost 0.6 -> 0.45.

## 0.1.23 feedback, 0.1.24 — first Ragnarök HUD art (2026-10-11)
- Test aeaf0ba1 stopped early by the player. "The boss is still a bit easy; I still have to fight 2 bosses" (one summon was not enough to fill and use Rage). Gallery screenshots (filters meter / rage / runic / health) gave the HUD candidates: TX_player_health_housing_3F78F9427233D20B, TX_player_rage_housing_0B32235030FD391C, TX_mask_segments_ragemeter_6DA0A358A445, TX_enemyhud_meter_m_F808F3D038FF250A, TX_sonhud_meter_4BD3CAF77E6696E4 (quarter arc), TX_sonhud_meterglow_050809E18431DD07, TX_rage_activate_a_*, TX_icon_runicarmor_D0945976D09F21C9.
- HUD: `assets::is_mask` (every visible pixel near-white and grey) marks white-shape textures; the overlay draws a masked bar frame as a dark body (0x17140F, 0.82) in that shape and the fill as the same mask tinted with the bar colour, clipped to the fraction (trail likewise); a masked fill is tinted too. Icons were already tinted.
- `hudmap::render/fill_suggestions` + launcher `SUGGESTED_HUD`: once per file (marker `_suggestions_applied`), empty pieces whose picture was exported get the starter picks (health frame, rage frame, enemy health, shield icon). Player choices and later empties are never overridden. Runic rings stay vector (Ragnarök's is a quarter arc to rotate).
- Bosses (summons included) take half the status build-up and half the duration (`BOSS_STATUS_RESIST`). Settings v6: summon 12000 -> 20000 health, x2.5 -> x3.5 damage (unchanged values migrated).

## 0.1.24 feedback, 0.1.25 — solid HUD art, respawn rescale (2026-10-12)
- Test 8bc11937 (1 run, 10 min). Player: "1 hud art is transparent 2 boss is good 3 works". Melty's own self-checks failed only because the player sat on the title screen for ~4 min (save opened at 243 s); not a mod fault.
- Log: all 4 starter masks uploaded as white shapes, but drawn as thin strips: the textures carry wide empty borders and a low maximum alpha, so the stretched shape was mostly transparent. Regular enemies: "max hp is back to 55 (wrote 165)" at 324 s — the game resets max hp when an enemy respawns, and our scale only ran once per id.
- 0.1.25: `assets::trim_mask` crops a mask to its alpha>8 box and stretches alpha (x255/max, x3, clamped) before upload; masked bar frames get a bronze edge (rect +2 px, 0xC9A15A at 0.9) under a dark body (0x17140F at 0.95). `host`: an enemy already scaled that is back at full health below its scaled max goes on a `rescale` queue; `apply` writes max/base/uncapped/hp back to the scaled value (logged once per id).
