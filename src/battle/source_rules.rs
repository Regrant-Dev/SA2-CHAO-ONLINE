//! Explicit source-to-engine semantic compiler and coverage gate.
//!
//! Archive effect sequence numbers are never cast into engine opcodes. A move's behavior comes
//! from its own record fields (category, added status and its chance, stat changes, hit
//! counts, critical stage, recoil or drain, flags) for audited move identities, and from a
//! short list of named engine mechanics where the record can't express what a move does.
//! Unknown mechanics stay unavailable instead of becoming plain hits. All names, stats and
//! gameplay records come from the player's imported source.

use super::data::{Category, MoveData, MoveDetails, Target, flags};
use super::fighter::{Ailment, Stat};
use super::{abilities, effects};
use crate::creature::data::{MoveCategory, MovePolicy, MoveRecord};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SupportStatus {
    Supported,
    ExcludedPowerUp,
    Unsupported(&'static str),
}

/// Deterministic rules identity; bump whenever behavior or the support policy changes.
pub const RULES_VERSION: &str = "ordinary-v8-record-mechanics-2";

/// Moves whose record fields say everything the engine needs. Indices are canonical IDs.
const RECORD_DRIVEN: &[u16] = &[
    1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 14, 15, 16, 17, 18, 20, 21, 22, 23, 24, 25, 27, 28, 29,
    30, 31, 32, 33, 34, 35, 36, 38, 39, 40, 41, 42, 43, 44, 45, 46, 47, 48, 51, 52, 53, 55, 56, 57,
    58, 59, 60, 61, 62, 63, 64, 65, 66, 70, 71, 72, 73, 74, 75, 77, 78, 79, 81, 83, 84, 85, 86, 88,
    89, 90, 92, 93, 94, 95, 96, 97, 98, 103, 104, 105, 106, 108, 109, 110, 112, 121, 122, 123, 124,
    125, 126, 127, 128, 129, 131, 132, 133, 134, 135, 137, 139, 140, 141, 142, 145, 146, 147, 148,
    151, 152, 154, 155, 157, 158, 159, 163, 171, 172, 177, 178, 181, 183, 184, 185, 186, 188, 189,
    190, 192, 193, 195, 196, 198, 202, 204, 207, 208, 209, 211, 221, 223, 224, 225, 230, 231, 232,
    233, 238, 239, 242, 245, 246, 247, 249, 250, 257, 259, 260, 261, 275, 276, 281, 292, 294, 295,
    296, 297, 298, 299, 302, 303, 304, 305, 306, 307, 308, 309, 310, 313, 314, 315, 316, 317, 318,
    319, 320, 321, 322, 324, 325, 326, 327, 328, 329, 330, 331, 332, 333, 334, 336, 337, 338, 339,
    341, 342, 344, 345, 347, 348, 349, 350, 351, 352, 354, 359, 370, 394, 395, 396, 397, 398, 399,
    400, 401, 402, 403, 404, 405, 406, 407, 408, 409, 410, 411, 412, 413, 414, 416, 417, 418, 420,
    421, 422, 423, 424, 425, 426, 427, 428, 429, 430, 431, 434, 435, 436, 437, 438, 439, 440, 441,
    442, 443, 444, 448, 451, 452, 453, 454, 455, 457, 459, 460, 463, 465, 466, 468, 482, 483, 488,
    489, 490, 491, 498, 508, 517, 522, 523, 526, 527, 528, 529, 531, 532, 533, 534, 536, 537, 538,
    539, 543, 545, 549, 550, 551, 552, 555, 556, 557, 568, 570, 572, 574, 577, 583, 584, 585, 586,
    589, 590, 591, 595, 598, 605, 608, 609, 611, 612, 613, 616, 618, 619, 620, 665, 667, 670, 679,
    680, 684, 688, 692, 693, 705, 708, 709, 710, 711, 715, 716,
];

/// Engine behavior a move record can't express.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Mechanic {
    /// This engine effect is the move's whole behavior; the record's added-effect fields are
    /// not read (the effect's chance still comes from the record).
    Own(u8),
    /// Locked in for 2 or 3 turns, then confused. The record's added effects still apply.
    Rampage,
}

