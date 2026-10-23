//! Reads trainers, wild encounter tables and map names from the GBA ROM (see `rom` for the format).

use super::rom::{MOVE_COUNT, Rom};

/// Trainer records (pret `gTrainers`): 40 bytes {party flags, class, music | 0x80 female, pic,
/// 12-byte name, 4 u16 items, double battle @0x18, u32 AI flags @0x1C, party size @0x20, party
/// pointer @0x24}.
const TRAINERS: usize = 0x310030;
const TRAINER_SIZE: usize = 40;
/// Party flags: entries carry 4 move ids (else they use their learnset), and/or a held item.
const PARTY_CUSTOM_MOVES: u8 = 1 << 0;
const PARTY_HELD_ITEM: u8 = 1 << 1;
/// Trainer class names (pret `gTrainerClassNames`): 13-byte strings, indexed by class id.
const TRAINER_CLASS_NAMES: usize = 0x30FCD4;
const TRAINER_CLASS_NAME_LEN: usize = 13;
/// Wild encounter headers (pret `gWildMonHeaders`): 20 bytes {map group, map number, pad, then
/// u32 pointers to the land, water, rock and fishing tables (0 when absent)}, ended by a map group
/// of 0xFF. Each table is {u8 encounter rate, pad, u32 pointer to slots}; a slot is
/// {u8 min level, u8 max level, u16 species}.
const WILD_HEADERS: usize = 0x552D48;
const WILD_HEADER_SIZE: usize = 20;
const WILD_HEADERS_MAX: usize = 256;
/// Map groups (pret `gMapGroups`): a u32 pointer per group to its array of map header pointers.
/// The arrays sit back to back just before this table, so each ends where the next begins.
const MAP_GROUPS: usize = 0x486578;
/// Map header byte holding the map's region section id.
const MAP_HEADER_SECTION: usize = 0x14;
/// Region map sections (pret `gRegionMapEntries`): 8 bytes {x, y, width, height, u32 name pointer}.
const MAP_SECTIONS: usize = 0x5A147C;

/// Trainer ids, including 0 (none); pret `TRAINERS_COUNT`.
pub const TRAINER_COUNT: u16 = 855;
/// Trainer class ids.
pub const TRAINER_CLASS_COUNT: u8 = 66;
/// Most creatures in a party.
pub const PARTY_MAX: usize = 6;
/// Map groups.
pub const MAP_GROUP_COUNT: u8 = 34;
/// Region map section ids that have a name (0..MAP_SECTION_COUNT).
pub const MAP_SECTION_COUNT: u8 = 213;

/// Encounter chance in percent of each slot, per table kind.
const LAND_CHANCES: [u8; 12] = [20, 20, 10, 10, 10, 10, 5, 5, 4, 4, 1, 1];
const WATER_CHANCES: [u8; 5] = [60, 30, 5, 4, 1];
/// Fishing slots 0..2 are the basic rod's, 2..5 the better rod's and 5..10 the best rod's.
const FISHING_CHANCES: [u8; 10] = [70, 30, 60, 20, 20, 40, 40, 15, 4, 1];
const FISHING_RODS: [(WildMethod, std::ops::Range<usize>); 3] = [
    (WildMethod::BasicRod, 0..2),
    (WildMethod::BetterRod, 2..5),
    (WildMethod::BestRod, 5..10),
];

/// A trainer and their party.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Trainer {
    /// Trainer class id (0..TRAINER_CLASS_COUNT).
    pub class: u8,
    pub name: String,
    pub female: bool,
    /// Fights double battles.
    pub double: bool,
    /// The game's AI behaviour bits.
    pub ai_flags: u32,
    /// 1..=PARTY_MAX creatures.
    pub party: Vec<TrainerMon>,
}

/// One creature in a trainer's party.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TrainerMon {
    pub species: u16,
    /// 1..=100.
    pub level: u8,
    /// The individual value every stat gets (0..=31).
    pub iv: u8,
    /// Move ids (0 for none): the party's own moves, or the learnset's last four at `level`.
    pub moves: [u16; 4],
}

/// How a wild table's creatures are met.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WildMethod {
    Land,
    Water,
    /// Breaking rocks.
    Rocks,
    /// Fishing with the three rods, weakest first.
    BasicRod,
    BetterRod,
    BestRod,
}

/// One wild encounter slot.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WildSlot {
    pub species: u16,
    /// Level range, `min_level <= max_level`.
    pub min_level: u8,
    pub max_level: u8,
    /// Chance in percent that an encounter picks this slot; a table's slots add up to 100.
    pub chance: u8,
}

