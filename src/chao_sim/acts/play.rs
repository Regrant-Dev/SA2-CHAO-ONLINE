//! Learned play (picker #14 `0x5A10B0`'s sub-choosers) and the Chao who watch or join it
//! (picker #7..#10): animal plays by absorbed animals (`rec+0x118`, table `0x11C4BE0`), the
//! three skill sets (A `rec+0x163` → `0x11C9BB4`, B `rec+0x161` → `0x11C3F44`, C `rec+0x162`
//! → `0x11C9BD4`), the animal-10 tricks and singing (re/chao-ai.md §1.5, §2A.5, §2A.6, §2B
//! "Animal play"). Decoded from the game's bytes.
//!
//! A performer gives off a signal (`0x55A920`, kinds 0xA5..0xA9) 8 units in front of it and
//! plays while it senses one of that kind, its own included; the others answer through
//! `Senses::signal`. Hand effects, the stadium model of skill A6 (`D+0x650`), collision tweaks
//! (`0x54A690`), sound volumes and motion speeds other than `D+0xB8` are the clients' business
//! and aren't modelled.

use super::super::brain::{Behaviour, Chao, Emitted, Next, Rng};
use super::super::land::{Garden, V3};
use super::super::picker::{Senses, Signal};
use super::super::record::off;

pub(super) fn run(
    chao: &mut Chao,
    address: u32,
    garden: &Garden,
    senses: &dyn Senses,
    rng: &mut Rng,
) -> Option<Next> {
    let _ = garden;
    Some(match address {
        // Animal plays.
        0x5A2BF0 => scoot(chao, rng),
        0x5A2B10 => sidle(chao, rng),
        0x5A2860 => pulses(chao, rng, 0x135, 4, 4),
        0x5A1F10 => pulses(chao, rng, 0x118, 2, 3),
        0x5A2310 => slide(chao, rng),
        0x5A2230 => creep(chao, rng),
        0x5A2E40 => to_tree(chao, senses, rng),
        0x5A2DA0 => timed(chao, rng, 0x120, 300, 201),
        0x5A2010 => preen(chao, rng),
        0x5A2460 => lounge(chao, rng),
        0x5A29C0 => sit_then_pulses(chao),
        0x5A2910 => pulses(chao, rng, 0x136, 4, 4),
        0x5A25D0 => dig(chao, rng),
        0x5A2A10 => hop_along(chao, rng),
        0x5A2CD0 => cheer(chao, rng),
        0x5A27B0 => until_held(chao, 0x11D),
        0x5A1FC0 => until_held(chao, 0x12F),
        0x5A2530 => timed(chao, rng, 0x187, 300, 201),
        // Skill A.
        0x59D8E0 => skill_a0(chao, senses, rng),
        0x59DE90 => skill_a1(chao, senses, rng),
        0x59E2D0 => skill_a2(chao, senses, rng),
        0x59DA40 => skill_a3(chao, senses, rng),
        0x59DD10 => skill_a4(chao, senses, rng),
        0x59D5A0 => skill_a5(chao, senses, rng),
        0x59E120 => skill_a6(chao, senses, rng),
        0x59E500 => skill_a7(chao, senses, rng),
        // Skill B.
        0x59C870 => skill_b_poses(chao, senses, rng, B0),
        0x59C980 => skill_b_poses(chao, senses, rng, B1),
        0x59CA90 => skill_b2(chao, senses),
        0x59CB20 => skill_b3(chao, senses, rng),
        0x59CBF0 => skill_b4(chao, senses, rng),
        // Skill C and the animal-10 tricks.
        0x59A740 => skill_c(chao, senses, rng, &C1),
        0x59A8A0 => skill_c(chao, senses, rng, &C2),
        0x59AA30 => skill_c(chao, senses, rng, &C3),
        0x59ABC0 => skill_c(chao, senses, rng, &C4),
        0x59AD50 => skill_c5(chao, senses, rng),
        0x59B0E0 => skill_c(chao, senses, rng, &TRICK),
        0x59B270 => skill_c(chao, senses, rng, &DARK_TRICK),
        // Watchers.
        0x59E710 => watch(chao, senses, rng, 0xA5),
        0x59CDA0 => watch(chao, senses, rng, 0xA6),
        0x59B3D0 => watch(chao, senses, rng, 0xA7),
        0x59CF50 => watch_b(chao, senses, rng),
        0x59B580 => scorn(chao, rng),
        0x5AB2C0 => sing(chao, rng),
        _ => return None,
    })
}

/// Animal plays by animal (`0x11C4BE0`; 0 where there's none).
const ANIMAL_PLAYS: [u32; 21] = [
    0x5A2BF0, 0x5A2B10, 0, 0x5A2860, 0x5A1F10, 0x5A2310, 0x5A2230, 0x5A2E40, 0x5A2010, 0x5A2460, 0,
    0x5A29C0, 0x5A25D0, 0x5A2A10, 0x5A2CD0, 0, 0, 0, 0x5A27B0, 0x5A1FC0, 0x5A2530,
];
/// Skill A performances by bit of `rec+0x163` (`0x11C9BB4`).
const SKILL_A: [u32; 8] = [
    0x59D8E0, 0x59DE90, 0x59E2D0, 0x59DA40, 0x59DD10, 0x59D5A0, 0x59E120, 0x59E500,
];
/// Skill B performances by bit of `rec+0x161` (`0x11C3F44`).
const SKILL_B: [u32; 5] = [0x59C870, 0x59C980, 0x59CA90, 0x59CB20, 0x59CBF0];
/// Skill C performances by level (`0x11C9BD4`; levels 5..8 are all `0x59AD50`, the chooser
/// reaches 4).
const SKILL_C: [u32; 5] = [0x59A740, 0x59A8A0, 0x59AA30, 0x59ABC0, 0x59AD50];
/// `rec+0x162`, `rec+0x161`, `rec+0x163`, `rec+0x160`, `rec+0x164`, `rec+0x118`.
const SKILLS_C: usize = 0x162;
const SKILLS_B: usize = 0x161;
const SKILLS_A: usize = 0x163;
const SONGS: usize = 0x160;
const TOY_SKILLS: usize = 0x164;
const ANIMALS: usize = 0x118;

/// Models the performances hold (`0x5669B0`; for a parameter block `0x566A20`, its model),
/// as `Chao::props` keeps them.
mod prop {
    /// Skill A5: nodes 4, 11.
    pub const A5: [u32; 2] = [0x11C_505C, 0x11C_54F4];
    /// Skill A3: nodes 4, 11, 35.
    pub const A3: [u32; 3] = [0x11C_6674, 0x11C_6A14, 0x11C_62C4];
    /// Skill A2: nodes 4, 11.
    pub const A2: [u32; 2] = [0x11C_8964, 0x11C_8FFC];
    /// Skill A7: node 4.
    pub const A7: u32 = 0x11C_9AE4;
    /// Blocks: A0 `0x11C9B54` (node 11), A4 `0x11C9B74` (node 11), A1 `0x11C9B94` (node 4).
    pub const A0: u32 = 0x11C_5B3C;
    pub const A4: u32 = 0x11C_73A4;
    pub const A1: u32 = 0x11C_7A9C;
    /// Singing: blocks `0x11BB93C` (node 4), `0x11BB95C` (node 11).
    pub const SING: [u32; 2] = [0x12F_CD74, 0x12F_D044];
}

/// `Chao::props` slots: nodes 4, 11, 35.
const NODE_4: usize = 0;
const NODE_11: usize = 1;
const NODE_35: usize = 2;

/// Voices of skill C (`0x11C414C`), its level-5 special (`0x11C415C`) and the trick's
/// (`0x11C40A4`).
const C_VOICES: [u16; 4] = [0x607E, 0x6079, 0x607A, 0x607B];
const C_SPECIAL: u16 = 0x6080;
const TRICK_VOICES: [u16; 3] = [0x6076, 0x6077, 0x6078];

/// `0x486880(obj, 1, kind)` with `hit+4` set: a signal of `kind` it touches. Its own count
/// when its sphere (1.5 around 2 up) reaches them (`0x483170`: `|cA − cB| < rA + rB`; a
/// performer stands 8 units from the middle of its 20); the rest come from `senses`.
fn heard(chao: &Chao, senses: &dyn Senses, kind: u8) -> Option<Signal> {
    let p = chao.body.pos;
    let me = [p[0], p[1] + 2.0, p[2]];
    let touches = |s: &Emitted| {
        let d2: f32 = s.pos.iter().zip(me).map(|(a, b)| (a - b) * (a - b)).sum();
        d2 < (s.radius + 1.5) * (s.radius + 1.5)
    };
    if let Some(s) = chao.signals.iter().find(|s| s.kind == kind && touches(s)) {
        let doing = chao.current().map_or(0, Behaviour::address);
        return Some(Signal {
            pos: s.pos,
            parent: Some((chao.body.pos, doing)),
        });
    }
    senses.signal(kind)
}

/// The performances' last step: on while a signal of `kind` is there, else done.
fn while_heard(chao: &Chao, senses: &dyn Senses, kind: u8) -> Next {
    if heard(chao, senses, kind).is_some() {
        Next::Keep
    } else {
        Next::Pop
    }
}

/// `pos + facing·d`, same height (where performers put their signal).
fn ahead(chao: &Chao, d: f32) -> V3 {
    let (s, c) = chao.body.facing();
    let p = chao.body.pos;
    [p[0] + s * d, p[1], p[2] + c * d]
}

