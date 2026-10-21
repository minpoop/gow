//! The bridge between Elden Ring's memory and `gowring-core`: reads the world into a `Frame`
//! (hooks.player_state / enemy_scan / action_read / hit_observe) and applies `Command`s back
//! (hooks.enemy_damage / enemy_stagger / action_mask / speed_scale).
//!
//! Everything here touches live game memory through the `eldenring` crate and runs on the game's task
//! thread. Nothing in this file has been run against the real game yet (docs/STATUS.md).

use crate::audio::Audio;
use eldenring::cs::{CSCamExt, CSCamera, CSFeManImp, CSHavokMan, ChrActions, ChrAsmArmStyle, ChrDebugSpawnRequest, ChrIns, EnemyIns, FieldInsHandle, PlayerIns, WorldChrMan};
use eldenring::position::{HavokPosition, PositionDelta};
use fromsoftware_shared::{FromStatic, Superclass};
use gowring_core::sheets::{ErAction, RunicStatus, RUNICS};
use gowring_core::{ActionBits, Command, DamageEvent, EnemyView, Frame, PlayerView, RayResult, Vec3, VoiceInfo};
use std::collections::{HashMap, HashSet};
use std::path::Path;

/// Map collision filter used by the game's own raycasts (field note: CS2 conversion).
const MAP_RAY_FILTER: u32 = 0x2000058;

pub fn id_of(h: &FieldInsHandle) -> u64 {
    ((h.block_id.0 as u32 as u64) << 32) | h.selector.0 as u64
}

/// ER's action bits → ours, via the `ChrActions` accessors (the tuple field is private).
pub fn bits_from_er(a: &ChrActions) -> ActionBits {
    let mut b = ActionBits::NONE;
    macro_rules! m {
        ($($f:ident => $v:expr),* $(,)?) => { $( if a.$f() { b = b.with($v); } )* };
    }
    m!(r1 => ErAction::R1, r2 => ErAction::R2, l1 => ErAction::L1, l2 => ErAction::L2, action => ErAction::Action,
       sp_move => ErAction::SpMove, jump => ErAction::Jump, use_item => ErAction::UseItem, switch_form => ErAction::SwitchForm,
       change_weapon_r => ErAction::ChangeWeaponR, change_weapon_l => ErAction::ChangeWeaponL, change_item => ErAction::ChangeItem,
       r3 => ErAction::R3, l3 => ErAction::L3, guard => ErAction::Guard, change_style_r => ErAction::ChangeStyleR, change_style_l => ErAction::ChangeStyleL);
    b
}

fn set_er_bits(a: &mut ChrActions, bits: ActionBits, value: bool) {
    macro_rules! s {
        ($($set:ident => $v:expr),* $(,)?) => { $( if bits.has($v) { a.$set(value); } )* };
    }
    s!(set_r1 => ErAction::R1, set_r2 => ErAction::R2, set_l1 => ErAction::L1, set_l2 => ErAction::L2, set_action => ErAction::Action,
       set_sp_move => ErAction::SpMove, set_jump => ErAction::Jump, set_use_item => ErAction::UseItem, set_switch_form => ErAction::SwitchForm,
       set_change_weapon_r => ErAction::ChangeWeaponR, set_change_weapon_l => ErAction::ChangeWeaponL, set_change_item => ErAction::ChangeItem,
       set_r3 => ErAction::R3, set_l3 => ErAction::L3, set_guard => ErAction::Guard, set_change_style_r => ErAction::ChangeStyleR, set_change_style_l => ErAction::ChangeStyleL);
}

/// The eight ways a yaw angle might map to a ground direction; the right one is picked at run time
/// by watching which mapping agrees with where the character actually walks.
fn facing_candidate(mode: usize, yaw: f32) -> Vec3 {
    let (s, c) = (yaw.sin(), yaw.cos());
    match mode {
        0 => Vec3::new(s, 0.0, c),
        1 => Vec3::new(-s, 0.0, c),
        2 => Vec3::new(s, 0.0, -c),
        3 => Vec3::new(-s, 0.0, -c),
        4 => Vec3::new(c, 0.0, s),
        5 => Vec3::new(-c, 0.0, s),
        6 => Vec3::new(c, 0.0, -s),
        _ => Vec3::new(-c, 0.0, -s),
    }
}

struct Frozen {
    until: f64,
    saved_speed: f32,
}

/// A runic's animation being started: the skill (Ash of War) put on the weapon, when its button went down and for
/// how many frames it has been held.
struct SkillCast {
    /// The runic (or `spartan_rage`) it plays for.
    tag: &'static str,
    skill: i32,
    /// The weapon rows' own skills, put back once this one has played (empty: a runic's skill stays on its weapon).
    restore: Vec<(u32, i32)>,
    /// The animation category the skill's animations are in (600 + its type), when the patch read it.
    category: Option<i32>,
    at: f64,
    frames: u32,
    /// How long the button is held waiting for the animation to start.
    wait: f64,
}

/// How long the skill button is held waiting for the skill's animation, and how many misses in a row make the
/// bridge stop trying a runic's animation for the session.
const SKILL_WAIT_S: f64 = 0.6;
/// Spartan Rage's roar waits longer: in 0.1.37 it was missed while Kratos was still staggered from a hit (about 1.9 s).
const ROAR_WAIT_S: f64 = 2.0;
const SKILL_GIVE_UP: u32 = 3;

/// Whether `id` is one of the skill's animations (aXXX_YYYYYY ids are XXX * 1 000 000 + YYYYYY; a skill's are in
/// category 600 + its animation type, e.g. a664 Thunderbolt, a710 Hoarfrost Stomp).
fn is_skill_anim(id: i32, category: Option<i32>) -> bool {
    match category {
        Some(c) => id / 1_000_000 == c,
        None => (600..1000).contains(&(id / 1_000_000)),
    }
}

struct Burn {
    id: u64,
    until: f64,
    next_tick: f64,
    per_tick: i32,
}

/// How long after the mod last owned D-pad up/down a spell or quick-item switch is still put back.
const DPAD_LOCK_S: f64 = 0.6;

/// How long the shield stays up after the guard button is let go, so a tap does not flick the stance back and forth.
const SHIELD_LINGER_S: f64 = 0.25;
/// Frames in a row the game may undo the stance written before two-handing by default stops for the session.
const ARM_STYLE_GIVE_UP: u32 = 120;
/// Elden Ring's "unarmed" weapon, in an empty hand.
const UNARMED: i32 = 110000;

fn arm_style_name(style: u32) -> &'static str {
    match style {
        0 => "empty-handed",
        1 => "one-handed",
        2 => "left weapon in both hands",
        3 => "right weapon in both hands",
        _ => "unknown",
    }
}

