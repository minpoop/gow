//! GowRing.exe — the launcher for "Kratos in the Lands Between".
//!
//!   --prepare   read the player's God of War Ragnarök folder, convert HUD art / audio into the cache
//!   --play      make sure that is done, back up the save, point ModEngine2 at the mod and start Elden Ring
//!   --diagnose  write a report of what the reader finds in both installs
//!
//! Melty passes the games' folders as environment variables (`GOWRING_ER`, `GOWRING_GOWR`), so the
//! launcher never asks for them.

mod config;
mod me2;
mod proc;
mod saves;

use anyhow::{bail, Context, Result};
use config::Ctx;
use gowring_core::sheets::{AssetConverter, ASSETS};
use gowring_gow::prepare::{AssetReq, Converter, Tools};
use std::path::Path;

/// "alpha": a partly converted install still starts (so the rules and HUD can be tested) and the
/// game says so on screen. "release": every asset must convert or Play stops with a clear message.
pub const CHANNEL: &str = config::CHANNEL;

/// HUD pieces that can take a Ragnarök picture: (id, what it is, cache path).
fn hud_pieces() -> Vec<(&'static str, &'static str, &'static str)> {
    ASSETS.iter().filter(|a| a.id.starts_with("hud_") && a.output.ends_with(".png")).map(|a| (a.id, a.describes, a.output)).collect()
}

/// Ragnarök pictures picked from the player's own gallery as the starting look (`ragnarok_pick` in assets.json: the
/// bar shapes, the shield icon, and the pieces the runic plates, shield octagon, Rage mark and buttons are built from).
fn suggested_hud() -> Vec<(&'static str, &'static str)> {
    ASSETS.iter().filter(|a| a.ragnarok_pick != "none").map(|a| (a.id, a.ragnarok_pick)).collect()
}

/// Copy the Ragnarök pictures chosen in `ragnarok_hud.json` into the cache for the overlay (before every Play).
fn apply_hud_choices(ctx: &Ctx) {
    let path = ctx.local.join("ragnarok_hud.json");
    let Ok(mut text) = std::fs::read_to_string(&path) else { return };
    let pieces: Vec<(&str, &str)> = hud_pieces().iter().map(|(id, what, _)| (*id, *what)).collect();
    if let Some(filled) = gowring_gow::hudmap::fill_suggestions(&text, &pieces, &suggested_hud(), &ctx.local.join("ragnarok_ui")) {
        if std::fs::write(&path, &filled).is_ok() {
            ctx.log("ragnarok_hud: starter Ragnarök pictures filled in (bar shapes, shield icon, and the pieces of the runic plates, shield octagon, Rage mark and buttons)");
            text = filled;
        }
    }
    if let Some(grown) = gowring_gow::hudmap::add_new_pieces(&text, &pieces) {
        if std::fs::write(&path, &grown).is_ok() {
            ctx.log("ragnarok_hud: new HUD pieces listed (your choices kept)");
            text = grown;
        }
    }
    match gowring_gow::hudmap::parse(&text) {
        Ok(choices) => {
            let outputs: Vec<(&str, &str)> = hud_pieces().iter().map(|(id, _, out)| (*id, *out)).collect();
            let report = gowring_gow::hudmap::apply(&choices, &ctx.local.join("ragnarok_ui"), &ctx.cache, &outputs);
            for line in report.lines() {
                ctx.log(&format!("ragnarok_hud: {line}"));
            }
        }
        Err(e) => ctx.log(&format!("ragnarok_hud: {} could not be read ({e:#}); the HUD keeps its current art", path.display())),
    }
}

fn requests() -> Vec<AssetReq> {
    ASSETS
        .iter()
        .map(|a| AssetReq {
            id: a.id.to_string(),
            search: a.search.to_string(),
            converter: match a.converter {
                AssetConverter::DdsToPng => Converter::DdsToPng,
                AssetConverter::WemToWav => Converter::WemToWav,
                AssetConverter::WemToWavSet => Converter::WemToWavSet,
            },
            output: a.output.to_string(),
        })
        .collect()
}

fn tools(ctx: &Ctx, budget_s: u64) -> Tools {
    let exe = ctx.managed.join("tools").join("vgmstream-cli.exe");
    Tools { vgmstream: exe.exists().then_some(exe), budget: Some(std::time::Duration::from_secs(budget_s)) }
}

fn overrides(ctx: &Ctx) -> serde_json::Value {
    std::fs::read_to_string(ctx.managed.join("assets_map.json"))
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_else(|| serde_json::json!({}))
}

/// What the conversion step produced; `None` when it did not finish in time or panicked.
fn run_conversion(ctx: &Ctx, gow: &std::path::Path) -> Option<gowring_gow::prepare::PrepareReport> {
    let (tx, rx) = std::sync::mpsc::channel();
    let (c, g) = (ctx.clone(), gow.to_path_buf());
    std::thread::Builder::new()
        .name("prepare".into())
        .spawn(move || {
            let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                let reqs = requests();
                gowring_gow::prepare::prepare(&g, &reqs, &overrides(&c), &c.cache, &tools(&c, 40))
            }));
            let _ = tx.send(match r {
                Ok(Ok(rep)) => Ok(rep),
                Ok(Err(e)) => Err(format!("{e:#}")),
                Err(_) => Err("the converter panicked".to_string()),
            });
        })
        .ok()?;
    // Hard stop: the setup step runs before the game and must never keep it from starting.
    match rx.recv_timeout(std::time::Duration::from_secs(75)) {
        Ok(Ok(rep)) => Some(rep),
        Ok(Err(e)) => {
            ctx.log(&format!("prepare: conversion failed: {e}"));
            None
        }
        Err(_) => {
            ctx.log("prepare: conversion did not finish in 75 s; continuing without it");
            None
        }
    }
}

fn prepare(ctx: &Ctx) -> Result<()> {
    let gow = ctx.gow.as_ref().context("God of War Ragnarök was not found (GOWRING_GOWR is not set)")?;
    ctx.log(&format!("prepare: reading {}", gow.display()));
    let _ = std::fs::remove_file(ctx.managed.join(".melty-ready"));
    let _ = std::fs::remove_file(ctx.managed.join(".melty-failure.txt"));
    let reqs = requests();
    let t0 = std::time::Instant::now();
    let report = run_conversion(ctx, gow);
    ctx.log(&format!("prepare: conversion step took {:.1}s", t0.elapsed().as_secs_f32()));
    let (ok, total, missing) = match &report {
        Some(r) => {
            ctx.log(&format!("prepare: notes {:?}", r.notes));
            (r.converted(), reqs.len(), r.missing().iter().map(|s| s.to_string()).collect::<Vec<_>>())
        }
        None => (0, reqs.len(), reqs.iter().map(|r| r.id.clone()).collect()),
    };
    let manifest = serde_json::json!({
        "version": config::VERSION,
        "channel": CHANNEL,
        "gow_folder": gow.to_string_lossy(),
        "finished": report.is_some(),
        "report": report,
    });
    std::fs::create_dir_all(&ctx.cache)?;
    std::fs::write(ctx.cache.join("manifest.json"), serde_json::to_string_pretty(&manifest)?)?;
    ctx.log(&format!("prepare: converted {ok}/{total}; missing {missing:?}"));
    if ok < total && CHANNEL == "release" {
        let msg = format!(
            "Kratos in the Lands Between could not read {} of {} pieces from your God of War Ragnarök install ({}). \
             Run GowRing.exe --diagnose and send the report to the mashup's page.",
            total - ok,
            total,
            missing.join(", ")
        );
        std::fs::write(ctx.managed.join(".melty-failure.txt"), &msg)?;
        bail!(msg);
    }
    std::fs::write(ctx.managed.join(".melty-ready"), config::READY_TOKEN)?;
    ctx.log("prepare: ready token written");
    println!("Prepared {ok}/{total} Ragnarök assets ({} channel). Cache: {}", CHANNEL, ctx.cache.display());
    Ok(())
}

