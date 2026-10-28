//! Player settings: `%LOCALAPPDATA%\GowRing\settings.json`. Created with the defaults on first start and re-read
//! about every second while the game runs, so a value can be changed in a text editor and seen straight away.
//! Unknown keys are ignored, a missing or wrong-typed key keeps its default, and a file that does not parse
//! leaves the last good settings in place (and says so in gowring.log).

use std::path::{Path, PathBuf};
use std::sync::{OnceLock, RwLock};
use std::time::SystemTime;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Settings {
    /// Every point of Spartan Rage earned is multiplied by this (0 = never fills, 2 = twice as fast).
    pub rage_gain_multiplier: f32,
    /// Multiplies the damage the mod itself deals (axe and spear throws, runics, finishers, Rage burst).
    pub mod_damage_multiplier: f32,
    pub hud_scale: f32,
    pub hud_opacity: f32,
    pub sfx_volume: f32,
    pub stand_in_sounds: bool,
    pub mimir_subtitles: bool,
    /// Multiplies how often Mimir speaks (0 = silent, 2 = twice as often).
    pub mimir_frequency: f32,
    pub camera_shake: f32,
    pub effect_rings: bool,
    /// How long the controls panel stays up after you enter the world (0 = never show it).
    pub controls_hint_seconds: f32,
    /// Regular enemies get this many times their normal HP when they first come into range (bosses are left alone).
    pub enemy_health_multiplier: f32,
    /// A regular enemy worn down to a quarter of its health is staggered once, so you can finish it with R3.
    pub finisher_when_wounded: bool,
    /// Testing aids (the summon key) are on.
    pub debug_tools: bool,
    /// The function key (1-12 for F1-F12) that summons a tough enemy.
    pub debug_spawn_key: u8,
    /// The function key that fills Spartan Rage.
    pub debug_rage_key: u8,
    /// The function key that steps the close-up camera (chest, nearer, waist, off) for looking at Kratos's model.
    pub debug_camera_key: u8,
    /// The function key that cycles Kratos's own moves (see `moves`): his moves, them with the game's arms, Elden
    /// Ring's rest pose (a check), the game's pose.
    pub debug_pose_key: u8,
    /// The function key that switches the cape's swing off and on while playing (off: the cloak as built, carried by
    /// his bones), to compare the two.
    pub debug_cape_key: u8,
    /// Kratos's cloak swings and blows in the wind (the launcher skins it to the bones the DLL moves; applies the next
    /// time Play is pressed).
    pub cape_physics: bool,
    /// How hard the wind blows on the cloak (0 = no wind; his own movement still swings it).
    pub cape_wind: f32,
    /// Enemies drop health stones that heal Kratos when he walks over them (Ragnarök's way of healing).
    pub health_stones: bool,
    /// Health given to a summoned enemy.
    pub debug_enemy_health: f32,
    /// How much harder a summoned enemy hits than its original.
    pub debug_enemy_damage_multiplier: f32,
    /// The runic (id) in each weapon's light and heavy slot, in the order of `RUNIC_SLOTS`.
    pub runics: [&'static str; 6],
    /// Kratos wears the Black Bear Set (the fur cloak, black leggings, boots and belt) instead of the Survival Set.
    /// Read by the launcher when it builds his body part, so it applies from the next Play.
    pub black_bear_outfit: bool,
    /// The fur on cards (the bear cloak, the harness's tufts) is drawn with Elden Ring's hair material instead of the
    /// cut-out one. Read by the launcher when it builds his body part, so it applies from the next Play.
    pub hair_fur: bool,
    /// Kratos holds his right-hand weapon in both hands, and the shield comes out (one-handed) only while the guard
    /// button is held.
    pub two_hand_by_default: bool,
    /// While Kratos two-hands, the Guardian Shield is put away (his left hand switched to an empty weapon slot) instead
    /// of hanging on his back; it comes out with the guard.
    pub hide_stowed_shield: bool,
}

/// The six runic slots: (settings key, weapon, heavy?, the weapon's own pick).
pub const RUNIC_SLOTS: [(&str, &str, bool, &str); 6] = [
    ("axe_light_runic", "leviathan_axe", false, "axe_runic_light"),
    ("axe_heavy_runic", "leviathan_axe", true, "axe_runic_heavy"),
    ("blades_light_runic", "blades_of_chaos", false, "blades_runic_light"),
    ("blades_heavy_runic", "blades_of_chaos", true, "blades_runic_heavy"),
    ("spear_light_runic", "draupnir_spear", false, "spear_runic_light"),
    ("spear_heavy_runic", "draupnir_spear", true, "spear_runic_heavy"),
];
const OWN_RUNICS: [&str; 6] = [RUNIC_SLOTS[0].3, RUNIC_SLOTS[1].3, RUNIC_SLOTS[2].3, RUNIC_SLOTS[3].3, RUNIC_SLOTS[4].3, RUNIC_SLOTS[5].3];

/// The runics that fit a slot, in the sheet's order.
fn slot_options(slot: usize) -> impl Iterator<Item = &'static gowring_core::sheets::RunicDef> {
    let (_, weapon, heavy, _) = RUNIC_SLOTS[slot];
    gowring_core::sheets::RUNICS.iter().filter(move |r| r.weapon == weapon && (r.tier == gowring_core::sheets::RunicTier::Heavy) == heavy)
}

