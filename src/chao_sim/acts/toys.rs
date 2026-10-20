//! Toys: the TV, the radio, the horse and the box (re/chao-ai.md §1.5 #11..#14, §2B "TV,
//! radio, horse, box" and "Toy skills", §6.4), and the learned toy skills, decoded from the
//! game's bytes. What the Chao does to a toy goes out as `Chao::toy_events`.

use super::super::brain::{Behaviour, Chao, Link, Next, Rng};
use super::super::land::{Garden, V3};
use super::super::picker::{Senses, TIMER_TOY, ToyEvent, f16, jitter};
use super::super::record::off;

/// The behaviours here, by game address.
pub(super) const WATCH_TV: u32 = 0x598890;
pub(super) const LISTEN_RADIO: u32 = 0x598B90;
pub(super) const GO_TO_TV: u32 = 0x599210;
pub(super) const GO_TO_RADIO: u32 = 0x599400;
pub(super) const NO_TOY: u32 = 0x40AD80;
pub(super) const GO_TO_HORSE: u32 = 0x5995F0;
pub(super) const GO_TO_BOX: u32 = 0x599770;
pub(super) const RIDE: u32 = 0x598F40;
pub(super) const DISMOUNT: u32 = 0x598E90;
pub(super) const PRESS: u32 = 0x56BBA0;

/// The reach helpers these chains queue (in `reach`).
const NOTICE: u32 = 0x56B480;
const WALK_TO_LINKED: u32 = 0x56BA80;
const WALK_TO: u32 = 0x56B560;
const FACE_LINKED: u32 = 0x56B6C0;
const FACE: u32 = 0x56B500;

/// Toy kinds (ALW `+4`).
const TV: u8 = 2;
const RADIO: u8 = 3;

/// Signals the TV and radio give while they play (`0x486880` kinds).
const TV_ON: u8 = 0xC9;
const RADIO_ON: u8 = 0xCA;

/// Toy skills by bit of `rec+0x164` (`0x12FE978`; 3, 8, 10 and 11 have none).
const TOY_SKILLS: [Option<u32>; 12] = [
    Some(0x55DDE0),
    Some(0x55E3E0),
    Some(0x55E210),
    None,
    Some(0x55E600),
    Some(0x55DD10),
    Some(0x55DEB0),
    Some(0x55E6D0),
    None,
    Some(0x55DB10),
    None,
    None,
];
const TOY_SKILL_BITS: usize = 0x164;
/// Hand nodes, as indices into `Chao::props` (`garden::PROP_NODES` 4, 11).
const NODE_4: usize = 0;
const NODE_11: usize = 1;

pub(super) fn run(
    chao: &mut Chao,
    address: u32,
    garden: &Garden,
    senses: &dyn Senses,
    rng: &mut Rng,
) -> Option<Next> {
    let _ = garden;
    Some(match address {
        WATCH_TV => watch(chao, TV_ON, senses, rng),
        LISTEN_RADIO => watch(chao, RADIO_ON, senses, rng),
        GO_TO_TV => go_to_player(chao, TV, WATCH_TV, senses, rng),
        GO_TO_RADIO => go_to_player(chao, RADIO, LISTEN_RADIO, senses, rng),
        NO_TOY => Next::Pop,
        GO_TO_HORSE => go_to_horse(chao, senses),
        GO_TO_BOX => go_to_box(chao, senses),
        RIDE => ride(chao, senses, rng),
        DISMOUNT => dismount(chao),
        PRESS => press(chao),
        0x55DDE0 => hold_a_while(chao, 0x14A, 0x12F_A174, 201, rng),
        0x55DD10 => hold_a_while(chao, 0x14E, 0x12F_BC5C, 201, rng),
        0x55DEB0 => hold_a_while(chao, 0x14B, 0x12F_6D54, 201, rng),
        0x55E6D0 => hold_a_while(chao, 0x14C, 0x12F_B41C, 301, rng),
        0x55E3E0 => drum(chao, rng),
        0x55E210 => book(chao, rng),
        0x55E600 => skill_4(chao, rng),
        0x55DB10 => bubbles(chao, rng),
        _ => return None,
    })
}

/// sin and cos of a BAMS angle (`0x42AAB0`, `0x42AC30`).
fn sin_cos(a: u16) -> (f32, f32) {
    let a = f32::from(a) * std::f32::consts::TAU / 65536.0;
    (a.sin(), a.cos())
}

/// `0x446960`: `cur` toward `target` by at most `max` BAMS.
fn approach(cur: u16, target: u16, max: u16) -> u16 {
    let d = i32::from(target.wrapping_sub(cur) as i16);
    let m = i32::from(max);
    if d > m || d < -m {
        if d < 0 {
            cur.wrapping_sub(max)
        } else {
            cur.wrapping_add(max)
        }
    } else {
        target
    }
}

