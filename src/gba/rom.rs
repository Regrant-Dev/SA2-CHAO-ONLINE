//! Reads species, move, type and ability data plus front sprites from the GBA ROM (game code BPEE,
//! version 0). Trainers, wild encounters and map names are in `world`.
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
/// Move records (pret `gBattleMoves`): 12 bytes {effect, power, type, accuracy, pp, secondary
/// effect chance, target, priority (signed), flags, 3 pad}.
const MOVES: usize = 0x31C898;
const MOVE_SIZE: usize = 12;
/// Move descriptions (pret `gMoveDescriptionPointers`): a u32 pointer per move id - 1.
const MOVE_DESCRIPTIONS: usize = 0x61C524;
/// Type matchups (pret `gTypeEffectiveness`): {attacking type, defending type, multiplier in
/// tenths} triples. A 0xFE triple separates the matchups that identifying a target removes; a 0xFF
/// triple ends the table.
const TYPE_CHART: usize = 0x31ACE8;
const TYPE_CHART_LEN: usize = 336;
const TYPE_CHART_IDENTIFY: u8 = 0xFE;
const TYPE_CHART_END: u8 = 0xFF;
/// Sound moves (pret `sSoundMovesTable`): LE u16 move ids, ended by 0xFFFF.
const SOUND_MOVES: usize = 0x31BCF8;
const SOUND_MOVES_END: u16 = 0xFFFF;
/// Ability names (pret `gAbilityNames`): 13-byte strings, indexed by ability id.
const ABILITY_NAMES: usize = 0x31B6DB;
const ABILITY_NAME_LEN: usize = 13;
/// Ability descriptions (pret `gAbilityDescriptionPointers`): a u32 pointer per ability id.
const ABILITY_DESCRIPTIONS: usize = 0x31BAD4;
/// TM/HM compatibility (pret `gTMHMLearnsets`): 8 bytes per species id, a little-endian bit set
/// where bit i is TM/HM index i.
const TMHM_LEARNSETS: usize = 0x31E898;
/// The move each TM/HM teaches (pret `sTMHMMoves`): LE u16 per TM/HM index.
const TMHM_MOVES: usize = 0x616040;
/// Experience tables (pret `gExperienceTables`): 6 growth rates × 101 LE u32 (index = level).
const EXP_TABLES: usize = 0x31F72C;
const GROWTH_COUNT: u8 = 6;
/// Species records (pret `gSpeciesInfo`): 28 bytes; base stats @0..6, types @6..8, growth rate
/// @0x13, abilities @0x16..0x18.
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
/// Ability ids, including 0 (none); pret `ABILITIES_COUNT`.
pub const ABILITY_COUNT: u8 = 78;
/// Move effect ids 0..214; pret `NUM_BATTLE_MOVE_EFFECTS`.
pub const EFFECT_COUNT: u8 = 214;
/// TMs are TM/HM indices 0..50, HMs 50..58.
pub const TM_COUNT: u8 = 50;
pub const TMHM_COUNT: u8 = 58;
/// Highest PP any move has before boosts.
const MAX_PP: u8 = 40;

/// Move target bits (`MoveInfo::target`); 0 is one selected target.
pub const TARGET_SELECTED: u8 = 0;
/// Decided by the move's effect (the user's last attacker, or a random foe).
pub const TARGET_DEPENDS: u8 = 1 << 0;
/// The user or a chosen ally.
pub const TARGET_USER_OR_ALLY: u8 = 1 << 1;
pub const TARGET_RANDOM: u8 = 1 << 2;
/// Both foes.
pub const TARGET_BOTH_FOES: u8 = 1 << 3;
pub const TARGET_USER: u8 = 1 << 4;
/// Both foes and the user's ally.
pub const TARGET_ALL_OTHERS: u8 = 1 << 5;
/// The foes' side of the field (entry hazards and the like).
pub const TARGET_FOES_FIELD: u8 = 1 << 6;

/// Move flag bits (`MoveInfo::flags`).
pub const FLAG_CONTACT: u8 = 1 << 0;
/// Blocked by the protecting moves.
pub const FLAG_PROTECTABLE: u8 = 1 << 1;
/// Bounced back by the reflecting move.
pub const FLAG_REFLECTABLE: u8 = 1 << 2;
/// Taken by the stealing move.
pub const FLAG_STEALABLE: u8 = 1 << 3;
/// Repeatable by the copying move.
pub const FLAG_COPYABLE: u8 = 1 << 4;
/// May make the target flinch through the flinch-causing held item.
pub const FLAG_FLINCH_ITEM: u8 = 1 << 5;

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
    /// Ability ids for the two ability slots; the second is 0 when there is only one.
    pub abilities: [u8; 2],
}

