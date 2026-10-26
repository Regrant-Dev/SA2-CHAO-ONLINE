//! The life tick (`0x5353C0`): every 96 frames per Chao, the clocks in its record drive aging,
//! emotion decay, needs, illness and bonds (re/chao-ai.md §3, §4.2).

use super::brain::Rng;
use super::record::{Record, TYPE_CHILD, off, scale};

/// Frames between a Chao's life ticks: one of 32 slots every 3 frames (`0x5363D0`).
pub const LIFE_TICK_FRAMES: u32 = 96;

/// The garden being lived in, for the alignment drift (2 Hero, 3 Dark).
pub fn life_tick(rec: &mut Record, area: u8, rng: &mut Rng) {
    if fires(rec, off::AGE_CLOCK, 120, true) {
        age_tick(rec, area, rng);
    }
    if fires(rec, off::EMOTION_CLOCK, 7, false) {
        emotion_decay(rec);
    }
    if fires(rec, off::NEED_CLOCK, 10, false) {
        need_tick(rec, rng);
    }
    if fires(rec, off::CONDITION_CLOCK, 80, false) {
        for i in 0..6 {
            let v = rec.i8(off::ILLNESS + i);
            rec.set_u8(off::ILLNESS + i, (v - v.signum()) as u8);
        }
    }
    bond_tick(rec);
}

/// A clock that fires when its old value has reached `period` (then restarts at 0).
fn fires(rec: &mut Record, at: usize, period: u32, wide: bool) -> bool {
    let old = if wide {
        rec.u32(at)
    } else {
        u32::from(rec.u16(at))
    };
    let fire = old >= period;
    let new = if fire { 0 } else { old + 1 };
    if wide {
        rec.set_u32(at, new);
    } else {
        rec.set_u16(at, new as u16);
    }
    fire
}

/// `0x535409..0x535587`.
fn age_tick(rec: &mut Record, area: u8, rng: &mut Rng) {
    rec.set_u16(off::AGE, rec.u16(off::AGE).wrapping_add(1));
    if rec.u8(off::TYPE) == TYPE_CHILD {
        add_evolution(rec, 1.0 / 60.0);
        match area {
            2 => add_alignment(rec, 1.0 / 120.0),
            3 => add_alignment(rec, -1.0 / 120.0),
            _ => {}
        }
    } else {
        let adult = rec.u16(off::ADULT_AGE).wrapping_add(1);
        rec.set_u16(off::ADULT_AGE, adult);
        if rec.u8(off::TYPE) < 0x14 {
            let max = i32::from(rec.i16(off::MAX_LIFE)) - (i32::from(adult) + 100) / 15;
            rec.set_u16(off::MAX_LIFE, max.max(0) as u16);
        }
        let life = rec.i16(off::LIFE).min(rec.i16(off::MAX_LIFE));
        rec.set_u16(off::LIFE, life as u16);
        add_evolution(rec, 1.0 / 240.0);
        match area {
            2 => add_evolution(rec, 1.0 / 240.0),
            3 => add_evolution(rec, -1.0 / 240.0),
            _ => {}
        }
    }
    // Illness onset (`0x535510`).
    if rng.r() < 1.6667e-4 {
        let slot = (rng.r() * 5.9999) as usize;
        if slot != 3 && rec.i8(off::ILLNESS + slot) == 0 {
            rec.set_u8(off::ILLNESS + slot, 0x9C);
        }
    }
}

/// `0x536110`.
fn add_evolution(rec: &mut Record, by: f32) {
    rec.set_f32(
        off::EVOLUTION,
        (rec.f32(off::EVOLUTION) + by).clamp(0.0, 1.2),
    );
}

/// `0x536260`.
fn add_alignment(rec: &mut Record, by: f32) {
    rec.set_f32(
        off::ALIGNMENT,
        (rec.f32(off::ALIGNMENT) + by).clamp(-1.0, 1.0),
    );
}

/// `0x5355AA..0x535946`: each term its own clamped subtraction. (`0x133` isn't touched.)
fn emotion_decay(rec: &mut Record) {
    let p = |i: usize| rec.p(i);
    let terms: [(usize, i32); 12] = [
        (0x12C, 4),
        (0x12D, scale(4, -p(0x14C))),
        (0x12D, scale(4, p(0x14B))),
        (0x12D, scale(4, p(0x155))),
        (0x12E, scale(4, p(0x14C))),
        (0x12E, scale(4, p(0x155))),
        (0x12F, scale(4, p(0x14C))),
        (0x12F, scale(4, p(0x155))),
        (0x130, scale(4, p(0x155))),
        (0x131, scale(4, p(0x151))),
        (0x132, scale(4, p(0x14A))),
        (0x132, scale(4, p(0x156))),
    ];
    for (at, by) in terms {
        rec.add_emotion(at, -by);
    }
}

