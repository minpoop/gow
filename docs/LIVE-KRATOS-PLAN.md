# Live Kratos: Ragnarök draws him, Elden Ring shows him — feasibility study and plan

Status: **study only, no code yet** (2026-10-28). Branch `claude/kratos-live-render`. The main line
(`claude/gow-elden-ring-mashup-bk6ut7`, 0.1.102) is untouched and keeps working as it does.

## The idea in one paragraph
Run God of War Ragnarök as a second program next to Elden Ring. Each frame, Elden Ring tells Ragnarök where its
camera is and what Kratos is doing; Ragnarök draws Kratos (his real model, fur, shaders, armour) from that camera;
the picture of Kratos goes back to Elden Ring on the graphics card, and Elden Ring paints it into its own frame
where the Tarnished would be, hidden behind anything that stands in front of him. Elden Ring stays in charge of
everything that matters for play (where he is, hits, health, enemies). Ragnarök only paints.

## 1. What Minecraft-Ring does, and what carries over
Source read: `github.com/siddoff/Minecraft-Ring` (MIT licence, so ideas and code can be borrowed with credit).

| Minecraft-Ring piece | What it does | For us |
|---|---|---|
| `er-bridge/include/bridge_protocol.h` + `link/Protocol.java` | One 8 MB file-backed shared memory block. Every block written while the other side reads is a *seqlock*: writer makes `seq` odd, writes, makes it even; readers retry. Heartbeats, PIDs, camera (`ErmcGameState`: eye, target, up, fov, near/far, view + projection matrices, player position/rotation), control flags, hit/damage rings, entity table. | **Carries over almost as is** (rewritten in Rust). We need far less of it: camera, Kratos's pose/move, flags, heartbeats. No terrain, no entities, no damage ring — Elden Ring keeps all of that. |
| `er-bridge/src/gpu_transport.h` | Elden Ring creates 6 frame slots × 4 textures (colour BGRA8, depth R32F, HUD, hand) with `D3D12_HEAP_FLAG_SHARED`, names them `Local\ERMCGPU_<pid>_<gen>_<slot>_<layer>`, plus one shared fence. The other side opens them, draws into them, signals the fence; Elden Ring acknowledges slots when it is done. A CPU-memory fallback exists. | **Carries over, and is simpler for us**: Minecraft is OpenGL and needs GL↔D3D12 interop; Ragnarök is D3D12 itself, so it opens the same handles with `OpenSharedHandle` and copies straight in. Both games must be on the same graphics card (a problem only on laptops with two GPUs). |
| `er-bridge/src/compositor.cpp` | Hooks Present + `ExecuteCommandLists`; finds Elden Ring's scene depth; draws the other game's colour where its linear depth is nearer than Elden Ring's (handles reversed depth); relights the pixels from a blurred copy of Elden Ring's frame and adds distance haze. Picks the received frame whose *camera pose id* matches the pose Elden Ring rendered (`pose_for_present`, `pick_slot`) and logs exact/older/missing counts. | **Carries over** (the shader is ~60 lines; port to Rust/HLSL). Its depth finder has two parts: a version-independent one (`depth_tracking.h`: watch resource barriers for full-screen depth buffers) and a version-specific vtable address for ER 2.7.1.0. We need the first; the address will not match our 2.7.2.x. |
| `CameraSync.java` | Two modes: `DRIVE_HOST` (Minecraft owns the camera; Elden Ring is forced to render from it) and `FOLLOW_HOST` (the other game renders from Elden Ring's camera). Minecraft-Ring normally uses DRIVE_HOST. | **We use FOLLOW_HOST**: Elden Ring keeps its camera; Ragnarök must be made to follow it. That is the part Minecraft gets for free from Fabric and Ragnarök does not. |
| `proxy.cpp` (dinput8 loader, hot reload) | Loads the bridge into Elden Ring. | **Not needed**: `gowring.dll` is already loaded by ModEngine2. Hot reload is a nice idea for later. |
| The Fabric mod (`mc-bridge`) | Inside Minecraft: renders with the synced camera, captures world/hand/HUD layers, publishes frames. | **Needs a Ragnarök equivalent built from nothing**: a DLL injected into `GoWR.exe` that does the same with no modding framework to help. This is the whole research project. |

Also worth knowing: Minecraft-Ring was written first for macOS/CrossOver and supports only Elden Ring 2.7.1.0;
its own README calls it experimental. It proves the *Elden Ring side* of the idea works; it says nothing about
whether a game with no mod framework can be the other side.

## 2. What is publicly known about Ragnarök on PC
* **Engine and graphics**: Santa Monica's own engine, ported by Jetpack Interactive; **DirectX 12 (Ultimate, bindless
  resources)**; shaders compiled ahead of time. (PCGH interview with the port team.)
