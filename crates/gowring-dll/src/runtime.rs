//! hooks.rules_tick — once per frame: read the world, run `gowring-core`, apply what it decided,
//! and publish the HUD for the overlay.

use crate::assets::Store;
use crate::audio::Audio;
use crate::host::Bridge;
use crate::overlay::Shared;
use crate::params::Carriers;
use eldenring::cs::{CSCamExt, CSCamera, CSFeManHudState, CSFeManImp, WorldChrMan};
use fromsoftware_shared::FromStatic;
use gowring_core::{Command, Data, Engine, Frame, RayResult, VoiceInfo};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Instant;

pub struct Runtime {
    engine: Engine,
    bridge: Bridge,
    audio: Option<Audio>,
    shared: Shared,
    store: Arc<Store>,
    cache: PathBuf,
    start: Instant,
    last: Instant,
    ray_results: Vec<RayResult>,
    carriers: Option<Carriers>,
    params_tried: u32,
    ticks: u64,
    pub camera_req: Arc<Mutex<gowring_core::CameraRequest>>,
    banner: String,
    stats: Stats,
}

#[derive(Default, Debug)]
struct Stats {
    frames: u64,
    commands: u64,
    damage_orders: u64,
    sfx: u64,
    voices: u64,
    masks: u64,
    dealt: u64,
    taken: u64,
}

impl Runtime {
    pub fn new(shared: Shared, store: Arc<Store>, cache: PathBuf, camera_req: Arc<Mutex<gowring_core::CameraRequest>>) -> Self {
        let d = Data::get();
        let banner = format!("GowRing alpha {} - Ragnarok art {}/{}", env!("CARGO_PKG_VERSION"), store.converted, store.total);
        Runtime {
            engine: Engine::new(),
            bridge: Bridge::new(d.tuning.scan_range_m),
            audio: Some(Audio::start(cache.clone())),
            shared,
            store,
            cache,
            start: Instant::now(),
            last: Instant::now(),
            ray_results: Vec::new(),
            carriers: None,
            params_tried: 0,
            ticks: 0,
            camera_req,
            banner,
            stats: Stats::default(),
        }
    }

    pub fn carriers_ready(&self) -> bool {
        self.carriers.is_some()
    }

    /// Patch the params as soon as the repository exists (called every frame from an early task, so the
    /// ten starting classes are already Kratos when the player reaches character creation).
    pub fn ensure_params(&mut self) -> bool {
        if self.carriers.is_some() {
            return true;
        }
        self.params_tried += 1;
        crate::testmode::PARAM_TRIES.store(self.params_tried, std::sync::atomic::Ordering::SeqCst);
        // After many failures (e.g. the param table is never ready) slow down to one try a second.
        if self.params_tried > 600 && self.params_tried % 60 != 0 {
            return false;
        }
        let attempt = std::panic::catch_unwind(std::panic::AssertUnwindSafe(crate::params::apply)).unwrap_or_else(|p| {
            let why = p.downcast_ref::<String>().cloned().or_else(|| p.downcast_ref::<&str>().map(|s| s.to_string())).unwrap_or_else(|| "panic".into());
            Err(format!("panicked: {why}"))
        });
        match attempt {
            Ok(c) => {
                for n in &c.notes {
                    crate::glog!("params", "{n}");
                }
                self.carriers = Some(c);
                crate::testmode::PARAMS_OK.store(true, std::sync::atomic::Ordering::SeqCst);
                true
            }
            Err(e) => {
                crate::testmode::param_error(&e);
                if self.params_tried % 600 == 1 {
                    crate::glog!("params", "not ready yet / failed: {e}");
                }
                false
            }
        }
    }

