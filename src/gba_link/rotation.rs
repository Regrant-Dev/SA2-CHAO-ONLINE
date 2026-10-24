//! Each garden's wild area (re/chao-wild-visitors.md §5): an area is a region section of the
//! ROM's wild tables (the first wild header of each of its maps), and each goes to a garden by
//! what its maps and creatures are like (some fit a second garden too). A garden's area follows
//! the clock: each 10-minute
//! window (`slot`) takes the next of its areas from a shuffle of them, so each comes once before
//! any comes again, and every game with the same ROM has the same area for a garden at the same
//! time.

use crate::gba::rom::Rom;
use crate::gba::terrain::{
    ASH_GRASS, Behaviours, MAP_INDOOR, MAP_OCEAN_ROUTE, MAP_ROUTE, MAP_UNDERGROUND, MAP_UNDERWATER,
    SAND, TALL_GRASS, WATER,
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
    /// The garden it goes to (`theme_of`).
    pub theme: Theme,
    /// Other gardens it fits too (`also_of`).
    pub also: Vec<Theme>,
    pub land: Vec<Vec<WildSlot>>,
    pub water: Vec<Vec<WildSlot>>,
    pub rocks: Vec<Vec<WildSlot>>,
    /// Each rod's table on its own, with its rod (`WildMethod::BasicRod` ..= `BestRod`).
    pub fishing: Vec<(WildMethod, Vec<WildSlot>)>,
}

impl Area {
    /// It's one of the areas of the garden of `theme`.
    pub fn fits(&self, theme: Theme) -> bool {
        self.theme == theme || self.also.contains(&theme)
    }

