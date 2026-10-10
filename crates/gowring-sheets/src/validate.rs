//! Preflight: lay every sheet over the others and list what is unfilled, out of range or unresolved.

use crate::model::{ColType, Column, Sheet, Sheets};
use serde_json::{Map, Value};
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Severity {
    /// Blocks the build.
    Error,
    /// Worth a look; does not block.
    Warn,
    /// Verification tallies.
    Info,
}

#[derive(Debug, Clone)]
pub struct Finding {
    pub severity: Severity,
    pub sheet: String,
    pub row: Option<String>,
    pub column: Option<String>,
    pub message: String,
}

#[derive(Debug, Default)]
pub struct Report {
    pub findings: Vec<Finding>,
    /// Cells checked (rows x columns), for the summary line.
    pub cells: usize,
    pub rows: usize,
}

impl Report {
    fn push(&mut self, severity: Severity, sheet: &str, row: Option<&str>, column: Option<&str>, message: String) {
        self.findings.push(Finding {
            severity,
            sheet: sheet.to_string(),
            row: row.map(str::to_string),
            column: column.map(str::to_string),
            message,
        });
    }
    fn err(&mut self, sheet: &str, row: Option<&str>, column: Option<&str>, msg: impl Into<String>) {
        self.push(Severity::Error, sheet, row, column, msg.into());
    }
    fn warn(&mut self, sheet: &str, row: Option<&str>, column: Option<&str>, msg: impl Into<String>) {
        self.push(Severity::Warn, sheet, row, column, msg.into());
    }
    fn info(&mut self, sheet: &str, msg: impl Into<String>) {
        self.push(Severity::Info, sheet, None, None, msg.into());
    }
    pub fn errors(&self) -> usize {
        self.findings.iter().filter(|f| f.severity == Severity::Error).count()
    }
    pub fn warnings(&self) -> usize {
        self.findings.iter().filter(|f| f.severity == Severity::Warn).count()
    }
    pub fn is_clean(&self) -> bool {
        self.errors() == 0
    }
}

fn is_snake(s: &str) -> bool {
    let mut chars = s.chars();
    matches!(chars.next(), Some(c) if c.is_ascii_lowercase())
        && chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
}

fn is_color(s: &str) -> bool {
    s.len() == 7 && s.starts_with('#') && s[1..].chars().all(|c| c.is_ascii_hexdigit())
}

/// Run the whole preflight.
pub fn check(sheets: &Sheets) -> Report {
    let mut r = Report::default();
    let all_ids: HashMap<&str, HashSet<&str>> =
        sheets.sheets.iter().map(|s| (s.name.as_str(), s.ids().collect())).collect();

    for sheet in &sheets.sheets {
        check_sheet(sheet, &all_ids, &mut r);
    }
    project_rules(sheets, &mut r);
    r
}

fn check_sheet(sheet: &Sheet, all_ids: &HashMap<&str, HashSet<&str>>, r: &mut Report) {
    r.rows += sheet.rows.len();
    if sheet.rows.is_empty() {
        r.err(&sheet.name, None, None, "sheet has no rows");
    }
    if sheet.column("id").map(|c| c.ty) != Some(ColType::Id) {
        r.err(&sheet.name, None, Some("id"), "first column must be `id` of type id");
    }
    let known: HashSet<&str> = sheet.columns.iter().map(|c| c.name.as_str()).collect();
    let mut seen_ids: HashSet<String> = HashSet::new();

    for (i, row) in sheet.rows.iter().enumerate() {
        let rid_owned = Sheet::row_id(row).map(str::to_string);
        let rid = rid_owned.as_deref().unwrap_or("");
        let label = if rid.is_empty() { format!("#{i}") } else { rid.to_string() };
        for key in row.keys() {
            if !known.contains(key.as_str()) {
                r.err(&sheet.name, Some(&label), Some(key), "column is not declared in the sheet's schema");
            }
        }
        if !rid.is_empty() && !seen_ids.insert(rid.to_string()) {
            r.err(&sheet.name, Some(&label), Some("id"), "duplicate id");
        }
        for col in &sheet.columns {
            r.cells += 1;
            match row.get(&col.name) {
                None => r.err(&sheet.name, Some(&label), Some(&col.name), "unfilled (key missing)"),
                Some(v) => check_cell(sheet, &label, col, v, all_ids, r),
            }
        }
    }
}

