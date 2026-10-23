//! Each garden's wild area of the day (re/chao-wild-visitors.md §5): an area is a region section
//! of the ROM's wild tables (the first wild header of each of its maps), and each goes to one
//! garden by what its maps and creatures are like. Every UTC day each garden takes one of its
//! areas, the same for everyone with the same ROM: a shuffle of the garden's areas seeded by
//! the day, so each comes once before any comes again.

use crate::gba::rom::Rom;
use crate::gba::terrain::{
    ASH_GRASS, Behaviours, MAP_INDOOR, MAP_OCEAN_ROUTE, MAP_UNDERGROUND, MAP_UNDERWATER, SAND,
    WATER,
};
use crate::gba::world::{WildMethod, WildSlot};

/// The garden an area goes to.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Theme {
    /// Grassy routes and woods.
    Neutral,
    /// Sea, sky and shore.
    Hero,
    /// Caves, mountains and sand.
    Dark,
}

impl Theme {
    /// The theme of garden `garden` (`CurrentChaoArea` 1..=3).
    pub fn of_garden(garden: u8) -> Option<Self> {
        match garden {
            1 => Some(Self::Neutral),
            2 => Some(Self::Hero),
            3 => Some(Self::Dark),
            _ => None,
        }
    }
}

/// Type ids the rules look at.
const FLYING: u8 = 2;
const GROUND: u8 = 4;
const ROCK: u8 = 5;
const FIRE: u8 = 10;
const WATER_TYPE: u8 = 11;
/// The open-air tower whose maps are typed underground: a sky area.
const OPEN_AIR_TOWER: u8 = 85;

/// One wild area: a region section and its maps' tables (the first wild header of each map).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Area {
    pub section: u8,
    pub theme: Theme,
    pub land: Vec<Vec<WildSlot>>,
    pub water: Vec<Vec<WildSlot>>,
    pub rocks: Vec<Vec<WildSlot>>,
    /// Each rod's table on its own.
    pub fishing: Vec<Vec<WildSlot>>,
}

/// What the theme rules read of an area.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Summary {
    pub map_types: Vec<u8>,
    pub behaviours: Behaviours,
    /// Land slots' chances by type id, in half percents (a two-type species' chance counts half
    /// for each type).
    pub land_types: [u32; 32],
}

impl Summary {
    fn land_share_at_least_half(&self, types: &[u8]) -> bool {
        let total: u32 = self.land_types.iter().sum();
        let of: u32 = types.iter().map(|&t| self.land_types[usize::from(t)]).sum();
        total > 0 && of * 2 >= total
    }
}

/// The garden an area goes to: the first rule that matches.
pub fn theme_of(section: u8, has_land: bool, s: &Summary) -> Theme {
    let typed = |kinds: &[u8]| s.map_types.iter().any(|t| kinds.contains(t));
    let blocks = s.behaviours.blocks;
    if section == OPEN_AIR_TOWER || !has_land || typed(&[MAP_UNDERWATER, MAP_OCEAN_ROUTE]) {
        Theme::Hero
    } else if typed(&[MAP_UNDERGROUND, MAP_INDOOR])
        || s.behaviours.of(&[ASH_GRASS]) * 20 >= blocks
        || s.land_share_at_least_half(&[GROUND, ROCK, FIRE])
    {
        Theme::Dark
    } else if (s.behaviours.of(&WATER) + s.behaviours.of(&SAND)) * 4 >= blocks
        || s.land_share_at_least_half(&[FLYING, WATER_TYPE])
    {
        Theme::Hero
    } else {
        Theme::Neutral
    }
}

