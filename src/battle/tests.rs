//! Whole-battle tests with synthetic fighters, moves and type chart.

use super::abilities;
use super::ai;
use super::data::tests::chart;
use super::data::types::*;
use super::effects;
use super::event::{Blocked, HurtBy};
use super::field::{Screen, Weather};
use super::rng::Rng;
use super::*;

mod ability_cases;
mod effect_cases;

fn no_pp_move() -> MoveData {
    MoveData {
        effect: effects::RECOIL_QUARTER,
        pp: 1,
        ..MoveData::plain(900, NORMAL, 50)
    }
}

fn rules() -> Rules {
    Rules {
        out_of_pp: no_pp_move(),
        turn_limit: 100,
    }
}

fn stats(hp: u16, all: u16) -> Stats {
    Stats {
        hp,
        attack: all,
        defense: all,
        speed: all,
        sp_attack: all,
        sp_defense: all,
    }
}

fn mv(id: u16, type_id: u8, power: u8) -> MoveData {
    MoveData::plain(id, type_id, power)
}

fn with(m: MoveData, effect: u8) -> MoveData {
    MoveData { effect, ..m }
}

fn status(id: u16, effect: u8, target: Target) -> MoveData {
    MoveData {
        effect,
        target,
        ..MoveData::plain(id, NORMAL, 0)
    }
}

fn fighter(name: &str, types: [u8; 2], speed: u16, moves: &[MoveData]) -> Fighter {
    let mut s = stats(150, 100);
    s.speed = speed;
    Fighter::new(name, 50, s, types, 0, moves)
}

fn battle(format: Format, a: Vec<Fighter>, b: Vec<Fighter>, seed: u64) -> (Battle, Vec<Event>) {
    let mut bt = Battle::new(format, [a, b], chart(), rules(), seed).unwrap();
    let ev = bt.start();
    (bt, ev)
}

const P: Slot = Slot::new(0, 0);
const O: Slot = Slot::new(1, 0);
const P2: Slot = Slot::new(0, 1);
const O2: Slot = Slot::new(1, 1);

fn use_move(at: Slot, index: usize) -> Choice {
    Choice {
        at,
        action: Action::Move {
            index,
            target: None,
        },
    }
}

fn aimed(at: Slot, index: usize, target: Slot) -> Choice {
    Choice {
        at,
        action: Action::Move {
            index,
            target: Some(target),
        },
    }
}

fn switch(at: Slot, member: usize) -> Choice {
    Choice {
        at,
        action: Action::Switch { member },
    }
}

fn users(ev: &[Event]) -> Vec<Slot> {
    ev.iter()
        .filter_map(|e| match e {
            Event::UsedMove { by, .. } => Some(*by),
            _ => None,
        })
        .collect()
}

fn hits(ev: &[Event]) -> Vec<(Slot, Slot, u16)> {
    ev.iter()
        .filter_map(|e| match e {
            Event::Hit {
                by, target, damage, ..
            } => Some((*by, *target, *damage)),
            _ => None,
        })
        .collect()
}

#[test]
fn start_sends_out_leads() {
    let (b, ev) = battle(
        Format::Singles,
        vec![fighter("a", [NORMAL; 2], 100, &[mv(1, NORMAL, 40)])],
        vec![fighter("b", [NORMAL; 2], 100, &[mv(1, NORMAL, 40)])],
        1,
    );
    assert_eq!(
        ev,
        vec![
            Event::SwitchedIn { at: P, member: 0 },
            Event::SwitchedIn { at: O, member: 0 },
        ]
    );
    assert_eq!(b.phase(), Phase::Actions);
    assert_eq!(b.slots(), vec![P, O]);
    // Doubles with one fighter on a side: only its first slot is used.
    let (b, ev) = battle(
        Format::Doubles,
        vec![
            fighter("a", [NORMAL; 2], 100, &[]),
            fighter("b", [NORMAL; 2], 100, &[]),
        ],
        vec![fighter("c", [NORMAL; 2], 100, &[])],
        1,
    );
    assert_eq!(ev.len(), 3);
    assert!(b.alive(P2) && !b.alive(O2));
    // Fainted members aren't sent out.
    let mut down = fighter("x", [NORMAL; 2], 100, &[]);
    down.hp = 0;
    let (b, _) = battle(
        Format::Singles,
        vec![down, fighter("y", [NORMAL; 2], 100, &[])],
        vec![fighter("z", [NORMAL; 2], 100, &[])],
        1,
    );
    assert_eq!(b.member(P), Some(1));
}

#[test]
fn bad_teams_are_refused() {
    let one = || vec![fighter("a", [NORMAL; 2], 100, &[])];
    assert!(Battle::new(Format::Singles, [vec![], one()], chart(), rules(), 0).is_err());
    let seven = (0..7)
        .map(|_| fighter("a", [NORMAL; 2], 100, &[]))
        .collect();
    assert!(Battle::new(Format::Singles, [seven, one()], chart(), rules(), 0).is_err());
    let mut down = fighter("x", [NORMAL; 2], 100, &[]);
    down.hp = 0;
    assert_eq!(
        Battle::new(Format::Singles, [one(), vec![down]], chart(), rules(), 0).err(),
        Some(BattleError::BadTeam)
    );
}

#[test]
fn a_turn_hits_in_speed_order_with_formula_damage() {
    let (mut b, _) = battle(
        Format::Singles,
        vec![fighter("fast", [FIRE; 2], 120, &[mv(1, NORMAL, 80)])],
        vec![fighter("slow", [GRASS; 2], 80, &[mv(2, FIRE, 80)])],
        5,
    );
    let ev = b.play_turn(&[use_move(P, 0), use_move(O, 0)]).unwrap();
    assert_eq!(ev[0], Event::TurnStarted { turn: 1 });
    assert_eq!(users(&ev), vec![P, O]);
    let h = hits(&ev);
    // 37 base; no STAB, neutral: 31..=37 (doubled on a crit).
    let (_, _, d) = h[0];
    assert!((31..=37).contains(&d) || (62..=74).contains(&d), "{d}");
    // Fire into fire: not very effective, no STAB for a grass user: 37 / 2 = 18 -> 15..=18.
    let (_, _, d) = h[1];
    assert!((15..=18).contains(&d) || (31..=37).contains(&d), "{d}");
    assert!(ev.iter().any(|e| matches!(
        e,
        Event::Hit {
            target: P,
            effect: Effectiveness::NotVery,
            ..
        }
    )));
    assert_eq!(b.fighter(P).unwrap().moves[0].pp, 19);
    assert_eq!(b.fighter(P).unwrap().hp, 150 - h[1].2);
}