fn mechanic(id: u16) -> Option<Mechanic> {
    use Mechanic::{Own, Rampage};
    use effects as e;
    Some(match id {
        37 | 80 | 200 => Rampage,
        13 => Own(e::TWO_TURN),
        19 | 91 | 291 | 340 => Own(e::HIDE_TWO_TURN),
        26 | 136 => Own(e::CRASH_ON_MISS),
        49 => Own(e::FIXED_20),
        50 => Own(e::SEAL_LAST_MOVE),
        54 => Own(e::STAT_GUARD),
        67 | 447 => Own(e::WEIGHT_POWER),
        68 => Own(e::RETURN_PHYSICAL),
        69 | 101 => Own(e::LEVEL_DAMAGE),
        76 => Own(e::SUN_CHARGE),
        82 => Own(e::FIXED_40),
        87 => Own(e::STORM_PARALYZE_CHANCE),
        99 => Own(e::ANGER_HIT),
        100 => Own(e::FLEE),
        102 => Own(e::COPY_MOVE_TEMP),
        107 => Own(e::SHRINK),
        111 => Own(e::CURL),
        113 => Own(e::SPECIAL_WALL),
        114 => Own(e::RESET_STAGES),
        115 => Own(e::PHYSICAL_WALL),
        116 => Own(e::FOCUS),
        117 => Own(e::STORE_DAMAGE),
        118 => Own(e::RANDOM_MOVE),
        119 => Own(e::COPY_FOES_MOVE),
        120 | 153 => Own(e::SELF_FAINT),
        130 => Own(e::TWO_TURN_DEFENSE_UP),
        138 => Own(e::DRAIN_SLEEPER),
        143 => Own(e::TWO_TURN_FLINCH_HIGH_CRIT),
        149 => Own(e::LEVEL_RANDOM_DAMAGE),
        150 => Own(e::NOTHING),
        156 => Own(e::SLEEP_HEAL),
        160 => Own(e::TYPE_FROM_OWN_MOVE),
        161 => Own(e::TRIPLE_STATUS_CHANCE),
        162 => Own(e::HALVE_HP),
        164 => Own(e::DECOY),
        165 => Own(e::RECOIL_QUARTER),
        167 => Own(e::TRIPLE_RISING),
        168 | 343 => Own(e::HIT_TAKE_ITEM),
        169 | 212 | 335 => Own(e::BLOCK_ESCAPE),
        170 | 199 => Own(e::SURE_HIT_NEXT),
        173 => Own(e::SLEEP_FLINCH_HIT),
        174 => Own(e::HEX_OR_BOOST),
        175 | 179 => Own(e::LOW_HP_POWER),
        176 => Own(e::TYPE_RESIST_LAST),
        180 => Own(e::CUT_PP),
        182 | 197 => Own(e::SHIELD),
        187 => Own(e::MAX_ATTACK_HALF_HP),
        191 => Own(e::HAZARD),
        201 => Own(e::SAND),
        203 => Own(e::BRACE),
        205 | 301 => Own(e::ROLLING),
        206 | 610 => Own(e::LEAVE_ONE_HP),
        210 => Own(e::GROWING_HIT),
        214 => Own(e::SLEEP_RANDOM_MOVE),
        215 | 312 => Own(e::CURE_TEAM),
        216 => Own(e::BOND_POWER),
        217 => Own(e::RANDOM_GIFT),
        218 => Own(e::BOND_POWER_INVERSE),
        219 => Own(e::STATUS_GUARD),
        220 => Own(e::SHARE_HP),
        222 => Own(e::RANDOM_POWER),
        226 => Own(e::PASS_STATE),
        227 => Own(e::REPEAT_LAST),
        228 => Own(e::HITS_SWITCHING),
        229 => Own(e::CLEAR_HAZARDS),
        234 => Own(e::WEATHER_HEAL_A),
        235 => Own(e::WEATHER_HEAL_B),
        236 => Own(e::WEATHER_HEAL_C),
        237 => Own(e::STAT_BITS_POWER),
        240 => Own(e::RAIN),
        241 => Own(e::SUN),
        243 => Own(e::RETURN_SPECIAL),
        244 => Own(e::COPY_STAGES),
        248 | 353 => Own(e::DELAYED_HIT),
        251 => Own(e::TEAM_HITS),
        252 => Own(e::FIRST_TURN_FLINCH),
        253 => Own(e::NOISE),
        254 => Own(e::STORE_UP),
        255 => Own(e::RELEASE_HIT),
        256 => Own(e::RELEASE_HEAL),
        258 => Own(e::HAIL),
        262 => Own(e::FAINT_LOWER_TARGET),
        263 => Own(e::STATUS_POWER),
        264 => Own(e::FOCUSED_HIT),
        265 => Own(e::PARALYSIS_POWER_CURE),
        266 => Own(e::DRAW_MOVES),
        268 => Own(e::POWER_UP_ELECTRIC),
        269 => Own(e::PROVOKE),
        270 => Own(e::HELP_ALLY),
        271 => Own(e::SWAP_ITEMS),
        272 => Own(e::COPY_ABILITY),
        273 => Own(e::DELAYED_HEAL),
        274 => Own(e::ALLY_RANDOM_MOVE),
        277 => Own(e::BOUNCE_STATUS),
        278 => Own(e::RESTORE_ITEM),
        279 => Own(e::HURT_FIRST_POWER),
        280 => Own(e::BREAK_SCREENS),
        282 => Own(e::HIT_DROP_ITEM),
        283 => Own(e::MATCH_HP),
        284 | 323 => Own(e::HP_POWER),
        285 => Own(e::SWAP_ABILITIES),
        286 => Own(e::SEAL_SHARED_MOVES),
        287 => Own(e::CURE_SELF),
        288 => Own(e::PP_RANCOR),
        289 => Own(e::STEAL_MOVE),
        293 => Own(e::ARENA_TYPE),
        311 => Own(e::WEATHER_POWER),
        546 => Own(e::ALWAYS_HITS),
        580 => Own(e::OVERGROWN_FIELD),
        581 => Own(e::HAZY_FIELD),
        604 => Own(e::CHARGED_FIELD),
        678 => Own(e::STRANGE_FIELD),
        _ => return None,
    })
}

