//! Move effects, a family at a time.

use super::super::data::flags;
use super::super::event::Condition;
use super::*;

fn wait() -> MoveData {
    status(9, effects::NOTHING, Target::User)
}

/// A move shields and bouncing moves care about.
fn guarded(m: MoveData) -> MoveData {
    MoveData {
        flags: m.flags | flags::BLOCKED_BY_PROTECTION,
        ..m
    }
}

fn duel(a: Fighter, b: Fighter, seed: u64) -> Battle {
    battle(Format::Singles, vec![a], vec![b], seed).0
}

fn turn(b: &mut Battle, choices: &[Choice]) -> Vec<Event> {
    b.play_turn(choices).unwrap()
}

fn hp(b: &Battle, at: Slot) -> u16 {
    b.fighter(at).unwrap().hp
}

fn started(ev: &[Event], target: Slot, condition: Condition) -> bool {
    ev.contains(&Event::Started { target, condition })
}

#[test]
fn two_turn_moves_charge_then_hit() {
    let two_turn = with(mv(1, NORMAL, 80), effects::TWO_TURN);
    let mut b = duel(
        fighter("a", [NORMAL; 2], 100, &[two_turn, mv(2, NORMAL, 10)]),
        fighter("b", [NORMAL; 2], 50, &[wait()]),
        1,
    );
    let ev = turn(&mut b, &[use_move(P, 0), use_move(O, 0)]);
    assert!(started(&ev, P, Condition::Readying(1)));
    assert!(hits(&ev).is_empty());
    assert!(b.forced(P));
    assert_eq!(b.legal_actions(P).len(), 1);
    // Its choice can be left out: it hits now, without spending PP again.
    let ev = turn(&mut b, &[use_move(O, 0)]);
    assert_eq!(hits(&ev).len(), 1);
    assert!(!b.forced(P));
    assert_eq!(b.fighter(P).unwrap().moves[0].pp, 19);

    // A sun move doesn't charge in sun; another charging move raises defense first.
    let sun_charged = with(mv(3, GRASS, 120), effects::SUN_CHARGE);
    let bash = with(mv(4, NORMAL, 100), effects::TWO_TURN_DEFENSE_UP);
    let mut b = duel(
        fighter("a", [NORMAL; 2], 100, &[sun_charged, bash]),
        fighter("b", [NORMAL; 2], 50, &[wait()]),
        1,
    );
    b.field_mut().weather = Weather::Sun;
    let ev = turn(&mut b, &[use_move(P, 0), use_move(O, 0)]);
    assert_eq!(hits(&ev).len(), 1);
    let ev = turn(&mut b, &[use_move(P, 1), use_move(O, 0)]);
    assert!(ev.contains(&Event::StatChanged {
        target: P,
        stat: Stat::Defense,
        by: 1
    }));
}

#[test]
fn hiding_moves_dodge_all_but_reaching_moves() {
    let airborne = with(mv(1, FLYING, 90), effects::HIDE_TWO_TURN);
    let air_hit = with(mv(2, FLYING, 40), effects::HITS_AIR);
    let mut b = duel(
        fighter("a", [NORMAL; 2], 100, &[airborne]),
        fighter("b", [NORMAL; 2], 50, &[mv(3, NORMAL, 40), air_hit]),
        1,
    );
    let ev = turn(&mut b, &[use_move(P, 0), use_move(O, 0)]);
    assert!(ev.contains(&Event::Missed { by: O, target: P }));
    assert!(b.fighter(P).unwrap().volatile.hidden.is_some());
    turn(&mut b, &[use_move(O, 0)]);
    assert!(b.fighter(P).unwrap().volatile.hidden.is_none());
    // A move that reaches the air hits at double damage.
    let ev = turn(&mut b, &[use_move(P, 0), use_move(O, 1)]);
    let plain = b.estimate(O, 1, P).0;
    let (_, _, d) = hits(&ev)[0];
    assert!(u32::from(d) > plain * 3 / 2, "{d} vs {plain}");
}

#[test]
fn recharge_after_a_hit() {
    let beam = with(mv(1, NORMAL, 150), effects::RECHARGE_AFTER);
    let mut b = duel(
        fighter("a", [NORMAL; 2], 100, &[beam]),
        fighter("b", [NORMAL; 2], 50, &[wait()]),
        1,
    );
    b.member_mut(1, 0).stats.hp = 999;
    b.member_mut(1, 0).hp = 999;
    turn(&mut b, &[use_move(P, 0), use_move(O, 0)]);
    assert!(b.forced(P));
    let ev = turn(&mut b, &[use_move(O, 0)]);
    assert!(ev.contains(&Event::CantAct {
        by: P,
        why: Blocked::Recharging
    }));
    assert!(!b.forced(P));
}

#[test]
fn shields_block_and_wear_thin() {
    let shield = MoveData {
        priority: 4,
        ..status(1, effects::SHIELD, Target::User)
    };
    let hit = guarded(mv(2, NORMAL, 40));
    let mut b = duel(
        fighter("a", [NORMAL; 2], 100, &[shield]),
        fighter("b", [NORMAL; 2], 50, &[hit]),
        1,
    );
    let ev = turn(&mut b, &[use_move(P, 0), use_move(O, 0)]);
    assert!(started(&ev, P, Condition::Shielded));
    assert!(ev.contains(&Event::Shielded { target: P }));
    assert_eq!(hp(&b, P), 150);

    // Consecutive protection succeeds about one third of the time.
    let mut worked = 0;
    for seed in 0..200 {
        let mut b = duel(
            fighter("a", [NORMAL; 2], 100, &[shield]),
            fighter("b", [NORMAL; 2], 50, &[wait()]),
            seed,
        );
        turn(&mut b, &[use_move(P, 0), use_move(O, 0)]);
        let ev = turn(&mut b, &[use_move(P, 0), use_move(O, 0)]);
        worked += usize::from(started(&ev, P, Condition::Shielded));
    }
    assert!((45..90).contains(&worked), "{worked}");

    // Unlike the old rule, moving last does not make protection fail.
    let slow_shield = status(1, effects::SHIELD, Target::User);
    let mut b = duel(
        fighter("a", [NORMAL; 2], 10, &[slow_shield]),
        fighter("b", [NORMAL; 2], 50, &[wait()]),
        1,
    );
    let ev = turn(&mut b, &[use_move(P, 0), use_move(O, 0)]);
    assert!(started(&ev, P, Condition::Shielded));
}

#[test]
fn bracing_hangs_on_and_one_hp_moves() {
    let brace = MoveData {
        priority: 3,
        ..status(1, effects::BRACE, Target::User)
    };
    let mut b = duel(
        fighter("a", [NORMAL; 2], 100, &[brace]),
        fighter("b", [NORMAL; 2], 50, &[mv(2, NORMAL, 250)]),
        1,
    );
    let ev = turn(&mut b, &[use_move(P, 0), use_move(O, 0)]);
    assert!(ev.contains(&Event::HungOn { target: P }));
    assert_eq!(hp(&b, P), 1);

    let swipe = with(mv(3, NORMAL, 250), effects::LEAVE_ONE_HP);
    let mut b = duel(
        fighter("a", [NORMAL; 2], 100, &[swipe]),
        fighter("b", [NORMAL; 2], 50, &[wait()]),
        1,
    );
    turn(&mut b, &[use_move(P, 0), use_move(O, 0)]);
    assert_eq!(hp(&b, O), 1);
}

#[test]
fn decoys_take_hits_and_stop_status() {
    let decoy = status(1, effects::DECOY, Target::User);
    let sleep = status(2, effects::SLEEP, Target::Selected);
    let mut b = duel(
        fighter("a", [NORMAL; 2], 100, &[decoy]),
        fighter("b", [NORMAL; 2], 50, &[sleep, mv(3, NORMAL, 40)]),
        1,
    );
    let ev = turn(&mut b, &[use_move(P, 0), use_move(O, 0)]);
    assert!(started(&ev, P, Condition::Decoy));
    assert_eq!(hp(&b, P), 150 - 37);
    assert_eq!(b.fighter(P).unwrap().volatile.decoy, 37);
    assert!(ev.contains(&Event::Failed { by: O }));
    assert_eq!(b.fighter(P).unwrap().ailment, None);
    // A second one fails while the first stands.
    let ev = turn(&mut b, &[use_move(P, 0), use_move(O, 1)]);
    assert!(ev.contains(&Event::Failed { by: P }));
    assert!(
        ev.iter()
            .any(|e| matches!(e, Event::DecoyTook { target: P, .. }))
    );
    assert_eq!(hp(&b, P), 150 - 37);
    for _ in 0..4 {
        let ev = turn(&mut b, &[use_move(P, 0), use_move(O, 1)]);
        if ev.contains(&Event::DecoyFaded { target: P }) {
            return;
        }
    }
    panic!("the decoy never faded");
}

