//! Ability hooks: each point in a turn where an ability can step in asks here. A fighter has one
//! ability id (0 = none).
//!
//! Ability ids run 0..COUNT; `implemented` lists the ones the engine handles, and no other
//! ability is allowed in battle (`source_rules::ability_supported`). Eight handled ids do
//! nothing in battle (`PASSIVE_*`). Two only work against fighters with a gender, which none
//! have here (`NO_INFATUATION`, `CONTACT_INFATUATE`), and `WEATHER_TYPE` changes any holder's
//! type with the weather. The sound-move blocker (`NO_SOUND_MOVES`) needs the imported sound
//! move list (`Battle::set_sound_moves`).

use super::damage::Attack;
use super::data::types;
use super::field::Weather;
use super::fighter::{Ailment, Fighter, Stat};

/// Ability ids 0..COUNT.
pub const COUNT: u8 = 234;

pub const NONE: u8 = 0;
/// No effect in battle.
pub const PASSIVE_A: u8 = 1;
pub const PASSIVE_B: u8 = 35;
pub const PASSIVE_C: u8 = 50;
pub const PASSIVE_D: u8 = 53;
pub const PASSIVE_E: u8 = 60;
pub const PASSIVE_F: u8 = 82;
pub const PASSIVE_G: u8 = 103;
pub const PASSIVE_H: u8 = 118;
/// Sets lasting weather on entry.
pub const ENTRY_RAIN: u8 = 2;
pub const ENTRY_SAND: u8 = 45;
pub const ENTRY_SUN: u8 = 70;
pub const ENTRY_HAIL: u8 = 117;
/// Raises speed at each turn's end.
pub const SPEED_EACH_TURN: u8 = 3;
/// Never takes critical hits.
pub const NO_CRITS_A: u8 = 4;
pub const NO_CRITS_B: u8 = 75;
/// One-hit knock-out moves fail on it.
pub const NO_ONE_HIT_KO: u8 = 5;
/// Self-fainting moves fail while it's on the field.
pub const NO_SELF_FAINT_MOVES: u8 = 6;
/// Can't be paralysed.
pub const NO_PARALYSIS: u8 = 7;
/// Takes no sand damage, and is harder to hit in sand.
pub const SAND_SHELTER: u8 = 8;
/// Contact may paralyse the attacker (1/3).
pub const CONTACT_PARALYZE: u8 = 9;
/// Electric / water moves heal it by 1/4 instead of hurting.
pub const ELECTRIC_HEALS: u8 = 10;
pub const WATER_HEALS: u8 = 11;
/// Can't be infatuated.
pub const NO_INFATUATION: u8 = 12;
/// Weather has no effect while it's on the field.
pub const NO_WEATHER_A: u8 = 13;
pub const NO_WEATHER_B: u8 = 76;
/// Hits more often.
pub const SURE_AIM: u8 = 14;
/// Can't fall asleep.
pub const NO_SLEEP_A: u8 = 15;
pub const NO_SLEEP_B: u8 = 72;
/// Its type becomes the type of a move that hits it.
pub const TYPE_FROM_HIT: u8 = 16;
/// Can't be poisoned.
pub const NO_POISON: u8 = 17;
/// Fire moves don't hurt it; they power up its own fire moves instead.
pub const FIRE_ABSORB_BOOST: u8 = 18;
/// Moves' added effects don't reach it.
pub const NO_ADDED_EFFECTS: u8 = 19;
/// Can't be confused.
pub const NO_CONFUSION: u8 = 20;
/// Can't be forced out.
pub const NO_FORCED_SWITCH: u8 = 21;
/// Lowers the foes' attack on entry.
pub const ENTRY_LOWERS_ATTACK: u8 = 22;
/// Foes can't switch out.
pub const FOES_CANT_ESCAPE: u8 = 23;
/// Contact costs the attacker 1/16 of its max HP.
pub const CONTACT_HURTS_A: u8 = 24;
pub const CONTACT_HURTS_B: u8 = 160;
/// Only super-effective damaging moves hurt it.
pub const ONLY_SUPER_EFFECTIVE: u8 = 25;
/// Ground moves don't reach it.
pub const IMMUNE_TO_GROUND: u8 = 26;
/// Contact may poison, paralyse or put the attacker to sleep (1/10).
pub const CONTACT_SPORES: u8 = 27;
/// A foe that poisons, burns or paralyses it gets the same.
pub const SHARES_STATUS: u8 = 28;
/// Other fighters can't lower its stages.
pub const NO_STAT_DROPS_A: u8 = 29;
pub const NO_STAT_DROPS_B: u8 = 73;
pub const NO_STAT_DROPS_C: u8 = 230;
/// Its status is cured when it switches out.
pub const CURED_ON_SWITCH: u8 = 30;
/// Draws foes' single-target electric moves to itself (doubles).
pub const DRAWS_ELECTRIC: u8 = 31;
/// Doubles its moves' added-effect chances.
pub const DOUBLE_CHANCES: u8 = 32;
/// Doubled speed in rain / in sun.
pub const RAIN_SPEED: u8 = 33;
pub const SUN_SPEED: u8 = 34;
/// Doubled speed in sand / in hail.
pub const SAND_SPEED: u8 = 146;
pub const HAIL_SPEED: u8 = 202;
/// Takes a foe's ability on entry.
pub const COPIES_FOE_ABILITY: u8 = 36;
/// Doubled attack.
pub const DOUBLE_ATTACK_A: u8 = 37;
pub const DOUBLE_ATTACK_B: u8 = 74;
/// Contact may poison the attacker (1/3).
pub const CONTACT_POISON: u8 = 38;
/// Never flinches.
pub const NO_FLINCH: u8 = 39;
/// Can't be frozen.
pub const NO_FREEZE: u8 = 40;
/// Can't be burned.
pub const NO_BURN: u8 = 41;
/// Steel foes can't switch out.
pub const STEEL_FOES_CANT_ESCAPE: u8 = 42;
/// Sound moves don't reach it.
pub const NO_SOUND_MOVES: u8 = 43;
/// Heals a little each turn in rain.
pub const RAIN_HEALS: u8 = 44;
/// Heals a little each turn in hail.
pub const HAIL_HEALS: u8 = 115;
/// Foes' moves aimed at it use an extra PP.
pub const COSTS_EXTRA_PP: u8 = 46;
/// Fire and ice moves hit it at half the attacker's sp. attack.
pub const HALVES_FIRE_ICE: u8 = 47;
/// Sleep counts down twice as fast.
pub const WAKES_FAST: u8 = 48;
/// Contact may burn the attacker (1/3).
pub const CONTACT_BURN: u8 = 49;
/// Its accuracy can't be lowered.
pub const NO_ACCURACY_DROPS: u8 = 51;
/// Its attack can't be lowered.
pub const NO_ATTACK_DROPS: u8 = 52;
/// It loafs every other turn.
pub const LOAFS: u8 = 54;
/// Attack x1.5, but its physical moves are less accurate.
pub const ATTACK_UP_ACCURACY_DOWN: u8 = 55;
/// Contact may infatuate the attacker.
pub const CONTACT_INFATUATE: u8 = 56;
/// Sp. attack x1.5 while a fighter with the other one of the pair is on the field.
pub const PAIR_BOOST_A: u8 = 57;
pub const PAIR_BOOST_B: u8 = 58;
/// Its type follows the weather.
pub const WEATHER_TYPE: u8 = 59;
/// Its status may clear at the end of each turn (1/3).
pub const SHEDS_STATUS: u8 = 61;
/// Attack x1.5 with a major status, and a burn doesn't weaken it.
pub const STATUS_BOOSTS_ATTACK: u8 = 62;
/// Defense x1.5 with a major status.
pub const STATUS_BOOSTS_DEFENSE: u8 = 63;
/// Draining it hurts the drainer instead.
pub const DRAINS_HURT: u8 = 64;
/// Moves of its type x1.5 at 1/3 HP or less: grass, fire, water, bug.
pub const PINCH_GRASS: u8 = 65;
pub const PINCH_FIRE: u8 = 66;
pub const PINCH_WATER: u8 = 67;
pub const PINCH_BUG: u8 = 68;
/// No recoil from its own moves.
pub const NO_RECOIL: u8 = 69;
/// Foes on the ground can't switch out.
pub const GROUNDED_FOES_CANT_ESCAPE: u8 = 71;
/// Harder to hit (half accuracy) while confused.
pub const CONFUSED_EVASION: u8 = 77;
/// Electric moves raise its speed instead of hurting.
pub const ELECTRIC_RAISES_SPEED: u8 = 78;
/// Takes no hail damage, and is harder to hit in hail.
pub const SNOW_SHELTER: u8 = 81;
/// Fire moves hit it at half power, and a burn hurts it half as much.
pub const HALVES_FIRE: u8 = 85;
/// Its stage changes are doubled.
pub const DOUBLE_STAGES: u8 = 86;
/// Punching moves x1.2.
pub const PUNCH_BOOST: u8 = 89;
/// Its same-type bonus is x2 instead of x1.5.
pub const STRONG_SAME_TYPE: u8 = 91;
/// Multi-hit moves always hit five times.
pub const MOST_HITS: u8 = 92;
/// Its status is cured at the end of each turn in rain.
pub const RAIN_CURES: u8 = 93;
/// Speed x1.5 with a major status, and paralysis doesn't slow it.
pub const STATUS_BOOSTS_SPEED: u8 = 95;
/// Its critical hits do another x1.5.
pub const STRONG_CRITS: u8 = 97;
/// Every move it uses or that targets it hits.
pub const ALL_MOVES_HIT: u8 = 99;
/// Moves last within its priority.
pub const MOVES_LAST: u8 = 100;
/// Moves of 60 power or less x1.5.
pub const WEAK_MOVES_BOOST: u8 = 101;
/// Can't get a major status in sun.
pub const SUN_STATUS_GUARD: u8 = 102;
/// One extra critical-hit stage.
pub const CRIT_STAGE_UP: u8 = 105;
/// Ignores the other side's attack and defense stages.
pub const IGNORES_STAGES: u8 = 109;
/// Its not-very-effective hits do double.
pub const RESISTED_BOOST: u8 = 110;
/// Super-effective hits on it do x0.75.
pub const SUPER_EFFECTIVE_GUARD_A: u8 = 111;
pub const SUPER_EFFECTIVE_GUARD_B: u8 = 116;
pub const SUPER_EFFECTIVE_GUARD_C: u8 = 232;
/// Water moves raise its sp. attack instead of hurting.
pub const WATER_RAISES_SP_ATTACK: u8 = 114;
/// Recoil and crash moves x1.2.
pub const RECOIL_BOOST: u8 = 120;
/// Its stage changes are reversed.
pub const INVERTS_STAGES: u8 = 126;
/// Halved attack and sp. attack at half HP or less.
pub const HALF_HP_WEAKENS: u8 = 129;
/// Doubled / halved weight.
pub const DOUBLE_WEIGHT: u8 = 134;
pub const HALF_WEIGHT: u8 = 135;
/// Takes half damage at full HP.
pub const FULL_HP_GUARD_A: u8 = 136;
pub const FULL_HP_GUARD_B: u8 = 231;
/// Attack x1.5 while poisoned.
pub const POISON_BOOSTS_ATTACK: u8 = 137;
/// Sp. attack x1.5 while burned.
pub const BURN_BOOSTS_SP_ATTACK: u8 = 138;
/// Takes no weather damage, and powder moves and spores don't reach it.
pub const WEATHER_POWDER_GUARD: u8 = 142;
/// Heals 1/3 of max HP when it switches out.
pub const HEALS_ON_SWITCH: u8 = 144;
/// Its defense can't be lowered.
pub const NO_DEFENSE_DROPS: u8 = 145;
/// Status moves aimed at it hit at most half the time.
pub const STATUS_MOVES_MISS: u8 = 147;
/// Grass moves raise its attack instead of hurting.
pub const GRASS_RAISES_ATTACK: u8 = 157;
/// Its status moves go first (+1 priority), but don't reach dark-type foes.
pub const STATUS_PRIORITY: u8 = 158;
/// It and its ally hit x1.1 more often.
pub const TEAM_AIM: u8 = 162;
/// Doubled defense.
pub const DOUBLE_DEFENSE: u8 = 169;
/// Its flying moves go first (+1 priority) at full HP.
pub const FLYING_PRIORITY: u8 = 177;
/// Its target-healing pulse heals 3/4 instead of 1/2. Only that part is handled, so it isn't
/// in `implemented`.
pub const PULSE_HEAL_BOOST: u8 = 178;
/// Defense x1.5 on the overgrown field.
pub const OVERGROWN_DEFENSE: u8 = 179;
/// Contact moves x1.3.
pub const CONTACT_BOOST: u8 = 181;
/// Can't be burned, fire moves hit it at half power, and its water moves do double.
pub const FIRE_GUARD_WATER_BOOST: u8 = 199;
/// Steel moves x1.5.
pub const STEEL_BOOST: u8 = 200;
/// Its moves don't make contact.
pub const NO_CONTACT: u8 = 203;
/// Its sound moves become water type.
pub const SOUND_TO_WATER: u8 = 204;
/// Its healing moves go first (+3 priority).
pub const HEAL_PRIORITY: u8 = 205;
/// Doubled speed on the charged field.
pub const CHARGED_FIELD_SPEED: u8 = 207;
/// Foes' priority moves fail against it and its ally.
pub const NO_FOE_PRIORITY_A: u8 = 214;
pub const NO_FOE_PRIORITY_B: u8 = 219;
/// Sets a field on entry: charged, strange, hazy, overgrown.
pub const ENTRY_CHARGED_FIELD: u8 = 226;
pub const ENTRY_STRANGE_FIELD: u8 = 227;
pub const ENTRY_HAZY_FIELD: u8 = 228;
pub const ENTRY_OVERGROWN_FIELD: u8 = 229;
/// Its super-effective hits do x1.25.
pub const SUPER_EFFECTIVE_BOOST: u8 = 233;