* **Protection**: shipped **without Denuvo** (Gamepressure, DSOGaming). The executable is `GoWR.exe`. PSN linking is now
  **optional** (PlayStation's support page). Nothing found suggests anti-cheat or anti-debug that blocks injection.
* **Proof that injection and camera control work**: Otis_Inf's camera tools (paid, Patreon, v1.0.1 Sep 2024) inject a
  DLL into `GoWR.exe` and give a free camera, FOV control, pause/timestop, HUD toggle, *higher LODs* and window
  resizing. So: a DLL can live in Ragnarök and the camera can be driven from outside. His code is not public.
* **Proof that drawing can be filtered**: Nexus mods "Toggle Path Markers – Characters – Beard – Chain and Blade –
  Snow – HUD…" (mod 72) and "No-HUD-No-Effects" (mod 190) hide classes of things by skipping draws through ReShade /
  ShaderToggler. Hiding the *world* but not Kratos is the reverse of that and has not been shown.
* **Not found anywhere**: a public SDK, script hooks, an entity/visibility API, or anyone running Ragnarök as a
  renderer for another game. No public notes on its camera structs, render passes or skeleton in memory.
* **Unknown and needs testing on the creator's PC**: whether Ragnarök keeps drawing at full speed while it does not
  have focus or sits behind Elden Ring's window; whether it reads the controller in the background (it would then
  move its own Kratos); memory and speed with both games running. Recommended VRAM is 8 GB for Ragnarök alone at
  1080p, 12 GB for 1440p+; Elden Ring wants several more.

## 3. The design this study recommends
**Elden Ring is the only authority; Ragnarök is a puppet painter.** This avoids the hardest thing in the brief
("keep hits in sync"): the hidden Tarnished still walks, gets hit and deals damage through GowRing's existing rules;
Ragnarök only has to *look* right.

* **Camera**: send Ragnarök the camera *relative to Kratos*: `cam_gow = KratosGoW × inverse(TarnishedER) × cam_er`.
  Kratos can then stand still somewhere quiet in Ragnarök's world while the camera orbits him exactly as Elden Ring's
  orbits the Tarnished. Same FOV, same aspect, same resolution.
* **Pose — two options, decided by what we find**:
  * *Puppet (preferred)*: write bone transforms into Ragnarök's live pose each frame, exactly as `livepose.rs` /
    `moves.rs` do in Elden Ring. The poses come from Ragnarök's own animations, which `gowring-gow` already decodes
    (HADO/CADO), picked by Elden Ring's state as `moves.rs` does now. Every move Ragnarök has becomes available
    without driving its game logic.
  * *Driven*: feed Ragnarök fake controller input so its own Kratos acts. Real moves for free but loose sync, he
    wanders in Ragnarök's world and enemies/menus can interfere. Fallback only.
* **Isolating Kratos, cheapest first**: (a) cut out pixels whose depth-reconstructed position lies outside a capsule
  around his bones (we know where his bones are), floor clipped below his feet; (b) stand him somewhere with open
  sky; (c) find the engine's per-object visibility and switch the world off (best result, hardest).
* **Composite** in Elden Ring with the ported Minecraft-Ring shader: Ragnarök's colour + linear depth tested against
  Elden Ring's depth, optional relight from Elden Ring's blurred frame.

Known costs of this design, even if every step works:
* Kratos is lit by Ragnarök's light, not Elden Ring's (the relight pass softens this); he casts **no shadow** in
  Elden Ring unless the hidden Tarnished is kept in the shadow pass only (unknown how).
