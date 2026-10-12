//! `ragnarok_hud.json`: which of Ragnarök's own UI textures stands in for which piece of the mod's HUD.
//!
//! The background check decodes the textures of the player's `r_ui.wad` into `ragnarok_ui/` (see `wadtex`). The
//! player (or we, once they tell us) writes a texture name next to a HUD piece in this file; before each Play the
//! launcher copies that picture to the place in the cache where the overlay looks for the piece's art. An empty
//! value keeps the mod's own drawing.

use anyhow::{bail, Result};
use std::fmt::Write;
use std::path::Path;

/// The pieces the first starter choices (0.1.24) covered: a file marked `_suggestions_applied` before the list of
/// suggested pieces was kept had these.
const FIRST_SUGGESTED: [&str; 4] = ["hud_health_frame", "hud_rage_frame", "hud_enemy_health", "hud_icon_shield"];

/// The file's text when it does not exist yet: every HUD piece, empty, with what it is.
pub fn template(pieces: &[(&str, &str)]) -> String {
    render(pieces, &[], &[])
}

/// The file's text with these values (pieces not listed are empty). `suggested` lists the pieces a starter choice was
/// offered for once, so a piece the player empties again stays empty.
pub fn render(pieces: &[(&str, &str)], values: &[(String, String)], suggested: &[String]) -> String {
    let mut o = String::from("{\n  \"_about\": \"Put the name of a picture from the ragnarok_ui folder (for example TX_sonhud_meter_bg_1234ABCD) next to a HUD piece, save, and start the game again. Leave a piece empty to keep the mod's own drawing. Open ragnarok_ui/index.html in a browser to see every picture with its name. White shapes are coloured by the HUD (health green, Rage red).\"");
    if !suggested.is_empty() {
        o.push_str(",\n  \"_suggestions_applied\": \"Starter choices were filled in once for the pieces in _suggested; change or empty any of them and they stay as you leave them.\"");
        let _ = write!(o, ",\n  \"_suggested\": \"{}\"", suggested.join(" ").replace('"', ""));
    }
    for (id, what) in pieces {
        let v = values.iter().find(|(k, _)| k == id).map(|(_, v)| v.replace('"', "")).unwrap_or_default();
        let _ = write!(o, ",\n\n  \"{id}\": \"{v}\",\n  \"_{id}\": \"{}\"", what.replace('"', "'"));
    }
    o.push_str("\n}\n");
    o
}

/// The pieces a file has already been offered a starter choice for.
fn suggested_ids(obj: &serde_json::Map<String, serde_json::Value>) -> Vec<String> {
    match obj.get("_suggested").and_then(|v| v.as_str()) {
        Some(list) => list.split_whitespace().map(str::to_string).collect(),
        None if obj.contains_key("_suggestions_applied") => FIRST_SUGGESTED.iter().map(|s| s.to_string()).collect(),
        None => Vec::new(),
    }
}

/// Offer each starter choice once: a piece never offered one gets it when it is empty and its picture was exported;
/// a piece the player already chose for is only marked offered, and a piece offered once is never filled again (so
/// emptying it sticks). A picture not exported yet is offered again later. `None` when nothing changes.
pub fn fill_suggestions(text: &str, pieces: &[(&str, &str)], suggestions: &[(&str, &str)], ui_dir: &Path) -> Option<String> {
    let v: serde_json::Value = serde_json::from_str(text).ok()?;
    let obj = v.as_object()?;
    let mut offered = suggested_ids(obj);
    let mut values: Vec<(String, String)> = obj
        .iter()
        .filter(|(k, _)| !k.starts_with('_'))
        .filter_map(|(k, v)| v.as_str().map(|s| (k.clone(), s.trim().to_string())))
        .filter(|(_, s)| !s.is_empty())
        .collect();
    let mut changed = false;
    for (id, tex) in suggestions {
        if offered.iter().any(|o| o == id) || !pieces.iter().any(|(p, _)| p == id) {
            continue;
        }
        let chosen = values.iter().any(|(k, _)| k == id);
        if !chosen {
            if !ui_dir.join(picture_name(tex)).exists() {
                continue;
            }
            values.push((id.to_string(), tex.to_string()));
        }
        offered.push(id.to_string());
        changed = true;
    }
    changed.then(|| render(pieces, &values, &offered))
}