/// A runic named in the file (its name or id, any case) that fits the slot.
fn runic_for(slot: usize, text: &str) -> Option<&'static str> {
    let t = text.trim();
    slot_options(slot).find(|r| r.id.eq_ignore_ascii_case(t) || r.name.eq_ignore_ascii_case(t)).map(|r| r.id)
}

fn runic_name(id: &str) -> &'static str {
    gowring_core::sheets::RUNICS.iter().find(|r| r.id == id).map(|r| r.name).unwrap_or("")
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            rage_gain_multiplier: 1.0,
            mod_damage_multiplier: 1.0,
            hud_scale: 1.0,
            hud_opacity: 1.0,
            sfx_volume: 1.0,
            stand_in_sounds: true,
            mimir_subtitles: true,
            mimir_frequency: 1.0,
            camera_shake: 1.0,
            effect_rings: true,
            controls_hint_seconds: 25.0,
            enemy_health_multiplier: 3.0,
            finisher_when_wounded: true,
            debug_tools: true,
            debug_spawn_key: 9,
            debug_rage_key: 10,
            debug_camera_key: 11,
            debug_pose_key: 8,
            debug_cape_key: 7,
            cape_physics: true,
            cape_wind: 1.0,
            health_stones: true,
            debug_enemy_health: 20000.0,
            debug_enemy_damage_multiplier: 3.5,
            runics: OWN_RUNICS,
            black_bear_outfit: true,
            hair_fur: true,
            two_hand_by_default: true,
            hide_stowed_shield: true,
        }
    }
}

/// Written into the file so a settings file from an older build can be recognised and brought forward.
const FILE_VERSION: u32 = 21;