/// The handled ids after the unbroken run `PASSIVE_A..=CONFUSED_EVASION`.
const LATER: &[u8] = &[
    ELECTRIC_RAISES_SPEED,
    SNOW_SHELTER,
    PASSIVE_F,
    HALVES_FIRE,
    DOUBLE_STAGES,
    PUNCH_BOOST,
    STRONG_SAME_TYPE,
    MOST_HITS,
    RAIN_CURES,
    STATUS_BOOSTS_SPEED,
    STRONG_CRITS,
    ALL_MOVES_HIT,
    MOVES_LAST,
    WEAK_MOVES_BOOST,
    SUN_STATUS_GUARD,
    PASSIVE_G,
    CRIT_STAGE_UP,
    IGNORES_STAGES,
    RESISTED_BOOST,
    SUPER_EFFECTIVE_GUARD_A,
    WATER_RAISES_SP_ATTACK,
    HAIL_HEALS,
    SUPER_EFFECTIVE_GUARD_B,
    ENTRY_HAIL,
    PASSIVE_H,
    RECOIL_BOOST,
    INVERTS_STAGES,
    HALF_HP_WEAKENS,
    DOUBLE_WEIGHT,
    HALF_WEIGHT,
    FULL_HP_GUARD_A,
    POISON_BOOSTS_ATTACK,
    BURN_BOOSTS_SP_ATTACK,
    WEATHER_POWDER_GUARD,
    HEALS_ON_SWITCH,
    NO_DEFENSE_DROPS,
    SAND_SPEED,
    STATUS_MOVES_MISS,
    GRASS_RAISES_ATTACK,
    STATUS_PRIORITY,
    CONTACT_HURTS_B,
    TEAM_AIM,
    DOUBLE_DEFENSE,
    FLYING_PRIORITY,
    OVERGROWN_DEFENSE,
    CONTACT_BOOST,
    FIRE_GUARD_WATER_BOOST,
    STEEL_BOOST,
    HAIL_SPEED,
    NO_CONTACT,
    SOUND_TO_WATER,
    HEAL_PRIORITY,
    CHARGED_FIELD_SPEED,
    NO_FOE_PRIORITY_A,
    NO_FOE_PRIORITY_B,
    ENTRY_CHARGED_FIELD,
    ENTRY_STRANGE_FIELD,
    ENTRY_HAZY_FIELD,
    ENTRY_OVERGROWN_FIELD,
    NO_STAT_DROPS_C,
    FULL_HP_GUARD_B,
    SUPER_EFFECTIVE_GUARD_C,
    SUPER_EFFECTIVE_BOOST,
];