/// One way of meeting wild creatures on a map.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WildTable {
    pub method: WildMethod,
    /// How often encounters happen (the game's per-step rate, 0..=255).
    pub rate: u8,
    pub slots: Vec<WildSlot>,
}

/// The wild tables of one map. A map can have several entries (variants the game picks between).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WildMap {
    pub group: u8,
    pub num: u8,
    pub tables: Vec<WildTable>,
}

impl Rom {
    /// Trainer for ids 1..TRAINER_COUNT; `None` for 0, out-of-range ids and malformed records.
    pub fn trainer(&self, id: u16) -> Option<Trainer> {
        if id == 0 || id >= TRAINER_COUNT {
            return None;
        }
        let off = TRAINERS + usize::from(id) * TRAINER_SIZE;
        let rec = self.bytes_at(off, TRAINER_SIZE)?;
        let (flags, class, size) = (rec[0], rec[1], usize::from(rec[0x20]));
        if flags > (PARTY_CUSTOM_MOVES | PARTY_HELD_ITEM)
            || class >= TRAINER_CLASS_COUNT
            || !(1..=PARTY_MAX).contains(&size)
        {
            return None;
        }
        let custom = flags & PARTY_CUSTOM_MOVES != 0;
        let item = flags & PARTY_HELD_ITEM != 0;
        // Entries are {u16 iv, u8 level, pad, u16 species, [u16 item], [4 u16 moves]}, padded to 4.
        let entry = if custom { 16 } else { 8 };
        let moves_at = if item { 8 } else { 6 };
        let party_at = self.ptr_at(off + 0x24)?;
        let raw = self.bytes_at(party_at, entry * size)?;
        let party = raw
            .chunks_exact(entry)
            .map(|e| {
                let u16_at = |i: usize| u16::from_le_bytes([e[i], e[i + 1]]);
                let (species, level) = (u16_at(4), e[2]);
                if !self.species_exists(species) || !(1..=100).contains(&level) {
                    return None;
                }
                let moves = if custom {
                    let moves = [0, 1, 2, 3].map(|k| u16_at(moves_at + k * 2));
                    if moves.iter().any(|&m| m >= MOVE_COUNT) {
                        return None;
                    }
                    moves
                } else {
                    self.moves_at_level(species, level)
                };
                Some(TrainerMon {
                    species,
                    level,
                    iv: (u32::from(u16_at(0).min(255)) * 31 / 255) as u8,
                    moves,
                })
            })
            .collect::<Option<Vec<_>>>()?;
        Some(Trainer {
            class,
            name: self.string_at(off + 4, 12)?,
            female: rec[2] & 0x80 != 0,
            double: rec[0x18] != 0,
            ai_flags: u32::from_le_bytes([rec[0x1C], rec[0x1D], rec[0x1E], rec[0x1F]]),
            party,
        })
    }

    /// Trainer class name for ids 0..TRAINER_CLASS_COUNT.
    pub fn trainer_class_name(&self, class: u8) -> Option<String> {
        if class >= TRAINER_CLASS_COUNT {
            return None;
        }
        self.string_at(
            TRAINER_CLASS_NAMES + usize::from(class) * TRAINER_CLASS_NAME_LEN,
            TRAINER_CLASS_NAME_LEN,
        )
    }

    /// Every map's wild tables, in ROM order; `None` when the table is malformed.
    pub fn wild_maps(&self) -> Option<Vec<WildMap>> {
        let mut maps = Vec::new();
        for i in 0..WILD_HEADERS_MAX {
            let h = self.bytes_at(WILD_HEADERS + i * WILD_HEADER_SIZE, WILD_HEADER_SIZE)?;
            if h[0] == 0xFF {
                return Some(maps);
            }
            let mut tables = Vec::new();
            let kinds: [(&[u8], Option<WildMethod>); 4] = [
                (&LAND_CHANCES, Some(WildMethod::Land)),
                (&WATER_CHANCES, Some(WildMethod::Water)),
                (&WATER_CHANCES, Some(WildMethod::Rocks)),
                (&FISHING_CHANCES, None),
            ];
            for (k, (chances, method)) in kinds.into_iter().enumerate() {
                let at = WILD_HEADERS + i * WILD_HEADER_SIZE + 4 + k * 4;
                if self.u32_at(at)? == 0 {
                    continue;
                }
                let info = self.ptr_at(at)?;
                let rate = *self.bytes_at(info, 1)?.first()?;
                let slots = self.wild_slots(self.ptr_at(info + 4)?, chances)?;
                match method {
                    Some(method) => tables.push(WildTable {
                        method,
                        rate,
                        slots,
                    }),
                    None => {
                        for (method, range) in FISHING_RODS {
                            tables.push(WildTable {
                                method,
                                rate,
                                slots: slots[range].to_vec(),
                            });
                        }
                    }
                }
            }
            maps.push(WildMap {
                group: h[0],
                num: h[1],
                tables,
            });
        }
        None
    }