/// Moves the position itself along `rot_y` (no physics), as the animal plays do.
fn shift(chao: &mut Chao, rot_y: u16, d: f32) {
    let a = f32::from(rot_y) * std::f32::consts::TAU / 65536.0;
    chao.body.pos[0] += a.sin() * d;
    chao.body.pos[2] += a.cos() * d;
}

/// A random entry of `n`: `trunc(r·(n − eps))` (`0x8287C0` / FPU truncate).
fn pick(rng: &mut Rng, n: usize, eps: f64) -> usize {
    ((f64::from(rng.r()) * (n as f64 - eps)) as usize).min(n - 1)
}

/// `trunc(r·m)` with the product stored as a float first (the voice picks).
fn pick_scaled(rng: &mut Rng, m: f64) -> usize {
    (f64::from(rng.r()) * m) as f32 as usize
}

/// The bit numbers set in the low `n` bits of `bits`, in order.
fn bits_set(bits: u32, n: u32) -> Vec<usize> {
    (0..n)
        .filter(|i| bits & (1 << i) != 0)
        .map(|i| i as usize)
        .collect()
}

/// The current motion id (`D+0xB0`; the old one while a blend runs).
fn motion_id(chao: &Chao) -> i32 {
    chao.player.current
}

// --- Choosers ---------------------------------------------------------------------------

/// `(v − t)/(10000 − t)` above `t`, else 0 (`0x562440`; as picker.rs).
fn f16(v: u16, t: i32) -> f32 {
    let v = i32::from(v as i16);
    if v > t {
        (v - t) as f32 / (10_000 - t) as f32
    } else {
        0.0
    }
}

/// `s·(0.4 + 0.6r)` (`0x5624D0`; as picker.rs).
fn jitter(s: f32, rng: &mut Rng) -> f32 {
    s * 0.4 + rng.r() * (1.0 - 0.4) * s
}

/// The joiners' score: TEDIOUS (+3000 under 7000) over `t`, jittered; 0 at or under `t`.
fn join_score(chao: &Chao, t: u16, rng: &mut Rng) -> f32 {
    let mut v = chao.rec.u16(off::TEDIOUS);
    if v <= t {
        return 0.0;
    }
    if v < 7000 {
        v += 3000;
    }
    jitter(0.99 * f16(v, i32::from(t)), rng)
}

impl Chao {
    /// `0x5A2F10` (picker #14, animal play): a random absorbed animal (`rec+0x118` bits
    /// 0..20) that has a play installs it. Whether it did.
    pub(in super::super) fn animal_play(&mut self, _senses: &dyn Senses, rng: &mut Rng) -> bool {
        let bits = self.rec.u32(ANIMALS);
        let known: Vec<usize> = bits_set(bits, 21)
            .into_iter()
            .filter(|&i| ANIMAL_PLAYS[i] != 0)
            .collect();
        if known.is_empty() {
            return false;
        }
        let i = known[pick(rng, known.len(), 1e-4)];
        self.set(Behaviour::Game(ANIMAL_PLAYS[i]), -1);
        true
    }

    /// `0x59D410` (picker #14, skill A): unless a 0xA5 performance is near, a random known
    /// skill A (`rec+0x163` bits 0..7), with signal 0xA5 (radius 20, 1800..4200 frames)
    /// 8 units ahead. Whether it did.
    pub(in super::super) fn skill_a(&mut self, senses: &dyn Senses, rng: &mut Rng) -> bool {
        self.perform(senses, rng, 0xA5, SKILLS_A, 8, &SKILL_A)
    }

    /// `0x59C6D0` (picker #14, skill B): as skill A with `rec+0x161` bits 0..4 and signal
    /// 0xA6.
    pub(in super::super) fn skill_b(&mut self, senses: &dyn Senses, rng: &mut Rng) -> bool {
        self.perform(senses, rng, 0xA6, SKILLS_B, 5, &SKILL_B)
    }

    /// `0x59D410` / `0x59C6D0`.
    fn perform(
        &mut self,
        senses: &dyn Senses,
        rng: &mut Rng,
        kind: u8,
        at: usize,
        n: u32,
        table: &[u32],
    ) -> bool {
        if heard(self, senses, kind).is_some() {
            return false;
        }
        let known = bits_set(u32::from(self.rec.u8(at)), n);
        if known.is_empty() {
            return false;
        }
        let i = known[pick(rng, known.len(), 1e-5)];
        let pos = ahead(self, 8.0);
        self.set(Behaviour::Game(table[i]), -1);
        let frames = 1800 + rng.below(2401);
        self.emit(kind, pos, 20.0, frames);
        true
    }

    /// `0x59B900` (picker #14, skill C): unless a 0xA7 performance is near, the level from
    /// `rec+0x162` bits 0..4 (bit 0 counts 0, each other bit 1, so without bit 0 the count
    /// starts at -1), signal 0xA7 for 1500..3300 frames. With no level, an animal-10 Chao
    /// (`rec+0x118 & 0x400`) does its trick: dark (alignment `rec+0xB0` < -0.5) `0x59B270`
    /// with 0xA8 for 1800..3600, else `0x59B0E0` with 0xA7 for 1200..2400. Whether it did.
    pub(in super::super) fn skill_c(&mut self, senses: &dyn Senses, rng: &mut Rng) -> bool {
        if heard(self, senses, 0xA7).is_some() {
            return false;
        }
        let bits = self.rec.u8(SKILLS_C);
        let mut level: i32 = if bits & 1 != 0 { 0 } else { -1 };
        level += (bits >> 1 & 0xF).count_ones() as i32;
        let (behaviour, kind, base, spread) = if level >= 0 {
            (SKILL_C[level as usize], 0xA7, 1500, 1801)
        } else if self.rec.u32(ANIMALS) & 0x400 == 0 {
            return false;
        } else if self.rec.f32(off::ALIGNMENT) < -0.5 {
            (0x59B270, 0xA8, 1800, 1801)
        } else {
            (0x59B0E0, 0xA7, 1200, 1201)
        };
        let pos = ahead(self, 8.0);
        self.set(Behaviour::Game(behaviour), -1);
        let frames = base + rng.below(spread);
        self.emit(kind, pos, 20.0, frames);
        true
    }

    /// `0x5AB5D0` (picker #14, song): under 100 song objects about. Half the time a known
    /// song (`rec+0x160` bits 0..4; bit 1's song goes by type); else, with `rec+0x164 &
    /// 0x100`, a song for a liked character (`0x5AB4B0`). Installs `0x5AB2C0`. Whether it
    /// did.
    pub(in super::super) fn sing(&mut self, senses: &dyn Senses, rng: &mut Rng) -> bool {
        if senses.songs() >= 100 {
            return false;
        }
        let known = bits_set(u32::from(self.rec.u8(SONGS)), 5);
        if !known.is_empty() && rng.r() < 0.5 {
            let i = known[pick(rng, known.len(), 1e-5)];
            self.set(Behaviour::Game(0x5AB2C0), -1);
            self.song = Some(match i {
                0 => 9,
                1 => by_type(self.rec.u8(off::TYPE), [0xB, 0xC, 0xA]),
                2 => 0xD,
                3 => 0xF,
                _ => 0xE,
            });
            return true;
        }
        if self.rec.u16(TOY_SKILLS) & 0x100 == 0 {
            return false;
        }
        self.set(Behaviour::Game(0x5AB2C0), -1);
        self.song = Some(self.liked_song(rng));
        true
    }

    /// `0x5AB4B0`: the characters liked at 50 or more (bond `+0` like; the game writes 50
    /// back into each of those likes, which this keeps); 80% one of theirs (the bond index
    /// is the song), else the type's own (7 / 8 / 6).
    fn liked_song(&mut self, rng: &mut Rng) -> u8 {
        let mut liked = Vec::new();
        for c in 0..6 {
            let at = off::BONDS + c * 6;
            if self.rec.i8(at) >= 50 {
                self.rec.set_u8(at, 50);
                liked.push(c as u8);
            }
        }
        if !liked.is_empty() && rng.r() < 0.8 {
            let n = liked.len() as f64;
            let i = ((f64::from(rng.r()) * (n - 1e-4)) as usize).min(liked.len() - 1);
            return liked[i];
        }
        by_type(self.rec.u8(off::TYPE), [7, 8, 6])
    }

    /// Picker #7 `0x59D0D0` (signal 0xA5): best < 1, TEDIOUS over 1000. Knowing skill A, walk
    /// to the signal (`0x56B500`) and join with a random known one; TEDIOUS and LONELY −4000,
    /// the performer's LONELY −4000. Else walk to the performer, sit and watch (`0x59E710`).
    pub(in super::super) fn join_skill_a(
        &mut self,
        best: &mut f32,
        senses: &dyn Senses,
        rng: &mut Rng,
    ) {
        let signal = heard(self, senses, 0xA5);
        let Some(signal) = signal.filter(|_| *best < 1.0) else {
            return;
        };
        let known = bits_set(u32::from(self.rec.u8(SKILLS_A)), 8);
        let s = join_score(self, 2000 >> 1, rng);
        if s <= *best {
            return;
        }
        if known.is_empty() {
            if let Some((p, _)) = signal.parent {
                self.body.target = p;
            }
            self.set(Behaviour::Game(0x56B500), -1);
            self.push(Behaviour::StandUpSlow);
            self.push(Behaviour::Game(0x59E710));
        } else {
            let i = known[pick(rng, known.len(), 1e-5)];
            self.body.target = signal.pos;
            self.set(Behaviour::Game(0x56B500), -1);
            self.push(Behaviour::Game(SKILL_A[i]));
            self.rec.add_need(off::TEDIOUS, -4000);
            self.rec.add_need(off::LONELY, -4000);
            if signal.parent.is_some() {
                self.performer_lonely = Some((0xA5, -4000));
            }
        }
        *best = 0.99;
    }