/// Whether the engine does this ability's own thing (every id but 0).
pub fn implemented(ability: u8) -> bool {
    (PASSIVE_A..=CONFUSED_EVASION).contains(&ability) || LATER.contains(&ability)
}

/// Whether weather does anything with these battlers on the field.
pub fn weather_works(on_field: impl IntoIterator<Item = u8>) -> bool {
    !on_field
        .into_iter()
        .any(|a| a == NO_WEATHER_A || a == NO_WEATHER_B)
}

/// What an ability does when its fighter comes in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Entry {
    Terrain(super::field::Terrain),
    /// Weather that lasts until replaced.
    Weather(Weather),
    /// A stat change on each foe on the field.
    FoesStat(Stat, i8),
    /// Takes a foe's ability.
    CopyFoe,
    /// Its type follows the weather.
    WeatherType,
}

pub fn on_entry(ability: u8) -> Option<Entry> {
    match ability {
        ENTRY_HAIL => Some(Entry::Weather(Weather::Hail)),
        ENTRY_CHARGED_FIELD => Some(Entry::Terrain(super::field::Terrain::Charged)),
        ENTRY_STRANGE_FIELD => Some(Entry::Terrain(super::field::Terrain::Strange)),
        ENTRY_HAZY_FIELD => Some(Entry::Terrain(super::field::Terrain::Hazy)),
        ENTRY_OVERGROWN_FIELD => Some(Entry::Terrain(super::field::Terrain::Overgrown)),
        ENTRY_RAIN => Some(Entry::Weather(Weather::Rain)),
        ENTRY_SAND => Some(Entry::Weather(Weather::Sand)),
        ENTRY_SUN => Some(Entry::Weather(Weather::Sun)),
        ENTRY_LOWERS_ATTACK => Some(Entry::FoesStat(Stat::Attack, -1)),
        COPIES_FOE_ABILITY => Some(Entry::CopyFoe),
        WEATHER_TYPE => Some(Entry::WeatherType),
        _ => None,
    }
}

