//! Abilities, a family at a time.

use super::super::data::flags;
use super::super::event::Condition;
use super::*;

fn wait() -> MoveData {
    status(9, effects::NOTHING, Target::User)
}

fn touch(m: MoveData) -> MoveData {
    MoveData {
        flags: m.flags | flags::CONTACT,
        ..m
    }
}

fn with_ability(mut f: Fighter, ability: u8) -> Fighter {
    f.ability = ability;
    f
}

fn duel(a: Fighter, b: Fighter, seed: u64) -> Battle {
    battle(Format::Singles, vec![a], vec![b], seed).0
}

fn turn(b: &mut Battle, choices: &[Choice]) -> Vec<Event> {
    b.play_turn(choices).unwrap()
}

#[test]
fn contact_abilities_hit_back() {
    let poke = touch(mv(1, NORMAL, 10));
    let mut statics = [0; 2];
    for seed in 0..120 {
        for (i, m) in [poke, mv(2, NORMAL, 10)].into_iter().enumerate() {
            let mut b = duel(
                fighter("a", [NORMAL; 2], 100, &[m]),
                with_ability(
                    fighter("b", [NORMAL; 2], 50, &[wait()]),
                    abilities::CONTACT_PARALYZE,
                ),
                seed,
            );
            turn(&mut b, &[use_move(P, 0), use_move(O, 0)]);
            if b.fighter(P).unwrap().ailment == Some(Ailment::Paralysis) {
                statics[i] += 1;
            }
        }
    }
    // About one in three on contact, never without it.
    assert!((25..60).contains(&statics[0]), "{statics:?}");
    assert_eq!(statics[1], 0);

    let mut b = duel(
        fighter("a", [NORMAL; 2], 100, &[poke]),
        with_ability(
            fighter("b", [NORMAL; 2], 50, &[wait()]),
            abilities::CONTACT_HURTS,
        ),
        1,
    );
    let ev = turn(&mut b, &[use_move(P, 0), use_move(O, 0)]);
    assert!(ev.contains(&Event::Hurt {
        target: P,
        damage: 9,
        hp: 141,
        by: HurtBy::Ability(abilities::CONTACT_HURTS)
    }));
}

#[test]
fn absorbing_abilities() {
    let zap = mv(1, ELECTRIC, 40);
    let mut b = duel(
        fighter("a", [NORMAL; 2], 100, &[zap, mv(2, FIRE, 40)]),
        with_ability(
            fighter("b", [NORMAL; 2], 50, &[wait(), mv(3, FIRE, 40)]),
            abilities::ELECTRIC_HEALS,
        ),
        1,
    );
    b.member_mut(1, 0).hp = 100;
    let ev = turn(&mut b, &[use_move(P, 0), use_move(O, 0)]);
    assert!(ev.contains(&Event::Healed {
        target: O,
        amount: 37,
        hp: 137
    }));
    assert!(hits(&ev).is_empty());
    assert_eq!(b.estimate(P, 0, O), (0, 0));

    // Fire powers up the taker's own fire moves.
    b.member_mut(1, 0).ability = abilities::FIRE_ABSORB_BOOST;
    let before = b.estimate(O, 1, P).0;
    let ev = turn(&mut b, &[use_move(P, 1), use_move(O, 0)]);
    assert!(ev.contains(&Event::Started {
        target: O,
        condition: Condition::FireBoost
    }));
    assert!(b.estimate(O, 1, P).0 > before * 4 / 3);
}

#[test]
fn only_super_effective_hits() {
    let mut b = duel(
        fighter("a", [NORMAL; 2], 100, &[mv(1, NORMAL, 80), mv(2, FIRE, 40)]),
        with_ability(
            fighter("b", [GRASS; 2], 50, &[wait()]),
            abilities::ONLY_SUPER_EFFECTIVE,
        ),
        1,
    );
    let ev = turn(&mut b, &[use_move(P, 0), use_move(O, 0)]);
    assert!(ev.contains(&Event::NoEffect { target: O }));
    assert!(hits(&ev).is_empty());
    let ev = turn(&mut b, &[use_move(P, 1), use_move(O, 0)]);
    assert_eq!(hits(&ev).len(), 1);
}

