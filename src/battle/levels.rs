//! Battle levels and XP. Each Chao has a battle level from 1 to 100 (a new one is at 5) and the
//! XP it has toward the next level. The mod keeps them in `<mod>/gba/partners.txt` by the Chao's
//! identity (`gba_link::store`); the SA2 save is never changed for them. XP only comes from
//! battles:
//!
//! - Trainer battles (Gen 3's formula): each foe that fainted is worth its species' XP yield
//!   (from the ROM) times its level / 7. The total is shared equally among the Chao that took
//!   part (were sent out at least once), then 1.5 times as in any trainer battle. A loss gives
//!   only what the fainted foes were worth.
//! - Battles with a wild visitor in a garden: the same worth (XP yield times level / 7), shared
//!   among the Chao that took part, without the trainer bonus, and only for a win (Gen 3 gives
//!   XP for a wild creature only when it faints).
//! - Live battles: each Chao that took part gets a share of its own level's step (`step`):
//!   `step * R * (n + 3w) / (4 * max(n, 3))`, shared among the Chao that took part, where R is 1
//!   for the winner and 1/2 for the loser, n is the size of the other team and w counts its
//!   Chao that fainted, each as its battle level over the Chao's (at most 1). A win that knocks
//!   out a whole team of three or more at the Chao's level is one level step, split among the
//!   team; a smaller or weaker team is worth less.
//! - Giving up gives nothing, and neither does a live battle that doesn't count.
//!
//! The XP from one level to the next (`step`) follows Gen 3's medium-fast curve
//! (`(L + 1)^3 - L^3`) times 1.4, scaled down below level 30 so the first levels come quickly,
//! and stays at level 60's from there on, so the levels past the trainers' stay steady. Level
//! 100 is about 7.5 hours of trainer and live battles for a Chao that fights them alone; Chao
//! that share battles share the XP (`tests::the_pace`).
//!
//! No game access here.

/// The lowest, starting and highest battle levels.
pub const MIN_LEVEL: u8 = 1;
pub const START_LEVEL: u8 = 5;
pub const MAX_LEVEL: u8 = 100;
/// Below this level the steps are scaled down (by level / `RAMP_UNTIL`).
const RAMP_UNTIL: u32 = 30;
/// From this level on every step is the same.
const FLAT_FROM: u32 = 60;
/// Steps are this percent of the medium-fast curve's.
const CURVE_PERCENT: u32 = 140;
/// Trainer battles give this percent of the foes' worth (Gen 3's trainer bonus).
const TRAINER_PERCENT: u32 = 150;
/// A live team smaller than this is worth as much XP per Chao beaten as one this size.
const LIVE_TEAM: u64 = 3;

/// A Chao's battle level and the XP it has toward the next one.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Level {
    pub level: u8,
    pub xp: u32,
}

impl Default for Level {
    fn default() -> Self {
        Level {
            level: START_LEVEL,
            xp: 0,
        }
    }
}

/// The XP from `level` to the next (0 at `MAX_LEVEL`).
pub fn step(level: u8) -> u32 {
    if level >= MAX_LEVEL {
        return 0;
    }
    let l = u32::from(level.max(MIN_LEVEL));
    let m = l.min(FLAT_FROM);
    ((3 * m * m + 3 * m + 1) * l.min(RAMP_UNTIL) * CURVE_PERCENT / (RAMP_UNTIL * 100)).max(1)
}

/// The XP from level 1 to `level`.
pub fn need(level: u8) -> u64 {
    (MIN_LEVEL..level.min(MAX_LEVEL))
        .map(|l| u64::from(step(l)))
        .sum()
}

impl Level {
    /// `level` (kept in 1..=100) with `xp` toward the next (less than a step; none at 100).
    pub fn new(level: u8, xp: u32) -> Level {
        let level = level.clamp(MIN_LEVEL, MAX_LEVEL);
        Level {
            level,
            xp: xp.min(step(level).saturating_sub(1)),
        }
    }

    /// Adds `xp`; the levels it went up.
    pub fn gain(&mut self, xp: u32) -> u8 {
        let before = self.level;
        let mut xp = u64::from(self.xp) + u64::from(xp);
        while self.level < MAX_LEVEL && xp >= u64::from(step(self.level)) {
            xp -= u64::from(step(self.level));
            self.level += 1;
        }
        self.xp = if self.level >= MAX_LEVEL {
            0
        } else {
            xp as u32
        };
        self.level - before
    }