#[test]
fn priority_goes_before_speed() {
    let quick = MoveData {
        priority: 1,
        ..mv(3, NORMAL, 40)
    };
    let (mut b, _) = battle(
        Format::Singles,
        vec![fighter("fast", [NORMAL; 2], 200, &[mv(1, NORMAL, 40)])],
        vec![fighter("slow", [NORMAL; 2], 10, &[quick])],
        5,
    );
    let ev = b.play_turn(&[use_move(P, 0), use_move(O, 0)]).unwrap();
    assert_eq!(users(&ev), vec![O, P]);
}

#[test]
fn speed_ties_are_random_but_seeded() {
    let mut first = [0; 2];
    for seed in 0..40 {
        let run = |seed| {
            let (mut b, _) = battle(
                Format::Singles,
                vec![fighter("a", [NORMAL; 2], 100, &[mv(1, NORMAL, 10)])],
                vec![fighter("b", [NORMAL; 2], 100, &[mv(1, NORMAL, 10)])],
                seed,
            );
            b.play_turn(&[use_move(P, 0), use_move(O, 0)]).unwrap()
        };
        let ev = run(seed);
        assert_eq!(ev, run(seed));
        first[users(&ev)[0].side] += 1;
    }
    assert!(first[0] > 5 && first[1] > 5, "{first:?}");
}

#[test]
fn type_immunity_and_ability_immunity() {
    let (mut b, _) = battle(
        Format::Singles,
        vec![fighter("a", [NORMAL; 2], 100, &[mv(1, ELECTRIC, 90)])],
        vec![fighter("b", [GROUND; 2], 50, &[mv(2, GROUND, 90)])],
        1,
    );
    b.member_mut(0, 0).ability = abilities::IMMUNE_TO_GROUND;
    let ev = b.play_turn(&[use_move(P, 0), use_move(O, 0)]).unwrap();
    assert!(ev.contains(&Event::NoEffect { target: O }));
    assert!(ev.contains(&Event::NoEffect { target: P }));
    assert!(ev.contains(&Event::Ability {
        at: P,
        ability: abilities::IMMUNE_TO_GROUND
    }));
    assert!(hits(&ev).is_empty());
    assert_eq!(b.fighter(P).unwrap().hp, 150);
    assert_eq!(b.fighter(O).unwrap().hp, 150);
}

#[test]
fn accuracy_evasion_and_sure_hits() {
    let mut always = mv(2, NORMAL, 10);
    always.effect = effects::ALWAYS_HITS;
    let (mut b, _) = battle(
        Format::Singles,
        vec![fighter("a", [NORMAL; 2], 100, &[mv(1, NORMAL, 10), always])],
        vec![fighter(
            "b",
            [NORMAL; 2],
            50,
            &[status(3, effects::NOTHING, Target::User)],
        )],
        1,
    );
    b.member_mut(1, 0).evasion_edge = 100;
    let ev = b.play_turn(&[use_move(P, 0), use_move(O, 0)]).unwrap();
    assert!(ev.contains(&Event::Missed { by: P, target: O }));
    assert!(ev.contains(&Event::Nothing));
    let ev = b.play_turn(&[use_move(P, 1), use_move(O, 0)]).unwrap();
    assert_eq!(hits(&ev).len(), 1);

    // A 50% move lands about half the time; +6 evasion makes it rare.
    let mut landed = [0; 2];
    for seed in 0..300 {
        let mut half = mv(1, NORMAL, 1);
        half.accuracy = 50;
        let (mut b, _) = battle(
            Format::Singles,
            vec![fighter("a", [NORMAL; 2], 100, &[half])],
            vec![fighter(
                "b",
                [NORMAL; 2],
                50,
                &[status(3, effects::NOTHING, Target::User)],
            )],
            seed,
        );
        let i = (seed % 2) as usize;
        if i == 1 {
            b.member_mut(1, 0).change_stage(Stat::Evasion, 6);
        }
        let ev = b.play_turn(&[use_move(P, 0), use_move(O, 0)]).unwrap();
        landed[i] += hits(&ev).len();
    }
    assert!((55..95).contains(&landed[0]), "{landed:?}");
    // 50 * 33% = 16%.
    assert!(landed[1] < 45, "{landed:?}");
}

#[test]
fn paralysis_slows_and_sometimes_stops() {
    let para = MoveData {
        target: Target::Selected,
        ..status(5, effects::PARALYZE, Target::Selected)
    };
    let para = MoveData {
        type_id: ELECTRIC,
        ..para
    };
    let (mut b, _) = battle(
        Format::Singles,
        vec![fighter("a", [NORMAL; 2], 100, &[para])],
        vec![fighter("b", [NORMAL; 2], 80, &[mv(1, NORMAL, 1)])],
        3,
    );
    let ev = b.play_turn(&[use_move(P, 0), use_move(O, 0)]).unwrap();
    assert!(ev.contains(&Event::Ailment {
        target: O,
        ailment: Ailment::Paralysis
    }));
    assert_eq!(b.speed(O), 20);
    // Again: already paralysed.
    let mut stopped = 0;
    for turn in 0..80 {
        let ev = b.play_turn(&[use_move(P, 0), use_move(O, 0)]).unwrap();
        if turn == 0 {
            assert!(ev.contains(&Event::AlreadyAilment {
                target: O,
                ailment: Ailment::Paralysis
            }));
        }
        if ev.contains(&Event::CantAct {
            by: O,
            why: Blocked::FullyParalyzed,
        }) {
            stopped += 1;
        }
        if b.phase() != Phase::Actions || b.fighter(P).unwrap().moves[0].pp == 0 {
            break;
        }
    }
    assert!((1..12).contains(&stopped), "{stopped}");

    // Ground types don't take paralysis from an electric move.
    let (mut b, _) = battle(
        Format::Singles,
        vec![fighter("a", [NORMAL; 2], 100, &[para])],
        vec![fighter("b", [GROUND; 2], 80, &[mv(1, NORMAL, 1)])],
        3,
    );
    let ev = b.play_turn(&[use_move(P, 0), use_move(O, 0)]).unwrap();
    assert!(ev.contains(&Event::NoEffect { target: O }));
}

