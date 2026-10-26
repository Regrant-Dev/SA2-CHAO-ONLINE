//! Ordinary damage with explicit per-move category and deterministic integer modifiers.

use super::abilities::{self, Mods};
use super::data::{Category, TypeChart, types};
use super::effects;
use super::field::Weather;
use super::fighter::{Fighter, Stat, staged};

/// One attack on one target.
#[derive(Clone, Copy, Debug)]
pub struct Attack<'a> {
    pub user: &'a Fighter,
    pub target: &'a Fighter,
    pub move_type: u8,
    pub category: Category,
    pub move_id: u16,
    pub terrain: super::field::Terrain,
    pub flags: u32,
    pub grounded_override: bool,
    pub power: u32,
    /// The move's effect id (a self-fainting move halves the target's defense).
    pub effect: u8,
    /// The move hurts its user by a share of the damage it deals.
    pub recoil: bool,
    pub crit: bool,
    /// The weather in force (`Clear` when something cancels it).
    pub weather: Weather,
    /// The target's side has the screen for this move's kind up.
    pub screen: bool,
    /// Doubles with both of the target's side up (screens give 2/3 instead of 1/2).
    pub crowded: bool,
    /// A move hitting more than one fighter in doubles: 3/4 damage.
    pub spread: bool,
    /// The move's own multiplier (2 on a target it reaches in hiding, a shrunk target...),
    /// applied with the critical hit.
    pub mult: u32,
    /// The user powered up its electric moves.
    pub charged: bool,
    /// An ally's helping move.
    pub helped: bool,
    /// A fighter with the other pair-boost ability is on the field.
    pub paired: bool,
    /// A fighter on the field is dulling electric / fire moves.
    pub dull_electric: bool,
    pub dull_fire: bool,
}

impl<'a> Attack<'a> {
    /// A plain attack: no weather, screens, spread or field effects.
    pub fn new(user: &'a Fighter, target: &'a Fighter, move_type: u8, power: u32) -> Attack<'a> {
        Attack {
            user,
            target,
            move_type,
            category: if types::is_physical(move_type) {
                Category::Physical
            } else {
                Category::Special
            },
            move_id: 0,
            terrain: super::field::Terrain::None,
            flags: 0,
            grounded_override: false,
            power,
            effect: 0,
            recoil: false,
            crit: false,
            weather: Weather::Clear,
            screen: false,
            crowded: false,
            spread: false,
            mult: 1,
            charged: false,
            helped: false,
            paired: false,
            dull_electric: false,
            dull_fire: false,
        }
    }
}

/// Base damage before the spread/weather/critical/random/type/burn modifiers.
pub fn base(a: &Attack) -> i32 {
    let mut m = Mods {
        attack: u32::from(a.user.stats.attack),
        defense: u32::from(a.target.stats.defense),
        sp_attack: u32::from(a.user.stats.sp_attack),
        sp_defense: u32::from(a.target.stats.sp_defense),
        power: a.power,
        boost_final: false,
    };
    if a.flags & super::data::flags::TARGET_ATTACK != 0 {
        m.attack = u32::from(a.target.stats.attack);
    }
    if a.flags & super::data::flags::SWAP_DEFENSE_STATS != 0 {
        std::mem::swap(&mut m.defense, &mut m.sp_defense);
    }
    abilities::damage_mods(&mut m, a);
    use super::field::Terrain;
    if a.user.grounded()
        && matches!(
            (a.terrain, a.move_type),
            (Terrain::Charged, types::ELECTRIC)
                | (Terrain::Overgrown, types::GRASS)
                | (Terrain::Strange, types::PSYCHIC)
        )
    {
        m.power = m.power * 3 / 2;
    }
    if a.target.grounded()
        && ((a.terrain == Terrain::Hazy && a.move_type == types::DRAGON)
            || (a.terrain == Terrain::Overgrown && matches!(a.move_id, 89 | 222 | 523)))
    {
        m.power /= 2;
    }
    if a.effect == effects::SUN_CHARGE
        && matches!(a.weather, Weather::Rain | Weather::Sand | Weather::Hail)
    {
        m.power /= 2;
    }
    if m.boost_final {
        m.power = m.power * 3 / 2;
    }
    let level = i64::from(2 * u32::from(a.user.level) / 5 + 2);
    let damage = match a.category {
        Category::Physical => side_damage(
            a,
            m.attack,
            m.defense,
            m.power,
            Stat::Attack,
            Stat::Defense,
            level,
        ),
        Category::Special if a.flags & super::data::flags::TARGET_DEFENSE != 0 => side_damage(
            a,
            m.sp_attack,
            m.defense,
            m.power,
            Stat::SpAttack,
            Stat::Defense,
            level,
        ),
        Category::Special => side_damage(
            a,
            m.sp_attack,
            m.sp_defense,
            m.power,
            Stat::SpAttack,
            Stat::SpDefense,
            level,
        ),
        Category::Status => return 0,
    };
    (damage + 2).min(i64::from(i32::MAX)) as i32
}

