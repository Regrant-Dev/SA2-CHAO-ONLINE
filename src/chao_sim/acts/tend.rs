//! The body's needs and its life turns: hungry with no fruit about, rest, the six illnesses,
//! the cocoon and the end of a life (re/chao-ai.md §2A.2, §2A.4, §2B "Life events", §3.2,
//! §4.2). Eating (`0x5607C0`) isn't here: it runs on a fruit in the Chao's hands (`0x56CFF0`)
//! and returns 1 at once without one, so it comes in with the garden's fruit.

use super::super::brain::{Chao, LifeEvent, Next, Rng};
use super::super::land::Garden;
use super::super::picker::{Senses, TIMER_FRUIT};
use super::super::record::{off, scale};

pub(super) fn run(
    chao: &mut Chao,
    address: u32,
    garden: &Garden,
    senses: &dyn Senses,
    rng: &mut Rng,
) -> Option<Next> {
    let _ = (garden, senses);
    Some(match address {
        0x59ED60 => hungry(chao, rng),
        0x59EF40 => starving(chao, rng),
        0x5A1BA0 => rest(chao, rng),
        0x599D40 => ill_0(chao, rng),
        0x599F30 => ill_1(chao, rng),
        0x59A0B0 => ill_2(chao, rng),
        0x59A260 => ill_3(chao, rng),
        0x59A400 => ill_4(chao, rng),
        0x59A5A0 => ill_5(chao, rng),
        0x568D30 => cocoon(chao),
        0x568F60 => end_of_life(chao),
        _ => return None,
    })
}

/// Sitting or lying (`posture − 1 ≤ 2`, unsigned).
fn sitting(chao: &Chao) -> bool {
    (1..=3).contains(&chao.posture())
}

/// A step's wait: counts `count_a` up and says when its old value passed `limit`.
fn waited(chao: &mut Chao, limit: i32) -> bool {
    let old = chao.count_a;
    chao.count_a += 1;
    old > limit
}

fn next_mode(chao: &mut Chao, mode: u16) {
    chao.mode = mode;
    chao.count_a = 0;
    chao.count_b = 0;
}

/// After a hungry spell, the fruit path's gate (`D+0x20C`, `0x59EE8C..0x59EF18`):
/// `trunc(1200 + 0.5·10800·(r + (10000 − HUNGER)/10000))`. The game also clears the
/// picker's pause `D+0x1CC/0x1CE`, which the server doesn't have.
fn fruit_gate(chao: &mut Chao, rng: &mut Rng) {
    let hunger = f64::from(chao.rec.u16(off::HUNGER));
    let r = f64::from(rng.r());
    let gate = (r + (10_000.0 - hunger) / 10_000.0) * (10_800.0 * 0.5) + 1200.0;
    chao.timers[TIMER_FRUIT] = gate as i32;
}

/// `0x59ED60`: hungry, no fruit. Sitting (posture 1): motion 0x5E for 180 frames, then its
/// end; otherwise 0xA3 to its end. Then 61 frames and the fruit gate.
fn hungry(chao: &mut Chao, rng: &mut Rng) -> Next {
    match chao.mode {
        0 => {
            if chao.posture() == 1 {
                chao.play(0x5E);
                chao.mode = 2;
                chao.count_a = 180;
            } else {
                chao.play(0xA3);
                chao.mode = 1;
            }
            chao.set_eyes(3, -1);
        }
        1 => {
            if chao.motion_done() {
                next_mode(chao, 3);
            }
        }
        2 => {
            if chao.count_a > 0 {
                chao.count_a -= 1;
            } else if chao.motion_done() {
                next_mode(chao, 3);
            }
        }
        3 => {
            if !waited(chao, 60) {
                return Next::Keep;
            }
            fruit_gate(chao, rng);
            return Next::Pop;
        }
        _ => {}
    }
    Next::Keep
}