#[test]
fn sleep_skips_turns_without_using_pp() {
    let (mut b, _) = battle(
        Format::Singles,
        vec![fighter(
            "a",
            [NORMAL; 2],
            100,
            &[status(4, effects::SLEEP, Target::Selected)],
        )],
        vec![fighter("b", [NORMAL; 2], 80, &[mv(1, NORMAL, 1)])],
        9,
    );
    let ev = b.play_turn(&[use_move(P, 0), use_move(O, 0)]).unwrap();
    assert!(ev.contains(&Event::Ailment {
        target: O,
        ailment: Ailment::Sleep
    }));
    // A count of 2..=5 that it already ticked once, trying to act after falling asleep; it
    // wakes (and acts) on the turn the count runs out, so it sleeps through 1..=4 turns.
    let turns = b.fighter(O).unwrap().sleep_turns;
    assert!((1..=4).contains(&turns));
    assert!(ev.contains(&Event::CantAct {
        by: O,
        why: Blocked::Asleep
    }));
    let mut skipped = 0;
    loop {
        let ev = b.play_turn(&[use_move(P, 0), use_move(O, 0)]).unwrap();
        if ev.contains(&Event::WokeUp { target: O }) {
            assert!(users(&ev).contains(&O));
            break;
        }
        assert!(ev.contains(&Event::CantAct {
            by: O,
            why: Blocked::Asleep
        }));
        skipped += 1;
    }
    assert_eq!(skipped, turns - 1);
    // Only the move it used after waking spent PP.
    assert_eq!(b.fighter(O).unwrap().moves[0].pp, 19);
}

#[test]
fn residual_damage_from_status() {
    let tick = |a: Ailment| {
        let (mut b, _) = battle(
            Format::Singles,
            vec![fighter(
                "a",
                [NORMAL; 2],
                100,
                &[status(9, effects::NOTHING, Target::User)],
            )],
            vec![fighter(
                "b",
                [NORMAL; 2],
                80,
                &[status(9, effects::NOTHING, Target::User)],
            )],
            1,
        );
        b.member_mut(1, 0).ailment = Some(a);
        let mut lost = Vec::new();
        for _ in 0..3 {
            let ev = b.play_turn(&[use_move(P, 0), use_move(O, 0)]).unwrap();
            for e in ev {
                if let Event::Hurt {
                    target: O, damage, ..
                } = e
                {
                    lost.push(damage);
                }
            }
        }
        ((), lost)
    };
    // 150 HP: 1/8 = 18; bad poison 1/16 = 9 times the turn count.
    assert_eq!(tick(Ailment::Burn).1, vec![18, 18, 18]);
    assert_eq!(tick(Ailment::Poison).1, vec![18, 18, 18]);
    assert_eq!(tick(Ailment::BadPoison).1, vec![9, 18, 27]);
    // Switching out resets the bad poison count (the fighter keeps the status).
    let mut team = vec![
        fighter(
            "a",
            [NORMAL; 2],
            100,
            &[status(9, effects::NOTHING, Target::User)],
        ),
        fighter(
            "c",
            [NORMAL; 2],
            100,
            &[status(9, effects::NOTHING, Target::User)],
        ),
    ];
    team[0].ailment = Some(Ailment::BadPoison);
    let (mut b3, _) = battle(
        Format::Singles,
        team,
        vec![fighter("b", [NORMAL; 2], 80, &[mv(1, NORMAL, 1)])],
        1,
    );
    b3.play_turn(&[use_move(P, 0), use_move(O, 0)]).unwrap();
    assert_eq!(b3.team(0)[0].volatile.bad_poison_turns, 1);
    b3.play_turn(&[switch(P, 1), use_move(O, 0)]).unwrap();
    assert_eq!(b3.team(0)[0].volatile.bad_poison_turns, 0);
    assert_eq!(b3.team(0)[0].ailment, Some(Ailment::BadPoison));
}

#[test]
fn burn_halves_physical_damage() {
    let run = |burned: bool| {
        let (mut b, _) = battle(
            Format::Singles,
            vec![fighter("a", [NORMAL; 2], 100, &[mv(1, FIGHTING, 80)])],
            vec![fighter(
                "b",
                [ROCK; 2],
                80,
                &[status(9, effects::NOTHING, Target::User)],
            )],
            1,
        );
        if burned {
            b.member_mut(0, 0).ailment = Some(Ailment::Burn);
        }
        b.estimate(P, 0, O).0
    };
    assert_eq!(run(false), 34);
    assert_eq!(run(true), 17);
}

#[test]
fn weather_moves_last_five_turns_and_change_damage() {
    let rain = status(10, effects::RAIN, Target::User);
    let water = mv(11, WATER, 20);
    let (mut b, _) = battle(
        Format::Singles,
        vec![fighter("a", [NORMAL; 2], 100, &[rain, water])],
        vec![fighter(
            "b",
            [NORMAL; 2],
            80,
            &[status(9, effects::NOTHING, Target::User)],
        )],
        1,
    );
    let clear = b.estimate(P, 1, O).0;
    let ev = b.play_turn(&[use_move(P, 0), use_move(O, 0)]).unwrap();
    assert!(ev.contains(&Event::WeatherStarted {
        weather: Weather::Rain
    }));
    assert_eq!(b.weather(), Weather::Rain);
    assert!(b.estimate(P, 1, O).0 > clear);
    // Again: it fails while it's raining.
    let ev = b.play_turn(&[use_move(P, 0), use_move(O, 0)]).unwrap();
    assert!(ev.contains(&Event::Failed { by: P }));
    let mut turns = 2;
    while b.weather() == Weather::Rain {
        let ev = b.play_turn(&[use_move(P, 1), use_move(O, 0)]).unwrap();
        turns += 1;
        if ev.contains(&Event::WeatherEnded {
            weather: Weather::Rain,
        }) {
            break;
        }
    }
    assert_eq!(turns, 5);
}

#[test]
fn sand_hurts_all_but_its_types_and_entry_abilities_set_it() {
    let mut lead = fighter(
        "a",
        [NORMAL; 2],
        100,
        &[status(9, effects::NOTHING, Target::User)],
    );
    lead.ability = abilities::ENTRY_SAND;
    let (mut b, ev) = battle(
        Format::Singles,
        vec![lead],
        vec![fighter(
            "b",
            [ROCK; 2],
            80,
            &[status(9, effects::NOTHING, Target::User)],
        )],
        1,
    );
    assert!(ev.contains(&Event::WeatherStarted {
        weather: Weather::Sand
    }));
    assert_eq!(b.field().weather_turns, 0);
    for _ in 0..8 {
        b.play_turn(&[use_move(P, 0), use_move(O, 0)]).unwrap();
    }
    // Lasting sand: 150 / 16 = 9 a turn for the normal type, nothing for the rock type.
    assert_eq!(b.fighter(P).unwrap().hp, 150 - 8 * 9);
    assert_eq!(b.fighter(O).unwrap().hp, 150);
    assert_eq!(b.weather(), Weather::Sand);
}