    /// Picker #8 `0x59C3D0` (signal 0xA6): best < 1, TEDIOUS over 2000. When the performer
    /// does B1..B4 and this Chao knows it (`rec+0x161` bit 1..4), walk to the signal and join
    /// in; TEDIOUS and LONELY −4000, the performer's LONELY −3000. Else walk to the performer,
    /// sit and watch (`0x59CDA0` or `0x59CF50`).
    pub(in super::super) fn join_skill_b(
        &mut self,
        best: &mut f32,
        senses: &dyn Senses,
        rng: &mut Rng,
    ) {
        let signal = heard(self, senses, 0xA6);
        let Some(signal) = signal.filter(|_| *best < 1.0) else {
            return;
        };
        let s = join_score(self, 2000, rng);
        if s <= *best {
            return;
        }
        let known = self.rec.u8(SKILLS_B);
        let join = signal
            .parent
            .and_then(|(_, doing)| (1..5).find(|&i| SKILL_B[i] == doing && known & (1 << i) != 0));
        if let Some(i) = join {
            self.body.target = signal.pos;
            self.set(Behaviour::Game(0x56B500), -1);
            self.push(Behaviour::Game(SKILL_B[i]));
            self.rec.add_need(off::TEDIOUS, -4000);
            self.rec.add_need(off::LONELY, -4000);
            self.performer_lonely = Some((0xA6, -3000));
        } else {
            if let Some((p, _)) = signal.parent {
                self.body.target = p;
            }
            self.set(Behaviour::Game(0x56B500), -1);
            self.push(Behaviour::StandUpSlow);
            let watch = if rng.r() < 0.5 { 0x59CDA0 } else { 0x59CF50 };
            self.push(Behaviour::Game(watch));
        }
        *best = 0.99;
    }

    /// Picker #9 `0x59B710` (signal 0xA7): best < 1, TEDIOUS over 2000: walk to the
    /// performer, sit and watch (`0x59B3D0`).
    pub(in super::super) fn watch_skill_c(
        &mut self,
        best: &mut f32,
        senses: &dyn Senses,
        rng: &mut Rng,
    ) {
        let signal = heard(self, senses, 0xA7);
        let Some(signal) = signal.filter(|_| *best < 1.0) else {
            return;
        };
        if join_score(self, 2000, rng) <= *best {
            return;
        }
        if let Some((p, _)) = signal.parent {
            self.body.target = p;
        }
        self.set(Behaviour::Game(0x56B500), -1);
        self.push(Behaviour::StandUpSlow);
        self.push(Behaviour::Game(0x59B3D0));
        *best = 0.99;
    }

    /// Picker #10 `0x59B820` (signal 0xA8, the dark trick): best < 1; TEDIOUS over 2000 and
    /// r < 0.7 score 0.99 (no jitter), else 0 → `0x59B580`.
    pub(in super::super) fn scorn_dark_trick(
        &mut self,
        best: &mut f32,
        senses: &dyn Senses,
        rng: &mut Rng,
    ) {
        if *best >= 1.0 || heard(self, senses, 0xA8).is_none() {
            return;
        }
        let mut s = 0.0;
        if self.rec.u16(off::TEDIOUS) > 2000 && rng.r() < 0.7 {
            s = 0.99;
        }
        if s > *best {
            self.set(Behaviour::Game(0x59B580), -1);
            *best = s;
        }
    }
}

/// The type tables of `0x5AB5D0` / `0x5AB4B0`: by `(type − 6) % 3` for types 6..22 (0, 1,
/// 2), else the last.
fn by_type(kind: u8, songs: [u8; 3]) -> u8 {
    match kind.wrapping_sub(6) {
        k @ 0..=16 => songs[usize::from(k % 3)],
        _ => songs[2],
    }
}

// --- Animal plays -------------------------------------------------------------------------

/// Mode 0: `motion`, `base..base+spread−1` frames (the game's `base − trunc(r·−spread)`);
/// then counted down, done after it reaches 0 (`0x5A2DA0`, `0x5A2530`).
fn timed(chao: &mut Chao, rng: &mut Rng, motion: u16, base: i32, spread: u32) -> Next {
    match chao.mode {
        0 => {
            chao.play(motion);
            chao.count_a = base + rng.below(spread);
            chao.mode = 1;
        }
        1 => {}
        _ => return Next::Keep,
    }
    count_down(chao)
}

/// `t = count--; t ≤ 0` → done.
fn count_down(chao: &mut Chao) -> Next {
    let t = chao.count_a;
    chao.count_a -= 1;
    if t > 0 { Next::Keep } else { Next::Pop }
}

/// `motion` for `base..base+spread−1` end pulses (`0x5A2860`, `0x5A1F10`, `0x5A2910`).
fn pulses(chao: &mut Chao, rng: &mut Rng, motion: u16, base: i32, spread: u32) -> Next {
    match chao.mode {
        0 => {
            chao.play(motion);
            chao.mode = 1;
            chao.count_a = base + rng.below(spread);
        }
        1 => {}
        _ => return Next::Keep,
    }
    if chao.motion_done() {
        chao.count_a -= 1;
        if chao.count_a <= 0 {
            return Next::Pop;
        }
    }
    Next::Keep
}

/// `motion` until the hold pulse (`0x5A27B0` animal 18, whose hand effect after 20 frames is
/// the client's; `0x5A1FC0` animal 19).
fn until_held(chao: &mut Chao, motion: u16) -> Next {
    match chao.mode {
        0 => {
            chao.play(motion);
            chao.mode = 1;
        }
        1 => {}
        _ => return Next::Keep,
    }
    chao.count_a += 1;
    if chao.motion_held() {
        Next::Pop
    } else {
        Next::Keep
    }
}

/// `0x5A2BF0` (animal 0): motion 0x71 for 120..180 end pulses, 0.15 forward a frame.
fn scoot(chao: &mut Chao, rng: &mut Rng) -> Next {
    match chao.mode {
        0 => {
            chao.play(0x71);
            chao.mode = 1;
            chao.count_a = 120 + rng.below(61);
        }
        1 => {}
        _ => return Next::Keep,
    }
    if chao.motion_done() {
        chao.count_a -= 1;
        if chao.count_a <= 0 {
            return Next::Pop;
        }
    }
    shift(chao, chao.body.rot_y, 0.15);
    Next::Keep
}

/// `0x5A2B10` (animal 1): motion 0x141, 500..800 frames, 0.05 a frame to its right.
fn sidle(chao: &mut Chao, rng: &mut Rng) -> Next {
    match chao.mode {
        0 => {
            chao.play(0x141);
            chao.count_a = 500 + rng.below(301);
            chao.mode = 1;
        }
        1 => {}
        _ => return Next::Keep,
    }
    if count_down(chao) == Next::Pop {
        return Next::Pop;
    }
    shift(chao, chao.body.rot_y.wrapping_sub(0x4000), 0.05);
    Next::Keep
}

/// `0x5A2310` (animal 5): 0x13E → 0x13F → 0x140 on end pulses, then 0x81 for 120..180
/// frames, sliding 0.3 a frame from 0x140 on. Motion speed 1.5 (`D+0xB8`) but on the last
/// frame.
fn slide(chao: &mut Chao, rng: &mut Rng) -> Next {
    match chao.mode {
        0 | 1 => {
            if chao.mode == 0 {
                chao.play(0x13E);
                chao.mode = 1;
            }
            if chao.motion_done() {
                chao.play(0x13F);
                chao.mode = 2;
            }
        }
        2 => {
            if chao.motion_done() {
                chao.play(0x140);
                chao.mode = 3;
            }
        }
        3 => {
            if chao.motion_done() {
                chao.play(0x81);
                chao.count_a = 120 + rng.below(61);
                chao.mode = 4;
            }
            shift(chao, chao.body.rot_y, 0.3);
        }
        4 => {
            shift(chao, chao.body.rot_y, 0.3);
            if count_down(chao) == Next::Pop {
                return Next::Pop;
            }
        }
        _ => {}
    }
    chao.player.speed = 1.5;
    Next::Keep
}

/// `0x5A2230` (animal 6): motion 0x12D, 120..240 frames, 0.05 forward a frame.
fn creep(chao: &mut Chao, rng: &mut Rng) -> Next {
    match chao.mode {
        0 => {
            chao.play(0x12D);
            chao.count_a = 120 + rng.below(121);
            chao.mode = 1;
        }
        1 => {}
        _ => return Next::Keep,
    }
    if count_down(chao) == Next::Pop {
        return Next::Pop;
    }
    shift(chao, chao.body.rot_y, 0.05);
    Next::Keep
}