#[test]
fn stat_drop_guards() {
    let attack_down = status(1, effects::TARGET_DOWN_FIRST, Target::Selected);
    let power = with(
        mv(2, FIGHTING, 120),
        effects::USER_ATTACK_DEFENSE_DOWN_AFTER,
    );
    let mut b = duel(
        with_ability(
            fighter("a", [NORMAL; 2], 100, &[attack_down, power]),
            abilities::NO_STAT_DROPS_A,
        ),
        with_ability(
            fighter("b", [NORMAL; 2], 50, &[attack_down]),
            abilities::NO_ATTACK_DROPS,
        ),
        1,
    );
    let ev = turn(&mut b, &[use_move(P, 0), use_move(O, 0)]);
    assert!(ev.contains(&Event::Ability {
        at: O,
        ability: abilities::NO_ATTACK_DROPS
    }));
    assert!(ev.contains(&Event::StatProtected { target: P }));
    assert_eq!(b.fighter(O).unwrap().stage(Stat::Attack), 0);
    assert_eq!(b.fighter(P).unwrap().stage(Stat::Attack), 0);
    // Its own move's drop still happens.
    b.member_mut(1, 0).stats.hp = 999;
    b.member_mut(1, 0).hp = 999;
    turn(&mut b, &[use_move(P, 1), use_move(O, 0)]);
    assert_eq!(b.fighter(P).unwrap().stage(Stat::Attack), -1);

    // The entry attack drop is blocked the same way.
    let (b, ev) = battle(
        Format::Singles,
        vec![with_ability(
            fighter("a", [NORMAL; 2], 100, &[wait()]),
            abilities::ENTRY_LOWERS_ATTACK,
        )],
        vec![with_ability(
            fighter("b", [NORMAL; 2], 50, &[wait()]),
            abilities::NO_STAT_DROPS_B,
        )],
        1,
    );
    assert!(ev.contains(&Event::StatProtected { target: O }));
    assert_eq!(b.fighter(O).unwrap().stage(Stat::Attack), 0);
}

#[test]
fn copying_a_foes_ability_on_entry() {
    let (b, ev) = battle(
        Format::Singles,
        vec![with_ability(
            fighter("a", [NORMAL; 2], 100, &[wait()]),
            abilities::COPIES_FOE_ABILITY,
        )],
        vec![with_ability(
            fighter("b", [NORMAL; 2], 50, &[wait()]),
            abilities::NO_BURN,
        )],
        1,
    );
    assert!(ev.contains(&Event::AbilityChanged {
        target: P,
        ability: abilities::NO_BURN
    }));
    assert_eq!(b.fighter(P).unwrap().ability, abilities::NO_BURN);
    // Even the only-super-effective ability; never a foe without one.
    for (foe, copied) in [
        (abilities::ONLY_SUPER_EFFECTIVE, true),
        (abilities::NONE, false),
    ] {
        let (b, _) = battle(
            Format::Singles,
            vec![with_ability(
                fighter("a", [NORMAL; 2], 100, &[wait()]),
                abilities::COPIES_FOE_ABILITY,
            )],
            vec![with_ability(fighter("b", [NORMAL; 2], 50, &[wait()]), foe)],
            1,
        );
        assert_eq!(b.fighter(P).unwrap().ability == foe, copied);
    }
}

#[test]
fn loafing_every_other_turn() {
    let mut b = duel(
        with_ability(
            fighter("a", [NORMAL; 2], 100, &[mv(1, NORMAL, 10)]),
            abilities::LOAFS,
        ),
        fighter("b", [NORMAL; 2], 50, &[wait()]),
        1,
    );
    b.member_mut(1, 0).stats.hp = 999;
    b.member_mut(1, 0).hp = 999;
    let mut pattern = Vec::new();
    for _ in 0..4 {
        let ev = turn(&mut b, &[use_move(P, 0), use_move(O, 0)]);
        pattern.push(ev.contains(&Event::CantAct {
            by: P,
            why: Blocked::Loafing,
        }));
    }
    assert_eq!(pattern, vec![false, true, false, true]);
}

