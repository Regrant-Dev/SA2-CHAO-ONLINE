//! Move effect ids (the record's effect byte) and what the engine does for each.
//!
//! The ROM has 214 effect ids, 0..=213; ids 96, 110, 131, 141 and 163 belong to no move. Every
//! other id is handled. A few work differently because fighters here have no held items,
//! gender or weight, and battles take place in an indoor arena:
//! - the item moves (`HIT_TAKE_ITEM`, `HIT_DROP_ITEM`) are plain hits, and `SWAP_ITEMS` and
//!   `RESTORE_ITEM` fail;
//! - `INFATUATE` always fails (no fighter has a gender);
//! - `WEIGHT_POWER` hits with the middle power of its weight scale;
//! - `ARENA_MOVE`, `ARENA_CHANCE` and `ARENA_TYPE` use the indoor arena's move, paralysis chance
//!   and normal type;
//! - `FLEE` fails (a battle can't be run from);
//! - `HIT_MONEY` is a plain hit.
//!
//! Ids the engine doesn't know act as plain damage on a damaging move and do nothing on a
//! status move.

use super::data::{MoveData, types};
use super::field::{Screen, Weather};
use super::fighter::{Ailment, Fighter, Hidden, Stat};
use super::rng::Rng;

/// Effect ids 0..COUNT.
pub const COUNT: u8 = 214;
/// Ids no move uses.
pub const UNUSED: [u8; 5] = [96, 110, 131, 141, 163];

