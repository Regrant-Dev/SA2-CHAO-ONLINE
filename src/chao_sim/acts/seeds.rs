//! Seeds and watering: carrying a seed it picked up (`0x567640`, in `touch`) about until it
//! finds an empty tree spot, planting it, and watering a tree that wants it (re/chao-ai.md
//! "0xAE target, seeds and watering", §6.5). Decoded from the game's bytes. The trees are
//! the server's (`TreeEvent`); the hand flags (`D+0xA0` 0x400/0x800), the collision bits
//! (`0x54A690`) and the sound volumes are the clients'. The shovel and the watering can are
//! parameter blocks set at run time (`0x12F5954`, `0x12F5974`), which `garden::PROP_NODES`
//! doesn't carry: `props` names their models.

use super::super::brain::{Behaviour, Chao, Link, Next, Rng, TreeEvent, random_cell_point};
use super::super::land::Garden;
use super::super::picker::Senses;
use super::super::record::off;
use super::reach::{distance, link_distance, turn_to_link};

/// The behaviours here, by game address.
pub(super) const HOLD: u32 = 0x567710;
pub(super) const CARRY: u32 = 0x567830;
pub(super) const PLANT: u32 = 0x567B10;
pub(super) const GO_WATER: u32 = 0x567FA0;
pub(super) const WATER: u32 = 0x5680F0;
pub(super) const POUR: u32 = 0x568460;

/// Toy skill bits (`rec+0x164`, `0x53E360`): 10 planting, 11 the watering can.
const SKILLS: usize = 0x164;
const PLANTING: u16 = 1 << 10;
const WATERING: u16 = 1 << 11;
/// Intelligence points (`rec+0x44`).
const INTELLIGENCE: usize = 0x44;
/// The seed's touch kind (`0x8A7458`).
const SEED: u8 = 0x85;
/// Hand node 11, as an index into `Chao::props`.
const NODE_11: usize = 1;
/// The shovel (`0x12FDE44`, block `0x12F5954`) and the watering can (`0x12FCA1C`, block
/// `0x12F5974`), attached by `0x566A20`.
const SHOVEL: u32 = 0x12FDE44;
const WATERING_CAN: u32 = 0x12FCA1C;

pub(super) fn run(
    chao: &mut Chao,
    address: u32,
    garden: &Garden,
    senses: &dyn Senses,
    rng: &mut Rng,
) -> Option<Next> {
    Some(match address {
        HOLD => hold(chao, senses, rng),
        CARRY => carry(chao, garden, senses, rng),
        PLANT => plant(chao, senses, rng),
        GO_WATER => go_water(chao, senses, rng),
        WATER => water(chao, senses, rng),
        POUR => pour(chao),
        _ => return None,
    })
}

/// Still holding the seed (`0x56CFF0`): these end the frame it's gone.
fn holding(senses: &dyn Senses) -> Next {
    if senses.carrying().is_some() {
        Next::Keep
    } else {
        Next::Pop
    }
}

/// The walks' tail (`0x567AA4`, `0x56808D`): on the ground, drive 0.055·0.8 along the facing
/// into `W+0xC`/`W+0x14`; motion speed `D+0xB8 = |v|·18` (`0x9063C8`).
fn drive_on(chao: &mut Chao) {
    chao.body
        .drive((f64::from(0.055f32) * f64::from(0.8f32)) as f32);
    let v = chao.body.vel;
    chao.player.speed = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt() * 18.0;
}

/// Counts `count_a` down; true when its old value was ≤ 0.
fn spent(chao: &mut Chao) -> bool {
    let old = chao.count_a;
    chao.count_a -= 1;
    old <= 0
}

/// Counts `count_a` down; true when its new value is ≤ 0 (`0x5680F0`'s `dec`, `cmp 0`).
fn run_down(chao: &mut Chao) -> bool {
    chao.count_a -= 1;
    chao.count_a <= 0
}

/// `0x567710`: hold the seed up. Motion 3 (blend 15), eyes 0, emotion ball 3 for 300
/// frames, sound 0x601C; 300..420 frames, then carry it about (`0x567830`).
fn hold(chao: &mut Chao, senses: &dyn Senses, rng: &mut Rng) -> Next {
    if chao.mode == 0 {
        chao.play_blend(3, 15);
        chao.set_eyes(0, -1);
        chao.emotion_ball(3, 300);
        chao.sound(0x601C);
        chao.count_a = 300 + rng.below(121);
        chao.mode = 1;
    }
    chao.count_a -= 1;
    if chao.count_a <= 0 {
        chao.set(Behaviour::Game(CARRY), -1);
    }
    holding(senses)
}

