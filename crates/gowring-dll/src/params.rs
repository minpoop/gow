//! hooks.template_scan / param_weapon — Kratos' four carrier weapons are vanilla Elden Ring weapon
//! rows found by category and rewritten in memory (fixed damage, no stat scaling, no weight), and the
//! ten starting classes are rewritten to start with them: a new character on the mashup's own save is
//! born as Kratos, with the engine itself granting and equipping the kit. Carriers whose Ragnarök model the
//! launcher built show that model instead of the vanilla one.

use eldenring::cs::{AtkParam_Pc, BehaviorParam_PC, CharaInitParam, EquipParamProtector, EquipParamWeapon, SoloParam, SoloParamRepository, SwordArtsParam, WepAbsorpPosParam};
use fromsoftware_shared::FromStatic;
use gowring_core::sheets::{WeaponKind, ER_TEMPLATES, RAGE, RUNICS, WEAPONS};
use std::collections::{BTreeMap, HashMap};
use std::sync::OnceLock;

/// Elden Ring's "No Skill": the shield gets it, so the skill button reaches the weapon in Kratos' right hand.
const NO_SKILL: i32 = 10;

/// Rows earlier versions used as carriers. A character made then still holds that weapon, so the row is turned
/// into a copy of today's carrier: 0.1.0-0.1.32 gave the Blades the Caestus (fists, by a wrong weapon type),
/// 0.1.33-0.1.65 the Whip (still today's carrier without Shadow of the Erdtree, see [`DLC_CARRIERS`]).
const LEGACY_CARRIERS: [(&str, u32); 2] = [("blades_of_chaos", 21000000), ("blades_of_chaos", 20000000)];

/// Carriers taken instead of their template's first weapon of its type when Shadow of the Erdtree is installed, as
/// (sheet weapon id, EquipParamWeapon row): the Blades of Chaos take the Backhand Blade (64500000), a paired weapon,
/// so Kratos, two-handing by default, swings a blade in each hand with moves made for two short blades (the whip's
/// lashes were made for its chain). The regulation holds the row with or without the DLC, but its moves are in the
/// DLC's archive, so [`dlc_installed`] decides.
const DLC_CARRIERS: [(&str, u32); 1] = [("blades_of_chaos", 64500000)];

/// Whether Shadow of the Erdtree is installed: `DLC.bdt` beside eldenring.exe (about 15 GB), larger than the empty
/// stand-in some players make to play online without the DLC.
fn dlc_installed() -> bool {
    std::env::current_exe().ok().and_then(|exe| std::fs::metadata(exe.with_file_name("DLC.bdt")).ok()).is_some_and(|m| m.len() > 1 << 30)
}

/// The hide flags Kratos's helm and body set, so nothing of the character draws under him. Elden Ring hides a piece
/// whose material name carries `#NN#`, and the game's own part files show which: 0-19 and 73 tag the character's skin
/// (FG face, brows and lashes 0, chin 1, nose and cheeks 2; the FC skin 3-19 and 73), 17 an eyepatch, 60-69 every hair
/// section (HR_A), 78-80 the beard (FG_A_30xx). Paramdex's names for 0-9 (fringe, sideburns, crown) are Dark Souls 3's:
/// 0.1.53 set only those, and the hair at the sides of the head (HR_A_0009's #60# and #65#) still showed.
const UNDER_KRATOS: [usize; 34] = [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 60, 61, 62, 63, 64, 65, 66, 67, 68, 69, 73, 78, 79, 80];

/// Kratos's weapons whose models the launcher builds from the player's own Ragnarök install, as sheet weapon id and
/// Elden Ring weapon model id: the launcher writes `mod/parts/wp_a_<id>.partsbnd.dcx` and its `_l` twin, and every
/// carrier row of the weapon is pointed at that model. The armour's ids (9700-9703) are in the armour step below. The
/// Blades of Chaos are two rigid blades on the Backhand Blade's carrier (the whip's without Shadow of the Erdtree,
/// whose lashes lose the reach its animated chain gave), until the animation work.
const WEAPON_MODELS: [(&str, u16); 4] = [("leviathan_axe", 9710), ("blades_of_chaos", 9711), ("draupnir_spear", 9712), ("guardian_shield", 9713)];

/// Ported weapons whose part file holds a pair (the Blades of Chaos: the blade as model 0 and again as model 1).
const PAIRED: [&str; 1] = ["blades_of_chaos"];

/// Place model 1 of a paired weapon (its second blade) through the carrier's WepAbsorpPosParam row: in the other hand
/// while the weapon is held in both hands (two-handing by default is Kratos's stance, so he holds one blade in each
/// hand), and on the back at the other hang spot when one-handed (the left hand then holds the shield) or sheathed;
/// with `back`, both hang there (right, left hang spots) instead of at the type's own.
/// The row is shared with the game's other weapons of the type (every whip, or every Backhand Blade), whose second
/// models move with it.
fn place_second_model(repo: &mut SoloParamRepository, weapon: &str, row: Option<u32>, back: Option<(i16, i16)>) -> String {
    let Some(absorp) = row.and_then(|r| repo.get::<EquipParamWeapon>(r)).map(|r| r.absorp_param_id()) else { return format!("{weapon}: no carrier row for its second model") };
    if absorp < 0 {
        return format!("{weapon}: absorpParamId {absorp}, no row to place its second model with");
    }
    let Some(a) = repo.get_mut::<WepAbsorpPosParam>(absorp as u32) else { return format!("{weapon}: no WepAbsorpPosParam row {absorp}") };
    let was = [a.right_1(), a.left_1(), a.both_1(), a.left_both_1(), a.right_hang_1(), a.left_hang_1()];
    // sheathed, both on `back` (the axe's spots) when given: one blade where a right-hand weapon hangs, the other where a
    // left-hand one does
    if let Some((r, l)) = back {
        a.set_right_hang_0(r);
        a.set_left_hang_0(l);
    }
    let (right, left, right_hang, left_hang) = (a.right_0(), a.left_0(), a.right_hang_0(), a.left_hang_0());
    a.set_both_1(left);
    a.set_left_both_1(right);
    a.set_right_1(left_hang);
    a.set_left_1(right_hang);
    a.set_right_hang_1(left_hang);
    a.set_left_hang_1(right_hang);
    let hung = back.map_or("its own hang spots".to_string(), |(r, l)| format!("the axe's back spots {r} / {l}"));
    format!("{weapon}: sheathed on {hung}; second model placed by WepAbsorpPosParam {absorp}: both hands -> left hand {left}, one hand or sheathed -> the other hang spot {left_hang} (model 1 was right, left, both, leftBoth, rightHang, leftHang {was:?})")
}