#[test]
fn loafers_sent_in_later() {
    let loafer = || {
        with_ability(
            fighter("l", [NORMAL; 2], 100, &[mv(1, NORMAL, 10)]),
            abilities::LOAFS,
        )
    };
    let foe = || {
        let mut f = fighter("b", [NORMAL; 2], 50, &[mv(2, NORMAL, 40), wait()]);
        f.stats.hp = 999;
        f.hp = 999;
        f
    };
    let loafs = |ev: &[Event]| {
        ev.contains(&Event::CantAct {
            by: P,
            why: Blocked::Loafing,
        })
    };
    // Switched in mid-turn: it acts on its first full turn.
    let (mut b, _) = battle(
        Format::Singles,
        vec![fighter("a", [NORMAL; 2], 100, &[wait()]), loafer()],
        vec![foe()],
        1,
    );
    turn(&mut b, &[switch(P, 1), use_move(O, 1)]);
    let ev = turn(&mut b, &[use_move(P, 0), use_move(O, 1)]);
    assert!(!loafs(&ev));
    let ev = turn(&mut b, &[use_move(P, 0), use_move(O, 1)]);
    assert!(loafs(&ev));

    // Sent in after a faint: it loafs first.
    let (mut b, _) = battle(
        Format::Singles,
        vec![fighter("a", [NORMAL; 2], 100, &[wait()]), loafer()],
        vec![foe()],
        1,
    );
    b.member_mut(0, 0).hp = 1;
    turn(&mut b, &[use_move(P, 0), use_move(O, 0)]);
    assert_eq!(b.phase(), Phase::Replacements);
    b.replace(&[(P, 1)]).unwrap();
    let ev = turn(&mut b, &[use_move(P, 0), use_move(O, 1)]);
    assert!(loafs(&ev));
    let ev = turn(&mut b, &[use_move(P, 0), use_move(O, 1)]);
    assert!(!loafs(&ev));
}

#[test]
fn speed_rises_from_the_first_turn_end() {
    let quick = || {
        with_ability(
            fighter("a", [NORMAL; 2], 100, &[wait()]),
            abilities::SPEED_EACH_TURN,
        )
    };
    let raised = |ev: &[Event]| {
        ev.contains(&Event::StatChanged {
            target: P,
            stat: Stat::Speed,
            by: 1,
        })
    };
    let (mut b, _) = battle(
        Format::Singles,
        vec![quick(), quick()],
        vec![fighter("b", [NORMAL; 2], 50, &[wait()])],
        1,
    );
    let ev = turn(&mut b, &[use_move(P, 0), use_move(O, 0)]);
    assert!(raised(&ev));
    // One switched in waits for a turn's end first.
    let ev = turn(&mut b, &[switch(P, 1), use_move(O, 0)]);
    assert!(!raised(&ev));
    let ev = turn(&mut b, &[use_move(P, 0), use_move(O, 0)]);
    assert!(raised(&ev));
}

#[test]
fn extra_pp_and_fast_waking() {
    let mut b = duel(
        fighter("a", [NORMAL; 2], 100, &[mv(1, NORMAL, 10)]),
        with_ability(
            fighter("b", [NORMAL; 2], 50, &[wait()]),
            abilities::COSTS_EXTRA_PP,
        ),
        1,
    );
    turn(&mut b, &[use_move(P, 0), use_move(O, 0)]);
    assert_eq!(b.fighter(P).unwrap().moves[0].pp, 18);

    let mut b = duel(
        with_ability(
            fighter("a", [NORMAL; 2], 100, &[wait()]),
            abilities::WAKES_FAST,
        ),
        fighter("b", [NORMAL; 2], 50, &[wait()]),
        1,
    );
    b.member_mut(0, 0).ailment = Some(Ailment::Sleep);
    b.member_mut(0, 0).sleep_turns = 4;
    let ev = turn(&mut b, &[use_move(P, 0), use_move(O, 0)]);
    assert!(ev.contains(&Event::CantAct {
        by: P,
        why: Blocked::Asleep
    }));
    let ev = turn(&mut b, &[use_move(P, 0), use_move(O, 0)]);
    assert!(ev.contains(&Event::WokeUp { target: P }));
}

#[test]
fn status_shedding_curing_and_sharing() {
    let mut b = duel(
        with_ability(
            fighter("a", [NORMAL; 2], 100, &[wait()]),
            abilities::SHEDS_STATUS,
        ),
        fighter("b", [NORMAL; 2], 50, &[wait()]),
        2,
    );
    b.member_mut(0, 0).ailment = Some(Ailment::Burn);
    let mut turns = 0;
    while b.fighter(P).unwrap().ailment.is_some() {
        turn(&mut b, &[use_move(P, 0), use_move(O, 0)]);
        turns += 1;
        assert!(turns < 30);
    }

    let (mut b, _) = battle(
        Format::Singles,
        vec![
            with_ability(
                fighter("a", [NORMAL; 2], 100, &[wait()]),
                abilities::CURED_ON_SWITCH,
            ),
            fighter("c", [NORMAL; 2], 100, &[wait()]),
        ],
        vec![fighter("b", [NORMAL; 2], 50, &[wait()])],
        1,
    );
    b.member_mut(0, 0).ailment = Some(Ailment::Poison);
    turn(&mut b, &[switch(P, 1), use_move(O, 0)]);
    assert_eq!(b.team(0)[0].ailment, None);

    let poison = MoveData {
        type_id: POISON,
        ..status(1, effects::POISON, Target::Selected)
    };
    let mut b = duel(
        fighter("a", [NORMAL; 2], 100, &[poison]),
        with_ability(
            fighter("b", [NORMAL; 2], 50, &[wait()]),
            abilities::SHARES_STATUS,
        ),
        1,
    );
    turn(&mut b, &[use_move(P, 0), use_move(O, 0)]);
    assert_eq!(b.fighter(O).unwrap().ailment, Some(Ailment::Poison));
    assert_eq!(b.fighter(P).unwrap().ailment, Some(Ailment::Poison));
}