    fn wild_slots(&self, at: usize, chances: &[u8]) -> Option<Vec<WildSlot>> {
        let raw = self.bytes_at(at, chances.len() * 4)?;
        raw.as_chunks::<4>()
            .0
            .iter()
            .zip(chances)
            .map(|(&[a, b, lo, hi], &chance)| {
                let species = u16::from_le_bytes([lo, hi]);
                (self.species_exists(species) && a >= 1 && b >= 1 && a.max(b) <= 100).then_some(
                    WildSlot {
                        species,
                        min_level: a.min(b),
                        max_level: a.max(b),
                        chance,
                    },
                )
            })
            .collect()
    }

    /// Number of maps in a map group (0 for unknown groups).
    pub fn map_count(&self, group: u8) -> u8 {
        if group >= MAP_GROUP_COUNT {
            return 0;
        }
        let g = usize::from(group);
        let start = self.ptr_at(MAP_GROUPS + g * 4);
        let end = if group + 1 == MAP_GROUP_COUNT {
            Some(MAP_GROUPS)
        } else {
            self.ptr_at(MAP_GROUPS + (g + 1) * 4)
        };
        match (start, end) {
            (Some(s), Some(e)) if e > s && (e - s) % 4 == 0 => {
                u8::try_from((e - s) / 4).unwrap_or(0)
            }
            _ => 0,
        }
    }

    /// Region map section id of a map; `None` for unknown maps and maps outside any section.
    pub fn map_section(&self, group: u8, num: u8) -> Option<u8> {
        if num >= self.map_count(group) {
            return None;
        }
        let maps = self.ptr_at(MAP_GROUPS + usize::from(group) * 4)?;
        let header = self.ptr_at(maps + usize::from(num) * 4)?;
        let section = *self.bytes_at(header + MAP_HEADER_SECTION, 1)?.first()?;
        (section < MAP_SECTION_COUNT).then_some(section)
    }

    /// Name of a region map section (0..MAP_SECTION_COUNT); `None` for the one section whose
    /// name the game fills in at runtime.
    pub fn map_section_name(&self, section: u8) -> Option<String> {
        if section >= MAP_SECTION_COUNT {
            return None;
        }
        self.string_via(MAP_SECTIONS + usize::from(section) * 8 + 4, 32)
            .filter(|n| !n.is_empty())
    }

    /// The name shown for a map: its region section's name.
    pub fn map_name(&self, group: u8, num: u8) -> Option<String> {
        self.map_section_name(self.map_section(group, num)?)
    }

    pub(super) fn world_tables_look_sane(&self) -> bool {
        let trainers_ok = (1..TRAINER_COUNT).all(|id| self.trainer(id).is_some());
        let classes_ok = (0..TRAINER_CLASS_COUNT).all(|c| self.trainer_class_name(c).is_some());
        let wild_ok = self.wild_maps().is_some_and(|m| !m.is_empty());
        let groups_ok = (0..MAP_GROUP_COUNT).all(|g| {
            let n = self.map_count(g);
            n > 0 && (0..n).all(|m| self.map_header(g, m).is_some())
        });
        let sections_ok = (0..MAP_SECTION_COUNT)
            .all(|s| self.ptr_at(MAP_SECTIONS + usize::from(s) * 8 + 4).is_some());
        trainers_ok && classes_ok && wild_ok && groups_ok && sections_ok
    }

