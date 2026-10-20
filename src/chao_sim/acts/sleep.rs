//! Sleep: drowsing, going to bed, sleeping, nodding off and deep sleep (re/chao-ai.md §2A.3,
//! §3.2, §3.5), decoded from the game's bytes.

use super::super::brain::{Behaviour, Chao, Link, Next, Rng};
use super::super::land::Garden;
use super::super::picker::Senses;
use super::super::record::{self, off};

/// The behaviours here, by game address.
pub(super) const GO_TO_BED: u32 = 0x54EE10;
pub(super) const SLEEPING: u32 = 0x54EF10;
pub(super) const NODDING: u32 = 0x54F7A0;
pub(super) const DROWSY: u32 = 0x54F980;
pub(super) const DEEP_SLEEP: u32 = 0x54FCF0;
pub(super) const RANDOM_TURN: u32 = 0x56B960;

pub(super) fn run(
    chao: &mut Chao,
    address: u32,
    garden: &Garden,
    senses: &dyn Senses,
    rng: &mut Rng,
) -> Option<Next> {
    let _ = garden;
    Some(match address {
        GO_TO_BED => go_to_bed(chao, senses, rng),
        SLEEPING => sleeping(chao, rng),
        NODDING => nodding(chao, rng),
        DROWSY => drowsy(chao, rng),
        DEEP_SLEEP => deep_sleep(chao, rng),
        RANDOM_TURN => random_turn(chao, rng),
        _ => return None,
    })
}

/// A looping motion's end pulse (`D+0xAC & 1` each cycle). Without the game's motion table
/// the stand-in motion holds at its end, so it's started over to pulse again.
fn looped(chao: &mut Chao) -> bool {
    if !chao.motion_done() {
        return false;
    }
    if !chao.has_motions() {
        let id = chao.motion;
        chao.play(id);
    }
    true
}

/// `0x54EC40`: rested enough to wake.
fn awake(chao: &Chao) -> bool {
    chao.rec.u16(off::SLEEPY) < 1000 && chao.rec.u16(off::SLPDPTH) < 2000
}

/// `0x54EC70`: once per need period (the inline `rec+0x12A` latch), by phase `D+0x1B8`:
/// fall asleep deeper, then lose sleepiness and tiredness, then surface.
fn sleep_tick(chao: &mut Chao) {
    if !chao.rec.need_latch() {
        return;
    }
    match chao.phase {
        0 => {
            chao.rec.add_need(off::SLPDPTH, 1500);
            if chao.rec.u16(off::SLPDPTH) >= 10_000 {
                chao.phase += 1;
            }
        }
        1 => {
            chao.rec.add_need(off::SLEEPY, -1500);
            let tire = record::scale(300, chao.rec.p(0x151));
            chao.rec.add_need(off::TIRE, -tire);
            if chao.rec.u16(off::SLEEPY) == 0 {
                chao.phase += 1;
            }
        }
        2 => {
            chao.rec.add_need(off::SLEEPY, -1500);
            chao.rec.add_need(off::SLPDPTH, -3500);
        }
        _ => {}
    }
}

/// `0x54EE10`: to sleep where it stands, or (the nearest tree it sees under 30 away and grown,
/// state 3 or 5, and r > 0.5 + 0.003·p14C) link the tree, walk to it (`0x56BA80`), turn about
/// (`0x56B960`) and sleep there.
fn go_to_bed(chao: &mut Chao, senses: &dyn Senses, rng: &mut Rng) -> Next {
    chao.set(Behaviour::Game(SLEEPING), -1);
    if let Some(tree) = senses.nearest_tree() {
        let t = (0.5 + 0.003 * f64::from(chao.rec.p(0x14C))) as f32;
        // The distance perception noted (list entry `+8`), not today's.
        if rng.r() > t && tree.dist < 30.0 && matches!(tree.state, 3 | 5) {
            chao.link = Some(Link::At(tree.pos));
            chao.set(Behaviour::Game(super::reach::WALK_TO_LINK), -1);
            chao.push(Behaviour::Game(RANDOM_TURN));
            chao.push(Behaviour::Game(SLEEPING));
        }
    }
    Next::Keep
}