pub struct Bridge {
    /// The six weapon slot param ids last read from the player (for the self-checks).
    pub last_equip: [i32; 6],
    /// Last time the player pressed an attack button (R1/R2): damage to a nearby enemy shortly after counts as the player's.
    last_swing: f64,
    /// Frames L2 was masked, and in how many of them it was still visible in the game's requests (evidence for the log).
    l2_masked: (u32, u32),
    /// The player's current Elden Ring animation id, how many changes were logged, how often each id played, and when
    /// that tally was last written (evidence for choosing Kratos's animations).
    anim_seen: (i32, u32),
    anim_tally: HashMap<i32, u32>,
    anim_tally_at: f64,
    /// The skill animation being started (a runic's or Spartan Rage's roar) and the misses in a row per runic.
    skill_cast: Option<SkillCast>,
    /// Until when the skill button is kept out of the game's queue after a runic's skill started (a skill such as
    /// Thunderbolt strikes again if the button is pressed during it, which made runics come out twice).
    skill_quiet_until: f64,
    skill_misses: HashMap<&'static str, u32>,
    /// Weapon skills to put back (row, skill) and when: after Spartan Rage's roar has played.
    skill_restore: Option<(Vec<(u32, i32)>, f64)>,
    /// The spell and quick-item slots kept while the mod owns D-pad up/down (the game still switches them even with
    /// the actions masked), when the mod last owned them, and how many switches were put back (for the log).
    slot_lock: Option<(i32, i32)>,
    dpad_owned_at: f64,
    slot_restores: u32,
    /// Two-handing by default (see `steer_arm_style`): the stance last written, frames in a row the game had undone it,
    /// writes and undos so far (for the log), whether it stopped for the session, and until when the shield stays up.
    arm_written: Option<u32>,
    arm_undone_run: u32,
    arm_counts: (u32, u32),
    arm_stopped: bool,
    shield_until: f64,
    /// The shield only while it guards (see `steer_left_hand`): whether the left hand was switched to an empty slot, the
    /// slot last written, frames in a row the game undid it, writes and undos, whether it stopped for the session, and
    /// the Guardian Shield's carrier rows.
    shield_hidden: bool,
    left_written: Option<u32>,
    left_undone_run: u32,
    left_counts: (u32, u32),
    left_stopped: bool,
    shield_rows: Vec<i32>,
    prev_player_hp: Option<i32>,
    prev_enemy_hp: HashMap<u64, i32>,
    prev_player_last_hit: u64,
    handles: HashMap<u64, FieldInsHandle>,
    hostile_teams: HashSet<u8>,
    team_report_done: bool,
    frozen: HashMap<u64, Frozen>,
    burning: Vec<Burn>,
    /// Enemies whose health has been scaled up: handle id -> the max HP written (never scaled twice).
    scaled: HashMap<u64, i32>,
    /// A debug summon waiting for the game to create the character.
    pending_spawn: Option<PendingSpawn>,
    /// Summoned enemies still settling in: id -> (health to hold, until when).
    tough: HashMap<u64, (i32, f64)>,
    /// Every enemy summoned this session (never used as a template).
    summoned: HashSet<u64>,
    /// Their handles: the game keeps debug-created characters out of its by-distance list, so the scan adds them.
    summoned_handles: Vec<FieldInsHandle>,
    /// Extra damage owed to the player from summons hitting harder (applied in `apply`, never lethal on its own).
    pending_hurt: i32,
    /// Scaled enemies the game reset to their original health (respawned): (id, max HP to give back).
    rescale: Vec<(u64, i32)>,
    /// Where living summons stand, from the last scan (for the damage multiplier).
    summon_positions: Vec<Vec3>,
    last_ppos: Vec3,
    last_facing: Vec3,
    /// HP we took off enemies ourselves outside the engine's ledger (burn ticks), so it is not read back as melee.
    own_pending: HashMap<u64, i32>,
    my_mask: ActionBits,
    /// Facing calibration: candidate yaw→direction mappings scored against real movement.
    prev_pos: Option<Vec3>,
    face_score: [f32; 8],
    face_samples: u32,
    face_mode: usize,
    last_team_report: f64,
    pub scan_range: f32,
    pub max_enemies: usize,
}

/// A debug summon sent to the game's own character creator, waiting to show up.
struct PendingSpawn {
    at: f64,
    hp: i32,
    before: usize,
    want: Vec3,
    template: String,
}

impl Bridge {
    pub fn new(scan_range: f32) -> Self {
        Bridge {
            last_equip: [0; 6],
            last_swing: f64::NEG_INFINITY,
            l2_masked: (0, 0),
            anim_seen: (i32::MIN, 0),
            anim_tally: HashMap::new(),
            anim_tally_at: 0.0,
            skill_cast: None,
            skill_quiet_until: 0.0,
            skill_misses: HashMap::new(),
            skill_restore: None,
            slot_lock: None,
            dpad_owned_at: f64::NEG_INFINITY,
            slot_restores: 0,
            arm_written: None,
            arm_undone_run: 0,
            arm_counts: (0, 0),
            arm_stopped: false,
            shield_until: f64::NEG_INFINITY,
            shield_hidden: false,
            left_written: None,
            left_undone_run: 0,
            left_counts: (0, 0),
            left_stopped: false,
            shield_rows: Vec::new(),
            prev_player_hp: None,
            prev_enemy_hp: HashMap::new(),
            prev_player_last_hit: u64::MAX,
            handles: HashMap::new(),
            // Seeded guess (see hooks.enemy_scan); extended at run time from who actually hurts the player.
            hostile_teams: [6u8, 7u8].into_iter().collect(),
            team_report_done: false,
            frozen: HashMap::new(),
            burning: Vec::new(),
            scaled: HashMap::new(),
            pending_spawn: None,
            tough: HashMap::new(),
            summoned: HashSet::new(),
            summoned_handles: Vec::new(),
            pending_hurt: 0,
            rescale: Vec::new(),
            summon_positions: Vec::new(),
            last_ppos: Vec3::ZERO,
            last_facing: Vec3::new(0.0, 0.0, 1.0),
            own_pending: HashMap::new(),
            my_mask: ActionBits::NONE,
            prev_pos: None,
            face_score: [0.0; 8],
            face_samples: 0,
            face_mode: 0,
            last_team_report: f64::NEG_INFINITY,
            scan_range,
            max_enemies: 96,
        }
    }

    /// Testing aid: copy the toughest regular enemy the game has loaded and have the game's own debug character
    /// creator put the copy 5 m in front of the player, with `hp` health. Returns a line for the screen.
    pub fn debug_spawn(&mut self, hp: f32, now: f64) -> String {
        if self.pending_spawn.is_some() {
            return "Still summoning the last one".into();
        }
        let Ok(wcm) = (unsafe { WorldChrMan::instance_mut() }) else { return "Not in the world yet".into() };
        let Some(player) = wcm.main_player.as_ref() else { return "Not in the world yet".into() };
        let player_handle = player.chr_ins.field_ins_handle;
        let bosses: HashSet<u64> = unsafe { CSFeManImp::instance() }
            .map(|fe| fe.boss_health_displays.iter().filter(|b| !b.field_ins_handle.is_empty()).map(|b| id_of(&b.field_ins_handle)).collect())
            .unwrap_or_default();
        // (max hp, character id, npc param, think param, chara init param, distance)
        let mut best: Option<(i32, u32, i32, i32, i32, f32)> = None;
        for entry in wcm.chr_inses_by_distance.iter().take(512) {
            let e: &ChrIns = unsafe { entry.chr_ins.as_ref() };
            let id = id_of(&e.field_ins_handle);
            if e.field_ins_handle == player_handle || entry.distance > 250.0 || !self.hostile_teams.contains(&e.team_type) || !ehp_alive(e) {
                continue;
            }
            if bosses.contains(&id) || self.summoned.contains(&id) || e.character_id == 0 || e.npc_param_id <= 0 {
                continue;
            }
            let Some(d) = opt(&e.modules.data) else { continue };
            let Some(enemy) = e.as_subclass::<EnemyIns>() else { continue };
            let cand = (d.max_hp, e.character_id, e.npc_param_id, enemy.npc_think_param, d.chara_init_param_id, entry.distance);
            if best.map(|b| cand.0 > b.0).unwrap_or(true) {
                best = Some(cand);
            }
        }
        let Some((max_hp, chr_id, npc, think, init, dist)) = best else {
            crate::glog!("debug_spawn", "no regular hostile enemy is loaded within 250 m to copy");
            return "No enemy nearby to copy: go where enemies are first".into();
        };
        let dir = {
            let f = self.last_facing.flat();
            if f.len() > 0.01 { f.normalized() } else { Vec3::new(0.0, 0.0, 1.0) }
        };
        let want = self.last_ppos + dir * 5.0 + Vec3::new(0.0, 0.5, 0.0);
        let before = wcm.debug_chr_creator.last_created_chr.map(|p| p.as_ptr() as usize).unwrap_or(0);
        wcm.spawn_debug_character(&ChrDebugSpawnRequest {
            chr_id: chr_id as i32,
            chara_init_param_id: init,
            npc_param_id: npc,
            npc_think_param_id: think,
            event_entity_id: 0,
            talk_id: 0,
            pos_x: want.x,
            pos_y: want.y,
            pos_z: want.z,
        });
        let template = format!("c{chr_id:04} (npc {npc}, think {think}, init {init}, {max_hp} hp, {dist:.0} m away)");
        crate::glog!("debug_spawn", "asked the game for a copy of {template} at ({:.1},{:.1},{:.1})", want.x, want.y, want.z);
        self.pending_spawn = Some(PendingSpawn { at: now, hp: hp.round().clamp(100.0, 100_000.0) as i32, before, want, template });
        "Summoning a tough enemy...".into()
    }