/// The settings file's text for these values, with a plain-language note under every key.
fn render(s: &Settings) -> String {
    format!(
        r#"{{
  "settings_version": {ver},
  "_about": "Kratos in the Lands Between settings. Save this file while the game is running and the change applies within a second or two. Delete a line to get its default back; delete the whole file to start over.",

  "rage_gain_multiplier": {rage:?},
  "_rage_gain_multiplier": "Spartan Rage earned per hit, throw, parry and so on. 1.0 is the tuned speed; 0.5 = half as fast, 2 = twice as fast, 0 = it never fills.",

  "mod_damage_multiplier": {dmg:?},
  "_mod_damage_multiplier": "Damage the mod itself deals: axe and spear throws, runics, finishers, the Rage burst. Elden Ring's own weapon hits are not scaled.",

  "enemy_health_multiplier": {ehp:?},
  "_enemy_health_multiplier": "Regular enemies get this many times their normal health when they first come into range, so a fight lasts long enough to stun them. 1.0 = Elden Ring's own health. Bosses and mini-bosses are never changed. Applies to enemies you meet after the change.",

  "finisher_when_wounded": {wounded},
  "_finisher_when_wounded": "A regular enemy worn down to a quarter of its health is staggered once, so you can finish it with R3. false = only a full stun bar stuns.",

  "health_stones": {stones},
  "_health_stones": "Enemies drop green health stones, as in Ragnarok: walk over one to heal (only when hurt). Your flask still works with the use-item button. false = no stones.",

  "black_bear_outfit": {bear},
  "_black_bear_outfit": "Kratos wears the Black Bear Set: the bear fur cloak, with his leggings, boots and belt in black. false = the Survival Set with the leather cloak. Applies the next time you press Play.",

  "hair_fur": {hair},
  "_hair_fur": "true = the fur of Kratos's bear cloak (and the tufts around his shoulder pad) is drawn with Elden Ring's own hair material, for softer strands lit like hair (new and still being tried). false = the cut-out material it had before. Applies the next time you press Play.",

  "two_hand_by_default": {two_hand},
  "_two_hand_by_default": "Kratos holds his weapon in both hands, and the Guardian Shield comes up only while you hold the guard button (L1); let go and he two-hands again. false = Elden Ring's own one-handed stance and two-hand toggle.",

  "hide_stowed_shield": {hide_shield},
  "_hide_stowed_shield": "While Kratos two-hands, the Guardian Shield is put away instead of hanging on his back, and comes out when you hold L1. It works by switching his left hand to an empty weapon slot, so keep one left slot empty. false = the shield hangs on his back.",
{runic_lines}
  "hud_scale": {scale:?},
  "_hud_scale": "HUD size. 1.0 = normal, 1.3 = bigger, 0.8 = smaller (0.4 to 3.0).",

  "hud_opacity": {opacity:?},
  "_hud_opacity": "0.0 invisible to 1.0 solid.",

  "sfx_volume": {sfx:?},
  "_sfx_volume": "Volume of the mod's sound effects, 0.0 to 2.0.",

  "stand_in_sounds": {stand},
  "_stand_in_sounds": "Generated stand-in sounds used while the real Ragnarok sounds are not available. false = silence instead.",

  "mimir_subtitles": {subs},
  "_mimir_subtitles": "Mimir's stand-in subtitle lines (shown while no recording is available).",

  "mimir_frequency": {mimir:?},
  "_mimir_frequency": "How often Mimir speaks. 0 = never, 0.5 = half as often, 3 = three times as often.",

  "camera_shake": {shake:?},
  "_camera_shake": "Strength of the screen shake on Rage and spear blasts. 0 = none.",

  "effect_rings": {rings},
  "_effect_rings": "The expanding rings for runics, spear blasts and the Rage burst.",

  "controls_hint_seconds": {hint:?},
  "_controls_hint_seconds": "How long the controls panel stays on screen when you enter the world. 0 = never.",

  "debug_tools": {dbg},
  "_debug_tools": "Testing aids. While true, the summon key below works. false = off.",

  "debug_spawn_key": "F{dkey}",
  "_debug_spawn_key": "Keyboard key (F1 to F12) that summons a tough copy of a nearby enemy 5 metres in front of you, so a fight is one key away.",

  "debug_rage_key": "F{rkey}",
  "_debug_rage_key": "Keyboard key (F1 to F12) that fills Spartan Rage at once, for trying it out.",

  "debug_camera_key": "F{ckey}",
  "_debug_camera_key": "Keyboard key (F1 to F12) that brings the camera close to Kratos, for looking at his model: each press steps chest, nearer, waist, then back to the normal camera. Turn it with the camera stick as usual.",

  "debug_pose_key": "F{pkey}",
  "_debug_pose_key": "Keyboard key (F1 to F12) that cycles Kratos's own moves: his Ragnarök idle, walk, run and sprint; the same with the game's arms (so the weapon stays in his hands); Elden Ring's plain rest pose (a check for testing); and the game's own animation.",

  "debug_cape_key": "F{cape_key}",
  "_debug_cape_key": "Keyboard key (F1 to F12) that switches the swing of Kratos's cloak off and on while you play: off, the cloak hangs as it was built and only his back and hips carry it, so you can compare the two.",

  "cape_physics": {cape},
  "_cape_physics": "true = the back of Kratos's cloak swings as he moves and blows in the wind (new: it swings with his running and turning, and his back and hips carry it). false = it stays on his back as before. Applies the next time you press Play.",

  "cape_wind": {cwind:?},
  "_cape_wind": "How hard the wind blows on his cloak (0 to 3). 0 = no wind (it still swings as he moves).",

  "debug_enemy_health": {dhp:?},
  "_debug_enemy_health": "Health of a summoned enemy (100 to 100000). Summons are treated as bosses: the big health bar, a boss stun bar, and finishers that take a slice instead of killing.",

  "debug_enemy_damage_multiplier": {ddm:?},
  "_debug_enemy_damage_multiplier": "How much harder a summoned enemy hits than the enemy it copies (1 to 10). The extra damage never kills on its own."
}}
"#,
        ver = FILE_VERSION,
        rage = s.rage_gain_multiplier,
        dmg = s.mod_damage_multiplier,
        ehp = s.enemy_health_multiplier,
        wounded = s.finisher_when_wounded,
        scale = s.hud_scale,
        opacity = s.hud_opacity,
        sfx = s.sfx_volume,
        stand = s.stand_in_sounds,
        subs = s.mimir_subtitles,
        mimir = s.mimir_frequency,
        shake = s.camera_shake,
        rings = s.effect_rings,
        hint = s.controls_hint_seconds,
        dbg = s.debug_tools,
        dkey = s.debug_spawn_key,
        rkey = s.debug_rage_key,
        ckey = s.debug_camera_key,
        pkey = s.debug_pose_key,
        cape_key = s.debug_cape_key,
        cape = s.cape_physics,
        cwind = s.cape_wind,
        stones = s.health_stones,
        dhp = s.debug_enemy_health,
        ddm = s.debug_enemy_damage_multiplier,
        bear = s.black_bear_outfit,
        hair = s.hair_fur,
        two_hand = s.two_hand_by_default,
        hide_shield = s.hide_stowed_shield,
        runic_lines = runic_lines(s),
    )
}