/// The one-time weapon report reads BehaviorParam_PC once and AtkParam_Pc per attack; these caps keep it short even on
/// a regulation far larger than the vanilla one (about 13,000 behaviours), since it runs inside the param patch.
const REPORT_MAX_BEHAVIOURS: usize = 200_000;
const REPORT_MAX_ATTACKS: usize = 4096;

/// Sheet weapon id → every row that carries it (today's first, then legacy copies), once the patch has run.
static CARRIER_ROWS: OnceLock<HashMap<&'static str, Vec<u32>>> = OnceLock::new();
/// Skill (SwordArtsParam id) → the animation category its animations are in (600 + its animation type).
static SKILL_CATEGORY: OnceLock<HashMap<i32, i32>> = OnceLock::new();

pub fn carrier_rows(weapon: &str) -> Vec<u32> {
    CARRIER_ROWS.get().and_then(|m| m.get(weapon).cloned()).unwrap_or_default()
}

pub fn skill_category(skill: i32) -> Option<i32> {
    SKILL_CATEGORY.get()?.get(&skill).copied()
}

/// Give every row carrying `weapon` another skill (Ash of War); returns (row, the skill it had).
pub fn set_weapon_skill(weapon: &str, skill: i32) -> Vec<(u32, i32)> {
    let Ok(repo) = (unsafe { SoloParamRepository::instance_mut() }) else { return Vec::new() };
    carrier_rows(weapon)
        .into_iter()
        .filter_map(|row| {
            let r = repo.get_mut::<EquipParamWeapon>(row)?;
            let was = r.sword_arts_param_id();
            r.set_sword_arts_param_id(skill);
            Some((row, was))
        })
        .collect()
}

/// Put back the skills `set_weapon_skill` reported: (row, skill).
pub fn set_rows_skill(rows: &[(u32, i32)]) {
    let Ok(repo) = (unsafe { SoloParamRepository::instance_mut() }) else { return };
    for (row, skill) in rows {
        if let Some(r) = repo.get_mut::<EquipParamWeapon>(*row) {
            r.set_sword_arts_param_id(*skill);
        }
    }
}

pub struct Carriers {
    /// Sheet weapon id → the EquipParamWeapon row id that carries it.
    pub rows: HashMap<&'static str, u32>,
    pub notes: Vec<String>,
}

/// The game fills the param holders some time after start-up; reading one before that panics inside the
/// library, so look first (a missing table is "not ready yet", not an error worth a panic).
fn tables_ready(repo: &SoloParamRepository) -> Result<(), String> {
    for (name, index) in [("EquipParamWeapon", EquipParamWeapon::INDEX), ("CharaInitParam", CharaInitParam::INDEX), ("SwordArtsParam", SwordArtsParam::INDEX)] {
        if !loaded(repo, index) {
            return Err(format!("{name} is not loaded yet"));
        }
    }
    Ok(())
}

/// Whether the param holder at `index` has its table (reading a table that is not there panics inside the library).
fn loaded(repo: &SoloParamRepository, index: u32) -> bool {
    repo.solo_param_holders.get(index as usize).and_then(|h| h.get_res_cap(0)).is_some()
}

/// The weapon part the launcher installs for an Elden Ring weapon model id (Elden Ring loads weapon models by
/// `wp_a_` and the id in four digits).
fn weapon_part_file(model: u16) -> String {
    format!("wp_a_{model:04}.partsbnd.dcx")
}

/// Whether a param field names something: these params write 0 or -1 for "none".
fn named(v: i32) -> bool {
    v != 0 && v != -1
}

/// A weapon row's sword trails as "slot: sfx S on head->tail", leaving out the slots whose dummies are both unused.
fn trace_list(traces: &[(i32, i32, i32)]) -> String {
    let v: Vec<String> = traces.iter().enumerate().filter(|(_, (_, h, t))| named(*h) || named(*t)).map(|(k, (s, h, t))| format!("{k}: sfx {s} on {h}->{t}")).collect();
    if v.is_empty() { "none".into() } else { v.join(", ") }
}

/// A weapon row's resident effects (residentSfxId_n on residentSfx_DmyId_n) as "n: sfx S on D", the used ones only.
fn resident_list(resident: &[(i32, i32)]) -> String {
    let v: Vec<String> = resident.iter().enumerate().filter(|(_, (s, d))| named(*s) || named(*d)).map(|(k, (s, d))| format!("{}: sfx {s} on {d}", k + 1)).collect();
    if v.is_empty() { "none".into() } else { v.join(", ") }
}

/// A WepAbsorpPosParam row's body dummies per weapon model (0-3): right, left, both, leftBoth, rightHang, leftHang.
/// Model 0 is always listed; models 1-3 (a scabbard, the other of a pair) only when they name a dummy.
fn absorp_list(models: &[[i16; 6]; 4]) -> String {
    const NAMES: [&str; 6] = ["right", "left", "both", "leftBoth", "rightHang", "leftHang"];
    let v: Vec<String> = models
        .iter()
        .enumerate()
        .filter(|(k, m)| *k == 0 || m.iter().any(|d| named(*d as i32)))
        .map(|(k, m)| format!("model {k}: {}", NAMES.iter().zip(m).map(|(n, d)| format!("{n} {d}")).collect::<Vec<_>>().join(", ")))
        .collect();
    v.join("; ")
}