    /// Follow up a debug summon: find the new character, give it its health, report where it landed.
    fn settle_debug_spawn(&mut self, wcm: &mut WorldChrMan, now: f64) {
        if let Some(p) = self.pending_spawn.as_ref() {
            let cur = wcm.debug_chr_creator.last_created_chr;
            let addr = cur.map(|c| c.as_ptr() as usize).unwrap_or(0);
            if addr != 0 && addr != p.before {
                let chr = unsafe { &mut *cur.unwrap().as_ptr() };
                let id = id_of(&chr.field_ins_handle);
                let HavokPosition(x, y, z, _) = chr.modules.physics.position;
                let off = Vec3::new(x, y, z).dist(p.want);
                crate::glog!("debug_spawn", "created {id:#x} from {}: at ({x:.1},{y:.1},{z:.1}), {off:.1} m from where it was asked for; team {}; health set to {}", p.template, chr.team_type, p.hp);
                self.summoned.insert(id);
                self.summoned_handles.push(chr.field_ins_handle);
                self.scaled.insert(id, p.hp);
                self.tough.insert(id, (p.hp, now + 2.0));
                crate::overlay::toast(if off > 25.0 { "Tough enemy summoned, but it landed away from you (see gowring.log)" } else { "Tough enemy summoned" });
                self.pending_spawn = None;
            } else if now - p.at > 5.0 {
                crate::glog!("debug_spawn", "the game did not create {} within 5 s", p.template);
                crate::overlay::toast("Could not summon an enemy (see gowring.log)");
                self.pending_spawn = None;
            }
        }
        // Hold a fresh summon's health while the game finishes setting it up (it may write its own once).
        if !self.tough.is_empty() {
            self.tough.retain(|_, (_, until)| now < *until);
            for (id, (hp, _)) in self.tough.iter() {
                let Some(h) = self.handles.get(id).copied() else { continue };
                if let Some(chr) = wcm.chr_ins_by_handle_mut(&h) {
                    if let Some(d) = opt_mut(&mut chr.modules.data) {
                        if d.max_hp != *hp && d.hp > 0 {
                            let untouched = d.hp >= d.max_hp;
                            d.max_hp = *hp;
                            d.max_uncapped_hp = d.max_uncapped_hp.max(*hp);
                            d.base_hp = *hp;
                            if untouched {
                                d.hp = *hp;
                            }
                        }
                    }
                }
            }
        }
    }

    /// Where the player character's behaviour module is in memory (for reading its skeleton).
    pub fn player_behavior_module(&self) -> Option<usize> {
        let wcm = unsafe { WorldChrMan::instance() }.ok()?;
        let player = wcm.main_player.as_ref()?;
        let chr: &ChrIns = &player.chr_ins;
        let b = &chr.modules.behavior;
        Some(&**b as *const _ as usize)
    }

    /// The address of the player's model item (`CSFD4ModelItem`, which holds its bone matrices), for the pose probe.
    pub fn player_model_item(&self) -> Option<usize> {
        let wcm = unsafe { WorldChrMan::instance() }.ok()?;
        let player = wcm.main_player.as_ref()?;
        let model = &*player.chr_ins.chr_model_ins;
        Some(&*model.model_ins.model_item as *const _ as usize)
    }

    /// The address of the player's face data (`FaceDataBuffer`: "FACE", version, size, then the parts and sliders the
    /// character creator set), for the probe's log.
    pub fn player_face_data(&self) -> Option<usize> {
        let wcm = unsafe { WorldChrMan::instance() }.ok()?;
        let player = wcm.main_player.as_ref()?;
        let data = player.player_game_data.as_ptr() as usize;
        Some(data + std::mem::offset_of!(eldenring::cs::PlayerGameData, face_data) + std::mem::offset_of!(eldenring::cs::FaceData, face_data_buffer))
    }