/// The runic slots in the file: the equipped runic's name, and what else fits.
fn runic_lines(s: &Settings) -> String {
    let mut o = String::new();
    for (i, (key, weapon, heavy, _)) in RUNIC_SLOTS.iter().enumerate() {
        let names: Vec<&str> = slot_options(i).map(|r| r.name).collect();
        let wname = gowring_core::sheets::WEAPONS.iter().find(|w| w.id == *weapon).map(|w| w.name).unwrap_or(weapon);
        let (tier, pad) = if *heavy { ("heavy", "down") } else { ("light", "up") };
        o.push_str(&format!(
            "\n  \"{key}\": \"{}\",\n  \"_{key}\": \"The {wname}'s {tier} runic (D-pad {pad}). One of: {}. In game, hold L1 and press D-pad {pad} to go to the next one; the choice is saved here.\",\n",
            runic_name(s.runics[i]),
            names.join(", ")
        ));
    }
    o
}

/// Put a runic (by id) in its slot, as chosen in game, and save the file. Unknown ids are ignored.
pub fn set_runic(id: &str) {
    let Some((slot, new)) = (0..RUNIC_SLOTS.len()).find_map(|i| runic_for(i, id).map(|r| (i, r))) else { return };
    let mut s = get();
    if s.runics[slot] == new {
        return;
    }
    s.runics[slot] = new;
    if let Ok(mut g) = CURRENT.write() {
        *g = s;
    }
    if let Some(path) = PATH.get() {
        match std::fs::write(path, render(&s)) {
            Ok(()) => crate::glog!("settings", "{} = {} (chosen in game, saved)", RUNIC_SLOTS[slot].0, runic_name(id)),
            Err(e) => crate::glog!("settings", "could not save the runic choice to {}: {e}", path.display()),
        }
    }
}

static CURRENT: RwLock<Settings> = RwLock::new(Settings {
    rage_gain_multiplier: 1.0,
    mod_damage_multiplier: 1.0,
    hud_scale: 1.0,
    hud_opacity: 1.0,
    sfx_volume: 1.0,
    stand_in_sounds: true,
    mimir_subtitles: true,
    mimir_frequency: 1.0,
    camera_shake: 1.0,
    effect_rings: true,
    controls_hint_seconds: 25.0,
    enemy_health_multiplier: 3.0,
    finisher_when_wounded: true,
    debug_tools: true,
    debug_spawn_key: 9,
    debug_rage_key: 10,
    debug_camera_key: 11,
    debug_pose_key: 8,
    debug_cape_key: 7,
    cape_physics: true,
    cape_wind: 1.0,
    health_stones: true,
    debug_enemy_health: 20000.0,
    debug_enemy_damage_multiplier: 3.5,
    runics: OWN_RUNICS,
    black_bear_outfit: true,
    hair_fur: true,
    two_hand_by_default: true,
    hide_stowed_shield: true,
});
static PATH: OnceLock<PathBuf> = OnceLock::new();

/// The settings right now (a cheap copy).
pub fn get() -> Settings {
    CURRENT.read().map(|g| *g).unwrap_or_default()
}