/// `0x567830`: carry the seed about. Mode 0: motion 0xBF (blend 15), eyes 0, 1800..2200
/// frames (`D+0x1C8`), a land point to head for (`0x534F80` on list 1). Mode 1: an empty
/// spot it touches (`plant_spot`) closer than 20 + intelligence·0.001 (`rec+0x44`, ×3 as 32
/// bits) becomes the target, sound 0x601A; otherwise it turns 0x100 to the point and, there
/// (XZ squared < 64), r<0.7 holds the seed up again (`0x567710`). Mode 2: turn 0x280 to the
/// spot; within 2.5 (`0x165E030`) plant (`0x567B10`). Time up: done, seed in hand.
fn carry(chao: &mut Chao, garden: &Garden, senses: &dyn Senses, rng: &mut Rng) -> Next {
    match chao.mode {
        0 => {
            chao.play_blend(0xBF, 15);
            chao.set_eyes(0, -1);
            chao.patience = 1800 + rng.below(401);
            chao.mode = 1;
            if let Some((x, z)) = random_cell_point(garden, 1, rng) {
                chao.body.target[0] = x;
                chao.body.target[2] = z;
            }
        }
        1 => {
            let intelligence = u32::from(chao.rec.u16(INTELLIGENCE)) * 3;
            let reach = f64::from(intelligence) * 0.001_000_000_047_497_451_3 + 20.0;
            match senses.plant_spot() {
                Some(spot) if f64::from(distance(chao.body.pos, spot)) < reach => {
                    chao.link = Some(Link::At(spot));
                    chao.mode = 2;
                    chao.sound(0x601A);
                }
                _ => {
                    chao.body.turn_to_target(0x100);
                    if chao.body.target_distance2() < 64.0 && rng.r() < 0.7 {
                        chao.set(Behaviour::Game(HOLD), -1);
                    }
                }
            }
        }
        2 => {
            turn_to_link(chao, senses, 0x280);
            if link_distance(chao, senses) < 2.5 {
                chao.set(Behaviour::Game(PLANT), -1);
            }
        }
        _ => {}
    }
    chao.patience -= 1;
    if chao.patience <= 0 {
        return Next::Pop;
    }
    drive_on(chao);
    holding(senses)
}

/// `0x567B10`: plant the seed (jump table `0x567F84`). Mode 0: motion 0 (blend 15), eyes 4,
/// emotion ball 1 for 180 frames, 120..240 frames turning 0x100 to the spot. Then without
/// toy skill 10 it gives up (ball 5, done); with it, the shovel and motion 0xE5 for
/// 180..300 frames, then 0xE7: digging, sound 0x1023 every 35 frames until the motion ends.
/// The seed goes in (`TreeEvent::Plant`) and the spot becomes the target; motion 0xE8 for
/// 180..360 frames (sound 0x1024 every 35, 50% 0x6085 every 60), then 0xEA; at its hold,
/// water it (`0x5680F0`). Never ends for a lost seed.
fn plant(chao: &mut Chao, senses: &dyn Senses, rng: &mut Rng) -> Next {
    match chao.mode {
        0 => {
            chao.play_blend(0, 15);
            chao.set_eyes(4, -1);
            chao.emotion_ball(1, 180);
            chao.count_a = 120 + rng.below(121);
            chao.mode = 1;
        }
        1 => {
            turn_to_link(chao, senses, 0x100);
            if !spent(chao) {
                return Next::Keep;
            }
            if chao.rec.u16(SKILLS) & PLANTING == 0 {
                chao.emotion_ball(5, 180);
                return Next::Pop;
            }
            chao.play_blend(0xE5, 15);
            chao.props[NODE_11] = SHOVEL;
            chao.mode = 2;
            chao.count_a = 180 + rng.below(121);
        }
        2 => {
            if spent(chao) {
                chao.play_blend(0xE7, 15);
                chao.mode = 3;
                chao.count_a = 0;
                chao.count_b = 0;
            }
        }
        3 => {
            let old = chao.count_a;
            chao.count_a += 1;
            if old % 35 == 0 {
                chao.sound(0x1023);
            }
            if chao.motion_done() {
                // The seed it holds (`0x5306F0(obj, 7)`) goes either way; the spot it
                // touches (`0x54B1F0(1, 0xAC)`) gets it, and the Chao unlinks from the
                // seed and targets the spot (`0x530690`, `0x5304F0`).
                if let Some(seed) = senses.carrying().filter(|c| c.touch == SEED) {
                    let spot = senses.plant_spot();
                    if let Some(spot) = spot {
                        chao.link = Some(Link::At(spot));
                    }
                    chao.tree_events.push(TreeEvent::Plant {
                        spot_pos: spot,
                        seed_kind: seed.kind,
                    });
                }
                chao.play_blend(0xE8, 15);
                chao.count_a = 180 + rng.below(181);
                chao.mode = 4;
            }
        }
        4 => {
            if chao.count_a % 35 == 0 {
                chao.sound(0x1024);
            }
            if chao.count_a % 60 == 0 && rng.r() < 0.5 {
                chao.sound(0x6085);
            }
            if spent(chao) {
                chao.play_blend(0xEA, 15);
                chao.mode = 5;
            }
        }
        5 if chao.motion_held() => chao.set(Behaviour::Game(WATER), -1),
        _ => {}
    }
    Next::Keep
}