/// `0x59EF40`: starving. Eyes 1, mouth 6, motion 0x1DC (blend 20) for 240..440 frames, then
/// the fruit gate. Tears (`0x6ED270` at two body nodes) while `count_a % 8 < 4` are the
/// client's; every 60 frames (`D+0x30`), r<0.5 → sound 0x6003.
fn starving(chao: &mut Chao, rng: &mut Rng) -> Next {
    match chao.mode {
        0 => {
            chao.set_eyes(1, -1);
            chao.set_mouth(6, -1);
            chao.play(0x1DC);
            chao.count_a = 240 + rng.below(201);
            chao.mode = 1;
        }
        1 => {
            let old = chao.count_a;
            chao.count_a -= 1;
            if old <= 0 {
                fruit_gate(chao, rng);
                return Next::Pop;
            }
        }
        _ => {}
    }
    if chao.frame.is_multiple_of(60) && rng.r() < 0.5 {
        chao.sound(0x6003);
    }
    Next::Keep
}

/// `0x5A1BA0`: rest (after `0x55C430`, installed by the tired candidate). A rest motion
/// (blend 40) for a while; every frame the need latch takes TIRE −trunc(100·(300+p151)/300)
/// and gives CONDITN +trunc(30·(300+p151)/300). When the time is up: done if HUNGER,
/// SLEEPY, TEDIOUS or LONELY is over 8000 or TIRE under 100, else done with r<0.3, else
/// another rest.
fn rest(chao: &mut Chao, rng: &mut Rng) -> Next {
    match chao.mode {
        0 => {
            chao.count_a = 120 + rng.below(501);
            let r = rng.r();
            let id = match r {
                r if r < 0.2 => 0x3E,
                r if r < 0.3 => 0x41,
                r if r < 0.4 => 0x3F,
                r if r < 0.5 => 0x40,
                r if r < 0.6 => 0x41,
                r if r < 0.7 => 0x42,
                r if r < 0.8 => {
                    chao.count_a = 120 + rng.below(121);
                    if rng.r() < 0.5 { 0x43 } else { 0x44 }
                }
                _ => {
                    chao.count_a = 60 + rng.below(121);
                    0x47
                }
            };
            chao.play(id);
            chao.mode = 1;
            return Next::Keep;
        }
        1 => {
            let old = chao.count_a;
            chao.count_a -= 1;
            if old <= 0 {
                let over = [off::HUNGER, off::SLEEPY, off::TEDIOUS, off::LONELY]
                    .iter()
                    .any(|&at| chao.rec.u16(at) > 8000);
                if over || chao.rec.u16(off::TIRE) < 100 || rng.r() < 0.3 {
                    return Next::Pop;
                }
                chao.mode = 0;
            }
        }
        _ => return Next::Keep,
    }
    if chao.rec.need_latch() {
        let p = chao.rec.p(0x151);
        chao.rec.add_need(off::TIRE, -scale(100, p));
        chao.rec.add_need(off::CONDITN, scale(30, p));
    }
    Next::Keep
}

/// `0x599D40`, illness 0. Motion 0xF4/0xF5 sitting, 0xF6/0xF7 otherwise (r<0.5, blend 40),
/// eyes 1/3 and mouth 2/4 (r<0.7). Each time the motion ends: r<0.1 → done; past 11
/// cycles → done; else again. Every 60 frames r<0.4 → sound 0x606A.
fn ill_0(chao: &mut Chao, rng: &mut Rng) -> Next {
    match chao.mode {
        0 => {
            let first = rng.r() < 0.5;
            chao.play(match (sitting(chao), first) {
                (true, true) => 0xF4,
                (true, false) => 0xF5,
                (false, true) => 0xF6,
                (false, false) => 0xF7,
            });
            chao.set_eyes(if rng.r() < 0.7 { 1 } else { 3 }, -1);
            chao.set_mouth(if rng.r() < 0.7 { 2 } else { 4 }, -1);
            next_mode(chao, 1);
        }
        1 => {}
        _ => return Next::Keep,
    }
    if chao.motion_held() {
        if rng.r() < 0.1 {
            return Next::Pop;
        }
        let old = chao.count_b;
        chao.count_b += 1;
        if old > 10 {
            return Next::Pop;
        }
        chao.mode = 0;
    }
    let old = chao.count_a;
    chao.count_a += 1;
    if old % 60 == 0 && rng.r() < 0.4 {
        chao.sound(0x606A);
    }
    Next::Keep
}

