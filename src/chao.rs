//! Chao data: the game's 0x800-byte `ChaoData` record, kept as raw bytes.
//!
//! Field offsets come from `SA2Structs.h` (`ChaoDataBase`), measured by compiling
//! the header (see re/chao.md). Everything here is pure data handling so it can be
//! unit-tested without the game; reading/writing game memory lives in `game.rs`.

use std::fmt;

pub const CHAO_SIZE: usize = 0x800;

/// Field offsets inside `ChaoDataBase`.
mod off {
    pub const NAME: usize = 0x12;
    pub const NAME_LEN: usize = 7;
    pub const STAT_GRADES: usize = 0x28;
    pub const STAT_LEVELS: usize = 0x30;
    pub const STAT_POINTS: usize = 0x38;
    pub const TYPE: usize = 0x80;
    pub const GARDEN: usize = 0x81;
    pub const HAPPINESS: usize = 0x82;
}

/// Number of stats stored per Chao (swim, fly, run, power, stamina, luck, intelligence, unknown).
pub const STAT_COUNT: usize = 8;

/// `ChaoType`: 0 = empty slot, 1 = egg, 2 = child, 3..=0x19 = adult forms.
pub const TYPE_EMPTY: u8 = 0x00;
/// Eggs live in ChaoSlots too but are spawned by a different game function.
pub const TYPE_EGG: u8 = 0x01;
pub const TYPE_MAX: u8 = 0x19;

/// `SA2BChaoGarden`: 1 = Neutral (Chao Garden), 2 = Hero, 3 = Dark.
pub const GARDEN_MIN: u8 = 1;
pub const GARDEN_MAX: u8 = 3;

/// Highest grade seen in vanilla (E, D, C, B, A, S = 0..=5). X (6) appears with some mods.
pub const GRADE_MAX: u8 = 6;
pub const LEVEL_MAX: u8 = 99;
pub const POINTS_MAX: i16 = 9999;

#[repr(C, align(4))]
#[derive(Clone, PartialEq)]
pub struct ChaoData {
    pub bytes: [u8; CHAO_SIZE],
}

impl ChaoData {
    pub fn from_bytes(bytes: &[u8]) -> Option<Self> {
        Some(Self {
            bytes: bytes.try_into().ok()?,
        })
    }

    pub fn chao_type(&self) -> u8 {
        self.bytes[off::TYPE]
    }

    pub fn garden(&self) -> u8 {
        self.bytes[off::GARDEN]
    }

    pub fn is_empty(&self) -> bool {
        self.chao_type() == TYPE_EMPTY
    }

    pub fn happiness(&self) -> i16 {
        self.i16_at(off::HAPPINESS)
    }

    /// Raw name bytes. The game uses its own character table; not decoded yet.
    pub fn name_raw(&self) -> &[u8] {
        &self.bytes[off::NAME..off::NAME + off::NAME_LEN]
    }

    pub fn grade(&self, stat: usize) -> u8 {
        self.bytes[off::STAT_GRADES + stat]
    }

    pub fn level(&self, stat: usize) -> u8 {
        self.bytes[off::STAT_LEVELS + stat]
    }

    pub fn points(&self, stat: usize) -> i16 {
        self.i16_at(off::STAT_POINTS + stat * 2)
    }

    fn i16_at(&self, at: usize) -> i16 {
        i16::from_le_bytes([self.bytes[at], self.bytes[at + 1]])
    }

    /// Checks that the fields we rely on are in range. Data from files or the
    /// network must pass this before it goes anywhere near the game.
    pub fn validate(&self) -> Result<(), InvalidChao> {
        let t = self.chao_type();
        if t == TYPE_EMPTY || t > TYPE_MAX {
            return Err(InvalidChao::Type(t));
        }
        let g = self.garden();
        if !(GARDEN_MIN..=GARDEN_MAX).contains(&g) {
            return Err(InvalidChao::Garden(g));
        }
        // An egg's stat block isn't filled in until it hatches (seen in game: grades
        // 01 02 03 04 00 00 48 C4), so only hatched Chao get their stats checked.
        if t == TYPE_EGG {
            return Ok(());
        }
        for stat in 0..STAT_COUNT {
            let (grade, level, points) = (self.grade(stat), self.level(stat), self.points(stat));
            if grade > GRADE_MAX || level > LEVEL_MAX || !(0..=POINTS_MAX).contains(&points) {
                return Err(InvalidChao::Stat {
                    stat,
                    grade,
                    level,
                    points,
                });
            }
        }
        Ok(())
    }

