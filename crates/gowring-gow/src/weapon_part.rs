//! Kratos's weapons as Elden Ring weapon parts, built on the player's PC from their Ragnarök install: the Leviathan
//! Axe (`axe00`), the Blades of Chaos (`explosive00`), the Draupnir Spear (`kratosspear00`) and the Guardian Shield
//! (`kratosshield00`), read from `r_heroa00.wad` and the level-of-detail packs beside it, written as
//! `wp_a_<id>.partsbnd.dcx` and its `_l` twin with [`crate::erpart`] (see [`build_weapon`]). The Blades are one rigid
//! blade on the whip's carrier, in the right hand: the whip's moves keep their timing but lose their reach until the
//! animation work.
//!
//! No weapon's mesh bytes have been seen away from a player's PC, so every step is a heuristic that says what it did.
//! Each model holds every variant the game swaps in (upgrade levels, handles and pommels, every shield type), and the
//! parts that make the base weapon are kept by their materials' texture names ([`PartRules`], [`choose_parts`]). The
//! model is then turned into Elden Ring's weapon space ([`orient`]): the grip at the origin, the blade or shaft along
//! -Y, an axe's edge toward -X, a shield's face on +Z, at Kratos's own scale ([`crate::kratos_part::SCALE`]). The hit
//! points go along it ([`blade_dummies`], [`shield_dummies`]), and the result must look like the weapon at all
//! ([`check_shape`]). A weapon that fails any step is not built, so its carrier keeps Elden Ring's own model; none of
//! this touches the body part. Everything needed to check the choices away from the player's PC comes back as a
//! [`WeaponProbe`].
//!
//! The parts are rigid: every vertex follows the root node, which is named after the model and owns the hit points,
//! as the game's own static weapons are built (vanilla `WP_A_0300`: node 0 `WP_A_0300`, flags 8 and 2). They reuse
//! the body's material (`C[AMSN]_e`), its 40-byte vertex and its texture pipeline.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use anyhow::Result;

use crate::erpart::{self, Dummy, Mesh, Node};
use crate::kratos::{self, MeshRecord, Vertices};
use crate::kratos_look;
use crate::kratos_part::{self, LookCounts, Naming};
use crate::lodpack::LodPacks;
use crate::wtoc::Entry;

/// The weapon model of the Blades of Chaos (`wp_a_9711`).
pub const BLADES_MODEL_ID: u16 = 9711;

/// What a weapon's model is shaped like, which decides how it is turned and where its hit points go.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ShapeKind {
    /// A handle with a head: the head's farthest point from the handle is its edge.
    Axe,
    /// A long shaft with a blade at one end.
    Spear,
    /// A disc held on the forearm.
    Shield,
}

/// Which of a model's parts make the base weapon, as data. Markers are lower case and match texture names without
/// regard to case: `require`, `prefer` and `over` anywhere in a name, `reject` at the start of one of its words
/// (`_`-separated, so `frost` is found in `axe_frost_basiclvl1_alt` but not in `bifrost00_m_gen_2d`, and `ice` in
/// `axe02_base_ice2` but not in a `device`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PartRules {
    /// The parts to use, once picked from a probe the way the body's outfit was; None until then (the rules below
    /// choose).
    pub parts: Option<&'static [u16]>,
    /// The base look's colour maps: a model holds every upgrade look, attachment and imbued state of its weapon on top
    /// of each other (the axe five whole axes, each in a frost shell, 27 pommel knobs and seven colours of each runic
    /// gem), so a part is kept only when the colour map of every material it draws names one of these (anywhere in
    /// the name). When none does (another game version's names), the rules below choose alone.
    pub only: &'static [&'static str],
    /// Parts drawn with the base look's maps that are not part of it, by their index in the model (checked against a
    /// player's probe): the Guardian Shield's four solid centre discs, which would cover its own hub.
    pub leave_out: &'static [u16],
    /// Keep only the parts whose materials name one of these (none: no such need).
    pub require: &'static [&'static str],
    /// Variant markers: a material whose every colour map carries one draws a variant (a part drawn mostly so is
    /// another variant and is dropped, see [`choose_parts`]), and textures carrying one are passed over when a
    /// material's maps are chosen.
    pub reject: &'static [&'static str],
    /// When some part names one of `prefer`, the parts that name one of `over` and none of `prefer` are dropped.
    pub prefer: &'static [&'static str],
    pub over: &'static [&'static str],
}

/// A weapon this version builds.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WeaponSpec {
    /// Its id in `design/weapons.json`.
    pub weapon: &'static str,
    /// Its name, for the logs.
    pub name: &'static str,
    /// Its Ragnarök model (`MESH_<model>_0` and the rest in `r_heroa00.wad`).
    pub model: &'static str,
    /// The Elden Ring weapon model it becomes (`wp_a_<er_model>.partsbnd.dcx`): unused by the game, whose own end at
    /// 7520.
    pub er_model: u16,
    pub shape: ShapeKind,
    pub rules: PartRules,
    /// A pair, as the Blades: the part file holds the weapon twice, as model 0 and model 1 (`WP_A_<id>_1.flver`), and
    /// gowring.dll places model 1 in the other hand.
    pub paired: bool,
}

/// The weapons this version builds, each into its own Elden Ring model id.
pub const WEAPONS: &[WeaponSpec] = &[
    WeaponSpec {
        weapon: "leviathan_axe",
        name: "Leviathan Axe",
        model: "axe00",
        er_model: 9710,
        shape: ShapeKind::Axe,
        paired: false,
        // the first upgrade look's head and body (v1; the frost shell over each look's head is `blade02`), the plain
        // wrapped pommel (`axe02_down_01`), and the blue of each runic gem under the head; not the gold pommels, the
        // anniversary look, or the frost, ice, glow and runic layers
        rules: PartRules {
            parts: None,
            only: &["kratosaxe_head_v1_", "kratosaxe_body_v1_", "axe02_down_01_gen", "blue_07_gen", "blue_14_gen"],
            leave_out: &[],
            require: &[],
            reject: &["gold", "anniver", "frost", "ice", "glow", "runic"],
            prefer: &["axe02", "axe00"],
            over: &["axe04"],
        },
    },
    WeaponSpec {
        weapon: "blades_of_chaos",
        name: "Blades of Chaos",
        model: "explosive00",
        er_model: BLADES_MODEL_ID,
        // a blade with its handle: the flat of the blade across X, as a spear's
        shape: ShapeKind::Spear,
        // one blade per hand: Ragnarök draws the one model twice
        paired: true,
        // the model holds five blade looks (`bladea` to `bladee`), as many handles and eight pommels, the damaged
        // Blades and the anniversary look; the base look is the first of each (not yet checked against a probe of
        // this model, which this build writes: when none matches, the other rules choose)
        rules: PartRules {
            parts: None,
            only: &["kratosblades00_bladea_", "kratosblades00_handlea_", "kratosblades00_pommela_"],
            leave_out: &[],
            require: &[],
            reject: &["anniver", "damaged", "frost", "ice", "glow", "runic"],
            prefer: &[],
            over: &[],
        },
    },
    WeaponSpec {
        weapon: "draupnir_spear",
        name: "Draupnir Spear",
        model: "kratosspear00",
        er_model: 9712,
        shape: ShapeKind::Spear,
        paired: false,
        // the base look (`set_body` and its head, `head_b`), the first butt (`handle_a`) and the runic gems; not the
        // other two looks, the other butts, the charged state (`tempcharge`) or the cloth flag (`fabric02`), which
        // would hang stiff without the cloth physics Elden Ring gives its own
        rules: PartRules {
            parts: None,
            only: &["kratosspear00_set_body_gen", "kratosspear00_set_head_b_", "kratosspear00_set_handle_a_", "red_09_gen", "snow_gen"],
            leave_out: &[],
            require: &[],
            reject: &["thrown", "anniver"],
            prefer: &[],
            over: &[],
        },
    },
    WeaponSpec {
        weapon: "guardian_shield",
        name: "Guardian Shield",
        model: "kratosshield00",
        er_model: 9713,
        shape: ShapeKind::Shield,
        paired: false,
        // the one model holds the guardian, blitz, megaBuster, parry, siege and spartan shields and their
        // anniversary versions; the Guardian Shield is the pieces drawn with `kratosshield00_base2` (125 pieces:
        // hub, spine and clasps, rim, and a ring of 32 wedge plates, each on its own joint), less four solid discs
        // (247-250) as wide as its hub that would cover it
        rules: PartRules { parts: None, only: &["kratosshield00_base2"], leave_out: &[247, 248, 249, 250], require: &["guardian"], reject: &["anniver"], prefer: &[], over: &[] },
    },
];

/// The spec of the weapon with sheet id `weapon`.
pub fn weapon_spec(weapon: &str) -> Option<&'static WeaponSpec> {
    WEAPONS.iter().find(|w| w.weapon == weapon)
}

/// The name a weapon model goes by inside its files (`WP_A_9710`), as the game's own (`WP_A_0300`).
pub fn stem(er_model: u16) -> String {
    format!("WP_A_{er_model:04}")
}

/// The two files the game loads for a weapon model, which hold the same bytes: the model and its low-detail twin
/// (`wp_a_9710.partsbnd.dcx`, `wp_a_9710_l.partsbnd.dcx`). Weapons have no male and female files.
pub fn file_names(er_model: u16) -> [String; 2] {
    [format!("wp_a_{er_model:04}.partsbnd.dcx"), format!("wp_a_{er_model:04}_l.partsbnd.dcx")]
}

/// Whether `name` holds `marker` anywhere, without regard to case.
pub fn names_marker(name: &str, marker: &str) -> bool {
    name.to_ascii_lowercase().contains(&marker.to_ascii_lowercase())
}

/// Whether one of the `_`-separated words of `name` starts with `marker`, without regard to case.
pub fn starts_a_word(name: &str, marker: &str) -> bool {
    let marker = marker.to_ascii_lowercase();
    name.to_ascii_lowercase().split(['_', '-', '.', ' ']).any(|w| !marker.is_empty() && w.starts_with(&marker))
}

/// The first of `markers` that starts a word of `name`.
fn variant_marker<'a>(name: &str, markers: &[&'a str]) -> Option<&'a str> {
    markers.iter().copied().find(|m| starts_a_word(name, m))
}

/// Whether a material is drawn, and why not.
#[derive(Clone, Debug, PartialEq)]
pub enum MaterialUse {
    /// It has a colour map that is not a variant's.
    Draw,
    /// It names no textures: a shadow or collision shape, as Kratos's stand-ins.
    NoTextures,
    /// It names textures but no colour map among them (an effect layer such as a frost glow).
    NoColour,
    /// Its only colour maps are a variant's: `texture` carries `marker`.
    Variant { marker: String, texture: String },
}

/// What a material is for the weapon, from its references (`refs`, the names in its id-60 list) and the variant
/// markers.
pub fn material_use(refs: &[String], reject: &[&str]) -> MaterialUse {
    let tx: Vec<String> = refs.iter().filter(|r| r.starts_with("TX_")).cloned().collect();
    if tx.is_empty() {
        return MaterialUse::NoTextures;
    }
    let clean: Vec<String> = tx.iter().filter(|t| variant_marker(t, reject).is_none()).cloned().collect();
    if kratos_look::colour_map(&clean).is_some() {
        return MaterialUse::Draw;
    }
    match kratos_look::colour_map(&tx) {
        Some(t) => MaterialUse::Variant { marker: variant_marker(t, reject).unwrap_or_default().to_string(), texture: t.clone() },
        None => MaterialUse::NoColour,
    }
}

/// One part of a model as [`choose_parts`] sees it.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PartView {
    pub index: u16,
    /// Its most detailed level's draws: (record, material, triangles).
    pub draws: Vec<(usize, u16, u32)>,
    /// The bounds of its decoded draws in Ragnarök's space (None when none decoded).
    pub bounds: Option<([f32; 3], [f32; 3])>,
}

/// Which parts make the weapon: the parts kept, the draws to build (record, material), the parts dropped and the
/// draws of kept parts left out, each with the reason, and a line saying what was done.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Choice {
    pub kept: Vec<u16>,
    pub draws: Vec<(usize, u16)>,
    pub dropped: Vec<(u16, String)>,
    pub skipped_draws: Vec<(usize, u16, String)>,
    pub note: String,
}

/// Is a part's box disc-shaped: two sides alike, the third under 40 % of the largest, at least 15 cm across.
fn disc_shaped(bounds: &([f32; 3], [f32; 3])) -> bool {
    let mut e: Vec<f32> = (0..3).map(|k| bounds.1[k] - bounds.0[k]).collect();
    e.sort_by(|a, b| b.total_cmp(a));
    e[0] >= 0.15 && e[1] >= 0.6 * e[0] && e[2] <= 0.4 * e[0]
}

fn centre_of(b: &([f32; 3], [f32; 3])) -> [f32; 3] {
    std::array::from_fn(|k| (b.0[k] + b.1[k]) / 2.0)
}

