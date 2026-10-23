//! What happened in a turn, in order: the battle screen turns these into messages (names from
//! the ROM and the fighters), motions and HP bar changes.

use super::data::Effectiveness;
use super::field::{Screen, Weather};
use super::fighter::{Ailment, Stat};

/// A place on the field: side 0 is the player's, side 1 the opponent's; `pos` 0 (and 1 in
/// doubles).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Slot {
    pub side: usize,
    pub pos: usize,
}

impl Slot {
    pub const fn new(side: usize, pos: usize) -> Slot {
        Slot { side, pos }
    }
}

/// How a battle ended.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    /// This side won.
    Won(usize),
    Draw,
}

/// Why a fighter couldn't act.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Blocked {
    Asleep,
    Frozen,
    FullyParalyzed,
    Flinched,
    /// Hurt itself in confusion (a `Hurt` follows).
    Confusion,
    /// Resting after a move that needs it.
    Recharging,
    /// Its ability makes it loaf around this turn.
    Loafing,
    /// The move it picked is sealed.
    Sealed,
    /// Provoked: no status moves.
    Provoked,
    /// A foe sealed the move it picked.
    SealedByFoe,
    /// Hit before its focused move went off.
    LostFocus,
    /// Storing energy for a waiting move.
    Storing,
}

/// HP lost outside a move's hit.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HurtBy {
    Recoil,
    Confusion,
    Poison,
    Burn,
    Weather(Weather),
    /// Its own move (a self-fainting move).
    Sacrifice,
    /// HP it paid for its own move (a decoy, a half-HP cost).
    Cost,
    /// A seed draining it.
    Seed,
    BadDreams,
    Hex,
    /// Bound by a move (the move id).
    Bound(u16),
    /// Hazards on its side as it came in.
    Hazard,
    /// Missed a move that hurts its user on a miss.
    Crash,
    /// A foe's ability it touched (the ability id).
    Ability(u8),
    /// A draining move hit an ability that turns draining into damage.
    DrainBackfire,
    /// Its countdown ran out.
    Countdown,
    /// Taken down with the fighter it knocked out.
    Vengeance,
    /// A delayed attack (the move id) landed.
    Delayed(u16),
}