/// The type a weather-following fighter takes.
pub fn weather_type(weather: Weather) -> u8 {
    match weather {
        Weather::Sun => types::FIRE,
        Weather::Rain => types::WATER,
        Weather::Hail => types::ICE,
        _ => types::NORMAL,
    }
}

/// What an ability does at the end of each turn.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EndOfTurn {
    /// Heals 1/16 of max HP.
    Heal16,
    /// One stage up.
    Raise(Stat),
    /// A 1/3 chance of curing its status.
    MayShed,
}

/// `fresh`: it came in during this turn (stat raises wait a turn).
pub fn end_of_turn(ability: u8, weather: Weather, fresh: bool) -> Option<EndOfTurn> {
    match ability {
        HAIL_HEALS if weather == Weather::Hail => Some(EndOfTurn::Heal16),
        RAIN_HEALS if weather == Weather::Rain => Some(EndOfTurn::Heal16),
        SPEED_EACH_TURN if !fresh => Some(EndOfTurn::Raise(Stat::Speed)),
        SHEDS_STATUS => Some(EndOfTurn::MayShed),
        _ => None,
    }
}

pub fn prevents_crits(ability: u8) -> bool {
    matches!(ability, NO_CRITS_A | NO_CRITS_B)
}

/// Whether moves of `move_type` don't reach a fighter with `ability` at all.
pub fn immune_to_type(ability: u8, move_type: u8) -> bool {
    ability == IMMUNE_TO_GROUND && move_type == types::GROUND
}

