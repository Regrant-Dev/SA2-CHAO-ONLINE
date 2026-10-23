//! Reads what a map is like from the GBA ROM: its map type and how many of its blocks have each
//! metatile behaviour (grass, sand, water, ...).

use super::rom::Rom;

/// Map header byte holding the map type (pret `mapType`).
const MAP_HEADER_TYPE: usize = 0x17;
/// Map layout (header +0): {s32 width, s32 height, border*, u16 blocks*, primary tileset*,
/// secondary tileset*}.
const LAYOUT_BLOCKS: usize = 0x0C;
const LAYOUT_TILESETS: [usize; 2] = [0x10, 0x14];
/// Tileset +0x10: a u16 attribute per metatile; its low byte is the behaviour.
const TILESET_ATTRIBUTES: usize = 0x10;
/// A block's metatile is `block & METATILE_MASK`; the primary tileset has the first
/// `PRIMARY_METATILES`, the secondary the rest (from 0).
const METATILE_MASK: u16 = 0x3FF;
const PRIMARY_METATILES: u16 = 512;
/// The largest map read (blocks), well above any real one.
const MAX_BLOCKS: usize = 0x1_0000;

/// Map types (header +0x17).
pub const MAP_TOWN: u8 = 1;
pub const MAP_CITY: u8 = 2;
pub const MAP_ROUTE: u8 = 3;
pub const MAP_UNDERGROUND: u8 = 4;
pub const MAP_UNDERWATER: u8 = 5;
pub const MAP_OCEAN_ROUTE: u8 = 6;
pub const MAP_INDOOR: u8 = 8;

/// Metatile behaviours.
pub const TALL_GRASS: [u8; 4] = [0x02, 0x03, 0x07, 0x09];
pub const SAND: [u8; 3] = [0x06, 0x21, 0x25];
pub const ASH_GRASS: u8 = 0x24;
pub const CAVE_FLOOR: u8 = 0x08;
pub const WATER: [u8; 11] = [
    0x10, 0x11, 0x12, 0x14, 0x15, 0x17, 0x18, 0x19, 0x1A, 0x22, 0x2A,
];

/// How many of a map's blocks have each behaviour.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Behaviours {
    pub counts: Box<[u32; 256]>,
    pub blocks: u32,
}

impl Default for Behaviours {
    fn default() -> Self {
        Self {
            counts: Box::new([0; 256]),
            blocks: 0,
        }
    }
}

impl Behaviours {
    /// Blocks with any of `kinds`.
    pub fn of(&self, kinds: &[u8]) -> u32 {
        kinds.iter().map(|&k| self.counts[usize::from(k)]).sum()
    }

    /// Adds another map's.
    pub fn add(&mut self, other: &Behaviours) {
        for (a, b) in self.counts.iter_mut().zip(other.counts.iter()) {
            *a += b;
        }
        self.blocks += other.blocks;
    }
}

impl Rom {
    /// A map's type (`MAP_TOWN`, ...); `None` for unknown maps.
    pub fn map_type(&self, group: u8, num: u8) -> Option<u8> {
        if num >= self.map_count(group) {
            return None;
        }
        let header = self.map_header(group, num)?;
        self.bytes_at(header + MAP_HEADER_TYPE, 1).map(|b| b[0])
    }

    /// The behaviours of a map's blocks; `None` for unknown maps and malformed layouts.
    pub fn map_behaviours(&self, group: u8, num: u8) -> Option<Behaviours> {
        if num >= self.map_count(group) {
            return None;
        }
        let layout = self.ptr_at(self.map_header(group, num)?)?;
        let size = |at: usize| {
            self.u32_at(layout + at)
                .and_then(|v| usize::try_from(v as i32).ok())
        };
        let (width, height) = (size(0)?, size(4)?);
        let count = width.checked_mul(height)?;
        if count == 0 || count > MAX_BLOCKS {
            return None;
        }
        let blocks = self.bytes_at(self.ptr_at(layout + LAYOUT_BLOCKS)?, count * 2)?;
        let attributes = LAYOUT_TILESETS.map(|at| {
            self.ptr_at(layout + at)
                .and_then(|tileset| self.ptr_at(tileset + TILESET_ATTRIBUTES))
        });
        let mut out = Behaviours::default();
        for b in blocks.as_chunks::<2>().0 {
            let metatile = u16::from_le_bytes(*b) & METATILE_MASK;
            let (table, index) = if metatile < PRIMARY_METATILES {
                (attributes[0], metatile)
            } else {
                (attributes[1], metatile - PRIMARY_METATILES)
            };
            let attribute = self.u16_at(table? + usize::from(index) * 2)?;
            out.counts[usize::from(attribute as u8)] += 1;
        }
        out.blocks = count as u32;
        Some(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gba::world::MAP_GROUP_COUNT;
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
    fn every_wild_map_has_a_type_and_blocks() {
        let Some(rom) = rom() else { return };
        let mut types = std::collections::BTreeSet::new();
        let (mut grass, mut water) = (0, 0);
        for m in rom.wild_maps().unwrap() {
            let t = rom.map_type(m.group, m.num).unwrap();
            types.insert(t);
            let b = rom.map_behaviours(m.group, m.num).unwrap();
            assert!(b.blocks > 0);
            assert_eq!(b.counts.iter().sum::<u32>(), b.blocks);
            grass += b.of(&TALL_GRASS);
            water += b.of(&WATER);
        }
        assert!(grass > 1000 && water > 1000, "{grass} {water}");
        for t in [MAP_ROUTE, MAP_UNDERGROUND, MAP_OCEAN_ROUTE] {
            assert!(types.contains(&t), "{types:?}");
        }
        assert!(rom.map_type(MAP_GROUP_COUNT, 0).is_none());
        assert!(rom.map_behaviours(0, rom.map_count(0)).is_none());
    }
}