    /// How far it is toward the next level, 0.0..1.0 (1.0 at 100).
    pub fn fraction(&self) -> f32 {
        match step(self.level) {
            0 => 1.0,
            s => self.xp as f32 / s as f32,
        }
    }
}

/// One of a trainer's creatures, for the XP it's worth.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Foe {
    /// Its species' XP yield (ROM).
    pub exp_yield: u8,
    pub level: u8,
    pub fainted: bool,
}

/// What a foe is worth (Gen 3: yield * level / 7).
fn worth(f: &Foe) -> u32 {
    u32::from(f.exp_yield) * u32::from(f.level) / 7
}

/// The XP each of the `takers` Chao that took part gets from a trainer battle against `foes`
/// (`won`: the player won; a draw is a loss). A loss gives only the fainted foes' worth.
pub fn trainer_xp(foes: &[Foe], won: bool, takers: usize) -> u32 {
    if takers == 0 {
        return 0;
    }
    if !won && !foes.iter().any(|f| f.fainted) {
        return 0;
    }
    let pool: u32 = foes.iter().filter(|f| won || f.fainted).map(worth).sum();
    (pool / takers as u32).max(1) * TRAINER_PERCENT / 100
}

/// The XP each of the `takers` Chao that took part gets from a battle with the wild visitor
/// `foe` (`won`: the player won): its worth shared among them, at least 1; nothing for a loss.
pub fn wild_xp(foe: &Foe, won: bool, takers: usize) -> u32 {
    if !won || takers == 0 {
        return 0;
    }
    (worth(foe) / takers as u32).max(1)
}