#[test]
fn weather_cancelling_ability() {
    let mut calm = fighter(
        "a",
        [NORMAL; 2],
        100,
        &[status(10, effects::SUN, Target::User)],
    );
    calm.ability = abilities::NO_WEATHER_A;
    let (mut b, _) = battle(
        Format::Singles,
        vec![calm],
        vec![fighter(
            "b",
            [NORMAL; 2],
            80,
            &[status(9, effects::NOTHING, Target::User)],
        )],
        1,
    );
    b.play_turn(&[use_move(P, 0), use_move(O, 0)]).unwrap();
    assert_eq!(b.field().weather, Weather::Sun);
    assert_eq!(b.weather(), Weather::Clear);
}

#[test]
fn stat_moves_and_limits() {
    let raise = status(20, effects::USER_UP2_FIRST, Target::User);
    let attack_down = status(21, effects::TARGET_DOWN_FIRST, Target::BothFoes);
    let (mut b, _) = battle(
        Format::Singles,
        vec![fighter("a", [NORMAL; 2], 100, &[raise, mv(1, NORMAL, 60)])],
        vec![fighter("b", [NORMAL; 2], 80, &[attack_down])],
        1,
    );
    let before = b.estimate(P, 1, O).0;
    let ev = b.play_turn(&[use_move(P, 0), use_move(O, 0)]).unwrap();
    assert!(ev.contains(&Event::StatChanged {
        target: P,
        stat: Stat::Attack,
        by: 2
    }));
    assert!(ev.contains(&Event::StatChanged {
        target: P,
        stat: Stat::Attack,
        by: -1
    }));
    assert!(b.estimate(P, 1, O).0 > before);
    b.member_mut(0, 0).change_stage(Stat::Attack, 6);
    let ev = b.play_turn(&[use_move(P, 0), use_move(O, 0)]).unwrap();
    assert!(ev.contains(&Event::StatAtLimit {
        target: P,
        stat: Stat::Attack,
        up: true
    }));
    assert_eq!(b.fighter(P).unwrap().stage(Stat::Attack), 5);
}

#[test]
fn switching_goes_first_and_resets_stages() {
    let quick = MoveData {
        priority: 1,
        ..mv(3, NORMAL, 40)
    };
    let (mut b, _) = battle(
        Format::Singles,
        vec![
            fighter("a", [NORMAL; 2], 10, &[mv(1, NORMAL, 40)]),
            fighter("c", [GHOST; 2], 10, &[mv(1, NORMAL, 40)]),
        ],
        vec![fighter("b", [NORMAL; 2], 200, &[quick])],
        1,
    );
    b.member_mut(0, 0).change_stage(Stat::Attack, 3);
    let ev = b.play_turn(&[switch(P, 1), use_move(O, 0)]).unwrap();
    assert_eq!(ev[1], Event::Withdrawn { at: P, member: 0 });
    assert_eq!(ev[2], Event::SwitchedIn { at: P, member: 1 });
    // The foe's normal move now meets a ghost.
    assert!(ev.contains(&Event::NoEffect { target: P }));
    assert_eq!(b.team(0)[0].stage(Stat::Attack), 0);
    assert_eq!(b.member(P), Some(1));
    // Can't switch to the one already out, or to the same member twice.
    assert_eq!(
        b.play_turn(&[switch(P, 1), use_move(O, 0)]).err(),
        Some(BattleError::Invalid(P))
    );
}

#[test]
fn faint_replace_and_win() {
    let big = mv(1, NORMAL, 250);
    let mut weak = fighter("w", [NORMAL; 2], 10, &[mv(2, NORMAL, 10)]);
    weak.hp = 1;
    let mut weak2 = weak.clone();
    weak2.name = "w2".into();
    let (mut b, _) = battle(
        Format::Singles,
        vec![fighter("a", [NORMAL; 2], 100, &[big])],
        vec![weak, weak2],
        1,
    );
    let ev = b.play_turn(&[use_move(P, 0), use_move(O, 0)]).unwrap();
    assert!(ev.contains(&Event::Fainted { at: O, member: 0 }));
    // Its action was lost.
    assert_eq!(users(&ev), vec![P]);
    assert_eq!(b.phase(), Phase::Replacements);
    assert_eq!(b.pending_replacements(), &[O]);
    assert_eq!(
        b.play_turn(&[use_move(P, 0)]).err(),
        Some(BattleError::WrongPhase)
    );
    assert_eq!(b.replace(&[]).err(), Some(BattleError::Missing(O)));
    assert_eq!(b.replace(&[(O, 0)]).err(), Some(BattleError::Invalid(O)));
    let picks = ai::replacements(&b, 1);
    assert_eq!(picks, vec![(O, 1)]);
    let ev = b.replace(&picks).unwrap();
    assert_eq!(ev, vec![Event::SwitchedIn { at: O, member: 1 }]);
    assert_eq!(b.phase(), Phase::Actions);
    let ev = b.play_turn(&[use_move(P, 0), use_move(O, 0)]).unwrap();
    assert_eq!(
        ev.last(),
        Some(&Event::BattleEnded {
            outcome: Outcome::Won(0)
        })
    );
    assert_eq!(b.phase(), Phase::Over(Outcome::Won(0)));
    assert_eq!(
        b.play_turn(&[use_move(P, 0)]).err(),
        Some(BattleError::WrongPhase)
    );
}

#[test]
fn a_self_fainting_move_on_the_last_foes_is_a_draw() {
    let boom = with(mv(7, NORMAL, 250), effects::SELF_FAINT);
    let mut weak = fighter("w", [NORMAL; 2], 10, &[mv(2, NORMAL, 10)]);
    weak.hp = 1;
    let (mut b, _) = battle(
        Format::Singles,
        vec![fighter("a", [NORMAL; 2], 100, &[boom])],
        vec![weak],
        1,
    );
    let ev = b.play_turn(&[use_move(P, 0), use_move(O, 0)]).unwrap();
    let faints: Vec<_> = ev
        .iter()
        .filter(|e| matches!(e, Event::Fainted { .. }))
        .collect();
    // The target goes down before the user.
    assert_eq!(
        faints,
        vec![
            &Event::Fainted { at: O, member: 0 },
            &Event::Fainted { at: P, member: 0 }
        ]
    );
    assert_eq!(b.phase(), Phase::Over(Outcome::Draw));
}

#[test]
fn turn_limit_is_a_draw() {
    let wait = status(9, effects::NOTHING, Target::User);
    let mut r = rules();
    r.turn_limit = 3;
    let mut b = Battle::new(
        Format::Singles,
        [
            vec![fighter("a", [NORMAL; 2], 100, &[wait])],
            vec![fighter("b", [NORMAL; 2], 100, &[wait])],
        ],
        chart(),
        r,
        1,
    )
    .unwrap();
    b.start();
    for _ in 0..3 {
        b.play_turn(&[use_move(P, 0), use_move(O, 0)]).unwrap();
    }
    assert_eq!(b.phase(), Phase::Over(Outcome::Draw));
}

