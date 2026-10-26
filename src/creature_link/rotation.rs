//! Each garden's wild area (re/chao-wild-visitors.md §5): an area is one of the catalog's
//! habitats (`Rom::wild_maps`, one for each type, with the creatures of that first type), and
//! each goes to a garden by its type and what its creatures are like (some fit a second garden
//! too). A garden's area follows the clock: each 10-minute window (`slot`) takes the next of its
//! areas from a shuffle of them, so each comes once before any comes again, and every game with
//! the same catalog has the same area for a garden at the same time.

use crate::creature::rom::Rom;
use crate::creature::world::{WildMethod, WildSlot};

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
const FIRE: u8 = 9;
const WATER: u8 = 10;
/// Habitats of these types are sky and sea ones.
const SKY_AND_SEA: [u8; 4] = [FLYING, WATER, 14, 17];
/// Habitats of these types are caves and mountains.
const CAVES: [u8; 7] = [GROUND, ROCK, 7, 8, FIRE, 15, 16];

/// One wild area: a habitat (`section`, its type id) and its tables.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Area {
    pub section: u8,
    /// The garden it goes to (`theme_of`).
    pub theme: Theme,
    /// Other gardens it fits too (`also_of`).
    pub also: Vec<Theme>,
    pub land: Vec<Vec<WildSlot>>,
    pub water: Vec<Vec<WildSlot>>,
}

impl Area {
    /// It's one of the areas of the garden of `theme`.
    pub fn fits(&self, theme: Theme) -> bool {
        self.theme == theme || self.also.contains(&theme)
    }

    /// Every slot of its tables.
    pub fn slots(&self) -> impl Iterator<Item = &WildSlot> {
        [&self.land, &self.water].into_iter().flatten().flatten()
    }
}

/// What the theme rules read of an area's creatures.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Summary {
    /// Land slots' chances by type id, in half percents (a two-type species' chance counts half
    /// for each type).
    pub land_types: [u32; 32],
}

impl Summary {
    /// `types` make up at least `num / den` of the land slots by chance.
    fn land_share_at_least(&self, types: &[u8], num: u32, den: u32) -> bool {
        let total: u32 = self.land_types.iter().sum();
        let of: u32 = types.iter().map(|&t| self.land_types[usize::from(t)]).sum();
        total > 0 && of * den >= total * num
    }

    fn land_share_at_least_half(&self, types: &[u8]) -> bool {
        self.land_share_at_least(types, 1, 2)
    }
}

/// The garden habitat `section` goes to: the first rule that matches.
pub fn theme_of(section: u8, has_land: bool, s: &Summary) -> Theme {
    if !has_land || SKY_AND_SEA.contains(&section) {
        Theme::Hero
    } else if CAVES.contains(&section) || s.land_share_at_least_half(&[GROUND, ROCK, FIRE]) {
        Theme::Dark
    } else if s.land_share_at_least_half(&[FLYING, WATER]) {
        Theme::Hero
    } else {
        Theme::Neutral
    }
}

/// The other gardens habitat `section` of `theme` fits too:
/// - Neutral: a grassland habitat (neither sky and sea nor caves) with land creatures.
/// - Hero: one with land creatures, of which flying and water types make up at least a third.
/// - Dark: a cave habitat with water creatures only.
pub fn also_of(section: u8, theme: Theme, has_land: bool, s: &Summary) -> Vec<Theme> {
    let cave = CAVES.contains(&section);
    let mut also = Vec::new();
    if theme != Theme::Neutral && has_land && !cave && !SKY_AND_SEA.contains(&section) {
        also.push(Theme::Neutral);
    }
    if theme != Theme::Hero && has_land && s.land_share_at_least(&[FLYING, WATER], 1, 3) {
        also.push(Theme::Hero);
    }
    if theme != Theme::Dark && !has_land && cave {
        also.push(Theme::Dark);
    }
    also
}

/// Every area with wild creatures, in the order the catalog's wild tables first name them.
pub fn areas(rom: &Rom) -> Option<Vec<Area>> {
    let mut seen = Vec::new();
    let mut found: Vec<(Area, Summary)> = Vec::new();
    for map in rom.wild_maps()? {
        // A map listed twice counts once.
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
                        also: Vec::new(),
                        land: Vec::new(),
                        water: Vec::new(),
                    },
                    Summary::default(),
                ));
                found.len() - 1
            }
        };
        let (area, summary) = &mut found[at];
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
            }
        }
    }
    Some(
        found
            .into_iter()
            .map(|(mut a, s)| {
                a.theme = theme_of(a.section, !a.land.is_empty(), &s);
                a.also = also_of(a.section, a.theme, !a.land.is_empty(), &s);
                a
            })
            .collect(),
    )
}

/// How long each garden keeps its area (seconds).
pub const SLOT_SECS: u64 = 600;

/// The clock window of `unix` (seconds since 1970, UTC).
pub fn slot_of(unix: u64) -> u32 {
    (unix / SLOT_SECS) as u32
}

