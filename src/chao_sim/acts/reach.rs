//! Getting to a thing: the short behaviours other chains queue to bring a Chao to a spot or
//! an object and face it (re/chao-ai.md §2A.1, §5.8), decoded from the game's bytes. "The
//! linked object" is the ALW target (`entry+0x44`, `0x5304F0`), `Chao::link`. Pressing a
//! toy's switch (`0x56BBA0`) is in `toys`.

use super::super::brain::{Behaviour, Chao, Link, Next, Rng};
use super::super::land::{Garden, V3};
use super::super::picker::Senses;
use super::super::record::off;

/// The behaviours here, by game address.
pub(super) const NOTICE: u32 = 0x56B480;
pub(super) const FACE_TARGET: u32 = 0x56B500;
pub(super) const WALK_TO_TARGET: u32 = 0x56B560;
pub(super) const FACE_LINK: u32 = 0x56B6C0;
pub(super) const TURN_AROUND: u32 = 0x56B860;
pub(super) const WALK_TO_LINK: u32 = 0x56BA80;

pub(super) fn run(
    chao: &mut Chao,
    address: u32,
    garden: &Garden,
    senses: &dyn Senses,
    rng: &mut Rng,
) -> Option<Next> {
    let _ = garden;
    Some(match address {
        NOTICE => notice(chao),
        FACE_TARGET => face_target(chao),
        WALK_TO_TARGET => walk_to_target(chao),
        FACE_LINK => face_link(chao, senses),
        TURN_AROUND => turn_around(chao, rng),
        WALK_TO_LINK => walk_to_link(chao, senses),
        _ => return None,
    })
}

/// Turns toward `at` by at most `rate` BAMS and returns the angle left, as `0x796910` does
/// toward `W+0x54` (`0x530510`/`0x796AB0` are the same turn toward an object or a player).
pub(super) fn turn_to(chao: &mut Chao, at: V3, rate: u16) -> u16 {
    let target = chao.body.target;
    chao.body.target = at;
    let left = chao.body.turn_to_target(rate);
    chao.body.target = target;
    left
}

/// The linked object's position (`entry+0x44 → +0x3C`, its `D+0x14`). A fruit or toy that
/// has left the garden counts as no link.
pub(super) fn link_pos(chao: &Chao, senses: &dyn Senses) -> Option<V3> {
    match chao.link? {
        Link::At(pos) => Some(pos),
        Link::Fruit(id) => senses.fruit(id).map(|f| f.pos),
        Link::Toy(kind) => senses.toy(kind).map(|t| t.pos),
    }
}

/// `0x530510`: turn toward the linked object. With none the game reads an uninitialised
/// stack slot as the object; no turn is modelled.
pub(super) fn turn_to_link(chao: &mut Chao, senses: &dyn Senses, rate: u16) -> u16 {
    match link_pos(chao, senses) {
        Some(at) => turn_to(chao, at, rate),
        None => 0,
    }
}