/// `0x5A2E40` (animal 7): with a tree perceived, r > 0.3, nearer than 40 and in state 3 or
/// 5: walk to it (`0x56BA80`), face it (`0x56B6C0`) and play there (`0x5A2DA0`). Else done.
fn to_tree(chao: &mut Chao, senses: &dyn Senses, rng: &mut Rng) -> Next {
    let Some(tree) = senses.nearest_tree() else {
        return Next::Pop;
    };
    if rng.r() <= 0.3 || tree.dist >= 40.0 || !matches!(tree.state, 3 | 5) {
        return Next::Pop;
    }
    // `0x5304F0` makes the tree the ALW target the walk heads for; the server keeps its
    // position as the walk target.
    chao.body.target = tree.pos;
    chao.set(Behaviour::Game(0x56BA80), -1);
    chao.push(Behaviour::Game(0x56B6C0));
    chao.push(Behaviour::Game(0x5A2DA0));
    Next::Keep
}

/// `0x5A2010` (animal 8): 0x11A for 120..220 frames; then 40% done, else 40% 0x11C with
/// sound 0x6087 until the hold pulse, else 0x11A again, both then 160..280 frames more.
fn preen(chao: &mut Chao, rng: &mut Rng) -> Next {
    match chao.mode {
        0 | 1 => {
            if chao.mode == 0 {
                chao.play(0x11A);
                chao.count_a = 120 + rng.below(101);
                chao.mode = 1;
            }
            if count_down(chao) == Next::Keep {
                return Next::Keep;
            }
            if rng.r() < 0.4 {
                return Next::Pop;
            }
            if rng.r() < 0.4 {
                chao.play(0x11C);
                chao.sound(0x6087);
                chao.mode = 2;
            } else {
                chao.play(0x11A);
                chao.count_a = 160 + rng.below(121);
            }
        }
        2 if chao.motion_held() => {
            chao.play(0x11A);
            chao.count_a = 160 + rng.below(121);
            chao.mode = 1;
        }
        _ => {}
    }
    Next::Keep
}

/// `0x5A2460` (animal 9): 0x3A or 0x3C (50%) for 600..900 frames.
fn lounge(chao: &mut Chao, rng: &mut Rng) -> Next {
    match chao.mode {
        0 => {
            chao.play(if rng.r() < 0.5 { 0x3A } else { 0x3C });
            chao.count_a = 600 + rng.below(301);
            chao.mode = 1;
        }
        1 => {}
        _ => return Next::Keep,
    }
    count_down(chao)
}

/// `0x5A29C0` (animal 11): stand up slowly, then `0x5A2910` (0x136 for 4..7 end pulses).
fn sit_then_pulses(chao: &mut Chao) -> Next {
    chao.set(Behaviour::StandUpSlow, -1);
    chao.push(Behaviour::Game(0x5A2910));
    Next::Keep
}

/// `0x5A25D0` (animal 12): motion 1 for 120..240 frames, then sound 0x1013 and signal 0x97
/// (radius 5, 60 frames) 3 units behind, 0.2 up, where the client puts its effect
/// (`0x6CC4E0`); 120..240 frames more.
fn dig(chao: &mut Chao, rng: &mut Rng) -> Next {
    match chao.mode {
        0 | 1 => {
            if chao.mode == 0 {
                chao.play(1);
                chao.count_a = 120 + rng.below(121);
                chao.mode = 1;
            }
            if count_down(chao) == Next::Pop {
                chao.sound(0x1013);
                chao.count_a = 120 + rng.below(121);
                let (s, c) = chao.body.facing();
                let p = chao.body.pos;
                let at = [p[0] - s * 3.0, p[1] + 0.2, p[2] - c * 3.0];
                chao.emit(0x97, at, 5.0, 60);
                chao.mode = 2;
            }
            Next::Keep
        }
        2 => count_down(chao),
        _ => Next::Keep,
    }
}

/// `0x5A2A10` (animal 13): 0x132 to its end pulse, then 4..7 more end pulses moving 0.1
/// forward a frame.
fn hop_along(chao: &mut Chao, rng: &mut Rng) -> Next {
    match chao.mode {
        0 | 1 => {
            if chao.mode == 0 {
                chao.play(0x132);
                chao.mode = 1;
                chao.count_a = 4 + rng.below(4);
            }
            if chao.motion_done() {
                chao.mode = 2;
            }
        }
        2 => {
            if chao.motion_done() {
                chao.count_a -= 1;
                if chao.count_a <= 0 {
                    return Next::Pop;
                }
            }
            shift(chao, chao.body.rot_y, 0.1);
        }
        _ => {}
    }
    Next::Keep
}

/// `0x5A2CD0` (animal 14): motion 0x23F and sound 0x6084, 400..600 frames.
fn cheer(chao: &mut Chao, rng: &mut Rng) -> Next {
    match chao.mode {
        0 => {
            chao.play(0x23F);
            chao.mode = 1;
            chao.count_a = 400 + rng.below(201);
            chao.sound(0x6084);
        }
        1 => {}
        _ => return Next::Keep,
    }
    chao.count_a -= 1;
    if chao.count_a > 0 {
        Next::Keep
    } else {
        Next::Pop
    }
}

// --- Skill A ------------------------------------------------------------------------------

/// The skill A start: motion, eyes 4, mouth 3.
fn a_start(chao: &mut Chao, motion: u16) {
    chao.play(motion);
    chao.set_eyes(4, -1);
    chao.set_mouth(3, -1);
}

/// `60..60+spread−1` frames before the next motion change.
fn a_next(chao: &mut Chao, rng: &mut Rng, spread: u32) {
    chao.count_a = 60 + rng.below(spread);
}

/// `0x59D8E0` A0: 0x15F or 0x160, block A0 in hand 11; sound 0x4009 every 30 frames.
fn skill_a0(chao: &mut Chao, senses: &dyn Senses, rng: &mut Rng) -> Next {
    if chao.mode == 0 {
        a_start(chao, if rng.r() < 0.5 { 0x15F } else { 0x160 });
        chao.props[NODE_11] = prop::A0;
        // Set and never read.
        chao.count_a = 300 + rng.below(301);
        chao.mode = 1;
    }
    if chao.mode == 1 {
        let old = chao.count_b;
        chao.count_b += 1;
        if old % 30 == 0 {
            chao.sound(0x4009);
        }
    }
    while_heard(chao, senses, 0xA5)
}

/// `0x59DE90` A1: 0x165, then every 60..240 frames 0x166 (60%) or 0x167 from 0x165, else
/// back to 0x165; sound 0x400D at frame 30 of 0x166, at 30 and 60 of 0x167.
fn skill_a1(chao: &mut Chao, senses: &dyn Senses, rng: &mut Rng) -> Next {
    if chao.mode == 0 {
        a_start(chao, 0x165);
        chao.props[NODE_4] = prop::A1;
        chao.count_a = 300 + rng.below(301);
        chao.mode = 1;
    }
    if chao.mode == 1 {
        let t = chao.count_a;
        chao.count_a -= 1;
        if t <= 0 {
            if motion_id(chao) == 0x165 {
                let next = if rng.r() < 0.6 { 0x166 } else { 0x167 };
                chao.play_blend(next, 10);
            } else {
                chao.play_blend(0x165, 10);
            }
            a_next(chao, rng, 181);
            chao.count_b = 0;
        }
        chao.count_b += 1;
        let n = chao.count_b;
        match motion_id(chao) {
            0x165 => {}
            0x166 => {
                if n == 30 {
                    chao.sound(0x400D);
                }
            }
            _ => {
                if n == 30 || n == 60 {
                    chao.sound(0x400D);
                }
            }
        }
    }
    while_heard(chao, senses, 0xA5)
}

/// `0x59E2D0` A2: 0x16B ↔ 0x16C every 60..240 frames; sound 0x4007 at frame 30 of 0x16C.
fn skill_a2(chao: &mut Chao, senses: &dyn Senses, rng: &mut Rng) -> Next {
    if chao.mode == 0 {
        a_start(chao, 0x16B);
        chao.props[NODE_4] = prop::A2[0];
        chao.props[NODE_11] = prop::A2[1];
        chao.count_a = 300 + rng.below(301);
        chao.mode = 1;
    }
    if chao.mode == 1 {
        let t = chao.count_a;
        chao.count_a -= 1;
        if t <= 0 {
            if motion_id(chao) == 0x16B {
                chao.play_blend(0x16C, 10);
                chao.count_b = 0;
            } else {
                chao.play_blend(0x16B, 10);
            }
            a_next(chao, rng, 181);
        }
        chao.count_b += 1;
        if motion_id(chao) != 0x16B && chao.count_b == 30 {
            chao.sound(0x4007);
        }
    }
    while_heard(chao, senses, 0xA5)
}

/// `0x59DA40` A3: 0x161, then every 60..240 frames 0x161 again in phase (60%) or 0x162,
/// and from 0x162 back to 0x161; sound 0x4008 every 16 frames of 0x161, 40% every 12 of
/// 0x162.
fn skill_a3(chao: &mut Chao, senses: &dyn Senses, rng: &mut Rng) -> Next {
    if chao.mode == 0 {
        a_start(chao, 0x161);
        chao.props[NODE_4] = prop::A3[0];
        chao.props[NODE_11] = prop::A3[1];
        chao.props[NODE_35] = prop::A3[2];
        chao.count_a = 300 + rng.below(301);
        chao.mode = 1;
    }
    if chao.mode == 1 {
        let t = chao.count_a;
        chao.count_a -= 1;
        if t <= 0 {
            match motion_id(chao) {
                0x161 => {
                    if rng.r() < 0.6 {
                        chao.play_phase(0x161);
                    } else {
                        chao.play_blend(0x162, 10);
                    }
                    a_next(chao, rng, 181);
                }
                0x162 => {
                    chao.play_blend(0x161, 10);
                    a_next(chao, rng, 181);
                }
                _ => {}
            }
        }
        match motion_id(chao) {
            0x161 => {
                let old = chao.count_b;
                chao.count_b += 1;
                if old % 16 == 0 {
                    chao.sound(0x4008);
                }
            }
            0x162 => {
                let old = chao.count_b;
                chao.count_b += 1;
                if old % 12 == 0 && rng.r() < 0.4 {
                    chao.sound(0x4008);
                }
            }
            _ => {}
        }
    }
    while_heard(chao, senses, 0xA5)
}