/// A lasting condition on one fighter that starts (`Event::Started`) and may end
/// (`Event::Ended`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Condition {
    /// Getting ready for the second turn of a two-turn move (the move id).
    Readying(u16),
    Decoy,
    /// Shielded from moves this turn.
    Shielded,
    /// It will hang on with 1 HP this turn.
    Bracing,
    /// Locked into a move that makes noise (the move id).
    Locked(u16),
    /// Storing energy to hit back.
    Storing,
    Seeded,
    /// Bound by a move (the move id).
    Bound(u16),
    /// It can't switch out.
    CantEscape,
    /// A move id it can't use.
    Sealed(u16),
    /// It must repeat its last move.
    Repeating,
    /// It can only use damaging moves.
    Provoked,
    /// It can't use a move twice in a row.
    NoRepeats,
    Identified,
    BadDreams,
    Hexed,
    /// Its countdown, from 3 down.
    Countdown(u8),
    Vengeful,
    /// The move that knocks it out loses all PP.
    Rancor,
    /// The fighter at this slot won't miss it.
    SureHit(Slot),
    /// Stores held (0 when emptied).
    Stored(u8),
    /// Its next electric move is stronger.
    Powered,
    Rooted,
    Drowsy,
    /// Its ability is boosting its fire moves.
    FireBoost,
    /// Electric / fire moves are dulled while it stays.
    ElectricDulled,
    FireDulled,
    /// Foes can't use moves it knows.
    Sealing,
    /// Bounces status moves this turn.
    Bouncing,
    /// Waiting to take a foe's self-help move.
    Stealing,
    /// Drawing foes' moves to itself this turn.
    Drawing,
    /// Its move gets an ally's boost.
    Helped,
    /// Focusing before a move that fails if it's hit first.
    Focusing,
    /// A heal waits for this slot.
    HealPending,
    /// A delayed attack (the move id) is aimed at this slot.
    Delayed(u16),
    /// Its attack rises when hit.
    Fuming,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Event {
    TurnStarted {
        turn: u32,
    },
    /// `member` (team index) came out at `at`.
    SwitchedIn {
        at: Slot,
        member: usize,
    },
    /// `member` went back.
    Withdrawn {
        at: Slot,
        member: usize,
    },
    /// The move id (the out-of-PP move's id from `Rules` when it has no PP left).
    UsedMove {
        by: Slot,
        move_id: u16,
    },
    /// It couldn't act this turn.
    CantAct {
        by: Slot,
        why: Blocked,
    },
    /// No fighter left to aim at.
    NoTarget {
        by: Slot,
    },
    Missed {
        by: Slot,
        target: Slot,
    },
    /// The move or its effect doesn't affect this target.
    NoEffect {
        target: Slot,
    },
    /// "But it failed" (nothing to do, or blocked).
    Failed {
        by: Slot,
    },
    /// A move's hit: HP lost and left.
    Hit {
        by: Slot,
        target: Slot,
        damage: u16,
        hp: u16,
        effect: Effectiveness,
        crit: bool,
    },
    /// A multi-hit move landed this many times.
    HitTimes {
        count: u8,
    },
    Hurt {
        target: Slot,
        damage: u16,
        hp: u16,
        by: HurtBy,
    },
    Healed {
        target: Slot,
        amount: u16,
        hp: u16,
    },
    Ailment {
        target: Slot,
        ailment: Ailment,
    },
    /// It already has this (or another) status.
    AlreadyAilment {
        target: Slot,
        ailment: Ailment,
    },
    WokeUp {
        target: Slot,
    },
    Thawed {
        target: Slot,
    },
    /// A status went (the heal-and-sleep move cures the old one first).
    Cured {
        target: Slot,
        ailment: Ailment,
    },
    Confused {
        target: Slot,
    },
    AlreadyConfused {
        target: Slot,
    },
    /// About to act while confused.
    IsConfused {
        by: Slot,
    },
    ConfusionEnded {
        target: Slot,
    },
    /// `by` stages actually moved (non-zero).
    StatChanged {
        target: Slot,
        stat: Stat,
        by: i8,
    },
    /// It couldn't go further this way.
    StatAtLimit {
        target: Slot,
        stat: Stat,
        up: bool,
    },
    /// A side's stat guard kept a stat from dropping.
    StatProtected {
        target: Slot,
    },
    StagesReset,
    /// It copied the stages of the fighter at `from`.
    StagesCopied {
        by: Slot,
        from: Slot,
    },
    Focused {
        target: Slot,
    },
    Fainted {
        at: Slot,
        member: usize,
    },
    WeatherStarted {
        weather: Weather,
    },
    WeatherContinues {
        weather: Weather,
    },
    WeatherEnded {
        weather: Weather,
    },
    ScreenUp {
        side: usize,
        screen: Screen,
    },
    ScreenEnded {
        side: usize,
        screen: Screen,
    },
    /// An ability stepped in (shown with its ROM name).
    Ability {
        at: Slot,
        ability: u8,
    },
    /// "Nothing happened."
    Nothing,
    Started {
        target: Slot,
        condition: Condition,
    },
    Ended {
        target: Slot,
        condition: Condition,
    },
    /// A shielding move stopped a move.
    Shielded {
        target: Slot,
    },
    /// It hung on with 1 HP.
    HungOn {
        target: Slot,
    },
    /// Its decoy took the hit (after a `Hit` with no damage).
    DecoyTook {
        target: Slot,
        damage: u16,
    },
    DecoyFaded {
        target: Slot,
    },
    /// A one-hit knock-out.
    KnockedOut {
        target: Slot,
    },
    TypeChanged {
        target: Slot,
        types: [u8; 2],
    },
    AbilityChanged {
        target: Slot,
        ability: u8,
    },
    /// It took the form of the fighter at `into`.
    TookForm {
        by: Slot,
        into: Slot,
    },
    /// It copied a move into its moveset.
    MoveCopied {
        by: Slot,
        move_id: u16,
    },
    /// A move lost PP.
    PpCut {
        target: Slot,
        move_id: u16,
        amount: u8,
    },
    /// HP set to a value (shared out between two fighters).
    HpSet {
        target: Slot,
        hp: u16,
    },
    /// Every member of a side was cured of its status.
    TeamCured {
        side: usize,
    },
    ScreensBroken {
        side: usize,
    },
    HazardLaid {
        side: usize,
        layers: u8,
    },
    /// Forced out of the field (a `SwitchedIn` follows).
    ForcedOut {
        at: Slot,
        member: usize,
    },
    /// Passing its state on as it switches (a `Withdrawn` and `SwitchedIn` follow).
    Passing {
        at: Slot,
    },
    /// A status move was bounced back at its user.
    Bounced {
        by: Slot,
    },
    /// A foe took the move for itself.
    Stolen {
        by: Slot,
    },
    /// A move was pulled to this slot.
    Drawn {
        to: Slot,
    },
    /// Freed from binding, seeds and hazards.
    Freed {
        at: Slot,
    },
    /// A sleeper woke up because of the noise.
    NoiseWoke {
        target: Slot,
    },
    /// The move it called with another move (shown before the called move's `UsedMove`).
    Called {
        by: Slot,
        move_id: u16,
    },
    /// A hit from a team member who isn't on the field (the member index).
    TeamHit {
        member: usize,
    },
    BattleEnded {
        outcome: Outcome,
    },
}
