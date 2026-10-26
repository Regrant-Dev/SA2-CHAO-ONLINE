//! Mechanics compiled from synthetic move records, then run in battle.
use super::super::data::{Category, flags};
use super::super::fighter::Hidden;
use super::super::source_rules::compile_move;
use super::*;
use crate::creature::data::{MOVE_SIZE, MoveCategory, MoveRecord, parse_move};

/// A record: the type and category given, power 60 (0 for status), 100% accuracy, 15 PP, one
/// chosen target, nothing else.
fn record(id: u16, type_id: u8, category: MoveCategory) -> MoveRecord {
    let mut raw = [0u8; MOVE_SIZE];
    raw[0] = type_id;
    raw[2] = match category {
        MoveCategory::Status => 0,
        MoveCategory::Physical => 1,
        MoveCategory::Special => 2,
    };
    raw[3] = if category == MoveCategory::Status {
        0
    } else {
        60
    };
    raw[4] = 100;
    raw[5] = 15;
    parse_move(&raw, id).unwrap()
}

fn compiled(source: &MoveRecord) -> MoveData {
    compile_move(source).expect("a supported record")
}

fn idle() -> MoveData {
    status(9, effects::NOTHING, Target::User)
}

/// A move that hides its user for a turn: in the air, underground or underwater by its type.
fn hide(type_id: u8) -> MoveData {
    with(mv(8, type_id, 60), effects::HIDE_TWO_TURN)
}

fn tough(name: &str, speed: u16, moves: &[MoveData]) -> Fighter {
    let mut f = fighter(name, [NORMAL; 2], speed, moves);
    f.stats.hp = 999;
    f.hp = 999;
    f
}

#[test]
fn lock_in_records_rampage_then_confuse() {
    for id in [37, 80, 200] {
        let mut source = record(id, NORMAL, MoveCategory::Physical);
        source.target_raw = 9;
        let lock = compiled(&source);
        assert_eq!(lock.effect, effects::RAMPAGE, "{id}");
        assert_eq!(lock.target, Target::RandomFoe);
    }
    let mut source = record(37, NORMAL, MoveCategory::Physical);
    source.target_raw = 9;
    let lock = compiled(&source);
    let (mut b, _) = battle(
        Format::Singles,
        vec![fighter("a", [NORMAL; 2], 100, &[lock, mv(2, NORMAL, 10)])],
        vec![tough("b", 50, &[idle()])],
        3,
    );
    b.play_turn(&[use_move(P, 0), use_move(O, 0)]).unwrap();
    let mut uses = 1;
    let mut confused = false;
    while b.forced(P) {
        let ev = b.play_turn(&[use_move(P, 1), use_move(O, 0)]).unwrap();
        uses += 1;
        assert!(ev.contains(&Event::UsedMove { by: P, move_id: 37 }));
        confused |= ev.contains(&Event::Confused { target: P });
    }
    assert!((2..=3).contains(&uses), "{uses}");
    assert!(confused);
}