/// `0x59DD10` A4: 0x163, block A4 in hand 11, started over every 300..600 frames; every 180
/// frames 50% sound 0x400A.
fn skill_a4(chao: &mut Chao, senses: &dyn Senses, rng: &mut Rng) -> Next {
    if chao.mode == 0 {
        a_start(chao, 0x163);
        chao.props[NODE_11] = prop::A4;
        chao.count_a = 300 + rng.below(301);
        chao.mode = 1;
    }
    if chao.mode == 1 {
        a_every_180(chao, rng, 0x400A);
    }
    while_heard(chao, senses, 0xA5)
}

/// A4 / A6: start over when the count runs out; every 180 frames 50% `sound`.
fn a_every_180(chao: &mut Chao, rng: &mut Rng, sound: u16) {
    let t = chao.count_a;
    chao.count_a -= 1;
    if t <= 0 {
        chao.mode = 0;
    }
    let old = chao.count_b;
    chao.count_b += 1;
    if old % 180 == 0 && rng.r() < 0.5 {
        chao.sound(sound);
    }
}

/// `0x59D5A0` A5: 0x15D (eyes 1, mouth 1); every 300..600 frames, then 60..120, or at the
/// hold pulse: 30% 0x15C with sounds 0x400F and (40%) 0x6033, else 0x15D ↔ 0x15E in phase;
/// sound 0x400F every 16 frames of 0x15D/0x15E.
fn skill_a5(chao: &mut Chao, senses: &dyn Senses, rng: &mut Rng) -> Next {
    if chao.mode == 0 {
        chao.play(0x15D);
        chao.set_eyes(1, -1);
        chao.set_mouth(1, -1);
        chao.props[NODE_4] = prop::A5[0];
        chao.props[NODE_11] = prop::A5[1];
        chao.count_a = 300 + rng.below(301);
        chao.mode = 1;
    }
    if chao.mode == 1 {
        let t = chao.count_a;
        chao.count_a -= 1;
        if t == 0 || chao.motion_held() {
            if rng.r() < 0.3 {
                chao.play_blend(0x15C, 10);
                chao.sound(0x400F);
                if rng.r() < 0.4 {
                    chao.sound(0x6033);
                }
            } else {
                match motion_id(chao) {
                    0x15D => chao.play_phase(0x15E),
                    0x15E => chao.play_phase(0x15D),
                    _ => chao.play_blend(0x15D, 10),
                }
                a_next(chao, rng, 61);
            }
        }
        if matches!(motion_id(chao), 0x15D | 0x15E) {
            let old = chao.count_b;
            chao.count_b += 1;
            if old % 16 == 0 {
                chao.sound(0x400F);
            }
        }
    }
    while_heard(chao, senses, 0xA5)
}

/// `0x59E120` A6: 0x169 (the 50% roll before it is unused; its model `D+0x650` isn't a
/// hand prop), started over every 300..600 frames; every 180 frames 50% sound 0x4010.
fn skill_a6(chao: &mut Chao, senses: &dyn Senses, rng: &mut Rng) -> Next {
    if chao.mode == 0 {
        let _ = rng.r();
        a_start(chao, 0x169);
        chao.count_a = 300 + rng.below(301);
        chao.mode = 1;
    }
    if chao.mode == 1 {
        a_every_180(chao, rng, 0x4010);
    }
    while_heard(chao, senses, 0xA5)
}

/// `0x59E500` A7: 0x170 ↔ 0x16F every 60..120 frames; sound 0x4011 at frame 30 of 0x16F.
fn skill_a7(chao: &mut Chao, senses: &dyn Senses, rng: &mut Rng) -> Next {
    if chao.mode == 0 {
        a_start(chao, 0x170);
        chao.props[NODE_4] = prop::A7;
        a_next(chao, rng, 61);
        chao.mode = 1;
    }
    if chao.mode == 1 {
        let t = chao.count_a;
        chao.count_a -= 1;
        if t <= 0 {
            let next = if motion_id(chao) == 0x170 {
                0x16F
            } else {
                0x170
            };
            chao.play_blend(next, 10);
            a_next(chao, rng, 61);
            chao.count_b = 0;
        }
        chao.count_b += 1;
        if motion_id(chao) != 0x170 && chao.count_b == 30 {
            chao.sound(0x4011);
        }
    }
    while_heard(chao, senses, 0xA5)
}

// --- Skill B ------------------------------------------------------------------------------

/// B0 / B1: the cuts and their motions.
const B0: ([f64; 3], [u16; 4]) = ([0.25, 0.5, 0.75], [0x171, 0x173, 0x175, 0x177]);
const B1: ([f64; 3], [u16; 4]) = (
    [0.35f32 as f64, 0.7f32 as f64, 0.85],
    [0x179, 0x17B, 0x17D, 0x17E],
);

/// `0x59C870` B0, `0x59C980` B1: eyes 4 and a pose by the cuts, picked again at each hold
/// pulse.
fn skill_b_poses(
    chao: &mut Chao,
    senses: &dyn Senses,
    rng: &mut Rng,
    (cuts, motions): ([f64; 3], [u16; 4]),
) -> Next {
    match chao.mode {
        0 | 1 => {
            if chao.mode == 0 {
                chao.set_eyes(4, -1);
                let r = f64::from(rng.r());
                let i = cuts.iter().position(|&c| r < c).unwrap_or(3);
                chao.play(motions[i]);
                chao.mode = 1;
            }
            if chao.motion_held() {
                chao.mode = 0;
            }
        }
        _ => {}
    }
    while_heard(chao, senses, 0xA6)
}

/// `0x59CA90` B2: eyes 4, motion 0x183.
fn skill_b2(chao: &mut Chao, senses: &dyn Senses) -> Next {
    if chao.mode == 0 {
        chao.set_eyes(4, -1);
        chao.play(0x183);
        chao.mode = 1;
    }
    while_heard(chao, senses, 0xA6)
}

/// `0x59CB20` B3: eyes 4, 0x184; at each end pulse 10% 0x185.
fn skill_b3(chao: &mut Chao, senses: &dyn Senses, rng: &mut Rng) -> Next {
    match chao.mode {
        0 | 1 => {
            if chao.mode == 0 {
                chao.set_eyes(4, -1);
                chao.play(0x184);
                chao.mode = 1;
            }
            if chao.motion_done() && rng.r() < 0.1 {
                chao.play(0x185);
            }
        }
        _ => {}
    }
    while_heard(chao, senses, 0xA6)
}

/// `0x59CBF0` B4, by end pulses: 0x138 (30% on to) 0x13A → 0x13B for one → 0x13C → 0x13D →
/// back to 0x138.
fn skill_b4(chao: &mut Chao, senses: &dyn Senses, rng: &mut Rng) -> Next {
    fn next(chao: &mut Chao, motion: u16, mode: u16) {
        chao.play(motion);
        chao.mode = mode;
        chao.count_a = 0;
        chao.count_b = 0;
    }
    match chao.mode {
        0 | 1 => {
            if chao.mode == 0 {
                chao.set_eyes(4, -1);
                next(chao, 0x138, 1);
            }
            if chao.motion_done() && rng.r() < 0.3 {
                next(chao, 0x13A, 2);
            }
        }
        2 if chao.motion_done() => {
            chao.count_b += 1;
            if chao.count_b >= 2 {
                next(chao, 0x13C, 3);
            } else {
                chao.play(0x13B);
            }
        }
        // Their pulse counts reach the 1 they need at once.
        3 if chao.motion_done() => next(chao, 0x13D, 4),
        4 if chao.motion_done() => next(chao, 0x138, 1),
        _ => {}
    }
    while_heard(chao, senses, 0xA6)
}

// --- Skill C ------------------------------------------------------------------------------

/// A skill C performance's voice: every `every`+1 frames, one of `voices` by `trunc(r·m)`
/// (`m` 0: always the first, no roll); the signal it plays by.
struct SkillC {
    every: i32,
    voices: &'static [u16],
    m: f64,
    kind: u8,
}

const C1: SkillC = SkillC {
    every: 240,
    voices: &C_VOICES,
    m: 0.0,
    kind: 0xA7,
};
const C2: SkillC = SkillC { m: 1.99, ..C1 };
const C3: SkillC = SkillC { m: 2.99, ..C1 };
const C4: SkillC = SkillC { m: 3.99, ..C1 };
const TRICK: SkillC = SkillC {
    every: 180,
    voices: &TRICK_VOICES,
    m: 2.99,
    kind: 0xA7,
};
const DARK_TRICK: SkillC = SkillC {
    every: 220,
    voices: &[0x607C],
    m: 0.0,
    kind: 0xA8,
};

/// Skill C's start (eyes 4, motion 0x121) and its every-frame part: at each hold pulse
/// 0x121 or 0x125 (50%); every 30 frames mouth 3 or 2 (50%).
fn c_pose(chao: &mut Chao, rng: &mut Rng) {
    if chao.motion_held() {
        chao.play(if rng.r() < 0.5 { 0x121 } else { 0x125 });
    }
}

