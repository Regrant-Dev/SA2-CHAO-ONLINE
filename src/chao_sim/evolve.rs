//! Evolution: what the cocoon (`0x568D30` mode 2) does to the record when it opens a child
//! into an adult: `0x568630` picks the type and resets the tendencies, `0x5517B0` raises a
//! grade, and `0x56CF40` (the live model swap) leaves two more marks on the record
//! (re/chao-ai.md "Evolution (0x568630)"). No randomness is involved.

use super::record::{Record, off};

/// Record offsets only evolution touches.
mod at {
    /// Run (−) / power (+) tendency, f32.
    pub const RUN_POWER: usize = 0xA8;
    /// Swim (−) / fly (+) tendency, f32.
    pub const SWIM_FLY: usize = 0xAC;
    /// Grade bytes `rec+0x28+i` (0 E .. 5 S): swim, fly, run, power, stamina, ...
    pub const GRADES: usize = 0x28;
    /// Animals absorbed, one bit per animal 0..20.
    pub const ANIMALS: usize = 0x118;
    /// Animal part slots `rec+0x11C..=0x123` (signed; 0xFF = none).
    pub const PARTS: usize = 0x11C;
    /// Eye type byte.
    pub const EYES: usize = 0xD1;
    /// DNA `rec+0x438`; grade allele pair `+0x5C+2i`.
    pub const DNA_GRADES: usize = 0x438 + 0x5C;
}

/// `0x568630` (then `0x5517B0` and the record side of `0x56CF40`): evolve the Chao into its
/// adult type and return that type. The game only runs it on a child (picker #20 `0x562800`
/// requires `rec+0x80 == 2` and evolution `rec+0xC0 >= 1.0`), but it doesn't check itself.
///
/// - Kind (`0x56865F..0x568737`, both limits 0.5 at `0x1312C94`): unless run/power `0xA8`
///   and swim/fly `0xAC` both lie strictly inside (−0.5, 0.5), the larger magnitude wins:
///   `ac <= a8` → power 0x11, or swim 0x08 when `ac <= -a8`; otherwise fly 0x0B, or run
///   0x0E when `ac <= -a8`. Inside, normal 0x05, or Chaos 0x14 when all 21 animal bits
///   `rec+0x118` are set, happiness `(i16) rec+0x82 > 80` and reincarnations
///   `rec+0x8E >= 2` (`0x5686B9..0x5686F4`).
/// - Alignment `rec+0xB0` (limit 0.5 at `0x1312C98`): outside (−0.5, 0.5) it adds 1 (hero,
///   > 0) or 2 (dark, <= 0) (`0x56873C..0x56881C`).
/// - `rec+0xC0`, `0xA8`, `0xAC` = 0.0; `rec+0x80` = type (`0x56876A..0x568784`).
/// - `0x5517B0`/`0x551780`: grade i (swim 0, fly 1, run 2, power 3, stamina 4 for normal and
///   Chaos; table `0x551828`) goes up one when below 5 (S), and the DNA allele of pair
///   `0x438+0x5C+2i` that equalled the old grade (first one first) goes up with it.
/// - `0x56CF40` → `0x539F90`: dark types (7, 10, .., 22) get eyes `rec+0xD1` = 10
///   (`0x53A072..0x53A089`); → `0x566B80`/`0x5668B0`: part slots `rec+0x11C+i` outside
///   0..20 become 0xFF.
pub fn evolve(rec: &mut Record) -> u8 {
    let a8 = rec.f32(at::RUN_POWER);
    let ac = rec.f32(at::SWIM_FLY);
    let b0 = rec.f32(off::ALIGNMENT);
    let inside = |v: f32| v > -0.5 && v < 0.5;

    let mut kind = if inside(a8) && inside(ac) {
        let all_animals = (0..21).all(|bit| rec.u32(at::ANIMALS) & (1 << bit) != 0);
        if all_animals && rec.i16(off::HAPPINESS) > 80 && rec.u16(off::REINCARNATIONS) >= 2 {
            TYPE_CHAOS
        } else {
            TYPE_NORMAL
        }
    } else if ac <= a8 {
        if ac <= -a8 { TYPE_SWIM } else { TYPE_POWER }
    } else if ac <= -a8 {
        TYPE_RUN
    } else {
        TYPE_FLY
    };
    // The save's "raised a hero/dark adult" flags (`0x19F646C` |= 0x10 / 0x40) and the
    // jingles chao_g_born_h2.adx / chao_g_born_d2.adx belong to the owner's game.
    if !inside(b0) {
        kind += if b0 > 0.0 { 1 } else { 2 };
    }

    rec.set_f32(off::EVOLUTION, 0.0);
    rec.set_f32(at::RUN_POWER, 0.0);
    rec.set_f32(at::SWIM_FLY, 0.0);
    rec.set_u8(off::TYPE, kind);
    raise_grade(rec, kind);

    if (7..=22).contains(&kind) && (kind - 7) % 3 == 0 {
        rec.set_u8(at::EYES, 10);
    }
    for slot in 0..8 {
        let part = rec.i8(at::PARTS + slot);
        if !(0..21).contains(&part) {
            rec.set_u8(at::PARTS + slot, 0xFF);
        }
    }
    kind
}