    /// What the self-checks need to know about the player in the world (cheap; runs every frame).
    fn observe_for_tests(&mut self) {
        use std::sync::atomic::Ordering;
        crate::testmode::PLAYER_SEEN.store(true, Ordering::SeqCst);
        crate::testmode::TICKS_WITH_PLAYER.fetch_add(1, Ordering::Relaxed);
        if crate::testmode::KIT_OK.load(Ordering::SeqCst) || !crate::testmode::active() {
            return;
        }
        let Some(c) = self.carriers.as_ref() else { return };
        let ids = self.bridge.last_equip;
        // weapon param ids carry the upgrade level in the last two digits
        let have = |row: u32| ids.iter().any(|id| *id > 0 && (*id as u32) / 100 == row / 100);
        let missing: Vec<&str> = c.rows.iter().filter(|(_, row)| !have(**row)).map(|(n, _)| *n).collect();
        if missing.is_empty() {
            crate::glog!("kit", "all {} carrier weapons are equipped: {ids:?}", c.rows.len());
            crate::testmode::KIT_OK.store(true, Ordering::SeqCst);
        } else {
            crate::testmode::kit_state(format!("equipped weapon ids {ids:?}; carrier rows {:?}; not equipped: {missing:?}", c.rows));
        }
    }

    pub fn tick(&mut self) {
        // The mod stays inert until Elden Ring has opened the redirected save (see savehook).
        if !crate::savehook::redirected() {
            crate::log::once("gate", "boot_gate", "waiting: no redirected save open seen yet; the mod is inert");
            return;
        }
        if self.carriers.is_none() {
            crate::log::once("params-wait", "params", "waiting for the param patch before running the rules");
            return;
        }
        let now = self.start.elapsed().as_secs_f64();
        let dt = (self.last.elapsed().as_secs_f32()).clamp(0.001, 0.1);
        self.last = Instant::now();

        let voice = self.audio.as_ref().and_then(|a| a.current()).map(|(set, remaining_s, subtitle)| VoiceInfo { set, remaining_s, subtitle });
        let Some(mut frame) = self.bridge.build_frame(now, dt, voice) else {
            // no player (title screen / loading)
            if let Ok(mut s) = self.shared.lock() {
                s.valid = false;
            }
            return;
        };
        frame.rays = std::mem::take(&mut self.ray_results);
        self.observe_for_tests();
        self.upkeep(&frame);

        let out = self.engine.tick(&frame);
        for l in &out.logs {
            crate::glog!("rules", "{l}");
        }
        let out_len = out.commands.len() as u64;
        for c in &out.commands {
            match c {
                Command::Damage { .. } => self.stats.damage_orders += 1,
                Command::Sfx(_) => self.stats.sfx += 1,
                Command::Voice { .. } => self.stats.voices += 1,
                Command::Mask(_) => self.stats.masks += 1,
                _ => {}
            }
        }
        self.stats.dealt += frame.damage.iter().filter(|d| matches!(d, gowring_core::DamageEvent::Dealt { .. })).count() as u64;
        self.stats.taken += frame.damage.iter().filter(|d| matches!(d, gowring_core::DamageEvent::Taken { .. })).count() as u64;
        self.bridge.apply(&out.commands, now, self.audio.as_ref(), &self.cache);
        self.ray_results = self.bridge.cast(&out.commands);
        self.publish(&frame, out.hud, out.camera, out.projectiles);
        self.ticks += 1;
        self.stats.frames += 1;
        self.stats.commands += out_len;
        if self.ticks == 1 {
            crate::glog!("rules_tick", "first tick: player hp {}/{} at ({:.1},{:.1},{:.1}); {} hostile characters", frame.player.hp, frame.player.max_hp, frame.player.pos.x, frame.player.pos.y, frame.player.pos.z, frame.enemies.len());
        }
        if self.ticks % 1800 == 0 {
            crate::glog!("rules_tick", "alive: {} ticks, {} hostiles, hp {}/{} | {:?}", self.ticks, frame.enemies.len(), frame.player.hp, frame.player.max_hp, self.stats);
        }
        // one line of evidence per kind of input/damage the first time it is seen
        if !frame.actions_pressed.is_empty() {
            crate::log::once_u64("act", frame.actions_pressed.0, "action_read", &format!("ER action bits pressed: {:#x} = {} (down {:#x})", frame.actions_pressed.0, action_names(frame.actions_pressed), frame.actions_down.0));
        }
        for d in &frame.damage {
            match d {
                gowring_core::DamageEvent::Dealt { enemy, amount } => crate::log::once("first-dealt", "hit_observe", &format!("first damage dealt: {amount} to {enemy:#x}")),
                gowring_core::DamageEvent::Taken { amount, blocked, attacker } => crate::log::once("first-taken", "hit_observe", &format!("first damage taken: {amount} blocked={blocked} attacker={attacker:?}")),
            }
        }
    }