fn prepared(ctx: &Ctx) -> bool {
    std::fs::read_to_string(ctx.managed.join(".melty-ready")).map(|t| t.contains(config::READY_TOKEN)).unwrap_or(false)
}

/// PATH with `front` ahead of it: the game process inherits it, so the Visual C++ DLLs shipped next to
/// ModEngine2 are found when modengine2.dll loads.
fn path_with(front: &[std::path::PathBuf]) -> std::ffi::OsString {
    let mut parts: Vec<std::path::PathBuf> = front.to_vec();
    if let Some(old) = std::env::var_os("PATH") {
        parts.extend(std::env::split_paths(&old));
    }
    std::env::join_paths(parts).unwrap_or_default()
}

fn play(ctx: &Ctx) -> Result<()> {
    let er = ctx.er.as_ref().context("Elden Ring was not found (GOWRING_ER is not set)")?;
    let exe = er.join("Game").join("eldenring.exe");
    if !exe.exists() {
        bail!("{} does not exist", exe.display());
    }
    if !prepared(ctx) {
        prepare(ctx)?;
    }
    apply_hud_choices(ctx);
    ctx.log(&format!("play: package folder {} contains: {}", ctx.managed.display(), Ctx::listing(&ctx.managed)));
    if let Some(parent) = ctx.managed.parent() {
        ctx.log(&format!("play: parent folder {} contains: {}", parent.display(), Ctx::listing(parent)));
    }
    let Some(me2) = ctx.me2_dir() else {
        bail!("ModEngine2 is not installed: no modengine2_launcher.exe next to {} (see launcher.log for what is there)", ctx.managed.display());
    };
    ctx.log(&format!("play: ModEngine2 folder {} contains: {}", me2.display(), Ctx::listing(&me2)));
    let launcher = me2.join("modengine2_launcher.exe");
    let me2_dll = me2.join("modengine2").join("bin").join("modengine2.dll");
    ctx.log(&format!("play: modengine2.dll at {}: {}", me2_dll.display(), if me2_dll.exists() { "present" } else { "MISSING" }));
    let dll = ctx.managed.join("gowring.dll");
    if !dll.exists() {
        bail!("{} is missing; reinstall the mod", dll.display());
    }
    for note in saves::backup(ctx) {
        ctx.log(&note);
    }
    // Kratos's body part and weapons, built from the player's Ragnarök install, must be in the mod folder before the
    // game looks. They share one read of r_heroa00.wad, let go before the game starts.
    {
        let hero = HeroWad::default();
        let note = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| install_kratos_part(ctx, &hero))).unwrap_or_else(|_| "Kratos's body part: the build panicked".into());
        ctx.log(&format!("play: {note}"));
        let note = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| install_weapon_parts(ctx, &hero))).unwrap_or_else(|_| "Kratos's weapons: the build panicked".into());
        ctx.log(&format!("play: {note}"));
    }
    let cfg = me2::write_config(ctx, &dll)?;
    ctx.log(&format!("play: ModEngine2 config {}", cfg.display()));
    let mut cmd = std::process::Command::new(&launcher);
    cmd.current_dir(launcher.parent().unwrap())
        .arg("-t")
        .arg("er")
        .arg("-p")
        .arg(&exe)
        .arg("-c")
        .arg(&cfg)
        .env("GOWRING_HOME", &ctx.managed)
        .env("GOWRING_CACHE", &ctx.cache)
        .env("GOWRING_LOCAL", &ctx.local)
        .env("GOWRING_CHANNEL", CHANNEL)
        .env("GOWRING_SAVE", config::SAVE_FILE)
        .env("PATH", path_with(&[me2.join("modengine2").join("bin"), me2.clone()]));
    ctx.log(&format!("play: starting {} -t er -p {} -c {}", launcher.display(), exe.display(), cfg.display()));
    // ModEngine2's launcher returns as soon as it has created the game process. Its output goes to a file
    // (never a pipe: a game that inherited a pipe would keep us waiting until it exits).
    let me2_log = ctx.local.join("logs").join("me2-launcher.log");
    let _ = std::fs::create_dir_all(me2_log.parent().unwrap());
    let sink = std::fs::File::create(&me2_log).with_context(|| me2_log.display().to_string())?;
    cmd.stdout(std::process::Stdio::from(sink.try_clone()?)).stderr(std::process::Stdio::from(sink)).stdin(std::process::Stdio::null());
    let mut child = cmd.spawn().with_context(|| format!("starting {}", launcher.display()))?;
    let status = child.wait()?;
    let launcher_output = std::fs::read_to_string(&me2_log).unwrap_or_default();
    ctx.log(&format!("play: ModEngine2 launcher exited with {status}; output {:?}", launcher_output.trim()));
    if !status.success() {
        bail!("ModEngine2 launcher exited with {status} ({})", launcher_output.trim());
    }
    background_diagnose(ctx);
    // Stay alive while the game runs so Melty can show it as playing.
    let exe_name = "eldenring.exe";
    let appeared = proc::wait_for(exe_name, std::time::Duration::from_secs(90));
    if appeared {
        proc::wait_gone(exe_name);
        ctx.log("play: Elden Ring closed");
    } else {
        ctx.log("play: Elden Ring did not appear within 90 s");
        bail!("Elden Ring did not start. See {}", ctx.local.join("logs").display());
    }
    Ok(())
}

/// The first 200 MB of r_heroa00.wad, decompressed, and its chunk table: read at most once, by whichever of Kratos's
/// parts needs it first (a cached build needs none).
#[derive(Default)]
struct HeroWad {
    read: std::cell::OnceCell<Result<(Vec<u8>, Vec<gowring_gow::wtoc::Entry>), String>>,
}

impl HeroWad {
    fn get(&self, wad: &Path) -> Result<(&[u8], &[gowring_gow::wtoc::Entry])> {
        let read = self.read.get_or_init(|| {
            let (buf, _) = gowring_gow::lz4::read_file_prefix(wad, 200 << 20).map_err(|e| format!("{e:#}"))?;
            let es = gowring_gow::wtoc::read(&buf).ok_or("no chunk table in r_heroa00.wad")?;
            Ok((buf, es))
        });
        match read {
            Ok((buf, es)) => Ok((buf, es)),
            Err(e) => bail!("{e}"),
        }
    }
}

/// What the outfit probe keeps (see `install_kratos_part`): the bear cloak's fur (24), the fur under it (69), the
/// leather of the strap and cloak (75), the leather cloak's fur (29), the belt (61) and the unused waist, chest and
/// shoulder pieces' materials (66, 70, 71, 21); and the parts: skin (230), the bear cloak and its coarse shell (235,
/// 237), the underfur (224), the strap and two pieces like it (231-233), the belt (196, 197), the unused waist, skirt,
/// shoulder and chest pieces (199, 205-209, 225-229).
const OUTFIT_PROBE_MATERIALS: [u16; 9] = [24, 69, 75, 29, 61, 66, 70, 71, 21];
const OUTFIT_PROBE_PARTS: [u16; 22] = [230, 235, 237, 224, 231, 232, 233, 196, 197, 199, 205, 206, 207, 208, 209, 225, 226, 227, 228, 229, 194, 195];