/// `0x599F30`, illness 1. Motion 0xF2 sitting, else 0xF3 (blend 40), eyes 1/3 (r<0.7),
/// mouth 8. Once the motion has ended, every 60 frames r<0.3: r<0.4 → done, past 11 cycles
/// → done, else again. At frame 180, sound 0x606B and mouth 4.
fn ill_1(chao: &mut Chao, rng: &mut Rng) -> Next {
    match chao.mode {
        0 => {
            chao.play(if sitting(chao) { 0xF2 } else { 0xF3 });
            chao.set_eyes(if rng.r() < 0.7 { 1 } else { 3 }, -1);
            chao.set_mouth(8, -1);
            next_mode(chao, 1);
        }
        1 => {}
        _ => return Next::Keep,
    }
    if chao.motion_held() && chao.count_a % 60 == 0 && rng.r() < 0.3 {
        if rng.r() < 0.4 {
            return Next::Pop;
        }
        let old = chao.count_b;
        chao.count_b += 1;
        if old > 10 {
            return Next::Pop;
        }
        chao.mode = 0;
    }
    let old = chao.count_a;
    chao.count_a += 1;
    if old == 180 {
        chao.sound(0x606B);
        chao.set_mouth(4, -1);
    }
    Next::Keep
}

/// `0x59A0B0`, illness 2. r<0.2: motion 0 and no wait (the count is left as it is);
/// otherwise motion 0x31 (blend 40), sound 0x1015, 60..120 frames. Eyes 3, mouth 8, r<0.1 →
/// sound 0x602C. When the wait runs out or the motion ends: r<0.2 → done, past 14 cycles →
/// done, else again.
fn ill_2(chao: &mut Chao, rng: &mut Rng) -> Next {
    match chao.mode {
        0 => {
            if rng.r() < 0.2 {
                chao.play(0);
            } else {
                chao.play(0x31);
                chao.sound(0x1015);
                chao.count_a = 60 + rng.below(61);
            }
            chao.set_eyes(3, -1);
            chao.set_mouth(8, -1);
            if rng.r() < 0.1 {
                chao.sound(0x602C);
            }
            chao.mode = 1;
        }
        1 => {}
        _ => return Next::Keep,
    }
    let old = chao.count_a;
    chao.count_a -= 1;
    if old > 0 && !chao.motion_held() {
        return Next::Keep;
    }
    if rng.r() < 0.2 {
        return Next::Pop;
    }
    let old = chao.count_b;
    chao.count_b += 1;
    if old > 13 {
        return Next::Pop;
    }
    chao.mode = 0;
    Next::Keep
}

/// `0x59A260`, illness 3. Motion 0x51 sitting, else 0 (blend 40), eyes 9, mouth 8 (r<0.4)
/// or 0. Every 60 frames r<0.3: r<0.4 → done, else sound 0x1010 and a 61-frame pause (mode
/// 2), then again. The pause zeroes the cycle count, so its "past 3 cycles" exit never comes.
fn ill_3(chao: &mut Chao, rng: &mut Rng) -> Next {
    match chao.mode {
        0 => {
            chao.play(if sitting(chao) { 0x51 } else { 0 });
            chao.set_eyes(9, -1);
            chao.set_mouth(if rng.r() < 0.4 { 8 } else { 0 }, -1);
            next_mode(chao, 1);
        }
        1 => {}
        2 => {
            if waited(chao, 60) {
                next_mode(chao, 0);
            }
            return Next::Keep;
        }
        _ => return Next::Keep,
    }
    let old = chao.count_a;
    chao.count_a += 1;
    if old % 60 == 0 && rng.r() < 0.3 {
        if rng.r() < 0.4 {
            return Next::Pop;
        }
        let old = chao.count_b;
        chao.count_b += 1;
        if old > 2 {
            return Next::Pop;
        }
        next_mode(chao, 2);
        chao.sound(0x1010);
    }
    Next::Keep
}

