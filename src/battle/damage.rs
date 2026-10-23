//! The Gen 3 damage formula, step for step: base damage (stats with stages, level, power, burn,
//! screens, double-battle spread, weather, +2), then critical x2, same-type x1.5, the type chart
//! one row at a time and the random 85..100%.

use super::abilities::{self, Mods};
use super::data::{TypeChart, types};
use super::effects;
use super::field::Weather;
use super::fighter::{Fighter, Stat, staged};

/// One attack on one target.
#[derive(Clone, Copy, Debug)]
pub struct Attack<'a> {
    pub user: &'a Fighter,
    pub target: &'a Fighter,
    pub move_type: u8,
    pub power: u32,
    /// The move's effect id (a self-fainting move halves the target's defense).
    pub effect: u8,
    pub crit: bool,
    /// The weather in force (`Clear` when something cancels it).
    pub weather: Weather,
    /// The target's side has the screen for this move's kind up.
    pub screen: bool,
    /// Doubles with both of the target's side up (screens give 2/3 instead of 1/2).
    pub crowded: bool,
    /// A both-foes move in doubles with both foes up: half damage.
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
            power,
            effect: 0,
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

/// Gen 3 base damage, including the final +2.
pub fn base(a: &Attack) -> i32 {
    let mut m = Mods {
        attack: u32::from(a.user.stats.attack),
        defense: u32::from(a.target.stats.defense),
        sp_attack: u32::from(a.user.stats.sp_attack),
        sp_defense: u32::from(a.target.stats.sp_defense),
        power: a.power,
        boost_final: false,
    };
    abilities::damage_mods(&mut m, a);
    if a.effect == effects::SELF_FAINT {
        m.defense /= 2;
    }
    let level = i64::from(2 * u32::from(a.user.level) / 5 + 2);
    let mut damage: i64 = 0;
    if types::is_physical(a.move_type) {
        damage = side_damage(
            a,
            m.attack,
            m.defense,
            m.power,
            Stat::Attack,
            Stat::Defense,
            level,
        );
        if a.user.ailment == Some(super::fighter::Ailment::Burn)
            && !abilities::ignores_burn(a.user.ability)
        {
            damage /= 2;
        }
        damage = screened(damage, a);
        if a.spread {
            damage /= 2;
        }
        if damage == 0 {
            damage = 1;
        }
    }
    if types::is_special(a.move_type) {
        damage = side_damage(
            a,
            m.sp_attack,
            m.sp_defense,
            m.power,
            Stat::SpAttack,
            Stat::SpDefense,
            level,
        );
        damage = screened(damage, a);
        if a.spread {
            damage /= 2;
        }
        match (a.weather, a.move_type) {
            (Weather::Rain, types::FIRE) | (Weather::Sun, types::WATER) => damage /= 2,
            (Weather::Rain, types::WATER) | (Weather::Sun, types::FIRE) => {
                damage = damage * 15 / 10
            }
            _ => {}
        }
        if a.effect == effects::SUN_CHARGE
            && matches!(a.weather, Weather::Rain | Weather::Sand | Weather::Hail)
        {
            damage /= 2;
        }
        if m.boost_final {
            damage = damage * 15 / 10;
        }
    }
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
    let atk_stage = a.user.stage(atk_stat);
    let def_stage = a.target.stage(def_stat);
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

fn screened(damage: i64, a: &Attack) -> i64 {
    if !a.screen || a.crit {
        damage
    } else if a.crowded {
        2 * (damage / 3)
    } else {
        damage / 2
    }
}

/// Everything after base damage, and the type multiplier (hundredths). Without `chart` (the
/// out-of-PP move) there's neither STAB nor type; `random` is 0..=15 (percent off).
pub fn finish(base: i32, a: &Attack, chart: Option<&TypeChart>, random: u32) -> (i32, u32) {
    let mut d = i64::from(base);
    if a.crit {
        d *= 2;
    }
    d *= i64::from(a.mult.max(1));
    if a.charged && a.move_type == types::ELECTRIC {
        d *= 2;
    }
    if a.helped {
        d = d * 15 / 10;
    }
    // Far past any fighter's HP, and small enough for the chart's steps.
    let mut d = d.min(DAMAGE_CAP) as i32;
    let mut multiplier = 100;
    if let Some(chart) = chart {
        if a.user.has_type(a.move_type) {
            d = d * 15 / 10;
        }
        let identified = a.target.volatile.identified;
        multiplier = chart.multiplier(a.move_type, a.target.types, identified);
        d = chart.apply(d, a.move_type, a.target.types, identified);
    }
    if d != 0 {
        d = d * (100 - random.min(15) as i32) / 100;
        if d == 0 {
            d = 1;
        }
    }
    (d, multiplier)
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

/// Gen 3 critical-hit odds in 1/4096ths for a crit stage (0..=4: 1/16, 1/8, 1/4, 1/3, 1/2),
/// plus luck's share: up to another 1/16.
pub fn crit_chance(stage: u8, luck: u8) -> u32 {
    const ODDS: [u32; 5] = [16, 8, 4, 3, 2];
    4096 / ODDS[usize::from(stage.min(4))] + u32::from(luck) * 256 / 255
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
        // Typeless moves: only the +2.
        assert_eq!(base(&attack(&u, &t, TYPELESS, 120)), 2);
    }

    #[test]
    fn finish_applies_stab_type_and_random_in_order() {
        let u = fighter([FIRE, FIRE], 100);
        let t = fighter([GRASS, GRASS], 100);
        let c = chart();
        let a = attack(&u, &t, FIRE, 80);
        // 37 * 1.5 = 55, * 2 = 110, * 100%.
        assert_eq!(finish(37, &a, Some(&c), 0), (110, 200));
        // Lowest roll: 110 * 85 / 100 = 93.
        assert_eq!(finish(37, &a, Some(&c), 15).0, 93);
        // Critical: 74 -> 111 -> 222.
        let crit = Attack { crit: true, ..a };
        assert_eq!(finish(37, &crit, Some(&c), 0).0, 222);
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
        // Burn halves physical damage before the +2.
        let mut b = fighter([NORMAL, NORMAL], 100);
        b.ailment = Some(Ailment::Burn);
        let t = fighter([NORMAL, NORMAL], 100);
        assert_eq!(base(&attack(&b, &t, NORMAL, 80)), 17 + 2);
        assert_eq!(base(&attack(&b, &t, FIRE, 80)), 37);
    }

    #[test]
    fn screens_spread_and_weather() {
        let u = fighter([NORMAL, NORMAL], 100);
        let t = fighter([NORMAL, NORMAL], 100);
        let a = attack(&u, &t, NORMAL, 80);
        assert_eq!(base(&Attack { screen: true, ..a }), 17 + 2);
        assert_eq!(
            base(&Attack {
                screen: true,
                crowded: true,
                ..a
            }),
            22 + 2
        );
        assert_eq!(
            base(&Attack {
                screen: true,
                crit: true,
                ..a
            }),
            37
        );
        assert_eq!(base(&Attack { spread: true, ..a }), 17 + 2);
        let w = attack(&u, &t, WATER, 80);
        assert_eq!(
            base(&Attack {
                weather: Weather::Rain,
                ..w
            }),
            52 + 2
        );
        assert_eq!(
            base(&Attack {
                weather: Weather::Sun,
                ..w
            }),
            17 + 2
        );
        let f = attack(&u, &t, FIRE, 80);
        assert_eq!(
            base(&Attack {
                weather: Weather::Sun,
                ..f
            }),
            52 + 2
        );
        // Sand and hail don't change damage.
        assert_eq!(
            base(&Attack {
                weather: Weather::Sand,
                ..f
            }),
            37
        );
    }

    #[test]
    fn self_faint_halves_defense_and_abilities_apply() {
        let mut u = fighter([NORMAL, NORMAL], 100);
        let t = fighter([NORMAL, NORMAL], 100);
        let a = Attack {
            effect: effects::SELF_FAINT,
            ..attack(&u, &t, NORMAL, 80)
        };
        assert_eq!(base(&a), 72);
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
        assert_eq!(crit_chance(0, 0), 256);
        assert_eq!(crit_chance(1, 0), 512);
        assert_eq!(crit_chance(3, 0), 1365);
        assert_eq!(crit_chance(9, 0), 2048);
        assert_eq!(crit_chance(0, 255), 512);
        assert_eq!(crit_chance(0, 128), 384);
    }
}
