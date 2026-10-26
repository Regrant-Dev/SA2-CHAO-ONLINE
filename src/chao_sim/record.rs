//! A Chao's save record (`rec` = `Data1+0x5C`, the game's 0x800-byte `ChaoData`), read and
//! written at the game's offsets so the code follows re/chao-ai.md line for line.

use crate::chao::ChaoData;

/// Offsets the brain uses (re/chao-ai.md §3, §4; re/chao-details.md).
pub mod off {
    pub const FLY_POINTS: usize = 0x3A;
    pub const RUN_POINTS: usize = 0x3C;
    pub const TYPE: usize = 0x80;
    pub const HAPPINESS: usize = 0x82;
    pub const AGE: usize = 0x86;
    pub const ADULT_AGE: usize = 0x88;
    pub const LIFE: usize = 0x8A;
    pub const MAX_LIFE: usize = 0x8C;
    pub const REINCARNATIONS: usize = 0x8E;
    pub const AGE_CLOCK: usize = 0xA4;
    pub const ALIGNMENT: usize = 0xB0;
    pub const EVOLUTION: usize = 0xC0;
    pub const HAT: usize = 0xD5;
    pub const EMOTION_CLOCK: usize = 0x126;
    pub const CONDITION_CLOCK: usize = 0x128;
    pub const NEED_CLOCK: usize = 0x12A;
    /// Emotion bytes 0x12C..=0x133, 0..200.
    pub const EMOTIONS: usize = 0x12C;
    /// Need words 0x134..=0x148, 0..10000.
    pub const SLEEPY: usize = 0x134;
    pub const SLPDPTH: usize = 0x136;
    pub const HUNGER: usize = 0x138;
    pub const BREED: usize = 0x13A;
    pub const TEDIOUS: usize = 0x13C;
    pub const LONELY: usize = 0x13E;
    pub const TIRE: usize = 0x140;
    pub const STRESS: usize = 0x142;
    pub const NOURISH: usize = 0x144;
    pub const CONDITN: usize = 0x146;
    pub const THIRSTY: usize = 0x148;
    /// Personality bytes 0x14A..=0x156 (signed).
    pub const PERSONALITY: usize = 0x14A;
    /// Illness slots 0x15A..=0x15F (signed; negative = ill).
    pub const ILLNESS: usize = 0x15A;
    pub const BOND_CLOCK: usize = 0x168;
    /// Six character bonds of 6 bytes from 0x16C: +0 like (i8), +1 fear (0..200, `0x53E120`,
    /// `0x53E1F0`), +2 u16, +4 u16 count.
    pub const BONDS: usize = 0x16C;
    /// Twenty more bond words, 0x1C apart (`0x53E2D0`).
    pub const OTHER_BONDS: usize = 0x1A6;
}

/// ChaoType values.
pub const TYPE_CHILD: u8 = 2;

#[derive(Clone)]
pub struct Record(pub ChaoData);

impl std::fmt::Debug for Record {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Record")
    }
}

impl Default for Record {
    fn default() -> Self {
        Self(ChaoData {
            bytes: [0; crate::chao::CHAO_SIZE],
        })
    }
}

impl Record {
    pub fn u8(&self, at: usize) -> u8 {
        self.0.bytes[at]
    }
    pub fn i8(&self, at: usize) -> i8 {
        self.0.bytes[at] as i8
    }
    pub fn u16(&self, at: usize) -> u16 {
        u16::from_le_bytes([self.0.bytes[at], self.0.bytes[at + 1]])
    }
    pub fn i16(&self, at: usize) -> i16 {
        self.u16(at) as i16
    }
    pub fn u32(&self, at: usize) -> u32 {
        u32::from_le_bytes(self.0.bytes[at..at + 4].try_into().unwrap())
    }
    pub fn f32(&self, at: usize) -> f32 {
        f32::from_bits(self.u32(at))
    }
    pub fn set_u8(&mut self, at: usize, v: u8) {
        self.0.bytes[at] = v;
    }
    pub fn set_u16(&mut self, at: usize, v: u16) {
        self.0.bytes[at..at + 2].copy_from_slice(&v.to_le_bytes());
    }
    pub fn set_u32(&mut self, at: usize, v: u32) {
        self.0.bytes[at..at + 4].copy_from_slice(&v.to_le_bytes());
    }
    pub fn set_f32(&mut self, at: usize, v: f32) {
        self.set_u32(at, v.to_bits());
    }

    /// Personality byte `0x14A + i`.
    pub fn p(&self, at: usize) -> i32 {
        i32::from(self.i8(at))
    }

    /// Stat points as the garden compares them: ×3 as 16 bits (`0x5A3650`).
    pub fn scaled(&self, at: usize) -> u16 {
        self.u16(at).wrapping_mul(3)
    }

    /// A need word plus `by`, clamped 0..10000.
    pub fn add_need(&mut self, at: usize, by: i32) {
        self.set_u16(at, (i32::from(self.u16(at)) + by).clamp(0, 10_000) as u16);
    }

    /// An emotion byte plus `by`, clamped 0..200.
    pub fn add_emotion(&mut self, at: usize, by: i32) {
        self.set_u8(at, (i32::from(self.u8(at)) + by).clamp(0, 200) as u8);
    }

    /// The once-per-need-period latch (`0x536420`): true the first time it's asked after a
    /// need tick. Setting it brings the next need tick one life tick nearer, as in the game.
    pub fn need_latch(&mut self) -> bool {
        if self.u16(off::NEED_CLOCK) == 0 {
            self.set_u16(off::NEED_CLOCK, 1);
            true
        } else {
            false
        }
    }
}

/// `trunc(c·(300 + p)/300)`, the personality scale (toward zero).
pub fn scale(c: i32, p: i32) -> i32 {
    c * (300 + p) / 300
}
