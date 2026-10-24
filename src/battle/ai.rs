//! A simple trainer-like opponent: it mostly picks the move that hurts most (type matchups
//! count, and so does hurting its own ally), uses status moves now and then on a foe they'd
//! work on, aims at the foe it hurts most, and rarely switches (only when nothing it has can
//! hurt the foes).

use super::data::{MoveData, Target};
use super::effects::{self, Primary};
use super::engine::{Action, Battle, Choice, Phase};
use super::event::Slot;
use super::field::Weather;
use super::fighter::Fighter;
use super::rng::Rng;

/// Score noise (out of a KO's 100 + damage share), so it doesn't always pick the same move.
const JITTER: u32 = 20;
/// In 100: how often it switches out when nothing it has can hurt the foes.
const SWITCH_CHANCE: u32 = 35;

/// Choices for every fighter `side` has on the field.
pub fn choose(battle: &Battle, side: usize, rng: &mut Rng) -> Vec<Choice> {
    let mut out: Vec<Choice> = Vec::new();
    for at in battle.slots() {
        if at.side != side || !battle.alive(at) {
            continue;
        }
        if battle.forced(at) {
            if let Some(&action) = battle.legal_actions(at).first() {
                out.push(Choice { at, action });
            }
            continue;
        }
        let (action, best) = best_move(battle, at, rng);
        let taken: Vec<usize> = out
            .iter()
            .filter_map(|c| match c.action {
                Action::Switch { member } => Some(member),
                _ => None,
            })
            .collect();
        let action = if best == 0 && battle.can_switch(at) && rng.percent(SWITCH_CHANCE) {
            better_member(battle, at, &taken)
                .map(|member| Action::Switch { member })
                .unwrap_or(action)
        } else {
            action
        };
        out.push(Choice { at, action });
    }
    out
}

/// Members for `side`'s pending replacements: the ones whose moves hurt the foes most.
pub fn replacements(battle: &Battle, side: usize) -> Vec<(Slot, usize)> {
    if battle.phase() != Phase::Replacements {
        return Vec::new();
    }
    let mut taken = Vec::new();
    let mut out = Vec::new();
    for &slot in battle.pending_replacements() {
        if slot.side != side {
            continue;
        }
        if let Some(m) =
            better_member(battle, slot, &taken).or_else(|| first_free(battle, slot, &taken))
        {
            taken.push(m);
            out.push((slot, m));
        }
    }
    out
}

fn first_free(battle: &Battle, at: Slot, taken: &[usize]) -> Option<usize> {
    battle
        .bench_for(at)
        .into_iter()
        .find(|m| !taken.contains(m))
}

/// The member that can come in at `at` with the best type matchup against the foes on the
/// field, if any can hurt them at all.
fn better_member(battle: &Battle, at: Slot, taken: &[usize]) -> Option<usize> {
    let side = at.side;
    let foes: Vec<&Fighter> = battle
        .slots()
        .into_iter()
        .filter(|s| s.side != side && battle.alive(*s))
        .filter_map(|s| battle.fighter(s))
        .collect();
    battle
        .bench_for(at)
        .into_iter()
        .filter(|m| !taken.contains(m))
        .map(|m| (m, matchup(battle, &battle.team(side)[m], &foes)))
        .filter(|&(_, score)| score > 0)
        .max_by_key(|&(m, score)| (score, std::cmp::Reverse(m)))
        .map(|(m, _)| m)
}

/// The best type multiplier (hundredths) x power of its damaging moves against any foe.
fn matchup(battle: &Battle, f: &Fighter, foes: &[&Fighter]) -> u32 {
    let mut best = 0;
    for m in f.moves.iter().filter(|m| m.data.power > 0 && m.pp > 0) {
        for foe in foes {
            let mult =
                battle
                    .chart()
                    .multiplier(m.data.type_id, foe.types, foe.volatile.identified);
            let stab = if f.has_type(m.data.type_id) { 3 } else { 2 };
            best = best.max(mult * u32::from(m.data.power) * stab / 2);
        }
    }
    best
}

