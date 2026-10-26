//! Ordinary generation-seven behavior and the explicit source compiler boundary.
use super::super::data::{Category, MoveDetails};
use super::super::field::Terrain;
use super::*;

fn idle() -> MoveData {
    status(9, effects::NOTHING, Target::User)
}

#[test]
fn a_moves_category_selects_stats_independently_of_its_type() {
    let physical_fire = MoveData {
        category: Category::Physical,
        ..mv(1, FIRE, 80)
    };
    let special_fire = MoveData {
        category: Category::Special,
        ..mv(2, FIRE, 80)
    };
    let mut user = fighter("a", [NORMAL; 2], 100, &[physical_fire, special_fire]);
    user.stats.attack = 200;
    user.stats.sp_attack = 50;
    let (b, _) = battle(
        Format::Singles,
        vec![user],
        vec![fighter("b", [NORMAL; 2], 50, &[idle()])],
        1,
    );
    // L50 base damages are 72 and 19; the common 93% estimate floors to 66 and 17.
    assert_eq!(b.estimate(P, 0, O).0, 66);
    assert_eq!(b.estimate(P, 1, O).0, 17);
}

#[test]
fn charged_and_hazy_fields_protect_only_grounded_fighters() {
    let sleep = status(4, effects::SLEEP, Target::Selected);
    let (mut b, _) = battle(
        Format::Singles,
        vec![fighter(
            "a",
            [NORMAL; 2],
            100,
            &[mv(1, ELECTRIC, 80), sleep],
        )],
        vec![fighter("b", [NORMAL; 2], 50, &[idle()])],
        1,
    );
    let plain = b.estimate(P, 0, O).0;
    b.field_mut().terrain = Terrain::Charged;
    b.field_mut().terrain_turns = 5;
    assert!(b.estimate(P, 0, O).0 > plain * 14 / 10);
    assert!(!b.ailment_would_stick(P, O, Ailment::Sleep, &sleep));
    b.member_mut(1, 0).types = [FLYING; 2];
    assert!(b.ailment_would_stick(P, O, Ailment::Sleep, &sleep));
    b.member_mut(1, 0).types = [NORMAL; 2];
    b.field_mut().terrain = Terrain::Hazy;
    assert!(!b.ailment_would_stick(
        P,
        O,
        Ailment::Burn,
        &status(5, effects::BURN, Target::Selected)
    ));
}

#[test]
fn overgrown_field_heals_five_times_and_then_expires() {
    let (mut b, _) = battle(
        Format::Singles,
        vec![fighter("a", [NORMAL; 2], 100, &[idle()])],
        vec![fighter("b", [FLYING; 2], 50, &[idle()])],
        1,
    );
    b.member_mut(0, 0).hp = 50;
    b.member_mut(1, 0).hp = 50;
    b.field_mut().terrain = Terrain::Overgrown;
    b.field_mut().terrain_turns = 5;
    for _ in 0..5 {
        b.play_turn(&[use_move(P, 0), use_move(O, 0)]).unwrap();
    }
    assert_eq!(b.fighter(P).unwrap().hp, 50 + 5 * 9);
    assert_eq!(b.fighter(O).unwrap().hp, 50);
    assert_eq!(b.field().terrain, Terrain::None);
    b.play_turn(&[use_move(P, 0), use_move(O, 0)]).unwrap();
    assert_eq!(b.fighter(P).unwrap().hp, 95);
}

#[test]
fn strange_field_blocks_priority_against_grounded_foes() {
    let quick = MoveData {
        priority: 1,
        ..mv(1, NORMAL, 40)
    };
    let (mut b, _) = battle(
        Format::Singles,
        vec![fighter("a", [NORMAL; 2], 100, &[quick])],
        vec![fighter("b", [NORMAL; 2], 50, &[idle()])],
        1,
    );
    b.field_mut().terrain = Terrain::Strange;
    b.field_mut().terrain_turns = 5;
    let events = b.play_turn(&[use_move(P, 0), use_move(O, 0)]).unwrap();
    assert!(events.contains(&Event::NoEffect { target: O }));
    assert_eq!(b.fighter(O).unwrap().hp, 150);
    b.member_mut(1, 0).types = [FLYING; 2];
    let events = b.play_turn(&[use_move(P, 0), use_move(O, 0)]).unwrap();
    assert!(!hits(&events).is_empty());
}