/// "id:count" pairs of the dummy ids attacks name, most used first (then by id); -1, an unused end, is left out.
fn histogram(values: impl IntoIterator<Item = i16>) -> String {
    let mut counts: BTreeMap<i16, usize> = BTreeMap::new();
    for v in values.into_iter().filter(|v| *v != -1) {
        *counts.entry(v).or_default() += 1;
    }
    let mut v: Vec<(i16, usize)> = counts.into_iter().collect();
    v.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    if v.is_empty() { "none".into() } else { v.iter().map(|(id, n)| format!("{id}:{n}")).collect::<Vec<_>>().join(" ") }
}

/// The dummies an attack's sixteen hit shapes run between (DmyPoly1 to DmyPoly2; -1 for a sphere or an unused shape).
fn hit_dummies(a: &eldenring::param::ATK_PARAM_ST) -> [i16; 32] {
    [
        a.hit0_dmy_poly1(), a.hit0_dmy_poly2(), a.hit1_dmy_poly1(), a.hit1_dmy_poly2(), a.hit2_dmy_poly1(), a.hit2_dmy_poly2(), a.hit3_dmy_poly1(), a.hit3_dmy_poly2(),
        a.hit4_dmy_poly1(), a.hit4_dmy_poly2(), a.hit5_dmy_poly1(), a.hit5_dmy_poly2(), a.hit6_dmy_poly1(), a.hit6_dmy_poly2(), a.hit7_dmy_poly1(), a.hit7_dmy_poly2(),
        a.hit8_dmy_poly1(), a.hit8_dmy_poly2(), a.hit9_dmy_poly1(), a.hit9_dmy_poly2(), a.hit10_dmy_poly1(), a.hit10_dmy_poly2(), a.hit11_dmy_poly1(), a.hit11_dmy_poly2(),
        a.hit12_dmy_poly1(), a.hit12_dmy_poly2(), a.hit13_dmy_poly1(), a.hit13_dmy_poly2(), a.hit14_dmy_poly1(), a.hit14_dmy_poly2(), a.hit15_dmy_poly1(), a.hit15_dmy_poly2(),
    ]
}

/// What the carriers' models must provide, logged once for the weapon parts' next round. Elden Ring takes a weapon's
/// hit boxes, trails and resident effects from dummies (hit points) in the weapon's own model and places the model on a
/// body dummy its WepAbsorpPosParam row names, so these are the ids a ported model has to carry. It only reads: one
/// line per carrier for its row, one for its placement and one for the hit points its attacks name (a histogram over
/// the AtkParam_Pc rows its BehaviorParam_PC rows reach, weapon-sourced hits only). `vanilla` holds each carrier's model
/// before it was pointed at ours.
fn weapon_report(repo: &SoloParamRepository, rows: &HashMap<&'static str, u32>, vanilla: &HashMap<&str, u16>) -> Vec<String> {
    let mut out = Vec::new();
    let carriers: Vec<(&str, u32, &eldenring::param::EQUIP_PARAM_WEAPON_ST)> =
        WEAPONS.iter().filter(|w| w.template.is_some()).filter_map(|w| { let row = *rows.get(w.id)?; Some((w.id, row, repo.get::<EquipParamWeapon>(row)?)) }).collect();
    let absorp_ready = loaded(repo, WepAbsorpPosParam::INDEX);
    for (weapon, row, r) in &carriers {
        let traces = [
            (r.trace_sfx_id0(), r.trace_dmy_id_head0(), r.trace_dmy_id_tail0()),
            (r.trace_sfx_id1(), r.trace_dmy_id_head1(), r.trace_dmy_id_tail1()),
            (r.trace_sfx_id2(), r.trace_dmy_id_head2(), r.trace_dmy_id_tail2()),
            (r.trace_sfx_id3(), r.trace_dmy_id_head3(), r.trace_dmy_id_tail3()),
            (r.trace_sfx_id4(), r.trace_dmy_id_head4(), r.trace_dmy_id_tail4()),
            (r.trace_sfx_id5(), r.trace_dmy_id_head5(), r.trace_dmy_id_tail5()),
            (r.trace_sfx_id6(), r.trace_dmy_id_head6(), r.trace_dmy_id_tail6()),
            (r.trace_sfx_id7(), r.trace_dmy_id_head7(), r.trace_dmy_id_tail7()),
        ];
        let resident = [
            (r.resident_sfx_id_1(), r.resident_sfx_dmy_id_1()),
            (r.resident_sfx_id_2(), r.resident_sfx_dmy_id_2()),
            (r.resident_sfx_id_3(), r.resident_sfx_dmy_id_3()),
            (r.resident_sfx_id_4(), r.resident_sfx_dmy_id_4()),
        ];
        let was = vanilla.get(weapon).map(|m| m.to_string()).unwrap_or_else(|| "?".into());
        out.push(format!(
            "weapon report {weapon} (row {row}): equipModelId {} (vanilla {was}), absorpParamId {}, behaviorVariationId {}, trails [{}], resident sfx [{}]",
            r.equip_model_id(),
            r.absorp_param_id(),
            r.behavior_variation_id(),
            trace_list(&traces),
            resident_list(&resident)
        ));
        let absorp = r.absorp_param_id();
        let placement = if absorp < 0 {
            "absorpParamId -1: the game's built-in placement".to_string()
        } else if !absorp_ready {
            "WepAbsorpPosParam is not loaded".to_string()
        } else {
            match repo.get::<WepAbsorpPosParam>(absorp as u32) {
                Some(a) => format!(
                    "WepAbsorpPosParam {absorp}: hangPosType {}, isSkeletonBind {}, wepInvisibleType_0 {}; {}",
                    a.hang_pos_type(),
                    a.is_skeleton_bind(),
                    a.wep_invisible_type_0(),
                    absorp_list(&[
                        [a.right_0(), a.left_0(), a.both_0(), a.left_both_0(), a.right_hang_0(), a.left_hang_0()],
                        [a.right_1(), a.left_1(), a.both_1(), a.left_both_1(), a.right_hang_1(), a.left_hang_1()],
                        [a.right_2(), a.left_2(), a.both_2(), a.left_both_2(), a.right_hang_2(), a.left_hang_2()],
                        [a.right_3(), a.left_3(), a.both_3(), a.left_both_3(), a.right_hang_3(), a.left_hang_3()],
                    ])
                ),
                None => format!("WepAbsorpPosParam has no row {absorp}"),
            }
        };
        out.push(format!("weapon report {weapon}: {placement}"));
    }

    // ---- the attacks each carrier's behaviours reach (refType 0 = attack), and the dummies their hits run between --------
    if !loaded(repo, BehaviorParam_PC::INDEX) || !loaded(repo, AtkParam_Pc::INDEX) {
        out.push("weapon report: BehaviorParam_PC or AtkParam_Pc is not loaded, so no hit point histogram".into());
        return out;
    }
    let mut behaviours = vec![0usize; carriers.len()];
    let mut attacks: Vec<Vec<u32>> = vec![Vec::new(); carriers.len()];
    for (id, b) in repo.rows::<BehaviorParam_PC>().take(REPORT_MAX_BEHAVIOURS) {
        if b.ref_type() != 0 || b.ref_id() < 0 {
            continue;
        }
        for (k, (_, _, r)) in carriers.iter().enumerate() {
            // the paramdef says the game takes the variation from the row id (variation * 1000 + judge) rather than
            // the field, so either counts
            let variation = r.behavior_variation_id();
            if (b.variation_id() == variation || (id / 1000) as i64 == variation as i64) && attacks[k].len() < REPORT_MAX_ATTACKS {
                behaviours[k] += 1;
                attacks[k].push(b.ref_id() as u32);
            }
        }
    }
    for (k, (weapon, _, r)) in carriers.iter().enumerate() {
        let ids = &mut attacks[k];
        ids.sort_unstable();
        ids.dedup();
        let mut sources: BTreeMap<u8, usize> = BTreeMap::new();
        let mut missing = 0usize;
        // the hit dummies per hit source (which value means the left hand's weapon is not known, so each is listed)
        let mut dummies: BTreeMap<u8, Vec<i16>> = BTreeMap::new();
        for id in ids.iter() {
            match repo.get::<AtkParam_Pc>(*id) {
                Some(a) => {
                    *sources.entry(a.hit_source_type()).or_default() += 1;
                    dummies.entry(a.hit_source_type()).or_default().extend(hit_dummies(a));
                }
                None => missing += 1,
            }
        }
        let sources = sources.iter().map(|(s, n)| format!("source {s}: {n}")).collect::<Vec<_>>().join(", ");
        let dummies = dummies.into_iter().map(|(s, d)| format!("source {s}: {}", histogram(d))).collect::<Vec<_>>().join("; ");
        out.push(format!(
            "weapon report {weapon}: {} attack behaviours (variation {}) reach {} AtkParam_Pc rows ({sources}; {missing} missing); hit dummies {dummies}",
            behaviours[k],
            r.behavior_variation_id(),
            ids.len(),
        ));
    }
    out
}

