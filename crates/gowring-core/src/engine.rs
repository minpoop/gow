//! The rules engine: one `tick` per frame turns a `Frame` into `Command`s, HUD state and a camera request.

use crate::camera::Camera;
use crate::data::Data;
use crate::hud::{EnemyBar, HudState, RunicHud, RunicPicker};
use crate::status::StatusMeters;
use crate::input::{self, Inputs};
use crate::melee::{Ledger, Melee};
use crate::mimir::{Context as MimirCtx, Mimir};
use crate::rage::Rage;
use crate::runic::Runics;
use crate::sheets::*;
use crate::stun::StunTracker;
use crate::thrown::Thrown;
use crate::types::*;
use std::collections::{HashMap, HashSet, VecDeque};

/// Things that happened this frame; consumed by Mimir and the log.
#[derive(Clone, Debug)]
pub enum Event {
    Kill { enemy: EnemyId, boss: bool },
    RageOn,
    RageOff,
    RunicUsed { runic: &'static RunicDef, hit: usize, damage: f32 },
    AxeThrown,
    AxeCaught,
    SpearThrown,
    SpearsDetonated { spears: usize, hit: usize },
    StunFull { enemy: EnemyId },
    Parry { attacker: Option<EnemyId> },
    Finisher { enemy: EnemyId },
    WeaponChanged { weapon: &'static str },
    /// A different runic was put in a weapon's light or heavy slot (L1 + D-pad).
    RunicEquipped { runic: &'static RunicDef },
}

/// Rage and stun that subsystems earned this frame.
#[derive(Default, Debug)]
pub struct Gains {
    pub rage: f32,
    pub stun: Vec<(EnemyId, f32)>,
    /// Enemies the player damaged by non-melee means (kill credit).
    pub player_hit: Vec<EnemyId>,
    /// Elemental build-up from thrown weapons: (enemy, element, points, seconds the status lasts).
    pub status: Vec<(EnemyId, WeaponStatus, f32, f32)>,
}

pub struct TickOutput {
    pub commands: Vec<Command>,
    pub hud: HudState,
    pub camera: CameraRequest,
    pub logs: Vec<String>,
    /// Thrown weapons to draw: (weapon id, position).
    pub projectiles: Vec<(&'static str, Vec3)>,
    /// Runics equipped in game this frame (the DLL keeps the choice in settings.json).
    pub equipped: Vec<&'static str>,
}

/// How long the runic picker stays on screen after a slot is changed.
const PICKER_S: f64 = 3.0;

/// How much of a weapon's burn or freeze build-up, and of its duration, a boss takes.
const BOSS_STATUS_RESIST: f32 = 0.5;

/// Player-adjustable multipliers (the DLL reads them from `settings.json`).
#[derive(Clone, Copy, Debug)]
pub struct Tuning {
    pub rage_gain: f32,
    pub mod_damage: f32,
    pub mimir_chance: f32,
    /// Regular enemies get this many times their normal HP when first seen (1 = untouched).
    pub enemy_health: f32,
    /// A regular enemy the player has worn down to the tuning sheet's wounded level is staggered once, so it can be finished.
    pub wounded_stun: bool,
    /// Enemies drop health stones (Ragnarök's healing).
    pub health_stones: bool,
}

impl Default for Tuning {
    fn default() -> Self {
        Tuning { rage_gain: 1.0, mod_damage: 1.0, mimir_chance: 1.0, enemy_health: 1.0, wounded_stun: true, health_stones: true }
    }
}

/// A health stone on the ground: where, and when it dropped.
#[derive(Clone, Copy, Debug)]
pub struct Stone {
    pub pos: Vec3,
    pub born: f64,
}

pub struct Engine {
    d: &'static Data,
    /// Health stones dropped by enemies, waiting to be walked over, and the hostiles alive last frame (a death near
    /// Kratos drops stones whoever dealt the blow).
    stones: Vec<Stone>,
    stone_alive: HashSet<EnemyId>,
    tuning: Tuning,
    rage: Rage,
    runics: Runics,
    thrown: Thrown,
    melee: Melee,
    ledger: Ledger,
    stun: StunTracker,
    status: StatusMeters,
    mimir: Mimir,
    camera: Camera,
    weapon: &'static WeaponDef,
    // timing / state
    first_time: Option<f64>,
    greeted: bool,
    last_combat: f64,
    was_in_combat: bool,
    last_idle_fire: f64,
    hp_trail: f32,
    hp_low: bool,
    was_alive: bool,
    area: Option<u32>,
    kills: VecDeque<f64>,
    bosses_seen: HashSet<EnemyId>,
    guard_started: Option<f64>,
    parry_spent: bool,
    last_block_sfx: f64,
    rage_on_at: f64,
    voice_was_playing: bool,
    /// Last time the aim chord was seen held; aiming survives a missed frame or two (see `tick`).
    aim_last_down: f64,
    /// Enemies already sent a `ScaleEnemy` this session.
    scaled: HashSet<EnemyId>,
    /// Native hits written to the log so far (the first few only).
    hit_logs: u32,
    /// When the last finisher landed (drives the camera punch-in).
    finisher_at: f64,
    status_gc_at: f64,
    /// The runic in each weapon's light and heavy slot, where it differs from the weapon's own pick.
    equipped: HashMap<(&'static str, RunicTier), &'static str>,
    /// The slot the runic picker shows, and until when.
    picker: Option<(RunicTier, f64)>,
    /// Per D-pad slot (light, heavy): when its button was last seen down, and whether a change may happen (one per
    /// press: a held D-pad flickers while it is masked, so a press only counts again after a real release).
    dpad_seen: [f64; 2],
    cycle_armed: [bool; 2],
    /// What the health and Rage bars show: they ease toward the real values instead of jumping.
    hp_shown: f32,
    rage_shown: f32,
    /// When health was last lost (the damage chunk shows for a moment, then fades).
    hp_hit_at: f64,
    /// The bars start where the player is (no red chunk for loading in hurt).
    hud_started: bool,
}

/// How long a D-pad button must be up before L1 + D-pad changes a runic again.
const DPAD_RELEASE_S: f64 = 0.15;

/// Move `shown` toward `target`, taking about `tau` seconds (0 = at once).
fn ease(shown: f32, target: f32, dt: f32, tau: f32) -> f32 {
    if tau <= 0.0 || (target - shown).abs() < 0.0015 {
        return target;
    }
    shown + (target - shown) * (1.0 - (-dt / tau).exp())
}

impl Engine {
    pub fn new() -> Self {
        let d = Data::get();
        Engine {
            d,
            tuning: Tuning::default(),
            rage: Rage::new(d),
            runics: Runics::new(d),
            thrown: Thrown::new(d),
            melee: Melee::default(),
            ledger: Ledger::default(),
            stun: StunTracker::default(),
            status: StatusMeters::default(),
            mimir: Mimir::new(d),
            camera: Camera::new(d),
            weapon: d.right_weapon(0),
            first_time: None,
            greeted: false,
            last_combat: f64::NEG_INFINITY,
            was_in_combat: false,
            last_idle_fire: 0.0,
            hp_trail: 1.0,
            hp_low: false,
            was_alive: true,
            area: None,
            kills: VecDeque::new(),
            bosses_seen: HashSet::new(),
            guard_started: None,
            parry_spent: false,
            last_block_sfx: f64::NEG_INFINITY,
            rage_on_at: f64::NEG_INFINITY,
            voice_was_playing: false,
            aim_last_down: f64::NEG_INFINITY,
            scaled: HashSet::new(),
            hit_logs: 0,
            finisher_at: f64::NEG_INFINITY,
            status_gc_at: 0.0,
            equipped: HashMap::new(),
            picker: None,
            dpad_seen: [f64::NEG_INFINITY; 2],
            cycle_armed: [true; 2],
            hp_shown: 1.0,
            rage_shown: 0.0,
            hp_hit_at: f64::NEG_INFINITY,
            hud_started: false,
            stones: Vec::new(),
            stone_alive: HashSet::new(),
        }
    }

    /// The runic in a weapon's light or heavy slot (`None` for a weapon without runics).
    pub fn equipped(&self, weapon: &WeaponDef, tier: RunicTier) -> Option<&'static RunicDef> {
        let own = match tier {
            RunicTier::Light => weapon.light_runic,
            RunicTier::Heavy => weapon.heavy_runic,
        }?;
        Some(self.d.runic(self.equipped.get(&(weapon.id, tier)).copied().unwrap_or(own)))
    }

    /// Put a runic (by id) in its weapon's slot, as the player's settings ask. Returns whether anything changed;
    /// an unknown runic, or one for a weapon without that slot, is ignored.
    pub fn equip(&mut self, runic_id: &str) -> bool {
        let Some(r) = self.d.all_runics().find(|r| r.id == runic_id) else { return false };
        let w = self.d.weapon(r.weapon);
        if self.equipped(w, r.tier).is_none() {
            return false;
        }
        self.equipped.insert((r.weapon, r.tier), r.id) != Some(r.id)
    }

    /// The runics a weapon can put in a slot, in the sheet's order.
    pub fn runic_options(&self, weapon_id: &str, tier: RunicTier) -> Vec<&'static RunicDef> {
        self.d.all_runics().filter(|r| r.weapon == weapon_id && r.tier == tier).collect()
    }

    /// L1 + D-pad: the next runic for the current weapon's slot, shown in the picker.
    fn cycle_runic(&mut self, tier: RunicTier, now: f64, out: &mut Vec<Command>, ev: &mut Vec<Event>) -> Option<&'static RunicDef> {
        let weapon = self.weapon;
        let cur = self.equipped(weapon, tier)?;
        let opts = self.runic_options(weapon.id, tier);
        let i = opts.iter().position(|r| r.id == cur.id).unwrap_or(0);
        let next = opts[(i + 1) % opts.len()];
        self.equipped.insert((weapon.id, tier), next.id);
        self.picker = Some((tier, now + PICKER_S));
        if let Some(s) = weapon.swap_sfx {
            out.push(Command::Sfx(s));
        }
        ev.push(Event::RunicEquipped { runic: next });
        Some(next)
    }

    pub fn rage(&self) -> &Rage {
        &self.rage
    }
    /// Fill Spartan Rage at once (the debug key).
    pub fn debug_fill_rage(&mut self) {
        self.rage.fill();
    }
    pub fn stones(&self) -> &[Stone] {
        &self.stones
    }
    pub fn thrown(&self) -> &Thrown {
        &self.thrown
    }
    pub fn stun(&self) -> &StunTracker {
        &self.stun
    }
    pub fn runics(&self) -> &Runics {
        &self.runics
    }
    pub fn weapon(&self) -> &'static WeaponDef {
        self.weapon
    }

    fn all_base_damage_max(&self) -> f32 {
        WEAPONS.iter().map(|w| w.base_damage).max().unwrap_or(1) as f32
    }

    /// Add elemental build-up to an enemy; a full meter freezes or burns it (with a ring of frost or fire).
    fn build_status(&mut self, f: &Frame, enemy: EnemyId, kind: WeaponStatus, pts: f32, secs: f32, out: &mut Vec<Command>, logs: &mut Vec<String>) {
        // Bosses resist: the meter fills at half speed and the status lasts half as long (no freeze-lock).
        let boss = f.enemies.iter().any(|e| e.id == enemy && e.boss);
        let (pts, secs) = if boss { (pts * BOSS_STATUS_RESIST, secs * BOSS_STATUS_RESIST) } else { (pts, secs) };
        if !self.status.add(enemy, kind, pts, f.time) {
            return;
        }
        let (status, fx, word) = match kind {
            WeaponStatus::Frozen => (RunicStatus::Frozen, FxKind::Frost, "frozen"),
            _ => (RunicStatus::Burning, FxKind::Fire, "burning"),
        };
        out.push(Command::Status { target: enemy, status, seconds: secs });
        self.status.set_active(enemy, kind, f.time + secs as f64);
        if let Some(e) = f.enemies.iter().find(|e| e.id == enemy) {
            out.push(Command::Fx { kind: fx, pos: e.pos, dir: f.player.facing, radius: 1.6 + e.radius });
        }
        logs.push(format!("{word} {enemy:#x} for {secs:.1} s"));
    }

    pub fn set_tuning(&mut self, t: Tuning) {
        self.tuning = Tuning {
            rage_gain: t.rage_gain.clamp(0.0, 10.0),
            mod_damage: t.mod_damage.clamp(0.0, 20.0),
            mimir_chance: t.mimir_chance.clamp(0.0, 10.0),
            enemy_health: t.enemy_health.clamp(0.25, 20.0),
            wounded_stun: t.wounded_stun,
            health_stones: t.health_stones,
        };
    }

    pub fn tick(&mut self, f: &Frame) -> TickOutput {
        let d = self.d;
        self.rage.gain_mult = self.tuning.rage_gain * d.rage.gain_scale;
        self.mimir.chance_mult = self.tuning.mimir_chance;
        let now = f.time;
        let mut out: Vec<Command> = Vec::new();
        let mut ev: Vec<Event> = Vec::new();
        let mut gains = Gains::default();
        let mut logs: Vec<String> = Vec::new();
        let first = *self.first_time.get_or_insert(now);

        let mut inp: Inputs = input::read(d, f);
        // The game can drop a held button for a frame (a masked or re-evaluated input); a held aim must not flicker.
        if inp.aim_held {
            self.aim_last_down = now;
        } else if now - self.aim_last_down <= 0.12 {
            inp.aim_held = true;
        }
        let controllable = f.player.controllable && f.player.alive;
        let hp_frac = if f.player.max_hp > 0.0 { (f.player.hp / f.player.max_hp).clamp(0.0, 1.0) } else { 0.0 };

        // ---- death / respawn ------------------------------------------------------------------
        if self.was_alive && !f.player.alive {
            self.on_player_died(&mut out);
            self.mimir.event(MimirTriggerEvent::PlayerDeath, now, self.mimir_ctx(f, now), &mut out);
        }
        self.was_alive = f.player.alive;

        // ---- weapon changes ---------------------------------------------------------------------
        let weapon = d.right_weapon(f.right_slot);
        if weapon.id != self.weapon.id {
            if let Some(s) = weapon.swap_sfx {
                out.push(Command::Sfx(s));
            }
            self.melee.reset();
            self.weapon = weapon;
            ev.push(Event::WeaponChanged { weapon: weapon.id });
            logs.push(format!("weapon -> {}", weapon.name));
        }

        // ---- world sync ----------------------------------------------------------------------------
        self.stun.sync(d, f, now, &mut ev);
        if (self.tuning.enemy_health - 1.0).abs() > 0.01 {
            for e in &f.enemies {
                if e.alive && !e.boss && e.max_hp <= d.tuning.enemy_scale_hp_ceiling as f32 && self.scaled.insert(e.id) {
                    out.push(Command::ScaleEnemy { target: e.id, factor: self.tuning.enemy_health });
                }
            }
        }
        let near_engaged = f.enemies.iter().any(|e| e.alive && e.engaged && e.pos.dist(f.player.pos) <= d.tuning.engage_range_m);
        if near_engaged {
            self.last_combat = now;
        }

        // ---- guard / parry bookkeeping ---------------------------------------------------------------
        let shield_ok = f.shield_drawn && weapon.kind != WeaponKind::Blades;
        if inp.guard_pressed && shield_ok {
            self.guard_started = Some(now);
            self.parry_spent = false;
        }
        if !inp.guard_held {
            self.guard_started = None;
        }
        let parry_window = d.mv("shield_parry").buffer_close_s as f64;

        // ---- damage reports from the host ---------------------------------------------------------------
        for dm in &f.damage {
            match dm {
                DamageEvent::Dealt { enemy, amount } => {
                    self.stun.note_player_hit(*enemy, now);
                    self.last_combat = now;
                    let native = self.ledger.settle(*enemy, *amount, now);
                    if native > 0.5 {
                        if self.hit_logs < 60 {
                            self.hit_logs += 1;
                            let hp = f.enemies.iter().find(|e| e.id == *enemy).map(|e| (e.hp, e.max_hp)).unwrap_or((0.0, 0.0));
                            logs.push(format!("hit {native:.0} on {enemy:#x} (hp {:.0}/{:.0})", hp.0, hp.1));
                        }
                        let (stun_pts, rage_pts) = match self.melee.attribute(now) {
                            Some(m) => {
                                let h = m.hits.max(1) as f32;
                                (m.stun / h * weapon.stun_rate, m.rage_gain / h)
                            }
                            None => (4.0, 0.5),
                        };
                        gains.stun.push((*enemy, stun_pts * self.rage.stun_mult()));
                        gains.rage += rage_pts;
                        self.build_status(f, *enemy, weapon.status, weapon.status_per_hit, weapon.status_s, &mut out, &mut logs);
                        if self.rage.active {
                            out.push(Command::Damage { target: *enemy, amount: native * (self.rage.damage_mult() - 1.0), source: DamageSource::Melee });
                        }
                    }
                }
                DamageEvent::Taken { amount, blocked, attacker } => {
                    self.last_combat = now;
                    let in_window = self.guard_started.map(|t| now - t <= parry_window).unwrap_or(false) && !self.parry_spent;
                    if in_window && shield_ok {
                        self.parry_spent = true;
                        if *amount > 0.0 {
                            out.push(Command::Refund { amount: *amount });
                        }
                        if let Some(a) = attacker {
                            out.push(Command::Stagger { target: *a, seconds: 2.5 });
                            let p = d.mv("shield_parry");
                            gains.stun.push((*a, p.stun * d.shield.stun_rate * self.rage.stun_mult()));
                            gains.rage += p.rage_gain;
                        }
                        if let Some(s) = d.mv("shield_parry").sfx {
                            out.push(Command::Sfx(s));
                        }
                        ev.push(Event::Parry { attacker: *attacker });
                    } else if *blocked {
                        if now - self.last_block_sfx > 0.3 {
                            if let Some(s) = d.mv("shield_block").sfx {
                                out.push(Command::Sfx(s));
                            }
                            self.last_block_sfx = now;
                        }
                        gains.rage += d.mv("shield_block").rage_gain;
                    } else if *amount > 0.0 {
                        if self.rage.active && self.rage.taken_mult() < 1.0 {
                            out.push(Command::Refund { amount: amount * (1.0 - self.rage.taken_mult()) });
                        }
                        let before = self.rage.meter;
                        self.rage.gain_from_hp_loss(*amount, f.player.max_hp);
                        if self.rage.meter > before {
                            logs.push(format!("rage +{:.1} from losing {:.0} hp -> {:.0}/{:.0}", self.rage.meter - before, amount, self.rage.meter, self.rage.max()));
                        }
                    }
                }
            }
        }

        // ---- inputs → actions -----------------------------------------------------------------------------
        let melee_weapon = matches!(weapon.kind, WeaponKind::Axe | WeaponKind::Blades | WeaponKind::Spear);
        let axe_away = weapon.kind == WeaponKind::Axe && self.thrown.axe_out();
        if controllable {
            // Spartan Rage
            if inp.rage_pressed && self.rage.ready() {
                let base = self.all_base_damage_max();
                self.rage.activate(f, base, d.mv(d.rage.burst_move), weapon.id, &mut out, &mut ev);
                self.rage_on_at = now;
            }
            // Melee bookkeeping (ER plays the swing)
            if melee_weapon && !axe_away && !inp.aim_held {
                if inp.light && inp.guard_held && shield_ok {
                    self.melee.start(d.mv("shield_bash"), now);
                } else if inp.light {
                    self.melee.light_pressed(d, weapon, f, now);
                } else if inp.heavy {
                    self.melee.heavy_pressed(d, weapon, now);
                }
            }
            // Runics: D-pad up / down casts; with L1 held it changes the runic in that slot instead (once per press)
            for (i, id) in ["b_runic_light", "b_runic_heavy"].iter().enumerate() {
                if input::chord_down(d, d.binding(id), f) {
                    self.dpad_seen[i] = now;
                } else if now - self.dpad_seen[i] > DPAD_RELEASE_S {
                    self.cycle_armed[i] = true;
                }
            }
            if melee_weapon && (inp.runic_light || inp.runic_heavy) {
                let tier = if inp.runic_light { RunicTier::Light } else { RunicTier::Heavy };
                let slot = if inp.runic_light { 0 } else { 1 };
                if inp.guard_held {
                    if self.cycle_armed[slot] {
                        self.cycle_armed[slot] = false;
                        self.cycle_runic(tier, now, &mut out, &mut ev);
                    }
                } else if let Some(r) = self.equipped(weapon, tier) {
                    self.runics.press(r, now, &mut out);
                }
            }
        }

        // ---- finisher ------------------------------------------------------------------------------------------
        let finisher_target = if controllable { self.stun.finisher_target(d, f, now) } else { None };
        if controllable && inp.finisher_pressed {
            if let Some(id) = finisher_target {
                if let Some(e) = f.enemies.iter().find(|e| e.id == id) {
                    let rage = self.rage.active;
                    let dmg = self.stun.finisher_damage(e, rage);
                    out.push(Command::Damage { target: id, amount: dmg, source: DamageSource::Finisher });
                    out.push(Command::Finisher { target: id, rage });
                    out.push(Command::Shake(if rage { 1.6 } else { 1.2 }));
                    self.finisher_at = now;
                    let mv = d.mv(if rage { "rage_grab" } else { "finisher_grab" });
                    if let Some(s) = mv.sfx {
                        out.push(Command::Sfx(s));
                    }
                    if rage {
                        out.push(Command::Heal { amount: f.player.max_hp * 0.05 });
                    }
                    gains.rage += d.mv("finisher_grab").rage_gain;
                    gains.player_hit.push(id);
                    self.stun.clear(id);
                    ev.push(Event::Finisher { enemy: id });
                }
            }
        }

        // ---- subsystems -------------------------------------------------------------------------------------------------
        let dmg_mult = self.rage.damage_mult();
        let stun_mult = self.rage.stun_mult();
        if controllable || !self.thrown.axe_in_hand() {
            self.thrown.update(f, &inp, weapon, now, dmg_mult, stun_mult, &mut out, &mut ev, &mut gains);
        }
        let stun_adds = self.runics.update(f, now, dmg_mult, stun_mult, &mut out, &mut ev);
        gains.stun.extend(stun_adds);
        self.rage.update(f, &mut out, &mut ev);

        // gains → rage meter, stun meters (a filled meter staggers + sounds)
        for id in std::mem::take(&mut gains.player_hit) {
            self.stun.note_player_hit(id, now);
        }
        if gains.rage > 0.05 && !self.rage.active {
            self.rage.gain(gains.rage);
            logs.push(format!("rage +{:.1} (x{:.2}) -> {:.0}/{:.0}", gains.rage * self.rage.gain_mult, self.rage.gain_mult, self.rage.meter, self.rage.max()));
        } else {
            self.rage.gain(gains.rage);
        }
        for (id, kind, pts, secs) in std::mem::take(&mut gains.status) {
            self.build_status(f, id, kind, pts, secs, &mut out, &mut logs);
        }
        for (id, pts) in std::mem::take(&mut gains.stun) {
            self.stun.add(d, id, pts, now, &mut out, &mut ev);
        }
        if self.tuning.wounded_stun {
            self.stun.wounded_check(d, f, now, &mut out, &mut ev);
        }
        self.stun.update(d, f.dt, now);
        self.status.update(f.dt, now, d.tuning.status_decay_after_s, d.tuning.status_decay_per_s);
        if self.status_gc_at + 2.0 < now {
            self.status_gc_at = now;
            let live: HashSet<EnemyId> = f.enemies.iter().filter(|e| e.alive).map(|e| e.id).collect();
            self.status.keep_only(&live);
        }

        // ---- masks (keep ER from reacting to chords we own) -------------------------------------------------------
        let capable = matches!(weapon.kind, WeaponKind::Axe | WeaponKind::Spear);
        let mut mask = ActionBits::NONE;
        if controllable {
            mask = mask
                .union(input::mask_if(d, "b_aim", capable, f))
                .union(input::mask_if(d, "b_throw", self.thrown.aiming, f))
                .union(input::mask_if(d, "b_recall", self.thrown.recall_available(weapon), f))
                .union(input::mask_if(d, "b_rage", self.rage.ready() || now - self.rage_on_at <= 0.6, f))
                .union(input::mask_if(d, "b_finisher", finisher_target.is_some(), f))
                .union(input::mask_if(d, "b_runic_light", melee_weapon, f))
                .union(input::mask_if(d, "b_runic_heavy", melee_weapon, f));
            if axe_away {
                mask = mask.with(ErAction::R1).with(ErAction::R2);
            }
            // L3 alone is Elden Ring's crouch: with the meter full it is the start of L3 + R3, so a stick pressed a
            // moment before the other does not crouch Kratos instead of raging (R3 alone stays lock-on)
            if self.rage.ready() && f.actions_down.has(ErAction::L3) {
                mask = mask.with(ErAction::L3);
            }
        }
        if !mask.is_empty() {
            out.push(Command::Mask(mask));
        }

        // the player's damage setting scales everything the mod itself deals (before the ledger counts it)
        if (self.tuning.mod_damage - 1.0).abs() > 1e-3 {
            for c in out.iter_mut() {
                if let Command::Damage { amount, .. } = c {
                    *amount *= self.tuning.mod_damage;
                }
            }
        }

        // ---- ledger: whatever we ordered will echo back as damage ----------------------------------------------------
        for c in &out {
            if let Command::Damage { target, amount, .. } = c {
                self.ledger.expect(*target, *amount, now);
            }
        }
        self.ledger.expire(now);

        // ---- combat state & Mimir -------------------------------------------------------------------------------------------
        let in_combat = now - self.last_combat <= d.tuning.combat_quiet_s as f64;
        let mctx = MimirCtx { in_combat, voice_playing: f.voice.is_some() };
        if self.voice_was_playing && f.voice.is_none() {
            self.mimir.idle();
        }
        self.voice_was_playing = f.voice.is_some();
        self.drive_mimir(f, now, first, controllable, in_combat, hp_frac, &ev, mctx, &mut out);
        self.was_in_combat = in_combat;

        // ---- log lines ---------------------------------------------------------------------------------------------------------------
        for e in &ev {
            match e {
                Event::RageOn => logs.push("Spartan Rage ON".into()),
                Event::RageOff => logs.push("Spartan Rage off".into()),
                Event::Parry { .. } => logs.push("perfect parry".into()),
                Event::Kill { enemy, boss } => {
                    logs.push(format!("kill {enemy:#x}{}", if *boss { " (boss)" } else { "" }));
                    if self.rage.active {
                        self.rage.extend(d.rage.kill_extend_s);
                    }
                }
                Event::AxeThrown => logs.push("axe thrown".into()),
                Event::AxeCaught => logs.push("axe caught".into()),
                Event::SpearThrown => logs.push("spear thrown".into()),
                Event::WeaponChanged { weapon } => logs.push(format!("weapon changed to {weapon}")),
                Event::RunicEquipped { runic } => logs.push(format!("{:?} runic for {} is now {}", runic.tier, runic.weapon, runic.name)),
                Event::RunicUsed { runic, hit, damage } => logs.push(format!("runic {} hit {hit} for {damage:.0} each", runic.name)),
                Event::Finisher { enemy } => logs.push(format!("finisher on {enemy}")),
                Event::StunFull { enemy } => logs.push(format!("stun full on {enemy}")),
                Event::SpearsDetonated { spears, hit } => logs.push(format!("{spears} spears detonated, {hit} hits")),
                _ => {}
            }
        }

        // ---- health stones: dropped by the dead, healing Kratos when he walks over one while hurt -----------------------
        self.health_stones(f, now, &mut out, &mut logs);

        // ---- camera & HUD ------------------------------------------------------------------------------------------------------
        let since = (now - self.finisher_at) as f32;
        let punch = if since < 0.0 || since > 1.2 { 0.0 } else if since < 0.12 { since / 0.12 } else { (-(since - 0.12) / 0.35).exp() };
        let camera = self.camera.update(f.dt, self.thrown.aiming, controllable, punch);
        let hud = self.build_hud(f, now, hp_frac, in_combat, finisher_target, &inp);

        let equipped = ev.iter().filter_map(|e| if let Event::RunicEquipped { runic } = e { Some(runic.id) } else { None }).collect();
        TickOutput { commands: out, hud, camera, logs, projectiles: self.thrown.positions(), equipped }
    }

    fn mimir_ctx(&self, f: &Frame, now: f64) -> MimirCtx {
        MimirCtx { in_combat: now - self.last_combat <= self.d.tuning.combat_quiet_s as f64, voice_playing: f.voice.is_some() }
    }

    fn on_player_died(&mut self, out: &mut Vec<Command>) {
        self.rage = Rage::new(self.d);
        self.rage.meter = 0.0;
        self.thrown.reset();
        self.melee.reset();
        self.runics.reset_all();
        self.guard_started = None;
        out.push(Command::AnimSpeed(1.0));
    }

    #[allow(clippy::too_many_arguments)]
    fn drive_mimir(&mut self, f: &Frame, now: f64, first: f64, controllable: bool, in_combat: bool, hp_frac: f32, ev: &[Event], ctx: MimirCtx, out: &mut Vec<Command>) {
        let t = self.d.tuning;
        if !self.greeted && controllable && now - first >= t.greeting_delay_s as f64 {
            self.greeted = true;
            self.mimir.event(MimirTriggerEvent::GameStart, now, ctx, out);
        }
        if in_combat && !self.was_in_combat {
            self.mimir.event(MimirTriggerEvent::CombatStart, now, ctx, out);
        }
        if in_combat || !controllable {
            self.last_idle_fire = now;
        } else if now - self.last_idle_fire >= t.idle_after_s as f64 {
            self.last_idle_fire = now;
            self.mimir.event(MimirTriggerEvent::Idle, now, ctx, out);
        }
        // low health with hysteresis
        let low = t.hp_low_pct / 100.0;
        if !self.hp_low && hp_frac < low && f.player.alive {
            self.hp_low = true;
            self.mimir.event(MimirTriggerEvent::HpLow, now, ctx, out);
        } else if self.hp_low && hp_frac > t.hp_low_clear_pct / 100.0 {
            self.hp_low = false;
        }
        // boss comes into view
        for e in f.enemies.iter().filter(|e| e.boss && e.alive) {
            if e.pos.dist(f.player.pos) <= t.boss_sight_range_m && self.bosses_seen.insert(e.id) {
                self.mimir.event(MimirTriggerEvent::BossSight, now, ctx, out);
            }
        }
        // new area
        match self.area {
            None => self.area = Some(f.area),
            Some(a) if a != f.area => {
                self.area = Some(f.area);
                self.mimir.event(MimirTriggerEvent::NewArea, now, ctx, out);
            }
            _ => {}
        }
        for e in ev {
            match e {
                Event::Kill { boss, .. } => {
                    if *boss {
                        self.mimir.event(MimirTriggerEvent::BossDown, now, ctx, out);
                    } else {
                        self.mimir.event(MimirTriggerEvent::Kill, now, ctx, out);
                    }
                    self.kills.push_back(now);
                    while self.kills.front().map(|k| now - k > t.kill_streak_window_s as f64).unwrap_or(false) {
                        self.kills.pop_front();
                    }
                    if self.kills.len() >= t.kill_streak_count as usize {
                        self.kills.clear();
                        self.mimir.event(MimirTriggerEvent::KillStreak, now, ctx, out);
                    }
                }
                Event::RageOn => {
                    self.mimir.event(MimirTriggerEvent::RageOn, now, ctx, out);
                }
                Event::Parry { .. } => {
                    self.mimir.event(MimirTriggerEvent::Parry, now, ctx, out);
                }
                Event::AxeThrown => {
                    self.mimir.event(MimirTriggerEvent::AxeThrow, now, ctx, out);
                }
                Event::RunicUsed { hit, .. } if *hit >= t.runic_min_targets_voice as usize => {
                    self.mimir.event(MimirTriggerEvent::RunicHit, now, ctx, out);
                }
                _ => {}
            }
        }
    }

    fn health_stones(&mut self, f: &Frame, now: f64, out: &mut Vec<Command>, logs: &mut Vec<String>) {
        let t = &self.d.tuning;
        if !self.tuning.health_stones {
            self.stones.clear();
            return;
        }
        // a hostile seen alive that is now dead near Kratos (Elden Ring does not always say who struck the blow,
        // so the kill credit is not asked for)
        for e in &f.enemies {
            if e.alive && e.hp > 0.0 {
                continue;
            }
            if !self.stone_alive.remove(&e.id) {
                continue;
            }
            let dist = (e.pos - f.player.pos).len();
            if dist > t.health_stone_drop_m {
                continue;
            }
            // a repeatable roll from the enemy's id: one stone with the sheet's chance, a boss drops several
            let roll = (e.id.wrapping_mul(0x9E37_79B9_7F4A_7C15) >> 40) as f32 / (1u64 << 24) as f32;
            let n = if e.boss { t.health_stone_boss_count.max(0) as usize } else if roll < t.health_stone_chance { 1 } else { 0 };
            for k in 0..n {
                let a = k as f32 * std::f32::consts::TAU / n.max(1) as f32 + roll * 6.0;
                let r = if n > 1 { 1.2 } else { 0.0 };
                self.stones.push(Stone { pos: e.pos + Vec3::new(r * a.cos(), 0.0, r * a.sin()), born: now });
            }
            logs.push(format!("health stones: {:#x} died {dist:.1} m away{}; {n} dropped", e.id, if e.boss { " (boss)" } else { "" }));
        }
        self.stone_alive = f.enemies.iter().filter(|e| e.alive && e.hp > 0.0).map(|e| e.id).collect();
        let life = t.health_stone_life_s as f64;
        self.stones.retain(|s| now - s.born < life);
        let hurt = f.player.alive && f.player.hp < f.player.max_hp - 0.5;
        if hurt {
            let reach = t.health_stone_pickup_m;
            if let Some(i) = self.stones.iter().position(|s| (s.pos - f.player.pos).flat().len() <= reach) {
                self.stones.swap_remove(i);
                let amount = f.player.max_hp * t.health_stone_heal_pct / 100.0;
                out.push(Command::Heal { amount });
                out.push(Command::Sfx("sfx_runic_ready"));
                logs.push(format!("health stone: +{amount:.0} hp"));
            }
        }
    }

    fn build_hud(&mut self, f: &Frame, now: f64, hp_frac: f32, in_combat: bool, finisher_target: Option<EnemyId>, inp: &Inputs) -> HudState {
        let d = self.d;
        let weapon = self.weapon;
        // The bars ease to their real values, like Ragnarök's: a loss drops quickly and leaves a pale trail that waits
        // a moment and then drains; a gain (healing, Rage) fills smoothly.
        // Measured from a Ragnarök fight: health drops at once and the chunk just lost turns red, holds about 0.65 s and
        // fades; a hit while it shows keeps its top. Healing and Rage gains ease in over about a quarter second.
        let t = &d.tuning;
        if !self.hud_started {
            self.hud_started = true;
            (self.hp_shown, self.hp_trail, self.rage_shown) = (hp_frac, hp_frac, self.rage.frac());
        }
        let chunk_gone = now - self.hp_hit_at >= (t.hp_trail_hold_s + t.hp_trail_s) as f64;
        if hp_frac < self.hp_shown - 0.001 {
            if chunk_gone {
                self.hp_trail = self.hp_shown;
            }
            self.hp_hit_at = now;
        }
        let tau = if hp_frac < self.hp_shown { t.bar_ease_down_s } else { t.bar_ease_up_s };
        self.hp_shown = ease(self.hp_shown, hp_frac, f.dt, tau);
        if self.hp_shown >= self.hp_trail || (chunk_gone && now - self.hp_hit_at > 0.0) {
            self.hp_trail = self.hp_shown;
        }
        self.rage_shown = ease(self.rage_shown, self.rage.frac(), f.dt, t.bar_ease_up_s);

        let runic = |r: Option<&'static RunicDef>| {
            r.map(|r| {
                let w = d.weapon(r.weapon);
                let flash = (1.0 - (now - self.runics.changed_at(r)) / 0.45).clamp(0.0, 1.0) as f32;
                RunicHud { id: r.id, name: r.name, remaining_frac: self.runics.cooldown_frac(r), ready: self.runics.ready(r), color: w.runic_color, light: w.runic_light, glow: w.runic_glow, flash }
            })
        };
        let bar = |e: &EnemyView, st: &StunTracker| EnemyBar {
            id: e.id,
            pos: e.pos,
            hp_frac: if e.max_hp > 0.0 { (e.hp / e.max_hp).clamp(0.0, 1.0) } else { 0.0 },
            stun_frac: st.frac(e.id),
            stunned: st.is_stunned(e.id, now),
            boss: e.boss,
            status_frac: self.status.frac(e.id).0,
            status_label: match self.status.active(e.id, now) {
                Some(WeaponStatus::Frozen) => Some("FROZEN"),
                Some(WeaponStatus::Burning) => Some("BURNING"),
                _ => None,
            },
            status_color: match self.status.frac(e.id).1 {
                WeaponStatus::Frozen => 0x7FD8FF,
                WeaponStatus::Burning => 0xFF8A3A,
                WeaponStatus::None => 0,
            },
        };
        let mut enemies = Vec::new();
        let mut boss = None;
        for e in f.enemies.iter().filter(|e| e.alive && e.pos.dist(f.player.pos) <= d.tuning.scan_range_m) {
            let recently_hit = self.stun.enemies.get(&e.id).map(|s| now - s.player_hit_at <= 8.0).unwrap_or(false);
            let show = e.boss || recently_hit || (e.engaged && e.pos.dist(f.player.pos) <= d.tuning.bar_range_m);
            if !show {
                continue;
            }
            let b = bar(e, &self.stun);
            if e.boss {
                if boss.is_none() {
                    boss = Some(b.clone());
                }
            }
            enemies.push(b);
        }

        let recall_available = self.thrown.recall_available(weapon);
        let rage_overlay = if self.rage.active {
            let ramp_in = ((now - self.rage_on_at) / 0.3).clamp(0.0, 1.0) as f32;
            ramp_in * (self.rage.time_left / 2.0).clamp(0.0, 1.0)
        } else {
            0.0
        };
        let low = d.tuning.hp_low_pct / 100.0;
        let low_health_overlay = if f.player.alive && hp_frac < low { ((low - hp_frac) / low).clamp(0.0, 1.0) } else { 0.0 };
        let voice = f.voice.as_ref();

        HudState {
            hp_frac: self.hp_shown,
            hp_trail_frac: self.hp_trail,
            hp_hit_age: (now - self.hp_hit_at).min(1000.0) as f32,
            rage_flash: if self.rage.active { (1.0 - (now - self.rage_on_at) / 0.55).clamp(0.0, 1.0) as f32 } else { 0.0 },
            weapon_disc: weapon.disc_color,
            rage_frac: self.rage_shown,
            rage_ready: self.rage.ready(),
            rage_active: self.rage.active,
            runic_light: runic(self.equipped(weapon, RunicTier::Light)),
            runic_heavy: runic(self.equipped(weapon, RunicTier::Heavy)),
            runic_picker: self.picker.filter(|(_, until)| now < *until).and_then(|(tier, until)| {
                let cur = self.equipped(weapon, tier)?;
                let opts = self.runic_options(weapon.id, tier);
                Some(RunicPicker {
                    heavy: tier == RunicTier::Heavy,
                    weapon_name: weapon.name,
                    chosen: opts.iter().position(|r| r.id == cur.id).unwrap_or(0),
                    options: opts.iter().map(|r| (r.name, r.icon, r.effect)).collect(),
                    colors: (weapon.runic_color, weapon.runic_light, weapon.runic_glow),
                    alpha: ((until - now) / 0.4).clamp(0.0, 1.0) as f32,
                })
            }),
            time_s: (now % 3600.0) as f32,
            weapon_icon: weapon.icon,
            weapon_name: weapon.name,
            shield_drawn: f.shield_drawn && weapon.kind != WeaponKind::Blades,
            aiming: self.thrown.aiming || (inp.aim_held && matches!(weapon.kind, WeaponKind::Axe | WeaponKind::Spear)),
            recall_available,
            recall_label: if weapon.kind == WeaponKind::Spear { "Detonate" } else { "Recall" },
            enemies,
            boss,
            finisher_target,
            subtitle: voice.and_then(|v| v.subtitle.clone()),
            mimir_speaking: voice.is_some(),
            rage_overlay,
            low_health_overlay,
            in_combat,
            pulse: (now % 1.2) as f32 / 1.2,
            ui_scale: 1.0,
            // the renderer fills in which of Ragnarök's pieces it has
            art: 0,
            stones: self.stones.iter().map(|s| (s.pos, (d.tuning.health_stone_life_s as f64 - (now - s.born)).max(0.0) as f32)).collect(),
        }
    }
}

impl Default for Engine {
    fn default() -> Self {
        Self::new()
    }
}