/// `stat * power * level_factor / defense / 50`, with stages. A critical hit ignores the
/// user's lowered stages and the target's raised ones.
fn side_damage(
    a: &Attack,
    attack: u32,
    defense: u32,
    power: u32,
    atk_stat: Stat,
    def_stat: Stat,
    level: i64,
) -> i64 {
    let atk_stage = if a.target.ability == abilities::IGNORES_STAGES {
        0
    } else {
        if a.flags & super::data::flags::TARGET_ATTACK != 0 {
            a.target.stage(atk_stat)
        } else {
            a.user.stage(atk_stat)
        }
    };
    let def_stage = if a.user.ability == abilities::IGNORES_STAGES
        || a.flags & super::data::flags::IGNORE_DEFENSE_STAGES != 0
    {
        0
    } else {
        a.target.stage(def_stat)
    };
    let atk = if a.crit && atk_stage <= 0 {
        attack
    } else {
        staged(attack, atk_stage)
    };
    let def = if a.crit && def_stage >= 0 {
        defense
    } else {
        staged(defense, def_stage)
    };
    i64::from(atk) * i64::from(power) * level / i64::from(def.max(1)) / 50
}

/// Cartridge-style nearest rounding, with exact halves rounded down.
fn rounded(value: i64, numerator: i64, denominator: i64) -> i64 {
    (value * numerator * 2 + denominator - 1) / (denominator * 2)
}

/// Modifier order: spread, weather, critical, random, STAB, effectiveness, burn,
/// then screens. The Chao format's explicit move/field multipliers are applied as well.
pub fn finish(base: i32, a: &Attack, chart: Option<&TypeChart>, random: u32) -> (i32, u32) {
    let mut d = i64::from(base).clamp(0, DAMAGE_CAP);
    if a.spread {
        d = rounded(d, 3, 4);
    }
    match (a.weather, a.move_type) {
        (Weather::Rain, types::FIRE) | (Weather::Sun, types::WATER) => d = rounded(d, 1, 2),
        (Weather::Rain, types::WATER) | (Weather::Sun, types::FIRE) => d = rounded(d, 3, 2),
        _ => {}
    }
    if a.crit {
        d = d * 3 / 2;
        if a.user.ability == abilities::STRONG_CRITS {
            d = rounded(d, 3, 2);
        }
    }
    d = d * (100 - i64::from(random.min(15))) / 100;
    d *= i64::from(a.mult.max(1));
    if a.charged && a.move_type == types::ELECTRIC {
        d *= 2;
    }
    if a.helped {
        d = rounded(d, 3, 2);
    }
    let mut multiplier = 100;
    if let Some(chart) = chart {
        if a.user.has_type(a.move_type) {
            d = if a.user.ability == abilities::STRONG_SAME_TYPE {
                d * 2
            } else {
                rounded(d, 3, 2)
            };
        }
        let types = a.target.battle_types().map(|t| {
            if a.grounded_override && a.move_type == types::GROUND && t == types::FLYING {
                types::TYPELESS
            } else {
                t
            }
        });
        multiplier = chart.multiplier(a.move_type, types, a.target.volatile.identified);
        d = d * i64::from(multiplier) / 100;
    }
    if multiplier == 0 {
        return (0, 0);
    }
    if multiplier < 100 && a.user.ability == abilities::RESISTED_BOOST {
        d *= 2;
    }
    if multiplier > 100
        && matches!(
            a.target.ability,
            abilities::SUPER_EFFECTIVE_GUARD_A
                | abilities::SUPER_EFFECTIVE_GUARD_B
                | abilities::SUPER_EFFECTIVE_GUARD_C
        )
    {
        d = rounded(d, 3, 4);
    }
    if multiplier > 100 && a.user.ability == abilities::SUPER_EFFECTIVE_BOOST {
        d = rounded(d, 5, 4);
    }
    if matches!(
        a.target.ability,
        abilities::FULL_HP_GUARD_A | abilities::FULL_HP_GUARD_B
    ) && a.target.hp == a.target.stats.hp
    {
        d = rounded(d, 1, 2);
    }
    if a.category == Category::Physical
        && a.user.ailment == Some(super::fighter::Ailment::Burn)
        && !abilities::ignores_burn(a.user.ability)
        && a.effect != effects::STATUS_POWER
    {
        d /= 2;
    }
    if a.screen && !a.crit {
        d = if a.crowded {
            rounded(d, 2732, 4096)
        } else {
            rounded(d, 1, 2)
        };
    }
    (d.clamp(1, DAMAGE_CAP) as i32, multiplier)
}