/// Build Kratos's armour parts (see `kratos_part::build_parts`) from the player's Ragnarök install once per install,
/// builder version and player skeleton, keep them in the cache, and copy them into the mod folder ModEngine2 serves
/// (`mod/parts`). The skeleton is the one gowring.dll read from the game in an earlier session (`er_skeleton.json`);
/// until there is one, the parts carry the root bone alone.
fn install_kratos_part(ctx: &Ctx, hero: &HeroWad) -> String {
    use gowring_gow::kratos_part::{build_parts, skeleton_nodes};
    const BUILD: u32 = 19;
    let parts = ctx.managed.join("mod").join("parts");
    let Some(gow) = ctx.gow.as_deref() else { return "Kratos's body part: no Ragnarök install".into() };
    let (Some(wad), Some(pack)) = (find_file(gow, "r_heroa00.wad", 5), find_file(gow, "root.lodpack", 5)) else {
        return "Kratos's body part: r_heroa00.wad or root.lodpack not found".into();
    };
    let size = |p: &std::path::Path| std::fs::metadata(p).map(|m| m.len()).unwrap_or(0);
    let skeleton_json = std::fs::read_to_string(ctx.local.join("er_skeleton.json")).ok();
    let (skeleton, skeleton_note) = match skeleton_json.as_deref().map(skeleton_nodes) {
        Some(Ok(nodes)) => (Some(nodes), "the player skeleton"),
        Some(Err(_)) => (None, "the root bone only (er_skeleton.json did not read)"),
        None => (None, "the root bone only (no er_skeleton.json yet)"),
    };
    // a cheap fingerprint of the skeleton, so a new one rebuilds the parts
    let fingerprint = skeleton_json.as_deref().map_or(0u64, |j| j.bytes().fold(0xcbf29ce484222325u64, |h, b| (h ^ b as u64).wrapping_mul(0x100000001b3)));
    // the armour set: settings.json's "black_bear_outfit" (gowring.dll keeps the key; on unless it says false)
    let black_bear = std::fs::read_to_string(ctx.local.join("settings.json"))
        .ok()
        .and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok())
        .and_then(|v| v.get("black_bear_outfit").and_then(|b| b.as_bool()))
        .unwrap_or(true);
    let outfit = if black_bear { gowring_gow::kratos_part::Outfit::BlackBear } else { gowring_gow::kratos_part::Outfit::Survival };
    let stamp = format!("build {BUILD} wad {} pack {} skeleton {fingerprint:016x} outfit {outfit:?}\n", size(&wad), size(&pack));
    let cache = ctx.cache.join("kratos_part");
    let names: Vec<String> = ["bd", "am", "lg", "hd"]
        .iter()
        .zip([9701, 9702, 9703, 9700])
        .flat_map(|(slot, id)| ["m", "f"].iter().flat_map(move |sex| ["", "_l"].iter().map(move |low| format!("{slot}_{sex}_{id}{low}.partsbnd.dcx"))))
        .collect();
    let cached = std::fs::read_to_string(cache.join("stamp.txt")).is_ok_and(|s| s == stamp) && names.iter().all(|n| cache.join(n).exists());
    let t0 = std::time::Instant::now();
    let mut note = String::new();
    if !cached {
        let built = (|| -> anyhow::Result<String> {
            let (buf, es) = hero.get(&wad)?;
            let chunk = |name: &str, id: u16| es.iter().find(|e| e.name == name && e.id == id).and_then(|e| buf.get(e.file_off as usize..(e.file_off + e.length as u64) as usize));
            let (Some(mesh), Some(mg), Some(gpu)) = (chunk("MESH_heroa00_0", 1), chunk("MG_heroa00_0", 1), chunk("MG_heroa00_0_gpu", 29)) else {
                bail!("Kratos's mesh chunks are not in the first 200 MB of r_heroa00.wad");
            };
            // his own skeleton, to re-pose him onto the player's
            let kratos = chunk("goProtoheroa00", 1).and_then(gowring_gow::kratos::Skeleton::parse);
            // his colours: each material's colour map from the texture packs next to root.lodpack
            let t1 = std::time::Instant::now();
            let materials = gowring_gow::kratos_part::outfit_materials(mesh, mg, outfit);
            let wad_dir = pack.parent().unwrap_or(gow).to_path_buf();
            let (looks, look_note) = gowring_gow::kratos_look::outfit_looks(buf, es, &wad_dir, &materials, gowring_gow::kratos_part::STRANDS, gowring_gow::kratos_part::MAX_TEXTURE, gowring_gow::kratos_part::MAX_SURFACE, outfit.dyed());
            ctx.log(&format!("play: Kratos's look in {:.1}s for {} materials: {look_note}", t1.elapsed().as_secs_f32(), materials.len()));
            let (files, note) = build_parts(mesh, mg, gpu, &pack, skeleton.as_deref(), kratos.as_ref(), &looks, outfit)?;
            // for the next round: the fur's, strap's and waist pieces' maps, and the cloak, strap, waist and chest pieces
            // with their UVs (`outfit_probe`)
            let probe = ctx.local.join("outfit_probe");
            let _ = std::fs::remove_dir_all(&probe);
            if std::fs::create_dir_all(probe.join("maps")).is_ok() {
                let (pngs, lines) = gowring_gow::kratos_look::material_textures_png(buf, es, &wad_dir, &OUTFIT_PROBE_MATERIALS, 512);
                for (name, png) in &pngs {
                    let _ = std::fs::write(probe.join("maps").join(name), png);
                }
                let _ = std::fs::write(probe.join("maps.txt"), &lines);
                let obj = gowring_gow::kratos_part::parts_obj_from_packs(mesh, mg, gpu, &pack, &OUTFIT_PROBE_PARTS);
                let _ = std::fs::write(probe.join("parts.obj"), &obj);
                ctx.log(&format!("play: outfit probe: {} maps, {} part draws in {}", pngs.len(), obj.lines().filter(|l| l.starts_with("g ")).count(), probe.display()));
            }
            std::fs::create_dir_all(&cache)?;
            // the male, female and low-detail copies are the same bytes: write each once, link the others
            for (i, (name, bytes)) in files.iter().enumerate() {
                let path = cache.join(name);
                let _ = std::fs::remove_file(&path);
                let same = files[..i].iter().find(|(_, b)| b == bytes).map(|(n, _)| cache.join(n));
                if !same.is_some_and(|src| std::fs::hard_link(src, &path).is_ok()) {
                    std::fs::write(&path, bytes)?;
                }
            }
            std::fs::write(cache.join("stamp.txt"), &stamp)?;
            Ok(note)
        })();
        match built {
            Ok(n) => note = format!("built in {:.1}s: {n}; ", t0.elapsed().as_secs_f32()),
            Err(e) => return format!("Kratos's body part: could not build it: {e:#}"),
        }
    }
    if let Err(e) = std::fs::create_dir_all(&parts) {
        return format!("Kratos's body part: {}: {e}", parts.display());
    }
    for n in &names {
        // a link to the cached file where the two folders share a drive, else a copy
        let target = parts.join(n);
        let _ = std::fs::remove_file(&target);
        if std::fs::hard_link(cache.join(n), &target).is_err() {
            if let Err(e) = std::fs::copy(cache.join(n), &target) {
                return format!("Kratos's body part: copying {n}: {e}");
            }
        }
    }
    format!("Kratos's body part on {skeleton_note}: {note}installed in {}", parts.display())
}