/// `0x55C4A0` (TV) / `0x57C7A0` (radio): a spot 10..13 in front of the toy, up to 40° off
/// its facing, at its height; None without the toy (no roll then).
fn viewing_spot(kind: u8, senses: &dyn Senses, rng: &mut Rng) -> Option<V3> {
    let toy = senses.toy(kind)?;
    let dist = (f64::from(rng.r()) * 3.0 + 10.0) as f32;
    let a = ((f64::from(rng.r()) * 80.0 - 40.0) * -182.044_448_852_539_06) as i32;
    let angle = toy.rot_y.wrapping_sub(a as u16).wrapping_add(0x8000);
    let (s, c) = sin_cos(angle);
    Some([toy.pos[0] + s * dist, toy.pos[1], toy.pos[2] + c * dist])
}

/// Picker #11 `0x599A10` (TV, signal 0xC9) and #12 `0x5998C0` (radio, 0xCA): a toy already
/// playing draws a bored Chao to a spot in front of it. Best becomes 0.99 even when the toy
/// or its spot is missing and nothing is installed.
pub(in super::super) fn join_toy(
    chao: &mut Chao,
    best: &mut f32,
    signal: u8,
    senses: &dyn Senses,
    rng: &mut Rng,
) {
    let on = senses.signal(signal).is_some_and(|s| s.parent.is_some());
    if *best >= 1.0 || !on {
        return;
    }
    let mut t = u32::from(chao.rec.u16(off::TEDIOUS));
    let s = if t > 2000 {
        if t < 7000 {
            t += 3000;
        }
        jitter(f16(t as u16, 2000) * 0.99, rng)
    } else {
        0.0
    };
    if s <= *best {
        return;
    }
    let (kind, then) = if signal == TV_ON {
        (TV, WATCH_TV)
    } else {
        (RADIO, LISTEN_RADIO)
    };
    if let Some(spot) = viewing_spot(kind, senses, rng) {
        chao.body.target = spot;
        chao.link = Some(Link::Toy(kind));
        chao.set(Behaviour::StandUp, -1);
        for b in [NOTICE, WALK_TO, FACE_LINKED, then] {
            chao.push(Behaviour::Game(b));
        }
    }
    *best = 0.99;
}

/// `0x55E7A0`, from picker #14: one of its learned toy skills (bits of `rec+0x164`) at
/// random; false when it knows none.
pub(in super::super) fn toy_skill(chao: &mut Chao, rng: &mut Rng) -> bool {
    let bits = chao.rec.u16(TOY_SKILL_BITS);
    let known: Vec<u32> = TOY_SKILLS
        .iter()
        .enumerate()
        .filter(|&(i, _)| bits & (1 << i) != 0)
        .filter_map(|(_, s)| *s)
        .collect();
    if known.is_empty() {
        return false;
    }
    let n = known.len() as f64;
    let i = (f64::from(rng.r()) * (n - 9.999_999_747_378_752e-5)) as usize;
    chao.set(Behaviour::Game(known[i.min(known.len() - 1)]), -1);
    true
}

/// `0x598890` (TV, signal 0xC9) and `0x598B90` (radio, 0xCA): sit and watch in a pose
/// picked anew every 300..600 frames while the toy plays; when it stops, a 120..210-frame
/// motion 0x23D, then the toy cooldown (`D+0x1F4` = 1800..3600) and done.
fn watch(chao: &mut Chao, signal: u8, senses: &dyn Senses, rng: &mut Rng) -> Next {
    match chao.mode {
        0 | 1 => {
            if chao.mode == 0 {
                let r = rng.r();
                chao.play(if r < 0.3 {
                    0x3E
                } else if r < 0.7 {
                    0x3F
                } else {
                    0x1CB
                });
                // With animal 9 absorbed, sometimes 0x3A/0x3C, unless already in one
                // (`D+0xB0`, the motion playing before this frame's link).
                if chao.rec.u32(0x118) & 0x200 != 0
                    && rng.r() < 0.3
                    && !matches!(chao.player.current, 0x3A | 0x3C)
                {
                    chao.play(if rng.r() < 0.5 { 0x3A } else { 0x3C });
                }
                chao.set_eyes(if rng.r() < 0.5 { 4 } else { 0 }, -1);
                chao.set_mouth(if rng.r() < 0.5 { 3 } else { 0 }, -1);
                chao.count_a = 300 + rng.below(301);
                chao.mode = 1;
            }
            let t = chao.count_a;
            chao.count_a -= 1;
            if t <= 0 {
                chao.mode = 0;
            }
            // `0x486880(obj, 1, kind)`: no hit, or one with no object, means it's off.
            if !senses.signal(signal).is_some_and(|s| s.parent.is_some()) {
                chao.play(0x23D);
                chao.count_a = 120 + rng.below(91);
                chao.mode = 2;
            }
            Next::Keep
        }
        2 => {
            let t = chao.count_a;
            chao.count_a -= 1;
            if t > 0 {
                return Next::Keep;
            }
            chao.timers[TIMER_TOY] = 1800 + rng.below(1801);
            Next::Pop
        }
        _ => Next::Keep,
    }
}