/// What a fighter's ability does with a move of this type aimed at it, instead of being hit.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Absorb {
    Stat(Stat, i8),
    /// Heals 1/4 of max HP (or nothing at full HP).
    Heal,
    /// Its fire moves get stronger.
    FireBoost,
}

/// `damaging`: the move has power (healing abilities skip status moves).
pub fn absorbs(ability: u8, move_type: u8, _damaging: bool, _frozen: bool) -> Option<Absorb> {
    match (ability, move_type) {
        (ELECTRIC_HEALS, types::ELECTRIC) | (WATER_HEALS, types::WATER) => Some(Absorb::Heal),
        (DRAWS_ELECTRIC, types::ELECTRIC) | (WATER_RAISES_SP_ATTACK, types::WATER) => {
            Some(Absorb::Stat(Stat::SpAttack, 1))
        }
        (ELECTRIC_RAISES_SPEED, types::ELECTRIC) => Some(Absorb::Stat(Stat::Speed, 1)),
        (GRASS_RAISES_ATTACK, types::GRASS) => Some(Absorb::Stat(Stat::Attack, 1)),
        (FIRE_ABSORB_BOOST, types::FIRE) => Some(Absorb::FireBoost),
        _ => None,
    }
}

pub fn prevents_ailment(ability: u8, ailment: Ailment) -> bool {
    if ability == FIRE_GUARD_WATER_BOOST && ailment == Ailment::Burn {
        return true;
    }
    matches!(
        (ability, ailment),
        (NO_PARALYSIS, Ailment::Paralysis)
            | (NO_SLEEP_A | NO_SLEEP_B, Ailment::Sleep)
            | (NO_POISON, Ailment::Poison | Ailment::BadPoison)
            | (NO_FREEZE, Ailment::Freeze)
            | (NO_BURN, Ailment::Burn)
    )
}