    /// Read the world. `None` when there is no player yet (title screen, loading).
    pub fn build_frame(&mut self, time: f64, dt: f32, voice: Option<VoiceInfo>) -> Option<Frame> {
        let wcm = unsafe { WorldChrMan::instance() }.ok()?;
        let player = wcm.main_player.as_ref()?;
        let chr: &ChrIns = &player.chr_ins;
        let data = &chr.modules.data;
        let phys = &chr.modules.physics;
        let ar = &chr.modules.action_request;

        let HavokPosition(px, py, pz, _) = phys.position;
        let ppos = Vec3::new(px, py, pz);
        let yaw = phys.orientation.to_euler_angles().1;
        if let Some(prev) = self.prev_pos {
            let step = (ppos - prev).flat();
            let speed = step.len() / dt.max(0.001);
            if speed > 1.8 && speed < 12.0 && self.face_samples < 400 {
                let dir = step.normalized();
                for m in 0..8 {
                    self.face_score[m] += facing_candidate(m, yaw).dot(dir);
                }
                self.face_samples += 1;
                if self.face_samples == 40 || self.face_samples == 400 {
                    let best = (0..8).max_by(|a, b| self.face_score[*a].partial_cmp(&self.face_score[*b]).unwrap()).unwrap_or(0);
                    if best != self.face_mode {
                        crate::glog!("player_state", "facing mapping changed {} -> {best} after {} samples (scores {:?})", self.face_mode, self.face_samples, self.face_score.map(|v| (v / self.face_samples as f32 * 100.0).round() / 100.0));
                    } else {
                        crate::glog!("player_state", "facing mapping {best} confirmed after {} samples", self.face_samples);
                    }
                    self.face_mode = best;
                }
            }
        }
        self.prev_pos = Some(ppos);
        let facing = facing_candidate(self.face_mode, yaw);
        self.last_ppos = ppos;
        self.last_facing = facing;
        let (hp, max_hp) = (data.hp, data.max_hp);
        let alive = hp > 0 && !chr.chr_flags1c5.death_flag();
        let player_handle = chr.field_ins_handle;
        let player_id = id_of(&player_handle);

        let mut f = Frame { dt, time, ..Default::default() };
        f.player = PlayerView {
            pos: ppos,
            facing,
            hp: hp as f32,
            max_hp: max_hp.max(1) as f32,
            alive,
            grounded: phys.standing_on_solid_ground,
            sprinting: ar.movement_request_flags.dash(),
            controllable: alive,
        };
        f.actions_down = bits_from_er(&ar.action_requests);
        // which of Elden Ring's animations the player is playing: each change (the first 600) and, every two minutes,
        // the most played ones
        {
            let ta = &chr.modules.time_act;
            // `read_idx` is the slot of the animation last played or updated
            let last = &ta.anim_queue[ta.read_idx as usize % ta.anim_queue.len()];
            if last.anim_id != self.anim_seen.0 && last.anim_id > 0 {
                self.anim_seen.0 = last.anim_id;
                *self.anim_tally.entry(last.anim_id).or_default() += 1;
                if self.anim_seen.1 < 600 {
                    self.anim_seen.1 += 1;
                    crate::glog!("player_anim", "{time:.2}s a{:03}_{:06} ({} s long), buttons {:?}", last.anim_id / 1_000_000, last.anim_id % 1_000_000, (last.anim_length * 100.0).round() / 100.0, bits_from_er(&ar.new_action_presses));
                }
            }
            if time - self.anim_tally_at > 120.0 && !self.anim_tally.is_empty() {
                self.anim_tally_at = time;
                let mut top: Vec<(i32, u32)> = self.anim_tally.iter().map(|(k, v)| (*k, *v)).collect();
                top.sort_by(|a, b| b.1.cmp(&a.1));
                crate::glog!("player_anim", "most played so far: {:?}", top.iter().take(30).map(|(id, n)| format!("a{:03}_{:06} x{n}", id / 1_000_000, id % 1_000_000)).collect::<Vec<_>>());
            }
        }
        {
            let pressed = bits_from_er(&ar.new_action_presses);
            if pressed.has(ErAction::R1) || pressed.has(ErAction::R2) {
                self.last_swing = time;
            }
        }
        if self.my_mask.has(ErAction::L2) {
            self.l2_masked.0 += 1;
            if f.actions_down.has(ErAction::L2) {
                self.l2_masked.1 += 1;
            }
            if self.l2_masked.0 == 30 {
                crate::glog!("action_mask", "L2 was masked for 30 frames and still visible in {} of them (visible = the aim button can be held while its weapon art is blocked)", self.l2_masked.1);
            }
        } else {
            self.l2_masked.0 = 0;
            self.l2_masked.1 = 0;
        }
        f.actions_pressed = bits_from_er(&ar.new_action_presses);
        f.actions_released = bits_from_er(&ar.released_actions);
        f.area = player.play_region_id;

        // --- equipment: active right slot, and whether the left hand holds a shield ---------------------
        {
            let pgd = unsafe { player.player_game_data.as_ref() };
            let asm = &pgd.equipment.chr_asm;
            f.right_slot = asm.equipment.selected_slots.right_weapon_slot.min(2) as u8;
            // ChrAsmSlot order: WeaponLeft1=0, WeaponRight1=1, WeaponLeft2=2, WeaponRight2=3, WeaponLeft3=4, WeaponRight3=5
            let left_idx = (asm.equipment.selected_slots.left_weapon_slot.min(2) * 2) as usize;
            f.shield_drawn = asm.equipment_param_ids[left_idx] > 0;
            for (i, slot) in self.last_equip.iter_mut().enumerate() {
                *slot = asm.equipment_param_ids[i] as i32;
            }
        }

        // --- camera ----------------------------------------------------------------------------------------
        if let Ok(cam) = unsafe { CSCamera::instance() } {
            let pc = &cam.pers_cam_1;
            let fwd = pc.forward();
            f.aim_dir = Vec3::new(fwd.0, fwd.1, fwd.2).normalized();
            let HavokPosition(cx, cy, cz, _) = pc.position();
            f.camera_pos = Vec3::new(cx, cy, cz);
        }

        // --- enemies -------------------------------------------------------------------------------------------
        let boss_handles: HashSet<u64> = unsafe { CSFeManImp::instance() }
            .map(|fe| fe.boss_health_displays.iter().filter(|b| !b.field_ins_handle.is_empty()).map(|b| id_of(&b.field_ins_handle)).collect())
            .unwrap_or_default();
        let mut teams: HashMap<u8, usize> = HashMap::new();
        // Whatever the player locks on to is an enemy: learn its team.
        let locked = player.locked_on_enemy;
        if !locked.is_empty() {
            if let Some(h) = wcm.chr_ins_by_handle(&locked) {
                if h.team_type != chr.team_type && self.hostile_teams.insert(h.team_type) {
                    crate::glog!("enemy_scan", "learned hostile team_type {} (locked on)", h.team_type);
                }
            }
        }
        self.handles.clear();
        let mut nearest_hostile = f32::INFINITY;
        let mut seen_hp: HashMap<u64, i32> = HashMap::new();
        let mut damage: Vec<DamageEvent> = Vec::new();
        let mut summon_pos: Vec<Vec3> = Vec::new();
        let mut cands: Vec<&ChrIns> = wcm.chr_inses_by_distance.iter().take(512).map(|entry| unsafe { entry.chr_ins.as_ref() }).collect();
        self.summoned_handles.retain(|h| wcm.chr_ins_by_handle(h).is_some());
        for h in &self.summoned_handles {
            if let Some(e) = wcm.chr_ins_by_handle(h) {
                if !cands.iter().any(|c| c.field_ins_handle == e.field_ins_handle) {
                    crate::log::once_u64("summon-scan", id_of(h), "debug_spawn", &format!("{:#x} is not in the game's by-distance list; scanning it directly", id_of(h)));
                    cands.push(e);
                }
            }
        }
        for e in cands {
            if e.field_ins_handle == player_handle {
                continue;
            }
            let Some(edata) = opt(&e.modules.data) else { continue };
            let id = id_of(&e.field_ins_handle);
            if id == player_id {
                continue;
            }
            *teams.entry(e.team_type).or_default() += 1;
            let HavokPosition(ex, ey, ez, _) = e.modules.physics.position;
            let epos = Vec3::new(ex, ey, ez);
            let dist = epos.dist(ppos);
            if self.hostile_teams.contains(&e.team_type) && ehp_alive(e) {
                nearest_hostile = nearest_hostile.min(dist);
            }
            if dist > self.scan_range {
                continue;
            }
            if !self.hostile_teams.contains(&e.team_type) {
                continue;
            }
            let ehp = edata.hp;
            if let Some(want) = self.scaled.get(&id) {
                if edata.max_hp < *want {
                    crate::log::once_u64("scale-reverted", id, "enemy_health", &format!("{id:#x}: max hp is back to {} (wrote {want}); the game resets it", edata.max_hp));
                    // a respawned enemy (after a death or a rest) comes back at full, original health: scale it again
                    if edata.hp > 0 && edata.hp >= edata.max_hp && !self.rescale.iter().any(|(i, _)| *i == id) {
                        self.rescale.push((id, *want));
                    }
                }
            }
            let ealive = ehp > 0 && !e.chr_flags1c5.death_flag();
            // damage the player did since last frame
            if let Some(prev) = self.prev_enemy_hp.get(&id) {
                // Elden Ring does not reliably set `last_hit_by` for ordinary hits, so damage to a nearby enemy within
                // a moment of the player's own attack counts as the player's (mod orders are netted off by the ledger).
                let swinging = time - self.last_swing <= 1.4 && dist <= 8.0;
                if ehp < *prev && (e.last_hit_by == player_handle || swinging) {
                    let own = self.own_pending.remove(&id).unwrap_or(0);
                    let amount = (*prev - ehp - own).max(0);
                    if amount > 0 {
                        damage.push(DamageEvent::Dealt { enemy: id, amount: amount as f32 });
                    }
                }
            }
            seen_hp.insert(id, ehp);
            self.handles.insert(id, e.field_ins_handle);
            if self.summoned.contains(&id) && ealive {
                summon_pos.push(epos);
            }
            if f.enemies.len() < self.max_enemies {
                f.enemies.push(EnemyView {
                    id,
                    pos: epos,
                    hp: ehp.max(0) as f32,
                    max_hp: edata.max_hp.max(1) as f32,
                    alive: ealive,
                    // a summon is a test boss: the big bar, a boss stun bar, finishers that take a slice
                    boss: boss_handles.contains(&id) || self.summoned.contains(&id),
                    engaged: dist <= 35.0,
                    radius: e.modules.physics.hit_radius.max(0.4),
                    staggered: false,
                });
            }
        }
        self.prev_enemy_hp = seen_hp;
        self.summon_positions = summon_pos;
        if !self.team_report_done && time - self.last_team_report > 20.0 && teams.values().sum::<usize>() > 4 {
            self.last_team_report = time;
            self.team_report_done = !f.enemies.is_empty();
            crate::glog!("enemy_scan", "{} hostile in range (scan range {} m, nearest living hostile-team character {:.1} m); team_type histogram of everything near the player: {:?}; hostile set {:?}", f.enemies.len(), self.scan_range, nearest_hostile, teams, self.hostile_teams);
        }

        // --- damage the player took -----------------------------------------------------------------------------
        let last_hit = id_of(&chr.last_hit_by);
        let guarding = f.actions_down.has(ErAction::Guard) || f.actions_down.has(ErAction::L1);
        if let Some(prev) = self.prev_player_hp {
            if hp < prev {
                let attacker = (!chr.last_hit_by.is_empty()).then_some(last_hit);
                if let Some(a) = attacker {
                    if let Some(h) = wcm.chr_ins_by_handle(&chr.last_hit_by) {
                        if self.hostile_teams.insert(h.team_type) {
                            crate::glog!("enemy_scan", "learned hostile team_type {} (hurt the player)", h.team_type);
                        }
                    }
                    let _ = a;
                }
                let mut amount = prev - hp;
                let mult = crate::settings::get().debug_enemy_damage_multiplier;
                if mult > 1.0 && self.summon_positions.iter().any(|p| p.dist(ppos) <= 10.0) {
                    let extra = ((amount as f32) * (mult - 1.0)).round() as i32;
                    self.pending_hurt += extra;
                    amount += extra;
                    crate::log::once("summon-hurt", "debug_spawn", &format!("a summon hit for {} (x{mult:.1} -> {amount})", prev - hp));
                }
                damage.push(DamageEvent::Taken { amount: amount as f32, blocked: false, attacker });
            } else if hp >= prev && last_hit != self.prev_player_last_hit && !chr.last_hit_by.is_empty() && guarding {
                damage.push(DamageEvent::Taken { amount: 0.0, blocked: true, attacker: Some(last_hit) });
            }
        }
        self.prev_player_hp = Some(hp);
        self.prev_player_last_hit = last_hit;
        f.damage = damage;
        f.voice = voice;
        Some(f)
    }