    /// hp_scale / stamina_free / fp_hide / er_hud_hide.
    fn upkeep(&mut self, frame: &Frame) {
        let d = Data::get();
        if let Ok(wcm) = unsafe { WorldChrMan::instance_mut() } {
            if let Some(p) = wcm.main_player.as_mut() {
                let data = &mut p.chr_ins.modules.data;
                if d.loadout.free_stamina && frame.player.alive {
                    data.stamina = data.max_stamina;
                }
                if d.loadout.hide_fp {
                    data.fp = data.max_fp;
                }
            }
        }
        if let Ok(fe) = unsafe { CSFeManImp::instance_mut() } {
            fe.hud_state = CSFeManHudState::HideAll;
            crate::log::once("hud-hidden", "er_hud_hide", "Elden Ring's own HUD hidden (hud_state = HideAll)");
        }
    }

    fn publish(&mut self, _frame: &Frame, hud: gowring_core::hud::HudState, cam: gowring_core::CameraRequest, projectiles: Vec<(&'static str, gowring_core::Vec3)>) {
        if let Ok(mut c) = self.camera_req.lock() {
            *c = cam;
        }
        let basis = Bridge::camera_basis();
        if let Ok(mut s) = self.shared.lock() {
            s.valid = true;
            s.hud = hud;
            s.projectiles = projectiles;
            if let Some((r, u, f, p, fov)) = basis {
                s.cam_right = r;
                s.cam_up = u;
                s.cam_fwd = f;
                s.cam_pos = p;
                s.fov_rad = fov;
            }
            s.banner = self.banner.clone();
        }
    }
}

/// hooks.camera_offset — runs after ER has placed its follow camera for the frame.
pub fn apply_camera(req: &gowring_core::CameraRequest) {
    const ER_DEFAULT_DISTANCE_M: f32 = 3.8;
    if req.fov_deg <= 1.0 {
        return;
    }
    let Ok(cam) = (unsafe { CSCamera::instance_mut() }) else { return };
    let pc = &mut cam.pers_cam_1;
    let (r, u, f) = (pc.right(), pc.up(), pc.forward());
    let pull = req.distance_m - ER_DEFAULT_DISTANCE_M;
    let m = &mut pc.matrix;
    m.3 .0 += r.0 * req.shoulder_x_m + u.0 * req.height_m + f.0 * -pull;
    m.3 .1 += r.1 * req.shoulder_x_m + u.1 * req.height_m + f.1 * -pull;
    m.3 .2 += r.2 * req.shoulder_x_m + u.2 * req.height_m + f.2 * -pull;
    pc.fov = req.fov_deg.to_radians();
}

#[allow(dead_code)]
fn _cmd(_: &Command) {}

/// `R1+L1` for a set of action bits, for the log.
fn action_names(bits: gowring_core::ActionBits) -> String {
    use gowring_core::sheets::ErAction as A;
    let all = [("R1", A::R1), ("R2", A::R2), ("L1", A::L1), ("L2", A::L2), ("Action", A::Action), ("SpMove", A::SpMove), ("Jump", A::Jump), ("UseItem", A::UseItem), ("SwitchForm", A::SwitchForm), ("ChangeWeaponR", A::ChangeWeaponR), ("ChangeWeaponL", A::ChangeWeaponL), ("ChangeItem", A::ChangeItem), ("R3", A::R3), ("L3", A::L3), ("Guard", A::Guard), ("ChangeStyleR", A::ChangeStyleR), ("ChangeStyleL", A::ChangeStyleL)];
    let v: Vec<&str> = all.iter().filter(|(_, a)| bits.has(*a)).map(|(n, _)| *n).collect();
    if v.is_empty() { "?".into() } else { v.join("+") }
}
