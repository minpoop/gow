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

/// A short camera shake: (amplitude 0..1, when it started). Read by the camera task each frame.
pub static SHAKE: Mutex<Option<(f32, Instant)>> = Mutex::new(None);

/// The close-up camera's stage (an index into [`CLOSE_UPS`] + 1; 0 = the game's own camera), stepped by the debug
/// camera key and read by the camera task each frame.
pub static CLOSE_UP: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

/// The close-up camera's stops: (name, distance from the point it looks at in metres, that point's height above the
/// player's feet in metres).
const CLOSE_UPS: [(&str, f32, f32); 3] = [("chest", 1.2, 1.35), ("chest, nearer", 0.7, 1.35), ("waist", 0.9, 1.0)];

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
    /// Effect rings still alive: (kind, position, radius in metres, engine time born).
    fx_live: Vec<(gowring_core::sheets::FxKind, gowring_core::Vec3, f32, f64)>,
    /// When the player first appeared in the world (for the controls panel).
    entered_at: Option<Instant>,
    /// The summon key was down last tick (summon on the press, not while held).
    spawn_key_down: bool,
    rage_key_down: bool,
    camera_key_down: bool,
    pose_key_down: bool,
    cape_key_down: bool,
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
            fx_live: Vec::new(),
            entered_at: None,
            spawn_key_down: false,
            rage_key_down: false,
            camera_key_down: false,
            pose_key_down: false,
            cape_key_down: false,
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
        // a character made with an earlier version holds that version's carrier, which the patch turned into a copy
        let missing: Vec<&str> = c.rows.iter().filter(|(n, row)| !have(**row) && !crate::params::carrier_rows(n).iter().any(|r| have(*r))).map(|(n, _)| *n).collect();
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
        // once per session, a few seconds into the world, on a thread of our own: the player's skeleton for Kratos's
        // body part, then what the game made of the part files
        if self.entered_at.is_some_and(|t| t.elapsed().as_secs() >= 5) {
            if let Some(b) = self.bridge.player_behavior_module() {
                // once: it ran every frame in 0.1.54 and 0.1.55 (7,540 lines in one session)
                static FACE_LOGGED: std::sync::Once = std::sync::Once::new();
                if let Some(f) = self.bridge.player_face_data() {
                    FACE_LOGGED.call_once(|| {
                        let bytes = crate::skeleton::read::<[u8; 0x40]>(f).map(|b| b.iter().map(|x| format!("{x:02x}")).collect::<Vec<_>>().join(" ")).unwrap_or_default();
                        crate::glog!("pose", "face data at {f:#x}: {bytes}");
                    });
                }
                crate::filecaps::start_once(b, self.bridge.player_model_item());
            }
        }
        self.observe_for_tests();
        self.upkeep(&frame);

        let st = crate::settings::get();
        self.entered_at.get_or_insert_with(Instant::now);
        if st.debug_tools {
            let down = function_key_down(st.debug_spawn_key);
            if down && !self.spawn_key_down {
                let msg = self.bridge.debug_spawn(st.debug_enemy_health, now);
                crate::overlay::toast(&msg);
            }
            self.spawn_key_down = down;
            let down = function_key_down(st.debug_rage_key);
            if down && !self.rage_key_down {
                self.engine.debug_fill_rage();
                crate::glog!("debug", "F{} pressed: Spartan Rage filled", st.debug_rage_key);
                crate::overlay::toast("Spartan Rage filled (debug)");
            }
            self.rage_key_down = down;
            let down = function_key_down(st.debug_camera_key);
            if down && !self.camera_key_down {
                use std::sync::atomic::Ordering::Relaxed;
                let stage = (CLOSE_UP.load(Relaxed) + 1) % (CLOSE_UPS.len() + 1);
                CLOSE_UP.store(stage, Relaxed);
                let what = stage.checked_sub(1).map(|i| CLOSE_UPS[i]).map_or("normal camera".to_string(), |(name, d, _)| format!("close-up camera: {name} ({d} m)"));
                crate::glog!("debug", "F{} pressed: {what}", st.debug_camera_key);
                crate::overlay::toast(&format!("{what} (debug)"));
            }
            self.camera_key_down = down;
            let down = function_key_down(st.debug_pose_key);
            if down && !self.pose_key_down {
                let on = crate::moves::toggle();
                crate::glog!("debug", "F{} pressed: Kratos's own idle {}", st.debug_pose_key, if on { "on" } else { "off (the game's pose)" });
                crate::overlay::toast(if on { "Kratos's idle on (debug)" } else { "Kratos's idle off: the game's pose (debug)" });
            }
            self.pose_key_down = down;
            let down = function_key_down(st.debug_cape_key);
            if down && !self.cape_key_down {
                let on = crate::cape::toggle();
                crate::glog!("debug", "F{} pressed: cloak swing {}", st.debug_cape_key, if on { "on" } else { "off (the cloak as built)" });
                crate::overlay::toast(if on { "Cloak swing on (debug)" } else { "Cloak swing off: the cloak as built (debug)" });
            }
            self.cape_key_down = down;
        } else {
            CLOSE_UP.store(0, std::sync::atomic::Ordering::Relaxed);
        }
        self.engine.set_tuning(gowring_core::Tuning {
            rage_gain: st.rage_gain_multiplier,
            mod_damage: st.mod_damage_multiplier,
            mimir_chance: st.mimir_frequency,
            enemy_health: st.enemy_health_multiplier,
            wounded_stun: st.finisher_when_wounded,
            health_stones: st.health_stones,
        });
        // the runics chosen in settings.json (or earlier in game) go in their slots
        for id in st.runics {
            self.engine.equip(id);
        }
        let mut out = self.engine.tick(&frame);
        out.hud.ui_scale = st.hud_scale;
        // a runic changed in game (L1 + D-pad) is kept in settings.json for next time
        for id in &out.equipped {
            crate::settings::set_runic(id);
        }
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
        for c in &out.commands {
            match c {
                Command::Fx { kind, pos, radius, .. } if st.effect_rings => self.fx_live.push((*kind, *pos, radius.max(1.0), now)),
                Command::Shake(a) if st.camera_shake > 0.0 => {
                    let a = &(*a * st.camera_shake);
                    if let Ok(mut g) = SHAKE.lock() {
                        let amp = g.map(|(old, t0)| old * (-t0.elapsed().as_secs_f32() / 0.3).exp()).unwrap_or(0.0).max(*a);
                        *g = Some((amp, Instant::now()));
                    }
                }
                _ => {}
            }
        }
        self.fx_live.retain(|f| now - f.3 < crate::overlay::FX_LIFE_S as f64);
        self.fx_live.truncate(24);
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
            let cfg = crate::settings::get();
            s.opacity = cfg.hud_opacity;
            // the controls panel: fades in over a second, stays for `controls_hint_seconds`, fades out over two
            s.hint = match (self.entered_at, cfg.controls_hint_seconds) {
                (Some(t0), secs) if secs > 0.0 => {
                    let t = t0.elapsed().as_secs_f32();
                    ((t / 1.0).min(1.0) * ((secs - t) / 2.0).clamp(0.0, 1.0)).clamp(0.0, 1.0)
                }
                _ => 0.0,
            };
            let now = self.start.elapsed().as_secs_f64();
            s.fx = self.fx_live.iter().map(|(k, p, r, born)| crate::overlay::FxMark { kind: *k, pos: *p, radius: *r, age: (now - born) as f32 }).collect();
        }
    }
}