#[test]
fn a_move_with_no_pp_left() {
    let mut once = mv(1, NORMAL, 40);
    once.pp = 1;
    let (mut b, _) = battle(
        Format::Singles,
        vec![fighter("a", [NORMAL; 2], 100, &[once])],
        vec![fighter(
            "b",
            [GHOST; 2],
            80,
            &[status(9, effects::NOTHING, Target::User)],
        )],
        1,
    );
    b.play_turn(&[use_move(P, 0), use_move(O, 0)]).unwrap();
    assert_eq!(b.legal_actions(P), vec![Action::OutOfPp]);
    assert_eq!(
        b.play_turn(&[use_move(P, 0), use_move(O, 0)]).err(),
        Some(BattleError::Invalid(P))
    );
    let ev = b
        .play_turn(&[
            Choice {
                at: P,
                action: Action::OutOfPp,
            },
            use_move(O, 0),
        ])
        .unwrap();
    // It even hits a ghost, and the user takes a quarter back.
    let (_, _, dealt) = hits(&ev)[0];
    assert!(dealt > 0);
    assert!(ev.contains(&Event::Hurt {
        target: P,
        damage: (dealt / 4).max(1),
        hp: 150 - (dealt / 4).max(1),
        by: HurtBy::Recoil
    }));
}

#[test]
fn confusion_runs_out_and_sometimes_hurts() {
    let confuse = status(30, effects::CONFUSE, Target::Selected);
    let mut hurt = 0;
    let mut ended = 0;
    for seed in 0..30 {
        let (mut b, _) = battle(
            Format::Singles,
            vec![fighter("a", [NORMAL; 2], 100, &[confuse])],
            vec![fighter("b", [NORMAL; 2], 80, &[mv(1, NORMAL, 1)])],
            seed,
        );
        let ev = b.play_turn(&[use_move(P, 0), use_move(O, 0)]).unwrap();
        assert!(ev.contains(&Event::Confused { target: O }));
        for _ in 0..6 {
            let ev = b.play_turn(&[use_move(P, 0), use_move(O, 0)]).unwrap();
            hurt += ev
                .iter()
                .filter(|e| {
                    matches!(
                        e,
                        Event::Hurt {
                            by: HurtBy::Confusion,
                            damage: 16..=19,
                            ..
                        }
                    )
                })
                .count();
            ended += usize::from(ev.contains(&Event::ConfusionEnded { target: O }));
        }
    }
    assert!(hurt > 5, "{hurt}");
    // Confused for 1..=4 turns of acting: it ends within the 6 turns every time (it can be
    // confused again by the repeated move, so at least once each).
    assert!(ended >= 30, "{ended}");
}

#[test]
fn doubles_spread_halves_and_hits_everyone_listed() {
    let wave = MoveData {
        target: Target::BothFoes,
        ..mv(40, WATER, 80)
    };
    let everyone = MoveData {
        target: Target::AllOthers,
        ..mv(41, GROUND, 80)
    };
    let wait = status(9, effects::NOTHING, Target::User);
    let team = |n: &str| {
        vec![
            fighter(n, [NORMAL; 2], 100, &[wave, everyone, wait]),
            fighter(n, [NORMAL; 2], 90, &[wait]),
        ]
    };
    let (mut b, _) = battle(Format::Doubles, team("p"), team("o"), 4);
    let both = b.estimate(P, 0, O).0;
    let full = b.estimate(P, 1, O).0;
    // 37 vs 17+2 before the roll.
    assert_eq!(both, 19 * 93 / 100);
    assert_eq!(full, 37 * 93 / 100);
    let ev = b
        .play_turn(&[
            use_move(P, 0),
            use_move(P2, 0),
            use_move(O, 2),
            use_move(O2, 0),
        ])
        .unwrap();
    let targets: Vec<Slot> = hits(&ev).iter().map(|h| h.1).collect();
    assert_eq!(targets, vec![O, O2]);
    let ev = b
        .play_turn(&[
            use_move(P, 1),
            use_move(P2, 0),
            use_move(O, 2),
            use_move(O2, 0),
        ])
        .unwrap();
    let targets: Vec<Slot> = hits(&ev).iter().map(|h| h.1).collect();
    assert_eq!(targets, vec![O, P2, O2]);
    // With one foe down the spread move does full damage.
    b.member_mut(1, 1).hp = 0;
    assert_eq!(b.estimate(P, 0, O).0, 37 * 93 / 100);
}

#[test]
fn doubles_targets_ally_and_redirects() {
    let hit = mv(1, NORMAL, 40);
    let wait = status(9, effects::NOTHING, Target::User);
    let team = || {
        vec![
            fighter("a", [NORMAL; 2], 100, &[hit, wait]),
            fighter("b", [NORMAL; 2], 50, &[hit, wait]),
        ]
    };
    let (mut b, _) = battle(Format::Doubles, team(), team(), 2);
    assert_eq!(b.choosable_targets(P, 0), vec![O, O2, P2]);
    assert!(b.choosable_targets(P, 1).is_empty());
    // Hitting the ally works.
    let ev = b
        .play_turn(&[
            aimed(P, 0, P2),
            use_move(P2, 1),
            use_move(O, 1),
            use_move(O2, 1),
        ])
        .unwrap();
    assert_eq!(hits(&ev)[0].1, P2);
    // A chosen foe that's down passes to the other foe.
    b.member_mut(1, 1).hp = 0;
    let ev = b
        .play_turn(&[aimed(P, 0, O2), use_move(P2, 1), use_move(O, 1)])
        .unwrap();
    assert_eq!(hits(&ev)[0].1, O);
    // Aiming at itself is refused.
    assert!(b.phase() == Phase::Actions);
    assert_eq!(
        b.play_turn(&[aimed(P, 0, P), use_move(P2, 1), use_move(O, 1)])
            .err(),
        Some(BattleError::Invalid(P))
    );
    // An ally that's down: nothing to hit.
    b.member_mut(0, 1).hp = 0;
    let ev = b.play_turn(&[aimed(P, 0, P2), use_move(O, 1)]).unwrap();
    assert!(ev.contains(&Event::NoTarget { by: P }));
}

