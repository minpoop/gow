# Contract — where state crosses between processes and games

Kratos in the Lands Between has three parties: the **launcher** (`GowRing.exe`), the **DLL** (`gowring.dll`,
inside `eldenring.exe`) and the player's **God of War Ragnarök** folder (read-only, never run).

## Ownership
| Thing | Owner | Notes |
|---|---|---|
| The player character, camera, animations, hit detection, enemies | **Elden Ring** | Kratos is the Tarnished with a rewritten kit; ER plays every swing, roll and hit reaction. |
| Kratos' rules: stances, throw/recall, spears, runics, Rage, stun, finishers, parry window, Mimir timing, HUD | **gowring-core** | Pure Rust, no game access. Driven by `design/*.json`. |
| Ragnarök art/audio | **the player's GoW install** | Converted by `--prepare` into `%LOCALAPPDATA%/GowRing/cache`. Never shipped. |
| The save | **`ER0000.gowring2.sl2`** | The DLL redirects every ER save path; the real `ER0000.sl2` is never opened. |

## Launcher → DLL (environment, set by `GowRing.exe --play`)
`GOWRING_HOME` (package folder), `GOWRING_CACHE`, `GOWRING_LOCAL`, `GOWRING_CHANNEL` (`alpha`|`release`),
`GOWRING_SAVE` (save file name). `GOWRING_HOME` present ⇒ started by the launcher; absent ⇒ the DLL aborts.

## Melty → launcher (environment, from the recipe)
`GOWRING_ER={game}` (Elden Ring root, contains `Game/eldenring.exe`), `GOWRING_GOWR={game:custom-god-of-war-ragnarok}`.

## Prepare → DLL (the cache)
`cache/manifest.json` (per-asset state), `cache/hud/*.png`, `cache/sfx/*.wav`, `cache/mimir/<set>/<id>.wav`,
optional `cache/text/subtitles.json` (`{ "<wav stem>": "text" }`). `.melty-ready` in the package folder contains
`gowring-<version>-prep-<n>`; bump `<n>` whenever the cache format changes.

## Engine ↔ host (`gowring-core::Frame` / `Command`)
* Units: metres, seconds, HP in Elden Ring's integer HP, damage in ER damage points.
* Time: `Frame.time` seconds since the mod started; `dt` clamped to 1 ms … 100 ms.
* Positions: Havok space (same space as ER's camera and physics positions). Yaw → facing on the XZ plane.
* Input: `ActionBits` use ER's own `ChrActions` bit numbers (see `types.rs`), so any device works.
* One frame of latency is accepted for raycasts (`Command::Raycast` answers arrive in the next `Frame.rays`).
* Damage echo: damage the engine orders returns as `Dealt` events a frame later; the engine's ledger
  settles them so they are not counted as melee twice.
* Lifecycle: death resets Rage, thrown weapons, melee string and runic cooldowns; frozen enemies and masked
  actions are released.
