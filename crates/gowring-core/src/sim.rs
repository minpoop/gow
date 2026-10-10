//! A tiny stand-in for Elden Ring, used by the tests and the `gowring-sim` tool.
//!
//! It plays the host's part of the contract: it feeds frames, applies commands, echoes damage the
//! engine ordered back as `Dealt` events (the way the DLL sees HP drop), answers raycasts and
//! produces Elden-Ring-like melee hits when the player presses R1/R2. It proves the rules; it does
//! not prove the game integration (see docs/STATUS.md).

use crate::data::Data;
use crate::engine::{Engine, TickOutput};
use crate::sheets::{ErAction, WeaponKind};
use crate::types::*;

#[derive(Clone, Debug)]
pub struct SimEnemy {
    pub id: EnemyId,
    pub pos: Vec3,
    pub hp: f32,
    pub max_hp: f32,
    pub boss: bool,
    pub radius: f32,
    pub stagger_until: f64,
    /// Attack the player every N seconds when within 3 m.
    pub attack_every: Option<f64>,
    pub next_attack: f64,
    pub attack_damage: f32,
}

impl SimEnemy {
    pub fn new(id: EnemyId, x: f32, z: f32, hp: f32) -> Self {
        SimEnemy { id, pos: Vec3::new(x, 0.0, z), hp, max_hp: hp, boss: false, radius: 0.6, stagger_until: f64::NEG_INFINITY, attack_every: None, next_attack: 0.0, attack_damage: 0.0 }
    }
    pub fn alive(&self) -> bool {
        self.hp > 0.0
    }
}

pub struct SimWorld {
    pub engine: Engine,
    pub time: f64,
    pub dt: f32,
    pub player: PlayerView,
    pub enemies: Vec<SimEnemy>,
    pub right_slot: u8,
    pub shield_drawn: bool,
    pub aim_dir: Vec3,
    pub area: u32,
    /// Infinite wall at this z (thrown weapons stick in it).
    pub wall_z: Option<f32>,
    pub rage_damage_taken: f32,
    // state carried between frames
    held: ActionBits,
    echo: Vec<DamageEvent>,
    rays: Vec<RayResult>,
    scheduled_hits: Vec<(f64, f32, f32)>, // (time, damage, reach)
    voice: Option<(VoiceInfo, f64)>,
    // observations for assertions
    pub log: Vec<String>,
    pub sfx: Vec<&'static str>,
    pub voices: Vec<&'static str>,
    pub last_mask: ActionBits,
    pub all_commands: Vec<Command>,
    pub anim_speed: f32,
    pub staggers: Vec<(EnemyId, f64)>,
    pub finishers: Vec<EnemyId>,
    pub casts: Vec<&'static str>,
}

impl SimWorld {
    pub fn new() -> Self {
        SimWorld {
            engine: Engine::new(),
            time: 0.0,
            dt: 1.0 / 60.0,
            player: PlayerView::default(),
            enemies: vec![],
            right_slot: 0,
            shield_drawn: true,
            aim_dir: Vec3::new(0.0, 0.0, 1.0),
            area: 1,
            wall_z: None,
            rage_damage_taken: 0.0,
            held: ActionBits::NONE,
            echo: vec![],
            rays: vec![],
            scheduled_hits: vec![],
            voice: None,
            log: vec![],
            sfx: vec![],
            voices: vec![],
            last_mask: ActionBits::NONE,
            all_commands: vec![],
            anim_speed: 1.0,
            staggers: vec![],
            finishers: vec![],
            casts: vec![],
        }
    }

    pub fn add_enemy(&mut self, e: SimEnemy) -> EnemyId {
        let id = e.id;
        self.enemies.push(e);
        id
    }

    pub fn enemy(&self, id: EnemyId) -> &SimEnemy {
        self.enemies.iter().find(|e| e.id == id).expect("enemy")
    }

    fn weapon_base_damage(&self) -> f32 {
        Data::get().right_weapon(self.right_slot).base_damage as f32
    }