pub fn apply() -> Result<Carriers, String> {
    let repo = unsafe { SoloParamRepository::instance_mut() }.map_err(|e| format!("SoloParamRepository: {e}"))?;
    tables_ready(repo)?;
    let mut notes = Vec::new();
    let mut rows: HashMap<&'static str, u32> = HashMap::new();

    // ---- template_scan: first base row of each category (or the DLC's own, see DLC_CARRIERS) -------------------------
    let dlc = dlc_installed();
    notes.push(format!("Shadow of the Erdtree: {}", if dlc { "installed (Game\\DLC.bdt)" } else { "not installed" }));
    let mut used: Vec<u32> = Vec::new();
    for w in WEAPONS.iter().filter(|w| w.template.is_some()) {
        let tpl = ER_TEMPLATES.iter().find(|t| Some(t.id) == w.template).ok_or("template row")?;
        let from_dlc = DLC_CARRIERS.iter().find(|(id, row)| dlc && *id == w.id && !used.contains(row) && repo.get::<EquipParamWeapon>(*row).is_some()).map(|(_, row)| *row);
        let found = from_dlc.or_else(|| {
            repo.rows::<EquipParamWeapon>()
                .filter(|(id, r)| r.wep_type() == tpl.wep_type as u16 && id % 10000 == 0 && !used.contains(id))
                .map(|(id, _)| id)
                .next()
        });
        match found {
            Some(id) => {
                used.push(id);
                rows.insert(w.id, id);
                let wep_type = repo.get::<EquipParamWeapon>(id).map(|r| r.wep_type()).unwrap_or_default();
                notes.push(format!("{} <- EquipParamWeapon {id} (wep_type {wep_type}{})", w.name, if from_dlc.is_some() { ", Shadow of the Erdtree's" } else { "" }));
            }
            None => return Err(format!("no vanilla weapon with wep_type {} for {}", tpl.wep_type, w.name)),
        }
    }

    // ---- param_weapon: rewrite the carriers -----------------------------------------------------------------
    for w in WEAPONS.iter().filter(|w| w.template.is_some()) {
        let id = rows[w.id];
        let Some(r) = repo.get_mut::<EquipParamWeapon>(id) else { return Err(format!("row {id} vanished")) };
        r.set_weight(0.0);
        r.set_correct_strength(0.0);
        r.set_correct_agility(0.0);
        r.set_correct_magic(0.0);
        r.set_correct_faith(0.0);
        let dmg = w.base_damage.clamp(0, u16::MAX as i32) as u16;
        r.set_attack_base_physics(dmg);
        r.set_attack_base_magic(0);
        r.set_attack_base_fire(0);
        r.set_attack_base_thunder(0);
        r.set_attack_base_stamina(0);
        // Skills (Ashes of War) carry Kratos' runic animations: the weapon starts with its first light runic's skill
        // (the bridge swaps in the skill of each runic as it is cast); the shield has none, so the skill button
        // reaches the weapon in the right hand
        let skill = if w.kind == WeaponKind::Shield { NO_SKILL } else { w.light_runic.and_then(|id| RUNICS.iter().find(|r| r.id == id)).map(|r| r.er_skill).filter(|s| *s > 0).unwrap_or(r.sword_arts_param_id()) };
        notes.push(format!("{}: skill {} -> {skill}", w.name, r.sword_arts_param_id()));
        r.set_sword_arts_param_id(skill);
        if w.kind == WeaponKind::Shield {
            // keep the shield's own guard values; Kratos' block is the vanilla block with full cut rates
            r.set_phys_guard_cut_rate(100.0);
            r.set_mag_guard_cut_rate(100.0);
            r.set_fire_guard_cut_rate(100.0);
            r.set_thun_guard_cut_rate(100.0);
            r.set_stamina_guard_def(255);
        }
    }

    // ---- the runics' skills (and Spartan Rage's roar) cost no FP (Kratos' runics run on their own cooldowns) ------------
    let mut skills: Vec<i32> = RUNICS.iter().map(|r| r.er_skill).chain(RAGE.iter().map(|r| r.er_skill)).filter(|s| *s > 0).collect();
    skills.sort();
    skills.dedup();
    let mut category = HashMap::new();
    for id in skills {
        match repo.get_mut::<SwordArtsParam>(id as u32) {
            Some(a) => {
                category.insert(id, 600 + a.sword_arts_type_new() as i32);
                notes.push(format!("skill {id}: animation type {} (a{:03}), FP {} -> 0", a.sword_arts_type_new(), 600 + a.sword_arts_type_new() as u32, a.use_magic_point_l2()));
                a.set_use_magic_point_l1(0);
                a.set_use_magic_point_l2(0);
                a.set_use_magic_point_r1(0);
                a.set_use_magic_point_r2(0);
            }
            None => notes.push(format!("skill {id}: no SwordArtsParam row (its runics play no animation)")),
        }
    }
    let _ = SKILL_CATEGORY.set(category);

    // ---- legacy carriers become copies of today's, so characters made with an earlier version get the same weapon ------
    let mut carriers: HashMap<&'static str, Vec<u32>> = rows.iter().map(|(w, r)| (*w, vec![*r])).collect();
    for (weapon, old) in LEGACY_CARRIERS {
        let Some(&now) = rows.get(weapon) else { continue };
        if old == now {
            continue;
        }
        match (repo.get::<EquipParamWeapon>(now).cloned(), repo.get_mut::<EquipParamWeapon>(old)) {
            (Some(copy), Some(slot)) => {
                *slot = copy;
                if let Some(v) = carriers.get_mut(weapon) {
                    v.push(old);
                }
                notes.push(format!("{weapon}: row {old} (an earlier version's carrier) is now a copy of {now}"));
            }
            _ => notes.push(format!("{weapon}: could not copy row {now} over {old}")),
        }
    }

    // ---- Kratos's armour parts (built by the launcher): body bd_*_9701, arms am_*_9702, legs lg_*_9703 -------------------
    // Each slot takes the first armour row of its category and points its model at our part, only when the part is in
    // the mod folder: an armour pointing at a missing model would show nothing at best.
    let parts = crate::paths::get().home.join("mod").join("parts");
    let mut slot_rows = [-1i32; 3];
    for (k, (category, model, file)) in [(1u8, 9701u16, "bd_m_9701"), (2, 9702, "am_m_9702"), (3, 9703, "lg_m_9703")].into_iter().enumerate() {
        if !parts.join(format!("{file}.partsbnd.dcx")).exists() {
            notes.push(format!("{file} not installed"));
            continue;
        }
        let in_slot = |r: &eldenring::param::EQUIP_PARAM_PROTECTOR_ST| match category {
            1 => r.body_equip(),
            2 => r.arm_equip(),
            _ => r.leg_equip(),
        };
        let first = repo.rows::<EquipParamProtector>().find(|(_, r)| r.protector_category() == category && in_slot(r) && r.equip_model_id() > 0).map(|(id, _)| id);
        match first.and_then(|id| repo.get_mut::<EquipParamProtector>(id).map(|r| (id, r))) {
            Some((id, r)) => {
                let was = r.equip_model_id();
                r.set_equip_model_id(model);
                slot_rows[k] = id as i32;
                notes.push(format!("armour {id} (category {category}, model {was}) now shows {file}"));
            }
            None => notes.push(format!("no armour row of category {category} for {file}")),
        }
    }
    let [body_row, arms_row, legs_row] = slot_rows;
    // Kratos's body piece has his head. The character's own head goes only under a helmet, so the head armour that
    // hides the most (a full helm) shows our empty head piece (`hd_*_9700`) and new characters wear it; its hide flags
    // (face, hair, beard) are also added to the body armour's own, for characters made before.
    let mut helm_row = -1i32;
    if body_row >= 0 {
        type P = eldenring::param::EQUIP_PARAM_PROTECTOR_ST;
        const GET: [fn(&P) -> u8; 96] = [P::invisible_flag_sex_ver00, P::invisible_flag_sex_ver01, P::invisible_flag_sex_ver02, P::invisible_flag_sex_ver03, P::invisible_flag_sex_ver04, P::invisible_flag_sex_ver05, P::invisible_flag_sex_ver06, P::invisible_flag_sex_ver07, P::invisible_flag_sex_ver08, P::invisible_flag_sex_ver09, P::invisible_flag_sex_ver10, P::invisible_flag_sex_ver11, P::invisible_flag_sex_ver12, P::invisible_flag_sex_ver13, P::invisible_flag_sex_ver14, P::invisible_flag_sex_ver15, P::invisible_flag_sex_ver16, P::invisible_flag_sex_ver17, P::invisible_flag_sex_ver18, P::invisible_flag_sex_ver19, P::invisible_flag_sex_ver20, P::invisible_flag_sex_ver21, P::invisible_flag_sex_ver22, P::invisible_flag_sex_ver23, P::invisible_flag_sex_ver24, P::invisible_flag_sex_ver25, P::invisible_flag_sex_ver26, P::invisible_flag_sex_ver27, P::invisible_flag_sex_ver28, P::invisible_flag_sex_ver29, P::invisible_flag_sex_ver30, P::invisible_flag_sex_ver31, P::invisible_flag_sex_ver32, P::invisible_flag_sex_ver33, P::invisible_flag_sex_ver34, P::invisible_flag_sex_ver35, P::invisible_flag_sex_ver36, P::invisible_flag_sex_ver37, P::invisible_flag_sex_ver38, P::invisible_flag_sex_ver39, P::invisible_flag_sex_ver40, P::invisible_flag_sex_ver41, P::invisible_flag_sex_ver42, P::invisible_flag_sex_ver43, P::invisible_flag_sex_ver44, P::invisible_flag_sex_ver45, P::invisible_flag_sex_ver46, P::invisible_flag_sex_ver47, P::invisible_flag_sex_ver48, P::invisible_flag_sex_ver49, P::invisible_flag_sex_ver50, P::invisible_flag_sex_ver51, P::invisible_flag_sex_ver52, P::invisible_flag_sex_ver53, P::invisible_flag_sex_ver54, P::invisible_flag_sex_ver55, P::invisible_flag_sex_ver56, P::invisible_flag_sex_ver57, P::invisible_flag_sex_ver58, P::invisible_flag_sex_ver59, P::invisible_flag_sex_ver60, P::invisible_flag_sex_ver61, P::invisible_flag_sex_ver62, P::invisible_flag_sex_ver63, P::invisible_flag_sex_ver64, P::invisible_flag_sex_ver65, P::invisible_flag_sex_ver66, P::invisible_flag_sex_ver67, P::invisible_flag_sex_ver68, P::invisible_flag_sex_ver69, P::invisible_flag_sex_ver70, P::invisible_flag_sex_ver71, P::invisible_flag_sex_ver72, P::invisible_flag_sex_ver73, P::invisible_flag_sex_ver74, P::invisible_flag_sex_ver75, P::invisible_flag_sex_ver76, P::invisible_flag_sex_ver77, P::invisible_flag_sex_ver78, P::invisible_flag_sex_ver79, P::invisible_flag_sex_ver80, P::invisible_flag_sex_ver81, P::invisible_flag_sex_ver82, P::invisible_flag_sex_ver83, P::invisible_flag_sex_ver84, P::invisible_flag_sex_ver85, P::invisible_flag_sex_ver86, P::invisible_flag_sex_ver87, P::invisible_flag_sex_ver88, P::invisible_flag_sex_ver89, P::invisible_flag_sex_ver90, P::invisible_flag_sex_ver91, P::invisible_flag_sex_ver92, P::invisible_flag_sex_ver93, P::invisible_flag_sex_ver94, P::invisible_flag_sex_ver95];
        const SET: [fn(&mut P, u8); 96] = [P::set_invisible_flag_sex_ver00, P::set_invisible_flag_sex_ver01, P::set_invisible_flag_sex_ver02, P::set_invisible_flag_sex_ver03, P::set_invisible_flag_sex_ver04, P::set_invisible_flag_sex_ver05, P::set_invisible_flag_sex_ver06, P::set_invisible_flag_sex_ver07, P::set_invisible_flag_sex_ver08, P::set_invisible_flag_sex_ver09, P::set_invisible_flag_sex_ver10, P::set_invisible_flag_sex_ver11, P::set_invisible_flag_sex_ver12, P::set_invisible_flag_sex_ver13, P::set_invisible_flag_sex_ver14, P::set_invisible_flag_sex_ver15, P::set_invisible_flag_sex_ver16, P::set_invisible_flag_sex_ver17, P::set_invisible_flag_sex_ver18, P::set_invisible_flag_sex_ver19, P::set_invisible_flag_sex_ver20, P::set_invisible_flag_sex_ver21, P::set_invisible_flag_sex_ver22, P::set_invisible_flag_sex_ver23, P::set_invisible_flag_sex_ver24, P::set_invisible_flag_sex_ver25, P::set_invisible_flag_sex_ver26, P::set_invisible_flag_sex_ver27, P::set_invisible_flag_sex_ver28, P::set_invisible_flag_sex_ver29, P::set_invisible_flag_sex_ver30, P::set_invisible_flag_sex_ver31, P::set_invisible_flag_sex_ver32, P::set_invisible_flag_sex_ver33, P::set_invisible_flag_sex_ver34, P::set_invisible_flag_sex_ver35, P::set_invisible_flag_sex_ver36, P::set_invisible_flag_sex_ver37, P::set_invisible_flag_sex_ver38, P::set_invisible_flag_sex_ver39, P::set_invisible_flag_sex_ver40, P::set_invisible_flag_sex_ver41, P::set_invisible_flag_sex_ver42, P::set_invisible_flag_sex_ver43, P::set_invisible_flag_sex_ver44, P::set_invisible_flag_sex_ver45, P::set_invisible_flag_sex_ver46, P::set_invisible_flag_sex_ver47, P::set_invisible_flag_sex_ver48, P::set_invisible_flag_sex_ver49, P::set_invisible_flag_sex_ver50, P::set_invisible_flag_sex_ver51, P::set_invisible_flag_sex_ver52, P::set_invisible_flag_sex_ver53, P::set_invisible_flag_sex_ver54, P::set_invisible_flag_sex_ver55, P::set_invisible_flag_sex_ver56, P::set_invisible_flag_sex_ver57, P::set_invisible_flag_sex_ver58, P::set_invisible_flag_sex_ver59, P::set_invisible_flag_sex_ver60, P::set_invisible_flag_sex_ver61, P::set_invisible_flag_sex_ver62, P::set_invisible_flag_sex_ver63, P::set_invisible_flag_sex_ver64, P::set_invisible_flag_sex_ver65, P::set_invisible_flag_sex_ver66, P::set_invisible_flag_sex_ver67, P::set_invisible_flag_sex_ver68, P::set_invisible_flag_sex_ver69, P::set_invisible_flag_sex_ver70, P::set_invisible_flag_sex_ver71, P::set_invisible_flag_sex_ver72, P::set_invisible_flag_sex_ver73, P::set_invisible_flag_sex_ver74, P::set_invisible_flag_sex_ver75, P::set_invisible_flag_sex_ver76, P::set_invisible_flag_sex_ver77, P::set_invisible_flag_sex_ver78, P::set_invisible_flag_sex_ver79, P::set_invisible_flag_sex_ver80, P::set_invisible_flag_sex_ver81, P::set_invisible_flag_sex_ver82, P::set_invisible_flag_sex_ver83, P::set_invisible_flag_sex_ver84, P::set_invisible_flag_sex_ver85, P::set_invisible_flag_sex_ver86, P::set_invisible_flag_sex_ver87, P::set_invisible_flag_sex_ver88, P::set_invisible_flag_sex_ver89, P::set_invisible_flag_sex_ver90, P::set_invisible_flag_sex_ver91, P::set_invisible_flag_sex_ver92, P::set_invisible_flag_sex_ver93, P::set_invisible_flag_sex_ver94, P::set_invisible_flag_sex_ver95];
        let hides = |r: &P| GET.iter().filter(|g| g(r) != 0).count();
        let helm = repo.rows::<EquipParamProtector>().filter(|(_, r)| r.protector_category() == 0 && r.head_equip() && r.equip_model_id() > 0).max_by_key(|(id, r)| (hides(r), std::cmp::Reverse(*id))).map(|(id, r)| (id, GET.map(|g| g(r))));
        if let Some((helm_id, _)) = helm {
            if parts.join("hd_m_9700.partsbnd.dcx").exists() {
                // The character's face still draws under it (Elden Ring draws it under every helmet; the face scale is
                // 1.0 on all 232 helmets that set it and changes nothing): Kratos's head is fitted over it instead.
                if let Some(r) = repo.get_mut::<EquipParamProtector>(helm_id) {
                    let was = r.equip_model_id();
                    r.set_equip_model_id(9700);
                    helm_row = helm_id as i32;
                    notes.push(format!("helm {helm_id} (model {was}) now shows hd_m_9700, Kratos's empty head piece"));
                }
            } else {
                notes.push("hd_m_9700 not installed".into());
            }
        }
        // and the character's hair, beard and skin (`UNDER_KRATOS`), which showed at the sides of Kratos's head. A flag
        // already set keeps its value; ours are written as 1, the value the game's own rows use.
        if let Some(r) = helm.and_then(|(id, _)| repo.get_mut::<EquipParamProtector>(id)) {
            for k in UNDER_KRATOS {
                if GET[k](r) == 0 {
                    SET[k](r, 1);
                }
            }
        }
        match (helm, repo.get_mut::<EquipParamProtector>(body_row as u32)) {
            (Some((helm_id, flags)), Some(body)) => {
                for k in 0..96 {
                    if GET[k](body) == 0 {
                        SET[k](body, if flags[k] != 0 { flags[k] } else { UNDER_KRATOS.contains(&k) as u8 });
                    }
                }
                let on: Vec<usize> = (0..96).filter(|k| flags[*k] != 0).collect();
                let all: Vec<usize> = (0..96).filter(|k| GET[*k](body) != 0).collect();
                notes.push(format!("armour {body_row} also hides the head (helm {helm_id}'s hide flags {on:?}) and the character's hair, beard and skin {UNDER_KRATOS:?}; it now hides {all:?}"));
            }
            _ => notes.push("no helm to take head-hiding flags from".into()),
        }
    }

    // ---- Kratos's weapon models (built by the launcher from the player's Ragnarök install): wp_a_9710 to wp_a_9713 -------
    // Every row carrying a ported weapon (today's carrier and the legacy copies) shows our model, only when its part is in
    // the mod folder: the launcher leaves out a weapon whose build failed or whose geometry looked wrong, and its carrier
    // keeps the vanilla model.
    let vanilla: HashMap<&str, u16> = rows.iter().filter_map(|(w, id)| Some((*w, repo.get::<EquipParamWeapon>(*id)?.equip_model_id()))).collect();
    for (weapon, model) in WEAPON_MODELS {
        let file = weapon_part_file(model);
        // the low-detail twin too: a model without it would have nothing to draw at a distance
        let low = file.replace(".partsbnd", "_l.partsbnd");
        if !parts.join(&file).exists() || !parts.join(&low).exists() {
            notes.push(format!("{file} or {low} not installed: {weapon} keeps its vanilla model"));
            continue;
        }
        let mut was = Vec::new();
        for row in carriers.get(weapon).cloned().unwrap_or_default() {
            if let Some(r) = repo.get_mut::<EquipParamWeapon>(row) {
                was.push(format!("row {row} model {}", r.equip_model_id()));
                r.set_equip_model_id(model);
            }
        }
        if was.is_empty() {
            notes.push(format!("{file} is installed but {weapon} has no carrier row to show it"));
        } else {
            notes.push(format!("{weapon} now shows wp_a_{model:04} (was: {})", was.join(", ")));
        }
        if PAIRED.contains(&weapon) {
            // on his back where the axe hangs (Elden Ring hung them at his hips, by his hands: 0.1.97's "stretched
            // fingers" were the blades seen edge-on)
            let back = rows.get("leviathan_axe").and_then(|r| repo.get::<EquipParamWeapon>(*r)).map(|r| r.absorp_param_id()).filter(|a| *a >= 0).and_then(|a| repo.get::<WepAbsorpPosParam>(a as u32)).map(|a| (a.right_hang_0(), a.left_hang_0()));
            notes.push(place_second_model(repo, weapon, carriers.get(weapon).and_then(|r| r.first().copied()), back));
        }
    }
    // ---- testing aid: weapons that draw nothing (`debug_empty_hands`), with the unarmed row's model (what the game shows
    // for an empty hand), so a weapon model at his hand can be told from his hand ----------------------------------------
    let empty = crate::settings::get().debug_empty_hands;
    if empty > 0 {
        match repo.get::<EquipParamWeapon>(110000).map(|r| r.equip_model_id()) {
            Some(none) => {
                let mut hidden = Vec::new();
                for (weapon, _) in WEAPON_MODELS {
                    if empty == 1 && weapon == "leviathan_axe" {
                        continue;
                    }
                    for row in carriers.get(weapon).cloned().unwrap_or_default() {
                        if let Some(r) = repo.get_mut::<EquipParamWeapon>(row) {
                            r.set_equip_model_id(none);
                        }
                    }
                    hidden.push(weapon);
                }
                notes.push(format!("debug_empty_hands \"{}\": {hidden:?} show the unarmed model {none}", crate::settings::EMPTY_HANDS[empty.min(2) as usize]));
            }
            None => notes.push("debug_empty_hands: no unarmed row 110000; every weapon shows".into()),
        }
    }
    // ---- the ten starting classes become Kratos ----------------------------------------------------------------------------
    let (axe, blades, spear, shield) = (rows["leviathan_axe"] as i32, rows["blades_of_chaos"] as i32, rows["draupnir_spear"] as i32, rows["guardian_shield"] as i32);
    let vigor = gowring_core::Data::get().loadout.vigor.clamp(1, 99) as u8;
    let mut classes = 0;
    for id in 3000u32..=3009 {
        if let Some(c) = repo.get_mut::<CharaInitParam>(id) {
            // Elden Ring derives max HP from Vigor and resets any direct write within a frame, so Kratos' toughness is his starting Vigor
            c.set_base_vit(vigor);
            c.set_equip_wep_right(axe);
            c.set_equip_subwep_right(blades);
            c.set_equip_subwep_right3(spear);
            c.set_equip_wep_left(shield);
            c.set_equip_subwep_left(-1);
            c.set_equip_subwep_left3(-1);
            // Kratos wears his own parts when they are installed (the helm is an empty piece that hides the head)
            c.set_equip_helm(helm_row);
            c.set_equip_armer(body_row);
            c.set_equip_gaunt(arms_row);
            c.set_equip_leg(legs_row);
            classes += 1;
        }
    }
    notes.push(format!("{classes} starting classes rewritten to the Kratos kit (vigor {vigor})"));
    if classes == 0 {
        return Err("no CharaInitParam rows 3000..3009 found".into());
    }
    // Reading the carriers' hit points must never cost the patch itself: a panic in the report only drops the report.
    // It runs once, after the patch has succeeded (a failed patch is retried).
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| weapon_report(repo, &rows, &vanilla))) {
        Ok(lines) => notes.extend(lines),
        Err(_) => notes.push("weapon report panicked and was skipped".into()),
    }
    let _ = CARRIER_ROWS.set(carriers);
    Ok(Carriers { rows, notes })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn weapon_parts_are_named_by_their_four_digit_model() {
        assert_eq!(weapon_part_file(9710), "wp_a_9710.partsbnd.dcx");
        assert_eq!(weapon_part_file(120), "wp_a_0120.partsbnd.dcx");
        // every weapon has its own model, the Blades included
        assert!(WEAPON_MODELS.iter().all(|(_, m)| (9710..=9713).contains(m)));
        assert!(WEAPON_MODELS.contains(&("blades_of_chaos", 9711)));
        assert!(PAIRED.iter().all(|p| WEAPON_MODELS.iter().any(|(w, _)| w == p)));
        let mut models: Vec<u16> = WEAPON_MODELS.iter().map(|(_, m)| *m).collect();
        models.sort_unstable();
        models.dedup();
        assert_eq!(models.len(), WEAPON_MODELS.len());
        // every ported weapon is a sheet weapon with a carrier row
        assert!(WEAPON_MODELS.iter().all(|(w, _)| WEAPONS.iter().any(|s| s.id == *w && s.template.is_some())));
        // the DLC's carriers stand in for sheet weapons, base rows; a character made on the whip gets the Backhand Blade
        assert!(DLC_CARRIERS.iter().all(|(w, row)| WEAPONS.iter().any(|s| s.id == *w && s.template.is_some()) && row % 10000 == 0));
        assert!(DLC_CARRIERS.iter().all(|(w, _)| LEGACY_CARRIERS.contains(&(*w, 20000000))));
    }

    #[test]
    fn trails_and_resident_effects_list_only_the_used_slots() {
        let mut traces = [(0, -1, -1); 8];
        traces[0] = (400, 300, 301);
        traces[3] = (401, 0, 302);
        assert_eq!(trace_list(&traces), "0: sfx 400 on 300->301, 3: sfx 401 on 0->302");
        assert_eq!(trace_list(&[(5, 0, -1); 8]), "none");
        assert_eq!(resident_list(&[(0, -1), (1234, 200), (-1, 0), (0, 0)]), "2: sfx 1234 on 200");
        assert_eq!(resident_list(&[(-1, -1); 4]), "none");
    }

    #[test]
    fn placement_lists_model_0_and_only_the_other_models_that_name_a_dummy() {
        let mut models = [[-1i16; 6]; 4];
        models[0] = [20, 21, 20, 21, -1, 2021];
        assert_eq!(absorp_list(&models), "model 0: right 20, left 21, both 20, leftBoth 21, rightHang -1, leftHang 2021");
        models[2][4] = 2020;
        assert_eq!(absorp_list(&models), "model 0: right 20, left 21, both 20, leftBoth 21, rightHang -1, leftHang 2021; model 2: right -1, left -1, both -1, leftBoth -1, rightHang 2020, leftHang -1");
    }

    #[test]
    fn histogram_counts_ids_most_used_first_without_unused_ends() {
        assert_eq!(histogram([100, 120, -1, 100, 110, 120, 100, -1]), "100:3 120:2 110:1");
        // ties go by id
        assert_eq!(histogram([130, 110]), "110:1 130:1");
        assert_eq!(histogram([-1, -1]), "none");
        assert_eq!(histogram(std::iter::empty()), "none");
    }

    #[test]
    fn named_treats_0_and_minus_1_as_none() {
        assert!(!named(0) && !named(-1));
        assert!(named(100) && named(-2) && named(10100));
    }
}