    /// One-line description for the log.
    pub fn summary(&self) -> String {
        let name: String = self
            .name_raw()
            .iter()
            .map(|b| format!("{b:02X}"))
            .collect::<Vec<_>>()
            .join(" ");
        let levels: Vec<String> = (0..5).map(|s| self.level(s).to_string()).collect();
        format!(
            "type 0x{:02X}, garden {}, happiness {}, name [{name}], levels swim/fly/run/power/stamina {}",
            self.chao_type(),
            self.garden(),
            self.happiness(),
            levels.join("/"),
        )
    }
}

impl fmt::Debug for ChaoData {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "ChaoData({})", self.summary())
    }
}

#[derive(Debug, PartialEq)]
pub enum InvalidChao {
    Type(u8),
    Garden(u8),
    Stat {
        stat: usize,
        grade: u8,
        level: u8,
        points: i16,
    },
}

impl fmt::Display for InvalidChao {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Self::Type(t) => write!(f, "bad chao type 0x{t:02X}"),
            Self::Garden(g) => write!(f, "bad garden {g}"),
            Self::Stat {
                stat,
                grade,
                level,
                points,
            } => {
                write!(
                    f,
                    "bad stat {stat}: grade {grade}, level {level}, points {points}"
                )
            }
        }
    }
}

/// `.sa2chao` file: 16-byte header + the raw 0x800-byte record.
///
/// | Offset | Size | Field |
/// |---|---|---|
/// | 0 | 8 | magic `SA2CHAO\0` |
/// | 8 | 2 | format version (LE) |
/// | 10 | 2 | reserved, 0 |
/// | 12 | 4 | CRC-32 of the record (LE) |
/// | 16 | 0x800 | `ChaoData` |
pub mod file {
    use super::{CHAO_SIZE, ChaoData, InvalidChao};
    use std::fmt;

    pub const MAGIC: &[u8; 8] = b"SA2CHAO\0";
    pub const VERSION: u16 = 1;
    pub const HEADER_SIZE: usize = 16;
    pub const FILE_SIZE: usize = HEADER_SIZE + CHAO_SIZE;

    pub fn encode(chao: &ChaoData) -> Vec<u8> {
        let mut out = Vec::with_capacity(FILE_SIZE);
        out.extend_from_slice(MAGIC);
        out.extend_from_slice(&VERSION.to_le_bytes());
        out.extend_from_slice(&[0, 0]);
        out.extend_from_slice(&crc32(&chao.bytes).to_le_bytes());
        out.extend_from_slice(&chao.bytes);
        out
    }

    pub fn decode(data: &[u8]) -> Result<ChaoData, FileError> {
        if data.len() != FILE_SIZE {
            return Err(FileError::Size(data.len()));
        }
        if &data[..8] != MAGIC {
            return Err(FileError::Magic);
        }
        let version = u16::from_le_bytes([data[8], data[9]]);
        if version != VERSION {
            return Err(FileError::Version(version));
        }
        let crc = u32::from_le_bytes(data[12..16].try_into().unwrap());
        let body = &data[HEADER_SIZE..];
        if crc32(body) != crc {
            return Err(FileError::Checksum);
        }
        let chao = ChaoData::from_bytes(body).ok_or(FileError::Size(data.len()))?;
        chao.validate().map_err(FileError::Invalid)?;
        Ok(chao)
    }

    #[derive(Debug, PartialEq)]
    pub enum FileError {
        Size(usize),
        Magic,
        Version(u16),
        Checksum,
        Invalid(InvalidChao),
    }