/// `0x59A400`, illness 4. Motion 0xF8 sitting, else 0xF9 (blend 40), eyes 1/3 (r<0.7),
/// mouth 4/8 (r<0.5). Every 60 frames once the motion has ended, r<0.5: r<0.3 → done, past
/// 11 cycles → done, else again. Sound 0x606E when the count reaches 20.
fn ill_4(chao: &mut Chao, rng: &mut Rng) -> Next {
    match chao.mode {
        0 => {
            chao.play(if sitting(chao) { 0xF8 } else { 0xF9 });
            chao.set_eyes(if rng.r() < 0.7 { 1 } else { 3 }, -1);
            chao.set_mouth(if rng.r() < 0.5 { 4 } else { 8 }, -1);
            next_mode(chao, 1);
        }
        1 => {}
        _ => return Next::Keep,
    }
    let old = chao.count_a;
    chao.count_a += 1;
    if old % 60 == 0 && chao.motion_held() && rng.r() < 0.5 {
        if rng.r() < 0.3 {
            return Next::Pop;
        }
        let old = chao.count_b;
        chao.count_b += 1;
        if old > 10 {
            return Next::Pop;
        }
        chao.mode = 0;
    }
    if chao.count_a == 20 {
        chao.sound(0x606E);
    }
    Next::Keep
}

/// `0x59A5A0`, illness 5. Motion 0x5E sitting, else 0xA3 (blend 40), eyes 1/3 (r<0.7),
/// mouth 8, sound 0x6022 (r<0.3) or 0x6023. Every 60 frames r<0.3: r<0.4 → done, past 11
/// cycles → done, else again.
fn ill_5(chao: &mut Chao, rng: &mut Rng) -> Next {
    match chao.mode {
        0 => {
            chao.play(if sitting(chao) { 0x5E } else { 0xA3 });
            chao.set_eyes(if rng.r() < 0.7 { 1 } else { 3 }, -1);
            chao.set_mouth(8, -1);
            chao.sound(if rng.r() < 0.3 { 0x6022 } else { 0x6023 });
            next_mode(chao, 1);
        }
        1 => {}
        _ => return Next::Keep,
    }
    let old = chao.count_a;
    chao.count_a += 1;
    if old % 60 == 0 && rng.r() < 0.3 {
        if rng.r() < 0.4 {
            return Next::Pop;
        }
        let old = chao.count_b;
        chao.count_b += 1;
        if old > 10 {
            return Next::Pop;
        }
        chao.mode = 0;
    }
    Next::Keep
}

/// `0x568D30`: the cocoon (jump table `0x568F0C`). Each step waits until its count's old
/// value passes the limit: 200 → the cocoon closes; 1800 → evolution (`0x568630`); 900 →
/// it opens; 900; 100 → done. No interrupts while it runs.
fn cocoon(chao: &mut Chao) -> Next {
    match chao.mode {
        0 => {
            chao.life_event = Some(LifeEvent::Cocooned);
            // `0x54A690`, `0x54A730`: it can't be touched or held (the server has neither).
            chao.play(match chao.posture() {
                0 => 0x2F,
                2 | 3 => 0x2A,
                _ => 0x3E,
            });
            next_mode(chao, 1);
            if waited(chao, 200) {
                next_mode(chao, 2);
            }
        }
        1 => {
            if waited(chao, 200) {
                // `0x568CD0(0)`: the cocoon object around it (the client's).
                next_mode(chao, 2);
            }
        }
        2 => {
            if waited(chao, 1800) {
                // `0x568630` evolves it, then `0x56CF40`: the server does it.
                chao.life_event = Some(LifeEvent::Evolve);
                next_mode(chao, 3);
            }
        }
        3 => {
            if waited(chao, 900) {
                // `0x568D10` opens the cocoon, and a jingle by the new type (music, no sound
                // id): chao_g_born_c.adx (types 20..22), chao_g_born_h.adx (hero adult),
                // chao_g_born_d.adx (dark adult), none otherwise.
                next_mode(chao, 4);
            }
        }
        4 => {
            if waited(chao, 900) {
                next_mode(chao, 5);
            }
        }
        5 => {
            if !waited(chao, 100) {
                return Next::Keep;
            }
            // `0x54A6C0`, `0x54A6F0`: touchable again.
            return Next::Pop;
        }
        _ => {}
    }
    Next::Keep
}

/// `0x568F30`: reborn rather than gone when happiness is over 50 and STRESS under 700.
fn reborn(chao: &Chao) -> bool {
    chao.rec.i16(off::HAPPINESS) > 50 && chao.rec.u16(off::STRESS) < 700
}

