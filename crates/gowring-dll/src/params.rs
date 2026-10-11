//! hooks.template_scan / param_weapon — Kratos' four carrier weapons are vanilla Elden Ring weapon
//! rows found by category and rewritten in memory (fixed damage, no stat scaling, no weight), and the
//! ten starting classes are rewritten to start with them: a new character on the mashup's own save is
//! born as Kratos, with the engine itself granting and equipping the kit.

use eldenring::cs::{CharaInitParam, EquipParamWeapon, SoloParam, SoloParamRepository};
use fromsoftware_shared::FromStatic;
use gowring_core::sheets::{WeaponKind, ER_TEMPLATES, WEAPONS};
use std::collections::HashMap;

pub struct Carriers {
    /// Sheet weapon id → the EquipParamWeapon row id that carries it.
    pub rows: HashMap<&'static str, u32>,
    pub notes: Vec<String>,
}

/// The game fills the param holders some time after start-up; reading one before that panics inside the
/// library, so look first (a missing table is "not ready yet", not an error worth a panic).
fn tables_ready(repo: &SoloParamRepository) -> Result<(), String> {
    for (name, index) in [("EquipParamWeapon", EquipParamWeapon::INDEX), ("CharaInitParam", CharaInitParam::INDEX)] {
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
        if w.kind == WeaponKind::Shield {
            // keep the shield's own guard values; Kratos' block is the vanilla block with full cut rates
            r.set_phys_guard_cut_rate(100.0);
            r.set_mag_guard_cut_rate(100.0);
            r.set_fire_guard_cut_rate(100.0);
            r.set_thun_guard_cut_rate(100.0);
            r.set_stamina_guard_def(255);
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
    Ok(Carriers { rows, notes })
}