/// `0x599210` (TV, kind 2) and `0x599400` (radio, 3), from the toy chooser: with a toy in
/// sight and a spot in front of it, walk to the toy, switch it on (`0x56BBA0`), walk to
/// the spot, face the toy and watch. Done at once otherwise.
fn go_to_player(chao: &mut Chao, kind: u8, then: u32, senses: &dyn Senses, rng: &mut Rng) -> Next {
    // The nearest toy in sight (list 4) is its target (ALW `+0x44`).
    let Some((near, _)) = senses.nearest_toy() else {
        return Next::Pop;
    };
    let Some(spot) = viewing_spot(kind, senses, rng) else {
        return Next::Pop;
    };
    chao.link = Some(Link::Toy(near));
    chao.body.target = spot;
    chao.set(Behaviour::StandUp, -1);
    for b in [NOTICE, WALK_TO_LINKED, PRESS, WALK_TO, FACE_LINKED, then] {
        chao.push(Behaviour::Game(b));
    }
    Next::Keep
}

/// `0x599770` (box, kind 8): walk to it, press it, then stand about.
fn go_to_box(chao: &mut Chao, senses: &dyn Senses) -> Next {
    let Some((near, _)) = senses.nearest_toy() else {
        return Next::Pop;
    };
    chao.link = Some(Link::Toy(near));
    chao.set(Behaviour::StandUp, -1);
    for b in [NOTICE, WALK_TO_LINKED, PRESS] {
        chao.push(Behaviour::Game(b));
    }
    chao.push(Behaviour::Settle);
    Next::Keep
}

/// `0x5995F0` (horse, kind 7): unless another Chao has it, link with it (`0x530640`), walk
/// up, face it and ride.
fn go_to_horse(chao: &mut Chao, senses: &dyn Senses) -> Next {
    let Some((near, pos)) = senses.nearest_toy() else {
        return Next::Pop;
    };
    if senses.toy(near).is_some_and(|t| t.taken) {
        return Next::Pop;
    }
    chao.body.target = pos;
    chao.toy_link = Some(near);
    chao.toy_events.push(ToyEvent::Link(near));
    chao.set(Behaviour::StandUp, -1);
    for b in [NOTICE, WALK_TO, FACE, RIDE] {
        chao.push(Behaviour::Game(b));
    }
    Next::Keep
}

/// `0x56BBA0`: reach out (motion 0xBE) for 61 frames, slowing down, then signal 0xAF a step
/// ahead (3 forward, 1 up; radius 3.5, 10 frames): a toy there that is off switches on.
fn press(chao: &mut Chao) -> Next {
    if chao.mode == 0 {
        chao.play(0xBE);
        chao.mode = 1;
        chao.count_a = 0;
    }
    if chao.mode == 1 {
        let t = chao.count_a;
        chao.count_a += 1;
        if t > 60 {
            let (s, c) = chao.body.facing();
            let p = chao.body.pos;
            chao.toy_events.push(ToyEvent::Press {
                toy: chao.link.and_then(Link::toy),
                pos: [p[0] + s * 3.0, p[1] + 1.0, p[2] + c * 3.0],
                radius: 3.5,
                frames: 10,
            });
            return Next::Pop;
        }
    }
    if chao.body.on_ground() {
        for v in &mut chao.body.vel {
            *v *= 0.9;
        }
    }
    Next::Keep
}