#[test]
fn seeds_drain_each_turn() {
    let seed = status(1, effects::SEED_DRAIN, Target::Selected);
    let mut b = duel(
        fighter("a", [NORMAL; 2], 100, &[seed]),
        fighter("b", [NORMAL; 2], 50, &[wait()]),
        1,
    );
    b.member_mut(0, 0).hp = 100;
    let ev = turn(&mut b, &[use_move(P, 0), use_move(O, 0)]);
    assert!(started(&ev, O, Condition::Seeded));
    assert_eq!(hp(&b, O), 150 - 18);
    assert_eq!(hp(&b, P), 118);
    // Again: it already has one. Grass types are immune.
    let ev = turn(&mut b, &[use_move(P, 0), use_move(O, 0)]);
    assert!(ev.contains(&Event::Failed { by: P }));
    let mut b = duel(
        fighter("a", [NORMAL; 2], 100, &[seed]),
        fighter("b", [GRASS; 2], 50, &[wait()]),
        1,
    );
    let ev = turn(&mut b, &[use_move(P, 0), use_move(O, 0)]);
    assert!(ev.contains(&Event::NoEffect { target: O }));
    // Draining a fighter whose ability makes draining hurt hurts the seeder instead.
    let mut b = duel(
        fighter("a", [NORMAL; 2], 100, &[seed]),
        fighter("b", [NORMAL; 2], 50, &[wait()]),
        1,
    );
    b.member_mut(1, 0).ability = abilities::DRAINS_HURT;
    b.member_mut(0, 0).hp = 100;
    let ev = turn(&mut b, &[use_move(P, 0), use_move(O, 0)]);
    assert!(ev.contains(&Event::Hurt {
        target: P,
        damage: 18,
        hp: 82,
        by: HurtBy::DrainBackfire
    }));
    assert_eq!(hp(&b, O), 150 - 18);
}

#[test]
fn binding_and_holding_stop_switching() {
    let trap_hit = MoveData {
        chance: 100,
        ..with(mv(1, NORMAL, 15), effects::TRAP_HIT)
    };
    let hold = status(2, effects::BLOCK_ESCAPE, Target::Selected);
    let team = || {
        vec![
            fighter("b", [NORMAL; 2], 50, &[wait()]),
            fighter("c", [NORMAL; 2], 50, &[wait()]),
        ]
    };
    let (mut b, _) = battle(
        Format::Singles,
        vec![fighter("a", [NORMAL; 2], 100, &[trap_hit, hold])],
        team(),
        1,
    );
    let ev = turn(&mut b, &[use_move(P, 0), use_move(O, 0)]);
    assert!(started(&ev, O, Condition::Bound(1)));
    assert!(ev.contains(&Event::Hurt {
        target: O,
        damage: 18,
        hp: hp(&b, O),
        by: HurtBy::Bound(1)
    }));
    assert!(!b.can_switch(O));
    assert!(
        !b.legal_actions(O)
            .iter()
            .any(|a| matches!(a, Action::Switch { .. }))
    );
    assert_eq!(
        b.play_turn(&[use_move(P, 0), switch(O, 1)]).err(),
        Some(BattleError::Invalid(O))
    );

    let (mut b, _) = battle(
        Format::Singles,
        vec![fighter("a", [NORMAL; 2], 100, &[trap_hit, hold])],
        team(),
        1,
    );
    let ev = turn(&mut b, &[use_move(P, 1), use_move(O, 0)]);
    assert!(started(&ev, O, Condition::CantEscape));
    assert!(!b.can_switch(O));

    // Binding lasts four or five turns.
    let mut seen = std::collections::HashSet::new();
    for seed in 0..40 {
        let (mut b, _) = battle(
            Format::Singles,
            vec![fighter("a", [NORMAL; 2], 100, &[trap_hit, wait()])],
            vec![fighter("b", [NORMAL; 2], 50, &[wait()])],
            seed,
        );
        b.member_mut(1, 0).stats.hp = 999;
        b.member_mut(1, 0).hp = 999;
        let mut hurt = 0;
        let mut index = 0;
        for _ in 0..8 {
            let ev = turn(&mut b, &[use_move(P, index), use_move(O, 0)]);
            index = 1;
            hurt += ev
                .iter()
                .filter(|e| {
                    matches!(
                        e,
                        Event::Hurt {
                            target: O,
                            by: HurtBy::Bound(1),
                            ..
                        }
                    )
                })
                .count();
        }
        assert!((4..=5).contains(&hurt), "{hurt}");
        seen.insert(hurt);
    }
    assert_eq!(seen.len(), 2);
}

#[test]
fn one_hit_knock_outs() {
    let ko = MoveData {
        accuracy: 100,
        ..with(mv(1, NORMAL, 1), effects::ONE_HIT_KO)
    };
    let mut b = duel(
        fighter("a", [NORMAL; 2], 100, &[ko]),
        fighter("b", [NORMAL; 2], 50, &[wait()]),
        1,
    );
    b.member_mut(0, 0).level = 60;
    let ev = turn(&mut b, &[use_move(P, 0), use_move(O, 0)]);
    assert!(ev.contains(&Event::KnockedOut { target: O }));
    assert_eq!(b.phase(), Phase::Over(Outcome::Won(0)));

    // A higher-level target, and the ability that stops it.
    let mut b = duel(
        fighter("a", [NORMAL; 2], 100, &[ko]),
        fighter("b", [NORMAL; 2], 50, &[wait()]),
        1,
    );
    b.member_mut(1, 0).level = 60;
    let ev = turn(&mut b, &[use_move(P, 0), use_move(O, 0)]);
    assert!(ev.contains(&Event::Failed { by: P }));
    b.member_mut(1, 0).level = 40;
    b.member_mut(1, 0).ability = abilities::NO_ONE_HIT_KO;
    let ev = turn(&mut b, &[use_move(P, 0), use_move(O, 0)]);
    assert!(ev.contains(&Event::NoEffect { target: O }));
    assert_eq!(hp(&b, O), 150);
}

#[test]
fn returning_damage_doubles_what_it_took() {
    let return_hit = MoveData {
        priority: -5,
        target: Target::Depends,
        ..with(mv(1, FIGHTING, 1), effects::RETURN_PHYSICAL)
    };
    let mut b = duel(
        fighter("a", [NORMAL; 2], 100, &[return_hit]),
        fighter("b", [NORMAL; 2], 50, &[mv(2, NORMAL, 40), mv(3, WATER, 40)]),
        1,
    );
    b.member_mut(1, 0).stats.hp = 999;
    b.member_mut(1, 0).hp = 999;
    let ev = turn(&mut b, &[use_move(P, 0), use_move(O, 0)]);
    let h = hits(&ev);
    assert_eq!(h[0].0, O);
    assert_eq!(h[1], (P, O, h[0].2 * 2));
    // A special hit gives it nothing to return.
    let ev = turn(&mut b, &[use_move(P, 0), use_move(O, 1)]);
    assert!(ev.contains(&Event::Failed { by: P }));
    // Only the last hit counts.
    b.member_mut(1, 0).moves[1].data = with(mv(3, NORMAL, 40), effects::TWO_HITS);
    let ev = turn(&mut b, &[use_move(P, 0), use_move(O, 1)]);
    let h = hits(&ev);
    assert_eq!(h.len(), 3);
    assert_eq!(h[2], (P, O, h[1].2 * 2));
}

#[test]
fn rampage_locks_then_confuses() {
    let rampage = MoveData {
        chance: 100,
        target: Target::RandomFoe,
        ..with(mv(1, NORMAL, 20), effects::RAMPAGE)
    };
    let mut b = duel(
        fighter("a", [NORMAL; 2], 100, &[rampage, mv(2, NORMAL, 10)]),
        fighter("b", [NORMAL; 2], 50, &[wait()]),
        3,
    );
    b.member_mut(1, 0).stats.hp = 999;
    b.member_mut(1, 0).hp = 999;
    turn(&mut b, &[use_move(P, 0), use_move(O, 0)]);
    let mut uses = 1;
    while b.forced(P) {
        // Whatever it picks, it carries on.
        let ev = turn(&mut b, &[use_move(P, 1), use_move(O, 0)]);
        uses += 1;
        assert!(ev.contains(&Event::UsedMove { by: P, move_id: 1 }));
        if !b.forced(P) {
            assert!(ev.contains(&Event::Confused { target: P }));
        }
    }
    assert!((2..=3).contains(&uses), "{uses}");
}

#[test]
fn rolling_and_growing_moves_get_stronger() {
    for effect in [effects::ROLLING, effects::GROWING_HIT] {
        let roll = with(mv(1, ROCK, 10), effect);
        let mut b = duel(
            fighter("a", [NORMAL; 2], 100, &[roll]),
            fighter("b", [NORMAL; 2], 50, &[wait()]),
            1,
        );
        b.member_mut(1, 0).stats.hp = 999;
        b.member_mut(1, 0).hp = 999;
        b.member_mut(1, 0).ability = abilities::NO_CRITS_A;
        let mut last = 0;
        for _ in 0..4 {
            let ev = turn(&mut b, &[use_move(P, 0), use_move(O, 0)]);
            let d = hits(&ev)[0].2;
            assert!(d > last, "{effect}: {d} after {last}");
            last = d;
        }
    }
}

