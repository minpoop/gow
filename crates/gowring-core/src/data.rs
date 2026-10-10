//! Resolved view of the sheets: id lookups, class tables, binding chords.

use crate::sheets::*;
use crate::types::ActionBits;
use std::collections::HashMap;
use std::sync::OnceLock;

pub struct Data {
    weapons: HashMap<&'static str, &'static WeaponDef>,
    moves: HashMap<&'static str, &'static MoveDef>,
    runics: HashMap<&'static str, &'static RunicDef>,
    assets: HashMap<&'static str, &'static AssetDef>,
    bindings: HashMap<&'static str, &'static BindingDef>,
    triggers_by_event: HashMap<MimirTriggerEvent, Vec<&'static MimirTriggerDef>>,
    pub rage: &'static RageDef,
    pub camera: &'static CameraDef,
    pub loadout: &'static LoadoutDef,
    pub tuning: &'static TuningDef,
    pub mimir: &'static MimirSettingsDef,
    /// Equipable right-hand weapons by slot index 0..=2.
    right_slots: [&'static WeaponDef; 3],
    pub shield: &'static WeaponDef,
    pub hands: &'static WeaponDef,
}

static DATA: OnceLock<Data> = OnceLock::new();

impl Data {
    /// The process-wide table built from the generated sheets.
    pub fn get() -> &'static Data {
        DATA.get_or_init(Data::build)
    }

    fn build() -> Data {
        let weapons: HashMap<_, _> = WEAPONS.iter().map(|w| (w.id, w)).collect();
        let moves = MOVES.iter().map(|m| (m.id, m)).collect();
        let runics = RUNICS.iter().map(|m| (m.id, m)).collect();
        let assets = ASSETS.iter().map(|m| (m.id, m)).collect();
        let bindings = BINDINGS.iter().map(|m| (m.id, m)).collect();
        let mut triggers_by_event: HashMap<MimirTriggerEvent, Vec<&'static MimirTriggerDef>> = HashMap::new();
        for t in MIMIR_TRIGGERS {
            triggers_by_event.entry(t.event).or_default().push(t);
        }
        let by_slot = |s: WeaponSlot| *weapons.values().find(|w| w.slot == s).expect("weapon for slot");
        let right_slots = [by_slot(WeaponSlot::Right1), by_slot(WeaponSlot::Right2), by_slot(WeaponSlot::Right3)];
        let shield = by_slot(WeaponSlot::Left1);
        let hands = weapons.values().find(|w| w.kind == WeaponKind::Hands).copied().expect("hands");
        Data {
            weapons,
            moves,
            runics,
            assets,
            bindings,
            triggers_by_event,
            rage: &RAGE[0],
            camera: &CAMERA[0],
            loadout: &LOADOUT[0],
            tuning: &TUNING[0],
            mimir: &MIMIR_SETTINGS[0],
            right_slots,
            shield,
            hands,
        }
    }

    pub fn weapon(&self, id: &str) -> &'static WeaponDef {
        self.weapons.get(id).unwrap_or_else(|| panic!("unknown weapon `{id}`"))
    }
    pub fn mv(&self, id: &str) -> &'static MoveDef {
        self.moves.get(id).unwrap_or_else(|| panic!("unknown move `{id}`"))
    }
    pub fn runic(&self, id: &str) -> &'static RunicDef {
        self.runics.get(id).unwrap_or_else(|| panic!("unknown runic `{id}`"))
    }
    pub fn asset(&self, id: &str) -> &'static AssetDef {
        self.assets.get(id).unwrap_or_else(|| panic!("unknown asset `{id}`"))
    }
    pub fn binding(&self, id: &str) -> &'static BindingDef {
        self.bindings.get(id).unwrap_or_else(|| panic!("unknown binding `{id}`"))
    }
    pub fn right_weapon(&self, slot: u8) -> &'static WeaponDef {
        self.right_slots[(slot as usize).min(2)]
    }
    pub fn triggers(&self, ev: MimirTriggerEvent) -> &[&'static MimirTriggerDef] {
        self.triggers_by_event.get(&ev).map(Vec::as_slice).unwrap_or(&[])
    }
    pub fn all_runics(&self) -> impl Iterator<Item = &'static RunicDef> {
        RUNICS.iter()
    }

    /// Enemy class for a max HP (the first class whose ceiling covers it) or the boss class.
    pub fn enemy_class(&self, max_hp: f32, boss: bool) -> &'static EnemyClassDef {
        if boss {
            return ENEMY_CLASSES.iter().find(|c| c.health_bar == EnemyClassHealthBar::Boss).unwrap();
        }
        ENEMY_CLASSES
            .iter()
            .find(|c| c.health_bar != EnemyClassHealthBar::Boss && (c.hp_ceiling as f32) >= max_hp)
            .unwrap_or_else(|| ENEMY_CLASSES.iter().rev().nth(1).unwrap())
    }

    /// The ER action bits of a binding's chord.
    pub fn chord_bits(&self, b: &BindingDef) -> ActionBits {
        b.chord.iter().fold(ActionBits::NONE, |acc, a| acc.with(*a))
    }
    pub fn blocked_bits(&self, b: &BindingDef) -> ActionBits {
        b.blocks.iter().fold(ActionBits::NONE, |acc, a| acc.with(*a))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_reference_resolves() {
        let d = Data::get();
        for m in MOVES {
            d.weapon(m.weapon);
            d.binding(m.binding);
            for n in [m.next_light, m.next_heavy].into_iter().flatten() {
                d.mv(n);
            }
            for a in [m.sfx, m.sfx_end].into_iter().flatten() {
                d.asset(a);
            }
        }
        for w in WEAPONS {
            for l in w.lights.iter().chain(w.heavies).chain(w.specials) {
                assert_eq!(d.mv(l).weapon, w.id);
            }
            for r in [w.light_runic, w.heavy_runic].into_iter().flatten() {
                assert_eq!(d.runic(r).weapon, w.id);
            }
        }
        for t in MIMIR_TRIGGERS {
            d.asset(t.voice);
        }
        assert_eq!(d.right_weapon(0).id, "leviathan_axe");
        assert_eq!(d.right_weapon(1).id, "blades_of_chaos");
        assert_eq!(d.right_weapon(2).id, "draupnir_spear");
        assert_eq!(d.shield.id, "guardian_shield");
    }

    #[test]
    fn classes_by_hp() {
        let d = Data::get();
        assert_eq!(d.enemy_class(200.0, false).id, "fodder");
        assert_eq!(d.enemy_class(900.0, false).id, "soldier");
        assert_eq!(d.enemy_class(4000.0, false).id, "elite");
        assert_eq!(d.enemy_class(50_000.0, false).id, "elite");
        assert_eq!(d.enemy_class(1.0, true).id, "boss");
    }

    #[test]
    fn chords_use_er_bits() {
        let d = Data::get();
        let rage = d.binding("b_rage");
        let bits = d.chord_bits(rage);
        assert!(bits.has(ErAction::L3) && bits.has(ErAction::R3));
        assert!(!bits.has(ErAction::R1));
    }
}