/// `0x54EF10`: lie down, sleep in one of two poses with snores and turns, wake when rested.
/// The pose (`D+0x41C`) is `memo[0]`; the sleep tick runs every frame but the last.
fn sleeping(chao: &mut Chao, rng: &mut Rng) -> Next {
    match chao.mode {
        0 => {
            chao.memo[0] = 0.0;
            match chao.posture() {
                0 => {
                    chao.play(0x25);
                    chao.mode = 1;
                }
                1 if chao.rec.u16(off::TIRE) > 7000 && rng.r() < 0.5 => {
                    chao.set(Behaviour::Game(NODDING), -1);
                }
                _ => chao.mode = 2,
            }
        }
        1 => {
            if chao.motion_done() {
                chao.mode = 2;
            }
        }
        2 | 3 => {
            if chao.mode == 2 {
                match chao.memo[0] as i32 {
                    0 => {
                        let p = chao.rec.p(0x14C) as f32;
                        if p > rng.r() * 100.0 {
                            chao.play(0x1B);
                            chao.memo[0] = 1.0;
                        } else {
                            chao.play(0x12);
                            chao.memo[0] = 2.0;
                        }
                    }
                    1 => chao.play(0x1C),
                    2 => chao.play(0x13),
                    _ => {}
                }
                chao.count_a = 1000;
                chao.set_eyes(7, -1);
                chao.mode = 3;
            }
            snooze(chao, rng);
        }
        4 | 6 | 8 => {
            let t = chao.count_a;
            chao.count_a -= 1;
            if t <= 0 || awake(chao) {
                chao.play(match chao.mode {
                    4 => 0x1A,
                    6 => 0x36,
                    _ => 0x39,
                });
                chao.mode += 1;
            }
        }
        5 | 7 | 9 | 11 | 13 => {
            if chao.motion_done() {
                chao.mode = 2;
            }
        }
        10 | 12 => {
            let id = if chao.mode == 10 { 0x24 } else { 0x21 };
            if looped(chao) {
                let t = chao.count_a;
                chao.count_a -= 1;
                if t <= 0 {
                    chao.play(id);
                    chao.mode += 1;
                }
            }
            if awake(chao) {
                chao.play(id);
                chao.mode = if id == 0x24 { 11 } else { 13 };
            }
        }
        15 => {
            chao.play(0x1E);
            chao.mode = 16;
            chao.set_eyes(1, -1);
            chao.set_mouth(3, -1);
        }
        // Pulse 0x8; the forced-behaviour words `D+0x1CC/0x1CE` it clears aren't modelled.
        16 if chao.motion_done() => return Next::Pop,
        // 14 (nothing sets it), and 16 until its pulse: only the sleep tick.
        _ => {}
    }
    sleep_tick(chao);
    Next::Keep
}

/// `0x54F115`: sleeping mode 3, snoring every 120 frames; after 1000 frames roll over
/// (mode 4), toss (6 or 8), stir a few times (10 or 12) or lie on; then the wake test.
fn snooze(chao: &mut Chao, rng: &mut Rng) {
    if chao.count_a % 120 == 0 {
        let (chance, low, high) = match chao.memo[0] as i32 {
            1 => (0.5, 0x6008, 0x6009),
            2 => (0.4, 0x600B, 0x600A),
            _ => (0.0, 0, 0),
        };
        if chance > 0.0 && rng.r() < chance {
            chao.sound(if rng.r() < 0.8 { low } else { high });
        }
    }
    let t = chao.count_a;
    chao.count_a -= 1;
    if t <= 0 {
        let r = rng.r();
        if r < 0.3 {
            chao.count_a = 1000;
            chao.play(0x18);
            chao.mode = 4;
        } else if r < 0.6 {
            chao.count_a = 1000;
            if rng.r() < 0.5 {
                chao.play(0x34);
                chao.mode = 6;
            } else {
                chao.play(0x37);
                chao.mode = 8;
            }
        } else if r < 0.9 {
            let (id, mode) = if rng.r() < 0.5 {
                (0x22, 10)
            } else {
                (0x1F, 12)
            };
            chao.count_a = 2 + (rng.r() * 4.0) as i32;
            chao.play(id);
            chao.mode = mode;
        } else {
            chao.count_a = 1000;
        }
        if rng.r() < 0.25 {
            chao.sound(if rng.r() < 0.5 { 0x600F } else { 0x6011 });
        }
    }
    if awake(chao) {
        chao.mode = 15;
    }
}

