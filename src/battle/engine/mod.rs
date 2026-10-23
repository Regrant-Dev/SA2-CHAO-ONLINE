//! The battle: two sides of up to 6 fighters, one (singles) or two (doubles) out at a time.
//!
//! `start` sends the leads out; then each turn every fighter on the field gets a `Choice` and
//! `play_turn` runs it: switches first, then moves by priority and speed (ties at random), then
//! weather, status and the other end-of-turn effects. Fainted fighters are replaced through
//! `replace` before the next turn. Every call returns the `Event`s it caused, in order.
//!
//! - `moves`: using a move: the checks before it, targets, hits and damage.
//! - `status`: status moves and the stat, status and condition changes moves cause.
//! - `turn_end`: the end of a turn.

mod moves;
mod status;
mod turn_end;

use super::abilities::{self, Entry};
use super::data::{MoveData, Target, TypeChart, types};
use super::effects;
use super::event::{Condition, Event, HurtBy, Outcome, Slot};
use super::field::{Field, Weather};
use super::fighter::{Ailment, Fighter, Stat, Volatile};
use super::rng::Rng;

/// Most fighters on a team.
pub const TEAM_MAX: usize = 6;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Format {
    Singles,
    Doubles,
}

impl Format {
    /// Fighters out per side.
    pub fn slots(self) -> usize {
        match self {
            Format::Singles => 1,
            Format::Doubles => 2,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rules {
    /// The move a fighter uses with no PP left (read from the ROM).
    pub out_of_pp: MoveData,
    /// After this many turns the battle is a draw.
    pub turn_limit: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    /// Move `index` of the fighter's moves; `target` matters for a single-target move in
    /// doubles (`None` picks a foe).
    Move { index: usize, target: Option<Slot> },
    /// The only move left when no move can be used.
    OutOfPp,
    /// Switch to team member `member`.
    Switch { member: usize },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Choice {
    pub at: Slot,
    pub action: Action,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    /// Waiting for `play_turn`.
    Actions,
    /// Waiting for `replace` (see `pending_replacements`).
    Replacements,
    Over(Outcome),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BattleError {
    /// A team is empty, too big or all fainted.
    BadTeam,
    /// Not the call this phase waits for.
    WrongPhase,
    /// A fighter on the field (or a slot to fill) has no choice.
    Missing(Slot),
    /// A choice that isn't allowed for this slot.
    Invalid(Slot),
}

/// Battler order for "everyone" effects, as Gen 3 numbers them.
const FIELD_ORDER: [Slot; 4] = [
    Slot::new(0, 0),
    Slot::new(1, 0),
    Slot::new(0, 1),
    Slot::new(1, 1),
];

pub struct Battle {
    format: Format,
    teams: [Vec<Fighter>; 2],
    /// Team index out in each slot.
    active: [[Option<usize>; 2]; 2],
    /// A slot's faint has been reported.
    down: [[bool; 2]; 2],
    field: Field,
    chart: TypeChart,
    rules: Rules,
    rng: Rng,
    turn: u32,
    phase: Phase,
    started: bool,
    pending: Vec<Slot>,
    /// Moves a random-move effect picks from.
    move_table: Vec<MoveData>,
    /// Ids of the moves sound-blocking abilities stop.
    sound_moves: Vec<u16>,
    /// For the next turn: who a fighter's passing move hands on to.
    pass_picks: Vec<(Slot, usize)>,
}

/// Turn order: switches first, then priority, speed and a random tie-break.
type OrderKey = (u8, std::cmp::Reverse<i8>, std::cmp::Reverse<u32>, u32);

/// One planned action in a turn's queue.
#[derive(Clone, Copy)]
struct Planned {
    at: Slot,
    member: usize,
    action: Action,
    done: bool,
}

impl Battle {
    /// A battle between `teams[0]` (the player) and `teams[1]`; `seed` decides every roll.
    pub fn new(
        format: Format,
        teams: [Vec<Fighter>; 2],
        chart: TypeChart,
        rules: Rules,
        seed: u64,
    ) -> Result<Battle, BattleError> {
        for team in &teams {
            if team.is_empty() || team.len() > TEAM_MAX || team.iter().all(Fighter::fainted) {
                return Err(BattleError::BadTeam);
            }
        }
        Ok(Battle {
            format,
            teams,
            active: [[None; 2]; 2],
            down: [[false; 2]; 2],
            field: Field::default(),
            chart,
            rules,
            rng: Rng::new(seed),
            turn: 0,
            phase: Phase::Actions,
            started: false,
            pending: Vec::new(),
            move_table: Vec::new(),
            sound_moves: Vec::new(),
            pass_picks: Vec::new(),
        })
    }

    /// The moves a random-move effect can call (the ROM's move table). Without it, it picks
    /// from the moves the fighters in the battle know.
    pub fn set_move_table(&mut self, moves: Vec<MoveData>) {
        self.move_table = moves;
    }

    /// The ids of sound moves, which the sound-blocking abilities stop.
    pub fn set_sound_moves(&mut self, ids: Vec<u16>) {
        self.sound_moves = ids;
    }

    /// From now on the rolls come from `seed` (a live battle takes a new seed each step).
    pub fn reseed(&mut self, seed: u64) {
        self.rng = Rng::new(seed);
    }

    /// Sends out the first fighters able to battle and runs their entry abilities.
    pub fn start(&mut self) -> Vec<Event> {
        let mut ev = Vec::new();
        if self.started {
            return ev;
        }
        self.started = true;
        let mut entered = Vec::new();
        for side in 0..2 {
            let leads: Vec<usize> = (0..self.teams[side].len())
                .filter(|&m| !self.teams[side][m].fainted())
                .take(self.format.slots())
                .collect();
            for (pos, m) in leads.into_iter().enumerate() {
                let at = Slot::new(side, pos);
                self.place(at, m, &mut ev);
                let v = &mut self.teams[side][m].volatile;
                v.fresh -= 1;
                v.loafing = false;
                entered.push(at);
            }
        }
        self.entry_abilities(entered, &mut ev);
        self.check_end(&mut ev);
        ev
    }

    pub fn format(&self) -> Format {
        self.format
    }

    pub fn phase(&self) -> Phase {
        self.phase
    }

    /// Turns played so far.
    pub fn turn(&self) -> u32 {
        self.turn
    }

    pub fn team(&self, side: usize) -> &[Fighter] {
        &self.teams[side]
    }

    pub fn field(&self) -> &Field {
        &self.field
    }

    pub fn chart(&self) -> &TypeChart {
        &self.chart
    }

    pub fn rules(&self) -> &Rules {
        &self.rules
    }

    /// The team index out at `at`.
    pub fn member(&self, at: Slot) -> Option<usize> {
        self.active
            .get(at.side)
            .and_then(|s| s.get(at.pos))
            .copied()
            .flatten()
    }

    /// The fighter out at `at` (a fainted one stays until replaced).
    pub fn fighter(&self, at: Slot) -> Option<&Fighter> {
        self.member(at).map(|m| &self.teams[at.side][m])
    }

    /// A fighter able to battle is out at `at`.
    pub fn alive(&self, at: Slot) -> bool {
        self.fighter(at).is_some_and(|f| !f.fainted())
    }

    /// Every slot of this format, in field order.
    pub fn slots(&self) -> Vec<Slot> {
        FIELD_ORDER
            .iter()
            .copied()
            .filter(|s| s.pos < self.format.slots())
            .collect()
    }

    /// Slots with a fighter able to battle, in field order.
    fn on_field(&self) -> Vec<Slot> {
        self.slots()
            .into_iter()
            .filter(|&s| self.alive(s))
            .collect()
    }

    /// The first fighter on the field with this ability.
    fn ability_on_field(&self, ability: u8) -> Option<Slot> {
        self.on_field()
            .into_iter()
            .find(|&s| self.fighter(s).is_some_and(|f| f.ability == ability))
    }

    /// Foes able to battle, left to right.
    pub fn foes(&self, at: Slot) -> Vec<Slot> {
        (0..self.format.slots())
            .map(|pos| Slot::new(1 - at.side, pos))
            .filter(|&s| self.alive(s))
            .collect()
    }

    /// The ally able to battle (doubles).
    pub fn ally(&self, at: Slot) -> Option<Slot> {
        let s = Slot::new(at.side, 1 - at.pos);
        (self.format == Format::Doubles && self.alive(s)).then_some(s)
    }

    /// Team members that can come in for `side`.
    pub fn bench(&self, side: usize) -> Vec<usize> {
        (0..self.teams[side].len())
            .filter(|&m| !self.teams[side][m].fainted() && !self.active[side].contains(&Some(m)))
            .collect()
    }

    /// Targets the player picks from for move `index`: in doubles, a single-target move can go
    /// to either foe or the ally; otherwise the target is decided by the move (empty).
    pub fn choosable_targets(&self, at: Slot, index: usize) -> Vec<Slot> {
        let Some(mv) = self.fighter(at).and_then(|f| f.moves.get(index)) else {
            return Vec::new();
        };
        if self.format != Format::Doubles || mv.data.target != Target::Selected {
            return Vec::new();
        }
        let mut out = self.foes(at);
        out.extend(self.ally(at));
        out
    }

    /// Whether the fighter at `at` is carrying on with a move (charging, rampaging, resting
    /// after one...): its choice is ignored and it can't switch this turn.
    pub fn forced(&self, at: Slot) -> bool {
        self.fighter(at)
            .is_some_and(|f| f.volatile.lock.is_some() || f.volatile.recharge)
    }

    /// Whether the move in slot `index` can be picked: PP left and nothing (sealing, a
    /// provoke, a foe's seal, no repeats) stopping it.
    pub fn usable(&self, at: Slot, index: usize) -> bool {
        let Some(f) = self.fighter(at) else {
            return false;
        };
        let Some(m) = f.moves.get(index) else {
            return false;
        };
        if m.pp == 0 {
            return false;
        }
        let v = &f.volatile;
        if v.sealed.is_some_and(|(id, _)| id == m.data.id) {
            return false;
        }
        if v.provoked > 0 && m.data.power == 0 {
            return false;
        }
        if v.no_repeats && v.last_move == Some(m.data.id) {
            return false;
        }
        if let Some((enc, _)) = v.repeat
            && enc != index
        {
            return false;
        }
        !self.sealed_by_foe(at, m.data.id)
    }

    /// A foe on the field sealing moves it knows knows this one.
    fn sealed_by_foe(&self, at: Slot, move_id: u16) -> bool {
        self.foes(at).into_iter().any(|s| {
            self.fighter(s)
                .is_some_and(|f| f.volatile.sealing && f.moves.iter().any(|m| m.data.id == move_id))
        })
    }

    /// The fighter's moves it can pick this turn.
    pub fn usable_moves(&self, at: Slot) -> Vec<usize> {
        let n = self.fighter(at).map_or(0, |f| f.moves.len());
        (0..n).filter(|&i| self.usable(at, i)).collect()
    }

    /// Whether the fighter at `at` may switch out: nothing binds or holds it, it isn't rooted
    /// and no foe's ability traps it.
    pub fn can_switch(&self, at: Slot) -> bool {
        let Some(f) = self.fighter(at) else {
            return false;
        };
        if self.forced(at) {
            return false;
        }
        let v = &f.volatile;
        if v.bound.is_some() || v.held_by.is_some() || v.rooted {
            return false;
        }
        !self.foes(at).into_iter().any(|s| {
            self.fighter(s)
                .is_some_and(|foe| abilities::traps(foe.ability, f))
        })
    }

    /// What the fighter at `at` may do this turn. A forced fighter has one placeholder action
    /// (any choice for it is fine).
    pub fn legal_actions(&self, at: Slot) -> Vec<Action> {
        let mut out = Vec::new();
        let Some(f) = self.fighter(at).filter(|f| !f.fainted()) else {
            return out;
        };
        if self.forced(at) {
            let index = f.volatile.lock.and_then(|l| l.index).unwrap_or(0);
            out.push(Action::Move {
                index,
                target: None,
            });
            return out;
        }
        let usable = self.usable_moves(at);
        if usable.is_empty() {
            out.push(Action::OutOfPp);
        }
        for index in usable {
            out.push(Action::Move {
                index,
                target: None,
            });
        }
        if self.can_switch(at) {
            for member in self.bench(at.side) {
                out.push(Action::Switch { member });
            }
        }
        out
    }

    /// Slots to fill before the next turn.
    pub fn pending_replacements(&self) -> &[Slot] {
        &self.pending
    }

    /// The fighter's speed for turn order: stages, weather abilities, paralysis.
    pub fn speed(&self, at: Slot) -> u32 {
        let Some(f) = self.fighter(at) else {
            return 0;
        };
        let factor = abilities::speed_factor(f.ability, self.weather());
        let mut s = super::fighter::staged(u32::from(f.stats.speed) * factor, f.stage(Stat::Speed));
        if f.ailment == Some(Ailment::Paralysis) {
            s /= 4;
        }
        s
    }

    /// Whether status move `mv` from the fighter at `at` could give the fighter at `t` the
    /// status `a`: it has none, and no decoy, type, ability or side guard stops it.
    pub fn ailment_would_stick(&self, at: Slot, t: Slot, a: Ailment, mv: &MoveData) -> bool {
        let Some(f) = self.fighter(t).filter(|f| !f.fainted()) else {
            return false;
        };
        let immune_by_chart = a == Ailment::Paralysis
            && self
                .chart
                .multiplier(mv.type_id, f.types, f.volatile.identified)
                == 0;
        f.ailment.is_none()
            && !immune_by_chart
            && self.type_allows(f, a)
            && !abilities::prevents_ailment(f.ability, a)
            && !(a == Ailment::Sleep && self.noise_on())
            && self.status_reachable(at, t, mv)
    }

    /// Whether status move `mv` from the fighter at `at` could confuse the fighter at `t`.
    pub fn confusion_would_stick(&self, at: Slot, t: Slot, mv: &MoveData) -> bool {
        self.fighter(t).is_some_and(|f| {
            !f.fainted()
                && f.volatile.confusion == 0
                && !abilities::prevents_confusion(f.ability)
                && self.status_reachable(at, t, mv)
        })
    }

    /// No decoy, sound-blocking ability or side guard keeps status move `mv` from `t`.
    fn status_reachable(&self, at: Slot, t: Slot, mv: &MoveData) -> bool {
        self.fighter(t).is_some_and(|f| {
            f.volatile.decoy == 0
                && !(abilities::blocks_sound(f.ability) && self.sound_moves.contains(&mv.id))
                && !self.guarded(at, t)
        })
    }

    /// The weather that's working (`Clear` while an ability cancels it).
    pub fn weather(&self) -> Weather {
        let on_field = self
            .on_field()
            .into_iter()
            .filter_map(|s| self.fighter(s).map(|f| f.ability));
        if abilities::weather_works(on_field) {
            self.field.weather
        } else {
            Weather::Clear
        }
    }

    // ---- turns ----

    /// For the next turn: the team member that a passing move used by the fighter at `at` hands
    /// its state to. Without one (or when it can't come in by then), the healthiest member on
    /// the bench gets it.
    pub fn pick_pass(&mut self, at: Slot, member: usize) {
        self.pass_picks.retain(|p| p.0 != at);
        self.pass_picks.push((at, member));
    }

    /// Runs a turn with a choice for every fighter on the field (both sides); a forced
    /// fighter's choice may be left out.
    pub fn play_turn(&mut self, choices: &[Choice]) -> Result<Vec<Event>, BattleError> {
        let out = self.run_turn(choices);
        self.pass_picks.clear();
        out
    }

    fn run_turn(&mut self, choices: &[Choice]) -> Result<Vec<Event>, BattleError> {
        if self.phase != Phase::Actions || !self.started {
            return Err(BattleError::WrongPhase);
        }
        self.check_choices(choices)?;
        let mut ev = vec![Event::TurnStarted {
            turn: self.turn + 1,
        }];
        self.turn += 1;
        for side in &mut self.field.sides {
            side.drawing = None;
        }
        for at in self.slots() {
            if let Some(m) = self.member(at) {
                self.teams[at.side][m].volatile.turn = Default::default();
            }
        }

        // Every fighter's action, a repeat turning a move choice into the repeated move.
        let mut queue = Vec::new();
        for at in self.on_field() {
            let member = self.member(at).unwrap_or(usize::MAX);
            let given = choices.iter().find(|c| c.at == at).map(|c| c.action);
            let action = if self.forced(at) {
                let index = self.fm(at).volatile.lock.and_then(|l| l.index).unwrap_or(0);
                Action::Move {
                    index,
                    target: None,
                }
            } else {
                match (given, self.fm(at).volatile.repeat) {
                    (Some(Action::Move { target, .. }), Some((index, _))) => {
                        Action::Move { index, target }
                    }
                    (Some(a), _) => a,
                    (None, _) => Action::OutOfPp,
                }
            };
            queue.push(Planned {
                at,
                member,
                action,
                done: false,
            });
        }
        let mut keyed: Vec<(Planned, OrderKey)> = Vec::new();
        for p in queue {
            let (class, priority) = match p.action {
                Action::Switch { .. } => (0u8, 0i8),
                _ => (1, self.planned_move(&p).map_or(0, |m| m.priority)),
            };
            let key = (
                class,
                std::cmp::Reverse(priority),
                std::cmp::Reverse(self.speed(p.at)),
                self.rng.next_u32(),
            );
            keyed.push((p, key));
        }
        keyed.sort_by_key(|q| q.1);
        let mut queue: Vec<Planned> = keyed.into_iter().map(|q| q.0).collect();

        // A move that fails if its user is hit first: its user focuses now.
        for p in &queue {
            if !self.forced(p.at)
                && self
                    .planned_move(p)
                    .is_some_and(|m| m.effect == effects::FOCUSED_HIT)
            {
                ev.push(Event::Started {
                    target: p.at,
                    condition: Condition::Focusing,
                });
            }
        }

        for i in 0..queue.len() {
            if queue[i].done {
                continue;
            }
            queue[i].done = true;
            let p = queue[i];
            // Fainted or replaced since choosing: the action is lost.
            if self.member(p.at) != Some(p.member) || !self.alive(p.at) {
                continue;
            }
            match p.action {
                Action::Switch { member } => {
                    self.pursue(p.at, &mut queue[i + 1..], &mut ev);
                    if self.check_end(&mut ev) {
                        return Ok(ev);
                    }
                    if self.alive(p.at) && self.member(p.at) == Some(p.member) {
                        self.fm(p.at).volatile.turn.acted = true;
                        self.switch(p.at, member, &mut ev);
                    }
                }
                Action::Move { index, target } => {
                    self.take_turn(p.at, Some(index), target, false, &mut ev)
                }
                Action::OutOfPp => self.take_turn(p.at, None, None, false, &mut ev),
            }
            if self.check_end(&mut ev) {
                return Ok(ev);
            }
        }

        self.end_of_turn(&mut ev);
        if self.check_end(&mut ev) {
            return Ok(ev);
        }
        if self.turn >= self.rules.turn_limit {
            self.finish(Outcome::Draw, &mut ev);
            return Ok(ev);
        }
        self.plan_replacements();
        Ok(ev)
    }

    /// The move a planned action uses (the locked one for a forced fighter).
    fn planned_move(&self, p: &Planned) -> Option<MoveData> {
        let f = self.fighter(p.at)?;
        if let Some(lock) = f.volatile.lock {
            return Some(lock.data);
        }
        if f.volatile.recharge {
            return None;
        }
        match p.action {
            Action::Move { index, .. } => f.moves.get(index).map(|m| m.data),
            Action::OutOfPp => Some(self.rules.out_of_pp),
            Action::Switch { .. } => None,
        }
    }

    /// Foes still to act whose move hits a switching target first strike `at` now.
    fn pursue(&mut self, at: Slot, rest: &mut [Planned], ev: &mut Vec<Event>) {
        for p in rest.iter_mut() {
            if p.done || p.at.side == at.side || !self.alive(p.at) || self.forced(p.at) {
                continue;
            }
            let Action::Move { index, target } = p.action else {
                continue;
            };
            let aimed = target.is_none_or(|t| t == at);
            let hits_switching = self
                .planned_move(p)
                .is_some_and(|m| m.effect == effects::HITS_SWITCHING);
            if aimed && hits_switching && self.member(p.at) == Some(p.member) {
                p.done = true;
                self.fm(p.at).volatile.turn.acted = true;
                self.pursuing(p.at, index, at, ev);
                if !self.alive(at) {
                    return;
                }
            }
        }
    }

    /// Fills the pending slots: one `(slot, member)` for each.
    pub fn replace(&mut self, picks: &[(Slot, usize)]) -> Result<Vec<Event>, BattleError> {
        if self.phase != Phase::Replacements {
            return Err(BattleError::WrongPhase);
        }
        for &slot in &self.pending {
            if !picks.iter().any(|p| p.0 == slot) {
                return Err(BattleError::Missing(slot));
            }
        }
        for (i, &(slot, member)) in picks.iter().enumerate() {
            let dup = picks[..i]
                .iter()
                .any(|p| p.0 == slot || (p.0.side == slot.side && p.1 == member));
            if !self.pending.contains(&slot) || dup || !self.bench(slot.side).contains(&member) {
                return Err(BattleError::Invalid(slot));
            }
        }
        let mut ev = Vec::new();
        for &(slot, member) in picks {
            self.leave(slot, false);
            self.place(slot, member, &mut ev);
            self.teams[slot.side][member].volatile.fresh -= 1;
        }
        self.pending.clear();
        self.phase = Phase::Actions;
        for &(slot, _) in picks {
            self.hazards(slot, &mut ev);
        }
        self.entry_abilities(picks.iter().map(|p| p.0).collect(), &mut ev);
        if !self.check_end(&mut ev) {
            self.plan_replacements();
        }
        Ok(ev)
    }

    fn check_choices(&self, choices: &[Choice]) -> Result<(), BattleError> {
        for at in self.slots() {
            let n = choices.iter().filter(|c| c.at == at).count();
            if self.alive(at) && n > 1 {
                return Err(BattleError::Invalid(at));
            }
            if self.alive(at) && n == 0 && !self.forced(at) {
                return Err(BattleError::Missing(at));
            }
        }
        for (i, c) in choices.iter().enumerate() {
            let bad = BattleError::Invalid(c.at);
            let f = self.fighter(c.at).filter(|f| !f.fainted()).ok_or(bad)?;
            if self.forced(c.at) {
                continue;
            }
            let repeating = f.volatile.repeat.is_some();
            let ok = match c.action {
                Action::Move { index, target } => {
                    (repeating || self.usable(c.at, index))
                        && target
                            .is_none_or(|t| t != c.at && t.side < 2 && t.pos < self.format.slots())
                }
                Action::OutOfPp => self.usable_moves(c.at).is_empty(),
                Action::Switch { member } => {
                    self.can_switch(c.at)
                        && self.bench(c.at.side).contains(&member)
                        && !choices[..i].iter().any(|o| {
                            o.at.side == c.at.side && o.action == Action::Switch { member }
                        })
                }
            };
            if !ok {
                return Err(bad);
            }
        }
        Ok(())
    }

    /// After a turn: fainted slots with someone to send in wait for `replace`; the others are
    /// left empty.
    fn plan_replacements(&mut self) {
        self.pending.clear();
        for side in 0..2 {
            let mut bench = self.bench(side).len();
            for pos in 0..self.format.slots() {
                let at = Slot::new(side, pos);
                if self.fighter(at).is_some_and(Fighter::fainted) {
                    if bench > 0 {
                        bench -= 1;
                        self.pending.push(at);
                    } else {
                        self.active[side][pos] = None;
                    }
                }
            }
        }
        self.phase = if self.pending.is_empty() {
            Phase::Actions
        } else {
            Phase::Replacements
        };
    }

    fn check_end(&mut self, ev: &mut Vec<Event>) -> bool {
        if let Phase::Over(_) = self.phase {
            return true;
        }
        let out = |side: usize| self.teams[side].iter().all(Fighter::fainted);
        let outcome = match (out(0), out(1)) {
            (true, true) => Outcome::Draw,
            (true, false) => Outcome::Won(1),
            (false, true) => Outcome::Won(0),
            _ => return false,
        };
        self.finish(outcome, ev);
        true
    }

    fn finish(&mut self, outcome: Outcome, ev: &mut Vec<Event>) {
        self.phase = Phase::Over(outcome);
        self.pending.clear();
        ev.push(Event::BattleEnded { outcome });
    }

    // ---- switching ----

    fn place(&mut self, at: Slot, member: usize, ev: &mut Vec<Event>) {
        self.active[at.side][at.pos] = Some(member);
        self.down[at.side][at.pos] = false;
        let f = &mut self.teams[at.side][member];
        f.withdraw();
        f.volatile.fresh = 2;
        // A loafer sent in after the start loafs next unless a turn's end (which flips it) comes
        // first; `start` clears it for the leads.
        f.volatile.loafing = f.ability == abilities::LOAFS;
        ev.push(Event::SwitchedIn { at, member });
    }

    /// The fighter at `at` leaves the field: its own form comes back, its cure-on-switch
    /// ability works (`switching`: it's still able to battle) and the foes it held go free.
    fn leave(&mut self, at: Slot, switching: bool) {
        let Some(m) = self.member(at) else {
            return;
        };
        let f = &mut self.teams[at.side][m];
        f.withdraw();
        if switching && f.ability == abilities::CURED_ON_SWITCH {
            f.ailment = None;
        }
        self.release_holds(at);
    }

    /// Foes bound or held by the fighter at `at` go free.
    fn release_holds(&mut self, at: Slot) {
        for s in self.slots() {
            if let Some(m) = self.member(s) {
                let v = &mut self.teams[s.side][m].volatile;
                if v.held_by == Some(at) {
                    v.held_by = None;
                }
                if v.bound.is_some_and(|(_, by, _)| by == at) {
                    v.bound = None;
                }
            }
        }
    }

    fn switch(&mut self, at: Slot, member: usize, ev: &mut Vec<Event>) {
        if let Some(old) = self.member(at) {
            ev.push(Event::Withdrawn { at, member: old });
        }
        self.leave(at, true);
        self.place(at, member, ev);
        self.arrive(at, ev);
    }

    /// After coming in mid-turn: hazards, then its entry ability.
    fn arrive(&mut self, at: Slot, ev: &mut Vec<Event>) {
        self.hazards(at, ev);
        if self.alive(at) {
            self.entry_abilities(vec![at], ev);
        }
        self.report_faints(ev);
    }

    /// Switches with the state a passing move hands on.
    fn pass_to(&mut self, at: Slot, member: usize, ev: &mut Vec<Event>) {
        let Some(old) = self.member(at) else {
            return;
        };
        let v = self.teams[at.side][old].volatile.clone();
        ev.push(Event::Passing { at });
        ev.push(Event::Withdrawn { at, member: old });
        let f = &mut self.teams[at.side][old];
        f.withdraw();
        if f.ability == abilities::CURED_ON_SWITCH {
            f.ailment = None;
        }
        self.place(at, member, ev);
        let loafing = self.teams[at.side][member].volatile.loafing;
        let n = &mut self.teams[at.side][member].volatile;
        *n = Volatile {
            stages: v.stages,
            confusion: v.confusion,
            crit_stages: v.crit_stages,
            decoy: v.decoy,
            seeded_by: v.seeded_by,
            held_by: v.held_by,
            countdown: v.countdown,
            sure_hit: v.sure_hit,
            hexed: v.hexed,
            rooted: v.rooted,
            fresh: 2,
            loafing,
            ..Volatile::default()
        };
        self.arrive(at, ev);
    }

    /// Hazards on the side of a fighter that just came in.
    fn hazards(&mut self, at: Slot, ev: &mut Vec<Event>) {
        let layers = self.field.sides[at.side].hazards;
        if layers == 0 || !self.alive(at) {
            return;
        }
        let f = self.fm(at);
        if f.has_type(types::FLYING) || f.ability == abilities::IMMUNE_TO_GROUND {
            return;
        }
        let div = (5 - u32::from(layers.min(3))) * 2;
        let damage = f.hurt((u32::from(f.stats.hp) / div).max(1));
        ev.push(Event::Hurt {
            target: at,
            damage,
            hp: f.hp,
            by: HurtBy::Hazard,
        });
        self.report_faints(ev);
    }

    /// Entry abilities of the fighters that just came in, fastest first.
    fn entry_abilities(&mut self, mut slots: Vec<Slot>, ev: &mut Vec<Event>) {
        slots.sort_by_key(|&s| std::cmp::Reverse(self.speed(s)));
        for at in slots {
            if !self.alive(at) {
                continue;
            }
            let ability = self.fm(at).ability;
            match abilities::on_entry(ability) {
                Some(Entry::Weather(w)) => {
                    if self.field.weather != w || self.field.weather_turns != 0 {
                        ev.push(Event::Ability { at, ability });
                        self.field.weather = w;
                        self.field.weather_turns = 0;
                        ev.push(Event::WeatherStarted { weather: w });
                        self.weather_types(ev);
                    }
                }
                Some(Entry::FoesStat(stat, by)) => {
                    ev.push(Event::Ability { at, ability });
                    for foe in self.foes(at) {
                        if self.fm(foe).volatile.decoy > 0 {
                            continue;
                        }
                        self.lower_stat(at, foe, stat, by, true, ev);
                    }
                }
                Some(Entry::CopyFoe) => {
                    let foes: Vec<Slot> = self
                        .foes(at)
                        .into_iter()
                        .filter(|&s| self.fm(s).ability != abilities::NONE)
                        .collect();
                    if !foes.is_empty() {
                        let from = foes[self.rng.below(foes.len() as u32) as usize];
                        let copied = self.fm(from).ability;
                        ev.push(Event::Ability { at, ability });
                        self.set_ability(at, copied, ev);
                    }
                }
                Some(Entry::WeatherType) | None => {}
            }
        }
        // Weather may have started or stopped working.
        self.weather_types(ev);
    }

    /// Fighters whose ability makes their type follow the weather take the weather's type.
    fn weather_types(&mut self, ev: &mut Vec<Event>) {
        let ty = abilities::weather_type(self.weather());
        for at in self.on_field() {
            let f = self.fm(at);
            if f.ability == abilities::WEATHER_TYPE && f.types != [ty, ty] {
                f.keep_original(false);
                f.types = [ty, ty];
                let ability = f.ability;
                ev.push(Event::Ability { at, ability });
                ev.push(Event::TypeChanged {
                    target: at,
                    types: [ty, ty],
                });
            }
        }
    }

    /// Gives the fighter at `at` another ability; one that rules out its current status or
    /// confusion cures it.
    fn set_ability(&mut self, at: Slot, ability: u8, ev: &mut Vec<Event>) {
        let f = self.fm(at);
        f.keep_original(false);
        f.ability = ability;
        ev.push(Event::AbilityChanged {
            target: at,
            ability,
        });
        if let Some(a) = f.ailment
            && abilities::cures(ability, Some(a))
        {
            f.ailment = None;
            ev.push(Event::Cured {
                target: at,
                ailment: a,
            });
        }
        if f.volatile.confusion > 0 && abilities::cures(ability, None) {
            f.volatile.confusion = 0;
            ev.push(Event::ConfusionEnded { target: at });
        }
    }

    /// Forces the fighter at `at` out for a random bench member; whether it went.
    fn force_out(&mut self, at: Slot, ev: &mut Vec<Event>) -> bool {
        let bench = self.bench(at.side);
        if bench.is_empty() {
            return false;
        }
        let member = bench[self.rng.below(bench.len() as u32) as usize];
        if let Some(old) = self.member(at) {
            ev.push(Event::ForcedOut { at, member: old });
        }
        self.leave(at, true);
        self.place(at, member, ev);
        self.arrive(at, ev);
        true
    }

    /// Reports fighters that hit 0 HP since the last report, in field order.
    fn report_faints(&mut self, ev: &mut Vec<Event>) {
        self.report_faints_except(None, ev);
    }

    /// The same, leaving `later` (a move's user, whose faint comes after its targets') for the
    /// next report.
    fn report_faints_except(&mut self, later: Option<Slot>, ev: &mut Vec<Event>) {
        for at in self.slots() {
            if Some(at) == later {
                continue;
            }
            if self.fighter(at).is_some_and(Fighter::fainted) && !self.down[at.side][at.pos] {
                self.down[at.side][at.pos] = true;
                let member = self.member(at).unwrap_or(0);
                ev.push(Event::Fainted { at, member });
                // Its locks and holds end with it.
                let v = &mut self.teams[at.side][member].volatile;
                v.lock = None;
                v.hidden = None;
                self.release_holds(at);
            }
        }
    }

    fn fm(&mut self, at: Slot) -> &mut Fighter {
        let m = self.member(at).expect("a fighter in this slot");
        &mut self.teams[at.side][m]
    }

    /// Whether this is the last fighter still to act this turn.
    fn moving_last(&self, at: Slot) -> bool {
        self.on_field()
            .into_iter()
            .filter(|&s| s != at)
            .all(|s| self.fighter(s).is_some_and(|f| f.volatile.turn.acted))
    }

    /// Ends a lock or charge (it couldn't act).
    fn cancel_lock(&mut self, at: Slot) {
        let v = &mut self.fm(at).volatile;
        v.lock = None;
        v.hidden = None;
        v.growth = 0;
    }
}

#[cfg(test)]
impl Battle {
    /// Test access to any team member.
    pub(crate) fn member_mut(&mut self, side: usize, member: usize) -> &mut Fighter {
        &mut self.teams[side][member]
    }

    pub(crate) fn field_mut(&mut self) -> &mut Field {
        &mut self.field
    }
}