    /// Every slot of its tables.
    pub fn slots(&self) -> impl Iterator<Item = &WildSlot> {
        [&self.land, &self.water, &self.rocks]
            .into_iter()
            .flatten()
            .flatten()
            .chain(self.fishing.iter().flat_map(|(_, t)| t))
    }
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

/// The other gardens an area of `theme` fits too:
/// - Hero: one with land creatures, of which flying and water types make up at least a third.
/// - Neutral: a route with land creatures whose tall grass is at least 3% of its blocks.
/// - Dark: a cave (underground or indoor maps) with water creatures only.
pub fn also_of(theme: Theme, has_land: bool, s: &Summary) -> Vec<Theme> {
    let typed = |kinds: &[u8]| s.map_types.iter().any(|t| kinds.contains(t));
    let mut also = Vec::new();
    if theme != Theme::Neutral
        && has_land
        && typed(&[MAP_ROUTE])
        && s.behaviours.of(&TALL_GRASS) * 100 >= s.behaviours.blocks * 3
    {
        also.push(Theme::Neutral);
    }
    if theme != Theme::Hero && has_land && s.land_share_at_least(&[FLYING, WATER_TYPE], 1, 3) {
        also.push(Theme::Hero);
    }
    if theme != Theme::Dark && !has_land && typed(&[MAP_UNDERGROUND, MAP_INDOOR]) {
        also.push(Theme::Dark);
    }
    also
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
                        also: Vec::new(),
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
                rod @ (WildMethod::BasicRod | WildMethod::BetterRod | WildMethod::BestRod) => {
                    area.fishing.push((rod, t.slots))
                }
            }
        }
    }
    Some(
        found
            .into_iter()
            .map(|(mut a, s)| {
                a.theme = theme_of(a.section, !a.land.is_empty(), &s);
                a.also = also_of(a.theme, !a.land.is_empty(), &s);
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
        return (slot as usize + usize::from(garden)) % 2;
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
    fn some_areas_fit_a_second_garden() {
        let grass = [(0x02, 100)];
        let normal = [(0, 200)];
        assert!(also_of(Theme::Neutral, true, &summary(&[3], &grass, &normal)).is_empty());
        // A third sky and sea creatures: the Hero Garden too, a cave's as well as a route's.
        let third = [(0, 100), (FLYING, 30), (WATER_TYPE, 20)];
        let cave = [(crate::gba::terrain::CAVE_FLOOR, 100)];
        assert_eq!(
            also_of(Theme::Dark, true, &summary(&[4], &cave, &third)),
            [Theme::Hero]
        );
        assert_eq!(
            also_of(Theme::Neutral, true, &summary(&[3], &grass, &third)),
            [Theme::Hero]
        );
        let less = [(0, 110), (FLYING, 40)];
        assert!(also_of(Theme::Neutral, true, &summary(&[3], &grass, &less)).is_empty());
        // Already Hero's: nothing more for that.
        assert!(also_of(Theme::Hero, false, &summary(&[6], &[(0x10, 100)], &[])).is_empty());
        // A grassy shore route: the Neutral Garden too; not with under 3% tall grass, nor off
        // a route.
        let shore = [(0x02, 3), (0x10, 97)];
        assert_eq!(
            also_of(Theme::Hero, true, &summary(&[3], &shore, &normal)),
            [Theme::Neutral]
        );
        let bare = [(0x02, 2), (0x10, 98)];
        assert!(also_of(Theme::Hero, true, &summary(&[3], &bare, &normal)).is_empty());
        assert!(also_of(Theme::Hero, true, &summary(&[1], &shore, &normal)).is_empty());
        // A cave with water creatures only: the Dark Garden too.
        assert_eq!(
            also_of(Theme::Hero, false, &summary(&[4], &cave, &[])),
            [Theme::Dark]
        );
        assert!(also_of(Theme::Hero, false, &summary(&[5], &cave, &[])).is_empty());
        // `fits` takes both, `area_at` picks from both.
        let area = |section, theme, also: Vec<Theme>| Area {
            section,
            theme,
            also,
            land: Vec::new(),
            water: Vec::new(),
            rocks: Vec::new(),
            fishing: Vec::new(),
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
        // With the areas that fit a second garden: two grassy shore routes for the Neutral
        // Garden, five areas of mostly sky and sea creatures (a route and four caves) for the
        // Hero Garden, a cave with water creatures only for the Dark Garden.
        let fits = |t: Theme| all.iter().filter(|a| a.fits(t)).count();
        assert_eq!(
            (fits(Theme::Neutral), fits(Theme::Hero), fits(Theme::Dark)),
            (15, 37, 20)
        );
        let also = |t: Theme| -> Vec<u8> {
            let mut s: Vec<u8> = all
                .iter()
                .filter(|a| a.also.contains(&t))
                .map(|a| a.section)
                .collect();
            s.sort();
            s
        };
        assert_eq!(also(Theme::Neutral), [19, 25]);
        assert_eq!(also(Theme::Hero), [29, 67, 68, 72, 210]);
        assert_eq!(also(Theme::Dark), [61]);
        assert!(
            all.iter()
                .all(|a| !a.also.contains(&a.theme) && a.also.len() <= 1)
        );
        // The Hero Garden's land areas: its own and the ones it shares.
        let hero_land = all
            .iter()
            .filter(|a| a.fits(Theme::Hero) && !a.land.is_empty())
            .count();
        assert_eq!(hero_land, 10);
        for a in &all {
            assert!(rom.map_section_name(a.section).is_some(), "{}", a.section);
            let tables = a.land.len() + a.water.len() + a.rocks.len() + a.fishing.len();
            assert!(tables > 0);
            if a.land.is_empty() {
                assert_eq!(a.theme, Theme::Hero);
                assert!(!a.water.is_empty() || !a.fishing.is_empty());
            }
        }
        // Each garden has an area of its theme every window, the same each time it's asked,
        // never the same two windows running, and each once before any comes again.
        let now = slot_of(1_792_800_000);
        for garden in 1..=3 {
            let theme = Theme::of_garden(garden).unwrap();
            let mine = all.iter().filter(|a| a.fits(theme)).count();
            let slots: Vec<u8> = (now..now + 200)
                .map(|t| area_at(&all, garden, t).unwrap().section)
                .collect();
            for (t, s) in (now..).zip(&slots) {
                assert!(all.iter().any(|a| a.section == *s && a.fits(theme)));
                assert_eq!(area_at(&all, garden, t).unwrap().section, *s);
            }
            assert!(
                slots.windows(2).all(|w| w[0] != w[1]),
                "{garden}: {slots:?}"
            );
            let round = (now / mine as u32 + 1) * mine as u32;
            let mut once: Vec<u8> = (round..round + mine as u32)
                .map(|t| area_at(&all, garden, t).unwrap().section)
                .collect();
            once.sort();
            once.dedup();
            assert_eq!(once.len(), mine, "{garden}");
        }
        assert!(area_at(&all, 0, 20_000).is_none());
        assert!(area_at(&all, 4, 20_000).is_none());
    }
}