/// Adult types by kind (hero +1, dark +2).
pub const TYPE_NORMAL: u8 = 0x05;
pub const TYPE_SWIM: u8 = 0x08;
pub const TYPE_FLY: u8 = 0x0B;
pub const TYPE_RUN: u8 = 0x0E;
pub const TYPE_POWER: u8 = 0x11;
pub const TYPE_CHAOS: u8 = 0x14;

/// `0x5517B0` → `0x551780` (EDI = rec, ESI = DNA, EAX = grade index).
fn raise_grade(rec: &mut Record, kind: u8) {
    let i = match kind {
        8..=10 => 0,
        11..=13 => 1,
        14..=16 => 2,
        17..=19 => 3,
        5..=7 | 20..=22 => 4,
        _ => return,
    };
    let grade = rec.u8(at::GRADES + i);
    if grade >= 5 {
        return;
    }
    let pair = at::DNA_GRADES + 2 * i;
    if rec.u8(pair) == grade {
        rec.set_u8(pair, grade + 1);
    } else if rec.u8(pair + 1) == grade {
        rec.set_u8(pair + 1, grade + 1);
    }
    rec.set_u8(at::GRADES + i, grade + 1);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn child(run_power: f32, swim_fly: f32, alignment: f32) -> Record {
        let mut rec = Record::default();
        rec.set_u8(off::TYPE, 2);
        rec.set_f32(at::RUN_POWER, run_power);
        rec.set_f32(at::SWIM_FLY, swim_fly);
        rec.set_f32(off::ALIGNMENT, alignment);
        rec.set_f32(off::EVOLUTION, 1.0);
        for slot in 0..8 {
            rec.set_u8(at::PARTS + slot, 0xFF);
        }
        rec
    }

    #[test]
    fn every_kind_and_alignment() {
        let kinds = [
            ((0.0, 0.0), TYPE_NORMAL),
            ((0.0, -0.8), TYPE_SWIM),
            ((0.0, 0.8), TYPE_FLY),
            ((-0.8, 0.0), TYPE_RUN),
            ((0.8, 0.0), TYPE_POWER),
        ];
        let sides = [(0.0, 0), (0.7, 1), (-0.7, 2)];
        for ((a8, ac), kind) in kinds {
            for (b0, add) in sides {
                let mut rec = child(a8, ac, b0);
                let want = kind + add;
                assert_eq!(evolve(&mut rec), want, "{a8} {ac} {b0}");
                assert_eq!(rec.u8(off::TYPE), want);
                assert_eq!(rec.f32(off::EVOLUTION), 0.0);
                assert_eq!(rec.f32(at::RUN_POWER), 0.0);
                assert_eq!(rec.f32(at::SWIM_FLY), 0.0);
                assert_eq!(rec.f32(off::ALIGNMENT), b0, "alignment is kept");
            }
        }
    }

    #[test]
    fn limits_and_ties() {
        // 0.5 on either axis is already out of normal.
        assert_eq!(evolve(&mut child(0.5, 0.0, 0.0)), TYPE_POWER);
        assert_eq!(evolve(&mut child(0.0, -0.5, 0.0)), TYPE_SWIM);
        // The bigger magnitude wins, whatever the other axis.
        assert_eq!(evolve(&mut child(0.6, 0.9, 0.0)), TYPE_FLY);
        assert_eq!(evolve(&mut child(-0.9, 0.6, 0.0)), TYPE_RUN);
        // Ties: a8 == ac → power; ac == -a8 → swim (a8 > 0) or run (a8 < 0).
        assert_eq!(evolve(&mut child(0.7, 0.7, 0.0)), TYPE_POWER);
        assert_eq!(evolve(&mut child(0.7, -0.7, 0.0)), TYPE_SWIM);
        assert_eq!(evolve(&mut child(-0.7, 0.7, 0.0)), TYPE_RUN);
        assert_eq!(evolve(&mut child(-0.7, -0.7, 0.0)), TYPE_SWIM);
        // Alignment: ±0.5 counts; exactly 0.5 is hero, -0.5 dark.
        assert_eq!(evolve(&mut child(0.0, 0.0, 0.5)), TYPE_NORMAL + 1);
        assert_eq!(evolve(&mut child(0.0, 0.0, -0.5)), TYPE_NORMAL + 2);
        assert_eq!(evolve(&mut child(0.0, 0.0, 0.49)), TYPE_NORMAL);
    }

    fn chaos_ready(alignment: f32) -> Record {
        let mut rec = child(0.1, -0.1, alignment);
        rec.set_u32(at::ANIMALS, 0x1F_FFFF);
        rec.set_u16(off::HAPPINESS, 81);
        rec.set_u16(off::REINCARNATIONS, 2);
        rec
    }

    #[test]
    fn chaos() {
        for (b0, want) in [(0.0, 0x14), (0.9, 0x15), (-0.9, 0x16)] {
            assert_eq!(evolve(&mut chaos_ready(b0)), want);
        }
        let mut rec = chaos_ready(0.0);
        rec.set_u32(at::ANIMALS, 0x1F_FFFF & !(1 << 20));
        assert_eq!(evolve(&mut rec), TYPE_NORMAL, "one animal missing");
        let mut rec = chaos_ready(0.0);
        rec.set_u16(off::HAPPINESS, 80);
        assert_eq!(evolve(&mut rec), TYPE_NORMAL, "happiness must be over 80");
        let mut rec = chaos_ready(0.0);
        rec.set_u16(off::HAPPINESS, (-5i16) as u16);
        assert_eq!(evolve(&mut rec), TYPE_NORMAL, "happiness is signed");
        let mut rec = chaos_ready(0.0);
        rec.set_u16(off::REINCARNATIONS, 1);
        assert_eq!(evolve(&mut rec), TYPE_NORMAL, "needs two rebirths");
        // A directional Chao with everything else in place is still directional.
        let mut rec = chaos_ready(0.0);
        rec.set_f32(at::SWIM_FLY, 0.6);
        assert_eq!(evolve(&mut rec), TYPE_FLY);
    }

    #[test]
    fn grade_goes_up_with_its_allele() {
        // Fly: grade 1, both alleles 1: only the first goes up.
        let mut rec = child(0.0, 0.8, 0.0);
        rec.set_u8(at::GRADES + 1, 1);
        rec.set_u8(at::DNA_GRADES + 2, 1);
        rec.set_u8(at::DNA_GRADES + 3, 1);
        evolve(&mut rec);
        assert_eq!(rec.u8(at::GRADES + 1), 2);
        assert_eq!(rec.u8(at::DNA_GRADES + 2), 2, "first matching allele");
        assert_eq!(rec.u8(at::DNA_GRADES + 3), 1);

        // Run: only the second allele matches.
        let mut rec = child(-0.8, 0.0, 0.0);
        rec.set_u8(at::GRADES + 2, 3);
        rec.set_u8(at::DNA_GRADES + 4, 4);
        rec.set_u8(at::DNA_GRADES + 5, 3);
        evolve(&mut rec);
        assert_eq!(rec.u8(at::GRADES + 2), 4);
        assert_eq!(rec.u8(at::DNA_GRADES + 4), 4);
        assert_eq!(rec.u8(at::DNA_GRADES + 5), 4);

        // Neither matches: the grade still goes up.
        let mut rec = child(0.0, -0.8, 0.0);
        rec.set_u8(at::DNA_GRADES, 4);
        rec.set_u8(at::DNA_GRADES + 1, 4);
        evolve(&mut rec);
        assert_eq!(rec.u8(at::GRADES), 1);
        assert_eq!((rec.u8(at::DNA_GRADES), rec.u8(at::DNA_GRADES + 1)), (4, 4));

        // S stays S.
        let mut rec = child(0.8, 0.0, 0.0);
        rec.set_u8(at::GRADES + 3, 5);
        rec.set_u8(at::DNA_GRADES + 6, 5);
        evolve(&mut rec);
        assert_eq!(rec.u8(at::GRADES + 3), 5);
        assert_eq!(rec.u8(at::DNA_GRADES + 6), 5);

        // Normal and Chaos raise stamina; nothing else moves.
        let mut rec = child(0.0, 0.0, -0.9);
        evolve(&mut rec);
        assert_eq!(rec.u8(at::GRADES + 4), 1);
        assert_eq!((0..8).filter(|&i| rec.u8(at::GRADES + i) != 0).count(), 1);
        let mut rec = chaos_ready(0.9);
        evolve(&mut rec);
        assert_eq!(rec.u8(at::GRADES + 4), 1);
    }

    #[test]
    fn dark_eyes_and_parts() {
        for (b0, eyes) in [(0.0, 3), (0.9, 3), (-0.9, 10)] {
            for (a8, ac) in [(0.0, 0.0), (0.0, -0.8), (0.0, 0.8), (-0.8, 0.0), (0.8, 0.0)] {
                let mut rec = child(a8, ac, b0);
                rec.set_u8(at::EYES, 3);
                evolve(&mut rec);
                assert_eq!(rec.u8(at::EYES), eyes);
            }
        }
        let mut rec = chaos_ready(-0.9);
        evolve(&mut rec);
        assert_eq!(rec.u8(at::EYES), 10, "dark Chaos too");

        let mut rec = child(0.0, 0.0, 0.0);
        let parts = [0u8, 20, 21, 0x7F, 0x80, 0xFF, 5, 0xFE];
        for (slot, part) in parts.iter().enumerate() {
            rec.set_u8(at::PARTS + slot, *part);
        }
        evolve(&mut rec);
        let got: Vec<u8> = (0..8).map(|i| rec.u8(at::PARTS + i)).collect();
        assert_eq!(got, [0, 20, 0xFF, 0xFF, 0xFF, 0xFF, 5, 0xFF]);
    }
}