fn c_mouth(chao: &mut Chao, rng: &mut Rng) {
    let old = chao.count_b;
    chao.count_b += 1;
    if old % 30 == 0 {
        chao.set_mouth(if rng.r() < 0.5 { 3 } else { 2 }, -1);
    }
}

fn c_start(chao: &mut Chao) {
    chao.set_eyes(4, -1);
    chao.play(0x121);
    chao.mode = 1;
    chao.count_a = 0;
    chao.count_b = 0;
}

/// `0x59A740`, `0x59A8A0`, `0x59AA30`, `0x59ABC0` (skill C levels 1..4), `0x59B0E0` and
/// `0x59B270` (the animal-10 tricks).
fn skill_c(chao: &mut Chao, senses: &dyn Senses, rng: &mut Rng, c: &SkillC) -> Next {
    if chao.mode == 0 {
        c_start(chao);
    }
    if chao.mode == 1 {
        c_pose(chao, rng);
        let old = chao.count_a;
        chao.count_a += 1;
        if old > c.every {
            let i = if c.m > 0.0 { pick_scaled(rng, c.m) } else { 0 };
            chao.count_a = 0;
            chao.sound(c.voices[i.min(c.voices.len() - 1)]);
        }
        c_mouth(chao, rng);
    }
    while_heard(chao, senses, c.kind)
}

/// `0x59AD50` (skill C level 5+): two cadences. Mode 1 every 241 frames: 70% a voice of the
/// four, else the special 0x6080 and mode 2. Mode 2 every 451: 30% the special, else a voice
/// and back to mode 1. After a voice it checks for 0xA7 an extra time.
fn skill_c5(chao: &mut Chao, senses: &dyn Senses, rng: &mut Rng) -> Next {
    if chao.mode == 0 {
        c_start(chao);
    }
    let (every, mode) = match chao.mode {
        1 => (240, 1),
        2 => (450, 2),
        _ => return while_heard(chao, senses, 0xA7),
    };
    c_pose(chao, rng);
    let old = chao.count_a;
    chao.count_a += 1;
    if old > every {
        chao.count_a = 0;
        let special = if mode == 1 {
            rng.r() >= 0.7
        } else {
            rng.r() < 0.3
        };
        if special {
            chao.sound(C_SPECIAL);
            chao.mode = 2;
        } else {
            let i = pick_scaled(rng, 3.99).min(3);
            chao.sound(C_VOICES[i]);
            chao.mode = 1;
        }
        // `0x54B1F0(0xA7)`.
        if heard(chao, senses, 0xA7).is_none() {
            return Next::Pop;
        }
    }
    c_mouth(chao, rng);
    while_heard(chao, senses, 0xA7)
}

// --- Watchers and singing ------------------------------------------------------------------

/// `0x59E710` (0xA5), `0x59CDA0` (0xA6), `0x59B3D0` (0xA7): watch, 0x181 or 0x3F (50%), eyes
/// 4, mouth 3, re-picked every 300..600 frames while the signal lasts; then 0x23D for
/// 180..300 frames and done.
fn watch(chao: &mut Chao, senses: &dyn Senses, rng: &mut Rng, kind: u8) -> Next {
    match chao.mode {
        0 | 1 => {
            if chao.mode == 0 {
                a_start(chao, if rng.r() < 0.5 { 0x181 } else { 0x3F });
                chao.count_a = 300 + rng.below(301);
                chao.mode = 1;
            }
            let t = chao.count_a;
            chao.count_a -= 1;
            if t <= 0 {
                chao.mode = 0;
            }
            watch_end(chao, senses, rng, kind, 0x23D);
            Next::Keep
        }
        2 => count_down(chao),
        _ => Next::Keep,
    }
}

/// The watchers' signal check: gone → `motion`, 180..300 frames, mode 2.
fn watch_end(chao: &mut Chao, senses: &dyn Senses, rng: &mut Rng, kind: u8, motion: u16) {
    if heard(chao, senses, kind).is_none() {
        chao.play(motion);
        chao.count_a = 180 + rng.below(121);
        chao.mode = 2;
    }
}

/// `0x59CF50` (0xA6, the other watcher): 0x17F, eyes 4, mouth 3 while the signal lasts;
/// then 0x23E for 180..300 frames and done.
fn watch_b(chao: &mut Chao, senses: &dyn Senses, rng: &mut Rng) -> Next {
    match chao.mode {
        0 | 1 => {
            if chao.mode == 0 {
                a_start(chao, 0x17F);
                // Set and never read.
                chao.count_a = 300 + rng.below(301);
                chao.mode = 1;
            }
            watch_end(chao, senses, rng, 0xA6, 0x23E);
            Next::Keep
        }
        2 => count_down(chao),
        _ => Next::Keep,
    }
}

/// `0x59B580` (picker #10, the dark trick): 0x232 (blend 35) when sitting or lying, else
/// 0x3F; eyes 1 or 3, mouth 8 or 4; 300..600 frames, then 50% emotion `0x12D` +150.
fn scorn(chao: &mut Chao, rng: &mut Rng) -> Next {
    match chao.mode {
        0 => {
            if (1..=3).contains(&chao.posture()) {
                chao.play_blend(0x232, 35);
            } else {
                chao.play(0x3F);
            }
            chao.set_eyes(if rng.r() < 0.5 { 1 } else { 3 }, -1);
            chao.set_mouth(if rng.r() < 0.5 { 8 } else { 4 }, -1);
            chao.count_a = 300 + rng.below(301);
            chao.mode = 1;
        }
        1 => {}
        _ => return Next::Keep,
    }
    if count_down(chao) == Next::Keep {
        return Next::Keep;
    }
    if rng.r() < 0.5 {
        chao.rec.add_emotion(0x12D, 150);
    }
    Next::Pop
}

/// `0x5AB2C0` sing (song `Chao::song`): eyes 4, 0x143 (blend 20) with block 1 in hand 4;
/// block 2 in hand 11 at its end pulse; at the hold pulse signal 0xA9 (radius 2, 1800
/// frames) 3 units ahead where the song object goes (`0x5AB200`), 0x145 (blend 20); 1800
/// frames, 30% sound 0x1026 every 120.
fn sing(chao: &mut Chao, rng: &mut Rng) -> Next {
    match chao.mode {
        0 | 1 => {
            if chao.mode == 0 {
                chao.set_eyes(4, -1);
                chao.play_blend(0x143, 20);
                chao.props[NODE_4] = prop::SING[0];
                chao.mode = 1;
            }
            if chao.motion_done() {
                chao.props[NODE_11] = prop::SING[1];
                chao.mode = 2;
            }
        }
        2 => {
            if chao.motion_held() {
                let at = ahead(chao, 3.0);
                chao.emit(0xA9, at, 2.0, 1800);
                chao.play_blend(0x145, 20);
                chao.mode = 3;
                chao.count_a = 1800;
            }
        }
        3 => {
            if count_down(chao) == Next::Pop {
                return Next::Pop;
            }
            let old = chao.count_b;
            chao.count_b += 1;
            if old % 120 == 0 && rng.r() < 0.3 {
                chao.sound(0x1026);
            }
        }
        _ => {}
    }
    Next::Keep
}

#[cfg(test)]
mod tests {
    use super::super::super::land::{GRID_CELLS, Tri};
    use super::super::super::picker::{Alone, Tree};
    use super::super::super::record::Record;
    use super::*;
    use std::cell::Cell;

    /// A flat floor at y = 0 in 20-unit tiles, all land.
    fn lawn() -> Garden {
        let mut tris = Vec::new();
        for i in -10..10 {
            for j in -10..10 {
                let (x, z) = (i as f32 * 20.0, j as f32 * 20.0);
                tris.push(Tri {
                    v: [[x, 0.0, z], [x + 20.0, 0.0, z], [x, 0.0, z + 20.0]],
                    flags: 1,
                });
                tris.push(Tri {
                    v: [
                        [x + 20.0, 0.0, z],
                        [x + 20.0, 0.0, z + 20.0],
                        [x, 0.0, z + 20.0],
                    ],
                    flags: 1,
                });
            }
        }
        Garden {
            tris,
            grid: vec![1; GRID_CELLS],
            ledges: Vec::new(),
            blockers: Vec::new(),
            toys: Vec::new(),
            tree_spots: Vec::new(),
        }
    }

    fn child() -> Chao {
        let mut rec = Record::default();
        rec.set_u8(off::TYPE, 2);
        Chao::new([0.0; 3], 0, rec)
    }

    /// Another Chao's signal of `kind` while `on`, sent from `from` by one doing `doing`.
    struct Performer {
        kind: u8,
        on: Cell<bool>,
        from: V3,
        doing: u32,
    }

    impl Performer {
        fn new(kind: u8, doing: u32) -> Self {
            Self {
                kind,
                on: Cell::new(true),
                from: [0.0, 0.0, 30.0],
                doing,
            }
        }
    }

    impl Senses for Performer {
        fn signal(&self, kind: u8) -> Option<Signal> {
            (self.on.get() && kind == self.kind).then_some(Signal {
                pos: [0.0, 0.0, 22.0],
                parent: Some((self.from, self.doing)),
            })
        }
    }