#[test]
fn doubles_faints_are_replaced_and_empty_slots_stay_empty() {
    let big = MoveData {
        target: Target::BothFoes,
        ..mv(1, FIGHTING, 250)
    };
    let wait = status(9, effects::NOTHING, Target::User);
    let mut foes: Vec<Fighter> = (0..3)
        .map(|i| fighter(&format!("f{i}"), [NORMAL; 2], 10, &[wait]))
        .collect();
    for f in &mut foes {
        f.hp = 1;
    }
    let (mut b, _) = battle(
        Format::Doubles,
        vec![
            fighter("a", [NORMAL; 2], 100, &[big]),
            fighter("b", [NORMAL; 2], 100, &[wait]),
        ],
        foes,
        1,
    );
    b.play_turn(&[
        use_move(P, 0),
        use_move(P2, 0),
        use_move(O, 0),
        use_move(O2, 0),
    ])
    .unwrap();
    // Two down, one left to send in.
    assert_eq!(b.pending_replacements(), &[O]);
    assert_eq!(b.fighter(O2), None);
    assert_eq!(
        b.replace(&[(O, 2), (O2, 2)]).err(),
        Some(BattleError::Invalid(O2))
    );
    b.replace(&[(O, 2)]).unwrap();
    // Only the fighters on the field choose.
    let ev = b
        .play_turn(&[use_move(P, 0), use_move(P2, 0), use_move(O, 0)])
        .unwrap();
    assert_eq!(
        ev.last(),
        Some(&Event::BattleEnded {
            outcome: Outcome::Won(0)
        })
    );
}

#[test]
fn missing_and_bad_choices() {
    let (mut b, _) = battle(
        Format::Singles,
        vec![fighter("a", [NORMAL; 2], 100, &[mv(1, NORMAL, 40)])],
        vec![fighter("b", [NORMAL; 2], 100, &[mv(1, NORMAL, 40)])],
        1,
    );
    assert_eq!(
        b.play_turn(&[use_move(P, 0)]).err(),
        Some(BattleError::Missing(O))
    );
    assert_eq!(
        b.play_turn(&[use_move(P, 3), use_move(O, 0)]).err(),
        Some(BattleError::Invalid(P))
    );
    assert_eq!(
        b.play_turn(&[use_move(P, 0), use_move(P, 0), use_move(O, 0)])
            .err(),
        Some(BattleError::Invalid(P))
    );
    assert_eq!(
        b.play_turn(&[switch(P, 0), use_move(O, 0)]).err(),
        Some(BattleError::Invalid(P))
    );
    assert_eq!(b.turn(), 0);
}

#[test]
fn multi_hit_drain_recoil_and_fixed_damage() {
    let multi = with(mv(1, NORMAL, 10), effects::MULTI_HIT);
    let drain = with(mv(2, GRASS, 60), effects::DRAIN_HALF);
    let recoil = with(mv(3, NORMAL, 120), effects::RECOIL_THIRD);
    let fixed = with(mv(4, NORMAL, 1), effects::FIXED_40);
    let (mut b, _) = battle(
        Format::Singles,
        vec![fighter(
            "a",
            [NORMAL; 2],
            100,
            &[multi, drain, recoil, fixed],
        )],
        vec![fighter(
            "b",
            [WATER; 2],
            80,
            &[status(9, effects::NOTHING, Target::User)],
        )],
        8,
    );
    b.member_mut(1, 0).stats.hp = 999;
    b.member_mut(1, 0).hp = 999;
    let ev = b.play_turn(&[use_move(P, 0), use_move(O, 0)]).unwrap();
    let n = hits(&ev).len();
    assert!((2..=5).contains(&n));
    assert!(ev.contains(&Event::HitTimes { count: n as u8 }));

    b.member_mut(0, 0).hp = 50;
    let ev = b.play_turn(&[use_move(P, 1), use_move(O, 0)]).unwrap();
    let dealt = hits(&ev)[0].2;
    assert!(ev.contains(&Event::Healed {
        target: P,
        amount: (dealt / 2).max(1),
        hp: 50 + (dealt / 2).max(1)
    }));

    let hp = b.fighter(P).unwrap().hp;
    let ev = b.play_turn(&[use_move(P, 2), use_move(O, 0)]).unwrap();
    let dealt = hits(&ev)[0].2;
    assert_eq!(b.fighter(P).unwrap().hp, hp - (dealt / 3).max(1));

    let before = b.fighter(O).unwrap().hp;
    let ev = b.play_turn(&[use_move(P, 3), use_move(O, 0)]).unwrap();
    assert_eq!(hits(&ev)[0].2, 40);
    assert_eq!(b.fighter(O).unwrap().hp, before - 40);
}

#[test]
fn added_effects_roll_their_chance() {
    let burn = MoveData {
        chance: 100,
        ..with(mv(1, FIRE, 1), effects::BURN_CHANCE)
    };
    let never = MoveData {
        chance: 0,
        ..with(mv(2, FIRE, 1), effects::BURN_CHANCE)
    };
    let wait = status(9, effects::NOTHING, Target::User);
    let (mut b, _) = battle(
        Format::Singles,
        vec![fighter("a", [NORMAL; 2], 100, &[never, burn])],
        vec![
            fighter("b", [NORMAL; 2], 80, &[wait]),
            fighter("c", [FIRE; 2], 80, &[wait]),
        ],
        1,
    );
    b.play_turn(&[use_move(P, 0), use_move(O, 0)]).unwrap();
    assert_eq!(b.fighter(O).unwrap().ailment, None);
    let ev = b.play_turn(&[use_move(P, 1), use_move(O, 0)]).unwrap();
    assert!(ev.contains(&Event::Ailment {
        target: O,
        ailment: Ailment::Burn
    }));
    // Fire types don't burn.
    b.play_turn(&[use_move(P, 1), switch(O, 1)]).unwrap();
    assert_eq!(b.fighter(O).unwrap().ailment, None);
}

#[test]
fn flinch_only_stops_a_slower_target() {
    let flinch_hit = MoveData {
        chance: 100,
        ..with(mv(1, DARK, 1), effects::FLINCH_CHANCE)
    };
    let (mut b, _) = battle(
        Format::Singles,
        vec![fighter("a", [NORMAL; 2], 100, &[flinch_hit])],
        vec![fighter("b", [NORMAL; 2], 80, &[flinch_hit])],
        1,
    );
    let ev = b.play_turn(&[use_move(P, 0), use_move(O, 0)]).unwrap();
    assert!(ev.contains(&Event::CantAct {
        by: O,
        why: Blocked::Flinched
    }));
    assert!(!b.fighter(O).unwrap().volatile.flinched);
}