fn check_cell(
    sheet: &Sheet,
    label: &str,
    col: &Column,
    v: &Value,
    all_ids: &HashMap<&str, HashSet<&str>>,
    r: &mut Report,
) {
    let (s, l, c) = (sheet.name.as_str(), Some(label), Some(col.name.as_str()));
    if v.is_null() && col.ty != ColType::RefOpt {
        r.err(s, l, c, "unfilled (null)");
        return;
    }
    match col.ty {
        ColType::Id => match v.as_str() {
            Some(x) if is_snake(x) => {}
            Some(x) => r.err(s, l, c, format!("id `{x}` is not snake_case")),
            None => r.err(s, l, c, "id must be a string"),
        },
        ColType::Str | ColType::Text => match v.as_str() {
            Some(x) if !x.trim().is_empty() => {}
            _ => r.err(s, l, c, "unfilled (empty text)"),
        },
        ColType::Int => match v.as_i64() {
            Some(n) => check_range(s, l, c, n as f64, col, r),
            None => r.err(s, l, c, format!("expected integer, got {v}")),
        },
        ColType::Float => match v.as_f64() {
            Some(n) => check_range(s, l, c, n, col, r),
            None => r.err(s, l, c, format!("expected number, got {v}")),
        },
        ColType::Bool => {
            if !v.is_boolean() {
                r.err(s, l, c, format!("expected true/false, got {v}"));
            }
        }
        ColType::Color => match v.as_str() {
            Some(x) if is_color(x) => {}
            _ => r.err(s, l, c, format!("expected #rrggbb, got {v}")),
        },
        ColType::Enum => match v.as_str() {
            Some(x) if col.values.iter().any(|a| a == x) => {}
            _ => r.err(s, l, c, format!("`{v}` is not one of {:?}", col.values)),
        },
        ColType::ListEnum => match v.as_array() {
            Some(a) => {
                for item in a {
                    match item.as_str() {
                        Some(x) if col.values.iter().any(|a| a == x) => {}
                        _ => r.err(s, l, c, format!("`{item}` is not one of {:?}", col.values)),
                    }
                }
            }
            None => r.err(s, l, c, "expected a list"),
        },
        ColType::Ref | ColType::RefOpt => {
            if v.is_null() {
                return;
            }
            check_ref(s, l, c, col, v, all_ids, r);
        }
        ColType::ListRef => match v.as_array() {
            Some(a) => {
                let mut seen = HashSet::new();
                for item in a {
                    check_ref(s, l, c, col, item, all_ids, r);
                    if let Some(x) = item.as_str() {
                        if !seen.insert(x) {
                            r.err(s, l, c, format!("`{x}` listed twice"));
                        }
                    }
                }
            }
            None => r.err(s, l, c, "expected a list"),
        },
    }
}

fn check_range(s: &str, l: Option<&str>, c: Option<&str>, n: f64, col: &Column, r: &mut Report) {
    if let Some(min) = col.min {
        if n < min {
            r.err(s, l, c, format!("{n} is below the minimum {min}"));
        }
    }
    if let Some(max) = col.max {
        if n > max {
            r.err(s, l, c, format!("{n} is above the maximum {max}"));
        }
    }
}

fn check_ref(
    s: &str,
    l: Option<&str>,
    c: Option<&str>,
    col: &Column,
    v: &Value,
    all_ids: &HashMap<&str, HashSet<&str>>,
    r: &mut Report,
) {
    let target = col.sheet.as_deref().unwrap_or("");
    let Some(id) = v.as_str() else {
        r.err(s, l, c, format!("reference must be a string id, got {v}"));
        return;
    };
    match all_ids.get(target) {
        None => r.err(s, l, c, format!("references unknown sheet `{target}`")),
        Some(ids) if !ids.contains(id) => {
            r.err(s, l, c, format!("`{id}` does not exist in sheet `{target}`"));
        }
        _ => {}
    }
}

// ---------------------------------------------------------------------------------------------
// Cross-sheet rules that are specific to this mashup.
// ---------------------------------------------------------------------------------------------

fn text<'a>(row: &'a Map<String, Value>, key: &str) -> &'a str {
    row.get(key).and_then(Value::as_str).unwrap_or("")
}
fn opt_text<'a>(row: &'a Map<String, Value>, key: &str) -> Option<&'a str> {
    row.get(key).and_then(Value::as_str)
}
fn list<'a>(row: &'a Map<String, Value>, key: &str) -> Vec<&'a str> {
    row.get(key)
        .and_then(Value::as_array)
        .map(|a| a.iter().filter_map(Value::as_str).collect())
        .unwrap_or_default()
}