pub const HIT: u8 = 0;
pub const SLEEP: u8 = 1;
pub const POISON_CHANCE: u8 = 2;
pub const DRAIN_HALF: u8 = 3;
pub const BURN_CHANCE: u8 = 4;
pub const FREEZE_CHANCE: u8 = 5;
pub const PARALYZE_CHANCE: u8 = 6;
/// The user faints; the target's defense counts half.
pub const SELF_FAINT: u8 = 7;
/// Drains half the damage; only works on a sleeping target.
pub const DRAIN_SLEEPER: u8 = 8;
/// Uses the last move aimed at the user.
pub const COPY_FOES_MOVE: u8 = 9;
/// One stage up for the user: attack, defense, speed, sp. attack, sp. defense, accuracy,
/// evasion (in `Stat` order).
pub const USER_UP_FIRST: u8 = 10;
pub const USER_UP_LAST: u8 = 16;
pub const ALWAYS_HITS: u8 = 17;
/// One stage down for the target, the same order.
pub const TARGET_DOWN_FIRST: u8 = 18;
pub const TARGET_DOWN_LAST: u8 = 24;
/// Everyone's stages back to 0.
pub const RESET_STAGES: u8 = 25;
/// Stores the damage taken for two turns, then deals it back double.
pub const STORE_DAMAGE: u8 = 26;
/// Locked in for 2 or 3 turns, then confused.
pub const RAMPAGE: u8 = 27;
/// Forces the target out for a random team member.
pub const FORCE_SWITCH: u8 = 28;
/// 2 to 5 hits.
pub const MULTI_HIT: u8 = 29;
/// The user's type becomes the type of one of its moves.
pub const TYPE_FROM_OWN_MOVE: u8 = 30;
pub const FLINCH_CHANCE: u8 = 31;
pub const HEAL_HALF: u8 = 32;
pub const BAD_POISON: u8 = 33;
/// A plain hit (it scatters money elsewhere).
pub const HIT_MONEY: u8 = 34;
pub const SPECIAL_WALL: u8 = 35;
/// Burn, freeze or paralysis, one at random.
pub const TRIPLE_STATUS_CHANCE: u8 = 36;
/// Full heal and two turns of sleep.
pub const SLEEP_HEAL: u8 = 37;
/// Knocks out in one hit; fails on a higher-level target.
pub const ONE_HIT_KO: u8 = 38;
/// Charges one turn, hits the next.
pub const TWO_TURN: u8 = 39;
pub const HALVE_HP: u8 = 40;
pub const FIXED_40: u8 = 41;
/// Binds the target for 2 to 5 turns: 1/16 a turn and no switching.
pub const TRAP_HIT: u8 = 42;
pub const HIGH_CRIT: u8 = 43;
pub const TWO_HITS: u8 = 44;
/// The user takes half the damage it would have dealt when it misses.
pub const CRASH_ON_MISS: u8 = 45;
pub const STAT_GUARD: u8 = 46;
/// Two extra critical-hit stages while it stays in.
pub const FOCUS: u8 = 47;
/// The user takes 1/4 of the damage dealt.
pub const RECOIL_QUARTER: u8 = 48;
pub const CONFUSE: u8 = 49;
pub const USER_UP2_FIRST: u8 = 50;
pub const USER_UP2_LAST: u8 = 56;
/// The user takes the target's types, stats, stages, ability and moves.
pub const TAKE_FORM: u8 = 57;
pub const TARGET_DOWN2_FIRST: u8 = 58;
pub const TARGET_DOWN2_LAST: u8 = 64;
pub const PHYSICAL_WALL: u8 = 65;
pub const POISON: u8 = 66;
pub const PARALYZE: u8 = 67;
/// A chance of one stage down for the target, in `Stat` order.
pub const TARGET_DOWN_CHANCE_FIRST: u8 = 68;
pub const TARGET_DOWN_CHANCE_LAST: u8 = 74;
/// Two-turn, with a flinch chance and a high critical-hit ratio.
pub const TWO_TURN_FLINCH_HIGH_CRIT: u8 = 75;
pub const CONFUSE_CHANCE: u8 = 76;
pub const TWO_HITS_POISON_CHANCE: u8 = 77;
pub const ALWAYS_HITS_B: u8 = 78;
/// The user pays 1/4 of its max HP for a decoy that takes hits and blocks status moves.
pub const DECOY: u8 = 79;
/// The user must rest the next turn after it hits.
pub const RECHARGE_AFTER: u8 = 80;
/// The user's attack rises each time it's hit until it uses another move.
pub const ANGER_HIT: u8 = 81;
/// Copies the target's last move until the user leaves.
pub const COPY_MOVE_TEMP: u8 = 82;
/// Uses a random move.
pub const RANDOM_MOVE: u8 = 83;
/// Drains 1/8 of the target's max HP each turn.
pub const SEED_DRAIN: u8 = 84;
pub const NOTHING: u8 = 85;
/// The target can't use its last move for 2 to 5 turns.
pub const SEAL_LAST_MOVE: u8 = 86;
/// Damage equal to the user's level.
pub const LEVEL_DAMAGE: u8 = 87;
/// 0.5x to 1.5x the user's level.
pub const LEVEL_RANDOM_DAMAGE: u8 = 88;
/// Twice the physical damage the user took this turn, back at whoever dealt it.
pub const RETURN_PHYSICAL: u8 = 89;
/// The target repeats its last move for 3 to 6 turns.
pub const REPEAT_LAST: u8 = 90;
/// Shares the user's and target's HP out evenly.
pub const SHARE_HP: u8 = 91;
/// Usable only while asleep; a flinch chance.
pub const SLEEP_FLINCH_HIT: u8 = 92;
/// The user's type becomes one that resists the last move that hit it.
pub const TYPE_RESIST_LAST: u8 = 93;
/// The user's next move won't miss the target.
pub const SURE_HIT_NEXT: u8 = 94;
/// Copies the target's last move for good.
pub const COPY_MOVE_KEEP: u8 = 95;
/// Usable only while asleep: one of the user's other moves at random.
pub const SLEEP_RANDOM_MOVE: u8 = 97;
/// If the user faints to a foe's move before its next move, the foe faints too.
pub const VENGEANCE: u8 = 98;
/// Stronger the lower the user's HP.
pub const LOW_HP_POWER: u8 = 99;
/// The target's last move loses 2 to 5 PP.
pub const CUT_PP: u8 = 100;
/// Leaves the target at least 1 HP.
pub const LEAVE_ONE_HP: u8 = 101;
/// Cures the user's whole team of status.
pub const CURE_TEAM: u8 = 102;
pub const PRIORITY_HIT: u8 = 103;
/// Up to three hits of rising power, each rolled to hit.
pub const TRIPLE_RISING: u8 = 104;
/// A plain hit (it takes the target's item elsewhere).
pub const HIT_TAKE_ITEM: u8 = 105;
/// The target can't switch out while the user stays.
pub const BLOCK_ESCAPE: u8 = 106;
/// A sleeping target loses 1/4 of its max HP each turn.
pub const BAD_DREAMS: u8 = 107;
/// Evasion up a stage; stomping moves hit it double.
pub const SHRINK: u8 = 108;
/// Ghost user: half its HP to make the target lose 1/4 a turn. Others: speed down, attack and
/// defense up.
pub const HEX_OR_BOOST: u8 = 109;
/// Blocks moves aimed at the user this turn; less likely to work each time in a row.
pub const SHIELD: u8 = 111;
/// A layer of hazards on the foes' side (up to 3) that hurts whoever comes in.
pub const HAZARD: u8 = 112;
/// The target loses its evasion edge and type immunities to normal and fighting moves.
pub const IDENTIFY: u8 = 113;
/// Everyone on the field faints in three turns unless they switch out.
pub const COUNTDOWN: u8 = 114;
pub const SAND: u8 = 115;
/// The user hangs on with 1 HP this turn; shares the shield's odds.
pub const BRACE: u8 = 116;
/// Locked in for up to 5 hits, doubling each hit.
pub const ROLLING: u8 = 117;
/// The target's attack up two stages, and it's confused.
pub const CONFUSE_ATTACK_UP2: u8 = 118;
/// Doubles each hit in a row (up to 16x).
pub const GROWING_HIT: u8 = 119;
/// Needs genders: always fails here.
pub const INFATUATE: u8 = 120;
/// Power from the user's bond (friendship).
pub const BOND_POWER: u8 = 121;
/// 40, 80 or 120 power, or heals the target 1/4.
pub const RANDOM_GIFT: u8 = 122;
/// Power from the lack of a bond.
pub const BOND_POWER_INVERSE: u8 = 123;
pub const STATUS_GUARD: u8 = 124;
/// A burn chance; usable while frozen, which thaws the user.
pub const THAW_BURN_CHANCE: u8 = 125;
/// A random power from 10 to 150; double on an underground target.
pub const RANDOM_POWER: u8 = 126;
/// Switches out, passing stages and most conditions to the one coming in.
pub const PASS_STATE: u8 = 127;
/// Double power on a target switching out this turn, hitting before it goes.
pub const HITS_SWITCHING: u8 = 128;
/// Frees the user from binding, seeds and hazards.
pub const CLEAR_HAZARDS: u8 = 129;
pub const FIXED_20: u8 = 130;
/// Heals by the weather: 1/2, 2/3 in sun, 1/4 in other weather.
pub const WEATHER_HEAL_A: u8 = 132;
pub const WEATHER_HEAL_B: u8 = 133;
pub const WEATHER_HEAL_C: u8 = 134;
/// Type and power from the user's stats.
pub const STAT_BITS_POWER: u8 = 135;
pub const RAIN: u8 = 136;
pub const SUN: u8 = 137;
pub const USER_DEFENSE_UP_CHANCE: u8 = 138;
pub const USER_ATTACK_UP_CHANCE: u8 = 139;
pub const USER_ALL_UP_CHANCE: u8 = 140;
/// Half the user's HP for attack +6.
pub const MAX_ATTACK_HALF_HP: u8 = 142;
/// Copies the target's stages.
pub const COPY_STAGES: u8 = 143;
/// Twice the special damage the user took this turn, back at whoever dealt it.
pub const RETURN_SPECIAL: u8 = 144;
/// Two-turn; defense up on the first.
pub const TWO_TURN_DEFENSE_UP: u8 = 145;
/// A flinch chance; reaches a target in the air, at double damage.
pub const FLINCH_CHANCE_B: u8 = 146;
/// Reaches an underground target, at double damage.
pub const HITS_UNDERGROUND: u8 = 147;
/// Hits the target's slot two turns later.
pub const DELAYED_HIT: u8 = 148;
/// Reaches a target in the air, at double damage.
pub const HITS_AIR: u8 = 149;
/// A flinch chance; double damage on a shrunk target.
pub const FLINCH_CHANCE_C: u8 = 150;
/// Two-turn, but no charging in sun; half damage in other weather.
pub const SUN_CHARGE: u8 = 151;
/// A paralysis chance; never misses in rain, 50% accurate in sun, reaches the air.
pub const STORM_PARALYZE_CHANCE: u8 = 152;
/// Flees the battle: fails here.
pub const FLEE: u8 = 153;
/// One hit for each team member able to battle without a status.
pub const TEAM_HITS: u8 = 154;
/// Two-turn, out of reach in between (the move's type picks air, underground or underwater);
/// a paralysis chance when the move has one.
pub const HIDE_TWO_TURN: u8 = 155;
/// Defense up; rolling moves hit double.
pub const CURL: u8 = 156;
pub const HEAL_HALF_B: u8 = 157;
/// Works only on the user's first turn out; always flinches.
pub const FIRST_TURN_FLINCH: u8 = 158;
/// Locked in for 2 to 5 turns; nobody can sleep meanwhile.
pub const NOISE: u8 = 159;
/// Stores up to 3.
pub const STORE_UP: u8 = 160;
/// 100 power per store, then empty.
pub const RELEASE_HIT: u8 = 161;
/// Heals 1/4, 1/2 or all for 1, 2 or 3 stores, then empty.
pub const RELEASE_HEAL: u8 = 162;
pub const HAIL: u8 = 164;
/// The target can't use the same move twice in a row.
pub const NO_REPEATS: u8 = 165;
/// The target's sp. attack up a stage, and it's confused.
pub const CONFUSE_SP_ATTACK_UP: u8 = 166;
pub const BURN: u8 = 167;
/// The user faints; the target's attack and sp. attack down two stages.
pub const FAINT_LOWER_TARGET: u8 = 168;
/// Double power while the user is burned, poisoned or paralysed.
pub const STATUS_POWER: u8 = 169;
/// Fails if the user is hurt by a move before it goes.
pub const FOCUSED_HIT: u8 = 170;
/// Double power on a paralysed target, curing it.
pub const PARALYSIS_POWER_CURE: u8 = 171;
/// Foes' single-target moves go to the user this turn.
pub const DRAW_MOVES: u8 = 172;
/// Uses the arena's move (`ARENA_MOVE_ID`).
pub const ARENA_MOVE: u8 = 173;
/// The user's next electric move is doubled.
pub const POWER_UP_ELECTRIC: u8 = 174;
/// The target can only use damaging moves for 2 turns.
pub const PROVOKE: u8 = 175;
/// The ally's move this turn is 1.5x.
pub const HELP_ALLY: u8 = 176;
/// Swaps held items: fails here.
pub const SWAP_ITEMS: u8 = 177;
/// The user takes the target's ability.
pub const COPY_ABILITY: u8 = 178;
/// Heals whoever is in the user's slot by half the user's max HP at the end of next turn.
pub const DELAYED_HEAL: u8 = 179;
/// Uses a random move of a team member.
pub const ALLY_RANDOM_MOVE: u8 = 180;
/// Heals 1/16 a turn; the user can't switch or be forced out.
pub const ROOT: u8 = 181;
/// After hitting, the user's attack and defense go down a stage.
pub const USER_ATTACK_DEFENSE_DOWN_AFTER: u8 = 182;
/// Bounces status moves back this turn.
pub const BOUNCE_STATUS: u8 = 183;
/// Restores a used item: fails here.
pub const RESTORE_ITEM: u8 = 184;
/// Double power if the target hurt the user this turn.
pub const REVENGE_POWER: u8 = 185;
/// Breaks the target side's walls before hitting.
pub const BREAK_SCREENS: u8 = 186;
/// The target falls asleep at the end of the next turn.
pub const DROWSE: u8 = 187;
/// A plain hit (it knocks the target's item away elsewhere).
pub const HIT_DROP_ITEM: u8 = 188;
/// Brings the target's HP down to the user's.
pub const MATCH_HP: u8 = 189;
/// Power from the user's HP share (150 at full).
pub const HP_POWER: u8 = 190;
pub const SWAP_ABILITIES: u8 = 191;
/// Foes can't use moves the user knows.
pub const SEAL_SHARED_MOVES: u8 = 192;
/// Cures the user's burn, poison or paralysis.
pub const CURE_SELF: u8 = 193;
/// If the user faints to a foe's move before its next move, that move loses all its PP.
pub const PP_RANCOR: u8 = 194;
/// Takes the next self-help move a foe uses this turn.
pub const STEAL_MOVE: u8 = 195;
/// Power from the target's weight (`WEIGHT_POWER_DEFAULT` here).
pub const WEIGHT_POWER: u8 = 196;
/// A chance of the arena's added effect (paralysis indoors).
pub const ARENA_CHANCE: u8 = 197;
/// The user takes 1/3 of the damage dealt.
pub const RECOIL_THIRD: u8 = 198;
/// Confuses everyone else on the field.
pub const CONFUSE_ALL: u8 = 199;
pub const BURN_CHANCE_HIGH_CRIT: u8 = 200;
/// Electric moves are halved while the user is on the field.
pub const DULL_ELECTRIC: u8 = 201;
pub const BAD_POISON_CHANCE: u8 = 202;
/// Type and doubled power from the weather.
pub const WEATHER_POWER: u8 = 203;
/// After hitting, the user's sp. attack goes down two stages.
pub const USER_SP_ATTACK_DOWN2_AFTER: u8 = 204;
pub const TARGET_ATTACK_DEFENSE_DOWN: u8 = 205;
pub const USER_DEFENSES_UP: u8 = 206;
/// Reaches a target in the air (no bonus).
pub const HITS_AIR_B: u8 = 207;
pub const USER_ATTACK_DEFENSE_UP: u8 = 208;
pub const POISON_CHANCE_HIGH_CRIT: u8 = 209;
/// Fire moves are halved while the user is on the field.
pub const DULL_FIRE: u8 = 210;
pub const USER_SPECIALS_UP: u8 = 211;
pub const USER_ATTACK_SPEED_UP: u8 = 212;
/// The user's type becomes the arena's (normal indoors).
pub const ARENA_TYPE: u8 = 213;