/// The XP a Chao at `level` gets from a live battle (`won`: its side won) against a team of
/// `foes`, whose Chao at the battle levels in `fainted` fainted, shared among the `takers` Chao
/// that took part.
pub fn online_xp(level: u8, won: bool, fainted: &[u8], foes: usize, takers: usize) -> u32 {
    if takers == 0 || foes == 0 {
        return 0;
    }
    let n = foes as u64;
    let own = u64::from(level.max(MIN_LEVEL));
    // Each fainted foe in thousandths: its level over the Chao's, at most 1.
    let w: u64 = fainted
        .iter()
        .take(foes)
        .map(|&l| (u64::from(l) * 1000 / own).min(1000))
        .sum();
    // step * R * (n + 3w) / (4 * max(n, 3)) with R = r/2.
    let r = if won { 2 } else { 1 };
    (u64::from(step(level)) * r * (n * 1000 + 3 * w)
        / (8 * 1000 * n.max(LIVE_TEAM) * takers as u64)) as u32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_curve() {
        assert_eq!(need(1), 0);
        assert_eq!(need(START_LEVEL), 18);
        assert_eq!(need(20), 5_401);
        assert_eq!(need(50), 164_885);
        assert_eq!(need(60), 292_279);
        assert_eq!(need(MAX_LEVEL), 907_199);
        for l in MIN_LEVEL..MAX_LEVEL {
            assert!(need(l + 1) > need(l), "{l}");
            assert!(step(l) >= 1);
        }
        for l in 60..MAX_LEVEL {
            assert_eq!(step(l), 15_373);
        }
        assert_eq!(step(MAX_LEVEL), 0);
    }

    #[test]
    fn gaining_xp_goes_up_levels() {
        let mut l = Level::default();
        assert_eq!(l.gain(step(5) - 1), 0);
        assert_eq!(l.level, 5);
        assert_eq!(l.gain(1), 1);
        assert_eq!(l, Level { level: 6, xp: 0 });
        // Several at once, with the rest kept.
        let mut l = Level::default();
        let to_ten = (need(10) - need(5)) as u32;
        assert_eq!(l.gain(to_ten + 3), 5);
        assert_eq!(l, Level { level: 10, xp: 3 });
        assert!((l.fraction() - 3.0 / step(10) as f32).abs() < 1e-6);
        // Level 100 takes no more.
        let mut top = Level::new(99, 0);
        assert_eq!(top.gain(u32::MAX), 1);
        assert_eq!(top, Level { level: 100, xp: 0 });
        assert_eq!(top.gain(5_000), 0);
        assert_eq!(top.xp, 0);
        assert_eq!(top.fraction(), 1.0);
        // Out-of-range values are kept in range.
        assert_eq!(Level::new(0, 0).level, MIN_LEVEL);
        assert_eq!(Level::new(200, 9), Level { level: 100, xp: 0 });
        assert_eq!(Level::new(5, 1_000_000).xp, step(5) - 1);
    }

    #[test]
    fn trainer_xp_follows_gen_3() {
        let foe = |exp_yield, level, fainted| Foe {
            exp_yield,
            level,
            fainted,
        };
        // 140 * 20 / 7 = 400, 1.5x: 600.
        assert_eq!(trainer_xp(&[foe(140, 20, true)], true, 1), 600);
        // Shared: 300 each.
        assert_eq!(trainer_xp(&[foe(140, 20, true)], true, 2), 300);
        // Two foes add up.
        assert_eq!(
            trainer_xp(&[foe(140, 20, true), foe(70, 14, true)], true, 1),
            (400 + 140) * 3 / 2
        );
        // A loss: only the fainted ones.
        let party = [foe(140, 20, true), foe(140, 20, false)];
        assert_eq!(trainer_xp(&party, false, 1), 600);
        let party = [foe(140, 20, false), foe(140, 20, false)];
        assert_eq!(trainer_xp(&party, false, 1), 0);
        // Nobody took part: none; a tiny foe still gives something.
        assert_eq!(trainer_xp(&party, true, 0), 0);
        assert_eq!(trainer_xp(&[foe(1, 1, true)], true, 3), 1);
    }

    #[test]
    fn wild_xp_is_gen_3_without_the_trainer_bonus() {
        let foe = Foe {
            exp_yield: 140,
            level: 20,
            fainted: true,
        };
        // 140 * 20 / 7 = 400, and two that took part share it.
        assert_eq!(wild_xp(&foe, true, 1), 400);
        assert_eq!(wild_xp(&foe, true, 2), 200);
        assert_eq!(wild_xp(&foe, true, 1) * 3 / 2, trainer_xp(&[foe], true, 1));
        // Nothing for a loss or when nobody took part; a tiny one still gives something.
        assert_eq!(wild_xp(&foe, false, 1), 0);
        assert_eq!(wild_xp(&foe, true, 0), 0);
        let tiny = Foe {
            exp_yield: 1,
            level: 1,
            fainted: true,
        };
        assert_eq!(wild_xp(&tiny, true, 3), 1);
    }

    /// Losing a trainer battle on purpose (sending a weak Chao against the strongest party)
    /// pays less a minute than winning.
    #[test]
    fn losing_on_purpose_doesnt_pay() {
        let party = |fainted: usize| -> Vec<Foe> {
            (0..5)
                .map(|i| Foe {
                    exp_yield: 199,
                    level: 54 + i as u8,
                    fainted: i < fainted,
                })
                .collect()
        };
        // An instant loss gives nothing.
        assert_eq!(trainer_xp(&party(0), false, 1), 0);
        // Losing after one knock-out (about a minute) against winning (3.3 minutes).
        let lost = f64::from(trainer_xp(&party(1), false, 1)) / 1.0;
        let won = f64::from(trainer_xp(&party(5), true, 1)) / 3.3;
        assert!(lost < won, "{lost:.0} vs {won:.0} XP a minute");
    }

    #[test]
    fn live_xp_is_part_of_a_level_step() {
        let s = step(30);
        let at = |n: usize| vec![30u8; n];
        assert_eq!(online_xp(30, true, &at(3), 3, 1), s);
        assert_eq!(online_xp(30, true, &at(3), 3, 2), s / 2);
        assert_eq!(online_xp(30, false, &at(1), 3, 1), s / 4);
        // The winner always gets more than the loser.
        for f in 0..=3 {
            assert!(online_xp(30, true, &at(f), 3, 1) > online_xp(30, false, &at(f), 3, 1));
        }
        assert_eq!(online_xp(MAX_LEVEL, true, &at(3), 3, 1), 0);
        assert_eq!(online_xp(30, true, &at(3), 0, 1), 0);
        // A lone foe is worth a third of a team of three.
        assert_eq!(online_xp(30, true, &at(1), 1, 1), s / 3);
        // A foe far below the Chao's level is worth little; one above counts as its level.
        assert!(online_xp(60, true, &[5], 1, 1) < step(60) / 9);
        assert_eq!(
            online_xp(30, true, &[90, 90, 90], 3, 1),
            online_xp(30, true, &at(3), 3, 1)
        );
        // Six beaten foes are worth as much as three: a bigger team means a longer battle.
        assert_eq!(online_xp(30, true, &at(6), 6, 1), s);
        // Quick one-a-side battles (1.5 minutes, won and lost in turn) pay no more a minute
        // than three a side (4.5 minutes, as in `the_pace`).
        for level in [10, 30, 70] {
            let per_minute = |n: usize, minutes: f64| {
                let won = online_xp(level, true, &vec![level; n], n, 1);
                // The loser knocks out one of three, none of one.
                let lost = online_xp(level, false, &vec![level; n / 3], n, 1);
                f64::from(won + lost) / (2.0 * minutes)
            };
            assert!(per_minute(1, 1.5) <= per_minute(3, 4.5), "{level}");
        }
    }

    /// The model of normal play behind the curve: trainers about the Chao's level (2 foes up to
    /// level 46, 5 above it, the strongest at 58), 85% of them won, every fifth battle live
    /// (three a side at the Chao's level, alternately won knocking out all 3 and lost knocking
    /// out 1); a lost trainer battle knocks out 1 of 2 foes or 2 of 5. Minutes a battle: 3.0 for
    /// trainers below level 15, 1.8 up to 46, 3.3 for the full parties, 4.5 for a live one.
    /// Returns the minutes at which each level was reached.
    fn play(takers: usize) -> Vec<f64> {
        let mut at = vec![0.0; usize::from(MAX_LEVEL) + 1];
        let mut l = Level::default();
        let mut minutes = 0.0;
        let (mut battles, mut trainers) = (0u32, 0u32);
        while l.level < MAX_LEVEL {
            battles += 1;
            assert!(battles < 10_000);
            let xp = if battles.is_multiple_of(5) {
                minutes += 4.5;
                let won = (battles / 5).is_multiple_of(2);
                let fainted = vec![l.level; if won { 3 } else { 1 }];
                online_xp(l.level, won, &fainted, 3, takers)
            } else {
                trainers += 1;
                let t = l.level.min(58);
                let exp_yield = (60 + 12 * u32::from(t) / 5) as u8;
                let won = ![3, 7, 11].contains(&(trainers % 20));
                let (foes, mins): (Vec<Foe>, f64) = if t <= 46 {
                    let levels = [t, t.saturating_sub(2).max(1)];
                    let foes = levels
                        .iter()
                        .enumerate()
                        .map(|(i, &level)| Foe {
                            exp_yield,
                            level,
                            fainted: won || i == 0,
                        })
                        .collect();
                    (foes, if t < 15 { 3.0 } else { 1.8 })
                } else {
                    let foes = (0..5)
                        .map(|i| Foe {
                            exp_yield,
                            level: t - 4 + i,
                            fainted: won || i < 2,
                        })
                        .collect();
                    (foes, 3.3)
                };
                minutes += mins;
                trainer_xp(&foes, won, takers)
            };
            let before = l.level;
            l.gain(xp);
            for reached in before + 1..=l.level {
                at[usize::from(reached)] = minutes;
            }
        }
        at
    }

    #[test]
    fn the_pace() {
        let at = play(1);
        let hours = at[100] / 60.0;
        assert!((7.0..=8.0).contains(&hours), "{hours:.2} h");
        // The first levels come quickly.
        assert!(at[20] <= 45.0, "level 20 at {:.0} min", at[20]);
        let per_level = |a: usize, b: usize| (at[b] - at[a]) / (b - a) as f64;
        assert!(per_level(5, 20) <= 0.6 * per_level(20, 100));
        // Then steady.
        let blocks: Vec<f64> = [20, 40, 60, 80]
            .iter()
            .map(|&a| per_level(a, a + 20))
            .collect();
        for b in &blocks {
            assert!((4.0..=6.5).contains(b), "{blocks:?}");
        }
        let (lo, hi) = blocks
            .iter()
            .fold((f64::MAX, 0.0f64), |(lo, hi), &b| (lo.min(b), hi.max(b)));
        assert!(hi <= 1.3 * lo, "{blocks:?}");
        // A team of three sharing every battle takes about three times as long each.
        let ratio = play(3)[100] / at[100];
        assert!((2.7..=3.3).contains(&ratio), "{ratio:.2}");
    }
}