/// `0x568F60`: the end of a life (jump table `0x569180`). Eyes 3, mouth 4, emotion ball 5
/// for 300 frames; 200 frames → into a cocoon; 1800 → reborn as an egg (`0x568570`) or
/// gone: 900 → the cocoon opens; 930 → the record is cleared (`0x550620`) and the Chao goes.
/// Never returns 1; no interrupts while it runs.
fn end_of_life(chao: &mut Chao) -> Next {
    match chao.mode {
        0 => {
            // `0x54A690`, `0x54A730`: it can't be touched or held.
            chao.set_eyes(3, -1);
            chao.set_mouth(4, -1);
            chao.emotion_ball(5, 300);
            match chao.posture() {
                0 => chao.play(0x2F),
                1 | 3 => chao.play(0x3E),
                2 => chao.play(0x2A),
                _ => {}
            }
            next_mode(chao, 1);
            end_wait(chao);
        }
        1 => end_wait(chao),
        2 => {
            if waited(chao, 1800) {
                // Clears `D+0xA0` bits 0x200, 0x100000, 0x80000 (not modelled).
                if reborn(chao) {
                    // `0x568570`: reincarnations +1, record reset `0x551560`, a new egg
                    // `0x57B9C0` with the same record: the server does it.
                    chao.life_event = Some(LifeEvent::Reborn);
                    next_mode(chao, 5);
                } else {
                    // Jingle chao_g_dead.adx (`0x443480`, music, no sound id).
                    next_mode(chao, 3);
                }
            }
        }
        3 | 5 => {
            if waited(chao, 900) {
                // `0x568D10`: the cocoon opens.
                let next = chao.mode + 1;
                next_mode(chao, next);
            }
        }
        4 => {
            if waited(chao, 930) {
                // `0x550620`: memset(rec, 0, 0x4DC) and the Chao is deleted; the server
                // clears the record and removes it.
                chao.life_event = Some(LifeEvent::Died);
            }
        }
        6 => {
            // The old Chao is deleted (MainSub `0x46F720`); its record went to the egg.
            let _ = waited(chao, 930);
        }
        _ => {}
    }
    Next::Keep
}

/// `0x568F60` mode 1: 200 frames, then into the cocoon (`0x568CD0(1)` reborn, `(2)` gone;
/// the client's).
fn end_wait(chao: &mut Chao) {
    if waited(chao, 200) {
        next_mode(chao, 2);
    }
}

#[cfg(test)]
mod tests {
    use super::super::super::brain::Behaviour;
    use super::super::super::land::{GRID_CELLS, Tri};
    use super::super::super::picker::Alone;
    use super::super::super::record::{Record, TYPE_CHILD};
    use super::*;

    fn garden() -> Garden {
        let tris = vec![
            Tri {
                v: [[-50.0, 0.0, -50.0], [50.0, 0.0, -50.0], [-50.0, 0.0, 50.0]],
                flags: 1,
            },
            Tri {
                v: [[50.0, 0.0, -50.0], [50.0, 0.0, 50.0], [-50.0, 0.0, 50.0]],
                flags: 1,
            },
        ];
        Garden {
            tris,
            grid: vec![1; GRID_CELLS],
            ledges: Vec::new(),
            blockers: Vec::new(),
            toys: Vec::new(),
            tree_spots: Vec::new(),
        }
    }

    fn chao(set: impl Fn(&mut Record)) -> Chao {
        let mut rec = Record::default();
        rec.set_u8(off::TYPE, TYPE_CHILD);
        // The need latch stays shut unless a test opens it.
        rec.set_u16(off::NEED_CLOCK, 1);
        set(&mut rec);
        Chao::new([0.0; 3], 0, rec)
    }

    /// Runs behaviour `address` until it's left; the frames it took (None: still on it).
    fn run_out(chao: &mut Chao, address: u32, max: u32, rng: &mut Rng) -> Option<u32> {
        let garden = garden();
        chao.set(Behaviour::Game(address), -1);
        for n in 1..=max {
            chao.frame(&garden, 0, &Alone, rng);
            if chao.current() != Some(Behaviour::Game(address)) {
                return Some(n);
            }
        }
        None
    }