    /// The file offset of a map's header (`num` unchecked against the group's count).
    pub(super) fn map_header(&self, group: u8, num: u8) -> Option<usize> {
        let maps = self.ptr_at(MAP_GROUPS + usize::from(group) * 4)?;
        self.ptr_at(maps + usize::from(num) * 4)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gba::rom::SPECIES_COUNT;
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

    #[test]
    fn chance_tables_add_up() {
        assert_eq!(LAND_CHANCES.iter().map(|&c| u32::from(c)).sum::<u32>(), 100);
        assert_eq!(
            WATER_CHANCES.iter().map(|&c| u32::from(c)).sum::<u32>(),
            100
        );
        for (_, range) in FISHING_RODS {
            let sum: u32 = FISHING_CHANCES[range].iter().map(|&c| u32::from(c)).sum();
            assert_eq!(sum, 100);
        }
    }

    #[test]
    fn rom_trainers() {
        let Some(rom) = rom() else { return };
        assert!(rom.trainer(0).is_none());
        assert!(rom.trainer(TRAINER_COUNT).is_none());
        let (mut doubles, mut custom, mut sizes) = (0, 0, [0usize; PARTY_MAX + 1]);
        for id in 1..TRAINER_COUNT {
            let t = rom.trainer(id).unwrap_or_else(|| panic!("trainer {id}"));
            assert!(
                rom.trainer_class_name(t.class)
                    .is_some_and(|n| !n.is_empty())
            );
            sizes[t.party.len()] += 1;
            doubles += usize::from(t.double);
            for m in &t.party {
                assert!(rom.species(m.species).is_some());
                assert!((1..=100).contains(&m.level) && m.iv <= 31);
                assert!(
                    m.moves
                        .iter()
                        .all(|&mv| mv == 0 || rom.move_info(mv).is_some())
                );
                assert!(m.moves[0] != 0, "trainer {id} has a creature with no moves");
                if m.moves != rom.moves_at_level(m.species, m.level) {
                    custom += 1;
                }
            }
        }
        assert_eq!(sizes[0], 0);
        assert!(sizes[1] > 100 && sizes[6] > 10, "{sizes:?}");
        assert!(doubles > 20, "{doubles}");
        assert!(custom > 100, "{custom}");
        assert!(rom.trainer_class_name(TRAINER_CLASS_COUNT).is_none());
    }

    #[test]
    fn rom_wild_tables() {
        let Some(rom) = rom() else { return };
        let maps = rom.wild_maps().unwrap();
        assert!(
            (100..WILD_HEADERS_MAX).contains(&maps.len()),
            "{}",
            maps.len()
        );
        let mut methods = [0usize; 6];
        for m in &maps {
            assert!(m.group < MAP_GROUP_COUNT && m.num < rom.map_count(m.group));
            assert!(!m.tables.is_empty());
            for t in &m.tables {
                methods[t.method as usize] += 1;
                let sum: u32 = t.slots.iter().map(|s| u32::from(s.chance)).sum();
                assert_eq!(sum, 100);
                let want = match t.method {
                    WildMethod::Land => 12,
                    WildMethod::Water | WildMethod::Rocks | WildMethod::BestRod => 5,
                    WildMethod::BasicRod => 2,
                    WildMethod::BetterRod => 3,
                };
                assert_eq!(t.slots.len(), want);
                for s in &t.slots {
                    assert!(s.species > 0 && s.species < SPECIES_COUNT);
                    assert!(rom.species(s.species).is_some());
                    assert!(1 <= s.min_level && s.min_level <= s.max_level && s.max_level <= 100);
                }
            }
        }
        assert!(methods.iter().all(|&n| n > 0), "{methods:?}");
        // Fishing tables come as all three rods together.
        assert_eq!(methods[3], methods[4]);
        assert_eq!(methods[4], methods[5]);
    }

    #[test]
    fn rom_maps() {
        let Some(rom) = rom() else { return };
        let total: usize = (0..MAP_GROUP_COUNT)
            .map(|g| usize::from(rom.map_count(g)))
            .sum();
        assert!(total > 400, "{total}");
        assert_eq!(rom.map_count(MAP_GROUP_COUNT), 0);
        let unnamed = (0..MAP_SECTION_COUNT)
            .filter(|&s| rom.map_section_name(s).is_none())
            .count();
        assert_eq!(unnamed, 1);
        assert!(rom.map_section_name(MAP_SECTION_COUNT).is_none());
        let mut named = 0;
        for g in 0..MAP_GROUP_COUNT {
            for n in 0..rom.map_count(g) {
                if let Some(name) = rom.map_name(g, n) {
                    assert!(!name.is_empty());
                    named += 1;
                }
            }
            assert!(rom.map_section(g, rom.map_count(g)).is_none());
        }
        assert!(named * 10 > total * 9, "{named}/{total}");
        // Every map with wild creatures has a name.
        for m in rom.wild_maps().unwrap() {
            assert!(rom.map_name(m.group, m.num).is_some());
        }
    }
}