    /// Ray results for the commands of the previous frame.
    pub fn cast(&self, cmds: &[Command]) -> Vec<RayResult> {
        let mut out = Vec::new();
        let (Ok(wcm), Ok(hm)) = (unsafe { WorldChrMan::instance() }, unsafe { CSHavokMan::instance() }) else { return out };
        let Some(player) = wcm.main_player.as_ref() else { return out };
        for c in cmds {
            if let Command::Raycast { tag, from, to } = c {
                let origin = HavokPosition(from.x, from.y, from.z, 0.0);
                let d = *to - *from;
                let hit = hm.phys_world.cast_ray(MAP_RAY_FILTER, &origin, PositionDelta(d.x, d.y, d.z), player).map(|HavokPosition(x, y, z, _)| Vec3::new(x, y, z));
                out.push(RayResult { tag: *tag, hit });
            }
        }
        out
    }

    /// Apply the engine's commands to the game.
    pub fn apply(&mut self, cmds: &[Command], now: f64, audio: Option<&Audio>, cache: &Path) {
        let Ok(wcm) = (unsafe { WorldChrMan::instance_mut() }) else { return };
        let player_handle = match wcm.main_player.as_ref() {
            Some(p) => p.chr_ins.field_ins_handle,
            None => return,
        };
        self.settle_debug_spawn(wcm, now);
        for (id, want) in std::mem::take(&mut self.rescale) {
            let Some(h) = self.handles.get(&id).copied() else { continue };
            if let Some(chr) = wcm.chr_ins_by_handle_mut(&h) {
                if let Some(d) = opt_mut(&mut chr.modules.data) {
                    if d.hp > 0 && d.hp >= d.max_hp && d.max_hp < want {
                        let before = d.max_hp;
                        d.max_hp = want;
                        d.max_uncapped_hp = d.max_uncapped_hp.max(want);
                        d.base_hp = d.base_hp.max(want);
                        d.hp = want;
                        crate::log::once_u64("rescaled", id, "enemy_health", &format!("{id:#x}: respawned at {before} hp, scaled back to {want}"));
                    }
                }
            }
        }
        if self.pending_hurt > 0 {
            if let Some(p) = wcm.main_player.as_mut() {
                if let Some(d) = opt_mut(&mut p.chr_ins.modules.data) {
                    if d.hp > 1 {
                        d.hp = (d.hp - self.pending_hurt).max(1);
                        self.prev_player_hp = Some(d.hp);
                    }
                }
            }
            self.pending_hurt = 0;
        }
        let mut new_mask = ActionBits::NONE;
        let mut anim_speed: Option<f32> = None;
        let sheets = gowring_core::Data::get();
        for c in cmds {
            match c {
                Command::Damage { target, amount, .. } => {
                    if let Some(h) = self.handles.get(target).copied() {
                        if let Some(chr) = wcm.chr_ins_by_handle_mut(&h) {
                            if let Some(d) = opt_mut(&mut chr.modules.data) {
                                let before = d.hp;
                                d.hp = (d.hp - amount.round() as i32).max(0);
                                crate::log::once_u64("dmg-target", *target, "enemy_damage", &format!("order of {:.0} on {target:#x}: hp {before} -> {}", amount, d.hp));
                            }
                            chr.last_hit_by = player_handle;
                        }
                    }
                }
                Command::ScaleEnemy { target, factor } => {
                    if self.scaled.contains_key(target) {
                        continue;
                    }
                    if let Some(h) = self.handles.get(target).copied() {
                        if let Some(chr) = wcm.chr_ins_by_handle_mut(&h) {
                            if let Some(d) = opt_mut(&mut chr.modules.data) {
                                let k = factor.clamp(0.25, 20.0) as f64;
                                let (hp0, max0, base0) = (d.hp, d.max_hp, d.base_hp);
                                if hp0 > 0 && max0 > 0 {
                                    let scale = |v: i32| ((v as f64) * k).round().clamp(1.0, 1.0e8) as i32;
                                    d.max_hp = scale(max0);
                                    d.max_uncapped_hp = d.max_uncapped_hp.max(d.max_hp);
                                    d.base_hp = scale(base0.max(1));
                                    d.hp = scale(hp0).min(d.max_hp);
                                    self.scaled.insert(*target, d.max_hp);
                                    crate::log::once_u64("scale-enemy", *target, "enemy_health", &format!("{target:#x}: hp {hp0}/{max0} -> {}/{} (x{k})", d.hp, d.max_hp));
                                }
                            }
                        }
                    }
                }
                Command::Stagger { target, seconds } => self.freeze(wcm, *target, now, (*seconds).min(6.0)),
                Command::Status { target, status, seconds } => match status {
                    RunicStatus::Frozen => self.freeze(wcm, *target, now, (*seconds).min(8.0)),
                    RunicStatus::Stagger => self.freeze(wcm, *target, now, (*seconds).min(3.0)),
                    RunicStatus::Burning => {
                        let max_hp = self.handles.get(target).and_then(|h| wcm.chr_ins_by_handle(h)).map(|c| c.modules.data.max_hp).unwrap_or(1000);
                        self.burning.retain(|b| b.id != *target);
                        self.burning.push(Burn { id: *target, until: now + *seconds as f64, next_tick: now + 0.5, per_tick: (max_hp / 50).max(8) });
                    }
                    RunicStatus::None => {}
                },
                Command::Heal { amount } | Command::Refund { amount } => {
                    if let Some(p) = wcm.main_player.as_mut() {
                        if let Some(d) = opt_mut(&mut p.chr_ins.modules.data) {
                            d.hp = (d.hp + amount.round() as i32).min(d.max_hp);
                            // keep our own diff from reading a heal as a new hit
                            self.prev_player_hp = Some(d.hp);
                        }
                    }
                }
                Command::Mask(bits) => new_mask = new_mask.union(*bits),
                Command::AnimSpeed(s) => anim_speed = Some(*s),
                Command::Sfx(id) => {
                    if let Some(a) = audio {
                        a.sfx(id, cache.join(sheets_asset_output(id)), 0.9);
                    }
                }
                Command::Voice { set, no_repeat, interrupt, .. } => {
                    if let Some(a) = audio {
                        a.voice(cache.join(sheets_asset_output(set)), set, *no_repeat, *interrupt, sheets.mimir.volume);
                    }
                }
                Command::Finisher { target, .. } => {
                    crate::log::once("finisher-visual", "enemy_damage", &format!("finisher on {target:#x}: damage applied; no execution animation in this build"));
                }
                Command::Cast { runic } => {
                    // Kratos plays the runic's Elden Ring skill: put it on the weapon and press the skill button
                    // (held below until the skill's animation starts)
                    match RUNICS.iter().find(|r| r.id == *runic).filter(|r| r.er_skill > 0) {
                        Some(r) => self.start_skill(r.id, r.weapon, r.er_skill, false, SKILL_WAIT_S, now),
                        None => crate::log::once("cast-visual", "enemy_damage", &format!("runic `{runic}`: effects applied; no animation set for it")),
                    }
                }
                // Spartan Rage's roar: the same path, and the weapon gets its own skill back once it has played
                Command::Skill { tag, weapon, skill } => self.start_skill(tag, weapon, *skill, true, ROAR_WAIT_S, now),
                Command::Raycast { .. } | Command::Fx { .. } | Command::Shake(_) => {}
            }
        }
        // masks: clear what we set last frame that we no longer want, set what we want now
        if let Some(p) = wcm.main_player.as_mut() {
            // the guard button as the player holds it, before the masks below take anything out
            let held = bits_from_er(&p.chr_ins.modules.action_request.action_requests);
            let st = crate::settings::get();
            if !self.arm_stopped && st.two_hand_by_default {
                self.steer_arm_style(p, held.has(ErAction::Guard) || held.has(ErAction::L1), st.hide_stowed_shield, now);
            } else if self.shield_hidden {
                // two-handing by default was turned off (or stopped): the shield comes back
                self.steer_left_hand(p, true, now);
            }
            let ar = &mut p.chr_ins.modules.action_request;
            let stale = ActionBits(self.my_mask.0 & !new_mask.0);
            // L2 is the aim button and must stay readable while held: a disabled input vanishes from the game's
            // requests, so the aim would drop the moment it is masked and flicker. For L2 only the queued/pending
            // copies are cleared (below), which is enough to keep the weapon art from starting.
            let disable = ActionBits(new_mask.0 & !ActionBits::bit(ErAction::L2));
            set_er_bits(&mut ar.disabled_action_inputs, stale, false);
            set_er_bits(&mut ar.disabled_action_inputs, disable, true);
            // Our task runs after the game has worked out this frame's requests and before the behaviour
            // graph reads them, so a masked action must also be removed from what is already queued.
            if !new_mask.is_empty() {
                set_er_bits(&mut ar.action_requests, new_mask, false);
                set_er_bits(&mut ar.new_action_presses, new_mask, false);
                set_er_bits(&mut ar.queued_action_inputs, new_mask, false);
                set_er_bits(&mut ar.cancel_ready_actions, new_mask, false);
                set_er_bits(&mut ar.readback_new_presses, new_mask, false);
                set_er_bits(&mut ar.readback_queued_inputs, new_mask, false);
                set_er_bits(&mut ar.readback_cancel_ready, new_mask, false);
                crate::log::once_u64("mask", new_mask.0, "action_mask", &format!("masked ER action bits {:#x}", new_mask.0));
            }
            self.my_mask = new_mask;
            // D-pad up/down cast runics, but the game switches the spell and the quick item on its own path, which
            // the masks do not reach, so the flask moved away from the item button. While the mod owns the D-pad (and
            // a moment after), a switch is put back; at any other time a change (from the menu, say) is kept.
            if new_mask.has(ErAction::SwitchForm) || new_mask.has(ErAction::ChangeItem) {
                self.dpad_owned_at = now;
            }
            let owned = now - self.dpad_owned_at < DPAD_LOCK_S;
            let pgd = unsafe { p.player_game_data.as_mut() };
            let quick = pgd.equipment.equip_item_data.selected_quick_slot;
            if let Some(magic) = opt_mut(&mut pgd.equipment.equip_magic_data) {
                let now_slots = (magic.selected_slot, quick);
                match self.slot_lock {
                    Some(kept) if owned && kept != now_slots => {
                        magic.selected_slot = kept.0;
                        pgd.equipment.equip_item_data.selected_quick_slot = kept.1;
                        self.slot_restores += 1;
                        if self.slot_restores <= 20 || self.slot_restores % 50 == 0 {
                            crate::glog!("dpad_lock", "the game switched spell {} -> {} and quick item {} -> {} on a runic press; put back ({} so far)", kept.0, now_slots.0, kept.1, now_slots.1, self.slot_restores);
                        }
                    }
                    _ => self.slot_lock = Some(now_slots),
                }
            }
            // a runic's skill: hold the skill button (after the masks, so it gets through) until the skill's
            // animation starts or the wait runs out
            let l2 = ActionBits(ActionBits::bit(ErAction::L2));
            if let Some(sc) = self.skill_cast.as_mut() {
                sc.frames += 1;
                let ta = &p.chr_ins.modules.time_act;
                let slot = &ta.anim_queue[ta.read_idx as usize % ta.anim_queue.len()];
                let (cur, len) = (slot.anim_id, slot.anim_length);
                let age = now - sc.at;
                if is_skill_anim(cur, sc.category) {
                    crate::glog!("skill_anim", "`{}`: skill {} animation a{:03}_{:06} started after {age:.2}s ({} frames held)", sc.tag, sc.skill, cur / 1_000_000, cur % 1_000_000, sc.frames);
                    self.skill_misses.insert(sc.tag, 0);
                    self.skill_quiet_until = now + (len as f64).clamp(0.5, 4.0);
                    if !sc.restore.is_empty() {
                        self.skill_restore = Some((std::mem::take(&mut sc.restore), self.skill_quiet_until));
                    }
                    self.skill_cast = None;
                } else if age > sc.wait {
                    let n = self.skill_misses.entry(sc.tag).or_default();
                    *n += 1;
                    crate::glog!("skill_anim", "`{}`: no skill animation after {age:.2}s (playing a{:03}_{:06}); miss {n} of {SKILL_GIVE_UP}", sc.tag, cur / 1_000_000, cur % 1_000_000);
                    if !sc.restore.is_empty() {
                        crate::params::set_rows_skill(&sc.restore);
                    }
                    self.skill_cast = None;
                } else {
                    // one press, then held: nothing left queued for the game to read as a second press. The roar's
                    // longer wait presses again every 12 frames, since a press made while Kratos is staggered is dropped
                    let ar = &mut p.chr_ins.modules.action_request;
                    set_er_bits(&mut ar.action_requests, l2, true);
                    if sc.frames == 1 || (sc.wait > SKILL_WAIT_S && sc.frames % 12 == 1) {
                        set_er_bits(&mut ar.new_action_presses, l2, true);
                        set_er_bits(&mut ar.readback_new_presses, l2, true);
                    }
                }
            } else if now < self.skill_quiet_until {
                // while the skill plays, no skill-button press reaches it (no second strike)
                let ar = &mut p.chr_ins.modules.action_request;
                set_er_bits(&mut ar.new_action_presses, l2, false);
                set_er_bits(&mut ar.queued_action_inputs, l2, false);
                set_er_bits(&mut ar.cancel_ready_actions, l2, false);
                set_er_bits(&mut ar.readback_new_presses, l2, false);
                set_er_bits(&mut ar.readback_queued_inputs, l2, false);
                set_er_bits(&mut ar.readback_cancel_ready, l2, false);
            }
            if let Some(s) = anim_speed {
                p.chr_ins.modules.behavior.animation_speed = s;
            }
            if self.skill_restore.as_ref().is_some_and(|(_, at)| now >= *at) {
                let (rows, _) = self.skill_restore.take().unwrap();
                crate::params::set_rows_skill(&rows);
                crate::glog!("skill_anim", "{now:.2}s weapon skills put back (row, skill) {rows:?}");
            }
        }
        self.tick_statuses(wcm, now, player_handle);
    }