/// The move id `ARENA_MOVE` calls in an indoor arena.
pub const ARENA_MOVE_ID: u16 = 129;
/// `WEIGHT_POWER`'s power: fighters have no weight, so the middle of its scale.
pub const WEIGHT_POWER_DEFAULT: u8 = 60;

/// Whether the engine does this id's own thing (every id but the unused ones).
pub fn implemented(effect: u8) -> bool {
    effect < COUNT && !UNUSED.contains(&effect)
}

/// The stat a ranged effect id points at.
fn stat_in(effect: u8, first: u8) -> Stat {
    Stat::ALL[usize::from(effect - first).min(Stat::ALL.len() - 1)]
}

/// What a status move (no normal damage) does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Primary {
    Ailment(Ailment),
    Confuse,
    /// Stage changes on the user (`true`) or each target.
    Stats(bool, Changes),
    /// A stage change on the target, then confusion.
    ConfuseAndRaise(Stat, i8),
    Weather(Weather),
    /// A screen on the user's side.
    Screen(Screen),
    /// Heals this fraction of max HP.
    Heal(u32, u32),
    WeatherHeal,
    SleepHeal,
    Focus,
    ResetStages,
    Nothing,
    /// The move can't work here.
    Fails,
    Seed,
    SealLastMove,
    RepeatLast,
    Provoke,
    NoRepeats,
    BlockEscape,
    BadDreams,
    Identify,
    SureHitNext,
    CutPp,
    ShareHp,
    CopyStages,
    CopyAbility,
    SwapAbilities,
    TakeForm,
    /// Copies the target's last move: for good (`true`) or until the user leaves.
    CopyMove(bool),
    Drowse,
    ForceSwitch,
    FaintLowerTarget,
    HexOrBoost,
    TypeFromOwnMove,
    TypeResistLast,
    Decoy,
    Shield,
    Brace,
    Hazard,
    Countdown,
    Vengeance,
    PpRancor,
    CureTeam,
    /// Evasion up and shrunk.
    Shrink,
    /// Defense up and curled.
    Curl,
    MaxAttack,
    PassState,
    StoreUp,
    ReleaseHeal,
    PowerUpElectric,
    HelpAlly,
    DrawMoves,
    DelayedHeal,
    Root,
    BounceStatus,
    StealMove,
    SealShared,
    CureSelf,
    ConfuseAll,
    /// Halves electric (`true`) or fire moves while the user stays.
    Dull(bool),
    ArenaType,
    Infatuate,
}

