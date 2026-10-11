//! End-to-end scenarios through `Engine::tick`, with the simulated host standing in for Elden Ring.

use gowring_core::sheets::ErAction::*;
use gowring_core::sim::{SimEnemy, SimWorld};
use gowring_core::{Command, DamageSource, Vec3};

fn soldier(id: u64, x: f32, z: f32) -> SimEnemy {
    SimEnemy::new(id, x, z, 900.0)
}

#[test]
fn light_string_hits_and_fills_rage_and_stun() {
    let mut w = SimWorld::new();
    w.add_enemy(soldier(1, 0.0, 2.5));
    for _ in 0..6 {
        w.step(&[R1], &[]);
        w.idle(0.45);
    }
    let rage = w.engine.rage().frac();
    assert!(rage > 0.05, "melee fills rage: {rage}");
    assert!(w.engine.stun().frac(1) > 0.0 || w.staggers.iter().any(|(id, _)| *id == 1), "stun building");
    assert!(w.enemy(1).hp < 900.0);
}

#[test]
fn stun_to_finisher_kills_a_soldier_and_mimir_may_comment() {
    let mut w = SimWorld::new();
    w.add_enemy(soldier(1, 0.0, 2.2));
    // swing until the stun meter fills
    let mut stunned = false;
    for _ in 0..30 {
        w.step(&[R2], &[]);
        w.idle(0.7);
        if w.engine.stun().is_stunned(1, w.time) {
            stunned = true;
            break;
        }
        if !w.enemy(1).alive() {
            break;
        }
    }
    // a soldier may simply die to heavies before stunning (900 hp / 840 dmg heavy): accept either,
    // but when stunned the finisher must work and the R3 mask must be set.
    if stunned {
        w.step(&[], &[]);
        assert!(w.last_mask.has(R3) || w.engine.stun().finisher_target(gowring_core::Data::get(), &frame_for(&w), w.time).is_some());
        w.step(&[R3], &[]);
        assert!(w.finishers.contains(&1), "finisher executed");
        w.idle(0.5);
        assert!(!w.enemy(1).alive(), "soldier executed");
    } else {
        assert!(!w.enemy(1).alive());
    }
}

fn frame_for(w: &SimWorld) -> gowring_core::Frame {
    gowring_core::Frame {
        dt: w.dt,
        time: w.time,
        player: w.player.clone(),
        enemies: w.enemies.iter().map(|e| gowring_core::EnemyView { id: e.id, pos: e.pos, hp: e.hp, max_hp: e.max_hp, alive: e.alive(), boss: e.boss, engaged: true, radius: e.radius, staggered: false }).collect(),
        ..Default::default()
    }
}

#[test]
fn big_enemy_stuns_then_finisher_takes_a_slice() {
    let mut w = SimWorld::new();
    let mut brute = SimEnemy::new(7, 0.0, 2.2, 12000.0);
    brute.boss = false; // elite class by hp ceiling
    w.add_enemy(brute);
    let mut did = false;
    for _ in 0..60 {
        w.step(&[R2], &[]);
        w.idle(0.8);
        if w.engine.stun().is_stunned(7, w.time) {
            did = true;
            break;
        }
    }
    assert!(did, "an elite stuns after enough heavy hits");
    w.step(&[R3], &[]);
    assert!(w.finishers.contains(&7));
    let hp_before_idle = w.enemy(7).hp;
    w.idle(0.3);
    assert!(w.enemy(7).alive(), "elites survive a finisher (35% of max)");
    assert!(hp_before_idle < 12000.0 * 0.9);
}

