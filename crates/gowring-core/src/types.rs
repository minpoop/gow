//! Plain data crossing the host boundary.

use crate::sheets::{ErAction, FxKind, RunicStatus};
use std::ops::{Add, Mul, Sub};

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Vec3 {
    pub x: f32,
    pub y: f32,
    pub z: f32,
}

impl Vec3 {
    pub const ZERO: Vec3 = Vec3 { x: 0.0, y: 0.0, z: 0.0 };
    pub const fn new(x: f32, y: f32, z: f32) -> Self {
        Vec3 { x, y, z }
    }
    pub fn dot(self, o: Vec3) -> f32 {
        self.x * o.x + self.y * o.y + self.z * o.z
    }
    pub fn len(self) -> f32 {
        self.dot(self).sqrt()
    }
    pub fn dist(self, o: Vec3) -> f32 {
        (self - o).len()
    }
    pub fn flat(self) -> Vec3 {
        Vec3::new(self.x, 0.0, self.z)
    }
    pub fn normalized(self) -> Vec3 {
        let l = self.len();
        if l < 1e-6 {
            Vec3::ZERO
        } else {
            self * (1.0 / l)
        }
    }
    pub fn lerp(self, o: Vec3, t: f32) -> Vec3 {
        self + (o - self) * t
    }
    /// Angle in degrees between two directions (0 when either is zero).
    pub fn angle_deg(self, o: Vec3) -> f32 {
        let (a, b) = (self.normalized(), o.normalized());
        if a == Vec3::ZERO || b == Vec3::ZERO {
            return 0.0;
        }
        a.dot(b).clamp(-1.0, 1.0).acos().to_degrees()
    }
}

impl Add for Vec3 {
    type Output = Vec3;
    fn add(self, o: Vec3) -> Vec3 {
        Vec3::new(self.x + o.x, self.y + o.y, self.z + o.z)
    }
}
impl Sub for Vec3 {
    type Output = Vec3;
    fn sub(self, o: Vec3) -> Vec3 {
        Vec3::new(self.x - o.x, self.y - o.y, self.z - o.z)
    }
}
impl Mul<f32> for Vec3 {
    type Output = Vec3;
    fn mul(self, k: f32) -> Vec3 {
        Vec3::new(self.x * k, self.y * k, self.z * k)
    }
}

/// Elden Ring's `ChrActions` bit set (`CSChrActionRequestModule`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ActionBits(pub u64);

impl ActionBits {
    pub const NONE: ActionBits = ActionBits(0);
    /// ER's bit number for an action (fixed by the game's `ChrActions`).
    pub fn bit(a: ErAction) -> u64 {
        1u64 << match a {
            ErAction::R1 => 0,
            ErAction::R2 => 1,
            ErAction::L1 => 2,
            ErAction::L2 => 3,
            ErAction::Action => 4,
            ErAction::SpMove => 5,
            ErAction::Jump => 6,
            ErAction::UseItem => 7,
            ErAction::SwitchForm => 8,
            ErAction::ChangeWeaponR => 9,
            ErAction::ChangeWeaponL => 10,
            ErAction::ChangeItem => 11,
            ErAction::R3 => 12,
            ErAction::L3 => 13,
            ErAction::Guard => 24,
            ErAction::ChangeStyleR => 28,
            ErAction::ChangeStyleL => 29,
        }
    }
    pub fn has(self, a: ErAction) -> bool {
        self.0 & Self::bit(a) != 0
    }
    pub fn with(self, a: ErAction) -> Self {
        ActionBits(self.0 | Self::bit(a))
    }
    pub fn union(self, o: ActionBits) -> Self {
        ActionBits(self.0 | o.0)
    }
    pub fn is_empty(self) -> bool {
        self.0 == 0
    }
}

pub type EnemyId = u64;

#[derive(Clone, Debug)]
pub struct PlayerView {
    pub pos: Vec3,
    /// Unit vector on the ground plane the character faces.
    pub facing: Vec3,
    pub hp: f32,
    pub max_hp: f32,
    pub alive: bool,
    pub grounded: bool,
    pub sprinting: bool,
    /// False during cutscenes, menus, loading, death screens.
    pub controllable: bool,
}