impl Primary {
    /// Whether it acts on its targets (and so is checked per target for reach, accuracy and
    /// decoys), rather than on the user or the field.
    pub fn on_target(self) -> bool {
        use Primary as P;
        match self {
            P::Stats(user, _) => !user,
            P::Ailment(_)
            | P::Confuse
            | P::ConfuseAndRaise(..)
            | P::Seed
            | P::SealLastMove
            | P::RepeatLast
            | P::Provoke
            | P::NoRepeats
            | P::BlockEscape
            | P::BadDreams
            | P::Identify
            | P::SureHitNext
            | P::CutPp
            | P::ShareHp
            | P::CopyStages
            | P::CopyAbility
            | P::SwapAbilities
            | P::TakeForm
            | P::CopyMove(_)
            | P::Drowse
            | P::ForceSwitch
            | P::FaintLowerTarget
            | P::Infatuate => true,
            _ => false,
        }
    }

    /// Whether a decoy in front of the target stops it.
    pub fn stopped_by_decoy(self) -> bool {
        use Primary as P;
        matches!(
            self,
            P::Ailment(_)
                | P::Confuse
                | P::ConfuseAndRaise(..)
                | P::Stats(false, _)
                | P::Seed
                | P::BadDreams
                | P::Drowse
                | P::FaintLowerTarget
                | P::Infatuate
                | P::ShareHp
        )
    }
}

/// Up to two stage changes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Changes(pub [Option<(Stat, i8)>; 2]);

impl Changes {
    pub fn one(stat: Stat, by: i8) -> Changes {
        Changes([Some((stat, by)), None])
    }

    pub fn two(a: Stat, b: Stat, by: i8) -> Changes {
        Changes([Some((a, by)), Some((b, by))])
    }

    pub fn iter(&self) -> impl Iterator<Item = (Stat, i8)> + '_ {
        self.0.iter().flatten().copied()
    }
}