/// List HUD pieces a newer version added (for example the runic icons) in a file that predates them, keeping every
/// choice and the starter-choices marker. `None` when nothing is missing or the file cannot be read.
pub fn add_new_pieces(text: &str, pieces: &[(&str, &str)]) -> Option<String> {
    let v: serde_json::Value = serde_json::from_str(text).ok()?;
    let obj = v.as_object()?;
    if pieces.iter().all(|(id, _)| obj.contains_key(*id)) {
        return None;
    }
    let values: Vec<(String, String)> = obj
        .iter()
        .filter(|(k, _)| !k.starts_with('_'))
        .filter_map(|(k, v)| v.as_str().map(|s| (k.clone(), s.trim().to_string())))
        .filter(|(_, s)| !s.is_empty())
        .collect();
    Some(render(pieces, &values, &suggested_ids(obj)))
}

/// The non-empty choices: (HUD piece id, texture name).
pub fn parse(text: &str) -> Result<Vec<(String, String)>> {
    let v: serde_json::Value = serde_json::from_str(text)?;
    let Some(obj) = v.as_object() else { bail!("the file must contain one JSON object") };
    Ok(obj
        .iter()
        .filter(|(k, _)| !k.starts_with('_'))
        .filter_map(|(k, v)| v.as_str().map(|s| (k.clone(), s.trim().to_string())))
        .filter(|(_, s)| !s.is_empty())
        .collect())
}

/// The file name a texture was exported under (same rule as `wadtex::export`).
pub fn picture_name(texture: &str) -> String {
    let t = texture.trim().trim_end_matches(".png");
    let safe: String = t.chars().map(|c| if c.is_ascii_alphanumeric() || c == '_' || c == '-' { c } else { '_' }).collect();
    format!("{safe}.png")
}