impl Default for PlayerView {
    fn default() -> Self {
        PlayerView {
            pos: Vec3::ZERO,
            facing: Vec3::new(0.0, 0.0, 1.0),
            hp: 2500.0,
            max_hp: 2500.0,
            alive: true,
            grounded: true,
            sprinting: false,
            controllable: true,
        }
    }
}

#[derive(Clone, Debug)]
pub struct EnemyView {
    pub id: EnemyId,
    pub pos: Vec3,
    pub hp: f32,
    pub max_hp: f32,
    pub alive: bool,
    /// ER shows this character a boss health bar.
    pub boss: bool,
    /// Currently targeting / hunting the player.
    pub engaged: bool,
    pub radius: f32,
    /// ER's own hit-reaction state (poise broken / staggered).
    pub staggered: bool,
}

#[derive(Clone, Debug)]
pub enum DamageEvent {
    /// The player took `amount` HP of damage dealt to `enemy`.
    Dealt { enemy: EnemyId, amount: f32 },
    /// The player took `amount` HP (0 when blocked) from `attacker`.
    /// `blocked` is true when the hit landed on a raised guard.
    Taken { amount: f32, blocked: bool, attacker: Option<EnemyId> },
}

#[derive(Clone, Debug)]
pub struct RayResult {
    pub tag: u32,
    pub hit: Option<Vec3>,
}

#[derive(Clone, Debug, Default)]
pub struct VoiceInfo {
    pub set: String,
    pub remaining_s: f32,
    pub subtitle: Option<String>,
}

/// One frame of the world, as the host sees it.
#[derive(Clone, Debug, Default)]
pub struct Frame {
    pub dt: f32,
    /// Seconds since the mod started (monotonic).
    pub time: f64,
    pub player: PlayerView,
    pub enemies: Vec<EnemyView>,
    pub actions_down: ActionBits,
    pub actions_pressed: ActionBits,
    pub actions_released: ActionBits,
    /// Camera forward direction (unit).
    pub aim_dir: Vec3,
    pub camera_pos: Vec3,
    /// Which right-hand slot is active: 0 Axe, 1 Blades, 2 Spear.
    pub right_slot: u8,
    pub shield_drawn: bool,
    /// Named-region id; a change means "entered a new area".
    pub area: u32,
    pub damage: Vec<DamageEvent>,
    pub rays: Vec<RayResult>,
    pub voice: Option<VoiceInfo>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DamageSource {
    Melee,
    Thrown,
    Runic,
    RageBurst,
    Finisher,
    Blast,
}

#[derive(Clone, Debug)]
pub enum Command {
    /// Deal `amount` HP of damage to an enemy (the host uses engine bullets so ER credits kills).
    Damage { target: EnemyId, amount: f32, source: DamageSource },
    Stagger { target: EnemyId, seconds: f32 },
    /// Multiply a regular enemy's HP and max HP once (the host keeps a record so no enemy is scaled twice).
    ScaleEnemy { target: EnemyId, factor: f32 },
    Status { target: EnemyId, status: RunicStatus, seconds: f32 },
    Heal { amount: f32 },
    /// Give back HP the player lost this frame (parry, rage damage reduction).
    Refund { amount: f32 },
    /// Mask these ER actions this frame.
    Mask(ActionBits),
    AnimSpeed(f32),
    /// Play a converted Ragnarök sound by asset id.
    Sfx(&'static str),
    /// Ask the host to pick and play a Mimir line from this voice set.
    Voice { set: &'static str, no_repeat: usize, priority: i32, interrupt: bool },
    Raycast { tag: u32, from: Vec3, to: Vec3 },
    Fx { kind: FxKind, pos: Vec3, dir: Vec3, radius: f32 },
    /// Execute a stunned enemy (host plays whatever it can: riposte-like hit, camera, fx).
    Finisher { target: EnemyId, rage: bool },
    /// Play the cast pose for a runic.
    Cast { runic: &'static str },
    Shake(f32),
}

/// What the camera should do this frame (host applies on top of ER's follow camera).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct CameraRequest {
    pub shoulder_x_m: f32,
    pub height_m: f32,
    pub distance_m: f32,
    pub fov_deg: f32,
}
