//! The rules engine: one `tick` per frame turns a `Frame` into `Command`s, HUD state and a camera request.

use crate::camera::Camera;
use crate::data::Data;
use crate::hud::{EnemyBar, HudState, RunicHud};
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
use std::collections::{HashSet, VecDeque};

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
}

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
}

impl Default for Tuning {
    fn default() -> Self {
        Tuning { rage_gain: 1.0, mod_damage: 1.0, mimir_chance: 1.0, enemy_health: 1.0, wounded_stun: true }
    }
}

pub struct Engine {
    d: &'static Data,
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
        }
    }

    pub fn rage(&self) -> &Rage {
        &self.rage
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
                self.rage.activate(f, base, d.mv(d.rage.burst_move), &mut out, &mut ev);
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
            // Runics
            if melee_weapon {
                if inp.runic_light {
                    self.runics.press(RunicTier::Light, weapon.id, now, &mut out);
                } else if inp.runic_heavy {
                    self.runics.press(RunicTier::Heavy, weapon.id, now, &mut out);
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
                Event::RunicUsed { runic, hit, damage } => logs.push(format!("runic {} hit {hit} for {damage:.0} each", runic.name)),
                Event::Finisher { enemy } => logs.push(format!("finisher on {enemy}")),
                Event::StunFull { enemy } => logs.push(format!("stun full on {enemy}")),
                Event::SpearsDetonated { spears, hit } => logs.push(format!("{spears} spears detonated, {hit} hits")),
                _ => {}
            }
        }

        // ---- camera & HUD ------------------------------------------------------------------------------------------------------
        let since = (now - self.finisher_at) as f32;
        let punch = if since < 0.0 || since > 1.2 { 0.0 } else if since < 0.12 { since / 0.12 } else { (-(since - 0.12) / 0.35).exp() };
        let camera = self.camera.update(f.dt, self.thrown.aiming, controllable, punch);
        let hud = self.build_hud(f, now, hp_frac, in_combat, finisher_target, &inp);

        TickOutput { commands: out, hud, camera, logs, projectiles: self.thrown.positions() }
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

    fn build_hud(&mut self, f: &Frame, now: f64, hp_frac: f32, in_combat: bool, finisher_target: Option<EnemyId>, inp: &Inputs) -> HudState {
        let d = self.d;
        let weapon = self.weapon;
        // delayed damage trail
        let fall = if d.tuning.hp_trail_s > 0.0 { f.dt / d.tuning.hp_trail_s } else { 1.0 };
        self.hp_trail = if hp_frac >= self.hp_trail { hp_frac } else { (self.hp_trail - fall).max(hp_frac) };

        let runic = |id: Option<&'static str>| {
            id.map(|id| {
                let r = d.runic(id);
                RunicHud { id: r.id, name: r.name, remaining_frac: self.runics.cooldown_frac(r), ready: self.runics.ready(r) }
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
            hp_frac,
            hp_trail_frac: self.hp_trail,
            rage_frac: self.rage.frac(),
            rage_ready: self.rage.ready(),
            rage_active: self.rage.active,
            runic_light: runic(weapon.light_runic),
            runic_heavy: runic(weapon.heavy_runic),
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
        }
    }
}

impl Default for Engine {
    fn default() -> Self {
        Self::new()
    }
}