/// The most damage the steps after base damage work with.
const DAMAGE_CAP: i64 = 10_000_000;

/// The damage of a confused fighter hitting itself: a 40-power physical hit on its own
/// defense with the random roll (`random` 0..=15, as in `finish`), without critical or type.
pub fn confusion(user: &Fighter, random: u32) -> i32 {
    let d = base(&Attack::new(user, user, types::NORMAL, CONFUSION_POWER));
    if d <= 0 {
        return d;
    }
    (d * (100 - random.min(15) as i32) / 100).max(1)
}

pub const CONFUSION_POWER: u32 = 40;

/// Critical-hit odds in 1/4096ths (1/24, 1/8, 1/2, guaranteed), plus the
/// Chao format's existing luck extension of up to another 1/16, capped at certainty.
pub fn crit_chance(stage: u8, luck: u8) -> u32 {
    const ODDS: [u32; 4] = [24, 8, 2, 1];
    (4096 / ODDS[usize::from(stage.min(3))] + u32::from(luck) * 256 / 255).min(4096)
}

#[cfg(test)]
mod tests {
    use super::super::data::tests::chart;
    use super::super::data::types::*;
    use super::super::fighter::{Ailment, Stats};
    use super::*;

    fn fighter(types: [u8; 2], s: u16) -> Fighter {
        let stats = Stats {
            hp: 200,
            attack: s,
            defense: s,
            speed: s,
            sp_attack: s,
            sp_defense: s,
        };
        Fighter::new("x", 50, stats, types, 0, &[])
    }

    fn attack<'a>(user: &'a Fighter, target: &'a Fighter, move_type: u8, power: u32) -> Attack<'a> {
        Attack::new(user, target, move_type, power)
    }

    #[test]
    fn base_damage_matches_hand_values() {
        // L50, 100 attack vs 100 defense, 80 power: 100*80*22/100/50 = 35, +2 = 37.
        let u = fighter([NORMAL, NORMAL], 100);
        let t = fighter([NORMAL, NORMAL], 100);
        let a = attack(&u, &t, NORMAL, 80);
        assert_eq!(base(&a), 37);
        // Special side the same with equal stats.
        assert_eq!(base(&attack(&u, &t, WATER, 80)), 37);
        // 150 attack vs 80 defense, 120 power: 150*120*22/80/50 = 99, +2.
        let u = fighter([NORMAL, NORMAL], 150);
        let t = fighter([NORMAL, NORMAL], 80);
        assert_eq!(base(&attack(&u, &t, FIGHTING, 120)), 101);
        // A typeless move still uses its explicit physical/special damage category.
        assert_eq!(base(&attack(&u, &t, TYPELESS, 120)), 101);
    }