/// Build the weapons Kratos carries (see `weapon_part::build_weapon`) once per install and builder version, keep them
/// in the cache and link them into `mod/parts`. A weapon that does not build is taken out of the mod folder, so its
/// carrier keeps its own model (gowring.dll repoints a carrier only when the file is there). A build also writes a
/// probe of each model to `weapon_probe\<model>\` for the next round.
fn install_weapon_parts(ctx: &Ctx, hero: &HeroWad) -> String {
    use gowring_gow::weapon_part::{build_weapon, file_names, WEAPONS};
    const WEAPON_BUILD: u32 = 6;
    let parts = ctx.managed.join("mod").join("parts");
    // when nothing can be built, an earlier build's files leave too, so the carriers keep their own models
    let unplug = |why: String| -> String {
        for n in WEAPONS.iter().flat_map(|spec| file_names(spec.er_model)) {
            let _ = std::fs::remove_file(parts.join(n));
        }
        format!("Kratos's weapons: {why}")
    };
    let Some(gow) = ctx.gow.as_deref() else { return unplug("no Ragnarök install".into()) };
    let (Some(wad), Some(pack)) = (find_file(gow, "r_heroa00.wad", 5), find_file(gow, "root.lodpack", 5)) else {
        return unplug("r_heroa00.wad or root.lodpack not found".into());
    };
    let size = |p: &std::path::Path| std::fs::metadata(p).map(|m| m.len()).unwrap_or(0);
    let stamp = format!("weapons {WEAPON_BUILD} wad {} pack {}\n", size(&wad), size(&pack));
    let cache = ctx.cache.join("weapon_parts");
    let cached = std::fs::read_to_string(cache.join("stamp.txt")).is_ok_and(|s| s == stamp);
    let t0 = std::time::Instant::now();
    let mut built = String::new();
    if !cached {
        let (buf, es) = match hero.get(&wad) {
            Ok(read) => read,
            Err(e) => return unplug(format!("could not build them: {e:#}")),
        };
        let wad_dir = pack.parent().unwrap_or(gow).to_path_buf();
        let mut lod = gowring_gow::lodpack::LodPacks::open(&pack).ok();
        let probe_root = ctx.local.join("weapon_probe");
        let _ = std::fs::remove_dir_all(&cache);
        let _ = std::fs::remove_dir_all(&probe_root);
        if let Err(e) = std::fs::create_dir_all(&cache) {
            return unplug(format!("{}: {e}", cache.display()));
        }
        let mut notes = Vec::new();
        for spec in WEAPONS {
            let mut log = |l: &str| ctx.log(&format!("play: weapon: {l}"));
            let made = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| build_weapon(spec, buf, es, &wad_dir, lod.as_mut(), &mut log)));
            let (files, note, probe) = match made {
                Ok(Ok(made)) => made,
                Ok(Err(e)) => (Vec::new(), format!("{}: {e:#}", spec.name), Default::default()),
                Err(_) => (Vec::new(), format!("{}: the build panicked", spec.name), Default::default()),
            };
            let dir = probe_root.join(spec.model);
            for (name, bytes) in &probe.files {
                let path = dir.join(name);
                let _ = path.parent().map(std::fs::create_dir_all);
                let _ = std::fs::write(&path, bytes);
            }
            // the low-detail copy is the same bytes: write it once, link the other
            for (i, (name, bytes)) in files.iter().enumerate() {
                let path = cache.join(name);
                let same = files[..i].iter().find(|(_, b)| b == bytes).map(|(n, _)| cache.join(n));
                if !same.is_some_and(|src| std::fs::hard_link(src, &path).is_ok()) {
                    if let Err(e) = std::fs::write(&path, bytes) {
                        let _ = std::fs::remove_file(cache.join("stamp.txt"));
                        return unplug(format!("{}: {e}", path.display()));
                    }
                }
            }
            notes.push(note);
        }
        let _ = std::fs::write(cache.join("notes.txt"), notes.join("\n"));
        let _ = std::fs::write(cache.join("stamp.txt"), &stamp);
        built = format!("built in {:.1}s; ", t0.elapsed().as_secs_f32());
    }
    if let Err(e) = std::fs::create_dir_all(&parts) {
        return format!("Kratos's weapons: {}: {e}", parts.display());
    }
    let mut installed = Vec::new();
    for spec in WEAPONS {
        let names = file_names(spec.er_model);
        let ready = names.iter().all(|n| cache.join(n).exists());
        for n in &names {
            // a link to the cached file where the two folders share a drive, else a copy; none for a weapon not built
            let target = parts.join(n);
            let _ = std::fs::remove_file(&target);
            if ready && std::fs::hard_link(cache.join(n), &target).is_err() {
                if let Err(e) = std::fs::copy(cache.join(n), &target) {
                    return unplug(format!("copying {n}: {e}"));
                }
            }
        }
        if ready {
            installed.push(spec.name);
        }
    }
    let notes = std::fs::read_to_string(cache.join("notes.txt")).unwrap_or_default().replace('\n', "; ");
    format!("Kratos's weapons: {built}installed {} of {} ({}): {notes}", installed.len(), WEAPONS.len(), installed.join(", "))
}

/// While the game loads, write the same report `--diagnose` writes (once per version) so a tester only has to
/// send a file. It stops reading after two minutes and can never affect the game.
fn background_diagnose(ctx: &Ctx) {
    let Some(gow) = ctx.gow.clone() else { return };
    let path = ctx.local.join("diagnose.txt");
    let mark = format!("GowRing diagnose — {}", config::VERSION);
    if std::fs::read_to_string(&path).map(|t| t.starts_with(&mark) && t.contains("== WAD heads")).unwrap_or(false) {
        return;
    }
    let c = ctx.clone();
    let _ = std::thread::Builder::new().name("diagnose".into()).spawn(move || {
        // Not while the game is starting: it reads a lot of files, and Elden Ring's own load comes first.
        std::thread::sleep(std::time::Duration::from_secs(180));
        let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let vg = tools(&c, 60).vgmstream;
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(120);
            gowring_gow::diagnose::report_until(&gow, c.er.as_deref(), &requests(), vg.as_deref(), Some(deadline))
        }));
        match r {
            Ok(mut text) => {
                let _ = std::fs::create_dir_all(&c.local);
                // Ragnarök's own UI textures, decoded from the player's install for a look (not used in game yet).
                let ui = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| export_ui_textures(&gow, &c.local.join("ragnarok_ui"))));
                let map = c.local.join("ragnarok_hud.json");
                if !map.exists() {
                    let pieces: Vec<(&str, &str)> = hud_pieces().iter().map(|(id, what, _)| (*id, *what)).collect();
                    let _ = std::fs::write(&map, gowring_gow::hudmap::template(&pieces));
                }
                text.push_str(&match ui {
                    Ok(t) => t,
                    Err(_) => "\n== Ragnarök UI textures\n(the export panicked)\n".into(),
                });
                // Kratos's skeleton and animation files, written out for study (roadmap: his own animations)
                let kr = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| export_kratos(&gow, &c.local.join("kratos_probe"))));
                text.push_str(&match kr {
                    Ok(t) => t,
                    Err(_) => "\n== Kratos's skeleton and animations\n(the export panicked)\n".into(),
                });
                // Elden Ring's install, names and sizes only, for planning Kratos's body part
                if let Some(er) = c.er.as_deref() {
                    let li = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| list_er_install(er, &c.local.join("kratos_tex").join("er_install.tsv"))));
                    text.push_str(&li.unwrap_or_else(|_| "\n-- Elden Ring install listing panicked\n".into()));
                }
                match std::fs::write(c.local.join("diagnose.txt"), text) {
                    Ok(()) => c.log("diagnose: wrote diagnose.txt"),
                    Err(e) => c.log(&format!("diagnose: could not write the report: {e}")),
                }
            }
            Err(_) => c.log("diagnose: the report panicked"),
        }
    });
}

/// Find a file by name under `root` (a few folders deep).
fn find_file(root: &std::path::Path, name: &str, depth: u32) -> Option<std::path::PathBuf> {
    let rd = std::fs::read_dir(root).ok()?;
    let mut dirs = Vec::new();
    for e in rd.flatten() {
        let p = e.path();
        if p.is_dir() {
            dirs.push(p);
        } else if p.file_name().map(|n| n.to_string_lossy().eq_ignore_ascii_case(name)).unwrap_or(false) {
            return Some(p);
        }
    }
    if depth == 0 {
        return None;
    }
    dirs.into_iter().find_map(|d| find_file(&d, name, depth - 1))
}

/// Decode the textures inside Ragnarök's r_ui.wad to PNG files in `out` (at most 90 s of work). Returns a report.
fn export_ui_textures(gow: &std::path::Path, out: &std::path::Path) -> String {
    let mut o = String::from("\n== Ragnarök UI textures (decoded to PNG; the pieces picked in ragnarok_hud.json are drawn by the HUD)\n");
    let Some(wad) = find_file(gow, "r_ui.wad", 5) else {
        o.push_str("r_ui.wad not found\n");
        return o;
    };
    let buf = match gowring_gow::lz4::read_file_prefix(&wad, 768 << 20) {
        Ok((b, _)) => b,
        Err(e) => {
            o.push_str(&format!("{}: {e:#}\n", wad.display()));
            return o;
        }
    };
    let until = std::time::Instant::now() + std::time::Duration::from_secs(150);
    let report = gowring_gow::wadtex::export(&buf, out, 4000, || std::time::Instant::now() < until);
    o.push_str(&format!("{} ({} decompressed bytes) -> {}\n", wad.display(), buf.len(), out.display()));
    // the summary first, then the first 300 lines
    if let Some(sum) = report.lines().last() {
        o.push_str(sum);
        o.push('\n');
    }
    for l in report.lines().take(300) {
        o.push_str("  ");
        o.push_str(l);
        o.push('\n');
    }
    o
}

