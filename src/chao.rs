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
/// The ones the game uses and checks: swim .. stamina. The others can keep the egg's filler
/// after hatching (seen in game: grade 0x48 in stat 6 of a child Chao, 2026-10-16).
const CHECKED_STATS: usize = 5;

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
        for stat in 0..CHECKED_STATS {
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

/// What a friend's interaction changed in a visiting copy of a Chao, to apply to the real one
/// (re/chao-garden-items.md 2): numbers travel as differences, so changes the owner's own game
/// made in the meantime are kept; animal parts travel as the new values.
pub mod changes {
    use super::{CHAO_SIZE, ChaoData};

    #[derive(Clone, Copy)]
    enum Kind {
        /// Unsigned byte, kept within min..=max.
        U8(u8, u8),
        /// Signed byte (bond "like"), within ±100.
        I8,
        /// Little-endian i16 within min..=max; `grow_only` drops decreases (lifespans age on
        /// both sides, so only food's increases are forwarded).
        I16(i16, i16, bool),
        /// f32 within min..=max.
        F32(f32, f32),
        /// Byte copied as is (animal parts).
        Set,
        /// u32 bit mask, OR-ed (animals seen).
        Or32,
    }

    /// (offset, count, stride, kind). Not evolution progress (0xC0): the game raises it by
    /// itself over time, in the owner's game too, so a friend's copy forwarding it while they
    /// touch it would count that time twice.
    const FIELDS: &[(usize, usize, usize, Kind)] = &[
        (0x20, 8, 1, Kind::U8(0, 99)),             // stat progress
        (0x30, 8, 1, Kind::U8(0, 99)),             // stat levels
        (0x38, 8, 2, Kind::I16(0, 4000, false)),   // stat points
        (0x82, 1, 2, Kind::I16(-100, 100, false)), // happiness
        (0x8A, 1, 2, Kind::I16(0, 9999, true)),    // lifespan
        (0x8C, 1, 2, Kind::I16(0, 9999, true)),    // max lifespan
        (0xA8, 1, 4, Kind::F32(-1.0, 1.0)),        // power/run
        (0xAC, 1, 4, Kind::F32(-1.0, 1.0)),        // fly/swim
        (0xB0, 1, 4, Kind::F32(-1.0, 1.0)),        // alignment
        (0x118, 1, 4, Kind::Or32),                 // animals seen
        (0x11C, 8, 1, Kind::Set),                  // animal parts
        (0x16C, 6, 6, Kind::I8),                   // bond "like", per character
        (0x16D, 6, 6, Kind::U8(0, 200)),           // bond familiarity, per character
    ];

    /// Offsets a friend's changes may only increase (stat levels, stat points, lifespans).
    const GROW_ONLY: [std::ops::Range<usize>; 2] = [0x30..0x48, 0x8A..0x8E];

    fn read(b: &[u8; CHAO_SIZE], at: usize, kind: Kind) -> f32 {
        match kind {
            Kind::U8(..) | Kind::Set => f32::from(b[at]),
            Kind::I8 => f32::from(b[at] as i8),
            Kind::I16(..) => f32::from(i16::from_le_bytes([b[at], b[at + 1]])),
            Kind::F32(..) => f32::from_le_bytes([b[at], b[at + 1], b[at + 2], b[at + 3]]),
            // Exact for masks below 2^24; the animal mask is 21 + 4 bits.
            Kind::Or32 => u32::from_le_bytes([b[at], b[at + 1], b[at + 2], b[at + 3]]) as f32,
        }
    }

    fn each(mut f: impl FnMut(usize, Kind)) {
        for &(start, count, stride, kind) in FIELDS {
            for i in 0..count {
                f(start + i * stride, kind);
            }
        }
    }

    /// The differences from `before` to `after` worth forwarding: (offset, delta), or for
    /// `Set`/`Or32` fields (offset, new value).
    pub fn diff(before: &ChaoData, after: &ChaoData) -> Vec<(u16, f32)> {
        let mut out = Vec::new();
        each(|at, kind| {
            let (a, b) = (read(&before.bytes, at, kind), read(&after.bytes, at, kind));
            if a == b {
                return;
            }
            let value = match kind {
                Kind::Set | Kind::Or32 => b,
                Kind::I16(_, _, true) if b < a => return,
                _ => b - a,
            };
            out.push((at as u16, value));
        });
        out
    }

    /// Applies `diff` output to `chao` with the game's limits. Unknown offsets are ignored.
    pub fn apply(chao: &mut ChaoData, changes: &[(u16, f32)]) {
        for &(at, value) in changes {
            let at = usize::from(at);
            let mut kind = None;
            each(|o, k| {
                if o == at {
                    kind = Some(k);
                }
            });
            let Some(kind) = kind else { continue };
            // A friend can only add to these, whatever their game sends: stat levels and
            // points, lifespan and max lifespan (a shortened life could end it).
            if value < 0.0 && GROW_ONLY.iter().any(|r| r.contains(&at)) {
                continue;
            }
            let b = &mut chao.bytes;
            let now = read(b, at, kind);
            match kind {
                Kind::U8(min, max) => {
                    b[at] = (now + value).round().clamp(f32::from(min), f32::from(max)) as u8
                }
                Kind::I8 => b[at] = (now + value).round().clamp(-100.0, 100.0) as i8 as u8,
                Kind::I16(min, max, _) => {
                    let v = (now + value).round().clamp(f32::from(min), f32::from(max)) as i16;
                    b[at..at + 2].copy_from_slice(&v.to_le_bytes());
                }
                Kind::F32(min, max) => {
                    let v = (now + value).clamp(min, max);
                    b[at..at + 4].copy_from_slice(&v.to_le_bytes());
                }
                Kind::Set => b[at] = value.clamp(0.0, 255.0) as u8,
                Kind::Or32 => {
                    let v = (now as u32) | (value.max(0.0) as u32);
                    b[at..at + 4].copy_from_slice(&v.to_le_bytes());
                }
            }
        }
    }

    #[cfg(test)]
    mod tests {
        use super::{apply, diff};
        use crate::chao::{CHAO_SIZE, ChaoData};

        fn blank() -> ChaoData {
            ChaoData {
                bytes: [0; CHAO_SIZE],
            }
        }

        #[test]
        fn a_friend_cannot_shorten_life_or_lower_stats() {
            let mut owner = blank();
            owner.bytes[0x8A..0x8C].copy_from_slice(&500i16.to_le_bytes());
            owner.bytes[0x30] = 10;
            apply(&mut owner, &[(0x8A, -500.0), (0x30, -10.0), (0x82, -5.0)]);
            assert_eq!(
                i16::from_le_bytes([owner.bytes[0x8A], owner.bytes[0x8B]]),
                500
            );
            assert_eq!(owner.bytes[0x30], 10);
            // Happiness can drop (hitting it counts, as in the game).
            assert_eq!(owner.bytes[0x82] as i8, -5);
        }

        #[test]
        fn a_friends_petting_and_feeding_add_to_the_owners_chao() {
            let before = blank();
            let mut after = before.clone();
            after.bytes[0x82] = 1; // happiness +1
            after.bytes[0xB0..0xB4].copy_from_slice(&0.05f32.to_le_bytes());
            after.bytes[0x16C + 6] = 1; // bond like +1 (character 1)
            after.bytes[0x38..0x3A].copy_from_slice(&14i16.to_le_bytes()); // swim points
            after.bytes[0x8A..0x8C].copy_from_slice(&3i16.to_le_bytes()); // lifespan +3
            let changes = diff(&before, &after);

            // The owner's Chao has moved on meanwhile; the changes add to it.
            let mut owner = blank();
            owner.bytes[0x82] = 10;
            owner.bytes[0x38..0x3A].copy_from_slice(&100i16.to_le_bytes());
            apply(&mut owner, &changes);
            assert_eq!(owner.bytes[0x82], 11);
            assert_eq!(
                i16::from_le_bytes([owner.bytes[0x38], owner.bytes[0x39]]),
                114
            );
            assert_eq!(owner.bytes[0x16C + 6], 1);
            assert_eq!(
                i16::from_le_bytes([owner.bytes[0x8A], owner.bytes[0x8B]]),
                3
            );
            let align = f32::from_le_bytes(owner.bytes[0xB0..0xB4].try_into().unwrap());
            assert!((align - 0.05).abs() < 1e-6);
        }

        #[test]
        fn aging_and_limits() {
            // Lifespan going down (aging) isn't forwarded; happiness stays within ±100.
            let mut before = blank();
            before.bytes[0x8A..0x8C].copy_from_slice(&50i16.to_le_bytes());
            let mut after = before.clone();
            after.bytes[0x8A..0x8C].copy_from_slice(&49i16.to_le_bytes());
            after.bytes[0x82..0x84].copy_from_slice(&(-30i16).to_le_bytes());
            let changes = diff(&before, &after);
            assert_eq!(changes, vec![(0x82, -30.0)]);
            let mut owner = blank();
            owner.bytes[0x82..0x84].copy_from_slice(&(-90i16).to_le_bytes());
            apply(&mut owner, &changes);
            assert_eq!(
                i16::from_le_bytes([owner.bytes[0x82], owner.bytes[0x83]]),
                -100
            );
            // Unknown offsets from a newer build are ignored.
            apply(&mut owner, &[(0x7FF, 5.0)]);
            assert_eq!(owner.bytes[0x7FF], 0);
        }
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
    fn egg_filler_in_unused_stats_passes() {
        // Grades from a real unhatched egg (export of slot 00, 2026-10-14).
        let mut c = sample();
        c.bytes[off::TYPE] = TYPE_EGG;
        c.bytes[off::STAT_GRADES..off::STAT_GRADES + 8]
            .copy_from_slice(&[0x01, 0x02, 0x03, 0x04, 0x00, 0x00, 0x48, 0xC4]);
        assert_eq!(c.validate(), Ok(()));
        // A Chao just hatched from it keeps that filler in stats 6 and 7: still fine.
        c.bytes[off::TYPE] = 0x02;
        assert_eq!(c.validate(), Ok(()));
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