* One frame of lag between the two, unless Elden Ring's own render queue gives Ragnarök enough head start (its
  simulation runs ahead of what it presents, which may hide the lag; Minecraft-Ring's pose-id matching measures it).
* Ragnarök's TAA, motion blur, depth of field and vignette must be off or they smear the edges.
* Two heavy games at once: a 12 GB+ card and a strong CPU will be the practical minimum.
* Thrown axe, Spartan Rage effects, the cloak's motion: the cloak simulates in Ragnarök, but if Kratos stands still
  there it will not trail when he runs in Elden Ring (we could slide him along a flat floor in Ragnarök to fix that).

## Stage 0 result (2026-10-29, the creator's PC)
RTX 4070 12 GB, Ryzen 7 9800X3D, 64 GB RAM. With both games running: **Ragnarök keeps drawing frames while Elden
Ring is in front**; Elden Ring holds a stable 60 fps at 4K; the GPU sits at 100 % (expected: both games take all it
gives). The controller appears to drive whichever window has focus (to confirm). Not yet measured: Ragnarök's own
frame rate alongside, video memory in use. The go/no-go passed; stage 1's probe will log the rest.

## Stage 1 status (2026-10-29)
Built: GowRProbe 0.1.0 (`dist/GowRProbe-0.1.0.zip`, MODLOG). Tested under Wine against a stand-in that draws with
D3D12. **Run on the creator's PC (2026-10-29): passed.** Loaded into the real GoWR.exe without a freeze, hooks in,
one DIRECT queue, 8 back-buffer copies (5-15 ms each), nothing blocked injection. Ragnarök drew 40-106 fps behind
Elden Ring (at 720p, full world, Elden Ring at 4K) and its memory budget halves while Elden Ring is in front: GPU
time, not memory, is the constraint to plan for (render Kratos at a lower resolution, keep frame generation off).
Details in MODLOG.

## Stage 2a status (2026-10-29)
Built: GowRProbe 0.2.0 — a camera search guided by beeps (Ctrl+F9), tested under Wine against a fake camera;
read offline with `tools/live_kratos/camsearch.py`. First real run: the per-frame camera blocks found (view,
camera-to-world, projection; y up, metres); the source they are copied from not yet. GowRProbe 0.3.0 looks for the
position kept as doubles or matrix columns and watches which code writes the copies (debug registers).

## 4. Staged plan — riskiest unknowns first, each stage can stop the project cheaply

Every discovery step works like the 0.1.89 pose search: I build a probe, you run it, it writes a log/dump, I read
it offline. I cannot run either game here, so each step is at least one round trip with you.

