//! Reads species, move and type data plus front sprites from the GBA ROM (game code BPEE, version 0).
//!
//! All offsets are file offsets into the 16 MiB ROM; ROM pointers are those offsets + 0x0800_0000.

use std::path::Path;

use super::{lz77, text};

/// ROM file size.
const ROM_SIZE: usize = 16 << 20;
/// Game code at 0xAC.
const GAME_CODE: &[u8; 4] = b"BPEE";
/// Software version byte at 0xBC.
const VERSION: u8 = 0;
/// Base address of cartridge ROM in GBA memory.
const ROM_BASE: u32 = 0x0800_0000;

/// Species names (pret `gSpeciesNames`): 11-byte strings, indexed by species id.
const SPECIES_NAMES: usize = 0x3185C8;
const SPECIES_NAME_LEN: usize = 11;
/// Move names (pret `gMoveNames`): 13-byte strings, indexed by move id.
const MOVE_NAMES: usize = 0x31977C;
const MOVE_NAME_LEN: usize = 13;
/// Type names (pret `gTypeNames`): 7-byte strings, indexed by type id.
const TYPE_NAMES: usize = 0x31AE38;
const TYPE_NAME_LEN: usize = 7;
/// Move records (pret `gBattleMoves`): 12 bytes; power @1, type @2.
const MOVES: usize = 0x31C898;
const MOVE_SIZE: usize = 12;
/// Experience tables (pret `gExperienceTables`): 6 growth rates × 101 LE u32 (index = level).
const EXP_TABLES: usize = 0x31F72C;
const GROWTH_COUNT: u8 = 6;
/// Species records (pret `gSpeciesInfo`): 28 bytes; base stats @0..6, types @6..8, growth rate @0x13.
const SPECIES_INFO: usize = 0x3203CC;
const SPECIES_INFO_SIZE: usize = 28;
/// Front pics (pret `gMonFrontPicTable`): 8 bytes {u32 ptr to LZ77 4bpp tiles, u16 size, u16 tag}.
const FRONT_PICS: usize = 0x30A18C;
/// Normal palettes (pret `gMonPaletteTable`): 8 bytes {u32 ptr to LZ77 BGR555 colours, u16 tag, pad}.
const PALETTES: usize = 0x303678;
/// Level-up learnsets (pret `gLevelUpLearnsets`): a u32 pointer per species id to LE u16 entries
/// `level << 9 | move id`, ended by 0xFFFF.
const LEARNSETS: usize = 0x32937C;
/// The new game's three first-partner species ids (pret `sStarterMon`): 3 LE u16.
const STARTERS: usize = 0x5B1DF8;

/// Species ids in use, including 0 (none); pret `NUM_SPECIES`. Ids 252..=276 are placeholders
/// whose ROM name is a lone '?' and are reported as absent.
pub const SPECIES_COUNT: u16 = 412;
/// Move ids, including 0 (none); pret `MOVES_COUNT`.
pub const MOVE_COUNT: u16 = 355;
/// Type ids 0..18. Id 9 is an unused type whose ROM name is "???".
pub const TYPE_COUNT: u8 = 18;

/// One frame of a front sprite: 64×64 pixels, 4bpp tiles of 8×8.
const FRAME_SIDE: u32 = 64;
const FRAME_BYTES: usize = (FRAME_SIDE * FRAME_SIDE / 2) as usize;
/// One 16-colour BGR555 palette.
const PALETTE_BYTES: usize = 32;

/// A species record.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Species {
    /// Primary and secondary type ids (equal for single-typed species).
    pub types: [u8; 2],
    /// Base HP, attack, defense, speed, sp. attack, sp. defense (ROM order).
    pub base: [u8; 6],
    /// Growth rate: index into the experience tables (0..6).
    pub growth: u8,
}

/// A move record.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MoveInfo {
    pub type_id: u8,
    /// Base power; 0 for status moves.
    pub power: u8,
}

/// Decoded sprite frames.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Sprite {
    pub width: u32,
    pub height: u32,
    /// RGBA8, row-major, `width * height * 4` bytes each.
    pub frames: Vec<Vec<u8>>,
}

/// A loaded GBA ROM.
pub struct Rom {
    data: Vec<u8>,
}

impl Rom {
    /// Reads and checks a ROM file.
    pub fn open(path: &Path) -> Result<Rom, String> {
        let data = std::fs::read(path).map_err(|e| format!("can't read the GBA ROM: {e}"))?;
        Rom::from_bytes(data)
    }

    /// Checks size, header and table plausibility.
    pub fn from_bytes(data: Vec<u8>) -> Result<Rom, String> {
        const UNSUPPORTED: &str = "not a supported GBA ROM (needs game code BPEE, version 0)";
        if data.len() != ROM_SIZE || &data[0xAC..0xB0] != GAME_CODE || data[0xBC] != VERSION {
            return Err(UNSUPPORTED.into());
        }
        let rom = Rom { data };
        if !rom.tables_look_sane() {
            return Err("the GBA ROM's data tables look damaged or modified".into());
        }
        Ok(rom)
    }