#[test]
fn fire_thaws_a_frozen_target_and_screens_wear_off() {
    let wait = status(9, effects::NOTHING, Target::User);
    let wall = status(50, effects::PHYSICAL_WALL, Target::User);
    let (mut b, _) = battle(
        Format::Singles,
        vec![fighter(
            "a",
            [NORMAL; 2],
            100,
            &[mv(1, FIRE, 10), mv(2, NORMAL, 80)],
        )],
        vec![fighter("b", [NORMAL; 2], 50, &[wait, wall])],
        1,
    );
    b.member_mut(1, 0).ailment = Some(Ailment::Freeze);
    let ev = b.play_turn(&[use_move(P, 0), use_move(O, 0)]).unwrap();
    assert!(ev.contains(&Event::Thawed { target: O }));
    // 37 with STAB: 55.
    assert_eq!(b.estimate(P, 1, O).0, 55 * 93 / 100);
    let ev = b.play_turn(&[use_move(P, 0), use_move(O, 1)]).unwrap();
    assert!(ev.contains(&Event::ScreenUp {
        side: 1,
        screen: Screen::PhysicalWall
    }));
    // 35 / 2 + 2 = 19, with STAB 28.
    assert_eq!(b.estimate(P, 1, O).0, 28 * 93 / 100);
    let mut ended = false;
    for _ in 0..4 {
        let ev = b.play_turn(&[use_move(P, 0), use_move(O, 0)]).unwrap();
        ended |= ev.contains(&Event::ScreenEnded {
            side: 1,
            screen: Screen::PhysicalWall,
        });
    }
    assert!(ended);
}

#[test]
fn crit_immunity_and_luck() {
    let count_crits = |luck: u8, ability: u8| {
        let mut crits = 0;
        let mut total = 0;
        for seed in 0..40 {
            let mut a = fighter("a", [NORMAL; 2], 100, &[mv(1, NORMAL, 1)]);
            a.luck = luck;
            let mut o = fighter(
                "b",
                [NORMAL; 2],
                80,
                &[status(9, effects::NOTHING, Target::User)],
            );
            o.ability = ability;
            o.stats.hp = 999;
            o.hp = 999;
            let (mut b, _) = battle(Format::Singles, vec![a], vec![o], seed);
            for _ in 0..19 {
                let ev = b.play_turn(&[use_move(P, 0), use_move(O, 0)]).unwrap();
                for e in ev {
                    if let Event::Hit { crit, .. } = e {
                        total += 1;
                        crits += u32::from(crit);
                    }
                }
            }
        }
        (crits, total)
    };
    let (none, _) = count_crits(255, abilities::NO_CRITS_A);
    assert_eq!(none, 0);
    let (base, n) = count_crits(0, 0);
    let (lucky, _) = count_crits(255, 0);
    // About 1/16 and 1/8 of 760 hits.
    assert!((25..75).contains(&base), "{base}/{n}");
    assert!((65..135).contains(&lucky), "{lucky}/{n}");
}

#[test]
fn same_seed_same_battle_with_the_ai() {
    let run = |seed: u64| {
        let moves = [
            mv(1, FIRE, 60),
            mv(2, WATER, 60),
            with(
                MoveData {
                    chance: 30,
                    ..mv(3, ELECTRIC, 50)
                },
                effects::PARALYZE_CHANCE,
            ),
            status(4, effects::USER_UP2_FIRST, Target::User),
        ];
        let team = |types: [[u8; 2]; 3]| -> Vec<Fighter> {
            types
                .iter()
                .enumerate()
                .map(|(i, &t)| fighter(&format!("f{i}"), t, 60 + i as u16 * 20, &moves))
                .collect()
        };
        let (mut b, mut log) = battle(
            Format::Doubles,
            team([[FIRE; 2], [WATER; 2], [GRASS; 2]]),
            team([[GRASS; 2], [GROUND; 2], [NORMAL; 2]]),
            seed,
        );
        let mut rng = Rng::new(seed ^ 0xABCD);
        for _ in 0..200 {
            match b.phase() {
                Phase::Actions => {
                    let mut c = ai::choose(&b, 0, &mut rng);
                    c.extend(ai::choose(&b, 1, &mut rng));
                    log.extend(b.play_turn(&c).unwrap());
                }
                Phase::Replacements => {
                    let mut p = ai::replacements(&b, 0);
                    p.extend(ai::replacements(&b, 1));
                    log.extend(b.replace(&p).unwrap());
                }
                Phase::Over(_) => break,
            }
        }
        (b.phase(), log)
    };
    let (phase, log) = run(42);
    assert!(matches!(phase, Phase::Over(_)), "{phase:?}");
    assert_eq!(run(42).1, log);
    assert_ne!(run(43).1, log);
}

#[test]
fn ai_prefers_effective_moves() {
    let wait = status(9, effects::NOTHING, Target::User);
    let (b, _) = battle(
        Format::Singles,
        vec![fighter("a", [GRASS; 2], 100, &[wait])],
        vec![fighter(
            "b",
            [NORMAL; 2],
            100,
            &[mv(1, WATER, 90), mv(2, FIRE, 90), mv(3, ELECTRIC, 90)],
        )],
        1,
    );
    let mut rng = Rng::new(7);
    for _ in 0..20 {
        let c = ai::choose(&b, 1, &mut rng);
        assert_eq!(c.len(), 1);
        assert_eq!(c[0].at, O);
        assert!(
            matches!(c[0].action, Action::Move { index: 1, .. }),
            "{c:?}"
        );
    }
    // Nothing that can touch the foe: it sometimes switches to a member that can.
    let (b, _) = battle(
        Format::Singles,
        vec![fighter("a", [GHOST; 2], 100, &[wait])],
        vec![
            fighter("b", [NORMAL; 2], 100, &[mv(1, NORMAL, 60)]),
            fighter("c", [NORMAL; 2], 100, &[mv(2, FIRE, 60)]),
        ],
        1,
    );
    let switched = (0..100)
        .filter(|_| ai::choose(&b, 1, &mut rng)[0].action == Action::Switch { member: 1 })
        .count();
    assert!((15..60).contains(&switched), "{switched}");
}

#[test]
fn ai_skips_status_that_cannot_stick() {
    let wait = status(9, effects::NOTHING, Target::User);
    let poison = status(4, effects::POISON, Target::Selected);
    // A weak hit beats a poisoning move on a foe that can't be poisoned.
    let (b, _) = battle(
        Format::Singles,
        vec![fighter("a", [STEEL; 2], 100, &[wait])],
        vec![fighter("b", [NORMAL; 2], 100, &[poison, mv(1, NORMAL, 10)])],
        1,
    );
    let mut rng = Rng::new(7);
    for _ in 0..20 {
        let c = ai::choose(&b, 1, &mut rng);
        assert!(
            matches!(c[0].action, Action::Move { index: 1, .. }),
            "{c:?}"
        );
    }
    // In doubles it aims a status move at the foe it works on.
    let (b, _) = battle(
        Format::Doubles,
        vec![
            fighter("a", [STEEL; 2], 100, &[wait]),
            fighter("c", [NORMAL; 2], 100, &[wait]),
        ],
        vec![
            fighter("b", [NORMAL; 2], 100, &[poison]),
            fighter("d", [NORMAL; 2], 100, &[wait]),
        ],
        1,
    );
    let c = ai::choose(&b, 1, &mut rng);
    assert_eq!(
        c[0].action,
        Action::Move {
            index: 0,
            target: Some(P2)
        }
    );
}