#[test]
fn added_effect_and_flinch_blockers() {
    let burn = MoveData {
        chance: 100,
        ..with(mv(1, FIRE, 10), effects::BURN_CHANCE)
    };
    let flinch_hit = MoveData {
        chance: 100,
        ..with(mv(2, DARK, 10), effects::FLINCH_CHANCE)
    };
    let mut b = duel(
        fighter("a", [NORMAL; 2], 100, &[burn, flinch_hit]),
        with_ability(
            fighter("b", [NORMAL; 2], 50, &[wait()]),
            abilities::NO_ADDED_EFFECTS,
        ),
        1,
    );
    turn(&mut b, &[use_move(P, 0), use_move(O, 0)]);
    assert_eq!(b.fighter(O).unwrap().ailment, None);
    b.member_mut(1, 0).ability = abilities::NO_FLINCH;
    let ev = turn(&mut b, &[use_move(P, 1), use_move(O, 0)]);
    assert!(users(&ev).contains(&O));
}

#[test]
fn recoil_and_drain_abilities() {
    let edge = with(mv(1, NORMAL, 120), effects::RECOIL_THIRD);
    let drain = with(mv(2, GRASS, 60), effects::DRAIN_HALF);
    let mut b = duel(
        with_ability(
            fighter("a", [NORMAL; 2], 100, &[edge, drain]),
            abilities::NO_RECOIL,
        ),
        with_ability(
            fighter("b", [NORMAL; 2], 50, &[wait()]),
            abilities::DRAINS_HURT,
        ),
        1,
    );
    b.member_mut(1, 0).stats.hp = 999;
    b.member_mut(1, 0).hp = 999;
    turn(&mut b, &[use_move(P, 0), use_move(O, 0)]);
    assert_eq!(b.fighter(P).unwrap().hp, 150);
    let ev = turn(&mut b, &[use_move(P, 1), use_move(O, 0)]);
    assert!(ev.iter().any(|e| matches!(
        e,
        Event::Hurt {
            target: P,
            by: HurtBy::DrainBackfire,
            ..
        }
    )));
}

#[test]
fn trapping_abilities() {
    let team = |t: u8| {
        vec![
            fighter("a", [t; 2], 100, &[wait()]),
            fighter("c", [NORMAL; 2], 100, &[wait()]),
        ]
    };
    for (ability, ty, trapped) in [
        (abilities::FOES_CANT_ESCAPE, NORMAL, true),
        (abilities::STEEL_FOES_CANT_ESCAPE, NORMAL, false),
        (abilities::STEEL_FOES_CANT_ESCAPE, STEEL, true),
        (abilities::GROUNDED_FOES_CANT_ESCAPE, NORMAL, true),
        (abilities::GROUNDED_FOES_CANT_ESCAPE, FLYING, false),
    ] {
        let (b, _) = battle(
            Format::Singles,
            team(ty),
            vec![with_ability(
                fighter("b", [NORMAL; 2], 50, &[wait()]),
                ability,
            )],
            1,
        );
        assert_eq!(b.can_switch(P), !trapped, "{ability} {ty}");
    }
}

#[test]
fn types_that_change_with_weather_and_hits() {
    let sun = status(1, effects::SUN, Target::User);
    let (mut b, ev) = battle(
        Format::Singles,
        vec![with_ability(
            fighter("a", [NORMAL; 2], 100, &[sun]),
            abilities::WEATHER_TYPE,
        )],
        vec![with_ability(
            fighter("b", [NORMAL; 2], 50, &[mv(2, WATER, 10)]),
            abilities::TYPE_FROM_HIT,
        )],
        1,
    );
    assert!(!ev.iter().any(|e| matches!(e, Event::TypeChanged { .. })));
    let ev = turn(&mut b, &[use_move(P, 0), use_move(O, 0)]);
    assert!(ev.contains(&Event::TypeChanged {
        target: P,
        types: [FIRE; 2]
    }));
    assert_eq!(b.fighter(P).unwrap().types, [FIRE; 2]);
    // Hit by water, the other one turns water.
    let mut b = duel(
        fighter("a", [NORMAL; 2], 100, &[mv(3, WATER, 10)]),
        with_ability(
            fighter("b", [NORMAL; 2], 50, &[wait()]),
            abilities::TYPE_FROM_HIT,
        ),
        1,
    );
    turn(&mut b, &[use_move(P, 0), use_move(O, 0)]);
    assert_eq!(b.fighter(O).unwrap().types, [WATER; 2]);
}