fn num(v: &serde_json::Value, key: &str, lo: f32, hi: f32, into: &mut f32) {
    if let Some(x) = v.get(key).and_then(|x| x.as_f64()) {
        *into = (x as f32).clamp(lo, hi);
    }
}

fn flag(v: &serde_json::Value, key: &str, into: &mut bool) {
    if let Some(x) = v.get(key).and_then(|x| x.as_bool()) {
        *into = x;
    }
}

/// "F9" -> 9 (F1 to F12, any case).
fn fkey(name: &str) -> Option<u8> {
    let t = name.trim();
    let n: u8 = t.strip_prefix('F').or_else(|| t.strip_prefix('f'))?.parse().ok()?;
    (1..=12).contains(&n).then_some(n)
}

/// Parse a settings file's text; every missing key keeps its default.
pub fn parse(text: &str) -> Result<Settings, String> {
    let v: serde_json::Value = serde_json::from_str(text).map_err(|e| e.to_string())?;
    if !v.is_object() {
        return Err("the file must contain one JSON object { ... }".into());
    }
    let mut s = Settings::default();
    num(&v, "rage_gain_multiplier", 0.0, 10.0, &mut s.rage_gain_multiplier);
    num(&v, "mod_damage_multiplier", 0.0, 20.0, &mut s.mod_damage_multiplier);
    num(&v, "hud_scale", 0.4, 3.0, &mut s.hud_scale);
    num(&v, "hud_opacity", 0.0, 1.0, &mut s.hud_opacity);
    num(&v, "sfx_volume", 0.0, 2.0, &mut s.sfx_volume);
    flag(&v, "stand_in_sounds", &mut s.stand_in_sounds);
    flag(&v, "mimir_subtitles", &mut s.mimir_subtitles);
    num(&v, "mimir_frequency", 0.0, 10.0, &mut s.mimir_frequency);
    num(&v, "camera_shake", 0.0, 3.0, &mut s.camera_shake);
    flag(&v, "effect_rings", &mut s.effect_rings);
    num(&v, "controls_hint_seconds", 0.0, 600.0, &mut s.controls_hint_seconds);
    num(&v, "enemy_health_multiplier", 0.25, 20.0, &mut s.enemy_health_multiplier);
    flag(&v, "finisher_when_wounded", &mut s.finisher_when_wounded);
    flag(&v, "debug_tools", &mut s.debug_tools);
    if let Some(k) = v.get("debug_spawn_key").and_then(|x| x.as_str()).and_then(fkey) {
        s.debug_spawn_key = k;
    }
    if let Some(k) = v.get("debug_rage_key").and_then(|x| x.as_str()).and_then(fkey) {
        s.debug_rage_key = k;
    }
    if let Some(k) = v.get("debug_camera_key").and_then(|x| x.as_str()).and_then(fkey) {
        s.debug_camera_key = k;
    }
    if let Some(k) = v.get("debug_pose_key").and_then(|x| x.as_str()).and_then(fkey) {
        s.debug_pose_key = k;
    }
    if let Some(k) = v.get("debug_cape_key").and_then(|x| x.as_str()).and_then(fkey) {
        s.debug_cape_key = k;
    }
    if let Some(b) = v.get("cape_physics").and_then(|x| x.as_bool()) {
        s.cape_physics = b;
    }
    if let Some(w) = v.get("cape_wind").and_then(|x| x.as_f64()) {
        s.cape_wind = (w as f32).clamp(0.0, 3.0);
    }
    flag(&v, "health_stones", &mut s.health_stones);
    flag(&v, "black_bear_outfit", &mut s.black_bear_outfit);
    flag(&v, "hair_fur", &mut s.hair_fur);
    flag(&v, "two_hand_by_default", &mut s.two_hand_by_default);
    flag(&v, "hide_stowed_shield", &mut s.hide_stowed_shield);
    num(&v, "debug_enemy_health", 100.0, 100000.0, &mut s.debug_enemy_health);
    num(&v, "debug_enemy_damage_multiplier", 1.0, 10.0, &mut s.debug_enemy_damage_multiplier);
    for (i, (key, ..)) in RUNIC_SLOTS.iter().enumerate() {
        if let Some(id) = v.get(*key).and_then(|x| x.as_str()).and_then(|t| runic_for(i, t)) {
            s.runics[i] = id;
        }
    }
    Ok(s)
}

fn mtime(p: &Path) -> Option<SystemTime> {
    std::fs::metadata(p).and_then(|m| m.modified()).ok()
}

