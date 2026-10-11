# Listing (draft)

**Title:** Kratos in the Lands Between

**Tagline:** Play Elden Ring as post-game Kratos: axe and shield, Blades of Chaos, Draupnir Spear, runics, Spartan Rage and a Ragnarök HUD.

**Tags:** god-of-war, kratos, combat, total-conversion, alpha, singleplayer

---

# Kratos in the Lands Between
*God of War Ragnarök × Elden Ring · early alpha*

Step into the Lands Between as post-game Kratos. A new character on this mashup's own save is born with the full Ragnarök kit and a Ragnarök-style HUD, and Elden Ring's world is the sandbox.

## What you get
- **Leviathan Axe + Guardian Shield** — hold **L2** to aim, **L2 + R1** to throw, **Triangle / Y** (Elden Ring's interact button, taken over only while a throw is out) to recall it through everything in its path. Raise the shield to block; start a guard just before a hit lands for a **perfect parry** that staggers the attacker.
- **Blades of Chaos** — long, fast, chained arcs. (The shield is stowed while they are out, as in Ragnarök.)
- **Draupnir Spear** — throw up to three spears, then **Triangle / Y** to detonate them together.
- **Light and heavy runic attacks** for every weapon (**D-pad up / down**), each with a cooldown ring on the HUD.
- **Spartan Rage** — fills as you fight and take hits; **L3 + R3** when full. Hit harder, take half damage, regenerate, and shake the ground on activation.
- **Stun meters and finishers** — build an enemy's stun bar, then press **R3** to grab and execute.
- An over-the-shoulder camera, Elden Ring's own HUD swapped for the Ragnarök one, and **Mimir** with his own subtitles.
- **D-pad right** swaps Axe → Blades → Spear. Solo, offline.
- **Tweakable while you play.** The first launch writes `%LOCALAPPDATA%\GowRing\settings.json`; edit and save it with the game running to change Rage speed, damage the mod deals, HUD size and opacity, volume, Mimir's chattiness and the screen shake. A controls card is shown for the first 25 seconds in the world.

## What you need
- **Elden Ring** on Steam (PC), version 2.7.1.0–2.7.1.1. Melty starts it through ModEngine2 with Easy Anti-Cheat off — stay offline.
- **God of War Ragnarök** on Steam (PC). Its art and sound are read from *your own copy* when you press Play; nothing from either game is included in this download.

## Good to know (alpha)
- It plays on its **own save file** — your Elden Ring saves are not used, and a backup of them is kept anyway.
- Kratos' *moves* are Elden Ring's own animations for now, and his weapons and body use Elden Ring's models: Ragnarök's PC animation and model formats are not readable yet. Runics and finishers do their damage and effects without a dedicated animation.
- The HUD *layout* and Mimir's timing are in; Ragnarök's own HUD art and Mimir's recorded lines appear only where the converter can find them in your install, and flat stand-in shapes are drawn (with an "ALPHA" note on screen) where it cannot yet.
- **How tested:** Melty's test passed 3 of 3 five-minute runs of version 0.1.11 on Elden Ring 2.7.1.0 (the mod loads, plays on its own save, equips the whole kit on a new character, hides Elden Ring's HUD, draws the Ragnarök HUD layout and runs the combat rules without a crash). Later versions, this one included, have been built and played in short sessions but have not had that full three-run test again. Seen working in real play: the loadout, the HUD overlay, the **perfect parry**, holding the aim crosshair, **axe throw and recall** (a recalled axe flies back and is caught), spear throws, weapon swaps, runics, and mod-dealt damage that killed enemies and the Chapel boss. Spartan Rage filling from ordinary hits, finishers and the Blades' own moves are **not yet confirmed in the real game**; if something is missing or wrong, `STATUS.md` in the mod folder lists exactly what, and `%LOCALAPPDATA%\GowRing\logs\gowring.log` says what happened.

## Credits
- **ModEngine2** (soulsmods, MIT) loads the mod into Elden Ring offline; it is shipped unmodified, with its licence.
- **fromsoftware-rs** (`eldenring` crate, vswarte and contributors, MIT or Apache-2.0) gives the mod Elden Ring's structures and per-frame hooks.
- **hudhook** and **Dear ImGui** draw the HUD; **retour**, **rodio**, **png** and other Rust crates (MIT / Apache-2.0) do the rest.
- **vgmstream** (ISC) converts Wwise audio from *your own* God of War Ragnarök install; **gowr_soundtool** by iArtorias (MIT) documents the audio-pack layout the reader follows.
- Microsoft's Visual C++ runtime DLLs are included app-local, as Microsoft allows.
- God of War Ragnarök, Kratos, Mimir and their weapons belong to Sony Interactive Entertainment / Santa Monica Studio; Elden Ring belongs to FromSoftware / Bandai Namco. This is an unofficial fan project and ships no files from either game.
- Built with AI (Claude Code, Anthropic).

*Built with AI (Claude). Unofficial fan project; Elden Ring and God of War Ragnarök belong to their owners.*