pub fn prevents_confusion(ability: u8) -> bool {
    ability == NO_CONFUSION
}

pub fn prevents_flinch(ability: u8) -> bool {
    ability == NO_FLINCH
}

pub fn blocks_added_effects(ability: u8) -> bool {
    ability == NO_ADDED_EFFECTS
}

/// Whether another fighter's move or ability can lower this stat.
pub fn prevents_drop(ability: u8, stat: Stat) -> bool {
    matches!(ability, NO_STAT_DROPS_A | NO_STAT_DROPS_B | NO_STAT_DROPS_C)
        || (ability == NO_DEFENSE_DROPS && stat == Stat::Defense)
        || (ability == NO_ATTACK_DROPS && stat == Stat::Attack)
        || (ability == NO_ACCURACY_DROPS && stat == Stat::Accuracy)
}

/// Whether a foe with `ability` keeps `f` (on the ground unless flying or levitating) from
/// switching.
pub fn traps(ability: u8, f: &Fighter) -> bool {
    match ability {
        FOES_CANT_ESCAPE => true,
        STEEL_FOES_CANT_ESCAPE => f.has_type(types::STEEL),
        GROUNDED_FOES_CANT_ESCAPE => !f.has_type(types::FLYING) && f.ability != IMMUNE_TO_GROUND,
        _ => false,
    }
}

/// Speed multiplier in this weather (weather already checked to work).
pub fn speed_factor(ability: u8, weather: Weather) -> u32 {
    match (ability, weather) {
        (RAIN_SPEED, Weather::Rain)
        | (SUN_SPEED, Weather::Sun)
        | (SAND_SPEED, Weather::Sand)
        | (HAIL_SPEED, Weather::Hail) => 2,
        _ => 1,
    }
}

/// An added effect's percent chance for a user with `ability`.
pub fn effect_chance(ability: u8, chance: u8) -> u32 {
    let c = u32::from(chance);
    if ability == DOUBLE_CHANCES { c * 2 } else { c }
}

/// The hit chance after the user's and target's abilities.
pub fn accuracy(
    calc: u32,
    user: &Fighter,
    target: &Fighter,
    category: super::data::Category,
    weather: Weather,
) -> u32 {
    let mut calc = calc;
    if user.ability == SURE_AIM {
        calc = calc * 130 / 100;
    }
    if target.ability == SAND_SHELTER && weather == Weather::Sand {
        calc = calc * 80 / 100;
    }
    if target.ability == SNOW_SHELTER && weather == Weather::Hail {
        calc = calc * 80 / 100;
    }
    if target.ability == CONFUSED_EVASION && target.volatile.confusion > 0 {
        calc /= 2;
    }
    if target.ability == STATUS_MOVES_MISS && category == super::data::Category::Status {
        calc = calc.min(50);
    }
    if user.ability == TEAM_AIM {
        calc = calc * 110 / 100;
    }
    if user.ability == ATTACK_UP_ACCURACY_DOWN && category == super::data::Category::Physical {
        calc = calc * 80 / 100;
    }
    calc
}