    fn tables_look_sane(&self) -> bool {
        let species_ok = self.species(1).is_some_and(|s| {
            s.types.iter().all(|&t| t < TYPE_COUNT) && s.growth < GROWTH_COUNT && s.base[0] > 0
        });
        let exp_ok = (0..GROWTH_COUNT).all(|g| {
            self.exp_at(g, 0) == Some(0)
                && (1..=100).all(|lv| self.exp_at(g, lv - 1) <= self.exp_at(g, lv))
        });
        let pics_ok = (0..SPECIES_COUNT).all(|id| {
            let i = usize::from(id);
            self.ptr_at(FRONT_PICS + i * 8).is_some() && self.ptr_at(PALETTES + i * 8).is_some()
        });
        species_ok && exp_ok && pics_ok && self.move_info(1).is_some_and(|m| m.power > 0)
    }

    fn u16_at(&self, off: usize) -> Option<u16> {
        let b = self.data.get(off..off + 2)?;
        Some(u16::from_le_bytes([b[0], b[1]]))
    }

    fn u32_at(&self, off: usize) -> Option<u32> {
        let b = self.data.get(off..off + 4)?;
        Some(u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    }

    /// Reads a ROM pointer at `off` and returns it as a file offset inside the ROM.
    fn ptr_at(&self, off: usize) -> Option<usize> {
        let p = self.u32_at(off)?.checked_sub(ROM_BASE)? as usize;
        (p < self.data.len()).then_some(p)
    }

    fn string_at(&self, off: usize, len: usize) -> Option<String> {
        Some(text::decode(self.data.get(off..off + len)?))
    }

    /// True for ids 1..SPECIES_COUNT whose name isn't made only of '?' (placeholders).
    fn species_exists(&self, id: u16) -> bool {
        if id == 0 || id >= SPECIES_COUNT {
            return false;
        }
        self.string_at(
            SPECIES_NAMES + usize::from(id) * SPECIES_NAME_LEN,
            SPECIES_NAME_LEN,
        )
        .is_some_and(|n| n.chars().any(|c| c != '?'))
    }

    /// Species record; `None` for 0, out-of-range and placeholder ids.
    pub fn species(&self, id: u16) -> Option<Species> {
        if !self.species_exists(id) {
            return None;
        }
        let off = SPECIES_INFO + usize::from(id) * SPECIES_INFO_SIZE;
        let rec = self.data.get(off..off + SPECIES_INFO_SIZE)?;
        let mut base = [0; 6];
        base.copy_from_slice(&rec[..6]);
        Some(Species {
            types: [rec[6], rec[7]],
            base,
            growth: rec[0x13],
        })
    }

    /// Species name; `None` for 0, out-of-range and placeholder ids.
    pub fn species_name(&self, id: u16) -> Option<String> {
        if !self.species_exists(id) {
            return None;
        }
        self.string_at(
            SPECIES_NAMES + usize::from(id) * SPECIES_NAME_LEN,
            SPECIES_NAME_LEN,
        )
    }

    /// Move name for ids 1..MOVE_COUNT.
    pub fn move_name(&self, id: u16) -> Option<String> {
        if id == 0 || id >= MOVE_COUNT {
            return None;
        }
        self.string_at(MOVE_NAMES + usize::from(id) * MOVE_NAME_LEN, MOVE_NAME_LEN)
    }

    /// Move type and power for ids 1..MOVE_COUNT.
    pub fn move_info(&self, id: u16) -> Option<MoveInfo> {
        if id == 0 || id >= MOVE_COUNT {
            return None;
        }
        let off = MOVES + usize::from(id) * MOVE_SIZE;
        let rec = self.data.get(off..off + MOVE_SIZE)?;
        Some(MoveInfo {
            type_id: rec[2],
            power: rec[1],
        })
    }

    /// Type name for ids 0..TYPE_COUNT.
    pub fn type_name(&self, type_id: u8) -> Option<String> {
        if type_id >= TYPE_COUNT {
            return None;
        }
        self.string_at(
            TYPE_NAMES + usize::from(type_id) * TYPE_NAME_LEN,
            TYPE_NAME_LEN,
        )
    }

    /// The moves a species knows at `level` by its learnset: the last four learned at or below
    /// it (0 for none), as the game fills a new one's moves.
    pub fn moves_at_level(&self, species: u16, level: u8) -> [u16; 4] {
        let mut moves = [0u16; 4];
        if !self.species_exists(species) {
            return moves;
        }
        let Some(mut at) = self.ptr_at(LEARNSETS + usize::from(species) * 4) else {
            return moves;
        };
        let mut known: Vec<u16> = Vec::new();
        // Learnsets are short; the bound keeps a bad table from running on.
        for _ in 0..64 {
            let Some(w) = self.u16_at(at) else { break };
            if w == 0xFFFF || (w >> 9) as u8 > level {
                break;
            }
            let id = w & 0x1FF;
            if id != 0 && id < MOVE_COUNT && !known.contains(&id) {
                known.push(id);
            }
            at += 2;
        }
        let start = known.len().saturating_sub(4);
        for (slot, id) in moves.iter_mut().zip(&known[start..]) {
            *slot = *id;
        }
        moves
    }

    /// The new game's three first-partner species ids, when all three are real species.
    pub fn starters(&self) -> Option<[u16; 3]> {
        let ids = [
            self.u16_at(STARTERS)?,
            self.u16_at(STARTERS + 2)?,
            self.u16_at(STARTERS + 4)?,
        ];
        ids.iter().all(|&id| self.species_exists(id)).then_some(ids)
    }

    fn exp_at(&self, growth: u8, level: usize) -> Option<u32> {
        if growth >= GROWTH_COUNT || level > 100 {
            return None;
        }
        self.u32_at(EXP_TABLES + (usize::from(growth) * 101 + level) * 4)
    }

    /// Level (1..=100) reached with `exp` points: the highest level whose table entry is ≤ `exp`.
    /// An unknown growth rate gives 1.
    pub fn level_for_exp(&self, growth: u8, exp: u32) -> u8 {
        let mut level = 1;
        for lv in 2..=100u8 {
            match self.exp_at(growth, usize::from(lv)) {
                Some(need) if need <= exp => level = lv,
                _ => break,
            }
        }
        level
    }

    /// Front sprite frames (64×64 each). Sheets are 64×(64·n); a sheet whose palette entry holds
    /// several 16-colour palettes stores one form per palette, and only the first form is returned.
    /// Colour index 0 is transparent.
    pub fn front_sprite(&self, species: u16) -> Option<Sprite> {
        if !self.species_exists(species) {
            return None;
        }
        let i = usize::from(species);
        let tiles = lz77::decompress(self.data.get(self.ptr_at(FRONT_PICS + i * 8)?..)?)?;
        let pal = lz77::decompress(self.data.get(self.ptr_at(PALETTES + i * 8)?..)?)?;
        let forms = pal.len() / PALETTE_BYTES;
        let total = tiles.len() / FRAME_BYTES;
        if forms == 0 || total == 0 {
            return None;
        }
        let colours: Vec<[u8; 4]> = (0..16)
            .map(|c| {
                let v = u16::from_le_bytes([pal[c * 2], pal[c * 2 + 1]]);
                let ch = |s: u16| {
                    let x = (v >> s & 31) as u8;
                    x << 3 | x >> 2
                };
                [ch(0), ch(5), ch(10), if c == 0 { 0 } else { 255 }]
            })
            .collect();
        let per_form = (total / forms).max(1);
        let frames = (0..per_form.min(total))
            .map(|f| decode_frame(&tiles[f * FRAME_BYTES..(f + 1) * FRAME_BYTES], &colours))
            .collect();
        Some(Sprite {
            width: FRAME_SIDE,
            height: FRAME_SIDE,
            frames,
        })
    }
}

/// Converts one 64×64 frame of 4bpp 8×8 tiles (row-major tiles; low nibble = left pixel) to RGBA8.
fn decode_frame(tiles: &[u8], colours: &[[u8; 4]]) -> Vec<u8> {
    let side = FRAME_SIDE as usize;
    let mut out = vec![0u8; side * side * 4];
    for (i, &byte) in tiles.iter().enumerate() {
        let tile = i / 32;
        let (tx, ty) = (tile % (side / 8), tile / (side / 8));
        let within = i % 32;
        let (px, py) = ((within % 4) * 2, within / 4);
        for (k, idx) in [byte & 0x0F, byte >> 4].into_iter().enumerate() {
            let (x, y) = (tx * 8 + px + k, ty * 8 + py);
            let o = (y * side + x) * 4;
            out[o..o + 4].copy_from_slice(&colours[usize::from(idx)]);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn zeroed_rom_rejected() {
        assert!(Rom::from_bytes(vec![0; ROM_SIZE]).is_err());
        assert!(Rom::from_bytes(vec![]).is_err());
        let mut fake = vec![0; ROM_SIZE];
        fake[0xAC..0xB0].copy_from_slice(GAME_CODE);
        assert!(Rom::from_bytes(fake).is_err());
    }

    #[test]
    fn decode_frame_layout() {
        let mut colours = vec![[0u8; 4]; 16];
        colours[1] = [1, 1, 1, 255];
        colours[2] = [2, 2, 2, 255];
        let mut tiles = vec![0u8; FRAME_BYTES];
        tiles[0] = 0x21; // tile 0: pixel (0,0)=1, (1,0)=2
        tiles[32 + 4] = 0x10; // tile 1, row 1: pixel (9,1)=1
        tiles[8 * 32] = 0x02; // tile 8 (second tile row): pixel (0,8)=2
        let f = decode_frame(&tiles, &colours);
        let px = |x: usize, y: usize| f[(y * 64 + x) * 4];
        assert_eq!(
            (px(0, 0), px(1, 0), px(9, 1), px(0, 8), px(8, 1)),
            (1, 2, 1, 2, 0)
        );
    }

    #[test]
    fn rom_species_and_names() {
        let Some(rom) = rom() else { return };
        assert!(rom.species(0).is_none());
        assert!(rom.species(SPECIES_COUNT).is_none());
        let s = rom.species(1).unwrap();
        assert!(s.base.iter().all(|&b| b > 0));
        assert!(s.types.iter().all(|&t| t < TYPE_COUNT));
        // Type ids per pret's species data.
        assert_eq!(s.types, [12, 3]);
        assert_eq!(rom.species(4).unwrap().types, [10, 10]);
        assert_eq!(rom.species(7).unwrap().types, [11, 11]);
        assert_eq!(rom.species(25).unwrap().types, [13, 13]);
        // Placeholder range.
        for id in 252..=276 {
            assert!(rom.species(id).is_none(), "{id}");
            assert!(rom.species_name(id).is_none());
        }
        let mut real = 0;
        for id in 1..SPECIES_COUNT {
            if let Some(n) = rom.species_name(id) {
                assert!(!n.is_empty());
                assert!(rom.species(id).unwrap().growth < GROWTH_COUNT);
                real += 1;
            }
        }
        assert_eq!(real, usize::from(SPECIES_COUNT) - 1 - 25);
    }

    #[test]
    fn rom_moves_types_levels() {
        let Some(rom) = rom() else { return };
        let m = rom.move_info(1).unwrap();
        assert!(m.power > 0 && m.type_id < TYPE_COUNT);
        assert!(rom.move_info(0).is_none() && rom.move_info(MOVE_COUNT).is_none());
        assert!(!rom.move_name(1).unwrap().is_empty());
        assert!(!rom.move_name(MOVE_COUNT - 1).unwrap().is_empty());
        for t in 0..TYPE_COUNT {
            assert!(!rom.type_name(t).unwrap().is_empty());
        }
        assert!(rom.type_name(TYPE_COUNT).is_none());
        for g in 0..GROWTH_COUNT {
            assert_eq!(rom.level_for_exp(g, 0), 1);
            assert_eq!(rom.level_for_exp(g, u32::MAX), 100);
            assert_eq!(rom.level_for_exp(g, 2_000_000), 100);
        }
        // Medium-fast growth is level³.
        assert_eq!(rom.level_for_exp(0, 125), 5);
        assert_eq!(rom.level_for_exp(0, 124), 4);
        assert_eq!(rom.level_for_exp(GROWTH_COUNT, 1000), 1);
    }

    #[test]
    fn rom_front_sprites() {
        let Some(rom) = rom() else { return };
        let s = rom.front_sprite(1).unwrap();
        assert_eq!((s.width, s.height), (64, 64));
        assert!((1..=2).contains(&s.frames.len()));
        for f in &s.frames {
            assert_eq!(f.len(), 64 * 64 * 4);
            assert!(f.chunks(4).any(|p| p[3] == 0));
            assert!(f.chunks(4).any(|p| p[3] == 255));
        }
        assert!(rom.front_sprite(0).is_none());
        for id in 1..SPECIES_COUNT {
            if rom.species(id).is_some() {
                let s = rom
                    .front_sprite(id)
                    .unwrap_or_else(|| panic!("sprite {id}"));
                assert!((1..=2).contains(&s.frames.len()), "{id}");
            }
        }
    }

    #[test]
    fn rom_learnsets_and_first_partners() {
        let Some(rom) = rom() else { return };
        // Species 1 learns move 33 at level 1 and move 45 at level 4 (pret's learnset data).
        assert_eq!(rom.moves_at_level(1, 1), [33, 0, 0, 0]);
        assert_eq!(rom.moves_at_level(1, 5), [33, 45, 0, 0]);
        let many = rom.moves_at_level(1, 100);
        assert!(many.iter().all(|&m| m > 0 && m < MOVE_COUNT));
        assert_eq!(rom.moves_at_level(0, 50), [0; 4]);
        let starters = rom.starters().unwrap();
        for id in starters {
            assert!(rom.species(id).is_some());
            assert_ne!(rom.moves_at_level(id, 5), [0; 4]);
        }
    }
}