/// Kratos's skeleton, animation chunks and streamed animation sets from r_heroa00, written to `out` for study (nothing
/// is used in game); the report says what was found.
fn export_kratos(gow: &std::path::Path, out: &std::path::Path) -> String {
    let mut o = String::from("\n== Kratos's skeleton and animations (written out for study; not used in game)\n");
    let Some(wad) = find_file(gow, "r_heroa00.wad", 5) else {
        o.push_str("r_heroa00.wad not found\n");
        return o;
    };
    if let Some(dir) = wad.parent() {
        o.push_str(&gowring_gow::kratos::set_inventory(dir));
    }
    let buf = match gowring_gow::lz4::read_file_prefix(&wad, 256 << 20) {
        Ok((b, _)) => b,
        Err(e) => {
            o.push_str(&format!("{}: {e:#}\n", wad.display()));
            return o;
        }
    };
    let sets: Vec<(String, Vec<u8>)> = gowring_gow::kratos::sibling_sets(&wad)
        .iter()
        .take(3)
        .filter_map(|p| Some((p.file_name()?.to_string_lossy().into_owned(), gowring_gow::kratos::read_set_file(p, 96 << 20).ok()?)))
        .collect();
    o.push_str(&format!("{} ({} decompressed bytes read); .as files beside it: {:?}\n", wad.display(), buf.len(), sets.iter().map(|(n, b)| (n.as_str(), b.len())).collect::<Vec<_>>()));
    let until = std::time::Instant::now() + std::time::Duration::from_secs(90);
    o.push_str(&gowring_gow::kratos::export_from(&buf, &sets, "heroa00", out, || std::time::Instant::now() < until));
    // each step on its own, so one that fails keeps the others' report (0.1.38 lost it all to one panic)
    let step = |name: &str, f: &dyn Fn() -> String| std::panic::catch_unwind(std::panic::AssertUnwindSafe(f)).unwrap_or_else(|_| format!("\n-- {name}: panicked\n"));
    let model = out.parent().unwrap_or(out).join("kratos_model");
    o.push_str(&step("model export", &|| {
        let _ = std::fs::create_dir_all(&model);
        export_kratos_model(gow, &buf, &model) + &format!("written to {}\n", model.display())
    }));
    let local = out.parent().unwrap_or(out);
    o.push_str(&step("textures and default outfit", &|| export_kratos_look(gow, &buf, &local.join("kratos_tex"), &local.join("kratos_default"))));
    o.push_str(&step(".as walk", &|| walk_kratos_sets(&wad, out)));
    o.push_str(&format!("written to {}\n", out.display()));
    o
}

/// Kratos's model at its most detailed: each part's first level of detail, read from `root.lodpack` (or the WAD's GPU
/// chunk for the few parts whose best level lives there), written compactly to `kratos_model/detail.grkm` with a list
/// of every part and level (`parts.tsv`) and a few whole resources (`samples/`) for checking the other streams.
/// Once per pack size and version (`done.txt`).
fn export_kratos_model(gow: &std::path::Path, buf: &[u8], out: &std::path::Path) -> String {
    let o = String::from("\n-- Kratos's model at its most detailed (kratos_model)\n");
    let Some(es) = gowring_gow::wtoc::read(buf) else { return o + "no chunk table\n" };
    let chunk = |name: &str, id: u16| es.iter().find(|e| e.name == name && e.id == id).and_then(|e| buf.get(e.file_off as usize..(e.file_off + e.length as u64) as usize));
    let (Some(mesh), Some(mg), Some(gpu)) = (chunk("MESH_heroa00_0", 1), chunk("MG_heroa00_0", 1), chunk("MG_heroa00_0_gpu", 29)) else {
        return o + "MESH_heroa00_0, MG_heroa00_0 or MG_heroa00_0_gpu not in the bytes read\n";
    };
    let Some(pack) = find_file(gow, "root.lodpack", 5) else { return o + "root.lodpack not found\n" };
    let size = std::fs::metadata(&pack).map(|m| m.len()).unwrap_or(0);
    let stamp = format!("v2 {size}\n");
    if std::fs::read_to_string(out.join("done.txt")).is_ok_and(|d| d == stamp) {
        return o + "already written for this install\n";
    }
    let o = o + &write_kratos_model(&pack, mesh, mg, gpu, out);
    if out.join("detail.grkm").exists() {
        let _ = std::fs::write(out.join("done.txt"), stamp);
    }
    o
}

fn write_kratos_model(pack: &std::path::Path, mesh: &[u8], mg: &[u8], gpu: &[u8], out: &std::path::Path) -> String {
    use gowring_gow::kratos::{compact_geometry, draw_geometry, mesh_records, mg_parts};
    use gowring_gow::lodpack::Toc;
    let mut o = String::new();
    let t0 = std::time::Instant::now();
    let toc = match Toc::read_from_pack(pack) {
        Ok(t) => t,
        Err(e) => return o + &format!("{e:#}\n"),
    };
    let rs = mesh_records(mesh);
    let parts = mg_parts(mg);
    let covered: usize = parts.iter().flatten().map(|l| l.draws as usize).sum();
    o.push_str(&format!("root.lodpack: {} blocks, {} resources; MESH_heroa00_0: {} draws; MG_heroa00_0: {} parts covering {covered} draws\n", toc.blocks.len(), toc.resources.len(), rs.len(), parts.len()));
    let _ = std::fs::create_dir_all(out.join("samples"));
    let mut tsv = String::from("part\tlevel\tdistance\trecord\tvertices\ttriangles\thash\twhere\tpack_offset\tbytes\n");
    for (p, levels) in parts.iter().enumerate() {
        for (l, lv) in levels.iter().enumerate() {
            for k in lv.first as usize..lv.first as usize + lv.draws as usize {
                let Some(r) = rs.get(k) else { continue };
                let (place, at, n) = match (r.hash, toc.find(r.hash)) {
                    (0, _) => ("wad".to_string(), 0, gpu.len() as u32),
                    (_, Some((at, n))) => ("root.lodpack".to_string(), at, n),
                    _ => ("missing".to_string(), 0, 0),
                };
                tsv.push_str(&format!("{p}\t{l}\t{}\t{k}\t{}\t{}\t{:016x}\t{place}\t{at}\t{n}\n", lv.distance, r.vertices, r.triangles, r.hash));
            }
        }
    }
    let _ = std::fs::write(out.join("parts.tsv"), tsv);
    // the most detailed level of every part
    let mut f = match std::fs::File::open(pack) {
        Ok(f) => f,
        Err(e) => return o + &format!("{}: {e}\n", pack.display()),
    };
    let mut cache: std::collections::HashMap<u64, Vec<u8>> = Default::default();
    let mut geo = Vec::new();
    let (mut bad, mut missing, mut read) = (Vec::new(), 0, 0u64);
    for (p, levels) in parts.iter().enumerate() {
        let Some(lv) = levels.first() else { continue };
        for k in lv.first as usize..lv.first as usize + lv.draws as usize {
            let Some(r) = rs.get(k) else { continue };
            let res: &[u8] = if r.hash == 0 {
                gpu
            } else {
                if !cache.contains_key(&r.hash) {
                    match toc.read(&mut f, r.hash) {
                        Ok(Some(b)) => {
                            read += b.len() as u64;
                            cache.insert(r.hash, b);
                        }
                        _ => {
                            missing += 1;
                            continue;
                        }
                    }
                }
                &cache[&r.hash]
            };
            match draw_geometry(res, r) {
                Some((pos, tri)) => geo.push((k as u32, p as u32, r, pos, tri)),
                None => bad.push(k),
            }
        }
    }
    let view: Vec<_> = geo.iter().map(|(k, p, r, pos, tri)| (*k, *p, *r, pos.as_slice(), tri.as_slice())).collect();
    let bytes = compact_geometry(&view);
    if !geo.is_empty() {
        let _ = std::fs::write(out.join("detail.grkm"), &bytes);
    }
    let (verts, tris): (usize, usize) = geo.iter().fold((0, 0), |(v, t), g| (v + g.3.len(), t + g.4.len()));
    o.push_str(&format!(
        "most detailed level: {} draws decoded ({verts} vertices, {tris} triangles) from {read} bytes of the pack; {} would not decode {:?}; {missing} missing; detail.grkm is {} bytes\n",
        geo.len(),
        bad.len(),
        &bad[..bad.len().min(12)],
        bytes.len()
    ));
    // whole resources for checking the other streams: the largest detailed draw, one shared by several, one small
    let mut by_size: Vec<(&u64, &Vec<u8>)> = cache.iter().collect();
    by_size.sort_by_key(|(h, b)| (std::cmp::Reverse(b.len()), **h));
    let shared = cache.keys().find(|h| rs.iter().filter(|r| r.hash == **h).count() > 1).copied();
    let mut picks: Vec<u64> = by_size.first().map(|(h, _)| **h).into_iter().chain(shared).chain(by_size.iter().rev().find(|(_, b)| b.len() > 4096).map(|(h, _)| **h)).collect();
    picks.dedup();
    let mut saved = 0;
    for h in picks {
        let b = &cache[&h];
        if saved + b.len() > 6 << 20 {
            continue;
        }
        saved += b.len();
        let recs: Vec<String> = rs.iter().enumerate().filter(|(_, r)| r.hash == h).map(|(k, _)| k.to_string()).collect();
        let _ = std::fs::write(out.join("samples").join(format!("{h:016x}_records_{}.bin", recs.join("_"))), b);
    }
    o.push_str(&format!("{saved} bytes of whole resources under samples/; took {:.1}s\n", t0.elapsed().as_secs_f32()));
    o
}