#[test]
fn storing_returns_double() {
    let store = MoveData {
        target: Target::User,
        priority: 1,
        ..with(mv(1, NORMAL, 1), effects::STORE_DAMAGE)
    };
    let mut b = duel(
        fighter("a", [NORMAL; 2], 100, &[store]),
        fighter("b", [NORMAL; 2], 50, &[mv(2, NORMAL, 20)]),
        1,
    );
    b.member_mut(1, 0).stats.hp = 999;
    b.member_mut(1, 0).hp = 999;
    let ev = turn(&mut b, &[use_move(P, 0), use_move(O, 0)]);
    assert!(started(&ev, P, Condition::Storing));
    let mut taken = u32::from(hits(&ev)[0].2);
    let ev = turn(&mut b, &[use_move(O, 0)]);
    assert!(ev.contains(&Event::CantAct {
        by: P,
        why: Blocked::Storing
    }));
    taken += u32::from(hits(&ev)[0].2);
    let ev = turn(&mut b, &[use_move(O, 0)]);
    let back = hits(&ev)[0];
    assert_eq!(back.0, P);
    assert_eq!(u32::from(back.2), taken * 2);
}

#[test]
fn seals_repeats_provokes_and_no_repeats() {
    let seal = status(1, effects::SEAL_LAST_MOVE, Target::Selected);
    let repeat = status(2, effects::REPEAT_LAST, Target::Selected);
    let provoke = MoveData {
        priority: 1,
        ..status(3, effects::PROVOKE, Target::Selected)
    };
    let no_repeats = status(4, effects::NO_REPEATS, Target::Selected);
    let foe = || fighter("b", [NORMAL; 2], 100, &[mv(5, NORMAL, 10), wait()]);
    let me = || fighter("a", [NORMAL; 2], 50, &[seal, repeat, provoke, no_repeats]);

    // Sealed: the move can't be picked.
    let mut b = duel(me(), foe(), 1);
    let ev = turn(&mut b, &[use_move(P, 0), use_move(O, 0)]);
    assert!(started(&ev, O, Condition::Sealed(5)));
    assert!(!b.usable(O, 0));
    assert_eq!(
        b.play_turn(&[use_move(P, 0), use_move(O, 0)]).err(),
        Some(BattleError::Invalid(O))
    );

    // Repeating: another pick turns into the last move.
    let mut b = duel(me(), foe(), 1);
    let ev = turn(&mut b, &[use_move(P, 1), use_move(O, 0)]);
    assert!(started(&ev, O, Condition::Repeating));
    let ev = turn(&mut b, &[use_move(P, 1), use_move(O, 1)]);
    assert!(ev.contains(&Event::UsedMove { by: O, move_id: 5 }));
    assert_eq!(
        b.legal_actions(O)[0],
        Action::Move {
            index: 0,
            target: None
        }
    );

    // Provoked: status moves fail.
    let mut b = duel(me(), foe(), 1);
    let ev = turn(&mut b, &[use_move(P, 2), use_move(O, 1)]);
    assert!(started(&ev, O, Condition::Provoked));
    assert!(ev.contains(&Event::CantAct {
        by: O,
        why: Blocked::Provoked
    }));
    assert!(!b.usable(O, 1));

    // No repeats: the same move can't come twice in a row.
    let mut b = duel(me(), foe(), 1);
    turn(&mut b, &[use_move(P, 3), use_move(O, 0)]);
    assert!(!b.usable(O, 0) && b.usable(O, 1));
}

#[test]
fn drowsy_targets_fall_asleep_a_turn_later() {
    let drowse = status(1, effects::DROWSE, Target::Selected);
    let mut b = duel(
        fighter("a", [NORMAL; 2], 100, &[drowse, wait()]),
        fighter("b", [NORMAL; 2], 50, &[wait()]),
        1,
    );
    let ev = turn(&mut b, &[use_move(P, 0), use_move(O, 0)]);
    assert!(started(&ev, O, Condition::Drowsy));
    assert_eq!(b.fighter(O).unwrap().ailment, None);
    let ev = turn(&mut b, &[use_move(P, 1), use_move(O, 0)]);
    assert!(ev.contains(&Event::Ailment {
        target: O,
        ailment: Ailment::Sleep
    }));
}

#[test]
fn heals_that_wait_and_roots() {
    let delayed_heal = status(1, effects::DELAYED_HEAL, Target::User);
    let root = status(2, effects::ROOT, Target::User);
    let mut b = battle(
        Format::Singles,
        vec![
            fighter("a", [NORMAL; 2], 100, &[delayed_heal, root]),
            fighter("c", [NORMAL; 2], 100, &[wait()]),
        ],
        vec![fighter("b", [NORMAL; 2], 50, &[wait()])],
        1,
    )
    .0;
    b.member_mut(0, 0).hp = 10;
    let ev = turn(&mut b, &[use_move(P, 0), use_move(O, 0)]);
    assert!(started(&ev, P, Condition::HealPending));
    assert_eq!(hp(&b, P), 10);
    turn(&mut b, &[use_move(P, 1), use_move(O, 0)]);
    // 75 from the delayed heal, 9 from the roots.
    assert_eq!(hp(&b, P), 10 + 75 + 9);
    assert!(!b.can_switch(P));
}

#[test]
fn hitting_a_switching_foe() {
    let chase = with(mv(1, DARK, 40), effects::HITS_SWITCHING);
    let (mut b, _) = battle(
        Format::Singles,
        vec![fighter("a", [NORMAL; 2], 10, &[chase])],
        vec![
            fighter("b", [NORMAL; 2], 50, &[wait()]),
            fighter("c", [NORMAL; 2], 50, &[wait()]),
        ],
        1,
    );
    let plain = b.estimate(P, 0, O).0;
    let ev = turn(&mut b, &[use_move(P, 0), switch(O, 1)]);
    let hit_at = ev
        .iter()
        .position(|e| matches!(e, Event::Hit { .. }))
        .unwrap();
    let out_at = ev
        .iter()
        .position(|e| matches!(e, Event::Withdrawn { .. }))
        .unwrap();
    assert!(hit_at < out_at);
    assert!(b.team(1)[0].hp < 150);
    assert_eq!(b.team(1)[1].hp, 150);
    let (_, _, d) = hits(&ev)[0];
    assert!(u32::from(d) > plain * 3 / 2, "{d} vs {plain}");
    assert_eq!(users(&ev), vec![P]);
}

#[test]
fn calling_moves() {
    let random = status(1, effects::RANDOM_MOVE, Target::User);
    let copy = status(2, effects::COPY_FOES_MOVE, Target::User);
    let callable = MoveData {
        flags: flags::COPYABLE,
        ..mv(30, NORMAL, 40)
    };
    let mut b = duel(
        fighter("a", [NORMAL; 2], 50, &[random, copy]),
        fighter("b", [NORMAL; 2], 100, &[callable]),
        1,
    );
    b.set_move_table(vec![
        random,
        with(mv(31, NORMAL, 0), effects::SHIELD),
        mv(32, FIRE, 50),
    ]);
    let ev = turn(&mut b, &[use_move(P, 0), use_move(O, 0)]);
    assert!(ev.contains(&Event::Called { by: P, move_id: 32 }));
    assert!(ev.contains(&Event::UsedMove { by: P, move_id: 32 }));
    // Copying the last move aimed at it.
    let ev = turn(&mut b, &[use_move(P, 1), use_move(O, 0)]);
    assert!(ev.contains(&Event::Called { by: P, move_id: 30 }));
    assert!(hits(&ev).iter().any(|h| h.0 == P));

    // Asleep, the sleeping-random move picks another of its moves; the sleep-only hit works.
    let sleep_call = status(3, effects::SLEEP_RANDOM_MOVE, Target::User);
    let sleep_hit = MoveData {
        chance: 0,
        ..with(mv(4, NORMAL, 40), effects::SLEEP_FLINCH_HIT)
    };
    let mut b = duel(
        fighter(
            "a",
            [NORMAL; 2],
            100,
            &[sleep_call, sleep_hit, mv(5, FIRE, 40)],
        ),
        fighter("b", [NORMAL; 2], 50, &[wait()]),
        1,
    );
    let ev = turn(&mut b, &[use_move(P, 1), use_move(O, 0)]);
    assert!(ev.contains(&Event::Failed { by: P }));
    b.member_mut(0, 0).ailment = Some(Ailment::Sleep);
    b.member_mut(0, 0).sleep_turns = 5;
    let ev = turn(&mut b, &[use_move(P, 0), use_move(O, 0)]);
    assert!(ev.iter().any(|e| matches!(
        e,
        Event::Called {
            by: P,
            move_id: 4 | 5
        }
    )));
    let ev = turn(&mut b, &[use_move(P, 1), use_move(O, 0)]);
    assert_eq!(hits(&ev).len(), 1);
}