/// The move (with target) it scores highest, and the best damage score any move reached.
fn best_move(battle: &Battle, at: Slot, rng: &mut Rng) -> (Action, u32) {
    let Some(f) = battle.fighter(at) else {
        return (Action::OutOfPp, 0);
    };
    let usable = battle.usable_moves(at);
    if usable.is_empty() {
        return (Action::OutOfPp, 1);
    }
    let foes = battle.foes(at);
    let mut best: Option<(u32, Action)> = None;
    let mut best_damage = 0;
    for &index in &usable {
        let m = &f.moves[index];
        let (score, target) = if m.data.power > 0 || effects::is_fixed_damage(m.data.effect) {
            let mut top = (0, None);
            for &foe in &foes {
                let s = damage_score(battle, at, index, foe);
                if s > top.0 {
                    top = (s, Some(foe));
                }
            }
            // A move that hits both foes counts for both.
            if matches!(m.data.target, Target::BothFoes | Target::AllOthers) && foes.len() > 1 {
                let both: u32 = foes
                    .iter()
                    .map(|&t| damage_score(battle, at, index, t))
                    .sum();
                top.0 = top.0.max(both * 3 / 4);
            }
            best_damage = best_damage.max(top.0);
            // One that hits everyone else hurts the ally too: never worth knocking it out.
            if m.data.target == Target::AllOthers
                && let Some(ally) = battle.ally(at)
            {
                top.0 = if knocks_out(battle, at, index, ally) {
                    0
                } else {
                    top.0.saturating_sub(damage_score(battle, at, index, ally))
                };
            }
            top
        } else {
            // The foe it would work best on (any foe for a move that picks none).
            let picks: Vec<Option<Slot>> = if foes.is_empty() {
                vec![None]
            } else {
                foes.iter().map(|&s| Some(s)).collect()
            };
            picks
                .into_iter()
                .map(|t| (status_score(battle, at, f, &m.data, t), t))
                .fold(
                    (0, foes.first().copied()),
                    |a, b| {
                        if b.0 > a.0 { b } else { a }
                    },
                )
        };
        // Fainting on purpose only pays with others left to carry on, and even then rarely.
        let score = match m.data.effect {
            effects::SELF_FAINT | effects::FAINT_LOWER_TARGET
                if battle.bench_for(at).is_empty() =>
            {
                0
            }
            effects::SELF_FAINT | effects::FAINT_LOWER_TARGET => score / 3,
            _ => score,
        };
        if score == 0 {
            continue;
        }
        let score = score + rng.below(JITTER);
        if best.is_none_or(|(s, _)| score > s) {
            let target = (m.data.target == Target::Selected)
                .then_some(target)
                .flatten();
            best = Some((score, Action::Move { index, target }));
        }
    }
    let action = best.map(|b| b.1).unwrap_or_else(|| {
        // Nothing scores: use the first usable move anyway.
        let index = usable[0];
        let target = (f.moves[index].data.target == Target::Selected)
            .then(|| foes.first().copied())
            .flatten();
        Action::Move { index, target }
    });
    (action, best_damage)
}

/// Whether move `index` would knock the fighter at `target` out.
fn knocks_out(battle: &Battle, at: Slot, index: usize, target: Slot) -> bool {
    let (damage, mult) = battle.estimate(at, index, target);
    mult > 0
        && battle
            .fighter(target)
            .is_some_and(|t| damage >= u32::from(t.hp))
}