/// Kratos's default look, for working out his textures and the Elden Ring part: each of his materials' texture list
/// (the id-60 chunk after its `MAT_` chunk) in `kratos_tex/materials.tsv`; the default outfit's textures decoded from
/// the texture packs as previews (`kratos_tex/textures/*.png`, at most 512 pixels a side) with a few whole blocks kept
/// for checking the untiling (`kratos_tex/raw/`); and the default outfit's mesh resources from `root.lodpack`
/// (`kratos_default/`). Once per pack size and version (`done.txt` in each folder).
fn export_kratos_look(gow: &std::path::Path, buf: &[u8], tex_out: &std::path::Path, mesh_out: &std::path::Path) -> String {
    use gowring_gow::kratos::{material_names, mesh_records, mg_parts, DEFAULT_OUTFIT};
    let mut o = String::from("\n-- Kratos's default look: materials, textures and mesh resources (kratos_tex, kratos_default)\n");
    let Some(es) = gowring_gow::wtoc::read(buf) else { return o + "no chunk table\n" };
    let bytes = |e: &gowring_gow::wtoc::Entry| buf.get(e.file_off as usize..(e.file_off + e.length as u64) as usize);
    let chunk = |name: &str, id: u16| es.iter().find(|e| e.name == name && e.id == id).and_then(bytes);
    let (Some(mesh), Some(mg)) = (chunk("MESH_heroa00_0", 1), chunk("MG_heroa00_0", 1)) else { return o + "MESH_heroa00_0 or MG_heroa00_0 not in the bytes read\n" };
    // the mesh's material table is the id-60 chunk right after MESH_heroa00_0
    let Some(mesh_at) = es.iter().position(|e| e.name == "MESH_heroa00_0" && e.id == 1) else { return o };
    let Some(table) = es.get(mesh_at + 1).filter(|e| e.id == 60).and_then(bytes) else { return o + "no material table after MESH_heroa00_0\n" };
    let mats = material_names(table);
    let rs = mesh_records(mesh);
    let parts = mg_parts(mg);
    let default_records: Vec<usize> = DEFAULT_OUTFIT.iter().filter_map(|p| parts.get(*p as usize)).filter_map(|l| l.first()).flat_map(|l| l.first as usize..l.first as usize + l.draws as usize).collect();
    let default_mats: std::collections::BTreeSet<u16> = default_records.iter().filter_map(|k| rs.get(*k)).map(|r| r.material).collect();
    let Some(pack) = find_file(gow, "root.lodpack", 5) else { return o + "root.lodpack not found\n" };
    let stamp = format!("v1 {}\n", std::fs::metadata(&pack).map(|m| m.len()).unwrap_or(0));
    let done = |dir: &std::path::Path| std::fs::read_to_string(dir.join("done.txt")).is_ok_and(|d| d == stamp);
    let mut tsv = String::from("material\tname\tdefault\treferences\n");
    // (material, texture name, key from its id)
    let mut wanted: Vec<(u16, String, u64)> = Vec::new();
    let _ = std::fs::create_dir_all(tex_out.join("materials"));
    for (i, m) in mats.iter().enumerate() {
        let at = es.iter().position(|e| &e.name == m && e.id == 1);
        let refs = at.and_then(|a| es.get(a + 1)).filter(|e| e.id == 60).and_then(|e| bytes(e)).map(|b| {
            let _ = std::fs::write(tex_out.join("materials").join(format!("{i:02}_{m}_refs.bin")), b);
            gowring_gow::kratos::references(b)
        });
        let refs = refs.unwrap_or_default();
        let is_default = default_mats.contains(&(i as u16));
        if is_default {
            wanted.extend(refs.iter().filter(|r| r.1.starts_with("TX_")).map(|(id, n)| (i as u16, n.clone(), u64::from_le_bytes(id[8..16].try_into().unwrap()))));
        }
        tsv.push_str(&format!("{i}\t{m}\t{}\t{}\n", is_default as u8, refs.iter().map(|r| r.1.as_str()).collect::<Vec<_>>().join(" ")));
    }
    let _ = std::fs::write(tex_out.join("materials.tsv"), tsv);
    o.push_str(&format!("{} materials ({} used by the default outfit), {} texture references in those\n", mats.len(), default_mats.len(), wanted.len()));
    // textures: every texture pack's table, then each wanted texture's blocks
    let wad_dir = pack.parent().unwrap_or(gow).to_path_buf();
    if !done(tex_out) {
        let t0 = std::time::Instant::now();
        let mut tocs: Vec<(std::path::PathBuf, gowring_gow::texpack::Toc)> = Vec::new();
        if let Ok(rd) = std::fs::read_dir(&wad_dir) {
            let mut names: Vec<std::path::PathBuf> = rd.flatten().map(|e| e.path()).filter(|p| p.to_string_lossy().to_ascii_lowercase().ends_with(".texpack.toc")).collect();
            // the shared pack first
            names.sort_by_key(|p| (!p.to_string_lossy().to_ascii_lowercase().ends_with("root.texpack.toc"), p.clone()));
            for p in names {
                if let Ok(t) = gowring_gow::texpack::Toc::read(&p) {
                    tocs.push((p.with_extension(""), t));
                }
            }
        }
        o.push_str(&format!("{} texture pack tables read in {:.1}s\n", tocs.len(), t0.elapsed().as_secs_f32()));
        let _ = std::fs::create_dir_all(tex_out.join("textures"));
        let _ = std::fs::create_dir_all(tex_out.join("raw"));
        let mut list = String::from("material\ttexture\tpack\twidth\theight\tblocks\tresult\n");
        let (mut ok, mut failed, mut raw_saved, mut png_bytes) = (0, 0, 0usize, 0usize);
        let mut seen = std::collections::BTreeSet::new();
        for (mat, name, id_key) in &wanted {
            if !seen.insert(name.clone()) {
                continue;
            }
            // the name's hex suffix (unless the 60-character name cut it short), else the key in the reference's id
            let key = name.rsplit('_').next().filter(|h| h.len() == 16).and_then(|h| u64::from_str_radix(h, 16).ok());
            let found = [key, Some(*id_key), Some(id_key.swap_bytes())].into_iter().flatten().find_map(|k| tocs.iter().find_map(|(p, t)| t.find(k).map(|x| (p, x))));
            let line = match found {
                // small textures can live in the WAD alone (linear mips)
                None => match (chunk(name, 1).and_then(gowring_gow::wadtex::parse_parm), chunk(name, 29)) {
                    (Some(parm), Some(gpu)) => match gowring_gow::wadtex::decode(&parm, gpu) {
                        Ok((img, _)) => {
                            ok += 1;
                            if let Ok(png) = gowring_gow::dds::to_png(&gowring_gow::dds::downscale(&img, 512)) {
                                png_bytes += png.len();
                                let _ = std::fs::write(tex_out.join("textures").join(format!("{name}.png")), png);
                            }
                            format!("{}\t{}\t0\tdecoded from the WAD ({:?})", img.w, img.h, img.format)
                        }
                        Err(e) => {
                            failed += 1;
                            format!("\t\t0\tWAD only: {e:#}")
                        }
                    },
                    _ => {
                        failed += 1;
                        "\t\t0\tnot in any texture pack or the WAD's first 256 MB".to_string()
                    }
                },
                Some((p, t)) => {
                    let read = std::fs::File::open(p).and_then(|mut f| t.records.iter().map(|r| gowring_gow::texpack::read_record(&mut f, r).map_err(std::io::Error::other)).collect::<std::io::Result<Vec<_>>>());
                    match read {
                        Err(e) => format!("read failed: {e}"),
                        Ok(blocks) => {
                            let total: usize = blocks.iter().map(|b| b.len()).sum();
                            if raw_saved + total <= 3 << 20 && raw_saved < 3 << 20 && total <= 1 << 20 {
                                for (k, b) in blocks.iter().enumerate() {
                                    let _ = std::fs::write(tex_out.join("raw").join(format!("{name}_{k}.bin")), b);
                                }
                                raw_saved += total;
                            }
                            match gowring_gow::texpack::decode(&blocks) {
                                Ok(img) => {
                                    ok += 1;
                                    let small = gowring_gow::dds::downscale(&img, 512);
                                    if let Ok(png) = gowring_gow::dds::to_png(&small) {
                                        if png_bytes + png.len() <= 20 << 20 {
                                            png_bytes += png.len();
                                            let _ = std::fs::write(tex_out.join("textures").join(format!("{name}.png")), png);
                                        }
                                    }
                                    format!("{}\t{}\t{}\tdecoded ({:?})", img.w, img.h, t.records.len(), img.format)
                                }
                                Err(e) => {
                                    failed += 1;
                                    format!("\t\t{}\t{e:#}", t.records.len())
                                }
                            }
                        }
                    }
                }
            };
            let pack_name = found.map(|(p, _)| p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()).unwrap_or_default();
            list.push_str(&format!("{mat}\t{name}\t{pack_name}\t{line}\n"));
        }
        let _ = std::fs::write(tex_out.join("textures.tsv"), list);
        o.push_str(&format!("textures: {ok} decoded, {failed} failed, {} previews ({png_bytes} bytes), {raw_saved} bytes of raw blocks; {:.1}s\n", seen.len(), t0.elapsed().as_secs_f32()));
        if ok > 0 {
            let _ = std::fs::write(tex_out.join("done.txt"), &stamp);
        }
    } else {
        o.push_str("textures already written for this install\n");
    }
    // the default outfit's mesh resources
    if !done(mesh_out) {
        let _ = std::fs::create_dir_all(mesh_out);
        match gowring_gow::lodpack::Toc::read_from_pack(&pack).and_then(|toc| Ok((toc, std::fs::File::open(&pack)?))) {
            Err(e) => o.push_str(&format!("{e:#}\n")),
            Ok((toc, mut f)) => {
                let (mut n, mut total) = (0, 0usize);
                let mut seen = std::collections::BTreeSet::new();
                for k in &default_records {
                    let Some(r) = rs.get(*k) else { continue };
                    if r.hash == 0 || !seen.insert(r.hash) {
                        continue;
                    }
                    if let Ok(Some(b)) = toc.read(&mut f, r.hash) {
                        let recs: Vec<String> = rs.iter().enumerate().filter(|(_, x)| x.hash == r.hash).map(|(i, _)| i.to_string()).collect();
                        total += b.len();
                        n += 1;
                        let _ = std::fs::write(mesh_out.join(format!("{:016x}_records_{}.bin", r.hash, recs.join("_"))), b);
                    }
                }
                o.push_str(&format!("default outfit: {} draws, {n} resources ({total} bytes) written to {}\n", default_records.len(), mesh_out.display()));
                if n > 0 {
                    let _ = std::fs::write(mesh_out.join("done.txt"), &stamp);
                }
            }
        }
    } else {
        o.push_str("default outfit resources already written for this install\n");
    }
    o + &format!("written to {} and {}\n", tex_out.display(), mesh_out.display())
}