pub fn primary(effect: u8) -> Option<Primary> {
    use Primary as P;
    Some(match effect {
        SLEEP => P::Ailment(Ailment::Sleep),
        POISON => P::Ailment(Ailment::Poison),
        BAD_POISON => P::Ailment(Ailment::BadPoison),
        PARALYZE => P::Ailment(Ailment::Paralysis),
        BURN => P::Ailment(Ailment::Burn),
        CONFUSE => P::Confuse,
        CONFUSE_ATTACK_UP2 => P::ConfuseAndRaise(Stat::Attack, 2),
        CONFUSE_SP_ATTACK_UP => P::ConfuseAndRaise(Stat::SpAttack, 1),
        USER_UP_FIRST..=USER_UP_LAST => {
            P::Stats(true, Changes::one(stat_in(effect, USER_UP_FIRST), 1))
        }
        USER_UP2_FIRST..=USER_UP2_LAST => {
            P::Stats(true, Changes::one(stat_in(effect, USER_UP2_FIRST), 2))
        }
        TARGET_DOWN_FIRST..=TARGET_DOWN_LAST => {
            P::Stats(false, Changes::one(stat_in(effect, TARGET_DOWN_FIRST), -1))
        }
        TARGET_DOWN2_FIRST..=TARGET_DOWN2_LAST => {
            P::Stats(false, Changes::one(stat_in(effect, TARGET_DOWN2_FIRST), -2))
        }
        TARGET_ATTACK_DEFENSE_DOWN => {
            P::Stats(false, Changes::two(Stat::Attack, Stat::Defense, -1))
        }
        USER_DEFENSES_UP => P::Stats(true, Changes::two(Stat::Defense, Stat::SpDefense, 1)),
        USER_ATTACK_DEFENSE_UP => P::Stats(true, Changes::two(Stat::Attack, Stat::Defense, 1)),
        USER_SPECIALS_UP => P::Stats(true, Changes::two(Stat::SpAttack, Stat::SpDefense, 1)),
        USER_ATTACK_SPEED_UP => P::Stats(true, Changes::two(Stat::Attack, Stat::Speed, 1)),
        RAIN => P::Weather(Weather::Rain),
        SUN => P::Weather(Weather::Sun),
        SAND => P::Weather(Weather::Sand),
        HAIL => P::Weather(Weather::Hail),
        PHYSICAL_WALL => P::Screen(Screen::PhysicalWall),
        SPECIAL_WALL => P::Screen(Screen::SpecialWall),
        STAT_GUARD => P::Screen(Screen::StatGuard),
        STATUS_GUARD => P::Screen(Screen::StatusGuard),
        HEAL_HALF | HEAL_HALF_B => P::Heal(1, 2),
        WEATHER_HEAL_A | WEATHER_HEAL_B | WEATHER_HEAL_C => P::WeatherHeal,
        SLEEP_HEAL => P::SleepHeal,
        FOCUS => P::Focus,
        RESET_STAGES => P::ResetStages,
        NOTHING => P::Nothing,
        SWAP_ITEMS | RESTORE_ITEM | FLEE => P::Fails,
        SEED_DRAIN => P::Seed,
        SEAL_LAST_MOVE => P::SealLastMove,
        REPEAT_LAST => P::RepeatLast,
        PROVOKE => P::Provoke,
        NO_REPEATS => P::NoRepeats,
        BLOCK_ESCAPE => P::BlockEscape,
        BAD_DREAMS => P::BadDreams,
        IDENTIFY => P::Identify,
        SURE_HIT_NEXT => P::SureHitNext,
        CUT_PP => P::CutPp,
        SHARE_HP => P::ShareHp,
        COPY_STAGES => P::CopyStages,
        COPY_ABILITY => P::CopyAbility,
        SWAP_ABILITIES => P::SwapAbilities,
        TAKE_FORM => P::TakeForm,
        COPY_MOVE_TEMP => P::CopyMove(false),
        COPY_MOVE_KEEP => P::CopyMove(true),
        DROWSE => P::Drowse,
        FORCE_SWITCH => P::ForceSwitch,
        FAINT_LOWER_TARGET => P::FaintLowerTarget,
        HEX_OR_BOOST => P::HexOrBoost,
        TYPE_FROM_OWN_MOVE => P::TypeFromOwnMove,
        TYPE_RESIST_LAST => P::TypeResistLast,
        DECOY => P::Decoy,
        SHIELD => P::Shield,
        BRACE => P::Brace,
        HAZARD => P::Hazard,
        COUNTDOWN => P::Countdown,
        VENGEANCE => P::Vengeance,
        PP_RANCOR => P::PpRancor,
        CURE_TEAM => P::CureTeam,
        SHRINK => P::Shrink,
        CURL => P::Curl,
        MAX_ATTACK_HALF_HP => P::MaxAttack,
        PASS_STATE => P::PassState,
        STORE_UP => P::StoreUp,
        RELEASE_HEAL => P::ReleaseHeal,
        POWER_UP_ELECTRIC => P::PowerUpElectric,
        HELP_ALLY => P::HelpAlly,
        DRAW_MOVES => P::DrawMoves,
        DELAYED_HEAL => P::DelayedHeal,
        ROOT => P::Root,
        BOUNCE_STATUS => P::BounceStatus,
        STEAL_MOVE => P::StealMove,
        SEAL_SHARED_MOVES => P::SealShared,
        CURE_SELF => P::CureSelf,
        CONFUSE_ALL => P::ConfuseAll,
        DULL_ELECTRIC => P::Dull(true),
        DULL_FIRE => P::Dull(false),
        ARENA_TYPE => P::ArenaType,
        INFATUATE => P::Infatuate,
        _ => return None,
    })
}

/// Effects that use another move in their place.
pub fn calls_other_move(effect: u8) -> bool {
    matches!(
        effect,
        COPY_FOES_MOVE | RANDOM_MOVE | SLEEP_RANDOM_MOVE | ALLY_RANDOM_MOVE | ARENA_MOVE
    )
}

/// Effects a randomly called move can't be (nor the out-of-PP move).
pub fn never_called(effect: u8) -> bool {
    matches!(
        effect,
        RANDOM_MOVE
            | COPY_MOVE_KEEP
            | COPY_MOVE_TEMP
            | RETURN_PHYSICAL
            | RETURN_SPECIAL
            | SHIELD
            | BRACE
            | VENGEANCE
            | SLEEP_RANDOM_MOVE
            | HIT_TAKE_ITEM
            | DRAW_MOVES
            | STEAL_MOVE
            | HELP_ALLY
            | SWAP_ITEMS
            | FOCUSED_HIT
    )
}