/// `0x567FA0`: go to the tree that wants water (picker `0x5625B0`). Motion 0x64 (blend
/// 15), eyes 0, emotion ball 1 for 300 frames, 1800..1920 frames; turn 0x280 to the tree,
/// within 4 (`0x104DD30`) water it (`0x5680F0`). With no target the distance is 0.0
/// (`0x530580` measures the Chao against itself), so it waters at once.
fn go_water(chao: &mut Chao, senses: &dyn Senses, rng: &mut Rng) -> Next {
    if chao.mode == 0 {
        chao.play_blend(0x64, 15);
        chao.set_eyes(0, -1);
        chao.emotion_ball(1, 300);
        chao.patience = 1800 + rng.below(121);
        chao.mode = 1;
    }
    chao.patience -= 1;
    if chao.patience <= 0 {
        return Next::Pop;
    }
    turn_to_link(chao, senses, 0x280);
    if link_distance(chao, senses) < 4.0 {
        chao.set(Behaviour::Game(WATER), -1);
    }
    drive_on(chao);
    Next::Keep
}

/// `0x5680F0`: water the tree (jump table `0x568440`). Motion 1 (blend 15), 60..120
/// frames; then motion 0xD, emotion ball 1 for 120 frames, eyes 4, sound 0x601A. Without
/// toy skill 11 that's all: THIRSTY +2000, done. With it, 60..100 frames, motion 0x64 for
/// 150 frames backing off 0.015 a frame (`0x54B0E0`) and turning 0x200 to the tree, then the
/// watering can and motion 0xEB for 240..340 frames, 0xED; at its end THIRSTY +2000 and
/// pour (`0x568460`).
fn water(chao: &mut Chao, senses: &dyn Senses, rng: &mut Rng) -> Next {
    match chao.mode {
        0 => {
            chao.play_blend(1, 15);
            chao.count_a = 60 + rng.below(61);
            chao.mode = 1;
        }
        1 => {
            if !run_down(chao) {
                return Next::Keep;
            }
            chao.play_blend(0xD, 15);
            chao.emotion_ball(1, 120);
            chao.set_eyes(4, -1);
            chao.sound(0x601A);
            chao.count_a = 60 + rng.below(41);
            if chao.rec.u16(SKILLS) & WATERING == 0 {
                chao.rec.add_need(off::THIRSTY, 2000);
                return Next::Pop;
            }
            chao.mode = 2;
        }
        2 => {
            if run_down(chao) {
                chao.play_blend(0x64, 15);
                chao.mode = 3;
                chao.count_a = 150;
            }
        }
        3 => {
            let (s, c) = chao.body.facing();
            chao.body.pos[0] += s * -0.015;
            chao.body.pos[2] += c * -0.015;
            turn_to_link(chao, senses, 0x200);
            if run_down(chao) {
                chao.play_blend(0xEB, 15);
                chao.props[NODE_11] = WATERING_CAN;
                chao.mode = 4;
                chao.count_a = 240 + rng.below(101);
            }
        }
        4 => {
            if run_down(chao) {
                chao.play_blend(0xED, 15);
                chao.mode = 5;
            }
        }
        5 if chao.motion_done() => {
            chao.rec.add_need(off::THIRSTY, 2000);
            chao.set(Behaviour::Game(POUR), -1);
        }
        _ => {}
    }
    Next::Keep
}