    /// Two-handing by default: Kratos holds his right-hand weapon in both hands, and while the guard button is held
    /// (and `SHIELD_LINGER_S` after) in the right hand alone, so the Guardian Shield comes up on his left arm. The
    /// stance is written into both copies of the player's ChrAsm (the game data's and the character's) before the
    /// behaviour graph reads it, so the guard on that frame is the shield's. An empty-handed stance, an unarmed right
    /// hand and the left weapon in both hands (the player's own choice) are left alone. If the game undoes the stance
    /// for `ARM_STYLE_GIVE_UP` frames in a row, it stops for the session. With `hide_shield`, the shield is put away
    /// while Kratos two-hands (`steer_left_hand`).
    fn steer_arm_style(&mut self, p: &mut PlayerIns, guard: bool, hide_shield: bool, now: f64) {
        const ONE: u32 = ChrAsmArmStyle::OneHanded as u32;
        const BOTH: u32 = ChrAsmArmStyle::RightBothHands as u32;
        let pgd = unsafe { p.player_game_data.as_mut() };
        // read as numbers: the game may hold a value the enum does not name
        let game = &mut pgd.equipment.chr_asm.equipment.arm_style as *mut ChrAsmArmStyle as *mut u32;
        let chr = &mut p.chr_asm.equipment.arm_style as *mut ChrAsmArmStyle as *mut u32;
        let (style, chr_style) = unsafe { (game.read(), chr.read()) };
        if let Some(w) = self.arm_written.take() {
            if style != w {
                self.arm_counts.1 += 1;
                self.arm_undone_run += 1;
                if self.arm_counts.1 <= 10 || self.arm_counts.1 % 100 == 0 {
                    crate::glog!("two_hand", "{now:.2}s the game changed the stance written last frame ({}) to {} ({} undone of {} writes)", arm_style_name(w), arm_style_name(style), self.arm_counts.1, self.arm_counts.0);
                }
                if self.arm_undone_run >= ARM_STYLE_GIVE_UP {
                    self.arm_stopped = true;
                    crate::glog!("two_hand", "the game undid the stance {ARM_STYLE_GIVE_UP} frames in a row: two-handing by default is off for this session");
                    return;
                }
            } else {
                self.arm_undone_run = 0;
            }
        }
        if guard {
            self.shield_until = now + SHIELD_LINGER_S;
        }
        let asm = &pgd.equipment.chr_asm;
        let right = asm.equipment_param_ids[asm.equipment.selected_slots.right_weapon_slot.min(2) as usize * 2 + 1];
        if !(ONE..=BOTH).contains(&style) || style == ChrAsmArmStyle::LeftBothHands as u32 || right <= 0 || right == UNARMED {
            // a stance this leaves alone: the shield is out
            if self.shield_hidden {
                self.steer_left_hand(p, true, now);
            }
            return;
        }
        let want = if now < self.shield_until { ONE } else { BOTH };
        if hide_shield && !self.left_stopped {
            self.steer_left_hand(p, want == ONE, now);
        } else if self.shield_hidden {
            self.steer_left_hand(p, true, now);
        }
        if style == want && chr_style == want {
            return;
        }
        unsafe {
            game.write(want);
            chr.write(want);
        }
        self.arm_written = Some(want);
        self.arm_counts.0 += 1;
        if self.arm_counts.0 <= 10 || self.arm_counts.0 % 200 == 0 {
            let same = std::ptr::eq(game, chr);
            crate::glog!(
                "two_hand",
                "{now:.2}s stance {} (character's copy {}{}) -> {} ({}); write {}",
                arm_style_name(style),
                arm_style_name(chr_style),
                if same { ", the same memory" } else { "" },
                arm_style_name(want),
                if want == ONE { "guard held: the shield comes up" } else { "two-handed" },
                self.arm_counts.0
            );
        }
    }

