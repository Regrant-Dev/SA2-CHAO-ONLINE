//! The Chao motion player (the `MotionTable` at `D+0xAC`, re/chao-garden-sync.md §2): when a
//! motion ends, what posture it leaves the Chao in, and what follows it. Behaviours read its
//! end pulses (`D+0xAC & 1`, `& 8`) and posture (`D+0xAE`).
//!
//! The timings come from the Chao DLL's `al_motion_table` (Data_DLL, 622 entries of 0x1C
//! bytes). The mod reads it (`MotionTable::read`) and sends it up once (`to_bytes`); until a
//! table arrives every motion is `FALLBACK` (60 frames, posture 0).

use super::land::Mem;
use std::sync::Arc;

/// The Data_DLL export holding the table (looked up by `0x55C370`).
pub const TABLE_EXPORT: &str = "al_motion_table";
/// Entries in the game's table; reading stops at the first malformed one before this cap.
pub const GAME_MOTIONS: usize = 622;
/// Most entries a table may have (read or sent).
pub const MAX_MOTIONS: usize = 1024;
/// Bytes per table entry in the game.
const ENTRY: u32 = 0x1C;

/// Flags (`+0`): 1 end pulse, 2 blending into `next`, 4 both motions advance while blending,
/// 8 held at the end (play mode 3), 0x10 the blend is a play-mode-2 wrap.
pub mod flag {
    pub const END: u16 = 1;
    pub const BLEND: u16 = 2;
    pub const BOTH: u16 = 4;
    pub const HOLD: u16 = 8;
    pub const WRAP: u16 = 0x10;
}

/// Blend step `0x793C40` uses when the entry's blend length is negative (`0x904DCC`, 1/15).
const DEFAULT_BLEND_STEP: f32 = 0.066_666_67;

/// One `al_motion_table` entry (`{motion*, u16 mode, u16 posture, int next, int blend, f32
/// start, f32 end, f32 step}`), with the NJS_MOTION's frame count (`motion+4`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MotionEntry {
    /// Play mode at the end (+4, jump table `0x793C1C`): 0 loop, 1 loop through end+1,
    /// 2 loop with a wrap blend, 3 hold, 4 hold then blend to `next`, 5 switch to `next`,
    /// 6 switch to `next` keeping the overshoot, 7 ping-pong.
    pub mode: u16,
    /// Posture class (+6, copied to `D+0xAE` when the motion starts): 0 standing, 1 sitting,
    /// 2 lying, 3 on the belly (crawl, fallen), 4 and 5 others.
    pub posture: u16,
    /// The motion after this one (+8), -1 none.
    pub next: i32,
    /// Blend length in frames (+0xC); negative means the default (`0x793C40`: 15 frames;
    /// `0x793CB0`: -blend frames).
    pub blend: i32,
    /// Frame range and step per game frame (+0x10, +0x14, +0x18).
    pub start: f32,
    pub end: f32,
    pub step: f32,
    /// The NJS_MOTION's frame count (`nbFrame`, motion+4). The player doesn't use it (the
    /// game doesn't either); kept for drawing.
    pub frames: u32,
}

/// What every motion is without a table: done after 60 frames, posture 0 (the brain's old
/// stand-in), held at the end like play mode 3, a one-frame link. (A blend of 0 would be a
/// step of 1/0, which the first advance, at speed 0, turns into NaN for good.)
pub const FALLBACK: MotionEntry = MotionEntry {
    mode: 3,
    posture: 0,
    next: -1,
    blend: 1,
    start: 0.0,
    end: 60.0,
    step: 1.0,
    frames: 61,
};