/// `0x598F40`: hop on (mode 0), land in the saddle with physics off (1), signal 0xAF to set
/// it rocking (2), ride 1800 frames (3), then the toy cooldown and off (`0x598E90`; the
/// game returns 1 after setting it, so the pop leaves it with nothing and it settles).
/// While the link holds the Chao follows the horse; a broken link means settle.
fn ride(chao: &mut Chao, senses: &dyn Senses, rng: &mut Rng) -> Next {
    match chao.mode {
        0 => {
            chao.play(0x84);
            chao.set_eyes(if rng.r() < 0.5 { 4 } else { 0 }, -1);
            chao.set_mouth(if rng.r() < 0.5 { 3 } else { 0 }, -1);
            let (s, c) = chao.body.facing();
            chao.body.vel = [s * 0.08, 0.65, c * 0.08];
            chao.mode = 1;
            ride_land(chao);
        }
        1 => ride_land(chao),
        2 => {
            chao.count_a -= 1;
            if chao.count_a <= 0 {
                chao.play_now(0x159);
                chao.toy_events.push(ToyEvent::Press {
                    toy: chao.toy_link,
                    pos: chao.body.pos,
                    radius: 3.5,
                    frames: 10,
                });
                chao.count_a = 1800;
                chao.mode = 3;
            }
        }
        3 => {
            chao.count_a -= 1;
            if chao.count_a <= 0 {
                chao.timers[TIMER_TOY] = 1800 + rng.below(1801);
                chao.moves = true;
                chao.set(Behaviour::Game(DISMOUNT), -1);
                return Next::Pop;
            }
        }
        _ => {}
    }
    // The link must still be mutual (ALW `+0x40` both ways).
    let Some(horse) = chao
        .toy_link
        .and_then(|k| senses.toy(k))
        .filter(|t| !t.taken)
    else {
        chao.set(Behaviour::Settle, -1);
        return Next::Pop;
    };
    if matches!(chao.mode, 2 | 3) {
        for (p, h) in chao.body.pos.iter_mut().zip(horse.pos) {
            *p += (h - *p) * 0.1;
        }
    }
    if matches!(chao.mode, 1..=3) {
        chao.body.rot_y = approach(chao.body.rot_y, horse.rot_y, 0x400);
    }
    Next::Keep
}

/// `0x598F40` mode 1: falling into the saddle, blend 30 into 0x158, physics off, 29 frames.
fn ride_land(chao: &mut Chao) {
    if chao.body.vel[1] < 0.0 {
        chao.play_blend(0x158, 30);
        chao.moves = false;
        chao.count_a = 29;
        chao.mode = 2;
    }
}

/// `0x598E90`: hop off sideways (0.3 at facing + 90°, 0.8 up), redone every frame until it
/// is on the ground.
fn dismount(chao: &mut Chao) -> Next {
    if chao.mode == 0 {
        chao.play_now(0x84);
        let (s, c) = sin_cos(chao.body.rot_y.wrapping_add(0x4000));
        chao.body.vel = [s * 0.3, 0.8, c * 0.3];
    }
    if chao.body.on_ground() {
        Next::Pop
    } else {
        Next::Keep
    }
}

/// Toy skills 0 `0x55DDE0`, 5 `0x55DD10`, 6 `0x55DEB0` and 7 `0x55E6D0`: a toy in the right
/// hand (node 11), eyes 4, for 600 + 0..`spread`−1 frames.
fn hold_a_while(chao: &mut Chao, motion: u16, model: u32, spread: u32, rng: &mut Rng) -> Next {
    match chao.mode {
        0 | 1 => {
            if chao.mode == 0 {
                chao.play(motion);
                chao.set_eyes(4, -1);
                chao.props[NODE_11] = model;
                chao.count_a = 600 + rng.below(spread);
                chao.mode = 1;
            }
            let t = chao.count_a;
            chao.count_a -= 1;
            if t > 0 { Next::Keep } else { Next::Pop }
        }
        _ => Next::Keep,
    }
}

/// Toy skill 1 `0x55E3E0`, the drum: 300..400 frames of 0x14F, then 20% done, else a
/// flourish (0x150) and 380..560 more.
fn drum(chao: &mut Chao, rng: &mut Rng) -> Next {
    match chao.mode {
        0 | 1 => {
            if chao.mode == 0 {
                chao.play(0x14F);
                if rng.r() < 0.5 {
                    chao.set_eyes(4, -1);
                }
                chao.drum = true;
                chao.count_a = 300 + rng.below(101);
                chao.mode = 1;
            }
            let t = chao.count_a;
            chao.count_a -= 1;
            if t > 0 {
                return Next::Keep;
            }
            if rng.r() < 0.2 {
                return Next::Pop;
            }
            if rng.r() < 0.5 {
                chao.set_eyes(4, -1);
            }
            chao.play(0x150);
            chao.mode = 2;
        }
        2 if chao.motion_held() => {
            chao.play(0x14F);
            if rng.r() < 0.5 {
                chao.set_eyes(4, -1);
            }
            chao.count_a = 380 + rng.below(181);
            chao.mode = 1;
        }
        _ => {}
    }
    Next::Keep
}

/// Toy skill 2 `0x55E210`, the picture book (`0x55E180`): read 300..400 frames, then 30%
/// close it (book mode 4) and done, else turn a page (mode 2, motion 0x153) and read
/// 120..300 more.
fn book(chao: &mut Chao, rng: &mut Rng) -> Next {
    match chao.mode {
        0 | 1 => {
            if chao.mode == 0 {
                chao.play(0x151);
                if rng.r() < 0.5 {
                    chao.set_eyes(4, -1);
                }
                chao.count_a = 300 + rng.below(101);
                chao.book = Some(0);
                chao.mode = 1;
            }
            let t = chao.count_a;
            chao.count_a -= 1;
            if t > 0 {
                return Next::Keep;
            }
            if rng.r() < 0.3 {
                chao.book = Some(4);
                return Next::Pop;
            }
            chao.book = Some(2);
            chao.play(0x153);
            chao.mode = 2;
        }
        2 if chao.motion_held() => {
            chao.play(0x152);
            chao.count_a = 120 + rng.below(181);
            chao.mode = 1;
        }
        _ => {}
    }
    Next::Keep
}