/// Effects the sleeping-random-move effect won't pick from the user's moves.
pub fn not_for_sleep_call(effect: u8) -> bool {
    calls_other_move(effect)
        || charges(effect)
        || matches!(effect, STORE_DAMAGE | FOCUSED_HIT | NOISE)
}

/// Two-turn effects (the first turn charges).
pub fn charges(effect: u8) -> bool {
    matches!(
        effect,
        TWO_TURN | TWO_TURN_FLINCH_HIGH_CRIT | TWO_TURN_DEFENSE_UP | SUN_CHARGE | HIDE_TWO_TURN
    )
}

/// Where a hiding move's user goes between turns, by the move's type.
pub fn hides(mv: &MoveData) -> Option<Hidden> {
    if mv.effect != HIDE_TWO_TURN {
        return None;
    }
    Some(match mv.type_id {
        types::GROUND => Hidden::Underground,
        types::WATER => Hidden::Underwater,
        _ => Hidden::Air,
    })
}

/// Whether a move reaches a target hidden this way, and its damage multiplier there.
pub fn reaches(mv: &MoveData, hidden: Hidden) -> Option<u32> {
    match hidden {
        Hidden::Air => match mv.effect {
            FLINCH_CHANCE_B | HITS_AIR => Some(2),
            STORM_PARALYZE_CHANCE | HITS_AIR_B => Some(1),
            _ => None,
        },
        Hidden::Underground => matches!(mv.effect, HITS_UNDERGROUND | RANDOM_POWER).then_some(2),
        // Water moves that sweep the surface: the spread water hit and the water binding move.
        Hidden::Underwater => (mv.type_id == types::WATER
            && ((mv.effect == HIT && mv.target == super::data::Target::BothFoes)
                || mv.effect == TRAP_HIT))
            .then_some(2),
    }
}

/// A damaging move's added effect, rolled with the move's chance.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Added {
    Ailment(Ailment),
    /// One of burn, freeze and paralysis.
    AnyOfThree,
    Flinch,
    Confuse,
    Target(Stat, i8),
    User(Changes),
    /// One stage up on each of `ALL_FIVE`.
    UserAllUp,
    /// Binds the target.
    Bind,
}

pub fn added(effect: u8) -> Option<Added> {
    use Added as A;
    Some(match effect {
        POISON_CHANCE | TWO_HITS_POISON_CHANCE | POISON_CHANCE_HIGH_CRIT => {
            A::Ailment(Ailment::Poison)
        }
        BURN_CHANCE | THAW_BURN_CHANCE | BURN_CHANCE_HIGH_CRIT => A::Ailment(Ailment::Burn),
        FREEZE_CHANCE => A::Ailment(Ailment::Freeze),
        PARALYZE_CHANCE | STORM_PARALYZE_CHANCE | ARENA_CHANCE | HIDE_TWO_TURN => {
            A::Ailment(Ailment::Paralysis)
        }
        BAD_POISON_CHANCE => A::Ailment(Ailment::BadPoison),
        TRIPLE_STATUS_CHANCE => A::AnyOfThree,
        FLINCH_CHANCE
        | FLINCH_CHANCE_B
        | FLINCH_CHANCE_C
        | SLEEP_FLINCH_HIT
        | TWO_TURN_FLINCH_HIGH_CRIT => A::Flinch,
        CONFUSE_CHANCE => A::Confuse,
        TARGET_DOWN_CHANCE_FIRST..=TARGET_DOWN_CHANCE_LAST => {
            A::Target(stat_in(effect, TARGET_DOWN_CHANCE_FIRST), -1)
        }
        USER_DEFENSE_UP_CHANCE => A::User(Changes::one(Stat::Defense, 1)),
        USER_ATTACK_UP_CHANCE => A::User(Changes::one(Stat::Attack, 1)),
        USER_ALL_UP_CHANCE => A::UserAllUp,
        TRAP_HIT => A::Bind,
        _ => return None,
    })
}

/// Added effects that always happen when the move lands, whatever the move's chance.
pub fn certain(effect: u8) -> bool {
    matches!(effect, FIRST_TURN_FLINCH)
}

/// The five battle stats a raise-all effect raises.
pub const ALL_FIVE: [Stat; 5] = [
    Stat::Attack,
    Stat::Defense,
    Stat::Speed,
    Stat::SpAttack,
    Stat::SpDefense,
];

/// Stage changes the user always takes after hitting.
pub fn after_hit(effect: u8) -> Option<Changes> {
    match effect {
        USER_ATTACK_DEFENSE_DOWN_AFTER => Some(Changes::two(Stat::Attack, Stat::Defense, -1)),
        USER_SP_ATTACK_DOWN2_AFTER => Some(Changes::one(Stat::SpAttack, -2)),
        _ => None,
    }
}

/// Whether the effect deals set damage (`fixed_damage`) whatever the move's power.
pub fn is_fixed_damage(effect: u8) -> bool {
    matches!(
        effect,
        LEVEL_DAMAGE | LEVEL_RANDOM_DAMAGE | FIXED_40 | FIXED_20 | HALVE_HP | MATCH_HP
    )
}

/// Damage that doesn't come from the formula (only type immunity matters). `None` from
/// `MATCH_HP` when the target has no more HP than the user (it fails).
pub fn fixed_damage(effect: u8, user: &Fighter, target: &Fighter, rng: &mut Rng) -> Option<u32> {
    match effect {
        LEVEL_DAMAGE => Some(u32::from(user.level)),
        LEVEL_RANDOM_DAMAGE => {
            Some((u32::from(user.level) * (rng.below(11) * 10 + 50) / 100).max(1))
        }
        FIXED_40 => Some(40),
        FIXED_20 => Some(20),
        HALVE_HP => Some((u32::from(target.hp) / 2).max(1)),
        MATCH_HP => (target.hp > user.hp).then(|| u32::from(target.hp - user.hp)),
        _ => None,
    }
}