impl MotionEntry {
    fn valid(&self, count: usize) -> bool {
        self.mode <= 7
            && (-1..count as i64).contains(&i64::from(self.next))
            && self.blend.unsigned_abs() <= 100_000
            && [self.start, self.end, self.step]
                .iter()
                .all(|f| f.is_finite() && f.abs() <= 1e5)
            && self.frames <= 100_000
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct MotionTable(pub Vec<MotionEntry>);

const MAGIC: &[u8; 8] = b"SA2MOTN1";
const PACKED: usize = 28;

impl MotionTable {
    /// The table at `table` (the `al_motion_table` export): entries until the first one with
    /// no readable motion or out-of-range fields (the game has no count; its 622 entries are
    /// followed by other data).
    pub fn read(mem: &impl Mem, table: u32) -> Option<Self> {
        let mut out = Vec::new();
        for i in 0..MAX_MOTIONS as u32 {
            let at = table.checked_add(i * ENTRY)?;
            let Some(motion) = mem.u32(at).filter(|&m| m != 0) else {
                break;
            };
            let Some(frames) = mem.u32(motion.wrapping_add(4)) else {
                break;
            };
            let (Some(mode), Some(posture), Some(next), Some(blend)) = (
                mem.u16(at + 4),
                mem.u16(at + 6),
                mem.u32(at + 8),
                mem.u32(at + 0xC),
            ) else {
                break;
            };
            let (Some(start), Some(end), Some(step)) =
                (mem.f32(at + 0x10), mem.f32(at + 0x14), mem.f32(at + 0x18))
            else {
                break;
            };
            let e = MotionEntry {
                mode,
                posture,
                next: next as i32,
                blend: blend as i32,
                start,
                end,
                step,
                frames,
            };
            if !e.valid(MAX_MOTIONS) {
                break;
            }
            out.push(e);
        }
        // A `next` past the end means the walk ran into other data: refuse it.
        let n = out.len();
        (n > 0 && out.iter().all(|e| e.next < n as i32)).then_some(Self(out))
    }

    /// The entry for `id`, or `FALLBACK` when out of range (the game would read past its
    /// table).
    pub fn get(&self, id: i32) -> MotionEntry {
        usize::try_from(id)
            .ok()
            .and_then(|i| self.0.get(i))
            .copied()
            .unwrap_or(FALLBACK)
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(12 + self.0.len() * PACKED);
        out.extend_from_slice(MAGIC);
        out.extend_from_slice(&(self.0.len() as u32).to_le_bytes());
        for e in &self.0 {
            out.extend_from_slice(&e.mode.to_le_bytes());
            out.extend_from_slice(&e.posture.to_le_bytes());
            out.extend_from_slice(&e.next.to_le_bytes());
            out.extend_from_slice(&e.blend.to_le_bytes());
            out.extend_from_slice(&e.start.to_le_bytes());
            out.extend_from_slice(&e.end.to_le_bytes());
            out.extend_from_slice(&e.step.to_le_bytes());
            out.extend_from_slice(&e.frames.to_le_bytes());
        }
        out
    }

    /// The other side of `to_bytes`; anything malformed or over the limits is refused.
    pub fn from_bytes(bytes: &[u8]) -> Option<Self> {
        let rest = bytes.strip_prefix(MAGIC)?;
        let (count, rest) = rest.split_first_chunk::<4>()?;
        let count = u32::from_le_bytes(*count) as usize;
        if count == 0 || count > MAX_MOTIONS || rest.len() != count * PACKED {
            return None;
        }
        let u16_at = |c: &[u8], i: usize| u16::from_le_bytes([c[i], c[i + 1]]);
        let u32_at = |c: &[u8], i: usize| u32::from_le_bytes(c[i..i + 4].try_into().unwrap());
        let mut out = Vec::with_capacity(count);
        for c in rest.as_chunks::<PACKED>().0 {
            let e = MotionEntry {
                mode: u16_at(c, 0),
                posture: u16_at(c, 2),
                next: u32_at(c, 4) as i32,
                blend: u32_at(c, 8) as i32,
                start: f32::from_bits(u32_at(c, 12)),
                end: f32::from_bits(u32_at(c, 16)),
                step: f32::from_bits(u32_at(c, 20)),
                frames: u32_at(c, 24),
            };
            if !e.valid(count) {
                return None;
            }
            out.push(e);
        }
        Some(Self(out))
    }

    /// What players compare to agree on a table (`chao::file::crc32` of `to_bytes`).
    pub fn fingerprint(&self) -> u32 {
        crate::chao::file::crc32(&self.to_bytes())
    }
}

/// The `MotionTable` struct at `D+0xAC` (0x4C bytes) and its functions. Pointers (+0x2C,
/// +0x44, +0x48) are left out; the table is shared.
#[derive(Clone, Debug)]
pub struct MotionPlayer {
    table: Option<Arc<MotionTable>>,
    /// +0: `flag::*`.
    pub flags: u16,
    /// +2 (`D+0xAE`): the current motion's posture.
    pub posture: u16,
    /// +4 (`D+0xB0`) current id; +8 (`D+0xB4`) the blend target while blending, otherwise
    /// the motion queued after this one (-1 none).
    pub current: i32,
    pub next: i32,
    /// +0xC (`D+0xB8`): speed for the next advance; back to 1.0 after every advance.
    pub speed: f32,
    /// +0x10 blend step, +0x14 blend progress (0..1).
    pub blend_step: f32,
    pub progress: f32,
    /// +0x18 play mode, +0x1C (`D+0xC8`) frame, +0x20/+0x24 range, +0x28 step.
    pub mode: u16,
    pub frame: f32,
    pub start: f32,
    pub end: f32,
    pub step: f32,
    /// +0x30..+0x40: the same for the motion being blended in.
    pub next_mode: u16,
    pub next_frame: f32,
    pub next_start: f32,
    pub next_end: f32,
    pub next_step: f32,
}

impl MotionPlayer {
    /// `0x793880` (init with the table, `0x55C370`): everything zero, speed 0.0, `next` -1,
    /// then `set(0)`.
    pub fn new(table: Option<Arc<MotionTable>>) -> Self {
        let mut p = Self {
            table,
            flags: 0,
            posture: 0,
            current: 0,
            next: -1,
            speed: 0.0,
            blend_step: 0.0,
            progress: 0.0,
            mode: 0,
            frame: 0.0,
            start: 0.0,
            end: 0.0,
            step: 0.0,
            next_mode: 0,
            next_frame: 0.0,
            next_start: 0.0,
            next_end: 0.0,
            next_step: 0.0,
        };
        p.set(0);
        p
    }

    /// Swaps in a table (the current motion keeps playing as it is).
    pub fn set_table(&mut self, table: Option<Arc<MotionTable>>) {
        self.table = table;
    }

    pub fn table(&self) -> Option<&Arc<MotionTable>> {
        self.table.as_ref()
    }

    fn entry(&self, id: i32) -> MotionEntry {
        self.table.as_ref().map_or(FALLBACK, |t| t.get(id))
    }

    /// `0x793F70`: the end pulse (`D+0xAC & 1`).
    pub fn ended(&self) -> bool {
        self.flags & flag::END != 0
    }

    /// `0x793F80`: `D+0xAC & 8`, set only by play mode 3 while it holds its last frame.
    pub fn cycled(&self) -> bool {
        self.flags & flag::HOLD != 0
    }

    /// `D+0xAE`.
    pub fn posture(&self) -> u16 {
        self.posture
    }

    /// The motion on show or coming: `next` while blending, else `current` (what the
    /// visitor sync compares, re/chao-garden-sync.md).
    pub fn shown(&self) -> i32 {
        if self.flags & flag::BLEND != 0 {
            self.next
        } else {
            self.current
        }
    }

    /// `0x793E40` (MotionSet): straight to `id` at its start, no blend; pulses cleared.
    pub fn set(&mut self, id: i32) {
        let e = self.entry(id);
        self.current = id;
        self.next = e.next;
        self.posture = e.posture;
        self.blend_step = 0.0;
        self.progress = 0.0;
        self.mode = e.mode;
        self.frame = e.start;
        self.start = e.start;
        self.end = e.end;
        self.step = e.step;
        self.flags = 0;
    }

    /// `0x793EA0` (play mode 6): like `set`, but the frame keeps the overshoot past the old
    /// end (`frame + start − old end`).
    fn set_carry(&mut self, id: i32) {
        let e = self.entry(id);
        let frame = self.frame + e.start - self.end;
        self.set(id);
        self.frame = frame;
    }

    /// Loads `e` as the motion being blended in, at its start (`0x793C40` and kin).
    fn link(&mut self, id: i32, blend_step: f32) {
        let e = self.entry(id);
        self.next = id;
        self.blend_step = blend_step;
        self.progress = 0.0;
        self.next_mode = e.mode;
        self.next_frame = e.start;
        self.next_start = e.start;
        self.next_end = e.end;
        self.next_step = e.step;
        self.flags = flag::BLEND;
    }

    /// `0x793C40` (SetMotionLink, what behaviours call): blend into `id` over the entry's
    /// blend length, or 15 frames when it is negative (every entry but two in the game's
    /// table). The current motion freezes during the blend; posture changes when it ends.
    pub fn set_link(&mut self, id: i32) {
        let blend = self.entry(id).blend;
        let step = if blend < 0 {
            DEFAULT_BLEND_STEP
        } else {
            1.0 / blend as f32
        };
        self.link(id, step);
    }

    /// `0x793D30`: blend into `id` over `frames` frames (the pushed u16).
    pub fn set_blend(&mut self, id: i32, frames: u16) {
        self.link(id, 1.0 / f32::from(frames));
    }

    /// `0x793D90`: blend into `id` starting at the same phase of its range; when both
    /// motions are play mode 0, both advance during the blend (flag 4).
    pub fn set_keep_phase(&mut self, id: i32) {
        let ratio = (self.frame - self.start) / (self.end - self.start);
        self.set_link(id);
        self.next_frame = (self.next_end - self.next_start) * ratio + self.next_start;
        if self.mode == 0 && self.next_mode == 0 {
            self.flags = flag::BLEND | flag::BOTH;
        }
    }

    /// `0x793CB0` (play modes 4): blend into the queued `next`, over −blend of the current
    /// entry when that is negative, else over the next entry's blend.
    fn chain(&mut self) {
        let blend = self.entry(self.current).blend;
        let frames = if blend < 0 {
            -blend
        } else {
            self.entry(self.next).blend
        };
        self.link(self.next, 1.0 / frames as f32);
    }

    /// `0x793F10`: the blend is over; the blended-in motion becomes current where it is.
    fn finish_blend(&mut self) {
        let e = self.entry(self.next);
        self.current = self.next;
        self.next = e.next;
        self.posture = e.posture;
        self.blend_step = 0.0;
        self.progress = 0.0;
        self.mode = self.next_mode;
        self.frame = self.next_frame;
        self.start = self.next_start;
        self.end = self.next_end;
        self.step = self.next_step;
        self.flags = 0;
    }

    /// `0x7938D0`: one frame (MainSub calls it after the AI when `D+0xA0 & 4`). Clears
    /// pulses 1 and 8, then blends or plays, then speed = 1.0.
    pub fn advance(&mut self) {
        self.flags &= !(flag::END | flag::HOLD);
        if self.flags & flag::BLEND != 0 {
            self.advance_blend();
        } else {
            self.play();
        }
        self.speed = 1.0;
    }

    fn advance_blend(&mut self) {
        if self.blend_step < 0.0 {
            self.blend_step = -self.blend_step;
        }
        let p = self.blend_step * self.speed + self.progress;
        self.progress = p;
        if self.flags & flag::BOTH != 0 {
            // Both loop (`0x793920`): wrap each by its range, then switch once the blend
            // is full.
            self.frame = wrap(
                self.frame + self.step * self.speed,
                self.start,
                self.end,
                self.step,
            );
            self.next_frame = wrap(
                self.next_frame + self.speed * self.next_step,
                self.next_start,
                self.next_end,
                self.next_step,
            );
            if p >= 1.0 {
                self.progress = 1.0;
                self.finish_blend();
            }
            return;
        }
        if p < 1.0 || p.is_nan() {
            return;
        }
        if self.flags & flag::WRAP != 0 {
            // Play mode 2's wrap (`0x7939CD`): back to the start, carrying what's over 1.
            self.frame = if self.step > 0.0 {
                self.start + (p - 1.0)
            } else {
                self.start - (p - 1.0)
            };
            self.flags &= !(flag::BLEND | flag::WRAP);
            self.progress = 1.0;
        } else {
            self.progress = 1.0;
            self.set(self.next);
        }
    }

    /// Not blending (`0x793A36`): step the frame; past the end, the play mode decides.
    fn play(&mut self) {
        let f = self.step * self.speed + self.frame;
        self.frame = f;
        let reached = if self.step >= 0.0 {
            let end = if self.mode == 1 {
                self.end + 1.0
            } else {
                self.end
            };
            end <= f
        } else {
            let end = if self.mode == 1 {
                self.end - 1.0
            } else {
                self.end
            };
            end >= f
        };
        if !reached {
            return;
        }
        match self.mode {
            // Loop.
            0 => {
                self.frame = f - (self.end - self.start);
                self.flags |= flag::END;
            }
            // Loop through end+1 (the last frame blends back to the first).
            1 => {
                let len = if self.step >= 0.0 {
                    self.end + 1.0 - self.start
                } else {
                    self.end - 1.0 - self.start
                };
                self.frame = f - len;
                self.flags |= flag::END;
            }
            // Loop by a blend from where it is to the start, one step long; the frame stays
            // until it ends (`0x793AFE`).
            2 => {
                if self.step > 0.0 {
                    self.blend_step = self.step;
                    self.progress = f - self.end;
                } else {
                    self.blend_step = -self.step;
                    self.progress = -(f - self.end);
                }
                self.next_mode = self.mode;
                self.next_frame = self.start;
                self.next_start = self.start;
                self.next_end = self.end;
                self.next_step = self.step;
                self.flags = flag::END | flag::BLEND | flag::WRAP;
            }
            // Hold the last frame; pulses 1 and 8 every frame from now on.
            3 => {
                self.frame = self.end;
                self.flags |= flag::END | flag::HOLD;
            }
            // Hold, then blend into `next` (or just hold with pulse 1).
            4 => {
                self.frame = self.end;
                if self.next != -1 {
                    self.chain();
                }
                self.flags |= flag::END;
            }
            // Switch to `next` (none: the frame runs on, pulsing every frame).
            5 => {
                if self.next != -1 {
                    self.set(self.next);
                }
                self.flags |= flag::END;
            }
            // Switch to `next` keeping the overshoot.
            6 => {
                if self.next != -1 {
                    self.set_carry(self.next);
                }
                self.flags |= flag::END;
            }
            // Ping-pong: turn around at the end.
            7 => {
                self.frame = self.end;
                std::mem::swap(&mut self.start, &mut self.end);
                self.step = -self.step;
                self.flags |= flag::END;
            }
            _ => {}
        }
    }
}

/// Flag-4 blends' wrap (`0x793934`, `0x793960`): forward, at or past the end; backward, at
/// or before it; the frame goes back by the range.
fn wrap(f: f32, start: f32, end: f32, step: f32) -> f32 {
    let past = if step >= 0.0 { end <= f } else { end >= f };
    if past { f - (end - start) } else { f }
}

/// For when the table isn't in: the posture class of a motion (`al_motion_table` entry `+6`, which the game keeps at
/// `D+0xAE`): 0 standing, 1 sitting, 2 lying, 3 crawling, 4 other, 5 ... Read from the 622
/// entries of the Chao DLL's table. The server takes it from the motion last set (the game
/// switches it as the motion takes over; chains and blends aren't modelled).
pub fn fallback_posture(id: u16) -> u16 {
    match id {
        0xB..=0xC
        | 0xF..=0x11
        | 0x14..=0x17
        | 0x1E
        | 0x2A..=0x2F
        | 0x3E..=0x47
        | 0x50..=0x51
        | 0x5E..=0x63
        | 0xA4..=0xA9
        | 0xAB..=0xBA
        | 0x1A9..=0x1AF
        | 0x1CB..=0x1CC
        | 0x1D7..=0x1D8
        | 0x1ED..=0x1F0
        | 0x1F9..=0x1FB
        | 0x210..=0x215
        | 0x21A..=0x21F
        | 0x225..=0x229
        | 0x230..=0x234
        | 0x23D
        | 0x241..=0x24D
        | 0x262..=0x263
        | 0x268..=0x26A
        | 0x26D => 1,
        0x12..=0x13
        | 0x18
        | 0x1A..=0x1D
        | 0x1F..=0x25
        | 0x34
        | 0x36..=0x37
        | 0x39..=0x3D
        | 0x48..=0x4C
        | 0x55..=0x5D
        | 0x1DC
        | 0x238..=0x239
        | 0x24F..=0x258
        | 0x25B..=0x261
        | 0x265..=0x267
        | 0x26C => 2,
        0x19 | 0x35 | 0x38 | 0x4D..=0x4F | 0x6E..=0x74 | 0x9F => 3,
        0x82..=0x9E | 0xA0..=0xA2 | 0xCA..=0xE4 | 0xEE..=0xF9 | 0x10F..=0x113 => 4,
        0x24E | 0x259..=0x25A | 0x264 => 5,
        _ => 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(
        mode: u16,
        posture: u16,
        next: i32,
        blend: i32,
        range: (f32, f32),
        step: f32,
    ) -> MotionEntry {
        MotionEntry {
            mode,
            posture,
            next,
            blend,
            start: range.0,
            end: range.1,
            step,
            frames: range.0.max(range.1) as u32 + 1,
        }
    }

    /// Advances until the end pulse, returning how many advances it took (None after 1000).
    fn until_end(p: &mut MotionPlayer) -> Option<u32> {
        (1..=1000).find(|_| {
            p.advance();
            p.ended()
        })
    }

    fn player(entries: Vec<MotionEntry>) -> MotionPlayer {
        let mut p = MotionPlayer::new(Some(Arc::new(MotionTable(entries))));
        p.speed = 1.0;
        p
    }

    #[test]
    fn without_a_table_motions_end_after_60_frames_and_stay_ended() {
        let mut p = MotionPlayer::new(None);
        assert_eq!(p.speed, 0.0, "init leaves speed 0 for the first advance");
        p.advance();
        p.set_link(0x64);
        assert_eq!(p.posture(), 0);
        // The link frame finishes the zero-length blend; then 60 frames to the end.
        p.advance();
        assert_eq!((p.current, p.flags), (0x64, 0));
        assert_eq!(until_end(&mut p), Some(60));
        assert!(p.cycled());
        p.advance();
        assert!(p.ended() && p.cycled());
    }

    #[test]
    fn mode_0_loops_with_a_pulse_per_cycle() {
        let mut p = player(vec![entry(0, 0, -1, -1, (0.0, 4.0), 1.0)]);
        assert_eq!(until_end(&mut p), Some(4));
        assert_eq!(p.frame, 0.0);
        assert!(!p.cycled());
        p.advance();
        assert!(!p.ended(), "the pulse lasts one frame");
        assert_eq!(until_end(&mut p), Some(3));
    }

    #[test]
    fn mode_1_runs_through_end_plus_one() {
        let mut p = player(vec![entry(1, 0, -1, -1, (0.0, 4.0), 1.0)]);
        assert_eq!(until_end(&mut p), Some(5));
        assert_eq!(p.frame, 0.0);
        // Backwards: down to end − 1.
        let mut p = player(vec![entry(1, 0, -1, -1, (4.0, 0.0), -1.0)]);
        assert_eq!(until_end(&mut p), Some(5));
        assert_eq!(p.frame, 4.0);
    }

    #[test]
    fn mode_2_wraps_through_a_one_step_blend() {
        let mut p = player(vec![entry(2, 0, -1, -1, (0.0, 2.0), 0.5)]);
        assert_eq!(until_end(&mut p), Some(4));
        assert_eq!(p.flags, flag::END | flag::BLEND | flag::WRAP);
        assert_eq!((p.frame, p.progress), (2.0, 0.0));
        // Progress climbs by the step with the frame frozen, then the frame restarts.
        p.advance();
        assert_eq!((p.frame, p.flags), (2.0, flag::BLEND | flag::WRAP));
        p.advance();
        assert_eq!((p.frame, p.flags, p.progress), (0.0, 0, 1.0));
        assert_eq!(until_end(&mut p), Some(4));
    }

    #[test]
    fn mode_3_holds_and_pulses_1_and_8_every_frame() {
        let mut p = player(vec![
            entry(0, 0, -1, -1, (0.0, 9.0), 1.0),
            entry(3, 1, -1, -1, (3.0, 5.0), 0.5),
        ]);
        p.set(1);
        assert_eq!(p.posture(), 1);
        assert_eq!(until_end(&mut p), Some(4));
        assert!(p.cycled());
        for _ in 0..3 {
            p.advance();
            assert!(p.ended() && p.cycled());
            assert_eq!(p.frame, 5.0);
        }
    }

    #[test]
    fn mode_4_holds_then_chains_with_its_own_blend() {
        let mut p = player(vec![
            entry(0, 0, -1, -1, (0.0, 9.0), 1.0),
            entry(4, 1, 2, -4, (0.0, 2.0), 1.0),
            entry(0, 2, -1, -40, (0.0, 9.0), 1.0),
        ]);
        p.set(1);
        assert_eq!(until_end(&mut p), Some(2));
        assert_eq!(p.flags, flag::END | flag::BLEND);
        assert_eq!((p.next, p.blend_step), (2, 0.25));
        // Four blend frames (no pulses), then motion 2 starts with its posture.
        for _ in 0..3 {
            p.advance();
            assert_eq!((p.flags, p.current, p.posture()), (flag::BLEND, 1, 1));
        }
        p.advance();
        assert_eq!((p.flags, p.current, p.posture(), p.next), (0, 2, 2, -1));
    }

    #[test]
    fn modes_5_and_6_switch_to_next() {
        let table = vec![
            entry(5, 0, 1, -1, (0.0, 2.0), 0.75),
            entry(0, 3, -1, -1, (10.0, 20.0), 1.0),
            entry(6, 0, 1, -1, (0.0, 2.0), 0.75),
        ];
        let mut p = player(table);
        // Mode 5: at 2.25 → motion 1 at its start, pulse set after the switch.
        assert_eq!(until_end(&mut p), Some(3));
        assert_eq!(
            (p.current, p.frame, p.posture(), p.flags),
            (1, 10.0, 3, flag::END)
        );
        // Mode 6 keeps the 0.25 overshoot.
        p.set(2);
        assert_eq!(until_end(&mut p), Some(3));
        assert_eq!((p.current, p.frame), (1, 10.25));
    }

    #[test]
    fn mode_7_ping_pongs() {
        let mut p = player(vec![entry(7, 0, -1, -1, (0.0, 2.0), 1.0)]);
        assert_eq!(until_end(&mut p), Some(2));
        assert_eq!((p.start, p.end, p.step, p.frame), (2.0, 0.0, -1.0, 2.0));
        assert_eq!(until_end(&mut p), Some(2));
        assert_eq!((p.start, p.end, p.step, p.frame), (0.0, 2.0, 1.0, 0.0));
    }

    #[test]
    fn a_link_freezes_the_motion_then_starts_the_new_one() {
        let mut p = player(vec![
            entry(0, 0, -1, -40, (0.0, 100.0), 1.0),
            entry(3, 2, -1, -40, (0.0, 5.0), 1.0),
        ]);
        p.advance();
        p.advance();
        p.set_link(1);
        assert_eq!(p.shown(), 1);
        // Negative blend: 15 frames (1/15 per frame); the old frame stays put.
        for _ in 0..14 {
            p.advance();
            assert_eq!((p.current, p.frame, p.posture()), (0, 2.0, 0));
        }
        p.advance();
        assert_eq!((p.current, p.frame, p.posture(), p.flags), (1, 0.0, 2, 0));
        assert_eq!(until_end(&mut p), Some(5));
        // An explicit blend length (0x793D30), sped up by `speed` for one frame.
        p.set_blend(0, 10);
        p.speed = 5.0;
        p.advance();
        assert_eq!(p.speed, 1.0);
        assert!((p.progress - 0.5).abs() < 1e-6);
        for _ in 0..5 {
            p.advance();
        }
        assert_eq!(p.current, 0);
    }

    #[test]
    fn a_phase_blend_between_loops_advances_both() {
        let mut p = player(vec![
            entry(0, 0, -1, 4, (0.0, 10.0), 1.0),
            entry(0, 0, -1, 4, (0.0, 20.0), 1.0),
        ]);
        for _ in 0..5 {
            p.advance();
        }
        p.set_keep_phase(1);
        assert_eq!(
            (p.flags, p.next_frame, p.blend_step),
            (flag::BLEND | flag::BOTH, 10.0, 0.25)
        );
        for _ in 0..3 {
            p.advance();
        }
        assert_eq!((p.frame, p.next_frame, p.current), (8.0, 13.0, 0));
        p.advance();
        assert_eq!((p.current, p.frame, p.flags), (1, 14.0, 0));
    }

    #[test]
    fn a_table_survives_the_trip_and_bad_ones_dont() {
        let t = MotionTable(vec![
            entry(4, 1, 1, -40, (0.0, 13.0), 0.12),
            entry(7, 2, -1, 0, (3.0, 3.5), 0.012),
        ]);
        let bytes = t.to_bytes();
        assert_eq!(MotionTable::from_bytes(&bytes), Some(t.clone()));
        assert!(MotionTable::from_bytes(&bytes[..bytes.len() - 1]).is_none());
        let mut bad = bytes.clone();
        bad[12..14].copy_from_slice(&8u16.to_le_bytes());
        assert!(MotionTable::from_bytes(&bad).is_none(), "play mode 8");
        let mut bad = bytes.clone();
        bad[16..20].copy_from_slice(&2i32.to_le_bytes());
        assert!(MotionTable::from_bytes(&bad).is_none(), "next past the end");
        let mut bad = bytes;
        bad[24..28].copy_from_slice(&f32::NAN.to_le_bytes());
        assert!(MotionTable::from_bytes(&bad).is_none());
        assert_eq!(t.get(5), FALLBACK);
    }

    /// The real table, when the game is installed here (skipped otherwise).
    #[test]
    fn the_installed_table_reads_like_the_notes() {
        let Ok(data) = std::fs::read(
            "/mnt/SSD1/SteamLibrary/steamapps/common/Sonic Adventure 2/resource/gd_PC/DLL/Win32/Data_DLL_orig.dll",
        ) else {
            return;
        };
        let dll = super::super::land::PeFile::parse(data).unwrap();
        let t = MotionTable::read(&dll, dll.export(TABLE_EXPORT).unwrap()).unwrap();
        assert_eq!(t.0.len(), GAME_MOTIONS);
        // (id, mode, posture, next, start, end, nbFrame)
        for (id, mode, posture, next, start, end, frames) in [
            (0, 1, 0, -1, 0.0, 13.0, 14),
            (1, 1, 0, -1, 0.0, 4.0, 5),
            (3, 3, 0, -1, 3.0, 5.0, 6),
            (0x2F, 3, 1, -1, 0.0, 12.0, 13),
            (0x25, 3, 2, -1, 0.0, 13.0, 17),
            (0x64, 1, 0, -1, 0.0, 11.0, 12),
            (0x73, 6, 3, 0x74, 0.0, 15.0, 20),
            (0x9F, 3, 3, -1, 0.0, 10.0, 11),
            (0x101, 7, 0, -1, 0.0, 0.5, 39),
        ] {
            let e = t.get(id);
            assert_eq!(
                (e.mode, e.posture, e.next, e.start, e.end, e.frames),
                (mode, posture, next, start, end, frames),
                "{id:#x}"
            );
        }
        assert_eq!(MotionTable::from_bytes(&t.to_bytes()), Some(t.clone()));
        // Walk (loop through end+1): 12 frames at 0.23 a frame = 53 game frames a cycle.
        let mut p = MotionPlayer::new(Some(Arc::new(t)));
        p.set(0x64);
        p.speed = 1.0; // init leaves 0.0 for the first advance
        assert_eq!(until_end(&mut p), Some(53));
    }
}