/// Elden Ring's install, names and sizes only (`er_install.tsv`, a few levels deep), and a summary of what a body
/// part could be built from: packed archives (`.bhd`/`.bdt`), loose parts or characters (an unpacked install), the
/// Oodle library, and ModEngine2's mod folders.
fn list_er_install(er: &std::path::Path, out: &std::path::Path) -> String {
    let mut o = String::from("\n-- Elden Ring install (for Kratos's body part)\n");
    let root = if er.is_file() { er.parent().unwrap_or(er) } else { er };
    let mut files: Vec<(String, u64)> = Vec::new();
    fn walk(base: &std::path::Path, dir: &std::path::Path, depth: usize, files: &mut Vec<(String, u64)>) {
        let Ok(rd) = std::fs::read_dir(dir) else { return };
        for e in rd.flatten() {
            let p = e.path();
            match e.metadata() {
                Ok(m) if m.is_dir() && depth > 0 && files.len() < 20000 => walk(base, &p, depth - 1, files),
                Ok(m) if m.is_file() => files.push((p.strip_prefix(base).unwrap_or(&p).to_string_lossy().replace('\\', "/"), m.len())),
                _ => {}
            }
        }
    }
    walk(root, root, 3, &mut files);
    files.sort();
    let _ = std::fs::create_dir_all(out.parent().unwrap_or(root));
    let _ = std::fs::write(out, files.iter().map(|(p, n)| format!("{p}\t{n}\n")).collect::<String>());
    let has = |f: &dyn Fn(&str) -> bool| files.iter().filter(|(p, _)| f(&p.to_ascii_lowercase())).count();
    o.push_str(&format!(
        "{} files under {}; archives (.bhd {}, .bdt {}); loose parts/ {} and chr/ {} files; oo2core dlls {}; regulation.bin {}\n",
        files.len(),
        root.display(),
        has(&|p| p.ends_with(".bhd")),
        has(&|p| p.ends_with(".bdt")),
        has(&|p| p.starts_with("parts/")),
        has(&|p| p.starts_with("chr/")),
        has(&|p| p.contains("oo2core")),
        has(&|p| p.ends_with("regulation.bin")),
    ));
    o.push_str(&format!("written to {}\n", out.display()));
    o
}