/// `0x568460`: pour. Motion 0xDB (blend 15), again at its hold; at the second hold the water
/// (`TreeEvent::Water`, 0xAD) 4 ahead (`0x904EA8`) at its height, r 5 (`0xB5D210`), 10
/// frames, and done.
fn pour(chao: &mut Chao) -> Next {
    match chao.mode {
        0 => {
            chao.play_blend(0xDB, 15);
            chao.mode = 1;
        }
        1 if chao.motion_held() => {
            chao.play_blend(0xDB, 15);
            chao.mode = 2;
        }
        2 if chao.motion_held() => {
            let (s, c) = chao.body.facing();
            let p = chao.body.pos;
            chao.tree_events.push(TreeEvent::Water {
                pos: [p[0] + s * 4.0, p[1], p[2] + c * 4.0],
                radius: 5.0,
                frames: 10,
            });
            return Next::Pop;
        }
        _ => {}
    }
    Next::Keep
}

#[cfg(test)]
mod tests {
    use super::super::super::land::{GRID_CELLS, GRID_COLS, Tri, V3};
    use super::super::super::physics::ON_GROUND;
    use super::super::super::picker::{Alone, Carried};
    use super::super::super::record::{Record, TYPE_CHILD};
    use super::*;

    /// A flat lawn 200 across whose land cells (value 1) lie within 40 of the middle.
    fn garden() -> Garden {
        let mut tris = Vec::new();
        for i in -5..5 {
            for j in -5..5 {
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
        let grid = (0..GRID_CELLS)
            .map(|i| {
                let (col, row) = (i % GRID_COLS, i / GRID_COLS);
                i32::from((12..20).contains(&col) && (16..24).contains(&row))
            })
            .collect();
        Garden {
            tris,
            grid,
            ledges: Vec::new(),
            blockers: Vec::new(),
            toys: Vec::new(),
            tree_spots: Vec::new(),
        }
    }

    /// A garden with a seed in the Chao's hands, an empty spot and a tree wanting water.
    #[derive(Default)]
    struct Plot {
        seed: Option<u8>,
        spot: Option<V3>,
        dry: Option<V3>,
    }

    impl Senses for Plot {
        fn carrying(&self) -> Option<Carried> {
            self.seed.map(|kind| Carried { touch: SEED, kind })
        }
        fn plant_spot(&self) -> Option<V3> {
            self.spot
        }
        fn watering_spot(&self) -> Option<V3> {
            self.dry
        }
    }

    fn chao(set: impl Fn(&mut Record)) -> Chao {
        let mut rec = Record::default();
        rec.set_u8(off::TYPE, 5);
        rec.set_u16(off::LIFE, 100);
        rec.set_u16(off::NEED_CLOCK, 1);
        set(&mut rec);
        let mut c = Chao::new([0.0; 3], 0, rec);
        c.body.flags |= ON_GROUND;
        c
    }

    /// Runs `address` until the Chao leaves it: the frames it took and what it went on to.
    fn run_out(
        c: &mut Chao,
        address: u32,
        senses: &dyn Senses,
        max: u32,
        rng: &mut Rng,
    ) -> Option<(u32, Option<Behaviour>)> {
        let garden = garden();
        if c.current() != Some(Behaviour::Game(address)) {
            c.set(Behaviour::Game(address), -1);
        }
        for n in 1..=max {
            c.frame(&garden, 0, senses, rng);
            if c.current() != Some(Behaviour::Game(address)) {
                return Some((n, c.current()));
            }
        }
        None
    }

    #[test]
    fn holding_a_seed_up_turns_into_carrying_it() {
        let mut rng = Rng::new(1);
        let plot = Plot {
            seed: Some(3),
            ..Plot::default()
        };
        let mut c = chao(|_| {});
        let (n, next) = run_out(&mut c, HOLD, &plot, 1000, &mut rng).unwrap();
        assert!((300..=420).contains(&n), "{n}");
        assert_eq!(next, Some(Behaviour::Game(CARRY)));
        // Without the seed it's over at once.
        let mut c = chao(|_| {});
        assert_eq!(
            run_out(&mut c, HOLD, &Alone, 10, &mut rng),
            Some((1, Some(Behaviour::Settle)))
        );
    }

    #[test]
    fn a_spot_within_reach_is_walked_to_and_dug() {
        let mut rng = Rng::new(2);
        let plot = Plot {
            seed: Some(3),
            spot: Some([15.0, 0.0, 0.0]),
            ..Plot::default()
        };
        let mut c = chao(|_| {});
        let (n, next) = run_out(&mut c, CARRY, &plot, 2000, &mut rng).unwrap();
        assert_eq!(next, Some(Behaviour::Game(PLANT)));
        assert!(n < 1800, "{n}");
        assert_eq!(c.link, Some(Link::At([15.0, 0.0, 0.0])));
        assert!(distance(c.body.pos, [15.0, 0.0, 0.0]) < 2.6);
    }

    #[test]
    fn cleverer_chao_find_spots_further_off() {
        let plot = Plot {
            seed: Some(3),
            spot: Some([0.0, 0.0, 30.0]),
            ..Plot::default()
        };
        let garden = garden();
        let mut rng = Rng::new(4);
        // 30 is past 20 + 0.
        let mut c = chao(|_| {});
        c.set(Behaviour::Game(CARRY), -1);
        for _ in 0..2 {
            c.frame(&garden, 0, &plot, &mut rng);
        }
        assert_eq!(c.link, None);
        // 4000 points (×3) reach 32.
        let mut c = chao(|r| r.set_u16(INTELLIGENCE, 4000));
        c.set(Behaviour::Game(CARRY), -1);
        for _ in 0..2 {
            c.frame(&garden, 0, &plot, &mut rng);
        }
        assert_eq!(c.link, Some(Link::At([0.0, 0.0, 30.0])));
        assert_eq!(c.sounds.last(), Some(&0x601A));
    }

    #[test]
    fn carrying_ends_when_the_seed_is_gone_or_time_is_up() {
        let mut rng = Rng::new(5);
        let mut c = chao(|_| {});
        assert_eq!(
            run_out(&mut c, CARRY, &Alone, 10, &mut rng),
            Some((1, Some(Behaviour::Settle)))
        );
        // No spot anywhere: it wanders between land points, holding the seed up again at
        // most of them; each carry starts its 1800..2200 frames over.
        let plot = Plot {
            seed: Some(3),
            ..Plot::default()
        };
        let garden = garden();
        let mut c = chao(|_| {});
        c.set(Behaviour::Game(CARRY), -1);
        let mut held = 0;
        let mut was = CARRY;
        for _ in 0..20_000 {
            c.frame(&garden, 0, &plot, &mut rng);
            let at = c.current().unwrap().address();
            assert!(at == CARRY || at == HOLD, "{at:#x}");
            if at == HOLD && was == CARRY {
                held += 1;
            }
            was = at;
        }
        assert!(held > 3, "{held}");
        assert!(c.tree_events.is_empty());
        // Time up on the way: done, seed in hand.
        let mut c = chao(|_| {});
        c.set(Behaviour::Game(CARRY), -1);
        c.frame(&garden, 0, &plot, &mut rng);
        c.patience = 1;
        c.body.target = [0.0, 0.0, 1000.0];
        c.frame(&garden, 0, &plot, &mut rng);
        assert_eq!(c.current(), Some(Behaviour::Settle));
    }

    #[test]
    fn without_the_skill_it_only_looks() {
        let mut rng = Rng::new(6);
        let plot = Plot {
            seed: Some(3),
            spot: Some([1.0, 0.0, 0.0]),
            ..Plot::default()
        };
        let mut c = chao(|_| {});
        let (n, next) = run_out(&mut c, PLANT, &plot, 1000, &mut rng).unwrap();
        assert!((122..=242).contains(&n), "{n}");
        assert_eq!(next, Some(Behaviour::Settle));
        assert_eq!(c.bubble.0, 5);
        assert!(c.tree_events.is_empty());
    }

    #[test]
    fn planting_puts_the_seed_in_then_waters_it() {
        let mut rng = Rng::new(7);
        let plot = Plot {
            seed: Some(3),
            spot: Some([1.0, 0.0, 0.0]),
            ..Plot::default()
        };
        let mut c = chao(|r| r.set_u16(SKILLS, PLANTING));
        let (_, next) = run_out(&mut c, PLANT, &plot, 5000, &mut rng).unwrap();
        assert_eq!(next, Some(Behaviour::Game(WATER)));
        assert_eq!(
            c.tree_events,
            [TreeEvent::Plant {
                spot_pos: Some([1.0, 0.0, 0.0]),
                seed_kind: 3
            }]
        );
        assert_eq!(c.link, Some(Link::At([1.0, 0.0, 0.0])));
        // No spot by then: the seed is still used up.
        let plot = Plot {
            seed: Some(3),
            ..Plot::default()
        };
        let mut c = chao(|r| r.set_u16(SKILLS, PLANTING));
        run_out(&mut c, PLANT, &plot, 5000, &mut rng).unwrap();
        assert_eq!(
            c.tree_events,
            [TreeEvent::Plant {
                spot_pos: None,
                seed_kind: 3
            }]
        );
    }

    #[test]
    fn watering_without_the_can_just_says_so() {
        let mut rng = Rng::new(8);
        let mut c = chao(|r| r.set_u16(off::THIRSTY, 1000));
        let (n, next) = run_out(&mut c, WATER, &Alone, 1000, &mut rng).unwrap();
        assert!((61..=121).contains(&n), "{n}");
        assert_eq!(next, Some(Behaviour::Settle));
        assert_eq!(c.rec.u16(off::THIRSTY), 3000);
        assert!(c.tree_events.is_empty());
    }

    #[test]
    fn watering_with_the_can_pours_ahead() {
        let mut rng = Rng::new(9);
        let mut c = chao(|r| {
            r.set_u16(off::THIRSTY, 1000);
            r.set_u16(SKILLS, WATERING);
        });
        c.link = Some(Link::At([0.0, 0.0, 5.0]));
        let (_, next) = run_out(&mut c, WATER, &Alone, 5000, &mut rng).unwrap();
        assert_eq!(next, Some(Behaviour::Game(POUR)));
        assert_eq!(c.rec.u16(off::THIRSTY), 3000);
        assert_eq!(c.props[NODE_11], 0);
        let (_, next) = run_out(&mut c, POUR, &Alone, 1000, &mut rng).unwrap();
        assert_eq!(next, Some(Behaviour::Settle));
        let [
            TreeEvent::Water {
                pos,
                radius,
                frames,
            },
        ] = c.tree_events[..]
        else {
            panic!("{:?}", c.tree_events);
        };
        assert_eq!((radius, frames), (5.0, 10));
        // Facing the tree at +z, 4 ahead.
        let p = c.body.pos;
        assert!(
            distance(pos, [p[0], p[1], p[2] + 4.0]) < 0.1,
            "{pos:?} {p:?}"
        );
    }

    #[test]
    fn a_dry_tree_draws_a_chao_that_has_not_watered_lately() {
        let plot = Plot {
            dry: Some([20.0, 0.0, 0.0]),
            ..Plot::default()
        };
        let garden = garden();
        let mut chosen = false;
        for seed in 0..20 {
            let mut rng = Rng::new(seed);
            let mut c = chao(|_| {});
            c.choose(&garden, &plot, &mut rng);
            if c.current() == Some(Behaviour::Game(GO_WATER)) {
                chosen = true;
                assert_eq!(c.link, Some(Link::At([20.0, 0.0, 0.0])));
            }
            // Watered plenty lately, or a child: never.
            let mut c = chao(|r| r.set_u16(off::THIRSTY, 5000));
            c.choose(&garden, &plot, &mut rng);
            assert_eq!(c.current(), Some(Behaviour::Wander));
            let mut c = chao(|r| r.set_u8(off::TYPE, TYPE_CHILD));
            c.choose(&garden, &plot, &mut rng);
            assert_eq!(c.current(), Some(Behaviour::Wander));
        }
        assert!(chosen);
    }

    #[test]
    fn going_to_water_walks_up_to_the_tree() {
        let mut rng = Rng::new(10);
        let mut c = chao(|_| {});
        c.link = Some(Link::At([0.0, 0.0, 20.0]));
        let (n, next) = run_out(&mut c, GO_WATER, &Alone, 2000, &mut rng).unwrap();
        assert_eq!(next, Some(Behaviour::Game(WATER)));
        assert!(n > 30, "{n}");
        assert!(distance(c.body.pos, [0.0, 0.0, 20.0]) < 4.0);
        // No target: there at once.
        let mut c = chao(|_| {});
        assert_eq!(
            run_out(&mut c, GO_WATER, &Alone, 10, &mut rng),
            Some((1, Some(Behaviour::Game(WATER))))
        );
    }
}