/// Copy each chosen picture to its HUD piece's place in the cache. `outputs` maps a piece id to its cache path.
/// Pieces that are no longer chosen get their copied picture removed (only pictures this step put there).
pub fn apply(choices: &[(String, String)], ui_dir: &Path, cache: &Path, outputs: &[(&str, &str)]) -> String {
    let mut o = String::new();
    let marker = cache.join("hud").join("from_ragnarok_ui.txt");
    let previously: Vec<String> = std::fs::read_to_string(&marker).map(|t| t.lines().map(str::to_string).collect()).unwrap_or_default();
    let mut placed: Vec<String> = Vec::new();
    for (id, tex) in choices {
        let Some((_, out)) = outputs.iter().find(|(i, _)| i == id) else {
            let _ = writeln!(o, "{id}: not a HUD piece (ignored)");
            continue;
        };
        let src = ui_dir.join(picture_name(tex));
        let dst = cache.join(out);
        if !src.exists() {
            let _ = writeln!(o, "{id}: {} not found in {}", picture_name(tex), ui_dir.display());
            continue;
        }
        if let Some(parent) = dst.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        match std::fs::copy(&src, &dst) {
            Ok(_) => {
                let _ = writeln!(o, "{id}: using {}", picture_name(tex));
                placed.push(out.to_string());
            }
            Err(e) => {
                let _ = writeln!(o, "{id}: could not copy {}: {e}", src.display());
            }
        }
    }
    for old in previously.iter().filter(|p| !placed.contains(p)) {
        if std::fs::remove_file(cache.join(old)).is_ok() {
            let _ = writeln!(o, "{old}: no longer chosen, back to the mod's own drawing");
        }
    }
    if let Some(parent) = marker.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let _ = std::fs::write(&marker, placed.join("\n"));
    o
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn template_parses_to_no_choices_and_a_choice_is_copied_then_undone() {
        let pieces = [("hud_health_frame", "Kratos' health bar frame"), ("hud_rage_fill", "Spartan Rage fill")];
        let t = template(&pieces);
        assert!(parse(&t).unwrap().is_empty(), "an untouched file chooses nothing");
        let dir = std::env::temp_dir().join(format!("gowring-hudmap-{}", std::process::id()));
        let (ui, cache) = (dir.join("ragnarok_ui"), dir.join("cache"));
        std::fs::create_dir_all(&ui).unwrap();
        std::fs::write(ui.join("TX_sonhud_meter_1234.png"), b"\x89PNG-test").unwrap();
        let edited = t.replacen("\"hud_health_frame\": \"\"", "\"hud_health_frame\": \"TX_sonhud_meter_1234\"", 1);
        let choices = parse(&edited).unwrap();
        assert_eq!(choices, vec![("hud_health_frame".to_string(), "TX_sonhud_meter_1234".to_string())]);
        let outputs = [("hud_health_frame", "hud/health_frame.png"), ("hud_rage_fill", "hud/rage_fill.png")];
        let r = apply(&choices, &ui, &cache, &outputs);
        assert!(r.contains("using TX_sonhud_meter_1234.png"), "{r}");
        assert_eq!(std::fs::read(cache.join("hud/health_frame.png")).unwrap(), b"\x89PNG-test");
        // emptied again: the copy goes, the mod's drawing comes back
        let r = apply(&[], &ui, &cache, &outputs);
        assert!(r.contains("no longer chosen"), "{r}");
        assert!(!cache.join("hud/health_frame.png").exists());
        // a name that was not exported is reported, not copied
        let r = apply(&[("hud_rage_fill".into(), "TX_missing".into())], &ui, &cache, &outputs);
        assert!(r.contains("not found"), "{r}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn new_pieces_are_listed_without_touching_choices() {
        let old = [("hud_health_frame", "health frame")];
        let now = [("hud_health_frame", "health frame"), ("hud_icon_runic_glacial", "Glacial Cleave icon")];
        let mine = render(&old, &[("hud_health_frame".into(), "TX_mine".into())], &["hud_health_frame".to_string()]);
        let grown = add_new_pieces(&mine, &now).expect("a piece to add");
        assert!(grown.contains("\"hud_icon_runic_glacial\": \"\""), "{grown}");
        assert!(grown.contains("_suggestions_applied"), "the marker stays");
        assert_eq!(parse(&grown).unwrap(), vec![("hud_health_frame".to_string(), "TX_mine".to_string())]);
        assert!(add_new_pieces(&grown, &now).is_none(), "nothing more to add");
    }

    #[test]
    fn broken_files_are_errors() {
        assert!(parse("{ nope").is_err());
        assert!(parse("[]").is_err());
    }

    #[test]
    fn starter_choices_fill_empty_pieces_once_and_never_override() {
        let pieces = [("hud_health_frame", "health frame"), ("hud_rage_frame", "rage frame"), ("hud_icon_shield", "shield")];
        let dir = std::env::temp_dir().join(format!("gowring-hudmap-s-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        for n in ["TX_health_A", "TX_rage_B"] {
            std::fs::write(dir.join(format!("{n}.png")), b"x").unwrap();
        }
        let sugg = [("hud_health_frame", "TX_health_A"), ("hud_rage_frame", "TX_rage_B"), ("hud_icon_shield", "TX_not_exported")];
        // the player already chose a rage picture: kept; the missing picture is not suggested
        let mine = render(&pieces, &[("hud_rage_frame".into(), "TX_mine".into())], &[]);
        let filled = fill_suggestions(&mine, &pieces, &sugg, &dir).expect("something to add");
        let got = parse(&filled).unwrap();
        assert!(got.contains(&("hud_health_frame".to_string(), "TX_health_A".to_string())));
        assert!(got.contains(&("hud_rage_frame".to_string(), "TX_mine".to_string())), "the player's choice wins");
        assert!(!got.iter().any(|(k, _)| k == "hud_icon_shield"));
        // emptied again by the player: never refilled
        let emptied = filled.replace("\"TX_health_A\"", "\"\"");
        assert!(fill_suggestions(&emptied, &pieces, &sugg, &dir).is_none());
        // the shield's picture exported later: offered then, once
        std::fs::write(dir.join("TX_not_exported.png"), b"x").unwrap();
        let later = fill_suggestions(&emptied, &pieces, &sugg, &dir).expect("the shield now");
        assert!(parse(&later).unwrap().contains(&("hud_icon_shield".to_string(), "TX_not_exported".to_string())));
        assert!(!parse(&later).unwrap().iter().any(|(k, _)| k == "hud_health_frame"), "still empty as the player left it");
        assert!(fill_suggestions(&later, &pieces, &sugg, &dir).is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_file_from_before_the_list_gets_only_the_new_starter_choices() {
        // 0.1.24 to 0.1.29 marked the file without saying which pieces: those were the first four
        let pieces = [("hud_health_frame", "health frame"), ("hud_icon_shield", "shield"), ("hud_part_plate_face", "plate face")];
        let dir = std::env::temp_dir().join(format!("gowring-hudmap-o-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        for n in ["TX_health_A", "TX_plate_F"] {
            std::fs::write(dir.join(format!("{n}.png")), b"x").unwrap();
        }
        let old = "{\n  \"_suggestions_applied\": \"...\",\n  \"hud_health_frame\": \"\",\n  \"hud_icon_shield\": \"\",\n  \"hud_part_plate_face\": \"\"\n}";
        let sugg = [("hud_health_frame", "TX_health_A"), ("hud_part_plate_face", "TX_plate_F")];
        let got = parse(&fill_suggestions(old, &pieces, &sugg, &dir).expect("the plate face")).unwrap();
        assert_eq!(got, vec![("hud_part_plate_face".to_string(), "TX_plate_F".to_string())], "the emptied health frame stays empty");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