/// `0x54F7A0`: sitting, nodding off (tired: maybe one long doze 0x14), until rested.
fn nodding(chao: &mut Chao, rng: &mut Rng) -> Next {
    match chao.mode {
        0 => {
            let doze = f64::from(chao.rec.u16(off::TIRE)) * 0.0001;
            if f64::from(rng.r()) < doze {
                chao.play(0x14);
                chao.mode = 1;
            } else {
                chao.play(0x16);
                chao.mode = 2;
                chao.count_a = (rng.r() * 10.0 + 3.0) as i32;
            }
            chao.set_eyes(7, -1);
        }
        1 => {
            if awake(chao) {
                chao.play(0x3E);
                return Next::Pop;
            }
        }
        2 => {
            if looped(chao) {
                let t = chao.count_a;
                chao.count_a -= 1;
                if t <= 0 {
                    chao.play(0x17);
                    chao.count_a = (rng.r() * 10.0 + 3.0) as i32;
                }
            }
            if awake(chao) {
                chao.play(0x3E);
                return Next::Pop;
            }
        }
        _ => {}
    }
    sleep_tick(chao);
    Next::Keep
}

/// `0x54F980`: a yawn or a nod, then done. Standing: stretch-yawn (0x27, 3..8 cycles, 0x29)
/// or 0x54. Otherwise: 0xF (2..7 cycles, 0x11), 0x1E, or a drowsy 0x15. No need changes.
fn drowsy(chao: &mut Chao, rng: &mut Rng) -> Next {
    match chao.mode {
        0 => {
            if chao.posture() != 0 {
                let r = rng.r();
                if r < 0.3 {
                    chao.count_a = (rng.r() * 6.0 + 2.0) as i32;
                    chao.play(0xF);
                    chao.mode = 4;
                    chao.sound(0x6005);
                    chao.set_eyes(3, -1);
                } else if r < 0.6 {
                    chao.play(0x1E);
                    chao.mode = 6;
                    chao.sound(0x600E);
                    chao.set_eyes(1, -1);
                } else {
                    chao.play(0x15);
                    chao.mode = 7;
                    chao.set_eyes(3, -1);
                    chao.set_mouth(8, -1);
                    return Next::Keep;
                }
            } else if rng.r() < 0.5 {
                chao.count_a = (rng.r() * 6.0 + 3.0) as i32;
                chao.play(0x27);
                chao.mode = 1;
                chao.sound(0x6005);
                chao.set_eyes(3, -1);
            } else {
                chao.play(0x54);
                chao.sound(0x600E);
                chao.mode = 3;
                chao.set_eyes(1, -1);
            }
            chao.set_mouth(2, -1);
        }
        1 | 4 => {
            if looped(chao) {
                let t = chao.count_a;
                chao.count_a -= 1;
                if t <= 0 {
                    let (id, mode) = if chao.mode == 1 { (0x29, 2) } else { (0x11, 5) };
                    chao.play(id);
                    chao.mode = mode;
                }
            }
        }
        2 | 3 | 5 | 6 | 7 => {
            if chao.motion_done() {
                chao.mode = 8;
            }
        }
        // `D+0x1CC/0x1CE` cleared (not modelled), done.
        8 => return Next::Pop,
        _ => {}
    }
    Next::Keep
}

/// `0x54FCF0`: sound asleep where it is; the sleep ball (state 5, 360 frames) every 402
/// frames, SLPDPTH −2 a frame (no latch), done at 0.
fn deep_sleep(chao: &mut Chao, rng: &mut Rng) -> Next {
    if chao.mode == 0 {
        chao.play(match chao.posture() {
            1 => 0x3F,
            2 => 0x12,
            _ => 0,
        });
        chao.set_eyes(9, -1);
        if rng.r() < 0.3 {
            chao.set_mouth(8, -1);
        }
        chao.emotion_ball(5, 360);
        chao.mode = 1;
    }
    if chao.mode == 1 {
        let old = chao.count_a;
        chao.count_a += 1;
        if old > 400 {
            chao.emotion_ball(5, 360);
            chao.count_a = 0;
        }
        chao.rec.add_need(off::SLPDPTH, -2);
        if chao.rec.u16(off::SLPDPTH) == 0 {
            return Next::Pop;
        }
    }
    Next::Keep
}

/// `0x56B960`: turn on the spot ±0x16C BAMS a frame for 0..90 frames.
fn random_turn(chao: &mut Chao, rng: &mut Rng) -> Next {
    match chao.mode {
        0 => {
            chao.play(0x64);
            chao.mode = if rng.r() < 0.5 { 1 } else { 2 };
            chao.count_a = i32::from((rng.r() * 91.0) as u16);
        }
        1 | 2 => {
            let t = chao.count_a;
            chao.count_a -= 1;
            if t <= 0 {
                return Next::Pop;
            }
            chao.body.rot_y = if chao.mode == 1 {
                chao.body.rot_y.wrapping_add(0x16C)
            } else {
                chao.body.rot_y.wrapping_sub(0x16C)
            };
        }
        _ => {}
    }
    Next::Keep
}