/// Every area with wild creatures, in the order the ROM's wild tables first name them.
pub fn areas(rom: &Rom) -> Option<Vec<Area>> {
    let mut seen = Vec::new();
    let mut found: Vec<(Area, Summary)> = Vec::new();
    for map in rom.wild_maps()? {
        // Variants of a map after its first header are left out (the game uses the first).
        if seen.contains(&(map.group, map.num)) {
            continue;
        }
        seen.push((map.group, map.num));
        let section = rom.map_section(map.group, map.num)?;
        let at = match found.iter().position(|(a, _)| a.section == section) {
            Some(i) => i,
            None => {
                found.push((
                    Area {
                        section,
                        theme: Theme::Neutral,
                        land: Vec::new(),
                        water: Vec::new(),
                        rocks: Vec::new(),
                        fishing: Vec::new(),
                    },
                    Summary::default(),
                ));
                found.len() - 1
            }
        };
        let (area, summary) = &mut found[at];
        summary.map_types.push(rom.map_type(map.group, map.num)?);
        summary
            .behaviours
            .add(&rom.map_behaviours(map.group, map.num)?);
        for t in map.tables {
            match t.method {
                WildMethod::Land => {
                    for slot in &t.slots {
                        let [a, b] = rom
                            .species(slot.species)?
                            .types
                            .map(|t| usize::from(t) % 32);
                        let chance = u32::from(slot.chance);
                        if a == b {
                            summary.land_types[a] += chance * 2;
                        } else {
                            summary.land_types[a] += chance;
                            summary.land_types[b] += chance;
                        }
                    }
                    area.land.push(t.slots);
                }
                WildMethod::Water => area.water.push(t.slots),
                WildMethod::Rocks => area.rocks.push(t.slots),
                WildMethod::BasicRod | WildMethod::BetterRod | WildMethod::BestRod => {
                    area.fishing.push(t.slots)
                }
            }
        }
    }
    Some(
        found
            .into_iter()
            .map(|(mut a, s)| {
                a.theme = theme_of(a.section, !a.land.is_empty(), &s);
                a
            })
            .collect(),
    )
}