#[test]
fn delayed_attacks_land_two_turns_later() {
    let later = with(mv(1, PSYCHIC, 80), effects::DELAYED_HIT);
    let mut b = duel(
        fighter("a", [NORMAL; 2], 100, &[later]),
        fighter("b", [NORMAL; 2], 50, &[wait()]),
        1,
    );
    let ev = turn(&mut b, &[use_move(P, 0), use_move(O, 0)]);
    assert!(started(&ev, O, Condition::Delayed(1)));
    let ev = turn(&mut b, &[use_move(P, 0), use_move(O, 0)]);
    assert!(ev.contains(&Event::Failed { by: P }));
    let ev = turn(&mut b, &[use_move(P, 0), use_move(O, 0)]);
    assert!(ev.iter().any(|e| matches!(
        e,
        Event::Hurt {
            target: O,
            by: HurtBy::Delayed(1),
            ..
        }
    )));
}

#[test]
fn team_hits_one_per_healthy_member() {
    let team = with(mv(1, DARK, 10), effects::TEAM_HITS);
    let mut sick = fighter("c", [NORMAL; 2], 100, &[wait()]);
    sick.ailment = Some(Ailment::Poison);
    let (mut b, _) = battle(
        Format::Singles,
        vec![
            fighter("a", [NORMAL; 2], 100, &[team]),
            sick,
            fighter("d", [NORMAL; 2], 100, &[wait()]),
        ],
        vec![fighter("b", [NORMAL; 2], 50, &[wait()])],
        1,
    );
    let ev = turn(&mut b, &[use_move(P, 0), use_move(O, 0)]);
    assert!(ev.contains(&Event::TeamHit { member: 0 }));
    assert!(ev.contains(&Event::TeamHit { member: 2 }));
    assert!(!ev.contains(&Event::TeamHit { member: 1 }));
    assert!(ev.contains(&Event::HitTimes { count: 2 }));
}

#[test]
fn passing_switches_and_forcing_out() {
    let up = status(1, effects::USER_UP2_FIRST, Target::User);
    let pass = status(2, effects::PASS_STATE, Target::User);
    let (mut b, _) = battle(
        Format::Singles,
        vec![
            fighter("a", [NORMAL; 2], 100, &[up, pass]),
            fighter("c", [NORMAL; 2], 100, &[wait()]),
        ],
        vec![fighter("b", [NORMAL; 2], 50, &[wait()])],
        1,
    );
    turn(&mut b, &[use_move(P, 0), use_move(O, 0)]);
    let ev = turn(&mut b, &[use_move(P, 1), use_move(O, 0)]);
    assert!(ev.contains(&Event::Passing { at: P }));
    assert_eq!(b.member(P), Some(1));
    assert_eq!(b.fighter(P).unwrap().stage(Stat::Attack), 2);
    assert_eq!(b.team(0)[0].stage(Stat::Attack), 0);

    // The player picks who gets it, over a healthier member.
    let (mut b, _) = battle(
        Format::Singles,
        vec![
            fighter("a", [NORMAL; 2], 100, &[up, pass]),
            fighter("c", [NORMAL; 2], 100, &[wait()]),
            fighter("d", [NORMAL; 2], 100, &[wait()]),
        ],
        vec![fighter("b", [NORMAL; 2], 50, &[wait()])],
        1,
    );
    b.member_mut(0, 2).hp = 10;
    turn(&mut b, &[use_move(P, 0), use_move(O, 0)]);
    b.pick_pass(P, 2);
    turn(&mut b, &[use_move(P, 1), use_move(O, 0)]);
    assert_eq!(b.member(P), Some(2));
    assert_eq!(b.fighter(P).unwrap().stage(Stat::Attack), 2);

    let force_out = MoveData {
        priority: -6,
        ..status(3, effects::FORCE_SWITCH, Target::Selected)
    };
    let foes = || {
        vec![
            fighter("b", [NORMAL; 2], 50, &[wait()]),
            fighter("c", [NORMAL; 2], 50, &[wait()]),
        ]
    };
    let (mut b, _) = battle(
        Format::Singles,
        vec![fighter("a", [NORMAL; 2], 100, &[force_out])],
        foes(),
        1,
    );
    let ev = turn(&mut b, &[use_move(P, 0), use_move(O, 0)]);
    assert!(ev.contains(&Event::ForcedOut { at: O, member: 0 }));
    assert_eq!(b.member(O), Some(1));
    // Rooted, or with the ability against it, it stays.
    let (mut b, _) = battle(
        Format::Singles,
        vec![fighter("a", [NORMAL; 2], 100, &[force_out])],
        foes(),
        1,
    );
    b.member_mut(1, 0).ability = abilities::NO_FORCED_SWITCH;
    let ev = turn(&mut b, &[use_move(P, 0), use_move(O, 0)]);
    assert!(ev.contains(&Event::Failed { by: P }));
    assert_eq!(b.member(O), Some(0));
}

#[test]
fn hazards_hurt_whoever_comes_in() {
    let hazard = MoveData {
        target: Target::FoesField,
        ..status(1, effects::HAZARD, Target::FoesField)
    };
    let (mut b, _) = battle(
        Format::Singles,
        vec![fighter("a", [NORMAL; 2], 100, &[hazard])],
        vec![
            fighter("b", [NORMAL; 2], 50, &[wait()]),
            fighter("c", [NORMAL; 2], 50, &[wait()]),
            fighter("d", [FLYING; 2], 50, &[wait()]),
        ],
        1,
    );
    let ev = turn(&mut b, &[use_move(P, 0), use_move(O, 0)]);
    assert!(ev.contains(&Event::HazardLaid { side: 1, layers: 1 }));
    let ev = turn(&mut b, &[use_move(P, 0), switch(O, 1)]);
    assert!(ev.contains(&Event::Hurt {
        target: O,
        damage: 18,
        hp: 132,
        by: HurtBy::Hazard
    }));
    // Two layers: 1/6. Flying fighters are spared.
    let ev = turn(&mut b, &[use_move(P, 0), switch(O, 0)]);
    assert!(ev.contains(&Event::Hurt {
        target: O,
        damage: 25,
        hp: 125,
        by: HurtBy::Hazard
    }));
    let ev = turn(&mut b, &[use_move(P, 0), switch(O, 2)]);
    assert!(
        !ev.iter()
            .any(|e| matches!(e, Event::Hurt { target: O, .. }))
    );
    // Freeing moves clear the user's side.
    let spin = with(mv(2, NORMAL, 20), effects::CLEAR_HAZARDS);
    let mut b = duel(
        fighter("a", [NORMAL; 2], 100, &[spin]),
        fighter("b", [NORMAL; 2], 50, &[wait()]),
        1,
    );
    b.field_mut().sides[0].hazards = 2;
    let ev = turn(&mut b, &[use_move(P, 0), use_move(O, 0)]);
    assert!(ev.contains(&Event::Freed { at: P }));
    assert_eq!(b.field().sides[0].hazards, 0);
}

#[test]
fn countdowns_and_vengeance() {
    let count = status(1, effects::COUNTDOWN, Target::User);
    let mut b = duel(
        fighter("a", [NORMAL; 2], 100, &[count, wait()]),
        fighter("b", [NORMAL; 2], 50, &[wait()]),
        1,
    );
    turn(&mut b, &[use_move(P, 0), use_move(O, 0)]);
    let mut turns = 1;
    while b.phase() == Phase::Actions {
        turn(&mut b, &[use_move(P, 1), use_move(O, 0)]);
        turns += 1;
    }
    assert_eq!(turns, 4);
    assert_eq!(b.phase(), Phase::Over(Outcome::Draw));

    let bond = status(2, effects::VENGEANCE, Target::User);
    let mut b = duel(
        fighter("a", [NORMAL; 2], 100, &[bond]),
        fighter("b", [NORMAL; 2], 50, &[mv(3, NORMAL, 250)]),
        1,
    );
    let ev = turn(&mut b, &[use_move(P, 0), use_move(O, 0)]);
    assert!(ev.iter().any(|e| matches!(
        e,
        Event::Hurt {
            target: O,
            by: HurtBy::Vengeance,
            ..
        }
    )));
    assert_eq!(b.phase(), Phase::Over(Outcome::Draw));

    let rancor = status(4, effects::PP_RANCOR, Target::User);
    let (mut b, _) = battle(
        Format::Singles,
        vec![
            fighter("a", [NORMAL; 2], 100, &[rancor]),
            fighter("c", [NORMAL; 2], 100, &[wait()]),
        ],
        vec![fighter("b", [NORMAL; 2], 50, &[mv(3, NORMAL, 250)])],
        1,
    );
    turn(&mut b, &[use_move(P, 0), use_move(O, 0)]);
    assert_eq!(b.fighter(O).unwrap().moves[0].pp, 0);
}

