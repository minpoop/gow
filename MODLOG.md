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