/// Hits per use: 2..=5 (3/8, 3/8, 1/8, 1/8) for multi-hit moves.
pub fn hit_count(effect: u8, rng: &mut Rng) -> u8 {
    match effect {
        MULTI_HIT => match rng.below(8) {
            0..=2 => 2,
            3..=5 => 3,
            6 => 4,
            _ => 5,
        },
        TWO_HITS | TWO_HITS_POISON_CHANCE => 2,
        TRIPLE_RISING => 3,
        _ => 1,
    }
}

/// The share of damage dealt the user gets back.
pub fn drain(effect: u8) -> Option<(u32, u32)> {
    matches!(effect, DRAIN_HALF | DRAIN_SLEEPER).then_some((1, 2))
}

/// The share of damage dealt the user takes back.
pub fn recoil(effect: u8) -> Option<(u32, u32)> {
    match effect {
        RECOIL_QUARTER => Some((1, 4)),
        RECOIL_THIRD => Some((1, 3)),
        _ => None,
    }
}

/// Extra critical-hit stages.
pub fn crit_stages(effect: u8) -> u8 {
    u8::from(matches!(
        effect,
        HIGH_CRIT | TWO_TURN_FLINCH_HIGH_CRIT | BURN_CHANCE_HIGH_CRIT | POISON_CHANCE_HIGH_CRIT
    ))
}

/// The accuracy to use: `None` never misses.
pub fn accuracy(mv: &MoveData, weather: Weather) -> Option<u8> {
    match mv.effect {
        ALWAYS_HITS | ALWAYS_HITS_B => None,
        STORM_PARALYZE_CHANCE if weather == Weather::Rain => None,
        STORM_PARALYZE_CHANCE if weather == Weather::Sun => Some(50),
        _ if mv.accuracy == 0 => None,
        _ => Some(mv.accuracy),
    }
}

/// Whether a frozen user can still use it (and thaws).
pub fn thaws_user(effect: u8) -> bool {
    effect == THAW_BURN_CHANCE
}

/// Whether a sleeping user can use it.
pub fn works_asleep(effect: u8) -> bool {
    matches!(effect, SLEEP_FLINCH_HIT | SLEEP_RANDOM_MOVE)
}

/// The fraction of max HP a weather heal gives.
pub fn weather_heal(weather: Weather) -> (u32, u32) {
    match weather {
        Weather::Clear => (1, 2),
        Weather::Sun => (2, 3),
        _ => (1, 4),
    }
}

/// `LOW_HP_POWER`'s power for the user's HP: by `hp * 48 / max`, as the ROM scales it.
pub fn low_hp_power(hp: u16, max: u16) -> u8 {
    let scale = u32::from(hp) * 48 / u32::from(max.max(1));
    match scale {
        0..=1 => 200,
        2..=4 => 150,
        5..=9 => 100,
        10..=16 => 80,
        17..=32 => 40,
        _ => 20,
    }
}

/// `RANDOM_POWER`'s power: 10 (5%), 30 (10%), 50 (20%), 70 (30%), 90 (20%), 110 (10%) or
/// 150 (5%).
pub fn random_power(rng: &mut Rng) -> u8 {
    match rng.below(100) {
        0..=4 => 10,
        5..=14 => 30,
        15..=34 => 50,
        35..=64 => 70,
        65..=84 => 90,
        85..=94 => 110,
        _ => 150,
    }
}

/// `STAT_BITS_POWER`'s type and power. Fighters have no hidden values, so the low bits of
/// each battle stat stand in for them: a stable type and power for each fighter.
pub fn stat_bits_power(f: &Fighter) -> (u8, u8) {
    let s = f.stats;
    let vals = [
        s.hp,
        s.attack,
        s.defense,
        s.speed,
        s.sp_attack,
        s.sp_defense,
    ];
    let mut type_bits = 0u32;
    let mut power_bits = 0u32;
    for (i, v) in vals.iter().enumerate() {
        type_bits |= u32::from(v & 1) << i;
        power_bits |= u32::from((v >> 1) & 1) << i;
    }
    let power = (40 * power_bits / 63 + 30) as u8;
    // The types other than normal and typeless, in id order.
    let mut t = (15 * type_bits / 63 + 1) as u8;
    if t >= types::TYPELESS {
        t += 1;
    }
    (t, power)
}

/// `WEATHER_POWER`'s type and whether its power doubles.
pub fn weather_power(weather: Weather) -> Option<u8> {
    match weather {
        Weather::Clear => None,
        Weather::Rain => Some(types::WATER),
        Weather::Sun => Some(types::FIRE),
        Weather::Sand => Some(types::ROCK),
        Weather::Hail => Some(types::ICE),
    }
}

/// Power from a bond of 0..=255 (`BOND_POWER`), or its lack (`BOND_POWER_INVERSE`).
pub fn bond_power(friendship: u8, inverse: bool) -> u8 {
    let f = if inverse {
        255 - friendship
    } else {
        friendship
    };
    (u32::from(f) * 10 / 25).max(1) as u8
}

/// `TRIPLE_RISING`'s power on hit `n` (0-based): 1x, 2x, 3x.
pub fn rising_power(power: u8, n: u8) -> u32 {
    u32::from(power) * (u32::from(n) + 1)
}

/// `GROWING_HIT`'s power after `count` hits in a row (this one included, 1..=5).
pub fn growing_power(power: u8, count: u8) -> u32 {
    u32::from(power) << count.clamp(1, 5).saturating_sub(1)
}