/// splitmix64: a well-mixed 64-bit value from `x`.
pub fn mix(x: u64) -> u64 {
    let mut z = x.wrapping_add(0x9E37_79B9_7F4A_7C15);
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

/// Which of `count` areas garden `garden` has on `day` (Unix days, UTC): the days run through a
/// shuffle of all of them, a new shuffle each round, which never starts with the area the round
/// before ended on.
pub fn pick(count: usize, day: u32, garden: u8) -> usize {
    if count < 2 {
        return 0;
    }
    if count == 2 {
        return (day as usize + usize::from(garden)) % 2;
    }
    let n = count as u64;
    let (round, at) = (u64::from(day) / n, (u64::from(day) % n) as usize);
    let mut order = shuffle(count, round, garden);
    if round > 0 && order[0] == shuffle(count, round - 1, garden)[count - 1] {
        order.swap(0, 1);
    }
    order[at]
}

/// Round `round`'s order of `count` areas for garden `garden`.
fn shuffle(count: usize, round: u64, garden: u8) -> Vec<usize> {
    let mut order: Vec<usize> = (0..count).collect();
    let mut seed = mix(round << 8 | u64::from(garden));
    for i in (1..count).rev() {
        seed = mix(seed);
        order.swap(i, (seed % (i as u64 + 1)) as usize);
    }
    order
}

/// Garden `garden`'s area on `day`.
pub fn area_of_day(areas: &[Area], garden: u8, day: u32) -> Option<&Area> {
    let theme = Theme::of_garden(garden)?;
    let mine: Vec<&Area> = areas.iter().filter(|a| a.theme == theme).collect();
    if mine.is_empty() {
        return None;
    }
    Some(mine[pick(mine.len(), day, garden)])
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    fn rom() -> Option<Rom> {
        let home = std::env::var_os("HOME")?;
        let path = Path::new(&home).join("Games/gba/emerald.gba");
        if !path.exists() {
            println!("skipped: no GBA ROM at {}", path.display());
            return None;
        }
        Some(Rom::open(&path).expect("ROM opens"))
    }

    fn summary(types: &[u8], blocks: &[(u8, u32)], land: &[(u8, u32)]) -> Summary {
        let mut s = Summary {
            map_types: types.to_vec(),
            ..Summary::default()
        };
        for &(b, n) in blocks {
            s.behaviours.counts[usize::from(b)] += n;
            s.behaviours.blocks += n;
        }
        for &(t, n) in land {
            s.land_types[usize::from(t)] += n;
        }
        s
    }

    #[test]
    fn the_rules_in_order() {
        let grass = [(0x02, 100)];
        let normal = [(0, 200)];
        let route = summary(&[3], &grass, &normal);
        assert_eq!(theme_of(1, true, &route), Theme::Neutral);
        assert_eq!(theme_of(1, false, &route), Theme::Hero);
        assert_eq!(theme_of(OPEN_AIR_TOWER, true, &route), Theme::Hero);
        assert_eq!(
            theme_of(1, true, &summary(&[3, 6], &grass, &normal)),
            Theme::Hero
        );
        assert_eq!(
            theme_of(1, true, &summary(&[4], &grass, &normal)),
            Theme::Dark
        );
        // Ocean beats underground.
        assert_eq!(
            theme_of(1, true, &summary(&[4, 5], &grass, &normal)),
            Theme::Hero
        );
        // 5% ash grass.
        let ash = [(0x02, 95), (ASH_GRASS, 5)];
        assert_eq!(
            theme_of(1, true, &summary(&[3], &ash, &normal)),
            Theme::Dark
        );
        let little = [(0x02, 96), (ASH_GRASS, 4)];
        assert_eq!(
            theme_of(1, true, &summary(&[3], &little, &normal)),
            Theme::Neutral
        );
        // Half rough creatures.
        let rough = [(0, 100), (GROUND, 60), (FIRE, 40)];
        assert_eq!(
            theme_of(1, true, &summary(&[3], &grass, &rough)),
            Theme::Dark
        );
        // A quarter shore, or half sky and sea creatures.
        let shore = [(0x02, 75), (0x10, 20), (0x06, 5)];
        assert_eq!(
            theme_of(1, true, &summary(&[3], &shore, &normal)),
            Theme::Hero
        );
        let sky = [(0, 100), (FLYING, 100)];
        assert_eq!(theme_of(1, true, &summary(&[3], &grass, &sky)), Theme::Hero);
        // Rough beats sky.
        let both = [(GROUND, 100), (FLYING, 100)];
        assert_eq!(
            theme_of(1, true, &summary(&[3], &grass, &both)),
            Theme::Dark
        );
    }

    #[test]
    fn the_shuffle_is_fixed_and_fair() {
        assert_eq!(pick(0, 5, 1), 0);
        assert_eq!(pick(1, 5, 1), 0);
        for count in [2, 13, 19, 32] {
            for garden in 1..=3 {
                // Each round of `count` days has each area once.
                for round in [0u32, 1, 700] {
                    let mut seen: Vec<usize> = (0..count as u32)
                        .map(|d| pick(count, round * count as u32 + d, garden))
                        .collect();
                    seen.sort();
                    assert_eq!(seen, (0..count).collect::<Vec<_>>());
                }
            }
        }
        // Never the same area two days running.
        for count in [2, 8, 13, 19] {
            for garden in 1..=3 {
                for day in 20_000..22_000 {
                    assert_ne!(
                        pick(count, day, garden),
                        pick(count, day + 1, garden),
                        "{count} {garden} {day}"
                    );
                }
            }
        }
        // The same day gives the same, and the gardens differ in order.
        assert_eq!(pick(19, 20_500, 3), pick(19, 20_500, 3));
        let a: Vec<usize> = (0..19).map(|d| pick(19, d, 1)).collect();
        let b: Vec<usize> = (0..19).map(|d| pick(19, d, 3)).collect();
        assert_ne!(a, b);
    }

    #[test]
    fn rom_areas_cover_every_garden() {
        let Some(rom) = rom() else { return };
        let all = areas(&rom).unwrap();
        assert_eq!(all, areas(&rom).unwrap());
        let count = |t: Theme| all.iter().filter(|a| a.theme == t).count();
        assert_eq!(
            (
                count(Theme::Neutral),
                count(Theme::Hero),
                count(Theme::Dark)
            ),
            (13, 32, 19)
        );
        for a in &all {
            assert!(rom.map_section_name(a.section).is_some(), "{}", a.section);
            let tables = a.land.len() + a.water.len() + a.rocks.len() + a.fishing.len();
            assert!(tables > 0);
            if a.land.is_empty() {
                assert_eq!(a.theme, Theme::Hero);
                assert!(!a.water.is_empty() || !a.fishing.is_empty());
            }
        }
        // Each garden has an area every day, and they change from day to day.
        for garden in 1..=3 {
            let days: Vec<u8> = (20_000..20_040)
                .map(|d| area_of_day(&all, garden, d).unwrap().section)
                .collect();
            let theme = Theme::of_garden(garden).unwrap();
            for s in &days {
                assert!(all.iter().any(|a| a.section == *s && a.theme == theme));
            }
            let mut distinct = days.clone();
            distinct.sort();
            distinct.dedup();
            assert!(distinct.len() > 10, "{garden}: {days:?}");
        }
        assert!(area_of_day(&all, 0, 20_000).is_none());
        assert!(area_of_day(&all, 4, 20_000).is_none());
    }
}