/// Engine flags a record can't express: where a move reaches a hiding target (and whether it
/// hits harder there), double damage on a shrunk target, powder, and stat or decoy rules.
fn listed_flags(id: u16) -> u32 {
    let reach = match id {
        16 | 239 => flags::REACHES_AIR | flags::DOUBLE_ON_REACH,
        327 => flags::REACHES_AIR,
        89 => flags::REACHES_UNDERGROUND | flags::DOUBLE_ON_REACH,
        57 | 250 => flags::REACHES_UNDERWATER | flags::DOUBLE_ON_REACH,
        _ => 0,
    };
    let mut out = reach;
    if matches!(id, 23 | 34 | 407 | 484 | 535 | 537 | 560 | 566) {
        out |= flags::PUNISHES_SHRINK;
    }
    if matches!(id, 77 | 78 | 79 | 147 | 178 | 476 | 600) {
        out |= flags::POWDER;
    }
    if matches!(id, 432 | 513 | 683) {
        out |= flags::BYPASS_DECOY;
    }
    if matches!(id, 473 | 540 | 548) {
        out |= flags::TARGET_DEFENSE;
    }
    if id == 492 {
        out |= flags::TARGET_ATTACK;
    }
    if id == 663 {
        out |= flags::IGNORE_DEFENSE_STAGES | flags::IGNORE_EVASION;
    }
    if matches!(id, 480 | 524) {
        out |= flags::ALWAYS_CRITICAL;
    }
    out
}

fn category(source: MoveCategory) -> Category {
    match source {
        MoveCategory::Status => Category::Status,
        MoveCategory::Physical => Category::Physical,
        MoveCategory::Special => Category::Special,
    }
}