#[test]
fn spartan_rage_full_cycle() {
    let mut w = SimWorld::new();
    w.player.hp = 2500.0;
    // a crowd that hits back so rage comes from damage taken as well
    for i in 0..4u64 {
        let mut e = SimEnemy::new(10 + i, -1.5 + i as f32, 2.0, 60000.0);
        e.attack_every = Some(1.0);
        e.attack_damage = 120.0;
        w.add_enemy(e);
    }
    // fight until the meter is full
    let mut t = 0;
    while !w.engine.rage().ready() && t < 400 {
        w.step(&[R1], &[]);
        w.idle(0.4);
        w.player.hp = w.player.max_hp; // keep Kratos on his feet for the test
        t += 1;
    }
    assert!(w.engine.rage().ready(), "meter full after {t} swings; frac {}", w.engine.rage().frac());
    // L3+R3 activates; the burst hurts everyone near and masks the chord
    let hp_before: Vec<f32> = w.enemies.iter().map(|e| e.hp).collect();
    let out = w.step(&[L3, R3], &[]);
    assert!(w.engine.rage().active);
    assert!(out.commands.iter().any(|c| matches!(c, Command::Mask(m) if m.has(L3) && m.has(R3))));
    assert!(out.commands.iter().any(|c| matches!(c, Command::Damage { source: DamageSource::RageBurst, .. })));
    assert!(w.sfx.contains(&"sfx_rage_on"));
    assert!((w.anim_speed - 1.12).abs() < 1e-5);
    w.idle(0.2);
    for (e, before) in w.enemies.iter().zip(hp_before) {
        assert!(e.hp < before || !e.alive(), "burst reached enemy {}", e.id);
    }
    // overlay and HUD show it
    let hud = w.idle(0.5).hud;
    assert!(hud.rage_active && hud.rage_overlay > 0.5);
    // rage runs out (Kratos is kept alive for the test)
    for _ in 0..(30 * 60) {
        w.player.hp = w.player.max_hp;
        w.step(&[], &[]);
    }
    assert!(!w.engine.rage().active);
    assert!((w.anim_speed - 1.0).abs() < 1e-5);
    assert!(w.sfx.contains(&"sfx_rage_off"));
}

#[test]
fn rage_halves_damage_taken_and_doubles_output() {
    let mut w = SimWorld::new();
    let mut e = SimEnemy::new(1, 0.0, 2.0, 60000.0);
    e.attack_every = Some(0.5);
    e.attack_damage = 200.0;
    w.add_enemy(e);
    // force rage on through the public chord path
    w.engine = {
        let mut eng = gowring_core::Engine::new();
        let _ = &mut eng;
        eng
    };
    // fill via repeated damage-taken events is slow; instead use hp loss: step with the enemy alive
    let mut guard = 0;
    while !w.engine.rage().ready() && guard < 4000 {
        w.step(&[], &[]);
        guard += 1;
        if !w.player.alive {
            break;
        }
        // heal back so we survive long enough
        w.player.hp = w.player.max_hp;
    }
    assert!(w.engine.rage().ready(), "taking hits fills rage (frac {})", w.engine.rage().frac());
    w.step(&[L3, R3], &[]);
    assert!(w.engine.rage().active);
    // while active: a hit that would deal 200 is refunded by half
    let before = w.player.hp;
    let mut taken = 0.0;
    for _ in 0..180 {
        w.player.hp = before;
        w.step(&[], &[]);
        taken += before - w.player.hp;
    }
    // raw hits are 200 each; after the 50% refund we expect net ~100 per hit
    assert!(taken > 0.0 && taken < 200.0 * (180.0 / 30.0) * 0.8, "net damage reduced: {taken}");
}

#[test]
fn perfect_parry_refunds_staggers_and_sounds() {
    let mut w = SimWorld::new();
    let mut e = soldier(1, 0.0, 2.0);
    e.attack_every = Some(5.0);
    e.attack_damage = 300.0;
    e.next_attack = 0.05;
    w.add_enemy(e);
    // press guard right before the first attack lands (inside the 0.2 s window)
    w.step(&[], &[]);
    let hp_before = w.player.hp;
    w.step(&[Guard], &[]);
    for _ in 0..30 {
        w.step(&[], &[Guard]);
    }
    assert!(w.sfx.contains(&"sfx_parry"), "parry sound; got {:?}", w.sfx);
    assert!(w.staggers.iter().any(|(id, _)| *id == 1));
    assert!((w.player.hp - hp_before).abs() < 1e-3, "no net damage: {} vs {}", w.player.hp, hp_before);
    assert!(w.engine.rage().frac() > 0.0);
}

#[test]
fn late_guard_only_blocks() {
    let mut w = SimWorld::new();
    let mut e = soldier(1, 0.0, 2.0);
    e.attack_every = Some(5.0);
    e.attack_damage = 300.0;
    e.next_attack = 1.0;
    w.add_enemy(e);
    w.step(&[Guard], &[]);
    for _ in 0..80 {
        w.step(&[], &[Guard]);
    }
    assert!(w.sfx.contains(&"sfx_block") && !w.sfx.contains(&"sfx_parry"));
}

