//! The ModEngine2 config that loads gowring.dll (and nothing else).

use crate::config::Ctx;
use anyhow::Result;
use std::path::{Path, PathBuf};

/// TOML basic-string escape for a Windows path.
fn toml_path(p: &Path) -> String {
    p.to_string_lossy().replace('\\', "\\\\").replace('"', "\\\"")
}

pub fn render(dll: &Path, mod_dir: &Path) -> String {
    format!(
        "# Written by GowRing.exe on every Play. Do not edit.\n\
         [modengine]\n\
         debug = false\n\
         external_dlls = [\"{dll}\"]\n\
         \n\
         [extension.mod_loader]\n\
         enabled = true\n\
         loose_params = false\n\
         mods = [\n    {{ enabled = true, name = \"gowring\", path = \"{mods}\" }}\n]\n\
         \n\
         [extension.scylla_hide]\n\
         enabled = false\n",
        dll = toml_path(dll),
        mods = toml_path(mod_dir)
    )
}

pub fn write_config(ctx: &Ctx, dll: &Path) -> Result<PathBuf> {
    let mod_dir = ctx.managed.join("mod");
    std::fs::create_dir_all(&mod_dir)?;
    let path = ctx.managed.join("gowring_me2.toml");
    std::fs::write(&path, render(dll, &mod_dir))?;
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_is_valid_toml_shape_and_escapes_windows_paths() {
        let t = render(Path::new(r"C:\Melty\mods\gow\gowring.dll"), Path::new(r"C:\Melty\mods\gow\mod"));
        assert!(t.contains(r#"external_dlls = ["C:\\Melty\\mods\\gow\\gowring.dll"]"#));
        assert!(t.contains(r#"path = "C:\\Melty\\mods\\gow\\mod""#));
        assert!(t.contains("[extension.scylla_hide]\nenabled = false"));
    }
}