    /// The shield only while it guards: while Kratos two-hands, his left hand is switched to an empty weapon slot (a
    /// left weapon not in use hangs on the back, and the Guardian Shield folds into his bracer in Ragnarök), and back
    /// to the shield's slot when the guard comes up (`show`), in both copies of the ChrAsm as the stance is. Only
    /// between those two slots: a left weapon the player picked is left alone, and nothing happens without an empty
    /// left slot. If the game undoes the switch for `ARM_STYLE_GIVE_UP` frames in a row, the shield stays out for the
    /// session.
    fn steer_left_hand(&mut self, p: &mut PlayerIns, show: bool, now: f64) {
        if self.shield_rows.is_empty() {
            self.shield_rows = crate::params::carrier_rows("guardian_shield").into_iter().map(|r| r as i32).collect();
        }
        let pgd = unsafe { p.player_game_data.as_mut() };
        let game = &mut pgd.equipment.chr_asm.equipment.selected_slots.left_weapon_slot as *mut u32;
        let chr = &mut p.chr_asm.equipment.selected_slots.left_weapon_slot as *mut u32;
        let (slot, chr_slot) = unsafe { (game.read(), chr.read()) };
        if let Some(w) = self.left_written.take() {
            if slot != w {
                self.left_counts.1 += 1;
                self.left_undone_run += 1;
                if self.left_counts.1 <= 10 || self.left_counts.1 % 100 == 0 {
                    crate::glog!("two_hand", "{now:.2}s the game changed the left hand's slot written last frame ({w}) to {slot} ({} undone of {} writes)", self.left_counts.1, self.left_counts.0);
                }
                if self.left_undone_run >= ARM_STYLE_GIVE_UP {
                    self.left_stopped = true;
                    self.shield_hidden = false;
                    crate::glog!("two_hand", "the game undid the left hand's slot {ARM_STYLE_GIVE_UP} frames in a row: the shield stays out for this session");
                    return;
                }
            } else {
                self.left_undone_run = 0;
            }
        }
        // left slots are ChrAsm slots 0, 2 and 4
        let ids = pgd.equipment.chr_asm.equipment_param_ids;
        let left = |s: u32| ids[(s.min(2) * 2) as usize];
        let is_empty = |s: u32| left(s) <= 0 || left(s) == UNARMED;
        let shield = (0..3u32).find(|s| left(*s) > 0 && self.shield_rows.contains(&left(*s)));
        let empty = (0..3u32).find(|s| is_empty(*s));
        let (Some(shield), Some(empty)) = (shield, empty) else { return };
        // any empty slot counts as put away (the player may have cycled to another)
        if slot != shield && !is_empty(slot) {
            self.shield_hidden = false;
            return;
        }
        let target = if show { shield } else if is_empty(slot) { slot } else { empty };
        self.shield_hidden = target != shield;
        if slot == target && chr_slot == target {
            return;
        }
        unsafe {
            game.write(target);
            chr.write(target);
        }
        self.left_written = Some(target);
        self.left_counts.0 += 1;
        if self.left_counts.0 <= 10 || self.left_counts.0 % 200 == 0 {
            crate::glog!(
                "two_hand",
                "{now:.2}s left hand slot {slot} (character's copy {chr_slot}) -> {target} ({}); write {}",
                if show { "the shield comes out" } else { "the shield is put away" },
                self.left_counts.0
            );
        }
    }