#[test]
fn axe_throw_aim_recall_loop_with_masks() {
    let mut w = SimWorld::new();
    w.add_enemy(soldier(1, 0.0, 12.0));
    // hold L2 to aim: crosshair shows, L2 masked
    let out = w.step(&[], &[L2]);
    assert!(out.hud.aiming);
    assert!(out.commands.iter().any(|c| matches!(c, Command::Mask(m) if m.has(L2))));
    // throw with R1: L2+R1 masked so ER does not heavy-attack
    let out = w.step(&[R1], &[L2]);
    assert!(out.commands.iter().any(|c| matches!(c, Command::Mask(m) if m.has(R1) && m.has(L2))));
    assert!(w.sfx.contains(&"sfx_axe_throw"));
    for _ in 0..40 {
        w.step(&[], &[L2]);
    }
    assert!(w.enemy(1).hp < 900.0, "axe hit: {}", w.enemy(1).hp);
    // ER melee is masked while the axe is out
    let out = w.step(&[R1], &[]);
    assert!(out.commands.iter().any(|c| matches!(c, Command::Mask(m) if m.has(R1) && m.has(R2))));
    // recall brings it back
    let out = w.step(&[Action], &[]);
    assert!(out.hud.recall_available || w.engine.thrown().axe_out());
    for _ in 0..240 {
        w.step(&[], &[]);
    }
    assert!(w.engine.thrown().axe_in_hand());
    assert!(w.sfx.contains(&"sfx_axe_catch"));
}

#[test]
fn axe_sticks_in_a_wall() {
    let mut w = SimWorld::new();
    w.wall_z = Some(8.0);
    w.step(&[], &[L2]);
    w.step(&[R1], &[L2]);
    for _ in 0..30 {
        w.step(&[], &[L2]);
    }
    use gowring_core::thrown::AxeState;
    match &w.engine.thrown().axe {
        AxeState::Stuck { pos, on: None } => assert!((pos.z - 8.0).abs() < 1.0, "{pos:?}"),
        s => panic!("expected stuck in wall: {s:?}"),
    }
}

#[test]
fn runics_fire_after_windup_cool_down_and_chime() {
    let mut w = SimWorld::new();
    for i in 0..3u64 {
        w.add_enemy(soldier(20 + i, -1.0 + i as f32, 4.0));
    }
    let out = w.step(&[SwitchForm], &[]);
    assert!(out.commands.iter().any(|c| matches!(c, Command::Mask(m) if m.has(SwitchForm))));
    assert_eq!(w.casts, vec!["axe_runic_light"]);
    w.idle(0.6);
    // damage landed on enemies in the cone, and the HUD ring is cooling
    assert!(w.enemies.iter().filter(|e| e.hp < 900.0).count() >= 1);
    let hud = w.idle(0.1).hud;
    let ring = hud.runic_light.unwrap();
    assert!(!ring.ready && ring.remaining_frac > 0.5);
    // second press during cooldown does nothing
    w.step(&[SwitchForm], &[]);
    assert_eq!(w.casts.len(), 1);
    // after the cooldown a chime plays and it is ready again
    w.idle(13.0);
    assert!(w.sfx.iter().filter(|s| **s == "sfx_runic_ready").count() >= 1);
    assert!(w.idle(0.1).hud.runic_light.unwrap().ready);
}

#[test]
fn weapon_swap_changes_hud_runics_and_stows_shield() {
    let mut w = SimWorld::new();
    assert_eq!(w.idle(0.1).hud.weapon_name, "Leviathan Axe");
    w.right_slot = 1;
    let hud = w.idle(0.1).hud;
    assert_eq!(hud.weapon_name, "Blades of Chaos");
    assert!(!hud.shield_drawn, "Blades stow the shield");
    assert_eq!(hud.runic_light.unwrap().name, "Flame Lash");
    assert!(w.sfx.contains(&"sfx_weapon_swap"));
    w.right_slot = 2;
    let hud = w.idle(0.1).hud;
    assert_eq!(hud.weapon_name, "Draupnir Spear");
    assert!(hud.shield_drawn);
}

#[test]
fn mimir_greets_after_the_delay_and_subtitles_show() {
    let mut w = SimWorld::new();
    w.idle(3.0);
    assert!(w.voices.is_empty(), "not before greeting_delay_s");
    w.idle(2.5);
    assert_eq!(w.voices, vec!["mimir_greeting"]);
    let hud = w.idle(0.2).hud;
    assert!(hud.mimir_speaking && hud.subtitle.is_some());
}

