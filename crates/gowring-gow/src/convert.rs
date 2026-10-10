//! Conversions: `.wem` → `.wav` (via the vgmstream-cli that ships in the package, ISC licence),
//! DDS → PNG (in-process).

use anyhow::{bail, Context, Result};
use std::path::{Path, PathBuf};
use std::process::Command;

/// `.wem` bytes → `.wav` file using vgmstream-cli.
pub fn wem_to_wav(vgmstream: &Path, wem: &[u8], tmp_dir: &Path, out: &Path) -> Result<()> {
    std::fs::create_dir_all(tmp_dir)?;
    if let Some(parent) = out.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let tmp: PathBuf = tmp_dir.join(format!("w{}.wem", std::process::id() as u64 * 1_000_003 + wem.len() as u64));
    std::fs::write(&tmp, wem)?;
    let result = Command::new(vgmstream).arg("-o").arg(out).arg(&tmp).output().with_context(|| format!("running {}", vgmstream.display()));
    let _ = std::fs::remove_file(&tmp);
    let o = result?;
    if !o.status.success() || !out.exists() {
        bail!("vgmstream failed ({}): {}", o.status, String::from_utf8_lossy(&o.stderr).lines().last().unwrap_or(""));
    }
    Ok(())
}

/// PCM `.wem` → `.wav` without any tool (the header only differs in the container).
pub fn pcm_wem_to_wav(wem: &[u8]) -> Option<Vec<u8>> {
    let info = crate::audiopack::wem_info(wem)?;
    if info.codec != 0x0001 && info.codec != 0xFFFE {
        return None;
    }
    Some(wem.to_vec())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pcm_passthrough_and_codec_gate() {
        let w = crate::audiopack::fake_pcm_wem(100, 48000);
        assert!(pcm_wem_to_wav(&w).is_some());
        let mut v = w.clone();
        v[20] = 0xFF;
        v[21] = 0xFF; // Wwise Vorbis tag
        assert!(pcm_wem_to_wav(&v).is_none());
    }

    #[test]
    fn missing_tool_is_a_clean_error() {
        let d = std::env::temp_dir().join("gowring-conv-test");
        let r = wem_to_wav(Path::new("/definitely/not/here"), b"x", &d, &d.join("o.wav"));
        assert!(r.is_err());
        let _ = std::fs::remove_dir_all(&d);
    }
}
