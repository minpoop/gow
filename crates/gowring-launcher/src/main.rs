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

fn tools(ctx: &Ctx) -> Tools {
    let exe = ctx.managed.join("tools").join("vgmstream-cli.exe");
    Tools { vgmstream: exe.exists().then_some(exe) }
}

fn overrides(ctx: &Ctx) -> serde_json::Value {
    std::fs::read_to_string(ctx.managed.join("assets_map.json"))
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_else(|| serde_json::json!({}))
}

fn prepare(ctx: &Ctx) -> Result<()> {
    let gow = ctx.gow.as_ref().context("God of War Ragnarök was not found (GOWRING_GOWR is not set)")?;
    ctx.log(&format!("prepare: reading {}", gow.display()));
    let _ = std::fs::remove_file(ctx.managed.join(".melty-ready"));
    let _ = std::fs::remove_file(ctx.managed.join(".melty-failure.txt"));
    let reqs = requests();
    let report = gowring_gow::prepare::prepare(gow, &reqs, &overrides(ctx), &ctx.cache, &tools(ctx))?;
    let manifest = serde_json::json!({
        "version": config::VERSION,
        "channel": CHANNEL,
        "gow_folder": gow.to_string_lossy(),
        "report": report,
    });
    std::fs::create_dir_all(&ctx.cache)?;
    std::fs::write(ctx.cache.join("manifest.json"), serde_json::to_string_pretty(&manifest)?)?;
    let total = reqs.len();
    let ok = report.converted();
    ctx.log(&format!("prepare: converted {ok}/{total}; missing {:?}", report.missing()));
    if ok < total && CHANNEL == "release" {
        let msg = format!(
            "Kratos in the Lands Between could not read {} of {} pieces from your God of War Ragnarök install ({}). \
             Run GowRing.exe --diagnose and send the report to the mashup's page.",
            total - ok,
            total,
            report.missing().join(", ")
        );
        std::fs::write(ctx.managed.join(".melty-failure.txt"), &msg)?;
        bail!(msg);
    }
    std::fs::write(ctx.managed.join(".melty-ready"), config::READY_TOKEN)?;
    println!("Prepared {ok}/{total} Ragnarök assets ({} channel). Cache: {}", CHANNEL, ctx.cache.display());
    Ok(())
}

fn prepared(ctx: &Ctx) -> bool {
    std::fs::read_to_string(ctx.managed.join(".melty-ready")).map(|t| t.contains(config::READY_TOKEN)).unwrap_or(false)
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
    let launcher = ctx.managed.join("modengine2").join("modengine2_launcher.exe");
    if !launcher.exists() {
        bail!("ModEngine2 is not installed at {}", launcher.display());
    }
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
        .env("GOWRING_SAVE", config::SAVE_FILE);
    let status = cmd.status().with_context(|| format!("starting {}", launcher.display()))?;
    if !status.success() {
        bail!("ModEngine2 launcher exited with {status}");
    }
    ctx.log("play: ModEngine2 started Elden Ring");
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

fn diagnose(ctx: &Ctx) -> Result<()> {
    let gow = ctx.gow.clone().unwrap_or_default();
    let vg = tools(ctx).vgmstream;
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