#[test]
fn listed_records_reach_hiding_foes() {
    let doubles_air = record(16, NORMAL, MoveCategory::Special);
    assert_eq!(
        effects::reaches(&compiled(&doubles_air), Hidden::Air),
        Some(2)
    );
    let mut flinching_air = record(239, NORMAL, MoveCategory::Special);
    flinching_air.flinch_percent = 10;
    let flinching_air = compiled(&flinching_air);
    assert_eq!(effects::reaches(&flinching_air, Hidden::Air), Some(2));
    assert_eq!(flinching_air.details.flinch_chance, 10);
    let reaches_air = record(327, NORMAL, MoveCategory::Physical);
    assert_eq!(
        effects::reaches(&compiled(&reaches_air), Hidden::Air),
        Some(1)
    );
    let mut grounded_spread = record(89, NORMAL, MoveCategory::Physical);
    grounded_spread.target_raw = 4;
    let grounded_spread = compiled(&grounded_spread);
    assert_eq!(
        effects::reaches(&grounded_spread, Hidden::Underground),
        Some(2)
    );
    assert_eq!(effects::reaches(&grounded_spread, Hidden::Air), None);
    // An all-adjacent record hits everyone else on the field, not only both foes.
    let mut reaches_water = record(57, NORMAL, MoveCategory::Special);
    reaches_water.target_raw = 4;
    let reaches_water = compiled(&reaches_water);
    assert_eq!(reaches_water.target, Target::AllOthers);
    assert_eq!(
        effects::reaches(&reaches_water, Hidden::Underwater),
        Some(2)
    );
    let unlisted = compiled(&record(56, NORMAL, MoveCategory::Special));
    assert_eq!(effects::reaches(&unlisted, Hidden::Underwater), None);

    // In battle: the diver is hit for double while hiding; the unlisted move misses it.
    for (sweep, reached) in [(reaches_water, true), (unlisted, false)] {
        let (mut b, _) = battle(
            Format::Singles,
            vec![fighter("a", [NORMAL; 2], 50, &[sweep])],
            vec![tough("diver", 100, &[hide(WATER)])],
            1,
        );
        let estimate = b.estimate(P, 0, O).0;
        let ev = b.play_turn(&[use_move(P, 0), use_move(O, 0)]).unwrap();
        let landed: Vec<_> = hits(&ev).into_iter().filter(|h| h.1 == O).collect();
        if reached {
            assert_eq!(landed.len(), 1, "{ev:?}");
            assert!(
                u32::from(landed[0].2) > estimate * 3 / 2,
                "{landed:?} vs {estimate}"
            );
        } else {
            assert!(landed.is_empty(), "{ev:?}");
            assert!(ev.contains(&Event::Missed { by: P, target: O }));
        }
    }
}

#[test]
fn confuse_and_raise_records_do_both() {
    let mut raise = record(207, NORMAL, MoveCategory::Status);
    raise.quality = 5;
    raise.inflict = 6;
    raise.stat_ids[0] = 1;
    raise.stat_stages[0] = 2;
    let raise = compiled(&raise);
    assert_eq!(raise.effect, effects::CONFUSE_ATTACK_UP2);
    let mut special = record(260, NORMAL, MoveCategory::Status);
    special.quality = 5;
    special.inflict = 6;
    special.stat_ids[0] = 3;
    special.stat_stages[0] = 1;
    assert_eq!(compiled(&special).effect, effects::CONFUSE_SP_ATTACK_UP);
    // A pairing the engine has no effect for stays unavailable.
    special.stat_ids[0] = 2;
    assert!(compile_move(&special).is_none());

    let (mut b, _) = battle(
        Format::Singles,
        vec![fighter("a", [NORMAL; 2], 100, &[raise])],
        vec![tough("b", 50, &[idle()])],
        1,
    );
    let ev = b.play_turn(&[use_move(P, 0), use_move(O, 0)]).unwrap();
    assert_eq!(b.fighter(O).unwrap().stage(Stat::Attack), 2);
    assert!(ev.contains(&Event::Confused { target: O }));
}

#[test]
fn the_record_thaw_flag_lets_a_frozen_user_move() {
    let mut thaws_user = record(172, NORMAL, MoveCategory::Physical);
    thaws_user.flags = 1 << 10;
    let thaws_user = compiled(&thaws_user);
    assert_ne!(thaws_user.flags & flags::THAWS_USER, 0);
    let cold = compiled(&record(172, NORMAL, MoveCategory::Physical));
    assert_eq!(cold.flags & flags::THAWS_USER, 0);
    let mut stayed_frozen = false;
    for seed in 0..20 {
        for (mv, thaws) in [(thaws_user, true), (cold, false)] {
            let (mut b, _) = battle(
                Format::Singles,
                vec![fighter("a", [NORMAL; 2], 100, &[mv])],
                vec![tough("b", 50, &[idle()])],
                seed,
            );
            b.member_mut(0, 0).ailment = Some(Ailment::Freeze);
            let ev = b.play_turn(&[use_move(P, 0), use_move(O, 0)]).unwrap();
            let moved = ev.contains(&Event::UsedMove {
                by: P,
                move_id: 172,
            });
            if thaws {
                assert!(ev.contains(&Event::Thawed { target: P }), "{seed}");
                assert!(moved, "{seed}");
            } else {
                stayed_frozen |= !moved;
            }
        }
    }
    assert!(stayed_frozen);
}