/// The record's added-status field: the major statuses, confusion and the battle conditions
/// the engine runs.
mod inflict {
    pub const PARALYSIS: u16 = 1;
    pub const SLEEP: u16 = 2;
    pub const FREEZE: u16 = 3;
    pub const BURN: u16 = 4;
    pub const POISON: u16 = 5;
    pub const CONFUSION: u16 = 6;
    pub const BIND: u16 = 8;
    pub const HAUNTED_SLEEP: u16 = 9;
    pub const NO_REPEATS: u16 = 12;
    pub const SEAL_LAST_MOVE: u16 = 13;
    pub const DROWSE: u16 = 14;
    pub const IDENTIFY: u16 = 17;
    pub const SEED: u16 = 18;
    pub const COUNTDOWN: u16 = 20;
    pub const ROOT: u16 = 21;
}

/// The record's quality field: what kind of move it is.
mod quality {
    pub const DAMAGE: u8 = 0;
    pub const STATUS: u8 = 1;
    pub const STATS: u8 = 2;
    pub const HEAL: u8 = 3;
    pub const DAMAGE_AILMENT: u8 = 4;
    pub const CONFUSE_AND_RAISE: u8 = 5;
    pub const DAMAGE_LOWER_TARGET: u8 = 6;
    pub const DAMAGE_RAISE_USER: u8 = 7;
    pub const DRAIN: u8 = 8;
    pub const ONE_HIT_KO: u8 = 9;
    pub const FORCE_SWITCH: u8 = 12;
}

/// The record flag bits the engine reads, besides the plain ones `mapped_flags` copies.
const CHARGES_FIRST: u32 = 1 << 1;
const RECHARGES_AFTER: u32 = 1 << 2;
/// The record's stat id for every battle stat at once.
const ALL_STATS: u8 = 8;
/// The record's turn count that marks a worsening poison.
const WORSENING_POISON_TURNS: u8 = 15;

fn ailment(source: &MoveRecord) -> Option<Ailment> {
    match source.inflict {
        inflict::PARALYSIS => Some(Ailment::Paralysis),
        inflict::SLEEP => Some(Ailment::Sleep),
        inflict::FREEZE => Some(Ailment::Freeze),
        inflict::BURN => Some(Ailment::Burn),
        inflict::POISON if source.min_turns == WORSENING_POISON_TURNS => Some(Ailment::BadPoison),
        inflict::POISON => Some(Ailment::Poison),
        _ => None,
    }
}

fn mapped_flags(source: &MoveRecord) -> u32 {
    let mut out = 0;
    for (raw, mapped) in [
        (0, flags::CONTACT),
        (3, flags::BLOCKED_BY_PROTECTION),
        (4, flags::REFLECTABLE),
        (5, flags::STEALABLE),
        (6, flags::COPYABLE),
        (7, flags::PUNCH),
        (8, flags::SOUND),
        (10, flags::THAWS_USER),
        (12, flags::HEAL),
        (13, flags::BYPASS_DECOY),
        (16, flags::DANCE),
    ] {
        if source.flags & (1 << raw) != 0 {
            out |= mapped;
        }
    }
    out
}

/// The record's stat changes, recoil or drain and added effects, in `details`.
fn record_details(source: &MoveRecord, details: &mut MoveDetails) -> Result<(), &'static str> {
    details.self_stats = source.quality == quality::DAMAGE_RAISE_USER || source.target_raw == 7;
    for i in 0..3 {
        let stat = source.stat_ids[i];
        if stat == 0 || stat == ALL_STATS {
            continue;
        }
        if stat > 7 || !(-6..=6).contains(&source.stat_stages[i]) {
            return Err("unknown stat effect");
        }
        // Move records order the special stats before speed, unlike personal records.
        let slot = [0usize, 1, 3, 4, 2, 5, 6][usize::from(stat - 1)];
        details.stages[slot] = source.stat_stages[i];
        details.stage_chances[slot] = if source.category == MoveCategory::Status {
            100
        } else {
            source.stat_percents[i]
        };
    }
    details.inflict = ailment(source);
    if details.inflict.is_some() {
        details.inflict_chance = source.inflict_percent;
    }
    if source.inflict == inflict::CONFUSION {
        details.confuse_chance = source.inflict_percent;
    }
    details.flinch_chance = source.flinch_percent;
    if source.category != MoveCategory::Status {
        // Negative values are recoil, positive ones drain, both percent of the damage dealt.
        let raw = source.recoil_raw as i8;
        if raw.unsigned_abs() > 100 {
            return Err("recoil or drain is outside the supported rules bounds");
        }
        if raw < 0 {
            details.recoil_percent = raw.unsigned_abs();
        } else {
            details.drain_percent = raw as u8;
        }
    }
    Ok(())
}