#[test]
fn power_from_the_battle() {
    let foe = || fighter("b", [NORMAL; 2], 50, &[wait()]);
    let low_hp_hit = with(mv(1, NORMAL, 1), effects::LOW_HP_POWER);
    let hp_hit = with(mv(2, FIRE, 150), effects::HP_POWER);
    let bond = with(mv(3, NORMAL, 1), effects::BOND_POWER);
    let status_hit = with(mv(4, NORMAL, 70), effects::STATUS_POWER);
    let mut b = duel(
        fighter(
            "a",
            [NORMAL; 2],
            100,
            &[low_hp_hit, hp_hit, bond, status_hit],
        ),
        foe(),
        1,
    );
    let (full_low_hp, full_erupt, bond_high, status_ok) = (
        b.estimate(P, 0, O).0,
        b.estimate(P, 1, O).0,
        b.estimate(P, 2, O).0,
        b.estimate(P, 3, O).0,
    );
    b.member_mut(0, 0).hp = 5;
    b.member_mut(0, 0).friendship = 0;
    b.member_mut(0, 0).ailment = Some(Ailment::Poison);
    assert!(b.estimate(P, 0, O).0 > full_low_hp * 5);
    assert!(b.estimate(P, 1, O).0 < full_erupt / 10);
    assert!(b.estimate(P, 2, O).0 < bond_high / 10);
    let ev = turn(&mut b, &[use_move(P, 3), use_move(O, 0)]);
    let (_, _, d) = hits(&ev)[0];
    assert!(u32::from(d) > status_ok * 3 / 2, "{d} vs {status_ok}");

    // Weather power: type and power from the weather.
    let ball = with(mv(5, NORMAL, 50), effects::WEATHER_POWER);
    let mut b = duel(fighter("a", [ELECTRIC; 2], 100, &[ball]), foe(), 1);
    let clear = b.estimate(P, 0, O).0;
    b.field_mut().weather = Weather::Rain;
    // Water, doubled, boosted by rain.
    assert!(b.estimate(P, 0, O).0 > clear * 2);

    // Matching HP.
    let match_hp = with(mv(6, NORMAL, 1), effects::MATCH_HP);
    let mut b = duel(
        fighter("a", [NORMAL; 2], 100, &[match_hp]),
        fighter("b", [NORMAL; 2], 50, &[mv(8, NORMAL, 10)]),
        1,
    );
    b.member_mut(0, 0).hp = 30;
    turn(&mut b, &[use_move(P, 0), use_move(O, 0)]);
    assert_eq!(hp(&b, O), 30);
}

#[test]
fn hurt_first_power_and_paralysis_cures() {
    let hit_back = MoveData {
        priority: -4,
        ..with(mv(1, FIGHTING, 60), effects::HURT_FIRST_POWER)
    };
    let cure_hit = with(mv(2, NORMAL, 60), effects::PARALYSIS_POWER_CURE);
    let mut b = duel(
        fighter("a", [NORMAL; 2], 100, &[hit_back, cure_hit]),
        fighter("b", [NORMAL; 2], 50, &[mv(3, NORMAL, 10), wait()]),
        1,
    );
    b.member_mut(1, 0).stats.hp = 999;
    b.member_mut(1, 0).hp = 999;
    b.member_mut(1, 0).ability = abilities::NO_CRITS_A;
    let calm = b.estimate(P, 0, O).0;
    let ev = turn(&mut b, &[use_move(P, 0), use_move(O, 0)]);
    let mine = hits(&ev).into_iter().find(|h| h.0 == P).unwrap().2;
    assert!(u32::from(mine) > calm * 3 / 2, "{mine} vs {calm}");

    let plain = b.estimate(P, 1, O).0;
    b.member_mut(1, 0).ailment = Some(Ailment::Paralysis);
    assert!(b.estimate(P, 1, O).0 >= plain);
    let ev = turn(&mut b, &[use_move(P, 1), use_move(O, 1)]);
    assert!(ev.contains(&Event::Cured {
        target: O,
        ailment: Ailment::Paralysis
    }));
    let (_, _, d) = hits(&ev)[0];
    assert!(u32::from(d) > plain * 3 / 2, "{d} vs {plain}");
}

#[test]
fn crashing_on_a_miss() {
    let kick = with(mv(1, FIGHTING, 70), effects::CRASH_ON_MISS);
    let mut b = duel(
        fighter("a", [NORMAL; 2], 100, &[kick]),
        fighter("b", [NORMAL; 2], 50, &[wait()]),
        1,
    );
    b.member_mut(1, 0).evasion_edge = 100;
    let ev = turn(&mut b, &[use_move(P, 0), use_move(O, 0)]);
    assert!(ev.contains(&Event::Missed { by: P, target: O }));
    assert!(ev.iter().any(|e| matches!(
        e,
        Event::Hurt {
            target: P,
            by: HurtBy::Crash,
            ..
        }
    )));

    // A target the move can't affect costs nothing, hit or miss.
    for edge in [0, 100] {
        let mut b = duel(
            fighter("a", [NORMAL; 2], 100, &[kick]),
            fighter("b", [GHOST; 2], 50, &[wait()]),
            1,
        );
        b.member_mut(1, 0).evasion_edge = edge;
        let ev = turn(&mut b, &[use_move(P, 0), use_move(O, 0)]);
        assert!(
            !ev.iter()
                .any(|e| matches!(e, Event::Hurt { target: P, .. }))
        );
        assert_eq!(hp(&b, P), b.fighter(P).unwrap().stats.hp);
    }
}

#[test]
fn taking_forms_moves_types_and_abilities() {
    let form = status(1, effects::TAKE_FORM, Target::Selected);
    let copy_temp = status(2, effects::COPY_MOVE_TEMP, Target::Selected);
    let copy_keep = status(3, effects::COPY_MOVE_KEEP, Target::Selected);
    let team = || {
        vec![
            fighter("a", [NORMAL; 2], 50, &[form, copy_temp, copy_keep]),
            fighter("c", [NORMAL; 2], 50, &[wait()]),
        ]
    };
    let foe = || {
        let mut f = fighter("b", [FIRE; 2], 100, &[mv(20, FIRE, 40), mv(21, WATER, 40)]);
        f.ability = abilities::NO_BURN;
        f.stats.attack = 222;
        f
    };
    let (mut b, _) = battle(Format::Singles, team(), vec![foe()], 1);
    let ev = turn(&mut b, &[use_move(P, 0), use_move(O, 0)]);
    assert!(ev.contains(&Event::TookForm { by: P, into: O }));
    let f = b.fighter(P).unwrap();
    assert_eq!(
        (f.types, f.ability, f.stats.attack),
        ([FIRE; 2], abilities::NO_BURN, 222)
    );
    assert_eq!(
        f.moves
            .iter()
            .map(|m| (m.data.id, m.pp))
            .collect::<Vec<_>>(),
        vec![(20, 5), (21, 5)]
    );
    assert_eq!(f.stats.hp, 150);
    // Switching out brings its own form back.
    turn(&mut b, &[switch(P, 1), use_move(O, 0)]);
    let f = &b.team(0)[0];
    assert_eq!(
        (f.types, f.ability, f.moves[0].data.id),
        ([NORMAL; 2], 0, 1)
    );

    // A copied move lasts until it leaves; a kept one stays.
    let (mut b, _) = battle(Format::Singles, team(), vec![foe()], 1);
    let ev = turn(&mut b, &[use_move(P, 1), use_move(O, 1)]);
    assert!(ev.contains(&Event::MoveCopied { by: P, move_id: 21 }));
    assert_eq!(b.fighter(P).unwrap().moves[1].data.id, 21);
    turn(&mut b, &[use_move(P, 2), use_move(O, 0)]);
    assert_eq!(b.fighter(P).unwrap().moves[2].data.id, 20);
    turn(&mut b, &[switch(P, 1), use_move(O, 0)]);
    let ids: Vec<u16> = b.team(0)[0].moves.iter().map(|m| m.data.id).collect();
    assert_eq!(ids, vec![1, 2, 20]);
}

#[test]
fn type_changes() {
    let convert = status(1, effects::TYPE_FROM_OWN_MOVE, Target::User);
    let resist = status(2, effects::TYPE_RESIST_LAST, Target::User);
    let arena = status(3, effects::ARENA_TYPE, Target::User);
    let mut b = duel(
        fighter(
            "a",
            [NORMAL; 2],
            50,
            &[convert, resist, arena, mv(4, ICE, 40)],
        ),
        fighter("b", [NORMAL; 2], 100, &[mv(5, FIRE, 10)]),
        1,
    );
    let ev = turn(&mut b, &[use_move(P, 0), use_move(O, 0)]);
    assert!(ev.contains(&Event::TypeChanged {
        target: P,
        types: [ICE; 2]
    }));
    // Hit by fire, it takes a type that resists fire (water, or fire itself).
    turn(&mut b, &[use_move(P, 1), use_move(O, 0)]);
    let t = b.fighter(P).unwrap().types[0];
    assert!(t == WATER || t == FIRE, "{t}");
    turn(&mut b, &[use_move(P, 2), use_move(O, 0)]);
    assert_eq!(b.fighter(P).unwrap().types, [NORMAL; 2]);
}