fn length3(v: [f32; 3]) -> f32 {
    (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt()
}

/// One shield of several: the disc-shaped part whose centre is nearest the origin (within a centimetre counts as
/// equal, then the lower part number: the base versions come first in the game's own lists), and the parts that are
/// not discs and sit on it (within its outline and 10 cm of its face: its boss, rim pieces and straps). The other
/// discs are the other shields.
fn one_disc(cands: &[&PartView]) -> (Vec<u16>, Vec<(u16, String)>, String) {
    let discs: Vec<&&PartView> = cands.iter().filter(|p| p.bounds.as_ref().is_some_and(disc_shaped)).collect();
    let Some(base) = discs.iter().min_by(|a, b| {
        let d = |p: &PartView| p.bounds.as_ref().map(|b| length3(centre_of(b))).unwrap_or(f32::MAX);
        let (da, db) = (d(a), d(b));
        if (da - db).abs() <= 0.01 {
            a.index.cmp(&b.index)
        } else {
            da.total_cmp(&db)
        }
    }) else {
        return (cands.iter().map(|p| p.index).collect(), Vec::new(), "no disc-shaped part: all kept".into());
    };
    let Some(bb) = base.bounds else { return (cands.iter().map(|p| p.index).collect(), Vec::new(), "no disc-shaped part: all kept".into()) };
    // within 2 cm of its face's outline, within 10 cm in front of or behind it (a boss, a grip)
    let thin = (0..3).min_by(|a, b| (bb.1[*a] - bb.0[*a]).total_cmp(&(bb.1[*b] - bb.0[*b]))).unwrap_or(2);
    let room = |k: usize| if k == thin { 0.1 } else { 0.02 };
    let inside = |c: [f32; 3]| (0..3).all(|k| c[k] >= bb.0[k] - room(k) && c[k] <= bb.1[k] + room(k));
    let (mut kept, mut dropped) = (vec![base.index], Vec::new());
    for p in cands.iter().filter(|p| p.index != base.index) {
        let Some(b) = p.bounds.as_ref() else { continue };
        if disc_shaped(b) {
            dropped.push((p.index, format!("another disc (an alternative shield; part {} was taken)", base.index)));
        } else if inside(centre_of(b)) {
            kept.push(p.index);
        } else {
            dropped.push((p.index, format!("outside the chosen disc (part {})", base.index)));
        }
    }
    kept.sort_unstable();
    let note = format!("{} disc-shaped part(s) {:?}: kept part {} (centre {:.3} m from the origin) and {} piece(s) inside it", discs.len(), discs.iter().map(|p| p.index).collect::<Vec<_>>(), base.index, length3(centre_of(&bb)), kept.len() - 1);
    (kept, dropped, note)
}

/// Choose the parts that make the weapon from `parts` (each model holds every variant), by `rules` and each
/// material's texture names (`refs`: material index to the names in its reference list). In order: a part list in
/// the rules is taken as it is; parts with no draws or none that decoded are dropped; a part is another variant when
/// more of its triangles draw a variant's colour map ([`material_use`]) than its own (a gold pommel with its gems),
/// else only those draws are left out (a frost layer over the head), as are draws whose material has no textures or
/// no colour map, and a part left with nothing to draw is dropped; then only parts naming a `require` marker are kept
/// (for a shield with none naming one, or with every part naming one because the marker sits in a shared material,
/// the one disc nearest the origin, as `one_disc` picks it); then `prefer` wins over `over`. Names are those of the
/// textures of the materials a part draws. An empty `kept` means nothing is left and the weapon is not built.
pub fn choose_parts(rules: &PartRules, shape: ShapeKind, parts: &[PartView], refs: &BTreeMap<u16, Vec<String>>) -> Choice {
    let mut c = Choice::default();
    let mut notes: Vec<String> = Vec::new();
    let uses: BTreeMap<u16, MaterialUse> = parts.iter().flat_map(|p| p.draws.iter().map(|d| d.1)).collect::<BTreeSet<u16>>().into_iter().map(|m| (m, material_use(refs.get(&m).map(Vec::as_slice).unwrap_or(&[]), rules.reject))).collect();
    let drawn = |m: u16| uses.get(&m) == Some(&MaterialUse::Draw);
    let names = |p: &PartView| -> Vec<String> {
        let mats: BTreeSet<u16> = p.draws.iter().map(|d| d.1).filter(|m| drawn(*m)).collect();
        mats.iter().flat_map(|m| refs.get(m).into_iter().flatten()).filter(|r| r.starts_with("TX_")).cloned().collect()
    };
    let mut cands: Vec<&PartView> = Vec::new();
    for p in parts {
        if let Some(list) = rules.parts {
            if !list.contains(&p.index) {
                c.dropped.push((p.index, "not in the weapon's part list".into()));
                continue;
            }
        }
        if p.draws.is_empty() {
            c.dropped.push((p.index, "no draws at its most detailed level".into()));
            continue;
        }
        if p.bounds.is_none() {
            c.dropped.push((p.index, "none of its draws decoded".into()));
            continue;
        }
        if rules.parts.is_none() {
            let own: u64 = p.draws.iter().filter(|d| drawn(d.1)).map(|d| d.2 as u64).sum();
            let variant: u64 = p.draws.iter().filter(|d| matches!(uses.get(&d.1), Some(MaterialUse::Variant { .. }))).map(|d| d.2 as u64).sum();
            let first = p.draws.iter().find_map(|d| match uses.get(&d.1) {
                Some(MaterialUse::Variant { marker, texture }) => Some(format!("material {} draws {texture} ({marker})", d.1)),
                _ => None,
            });
            if let Some(why) = first.filter(|_| variant > own) {
                c.dropped.push((p.index, format!("a variant: {why}; {variant} of its {} triangles", own + p.draws.iter().filter(|d| !drawn(d.1)).map(|d| d.2 as u64).sum::<u64>())));
                continue;
            }
        }
        if !p.draws.iter().any(|d| drawn(d.1)) {
            let why: BTreeSet<String> = p.draws.iter().map(|d| format!("material {}: {}", d.1, use_reason(uses.get(&d.1)))).collect();
            c.dropped.push((p.index, format!("nothing it could draw ({})", why.into_iter().collect::<Vec<_>>().join(", "))));
            continue;
        }
        cands.push(p);
    }
    let colour = |m: u16| refs.get(&m).and_then(|r| crate::kratos_look::colour_map(r)).map(|t| t.to_ascii_lowercase()).unwrap_or_default();
    let mut base_look = false;
    if rules.parts.is_none() && !rules.only.is_empty() {
        let base = |p: &PartView| p.draws.iter().filter(|d| drawn(d.1)).all(|d| rules.only.iter().any(|o| colour(d.1).contains(&o.to_ascii_lowercase())));
        let (keep, drop): (Vec<&PartView>, Vec<&PartView>) = cands.iter().partition(|p| base(p) && !rules.leave_out.contains(&p.index));
        if keep.is_empty() {
            notes.push(format!("no part draws only the base look's maps {:?}: the other rules choose", rules.only));
        } else {
            for p in drop {
                c.dropped.push((p.index, "not the base look (another upgrade look, attachment or state)".into()));
            }
            // the same piece drawn again with another material carrying the same maps (another state of a gem)
            let mut seen: Vec<(&PartView, Vec<String>)> = Vec::new();
            cands.clear();
            for p in keep {
                let maps: Vec<String> = p.draws.iter().filter(|d| drawn(d.1)).map(|d| colour(d.1)).collect();
                let same = seen.iter().find(|(q, m)| {
                    *m == maps
                        && q.draws.iter().map(|d| d.1).filter(|m| drawn(*m)).ne(p.draws.iter().map(|d| d.1).filter(|m| drawn(*m)))
                        && q.bounds.zip(p.bounds).is_some_and(|(a, b)| (0..3).all(|k| (a.0[k] - b.0[k]).abs() < 1e-3 && (a.1[k] - b.1[k]).abs() < 1e-3))
                });
                match same {
                    Some((q, _)) => c.dropped.push((p.index, format!("the same piece as part {} (another state of it)", q.index))),
                    None => {
                        seen.push((p, maps));
                        cands.push(p);
                    }
                }
            }
            notes.push(format!("the base look {:?}: {} part(s)", rules.only, cands.len()));
            base_look = true;
        }
    }
    if rules.parts.is_none() && !base_look && !rules.require.is_empty() && !cands.is_empty() {
        let named = |p: &PartView| names(p).iter().any(|t| rules.require.iter().any(|m| names_marker(t, m)));
        let matching: Vec<&PartView> = cands.iter().copied().filter(|p| named(p)).collect();
        let discs = cands.iter().filter(|p| p.bounds.as_ref().is_some_and(disc_shaped)).count();
        if matching.is_empty() || (shape == ShapeKind::Shield && matching.len() == cands.len() && discs > 1) {
            if matching.is_empty() {
                notes.push(format!("no part names {:?}", rules.require));
            } else {
                notes.push(format!("every part names {:?} (a shared material)", rules.require));
            }
            if shape == ShapeKind::Shield {
                let (kept, dropped, note) = one_disc(&cands);
                notes.push(note);
                c.dropped.extend(dropped);
                cands.retain(|p| kept.contains(&p.index));
            }
        } else {
            for p in cands.iter().filter(|p| !named(p)) {
                c.dropped.push((p.index, format!("names none of {:?}", rules.require)));
            }
            cands = matching;
            // the guardian parts can still hold several discs (damaged and broken ones): one is drawn
            if shape == ShapeKind::Shield && cands.iter().filter(|p| p.bounds.as_ref().is_some_and(disc_shaped)).count() > 1 {
                let (kept, dropped, note) = one_disc(&cands);
                notes.push(note);
                c.dropped.extend(dropped);
                cands.retain(|p| kept.contains(&p.index));
            }
        }
    }
    if rules.parts.is_none() && !base_look && !rules.prefer.is_empty() {
        let any = |p: &PartView, ms: &[&str]| names(p).iter().any(|t| ms.iter().any(|m| names_marker(t, m)));
        if cands.iter().any(|p| any(p, rules.prefer)) {
            let (drop, keep): (Vec<&PartView>, Vec<&PartView>) = cands.iter().partition(|p| any(p, rules.over) && !any(p, rules.prefer));
            for p in drop {
                c.dropped.push((p.index, format!("names {:?} where others name {:?}", rules.over, rules.prefer)));
            }
            cands = keep;
        }
    }
    for p in &cands {
        c.kept.push(p.index);
        for d in &p.draws {
            match uses.get(&d.1) {
                Some(MaterialUse::Draw) => c.draws.push((d.0, d.1)),
                u => c.skipped_draws.push((d.0, d.1, use_reason(u))),
            }
        }
    }
    c.dropped.sort();
    notes.insert(0, format!("{} of {} parts kept {:?}, {} draws", c.kept.len(), parts.len(), c.kept, c.draws.len()));
    if !c.skipped_draws.is_empty() {
        notes.push(format!("{} draws of kept parts left out ({})", c.skipped_draws.len(), c.skipped_draws.iter().map(|s| format!("record {} material {}: {}", s.0, s.1, s.2)).collect::<Vec<_>>().join("; ")));
    }
    notes.push(format!("dropped: {}", if c.dropped.is_empty() { "none".into() } else { c.dropped.iter().map(|(p, why)| format!("{p} ({why})")).collect::<Vec<_>>().join("; ") }));
    c.note = notes.join("; ");
    c
}

fn use_reason(u: Option<&MaterialUse>) -> String {
    match u {
        Some(MaterialUse::Draw) => "drawn".into(),
        Some(MaterialUse::NoTextures) => "no textures (a shadow or collision shape)".into(),
        Some(MaterialUse::NoColour) => "no colour map (an effect layer)".into(),
        Some(MaterialUse::Variant { marker, texture }) => format!("a variant's colour map {texture} ({marker})"),
        None => "no material".into(),
    }
}

/// Positions and triangles: the chosen draws together, in Ragnarök's space or (after [`Pose::place`]) in Elden
/// Ring's.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Geometry {
    pub positions: Vec<[f32; 3]>,
    pub triangles: Vec<[u32; 3]>,
}

impl Geometry {
    /// Append a draw's positions and triangles.
    pub fn add(&mut self, positions: &[[f32; 3]], triangles: &[[u16; 3]]) {
        let base = self.positions.len() as u32;
        self.positions.extend_from_slice(positions);
        self.triangles.extend(triangles.iter().map(|t| t.map(|i| i as u32 + base)));
    }

    /// The box around every position (zeros when there is none).
    pub fn bounds(&self) -> ([f32; 3], [f32; 3]) {
        if self.positions.is_empty() {
            return ([0.0; 3], [0.0; 3]);
        }
        let mut lo = [f32::MAX; 3];
        let mut hi = [f32::MIN; 3];
        for p in &self.positions {
            for k in 0..3 {
                lo[k] = lo[k].min(p[k]);
                hi[k] = hi[k].max(p[k]);
            }
        }
        (lo, hi)
    }

    /// Each triangle's centre and area.
    fn surface(&self) -> Vec<([f64; 3], f64)> {
        let p = |i: u32| self.positions.get(i as usize).map(|p| p.map(|x| x as f64));
        self.triangles
            .iter()
            .filter_map(|t| {
                let (a, b, c) = (p(t[0])?, p(t[1])?, p(t[2])?);
                let e1 = sub(b, a);
                let e2 = sub(c, a);
                let area = 0.5 * norm(cross(e1, e2));
                Some((std::array::from_fn(|k| (a[k] + b[k] + c[k]) / 3.0), area))
            })
            .collect()
    }

    /// Everything placed by `pose`.
    pub fn placed(&self, pose: &Pose) -> Geometry {
        Geometry { positions: self.positions.iter().map(|p| pose.place(*p)).collect(), triangles: self.triangles.clone() }
    }
}

type V3 = [f64; 3];

fn sub(a: V3, b: V3) -> V3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
fn dot(a: V3, b: V3) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
fn cross(a: V3, b: V3) -> V3 {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}
fn norm(a: V3) -> f64 {
    dot(a, a).sqrt()
}
fn scale(a: V3, s: f64) -> V3 {
    [a[0] * s, a[1] * s, a[2] * s]
}
fn unit(a: V3) -> V3 {
    let n = norm(a);
    if n > 0.0 {
        scale(a, 1.0 / n)
    } else {
        [0.0; 3]
    }
}
/// `a` without its part along the unit vector `u`, made unit length.
fn across(a: V3, u: V3) -> V3 {
    unit(sub(a, scale(u, dot(a, u))))
}

/// The eigenvalues and unit eigenvectors of a symmetric 3x3 matrix (Jacobi's rotations), largest value first.
fn eigen_sym3(m: [[f64; 3]; 3]) -> [(f64, V3); 3] {
    let mut a = m;
    let mut v = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];
    for _ in 0..64 {
        if a[0][1].abs() + a[0][2].abs() + a[1][2].abs() < 1e-18 {
            break;
        }
        for (p, q) in [(0, 1), (0, 2), (1, 2)] {
            if a[p][q].abs() < 1e-30 {
                continue;
            }
            let theta = (a[q][q] - a[p][p]) / (2.0 * a[p][q]);
            let t = theta.signum() / (theta.abs() + (theta * theta + 1.0).sqrt());
            let c = 1.0 / (t * t + 1.0).sqrt();
            let s = t * c;
            for row in a.iter_mut() {
                let (kp, kq) = (row[p], row[q]);
                row[p] = c * kp - s * kq;
                row[q] = s * kp + c * kq;
            }
            for k in 0..3 {
                let (pk, qk) = (a[p][k], a[q][k]);
                a[p][k] = c * pk - s * qk;
                a[q][k] = s * pk + c * qk;
            }
            for row in v.iter_mut() {
                let (kp, kq) = (row[p], row[q]);
                row[p] = c * kp - s * kq;
                row[q] = s * kp + c * kq;
            }
        }
    }
    let mut out: [(f64, V3); 3] = std::array::from_fn(|i| (a[i][i], unit([v[0][i], v[1][i], v[2][i]])));
    out.sort_by(|x, y| y.0.total_cmp(&x.0));
    out
}

/// The principal axes of the surface of the triangles `keep` accepts (by their centre), with their variances,
/// largest first. Each triangle counts with its whole area spread over it (its exact second moment, so neither a
/// finely cut head nor a handle made of a few long triangles is miscounted).
fn surface_axes(g: &Geometry, keep: impl Fn(V3) -> bool) -> Option<[(f64, V3); 3]> {
    let p = |i: u32| g.positions.get(i as usize).map(|p| p.map(|x| x as f64));
    let (mut total, mut first, mut second) = (0.0f64, [0.0f64; 3], [[0.0f64; 3]; 3]);
    for t in &g.triangles {
        let (Some(a), Some(b), Some(c)) = (p(t[0]), p(t[1]), p(t[2])) else { continue };
        let s: V3 = std::array::from_fn(|k| a[k] + b[k] + c[k]);
        if !keep(scale(s, 1.0 / 3.0)) {
            continue;
        }
        let area = 0.5 * norm(cross(sub(b, a), sub(c, a)));
        total += area;
        for i in 0..3 {
            first[i] += area * s[i] / 3.0;
            for j in 0..3 {
                second[i][j] += area / 12.0 * (a[i] * a[j] + b[i] * b[j] + c[i] * c[j] + s[i] * s[j]);
            }
        }
    }
    if total.is_nan() || total <= 1e-12 {
        return None;
    }
    let mean = scale(first, 1.0 / total);
    let cov: [[f64; 3]; 3] = std::array::from_fn(|i| std::array::from_fn(|j| second[i][j] / total - mean[i] * mean[j]));
    Some(eigen_sym3(cov))
}

/// The surface's principal axes, each with the extent of the positions along it: (axis, lowest, highest), from the
/// origin.
fn principal_axes(g: &Geometry) -> Option<[(V3, f64, f64); 3]> {
    let axes = surface_axes(g, |_| true)?;
    Some(axes.map(|(_, ax)| {
        let (lo, hi) = extent(g, ax);
        (ax, lo, hi)
    }))
}

/// The middle of the model's cross-section where the plane `dot(p, u) = t` cuts it: the middle of each triangle's cut,
/// weighted by the cut's length (where the plane only touches it, the touching points' mean). None when the plane
/// misses the model.
fn section_centre(g: &Geometry, u: V3, t: f64) -> Option<V3> {
    let p = |i: u32| g.positions.get(i as usize).map(|p| p.map(|x| x as f64));
    let (mut sum, mut weight, mut touch, mut touches) = ([0.0f64; 3], 0.0f64, [0.0f64; 3], 0usize);
    for tri in &g.triangles {
        let (Some(a), Some(b), Some(c)) = (p(tri[0]), p(tri[1]), p(tri[2])) else { continue };
        let mut cut: Vec<V3> = Vec::with_capacity(3);
        for (x, y) in [(a, b), (b, c), (c, a)] {
            let (tx, ty) = (dot(x, u) - t, dot(y, u) - t);
            if tx == 0.0 {
                cut.push(x);
            } else if tx * ty < 0.0 {
                let f = tx / (tx - ty);
                cut.push(std::array::from_fn(|k| x[k] + f * (y[k] - x[k])));
            }
        }
        if cut.len() >= 2 {
            let len = norm(sub(cut[1], cut[0]));
            let mid: V3 = std::array::from_fn(|k| (cut[0][k] + cut[1][k]) / 2.0);
            if len > 0.0 {
                sum = std::array::from_fn(|k| sum[k] + mid[k] * len);
                weight += len;
            } else {
                touch = std::array::from_fn(|k| touch[k] + mid[k]);
                touches += 1;
            }
        } else if cut.len() == 1 {
            touch = std::array::from_fn(|k| touch[k] + cut[0][k]);
            touches += 1;
        }
    }
    if weight > 0.0 {
        Some(scale(sum, 1.0 / weight))
    } else if touches > 0 {
        Some(scale(touch, 1.0 / touches as f64))
    } else {
        None
    }
}

/// How wide the model is where the plane square to `u` at `t` cuts it: the larger side of the cut's box across `u`
/// (None when the plane misses it).
fn section_span(g: &Geometry, u: V3, t: f64) -> Option<f64> {
    let (a, b) = (across(if u[0].abs() < 0.9 { [1.0, 0.0, 0.0] } else { [0.0, 1.0, 0.0] }, u), cross(u, across(if u[0].abs() < 0.9 { [1.0, 0.0, 0.0] } else { [0.0, 1.0, 0.0] }, u)));
    let p = |i: u32| g.positions.get(i as usize).map(|p| p.map(|x| x as f64));
    let mut box2 = [f64::MAX, f64::MIN, f64::MAX, f64::MIN];
    for tri in &g.triangles {
        let (Some(x0), Some(x1), Some(x2)) = (p(tri[0]), p(tri[1]), p(tri[2])) else { continue };
        for (x, y) in [(x0, x1), (x1, x2), (x2, x0)] {
            let (tx, ty) = (dot(x, u) - t, dot(y, u) - t);
            if tx * ty <= 0.0 && tx != ty {
                let f = tx / (tx - ty);
                let c: V3 = std::array::from_fn(|k| x[k] + f * (y[k] - x[k]));
                let (pa, pb) = (dot(c, a), dot(c, b));
                box2 = [box2[0].min(pa), box2[1].max(pa), box2[2].min(pb), box2[3].max(pb)];
            }
        }
    }
    (box2[0] <= box2[1]).then(|| (box2[1] - box2[0]).max(box2[3] - box2[2]))
}