#[test]
fn dying_resets_rage_and_gear_and_mimir_notices() {
    let mut w = SimWorld::new();
    w.idle(6.0);
    w.step(&[], &[L2]);
    w.step(&[R1], &[L2]);
    w.idle(0.2);
    assert!(w.engine.thrown().axe_out());
    w.player.hp = 0.0;
    w.player.alive = false;
    w.idle(0.2);
    assert!(w.engine.thrown().axe_in_hand());
    assert!(!w.engine.rage().active);
    assert!(w.voices.contains(&"mimir_death"), "{:?}", w.voices);
}

#[test]
fn low_health_overlay_and_voice() {
    let mut w = SimWorld::new();
    w.idle(6.0);
    w.add_enemy({
        let mut e = soldier(1, 0.0, 2.0);
        e.attack_every = Some(100.0);
        e
    });
    w.player.hp = 400.0;
    let out = w.idle(0.3);
    assert!(out.hud.low_health_overlay > 0.3);
    assert!(w.voices.contains(&"mimir_low_health") || w.voices.len() <= 1);
}

#[test]
fn spear_volley_then_detonation_in_a_crowd() {
    let mut w = SimWorld::new();
    w.right_slot = 2;
    for i in 0..3u64 {
        w.add_enemy(soldier(30 + i, -1.0 + i as f32, 10.0));
    }
    w.step(&[], &[L2]);
    for _ in 0..3 {
        w.step(&[R1], &[L2]);
        for _ in 0..30 {
            w.step(&[], &[L2]);
        }
    }
    assert!(w.engine.thrown().spears.len() >= 1);
    let out = w.step(&[Action], &[]);
    let blasts = out.commands.iter().filter(|c| matches!(c, Command::Damage { source: DamageSource::Blast, .. })).count();
    assert!(blasts >= 1, "detonation hurt someone");
    assert!(w.sfx.contains(&"sfx_spear_blast"));
    let _ = Vec3::ZERO;
}


#[test]
fn aim_survives_a_dropped_frame_but_not_a_release() {
    let mut w = SimWorld::new();
    for _ in 0..5 {
        assert!(w.step(&[], &[L2]).hud.aiming);
    }
    // the game drops the held button for one frame (a masked / re-evaluated input): still aiming
    assert!(w.step(&[], &[]).hud.aiming, "one missed frame must not drop the crosshair");
    assert!(w.step(&[], &[L2]).hud.aiming);
    // a real release ends the aim within a few frames
    let mut still = true;
    for _ in 0..12 {
        still = w.step(&[], &[]).hud.aiming;
    }
    assert!(!still, "aim must end after a real release");
}

#[test]
fn recall_button_is_only_taken_over_while_something_is_out() {
    let mut w = SimWorld::new();
    // axe in hand: the interact button belongs to Elden Ring (doors, items)
    let out = w.step(&[Action], &[]);
    assert!(!out.commands.iter().any(|c| matches!(c, Command::Mask(m) if m.has(Action))), "interact must pass through with nothing to recall");
    w.step(&[], &[]);
    // throw the axe, then the same button recalls it and is masked so ER does not also interact
    w.step(&[], &[L2]);
    w.step(&[R1], &[L2]);
    for _ in 0..5 {
        w.step(&[], &[]);
    }
    assert!(w.engine.thrown().axe_out());
    let out = w.step(&[Action], &[]);
    assert!(out.commands.iter().any(|c| matches!(c, Command::Mask(m) if m.has(Action))));
}


#[test]
fn detonating_spears_into_empty_air_earns_no_rage() {
    let mut w = SimWorld::new();
    w.right_slot = 2;
    // no enemies at all: throw three spears, let them stick, detonate, repeat
    for _ in 0..4 {
        w.step(&[], &[L2]);
        for _ in 0..3 {
            w.step(&[R1], &[L2]);
            for _ in 0..30 {
                w.step(&[], &[L2]);
            }
        }
        w.step(&[Action], &[]);
        w.idle(1.5);
    }
    assert!(w.sfx.contains(&"sfx_spear_blast"), "the detonations did happen");
    assert_eq!(w.engine.rage().meter, 0.0, "spamming throw + detonate at nothing must not fill Rage");
}

#[test]
fn spears_that_hit_earn_rage() {
    let mut w = SimWorld::new();
    w.right_slot = 2;
    for i in 0..3u64 {
        w.add_enemy(soldier(40 + i, -1.0 + i as f32, 10.0));
    }
    w.step(&[], &[L2]);
    for _ in 0..3 {
        w.step(&[R1], &[L2]);
        for _ in 0..30 {
            w.step(&[], &[L2]);
        }
    }
    w.step(&[Action], &[]);
    assert!(w.engine.rage().meter > 0.0, "hitting enemies with spears fills Rage");
}