/// The engine effect a record-driven move runs.
fn record_effect(source: &MoveRecord, details: &mut MoveDetails) -> Result<u8, &'static str> {
    let all_up = source.stat_ids[0] == ALL_STATS;
    if source.stat_ids[1..].contains(&ALL_STATS) || (all_up && source.stat_stages[0] != 1) {
        return Err("unknown stat effect");
    }
    if source.category == MoveCategory::Status {
        if all_up {
            return Err("status move mechanic is not implemented");
        }
        let stats = details.stages.iter().any(|&n| n != 0);
        return Ok(match (source.quality, source.inflict) {
            (quality::STATS, _) if stats => {
                if details.self_stats {
                    effects::DATA_SELF_STATS
                } else {
                    effects::DATA_TARGET_STATS
                }
            }
            (quality::CONFUSE_AND_RAISE, inflict::CONFUSION) => {
                let raised: Vec<_> = (0..7).filter(|&i| details.stages[i] != 0).collect();
                let effect = match raised[..] {
                    [i] if Stat::ALL[i] == Stat::Attack && details.stages[i] == 2 => {
                        effects::CONFUSE_ATTACK_UP2
                    }
                    [i] if Stat::ALL[i] == Stat::SpAttack && details.stages[i] == 1 => {
                        effects::CONFUSE_SP_ATTACK_UP
                    }
                    _ => return Err("status move mechanic is not implemented"),
                };
                // The effect raises and confuses; the stages aren't applied twice.
                *details = MoveDetails::default();
                effect
            }
            (quality::HEAL, 0) if source.healing_raw == 50 && source.target_raw == 7 => {
                effects::HEAL_HALF
            }
            (quality::FORCE_SWITCH, 0) => effects::FORCE_SWITCH,
            (quality::STATUS | quality::STATS, code) => match code {
                inflict::CONFUSION => effects::CONFUSE,
                inflict::HAUNTED_SLEEP => effects::HAUNTED_SLEEP,
                inflict::NO_REPEATS => effects::NO_REPEATS,
                inflict::SEAL_LAST_MOVE => effects::SEAL_LAST_MOVE,
                inflict::DROWSE => effects::DROWSE,
                inflict::IDENTIFY => effects::IDENTIFY,
                inflict::SEED => effects::SEED_DRAIN,
                inflict::COUNTDOWN => effects::COUNTDOWN,
                inflict::ROOT => effects::ROOT,
                _ => match details.inflict {
                    Some(Ailment::Sleep) => effects::SLEEP,
                    Some(Ailment::Poison) => effects::POISON,
                    Some(Ailment::BadPoison) => effects::BAD_POISON,
                    Some(Ailment::Paralysis) => effects::PARALYZE,
                    Some(Ailment::Burn) => effects::BURN,
                    _ => return Err("status move mechanic is not implemented"),
                },
            },
            _ => return Err("status move mechanic is not implemented"),
        });
    }
    if !matches!(
        source.quality,
        quality::DAMAGE
            | quality::DAMAGE_AILMENT
            | quality::DAMAGE_LOWER_TARGET
            | quality::DAMAGE_RAISE_USER
            | quality::DRAIN
            | quality::ONE_HIT_KO
    ) || source.healing_raw != 0
    {
        return Err("damaging move mechanic is not implemented");
    }
    if source.inflict != 0
        && details.inflict.is_none()
        && !matches!(source.inflict, inflict::CONFUSION | inflict::BIND)
    {
        return Err("source status mechanic is not implemented");
    }
    if source.flags & CHARGES_FIRST != 0 {
        return Err("charging move needs its specific implementation");
    }
    let hits = (source.min_hits, source.max_hits);
    if !matches!(hits, (0, 0) | (1, 1) | (2, 2) | (2, 5)) {
        return Err("multi-hit mode is not implemented");
    }
    Ok(if source.flags & RECHARGES_AFTER != 0 {
        effects::RECHARGE_AFTER
    } else if source.quality == quality::ONE_HIT_KO {
        effects::ONE_HIT_KO
    } else if all_up {
        effects::USER_ALL_UP_CHANCE
    } else if source.inflict == inflict::BIND {
        effects::TRAP_HIT
    } else if hits == (2, 5) {
        effects::MULTI_HIT
    } else if hits == (2, 2) {
        effects::TWO_HITS
    } else if source.crit_stage > 0 {
        effects::HIGH_CRIT
    } else {
        effects::HIT
    })
}

