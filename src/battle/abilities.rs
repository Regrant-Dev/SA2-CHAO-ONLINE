//! Ability hooks: each point in a turn where an ability can step in asks here. A fighter has its
//! partner's ability id (0 = none).
//!
//! The ROM has 78 ability ids, 0..=77 (0 is none). All 77 abilities are handled, six of them
//! by doing nothing in battle, as in the ROM (`PASSIVE_*`). Two only work against
//! fighters with a gender, which none have here (`NO_INFATUATION`, `CONTACT_INFATUATE`), and
//! `WEATHER_TYPE` changes any holder's type with the weather. The sound-move blocker
//! (`NO_SOUND_MOVES`) needs the ROM's sound move list (`Battle::set_sound_moves`).

use super::damage::Attack;
use super::data::types;
use super::field::Weather;
use super::fighter::{Ailment, Fighter, Stat};

/// Ability ids 0..COUNT.
pub const COUNT: u8 = 78;

pub const NONE: u8 = 0;
/// No effect in battle.
pub const PASSIVE_A: u8 = 1;
pub const PASSIVE_B: u8 = 35;
pub const PASSIVE_C: u8 = 50;
pub const PASSIVE_D: u8 = 53;
pub const PASSIVE_E: u8 = 60;
pub const PASSIVE_F: u8 = 76;
/// Sets lasting weather on entry.
pub const ENTRY_RAIN: u8 = 2;
pub const ENTRY_SAND: u8 = 45;
pub const ENTRY_SUN: u8 = 70;
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
pub const NO_WEATHER_B: u8 = 77;
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
pub const CONTACT_HURTS: u8 = 24;
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
/// Its status is cured when it switches out.
pub const CURED_ON_SWITCH: u8 = 30;
/// Draws foes' single-target electric moves to itself (doubles).
pub const DRAWS_ELECTRIC: u8 = 31;
/// Doubles its moves' added-effect chances.
pub const DOUBLE_CHANCES: u8 = 32;
/// Doubled speed in rain / in sun.
pub const RAIN_SPEED: u8 = 33;
pub const SUN_SPEED: u8 = 34;
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

/// Whether the engine does this ability's own thing (every id but 0).
pub fn implemented(ability: u8) -> bool {
    ability != NONE && ability < COUNT
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
    /// Heals 1/4 of max HP (or nothing at full HP).
    Heal,
    /// Its fire moves get stronger.
    FireBoost,
}

/// `damaging`: the move has power (healing abilities skip status moves).
pub fn absorbs(ability: u8, move_type: u8, damaging: bool, frozen: bool) -> Option<Absorb> {
    match (ability, move_type) {
        (ELECTRIC_HEALS, types::ELECTRIC) | (WATER_HEALS, types::WATER) if damaging => {
            Some(Absorb::Heal)
        }
        (FIRE_ABSORB_BOOST, types::FIRE) if !frozen => Some(Absorb::FireBoost),
        _ => None,
    }
}

pub fn prevents_ailment(ability: u8, ailment: Ailment) -> bool {
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
    matches!(ability, NO_STAT_DROPS_A | NO_STAT_DROPS_B)
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
        (RAIN_SPEED, Weather::Rain) | (SUN_SPEED, Weather::Sun) => 2,
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
    move_type: u8,
    weather: Weather,
) -> u32 {
    let mut calc = calc;
    if user.ability == SURE_AIM {
        calc = calc * 130 / 100;
    }
    if target.ability == SAND_SHELTER && weather == Weather::Sand {
        calc = calc * 80 / 100;
    }
    if user.ability == ATTACK_UP_ACCURACY_DOWN && types::is_physical(move_type) {
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
/// ROM's order.
pub fn damage_mods(m: &mut Mods, a: &Attack) {
    let (user, target, t) = (a.user, a.target, a.move_type);
    if matches!(user.ability, DOUBLE_ATTACK_A | DOUBLE_ATTACK_B) {
        m.attack *= 2;
    }
    if target.ability == HALVES_FIRE_ICE && (t == types::FIRE || t == types::ICE) {
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
}

/// Whether a burn leaves this fighter's physical damage alone.
pub fn ignores_burn(ability: u8) -> bool {
    ability == STATUS_BOOSTS_ATTACK
}

/// Whether this weather's damage passes this fighter by (on top of the type rule).
pub fn ignores_weather_damage(ability: u8, weather: Weather) -> bool {
    ability == SAND_SHELTER && weather == Weather::Sand
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
        CONTACT_HURTS => Some(Contact::Hurt),
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
        assert_eq!((0..=255u8).filter(|&a| implemented(a)).count(), 77);
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
        assert_eq!(absorbs(ELECTRIC_HEALS, types::ELECTRIC, false, false), None);
        assert_eq!(
            absorbs(FIRE_ABSORB_BOOST, types::FIRE, false, false),
            Some(Absorb::FireBoost)
        );
        assert_eq!(absorbs(FIRE_ABSORB_BOOST, types::FIRE, true, true), None);
    }
}