fn reload(path: &Path, first: bool) {
    match std::fs::read_to_string(path) {
        Ok(text) => match parse(&text) {
            Ok(new) => {
                let old = get();
                if let Ok(mut g) = CURRENT.write() {
                    *g = new;
                }
                if first {
                    crate::glog!("settings", "loaded {}: {:?}", path.display(), new);
                } else if old != new {
                    crate::glog!("settings", "reloaded: {:?}", new);
                }
            }
            Err(e) => crate::glog!("settings", "could not read {} ({e}); keeping the last good settings", path.display()),
        },
        Err(e) => crate::glog!("settings", "could not open {}: {e}", path.display()),
    }
}

/// What a settings file from an older build becomes: every value kept, new keys shown with their defaults, the notes
/// brought up to date, and (from version 1) the Rage multiplier divided by 3 (Rage gain was tuned up threefold in the
/// rules in version 2, so the same play feels the same).
/// `None` when the text is already current or is not a settings file (a file that does not parse is left for its owner).
fn migrated_text(text: &str) -> Option<String> {
    let v: serde_json::Value = serde_json::from_str(text).ok()?;
    let version = v.get("settings_version").and_then(|x| x.as_u64()).unwrap_or(1);
    if !v.is_object() || version >= FILE_VERSION as u64 {
        return None;
    }
    let mut s = parse(text).ok()?;
    if version < 2 && v.get("rage_gain_multiplier").and_then(|x| x.as_f64()).is_some() {
        s.rage_gain_multiplier = ((s.rage_gain_multiplier / 3.0) * 100.0).round() / 100.0;
    }
    // version 4: summoned enemies went from 3000 to 8000 health (a summon should outlast a Rage); keep a changed value
    if version < 4 && (s.debug_enemy_health - 3000.0).abs() < 0.5 {
        s.debug_enemy_health = Settings::default().debug_enemy_health;
    }
    // version 5: summons became bosses with 12000 health (8000 was "not that strong")
    if version < 5 && (s.debug_enemy_health - 8000.0).abs() < 0.5 {
        s.debug_enemy_health = Settings::default().debug_enemy_health;
    }
    // version 6: "the boss is still a bit easy" at 12000 / x2.5
    if version < 6 && (s.debug_enemy_health - 12000.0).abs() < 0.5 {
        s.debug_enemy_health = Settings::default().debug_enemy_health;
    }
    if version < 6 && (s.debug_enemy_damage_multiplier - 2.5).abs() < 0.01 {
        s.debug_enemy_damage_multiplier = Settings::default().debug_enemy_damage_multiplier;
    }
    // version 14: the cape went from off (experimental, 0.1.68-0.1.69 wrote "cape_physics": false into every file) to on
    if version < 14 {
        s.cape_physics = true;
    }
    // versions 3, 7-13 and 15 only added keys (7: the runic slots; 8: the Rage key and health stones; 9: the outfit and
    // two-handing; 10: the stowed shield; 11: the close-up camera key; 12: the pose probe key; 13: the cape; 15: the
    // hair material for the fur; 16: the cape key; 17-21 only rewrote the pose key's note: the pose search, the pose check, Kratos's idle, what standing still shows, his moves):
    // re-rendering the file shows them with their notes
    Some(render(&s))
}

fn migrate(path: &Path) {
    let Ok(text) = std::fs::read_to_string(path) else { return };
    let Some(new_text) = migrated_text(&text) else { return };
    let old_version = serde_json::from_str::<serde_json::Value>(&text).ok().and_then(|v| v.get("settings_version").and_then(|x| x.as_u64())).unwrap_or(1);
    let backup = path.with_file_name(format!("settings.v{old_version}.json"));
    let _ = std::fs::write(&backup, &text);
    match std::fs::write(path, new_text) {
        Ok(()) => crate::glog!("settings", "brought {} up to date (the old one is kept as {})", path.display(), backup.display()),
        Err(e) => crate::glog!("settings", "could not update {}: {e}", path.display()),
    }
}