fn compile(source: &MoveRecord) -> Result<MoveData, &'static str> {
    if source.id == 0 || source.id > 728 {
        return Err("empty or unsupported move identity");
    }
    if source.type_id >= 18
        || source.accuracy > 101
        || source.pp == 0
        || source.pp > 40
        || !(-7..=7).contains(&source.priority)
        || source.inflict_percent > 100
        || source.flinch_percent > 100
        || source.stat_percents.iter().any(|&chance| chance > 100)
    {
        return Err("move metadata is outside the supported rules bounds");
    }
    if crate::creature::data::move_policy(source.id) == MovePolicy::ExcludedPowerUpMove {
        return Err("excluded power-up move");
    }
    let target = match source.target_raw {
        0 | 3 => Target::Selected,
        1 | 2 => Target::UserOrSelected,
        6 => Target::User,
        4 => Target::AllOthers,
        5 => Target::BothFoes,
        7 | 8 | 10 | 12 => Target::User,
        9 => Target::RandomFoe,
        11 => Target::FoesField,
        13 => Target::Depends,
        _ => return Err("target mode is not implemented"),
    };
    let mut details = MoveDetails::default();
    let effect = if super::engine::ordinary::implemented(source.id) {
        details.special = source.id;
        if source.category == MoveCategory::Status {
            if matches!(source.target_raw, 6 | 7 | 8 | 10 | 12) {
                effects::ORDINARY_SELF
            } else {
                effects::ORDINARY_TARGET
            }
        } else {
            effects::HIT
        }
    } else if let Some(mechanic) = mechanic(source.id) {
        match mechanic {
            Mechanic::Own(effect) => effect,
            Mechanic::Rampage => {
                record_details(source, &mut details)?;
                record_effect(source, &mut details)?;
                effects::RAMPAGE
            }
        }
    } else if RECORD_DRIVEN.contains(&source.id) {
        record_details(source, &mut details)?;
        record_effect(source, &mut details)?
    } else {
        return Err("ordinary move mechanic is not implemented");
    };
    if !effects::implemented(effect) {
        return Err("semantic opcode is unavailable");
    }
    Ok(MoveData {
        id: source.id,
        effect,
        power: if source.category != MoveCategory::Status && details.special != 0 {
            source.power.max(1)
        } else {
            source.power
        },
        type_id: source.type_id,
        category: category(source.category),
        details,
        accuracy: if source.accuracy == 101 {
            0
        } else {
            source.accuracy
        },
        pp: source.pp,
        chance: source
            .inflict_percent
            .max(source.flinch_percent)
            .max(*source.stat_percents.iter().max().unwrap_or(&0)),
        target,
        priority: source.priority,
        flags: mapped_flags(source) | listed_flags(source.id),
    })
}