/// How wide a handle Elden Ring's closed hand holds: its fist closes to its own weapons' grips, so a thicker handle
/// (the Leviathan Axe's is 5.8 cm where the hand goes) has the fingers sink into it.
pub const GRIP_WIDTH: f32 = 0.034;

/// A position in Elden Ring's weapon space with the handle slimmed where the hand covers it: within 6 cm of the grip
/// along the length, fading out by 10 cm, points within 6 cm of the handle's line move toward it by `k` (the grip's
/// width over [`GRIP_WIDTH`], at most 1).
pub fn thin_grip(p: [f32; 3], k: f32) -> [f32; 3] {
    let along = p[1].abs();
    if k >= 1.0 || along >= 0.10 || p[0].hypot(p[2]) > 0.06 {
        return p;
    }
    let w = if along <= 0.06 { 1.0 } else { let t = (0.10 - along) / 0.04; t * t * (3.0 - 2.0 * t) };
    let f = 1.0 - (1.0 - k) * w;
    [p[0] * f, p[1], p[2] * f]
}

/// The lowest and highest of the positions along `ax`.
fn extent(g: &Geometry, ax: V3) -> (f64, f64) {
    g.positions.iter().map(|p| dot(p.map(|x| x as f64), ax)).fold((f64::MAX, f64::MIN), |(lo, hi), t| (lo.min(t), hi.max(t)))
}

/// Within 10 degrees of a coordinate axis, that axis (artists model weapons along one, and the analysis wobbles).
fn snap(a: V3) -> (V3, bool) {
    let k = (0..3).max_by(|i, j| a[*i].abs().total_cmp(&a[*j].abs())).unwrap_or(0);
    if a[k].abs() >= 10f64.to_radians().cos() {
        let mut s = [0.0; 3];
        s[k] = a[k].signum();
        (s, true)
    } else {
        (a, false)
    }
}

/// The coordinate axis nearest `a`, when `a` is within 25 degrees of it and the model's box is also clearly longest
/// (`longest`) or thinnest along it (by half again against the middle side). Weapons are modelled along an axis, and
/// a heavy head (or boss) tilts the surface's principal axes off it by more than [`snap`] allows.
fn box_axis(g: &Geometry, a: V3, longest: bool) -> Option<V3> {
    let k = (0..3).max_by(|i, j| a[*i].abs().total_cmp(&a[*j].abs()))?;
    if a[k].abs() < 25f64.to_radians().cos() {
        return None;
    }
    let (lo, hi) = g.bounds();
    let e: [f64; 3] = std::array::from_fn(|i| (hi[i] - lo[i]) as f64);
    let mut order = [0usize, 1, 2];
    order.sort_by(|i, j| e[*j].total_cmp(&e[*i]));
    let ok = if longest { order[0] == k && e[k] >= 1.5 * e[order[1]] } else { order[2] == k && e[k] <= e[order[1]] / 1.5 };
    ok.then(|| {
        let mut s = [0.0; 3];
        s[k] = a[k].signum();
        s
    })
}

/// `a` with its largest component positive, so a direction the analysis gives either way round comes out the same.
fn positive(a: V3) -> V3 {
    let k = (0..3).max_by(|i, j| a[*i].abs().total_cmp(&a[*j].abs())).unwrap_or(0);
    if a[k] < 0.0 {
        scale(a, -1.0)
    } else {
        a
    }
}

/// `a` snapped to a coordinate axis when that keeps it across the unit vector `u`, else only made square to `u`.
fn snap_across(a: V3, u: V3) -> (V3, bool) {
    let (s, snapped) = snap(across(a, u));
    if snapped && dot(s, u).abs() < 1e-9 {
        (s, true)
    } else {
        (across(a, u), false)
    }
}

/// How the chosen draws are moved and turned into Elden Ring's weapon space: `scale` x `rotation` x (p - `grip`),
/// with the rotation's rows the Ragnarök directions that become Elden Ring's +X, +Y and +Z.
#[derive(Clone, Debug, PartialEq)]
pub struct Pose {
    pub rotation: [[f32; 3]; 3],
    /// The Ragnarök point that becomes the origin (the grip, or a shield's centre).
    pub grip: [f32; 3],
    pub scale: f32,
    /// How each choice was made.
    pub note: String,
}

impl Pose {
    pub fn place(&self, p: [f32; 3]) -> [f32; 3] {
        let d = [p[0] - self.grip[0], p[1] - self.grip[1], p[2] - self.grip[2]];
        self.rotation.map(|r| self.scale * (r[0] * d[0] + r[1] * d[1] + r[2] * d[2]))
    }

    pub fn turn(&self, n: [f32; 3]) -> [f32; 3] {
        self.rotation.map(|r| r[0] * n[0] + r[1] * n[1] + r[2] * n[2])
    }

    /// Where Ragnarök's +X, +Y and +Z go, for the logs.
    pub fn axes_line(&self) -> String {
        let col = |k: usize| format!("({:.2}, {:.2}, {:.2})", self.rotation[0][k], self.rotation[1][k], self.rotation[2][k]);
        format!("Ragnarök +X -> {}, +Y -> {}, +Z -> {}", col(0), col(1), col(2))
    }
}

/// How near an end of its length the origin must be to count as the grip: within a third of it.
pub const NEAR_END: f64 = 1.0 / 3.0;
/// Where the grip goes when the origin is not one: this share of the length in from the end opposite the head, or
/// the thinnest section within `GRIP_RANGE` of it.
pub const GRIP_IN: f64 = 0.15;
pub const GRIP_RANGE: (f64, f64) = (0.08, 0.25);

fn v3s(v: V3) -> String {
    format!("({:.3}, {:.3}, {:.3})", v[0], v[1], v[2])
}

/// How to turn the chosen draws (`g`, Ragnarök's space) into Elden Ring's weapon space, by their shape:
///  * axe and spear: the longest of the surface's principal axes is the length (when that is not a coordinate axis,
///    the axis of the handle alone, the 60 % of the length away from the end with more surface, if that part is a
///    rod: a heavy head tilts the whole surface's axis toward it). When the origin lies within the
///    model's bounds and within [`NEAR_END`] of one end, that end is the grip and the origin stays; otherwise the grip
///    is [`GRIP_IN`] of the length in from the end opposite the head (the end with less surface) at the middle of
///    the model's cross-section there. From the grip toward the far end becomes -Y. An axe's edge, the direction
///    across the handle from it to the head's farthest point, becomes -X; a spear's blade width (the wider of the
///    other axes) becomes X.
///  * shield: the smallest principal axis is the face's normal and becomes +Z, pointing from the attach point (the
///    origin, behind the disc on the arm) to the disc; the larger of the others becomes +Y; the disc's centre becomes
///    the origin.
///
/// Directions within 10 degrees of a coordinate axis are taken as that axis, and so is one within 25 degrees when the
/// model's box is also clearly longest (for a shield's face, thinnest) along it. Fails when there is no surface to go
/// by.
pub fn orient(shape: ShapeKind, g: &Geometry, scale_by: f32) -> Result<Pose, String> {
    let Some(axes) = principal_axes(g) else { return Err("no surface to orient (empty or flat)".into()) };
    let rows: [V3; 3];
    let grip: V3;
    let mut note: Vec<String> = Vec::new();
    match shape {
        ShapeKind::Axe | ShapeKind::Spear => {
            let long = (0..3).max_by(|a, b| (axes[*a].2 - axes[*a].1).total_cmp(&(axes[*b].2 - axes[*b].1))).unwrap_or(0);
            let (mut u0, mut snapped) = match box_axis(g, axes[long].0, true) {
                Some(c) => (c, true),
                None => snap(axes[long].0),
            };
            if !snapped {
                // a heavy head tilts the whole surface's axis toward it: take the axis of the handle alone (the 60 % of
                // the length away from the end with more surface) when that part is a rod
                let (lo, hi) = extent(g, u0);
                let mid = (lo + hi) / 2.0;
                let s = g.surface();
                let low_area: f64 = s.iter().filter(|(c, _)| dot(*c, u0) < mid).map(|x| x.1).sum();
                let high_area: f64 = s.iter().filter(|(c, _)| dot(*c, u0) >= mid).map(|x| x.1).sum();
                let cut = if high_area >= low_area { lo + 0.6 * (hi - lo) } else { hi - 0.6 * (hi - lo) };
                let handle = surface_axes(g, |c| if high_area >= low_area { dot(c, u0) <= cut } else { dot(c, u0) >= cut });
                if let Some(h) = handle.filter(|h| h[0].0 > 4.0 * h[1].0.max(1e-12)) {
                    let a = if dot(h[0].1, u0) < 0.0 { scale(h[0].1, -1.0) } else { h[0].1 };
                    let (a, sn) = snap(a);
                    note.push(format!("the whole surface's axis {} refined along the handle", v3s(u0)));
                    (u0, snapped) = (a, sn);
                }
            }
            let (lo, hi) = extent(g, u0);
            let len = hi - lo;
            if len.is_nan() || len <= 1e-4 {
                return Err("no length to orient".into());
            }
            // the other two directions, square to the length
            let others: Vec<V3> = (0..3).filter(|k| *k != long).map(|k| across(axes[k].0, u0)).collect();
            let inside = (lo - 0.05 * len..=hi + 0.05 * len).contains(&0.0)
                && others.iter().all(|a| {
                    let (l, h) = extent(g, *a);
                    (l - 0.05 * len..=h + 0.05 * len).contains(&0.0)
                });
            // the end with more surface is the head
            let mid = (lo + hi) / 2.0;
            let s = g.surface();
            let low_area: f64 = s.iter().filter(|(c, _)| dot(*c, u0) < mid).map(|x| x.1).sum();
            let high_area: f64 = s.iter().filter(|(c, _)| dot(*c, u0) >= mid).map(|x| x.1).sum();
            // the head: the end farther from the origin, where Ragnarök puts it (axe00 and kratosspear00 both have
            // their origin nearer the butt, about where Kratos holds them); for a model without one inside it, the
            // end with more surface (a spear's slim blade can have less than its butt)
            let head_high = if inside && (hi + lo).abs() > 0.02 * len { hi + lo > 0.0 } else { high_area >= low_area };
            note.push(format!("length {len:.3} m along {}{}", v3s(u0), if snapped { " (a coordinate axis)" } else { "" }));
            let (from_lo, from_hi) = (-lo, hi);
            let u: V3;
            if inside && from_lo.min(from_hi) <= NEAR_END * len {
                // the origin is the grip; the far end is the other one
                let grip_low = from_lo <= from_hi;
                u = if grip_low { u0 } else { scale(u0, -1.0) };
                grip = [0.0; 3];
                note.push(format!("grip: the origin, {:.0} % of the length from one end", 100.0 * from_lo.min(from_hi) / len));
                if grip_low != head_high {
                    note.push(format!("warning: the grip end holds more surface ({:.4} vs {:.4} m²) than the far end", low_area.max(high_area), low_area.min(high_area)));
                }
            } else {
                // the grip a little in from the end without the head, in the middle of the cross-section there
                u = if head_high { u0 } else { scale(u0, -1.0) };
                let start = if head_high { lo } else { hi };
                // where the hand closes: the thinnest section within GRIP_RANGE of the end without the head (a hand on
                // a band or a flare sinks into it), the nearest to GRIP_IN among equals
                let at = |share: f64| start + if head_high { share * len } else { -share * len };
                let steps = ((GRIP_RANGE.1 - GRIP_RANGE.0) * len / 0.005).ceil().max(1.0) as usize;
                let best = (0..=steps)
                    .map(|k| GRIP_RANGE.0 + (GRIP_RANGE.1 - GRIP_RANGE.0) * k as f64 / steps as f64)
                    .chain(std::iter::once(GRIP_IN))
                    .filter_map(|share| section_span(g, u0, at(share)).map(|w| (share, w)))
                    .min_by(|a, b| (a.1 + 0.05 * (a.0 - GRIP_IN).abs()).total_cmp(&(b.1 + 0.05 * (b.0 - GRIP_IN).abs())));
                let share = best.map_or(GRIP_IN, |b| b.0);
                if let Some((s, w)) = best {
                    note.push(format!("the handle is thinnest {:.0} % in ({:.1} cm across)", 100.0 * s, 100.0 * w));
                }
                let t = at(share);
                let centre = section_centre(g, u0, t).unwrap_or_else(|| {
                    let mean: V3 = std::array::from_fn(|k| g.positions.iter().map(|p| p[k] as f64).sum::<f64>() / g.positions.len().max(1) as f64);
                    mean
                });
                // the cross-section's middle, exactly at the grip's place along the length
                grip = sub(centre, scale(u0, dot(centre, u0) - t));
                note.push(format!(
                    "grip: {:.0} % in from the end without the head (the origin is {}), at {}",
                    share * 100.0,
                    if inside { format!("{:.0} % of the length from the nearer end", 100.0 * from_lo.min(from_hi) / len) } else { "outside the model".into() },
                    v3s(grip)
                ));
            }
            let e = match shape {
                ShapeKind::Axe => {
                    // the head's farthest point from the handle (the line through the grip along the length)
                    let reach = g.positions.iter().map(|p| sub(p.map(|x| x as f64), grip)).map(|d| dot(d, u)).fold(0.0f64, f64::max);
                    let far = g.positions.iter().map(|p| sub(p.map(|x| x as f64), grip)).filter(|d| dot(*d, u) >= 0.5 * reach).map(|d| sub(d, scale(u, dot(d, u)))).max_by(|a, b| norm(*a).total_cmp(&norm(*b)));
                    match far {
                        Some(r) if norm(r) >= 0.01 => {
                            let (e, snapped) = snap_across(r, u);
                            note.push(format!("edge: the head's farthest point, {:.3} m from the handle, along {}{}", norm(r), v3s(e), if snapped { " (a coordinate axis)" } else { "" }));
                            e
                        }
                        _ => {
                            let (e, _) = snap_across(positive(others[0]), u);
                            note.push(format!("edge: no head reaches 1 cm from the handle; the wider cross axis {} taken", v3s(e)));
                            e
                        }
                    }
                }
                _ => {
                    let span = |a: V3| {
                        let (l, h) = extent(g, a);
                        h - l
                    };
                    let wide = others.iter().copied().max_by(|a, b| span(*a).total_cmp(&span(*b))).unwrap_or([1.0, 0.0, 0.0]);
                    let (e, snapped) = snap_across(positive(wide), u);
                    note.push(format!("blade width along {}{}", v3s(e), if snapped { " (a coordinate axis)" } else { "" }));
                    // the width becomes +X
                    scale(e, -1.0)
                }
            };
            // e -> -X, u -> -Y, e x u -> +Z
            rows = [scale(e, -1.0), scale(u, -1.0), cross(e, u)];
        }
        ShapeKind::Shield => {
            // the face's normal is the axis it is thinnest along, its long axis the one it is widest along
            let span = |k: usize| axes[k].2 - axes[k].1;
            let thin = (0..3).min_by(|a, b| span(*a).total_cmp(&span(*b))).unwrap_or(2);
            let wide = (0..3).filter(|k| *k != thin).max_by(|a, b| span(*a).total_cmp(&span(*b))).unwrap_or(0);
            let (n0, snapped) = match box_axis(g, axes[thin].0, false) {
                Some(c) => (c, true),
                None => snap(axes[thin].0),
            };
            let s = g.surface();
            let total: f64 = s.iter().map(|x| x.1).sum();
            let centroid: V3 = std::array::from_fn(|k| s.iter().map(|(c, a)| c[k] * a).sum::<f64>() / total);
            let delta = dot(centroid, n0);
            let front = if delta.abs() >= 0.005 {
                note.push(format!("face normal {}{}; the disc lies {:.3} m from the attach point along it: that side is the front", v3s(n0), if snapped { " (a coordinate axis)" } else { "" }, delta.abs()));
                if delta > 0.0 {
                    n0
                } else {
                    scale(n0, -1.0)
                }
            } else {
                let f = positive(n0);
                note.push(format!("face normal {}; the attach point is in the disc's plane: front taken as {} (it may need flipping)", v3s(n0), v3s(f)));
                f
            };
            // a round face has no long axis (the principal one follows a strap or a boss and turns the face by its
            // angle): Ragnarök's own up across the face is taken instead
            let other = (0..3).find(|k| *k != thin && *k != wide).unwrap_or(1);
            let round = span(other) >= 0.9 * span(wide);
            let long = if !round {
                positive(axes[wide].0)
            } else if dot([0.0, 1.0, 0.0], front).abs() < 0.95 {
                [0.0, 1.0, 0.0]
            } else {
                [0.0, 0.0, 1.0]
            };
            if round {
                note.push(format!("a round face ({:.3} and {:.3} m across): Ragnarök's {} across it is up", span(wide), span(other), v3s(long)));
            }
            let (up, _) = snap_across(long, front);
            let side = cross(up, front);
            let (fl, fh) = extent(g, front);
            let (ul, uh) = extent(g, up);
            let (sl, sh) = extent(g, side);
            // the hand behind the middle of the back, SHIELD_HAND_DEPTH behind its face nearest the arm (`fl`)
            grip = [scale(side, (sl + sh) / 2.0), scale(up, (ul + uh) / 2.0), scale(front, fl - SHIELD_HAND_DEPTH)].iter().fold([0.0; 3], |acc, v| [acc[0] + v[0], acc[1] + v[1], acc[2] + v[2]]);
            note.push(format!("disc {:.3} x {:.3} m, {:.3} m thick, the hand {:.0} cm behind the middle of its back ({}); the face's long axis {} to +Y", uh - ul, sh - sl, fh - fl, 100.0 * SHIELD_HAND_DEPTH, v3s(grip), v3s(up)));
            // front -> -Z (0.1.56 had +Z, and the player saw the shield's inside), up -> +Y, side -> -X (a turn, not a
            // mirror)
            rows = [scale(side, -1.0), up, scale(front, -1.0)];
        }
    }
    let pose = Pose { rotation: rows.map(|r| r.map(|x| x as f32)), grip: grip.map(|x| x as f32), scale: scale_by, note: String::new() };
    let axes_line = pose.axes_line();
    Ok(Pose { note: format!("{}; {axes_line}", note.join("; ")), ..pose })
}

