//! What a Chao gets from absorbing a partner: stat progress by the partner's type ids, scaled by
//! its level, applied with the game's own level-up rule (`GrowStats` 0x535F00,
//! re/chao-garden-items.md 1.1).

/// Stats in the Chao record's order (`ChaoData+0x20` progress, `+0x28` grade, `+0x30` level,
/// `+0x38` points).
pub const SWIM: usize = 0;
pub const FLY: usize = 1;
pub const RUN: usize = 2;
pub const POWER: usize = 3;
pub const STAMINA: usize = 4;
pub const LUCK: usize = 5;
pub const INTEL: usize = 6;
/// Stats a gain covers (the record has 8; the last is unused).
pub const STATS: usize = 7;

/// Type ids (the ROM's type table index) that train each stat; any other type id trains
/// stamina.
const SWIM_TYPES: [u8; 2] = [11, 15];
const FLY_TYPES: [u8; 2] = [2, 16];
const RUN_TYPES: [u8; 1] = [13];
const POWER_TYPES: [u8; 4] = [1, 4, 5, 10];

/// A partner with at least this base speed also trains running.
pub const FAST: u8 = 100;
/// Progress at level 1 and the most any absorb gives (100 = one stat level): never a whole
/// level of one stat at once, about what an animal gives (36..60).
const LEAST: u32 = 20;
pub const MOST: u32 = 80;
/// Luck and intelligence, as animals give (+20) but less.
const LUCK_INTEL: u8 = 10;

/// Record offsets (re/chao-garden-items.md).
const PROGRESS: usize = 0x20;
const GRADE: usize = 0x28;
const LEVEL: usize = 0x30;
const POINTS: usize = 0x38;
/// The game's limits: level 99, points 4000 from growing (0x535E90).
const LEVEL_MAX: u8 = 99;
const POINTS_MAX: i32 = 4000;

/// The stat a type id trains.
pub fn stat_for_type(type_id: u8) -> usize {
    if SWIM_TYPES.contains(&type_id) {
        SWIM
    } else if FLY_TYPES.contains(&type_id) {
        FLY
    } else if RUN_TYPES.contains(&type_id) {
        RUN
    } else if POWER_TYPES.contains(&type_id) {
        POWER
    } else {
        STAMINA
    }
}

/// Progress per stat for a partner of `types` (the same twice for one type), `level` 1..=100
/// and `speed` (base speed): `LEAST` at level 1 up to `MOST` at 100, shared by its types' stats;
/// a fast one gives a quarter of it to running. Luck and intelligence get a little.
pub fn gain(types: [u8; 2], level: u8, speed: u8) -> [u8; STATS] {
    let level = u32::from(level.clamp(1, 100));
    let total = (LEAST + (MOST - LEAST) * (level - 1) / 99).min(MOST);
    let mut out = [0u32; STATS];
    let mut stats = vec![stat_for_type(types[0])];
    if types[1] != types[0] {
        stats.push(stat_for_type(types[1]));
    }
    let mut left = total;
    if speed >= FAST && !stats.contains(&RUN) {
        out[RUN] += total / 4;
        left -= total / 4;
    }
    let share = left / stats.len() as u32;
    for (i, &s) in stats.iter().enumerate() {
        // The first type takes what doesn't divide evenly.
        out[s] += if i == 0 {
            left - share * (stats.len() as u32 - 1)
        } else {
            share
        };
    }
    out[LUCK] += u32::from(LUCK_INTEL);
    out[INTEL] += u32::from(LUCK_INTEL);
    out.map(|v| v.min(MOST) as u8)
}

/// What `apply` changed: stats that went up a level.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Applied {
    pub levels: [bool; STATS],
}