#[cfg(test)]
mod tests {
    use super::super::super::land::GRID_CELLS;
    use super::super::super::picker::Alone;
    use super::super::super::record::Record;
    use super::*;

    /// A floor at y = 0 (with none it would fall into the water and swim).
    fn garden() -> Garden {
        use super::super::super::land::Tri;
        Garden {
            tris: vec![
                Tri {
                    v: [
                        [-100.0, 0.0, -100.0],
                        [100.0, 0.0, -100.0],
                        [-100.0, 0.0, 100.0],
                    ],
                    flags: 1,
                },
                Tri {
                    v: [
                        [100.0, 0.0, -100.0],
                        [100.0, 0.0, 100.0],
                        [-100.0, 0.0, 100.0],
                    ],
                    flags: 1,
                },
            ],
            grid: vec![1; GRID_CELLS],
            ledges: Vec::new(),
            blockers: Vec::new(),
            toys: Vec::new(),
            tree_spots: Vec::new(),
        }
    }

    fn chao(address: u32, set: impl Fn(&mut Chao)) -> Chao {
        let mut chao = Chao::new([0.0; 3], 0, Record::default());
        set(&mut chao);
        chao.set(Behaviour::Game(address), -1);
        chao
    }

    /// Steps until the behaviour is gone (or `limit`); opens the need latch every
    /// `latch` frames as the life tick would. Returns the frames it took.
    fn run_out(chao: &mut Chao, address: u32, latch: u32, limit: u32, rng: &mut Rng) -> u32 {
        let garden = garden();
        for f in 1..=limit {
            if latch > 0 && f % latch == 0 {
                chao.rec.set_u16(off::NEED_CLOCK, 0);
            }
            chao.frame(&garden, 0, &Alone, rng);
            if chao.current() != Some(Behaviour::Game(address)) {
                return f;
            }
        }
        limit
    }

    #[test]
    fn postures_follow_the_motion_table() {
        assert_eq!(
            [0, 0x64, 0x3E, 0x12, 0x73, 0xEE].map(super::super::super::motion::fallback_posture),
            [0, 0, 1, 2, 3, 4]
        );
    }

    #[test]
    fn a_sleepy_chao_sleeps_deep_then_wakes_rested() {
        let mut c = chao(SLEEPING, |c| {
            c.rec.set_u16(off::SLEEPY, 9000);
            c.rec.set_u16(off::TIRE, 5000);
        });
        let mut rng = Rng::new(5);
        let garden = garden();
        // Standing: it lies down first.
        c.frame(&garden, 0, &Alone, &mut rng);
        assert_eq!((c.motion, c.mode), (0x25, 1));
        let mut deepest = 0;
        for f in 1..200_000u32 {
            if f % 100 == 0 {
                c.rec.set_u16(off::NEED_CLOCK, 0);
            }
            c.frame(&garden, 0, &Alone, &mut rng);
            deepest = deepest.max(c.rec.u16(off::SLPDPTH));
            if c.current() != Some(Behaviour::Game(SLEEPING)) {
                break;
            }
        }
        assert_eq!(deepest, 10_000);
        assert_eq!(c.current(), Some(Behaviour::Settle));
        assert!(c.rec.u16(off::SLEEPY) < 1000 && c.rec.u16(off::SLPDPTH) < 2000);
        // Phase 1 took 6 ticks of −1500 (9000 → 0) and of TIRE −300.
        assert_eq!(c.rec.u16(off::TIRE), 5000 - 6 * 300);
        assert!(c.sounds.iter().any(|s| (0x6008..=0x6011).contains(s)));
    }

    #[test]
    fn the_sleep_tick_waits_for_the_latch() {
        let mut c = chao(SLEEPING, |_| {});
        let mut rng = Rng::new(1);
        c.rec.set_u16(off::NEED_CLOCK, 1);
        for _ in 0..500 {
            c.frame(&garden(), 0, &Alone, &mut rng);
        }
        assert_eq!(c.rec.u16(off::SLPDPTH), 0);
    }