/// A move record.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MoveInfo {
    /// Effect id (0..EFFECT_COUNT); 0 is plain damage.
    pub effect: u8,
    /// Base power; 0 for status moves, 1 for moves whose effect sets the damage.
    pub power: u8,
    pub type_id: u8,
    /// Accuracy in percent; 0 never misses.
    pub accuracy: u8,
    pub pp: u8,
    /// Chance in percent that the effect happens on a hit; 0 for moves where it always does.
    pub effect_chance: u8,
    /// `TARGET_*` bits.
    pub target: u8,
    /// Higher goes first; 0 for most moves.
    pub priority: i8,
    /// `FLAG_*` bits.
    pub flags: u8,
}

/// One type matchup.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TypeMatchup {
    pub attack: u8,
    pub defend: u8,
    /// Damage multiplier in tenths: 0, 5 or 20.
    pub tenths: u8,
    /// Not applied while the defender is identified (lets normal and fighting moves hit it).
    pub identify_removes: bool,
}

/// The type effectiveness chart. Pairs not listed are neutral.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TypeChart {
    /// Matchups in ROM order; the game applies each match in this order, rounding down after each.
    pub matchups: Vec<TypeMatchup>,
}

impl TypeChart {
    /// Multiplier in tenths (0, 5, 10 or 20) of `attack` against one defending type.
    pub fn multiplier(&self, attack: u8, defend: u8, identified: bool) -> u8 {
        self.matchups
            .iter()
            .find(|m| {
                m.attack == attack && m.defend == defend && !(identified && m.identify_removes)
            })
            .map_or(10, |m| m.tenths)
    }

    /// Combined multiplier in percent of `attack` against a defender's types (a repeated type
    /// counts once): 0, 25, 50, 100, 200 or 400.
    pub fn effectiveness(&self, attack: u8, defend: [u8; 2], identified: bool) -> u16 {
        let first = u16::from(self.multiplier(attack, defend[0], identified));
        let second = if defend[1] == defend[0] {
            10
        } else {
            u16::from(self.multiplier(attack, defend[1], identified))
        };
        first * second
    }
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
    type_chart: TypeChart,
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
        const DAMAGED: &str = "the GBA ROM's data tables look damaged or modified";
        let mut rom = Rom {
            data,
            type_chart: TypeChart::default(),
        };
        rom.type_chart = rom.read_type_chart().ok_or(DAMAGED)?;
        if !rom.tables_look_sane() || !rom.world_tables_look_sane() {
            return Err(DAMAGED.into());
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
        let abilities_ok = (1..SPECIES_COUNT).all(|id| {
            self.species(id).is_none_or(|s| {
                s.abilities[0] != 0 && s.abilities.iter().all(|&a| a < ABILITY_COUNT)
            })
        }) && (0..ABILITY_COUNT).all(|a| {
            self.ptr_at(ABILITY_DESCRIPTIONS + usize::from(a) * 4)
                .is_some()
        });
        let moves_ok = (1..MOVE_COUNT).all(|id| {
            self.move_info(id).is_some_and(|m| {
                m.type_id < TYPE_COUNT
                    && m.effect < EFFECT_COUNT
                    && m.accuracy <= 100
                    && (1..=MAX_PP).contains(&m.pp)
                    && m.effect_chance <= 100
                    && m.target < TARGET_FOES_FIELD << 1
            }) && self
                .ptr_at(MOVE_DESCRIPTIONS + usize::from(id - 1) * 4)
                .is_some()
        });
        let tmhm_ok = (0..TMHM_COUNT).all(|i| self.tmhm_move(i).is_some());
        species_ok
            && exp_ok
            && pics_ok
            && abilities_ok
            && moves_ok
            && tmhm_ok
            && self.move_info(1).is_some_and(|m| m.power > 0)
    }

    /// Parses the type chart; `None` when it is malformed.
    fn read_type_chart(&self) -> Option<TypeChart> {
        let table = self.data.get(TYPE_CHART..TYPE_CHART + TYPE_CHART_LEN)?;
        let mut chart = TypeChart::default();
        let mut identify_removes = false;
        for &[attack, defend, tenths] in table.as_chunks::<3>().0 {
            match attack {
                TYPE_CHART_END => return (!chart.matchups.is_empty()).then_some(chart),
                TYPE_CHART_IDENTIFY if !identify_removes => identify_removes = true,
                _ => {
                    if attack >= TYPE_COUNT || defend >= TYPE_COUNT || ![0, 5, 20].contains(&tenths)
                    {
                        return None;
                    }
                    chart.matchups.push(TypeMatchup {
                        attack,
                        defend,
                        tenths,
                        identify_removes,
                    });
                }
            }
        }
        None
    }

