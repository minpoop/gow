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
    healed_on_entry: bool,
    pub camera_req: Arc<Mutex<gowring_core::CameraRequest>>,
    banner: String,
}

impl Runtime {
    pub fn new(shared: Shared, store: Arc<Store>, cache: PathBuf, camera_req: Arc<Mutex<gowring_core::CameraRequest>>) -> Self {
        let d = Data::get();
        let banner = format!("GowRing alpha 0.1.0 — Ragnarök art {}/{}", store.converted, store.total);
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
            healed_on_entry: false,
            camera_req,
            banner,
        }
    }

    pub fn carriers_ready(&self) -> bool {
        self.carriers.is_some()
    }

    /// Patch the params as soon as the repository exists. Returns false while still waiting.
    fn ensure_params(&mut self) -> bool {
        if self.carriers.is_some() {
            return true;
        }
        self.params_tried += 1;
        match crate::params::apply() {
            Ok(c) => {
                for n in &c.notes {
                    crate::glog!("params", "{n}");
                }
                self.carriers = Some(c);
                true
            }
            Err(e) => {
                if self.params_tried % 600 == 1 {
                    crate::glog!("params", "not ready yet / failed: {e}");
                }
                false
            }
        }
    }

    pub fn tick(&mut self) {
        // The mod stays inert until Elden Ring has opened the redirected save (see savehook).
        if !crate::savehook::redirected() {
            crate::log::once("gate", "boot_gate", "waiting: no redirected save open seen yet; the mod is inert");
            return;
        }
        if !self.ensure_params() {
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
        self.upkeep(&frame);

        let out = self.engine.tick(&frame);
        for l in &out.logs {
            crate::glog!("rules", "{l}");
        }
        self.bridge.apply(&out.commands, now, self.audio.as_ref(), &self.cache);
        self.ray_results = self.bridge.cast(&out.commands);
        self.publish(&frame, out.hud, out.camera, out.projectiles);
        self.ticks += 1;
        if self.ticks == 1 {
            crate::glog!("rules_tick", "first tick: player hp {}/{} at ({:.1},{:.1},{:.1}); {} hostile characters", frame.player.hp, frame.player.max_hp, frame.player.pos.x, frame.player.pos.y, frame.player.pos.z, frame.enemies.len());
        }
        if self.ticks % 1800 == 0 {
            crate::glog!("rules_tick", "alive: {} ticks, {} hostiles, hp {}/{}", self.ticks, frame.enemies.len(), frame.player.hp, frame.player.max_hp);
        }
    }

    /// hp_scale / stamina_free / fp_hide / er_hud_hide.
    fn upkeep(&mut self, frame: &Frame) {
        let d = Data::get();
        if let Ok(wcm) = unsafe { WorldChrMan::instance_mut() } {
            if let Some(p) = wcm.main_player.as_mut() {
                let data = &mut p.chr_ins.modules.data;
                if d.loadout.max_hp > 0 && data.max_hp != d.loadout.max_hp {
                    data.max_hp = d.loadout.max_hp;
                    data.max_uncapped_hp = d.loadout.max_hp;
                    if !self.healed_on_entry {
                        data.hp = d.loadout.max_hp;
                        self.healed_on_entry = true;
                        crate::glog!("hp_scale", "max HP set to {}", d.loadout.max_hp);
                    }
                }
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