    /// Advance one frame. `pressed` are actions that became down this frame, `hold` stay down;
    /// actions not mentioned are released.
    pub fn step(&mut self, pressed: &[ErAction], hold: &[ErAction]) -> TickOutput {
        let d = Data::get();
        let mut down = ActionBits::NONE;
        let mut newly = ActionBits::NONE;
        for a in hold {
            down = down.with(*a);
        }
        for a in pressed {
            down = down.with(*a);
            newly = newly.with(*a);
        }
        let released = ActionBits(self.held.0 & !down.0);
        self.held = down;

        // --- stand-in for ER's native melee ------------------------------------------------------------
        let weapon = d.right_weapon(self.right_slot);
        if matches!(weapon.kind, WeaponKind::Axe | WeaponKind::Blades | WeaponKind::Spear) && !self.last_mask.has(ErAction::R1) {
            if newly.has(ErAction::R1) && !down.has(ErAction::L2) {
                self.scheduled_hits.push((self.time + 0.25, self.weapon_base_damage(), 3.5));
            }
            if newly.has(ErAction::R2) {
                self.scheduled_hits.push((self.time + 0.5, self.weapon_base_damage() * 2.0, 3.5));
            }
        }
        let mut damage: Vec<DamageEvent> = std::mem::take(&mut self.echo);
        let now = self.time;
        let facing = self.player.facing;
        let ppos = self.player.pos;
        let due: Vec<(f64, f32, f32)> = self.scheduled_hits.iter().copied().filter(|h| h.0 <= now).collect();
        self.scheduled_hits.retain(|h| h.0 > now);
        for (_, dmg, reach) in due {
            for e in self.enemies.iter_mut().filter(|e| e.hp > 0.0) {
                let to = (e.pos - ppos).flat();
                if to.len() <= reach + e.radius && facing.angle_deg(to) <= 70.0 {
                    let amount = dmg.min(e.hp);
                    e.hp -= amount;
                    damage.push(DamageEvent::Dealt { enemy: e.id, amount });
                }
            }
        }

        // --- enemy attacks ----------------------------------------------------------------------------------------
        let guard = down.has(ErAction::Guard) || down.has(ErAction::L1);
        for e in self.enemies.iter_mut().filter(|e| e.hp > 0.0) {
            if let Some(every) = e.attack_every {
                if now >= e.next_attack && now >= e.stagger_until && e.pos.dist(ppos) <= 3.0 {
                    e.next_attack = now + every;
                    if guard && self.shield_drawn {
                        damage.push(DamageEvent::Taken { amount: 0.0, blocked: true, attacker: Some(e.id) });
                    } else {
                        let a = e.attack_damage.min(self.player.hp);
                        self.player.hp -= a;
                        self.rage_damage_taken += a;
                        damage.push(DamageEvent::Taken { amount: a, blocked: false, attacker: Some(e.id) });
                    }
                }
            }
        }
        if self.player.hp <= 0.0 {
            self.player.alive = false;
        }

        // --- build the frame ---------------------------------------------------------------------------------------
        let frame = Frame {
            dt: self.dt,
            time: self.time,
            player: self.player.clone(),
            enemies: self
                .enemies
                .iter()
                .map(|e| EnemyView { id: e.id, pos: e.pos, hp: e.hp.max(0.0), max_hp: e.max_hp, alive: e.hp > 0.0, boss: e.boss, engaged: true, radius: e.radius, staggered: now < e.stagger_until })
                .collect(),
            actions_down: down,
            actions_pressed: newly,
            actions_released: released,
            aim_dir: self.aim_dir,
            camera_pos: Vec3::ZERO,
            right_slot: self.right_slot,
            shield_drawn: self.shield_drawn,
            area: self.area,
            damage,
            rays: std::mem::take(&mut self.rays),
            voice: self.voice.as_ref().filter(|(_, end)| *end > now).map(|(v, end)| VoiceInfo { remaining_s: (*end - now) as f32, ..v.clone() }),
        };
        let out = self.engine.tick(&frame);

        // --- apply commands ------------------------------------------------------------------------------------------------
        self.last_mask = ActionBits::NONE;
        for c in &out.commands {
            match c {
                Command::Damage { target, amount, .. } => {
                    if let Some(e) = self.enemies.iter_mut().find(|e| e.id == *target) {
                        let a = amount.min(e.hp.max(0.0));
                        if a > 0.0 {
                            e.hp -= a;
                            self.echo.push(DamageEvent::Dealt { enemy: *target, amount: a });
                        }
                    }
                }
                Command::Stagger { target, seconds } => {
                    if let Some(e) = self.enemies.iter_mut().find(|e| e.id == *target) {
                        e.stagger_until = e.stagger_until.max(self.time + *seconds as f64);
                    }
                    self.staggers.push((*target, self.time));
                }
                Command::Heal { amount } => self.player.hp = (self.player.hp + amount).min(self.player.max_hp),
                Command::Refund { amount } => self.player.hp = (self.player.hp + amount).min(self.player.max_hp),
                Command::Mask(m) => self.last_mask = *m,
                Command::AnimSpeed(s) => self.anim_speed = *s,
                Command::Sfx(s) => self.sfx.push(s),
                Command::Voice { set, .. } => {
                    self.voices.push(set);
                    self.voice = Some((VoiceInfo { set: (*set).into(), remaining_s: 3.0, subtitle: Some(format!("[{set}]")) }, self.time + 3.0));
                }
                Command::Raycast { tag, from, to } => {
                    let hit = self.wall_z.and_then(|wz| {
                        if (from.z - wz) * (to.z - wz) <= 0.0 && (to.z - from.z).abs() > 1e-6 {
                            let t = (wz - from.z) / (to.z - from.z);
                            Some(*from + (*to - *from) * t)
                        } else {
                            None
                        }
                    });
                    self.rays.push(RayResult { tag: *tag, hit });
                }
                Command::Finisher { target, .. } => self.finishers.push(*target),
                Command::Cast { runic } => self.casts.push(runic),
                Command::Status { .. } | Command::Fx { .. } | Command::Shake(_) => {}
            }
        }
        self.log.extend(out.logs.iter().cloned());
        self.all_commands.extend(out.commands.iter().cloned());
        self.time += self.dt as f64;
        out
    }

    pub fn idle(&mut self, secs: f64) -> TickOutput {
        let n = (secs / self.dt as f64).round().max(1.0) as usize;
        let mut last = self.step(&[], &[]);
        for _ in 1..n {
            last = self.step(&[], &[]);
        }
        last
    }

    /// Press `a` for one frame, then idle a short beat.
    pub fn tap(&mut self, a: ErAction) -> TickOutput {
        let o = self.step(&[a], &[]);
        self.step(&[], &[]);
        o
    }
}

impl Default for SimWorld {
    fn default() -> Self {
        Self::new()
    }
}