#[test]
fn ai_spares_its_ally_from_everyone_hitting_moves() {
    let wait = status(9, effects::NOTHING, Target::User);
    let everyone = MoveData {
        target: Target::AllOthers,
        ..mv(1, FIGHTING, 120)
    };
    let (mut b, _) = battle(
        Format::Doubles,
        vec![
            fighter("a", [NORMAL; 2], 100, &[wait]),
            fighter("c", [NORMAL; 2], 100, &[wait]),
        ],
        vec![
            fighter("b", [NORMAL; 2], 100, &[everyone, mv(2, NORMAL, 30)]),
            fighter("d", [NORMAL; 2], 100, &[wait]),
        ],
        1,
    );
    // The ally would faint from it.
    b.member_mut(1, 1).hp = 5;
    let mut rng = Rng::new(7);
    for _ in 0..20 {
        let c = ai::choose(&b, 1, &mut rng);
        assert!(
            matches!(c[0].action, Action::Move { index: 1, .. }),
            "{c:?}"
        );
    }
    // An ally it can't touch is no reason to hold back.
    b.member_mut(1, 1).hp = 150;
    b.member_mut(1, 1).types = [GHOST; 2];
    for _ in 0..20 {
        let c = ai::choose(&b, 1, &mut rng);
        assert!(
            matches!(c[0].action, Action::Move { index: 0, .. }),
            "{c:?}"
        );
    }
}

/// A multi battle: side 0's first place has 2 Chao and its second 3, side 1's first place 1
/// and its second 2.
fn multi(seed: u64) -> Battle {
    let m = [mv(1, NORMAL, 40)];
    let team = |side: usize, sizes: [usize; 2]| -> Vec<Fighter> {
        (0..2)
            .flat_map(|place| {
                (0..sizes[place]).map(move |i| {
                    let name = format!("s{side}p{place}m{i}");
                    fighter(&name, [NORMAL; 2], 100 + i as u16, &m)
                })
            })
            .collect()
    };
    let mut b = Battle::new(
        Format::Doubles,
        [team(0, [2, 3]), team(1, [1, 2])],
        chart(),
        rules(),
        seed,
    )
    .unwrap();
    b.set_split(0, 2);
    b.set_split(1, 1);
    b
}

#[test]
fn a_multi_battle_keeps_each_places_team_its_own() {
    let mut b = multi(3);
    b.start();
    // Each place's first Chao leads.
    assert_eq!(
        [P, P2, O, O2].map(|s| b.member(s)),
        [Some(0), Some(2), Some(0), Some(1)]
    );
    assert_eq!(
        (b.owner(0, 1), b.owner(0, 2), b.owner(1, 0)),
        (Some(0), Some(1), Some(0))
    );
    // Only a place's own come in.
    assert_eq!(b.bench_for(P), [1]);
    assert_eq!(b.bench_for(P2), [3, 4]);
    assert!(b.bench_for(O).is_empty());
    assert_eq!(b.bench_for(O2), [2]);
    let switches: Vec<Action> = b
        .legal_actions(P)
        .into_iter()
        .filter(|a| matches!(a, Action::Switch { .. }))
        .collect();
    assert_eq!(switches, [Action::Switch { member: 1 }]);
    let others = [use_move(P2, 0), use_move(O, 0), use_move(O2, 0)];
    let mut choices = vec![switch(P, 3)];
    choices.extend(others);
    assert_eq!(b.play_turn(&choices), Err(BattleError::Invalid(P)));
    // P2's lead and O's only Chao go down: O's place stays empty, P2 sends in its own.
    b.member_mut(0, 2).hp = 1;
    b.member_mut(1, 0).hp = 1;
    b.play_turn(&[
        aimed(P, 0, O),
        aimed(P2, 0, O2),
        aimed(O, 0, P2),
        aimed(O2, 0, P2),
    ])
    .unwrap();
    assert_eq!(b.phase(), Phase::Replacements);
    assert_eq!(b.pending_replacements(), [P2]);
    assert_eq!(b.member(O), None);
    assert_eq!(b.replace(&[(P2, 1)]), Err(BattleError::Invalid(P2)));
    b.replace(&[(P2, 3)]).unwrap();
    assert_eq!(b.member(P2), Some(3));
    // The battle goes on with one foe, each place sending in only its own, to the end.
    let mut rng = Rng::new(5);
    let mut came_in = Vec::new();
    for _ in 0..500 {
        let ev = match b.phase() {
            Phase::Actions => {
                let mut c = ai::choose(&b, 0, &mut rng);
                c.extend(ai::choose(&b, 1, &mut rng));
                assert!(c.iter().all(|c| c.at != O), "{c:?}");
                b.play_turn(&c).unwrap()
            }
            Phase::Replacements => {
                let mut r = ai::replacements(&b, 0);
                r.extend(ai::replacements(&b, 1));
                b.replace(&r).unwrap()
            }
            Phase::Over(_) => break,
        };
        came_in.extend(ev.into_iter().filter_map(|e| match e {
            Event::SwitchedIn { at, member } => Some((at, member)),
            _ => None,
        }));
    }
    assert_eq!(b.phase(), Phase::Over(Outcome::Won(0)));
    for (at, member) in came_in {
        assert_eq!(b.owner(at.side, member), Some(at.pos), "{at:?} {member}");
    }
}

#[test]
fn only_doubles_teams_with_two_places_split() {
    let a = || vec![fighter("a", [NORMAL; 2], 100, &[mv(1, NORMAL, 40)]); 2];
    let mut b = Battle::new(Format::Singles, [a(), a()], chart(), rules(), 1).unwrap();
    b.set_split(0, 1);
    assert_eq!(b.split(0), None);
    let mut b = Battle::new(Format::Doubles, [a(), a()], chart(), rules(), 1).unwrap();
    b.set_split(0, 0);
    b.set_split(1, 2);
    assert_eq!((b.split(0), b.split(1)), (None, None));
    b.start();
    b.set_split(0, 1);
    assert_eq!(b.split(0), None);
    // Without a split the whole side's bench is a slot's.
    assert_eq!(b.bench_for(P), b.bench(0));
}