/// Walk each of Kratos's `.as` files end to end once (until its size changes): `as_index_<file>.tsv` lists every
/// animation set, and a few whole sets of everyday moves go under `as_full/` for working out the bone motion.
fn walk_kratos_sets(wad: &std::path::Path, out: &std::path::Path) -> String {
    let mut o = String::from("\n-- every animation set in Kratos's .as files\n");
    let until = std::time::Instant::now() + std::time::Duration::from_secs(300);
    let full = out.join("as_full");
    let _ = std::fs::create_dir_all(&full);
    let mut pick = gowring_gow::kratos::study_picker(3, 64 << 20);
    for p in gowring_gow::kratos::sibling_sets(wad) {
        let Some(file) = p.file_name().map(|n| n.to_string_lossy().into_owned()) else { continue };
        let len = std::fs::metadata(&p).map(|m| m.len()).unwrap_or(0);
        let index = out.join(format!("as_index_{}.tsv", file.replace('.', "_")));
        let done = std::fs::read_to_string(&index).ok().and_then(|t| t.lines().next().map(|l| l.contains(&format!("\t{len} bytes")) && l.contains("complete=true"))).unwrap_or(false);
        if done {
            o.push_str(&format!("{file}: already indexed ({})\n", index.display()));
            continue;
        }
        if std::time::Instant::now() >= until {
            o.push_str(&format!("{file}: skipped (out of time)\n"));
            continue;
        }
        let started = std::time::Instant::now();
        let mut saved: Vec<String> = Vec::new();
        let walked = gowring_gow::kratos::walk_set_file(
            &p,
            &mut pick,
            |s, b| {
                let name = format!("{}_{}.bin", file.replace('.', "_"), s.name.chars().map(|c| if c.is_ascii_alphanumeric() || c == '_' { c } else { '_' }).collect::<String>());
                if std::fs::write(full.join(&name), b).is_ok() {
                    saved.push(format!("{} ({} bytes)", s.name, b.len()));
                }
            },
            || std::time::Instant::now() < until,
        );
        match walked {
            Ok(idx) => {
                let own = idx.sets.iter().filter(|(_, _, own)| *own).count();
                let _ = std::fs::write(&index, gowring_gow::kratos::index_tsv(&file, len, &idx));
                o.push_str(&format!(
                    "{file}: {len} bytes, lz4={}, walked {} bytes in {:.1}s (complete={}); {} set headers ({own} name their own place); saved whole: {:?}\n",
                    idx.lz4,
                    idx.bytes,
                    started.elapsed().as_secs_f32(),
                    idx.complete,
                    idx.sets.len(),
                    saved
                ));
            }
            Err(e) => o.push_str(&format!("{file}: {e:#}\n")),
        }
    }
    o
}

fn diagnose(ctx: &Ctx) -> Result<()> {
    let gow = ctx.gow.clone().unwrap_or_default();
    let vg = tools(ctx, 150).vgmstream;
    let text = gowring_gow::diagnose::report(&gow, ctx.er.as_deref(), &requests(), vg.as_deref());
    std::fs::create_dir_all(&ctx.local)?;
    let path = ctx.local.join("diagnose.txt");
    std::fs::write(&path, &text)?;
    println!("{text}\n\nSaved to {}", path.display());
    Ok(())
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let ctx = match Ctx::from_env(&args) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("GowRing: {e:#}");
            std::process::exit(2);
        }
    };
    {
        let c = ctx.clone();
        std::panic::set_hook(Box::new(move |info| c.log(&format!("PANIC: {info}"))));
    }
    ctx.log(&format!(
        "start: GowRing {} ({CHANNEL}) args={args:?} exe={:?} GOWRING_ER={:?} GOWRING_GOWR={:?} managed={} local={}",
        config::VERSION,
        std::env::current_exe().ok(),
        ctx.er,
        ctx.gow,
        ctx.managed.display(),
        ctx.local.display()
    ));
    let cmd = args.iter().find(|a| a.starts_with("--") && !matches!(a.as_str(), "--er" | "--gow" | "--managed" | "--local")).map(String::as_str);
    let result = match cmd {
        Some("--prepare") => prepare(&ctx),
        Some("--play") => play(&ctx),
        Some("--diagnose") => diagnose(&ctx),
        Some("--version") => {
            println!("GowRing {} ({CHANNEL})", config::VERSION);
            Ok(())
        }
        _ => {
            println!("GowRing {} — Kratos in the Lands Between\n\nUsage: GowRing --prepare | --play | --diagnose | --version\n  --er <Elden Ring folder> --gow <God of War Ragnarök folder>  (Melty passes these for you)", config::VERSION);
            Ok(())
        }
    };
    if let Err(e) = result {
        ctx.log(&format!("ERROR: {e:#}"));
        eprintln!("GowRing: {e:#}");
        std::process::exit(1);
    }
}

#[allow(dead_code)]
fn _unused(_: &Path) {}

#[cfg(test)]
mod tests {
    /// Kratos's real draw and part tables with a stand-in pack whose resources are made to fit them:
    /// `GOWRING_MESH=... GOWRING_MG=... GOWRING_MG_GPU=... cargo test -p gowring-launcher model -- --ignored`.
    #[test]
    #[ignore]
    fn model_export_reads_each_parts_best_level_from_the_pack() {
        let (Ok(mesh), Ok(mg), Ok(gpu)) = (std::env::var("GOWRING_MESH"), std::env::var("GOWRING_MG"), std::env::var("GOWRING_MG_GPU")) else { return };
        let (mesh, mg, gpu) = (std::fs::read(mesh).unwrap(), std::fs::read(mg).unwrap(), std::fs::read(gpu).unwrap());
        let rs = gowring_gow::kratos::mesh_records(&mesh);
        // every streamed resource, sized to its draws, with positions at each draw's centre and a fan of triangles
        let mut res: std::collections::BTreeMap<u64, Vec<u8>> = Default::default();
        for r in rs.iter().filter(|r| r.hash != 0) {
            let b = res.entry(r.hash).or_default();
            let k = r.streams.iter().position(|s| s.0 == 0).unwrap();
            let (pos, stride) = (r.streams[k].3 as usize, r.strides[k] as usize);
            let end = (r.vertex_bytes as usize + 2 * r.indices as usize).max(pos + stride * r.vertices as usize);
            if b.len() < end {
                b.resize(end, 0);
            }
            for v in 0..r.vertices as usize {
                for c in 0..3 {
                    b[pos + stride * v + 4 * c..pos + stride * v + 4 * c + 4].copy_from_slice(&r.centre[c].to_le_bytes());
                }
            }
            for i in 0..r.indices as usize {
                let at = r.vertex_bytes as usize + 2 * i;
                b[at..at + 2].copy_from_slice(&((i % r.vertices as usize) as u16).to_le_bytes());
            }
        }
        // one block per resource
        let n = res.len();
        let table = 16 + 24 * 2 * n;
        let mut pack = Vec::new();
        for v in [n as u32, n as u32, 0, 1] {
            pack.extend(v.to_le_bytes());
        }
        let mut at = table as u64;
        for (h, b) in &res {
            pack.extend(at.to_le_bytes());
            pack.extend(h.to_le_bytes());
            pack.extend((b.len() as u32).to_le_bytes());
            pack.extend(1u32.to_le_bytes());
            at += b.len() as u64;
        }
        for (k, (h, b)) in res.iter().enumerate() {
            for v in [k as u32, 0] {
                pack.extend(v.to_le_bytes());
            }
            pack.extend(h.to_le_bytes());
            pack.extend((b.len() as u32).to_le_bytes());
            pack.extend(0u32.to_le_bytes());
        }
        for b in res.values() {
            pack.extend_from_slice(b);
        }
        let dir = std::env::temp_dir().join(format!("gowring-model-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("root.lodpack"), &pack).unwrap();
        let out = dir.join("kratos_model");
        std::fs::create_dir_all(&out).unwrap();
        let report = super::write_kratos_model(&dir.join("root.lodpack"), &mesh, &mg, &gpu, &out);
        println!("{report}");
        assert!(report.contains("451 draws decoded"), "{report}");
        assert!(report.contains("0 would not decode") && report.contains("0 missing"), "{report}");
        let grkm = std::fs::read(out.join("detail.grkm")).unwrap();
        assert_eq!(&grkm[..4], b"GRKM");
        assert_eq!(std::fs::read_to_string(out.join("parts.tsv")).unwrap().lines().count(), 1 + rs.len());
        assert!(std::fs::read_dir(out.join("samples")).unwrap().count() >= 2);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