#[test]
fn generic_stat_moves_respect_doubling_and_inverting_abilities() {
    let mut details = MoveDetails::default();
    details.stages[Stat::Attack as usize] = 2;
    let boost = MoveData {
        effect: effects::DATA_SELF_STATS,
        details,
        ..status(14, effects::NOTHING, Target::User)
    };
    for (ability, expected) in [
        (abilities::DOUBLE_STAGES, 4),
        (abilities::INVERTS_STAGES, -2),
    ] {
        let mut user = fighter("a", [NORMAL; 2], 100, &[boost]);
        user.ability = ability;
        let (mut b, _) = battle(
            Format::Singles,
            vec![user],
            vec![fighter("b", [NORMAL; 2], 50, &[idle()])],
            1,
        );
        b.play_turn(&[use_move(P, 0), use_move(O, 0)]).unwrap();
        assert_eq!(b.fighter(P).unwrap().stage(Stat::Attack), expected);
    }
}

#[test]
fn full_health_endurance_survives_one_hit_then_stops_protecting() {
    let mut target = fighter("b", [NORMAL; 2], 50, &[idle()]);
    target.ability = abilities::NO_ONE_HIT_KO;
    let (mut b, _) = battle(
        Format::Singles,
        vec![fighter("a", [NORMAL; 2], 100, &[mv(1, NORMAL, 250)])],
        vec![target],
        1,
    );
    let events = b.play_turn(&[use_move(P, 0), use_move(O, 0)]).unwrap();
    assert!(events.contains(&Event::HungOn { target: O }));
    assert_eq!(b.fighter(O).unwrap().hp, 1);
    b.play_turn(&[use_move(P, 0), use_move(O, 0)]).unwrap();
    assert_eq!(b.fighter(O).unwrap().hp, 0);
}

#[test]
fn source_driven_secondaries_apply_their_own_stat_order() {
    let mut details = MoveDetails::default();
    details.stages[Stat::SpDefense as usize] = -1;
    details.stage_chances[Stat::SpDefense as usize] = 100;
    let hit = MoveData {
        details,
        ..mv(412, GRASS, 40)
    };
    let (mut b, _) = battle(
        Format::Singles,
        vec![fighter("a", [NORMAL; 2], 100, &[hit])],
        vec![fighter("b", [NORMAL; 2], 50, &[idle()])],
        1,
    );
    b.play_turn(&[use_move(P, 0), use_move(O, 0)]).unwrap();
    assert_eq!(b.fighter(O).unwrap().stage(Stat::SpDefense), -1);
    assert_eq!(b.fighter(O).unwrap().stage(Stat::SpAttack), 0);
}

fn ordinary(id: u16, category: Category, power: u8, target: Target) -> MoveData {
    MoveData {
        category,
        details: MoveDetails {
            special: id,
            ..Default::default()
        },
        effect: if category == Category::Status {
            if target == Target::User {
                effects::ORDINARY_SELF
            } else {
                effects::ORDINARY_TARGET
            }
        } else {
            effects::HIT
        },
        target,
        ..mv(id, NORMAL, power)
    }
}

#[test]
fn reversed_order_reverses_speed_without_reversing_move_priority() {
    let hit = mv(1, NORMAL, 40);
    let (mut b, _) = battle(
        Format::Singles,
        vec![fighter("slow", [NORMAL; 2], 50, &[hit])],
        vec![fighter("fast", [NORMAL; 2], 100, &[hit])],
        1,
    );
    b.field_mut().reversed_order = 5;
    let events = b.play_turn(&[use_move(P, 0), use_move(O, 0)]).unwrap();
    assert_eq!(users(&events)[0], P);
    let quick = MoveData { priority: 1, ..hit };
    let (mut b, _) = battle(
        Format::Singles,
        vec![fighter("slow", [NORMAL; 2], 50, &[hit])],
        vec![fighter("fast", [NORMAL; 2], 100, &[quick])],
        1,
    );
    b.field_mut().reversed_order = 5;
    let events = b.play_turn(&[use_move(P, 0), use_move(O, 0)]).unwrap();
    assert_eq!(users(&events)[0], O);
}

