//! hooks.template_scan / param_weapon — Kratos' four carrier weapons are vanilla Elden Ring weapon
//! rows found by category and rewritten in memory (fixed damage, no stat scaling, no weight), and the
//! ten starting classes are rewritten to start with them: a new character on the mashup's own save is
//! born as Kratos, with the engine itself granting and equipping the kit.

use eldenring::cs::{CharaInitParam, EquipParamWeapon, SoloParam, SoloParamRepository, SwordArtsParam};
use fromsoftware_shared::FromStatic;
use gowring_core::sheets::{WeaponKind, ER_TEMPLATES, RUNICS, WEAPONS};
use std::collections::HashMap;
use std::sync::OnceLock;

/// Elden Ring's "No Skill": the shield gets it, so the skill button reaches the weapon in Kratos' right hand.
const NO_SKILL: i32 = 10;

/// Sheet weapon id → carrier row, once the patch has run (read by the bridge when a runic needs its animation).
static CARRIER_ROWS: OnceLock<HashMap<&'static str, u32>> = OnceLock::new();

pub fn carrier_row(weapon: &str) -> Option<u32> {
    CARRIER_ROWS.get()?.get(weapon).copied()
}

/// Give a carrier weapon row another skill (Ash of War); returns the one it had.
pub fn set_weapon_skill(row: u32, skill: i32) -> Option<i32> {
    let repo = unsafe { SoloParamRepository::instance_mut() }.ok()?;
    let r = repo.get_mut::<EquipParamWeapon>(row)?;
    let was = r.sword_arts_param_id();
    r.set_sword_arts_param_id(skill);
    Some(was)
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

    // ---- the runics' skills cost no FP (Kratos' runics run on their own cooldowns) -----------------------------------------
    let mut skills: Vec<i32> = RUNICS.iter().map(|r| r.er_skill).filter(|s| *s > 0).collect();
    skills.sort();
    skills.dedup();
    for id in skills {
        match repo.get_mut::<SwordArtsParam>(id as u32) {
            Some(a) => {
                notes.push(format!("skill {id}: animation type {} (a{:03}), FP {} -> 0", a.sword_arts_type_new(), 600 + a.sword_arts_type_new() as u32, a.use_magic_point_l2()));
                a.set_use_magic_point_l1(0);
                a.set_use_magic_point_l2(0);
                a.set_use_magic_point_r1(0);
                a.set_use_magic_point_r2(0);
            }
            None => notes.push(format!("skill {id}: no SwordArtsParam row (its runics play no animation)")),
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
            // Kratos fights bare-chested: no armour pieces
            c.set_equip_helm(-1);
            c.set_equip_armer(-1);
            c.set_equip_gaunt(-1);
            c.set_equip_leg(-1);
            classes += 1;
        }
    }
    notes.push(format!("{classes} starting classes rewritten to the Kratos kit (vigor {vigor})"));
    if classes == 0 {
        return Err("no CharaInitParam rows 3000..3009 found".into());
    }
    let _ = CARRIER_ROWS.set(rows.clone());
    Ok(Carriers { rows, notes })
}