    /// Put `skill` on every row carrying `weapon` and start pressing the skill button for up to `wait` seconds (see
    /// `apply`); `restore`: give the rows their own skill back once it has played.
    fn start_skill(&mut self, tag: &'static str, weapon: &'static str, skill: i32, restore: bool, wait: f64, now: f64) {
        let misses = self.skill_misses.get(tag).copied().unwrap_or(0);
        if misses >= SKILL_GIVE_UP {
            crate::log::once("skill-gave-up", tag, &format!("`{tag}`: skill {skill} showed no animation {SKILL_GIVE_UP} times in a row; played without one for the rest of the session"));
            return;
        }
        // a skill still waiting to be put back goes back first, so its row's own skill is what gets restored later
        if let Some((rows, _)) = self.skill_restore.take() {
            crate::params::set_rows_skill(&rows);
        }
        let rows = crate::params::set_weapon_skill(weapon, skill);
        if rows.is_empty() {
            crate::log::once("skill-no-carrier", weapon, &format!("`{tag}`: no carrier row for {weapon} yet; no animation"));
            return;
        }
        let category = crate::params::skill_category(skill);
        crate::glog!("skill_anim", "{now:.2}s `{tag}`: skill {skill} (animations a{}) on carrier rows (row, skill it had) {rows:?}; pressing the skill button", category.map(|c| format!("{c:03}")).unwrap_or_else(|| "?".into()));
        let restore = if restore { rows.into_iter().filter(|(_, was)| *was != skill).collect() } else { Vec::new() };
        self.skill_cast = Some(SkillCast { tag, skill, restore, category, at: now, frames: 0, wait });
    }

    fn freeze(&mut self, wcm: &mut WorldChrMan, target: u64, now: f64, seconds: f32) {
        let Some(h) = self.handles.get(&target).copied() else { return };
        let Some(chr) = wcm.chr_ins_by_handle_mut(&h) else { return };
        if let Some(t) = opt_mut(&mut chr.modules.toughness) {
            t.toughness = 0.0;
        }
        let b = &mut chr.modules.behavior;
        let entry = self.frozen.entry(target).or_insert(Frozen { until: now, saved_speed: if b.animation_speed > 0.05 { b.animation_speed } else { 1.0 } });
        entry.until = entry.until.max(now + seconds as f64);
        b.animation_speed = 0.02;
    }

    fn tick_statuses(&mut self, wcm: &mut WorldChrMan, now: f64, player: FieldInsHandle) {
        // thaw
        let done: Vec<u64> = self.frozen.iter().filter(|(_, f)| f.until <= now).map(|(k, _)| *k).collect();
        for id in done {
            if let Some(f) = self.frozen.remove(&id) {
                if let Some(chr) = self.handles.get(&id).and_then(|h| wcm.chr_ins_by_handle_mut(h)) {
                    chr.modules.behavior.animation_speed = f.saved_speed;
                }
            }
        }
        for (id, f) in &self.frozen {
            if let Some(chr) = self.handles.get(id).and_then(|h| wcm.chr_ins_by_handle_mut(h)) {
                chr.modules.behavior.animation_speed = 0.02;
                let _ = f;
            }
        }
        // burn
        for b in &mut self.burning {
            if now >= b.next_tick {
                b.next_tick += 0.5;
                if let Some(chr) = self.handles.get(&b.id).and_then(|h| wcm.chr_ins_by_handle_mut(h)) {
                    if let Some(d) = opt_mut(&mut chr.modules.data) {
                        let dealt = b.per_tick.min(d.hp.max(0));
                        d.hp -= dealt;
                        *self.own_pending.entry(b.id).or_default() += dealt;
                    }
                    chr.last_hit_by = player;
                }
            }
        }
        self.burning.retain(|b| b.until > now);
    }

    /// Release everything we are holding (frozen enemies, masked actions) — on shutdown or death.
    pub fn release_all(&mut self) {
        if let Ok(wcm) = unsafe { WorldChrMan::instance_mut() } {
            let ids: Vec<u64> = self.frozen.keys().copied().collect();
            for id in ids {
                if let Some(f) = self.frozen.remove(&id) {
                    if let Some(chr) = self.handles.get(&id).and_then(|h| wcm.chr_ins_by_handle_mut(h)) {
                        chr.modules.behavior.animation_speed = f.saved_speed;
                    }
                }
            }
        }
        self.burning.clear();
    }

    /// Current camera basis for the overlay's projection.
    pub fn camera_basis() -> Option<(Vec3, Vec3, Vec3, Vec3, f32)> {
        let cam = unsafe { CSCamera::instance() }.ok()?;
        let pc = &cam.pers_cam_1;
        let (r, u, f) = (pc.right(), pc.up(), pc.forward());
        let HavokPosition(x, y, z, _) = pc.position();
        Some((Vec3::new(r.0, r.1, r.2), Vec3::new(u.0, u.1, u.2), Vec3::new(f.0, f.1, f.2), Vec3::new(x, y, z), pc.fov))
    }
}

/// The cache-relative output of a sheet asset (sound file, or a voice-set directory).
fn sheets_asset_output(id: &str) -> &'static str {
    gowring_core::Data::get().asset(id).output
}

/// Alive by the game's own flags (hp above zero and no death flag).
fn ehp_alive(e: &ChrIns) -> bool {
    opt(&e.modules.data).map(|d| d.hp > 0).unwrap_or(false) && !e.chr_flags1c5.death_flag()
}

/// `OwnedPtr<T>` is declared non-null, but a character without a module can still hold null there.
fn opt<T>(p: &fromsoftware_shared::OwnedPtr<T>) -> Option<&T> {
    let raw = p.as_ptr();
    if raw.is_null() { None } else { Some(unsafe { &*raw }) }
}

fn opt_mut<T>(p: &mut fromsoftware_shared::OwnedPtr<T>) -> Option<&mut T> {
    let raw = p.as_ptr();
    if raw.is_null() { None } else { Some(unsafe { &mut *raw }) }
}