#[test]
fn landing_heal_temporarily_removes_flying_without_overwriting_a_permanent_type() {
    let heal = ordinary(355, Category::Status, 0, Target::User);
    let (mut b, _) = battle(
        Format::Singles,
        vec![fighter("bird", [FLYING; 2], 100, &[heal, idle()])],
        vec![fighter("foe", [NORMAL; 2], 50, &[mv(1, GROUND, 40)])],
        1,
    );
    b.member_mut(0, 0).hp = 30;
    let events = b.play_turn(&[use_move(P, 0), use_move(O, 0)]).unwrap();
    assert!(
        hits(&events)
            .iter()
            .any(|&(_, target, damage)| target == P && damage > 0)
    );
    assert_eq!(b.fighter(P).unwrap().types, [FLYING; 2]);
    assert_eq!(b.fighter(P).unwrap().battle_types(), [FLYING; 2]);
    let health = b.fighter(P).unwrap().hp;
    b.play_turn(&[use_move(P, 1), use_move(O, 0)]).unwrap();
    assert_eq!(b.fighter(P).unwrap().hp, health);
}

#[test]
fn new_entry_hazards_poison_slow_and_are_absorbed_by_poison_types() {
    let (mut b, _) = battle(
        Format::Singles,
        vec![fighter("a", [NORMAL; 2], 100, &[idle()])],
        vec![
            fighter("old", [NORMAL; 2], 50, &[idle()]),
            fighter("new", [NORMAL; 2], 50, &[idle()]),
            fighter("poison", [POISON; 2], 50, &[idle()]),
        ],
        1,
    );
    b.field_mut().sides[1].rock_hazard = true;
    b.field_mut().sides[1].poison_hazard = 2;
    b.field_mut().sides[1].slowing_hazard = true;
    let events = b.play_turn(&[use_move(P, 0), switch(O, 1)]).unwrap();
    assert!(
        events
            .iter()
            .any(|e| matches!(e,Event::Hurt{target,damage:18,by:HurtBy::Hazard,..}if*target==O))
    );
    assert_eq!(b.fighter(O).unwrap().ailment, Some(Ailment::BadPoison));
    assert_eq!(b.fighter(O).unwrap().stage(Stat::Speed), -1);
    b.play_turn(&[use_move(P, 0), switch(O, 2)]).unwrap();
    assert_eq!(b.field().sides[1].poison_hazard, 0);
    assert_eq!(b.fighter(O).unwrap().ailment, None);
}

#[test]
fn borrowed_stat_moves_use_the_named_stats() {
    use super::super::data::flags;
    let against_defense = MoveData {
        type_id: PSYCHIC,
        category: Category::Special,
        flags: flags::TARGET_DEFENSE,
        ..ordinary(473, Category::Special, 90, Target::Selected)
    };
    let normal = MoveData {
        flags: 0,
        ..against_defense
    };
    let mut foe = fighter("foe", [NORMAL; 2], 50, &[idle()]);
    foe.stats.defense = 20;
    foe.stats.sp_defense = 200;
    let (b, _) = battle(
        Format::Singles,
        vec![fighter("a", [NORMAL; 2], 100, &[against_defense, normal])],
        vec![foe],
        1,
    );
    assert_eq!(b.estimate(P, 0, O).0, 186);
    assert_eq!(b.estimate(P, 1, O).0, 19);
    let foes_attack = MoveData {
        type_id: DARK,
        flags: flags::TARGET_ATTACK,
        ..ordinary(492, Category::Physical, 95, Target::Selected)
    };
    let mut a = fighter("weak", [NORMAL; 2], 100, &[foes_attack]);
    a.stats.attack = 20;
    let mut foe = fighter("strong", [NORMAL; 2], 50, &[idle()]);
    foe.stats.attack = 200;
    let (b, _) = battle(Format::Singles, vec![a], vec![foe], 1);
    assert_eq!(b.estimate(P, 0, O).0, 79);
}