pub fn compile_move(source: &MoveRecord) -> Option<MoveData> {
    compile(source).ok()
}
pub fn move_supported(source: &MoveRecord) -> bool {
    compile_move(source).is_some()
}
pub fn support_status(source: &MoveRecord) -> SupportStatus {
    if crate::creature::data::move_policy(source.id) == MovePolicy::ExcludedPowerUpMove {
        SupportStatus::ExcludedPowerUp
    } else {
        match compile(source) {
            Ok(_) => SupportStatus::Supported,
            Err(reason) => SupportStatus::Unsupported(reason),
        }
    }
}
pub fn ability_supported(id: u8) -> bool {
    id == 0 || abilities::implemented(id)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(id: u16) -> MoveRecord {
        let mut raw = [0u8; crate::creature::data::MOVE_SIZE];
        raw[2] = 2;
        raw[3] = 90;
        raw[4] = 100;
        raw[5] = 15;
        crate::creature::data::parse_move(&raw, id).unwrap()
    }

    #[test]
    fn source_effect_sequences_are_not_engine_opcodes() {
        let mut source = record(1);
        source.effect_sequence = 411;
        source.category = MoveCategory::Physical;
        let compiled = compile_move(&source).unwrap();
        assert_eq!(compiled.effect, effects::HIT);
        assert_eq!(compiled.category, Category::Physical);
        // A move with special source-code behavior must not become a hit merely because
        // its data descriptor happens to use sequence zero.
        let mut special = record(722);
        special.effect_sequence = 0;
        assert!(compile_move(&special).is_none());
        assert!(matches!(
            support_status(&special),
            SupportStatus::Unsupported(_)
        ));
        // A listed mechanic doesn't follow the sequence number either.
        let mut listed = record(13);
        listed.effect_sequence = 0;
        assert_eq!(compile_move(&listed).unwrap().effect, effects::TWO_TURN);
    }

    #[test]
    fn ordinary_power_up_metadata_is_retained_but_power_up_moves_are_always_excluded() {
        let mut ordinary = record(53);
        ordinary.power_up.power = 175;
        ordinary.power_up.move_id = 640;
        assert!(compile_move(&ordinary).is_some());
        for id in [622, 658, 695, 703, 719, 723, 728] {
            let mut source = record(id);
            source.policy = MovePolicy::Ordinary; // untrusted deserialized policy must not bypass the ban
            assert!(compile_move(&source).is_none());
            assert_eq!(support_status(&source), SupportStatus::ExcludedPowerUp);
        }
    }

    #[test]
    fn source_stat_ids_map_special_stats_before_speed() {
        let mut lowers_sp_defense = record(412);
        lowers_sp_defense.quality = 6;
        lowers_sp_defense.stat_ids[0] = 4;
        lowers_sp_defense.stat_stages[0] = -1;
        lowers_sp_defense.stat_percents[0] = 10;
        let details = compile_move(&lowers_sp_defense).unwrap().details;
        assert_eq!(details.stages[Stat::SpDefense as usize], -1);
        assert_eq!(details.stages[Stat::SpAttack as usize], 0);
        let mut lowers_sp_attack = record(585);
        lowers_sp_attack.quality = 6;
        lowers_sp_attack.stat_ids[0] = 3;
        lowers_sp_attack.stat_stages[0] = -1;
        lowers_sp_attack.stat_percents[0] = 30;
        assert_eq!(
            compile_move(&lowers_sp_attack).unwrap().details.stages[Stat::SpAttack as usize],
            -1
        );
    }

    #[test]
    fn the_charged_field_keeps_its_own_identity() {
        let mut field = record(604);
        field.category = MoveCategory::Status;
        field.power = 0;
        field.target_raw = 10;
        assert_eq!(compile_move(&field).unwrap().effect, effects::CHARGED_FIELD);
        let mut neighbor = record(587);
        neighbor.category = MoveCategory::Status;
        neighbor.power = 0;
        neighbor.target_raw = 10;
        assert!(compile_move(&neighbor).is_none_or(|m| m.effect != effects::CHARGED_FIELD));
    }

    #[test]
    fn every_listed_mechanic_is_an_implemented_engine_effect() {
        for id in 1..=crate::creature::data::MAX_MOVE {
            if let Some(Mechanic::Own(effect)) = mechanic(id) {
                assert!(effects::implemented(effect), "{id}");
            }
            assert!(
                !(mechanic(id).is_some() && RECORD_DRIVEN.contains(&id)),
                "{id} is both listed and record-driven"
            );
            assert!(
                !(super::super::engine::ordinary::implemented(id)
                    && (mechanic(id).is_some() || RECORD_DRIVEN.contains(&id))),
                "{id} has two definitions"
            );
        }
        assert!(RECORD_DRIVEN.windows(2).all(|w| w[0] < w[1]));
    }
}