#[test]
fn shrink_punishing_records_double_on_a_shrunk_foe() {
    let punishes = compiled(&record(23, NORMAL, MoveCategory::Physical));
    assert_ne!(punishes.flags & flags::PUNISHES_SHRINK, 0);
    let plain = MoveData {
        flags: punishes.flags & !flags::PUNISHES_SHRINK,
        ..punishes
    };
    let damage = |mv: MoveData| {
        // Sure to hit either way (`ALL_MOVES_HIT`), so both take the same damage roll.
        let mut a = fighter("a", [NORMAL; 2], 100, &[mv]);
        a.ability = crate::battle::abilities::ALL_MOVES_HIT;
        let (mut b, _) = battle(Format::Singles, vec![a], vec![tough("b", 50, &[idle()])], 1);
        b.member_mut(1, 0).volatile.shrunk = true;
        let ev = b.play_turn(&[use_move(P, 0), use_move(O, 0)]).unwrap();
        u32::from(hits(&ev)[0].2)
    };
    let (doubled, single) = (damage(punishes), damage(plain));
    assert!(doubled.abs_diff(single * 2) <= 1, "{doubled} vs {single}");
}

#[test]
fn shrink_punishing_records_never_miss_a_shrunk_foe() {
    let punishes = compiled(&record(23, NORMAL, MoveCategory::Physical));
    let plain = MoveData {
        flags: punishes.flags & !flags::PUNISHES_SHRINK,
        ..punishes
    };
    let landed = |mv: MoveData| {
        (0..100)
            .filter(|&seed| {
                let (mut b, _) = battle(
                    Format::Singles,
                    vec![fighter("a", [NORMAL; 2], 100, &[mv])],
                    vec![tough("b", 50, &[idle()])],
                    seed,
                );
                let foe = b.member_mut(1, 0);
                foe.volatile.shrunk = true;
                foe.change_stage(Stat::Evasion, 6);
                let ev = b.play_turn(&[use_move(P, 0), use_move(O, 0)]).unwrap();
                hits(&ev).len() == 1
            })
            .count()
    };
    assert_eq!(landed(punishes), 100);
    assert!(landed(plain) < 70);
}

#[test]
fn the_recoil_boost_ability_powers_any_recoil_record() {
    let mut recoil = record(38, NORMAL, MoveCategory::Physical);
    recoil.recoil_raw = (-25i8) as u8;
    let recoil = compiled(&recoil);
    assert_eq!(recoil.details.recoil_percent, 25);
    assert_eq!(recoil.effect, effects::HIT);
    let plain = compiled(&record(38, NORMAL, MoveCategory::Physical));
    let estimate = |mv: MoveData, ability: u8| {
        let mut a = fighter("a", [NORMAL; 2], 100, &[mv]);
        a.ability = ability;
        let (b, _) = battle(Format::Singles, vec![a], vec![tough("b", 50, &[idle()])], 1);
        b.estimate(P, 0, O).0
    };
    assert!(estimate(recoil, 120) * 10 > estimate(recoil, 0) * 11);
    assert_eq!(estimate(plain, 120), estimate(plain, 0));
}