    /// Frames until the Chao is no longer doing `address` (None after `limit`), recording the
    /// motions it plays and the sounds it makes.
    fn run_out(
        chao: &mut Chao,
        address: u32,
        senses: &dyn Senses,
        rng: &mut Rng,
        limit: usize,
        motions: &mut Vec<u16>,
        sounds: &mut Vec<u16>,
    ) -> Option<usize> {
        let garden = lawn();
        for f in 1..=limit {
            chao.frame(&garden, 0, senses, rng);
            if motions.last() != Some(&chao.motion) {
                motions.push(chao.motion);
            }
            sounds.append(&mut chao.sounds);
            if chao.current() != Some(Behaviour::Game(address)) {
                return Some(f);
            }
        }
        None
    }

    fn queue(chao: &Chao) -> Vec<Behaviour> {
        chao.stack.iter().flatten().copied().collect()
    }

    #[test]
    fn an_absorbed_animal_with_a_play_picks_it() {
        let mut chao = child();
        chao.rec.set_u32(ANIMALS, 1 << 2);
        assert!(!chao.animal_play(&Alone, &mut Rng::new(1)));
        chao.rec.set_u32(ANIMALS, 1 << 2 | 1 << 13);
        assert!(chao.animal_play(&Alone, &mut Rng::new(1)));
        assert_eq!(chao.current(), Some(Behaviour::Game(0x5A2A10)));
    }

    #[test]
    fn animal_0_scoots_forward_for_its_pulses() {
        let mut chao = child();
        chao.set(Behaviour::Game(0x5A2BF0), -1);
        let (mut m, mut s) = (Vec::new(), Vec::new());
        let frames = run_out(
            &mut chao,
            0x5A2BF0,
            &Alone,
            &mut Rng::new(5),
            2000,
            &mut m,
            &mut s,
        )
        .expect("ends");
        // 60 frames to the first end pulse (held after), then 120..180 more.
        assert!((180..=245).contains(&frames), "{frames}");
        assert!(m.contains(&0x71));
        assert!(chao.body.pos[2] > 0.15 * 170.0, "{:?}", chao.body.pos);
    }

    #[test]
    fn animal_12_signals_behind_it() {
        let mut chao = child();
        chao.set(Behaviour::Game(0x5A25D0), -1);
        let (mut m, mut s) = (Vec::new(), Vec::new());
        let frames = run_out(
            &mut chao,
            0x5A25D0,
            &Alone,
            &mut Rng::new(9),
            1000,
            &mut m,
            &mut s,
        )
        .expect("ends");
        assert!((240..=485).contains(&frames), "{frames}");
        assert!(s.contains(&0x1013));
        // The signal (60 frames) is gone again by the end.
        assert!(chao.signals.is_empty());
    }

    #[test]
    fn animal_7_goes_to_a_grown_tree_near_by() {
        struct Grove(u8);
        impl Senses for Grove {
            fn nearest_tree(&self) -> Option<Tree> {
                Some(Tree {
                    pos: [10.0, 0.0, 10.0],
                    dist: 14.0,
                    state: self.0,
                })
            }
        }
        let garden = lawn();
        let mut went = 0;
        for seed in 0..20 {
            let mut chao = child();
            chao.set(Behaviour::Game(0x5A2E40), -1);
            chao.frame(&garden, 0, &Grove(3), &mut Rng::new(seed * 1000));
            if chao.current() == Some(Behaviour::Game(0x56BA80)) {
                went += 1;
                assert_eq!(
                    queue(&chao),
                    [0x56BA80, 0x56B6C0, 0x5A2DA0].map(Behaviour::Game)
                );
                assert_eq!(chao.body.target, [10.0, 0.0, 10.0]);
            }
            let mut chao = child();
            chao.set(Behaviour::Game(0x5A2E40), -1);
            chao.frame(&garden, 0, &Grove(1), &mut Rng::new(seed * 1000));
            assert_eq!(chao.current(), Some(Behaviour::Settle));
        }
        assert!((8..20).contains(&went), "{went}");
    }

    #[test]
    fn animal_11_stands_up_then_plays() {
        let mut chao = child();
        chao.rec.set_u32(ANIMALS, 1 << 11);
        assert!(chao.animal_play(&Alone, &mut Rng::new(2)));
        chao.frame(&lawn(), 0, &Alone, &mut Rng::new(2));
        assert_eq!(chao.current(), Some(Behaviour::StandUpSlow));
        chao.frame(&lawn(), 0, &Alone, &mut Rng::new(2));
        assert_eq!(chao.current(), Some(Behaviour::Game(0x5A2910)));
    }

    #[test]
    fn skill_a_performs_while_its_own_signal_lasts() {
        let mut chao = child();
        chao.rec.set_u8(SKILLS_A, 1);
        let mut rng = Rng::new(4);
        assert!(chao.skill_a(&Alone, &mut rng));
        assert_eq!(chao.current(), Some(Behaviour::Game(0x59D8E0)));
        let signal = chao.signals[0];
        assert_eq!((signal.kind, signal.radius), (0xA5, 20.0));
        assert_eq!(signal.pos, [0.0, 0.0, 8.0]);
        assert!((1800..=4200).contains(&signal.frames));
        // Another performance near by: no second one.
        assert!(!chao.clone().skill_a(&Alone, &mut rng));
        let (mut m, mut s) = (Vec::new(), Vec::new());
        let mut chao2 = chao.clone();
        chao2.frame(&lawn(), 0, &Alone, &mut rng);
        assert_eq!(chao2.props, [0, prop::A0, 0]);
        assert_eq!((chao2.eyes, chao2.mouth), (4, 3));
        let frames = run_out(&mut chao, 0x59D8E0, &Alone, &mut rng, 5000, &mut m, &mut s)
            .expect("ends with its signal");
        assert_eq!(frames as i32, signal.frames + 2);
        assert!(s.iter().filter(|&&x| x == 0x4009).count() >= frames / 30);
        // Done: the props go with the next behaviour.
        assert_eq!(chao.props, [0; 3]);
    }

    #[test]
    fn a_new_behaviour_deletes_the_signal() {
        let mut chao = child();
        chao.rec.set_u8(SKILLS_B, 1 << 3);
        assert!(chao.skill_b(&Alone, &mut Rng::new(3)));
        assert_eq!(chao.current(), Some(Behaviour::Game(0x59CB20)));
        assert_eq!(chao.signals.len(), 1);
        chao.set(Behaviour::Settle, -1);
        assert!(chao.signals.is_empty());
    }

    #[test]
    fn no_skill_a_while_another_performs() {
        let mut chao = child();
        chao.rec.set_u8(SKILLS_A, 0xFF);
        let other = Performer::new(0xA5, 0x59D8E0);
        assert!(!chao.skill_a(&other, &mut Rng::new(1)));
        other.on.set(false);
        assert!(chao.skill_a(&other, &mut Rng::new(1)));
    }

    #[test]
    fn skill_a_variants_play_their_motions_and_props() {
        for (i, address) in SKILL_A.into_iter().enumerate() {
            let mut chao = child();
            chao.rec.set_u8(SKILLS_A, 1 << i);
            let mut rng = Rng::new(11);
            assert!(chao.skill_a(&Alone, &mut rng));
            assert_eq!(chao.current(), Some(Behaviour::Game(address)));
            let (mut m, mut s) = (Vec::new(), Vec::new());
            assert!(run_out(&mut chao, address, &Alone, &mut rng, 5000, &mut m, &mut s).is_some());
            let first = m[0];
            let expect: &[u16] = match i {
                0 => &[0x15F, 0x160],
                1 => &[0x165],
                2 => &[0x16B],
                3 => &[0x161],
                4 => &[0x163],
                5 => &[0x15D],
                6 => &[0x169],
                _ => &[0x170],
            };
            assert!(expect.contains(&first), "{address:#x}: {m:x?}");
            assert!(!s.is_empty(), "{address:#x} made no sound");
        }
    }

    #[test]
    fn skill_c_levels_and_tricks() {
        let level = |bits: u8| {
            let mut chao = child();
            chao.rec.set_u8(SKILLS_C, bits);
            assert!(chao.skill_c(&Alone, &mut Rng::new(1)));
            (chao.current(), chao.signals[0].kind)
        };
        assert_eq!(level(0b00010), (Some(Behaviour::Game(0x59A740)), 0xA7));
        assert_eq!(level(0b00011), (Some(Behaviour::Game(0x59A8A0)), 0xA7));
        assert_eq!(level(0b11111), (Some(Behaviour::Game(0x59AD50)), 0xA7));
        let trick = |alignment: f32| {
            let mut chao = child();
            assert!(!chao.skill_c(&Alone, &mut Rng::new(1)));
            chao.rec.set_u32(ANIMALS, 0x400);
            chao.rec.set_f32(off::ALIGNMENT, alignment);
            assert!(chao.skill_c(&Alone, &mut Rng::new(1)));
            (chao.current(), chao.signals[0].kind, chao.signals[0].frames)
        };
        let (dark, kind, frames) = trick(-1.0);
        assert_eq!((dark, kind), (Some(Behaviour::Game(0x59B270)), 0xA8));
        assert!((1800..=3600).contains(&frames));
        let (light, kind, frames) = trick(-0.5);
        assert_eq!((light, kind), (Some(Behaviour::Game(0x59B0E0)), 0xA7));
        assert!((1200..=2400).contains(&frames));
    }

    #[test]
    fn skill_c5_speaks_in_two_cadences() {
        let mut chao = child();
        chao.rec.set_u8(SKILLS_C, 0x1F);
        let mut rng = Rng::new(21);
        assert!(chao.skill_c(&Alone, &mut rng));
        let (mut m, mut s) = (Vec::new(), Vec::new());
        assert!(run_out(&mut chao, 0x59AD50, &Alone, &mut rng, 4000, &mut m, &mut s).is_some());
        assert!(s.iter().any(|x| C_VOICES.contains(x) || *x == C_SPECIAL));
        assert!(m.contains(&0x121));
    }