/// Adds `gain` to a Chao record (at least 0x40 bytes) as `GrowStats` does: progress over 99
/// becomes a level (below 99) and `3 * grade + 11 + rand(0..4)` points, capped at 4000; at level
/// 99 progress stays at 99 or below. `rand4` gives 0..=3.
pub fn apply(rec: &mut [u8], gain: &[u8; STATS], mut rand4: impl FnMut() -> u8) -> Applied {
    let mut applied = Applied::default();
    if rec.len() < POINTS + 2 * STATS {
        return applied;
    }
    for (i, &g) in gain.iter().enumerate() {
        let mut progress = u32::from(rec[PROGRESS + i].min(99)) + u32::from(g);
        let mut level = rec[LEVEL + i];
        if progress > 99 && level < LEVEL_MAX {
            progress -= 100;
            level += 1;
            let at = POINTS + 2 * i;
            let points = i32::from(i16::from_le_bytes([rec[at], rec[at + 1]]));
            let grade = i32::from(rec[GRADE + i]);
            let points = (points + 3 * grade + 11 + i32::from(rand4() & 3)).min(POINTS_MAX);
            rec[at..at + 2].copy_from_slice(&(points as i16).to_le_bytes());
            applied.levels[i] = true;
        }
        if level >= LEVEL_MAX {
            progress = progress.min(99);
        }
        rec[PROGRESS + i] = progress.min(99) as u8;
        rec[LEVEL + i] = level;
    }
    applied
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn type_ids_pick_the_stat() {
        assert_eq!(stat_for_type(11), SWIM);
        assert_eq!(stat_for_type(15), SWIM);
        assert_eq!(stat_for_type(2), FLY);
        assert_eq!(stat_for_type(16), FLY);
        assert_eq!(stat_for_type(13), RUN);
        for t in [1, 4, 5, 10] {
            assert_eq!(stat_for_type(t), POWER);
        }
        for t in [0, 3, 6, 7, 8, 9, 12, 14, 17, 200] {
            assert_eq!(stat_for_type(t), STAMINA);
        }
    }

    #[test]
    fn gain_grows_with_level_and_stays_under_a_level() {
        let low = gain([11, 11], 1, 50);
        let high = gain([11, 11], 100, 50);
        assert_eq!(low[SWIM], 20);
        assert_eq!(high[SWIM], 80);
        assert!(gain([11, 11], 50, 50)[SWIM] > low[SWIM]);
        for level in [0, 1, 37, 100, 255] {
            for types in [[0, 0], [1, 2], [13, 13], [11, 2]] {
                for speed in [0, 99, 100, 255] {
                    let g = gain(types, level, speed);
                    assert!(g.iter().all(|&v| u32::from(v) <= MOST), "{g:?}");
                    let total: u32 = g[..LUCK].iter().map(|&v| u32::from(v)).sum();
                    assert!(total <= MOST, "{types:?} {level} {speed}: {g:?}");
                }
            }
        }
    }

    #[test]
    fn two_types_share_and_fast_ones_run() {
        let g = gain([10, 2], 100, 50);
        assert_eq!((g[POWER], g[FLY]), (40, 40));
        let g = gain([12, 12], 100, 120);
        assert_eq!((g[STAMINA], g[RUN]), (60, 20));
        // Already a runner: no extra.
        let g = gain([13, 13], 100, 120);
        assert_eq!(g[RUN], 80);
        assert_eq!((g[LUCK], g[INTEL]), (10, 10));
    }

    #[test]
    fn apply_levels_up_like_the_game() {
        let mut rec = vec![0u8; 0x800];
        rec[PROGRESS + SWIM] = 90;
        rec[GRADE + SWIM] = 2;
        rec[LEVEL + SWIM] = 5;
        rec[POINTS + 2 * SWIM..POINTS + 2 * SWIM + 2].copy_from_slice(&100i16.to_le_bytes());
        let mut g = [0u8; STATS];
        g[SWIM] = 30;
        g[FLY] = 40;
        let a = apply(&mut rec, &g, || 2);
        assert!(a.levels[SWIM] && !a.levels[FLY]);
        assert_eq!(rec[PROGRESS + SWIM], 20);
        assert_eq!(rec[LEVEL + SWIM], 6);
        let points = i16::from_le_bytes([rec[POINTS], rec[POINTS + 1]]);
        assert_eq!(points, 100 + 3 * 2 + 11 + 2);
        assert_eq!(rec[PROGRESS + FLY], 40);
    }

    #[test]
    fn apply_respects_the_caps() {
        let mut rec = vec![0u8; 0x800];
        rec[PROGRESS + RUN] = 99;
        rec[LEVEL + RUN] = 99;
        rec[PROGRESS + POWER] = 99;
        rec[LEVEL + POWER] = 98;
        rec[POINTS + 2 * POWER..POINTS + 2 * POWER + 2].copy_from_slice(&3999i16.to_le_bytes());
        let mut g = [0u8; STATS];
        g[RUN] = 80;
        g[POWER] = 80;
        apply(&mut rec, &g, || 3);
        assert_eq!((rec[LEVEL + RUN], rec[PROGRESS + RUN]), (99, 99));
        assert_eq!(rec[LEVEL + POWER], 99);
        assert!(rec[PROGRESS + POWER] <= 99);
        let points = i16::from_le_bytes([rec[POINTS + 6], rec[POINTS + 7]]);
        assert_eq!(points, 4000);
        // A short record is left alone.
        let mut short = vec![0u8; 0x10];
        assert_eq!(apply(&mut short, &g, || 0), Applied::default());
    }
}
