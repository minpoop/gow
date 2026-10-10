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
        let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let vg = tools(&c, 60).vgmstream;
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(120);
            gowring_gow::diagnose::report_until(&gow, c.er.as_deref(), &requests(), vg.as_deref(), Some(deadline))
        }));
        match r {
            Ok(text) => {
                let _ = std::fs::create_dir_all(&c.local);
                match std::fs::write(c.local.join("diagnose.txt"), text) {
                    Ok(()) => c.log("diagnose: wrote diagnose.txt"),
                    Err(e) => c.log(&format!("diagnose: could not write the report: {e}")),
                }
            }
            Err(_) => c.log("diagnose: the report panicked"),
        }
    });
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