    #[test]
    fn hungry_waits_out_its_motion_then_shuts_the_fruit_path() {
        let mut c = chao(|r| r.set_u16(off::HUNGER, 8000));
        let mut rng = Rng::new(5);
        let n = run_out(&mut c, 0x59ED60, 1000, &mut rng).unwrap();
        // The stand-in motion (60 frames, a one-frame link, the pulse a frame late), then 62
        // frames of mode 3.
        assert_eq!(n, 3 + 60 + 62);
        assert_eq!(c.current(), Some(Behaviour::Settle));
        // 1200 + 5400·(r + 0.2), less the frame since.
        let gate = c.timers[TIMER_FRUIT];
        assert!((2280 - 1..=7680).contains(&gate), "{gate}");
    }

    #[test]
    fn starving_cries_for_240_to_440_frames() {
        let mut c = chao(|r| r.set_u16(off::HUNGER, 10_000));
        let mut rng = Rng::new(9);
        let garden = garden();
        c.set(Behaviour::Game(0x59EF40), -1);
        c.frame(&garden, 0, &Alone, &mut rng);
        assert_eq!((c.motion, c.eyes, c.mouth), (0x1DC, 1, 6));
        let mut n = 1;
        let mut sounds = Vec::new();
        while c.current() == Some(Behaviour::Game(0x59EF40)) {
            sounds.append(&mut c.sounds);
            c.frame(&garden, 0, &Alone, &mut rng);
            n += 1;
        }
        assert!((242..=442).contains(&n), "{n}");
        assert!(sounds.iter().all(|&s| s == 0x6003));
        // Full: 1200 + 5400·r.
        assert!((1200 - 1..=6600).contains(&c.timers[TIMER_FRUIT]));
    }

    #[test]
    fn rest_follows_standing_up_and_eases_tiredness() {
        let mut c = chao(|r| {
            r.set_u16(off::TIRE, 5000);
            r.set_u16(off::NEED_CLOCK, 0);
        });
        let mut rng = Rng::new(2);
        let garden = garden();
        // As the tired candidate installs it.
        c.set(Behaviour::StandUpSlow, -1);
        c.push(Behaviour::Game(0x5A1BA0));
        c.frame(&garden, 0, &Alone, &mut rng);
        assert_eq!(c.current(), Some(Behaviour::Game(0x5A1BA0)));
        c.frame(&garden, 0, &Alone, &mut rng);
        assert!([0x3E, 0x3F, 0x40, 0x41, 0x42, 0x43, 0x44, 0x47].contains(&c.motion));
        c.frame(&garden, 0, &Alone, &mut rng);
        // One latch: −100 and +30 with personality 0; then it's shut.
        assert_eq!(c.rec.u16(off::TIRE), 4900);
        assert_eq!(c.rec.u16(off::CONDITN), 30);
        for _ in 0..50_000 {
            if c.current() != Some(Behaviour::Game(0x5A1BA0)) {
                break;
            }
            c.frame(&garden, 0, &Alone, &mut rng);
        }
        assert_eq!(c.current(), Some(Behaviour::Settle));
        assert_eq!(c.rec.u16(off::TIRE), 4900);
    }

    #[test]
    fn a_hungry_chao_rests_only_once() {
        // Hungry over 8000: done when the first rest runs out (120..620 frames).
        let mut c = chao(|r| r.set_u16(off::HUNGER, 9000));
        let mut rng = Rng::new(4);
        let n = run_out(&mut c, 0x5A1BA0, 5000, &mut rng).unwrap();
        assert!((122..=622).contains(&n), "{n}");
    }