    pub(super) fn u16_at(&self, off: usize) -> Option<u16> {
        let b = self.data.get(off..off + 2)?;
        Some(u16::from_le_bytes([b[0], b[1]]))
    }

    pub(super) fn u32_at(&self, off: usize) -> Option<u32> {
        let b = self.data.get(off..off + 4)?;
        Some(u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    }

    pub(super) fn bytes_at(&self, off: usize, len: usize) -> Option<&[u8]> {
        self.data.get(off..off.checked_add(len)?)
    }

    /// Reads a ROM pointer at `off` and returns it as a file offset inside the ROM.
    pub(super) fn ptr_at(&self, off: usize) -> Option<usize> {
        let p = self.u32_at(off)?.checked_sub(ROM_BASE)? as usize;
        (p < self.data.len()).then_some(p)
    }

    pub(super) fn string_at(&self, off: usize, len: usize) -> Option<String> {
        Some(text::decode(self.data.get(off..off + len)?))
    }

    /// The 0xFF-terminated string a ROM pointer at `off` points to (at most `max` bytes).
    pub(super) fn string_via(&self, off: usize, max: usize) -> Option<String> {
        let at = self.ptr_at(off)?;
        let end = (at + max).min(self.data.len());
        Some(text::decode(&self.data[at..end]))
    }

    /// True for ids 1..SPECIES_COUNT whose name isn't made only of '?' (placeholders).
    pub(super) fn species_exists(&self, id: u16) -> bool {
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
            abilities: [
                rec[0x16],
                if rec[0x17] == rec[0x16] { 0 } else { rec[0x17] },
            ],
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

    /// Move record for ids 1..MOVE_COUNT.
    pub fn move_info(&self, id: u16) -> Option<MoveInfo> {
        if id == 0 || id >= MOVE_COUNT {
            return None;
        }
        let rec = self.bytes_at(MOVES + usize::from(id) * MOVE_SIZE, MOVE_SIZE)?;
        Some(MoveInfo {
            effect: rec[0],
            power: rec[1],
            type_id: rec[2],
            accuracy: rec[3],
            pp: rec[4],
            effect_chance: rec[5],
            target: rec[6],
            priority: rec[7] as i8,
            flags: rec[8],
        })
    }

    /// Move description for ids 1..MOVE_COUNT; lines are separated by '\n'.
    pub fn move_description(&self, id: u16) -> Option<String> {
        if id == 0 || id >= MOVE_COUNT {
            return None;
        }
        self.string_via(MOVE_DESCRIPTIONS + usize::from(id - 1) * 4, 128)
    }

    /// The ids of the moves the sound-blocking ability stops.
    pub fn sound_moves(&self) -> Vec<u16> {
        let mut out = Vec::new();
        let mut off = SOUND_MOVES;
        while let Some(id) = self.u16_at(off) {
            if id == SOUND_MOVES_END || id == 0 || id >= MOVE_COUNT || out.len() >= 64 {
                break;
            }
            out.push(id);
            off += 2;
        }
        out
    }

    /// The type effectiveness chart.
    pub fn type_chart(&self) -> &TypeChart {
        &self.type_chart
    }

    /// Ability name for ids 1..ABILITY_COUNT.
    pub fn ability_name(&self, id: u8) -> Option<String> {
        if id == 0 || id >= ABILITY_COUNT {
            return None;
        }
        self.string_at(
            ABILITY_NAMES + usize::from(id) * ABILITY_NAME_LEN,
            ABILITY_NAME_LEN,
        )
    }

    /// Ability description for ids 1..ABILITY_COUNT.
    pub fn ability_description(&self, id: u8) -> Option<String> {
        if id == 0 || id >= ABILITY_COUNT {
            return None;
        }
        self.string_via(ABILITY_DESCRIPTIONS + usize::from(id) * 4, 64)
    }

    /// The move TM/HM `index` (0..TMHM_COUNT) teaches.
    pub fn tmhm_move(&self, index: u8) -> Option<u16> {
        if index >= TMHM_COUNT {
            return None;
        }
        let id = self.u16_at(TMHM_MOVES + usize::from(index) * 2)?;
        (id != 0 && id < MOVE_COUNT).then_some(id)
    }

    /// Whether `species` can learn TM/HM `index`.
    pub fn tmhm_compatible(&self, species: u16, index: u8) -> bool {
        if index >= TMHM_COUNT || !self.species_exists(species) {
            return false;
        }
        let off = TMHM_LEARNSETS + usize::from(species) * 8 + usize::from(index / 8);
        self.data
            .get(off)
            .is_some_and(|b| b >> (index % 8) & 1 != 0)
    }

    /// The moves `species` can learn by TM/HM, in TM/HM order.
    pub fn tmhm_moves_for(&self, species: u16) -> Vec<u16> {
        (0..TMHM_COUNT)
            .filter(|&i| self.tmhm_compatible(species, i))
            .filter_map(|i| self.tmhm_move(i))
            .collect()
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

    /// A species' level-up learnset as (level, move id) pairs in ROM order (ascending levels).
    pub fn learnset(&self, species: u16) -> Vec<(u8, u16)> {
        let mut out = Vec::new();
        if !self.species_exists(species) {
            return out;
        }
        let Some(mut at) = self.ptr_at(LEARNSETS + usize::from(species) * 4) else {
            return out;
        };
        // Learnsets are short; the bound keeps a bad table from running on.
        for _ in 0..64 {
            let Some(w) = self.u16_at(at) else { break };
            if w == 0xFFFF {
                break;
            }
            let id = w & 0x1FF;
            if id != 0 && id < MOVE_COUNT {
                out.push(((w >> 9) as u8, id));
            }
            at += 2;
        }
        out
    }

    /// The moves a species knows at `level` by its learnset: the last four learned at or below
    /// it (0 for none), as the game fills a new one's moves.
    pub fn moves_at_level(&self, species: u16, level: u8) -> [u16; 4] {
        let mut moves = [0u16; 4];
        let mut known: Vec<u16> = Vec::new();
        for (lv, id) in self.learnset(species) {
            if lv > level {
                break;
            }
            if !known.contains(&id) {
                known.push(id);
            }
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
    fn rom_move_records() {
        let Some(rom) = rom() else { return };
        let mut effects = std::collections::HashSet::new();
        let (mut spread, mut priority, mut contact) = (0, 0, 0);
        for id in 1..MOVE_COUNT {
            let m = rom.move_info(id).unwrap();
            assert!(m.type_id < TYPE_COUNT && m.effect < EFFECT_COUNT, "{id}");
            assert!(m.accuracy <= 100 && (1..=MAX_PP).contains(&m.pp), "{id}");
            assert!(
                m.effect_chance <= 100 && (-7..=5).contains(&m.priority),
                "{id}"
            );
            effects.insert(m.effect);
            spread += usize::from(m.target & (TARGET_BOTH_FOES | TARGET_ALL_OTHERS) != 0);
            priority += usize::from(m.priority != 0);
            contact += usize::from(m.flags & FLAG_CONTACT != 0);
            assert!(!rom.move_description(id).unwrap().is_empty(), "{id}");
        }
        assert!(effects.len() > 150, "{}", effects.len());
        assert!(spread > 10 && priority > 5 && contact > 100);
        // Move 1 is plain damage: full accuracy, single target, makes contact.
        let m = rom.move_info(1).unwrap();
        assert_eq!((m.effect, m.accuracy, m.target, m.priority), (0, 100, 0, 0));
        assert!(m.flags & FLAG_CONTACT != 0);
        assert!(rom.move_description(0).is_none() && rom.move_description(MOVE_COUNT).is_none());
    }

    #[test]
    fn rom_type_chart() {
        let Some(rom) = rom() else { return };
        let chart = rom.type_chart();
        assert!((100..=112).contains(&chart.matchups.len()));
        let mut pairs = std::collections::HashSet::new();
        for m in &chart.matchups {
            assert!(m.attack < TYPE_COUNT && m.defend < TYPE_COUNT);
            assert!(
                m.attack != 9 && m.defend != 9,
                "the unused type has no matchups"
            );
            assert!(pairs.insert((m.attack, m.defend)), "duplicate pair");
        }
        // Identifying only removes immunities, and they come last.
        let removable: Vec<_> = chart
            .matchups
            .iter()
            .filter(|m| m.identify_removes)
            .collect();
        assert!(!removable.is_empty() && removable.iter().all(|m| m.tenths == 0));
        assert!(
            chart
                .matchups
                .ends_with(&removable.iter().map(|m| **m).collect::<Vec<_>>())
        );
        let r = removable[0];
        assert_eq!(chart.multiplier(r.attack, r.defend, false), 0);
        assert_eq!(chart.multiplier(r.attack, r.defend, true), 10);
        // Fire (10) vs grass (12) is 2×, and also 2× against species 1.
        assert_eq!(chart.multiplier(10, 12, false), 20);
        assert_eq!(
            chart.effectiveness(10, rom.species(1).unwrap().types, false),
            200
        );
        assert_eq!(chart.effectiveness(10, [12, 12], false), 200);
        assert_eq!(chart.multiplier(0, 0, false), 10);
        let all: u32 = (0..TYPE_COUNT)
            .flat_map(|a| (0..TYPE_COUNT).map(move |d| (a, d)))
            .map(|(a, d)| u32::from(chart.multiplier(a, d, false)))
            .sum();
        assert!(all > 18 * 18 * 8 && all < 18 * 18 * 12);
    }

    #[test]
    fn rom_sound_moves() {
        let Some(rom) = rom() else { return };
        let list = rom.sound_moves();
        assert_eq!(list.len(), 10, "{list:?}");
        let distinct: std::collections::HashSet<_> = list.iter().collect();
        assert_eq!(distinct.len(), list.len());
        // Damaging and status moves alike.
        let infos: Vec<MoveInfo> = list.iter().map(|&id| rom.move_info(id).unwrap()).collect();
        assert!(infos.iter().any(|m| m.power > 0) && infos.iter().any(|m| m.power == 0));
        assert_eq!(
            rom.u16_at(SOUND_MOVES + list.len() * 2),
            Some(SOUND_MOVES_END)
        );
    }

    #[test]
    fn rom_abilities() {
        let Some(rom) = rom() else { return };
        for a in 1..ABILITY_COUNT {
            assert!(!rom.ability_name(a).unwrap().is_empty());
            assert!(!rom.ability_description(a).unwrap().is_empty());
        }
        assert!(rom.ability_name(0).is_none() && rom.ability_name(ABILITY_COUNT).is_none());
        let mut used = std::collections::HashSet::<u8>::new();
        let mut two = 0;
        for id in 1..SPECIES_COUNT {
            if let Some(s) = rom.species(id) {
                assert!(s.abilities[0] > 0 && s.abilities[0] < ABILITY_COUNT);
                assert!(s.abilities[1] < ABILITY_COUNT && s.abilities[1] != s.abilities[0]);
                two += usize::from(s.abilities[1] != 0);
                used.extend(s.abilities.iter().filter(|&&a| a != 0));
            }
        }
        assert!(two > 50, "{two}");
        assert!(used.len() > 70, "{}", used.len());
    }

    #[test]
    fn rom_tms_and_hms() {
        let Some(rom) = rom() else { return };
        let moves: Vec<u16> = (0..TMHM_COUNT).map(|i| rom.tmhm_move(i).unwrap()).collect();
        let distinct: std::collections::HashSet<_> = moves.iter().collect();
        assert_eq!(distinct.len(), usize::from(TMHM_COUNT));
        assert!(rom.tmhm_move(TMHM_COUNT).is_none());
        // Species 1 takes TM06 and HM01 (pret's compatibility data), not TM01.
        assert!(rom.tmhm_compatible(1, 5) && rom.tmhm_compatible(1, TM_COUNT));
        assert!(!rom.tmhm_compatible(1, 0));
        assert!(!rom.tmhm_compatible(0, 5) && !rom.tmhm_compatible(1, TMHM_COUNT));
        let mut every = 0;
        for id in 1..SPECIES_COUNT {
            if rom.species(id).is_some() {
                let list = rom.tmhm_moves_for(id);
                assert!(list.iter().all(|m| moves.contains(m)));
                every += usize::from(list.len() > 5);
            }
        }
        assert!(every > 300, "{every}");
        // The unused bits past the last HM are clear.
        for id in 0..SPECIES_COUNT {
            let b = rom.data[TMHM_LEARNSETS + usize::from(id) * 8 + 7];
            assert_eq!(b & 0xFC, 0, "{id}");
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
        let set = rom.learnset(1);
        assert_eq!(set[..2], [(1, 33), (4, 45)]);
        assert!(set.windows(2).all(|w| w[0].0 <= w[1].0));
        assert!(rom.learnset(0).is_empty());
        let starters = rom.starters().unwrap();
        for id in starters {
            assert!(rom.species(id).is_some());
            assert_ne!(rom.moves_at_level(id, 5), [0; 4]);
        }
    }
}