    impl fmt::Display for FileError {
        fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
            match self {
                Self::Size(n) => write!(f, "wrong file size {n} (expected {FILE_SIZE})"),
                Self::Magic => write!(f, "not a .sa2chao file"),
                Self::Version(v) => write!(f, "unsupported format version {v}"),
                Self::Checksum => write!(f, "checksum mismatch"),
                Self::Invalid(e) => write!(f, "{e}"),
            }
        }
    }

    /// CRC-32 (IEEE), bitwise; the data is small.
    pub fn crc32(data: &[u8]) -> u32 {
        let mut crc = !0u32;
        for &b in data {
            crc ^= b as u32;
            for _ in 0..8 {
                crc = if crc & 1 != 0 {
                    (crc >> 1) ^ 0xEDB8_8320
                } else {
                    crc >> 1
                };
            }
        }
        !crc
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> ChaoData {
        let mut c = ChaoData {
            bytes: [0; CHAO_SIZE],
        };
        c.bytes[off::TYPE] = 0x06; // Hero Normal
        c.bytes[off::GARDEN] = 2;
        c.bytes[off::NAME..off::NAME + 3].copy_from_slice(&[0x31, 0x32, 0x33]);
        c.bytes[off::STAT_LEVELS] = 10;
        c.bytes[off::STAT_POINTS..off::STAT_POINTS + 2].copy_from_slice(&500i16.to_le_bytes());
        c
    }

    #[test]
    fn accessors_read_header_offsets() {
        let c = sample();
        assert_eq!(c.chao_type(), 0x06);
        assert_eq!(c.garden(), 2);
        assert_eq!(c.level(0), 10);
        assert_eq!(c.points(0), 500);
        assert_eq!(&c.name_raw()[..3], &[0x31, 0x32, 0x33]);
    }

    #[test]
    fn valid_chao_passes() {
        assert_eq!(sample().validate(), Ok(()));
    }

    #[test]
    fn egg_with_unfilled_stats_passes() {
        // Grades from a real unhatched egg (export of slot 00, 2026-10-14).
        let mut c = sample();
        c.bytes[off::TYPE] = TYPE_EGG;
        c.bytes[off::STAT_GRADES..off::STAT_GRADES + 8]
            .copy_from_slice(&[0x01, 0x02, 0x03, 0x04, 0x00, 0x00, 0x48, 0xC4]);
        assert_eq!(c.validate(), Ok(()));
        // The same bytes on a hatched Chao are still rejected.
        c.bytes[off::TYPE] = 0x02;
        assert!(matches!(
            c.validate(),
            Err(InvalidChao::Stat { stat: 6, .. })
        ));
    }

    #[test]
    fn rejects_bad_fields() {
        let mut c = sample();
        c.bytes[off::TYPE] = 0;
        assert_eq!(c.validate(), Err(InvalidChao::Type(0)));

        let mut c = sample();
        c.bytes[off::TYPE] = 0x7F;
        assert_eq!(c.validate(), Err(InvalidChao::Type(0x7F)));

        let mut c = sample();
        c.bytes[off::GARDEN] = 9;
        assert_eq!(c.validate(), Err(InvalidChao::Garden(9)));

        let mut c = sample();
        c.bytes[off::STAT_LEVELS + 3] = 200;
        assert!(matches!(
            c.validate(),
            Err(InvalidChao::Stat { stat: 3, .. })
        ));

        let mut c = sample();
        c.bytes[off::STAT_POINTS..off::STAT_POINTS + 2].copy_from_slice(&(-1i16).to_le_bytes());
        assert!(matches!(
            c.validate(),
            Err(InvalidChao::Stat { stat: 0, .. })
        ));
    }

    #[test]
    fn file_round_trip() {
        let c = sample();
        let data = file::encode(&c);
        assert_eq!(data.len(), file::FILE_SIZE);
        let back = file::decode(&data).unwrap();
        assert_eq!(back.bytes, c.bytes);
    }

    #[test]
    fn file_rejects_corruption() {
        let mut data = file::encode(&sample());
        data[file::HEADER_SIZE + 100] ^= 0xFF;
        assert_eq!(file::decode(&data).unwrap_err(), file::FileError::Checksum);

        let data = file::encode(&sample());
        assert_eq!(
            file::decode(&data[..100]).unwrap_err(),
            file::FileError::Size(100)
        );

        let mut data = file::encode(&sample());
        data[0] = b'X';
        assert_eq!(file::decode(&data).unwrap_err(), file::FileError::Magic);
    }

    #[test]
    fn file_rejects_invalid_chao_even_with_good_checksum() {
        let mut c = sample();
        c.bytes[off::GARDEN] = 0;
        let data = file::encode(&c);
        assert_eq!(
            file::decode(&data).unwrap_err(),
            file::FileError::Invalid(InvalidChao::Garden(0))
        );
    }

    #[test]
    fn crc32_known_value() {
        assert_eq!(file::crc32(b"123456789"), 0xCBF4_3926);
    }
}
