use anyhow::{Context, Result};
use std::io::Write;
use std::path::PathBuf;

pub const VERSION: &str = env!("CARGO_PKG_VERSION");
pub const CHANNEL: &str = "alpha";
/// Bump the trailing number whenever the prepare output format changes.
pub const READY_TOKEN: &str = concat!("gowring-", env!("CARGO_PKG_VERSION"), "-prep-1");
/// The mashup's own save file; the DLL redirects Elden Ring to it.
pub const SAVE_FILE: &str = "ER0000.gowring.sl2";

pub struct Ctx {
    /// Folder GowRing.exe lives in (Melty's `{managed}`).
    pub managed: PathBuf,
    /// `%LOCALAPPDATA%/GowRing`
    pub local: PathBuf,
    pub cache: PathBuf,
    pub er: Option<PathBuf>,
    pub gow: Option<PathBuf>,
}

fn arg_value(args: &[String], name: &str) -> Option<PathBuf> {
    args.iter().position(|a| a == name).and_then(|i| args.get(i + 1)).map(PathBuf::from)
}

impl Ctx {
    pub fn from_env(args: &[String]) -> Result<Ctx> {
        let managed = match arg_value(args, "--managed") {
            Some(p) => p,
            None => std::env::current_exe().context("finding GowRing.exe")?.parent().context("exe has no folder")?.to_path_buf(),
        };
        let env_path = |k: &str| std::env::var_os(k).filter(|v| !v.is_empty()).map(PathBuf::from);
        let local = arg_value(args, "--local")
            .or_else(|| env_path("GOWRING_LOCAL"))
            .or_else(|| env_path("LOCALAPPDATA").map(|p| p.join("GowRing")))
            .unwrap_or_else(|| managed.join("local"));
        let cache = env_path("GOWRING_CACHE").unwrap_or_else(|| local.join("cache"));
        Ok(Ctx {
            managed,
            local,
            cache,
            er: arg_value(args, "--er").or_else(|| env_path("GOWRING_ER")),
            gow: arg_value(args, "--gow").or_else(|| env_path("GOWRING_GOWR")),
        })
    }

    pub fn log(&self, line: &str) {
        let dir = self.local.join("logs");
        let _ = std::fs::create_dir_all(&dir);
        if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(dir.join("launcher.log")) {
            let secs = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
            let _ = writeln!(f, "[{secs}] {line}");
        }
    }
}