#[test]
fn ability_copying_and_swapping() {
    let copy = status(1, effects::COPY_ABILITY, Target::Selected);
    let swap = status(2, effects::SWAP_ABILITIES, Target::Selected);
    let mut b = duel(
        fighter("a", [NORMAL; 2], 100, &[copy, swap]),
        fighter("b", [NORMAL; 2], 50, &[wait()]),
        1,
    );
    b.member_mut(1, 0).ability = abilities::NO_PARALYSIS;
    let ev = turn(&mut b, &[use_move(P, 0), use_move(O, 0)]);
    assert!(ev.contains(&Event::AbilityChanged {
        target: P,
        ability: abilities::NO_PARALYSIS
    }));
    b.member_mut(1, 0).ability = abilities::NO_BURN;
    turn(&mut b, &[use_move(P, 1), use_move(O, 0)]);
    assert_eq!(b.fighter(P).unwrap().ability, abilities::NO_BURN);
    assert_eq!(b.fighter(O).unwrap().ability, abilities::NO_PARALYSIS);
    // The only-super-effective ability can't be copied.
    b.member_mut(1, 0).ability = abilities::ONLY_SUPER_EFFECTIVE;
    let ev = turn(&mut b, &[use_move(P, 0), use_move(O, 0)]);
    assert!(ev.contains(&Event::Failed { by: P }));
    let ev = turn(&mut b, &[use_move(P, 1), use_move(O, 0)]);
    assert!(ev.contains(&Event::Failed { by: P }));
    // A swap with no ability on one side hands it over; with none on either, it fails.
    b.member_mut(1, 0).ability = abilities::NONE;
    turn(&mut b, &[use_move(P, 1), use_move(O, 0)]);
    assert_eq!(b.fighter(P).unwrap().ability, abilities::NONE);
    assert_eq!(b.fighter(O).unwrap().ability, abilities::NO_BURN);
    b.member_mut(1, 0).ability = abilities::NONE;
    let ev = turn(&mut b, &[use_move(P, 1), use_move(O, 0)]);
    assert!(ev.contains(&Event::Failed { by: P }));
}

#[test]
fn stores_release_and_heal() {
    let store = status(1, effects::STORE_UP, Target::User);
    let release = with(mv(2, NORMAL, 100), effects::RELEASE_HIT);
    let heal = status(3, effects::RELEASE_HEAL, Target::User);
    let mut b = duel(
        fighter("a", [NORMAL; 2], 100, &[store, release, heal]),
        fighter("b", [NORMAL; 2], 50, &[wait()]),
        1,
    );
    b.member_mut(1, 0).stats.hp = 999;
    b.member_mut(1, 0).hp = 999;
    let ev = turn(&mut b, &[use_move(P, 1), use_move(O, 0)]);
    assert!(ev.contains(&Event::Failed { by: P }));
    for n in 1..=3 {
        let ev = turn(&mut b, &[use_move(P, 0), use_move(O, 0)]);
        assert!(started(&ev, P, Condition::Stored(n)));
    }
    let ev = turn(&mut b, &[use_move(P, 0), use_move(O, 0)]);
    assert!(ev.contains(&Event::Failed { by: P }));
    let three = b.estimate(P, 1, O).0;
    turn(&mut b, &[use_move(P, 1), use_move(O, 0)]);
    assert_eq!(b.fighter(P).unwrap().volatile.stored, 0);
    assert!(three > 100);
    turn(&mut b, &[use_move(P, 0), use_move(O, 0)]);
    b.member_mut(0, 0).hp = 50;
    let ev = turn(&mut b, &[use_move(P, 2), use_move(O, 0)]);
    assert!(ev.contains(&Event::Healed {
        target: P,
        amount: 37,
        hp: 87
    }));
}

#[test]
fn focused_and_first_turn_moves() {
    let focus = with(mv(1, FIGHTING, 150), effects::FOCUSED_HIT);
    let first_turn = MoveData {
        priority: 1,
        ..with(mv(2, NORMAL, 40), effects::FIRST_TURN_FLINCH)
    };
    let mut b = duel(
        fighter("a", [NORMAL; 2], 10, &[focus, first_turn]),
        fighter("b", [NORMAL; 2], 50, &[mv(3, NORMAL, 10), wait()]),
        1,
    );
    let ev = turn(&mut b, &[use_move(P, 0), use_move(O, 0)]);
    assert!(started(&ev, P, Condition::Focusing));
    assert!(ev.contains(&Event::CantAct {
        by: P,
        why: Blocked::LostFocus
    }));
    let ev = turn(&mut b, &[use_move(P, 0), use_move(O, 1)]);
    assert!(hits(&ev).iter().any(|h| h.0 == P));
    // Not its first turn out any more.
    let ev = turn(&mut b, &[use_move(P, 1), use_move(O, 0)]);
    assert!(ev.contains(&Event::Failed { by: P }));

    let mut b = duel(
        fighter("a", [NORMAL; 2], 10, &[first_turn]),
        fighter("b", [NORMAL; 2], 50, &[mv(3, NORMAL, 10)]),
        1,
    );
    let ev = turn(&mut b, &[use_move(P, 0), use_move(O, 0)]);
    assert!(ev.contains(&Event::CantAct {
        by: O,
        why: Blocked::Flinched
    }));

    // One switched in mid-turn still has its first full turn.
    let (mut b, _) = battle(
        Format::Singles,
        vec![
            fighter("a", [NORMAL; 2], 10, &[wait()]),
            fighter("c", [NORMAL; 2], 10, &[first_turn]),
        ],
        vec![fighter("b", [NORMAL; 2], 50, &[wait()])],
        1,
    );
    turn(&mut b, &[switch(P, 1), use_move(O, 0)]);
    let ev = turn(&mut b, &[use_move(P, 0), use_move(O, 0)]);
    assert!(hits(&ev).iter().any(|h| h.0 == P));
    let ev = turn(&mut b, &[use_move(P, 0), use_move(O, 0)]);
    assert!(ev.contains(&Event::Failed { by: P }));
}

#[test]
fn breaking_walls() {
    let wall = status(1, effects::PHYSICAL_WALL, Target::User);
    let breaker = with(mv(2, FIGHTING, 75), effects::BREAK_SCREENS);
    let mut b = duel(
        fighter("a", [NORMAL; 2], 50, &[breaker]),
        fighter("b", [NORMAL; 2], 100, &[wall]),
        1,
    );
    let ev = turn(&mut b, &[use_move(P, 0), use_move(O, 0)]);
    assert!(ev.contains(&Event::ScreensBroken { side: 1 }));
    assert_eq!(b.field().sides[1].physical_wall, 0);
}

#[test]
fn doubles_helpers_and_drawing_moves() {
    let help = MoveData {
        priority: 5,
        ..status(1, effects::HELP_ALLY, Target::User)
    };
    let draw = MoveData {
        priority: 3,
        ..status(2, effects::DRAW_MOVES, Target::User)
    };
    let hit = mv(3, NORMAL, 40);
    let (mut b, _) = battle(
        Format::Doubles,
        vec![
            fighter("a", [NORMAL; 2], 100, &[help]),
            fighter("c", [NORMAL; 2], 100, &[hit]),
        ],
        vec![
            fighter("b", [NORMAL; 2], 50, &[draw]),
            fighter("d", [NORMAL; 2], 50, &[wait()]),
        ],
        1,
    );
    let ev = turn(
        &mut b,
        &[
            use_move(P, 0),
            aimed(P2, 0, O2),
            use_move(O, 0),
            use_move(O2, 0),
        ],
    );
    assert!(started(&ev, P2, Condition::Helped));
    assert!(started(&ev, O, Condition::Drawing));
    assert!(ev.contains(&Event::Drawn { to: O }));
    assert_eq!(hits(&ev)[0].1, O);
    assert_eq!(hp(&b, O2), 150);

    // Electric moves go to the foe whose ability draws them.
    let zap = mv(4, ELECTRIC, 40);
    let (mut b, _) = battle(
        Format::Doubles,
        vec![
            fighter("a", [NORMAL; 2], 100, &[zap]),
            fighter("c", [NORMAL; 2], 100, &[wait()]),
        ],
        vec![
            fighter("b", [NORMAL; 2], 50, &[wait()]),
            fighter("d", [NORMAL; 2], 50, &[wait()]),
        ],
        1,
    );
    b.member_mut(1, 1).ability = abilities::DRAWS_ELECTRIC;
    let ev = turn(
        &mut b,
        &[
            aimed(P, 0, O),
            use_move(P2, 0),
            use_move(O, 0),
            use_move(O2, 0),
        ],
    );
    assert!(hits(&ev).is_empty());
    assert_eq!(b.fighter(O2).unwrap().stage(Stat::SpAttack), 1);
}