#[test]
fn recoil_and_drain_outside_percent_bounds_are_rejected() {
    let mut source = record(38, NORMAL, MoveCategory::Physical);
    source.recoil_raw = 128;
    assert!(compile_move(&source).is_none());
    source.recoil_raw = (-100i8) as u8;
    assert_eq!(compiled(&source).details.recoil_percent, 100);
    source.recoil_raw = 101;
    assert!(compile_move(&source).is_none());
    source.recoil_raw = 75;
    assert_eq!(compiled(&source).details.drain_percent, 75);
}

#[test]
fn record_fields_pick_the_structural_mechanic() {
    let damaging = |id: u16, edit: &dyn Fn(&mut MoveRecord)| {
        let mut source = record(id, NORMAL, MoveCategory::Physical);
        edit(&mut source);
        compile_move(&source).map(|m| m.effect)
    };
    let count = |min, max| {
        move |s: &mut MoveRecord| {
            s.min_hits = min;
            s.max_hits = max;
        }
    };
    assert_eq!(damaging(3, &count(2, 5)), Some(effects::MULTI_HIT));
    assert_eq!(damaging(24, &count(2, 2)), Some(effects::TWO_HITS));
    assert_eq!(damaging(24, &count(3, 3)), None);
    assert_eq!(
        damaging(20, &|s| {
            s.inflict = 8;
            s.inflict_percent = 100;
        }),
        Some(effects::TRAP_HIT)
    );
    assert_eq!(damaging(12, &|s| s.quality = 9), Some(effects::ONE_HIT_KO));
    assert_eq!(damaging(2, &|s| s.crit_stage = 1), Some(effects::HIGH_CRIT));
    assert_eq!(
        damaging(63, &|s| s.flags = 1 << 2),
        Some(effects::RECHARGE_AFTER)
    );
    assert_eq!(damaging(63, &|s| s.flags = 1 << 1), None);
    assert_eq!(
        damaging(246, &|s| {
            s.quality = 7;
            s.stat_ids[0] = 8;
            s.stat_stages[0] = 1;
            s.stat_percents[0] = 10;
        }),
        Some(effects::USER_ALL_UP_CHANCE)
    );
    // An added condition the engine doesn't run makes the move unavailable, not a plain hit.
    assert_eq!(damaging(1, &|s| s.inflict = 15), None);
    assert_eq!(damaging(1, &|s| s.quality = 13), None);

    let status = |id: u16, edit: &dyn Fn(&mut MoveRecord)| {
        let mut source = record(id, NORMAL, MoveCategory::Status);
        edit(&mut source);
        compile_move(&source).map(|m| m.effect)
    };
    let poison = |turns| {
        move |s: &mut MoveRecord| {
            s.quality = 1;
            s.inflict = 5;
            s.min_turns = turns;
            s.max_turns = turns;
        }
    };
    assert_eq!(status(92, &poison(15)), Some(effects::BAD_POISON));
    assert_eq!(status(77, &poison(0)), Some(effects::POISON));
    assert_eq!(status(18, &|s| s.quality = 12), Some(effects::FORCE_SWITCH));
    assert_eq!(
        status(105, &|s| {
            s.quality = 3;
            s.healing_raw = 50;
            s.target_raw = 7;
        }),
        Some(effects::HEAL_HALF)
    );
    for (code, effect) in [
        (9, effects::HAUNTED_SLEEP),
        (13, effects::SEAL_LAST_MOVE),
        (18, effects::SEED_DRAIN),
        (21, effects::ROOT),
    ] {
        let edit = |s: &mut MoveRecord| {
            s.quality = 1;
            s.inflict = code;
        };
        assert_eq!(status(73, &edit), Some(effect), "{code}");
    }
    // An unlisted, unaudited identity stays unavailable whatever its record says.
    assert_eq!(status(721, &|s| s.quality = 1), None);
}

#[test]
fn compiled_categories_come_from_the_record() {
    let special = compiled(&record(1, NORMAL, MoveCategory::Special));
    assert_eq!(special.category, Category::Special);
    let physical = compiled(&record(1, FIRE, MoveCategory::Physical));
    assert_eq!(physical.category, Category::Physical);
}