/// `0x535964..0x535DDE`, in the game's order.
fn need_tick(rec: &mut Record, rng: &mut Rng) {
    let p = |rec: &Record, i: usize| rec.p(i);
    let tire = i32::from(rec.u16(off::TIRE));
    // SLEEPY: trunc(trunc(90·(300+p14D)/300) · (TIRE·6e-5 + 1)).
    let sleepy =
        (scale(90, p(rec, 0x14D)) as f64 * (f64::from(tire) * 5.999_999_8e-5 + 1.0)) as i32;
    rec.add_need(off::SLEEPY, sleepy);
    rec.add_need(off::HUNGER, scale(70, p(rec, 0x150)));
    if (3..=19).contains(&rec.u8(off::TYPE)) {
        rec.add_need(off::BREED, scale(12, p(rec, 0x14F)));
    }
    rec.add_need(off::TEDIOUS, scale(60, p(rec, 0x14A)));
    rec.add_need(off::TEDIOUS, scale(30, p(rec, 0x14C)));
    rec.add_need(off::TEDIOUS, scale(30, -p(rec, 0x155)));
    if rec.u16(off::TEDIOUS) > 9999 && rng.r() < 0.1 {
        rec.set_u16(off::TEDIOUS, 6000);
    }
    rec.add_need(off::LONELY, scale(60, -p(rec, 0x14E)));
    rec.add_need(off::TIRE, scale(10, -p(rec, 0x14F)));
    rec.add_need(off::NOURISH, -36);
    let nourish = rec.u16(off::NOURISH);
    if nourish > 9000 {
        rec.add_need(off::CONDITN, 80);
    }
    if nourish < 1000 {
        rec.add_need(off::CONDITN, -120);
    }
    if rec.u16(off::STRESS) > 8000 {
        rec.add_need(off::CONDITN, -60);
    }
    if rec.u8(0x131) > 100 {
        rec.add_need(off::CONDITN, -300);
    }
    rec.add_need(off::THIRSTY, 150);
}

/// `0x53E2D0`: bonds grow a little every 11 life ticks.
fn bond_tick(rec: &mut Record) {
    let old = rec.u32(off::BOND_CLOCK) as i32;
    if old < 10 {
        rec.set_u32(off::BOND_CLOCK, (old + 1) as u32);
        return;
    }
    rec.set_u32(off::BOND_CLOCK, 0);
    let words = (0..6)
        .map(|k| off::BONDS + 2 + k * 6)
        .chain((0..20).map(|k| off::OTHER_BONDS + k * 0x1C));
    for at in words {
        rec.set_u16(at, (u32::from(rec.u16(at)) + 16).min(50_000) as u16);
    }
}

/// A new Chao's needs (`0x5513C0`, after the record is made).
pub fn new_needs(rec: &mut Record, rng: &mut Rng) {
    rec.set_u16(off::CONDITN, 0);
    rec.set_u16(off::NOURISH, (rng.r() * 5000.0) as u16);
    rec.set_u16(off::THIRSTY, 3000 + (rng.r() * 6000.0) as u16);
    rec.set_u8(off::SLEEPY, 0);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ticks(rec: &mut Record, n: u32, rng: &mut Rng) {
        for _ in 0..n {
            life_tick(rec, 1, rng);
        }
    }

    #[test]
    fn needs_rise_every_eleven_ticks() {
        let mut rec = Record::default();
        rec.set_u8(off::TYPE, TYPE_CHILD);
        let mut rng = Rng::new(1);
        ticks(&mut rec, 10, &mut rng);
        assert_eq!(rec.u16(off::HUNGER), 0);
        ticks(&mut rec, 1, &mut rng);
        // Personality 0: SLEEPY +90, HUNGER +70, TEDIOUS +120, LONELY +60, TIRE +10, THIRSTY +150.
        assert_eq!(
            [
                off::SLEEPY,
                off::HUNGER,
                off::TEDIOUS,
                off::LONELY,
                off::TIRE,
                off::THIRSTY
            ]
            .map(|o| rec.u16(o)),
            [90, 70, 120, 60, 10, 150]
        );
        // Child: no BREED; NOURISH can't go under 0.
        assert_eq!(rec.u16(off::BREED), 0);
        assert_eq!(rec.u16(off::NOURISH), 0);
        // The latch is open right after; using it brings the next tick nearer.
        assert!(rec.need_latch() && !rec.need_latch());
        ticks(&mut rec, 10, &mut rng);
        assert_eq!(rec.u16(off::HUNGER), 140);
    }

    #[test]
    fn emotions_fade_every_eight_ticks() {
        let mut rec = Record::default();
        for i in 0..8 {
            rec.set_u8(off::EMOTIONS + i, 100);
        }
        let mut rng = Rng::new(1);
        ticks(&mut rec, 7, &mut rng);
        assert_eq!(rec.u8(0x12C), 100);
        ticks(&mut rec, 1, &mut rng);
        let got: Vec<u8> = (0..8).map(|i| rec.u8(off::EMOTIONS + i)).collect();
        assert_eq!(got, [96, 88, 92, 92, 96, 96, 92, 100]);
    }

    #[test]
    fn an_adult_ages_and_its_life_runs_down() {
        let mut rec = Record::default();
        rec.set_u8(off::TYPE, 5);
        rec.set_u16(off::LIFE, 3000);
        rec.set_u16(off::MAX_LIFE, 3000);
        let mut rng = Rng::new(1);
        ticks(&mut rec, 121, &mut rng);
        assert_eq!(rec.u16(off::AGE), 1);
        assert_eq!(rec.u16(off::ADULT_AGE), 1);
        // (1 + 100) / 15 = 6.
        assert_eq!([rec.u16(off::MAX_LIFE), rec.u16(off::LIFE)], [2994, 2994]);
        assert!((rec.f32(off::EVOLUTION) - 1.0 / 240.0).abs() < 1e-6);
    }

    #[test]
    fn bonds_grow_and_stop_at_fifty_thousand() {
        let mut rec = Record::default();
        rec.set_u16(off::BONDS + 2, 49_990);
        let mut rng = Rng::new(1);
        ticks(&mut rec, 11, &mut rng);
        assert_eq!(rec.u16(off::BONDS + 2), 50_000);
        assert_eq!(rec.u16(off::BONDS + 8), 16);
        assert_eq!(rec.u16(off::OTHER_BONDS + 19 * 0x1C), 16);
    }
}