#[test]
fn bouncing_and_stealing_status_moves() {
    let bouncer = MoveData {
        priority: 4,
        ..status(1, effects::BOUNCE_STATUS, Target::User)
    };
    let sleep = MoveData {
        flags: flags::REFLECTABLE,
        ..status(2, effects::SLEEP, Target::Selected)
    };
    let mut b = duel(
        fighter("a", [NORMAL; 2], 100, &[sleep]),
        fighter("b", [NORMAL; 2], 50, &[bouncer]),
        1,
    );
    let ev = turn(&mut b, &[use_move(P, 0), use_move(O, 0)]);
    assert!(ev.contains(&Event::Bounced { by: O }));
    assert_eq!(b.fighter(P).unwrap().ailment, Some(Ailment::Sleep));
    assert_eq!(b.fighter(O).unwrap().ailment, None);

    let steal = MoveData {
        priority: 4,
        ..status(3, effects::STEAL_MOVE, Target::User)
    };
    let up = MoveData {
        flags: flags::STEALABLE,
        ..status(4, effects::USER_UP2_FIRST, Target::User)
    };
    let mut b = duel(
        fighter("a", [NORMAL; 2], 100, &[up]),
        fighter("b", [NORMAL; 2], 50, &[steal]),
        1,
    );
    let ev = turn(&mut b, &[use_move(P, 0), use_move(O, 0)]);
    assert!(ev.contains(&Event::Stolen { by: O }));
    assert_eq!(b.fighter(O).unwrap().stage(Stat::Attack), 2);
    assert_eq!(b.fighter(P).unwrap().stage(Stat::Attack), 0);
}

#[test]
fn cures_hexes_and_half_hp_costs() {
    let team_cure = status(1, effects::CURE_TEAM, Target::User);
    let self_cure = status(2, effects::CURE_SELF, Target::User);
    let hex = status(3, effects::HEX_OR_BOOST, Target::Selected);
    let max_attack = status(4, effects::MAX_ATTACK_HALF_HP, Target::User);
    let mut benched = fighter("c", [NORMAL; 2], 50, &[wait()]);
    benched.ailment = Some(Ailment::Burn);
    let (mut b, _) = battle(
        Format::Singles,
        vec![
            fighter(
                "a",
                [NORMAL; 2],
                100,
                &[team_cure, self_cure, hex, max_attack],
            ),
            benched,
        ],
        vec![fighter("b", [NORMAL; 2], 50, &[wait()])],
        1,
    );
    b.member_mut(0, 0).ailment = Some(Ailment::Poison);
    let ev = turn(&mut b, &[use_move(P, 0), use_move(O, 0)]);
    assert!(ev.contains(&Event::TeamCured { side: 0 }));
    assert!(b.team(0).iter().all(|f| f.ailment.is_none()));
    b.member_mut(0, 0).ailment = Some(Ailment::Paralysis);
    turn(&mut b, &[use_move(P, 1), use_move(O, 0)]);
    assert_eq!(b.fighter(P).unwrap().ailment, None);
    // Not a ghost: speed down, attack and defense up.
    turn(&mut b, &[use_move(P, 2), use_move(O, 0)]);
    let f = b.fighter(P).unwrap();
    assert_eq!(
        (
            f.stage(Stat::Speed),
            f.stage(Stat::Attack),
            f.stage(Stat::Defense)
        ),
        (-1, 1, 1)
    );
    // Half HP for attack +6.
    turn(&mut b, &[use_move(P, 3), use_move(O, 0)]);
    assert_eq!(b.fighter(P).unwrap().stage(Stat::Attack), 6);
    assert_eq!(hp(&b, P), 75);

    // A ghost pays half its HP to hex the foe: 1/4 a turn.
    let mut b = duel(
        fighter("a", [GHOST; 2], 100, &[hex]),
        fighter("b", [NORMAL; 2], 50, &[wait()]),
        1,
    );
    let ev = turn(&mut b, &[use_move(P, 0), use_move(O, 0)]);
    assert!(started(&ev, O, Condition::Hexed));
    assert_eq!(hp(&b, P), 75);
    assert_eq!(hp(&b, O), 150 - 37);
}

#[test]
fn pp_cuts_and_haunted_sleep() {
    let pp_cut = status(1, effects::CUT_PP, Target::Selected);
    let haunting = status(2, effects::HAUNTED_SLEEP, Target::Selected);
    let mut b = duel(
        fighter("a", [NORMAL; 2], 50, &[pp_cut, haunting]),
        fighter("b", [NORMAL; 2], 100, &[mv(3, NORMAL, 10)]),
        1,
    );
    let ev = turn(&mut b, &[use_move(P, 0), use_move(O, 0)]);
    let cut = ev
        .iter()
        .find_map(|e| match e {
            Event::PpCut {
                target: O, amount, ..
            } => Some(*amount),
            _ => None,
        })
        .unwrap();
    assert!((2..=5).contains(&cut));
    assert_eq!(b.fighter(O).unwrap().moves[0].pp, 19 - cut);
    let ev = turn(&mut b, &[use_move(P, 1), use_move(O, 0)]);
    assert!(ev.contains(&Event::Failed { by: P }));
    b.member_mut(1, 0).ailment = Some(Ailment::Sleep);
    b.member_mut(1, 0).sleep_turns = 5;
    let before = hp(&b, O);
    turn(&mut b, &[use_move(P, 1), use_move(O, 0)]);
    assert_eq!(hp(&b, O), before - 37);
}

#[test]
fn confusing_everyone_and_raising_with_confusion() {
    let raise = MoveData {
        target: Target::AllOthers,
        ..status(1, effects::CONFUSE_ALL, Target::AllOthers)
    };
    let confuse_up = status(2, effects::CONFUSE_ATTACK_UP2, Target::Selected);
    let (mut b, _) = battle(
        Format::Doubles,
        vec![
            fighter("a", [NORMAL; 2], 100, &[raise, confuse_up]),
            fighter("c", [NORMAL; 2], 100, &[wait()]),
        ],
        vec![
            fighter("b", [NORMAL; 2], 50, &[wait()]),
            fighter("d", [NORMAL; 2], 50, &[wait()]),
        ],
        1,
    );
    b.member_mut(1, 1).ability = abilities::NO_CONFUSION;
    let ev = turn(
        &mut b,
        &[
            use_move(P, 0),
            use_move(P2, 0),
            use_move(O, 0),
            use_move(O2, 0),
        ],
    );
    assert!(ev.contains(&Event::Confused { target: O }));
    assert!(ev.contains(&Event::Confused { target: P2 }));
    assert!(!ev.contains(&Event::Confused { target: O2 }));
    assert!(!ev.contains(&Event::Confused { target: P }));

    let mut b = duel(
        fighter("a", [NORMAL; 2], 100, &[confuse_up]),
        fighter("b", [NORMAL; 2], 50, &[wait()]),
        1,
    );
    let ev = turn(&mut b, &[use_move(P, 0), use_move(O, 0)]);
    assert!(ev.contains(&Event::StatChanged {
        target: O,
        stat: Stat::Attack,
        by: 2
    }));
    assert!(ev.contains(&Event::Confused { target: O }));
}

#[test]
fn damping_sealing_identifying_and_sure_hits() {
    let dull = status(1, effects::DULL_ELECTRIC, Target::User);
    let mut b = duel(
        fighter("a", [NORMAL; 2], 100, &[dull]),
        fighter("b", [NORMAL; 2], 50, &[mv(2, ELECTRIC, 80)]),
        1,
    );
    let before = b.estimate(O, 0, P).0;
    turn(&mut b, &[use_move(P, 0), use_move(O, 0)]);
    assert!(b.estimate(O, 0, P).0 < before * 2 / 3);

    let seal = status(3, effects::SEAL_SHARED_MOVES, Target::User);
    let shared = mv(4, NORMAL, 40);
    let mut b = duel(
        fighter("a", [NORMAL; 2], 100, &[seal, shared]),
        fighter("b", [NORMAL; 2], 50, &[shared, wait()]),
        1,
    );
    let ev = turn(&mut b, &[use_move(P, 0), use_move(O, 1)]);
    assert!(started(&ev, P, Condition::Sealing));
    assert!(!b.usable(O, 0));

    let identify = status(5, effects::IDENTIFY, Target::Selected);
    let sure = status(6, effects::SURE_HIT_NEXT, Target::Selected);
    let mut b = duel(
        fighter("a", [NORMAL; 2], 100, &[identify, sure, mv(7, NORMAL, 40)]),
        fighter("b", [GHOST; 2], 50, &[wait()]),
        1,
    );
    assert_eq!(b.estimate(P, 2, O).0, 0);
    turn(&mut b, &[use_move(P, 0), use_move(O, 0)]);
    assert!(b.estimate(P, 2, O).0 > 0);
    turn(&mut b, &[use_move(P, 1), use_move(O, 0)]);
    b.member_mut(1, 0).evasion_edge = 100;
    let ev = turn(&mut b, &[use_move(P, 2), use_move(O, 0)]);
    assert_eq!(hits(&ev).len(), 1);
}