| Stage | What we do | What it proves | Effort | Risk |
|---|---|---|---|---|
| **0. Two games at once (no code)** | You start Elden Ring through Melty as usual, then Ragnarök from Steam, load a save, put Ragnarök in a window behind Elden Ring. Note frame rates, whether Ragnarök keeps moving (e.g. Kratos idle breathing, water) when Elden Ring has focus, whether Ragnarök reacts to your controller, memory use (Task Manager), and GPU name/VRAM. | Whether your PC can carry both, and whether Ragnarök renders in the background. If it stops drawing or the PC can't hold both, the idea is dead or needs a different shape before any code is written. | 30 min of your time | **High** (go/no-go) |
| **1. Get inside Ragnarök** | A small `gowr-probe.dll` plus an injector started by `GowRing.exe` (no files written into Ragnarök's folder). It hooks Ragnarök's D3D12 Present (reusing the hudhook approach from `boot.rs` / `overlay.rs`), logs the swapchain/GPU, saves a few back-buffer screenshots, and blocks Ragnarök's controller/keyboard reading while running. | We can run our code in `GoWR.exe` and see what it draws; Ragnarök stops reacting to your input. | 2–4 days of mine + 1 test | Low–medium |
| **2. Take the camera** | Probe dumps the constant buffers it uploads and memory near the player each frame while you turn the camera; offline I find the view/projection matrices and the camera object (method of `er_pose_find.py`). Then: override it — a test that slowly orbits Kratos on its own. | We can put Ragnarök's camera anywhere. Otis_Inf's tool says this is possible. | 1–3 weeks, 3–6 tests | Medium |
| **3. Find Kratos's live pose and puppet him** | Same pose search as 0.1.89–0.1.90 but in Ragnarök: dump candidate bone arrays while he idles/walks, match them offline against the skeleton we already decode, then write one bone (raise an arm) each frame and see it on screen. | Ragnarök will draw a pose we choose, with fur, cloth and armour following. This is the heart of the idea. | 2–6 weeks, many tests | **High** — if his skinning happens in a place we can't reach in time, puppeting fails and only the loose "driven" fallback remains |
| **4. Cut Kratos out** | Find Ragnarök's depth buffer (barrier-watch method from Minecraft-Ring), rebuild positions, keep only pixels inside the bone capsule; write colour+alpha+depth to a file to check. Later, if needed, hunt the engine's visibility switch. | A clean picture of just Kratos with depth. | 1–2 weeks for the cut-out; engine hiding open-ended | Medium (cut-out) / High (engine) |
| **5. The bridge, standing still** | Port the shared memory + shared-texture transport + compositor into `gowring.dll` (Elden Ring) and the Ragnarök DLL. Elden Ring sends its camera (already read in `runtime.rs` for the HUD markers), Ragnarök returns Kratos, Elden Ring draws him with depth occlusion; the Tarnished hidden. Kratos stands in his idle. F-key to toggle, like Minecraft-Ring's F6. | The full loop works: Kratos from Ragnarök standing in Limgrave, hidden behind rocks correctly, camera locked. First thing that looks like the goal. | 2–4 weeks | Medium |
| **6. Moves in step** | Drive the puppet from Elden Ring's state each frame with Ragnarök's animations (`moves.rs` logic), measure pose-id sync (exact/older/missing), tune lag. | He walks, runs, attacks in step with the hidden Tarnished. | 3–6 weeks | Medium–high |
| **7. Make it a mod** | `GowRing.exe` starts both games in order, waits, cleans up; relight, weapons/throws, effects, shadows if possible, settings, Melty listing. | Something a player can install. | weeks–months | Medium |

**Honest total**: stages 0–5 are roughly 2–4 months of round trips before Kratos stands in Elden Ring from
Ragnarök; full play is longer. Stages 0, 1 and 3 can each end the project; they come first for that reason.

## 5. What we reuse from GowRing
* `crates/gowring-dll/src/boot.rs`, `overlay.rs` — hudhook already hooks Elden Ring's D3D12 Present and command
  queue: the compositor draws there. The same crate hooks Ragnarök's Present in its own DLL.
* `crates/gowring-dll/src/runtime.rs` (camera into `Snapshot`, ~line 346) — Elden Ring's camera position, axes and FOV
  are already read every frame; that is the camera we send.
* `livepose.rs`, `moves.rs` — how to find a live pose buffer and write over it each frame; which move to play when.
* `tools/ragnarok_anim/` (`er_pose_find.py`, `skel.py`, `hado.py`, `cado.py`) — the offline pose search and
  Ragnarök's own skeleton/animation decoders: exactly what stage 3 needs, now pointed at Ragnarök's memory.
* `testmode.rs` + the Melty test flow and log — every stage's probe reports the same way.
* The existing converted-Kratos mod stays as the fallback and the shipping version until this one beats it.

## 6. Rules this work keeps
Nothing from either game is shipped; Ragnarök is read and run from the player's own install. No Elden Ring archive
decryption. Every Melty test is asked for first. Offline, single player only.

## Sources
Minecraft-Ring: github.com/siddoff/Minecraft-Ring (README, docs/development.md, bridge-base/elden-ring). PCGH,
"Tech-Interview zu God of War Ragnarök" (DirectX 12 Ultimate, precompiled shaders) and its benchmark article (VRAM).
Gamepressure, "God of War Ragnarok without Denuvo". PlayStation support, "God of War Ragnarök for PC" (PSN optional).
opm.fransbouma.com/Cameras/gowr.htm (Otis_Inf camera tools). Nexus Mods, God of War Ragnarök mods 72 and 190.