/// The values the damage formula works from, which abilities may change.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Mods {
    pub attack: u32,
    pub defense: u32,
    pub sp_attack: u32,
    pub sp_defense: u32,
    pub power: u32,
    /// x1.5 at the very end of a special move's damage.
    pub boost_final: bool,
}

/// Abilities' (and the field's) changes to the damage formula's inputs (before stages), in the
/// catalog's order.
pub fn damage_mods(m: &mut Mods, a: &Attack) {
    let (user, target, t) = (a.user, a.target, a.move_type);
    if matches!(user.ability, DOUBLE_ATTACK_A | DOUBLE_ATTACK_B) {
        m.attack *= 2;
    }
    if target.ability == HALVES_FIRE_ICE && (t == types::FIRE || t == types::ICE) {
        m.attack /= 2;
        m.sp_attack /= 2;
    }
    if user.ability == ATTACK_UP_ACCURACY_DOWN {
        m.attack = m.attack * 150 / 100;
    }
    if a.paired && matches!(user.ability, PAIR_BOOST_A | PAIR_BOOST_B) {
        m.sp_attack = m.sp_attack * 150 / 100;
    }
    if user.ability == STATUS_BOOSTS_ATTACK && user.ailment.is_some() {
        m.attack = m.attack * 150 / 100;
    }
    if target.ability == STATUS_BOOSTS_DEFENSE && target.ailment.is_some() {
        m.defense = m.defense * 150 / 100;
    }
    if (t == types::ELECTRIC && a.dull_electric) || (t == types::FIRE && a.dull_fire) {
        m.power /= 2;
    }
    let pinch = match user.ability {
        PINCH_GRASS => types::GRASS,
        PINCH_FIRE => types::FIRE,
        PINCH_WATER => types::WATER,
        PINCH_BUG => types::BUG,
        _ => types::TYPELESS,
    };
    if pinch == t && u32::from(user.hp) <= u32::from(user.stats.hp) / 3 {
        m.power = m.power * 150 / 100;
    }
    if user.volatile.fire_boost && t == types::FIRE {
        m.boost_final = true;
    }
    if user.ability == WEAK_MOVES_BOOST && m.power <= 60 {
        m.power = m.power * 3 / 2;
    }
    if user.ability == PUNCH_BOOST && a.flags & super::data::flags::PUNCH != 0 {
        m.power = m.power * 6 / 5;
    }
    if user.ability == CONTACT_BOOST && a.flags & super::data::flags::CONTACT != 0 {
        m.power = m.power * 13 / 10;
    }
    if user.ability == RECOIL_BOOST && (a.recoil || a.effect == super::effects::CRASH_ON_MISS) {
        m.power = m.power * 6 / 5;
    }
    if user.ability == HALF_HP_WEAKENS && user.hp <= user.stats.hp / 2 {
        m.attack /= 2;
        m.sp_attack /= 2;
    }
    if user.ability == POISON_BOOSTS_ATTACK
        && matches!(user.ailment, Some(Ailment::Poison | Ailment::BadPoison))
    {
        m.attack = m.attack * 3 / 2;
    }
    if user.ability == BURN_BOOSTS_SP_ATTACK && user.ailment == Some(Ailment::Burn) {
        m.sp_attack = m.sp_attack * 3 / 2;
    }
    if target.ability == DOUBLE_DEFENSE {
        m.defense *= 2;
    }
    if target.ability == OVERGROWN_DEFENSE && a.terrain == super::field::Terrain::Overgrown {
        m.defense = m.defense * 3 / 2;
    }
    if target.ability == HALVES_FIRE && t == types::FIRE {
        m.power /= 2;
    }
    if target.ability == FIRE_GUARD_WATER_BOOST && t == types::FIRE {
        m.power /= 2;
    }
    if user.ability == FIRE_GUARD_WATER_BOOST && t == types::WATER {
        m.power *= 2;
    }
    if user.ability == STEEL_BOOST && t == types::STEEL {
        m.power = m.power * 3 / 2;
    }
}