    #[test]
    fn finish_applies_stab_type_and_random_in_order() {
        let u = fighter([FIRE, FIRE], 100);
        let t = fighter([GRASS, GRASS], 100);
        let c = chart();
        let a = attack(&u, &t, FIRE, 80);
        // 37 * 1.5 = 55, * 2 = 110, * 100%.
        assert_eq!(finish(37, &a, Some(&c), 0), (110, 200));
        // Random comes before STAB: floor(37*.85)=31, round-down-half(31*1.5)=46, then *2.
        assert_eq!(finish(37, &a, Some(&c), 15).0, 92);
        // Critical is 1.5x: 37 -> 55 -> 82 -> 164.
        let crit = Attack { crit: true, ..a };
        assert_eq!(finish(37, &crit, Some(&c), 0).0, 164);
        // No chart (the out-of-PP move): neither STAB nor type.
        assert_eq!(finish(37, &a, None, 0), (37, 100));
        // Immune: 0 stays 0 through the roll.
        let g = fighter([GROUND, GROUND], 100);
        let e = attack(&u, &g, ELECTRIC, 80);
        assert_eq!(finish(37, &e, Some(&c), 7), (0, 0));
    }

    #[test]
    fn stages_burn_and_crit_rules() {
        let mut u = fighter([NORMAL, NORMAL], 100);
        let mut t = fighter([NORMAL, NORMAL], 100);
        u.change_stage(Stat::Attack, 2);
        // 200*80*22/100/50 = 70, +2.
        assert_eq!(base(&attack(&u, &t, NORMAL, 80)), 72);
        t.change_stage(Stat::Defense, 2);
        assert_eq!(base(&attack(&u, &t, NORMAL, 80)), 37);
        // A crit ignores the target's raised defense but keeps the user's raised attack.
        let crit = Attack {
            crit: true,
            ..attack(&u, &t, NORMAL, 80)
        };
        assert_eq!(base(&crit), 72);
        // ...and ignores the user's lowered attack.
        u.change_stage(Stat::Attack, -4);
        t.change_stage(Stat::Defense, -2);
        let crit = Attack {
            crit: true,
            ..attack(&u, &t, NORMAL, 80)
        };
        assert_eq!(base(&crit), 37);
        // Burn is a final physical modifier, after the base +2.
        let mut b = fighter([NORMAL, NORMAL], 100);
        b.ailment = Some(Ailment::Burn);
        let t = fighter([NORMAL, NORMAL], 100);
        let burned = attack(&b, &t, NORMAL, 80);
        assert_eq!(base(&burned), 37);
        assert_eq!(finish(base(&burned), &burned, None, 0).0, 18);
        assert_eq!(base(&attack(&b, &t, FIRE, 80)), 37);
    }

    #[test]
    fn screens_spread_and_weather() {
        let u = fighter([NORMAL; 2], 100);
        let t = fighter([NORMAL; 2], 100);
        let a = attack(&u, &t, NORMAL, 80);
        let result = |a: Attack| finish(base(&a), &a, None, 0).0;
        assert_eq!(result(Attack { screen: true, ..a }), 18);
        assert_eq!(
            result(Attack {
                screen: true,
                crowded: true,
                ..a
            }),
            25
        );
        assert_eq!(
            result(Attack {
                screen: true,
                crit: true,
                ..a
            }),
            55
        );
        assert_eq!(result(Attack { spread: true, ..a }), 28);
        let water = attack(&u, &t, WATER, 80);
        assert_eq!(
            result(Attack {
                weather: Weather::Rain,
                ..water
            }),
            55
        );
        assert_eq!(
            result(Attack {
                weather: Weather::Sun,
                ..water
            }),
            18
        );
        let fire = attack(&u, &t, FIRE, 80);
        assert_eq!(
            result(Attack {
                weather: Weather::Sun,
                ..fire
            }),
            55
        );
        assert_eq!(
            result(Attack {
                weather: Weather::Sand,
                ..fire
            }),
            37
        );
        // Weather applies to physical fire moves too, independent of their type category.
        assert_eq!(
            result(Attack {
                category: Category::Physical,
                weather: Weather::Sun,
                ..fire
            }),
            55
        );
    }

