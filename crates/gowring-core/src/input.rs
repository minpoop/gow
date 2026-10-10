//! Bindings → booleans. Everything is expressed in Elden Ring's own action bits, so the same
//! code serves keyboard, XInput and DualSense.

use crate::data::Data;
use crate::sheets::{BindingDef, BindingMode};
use crate::types::{ActionBits, Frame};

#[derive(Clone, Copy, Debug, Default)]
pub struct Inputs {
    pub light: bool,
    pub heavy: bool,
    pub guard_pressed: bool,
    pub guard_held: bool,
    pub dodge: bool,
    pub aim_held: bool,
    pub throw_pressed: bool,
    pub recall_pressed: bool,
    pub rage_pressed: bool,
    pub finisher_pressed: bool,
    pub runic_light: bool,
    pub runic_heavy: bool,
    pub swap: bool,
}

/// True when the binding's chord is completely held.
pub fn chord_down(d: &Data, b: &BindingDef, f: &Frame) -> bool {
    let bits = d.chord_bits(b);
    !bits.is_empty() && (f.actions_down.0 & bits.0) == bits.0
}

/// True on the frame the chord becomes complete: all bits held and at least one just pressed.
pub fn chord_edge(d: &Data, b: &BindingDef, f: &Frame) -> bool {
    let bits = d.chord_bits(b);
    chord_down(d, b, f) && (f.actions_pressed.0 & bits.0) != 0
}

pub fn read(d: &Data, f: &Frame) -> Inputs {
    let b = |id: &str| d.binding(id);
    let edge = |id: &str| {
        let bd = b(id);
        match bd.mode {
            BindingMode::Hold | BindingMode::ChordHold => chord_down(d, bd, f),
            _ => chord_edge(d, bd, f),
        }
    };
    Inputs {
        light: edge("b_light"),
        heavy: edge("b_heavy"),
        guard_pressed: f.actions_pressed.has(crate::sheets::ErAction::Guard) || f.actions_pressed.has(crate::sheets::ErAction::L1),
        guard_held: f.actions_down.has(crate::sheets::ErAction::Guard) || f.actions_down.has(crate::sheets::ErAction::L1),
        dodge: edge("b_dodge"),
        aim_held: chord_down(d, b("b_aim"), f),
        throw_pressed: edge("b_throw"),
        recall_pressed: edge("b_recall"),
        rage_pressed: edge("b_rage"),
        finisher_pressed: edge("b_finisher"),
        runic_light: edge("b_runic_light"),
        runic_heavy: edge("b_runic_heavy"),
        swap: edge("b_swap_r"),
    }
}

/// Mask for a binding that is "live" this frame and whose chord is held.
pub fn mask_if(d: &Data, id: &str, live: bool, f: &Frame) -> ActionBits {
    let bd = d.binding(id);
    if live && chord_down(d, bd, f) {
        d.blocked_bits(bd)
    } else {
        ActionBits::NONE
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sheets::ErAction;

    fn frame(down: &[ErAction], pressed: &[ErAction]) -> Frame {
        let mut f = Frame::default();
        for a in down {
            f.actions_down = f.actions_down.with(*a);
        }
        for a in pressed {
            f.actions_pressed = f.actions_pressed.with(*a);
        }
        f
    }

    #[test]
    fn throw_needs_l2_held_and_r1_edge() {
        let d = Data::get();
        // L2 held, R1 pressed this frame => throw
        let f = frame(&[ErAction::L2, ErAction::R1], &[ErAction::R1]);
        let i = read(d, &f);
        assert!(i.throw_pressed && i.aim_held);
        // R1 alone is a light attack, not a throw
        let f = frame(&[ErAction::R1], &[ErAction::R1]);
        let i = read(d, &f);
        assert!(!i.throw_pressed && i.light);
        // both held but neither new => no edge
        let f = frame(&[ErAction::L2, ErAction::R1], &[]);
        assert!(!read(d, &f).throw_pressed);
    }

    #[test]
    fn rage_is_l3_plus_r3() {
        let d = Data::get();
        assert!(!read(d, &frame(&[ErAction::L3], &[ErAction::L3])).rage_pressed);
        assert!(read(d, &frame(&[ErAction::L3, ErAction::R3], &[ErAction::R3])).rage_pressed);
    }
}
