//! hooks.template_scan / param_weapon — Kratos' four carrier weapons are vanilla Elden Ring weapon
//! rows found by category and rewritten in memory (fixed damage, no stat scaling, no weight), and the
//! ten starting classes are rewritten to start with them: a new character on the mashup's own save is
//! born as Kratos, with the engine itself granting and equipping the kit.

use eldenring::cs::{CharaInitParam, EquipParamProtector, EquipParamWeapon, SoloParam, SoloParamRepository, SwordArtsParam};
use fromsoftware_shared::FromStatic;
use gowring_core::sheets::{WeaponKind, ER_TEMPLATES, RAGE, RUNICS, WEAPONS};
use std::collections::HashMap;
use std::sync::OnceLock;

/// Elden Ring's "No Skill": the shield gets it, so the skill button reaches the weapon in Kratos' right hand.
const NO_SKILL: i32 = 10;

/// Rows earlier versions used as carriers. A character made then still holds that weapon, so the row is turned
/// into a copy of today's carrier: 0.1.0-0.1.32 gave the Blades the Caestus (fists, by a wrong weapon type).
const LEGACY_CARRIERS: [(&str, u32); 1] = [("blades_of_chaos", 21000000)];

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
        let ready = repo.solo_param_holders.get(index as usize).and_then(|h| h.get_res_cap(0)).is_some();
        if !ready {
            return Err(format!("{name} is not loaded yet"));
        }
    }
    Ok(())
}

pub fn apply() -> Result<Carriers, String> {
    let repo = unsafe { SoloParamRepository::instance_mut() }.map_err(|e| format!("SoloParamRepository: {e}"))?;
    tables_ready(repo)?;
    let mut notes = Vec::new();
    let mut rows: HashMap<&'static str, u32> = HashMap::new();

    // ---- template_scan: first base row of each category ---------------------------------------------
    let mut used: Vec<u32> = Vec::new();
    for w in WEAPONS.iter().filter(|w| w.template.is_some()) {
        let tpl = ER_TEMPLATES.iter().find(|t| Some(t.id) == w.template).ok_or("template row")?;
        let found = repo
            .rows::<EquipParamWeapon>()
            .filter(|(id, r)| r.wep_type() == tpl.wep_type as u16 && id % 10000 == 0 && !used.contains(id))
            .map(|(id, _)| id)
            .next();
        match found {
            Some(id) => {
                used.push(id);
                rows.insert(w.id, id);
                notes.push(format!("{} <- EquipParamWeapon {id} (wep_type {})", w.name, tpl.wep_type));
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

    // ---- Kratos's body: a body armour whose model is the part the launcher built (bd_m_9701 / bd_f_9701) ------------------
    // Only when the part is in the mod folder: an armour pointing at a missing model would show nothing at best.
    let part = crate::paths::get().home.join("mod").join("parts").join("bd_m_9701.partsbnd.dcx");
    let mut body_row: i32 = -1;
    if part.exists() {
        let first_body = repo.rows::<EquipParamProtector>().find(|(_, r)| r.protector_category() == 1 && r.body_equip() && r.equip_model_id() > 0).map(|(id, _)| id);
        match first_body.and_then(|id| repo.get_mut::<EquipParamProtector>(id).map(|r| (id, r))) {
            Some((id, r)) => {
                let was = r.equip_model_id();
                r.set_equip_model_id(9701);
                body_row = id as i32;
                notes.push(format!("body armour {id} (model {was}) now shows Kratos (model 9701)"));
            }
            None => notes.push("no body armour row found for Kratos's body part".into()),
        }
    } else {
        notes.push(format!("Kratos's body part not installed ({} missing)", part.display()));
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
            // Kratos fights bare-chested: no armour pieces, but his body when it is installed
            c.set_equip_helm(-1);
            c.set_equip_armer(body_row);
            c.set_equip_gaunt(-1);
            c.set_equip_leg(-1);
            classes += 1;
        }
    }
    notes.push(format!("{classes} starting classes rewritten to the Kratos kit (vigor {vigor})"));
    if classes == 0 {
        return Err("no CharaInitParam rows 3000..3009 found".into());
    }
    let _ = CARRIER_ROWS.set(carriers);
    Ok(Carriers { rows, notes })
}