/// `0x77FBD0`: 3-D distance.
pub(super) fn distance(a: V3, b: V3) -> f32 {
    let d = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
    (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt()
}

/// `0x530580`: distance to the linked object. With none it measures the Chao against itself
/// (its saved ECX), so 0.0; the −1.0 branch needs a null Chao.
pub(super) fn link_distance(chao: &Chao, senses: &dyn Senses) -> f32 {
    link_pos(chao, senses).map_or(0.0, |at| distance(chao.body.pos, at))
}

/// The walks' drive along the facing, on the ground or not (`0x1312C68` 0.055 ×
/// `0x905230` 0.8 = 0.044 into `W+0xC`/`W+0x14`).
fn drive_044(chao: &mut Chao) {
    let k = (f64::from(0.055f32) * f64::from(0.8f32)) as f32;
    let (s, c) = chao.body.facing();
    chao.body.accel[0] = s * k;
    chao.body.accel[2] = c * k;
}

/// The give-up timer `D+0x1C8` (`patience`): one less a frame; out → `AL_SetBehavior(0x565BE0)`.
fn wait_or_settle(chao: &mut Chao) -> Next {
    chao.patience -= 1;
    if chao.patience <= 0 {
        chao.set(Behaviour::Settle, -1);
    }
    Next::Keep
}

/// `0x56B480`: notice something. Emotion ball 1 for 180 frames, give up after 1800 frames;
/// done on the 122nd frame after the first.
fn notice(chao: &mut Chao) -> Next {
    match chao.mode {
        0 => {
            chao.patience = 1800;
            chao.emotion_ball(1, 180);
            chao.mode = 1;
            chao.count_a = 0;
            chao.count_b = 0;
        }
        1 => {
            let old = chao.count_a;
            chao.count_a += 1;
            if old > 120 {
                return Next::Pop;
            }
        }
        _ => {}
    }
    wait_or_settle(chao)
}

/// `0x56B500`: face `W+0x54`. Motion 0x64 (blend 15), `count_a` 60 (unused); turns 0x200 a
/// frame, the first included, done once less than 0xB6 (1°) is left.
fn face_target(chao: &mut Chao) -> Next {
    if chao.mode == 0 {
        chao.play_blend(0x64, 15);
        chao.mode = 1;
        chao.count_a = 60;
    }
    if chao.body.turn_to_target(0x200) < 0xB6 {
        Next::Pop
    } else {
        Next::Keep
    }
}

/// Mode 0 of the two walks to a thing: eyes 4 and the 1800-frame timer every frame; from
/// the 32nd frame, `walk` and on. True once walking.
fn start_walk(chao: &mut Chao, walk: u16) {
    chao.set_eyes(4, -1);
    let old = chao.count_a;
    chao.patience = 1800;
    chao.count_a += 1;
    if old > 30 {
        chao.play(walk);
        chao.mode = 1;
    }
}

/// After the walk's distance check: `< 0` resets, `< 4` arrives (`0x104DD30`).
fn arrived(chao: &mut Chao, d: f32) -> bool {
    if d < 0.0 {
        chao.set(Behaviour::Settle, -1);
        return false;
    }
    d < 4.0
}

/// `0x56B560`: walk to `W+0x54`. Slow walk 0x73 below 150 run points (scaled), else 0x64;
/// turn 0x180, drive 0.044; done within 4 (3-D, `0x7968A0`); 1800 frames → settle.
fn walk_to_target(chao: &mut Chao) -> Next {
    match chao.mode {
        0 => {
            let slow = chao.rec.scaled(off::RUN_POINTS) < 150;
            start_walk(chao, if slow { 0x73 } else { 0x64 });
        }
        1 => {
            chao.body.turn_to_target(0x180);
            drive_044(chao);
            let d = distance(chao.body.pos, chao.body.target);
            if arrived(chao, d) {
                return Next::Pop;
            }
        }
        _ => {}
    }
    chao.player.speed = 1.5;
    wait_or_settle(chao)
}

/// `0x56BA80`: walk to the linked object: as `0x56B560`, always motion 0x64, turning with
/// `0x530510` and measuring with `0x530580`. With no link that is 0, so it arrives on the
/// first walking frame.
fn walk_to_link(chao: &mut Chao, senses: &dyn Senses) -> Next {
    match chao.mode {
        0 => start_walk(chao, 0x64),
        1 => {
            turn_to_link(chao, senses, 0x180);
            drive_044(chao);
            let d = link_distance(chao, senses);
            if arrived(chao, d) {
                return Next::Pop;
            }
        }
        _ => {}
    }
    chao.player.speed = 1.5;
    wait_or_settle(chao)
}

/// `0x56B6C0`: face the linked object. Motion 0x64 (blend 15); 90 frames of `0x530510(0x200)`,
/// then done. Motion speed 1.6.
fn face_link(chao: &mut Chao, senses: &dyn Senses) -> Next {
    match chao.mode {
        0 => {
            chao.play_blend(0x64, 15);
            chao.count_a = 90;
            chao.mode = 1;
        }
        1 => {
            let old = chao.count_a;
            chao.count_a -= 1;
            if old <= 0 {
                return Next::Pop;
            }
            turn_to_link(chao, senses, 0x200);
        }
        _ => {}
    }
    chao.player.speed = 1.6;
    Next::Keep
}

/// `0x56B860`: turn about, ±0x16C BAMS a frame (50/50) for 90 frames: 180°. Motion 0x64
/// (blend 15), speed 1.6. The game has no caller for it.
fn turn_around(chao: &mut Chao, rng: &mut Rng) -> Next {
    match chao.mode {
        0 => {
            chao.play_blend(0x64, 15);
            chao.mode = if rng.r() < 0.5 { 1 } else { 2 };
            chao.count_a = 90;
        }
        1 | 2 => {
            let old = chao.count_a;
            chao.count_a -= 1;
            if old <= 0 {
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
    chao.player.speed = 1.6;
    Next::Keep
}

#[cfg(test)]
mod tests {
    use super::super::super::land::{GRID_CELLS, Tri};
    use super::super::super::picker::{Alone, Toy, Tree};
    use super::super::super::record::Record;
    use super::*;

    /// A floor at y = 0.
    pub(in super::super) fn garden() -> Garden {
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

    fn chao(address: u32) -> Chao {
        let mut rec = Record::default();
        rec.set_u8(off::TYPE, super::super::super::record::TYPE_CHILD);
        let mut c = Chao::new([0.0; 3], 0, rec);
        c.set(Behaviour::Game(address), -1);
        c
    }

    /// Frames until the Chao leaves `address` (that frame included).
    fn run_out(c: &mut Chao, address: u32, senses: &dyn Senses, rng: &mut Rng) -> u32 {
        let garden = garden();
        for n in 1..5000 {
            c.frame(&garden, 0, senses, rng);
            if c.current() != Some(Behaviour::Game(address)) {
                return n;
            }
        }
        panic!("still in {address:#X}");
    }

    #[test]
    fn notice_shows_the_ball_and_ends_on_frame_123() {
        let mut rng = Rng::new(1);
        let mut c = chao(NOTICE);
        c.frame(&garden(), 0, &Alone, &mut rng);
        assert_eq!(c.bubble.0, 1);
        assert_eq!(c.patience, 1799);
        assert_eq!(1 + run_out(&mut c, NOTICE, &Alone, &mut rng), 123);
    }

    #[test]
    fn facing_the_target_turns_0x200_a_frame() {
        let mut rng = Rng::new(1);
        let mut c = chao(FACE_TARGET);
        c.body.target = [10.0, 0.0, 0.0]; // +x: 0x4000, 32 turns of 0x200
        assert_eq!(run_out(&mut c, FACE_TARGET, &Alone, &mut rng), 32);
        assert_eq!(c.body.rot_y, 0x4000);
    }

    #[test]
    fn walking_to_the_target_arrives_within_4() {
        let mut rng = Rng::new(1);
        let mut c = chao(WALK_TO_TARGET);
        c.body.target = [0.0, 0.0, 30.0];
        let frames = run_out(&mut c, WALK_TO_TARGET, &Alone, &mut rng);
        assert!(frames > 32 && frames < 1800, "{frames}");
        assert!(distance(c.body.pos, [0.0, 0.0, 30.0]) < 4.5);
        assert_eq!(c.motion, 0x73);
    }

    #[test]
    fn a_walk_that_never_gets_there_gives_up_after_1800_frames() {
        let mut rng = Rng::new(1);
        let mut c = chao(WALK_TO_TARGET);
        // 3-D distance: a target high above can't be reached. The timer restarts on each
        // of the 32 frames before walking.
        c.body.target = [0.0, 50.0, 0.0];
        assert_eq!(run_out(&mut c, WALK_TO_TARGET, &Alone, &mut rng), 31 + 1800);
        assert_eq!(c.current(), Some(Behaviour::Settle));
    }

    #[test]
    fn walking_to_no_link_ends_when_it_starts_walking() {
        let mut rng = Rng::new(1);
        let mut c = chao(WALK_TO_LINK);
        assert_eq!(run_out(&mut c, WALK_TO_LINK, &Alone, &mut rng), 33);
    }

    #[test]
    fn walking_to_a_linked_spot_faces_and_reaches_it() {
        let mut rng = Rng::new(1);
        let mut c = chao(WALK_TO_LINK);
        c.link = Some(Link::At([-20.0, 0.0, 0.0]));
        c.push(Behaviour::Game(FACE_LINK));
        let frames = run_out(&mut c, WALK_TO_LINK, &Alone, &mut rng);
        assert!(frames > 32 && frames < 1800, "{frames}");
        assert!(distance(c.body.pos, [-20.0, 0.0, 0.0]) < 4.5);
        // Then 90 frames turning to it and one to finish.
        assert_eq!(run_out(&mut c, FACE_LINK, &Alone, &mut rng), 92);
        assert!(turn_to(&mut c, [-20.0, 0.0, 0.0], 0) < 0x800);
    }

    #[test]
    fn turning_around_is_half_a_turn() {
        let mut rng = Rng::new(5);
        let mut c = chao(TURN_AROUND);
        c.frame(&garden(), 0, &Alone, &mut rng);
        let dir = c.mode;
        assert_eq!(1 + run_out(&mut c, TURN_AROUND, &Alone, &mut rng), 92);
        let half = (0x16C * 90) as u16;
        assert_eq!(
            c.body.rot_y,
            if dir == 1 { half } else { half.wrapping_neg() }
        );
    }

    #[test]
    fn a_linked_toy_is_walked_to_where_it_is() {
        struct Telly;
        impl Senses for Telly {
            fn toy(&self, kind: u8) -> Option<Toy> {
                (kind == 2).then_some(Toy {
                    pos: [15.0, 0.0, 0.0],
                    rot_y: 0,
                    taken: false,
                })
            }
        }
        let mut rng = Rng::new(1);
        let mut c = chao(WALK_TO_LINK);
        c.link = Some(Link::Toy(2));
        let frames = run_out(&mut c, WALK_TO_LINK, &Telly, &mut rng);
        assert!(frames > 33, "{frames}");
        assert!(distance(c.body.pos, [15.0, 0.0, 0.0]) < 4.5);
    }

    #[test]
    fn going_to_bed_may_walk_to_a_grown_tree() {
        struct Grove(u8);
        impl Senses for Grove {
            fn nearest_tree(&self) -> Option<Tree> {
                Some(Tree {
                    pos: [10.0, 0.0, 10.0],
                    dist: 14.1,
                    state: self.0,
                })
            }
        }
        let mut went = false;
        let mut rng = Rng::new(3);
        for _ in 0..20 {
            let mut c = chao(0x54EE10);
            c.frame(&garden(), 0, &Grove(3), &mut rng);
            match c.current() {
                Some(Behaviour::Game(WALK_TO_LINK)) => {
                    went = true;
                    assert_eq!(c.link, Some(Link::At([10.0, 0.0, 10.0])));
                    assert_eq!(c.stack[1], Some(Behaviour::Game(0x56B960)));
                    assert_eq!(c.stack[2], Some(Behaviour::Game(0x54EF10)));
                }
                other => assert_eq!(other, Some(Behaviour::Game(0x54EF10))),
            }
            // A sapling (state 1) is never a bed.
            let mut c = chao(0x54EE10);
            c.frame(&garden(), 0, &Grove(1), &mut rng);
            assert_eq!(c.current(), Some(Behaviour::Game(0x54EF10)));
        }
        assert!(went);
    }
}