/// Seconds since 1970 by this machine's clock (0 if it's before that).
pub fn unix_now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

/// The clock window now.
pub fn slot_now() -> u32 {
    slot_of(unix_now())
}

/// The minutes (rounded up) the window of `unix` has left.
pub fn minutes_left(unix: u64) -> u64 {
    (SLOT_SECS - unix % SLOT_SECS).div_ceil(60)
}

/// The line under an area's name at `unix`: how long its window has left.
pub fn until_next(unix: u64) -> String {
    match minutes_left(unix) {
        1 => "Wild visitors come from here for the next minute".into(),
        n => format!("Wild visitors come from here for the next {n} minutes"),
    }
}

/// splitmix64: a well-mixed 64-bit value from `x`.
pub fn mix(x: u64) -> u64 {
    let mut z = x.wrapping_add(0x9E37_79B9_7F4A_7C15);
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

/// Which of `count` areas garden `garden` has in window `slot`: the windows run through a
/// shuffle of all of them, a new shuffle each round, which never starts with the area the round
/// before ended on.
pub fn pick(count: usize, slot: u32, garden: u8) -> usize {
    if count < 2 {
        return 0;
    }
    if count == 2 {
        return ((u64::from(slot) + u64::from(garden)) % 2) as usize;
    }
    let n = count as u64;
    let (round, at) = (u64::from(slot) / n, (u64::from(slot) % n) as usize);
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

/// Garden `garden`'s area in window `slot`.
pub fn area_at(areas: &[Area], garden: u8, slot: u32) -> Option<&Area> {
    let theme = Theme::of_garden(garden)?;
    let mine: Vec<&Area> = areas.iter().filter(|a| a.fits(theme)).collect();
    if mine.is_empty() {
        return None;
    }
    Some(mine[pick(mine.len(), slot, garden)])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rom() -> Option<crate::creature::rom::Rom> {
        crate::test_cache::rom()
    }

    fn summary(land: &[(u8, u32)]) -> Summary {
        let mut s = Summary::default();
        for &(t, n) in land {
            s.land_types[usize::from(t)] += n;
        }
        s
    }

    /// Habitats the tests use: a grassland one, a cave one and a sky and sea one.
    const GRASSLAND: u8 = 11;
    const CAVE: u8 = 16;
    const SKY: u8 = 14;

    #[test]
    fn the_rules_in_order() {
        let normal = summary(&[(0, 200)]);
        assert_eq!(theme_of(GRASSLAND, true, &normal), Theme::Neutral);
        // No land creatures, or a sky and sea habitat.
        assert_eq!(theme_of(GRASSLAND, false, &normal), Theme::Hero);
        assert_eq!(theme_of(SKY, true, &normal), Theme::Hero);
        assert_eq!(theme_of(FLYING, true, &normal), Theme::Hero);
        // A cave habitat.
        assert_eq!(theme_of(CAVE, true, &normal), Theme::Dark);
        assert_eq!(theme_of(GROUND, true, &normal), Theme::Dark);
        // Water creatures only beat a cave.
        assert_eq!(theme_of(CAVE, false, &normal), Theme::Hero);
        // Half rough creatures.
        let rough = summary(&[(0, 100), (GROUND, 60), (FIRE, 40)]);
        assert_eq!(theme_of(GRASSLAND, true, &rough), Theme::Dark);
        let less = summary(&[(0, 110), (GROUND, 50), (FIRE, 40)]);
        assert_eq!(theme_of(GRASSLAND, true, &less), Theme::Neutral);
        // Half sky and sea creatures.
        let sky = summary(&[(0, 100), (FLYING, 100)]);
        assert_eq!(theme_of(GRASSLAND, true, &sky), Theme::Hero);
        // Rough beats sky.
        let both = summary(&[(GROUND, 100), (FLYING, 100)]);
        assert_eq!(theme_of(GRASSLAND, true, &both), Theme::Dark);
    }

    #[test]
    fn some_areas_fit_a_second_garden() {
        let normal = summary(&[(0, 200)]);
        let none = Summary::default();
        assert!(also_of(GRASSLAND, Theme::Neutral, true, &normal).is_empty());
        // A third sky and sea creatures: the Hero Garden too, a cave's as well as a grassland's.
        let third = summary(&[(0, 100), (FLYING, 30), (WATER, 20)]);
        assert_eq!(also_of(CAVE, Theme::Dark, true, &third), [Theme::Hero]);
        assert_eq!(
            also_of(GRASSLAND, Theme::Neutral, true, &third),
            [Theme::Hero]
        );
        let less = summary(&[(0, 110), (FLYING, 40)]);
        assert!(also_of(GRASSLAND, Theme::Neutral, true, &less).is_empty());
        // Already Hero's: nothing more for that.
        assert!(also_of(WATER, Theme::Hero, false, &none).is_empty());
        // A grassland habitat its sky creatures give to Hero: the Neutral Garden too; not a sky
        // and sea habitat.
        let sky = summary(&[(0, 100), (FLYING, 100)]);
        assert_eq!(
            also_of(GRASSLAND, Theme::Hero, true, &sky),
            [Theme::Neutral]
        );
        assert!(also_of(SKY, Theme::Hero, true, &normal).is_empty());
        // A cave habitat with water creatures only: the Dark Garden too.
        assert_eq!(also_of(CAVE, Theme::Hero, false, &none), [Theme::Dark]);
        assert!(also_of(SKY, Theme::Hero, false, &none).is_empty());
        // `fits` takes both, `area_at` picks from both.
        let area = |section, theme, also: Vec<Theme>| Area {
            section,
            theme,
            also,
            land: Vec::new(),
            water: Vec::new(),
        };
        let areas = [
            area(1, Theme::Neutral, vec![Theme::Hero]),
            area(2, Theme::Hero, Vec::new()),
            area(3, Theme::Hero, Vec::new()),
        ];
        assert!(areas[0].fits(Theme::Neutral) && areas[0].fits(Theme::Hero));
        assert!(!areas[0].fits(Theme::Dark));
        let hero: Vec<u8> = (0..3)
            .map(|t| area_at(&areas, 2, t).unwrap().section)
            .collect();
        let mut sorted = hero.clone();
        sorted.sort();
        assert_eq!(sorted, [1, 2, 3]);
        assert_eq!(area_at(&areas, 1, 77).unwrap().section, 1);
        assert!(area_at(&areas, 3, 77).is_none());
    }

    #[test]
    fn the_shuffle_is_fixed_and_fair() {
        assert_eq!(pick(0, 5, 1), 0);
        assert_eq!(pick(1, 5, 1), 0);
        for count in [2, 13, 19, 32] {
            for garden in 1..=3 {
                // Each round of `count` windows has each area once.
                for round in [0u32, 1, 700] {
                    let mut seen: Vec<usize> = (0..count as u32)
                        .map(|d| pick(count, round * count as u32 + d, garden))
                        .collect();
                    seen.sort();
                    assert_eq!(seen, (0..count).collect::<Vec<_>>());
                }
            }
        }
        // Never the same area two windows running (about now on the clock, and at the ends of
        // the windows' range).
        let now = slot_of(1_792_800_000);
        for count in [2, 8, 13, 19, 32] {
            for garden in 1..=3 {
                for slot in (20_000..22_000).chain(now..now + 2_000) {
                    assert_ne!(
                        pick(count, slot, garden),
                        pick(count, slot + 1, garden),
                        "{count} {garden} {slot}"
                    );
                }
                assert!(pick(count, u32::MAX, garden) < count);
            }
        }
        // The same window gives the same, and the gardens differ in order.
        assert_eq!(pick(19, 20_500, 3), pick(19, 20_500, 3));
        let a: Vec<usize> = (0..19).map(|d| pick(19, d, 1)).collect();
        let b: Vec<usize> = (0..19).map(|d| pick(19, d, 3)).collect();
        assert_ne!(a, b);
    }

    #[test]
    fn windows_follow_the_clock() {
        assert_eq!(slot_of(0), 0);
        assert_eq!(slot_of(599), 0);
        assert_eq!(slot_of(600), 1);
        // Everyone in the same 10 minutes is in the same window.
        let start = 1_792_800_000 / SLOT_SECS * SLOT_SECS;
        assert!((start..start + SLOT_SECS).all(|t| slot_of(t) == slot_of(start)));
        assert_eq!(slot_of(start + SLOT_SECS), slot_of(start) + 1);
        assert_eq!(
            until_next(start),
            "Wild visitors come from here for the next 10 minutes"
        );
        assert_eq!(
            until_next(start + 545),
            "Wild visitors come from here for the next minute"
        );
        assert_eq!(
            until_next(start + 539),
            "Wild visitors come from here for the next 2 minutes"
        );
        assert!(slot_now() > slot_of(1_760_000_000));
    }

    #[test]
    fn catalog_areas_cover_every_garden() {
        let Some(rom) = rom() else {
            return;
        };
        let all = areas(&rom).unwrap();
        assert_eq!(all, areas(&rom).unwrap());
        assert_eq!(all.len(), 18);
        for theme in [Theme::Neutral, Theme::Hero, Theme::Dark] {
            let owned = all.iter().filter(|area| area.theme == theme).count();
            let fits = all.iter().filter(|area| area.fits(theme)).count();
            assert!(owned > 0 && fits >= owned);
        }
        let species: std::collections::BTreeSet<_> = all
            .iter()
            .flat_map(Area::slots)
            .map(|s| s.species)
            .collect();
        assert_eq!(species, (1..=807).collect());
        assert!(all.iter().all(|a| {
            rom.map_section_name(a.section)
                .is_some_and(|s| !s.is_empty())
        }));
    }

    #[test]
    fn the_last_windows_pick_without_overflowing() {
        for count in [2, 3, 7] {
            for garden in 1..=3 {
                assert!(pick(count, u32::MAX, garden) < count);
            }
        }
        assert_ne!(pick(2, u32::MAX, 1), pick(2, u32::MAX, 2));
    }
}