fn project_rules(sheets: &Sheets, r: &mut Report) {
    let get = |n: &str| sheets.get(n);
    let (Some(weapons), Some(moves), Some(runics)) = (get("weapons"), get("moves"), get("runics")) else {
        r.err("*", None, None, "weapons, moves and runics sheets are all required");
        return;
    };

    // --- weapon <-> move ownership, both directions -------------------------------------------
    let mut owned: HashMap<String, (String, &str)> = HashMap::new(); // move -> (weapon, list)
    for w in &weapons.rows {
        let wid = text(w, "id");
        for (key, kinds) in [("lights", &["light", "sprint", "aerial"][..]), ("heavies", &["heavy"][..]), ("specials", &[][..])] {
            for m in list(w, key) {
                if let Some(prev) = owned.insert(m.to_string(), (wid.to_string(), key)) {
                    r.err("weapons", Some(wid), Some(key), format!("move `{m}` is also listed by `{}`", prev.0));
                }
                if let Some(mrow) = moves.find(m) {
                    if text(mrow, "weapon") != wid {
                        r.err("weapons", Some(wid), Some(key), format!("lists `{m}` but moves.{m}.weapon is `{}`", text(mrow, "weapon")));
                    }
                    let kind = text(mrow, "kind");
                    if !kinds.is_empty() && !kinds.contains(&kind) {
                        r.err("weapons", Some(wid), Some(key), format!("`{m}` is a {kind} move, not allowed in `{key}`"));
                    }
                }
            }
        }
    }
    for m in &moves.rows {
        let mid = text(m, "id");
        if !owned.contains_key(mid) {
            r.err("moves", Some(mid), Some("weapon"), "move is not listed by its weapon (lights/heavies/specials)");
        }
        // chain links stay on the same weapon and go forward
        for key in ["next_light", "next_heavy"] {
            if let Some(n) = opt_text(m, key) {
                if let Some(nrow) = moves.find(n) {
                    if text(nrow, "weapon") != text(m, "weapon") {
                        r.err("moves", Some(mid), Some(key), format!("`{n}` belongs to another weapon"));
                    }
                    if n == mid {
                        r.err("moves", Some(mid), Some(key), "chains to itself");
                    }
                }
            }
        }
        if text(m, "kind") == "light" && m.get("chain_pos").and_then(Value::as_i64) == Some(0) {
            r.err("moves", Some(mid), Some("chain_pos"), "a light move needs a chain position >= 1");
        }
        let (open, close) = (
            m.get("buffer_open_s").and_then(Value::as_f64).unwrap_or(0.0),
            m.get("buffer_close_s").and_then(Value::as_f64).unwrap_or(0.0),
        );
        if open > close {
            r.err("moves", Some(mid), Some("buffer_open_s"), "buffer opens after it closes");
        }
        if text(m, "er_action") != "none" && close <= 0.0 && !matches!(text(m, "kind"), "block" | "parry") {
            r.err("moves", Some(mid), Some("buffer_close_s"), "an ER-driven move needs an input buffer window");
        }
    }

    // --- runic <-> weapon -----------------------------------------------------------------------
    for run in &runics.rows {
        let rid = text(run, "id");
        let Some(w) = weapons.find(text(run, "weapon")) else { continue };
        let key = if text(run, "tier") == "light" { "light_runic" } else { "heavy_runic" };
        if opt_text(w, key) != Some(rid) {
            r.err("runics", Some(rid), Some("weapon"), format!("weapon `{}` does not point back at this runic in `{key}`", text(w, "id")));
        }
    }
    for w in &weapons.rows {
        for (key, tier) in [("light_runic", "light"), ("heavy_runic", "heavy")] {
            if let Some(id) = opt_text(w, key) {
                if let Some(run) = runics.find(id) {
                    if text(run, "tier") != tier || text(run, "weapon") != text(w, "id") {
                        r.err("weapons", Some(text(w, "id")), Some(key), format!("runic `{id}` is not this weapon's {tier} runic"));
                    }
                }
            }
        }
    }

    // --- equipable weapons: unique slots, template present, first right slot is the start weapon
    let mut slots: HashMap<&str, &str> = HashMap::new();
    for w in &weapons.rows {
        let (id, slot, kind) = (text(w, "id"), text(w, "slot"), text(w, "kind"));
        if slot != "none" {
            if let Some(prev) = slots.insert(slot, id) {
                r.err("weapons", Some(id), Some("slot"), format!("slot `{slot}` already used by `{prev}`"));
            }
            if opt_text(w, "template").is_none() {
                r.err("weapons", Some(id), Some("template"), "an equipable weapon needs an ER template");
            }
            if opt_text(w, "icon").is_none() {
                r.err("weapons", Some(id), Some("icon"), "an equipable weapon needs a HUD icon");
            }
            if w.get("base_damage").and_then(Value::as_i64).unwrap_or(0) == 0 && kind != "shield" {
                r.err("weapons", Some(id), Some("base_damage"), "a damage-dealing weapon needs base_damage > 0");
            }
        } else if kind != "hands" {
            r.err("weapons", Some(id), Some("slot"), "only `hands` may have slot none");
        }
        let has_runics = opt_text(w, "light_runic").is_some() && opt_text(w, "heavy_runic").is_some();
        if matches!(kind, "axe" | "blades" | "spear") && !has_runics {
            r.err("weapons", Some(id), Some("light_runic"), "Kratos' three weapons each need a light and a heavy runic");
        }
        if matches!(kind, "axe" | "blades" | "spear") && list(w, "lights").len() < 3 {
            r.err("weapons", Some(id), Some("lights"), "needs a light chain of at least 3 moves");
        }
    }

    // --- loadout --------------------------------------------------------------------------------
    if let Some(loadout) = get("loadout") {
        for l in &loadout.rows {
            let lid = text(l, "id");
            let listed = list(l, "weapons");
            for key in ["start_weapon", "start_shield"] {
                let id = text(l, key);
                if !listed.contains(&id) {
                    r.err("loadout", Some(lid), Some(key), format!("`{id}` is not in the loadout's weapons list"));
                }
            }
            if let Some(w) = weapons.find(text(l, "start_weapon")) {
                if !text(w, "slot").starts_with("right") {
                    r.err("loadout", Some(lid), Some("start_weapon"), "start weapon must live in a right-hand slot");
                }
            }
            if let Some(w) = weapons.find(text(l, "start_shield")) {
                if text(w, "kind") != "shield" {
                    r.err("loadout", Some(lid), Some("start_shield"), "start shield is not a shield");
                }
            }
            for w in &weapons.rows {
                let id = text(w, "id");
                if text(w, "slot") != "none" && !listed.contains(&id) {
                    r.err("loadout", Some(lid), Some("weapons"), format!("post-game Kratos must start with `{id}`"));
                }
            }
        }
    }

    // --- rage ------------------------------------------------------------------------------------
    if let Some(rage) = get("rage") {
        for rg in &rage.rows {
            let id = text(rg, "id");
            if let Some(b) = get("bindings").and_then(|s| s.find(text(rg, "binding"))) {
                if list(b, "chord") != ["l3", "r3"] {
                    r.warn("rage", Some(id), Some("binding"), "rage is expected on the L3+R3 chord");
                }
            }
            if let Some(m) = moves.find(text(rg, "burst_move")) {
                if text(m, "kind") != "rage" {
                    r.err("rage", Some(id), Some("burst_move"), "burst_move must be a `rage` move");
                }
            }
        }
    }

    // --- enemy classes: ascending ceilings, exactly one boss -------------------------------------
    if let Some(ec) = get("enemy_classes") {
        let mut last = 0i64;
        for row in &ec.rows {
            let c = row.get("hp_ceiling").and_then(Value::as_i64).unwrap_or(0);
            if c <= last {
                r.err("enemy_classes", Some(text(row, "id")), Some("hp_ceiling"), "ceilings must ascend row by row");
            }
            last = c;
        }
        let bosses = ec.rows.iter().filter(|x| text(x, "health_bar") == "boss").count();
        if bosses != 1 {
            r.err("enemy_classes", None, Some("health_bar"), format!("exactly one class must use the boss bar, found {bosses}"));
        }
        if ec.rows.last().map(|x| text(x, "health_bar")) != Some("boss") {
            r.err("enemy_classes", None, Some("health_bar"), "the last (largest) class must be the boss class");
        }
    }

    // --- mimir: every event has a trigger, every voice set is used --------------------------------
    if let (Some(mt), Some(assets)) = (get("mimir_triggers"), get("assets")) {
        let events: BTreeSet<&str> = mt.rows.iter().map(|x| text(x, "event")).collect();
        if let Some(col) = mt.column("event") {
            for e in &col.values {
                if !events.contains(e.as_str()) {
                    r.err("mimir_triggers", None, Some("event"), format!("no trigger row handles event `{e}`"));
                }
            }
        }
        let used: HashSet<&str> = mt.rows.iter().map(|x| text(x, "voice")).collect();
        for a in &assets.rows {
            if text(a, "role") == "mimir" && !used.contains(text(a, "id")) {
                r.err("assets", Some(text(a, "id")), Some("role"), "Mimir voice set is not used by any trigger");
            }
        }
        for t in &mt.rows {
            if let Some(a) = assets.find(text(t, "voice")) {
                if text(a, "role") != "mimir" {
                    r.err("mimir_triggers", Some(text(t, "id")), Some("voice"), "voice must be a role=mimir asset");
                }
            }
        }
    }

    // --- HUD: every data source drawn, assets exist and have the right kind ------------------------
    if let (Some(hud), Some(assets)) = (get("hud"), get("assets")) {
        for h in &hud.rows {
            for key in ["frame", "fill", "glyph"] {
                if let Some(a) = opt_text(h, key).and_then(|id| assets.find(id)) {
                    if text(a, "kind") != "texture" {
                        r.err("hud", Some(text(h, "id")), Some(key), "HUD art must be a texture asset");
                    }
                }
            }
        }
        if let Some(col) = hud.column("bind") {
            let bound: HashSet<&str> = hud.rows.iter().map(|x| text(x, "bind")).collect();
            for b in &col.values {
                if !bound.contains(b.as_str()) {
                    r.err("hud", None, Some("bind"), format!("data source `{b}` is not drawn by any HUD element"));
                }
            }
        }
    }

    // --- bindings & assets must be used -----------------------------------------------------------
    let mut referenced: BTreeMap<(&str, String), usize> = BTreeMap::new();
    for sheet in &sheets.sheets {
        for col in &sheet.columns {
            let Some(target) = col.sheet.as_deref() else { continue };
            for row in &sheet.rows {
                let Some(v) = row.get(&col.name) else { continue };
                let ids: Vec<&str> = match v {
                    Value::String(s) => vec![s.as_str()],
                    Value::Array(a) => a.iter().filter_map(Value::as_str).collect(),
                    _ => vec![],
                };
                for id in ids {
                    let t: &str = sheets.get(target).map(|s| s.name.as_str()).unwrap_or("");
                    *referenced.entry((t, id.to_string())).or_default() += 1;
                }
            }
        }
    }
    for name in ["assets", "bindings", "moves", "er_templates", "runics"] {
        if let Some(s) = get(name) {
            for id in s.ids() {
                if !referenced.contains_key(&(s.name.as_str(), id.to_string())) {
                    r.warn(name, Some(id), None, "row is never referenced by another sheet");
                }
            }
        }
    }

    // --- hooks: acyclic, every sheet is covered -----------------------------------------------------
    if let Some(hooks) = get("hooks") {
        let needs: HashMap<&str, Vec<&str>> = hooks.rows.iter().map(|h| (text(h, "id"), list(h, "needs"))).collect();
        for id in needs.keys() {
            let mut stack = vec![*id];
            let mut seen = HashSet::new();
            while let Some(n) = stack.pop() {
                for d in needs.get(n).into_iter().flatten() {
                    if d == id {
                        r.err("hooks", Some(id), Some("needs"), "dependency cycle");
                        stack.clear();
                        break;
                    }
                    if seen.insert(*d) {
                        stack.push(d);
                    }
                }
            }
        }
        let covered: HashSet<String> = hooks.rows.iter().flat_map(|h| list(h, "covers")).map(str::to_string).collect();
        for s in &sheets.sheets {
            if s.name != "hooks" && !covered.contains(&s.name) {
                r.err("hooks", None, Some("covers"), format!("no hook covers sheet `{}`: nothing would drive it in the game", s.name));
            }
        }
        // verification tally (not errors: these are the honest "not yet proven" list)
        let mut tally: BTreeMap<String, usize> = BTreeMap::new();
        for h in &hooks.rows {
            *tally.entry(text(h, "status").to_string()).or_default() += 1;
        }
        r.info("hooks", format!("{} hooks by status: {:?}", hooks.rows.len(), tally));
    }
    for name in ["assets", "er_templates"] {
        if let Some(s) = get(name) {
            let mut tally: BTreeMap<String, usize> = BTreeMap::new();
            for row in &s.rows {
                *tally.entry(text(row, "status").to_string()).or_default() += 1;
            }
            r.info(name, format!("{} rows by status: {:?}", s.rows.len(), tally));
        }
    }
}