/// Read (and on first run create) the settings file and watch it for edits.
pub fn init(local_dir: &Path) {
    let path = local_dir.join("settings.json");
    let _ = PATH.set(path.clone());
    if !path.exists() {
        let _ = std::fs::create_dir_all(local_dir);
        match std::fs::write(&path, render(&Settings::default())) {
            Ok(()) => crate::glog!("settings", "created {} with the defaults", path.display()),
            Err(e) => crate::glog!("settings", "could not create {}: {e}", path.display()),
        }
    }
    migrate(&path);
    reload(&path, true);
    let _ = std::thread::Builder::new().name("gowring-settings".into()).spawn(move || {
        let mut seen = mtime(&path);
        loop {
            std::thread::sleep(std::time::Duration::from_millis(1000));
            let now = mtime(&path);
            if now != seen {
                seen = now;
                if now.is_some() {
                    reload(&path, false);
                }
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_default_file_parses_to_the_defaults() {
        assert_eq!(parse(&render(&Settings::default())).unwrap(), Settings::default());
    }

    #[test]
    fn partial_files_keep_defaults_and_values_are_clamped() {
        let s = parse(r#"{ "rage_gain_multiplier": 0.5, "hud_scale": 99, "mimir_subtitles": false, "sfx_volume": "loud", "unknown": 1 }"#).unwrap();
        assert_eq!(s.rage_gain_multiplier, 0.5);
        assert_eq!(s.hud_scale, 3.0);
        assert!(!s.mimir_subtitles);
        assert_eq!(s.sfx_volume, 1.0, "a wrong-typed value keeps its default");
        assert_eq!(s.mod_damage_multiplier, 1.0);
    }

    #[test]
    fn broken_files_are_errors_not_panics() {
        assert!(parse("{ not json").is_err());
        assert!(parse("[1, 2]").is_err());
        assert!(parse("").is_err());
    }

    #[test]
    fn every_value_survives_a_round_trip_through_the_file_text() {
        let s = Settings {
            rage_gain_multiplier: 0.5,
            mod_damage_multiplier: 2.5,
            hud_scale: 1.3,
            hud_opacity: 0.8,
            sfx_volume: 0.25,
            stand_in_sounds: false,
            mimir_subtitles: false,
            mimir_frequency: 3.0,
            camera_shake: 0.0,
            effect_rings: false,
            controls_hint_seconds: 0.0,
            enemy_health_multiplier: 5.0,
            finisher_when_wounded: false,
            debug_tools: false,
            debug_spawn_key: 11,
            debug_rage_key: 12,
            debug_camera_key: 8,
            debug_pose_key: 7,
            debug_cape_key: 6,
            cape_physics: false,
            cape_wind: 2.5,
            health_stones: false,
            debug_enemy_health: 5000.0,
            debug_enemy_damage_multiplier: 4.0,
            runics: ["axe_ice_lance", "axe_frostfall", "blades_runic_light", "blades_scorched_arc", "spear_thunder_step", "spear_runic_heavy"],
            black_bear_outfit: false,
            hair_fur: false,
            two_hand_by_default: false,
            hide_stowed_shield: false,
        };
        assert_eq!(parse(&render(&s)).unwrap(), s);
    }

    #[test]
    fn an_old_settings_file_is_brought_forward_once() {
        let old = r#"{ "rage_gain_multiplier": 3.0, "hud_scale": 1.5, "mimir_frequency": 0.5 }"#;
        let new = migrated_text(old).expect("old file is migrated");
        let s = parse(&new).unwrap();
        assert_eq!(s.rage_gain_multiplier, 1.0, "3x of the old rate is 1x of the new");
        assert_eq!(s.hud_scale, 1.5);
        assert_eq!(s.mimir_frequency, 0.5);
        assert_eq!(s.enemy_health_multiplier, 3.0);
        assert!(migrated_text(&new).is_none(), "a current file is left alone");
        // a file that never mentioned Rage keeps the default
        let s = parse(&migrated_text(r#"{ "hud_scale": 1.2 }"#).unwrap()).unwrap();
        assert_eq!(s.rage_gain_multiplier, 1.0);
        // not JSON: left for the owner to fix
        assert!(migrated_text("{ nope").is_none());
    }

    #[test]
    fn a_version_2_file_keeps_its_rage_and_gains_the_new_keys() {
        let v2 = r#"{ "settings_version": 2, "rage_gain_multiplier": 2.0, "hud_scale": 1.2 }"#;
        let s = parse(&migrated_text(v2).expect("v2 is brought forward")).unwrap();
        assert_eq!(s.rage_gain_multiplier, 2.0, "only version 1 files had Rage divided");
        assert_eq!(s.hud_scale, 1.2);
        assert!(s.debug_tools);
        assert_eq!(s.debug_spawn_key, 9);
    }

    #[test]
    fn a_version_13_file_turns_the_cape_on_once() {
        let s = parse(&migrated_text(r#"{ "settings_version": 13, "cape_physics": false, "cape_wind": 2.0 }"#).unwrap()).unwrap();
        assert!(s.cape_physics, "0.1.68-0.1.69 wrote false while the cape was experimental");
        assert_eq!(s.cape_wind, 2.0);
        let mut off = s.clone();
        off.cape_physics = false;
        assert!(migrated_text(&render(&off)).is_none(), "turned off again in a current file, it stays off");
    }

    #[test]
    fn function_keys_parse_and_others_keep_the_default() {
        assert_eq!(parse(r#"{ "debug_spawn_key": "f5" }"#).unwrap().debug_spawn_key, 5);
        assert_eq!(parse(r#"{ "debug_spawn_key": "F13" }"#).unwrap().debug_spawn_key, 9);
        assert_eq!(parse(r#"{ "debug_spawn_key": "Q" }"#).unwrap().debug_spawn_key, 9);
    }

    #[test]
    fn a_version_3_file_gets_the_tougher_summon_unless_it_was_changed() {
        let s = parse(&migrated_text(r#"{ "settings_version": 3, "debug_enemy_health": 3000.0 }"#).unwrap()).unwrap();
        assert_eq!(s.debug_enemy_health, 20000.0, "3000 -> 8000 -> 12000 -> 20000 across versions 4 to 6");
        let s = parse(&migrated_text(r#"{ "settings_version": 4, "debug_enemy_health": 8000.0 }"#).unwrap()).unwrap();
        assert_eq!(s.debug_enemy_health, 20000.0);
        let s = parse(&migrated_text(r#"{ "settings_version": 5, "debug_enemy_health": 12000.0, "debug_enemy_damage_multiplier": 2.5 }"#).unwrap()).unwrap();
        assert_eq!((s.debug_enemy_health, s.debug_enemy_damage_multiplier), (20000.0, 3.5));
        let s = parse(&migrated_text(r#"{ "settings_version": 5, "debug_enemy_health": 15000.0, "debug_enemy_damage_multiplier": 2.0 }"#).unwrap()).unwrap();
        assert_eq!((s.debug_enemy_health, s.debug_enemy_damage_multiplier), (15000.0, 2.0), "the player's own values stay");
        let s = parse(&migrated_text(r#"{ "settings_version": 3, "debug_enemy_health": 4500.0 }"#).unwrap()).unwrap();
        assert_eq!(s.debug_enemy_health, 4500.0, "a value the player chose is kept");
    }

    #[test]
    fn runic_slots_take_names_or_ids_and_only_runics_that_fit() {
        // the defaults are each weapon's own picks
        for (i, (_, weapon, heavy, own)) in RUNIC_SLOTS.iter().enumerate() {
            let w = gowring_core::sheets::WEAPONS.iter().find(|w| w.id == *weapon).unwrap();
            assert_eq!(Some(*own), if *heavy { w.heavy_runic } else { w.light_runic });
            assert_eq!(Settings::default().runics[i], *own);
        }
        let s = parse(r#"{ "axe_light_runic": "rime ring", "axe_heavy_runic": "axe_frostfall", "blades_light_runic": "Sky Lance", "spear_heavy_runic": "nonsense" }"#).unwrap();
        assert_eq!(s.runics[0], "axe_rime_ring", "by name, any case");
        assert_eq!(s.runics[1], "axe_frostfall", "by id");
        assert_eq!(s.runics[2], "blades_runic_light", "a spear runic does not fit a Blades slot");
        assert_eq!(s.runics[5], "spear_runic_heavy", "an unknown name keeps the default");
        // the file lists the choices for each slot
        let text = render(&Settings::default());
        assert!(text.contains("\"axe_light_runic\": \"Frost Shockwave\""), "{text}");
        assert!(text.contains("One of: Frost Shockwave, Rime Ring, Ice Lance."));
        // a version 6 file gains the runic slots and keeps its values
        let v6 = r#"{ "settings_version": 6, "hud_scale": 1.4 }"#;
        let s = parse(&migrated_text(v6).expect("v6 is brought forward")).unwrap();
        assert_eq!((s.hud_scale, s.runics), (1.4, OWN_RUNICS));
    }
}