/// Toy skill 4 `0x55E600`: a toy in the left hand (node 4); at each motion's end 20% done,
/// else 0x148 (60%) or 0x149 again.
fn skill_4(chao: &mut Chao, rng: &mut Rng) -> Next {
    if chao.mode == 0 {
        chao.play(0x148);
        chao.set_eyes(4, -1);
        chao.props[NODE_4] = 0x12F_8364;
        chao.mode = 1;
    }
    if chao.mode == 1 && chao.motion_held() {
        if rng.r() < 0.2 {
            return Next::Pop;
        }
        chao.play(if rng.r() < 0.6 { 0x148 } else { 0x149 });
    }
    Next::Keep
}

/// Toy skill 9 `0x55DB10`, bubbles: the wand and the bottle (nodes 4 and 11); after 60
/// frames, every 10th frame 40% blows a bubble (`0x55DA10` at the hand; the server only
/// makes its rolls, the bubble itself is the clients' to draw); at each motion's end 25%
/// done, else again from the start.
fn bubbles(chao: &mut Chao, rng: &mut Rng) -> Next {
    if chao.mode == 0 {
        chao.play(0x146);
        chao.set_eyes(4, -1);
        chao.props[NODE_4] = 0x12F_E33C;
        chao.props[NODE_11] = 0x12F_E574;
        chao.mode = 1;
    }
    if chao.mode != 1 {
        return Next::Keep;
    }
    let t = chao.count_a;
    chao.count_a += 1;
    if t > 60 && chao.count_a % 10 == 0 && rng.r() < 0.4 {
        // Speed, then the x/y and z jitters.
        let _ = (rng.r(), rng.r(), rng.r());
    }
    if chao.motion_held() {
        if rng.r() < 0.25 {
            return Next::Pop;
        }
        chao.play(0x146);
        chao.count_a = 0;
    }
    Next::Keep
}

#[cfg(test)]
mod tests {
    use super::super::super::land::{GRID_CELLS, Tri};
    use super::super::super::picker::{Alone, Signal, Toy};
    use super::super::super::record::Record;
    use super::*;