/// Whether a burn leaves this fighter's physical damage alone.
pub fn ignores_burn(ability: u8) -> bool {
    ability == STATUS_BOOSTS_ATTACK
}

/// Whether this weather's damage passes this fighter by (on top of the type rule).
pub fn ignores_weather_damage(ability: u8, weather: Weather) -> bool {
    ability == WEATHER_POWDER_GUARD
        || (ability == SNOW_SHELTER && weather == Weather::Hail)
        || (ability == SAND_SHELTER && weather == Weather::Sand)
}

/// What touching a fighter with this ability may do to the attacker.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Contact {
    /// One in `n` chance of this status.
    Ailment(Ailment, u32),
    /// One in 10: sleep, poison or paralysis.
    Spores,
    /// Loses 1/16 of its max HP.
    Hurt,
}

pub fn on_contact(ability: u8) -> Option<Contact> {
    match ability {
        CONTACT_PARALYZE => Some(Contact::Ailment(Ailment::Paralysis, 3)),
        CONTACT_POISON => Some(Contact::Ailment(Ailment::Poison, 3)),
        CONTACT_BURN => Some(Contact::Ailment(Ailment::Burn, 3)),
        CONTACT_SPORES => Some(Contact::Spores),
        CONTACT_HURTS_A | CONTACT_HURTS_B => Some(Contact::Hurt),
        _ => None,
    }
}

/// Whether the ability cures `ailment` (or confusion, with `None`) the moment its holder has
/// both, as after taking an ability.
pub fn cures(ability: u8, ailment: Option<Ailment>) -> bool {
    match ailment {
        Some(a) => prevents_ailment(ability, a),
        None => prevents_confusion(ability),
    }
}

/// Abilities a copying move can't take.
pub fn uncopyable(ability: u8) -> bool {
    ability == NONE || ability == ONLY_SUPER_EFFECTIVE
}

pub fn blocks_sound(ability: u8) -> bool {
    ability == NO_SOUND_MOVES
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn coverage() {
        assert!((1..=76u8).all(implemented));
        assert!(implemented(CONFUSED_EVASION) && implemented(ENTRY_OVERGROWN_FIELD));
        assert!(LATER.iter().all(|&a| implemented(a) && a < COUNT));
        assert!(!implemented(PULSE_HEAL_BOOST));
        assert!(!implemented(209) && !implemented(COUNT));
    }

    #[test]
    fn trapping_rules() {
        let mut f = Fighter::new("a", 50, Default::default(), [types::NORMAL; 2], 0, &[]);
        assert!(traps(FOES_CANT_ESCAPE, &f));
        assert!(!traps(STEEL_FOES_CANT_ESCAPE, &f));
        assert!(traps(GROUNDED_FOES_CANT_ESCAPE, &f));
        f.types = [types::STEEL, types::FLYING];
        assert!(traps(STEEL_FOES_CANT_ESCAPE, &f));
        assert!(!traps(GROUNDED_FOES_CANT_ESCAPE, &f));
        // Even a fighter with the same ability.
        f.ability = FOES_CANT_ESCAPE;
        assert!(traps(FOES_CANT_ESCAPE, &f));
    }

    #[test]
    fn drop_guards_and_absorbs() {
        assert!(prevents_drop(NO_STAT_DROPS_A, Stat::Speed));
        assert!(prevents_drop(NO_ATTACK_DROPS, Stat::Attack));
        assert!(!prevents_drop(NO_ATTACK_DROPS, Stat::Defense));
        assert!(prevents_drop(NO_ACCURACY_DROPS, Stat::Accuracy));
        assert_eq!(
            absorbs(ELECTRIC_HEALS, types::ELECTRIC, true, false),
            Some(Absorb::Heal)
        );
        // A status move of the type gets through.
        assert_eq!(
            absorbs(ELECTRIC_HEALS, types::ELECTRIC, false, false),
            Some(Absorb::Heal)
        );
        assert_eq!(
            absorbs(FIRE_ABSORB_BOOST, types::FIRE, false, false),
            Some(Absorb::FireBoost)
        );
        assert_eq!(
            absorbs(FIRE_ABSORB_BOOST, types::FIRE, true, true),
            Some(Absorb::FireBoost)
        );
    }
}