    #[test]
    fn every_illness_shows_and_passes() {
        // Behaviour, then the motions, eyes, mouths and sounds it may show.
        type Face = (
            u32,
            &'static [u16],
            &'static [i16],
            &'static [i16],
            &'static [u16],
        );
        let faces: [Face; 6] = [
            (0x599D40, &[0xF6, 0xF7], &[1, 3], &[2, 4], &[0x606A]),
            (0x599F30, &[0xF3], &[1, 3], &[8, 4], &[0x606B]),
            (0x59A0B0, &[0, 0x31], &[3], &[8], &[0x1015, 0x602C]),
            (0x59A260, &[0], &[9], &[8, 0], &[0x1010]),
            (0x59A400, &[0xF9], &[1, 3], &[4, 8], &[0x606E]),
            (0x59A5A0, &[0xA3], &[1, 3], &[8], &[0x6022, 0x6023]),
        ];
        let garden = garden();
        for (address, motions, eyes, mouths, sounds) in faces {
            for seed in 0..20 {
                let mut c = chao(|_| {});
                let mut rng = Rng::new(seed);
                c.set(Behaviour::Game(address), -1);
                let mut n = 0;
                while c.current() == Some(Behaviour::Game(address)) {
                    c.frame(&garden, 0, &Alone, &mut rng);
                    // The frame it ends, the next behaviour has the default face.
                    if c.current() == Some(Behaviour::Game(address)) {
                        assert!(motions.contains(&c.motion), "{address:#x}: {:#x}", c.motion);
                        assert!(eyes.contains(&c.eyes), "{address:#x}: eyes {}", c.eyes);
                        assert!(mouths.contains(&c.mouth), "{address:#x}: mouth {}", c.mouth);
                    }
                    for s in c.sounds.drain(..) {
                        assert!(sounds.contains(&s), "{address:#x}: sound {s:#x}");
                    }
                    n += 1;
                    assert!(n < 200_000, "{address:#x} never ends");
                }
                assert_eq!(c.current(), Some(Behaviour::Settle));
            }
        }
    }

    #[test]
    fn illness_2_with_no_wait_rolls_at_once() {
        // Seed 0's first roll is under 0.2 (motion 0, no wait): the cycle roll comes the same
        // frame.
        let mut rng = Rng::new(0);
        assert!(rng.r() < 0.2);
        let mut c = chao(|_| {});
        let mut rng = Rng::new(0);
        let garden = garden();
        c.set(Behaviour::Game(0x59A0B0), -1);
        c.frame(&garden, 0, &Alone, &mut rng);
        assert!(c.mode == 0 || c.current() == Some(Behaviour::Settle));
    }

    #[test]
    fn the_cocoon_evolves_then_opens() {
        let mut c = chao(|r| r.set_f32(off::EVOLUTION, 1.0));
        let mut rng = Rng::new(1);
        let garden = garden();
        c.set(Behaviour::Game(0x568D30), -1);
        let mut events = Vec::new();
        let mut n = 0;
        while c.current() == Some(Behaviour::Game(0x568D30)) {
            c.frame(&garden, 0, &Alone, &mut rng);
            n += 1;
            if let Some(e) = c.life_event.take() {
                events.push((n, e));
            }
        }
        assert_eq!(
            events,
            [(1, LifeEvent::Cocooned), (202 + 1802, LifeEvent::Evolve)]
        );
        assert_eq!(n, 202 + 1802 + 902 + 902 + 102);
        assert_eq!(c.motion, 0x2F);
    }

    fn end_of(set: impl Fn(&mut Record)) -> Vec<(u32, LifeEvent)> {
        let mut c = chao(set);
        let mut rng = Rng::new(1);
        let garden = garden();
        c.set(Behaviour::Game(0x568F60), -1);
        let mut events = Vec::new();
        for n in 1..=4000 {
            c.frame(&garden, 0, &Alone, &mut rng);
            if n == 1 {
                assert_eq!((c.eyes, c.mouth, c.bubble.0, c.motion), (3, 4, 5, 0x2F));
            }
            assert_eq!(c.current(), Some(Behaviour::Game(0x568F60)));
            if let Some(e) = c.life_event.take() {
                events.push((n, e));
            }
        }
        events
    }

    #[test]
    fn an_unhappy_chao_dies() {
        let events = end_of(|r| r.set_u16(off::LIFE, 0));
        assert_eq!(events[0], (202 + 1802 + 902 + 932, LifeEvent::Died));
    }

    #[test]
    fn a_happy_calm_chao_is_reborn() {
        let events = end_of(|r| {
            r.set_u16(off::HAPPINESS, 51);
            r.set_u16(off::STRESS, 699);
        });
        assert_eq!(events, [(202 + 1802, LifeEvent::Reborn)]);
        // Stress at 700 is too much.
        let events = end_of(|r| {
            r.set_u16(off::HAPPINESS, 51);
            r.set_u16(off::STRESS, 700);
        });
        assert_eq!(events[0].1, LifeEvent::Died);
    }
}