/// `ROLLING`'s power on hit `n` (1..=5), doubled again when curled.
pub fn rolling_power(power: u8, n: u8, curled: bool) -> u32 {
    let p = u32::from(power) << n.clamp(1, 5).saturating_sub(1);
    if curled { p * 2 } else { p }
}

/// `HP_POWER`'s power: 150 x HP share, at least 1.
pub fn hp_power(power: u8, hp: u16, max: u16) -> u32 {
    (u32::from(power) * u32::from(hp) / u32::from(max.max(1))).max(1)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::battle::fighter::Stats;

    #[test]
    fn ranged_ids_map_to_stats_in_order() {
        assert_eq!(
            primary(USER_UP_FIRST),
            Some(Primary::Stats(true, Changes::one(Stat::Attack, 1)))
        );
        assert_eq!(
            primary(USER_UP2_FIRST + 2),
            Some(Primary::Stats(true, Changes::one(Stat::Speed, 2)))
        );
        assert_eq!(
            primary(TARGET_DOWN_LAST),
            Some(Primary::Stats(false, Changes::one(Stat::Evasion, -1)))
        );
        assert_eq!(
            added(TARGET_DOWN_CHANCE_FIRST + 4),
            Some(Added::Target(Stat::SpDefense, -1))
        );
        assert_eq!(primary(HIT), None);
        assert_eq!(added(HIT), None);
        // Unknown ids: nothing special.
        assert_eq!(primary(250), None);
        assert_eq!(added(250), None);
    }

    #[test]
    fn coverage() {
        let handled = (0..=255u8).filter(|&e| implemented(e)).count();
        assert_eq!(handled, 209);
        assert!(!implemented(96) && !implemented(COUNT));
    }

    #[test]
    fn multi_hit_spread() {
        let mut rng = Rng::new(3);
        let mut seen = [0u32; 6];
        for _ in 0..8000 {
            seen[usize::from(hit_count(MULTI_HIT, &mut rng))] += 1;
        }
        assert_eq!(seen[0] + seen[1], 0);
        assert!((2700..3300).contains(&seen[2]), "{seen:?}");
        assert!((2700..3300).contains(&seen[3]), "{seen:?}");
        assert!((800..1200).contains(&seen[4]), "{seen:?}");
        assert!((800..1200).contains(&seen[5]), "{seen:?}");
        assert_eq!(hit_count(TWO_HITS, &mut rng), 2);
        assert_eq!(hit_count(HIT, &mut rng), 1);
    }

    #[test]
    fn accuracy_rules() {
        let mut mv = MoveData::plain(1, 0, 40);
        mv.accuracy = 70;
        assert_eq!(accuracy(&mv, Weather::Clear), Some(70));
        mv.effect = ALWAYS_HITS;
        assert_eq!(accuracy(&mv, Weather::Clear), None);
        mv.effect = STORM_PARALYZE_CHANCE;
        assert_eq!(accuracy(&mv, Weather::Rain), None);
        assert_eq!(accuracy(&mv, Weather::Sun), Some(50));
        mv.effect = HIT;
        mv.accuracy = 0;
        assert_eq!(accuracy(&mv, Weather::Clear), None);
    }

    #[test]
    fn power_scales() {
        assert_eq!(low_hp_power(1, 300), 200);
        assert_eq!(low_hp_power(300, 300), 20);
        assert_eq!(low_hp_power(60, 300), 100);
        assert_eq!(bond_power(255, false), 102);
        assert_eq!(bond_power(255, true), 1);
        assert_eq!(growing_power(10, 1), 10);
        assert_eq!(growing_power(10, 5), 160);
        assert_eq!(growing_power(10, 9), 160);
        assert_eq!(rolling_power(30, 3, false), 120);
        assert_eq!(rolling_power(30, 1, true), 60);
        assert_eq!(rising_power(10, 2), 30);
        assert_eq!(hp_power(150, 50, 100), 75);
        assert_eq!(weather_power(Weather::Sun), Some(types::FIRE));
        let mut rng = Rng::new(1);
        let mut seen = std::collections::BTreeMap::new();
        for _ in 0..2000 {
            *seen.entry(random_power(&mut rng)).or_insert(0) += 1;
        }
        assert_eq!(seen.len(), 7);
        assert!(seen[&70] > seen[&10] * 3, "{seen:?}");
    }

    #[test]
    fn stat_bits_give_a_real_type() {
        let mut f = Fighter::new("a", 50, Stats::default(), [0, 0], 0, &[]);
        assert_eq!(stat_bits_power(&f), (types::FIGHTING, 30));
        f.stats = Stats {
            hp: 63,
            attack: 63,
            defense: 63,
            speed: 63,
            sp_attack: 63,
            sp_defense: 63,
        };
        assert_eq!(stat_bits_power(&f), (types::DARK, 70));
        for v in 0..64u16 {
            f.stats.speed = v;
            let (t, p) = stat_bits_power(&f);
            assert!(t != types::NORMAL && t != types::TYPELESS && t < types::COUNT);
            assert!((30..=70).contains(&p));
        }
    }

    #[test]
    fn hiding_and_reach() {
        let mut hider = MoveData::plain(1, types::FLYING, 90);
        hider.effect = HIDE_TWO_TURN;
        assert_eq!(hides(&hider), Some(Hidden::Air));
        hider.type_id = types::GROUND;
        assert_eq!(hides(&hider), Some(Hidden::Underground));
        let mut digger = MoveData::plain(2, types::GROUND, 100);
        digger.effect = HITS_UNDERGROUND;
        assert_eq!(reaches(&digger, Hidden::Underground), Some(2));
        assert_eq!(reaches(&digger, Hidden::Air), None);
        let mut wave = MoveData::plain(3, types::WATER, 95);
        assert_eq!(reaches(&wave, Hidden::Underwater), None);
        wave.target = super::super::data::Target::BothFoes;
        assert_eq!(reaches(&wave, Hidden::Underwater), Some(2));
    }
}