/// How far behind the back of a shield the hand holds it: Elden Ring's fist is about this deep, so the shield's back
/// clears it (0.1.56 put the middle of the shield's thickness at the hand).
pub const SHIELD_HAND_DEPTH: f64 = 0.03;

/// The longest an axe or spear may be (Elden Ring's own greatspears are under 3 m).
pub const MAX_LENGTH: f32 = 3.0;
/// The shortest: a handle under 20 cm is not the weapon.
pub const MIN_LENGTH: f32 = 0.2;
/// The widest a shield may be (Elden Ring's greatshields are about 1.2 m).
pub const MAX_SHIELD: f32 = 1.2;
/// The narrowest a shield may be.
pub const MIN_SHIELD: f32 = 0.2;

/// Whether the weapon, in Elden Ring's space (`g`, after [`Pose::place`]), looks like one: not empty, not flat or a
/// line, every position a number, an axe or spear between [`MIN_LENGTH`] and [`MAX_LENGTH`] long, a shield between
/// [`MIN_SHIELD`] and [`MAX_SHIELD`] across. Ok with its size; Err with why not.
pub fn check_shape(shape: ShapeKind, g: &Geometry) -> Result<String, String> {
    if g.positions.is_empty() || g.triangles.is_empty() {
        return Err("no geometry".into());
    }
    if g.positions.iter().flatten().any(|x| !x.is_finite()) {
        return Err("positions that are not numbers".into());
    }
    let area: f64 = g.surface().iter().map(|x| x.1).sum();
    let (lo, hi) = g.bounds();
    let e: [f32; 3] = std::array::from_fn(|k| hi[k] - lo[k]);
    let mut sorted = e;
    sorted.sort_by(|a, b| b.total_cmp(a));
    if area < 1e-6 || sorted[1] < 0.002 {
        return Err(format!("degenerate: {area:.2e} m² of surface, {:.3} x {:.3} x {:.3} m", e[0], e[1], e[2]));
    }
    let size = format!("{:.3} x {:.3} x {:.3} m (x, y, z), from ({:.3}, {:.3}, {:.3}) to ({:.3}, {:.3}, {:.3})", e[0], e[1], e[2], lo[0], lo[1], lo[2], hi[0], hi[1], hi[2]);
    match shape {
        ShapeKind::Axe | ShapeKind::Spear => {
            let len = e[1];
            if len > MAX_LENGTH {
                return Err(format!("{len:.2} m long, longer than {MAX_LENGTH} m ({size})"));
            }
            if len < MIN_LENGTH {
                return Err(format!("{len:.2} m long, shorter than {MIN_LENGTH} m ({size})"));
            }
        }
        ShapeKind::Shield => {
            let wide = e[0].max(e[1]);
            if wide > MAX_SHIELD {
                return Err(format!("{wide:.2} m across, wider than {MAX_SHIELD} m ({size})"));
            }
            if wide < MIN_SHIELD {
                return Err(format!("{wide:.2} m across, narrower than {MIN_SHIELD} m ({size})"));
            }
        }
    }
    Ok(size)
}

/// Where the blade's hit points start: this share of the way from the grip to the tip (Elden Ring's own put 120 and
/// 300 there: 9 % on `WP_A_0300`, 13 % on a cane).
pub const BLADE_BASE: f32 = 0.1;

/// A hit point as the game's static weapons have them: relative to the root node (which owns it), attached to none,
/// pointing down the blade (-Y) with its up toward -Z.
pub fn dummy(id: i16, position: [f32; 3]) -> Dummy {
    Dummy { position, forward: [0.0, -1.0, 0.0], upward: [0.0, 0.0, -1.0], reference_id: id, parent_node: 0, attach_node: -1, color: [255; 4], flag1: false, use_upward: true }
}

/// The middle of the model's cross-section at height `y` ([`section_centre`]; else of the positions within 2 cm or 3 %
/// of `len` of it), or the Y axis there.
fn section_middle(g: &Geometry, y: f32, len: f32) -> [f32; 3] {
    if let Some(c) = section_centre(g, [0.0, 1.0, 0.0], y as f64) {
        return [c[0] as f32, y, c[2] as f32];
    }
    let w = (0.03 * len).max(0.02);
    let near: Vec<&[f32; 3]> = g.positions.iter().filter(|p| (p[1] - y).abs() <= w).collect();
    if near.is_empty() {
        return [0.0, y, 0.0];
    }
    let n = near.len() as f32;
    [near.iter().map(|p| p[0]).sum::<f32>() / n, y, near.iter().map(|p| p[2]).sum::<f32>() / n]
}

/// The hit points of an axe or spear in Elden Ring's space (`g`, the grip at the origin, the tip toward -Y), each at
/// the middle of the model's cross-section at its height: 100 at the tip, 110 in the middle of the blade (from
/// [`BLADE_BASE`] to the tip), 130 between them, 120 at the blade's base by the hand, 122 at the pommel (the +Y end),
/// 300 at the base and 301 at the tip (weapon buffs run between them). `extra` adds (id, share of the way from the
/// grip to the tip; negative toward the pommel), for ids the game turns out to ask for.
pub fn blade_dummies(g: &Geometry, extra: &[(i16, f32)]) -> Vec<Dummy> {
    let (lo, hi) = g.bounds();
    let (tip, pommel) = (lo[1].min(0.0), hi[1].max(0.0));
    let len = pommel - tip;
    let at = |share: f32| -> [f32; 3] { section_middle(g, if share >= 0.0 { share * tip } else { -share * pommel }, len) };
    let mid = (BLADE_BASE + 1.0) / 2.0;
    let mut out: Vec<Dummy> = [(100, 1.0), (110, mid), (130, (mid + 1.0) / 2.0), (120, BLADE_BASE), (300, BLADE_BASE), (301, 1.0)].iter().map(|(id, s)| dummy(*id, at(*s))).collect();
    out.push(dummy(122, section_middle(g, pommel, len)));
    out.extend(extra.iter().map(|(id, s)| dummy(*id, at(*s))));
    out
}

/// The hit points of a shield in Elden Ring's space (`g`, its centre at the origin, its face on +Z): across its face
/// along Y, 100 at 80 % of the way to its -Y edge, 110 at the centre, 120 at 80 % of the way to its +Y edge, 300 by
/// 120 and 301 by 100. `extra` adds (id, share of the way to the -Y edge; negative toward the +Y edge).
pub fn shield_dummies(g: &Geometry, extra: &[(i16, f32)]) -> Vec<Dummy> {
    let (lo, hi) = g.bounds();
    let (top, bottom) = (0.8 * lo[1].min(0.0), 0.8 * hi[1].max(0.0));
    let at = |share: f32| -> [f32; 3] { [0.0, if share >= 0.0 { share * top } else { -share * bottom }, 0.0] };
    let mut out: Vec<Dummy> = [(100, 1.0), (110, 0.0), (120, -1.0), (300, -1.0), (301, 1.0)].iter().map(|(id, s)| dummy(*id, at(*s))).collect();
    // the shield bash: Elden Ring's shield attacks (the Guardian Shield's carrier) hit from 130 in 122 of 124 uses, so
    // it sits on the middle of the face, in front (-Z)
    out.push(dummy(130, [0.0, 0.0, lo[2].min(0.0)]));
    out.extend(extra.iter().map(|(id, s)| dummy(*id, at(*s))));
    out
}

/// What the next round needs to check this one's choices away from the player's PC: files (name relative to the
/// model's probe folder, bytes): the model's raw chunks, every material's reference list (`refs/`), every level-0
/// resource the draws name from the level-of-detail packs (`lod/<hash>.bin`), `parts.tsv`, `ragnarok_raw.obj` (every
/// part's most detailed draws as stored, one group per part), `eldenring_final.obj` (the weapon as built, one group
/// per material, the hit points as comments) and `build.txt` (the build's log lines).
#[derive(Clone, Debug, Default)]
pub struct WeaponProbe {
    pub files: Vec<(String, Vec<u8>)>,
}

/// The most level-of-detail resource bytes a probe keeps per model.
pub const PROBE_RESOURCES_MAX: usize = 96 << 20;

/// A Wavefront OBJ of named groups of geometry (faces count from 1 across the file), with comment lines first.
pub fn obj_text(comments: &[String], groups: &[(String, &Geometry)]) -> String {
    use std::fmt::Write;
    let mut o = String::new();
    for c in comments {
        let _ = writeln!(o, "# {c}");
    }
    let mut base = 1u32;
    for (name, g) in groups {
        let _ = writeln!(o, "g {name}");
        for p in &g.positions {
            let _ = writeln!(o, "v {:.5} {:.5} {:.5}", p[0], p[1], p[2]);
        }
        for t in &g.triangles {
            let _ = writeln!(o, "f {} {} {}", t[0] + base, t[1] + base, t[2] + base);
        }
        base += g.positions.len() as u32;
    }
    o
}

/// A material's texture names for the parts list: its colour map first, then the others (less the layering maps every
/// material shares), up to five.
fn texture_summary(refs: &[String]) -> String {
    let colour = kratos_look::colour_map(refs).cloned();
    let short = |t: &str| -> String {
        let t = t.strip_prefix("TX_").unwrap_or(t);
        match t.rsplit_once('_') {
            Some((head, key)) if key.len() == 16 && u64::from_str_radix(key, 16).is_ok() => head.to_string(),
            _ => t.to_string(),
        }
    };
    let mut names: Vec<String> = colour.iter().map(|c| format!("colour {}", short(c))).collect();
    names.extend(refs.iter().filter(|r| r.starts_with("TX_") && Some(*r) != colour.as_ref() && !["death01", "dynamicmaterial"].iter().any(|k| r.contains(k))).take(4).map(|r| short(r)));
    if names.is_empty() {
        names.push(format!("no textures ({})", refs.iter().take(2).cloned().collect::<Vec<_>>().join(", ")));
    }
    names.join("|")
}

/// The decoded vertices of a draw, from the WAD's GPU chunk (hash 0) or the level-of-detail packs, with where it came
/// from; None when its resource is missing or does not decode.
fn decode_draw(r: &MeshRecord, gpu: &[u8], lod: &mut Option<&mut LodPacks>, probe_lod: &mut BTreeMap<u64, Vec<u8>>, probe_bytes: &mut usize, left_out: &mut BTreeSet<u64>) -> (Option<Vertices>, String) {
    if r.hash == 0 {
        return (kratos::draw_vertices(gpu, r), "wad".into());
    }
    let Some(packs) = lod.as_deref_mut() else { return (None, "no level-of-detail pack".into()) };
    match packs.get(r.hash) {
        Ok(Some(res)) => {
            if !probe_lod.contains_key(&r.hash) {
                if *probe_bytes + res.len() <= PROBE_RESOURCES_MAX {
                    *probe_bytes += res.len();
                    probe_lod.insert(r.hash, res.to_vec());
                } else {
                    left_out.insert(r.hash);
                }
            }
            let v = kratos::draw_vertices(res, r);
            (v, packs.pack_of(r.hash).unwrap_or_default())
        }
        Ok(None) => (None, "in no pack".into()),
        Err(e) => (None, format!("read failed: {e:#}")),
    }
}