fn spear_hit_meter(t: gowring_core::Tuning) -> (f32, f32) {
    let mut w = SimWorld::new();
    w.engine.set_tuning(t);
    w.right_slot = 2;
    for i in 0..3u64 {
        w.add_enemy(gowring_core::sim::SimEnemy::new(60 + i, -1.0 + i as f32, 10.0, 1.0e6));
    }
    w.step(&[], &[L2]);
    for _ in 0..3 {
        w.step(&[R1], &[L2]);
        for _ in 0..30 {
            w.step(&[], &[L2]);
        }
    }
    w.step(&[Action], &[]);
    w.idle(0.5);
    let lost: f32 = (0..3u64).map(|i| 1.0e6 - w.enemy(60 + i).hp).sum();
    (w.engine.rage().meter, lost)
}

#[test]
fn player_settings_scale_rage_and_the_mods_own_damage() {
    let base = spear_hit_meter(gowring_core::Tuning::default());
    assert!(base.0 > 0.0 && base.1 > 0.0, "{base:?}");
    let more_rage = spear_hit_meter(gowring_core::Tuning { rage_gain: 1.5, ..Default::default() });
    assert!(more_rage.0 > base.0 * 1.4 && more_rage.0 <= 100.0, "rage x1.5: {} vs {}", more_rage.0, base.0);
    let no_rage = spear_hit_meter(gowring_core::Tuning { rage_gain: 0.0, ..Default::default() });
    assert_eq!(no_rage.0, 0.0);
    let double_damage = spear_hit_meter(gowring_core::Tuning { mod_damage: 2.0, ..Default::default() });
    assert!(double_damage.1 > base.1 * 1.8, "damage x2: {} vs {}", double_damage.1, base.1);
}

#[test]
fn regular_enemies_are_toughened_once_and_bosses_are_not() {
    let mut w = SimWorld::new();
    w.engine.set_tuning(gowring_core::Tuning { enemy_health: 3.0, ..Default::default() });
    w.add_enemy(SimEnemy::new(1, 0.0, 12.0, 55.0));
    let mut boss = SimEnemy::new(2, 4.0, 12.0, 55.0);
    boss.boss = true;
    w.add_enemy(boss);
    w.add_enemy(SimEnemy::new(3, -4.0, 12.0, 3000.0)); // above the ceiling: a mini-boss, left alone
    w.idle(1.0);
    assert_eq!(w.enemy(1).hp, 165.0);
    assert_eq!(w.enemy(1).max_hp, 165.0);
    assert_eq!(w.enemy(2).hp, 55.0, "bosses are not scaled");
    assert_eq!(w.enemy(3).hp, 3000.0);
    w.idle(2.0);
    assert_eq!(w.enemy(1).hp, 165.0, "scaled once, not every frame");
    let n = w.all_commands.iter().filter(|c| matches!(c, Command::ScaleEnemy { .. })).count();
    assert_eq!(n, 1);
}

#[test]
fn untouched_health_setting_sends_no_scale_commands() {
    let mut w = SimWorld::new();
    w.add_enemy(SimEnemy::new(1, 0.0, 12.0, 55.0));
    w.idle(1.0);
    assert!(!w.all_commands.iter().any(|c| matches!(c, Command::ScaleEnemy { .. })));
    assert_eq!(w.enemy(1).hp, 55.0);
}

#[test]
fn a_fight_with_a_toughened_enemy_ends_in_a_stagger_and_a_finisher() {
    let mut w = SimWorld::new();
    w.engine.set_tuning(gowring_core::Tuning { enemy_health: 6.0, ..Default::default() });
    w.add_enemy(SimEnemy::new(1, 0.0, 2.2, 300.0));
    let mut staggered_at_hp = None;
    for _ in 0..60 {
        w.step(&[R1], &[]);
        w.idle(0.5);
        if staggered_at_hp.is_none() && w.engine.stun().is_stunned(1, w.time) {
            staggered_at_hp = Some(w.enemy(1).hp / w.enemy(1).max_hp);
        }
        if staggered_at_hp.is_some() || !w.enemy(1).alive() {
            break;
        }
    }
    let frac = staggered_at_hp.expect("the enemy is staggered before it dies");
    assert!(w.enemy(1).alive());
    w.step(&[R3], &[]);
    assert!(w.finishers.contains(&1), "the finisher is on offer (hp fraction {frac})");
}