    #[test]
    fn a_tired_sitting_chao_may_nod_off_instead() {
        let mut rng = Rng::new(2);
        let mut nodded = false;
        for _ in 0..20 {
            let mut c = chao(SLEEPING, |c| {
                c.play(0x3E);
                c.rec.set_u16(off::TIRE, 8000);
                c.rec.set_u16(off::SLEEPY, 9000);
            });
            c.frame(&garden(), 0, &Alone, &mut rng);
            match c.current() {
                Some(Behaviour::Game(NODDING)) => nodded = true,
                Some(Behaviour::Game(SLEEPING)) => assert_eq!(c.mode, 2),
                other => panic!("{other:?}"),
            }
        }
        assert!(nodded);
    }

    #[test]
    fn nodding_ends_when_rested() {
        let mut rng = Rng::new(9);
        let mut c = chao(NODDING, |c| c.rec.set_u16(off::SLEEPY, 3000));
        let frames = run_out(&mut c, NODDING, 50, 100_000, &mut rng);
        assert!(frames < 100_000);
        assert_eq!(c.motion, 0x3E);
        assert!(c.rec.u16(off::SLEEPY) < 1000 && c.rec.u16(off::SLPDPTH) < 2000);
    }

    #[test]
    fn drowsy_yawns_and_is_done() {
        let mut rng = Rng::new(11);
        let mut motions = std::collections::BTreeSet::new();
        for _ in 0..30 {
            let mut c = chao(DROWSY, |c| c.rec.set_u16(off::SLEEPY, 7000));
            let frames = run_out(&mut c, DROWSY, 0, 2000, &mut rng);
            assert!(frames < 2000);
            assert_eq!(c.current(), Some(Behaviour::Settle));
            assert_eq!(c.rec.u16(off::SLEEPY), 7000);
            motions.insert(c.motion);
            assert!(c.sounds.contains(&0x6005) || c.sounds.contains(&0x600E));
        }
        // Standing it ends on 0x29 after the yawn cycles, or 0x54; settle replaced it.
        assert!(!motions.is_empty());
    }

    #[test]
    fn drowsy_while_sitting_may_just_droop() {
        let mut rng = Rng::new(4);
        let mut drooped = false;
        for _ in 0..30 {
            let mut c = chao(DROWSY, |c| c.play(0x3E));
            c.frame(&garden(), 0, &Alone, &mut rng);
            assert!([0xF, 0x1E, 0x15].contains(&c.motion));
            if c.motion == 0x15 {
                drooped = true;
                assert_eq!((c.eyes, c.mouth), (3, 8));
            } else {
                assert_eq!(c.mouth, 2);
            }
        }
        assert!(drooped);
    }

    #[test]
    fn deep_sleep_lasts_its_depth() {
        let mut rng = Rng::new(3);
        let mut c = chao(DEEP_SLEEP, |c| c.rec.set_u16(off::SLPDPTH, 1000));
        let garden = garden();
        c.frame(&garden, 0, &Alone, &mut rng);
        assert_eq!((c.eyes, c.bubble.0), (9, 5));
        assert_eq!(c.rec.u16(off::SLPDPTH), 998);
        // The ball comes again on the 402nd frame.
        for _ in 1..401 {
            c.frame(&garden, 0, &Alone, &mut rng);
        }
        let balls = c.sounds.iter().filter(|&&s| s == 0x100E).count();
        c.frame(&garden, 0, &Alone, &mut rng);
        let more = c.sounds.iter().filter(|&&s| s == 0x100E).count();
        assert_eq!(more, balls + 1);
        let frames = run_out(&mut c, DEEP_SLEEP, 0, 1000, &mut rng);
        assert_eq!(402 + frames, 500);
        assert_eq!(c.rec.u16(off::SLPDPTH), 0);
    }

    #[test]
    fn going_to_bed_without_a_tree_sleeps_on_the_spot() {
        let mut rng = Rng::new(1);
        let mut c = chao(GO_TO_BED, |c| c.rec.set_u16(off::SLEEPY, 9000));
        c.frame(&garden(), 0, &Alone, &mut rng);
        assert_eq!(c.current(), Some(Behaviour::Game(SLEEPING)));
    }

    #[test]
    fn the_random_turn_spins_then_pops() {
        let mut rng = Rng::new(8);
        let mut c = chao(RANDOM_TURN, |_| {});
        let garden = garden();
        c.frame(&garden, 0, &Alone, &mut rng);
        let (dir, n) = (c.mode, c.count_a);
        assert!((0..=90).contains(&n));
        let frames = run_out(&mut c, RANDOM_TURN, 0, 200, &mut rng);
        assert_eq!(frames as i32, n + 1);
        let turn = (0x16C * n) as u16;
        let want = if dir == 1 { turn } else { turn.wrapping_neg() };
        assert_eq!(c.body.rot_y, want);
    }
}