/// Expected share of the target's HP (in percent, up to 100) plus a bonus for a knock-out.
fn damage_score(battle: &Battle, at: Slot, index: usize, target: Slot) -> u32 {
    let (damage, mult) = battle.estimate(at, index, target);
    let Some(t) = battle.fighter(target) else {
        return 0;
    };
    if mult == 0 || t.hp == 0 {
        return 0;
    }
    let acc = battle
        .fighter(at)
        .and_then(|f| f.moves.get(index))
        .map(|m| {
            if m.data.accuracy == 0 {
                100
            } else {
                u32::from(m.data.accuracy)
            }
        })
        .unwrap_or(100);
    let share = (damage * 100 / u32::from(t.hp)).min(100);
    let ko = if damage >= u32::from(t.hp) { 60 } else { 0 };
    ((share + ko) * acc / 100).max(1)
}

/// A rough worth for status move `mv` aimed at `foe`: high while it would do something, 0
/// when it wouldn't.
fn status_score(battle: &Battle, at: Slot, f: &Fighter, mv: &MoveData, foe: Option<Slot>) -> u32 {
    let hurt = u32::from(f.hp) * 2 < u32::from(f.stats.hp);
    let open_foe = |pick: fn(&Fighter) -> bool| {
        foe.and_then(|s| battle.fighter(s))
            .is_some_and(|t| t.volatile.decoy == 0 && pick(t))
    };
    match effects::primary(mv.effect) {
        Some(Primary::Ailment(a)) => {
            if foe.is_some_and(|t| battle.ailment_would_stick(at, t, a, mv)) {
                40
            } else {
                0
            }
        }
        Some(Primary::Confuse | Primary::ConfuseAndRaise(..)) => {
            if foe.is_some_and(|t| battle.confusion_would_stick(at, t, mv)) {
                25
            } else {
                0
            }
        }
        Some(Primary::Stats(true, changes)) => {
            let room = changes
                .iter()
                .any(|(stat, by)| (f.stage(stat) + by).abs() <= 2);
            if room { 30 } else { 0 }
        }
        Some(Primary::Stats(false, _)) => {
            if open_foe(|_| true) {
                15
            } else {
                0
            }
        }
        Some(Primary::Weather(w)) => {
            if battle.field().weather == w || w == Weather::Clear {
                0
            } else {
                20
            }
        }
        Some(Primary::Screen(s)) => {
            if battle.field().sides[at.side].get(s) == 0 {
                25
            } else {
                0
            }
        }
        Some(
            Primary::Heal(..) | Primary::WeatherHeal | Primary::SleepHeal | Primary::ReleaseHeal,
        ) => {
            if hurt {
                70
            } else {
                0
            }
        }
        Some(Primary::Focus) => {
            if f.volatile.crit_stages < 2 {
                15
            } else {
                0
            }
        }
        Some(Primary::Seed) => {
            if open_foe(|t| {
                t.volatile.seeded_by.is_none() && !t.has_type(super::data::types::GRASS)
            }) {
                35
            } else {
                0
            }
        }
        Some(Primary::Decoy) => {
            if f.volatile.decoy == 0 && !hurt {
                20
            } else {
                0
            }
        }
        Some(Primary::Shield | Primary::Brace) => {
            if f.volatile.shield_streak == 0 {
                10
            } else {
                0
            }
        }
        Some(Primary::Hazard) => {
            let side = 1 - at.side;
            if battle.field().sides[side].hazards < 3 && !battle.bench(side).is_empty() {
                20
            } else {
                0
            }
        }
        Some(Primary::DelayedHeal | Primary::Root) => {
            if hurt {
                30
            } else {
                0
            }
        }
        Some(Primary::StoreUp | Primary::Curl | Primary::Shrink | Primary::PowerUpElectric) => 10,
        Some(Primary::ResetStages) => 5,
        Some(
            Primary::Nothing
            | Primary::Fails
            | Primary::Infatuate
            | Primary::PassState
            | Primary::ForceSwitch
            | Primary::FaintLowerTarget
            | Primary::Countdown
            | Primary::Vengeance
            | Primary::PpRancor
            | Primary::HelpAlly
            | Primary::DrawMoves
            | Primary::BounceStatus
            | Primary::StealMove,
        )
        | None => 0,
        Some(_) => 10,
    }
}