    /// A floor at y = 0.
    fn garden() -> Garden {
        let (a, b) = (-100.0, 100.0);
        Garden {
            tris: vec![
                Tri {
                    v: [[a, 0.0, a], [b, 0.0, a], [a, 0.0, b]],
                    flags: 1,
                },
                Tri {
                    v: [[b, 0.0, a], [b, 0.0, b], [a, 0.0, b]],
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

    /// A garden with one toy of `kind` at `pos`, playing (`on`) or not.
    struct World {
        kind: u8,
        pos: V3,
        on: bool,
        taken: bool,
    }

    impl Senses for World {
        fn nearest_toy(&self) -> Option<(u8, V3)> {
            Some((self.kind, self.pos))
        }
        fn toy(&self, kind: u8) -> Option<Toy> {
            (kind == self.kind).then_some(Toy {
                pos: self.pos,
                rot_y: 0x4000,
                taken: self.taken,
            })
        }
        fn signal(&self, kind: u8) -> Option<Signal> {
            let playing =
                (self.kind == TV && kind == TV_ON) || (self.kind == RADIO && kind == RADIO_ON);
            (self.on && playing).then_some(Signal {
                pos: self.pos,
                parent: Some((self.pos, 0)),
            })
        }
    }

    fn chao(address: u32) -> Chao {
        let mut chao = Chao::new([0.0; 3], 0, Record::default());
        for _ in 0..30 {
            chao.frame(&garden(), 0, &Alone, &mut Rng::new(1));
        }
        chao.set(Behaviour::Game(address), -1);
        chao
    }

    fn queue(chao: &Chao) -> Vec<Behaviour> {
        chao.stack[chao.index..].iter().flatten().copied().collect()
    }

    #[test]
    fn the_spot_is_in_front_of_the_toy() {
        let w = World {
            kind: TV,
            pos: [20.0, 0.0, 0.0],
            on: false,
            taken: false,
        };
        let mut rng = Rng::new(9);
        for _ in 0..50 {
            let s = viewing_spot(TV, &w, &mut rng).unwrap();
            let (dx, dz) = (s[0] - 20.0, s[2]);
            let d = (dx * dx + dz * dz).sqrt();
            assert!((10.0..=13.0).contains(&d), "{d}");
            // Facing +x (0x4000) plus 0x8000 puts the spot on the -x side, ±40°.
            assert!(dx < 0.0 && (dz / -dx).abs() <= 0.84 + 1e-3);
        }
        assert!(viewing_spot(RADIO, &w, &mut rng).is_none());
    }

    #[test]
    fn going_to_the_tv_queues_the_walk_the_switch_and_the_watch() {
        let w = World {
            kind: TV,
            pos: [20.0, 0.0, 0.0],
            on: false,
            taken: false,
        };
        let mut c = chao(GO_TO_TV);
        c.frame(&garden(), 0, &w, &mut Rng::new(2));
        assert_eq!(c.link, Some(Link::Toy(TV)));
        assert_eq!(
            queue(&c),
            [
                Behaviour::StandUp,
                Behaviour::Game(NOTICE),
                Behaviour::Game(WALK_TO_LINKED),
                Behaviour::Game(PRESS),
                Behaviour::Game(WALK_TO),
                Behaviour::Game(FACE_LINKED),
                Behaviour::Game(WATCH_TV),
            ]
        );
        assert!(c.body.target[0] < 20.0);
        // No toy in sight: done at once.
        let mut c = chao(GO_TO_RADIO);
        c.frame(&garden(), 0, &Alone, &mut Rng::new(2));
        assert_eq!(c.current(), Some(Behaviour::Settle));
    }

    #[test]
    fn the_box_and_the_ball() {
        let w = World {
            kind: 8,
            pos: [5.0, 0.0, 5.0],
            on: false,
            taken: false,
        };
        let mut c = chao(GO_TO_BOX);
        c.frame(&garden(), 0, &w, &mut Rng::new(2));
        assert_eq!(
            queue(&c),
            [
                Behaviour::StandUp,
                Behaviour::Game(NOTICE),
                Behaviour::Game(WALK_TO_LINKED),
                Behaviour::Game(PRESS),
                Behaviour::Settle,
            ]
        );
        let mut c = chao(NO_TOY);
        c.frame(&garden(), 0, &w, &mut Rng::new(2));
        assert_eq!(c.current(), Some(Behaviour::Settle));
    }

    #[test]
    fn pressing_signals_0xaf_ahead_after_61_frames() {
        let mut c = chao(PRESS);
        c.link = Some(Link::Toy(TV));
        let mut rng = Rng::new(3);
        let mut frames = 0;
        while c.current() == Some(Behaviour::Game(PRESS)) {
            assert!(c.toy_events.is_empty());
            c.frame(&garden(), 0, &Alone, &mut rng);
            frames += 1;
        }
        assert_eq!(frames, 62);
        let [
            ToyEvent::Press {
                toy,
                pos,
                radius,
                frames,
            },
        ] = c.toy_events[..]
        else {
            panic!("{:?}", c.toy_events);
        };
        assert_eq!((toy, radius, frames), (Some(TV), 3.5, 10));
        // Facing +z: 3 ahead, 1 up.
        assert!(
            (pos[2] - c.body.pos[2] - 3.0).abs() < 0.2
                && (pos[1] - c.body.pos[1] - 1.0).abs() < 0.01
        );
    }

    #[test]
    fn watching_lasts_while_the_tv_plays_then_cools_down() {
        let mut w = World {
            kind: TV,
            pos: [0.0, 0.0, 20.0],
            on: true,
            taken: false,
        };
        let mut c = chao(WATCH_TV);
        let mut rng = Rng::new(4);
        let mut poses = std::collections::BTreeSet::new();
        for _ in 0..3000 {
            c.frame(&garden(), 0, &w, &mut rng);
            poses.insert(c.player.current);
            assert_eq!(c.current(), Some(Behaviour::Game(WATCH_TV)));
        }
        let picked = [0x3E, 0x3F, 0x1CB]
            .iter()
            .filter(|m| poses.contains(m))
            .count();
        assert!(picked >= 2, "{poses:?}");
        w.on = false;
        c.frame(&garden(), 0, &w, &mut rng);
        assert_eq!((c.mode, c.motion), (2, 0x23D));
        let left = c.count_a;
        assert!((119..=210).contains(&left));
        let mut frames = 1;
        while c.current() == Some(Behaviour::Game(WATCH_TV)) {
            c.frame(&garden(), 0, &w, &mut rng);
            frames += 1;
        }
        assert_eq!(frames, left + 2);
        assert!((1799..=3600).contains(&c.timers[TIMER_TOY]));
    }

    #[test]
    fn the_radio_listens_for_its_own_signal() {
        let w = World {
            kind: TV,
            pos: [0.0, 0.0, 20.0],
            on: true,
            taken: false,
        };
        let mut c = chao(LISTEN_RADIO);
        c.frame(&garden(), 0, &w, &mut Rng::new(4));
        assert_eq!(c.mode, 2);
    }

    #[test]
    fn a_bored_chao_joins_a_playing_tv() {
        let w = World {
            kind: TV,
            pos: [0.0, 0.0, 20.0],
            on: true,
            taken: false,
        };
        let mut c = chao(WATCH_TV);
        c.rec.set_u16(off::TEDIOUS, 9000);
        let mut best = 0.2;
        join_toy(&mut c, &mut best, TV_ON, &w, &mut Rng::new(5));
        assert_eq!(best, 0.99);
        assert_eq!(
            queue(&c),
            [
                Behaviour::StandUp,
                Behaviour::Game(NOTICE),
                Behaviour::Game(WALK_TO),
                Behaviour::Game(FACE_LINKED),
                Behaviour::Game(WATCH_TV),
            ]
        );
        // Not bored enough: nothing.
        let mut c = chao(WATCH_TV);
        c.rec.set_u16(off::TEDIOUS, 2000);
        let mut best = 0.2;
        join_toy(&mut c, &mut best, TV_ON, &w, &mut Rng::new(5));
        assert_eq!(best, 0.2);
        // Bored, but the TV isn't there: best still 0.99, nothing installed.
        let off_screen = World { kind: RADIO, ..w };
        let mut c = chao(WATCH_TV);
        c.rec.set_u16(off::TEDIOUS, 9000);
        let mut best = 0.2;
        let on_tv = Signalled(&off_screen);
        join_toy(&mut c, &mut best, TV_ON, &on_tv, &mut Rng::new(5));
        assert_eq!((best, c.current()), (0.99, Some(Behaviour::Game(WATCH_TV))));
    }

    /// The TV's signal with no TV object to find.
    struct Signalled<'a>(&'a World);

    impl Senses for Signalled<'_> {
        fn signal(&self, _kind: u8) -> Option<Signal> {
            Some(Signal {
                pos: self.0.pos,
                parent: Some((self.0.pos, 0)),
            })
        }
    }

    #[test]
    fn the_horse_ride() {
        let horse = [0.0, 0.0, 3.0];
        let w = World {
            kind: 7,
            pos: horse,
            on: false,
            taken: false,
        };
        let mut c = chao(GO_TO_HORSE);
        c.frame(&garden(), 0, &w, &mut Rng::new(6));
        assert_eq!(c.toy_link, Some(7));
        assert_eq!(c.toy_events, [ToyEvent::Link(7)]);
        assert_eq!(
            queue(&c),
            [
                Behaviour::StandUp,
                Behaviour::Game(NOTICE),
                Behaviour::Game(WALK_TO),
                Behaviour::Game(FACE),
                Behaviour::Game(RIDE),
            ]
        );
        // Someone else's horse: done.
        let mut c = chao(GO_TO_HORSE);
        c.frame(&garden(), 0, &World { taken: true, ..w }, &mut Rng::new(6));
        assert_eq!((c.current(), c.toy_link), (Some(Behaviour::Settle), None));

        let mut c = chao(RIDE);
        c.toy_link = Some(7);
        let mut rng = Rng::new(7);
        c.frame(&garden(), 0, &w, &mut rng);
        assert_eq!(c.mode, 1);
        assert!(c.body.vel[1] > 0.5);
        let mut frames = 1;
        while c.mode == 1 {
            c.toy_events.clear();
            c.frame(&garden(), 0, &w, &mut rng);
            frames += 1;
        }
        assert!(!c.moves && frames < 30);
        while c.mode == 2 {
            c.toy_events.clear();
            c.frame(&garden(), 0, &w, &mut rng);
        }
        assert!(matches!(
            c.toy_events[..],
            [ToyEvent::Press {
                toy: Some(7),
                radius: 3.5,
                frames: 10,
                ..
            }]
        ));
        for _ in 0..1799 {
            c.frame(&garden(), 0, &w, &mut rng);
        }
        // It sits on the horse, facing as it does.
        assert!((c.body.pos[2] - 3.0).abs() < 0.01 && c.body.rot_y == 0x4000);
        assert_eq!(c.current(), Some(Behaviour::Game(RIDE)));
        c.frame(&garden(), 0, &w, &mut rng);
        // Off the horse (the game's pop leaves nothing, so it settles), cooldown set.
        assert_eq!(c.current(), Some(Behaviour::Settle));
        assert!(c.moves && c.timers[TIMER_TOY] >= 1799);
    }

    #[test]
    fn a_broken_link_ends_the_ride() {
        let w = World {
            kind: 7,
            pos: [0.0, 0.0, 3.0],
            on: false,
            taken: true,
        };
        let mut c = chao(RIDE);
        c.toy_link = Some(7);
        c.frame(&garden(), 0, &w, &mut Rng::new(7));
        assert_eq!(c.current(), Some(Behaviour::Settle));
    }

    #[test]
    fn dismounting_hops_sideways_until_landed() {
        // On the ground from the last physics step: the hop is set and it's done at once.
        let mut c = chao(DISMOUNT);
        let mut rng = Rng::new(8);
        c.frame(&garden(), 0, &Alone, &mut rng);
        assert_eq!(c.current(), Some(Behaviour::Settle));
        // Facing +z: the hop goes +x.
        assert!(c.body.pos[0] > 0.2 && c.body.pos[1] > 0.5);
        // In the air the hop is set again every frame, so it never comes down.
        let mut c = chao(DISMOUNT);
        c.body.flags = 0;
        for _ in 0..100 {
            c.frame(&garden(), 0, &Alone, &mut rng);
        }
        assert_eq!(c.current(), Some(Behaviour::Game(DISMOUNT)));
        assert!(c.body.pos[1] > 50.0);
    }

    #[test]
    fn the_toy_skill_chooser_picks_a_known_skill() {
        let mut c = chao(WATCH_TV);
        assert!(!toy_skill(&mut c, &mut Rng::new(1)));
        // Bits 3, 8, 10, 11 have no skill.
        c.rec.set_u16(TOY_SKILL_BITS, 0xD08);
        assert!(!toy_skill(&mut c, &mut Rng::new(1)));
        c.rec.set_u16(TOY_SKILL_BITS, 0x202);
        let mut seen = std::collections::BTreeSet::new();
        let mut rng = Rng::new(1);
        for _ in 0..50 {
            assert!(toy_skill(&mut c, &mut rng));
            seen.extend(c.current().map(Behaviour::address));
        }
        assert_eq!(seen.into_iter().collect::<Vec<_>>(), [0x55DB10, 0x55E3E0]);
    }

    /// Runs `address` to its end; the frames it took (or `limit`).
    fn run_out(c: &mut Chao, address: u32, limit: u32, rng: &mut Rng) -> u32 {
        for f in 1..=limit {
            c.frame(&garden(), 0, &Alone, rng);
            if c.current() != Some(Behaviour::Game(address)) {
                return f;
            }
        }
        limit
    }

    #[test]
    fn held_toy_skills_last_their_time_and_put_the_toy_away() {
        for (address, motion, model, spread) in [
            (0x55DDE0, 0x14A, 0x12F_A174, 200),
            (0x55DD10, 0x14E, 0x12F_BC5C, 200),
            (0x55DEB0, 0x14B, 0x12F_6D54, 200),
            (0x55E6D0, 0x14C, 0x12F_B41C, 300),
        ] {
            let mut c = chao(address);
            let mut rng = Rng::new(address);
            c.frame(&garden(), 0, &Alone, &mut rng);
            assert_eq!((c.props, c.eyes), ([0, model, 0], 4));
            assert_eq!(c.motion, motion);
            let n = run_out(&mut c, address, 2000, &mut rng) + 1;
            assert!((601..=601 + spread).contains(&n), "{address:#x}: {n}");
            assert_eq!(c.props, [0; 3]);
        }
    }

    #[test]
    fn the_drum_book_skill_4_and_bubbles_end_in_time() {
        for address in [0x55E3E0, 0x55E210, 0x55E600, 0x55DB10] {
            for seed in 0..5 {
                let mut c = chao(address);
                let mut rng = Rng::new(seed);
                c.frame(&garden(), 0, &Alone, &mut rng);
                match address {
                    0x55E3E0 => assert!(c.drum),
                    0x55E210 => assert_eq!(c.book, Some(0)),
                    0x55E600 => assert_eq!(c.props, [0x12F_8364, 0, 0]),
                    _ => assert_eq!(c.props, [0x12F_E33C, 0x12F_E574, 0]),
                }
                let n = run_out(&mut c, address, 100_000, &mut rng);
                assert!(n < 100_000, "{address:#x} never ends");
                assert!(!c.drum && c.book.is_none() && c.props == [0; 3]);
            }
        }
    }

    #[test]
    fn the_book_turns_pages_until_it_closes() {
        let mut c = chao(0x55E210);
        let mut rng = Rng::new(11);
        let mut modes = Vec::new();
        for _ in 0..20_000 {
            c.frame(&garden(), 0, &Alone, &mut rng);
            if c.current() != Some(Behaviour::Game(0x55E210)) {
                break;
            }
            if modes.last() != Some(&c.book) {
                modes.push(c.book);
            }
        }
        assert_eq!(modes.first(), Some(&Some(0)));
        assert!(modes.iter().all(|m| matches!(m, Some(0 | 2))));
    }
}
