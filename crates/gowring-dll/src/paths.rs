//! Where things live. The launcher passes everything in environment variables; when the DLL is
//! loaded some other way it falls back to the folder next to itself.

use std::path::PathBuf;
use std::sync::OnceLock;

pub struct Paths {
    pub home: PathBuf,
    pub cache: PathBuf,
    pub local: PathBuf,
    pub channel: String,
    pub save_file: String,
    /// Set when the launcher started us (GOWRING_HOME present).
    pub from_launcher: bool,
}

static PATHS: OnceLock<Paths> = OnceLock::new();

fn env(k: &str) -> Option<PathBuf> {
    std::env::var_os(k).filter(|v| !v.is_empty()).map(PathBuf::from)
}

pub fn init(dll_dir: Option<PathBuf>) -> &'static Paths {
    PATHS.get_or_init(|| {
        let from_launcher = env("GOWRING_HOME").is_some();
        let home = env("GOWRING_HOME").or(dll_dir).unwrap_or_else(|| PathBuf::from("."));
        let local = env("GOWRING_LOCAL").or_else(|| env("LOCALAPPDATA").map(|p| p.join("GowRing"))).unwrap_or_else(|| home.join("local"));
        let cache = env("GOWRING_CACHE").unwrap_or_else(|| local.join("cache"));
        Paths {
            home,
            cache,
            local,
            channel: std::env::var("GOWRING_CHANNEL").unwrap_or_else(|_| "alpha".into()),
            save_file: std::env::var("GOWRING_SAVE").unwrap_or_else(|_| "ER0000.gowring.sl2".into()),
            from_launcher,
        }
    })
}

pub fn get() -> &'static Paths {
    PATHS.get().expect("paths::init not called")
}