    #[test]
    fn self_faint_keeps_defense_and_abilities_apply() {
        let mut u = fighter([NORMAL, NORMAL], 100);
        let t = fighter([NORMAL, NORMAL], 100);
        let a = Attack {
            effect: effects::SELF_FAINT,
            ..attack(&u, &t, NORMAL, 80)
        };
        assert_eq!(base(&a), 37);
        u.ability = abilities::DOUBLE_ATTACK_A;
        assert_eq!(base(&attack(&u, &t, NORMAL, 80)), 72);
    }

    #[test]
    fn ability_and_field_modifiers() {
        let mut u = fighter([NORMAL, NORMAL], 100);
        let mut t = fighter([NORMAL, NORMAL], 100);
        // Fire into a fire-and-ice halver: sp. attack 50 -> 50*80*22/100/50 = 17, +2.
        t.ability = abilities::HALVES_FIRE_ICE;
        assert_eq!(base(&attack(&u, &t, FIRE, 80)), 19);
        assert_eq!(base(&attack(&u, &t, WATER, 80)), 37);
        t.ability = 0;
        // Defense x1.5 with a status: 100*80*22/150/50 = 23, +2.
        t.ability = abilities::STATUS_BOOSTS_DEFENSE;
        t.ailment = Some(Ailment::Poison);
        assert_eq!(base(&attack(&u, &t, NORMAL, 80)), 25);
        let t = fighter([NORMAL, NORMAL], 100);
        // Attack x1.5.
        u.ability = abilities::ATTACK_UP_ACCURACY_DOWN;
        assert_eq!(base(&attack(&u, &t, NORMAL, 80)), 54);
        // Pinch: power x1.5 at a third of HP.
        u.ability = abilities::PINCH_FIRE;
        assert_eq!(base(&attack(&u, &t, FIRE, 80)), 37);
        u.hp = 66;
        assert_eq!(base(&attack(&u, &t, FIRE, 80)), 54);
        // Pair boost only with the other one on the field.
        u.ability = abilities::PAIR_BOOST_A;
        assert_eq!(base(&attack(&u, &t, FIRE, 80)), 37);
        let paired = Attack {
            paired: true,
            ..attack(&u, &t, FIRE, 80)
        };
        assert_eq!(base(&paired), 54);
        // Dulled fire: half power.
        let dulled = Attack {
            dull_fire: true,
            ..attack(&u, &t, FIRE, 80)
        };
        assert_eq!(base(&dulled), 17 + 2);
        // A boosted fire user: x1.5 at the end.
        u.ability = 0;
        u.volatile.fire_boost = true;
        assert_eq!(base(&attack(&u, &t, FIRE, 80)), 52 + 2);
    }

    #[test]
    fn multipliers_after_the_crit() {
        let c = chart();
        let u = fighter([NORMAL, NORMAL], 100);
        let t = fighter([NORMAL, NORMAL], 100);
        let a = Attack {
            mult: 2,
            ..attack(&u, &t, ELECTRIC, 80)
        };
        assert_eq!(finish(37, &a, Some(&c), 0).0, 74);
        let a = Attack { charged: true, ..a };
        assert_eq!(finish(37, &a, Some(&c), 0).0, 148);
        let a = Attack { helped: true, ..a };
        assert_eq!(finish(37, &a, Some(&c), 0).0, 222);
        // A sun-charging move is halved in other weather.
        let s = Attack {
            effect: effects::SUN_CHARGE,
            weather: Weather::Rain,
            ..attack(&u, &t, GRASS, 80)
        };
        assert_eq!(base(&s), 17 + 2);
    }

    #[test]
    fn confusion_hits_itself_with_40_power() {
        // 100*40*22/100/50 = 17, +2.
        assert_eq!(confusion(&fighter([FIRE, FIRE], 100), 0), 19);
        // 19 * 85 / 100.
        assert_eq!(confusion(&fighter([FIRE, FIRE], 100), 15), 16);
    }

    #[test]
    fn crit_odds() {
        assert_eq!(crit_chance(0, 0), 170);
        assert_eq!(crit_chance(1, 0), 512);
        assert_eq!(crit_chance(2, 0), 2048);
        assert_eq!(crit_chance(3, 0), 4096);
        assert_eq!(crit_chance(9, 0), 4096);
        assert_eq!(crit_chance(0, 255), 426);
        assert_eq!(crit_chance(0, 128), 298);
    }
}