#[test]
fn rising_hits_gifts_random_power_and_stat_bits() {
    let kick = with(mv(1, FIGHTING, 10), effects::TRIPLE_RISING);
    let gift = with(mv(2, NORMAL, 1), effects::RANDOM_GIFT);
    let random_power = with(mv(3, GROUND, 1), effects::RANDOM_POWER);
    let bits = with(mv(4, NORMAL, 1), effects::STAT_BITS_POWER);
    let mut healed = 0;
    let mut powers = std::collections::BTreeSet::new();
    for seed in 0..60 {
        let mut b = duel(
            fighter("a", [NORMAL; 2], 100, &[kick, gift, random_power, bits]),
            fighter("b", [NORMAL; 2], 50, &[wait()]),
            seed,
        );
        b.member_mut(1, 0).stats.hp = 999;
        b.member_mut(1, 0).hp = 900;
        b.member_mut(1, 0).ability = abilities::NO_CRITS_A;
        let ev = turn(&mut b, &[use_move(P, 0), use_move(O, 0)]);
        let h = hits(&ev);
        assert_eq!(h.len(), 3);
        assert!(h[0].2 < h[1].2 && h[1].2 < h[2].2, "{h:?}");
        let ev = turn(&mut b, &[use_move(P, 1), use_move(O, 0)]);
        healed += usize::from(
            ev.iter()
                .any(|e| matches!(e, Event::Healed { target: O, .. })),
        );
        let ev = turn(&mut b, &[use_move(P, 2), use_move(O, 0)]);
        powers.insert(hits(&ev)[0].2);
    }
    assert!(healed > 0 && healed < 30, "{healed}");
    assert!(powers.len() > 3);

    // A fighter's stats pick the type: a type the target is weak to hits harder.
    let mut b = duel(
        fighter("a", [NORMAL; 2], 100, &[bits]),
        fighter("b", [GRASS; 2], 50, &[wait()]),
        1,
    );
    b.member_mut(0, 0).stats.hp = 0b10_1000; // fire's bits
    let (t, _) = effects::stat_bits_power(b.fighter(P).unwrap());
    let (_, m) = b.estimate(P, 0, O);
    assert_eq!(m, b.chart().multiplier(t, [GRASS; 2], false));
}

#[test]
fn self_fainting_moves_and_the_ability_that_stops_them() {
    let boom = with(mv(1, NORMAL, 250), effects::SELF_FAINT);
    let (mut b, _) = battle(
        Format::Singles,
        vec![
            fighter("a", [NORMAL; 2], 100, &[boom]),
            fighter("c", [NORMAL; 2], 100, &[wait()]),
        ],
        vec![
            fighter("b", [NORMAL; 2], 50, &[wait()]),
            fighter("d", [NORMAL; 2], 50, &[wait()]),
        ],
        1,
    );
    b.member_mut(1, 0).ability = abilities::NO_SELF_FAINT_MOVES;
    let ev = turn(&mut b, &[use_move(P, 0), use_move(O, 0)]);
    assert!(ev.contains(&Event::Failed { by: P }));
    assert!(b.alive(P));

    let faint_lower = status(2, effects::FAINT_LOWER_TARGET, Target::Selected);
    let (mut b, _) = battle(
        Format::Singles,
        vec![
            fighter("a", [NORMAL; 2], 100, &[faint_lower]),
            fighter("c", [NORMAL; 2], 100, &[wait()]),
        ],
        vec![fighter("b", [NORMAL; 2], 50, &[wait()])],
        1,
    );
    let ev = turn(&mut b, &[use_move(P, 0), use_move(O, 0)]);
    assert!(ev.contains(&Event::StatChanged {
        target: O,
        stat: Stat::SpAttack,
        by: -2
    }));
    assert!(ev.contains(&Event::Fainted { at: P, member: 0 }));
}

#[test]
fn every_used_effect_id_is_handled() {
    for e in 0..effects::COUNT {
        let handled = effects::primary(e).is_some()
            || effects::added(e).is_some()
            || effects::after_hit(e).is_some()
            || effects::is_fixed_damage(e)
            || effects::drain(e).is_some()
            || effects::recoil(e).is_some()
            || effects::charges(e)
            || effects::calls_other_move(e)
            || effects::crit_stages(e) > 0
            || effects::hit_count(e, &mut Rng::new(0)) > 1
            || effects::certain(e)
            || matches!(
                e,
                effects::HIT
                    | effects::SELF_FAINT
                    | effects::ALWAYS_HITS
                    | effects::ALWAYS_HITS_B
                    | effects::STORE_DAMAGE
                    | effects::RAMPAGE
                    | effects::HIT_MONEY
                    | effects::ONE_HIT_KO
                    | effects::CRASH_ON_MISS
                    | effects::RECHARGE_AFTER
                    | effects::ANGER_HIT
                    | effects::RETURN_PHYSICAL
                    | effects::RETURN_SPECIAL
                    | effects::LOW_HP_POWER
                    | effects::LEAVE_ONE_HP
                    | effects::PRIORITY_HIT
                    | effects::HIT_TAKE_ITEM
                    | effects::ROLLING
                    | effects::GROWING_HIT
                    | effects::BOND_POWER
                    | effects::BOND_POWER_INVERSE
                    | effects::RANDOM_GIFT
                    | effects::RANDOM_POWER
                    | effects::HITS_SWITCHING
                    | effects::CLEAR_HAZARDS
                    | effects::STAT_BITS_POWER
                    | effects::HITS_UNDERGROUND
                    | effects::HITS_AIR
                    | effects::HITS_AIR_B
                    | effects::DELAYED_HIT
                    | effects::TEAM_HITS
                    | effects::NOISE
                    | effects::RELEASE_HIT
                    | effects::STATUS_POWER
                    | effects::FOCUSED_HIT
                    | effects::PARALYSIS_POWER_CURE
                    | effects::HURT_FIRST_POWER
                    | effects::BREAK_SCREENS
                    | effects::HIT_DROP_ITEM
                    | effects::HP_POWER
                    | effects::WEIGHT_POWER
                    | effects::WEATHER_POWER
            );
        assert_eq!(handled, effects::implemented(e), "effect {e}");
    }
}

#[test]
fn random_battles_with_every_effect_and_ability_run_to_the_end() {
    let targets = [
        Target::Selected,
        Target::Depends,
        Target::UserOrSelected,
        Target::RandomFoe,
        Target::BothFoes,
        Target::User,
        Target::AllOthers,
        Target::FoesField,
    ];
    let mut rng = Rng::new(42);
    let mut table = Vec::new();
    for id in 1..=120u16 {
        let effect = rng.below(u32::from(effects::COUNT)) as u8;
        let power = if rng.below(3) == 0 {
            0
        } else {
            rng.below(150) as u8 + 1
        };
        table.push(MoveData {
            id,
            effect,
            power,
            type_id: rng.below(u32::from(COUNT)) as u8,
            category: if power == 0 {
                super::super::data::Category::Status
            } else {
                super::super::data::Category::Physical
            },
            details: super::super::data::MoveDetails::default(),
            accuracy: [0, 50, 85, 100][rng.below(4) as usize],
            pp: rng.below(10) as u8 + 1,
            chance: [0, 10, 30, 100][rng.below(4) as usize],
            target: targets[rng.below(8) as usize],
            priority: rng.below(5) as i8 - 2,
            flags: rng.below(64),
        });
    }
    for seed in 0..160u64 {
        let format = if seed % 2 == 0 {
            Format::Singles
        } else {
            Format::Doubles
        };
        let team = |rng: &mut Rng| -> Vec<Fighter> {
            (0..rng.below(6) + 1)
                .map(|i| {
                    let moves: Vec<MoveData> = (0..4)
                        .map(|_| table[rng.below(table.len() as u32) as usize])
                        .collect();
                    let t = rng.below(u32::from(COUNT)) as u8;
                    let mut f = Fighter::new(
                        &format!("f{i}"),
                        50,
                        stats(400 + rng.below(400) as u16, 50 + rng.below(100) as u16),
                        [t, t],
                        rng.below(u32::from(abilities::COUNT)) as u8,
                        &moves,
                    );
                    f.luck = rng.below(256) as u8;
                    f.evasion_edge = rng.below(10) as u8;
                    f.friendship = rng.below(256) as u8;
                    f
                })
                .collect()
        };
        let teams = [team(&mut rng), team(&mut rng)];
        let mut b = Battle::new(format, teams, chart(), rules(), seed).unwrap();
        b.set_move_table(table.clone());
        b.start();
        let mut ai = Rng::new(seed);
        for _ in 0..300 {
            match b.phase() {
                Phase::Over(_) => break,
                Phase::Replacements => {
                    let mut picks = ai::replacements(&b, 0);
                    picks.extend(ai::replacements(&b, 1));
                    b.replace(&picks).unwrap();
                }
                Phase::Actions => {
                    let mut choices = ai::choose(&b, 0, &mut ai);
                    choices.extend(ai::choose(&b, 1, &mut ai));
                    b.play_turn(&choices)
                        .unwrap_or_else(|e| panic!("seed {seed}: {e:?} for {choices:?}"));
                }
            }
        }
        assert!(matches!(b.phase(), Phase::Over(_)), "seed {seed}");
    }
}

#[test]
fn the_ai_keeps_its_last_fighter() {
    let boom = with(mv(1, NORMAL, 250), effects::SELF_FAINT);
    let (b, _) = battle(
        Format::Singles,
        vec![fighter("a", [NORMAL; 2], 100, &[wait()])],
        vec![fighter("b", [NORMAL; 2], 50, &[boom, mv(2, NORMAL, 10)])],
        1,
    );
    let mut rng = Rng::new(3);
    for _ in 0..20 {
        let c = ai::choose(&b, 1, &mut rng);
        assert!(matches!(c[0].action, Action::Move { index: 1, .. }));
    }
}