/// Kratos's weapon `spec` as its two Elden Ring part files (see [`file_names`]), from `r_heroa00.wad` (its
/// decompressed start in `buf`, its table `es`), the texture packs in `wad_dir` and the level-of-detail packs `lod`
/// (None when there is no `root.lodpack`). Each step logs one line through `log`. The files are empty when the
/// weapon is not built (its chunks are missing, no part is left, it cannot be oriented or does not look like the
/// weapon): the note says why, and its carrier keeps Elden Ring's model. The probe is filled either way. An error
/// comes back only from a fault in this code's own logic, never from the player's data.
#[allow(clippy::type_complexity)]
pub fn build_weapon(spec: &WeaponSpec, buf: &[u8], es: &[Entry], wad_dir: &Path, mut lod: Option<&mut LodPacks>, log: &mut dyn FnMut(&str)) -> Result<(Vec<(String, Vec<u8>)>, String, WeaponProbe)> {
    let mut probe = WeaponProbe::default();
    let mut lines: Vec<String> = Vec::new();
    let mut say = |lines: &mut Vec<String>, l: String| {
        log(&l);
        lines.push(l);
    };
    let model = spec.model;
    let er = stem(spec.er_model);
    let refuse = |lines: &mut Vec<String>, probe: &mut WeaponProbe, why: String| -> (Vec<(String, Vec<u8>)>, String, WeaponProbe) {
        let note = format!("{} ({model} -> {er}) not built: {why}", spec.name);
        lines.push(note.clone());
        probe.files.push(("build.txt".into(), lines.join("\n").into_bytes()));
        (Vec::new(), note, std::mem::take(probe))
    };
    // the raw chunks, whatever else happens
    let mesh_name = format!("MESH_{model}_0");
    let mut raw = Vec::new();
    for (name, id) in [(mesh_name.clone(), 1), (mesh_name.clone(), 60), (format!("MG_{model}_0"), 1), (format!("MG_{model}_0_gpu"), 29), (format!("MDL_{model}"), 1), (format!("MDL_{model}"), 60), (format!("goProto{model}"), 1), (format!("go{model}"), 1), (format!("go{model}_overrideInst"), 1)] {
        if let Some(i) = kratos::chunk_at(es, &name, id) {
            if let Some(b) = kratos::entry_bytes(buf, &es[i]) {
                raw.push(format!("{} ({} bytes)", es[i].name, b.len()));
                probe.files.push((format!("{}_id{id}.bin", es[i].name), b.to_vec()));
            }
        }
    }
    if let Some(t) = kratos::chunk_at(es, &mesh_name, 1).and_then(|i| es.get(i + 1)).filter(|e| e.id == 60) {
        if let Some(b) = kratos::entry_bytes(buf, t) {
            probe.files.push((format!("material_table_{}_id60.bin", t.name), b.to_vec()));
        }
    }
    say(&mut lines, format!("{} ({model} -> {er}): raw chunks {}", spec.name, raw.join(", ")));
    let c = match kratos::model_chunks(buf, es, model) {
        Ok(c) => c,
        Err(e) => return Ok(refuse(&mut lines, &mut probe, e)),
    };
    let rs = kratos::mesh_records(c.mesh);
    let infos = kratos::mg_part_info(c.mg);
    let names = c.materials.map(kratos::material_names).unwrap_or_default();
    let joints = c.proto.and_then(|p| p.get(0x10..0x12)).map(|b| u16::from_le_bytes([b[0], b[1]]));
    let parsed = c.proto.and_then(kratos::Skeleton::parse).map(|s| s.parent.len());
    say(&mut lines, format!("{} draws, {} parts, {} materials, GPU chunk {} bytes, skeleton {}", rs.len(), infos.len(), names.len(), c.gpu.len(), match (joints, parsed) {
        (Some(j), Some(_)) => format!("{j} joints"),
        (Some(j), None) => format!("{j} joints (its tables did not read)"),
        _ => "none".into(),
    }));
    // every material's reference list
    let mut refs: BTreeMap<u16, Vec<String>> = BTreeMap::new();
    for (k, n) in names.iter().enumerate() {
        if let Some(b) = kratos::material_refs_chunk(buf, es, n) {
            probe.files.push((format!("refs/{k:02}_{n}_refs.bin"), b.to_vec()));
            refs.insert(k as u16, kratos::references(b).into_iter().map(|r| r.1).collect());
        }
    }
    say(&mut lines, format!("reference lists for {} of {} materials", refs.len(), names.len()));
    // every part's most detailed draws, decoded
    let (mut probe_lod, mut probe_bytes, mut left_out) = (BTreeMap::new(), 0usize, BTreeSet::new());
    let mut decoded: BTreeMap<usize, Vertices> = BTreeMap::new();
    let mut views = Vec::new();
    let mut tsv = String::from("part\tkind\tjoint\tlevels\tlevel0_first\tlevel0_draws\tlevel0_distance\tdecoded\tvertices\ttriangles\tmin\tmax\tmaterials\ttextures\tverdict\n");
    let (mut from_wad, mut failed, mut sources) = (0, Vec::new(), BTreeSet::new());
    let mut raw_groups: Vec<(String, Geometry)> = Vec::new();
    for (p, info) in infos.iter().enumerate() {
        let mut view = PartView { index: p as u16, ..Default::default() };
        let mut g = Geometry::default();
        if let Some(l0) = info.levels.first() {
            for k in l0.first as usize..l0.first as usize + l0.draws as usize {
                let Some(r) = rs.get(k) else { continue };
                view.draws.push((k, r.material, r.indices / 3));
                let (v, from) = decode_draw(r, c.gpu, &mut lod, &mut probe_lod, &mut probe_bytes, &mut left_out);
                match v {
                    Some(v) => {
                        if from == "wad" {
                            from_wad += 1;
                        } else {
                            sources.insert(from);
                        }
                        g.add(&v.position, &v.triangles);
                        decoded.insert(k, v);
                    }
                    None => failed.push(format!("{k} ({from})")),
                }
            }
        }
        if !g.positions.is_empty() {
            view.bounds = Some(g.bounds());
        }
        views.push(view);
        raw_groups.push((format!("part_{p}"), g));
    }
    let level0: usize = views.iter().map(|v| v.draws.len()).sum();
    say(&mut lines, format!(
        "level 0: {level0} draws, {} decoded ({from_wad} from the WAD, the others from {:?}), {} failed {:?}{}; the probe keeps {} pack resources ({probe_bytes} bytes){}",
        decoded.len(),
        sources,
        failed.len(),
        &failed[..failed.len().min(16)],
        lod.as_deref().map(|l| if l.note.is_empty() { String::new() } else { format!("; {}", l.note) }).unwrap_or_default(),
        probe_lod.len(),
        if left_out.is_empty() { String::new() } else { format!(", {} left out past {} MB", left_out.len(), PROBE_RESOURCES_MAX >> 20) }
    ));
    for (h, b) in std::mem::take(&mut probe_lod) {
        probe.files.push((format!("lod/{h:016x}.bin"), b));
    }
    // what each material the draws name is for the weapon
    let named: BTreeSet<u16> = views.iter().flat_map(|v| v.draws.iter().map(|d| d.1)).collect();
    let uses: Vec<String> = named
        .iter()
        .map(|m| {
            let r = refs.get(m).map(Vec::as_slice).unwrap_or(&[]);
            match material_use(r, spec.rules.reject) {
                MaterialUse::Draw => format!("{m} drawn ({})", kratos_look::colour_map(&r.iter().filter(|t| variant_marker(t, spec.rules.reject).is_none()).cloned().collect::<Vec<_>>()).cloned().unwrap_or_default()),
                u => format!("{m} {}", use_reason(Some(&u))),
            }
        })
        .collect();
    say(&mut lines, format!("materials: {}", uses.join("; ")));
    let choice = choose_parts(&spec.rules, spec.shape, &views, &refs);
    say(&mut lines, format!("parts: {}", choice.note));
    // the parts list, with each part's verdict
    for (p, info) in infos.iter().enumerate() {
        let v = &views[p];
        let mats: BTreeSet<u16> = v.draws.iter().map(|d| d.1).collect();
        let (nv, nt) = v.draws.iter().filter_map(|d| decoded.get(&d.0)).fold((0, 0), |(a, b), x| (a + x.position.len(), b + x.triangles.len()));
        let ok = v.draws.iter().filter(|d| decoded.contains_key(&d.0)).count();
        let (lo, hi) = v.bounds.unwrap_or(([0.0; 3], [0.0; 3]));
        let verdict = if choice.kept.contains(&(p as u16)) { "kept".to_string() } else { choice.dropped.iter().find(|d| d.0 == p as u16).map(|d| format!("dropped: {}", d.1)).unwrap_or_else(|| "dropped".into()) };
        let l0 = info.levels.first();
        tsv.push_str(&format!(
            "{p}\t{}\t{}\t{}\t{}\t{}\t{}\t{ok}/{}\t{nv}\t{nt}\t{:.3},{:.3},{:.3}\t{:.3},{:.3},{:.3}\t{}\t{}\t{verdict}\n",
            info.kind,
            info.joint,
            info.levels.len(),
            l0.map(|l| l.first).unwrap_or(0),
            l0.map(|l| l.draws).unwrap_or(0),
            l0.map(|l| l.distance).unwrap_or(0.0),
            v.draws.len(),
            lo[0],
            lo[1],
            lo[2],
            hi[0],
            hi[1],
            hi[2],
            mats.iter().map(|m| m.to_string()).collect::<Vec<_>>().join(","),
            mats.iter().map(|m| format!("{m}={}", texture_summary(refs.get(m).map(Vec::as_slice).unwrap_or(&[])))).collect::<Vec<_>>().join("; ")
        ));
    }
    probe.files.push(("parts.tsv".into(), tsv.into_bytes()));
    let groups: Vec<(String, &Geometry)> = raw_groups.iter().map(|(n, g)| (n.clone(), g)).collect();
    probe.files.push(("ragnarok_raw.obj".into(), obj_text(&[format!("{model}: every part's most detailed draws as stored (Ragnarok space, metres, Y up)")], &groups).into_bytes()));
    if choice.kept.is_empty() || choice.draws.is_empty() {
        return Ok(refuse(&mut lines, &mut probe, "no part left after the part rules".into()));
    }
    // the chosen draws, turned into Elden Ring's weapon space
    let mut chosen = Geometry::default();
    let draws: Vec<(usize, u16)> = choice.draws.iter().copied().filter(|d| decoded.contains_key(&d.0)).collect();
    for (k, _) in &draws {
        let v = &decoded[k];
        chosen.add(&v.position, &v.triangles);
    }
    if draws.len() < choice.draws.len() {
        say(&mut lines, format!("{} of the {} chosen draws did not decode and are left out", choice.draws.len() - draws.len(), choice.draws.len()));
    }
    if draws.is_empty() || chosen.triangles.is_empty() {
        return Ok(refuse(&mut lines, &mut probe, "none of the chosen draws decoded".into()));
    }
    let pose = match orient(spec.shape, &chosen, kratos_part::SCALE) {
        Ok(p) => p,
        Err(e) => return Ok(refuse(&mut lines, &mut probe, format!("cannot orient it: {e}"))),
    };
    let (blo, bhi) = chosen.bounds();
    say(&mut lines, format!("orientation: {}; Ragnarök box ({:.3}, {:.3}, {:.3}) to ({:.3}, {:.3}, {:.3}); x{}", pose.note, blo[0], blo[1], blo[2], bhi[0], bhi[1], bhi[2], pose.scale));
    let placed = chosen.placed(&pose);
    let size = match check_shape(spec.shape, &placed) {
        Ok(s) => s,
        Err(e) => return Ok(refuse(&mut lines, &mut probe, format!("it does not look like the weapon: {e}"))),
    };
    say(&mut lines, format!("Elden Ring shape: {size}"));
    // the handle slimmed where the hand closes (not the shield, held by its own strap)
    let grip_k = match spec.shape {
        ShapeKind::Shield => 1.0,
        _ => section_span(&placed, [0.0, 1.0, 0.0], 0.0).map_or(1.0, |w| (GRIP_WIDTH as f64 / w).min(1.0) as f32),
    };
    if grip_k < 1.0 {
        say(&mut lines, format!("grip: the handle slimmed to {:.1} cm where the hand closes (x{grip_k:.2}, fading out 6 to 10 cm from it)", 100.0 * GRIP_WIDTH));
    }
    // its materials' looks
    let used: Vec<u16> = draws.iter().map(|d| d.1).collect::<BTreeSet<u16>>().into_iter().collect();
    let t0 = std::time::Instant::now();
    let (looks, look_note) = kratos_look::model_looks(buf, es, wad_dir, &mesh_name, &used, &[], &[], kratos_part::MAX_TEXTURE, kratos_part::MAX_SURFACE, 1.0, spec.rules.reject, &[]);
    say(&mut lines, format!("looks in {:.1}s for materials {used:?}: {look_note}", t0.elapsed().as_secs_f32()));
    // the meshes, rigid on the root node
    let mut meshes: Vec<Mesh> = Vec::new();
    for (k, m) in &draws {
        let Some(material) = used.iter().position(|u| u == m) else { continue };
        meshes.push(kratos_part::draw_mesh(&decoded[k], material as u32, 0, |_, p, n| (thin_grip(pose.place(p), grip_k), pose.turn(n), [0; 4], [1.0, 0.0, 0.0, 0.0])));
    }
    let root = Node { name: er.clone(), parent: -1, t: [0.0; 3], r: [0.0; 3], s: [1.0; 3], flags: 8 | 2 };
    let (nodes, meshes) = kratos_part::merge_by_material_named(vec![root], meshes, 1, &format!("{er}_mesh"));
    let low = er.to_ascii_lowercase();
    let dir = format!("N:\\GR\\data\\INTERROOT_win64\\parts\\Weapon\\{er}\\tex");
    let (mats, textures, LookCounts { coloured, surfaced, .. }) = kratos_part::look_materials(&Naming { tex: &low, material: &er, dir: &dir }, &used, &looks);
    let mut final_geometry = Geometry::default();
    for m in &meshes {
        final_geometry.positions.extend(m.vertices.iter().map(|v| v.position));
    }
    let mut groups_final: Vec<(String, Geometry)> = Vec::new();
    let mut base = 0u32;
    for m in &meshes {
        let g = Geometry { positions: m.vertices.iter().map(|v| v.position).collect(), triangles: m.indices.chunks_exact(3).map(|t| [t[0], t[1], t[2]]).collect() };
        final_geometry.triangles.extend(g.triangles.iter().map(|t| t.map(|i| i + base)));
        base += g.positions.len() as u32;
        groups_final.push((format!("material_{}", used.get(m.material as usize).copied().unwrap_or(0)), g));
    }
    let dummies = match spec.shape {
        ShapeKind::Shield => shield_dummies(&final_geometry, &[]),
        _ => blade_dummies(&final_geometry, &[]),
    };
    let dummy_lines: Vec<String> = dummies.iter().map(|d| format!("{} ({:.3}, {:.3}, {:.3})", d.reference_id, d.position[0], d.position[1], d.position[2])).collect();
    say(&mut lines, format!("hit points: {}", dummy_lines.join(", ")));
    let flver = erpart::flver2_with(&nodes, &mats, &meshes, &dummies);
    let tpf = erpart::tpf(&textures);
    let mut models = vec![flver.clone()];
    if spec.paired {
        // the other of the pair: the same model under its own root name
        let mut second = nodes.clone();
        if let Some(root) = second.first_mut() {
            root.name = format!("{er}_1");
        }
        models.push(erpart::flver2_with(&second, &mats, &meshes, &dummies));
        say(&mut lines, format!("a pair: model 1 ({er}_1) is the same blade, for the other hand"));
    }
    let bnd = erpart::partsbnd_models(&er, models, tpf);
    let tris: usize = meshes.iter().map(|m| m.indices.len() / 3).sum();
    let files: Vec<(String, Vec<u8>)> = file_names(spec.er_model).into_iter().map(|n| (n, bnd.clone())).collect();
    let note = format!(
        "{} ({model} -> {er}): parts {:?}, {} meshes ({tris} triangles, {} materials, {coloured} with their colour map, {surfaced} with their surface), {} hit points, model {} bytes, file {} bytes",
        spec.name,
        choice.kept,
        meshes.len(),
        mats.len(),
        dummies.len(),
        flver.len(),
        bnd.len()
    );
    say(&mut lines, note.clone());
    let groups: Vec<(String, &Geometry)> = groups_final.iter().map(|(n, g)| (n.clone(), g)).collect();
    let mut comments = vec![format!("{} as built: Elden Ring weapon space (metres; grip at the origin, blade toward -Y)", spec.name), format!("pose: {}", pose.note)];
    comments.extend(dummies.iter().map(|d| format!("dummy {} {:.4} {:.4} {:.4}", d.reference_id, d.position[0], d.position[1], d.position[2])));
    probe.files.push(("eldenring_final.obj".into(), obj_text(&comments, &groups).into_bytes()));
    probe.files.push(("build.txt".into(), lines.join("\n").into_bytes()));
    Ok((files, note, probe))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A closed box from `lo` to `hi`, appended to `g`.
    fn cuboid(g: &mut Geometry, lo: [f32; 3], hi: [f32; 3]) {
        let base = g.positions.len() as u32;
        for k in 0..8 {
            g.positions.push([if k & 1 == 0 { lo[0] } else { hi[0] }, if k & 2 == 0 { lo[1] } else { hi[1] }, if k & 4 == 0 { lo[2] } else { hi[2] }]);
        }
        for f in [[0, 1, 3, 2], [4, 6, 7, 5], [0, 4, 5, 1], [2, 3, 7, 6], [0, 2, 6, 4], [1, 5, 7, 3]] {
            g.triangles.push([base + f[0], base + f[1], base + f[2]]);
            g.triangles.push([base + f[0], base + f[2], base + f[3]]);
        }
    }

    /// A 24-sided disc of radius `r` centred at `c`, its face square to axis `n`, `thick` thick.
    fn disc(g: &mut Geometry, c: [f32; 3], r: f32, n: usize, thick: f32) {
        let (a, b) = ((n + 1) % 3, (n + 2) % 3);
        let base = g.positions.len() as u32;
        for side in [-0.5f32, 0.5] {
            let mut centre = c;
            centre[n] += side * thick;
            g.positions.push(centre);
            for k in 0..24 {
                let t = k as f32 / 24.0 * std::f32::consts::TAU;
                let mut p = centre;
                p[a] += r * t.cos();
                p[b] += r * t.sin();
                g.positions.push(p);
            }
        }
        for s in 0..2u32 {
            let o = base + 25 * s;
            for k in 0..24u32 {
                g.triangles.push([o, o + 1 + k, o + 1 + (k + 1) % 24]);
            }
        }
        for k in 0..24u32 {
            let (p, q) = (base + 1 + k, base + 1 + (k + 1) % 24);
            g.triangles.push([p, q, q + 25]);
            g.triangles.push([p, q + 25, p + 25]);
        }
    }

    /// An axe along +Y: a 72 cm handle from y = -0.1 (the origin 10 cm up it, where it is held) and a head toward +X
    /// at its top.
    fn axe() -> Geometry {
        let mut g = Geometry::default();
        cuboid(&mut g, [-0.015, -0.1, -0.015], [0.015, 0.6, 0.015]);
        cuboid(&mut g, [0.015, 0.42, -0.01], [0.26, 0.62, 0.01]);
        g
    }

    fn turned(g: &Geometry, m: [[f32; 3]; 3], shift: [f32; 3]) -> Geometry {
        Geometry { positions: g.positions.iter().map(|p| std::array::from_fn(|r| m[r][0] * p[0] + m[r][1] * p[1] + m[r][2] * p[2] + shift[r])).collect(), triangles: g.triangles.clone() }
    }

    fn close(a: [f32; 3], b: [f32; 3], tol: f32) -> bool {
        (0..3).all(|k| (a[k] - b[k]).abs() <= tol)
    }

    fn det(m: [[f32; 3]; 3]) -> f32 {
        m[0][0] * (m[1][1] * m[2][2] - m[1][2] * m[2][1]) - m[0][1] * (m[1][0] * m[2][2] - m[1][2] * m[2][0]) + m[0][2] * (m[1][0] * m[2][1] - m[1][1] * m[2][0])
    }

    #[test]
    fn weapons_have_their_sheet_ids_model_ids_and_two_files_each() {
        assert_eq!(file_names(9710), ["wp_a_9710.partsbnd.dcx".to_string(), "wp_a_9710_l.partsbnd.dcx".to_string()]);
        assert_eq!(stem(9713), "WP_A_9713");
        let ids: Vec<(&str, &str, u16)> = WEAPONS.iter().map(|w| (w.weapon, w.model, w.er_model)).collect();
        assert_eq!(ids, [("leviathan_axe", "axe00", 9710), ("blades_of_chaos", "explosive00", BLADES_MODEL_ID), ("draupnir_spear", "kratosspear00", 9712), ("guardian_shield", "kratosshield00", 9713)]);
        assert!(WEAPONS.iter().all(|w| w.er_model != kratos_part::MODEL_ID));
        assert_eq!(weapon_spec("guardian_shield").map(|w| w.shape), Some(ShapeKind::Shield));
        assert_eq!(weapon_spec("blades_of_chaos").map(|w| w.shape), Some(ShapeKind::Spear));
        // the part file holds the model and its textures under the model's own name, as the armour does
        let bnd = erpart::partsbnd(&stem(9710), vec![1, 2, 3], vec![4, 5]);
        let names = bnd_names(&dcx_data(&bnd));
        assert_eq!(names, ["N:\\GR\\data\\INTERROOT_win64\\parts\\WP_A_9710\\WP_A_9710.tpf", "N:\\GR\\data\\INTERROOT_win64\\parts\\WP_A_9710\\WP_A_9710.flver"]);
        // a pair's second model is model 1, `_1`
        assert!(weapon_spec("blades_of_chaos").unwrap().paired && WEAPONS.iter().filter(|w| w.paired).count() == 1);
        let bnd = erpart::partsbnd_models(&stem(9711), vec![vec![1], vec![2]], vec![4, 5]);
        let names = bnd_names(&dcx_data(&bnd));
        assert_eq!(names[1..], ["N:\\GR\\data\\INTERROOT_win64\\parts\\WP_A_9711\\WP_A_9711.flver".to_string(), "N:\\GR\\data\\INTERROOT_win64\\parts\\WP_A_9711\\WP_A_9711_1.flver".to_string()]);
    }

    /// The data inside a DCX of stored zlib blocks (as [`erpart::dcx_dflt`] writes).
    fn dcx_data(d: &[u8]) -> Vec<u8> {
        let z = &d[0x4C..];
        let (mut at, mut out) = (2, Vec::new());
        loop {
            let last = z[at] & 1;
            let len = u16::from_le_bytes([z[at + 1], z[at + 2]]) as usize;
            out.extend_from_slice(&z[at + 5..at + 5 + len]);
            at += 5 + len;
            if last == 1 {
                return out;
            }
        }
    }

    fn utf16z(b: &[u8], at: usize) -> String {
        let units: Vec<u16> = b[at..].chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).take_while(|u| *u != 0).collect();
        String::from_utf16_lossy(&units)
    }

    fn bnd_names(b: &[u8]) -> Vec<String> {
        let n = i32::from_le_bytes(b[0xC..0x10].try_into().unwrap()) as usize;
        (0..n).map(|i| utf16z(b, u32::from_le_bytes(b[0x40 + i * 0x24 + 0x20..0x40 + i * 0x24 + 0x24].try_into().unwrap()) as usize)).collect()
    }

    fn bnd_file(b: &[u8], i: usize) -> Vec<u8> {
        let h = 0x40 + i * 0x24;
        let size = u64::from_le_bytes(b[h + 8..h + 16].try_into().unwrap()) as usize;
        let at = u32::from_le_bytes(b[h + 0x18..h + 0x1C].try_into().unwrap()) as usize;
        b[at..at + size].to_vec()
    }

    #[test]
    fn variant_markers_start_a_word_and_other_markers_match_anywhere() {
        assert!(starts_a_word("TX_axe02_base_ice2_0123456789ABCDEF", "ice"));
        assert!(starts_a_word("TX_axe_frost_basiclvl1_alt", "frost"));
        assert!(!starts_a_word("TX_bifrost00_m_gen_2d_0123456789ABCDEF", "frost"));
        assert!(!starts_a_word("TX_kratos_device_gen_0d", "ice"));
        assert!(starts_a_word("TX_guardian_anniversary_d_40D89DD705C2E4FF", "anniver"));
        assert!(names_marker("TX_kratosaxe00_int9_handlea_m1", "axe00"));
        assert!(names_marker("TX_GuardianShield00_e", "guardian"));
        assert_eq!(material_use(&["height_mesh_ps_00002206".into()], &["gold"]), MaterialUse::NoTextures);
        assert_eq!(material_use(&["TX_death01_112E6D2408347E77".into(), "TX_wave_flow_n_32F4CAFE7ED2E672".into()], &["gold"]), MaterialUse::NoColour);
        let gold = material_use(&["TX_axe02_down_01_gold_gen_0d_0123456789ABCDEF".into(), "TX_axe02_down_01_gold_gen_0n_0123456789ABCDEF".into()], &["gold"]);
        assert_eq!(gold, MaterialUse::Variant { marker: "gold".into(), texture: "TX_axe02_down_01_gold_gen_0d_0123456789ABCDEF".into() });
        // a material layered with a variant's maps draws with its own
        assert_eq!(material_use(&["TX_axe02_base_ice_gen_0d_0123456789ABCDEF".into(), "TX_axe02_base_metal_gen_0d_0123456789ABCDEF".into()], &["ice"]), MaterialUse::Draw);
    }

    /// A part whose draws have 10 triangles each.
    fn view(index: u16, draws: &[(usize, u16)], bounds: Option<([f32; 3], [f32; 3])>) -> PartView {
        PartView { index, draws: draws.iter().map(|d| (d.0, d.1, 10)).collect(), bounds }
    }

    fn refs_of(list: &[(u16, &[&str])]) -> BTreeMap<u16, Vec<String>> {
        list.iter().map(|(m, r)| (*m, r.iter().map(|s| s.to_string()).collect())).collect()
    }

    #[test]
    fn the_axe_keeps_its_base_parts_and_drops_variants_stand_ins_and_overlays() {
        let rules = &weapon_spec("leviathan_axe").unwrap().rules;
        let b = Some(([0.0; 3], [0.1; 3]));
        let parts = vec![
            view(0, &[(10, 0), (11, 5)], b),
            view(1, &[(12, 1)], b),
            view(2, &[(13, 2)], b),
            view(3, &[(14, 3)], b),
            view(4, &[(15, 4)], b),
            view(5, &[(16, 6)], b),
            view(6, &[], None),
            view(7, &[(17, 1)], None),
            // the handle again with an ice layer as big as itself: the layer is left out, the part kept
            view(8, &[(18, 7), (19, 1)], b),
            // a gold pommel with a small gem: a variant
            PartView { index: 9, draws: vec![(20, 3, 40), (21, 1, 4)], bounds: b },
        ];
        let refs = refs_of(&[
            (0, &["TX_death01_112E6D2408347E77", "TX_kratosaxe_body_v1_gen_0d_0123456789ABCDEF", "TX_axe_frost_basiclvl1_alt_0123456789ABCDEF"]),
            (1, &["TX_axe02_down_01_gen_0d_0123456789ABCDEF"]),
            (2, &["TX_axe04_down_blade_gen_0d_0123456789ABCDEF"]),
            (3, &["TX_axe02_down_01_gold_gen_0d_0123456789ABCDEF"]),
            (4, &["height_mesh_ps_00002206"]),
            (5, &["TX_wave_flow_n_32F4CAFE7ED2E672"]),
            (6, &["TX_kratosaxe_body_v4_anniversary_gen_0d_0123456789ABCDEF"]),
            (7, &["TX_axe02_base_ice_gen_0d_0123456789ABCDEF"]),
        ]);
        let c = choose_parts(rules, ShapeKind::Axe, &parts, &refs);
        assert_eq!(c.kept, vec![0, 1, 8], "{}", c.note);
        assert_eq!(c.draws, vec![(10, 0), (12, 1), (19, 1)]);
        // the effect layer of the head part and the ice layer of the handle are left out, their parts kept
        assert_eq!(c.skipped_draws.iter().map(|d| (d.0, d.1)).collect::<Vec<_>>(), [(11, 5), (18, 7)]);
        assert!(c.skipped_draws[1].2.contains("ice"), "{:?}", c.skipped_draws);
        let why = |p: u16| c.dropped.iter().find(|d| d.0 == p).map(|d| d.1.clone()).unwrap_or_default();
        assert!(why(2).contains("not the base look"), "{}", why(2));
        assert!(why(3).contains("gold"), "{}", why(3));
        assert!(why(4).contains("no textures"), "{}", why(4));
        assert!(why(5).contains("anniver"), "{}", why(5));
        assert!(why(6).contains("no draws"), "{}", why(6));
        assert!(why(7).contains("decoded"), "{}", why(7));
        assert!(why(9).contains("gold") && why(9).contains("40 of its 44 triangles"), "{}", why(9));
        assert!(c.note.starts_with("3 of 10 parts kept [0, 1, 8]"), "{}", c.note);
        // another upgrade look, its frost shell, and a runic gem drawn again in another colour state: dropped
        let more = vec![view(0, &[(10, 0)], b), view(10, &[(30, 8)], b), view(11, &[(31, 9)], b), view(12, &[(32, 10)], Some(([0.0; 3], [0.02; 3]))), view(13, &[(33, 11)], Some(([0.0; 3], [0.02; 3])))];
        let more_refs = refs_of(&[
            (0, &["TX_kratosaxe_body_v1_gen_0d_0123456789ABCDEF"]),
            (8, &["TX_kratosaxe_body_v2_gen_0d_0123456789ABCDEF"]),
            (9, &["TX_blade02_d_CE99FF98BE11CD9F"]),
            (10, &["TX_blue_07_gen_6d_15C6937243D9FD28"]),
            (11, &["TX_blue_07_gen_6d_15C6937243D9FD28"]),
        ]);
        let c = choose_parts(rules, ShapeKind::Axe, &more, &more_refs);
        assert_eq!(c.kept, vec![0, 12], "{}", c.note);
        assert!(c.dropped.iter().any(|d| d.0 == 13 && d.1.contains("same piece as part 12")), "{:?}", c.dropped);
        // names of another game version: the other rules choose
        let elsewhere = PartRules { only: &["no_such_map"], ..*rules };
        let c = choose_parts(&elsewhere, ShapeKind::Axe, &parts, &refs);
        assert_eq!(c.kept, vec![0, 1, 8], "{}", c.note);
        assert!(c.note.contains("the other rules choose") && c.dropped.iter().any(|d| d.0 == 2 && d.1.contains("axe04")), "{}", c.note);
        // a part list in the rules is taken as it is
        let listed = PartRules { parts: Some(&[1, 2]), ..*rules };
        assert_eq!(choose_parts(&listed, ShapeKind::Axe, &parts, &refs).kept, vec![1, 2]);
        // nothing drawable: nothing kept, and the weapon is not built
        assert!(choose_parts(rules, ShapeKind::Axe, &parts[3..5], &refs).kept.is_empty());
    }

    #[test]
    fn the_blades_keep_the_first_blade_handle_and_pommel() {
        let rules = &weapon_spec("blades_of_chaos").unwrap().rules;
        let b = Some(([0.0; 3], [0.1; 3]));
        let parts = vec![view(0, &[(10, 0)], b), view(1, &[(11, 1)], b), view(2, &[(12, 2)], b), view(3, &[(13, 3)], b), view(4, &[(14, 4)], b), view(5, &[(15, 5)], b), view(6, &[(16, 6)], b)];
        let refs = refs_of(&[
            (0, &["TX_kratosblades00_bladea_m_gen_0d_0123456789ABCDEF", "TX_kratosblades00_bladea_m_gen_0n_0123456789ABCDEF"]),
            (1, &["TX_kratosblades00_handlea_m_gen_0d_0123456789ABCDEF"]),
            (2, &["TX_kratosblades00_pommela_m_gen_0d_0123456789ABCDEF"]),
            (3, &["TX_kratosblades00_bladeb_m_gen_0d_0123456789ABCDEF"]),
            // the second handle of the first look is another handle, not the first
            (4, &["TX_kratosblades00_handlea2_m_gen_0d_0123456789ABCDEF"]),
            (5, &["TX_kratosblades00_damaged_d_0123456789ABCDEF"]),
            (6, &["TX_kratosblades00_pommelh_m_gen_0d_0123456789ABCDEF"]),
        ]);
        let c = choose_parts(rules, ShapeKind::Spear, &parts, &refs);
        assert_eq!(c.kept, vec![0, 1, 2], "{}", c.note);
        assert!(c.dropped.iter().any(|d| d.0 == 5 && d.1.contains("damaged")), "{:?}", c.dropped);
    }

    #[test]
    fn the_shield_keeps_the_guardian_parts_or_else_one_disc_at_the_origin() {
        let rules = &weapon_spec("guardian_shield").unwrap().rules;
        let disc_box = |y: f32| Some(([-0.25, y, -0.25], [0.25, y + 0.04, 0.25]));
        let boss = Some(([-0.04, 0.05, -0.04], [0.04, 0.09, 0.04]));
        let far = Some(([0.5, 0.0, 0.0], [0.55, 0.05, 0.05]));
        let parts = vec![view(0, &[(0, 0)], disc_box(0.0)), view(1, &[(1, 1)], disc_box(0.005)), view(2, &[(2, 2)], boss), view(3, &[(3, 3)], disc_box(0.0)), view(4, &[(4, 2)], far)];
        let named = refs_of(&[
            (0, &["TX_kratosshield00_guardian_gen_0d_0123456789ABCDEF"]),
            (1, &["TX_blitzshield00_gen_0d_51F4DF1D126132D2"]),
            (2, &["TX_shieldweaponcomponent1_gen_0d_FDA2FF1B24C31D5C"]),
            (3, &["TX_guardian_anniversary_d_40D89DD705C2E4FF"]),
        ]);
        let c = choose_parts(rules, ShapeKind::Shield, &parts, &named);
        // the anniversary version is a variant; of the rest, only the guardian part names "guardian"
        assert_eq!(c.kept, vec![0], "{}", c.note);
        assert!(c.dropped.iter().any(|d| d.0 == 3 && d.1.contains("anniver")));
        assert!(c.dropped.iter().any(|d| d.0 == 1 && d.1.contains("guardian")));
        // no part names it: the disc nearest the origin (ties to the lower part) and what sits inside it
        let unnamed = refs_of(&[(0, &["TX_shield_a_gen_0d_0123456789ABCDEF"]), (1, &["TX_shield_b_gen_0d_0123456789ABCDEF"]), (2, &["TX_boss_gen_0d_0123456789ABCDEF"]), (3, &["TX_shield_c_gen_0d_0123456789ABCDEF"])]);
        let c = choose_parts(rules, ShapeKind::Shield, &parts, &unnamed);
        assert_eq!(c.kept, vec![0, 2], "{}", c.note);
        assert!(c.dropped.iter().any(|d| d.0 == 1 && d.1.contains("another disc")));
        assert!(c.dropped.iter().any(|d| d.0 == 4 && d.1.contains("outside")));
        assert!(c.note.contains("no part names"), "{}", c.note);
        // every part names it (a material they share): one disc again
        let shared = refs_of(&[(0, &["TX_kratosshield00_guardian_gen_0d_0123456789ABCDEF"]), (1, &["TX_kratosshield00_guardian_gen_0d_0123456789ABCDEF"]), (2, &["TX_kratosshield00_guardian_gen_0d_0123456789ABCDEF"]), (3, &["TX_kratosshield00_guardian_gen_0d_0123456789ABCDEF"])]);
        let c = choose_parts(rules, ShapeKind::Shield, &parts, &shared);
        assert_eq!(c.kept, vec![0, 2], "{}", c.note);
        assert!(c.note.contains("shared material"), "{}", c.note);
        // the guardian parts hold a damaged disc too: one of them is drawn, not both on top of each other
        let damaged = refs_of(&[
            (0, &["TX_kratosshield00_guardian_gen_0d_0123456789ABCDEF"]),
            (1, &["TX_kratosshield00_guardian_d_damage_gen_0d_0123456789ABCDEF"]),
            (2, &["TX_shieldweaponcomponent1_gen_0d_FDA2FF1B24C31D5C"]),
            (3, &["TX_blitzshield00_gen_0d_51F4DF1D126132D2"]),
        ]);
        let c = choose_parts(rules, ShapeKind::Shield, &parts[..4], &damaged);
        assert_eq!(c.kept, vec![0], "{}", c.note);
        assert!(c.dropped.iter().any(|d| d.0 == 1 && d.1.contains("another disc")), "{:?}", c.dropped);
    }

    #[test]
    fn an_axe_held_at_its_origin_turns_with_the_handle_down_minus_y_and_the_edge_toward_minus_x() {
        let g = axe();
        let pose = orient(ShapeKind::Axe, &g, 1.0).unwrap();
        assert_eq!(pose.grip, [0.0; 3], "{}", pose.note);
        assert!(close(pose.place([0.26, 0.62, 0.0]), [-0.26, -0.62, 0.0], 1e-5), "{}", pose.note);
        assert!(close(pose.place([0.0, -0.1, 0.0]), [0.0, 0.1, 0.0], 1e-5));
        assert!((det(pose.rotation) - 1.0).abs() < 1e-5);
        assert!(pose.note.contains("grip: the origin, 14 %"), "{}", pose.note);
        // scaled like the body
        let small = orient(ShapeKind::Axe, &g, kratos_part::SCALE).unwrap();
        assert!(close(small.place([0.0, 0.6, 0.0]), [0.0, -0.6 * kratos_part::SCALE, 0.0], 1e-5));
        // turned and moved away in Ragnarök's space, the same axe comes out the same way
        let (s, c) = (0.5f32.sin(), 0.5f32.cos());
        let tilt = [[c, -s, 0.0], [s * 0.8, c * 0.8, -0.6], [s * 0.6, c * 0.6, 0.8]];
        assert!((det(tilt) - 1.0).abs() < 1e-5);
        let moved = turned(&g, tilt, [0.0; 3]);
        let pose2 = orient(ShapeKind::Axe, &moved, 1.0).unwrap();
        let head = turned(&Geometry { positions: vec![[0.26, 0.62, 0.0]], triangles: vec![] }, tilt, [0.0; 3]).positions[0];
        let p = pose2.place(head);
        // the whole surface's axis tilts toward the heavy head; the handle's own axis does not (the edge leans by the
        // head's thickness: its farthest point is a corner)
        assert!(close(p, [-0.26, -0.62, 0.0], 0.012), "{p:?}: {}", pose2.note);
        assert!(pose2.note.contains("refined along the handle"), "{}", pose2.note);
        let bottom = pose2.place(turned(&Geometry { positions: vec![[0.0, -0.1, 0.0]], triangles: vec![] }, tilt, [0.0; 3]).positions[0]);
        assert!(close(bottom, [0.0, 0.1, 0.0], 1e-4), "{bottom:?}");
    }

    #[test]
    fn an_axe_whose_origin_is_not_an_end_is_held_fifteen_percent_up_from_the_end_without_the_head() {
        let g = turned(&axe(), [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]], [0.0, -0.25, 0.0]);
        let pose = orient(ShapeKind::Axe, &g, 1.0).unwrap();
        // 72 cm from y = -0.35: the grip 10.8 cm up, in the middle of the handle
        assert!(close(pose.grip, [0.0, -0.35 + 0.108, 0.0], 1e-4), "{:?}: {}", pose.grip, pose.note);
        assert!(close(pose.place([0.0, -0.35, 0.0]), [0.0, 0.108, 0.0], 1e-4));
        assert!(close(pose.place([0.26, 0.37, 0.0]), [-0.26, -(0.72 - 0.108), 0.0], 1e-4), "{}", pose.note);
        assert!(pose.note.contains("15 % in from the end without the head"), "{}", pose.note);
        // a 5 cm band where the hand would go: the grip moves off it, onto the plain handle
        let mut banded = g.clone();
        cuboid(&mut banded, [-0.025, -0.255, -0.025], [0.025, -0.225, 0.025]);
        let pose = orient(ShapeKind::Axe, &banded, 1.0).unwrap();
        let up = pose.grip[1] + 0.35;
        assert!(!(0.12..=0.13).contains(&up) && (0.05..=0.18).contains(&up), "{up}: {}", pose.note);
        assert!(pose.note.contains("thinnest") && pose.note.contains("3.0 cm across"), "{}", pose.note);
    }

    #[test]
    fn a_spear_along_z_turns_its_point_to_minus_y_and_its_blade_flat_across_x() {
        let mut g = Geometry::default();
        cuboid(&mut g, [-0.012, -0.012, -1.0], [0.012, 0.012, 1.2]);
        cuboid(&mut g, [-0.05, -0.004, 1.2], [0.05, 0.004, 1.5]);
        let pose = orient(ShapeKind::Spear, &g, 1.0).unwrap();
        // the origin is 40 % up: the grip goes 15 % up from the butt
        assert!(close(pose.grip, [0.0, 0.0, -0.625], 1e-4), "{:?}: {}", pose.grip, pose.note);
        assert!(close(pose.place([0.0, 0.0, 1.5]), [0.0, -2.125, 0.0], 1e-4), "{}", pose.note);
        assert!(close(pose.place([0.05, 0.0, 1.3]), [0.05, -1.925, 0.0], 1e-4), "{}", pose.note);
        assert!((det(pose.rotation) - 1.0).abs() < 1e-5);
        assert!(check_shape(ShapeKind::Spear, &g.placed(&pose)).is_ok());
    }

    #[test]
    fn a_shield_faces_minus_z_with_its_back_a_fist_in_front_of_the_hand() {
        // a disc face along Y in front of the attach point, with a boss on its front
        let mut g = Geometry::default();
        disc(&mut g, [0.0, 0.04, 0.0], 0.25, 1, 0.03);
        cuboid(&mut g, [-0.04, 0.055, -0.04], [0.04, 0.08, 0.04]);
        let pose = orient(ShapeKind::Shield, &g, 1.0).unwrap();
        assert!((det(pose.rotation) - 1.0).abs() < 1e-5);
        assert!(close(pose.turn([0.0, 1.0, 0.0]), [0.0, 0.0, -1.0], 1e-5), "{}", pose.note);
        // the back (y = 0.025) 3 cm in front of the hand, the boss (y = 0.08) at the front
        assert!(close(pose.grip, [0.0, -0.005, 0.0], 1e-3), "{:?}", pose.grip);
        assert!(close(pose.place([0.0, 0.025, 0.0]), [0.0, 0.0, -0.03], 1e-3));
        assert!(close(pose.place([0.0, 0.08, 0.0]), [0.0, 0.0, -0.085], 1e-3));
        assert!(pose.note.contains("that side is the front"), "{}", pose.note);
        let placed = g.placed(&pose);
        let (lo, hi) = placed.bounds();
        assert!((hi[0] - lo[0] - 0.5).abs() < 0.01 && (hi[1] - lo[1] - 0.5).abs() < 0.01, "{lo:?} {hi:?}");
        assert!(check_shape(ShapeKind::Shield, &placed).is_ok());
        // turned in Ragnarök's space, its face still comes out on -Z, centred on the hand
        let (s, c) = (0.7f32.sin(), 0.7f32.cos());
        let tilt = [[c, -s, 0.0], [s * 0.6, c * 0.6, -0.8], [s * 0.8, c * 0.8, 0.6]];
        let pose = orient(ShapeKind::Shield, &turned(&g, tilt, [0.0; 3]), 1.0).unwrap();
        let normal = [tilt[0][1], tilt[1][1], tilt[2][1]];
        assert!(close(pose.turn(normal), [0.0, 0.0, -1.0], 1e-3), "{}", pose.note);
        let front = [0.08 * tilt[0][1], 0.08 * tilt[1][1], 0.08 * tilt[2][1]];
        assert!(close(pose.place(front), [0.0, 0.0, -0.085], 2e-3), "{:?}", pose.place(front));
        // the attach point in the disc's plane: the front is a guess, and the log says so
        let mut flat = Geometry::default();
        disc(&mut flat, [0.0; 3], 0.25, 1, 0.03);
        let pose = orient(ShapeKind::Shield, &flat, 1.0).unwrap();
        assert!(pose.note.contains("may need flipping"), "{}", pose.note);
        assert!(close(pose.turn([0.0, 1.0, 0.0]), [0.0, 0.0, -1.0], 1e-5));
    }

    #[test]
    fn a_round_shield_keeps_ragnaroks_up_whatever_its_strap_does() {
        // a round face along Y with a strap across it at 30 degrees: the strap must not turn the face
        let mut g = Geometry::default();
        disc(&mut g, [0.0, 0.04, 0.0], 0.3, 1, 0.03);
        let mut strap = Geometry::default();
        cuboid(&mut strap, [-0.01, 0.055, -0.27], [0.01, 0.065, 0.27]);
        let (s, c) = (30f32.to_radians().sin(), 30f32.to_radians().cos());
        let strap = turned(&strap, [[c, 0.0, s], [0.0, 1.0, 0.0], [-s, 0.0, c]], [0.0; 3]);
        let base = g.positions.len() as u32;
        g.positions.extend(strap.positions);
        g.triangles.extend(strap.triangles.iter().map(|t| t.map(|i| i + base)));
        let pose = orient(ShapeKind::Shield, &g, 1.0).unwrap();
        assert!(pose.note.contains("round face"), "{}", pose.note);
        assert!(close(pose.turn([0.0, 1.0, 0.0]), [0.0, 0.0, -1.0], 1e-5), "{}", pose.note);
        assert!(close(pose.turn([0.0, 0.0, 1.0]), [0.0, 1.0, 0.0], 1e-5), "{}", pose.note);
    }

    #[test]
    fn shapes_that_are_not_the_weapon_are_refused_with_the_reason() {
        let stick = |len: f32| {
            let mut g = Geometry::default();
            cuboid(&mut g, [-0.02, -len, -0.02], [0.02, 0.0, 0.02]);
            g
        };
        assert!(check_shape(ShapeKind::Axe, &stick(0.7)).is_ok());
        assert!(check_shape(ShapeKind::Spear, &stick(4.0)).unwrap_err().contains("longer than 3"));
        assert!(check_shape(ShapeKind::Axe, &stick(0.1)).unwrap_err().contains("shorter than 0.2"));
        let mut wide = Geometry::default();
        disc(&mut wide, [0.0; 3], 0.8, 2, 0.03);
        assert!(check_shape(ShapeKind::Shield, &wide).unwrap_err().contains("wider than 1.2"));
        assert!(check_shape(ShapeKind::Axe, &Geometry::default()).unwrap_err().contains("no geometry"));
        // a line: every point on the Y axis
        let line = Geometry { positions: vec![[0.0, 0.0, 0.0], [0.0, -0.5, 0.0], [0.0, -1.0, 0.0]], triangles: vec![[0, 1, 2]] };
        assert!(check_shape(ShapeKind::Spear, &line).unwrap_err().contains("degenerate"));
        let nan = Geometry { positions: vec![[f32::NAN, 0.0, 0.0], [0.0, -0.5, 0.0], [0.1, -1.0, 0.0]], triangles: vec![[0, 1, 2]] };
        assert!(check_shape(ShapeKind::Spear, &nan).unwrap_err().contains("not numbers"));
        assert!(orient(ShapeKind::Axe, &line, 1.0).is_err());
    }

    #[test]
    fn hit_points_run_from_the_pommel_to_the_tip_and_read_back_from_the_model() {
        let g = axe();
        let pose = orient(ShapeKind::Axe, &g, 1.0).unwrap();
        let placed = g.placed(&pose);
        let d = blade_dummies(&placed, &[(140, 0.5), (150, -1.0)]);
        let ids: Vec<i16> = d.iter().map(|x| x.reference_id).collect();
        assert_eq!(ids, [100, 110, 130, 120, 300, 301, 122, 140, 150]);
        let y = |id: i16| d.iter().find(|x| x.reference_id == id).unwrap().position[1];
        assert!((y(100) + 0.62).abs() < 1e-5 && (y(301) - y(100)).abs() < 1e-6);
        assert!((y(120) + 0.062).abs() < 1e-5 && y(300) == y(120));
        assert!(y(100) < y(130) && y(130) < y(110) && y(110) < y(120) && y(120) < 0.0 && y(122) > 0.0);
        assert!((y(110) + 0.62 * 0.55).abs() < 1e-5 && (y(140) + 0.31).abs() < 1e-5 && (y(150) - 0.1).abs() < 1e-5);
        // at the tip the head's cross-section pulls the point toward the edge (-X); on the handle it stays on it
        let at = |id: i16| d.iter().find(|x| x.reference_id == id).unwrap().position;
        assert!(at(100)[0] < -0.05 && at(120)[0].abs() < 1e-5, "{:?} {:?}", at(100), at(120));
        assert!(d.iter().all(|x| x.forward == [0.0, -1.0, 0.0] && x.upward == [0.0, 0.0, -1.0] && x.parent_node == 0 && x.attach_node == -1 && x.use_upward));
        // in the written model: the count in the header and each record's id after it
        let nodes = vec![Node { name: "WP_A_9710".into(), parent: -1, t: [0.0; 3], r: [0.0; 3], s: [1.0; 3], flags: 10 }, Node { name: "WP_A_9710_mesh_0".into(), parent: -1, t: [0.0; 3], r: [0.0; 3], s: [1.0; 3], flags: 4 }];
        let v = |p: [f32; 3]| erpart::Vertex { position: p, normal: [0.0, 0.0, 1.0], tangent: [1.0, 0.0, 0.0, 1.0], bones: [0; 4], weights: [1.0, 0.0, 0.0, 0.0], color: [255; 4], uv: [[0.0; 2]; 2] };
        let mesh = Mesh { material: 0, node: 1, vertices: placed.positions.iter().map(|p| v(*p)).collect(), indices: placed.triangles.iter().flatten().copied().collect() };
        let mat = erpart::Material { name: "WP_A_9710_m0".into(), mtd: erpart::MATERIAL_MTD.into(), textures: vec![] };
        let f = erpart::flver2_with(&nodes, &[mat], &[mesh], &d);
        assert_eq!(i32::from_le_bytes(f[0x14..0x18].try_into().unwrap()), 9);
        let back: Vec<i16> = (0..9).map(|i| i16::from_le_bytes(f[0x80 + 64 * i + 28..0x80 + 64 * i + 30].try_into().unwrap())).collect();
        assert_eq!(back, ids);
        let tip_y = f32::from_le_bytes(f[0x84..0x88].try_into().unwrap());
        assert_eq!(tip_y, y(100));
        // a shield's run across its face
        let mut s = Geometry::default();
        disc(&mut s, [0.0; 3], 0.25, 2, 0.03);
        let d = shield_dummies(&s, &[]);
        let ys: Vec<(i16, f32)> = d.iter().map(|x| (x.reference_id, x.position[1])).collect();
        assert_eq!(ys.iter().map(|x| x.0).collect::<Vec<_>>(), [100, 110, 120, 300, 301, 130]);
        assert!((ys[0].1 + 0.2).abs() < 1e-3 && ys[1].1 == 0.0 && (ys[2].1 - 0.2).abs() < 1e-3 && ys[3].1 == ys[2].1 && ys[4].1 == ys[0].1, "{ys:?}");
        // the bash point on the middle of the face, in front (-Z; the disc is 3 cm thick along Z)
        assert!((d[5].position[0], d[5].position[1]) == (0.0, 0.0) && (d[5].position[2] + 0.015).abs() < 1e-3, "{:?}", d[5].position);
    }

    #[test]
    fn the_handle_is_slimmed_only_where_the_hand_closes() {
        // at the grip fully, fading out from 6 to 10 cm along it, and not away from the handle's line
        assert_eq!(thin_grip([0.03, 0.0, -0.02], 0.5), [0.015, 0.0, -0.01]);
        assert_eq!(thin_grip([0.03, 0.06, 0.0], 0.5), [0.015, 0.06, 0.0]);
        let mid = thin_grip([0.03, -0.08, 0.0], 0.5);
        assert!((mid[0] - 0.0225).abs() < 1e-6, "{mid:?}");
        assert_eq!(thin_grip([0.03, 0.1, 0.0], 0.5), [0.03, 0.1, 0.0]);
        assert_eq!(thin_grip([0.2, 0.0, 0.0], 0.5), [0.2, 0.0, 0.0]);
        assert_eq!(thin_grip([0.03, 0.0, 0.0], 1.0), [0.03, 0.0, 0.0]);
    }

    #[test]
    fn principal_axes_of_a_symmetric_matrix_come_out_largest_first() {
        let e = eigen_sym3([[2.0, 1.0, 0.0], [1.0, 2.0, 0.0], [0.0, 0.0, 0.5]]);
        assert!((e[0].0 - 3.0).abs() < 1e-9 && (e[1].0 - 1.0).abs() < 1e-9 && (e[2].0 - 0.5).abs() < 1e-9, "{e:?}");
        let h = std::f64::consts::FRAC_1_SQRT_2;
        assert!((dot(e[0].1, [h, h, 0.0]).abs() - 1.0).abs() < 1e-9 && (dot(e[2].1, [0.0, 0.0, 1.0]).abs() - 1.0).abs() < 1e-9);
    }

    /// A model's chunks laid out as in r_heroa00.wad, with its draws in the GPU chunk: (buffer, table).
    struct Wad {
        buf: Vec<u8>,
        es: Vec<Entry>,
    }

    impl Wad {
        fn add(&mut self, name: &str, id: u16, bytes: Vec<u8>) {
            while self.buf.len() % 16 != 0 {
                self.buf.push(0);
            }
            self.es.push(Entry { id, version: 0, length: bytes.len() as u32, name: name.into(), debug_size: 0, temp_size: 0, align: 16, client: 0, server: 0, mem_type: 0, bits: 0, mem_off: 0, file_off: self.buf.len() as u64, block: 0 });
            self.buf.extend(bytes);
        }
    }

    /// An id-60 list of names (a material table, or a material's references).
    fn ref_list(names: &[&str]) -> Vec<u8> {
        let mut b = vec![0u8; 4 + 76 * names.len()];
        b[0..4].copy_from_slice(&(names.len() as u32).to_le_bytes());
        for (i, n) in names.iter().enumerate() {
            let at = 8 + 76 * i;
            b[at] = i as u8 + 1;
            let end = (at + 16 + n.len()).min(b.len());
            b[at + 16..end].copy_from_slice(&n.as_bytes()[..end - at - 16]);
        }
        b
    }

    /// `MESH_`, `MG_` and GPU chunks for parts of one draw each: (material, geometry, hash).
    fn model(parts: &[(u16, &Geometry, u64)]) -> (Vec<u8>, Vec<u8>, Vec<u8>) {
        let mut gpu = Vec::new();
        let mut recs = Vec::new();
        for (material, g, hash) in parts {
            let nv = g.positions.len() as u32;
            let (lo, hi) = g.bounds();
            let pos = gpu.len() as u32;
            for p in &g.positions {
                gpu.extend(p.iter().flat_map(|x| x.to_le_bytes()));
            }
            let nrm = gpu.len() as u32;
            for _ in 0..nv {
                gpu.extend((512u32 | 512 << 10 | 1023 << 20).to_le_bytes());
            }
            let uv = gpu.len() as u32;
            for p in &g.positions {
                gpu.extend(p[0].to_le_bytes());
                gpu.extend(p[1].to_le_bytes());
            }
            let idx = gpu.len() as u32;
            for t in &g.triangles {
                for i in t {
                    gpu.extend((*i as u16).to_le_bytes());
                }
            }
            while gpu.len() % 16 != 0 {
                gpu.push(0);
            }
            let mut r = vec![0u8; 0xC0];
            for k in 0..3 {
                r[0x10 + 4 * k..0x14 + 4 * k].copy_from_slice(&((hi[k] - lo[k]) / 2.0).to_le_bytes());
                r[0x1C + 4 * k..0x20 + 4 * k].copy_from_slice(&((hi[k] + lo[k]) / 2.0).to_le_bytes());
            }
            r[0x28..0x2A].copy_from_slice(&material.to_le_bytes());
            for (o, v) in [(0x30, idx), (0x44, nv), (0x48, g.triangles.len() as u32), (0x5C, 3 * g.triangles.len() as u32), (0x60, 0x90), (0x64, 0xB0)] {
                r[o..o + 4].copy_from_slice(&v.to_le_bytes());
            }
            r[0x68..0x70].copy_from_slice(&hash.to_le_bytes());
            r[0x84] = 3;
            r[0x90..0x98].copy_from_slice(&[0, 0, 3, 0, 0, 0, 0, 0]);
            r[0x98..0xA0].copy_from_slice(&[1, 3, 1, 0, 1, 0, 0, 0]);
            r[0xA0..0xA8].copy_from_slice(&[3, 0, 2, 0, 2, 0, 0, 0]);
            for (k, s) in [pos, nrm, uv].iter().enumerate() {
                r[0xB0 + 4 * k..0xB4 + 4 * k].copy_from_slice(&s.to_le_bytes());
            }
            recs.push(r);
        }
        let mut mesh = vec![0u8; 0x40 + 4 * recs.len()];
        mesh[0x10..0x14].copy_from_slice(&(recs.len() as u32).to_le_bytes());
        for (i, r) in recs.iter().enumerate() {
            let at = mesh.len() as u32;
            mesh[0x40 + 4 * i..0x44 + 4 * i].copy_from_slice(&(at - (0x40 + 4 * i as u32)).to_le_bytes());
            mesh.extend(r);
        }
        // one part per draw: its level 0, then the empty last level
        let mut mg = vec![0u8; 0x44 + 4 * parts.len()];
        mg[0x30..0x32].copy_from_slice(&(parts.len() as u16).to_le_bytes());
        for i in 0..parts.len() {
            let at = mg.len() as u32;
            mg[0x44 + 4 * i..0x48 + 4 * i].copy_from_slice(&at.to_le_bytes());
            let mut p = vec![0u8; 64];
            p[0..2].copy_from_slice(&(i as u16 + 3).to_le_bytes());
            p[2] = 2;
            p[56..60].copy_from_slice(&64u32.to_le_bytes());
            p[60..64].copy_from_slice(&80u32.to_le_bytes());
            let mut l0 = vec![0u8; 16];
            l0[0..4].copy_from_slice(&1u32.to_le_bytes());
            l0[4..8].copy_from_slice(&20.0f32.to_le_bytes());
            l0[10..12].copy_from_slice(&(i as u16).to_le_bytes());
            let mut l1 = vec![0u8; 16];
            l1[4..8].copy_from_slice(&32767.0f32.to_le_bytes());
            p.extend(l0);
            p.extend(l1);
            mg.extend(p);
        }
        (mesh, mg, gpu)
    }

    /// A WAD holding an axe: handle, head, a gold pommel, a shadow shape, and a draw in a pack that is not there.
    fn axe_wad() -> Wad {
        let mut handle = Geometry::default();
        cuboid(&mut handle, [-0.015, -0.1, -0.015], [0.015, 0.6, 0.015]);
        let mut head = Geometry::default();
        cuboid(&mut head, [0.015, 0.42, -0.01], [0.26, 0.62, 0.01]);
        let mut pommel = Geometry::default();
        cuboid(&mut pommel, [-0.03, -0.14, -0.03], [0.03, -0.1, 0.03]);
        // handle, head, a gold pommel, a shadow shape, and a draw in a pack that is not there
        let (mesh, mg, gpu) = model(&[(0, &handle, 0), (1, &head, 0), (2, &pommel, 0), (3, &handle, 0), (1, &head, 0xABCD)]);
        let mats = ["MAT_AAAA000000000001", "MAT_AAAA000000000002", "MAT_AAAA000000000003", "MAT_AAAA000000000004"];
        let refs: [&[&str]; 4] = [&["TX_death01_112E6D2408347E77", "TX_kratosaxe_head_v1_gen_0d_0123456789ABCDEF"], &["TX_kratosaxe_body_v1_gen_0d_0123456789ABCDEF", "TX_kratosaxe_body_v1_gen_0n_0123456789ABCDEF"], &["TX_axe02_down_01_gold_gen_0d_0123456789ABCDEF"], &["height_mesh_ps_00002206"]];
        let mut w = Wad { buf: Vec::new(), es: Vec::new() };
        w.add("MG_axe00_0_gpu", 29, gpu);
        w.add("MESH_axe00_0", 1, mesh);
        w.add(mats[0], 60, ref_list(&mats));
        w.add("MG_axe00_0", 1, mg);
        w.add("MDL_axe00", 1, vec![0; 56]);
        w.add("MESH_axe00_0", 60, vec![0; 156]);
        w.add("goProtoAxe00", 1, vec![0; 0x70]);
        for (m, r) in mats.iter().zip(refs) {
            w.add(m, 1, vec![0; 64]);
            w.add(r[0], 60, ref_list(r));
        }
        w
    }

    /// Writes the axe built from [`axe_wad`] for checking with an outside reader:
    /// `GOWRING_ERPART_OUT=dir cargo test -p gowring-gow sample_weapon -- --ignored`.
    #[test]
    #[ignore]
    fn sample_weapon() {
        let Ok(out) = std::env::var("GOWRING_ERPART_OUT") else { return };
        let w = axe_wad();
        let (files, note, probe) = build_weapon(weapon_spec("leviathan_axe").unwrap(), &w.buf, &w.es, Path::new(&out), None, &mut |l| println!("{l}")).unwrap();
        println!("{note}");
        for (name, bytes) in files.iter().chain(probe.files.iter().filter(|f| f.0.ends_with(".obj") || f.0.ends_with(".tsv"))) {
            std::fs::write(Path::new(&out).join(name), bytes).unwrap();
        }
    }

    /// Chooses, turns and checks each weapon from a player's weapon probe (its `parts.tsv`, `refs/` and
    /// `ragnarok_raw.obj`), and writes `<model>_final.obj` (Elden Ring's weapon space, the hit points as comments) for
    /// looking at: `GOWRING_WEAPON_PROBE=dir GOWRING_ERPART_OUT=out cargo test -p gowring-gow weapons_from_a_probe --
    /// --ignored --nocapture`.
    #[test]
    #[ignore]
    fn weapons_from_a_probe() {
        let Ok(dir) = std::env::var("GOWRING_WEAPON_PROBE") else { return };
        let out = std::env::var("GOWRING_ERPART_OUT").ok();
        for spec in WEAPONS {
            let root = Path::new(&dir).join(spec.model);
            let Ok(tsv) = std::fs::read_to_string(root.join("parts.tsv")) else { continue };
            let num = |t: &str| -> [f32; 3] {
                let v: Vec<f32> = t.split(',').filter_map(|x| x.parse().ok()).collect();
                [v.first().copied().unwrap_or(0.0), v.get(1).copied().unwrap_or(0.0), v.get(2).copied().unwrap_or(0.0)]
            };
            let views: Vec<PartView> = tsv
                .lines()
                .skip(1)
                .filter_map(|l| {
                    let c: Vec<&str> = l.split('\t').collect();
                    let (first, n, tris): (usize, usize, u32) = (c.get(4)?.parse().ok()?, c.get(5)?.parse().ok()?, c.get(9)?.parse().ok()?);
                    let mats: Vec<u16> = c.get(12)?.split(',').filter_map(|m| m.parse().ok()).collect();
                    let draws = (0..n).map(|k| (first + k, mats.get(k).or(mats.last()).copied().unwrap_or(0), tris / n.max(1) as u32)).collect();
                    let decoded = !c.get(7)?.starts_with('0');
                    Some(PartView { index: c[0].parse().ok()?, draws, bounds: decoded.then(|| (num(c[10]), num(c[11]))) })
                })
                .collect();
            let mut refs: BTreeMap<u16, Vec<String>> = BTreeMap::new();
            for e in std::fs::read_dir(root.join("refs")).unwrap().flatten() {
                let name = e.file_name().to_string_lossy().to_string();
                let Some(k) = name.split('_').next().and_then(|k| k.parse::<u16>().ok()) else { continue };
                refs.insert(k, crate::kratos::references(&std::fs::read(e.path()).unwrap()).into_iter().map(|r| r.1).collect());
            }
            let c = choose_parts(&spec.rules, spec.shape, &views, &refs);
            println!("== {}: {}", spec.name, c.note.chars().take(600).collect::<String>());
            // the kept parts' geometry from the raw OBJ (one group per part)
            let obj = std::fs::read_to_string(root.join("ragnarok_raw.obj")).unwrap();
            let (mut all, mut group, mut chosen) = (Vec::<[f32; 3]>::new(), String::new(), Geometry::default());
            let mut map: BTreeMap<u32, u32> = BTreeMap::new();
            for l in obj.lines() {
                if let Some(v) = l.strip_prefix("v ") {
                    all.push(num(&v.replace(' ', ",")));
                } else if let Some(g) = l.strip_prefix("g ") {
                    group = g.to_string();
                } else if let Some(f) = l.strip_prefix("f ") {
                    let keep = group.strip_prefix("part_").and_then(|i| i.parse::<u16>().ok()).is_some_and(|i| c.kept.contains(&i));
                    if keep {
                        let t: Vec<u32> = f.split(' ').filter_map(|x| x.parse::<u32>().ok()).map(|x| x - 1).collect();
                        let t: Vec<u32> = t.iter().map(|i| *map.entry(*i).or_insert_with(|| {
                            chosen.positions.push(all[*i as usize]);
                            chosen.positions.len() as u32 - 1
                        })).collect();
                        chosen.triangles.push([t[0], t[1], t[2]]);
                    }
                }
            }
            let pose = orient(spec.shape, &chosen, kratos_part::SCALE).unwrap();
            println!("   orientation: {}", pose.note);
            let mut placed = chosen.placed(&pose);
            println!("   shape: {:?}", check_shape(spec.shape, &placed));
            if spec.shape != ShapeKind::Shield {
                let k = section_span(&placed, [0.0, 1.0, 0.0], 0.0).map_or(1.0, |w| (GRIP_WIDTH as f64 / w).min(1.0) as f32);
                placed.positions.iter_mut().for_each(|p| *p = thin_grip(*p, k));
                println!("   grip x{k:.2}: now {:?} m across", section_span(&placed, [0.0, 1.0, 0.0], 0.0));
            }
            let dummies = match spec.shape {
                ShapeKind::Shield => shield_dummies(&placed, &[]),
                _ => blade_dummies(&placed, &[]),
            };
            let lines: Vec<String> = dummies.iter().map(|d| format!("dummy {} {:.3} {:.3} {:.3}", d.reference_id, d.position[0], d.position[1], d.position[2])).collect();
            println!("   {}", lines.join(", "));
            if let Some(o) = &out {
                std::fs::write(Path::new(o).join(format!("{}_final.obj", spec.model)), obj_text(&lines, &[(spec.model.to_string(), &placed)])).unwrap();
            }
        }
    }

    #[test]
    fn the_axe_is_built_from_its_chunks_into_two_files_with_its_hit_points_and_a_probe() {
        let w = axe_wad();
        let dir = std::env::temp_dir().join(format!("gowring-weapon-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let mut log = Vec::new();
        let (files, note, probe) = build_weapon(weapon_spec("leviathan_axe").unwrap(), &w.buf, &w.es, &dir, None, &mut |l| log.push(l.to_string())).unwrap();
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(files.iter().map(|f| f.0.as_str()).collect::<Vec<_>>(), ["wp_a_9710.partsbnd.dcx", "wp_a_9710_l.partsbnd.dcx"], "{note}\n{}", log.join("\n"));
        assert_eq!(files[0].1, files[1].1);
        assert!(note.contains("parts [0, 1]"), "{note}");
        let all = log.join("\n");
        assert!(all.contains("4 (no level-of-detail pack)") && all.contains("2 (a variant: material 2 draws TX_axe02_down_01_gold_gen_0d_0123456789ABCDEF (gold); 12 of its 12 triangles)"), "{all}");
        assert!(all.contains("skeleton 0 joints"), "{all}");
        // inside: the model and its textures under WP_A_9710, the hit points after the header, the root node first
        let bnd = dcx_data(&files[0].1);
        assert!(bnd_names(&bnd)[1].ends_with("parts\\WP_A_9710\\WP_A_9710.flver"));
        let f = bnd_file(&bnd, 1);
        assert_eq!(&f[..6], b"FLVER\0");
        let (nd, nm) = (i32::from_le_bytes(f[0x14..0x18].try_into().unwrap()) as usize, i32::from_le_bytes(f[0x18..0x1C].try_into().unwrap()) as usize);
        assert_eq!((nd, nm), (7, 2));
        let node0 = 0x80 + 64 * nd + 0x20 * nm;
        assert_eq!(utf16z(&f, u32::from_le_bytes(f[node0 + 12..node0 + 16].try_into().unwrap()) as usize), "WP_A_9710");
        assert_eq!(u32::from_le_bytes(f[node0 + 60..node0 + 64].try_into().unwrap()), 10);
        let tip = f32::from_le_bytes(f[0x84..0x88].try_into().unwrap());
        assert!((tip + 0.62 * kratos_part::SCALE).abs() < 1e-4, "{tip}");
        // its textures are named after the model, not the body's
        let tpf = bnd_file(&bnd, 0);
        assert!(utf16z(&tpf, u32::from_le_bytes(tpf[0x10 + 12..0x10 + 16].try_into().unwrap()) as usize).starts_with("wp_a_9710_m0_a"));
        let names: Vec<&str> = probe.files.iter().map(|f| f.0.as_str()).collect();
        for want in ["MESH_axe00_0_id1.bin", "MESH_axe00_0_id60.bin", "MG_axe00_0_id1.bin", "MG_axe00_0_gpu_id29.bin", "MDL_axe00_id1.bin", "goProtoAxe00_id1.bin", "material_table_MAT_AAAA000000000001_id60.bin", "refs/02_MAT_AAAA000000000003_refs.bin", "parts.tsv", "ragnarok_raw.obj", "eldenring_final.obj", "build.txt"] {
            assert!(names.contains(&want), "{want} not in {names:?}");
        }
        let text = |n: &str| String::from_utf8(probe.files.iter().find(|f| f.0 == n).unwrap().1.clone()).unwrap();
        let tsv = text("parts.tsv");
        assert_eq!(tsv.lines().count(), 6);
        assert!(tsv.lines().nth(1).unwrap().starts_with("0\t0\t3\t2\t0\t1\t"), "{tsv}");
        assert!(tsv.lines().nth(3).unwrap().contains("colour axe02_down_01_gold_gen_0d") && tsv.lines().nth(3).unwrap().ends_with("(gold); 12 of its 12 triangles"), "{tsv}");
        assert_eq!(text("ragnarok_raw.obj").matches("\ng part_").count(), 5);
        assert!(text("eldenring_final.obj").contains("# dummy 122"));
        // a model that is not in the WAD: not built, with the reason, and the probe still says what was found
        let missing = WeaponSpec { model: "kratosspear00", ..*weapon_spec("draupnir_spear").unwrap() };
        let (files, note, probe) = build_weapon(&missing, &w.buf, &w.es, Path::new("."), None, &mut |_| {}).unwrap();
        assert!(files.is_empty() && note.contains("not built: MESH_kratosspear00_0 (id 1) is not in the bytes read"), "{note}");
        assert!(probe.files.iter().any(|f| f.0 == "build.txt"));
    }
}