#[test]
fn accuracy_abilities() {
    let count = |user: u8, target: u8, weather: Weather, m: MoveData| {
        let mut n = 0;
        for seed in 0..300 {
            let mut b = duel(
                with_ability(fighter("a", [NORMAL; 2], 100, &[m]), user),
                with_ability(fighter("b", [ROCK; 2], 50, &[wait()]), target),
                seed,
            );
            b.field_mut().weather = weather;
            b.field_mut().weather_turns = 5;
            let ev = turn(&mut b, &[use_move(P, 0), use_move(O, 0)]);
            n += hits(&ev).len();
        }
        n
    };
    let half = MoveData {
        accuracy: 50,
        ..mv(1, NORMAL, 1)
    };
    let plain = count(0, 0, Weather::Clear, half);
    let sure = count(abilities::SURE_AIM, 0, Weather::Clear, half);
    let veiled = count(0, abilities::SAND_SHELTER, Weather::Sand, half);
    let eager = count(abilities::ATTACK_UP_ACCURACY_DOWN, 0, Weather::Clear, half);
    assert!(sure > plain + 20, "{sure} vs {plain}");
    assert!(veiled + 10 < plain, "{veiled} vs {plain}");
    assert!(eager + 10 < plain, "{eager} vs {plain}");
}

#[test]
fn pair_boosts_in_doubles() {
    let beam = mv(1, ELECTRIC, 80);
    let (mut b, _) = battle(
        Format::Doubles,
        vec![
            with_ability(
                fighter("a", [NORMAL; 2], 100, &[beam]),
                abilities::PAIR_BOOST_A,
            ),
            fighter("c", [NORMAL; 2], 100, &[wait()]),
        ],
        vec![
            fighter("b", [NORMAL; 2], 50, &[wait()]),
            fighter("d", [NORMAL; 2], 50, &[wait()]),
        ],
        1,
    );
    let alone = b.estimate(P, 0, O).0;
    b.member_mut(0, 1).ability = abilities::PAIR_BOOST_B;
    assert!(b.estimate(P, 0, O).0 > alone * 4 / 3);
}

#[test]
fn sound_blockers_with_the_sound_list() {
    let song = status(7, effects::SLEEP, Target::Selected);
    let mut b = duel(
        fighter("a", [NORMAL; 2], 100, &[song]),
        with_ability(
            fighter("b", [NORMAL; 2], 50, &[wait()]),
            abilities::NO_SOUND_MOVES,
        ),
        1,
    );
    b.set_sound_moves(vec![7, 8]);
    let ev = turn(&mut b, &[use_move(P, 0), use_move(O, 0)]);
    assert!(ev.contains(&Event::NoEffect { target: O }));
    assert_eq!(b.fighter(O).unwrap().ailment, None);
    // Damaging sound moves too.
    b.member_mut(0, 0).moves[0].data = mv(8, NORMAL, 90);
    let ev = turn(&mut b, &[use_move(P, 0), use_move(O, 0)]);
    assert!(ev.contains(&Event::Ability {
        at: O,
        ability: abilities::NO_SOUND_MOVES
    }));
    assert!(hits(&ev).is_empty());
    assert_eq!(b.fighter(O).unwrap().hp, 150);
}

#[test]
fn halving_fire_and_ice_and_status_defense() {
    let mut b = duel(
        fighter("a", [NORMAL; 2], 100, &[mv(1, FIRE, 80), mv(2, NORMAL, 80)]),
        fighter("b", [NORMAL; 2], 50, &[wait()]),
        1,
    );
    let (fire, normal) = (b.estimate(P, 0, O).0, b.estimate(P, 1, O).0);
    b.member_mut(1, 0).ability = abilities::HALVES_FIRE_ICE;
    assert!(b.estimate(P, 0, O).0 < fire * 2 / 3);
    b.member_mut(1, 0).ability = abilities::STATUS_BOOSTS_DEFENSE;
    b.member_mut(1, 0).ailment = Some(Ailment::Poison);
    assert!(b.estimate(P, 1, O).0 < normal * 5 / 6);
}