/// hooks.camera_offset — runs after ER has placed its follow camera for the frame.
pub fn apply_camera(req: &gowring_core::CameraRequest) {
    const ER_DEFAULT_DISTANCE_M: f32 = 3.8;
    // a decaying wobble for Rage, spear blasts and the like (zero when there is no shake)
    let shake = SHAKE.lock().ok().and_then(|g| *g).and_then(|(amp, t0)| {
        let t = t0.elapsed().as_secs_f32();
        (t < 1.0).then(|| (amp * (-t / 0.3).exp() * 0.05, t))
    });
    if let Some(&(_, dist, height)) = CLOSE_UP.load(std::sync::atomic::Ordering::Relaxed).checked_sub(1).and_then(|i| CLOSE_UPS.get(i)) {
        close_up(dist, height);
        return;
    }
    if req.fov_deg <= 1.0 && shake.is_none() {
        return;
    }
    let Ok(cam) = (unsafe { CSCamera::instance_mut() }) else { return };
    let pc = &mut cam.pers_cam_1;
    let (r, u, f) = (pc.right(), pc.up(), pc.forward());
    let m = &mut pc.matrix;
    if req.fov_deg > 1.0 {
        let pull = req.distance_m - ER_DEFAULT_DISTANCE_M;
        m.3 .0 += r.0 * req.shoulder_x_m + u.0 * req.height_m + f.0 * -pull;
        m.3 .1 += r.1 * req.shoulder_x_m + u.1 * req.height_m + f.1 * -pull;
        m.3 .2 += r.2 * req.shoulder_x_m + u.2 * req.height_m + f.2 * -pull;
    }
    if let Some((a, t)) = shake {
        let (dx, dy, dz) = ((t * 61.0).sin() * a, (t * 83.0 + 1.0).sin() * a * 0.7, (t * 47.0 + 2.0).sin() * a * 0.5);
        m.3 .0 += r.0 * dx + u.0 * dy + f.0 * dz;
        m.3 .1 += r.1 * dx + u.1 * dy + f.1 * dz;
        m.3 .2 += r.2 * dx + u.2 * dy + f.2 * dz;
    }
    if req.fov_deg > 1.0 {
        pc.fov = req.fov_deg.to_radians();
    }
}

/// The close-up camera: `dist` metres back along the game camera's own view from a point `height` metres above the
/// player's feet, so the camera stick still turns it around him (no shoulder offset, zoom or shake).
fn close_up(dist: f32, height: f32) {
    use eldenring::position::HavokPosition;
    let Ok(wcm) = (unsafe { WorldChrMan::instance() }) else { return };
    let Some(player) = wcm.main_player.as_ref() else { return };
    let HavokPosition(px, py, pz, _) = player.chr_ins.modules.physics.position;
    let Ok(cam) = (unsafe { CSCamera::instance_mut() }) else { return };
    let pc = &mut cam.pers_cam_1;
    let f = pc.forward();
    let m = &mut pc.matrix;
    m.3 .0 = px - f.0 * dist;
    m.3 .1 = py + height - f.1 * dist;
    m.3 .2 = pz - f.2 * dist;
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

/// Whether F`n` is held while the game's own window has the focus (a key pressed in another program is ignored).
fn function_key_down(n: u8) -> bool {
    use windows::Win32::System::Threading::GetCurrentProcessId;
    use windows::Win32::UI::Input::KeyboardAndMouse::{GetAsyncKeyState, VK_F1};
    use windows::Win32::UI::WindowsAndMessaging::{GetForegroundWindow, GetWindowThreadProcessId};
    if !(1..=12).contains(&n) {
        return false;
    }
    unsafe {
        let mut pid = 0u32;
        GetWindowThreadProcessId(GetForegroundWindow(), Some(&mut pid));
        if pid != GetCurrentProcessId() {
            return false;
        }
        (GetAsyncKeyState(VK_F1.0 as i32 + n as i32 - 1) as u16 & 0x8000) != 0
    }
}