    #[test]
    fn a_performer_senses_its_own_signal_only_within_reach() {
        let mut chao = child();
        chao.emit(0xA7, [0.0, 0.0, 8.0], 20.0, 100);
        assert!(heard(&chao, &Alone, 0xA7).is_some());
        // Its sphere (1.5 around 2 up) must reach the signal's 20.
        chao.body.pos = [0.0, 0.0, 30.0];
        assert!(heard(&chao, &Alone, 0xA7).is_none());
        chao.body.pos = [0.0, 0.0, 29.0];
        assert!(heard(&chao, &Alone, 0xA7).is_some());
    }

    #[test]
    fn skill_b4_runs_its_cycle() {
        let mut chao = child();
        chao.set(Behaviour::Game(0x59CBF0), -1);
        let always = Performer::new(0xA6, 0);
        let (mut m, mut s) = (Vec::new(), Vec::new());
        let mut rng = Rng::new(8);
        assert_eq!(
            run_out(&mut chao, 0x59CBF0, &always, &mut rng, 3000, &mut m, &mut s),
            None
        );
        for motion in [0x138, 0x13A, 0x13B, 0x13C, 0x13D] {
            assert!(m.contains(&motion), "{m:x?}");
        }
        always.on.set(false);
        assert!(run_out(&mut chao, 0x59CBF0, &always, &mut rng, 2, &mut m, &mut s).is_some());
    }

    #[test]
    fn a_watcher_watches_until_the_signal_goes() {
        let mut chao = child();
        chao.set(Behaviour::Game(0x59E710), -1);
        let show = Performer::new(0xA5, 0x59D8E0);
        let (mut m, mut s) = (Vec::new(), Vec::new());
        let mut rng = Rng::new(3);
        assert_eq!(
            run_out(&mut chao, 0x59E710, &show, &mut rng, 1500, &mut m, &mut s),
            None
        );
        assert_eq!((chao.eyes, chao.mouth), (4, 3));
        show.on.set(false);
        let frames = run_out(&mut chao, 0x59E710, &show, &mut rng, 1000, &mut m, &mut s);
        assert!(
            frames.is_some_and(|f| (181..=302).contains(&f)),
            "{frames:?}"
        );
        assert!(m.contains(&0x23D));
    }

    #[test]
    fn joining_skill_a_with_or_without_it() {
        let show = Performer::new(0xA5, 0x59D8E0);
        let mut chao = child();
        chao.rec.set_u16(off::TEDIOUS, 9000);
        let mut best = 0.2;
        chao.join_skill_a(&mut best, &show, &mut Rng::new(1));
        assert_eq!(best, 0.99);
        assert_eq!(
            queue(&chao),
            [
                Behaviour::Game(0x56B500),
                Behaviour::StandUpSlow,
                Behaviour::Game(0x59E710)
            ]
        );
        assert_eq!(chao.body.target, show.from);
        assert_eq!(chao.performer_lonely, None);

        let mut chao = child();
        chao.rec.set_u16(off::TEDIOUS, 9000);
        chao.rec.set_u16(off::LONELY, 5000);
        chao.rec.set_u8(SKILLS_A, 1 << 4);
        let mut best = 0.2;
        chao.join_skill_a(&mut best, &show, &mut Rng::new(1));
        assert_eq!(
            queue(&chao),
            [Behaviour::Game(0x56B500), Behaviour::Game(0x59DD10)]
        );
        assert_eq!(chao.body.target, [0.0, 0.0, 22.0]);
        assert_eq!(chao.rec.u16(off::TEDIOUS), 5000);
        assert_eq!(chao.rec.u16(off::LONELY), 1000);
        assert_eq!(chao.performer_lonely, Some((0xA5, -4000)));

        // Bored only a little: no score.
        let mut chao = child();
        chao.rec.set_u16(off::TEDIOUS, 1000);
        let mut best = 0.2;
        chao.join_skill_a(&mut best, &show, &mut Rng::new(1));
        assert_eq!(best, 0.2);
    }

    #[test]
    fn joining_skill_b_needs_the_same_one() {
        let mut chao = child();
        chao.rec.set_u16(off::TEDIOUS, 9000);
        chao.rec.set_u8(SKILLS_B, 0x1F);
        let mut best = 0.2;
        chao.join_skill_b(&mut best, &Performer::new(0xA6, 0x59CA90), &mut Rng::new(1));
        assert_eq!(
            queue(&chao),
            [Behaviour::Game(0x56B500), Behaviour::Game(0x59CA90)]
        );
        assert_eq!(chao.performer_lonely, Some((0xA6, -3000)));

        // B0 is never joined.
        let mut chao = child();
        chao.rec.set_u16(off::TEDIOUS, 9000);
        chao.rec.set_u8(SKILLS_B, 0x1F);
        let mut best = 0.2;
        chao.join_skill_b(&mut best, &Performer::new(0xA6, 0x59C870), &mut Rng::new(1));
        let q = queue(&chao);
        assert_eq!(q[..2], [Behaviour::Game(0x56B500), Behaviour::StandUpSlow]);
        assert!(matches!(q[2], Behaviour::Game(0x59CDA0 | 0x59CF50)));
    }

    #[test]
    fn the_dark_trick_angers_half_the_time() {
        let mut angry = 0;
        for seed in 0..20 {
            let mut chao = child();
            chao.rec.set_u16(off::TEDIOUS, 3000);
            let mut best = 0.2;
            let mut rng = Rng::new(seed * 1000);
            chao.scorn_dark_trick(&mut best, &Performer::new(0xA8, 0x59B270), &mut rng);
            if best != 0.99 {
                continue;
            }
            let (mut m, mut s) = (Vec::new(), Vec::new());
            let frames = run_out(&mut chao, 0x59B580, &Alone, &mut rng, 1000, &mut m, &mut s);
            assert!(
                frames.is_some_and(|f| (301..=602).contains(&f)),
                "{frames:?}"
            );
            if chao.rec.u8(0x12D) == 150 {
                angry += 1;
            }
        }
        assert!((3..18).contains(&angry), "{angry}");
    }

    #[test]
    fn a_song_for_a_liked_character_or_the_type() {
        let mut chao = child();
        chao.rec.set_u8(off::TYPE, 7);
        assert!(!chao.sing(&Alone, &mut Rng::new(1)));
        chao.rec.set_u16(TOY_SKILLS, 0x100);
        chao.rec.set_u8(off::BONDS + 2 * 6, 90);
        let mut songs = std::collections::BTreeSet::new();
        for seed in 0..30 {
            let mut c = chao.clone();
            assert!(c.sing(&Alone, &mut Rng::new(seed * 1000)));
            assert_eq!(c.current(), Some(Behaviour::Game(0x5AB2C0)));
            assert_eq!(c.rec.i8(off::BONDS + 2 * 6), 50);
            songs.extend(c.song);
        }
        // Character 2's song, or the dark type's 8.
        assert_eq!(songs.into_iter().collect::<Vec<_>>(), [2, 8]);

        struct Full;
        impl Senses for Full {
            fn songs(&self) -> usize {
                100
            }
        }
        assert!(!chao.clone().sing(&Full, &mut Rng::new(1)));
    }

    #[test]
    fn singing_holds_its_props_and_signals() {
        let mut chao = child();
        chao.rec.set_u8(SONGS, 1 << 3);
        let mut rng = Rng::new(0);
        while !chao.sing(&Alone, &mut rng) {}
        assert_eq!(chao.song, Some(0xF));
        let (mut m, mut s) = (Vec::new(), Vec::new());
        let garden = lawn();
        for _ in 0..200 {
            chao.frame(&garden, 0, &Alone, &mut rng);
        }
        assert_eq!(chao.props, [prop::SING[0], prop::SING[1], 0]);
        assert_eq!(chao.signals[0].kind, 0xA9);
        assert_eq!(chao.motion, 0x145);
        let frames = run_out(&mut chao, 0x5AB2C0, &Alone, &mut rng, 2000, &mut m, &mut s);
        assert!(frames.is_some_and(|f| f > 1500), "{frames:?}");
    }

    #[test]
    fn the_picker_installs_learned_play() {
        let garden = lawn();
        let mut seen = std::collections::BTreeSet::new();
        for seed in 0..40 {
            let mut chao = child();
            chao.body.flags |= super::super::super::physics::ON_GROUND;
            chao.rec.set_u16(off::TEDIOUS, 10_000);
            chao.rec.set_u8(SKILLS_A, 1);
            chao.rec.set_u8(SKILLS_B, 1);
            chao.rec.set_u8(SKILLS_C, 1);
            chao.rec.set_u8(SONGS, 1);
            chao.rec.set_u32(ANIMALS, 1);
            chao.choose(&garden, &Alone, &mut Rng::new(seed * 1000));
            if let Some(Behaviour::Game(a)) = chao.current() {
                assert_eq!(chao.rec.u16(off::TEDIOUS), 5000, "{a:#x}");
                seen.insert(a);
            }
        }
        for a in [0x59D8E0, 0x59C870, 0x59A740, 0x5AB2C0, 0x5A2BF0] {
            assert!(seen.contains(&a), "{a:#x} never: {seen:x?}");
        }
    }
}
