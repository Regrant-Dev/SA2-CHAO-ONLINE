//! In the water: the water check, the swim dispatcher and its strokes, floating, and climbing
//! out (re/chao-ai.md §5.7, §2B "Swimming"). The physics has no buoyancy: the strokes hold the
//! Chao up themselves, as in the game.

use super::super::brain::{Behaviour, Chao, Next, Rng, random_cell_point};
use super::super::land::Garden;
use super::super::physics::{FACING_WALL, GRAVITY, WALL_PUSHED};
use super::super::picker::Senses;
use super::super::record::off;

/// Swim points (`rec+0x38`).
const SWIM_POINTS: usize = 0x38;
/// Animals absorbed (`rec+0x118`); bit 2 lets it swim the third way.
const ANIMALS: usize = 0x118;

/// `D+0x1B4` bits: went in (latched until it leaves or is held), in the water this frame.
const ENTERED: u16 = 0x1;
const IN_WATER: u16 = 0x4;

const SWIM: Behaviour = Behaviour::Game(0x562330);
const WEAK: Behaviour = Behaviour::Game(0x561E00);
const STROKE: Behaviour = Behaviour::Game(0x561FB0);
const STRONG: Behaviour = Behaviour::Game(0x562040);
const ANIMAL: Behaviour = Behaviour::Game(0x5620D0);
const FLOAT: Behaviour = Behaviour::Game(0x562160);
const CLIMB: Behaviour = Behaviour::Game(0x562EB0);

/// How far under the surface the strokes keep it (`0x906C10`).
const DEPTH: f64 = 2.099_999_904_632_568_4;

pub(super) fn run(
    chao: &mut Chao,
    address: u32,
    garden: &Garden,
    senses: &dyn Senses,
    rng: &mut Rng,
) -> Option<Next> {
    match address {
        0x562330 => Some(swim(chao, garden, senses, rng)),
        0x561E00 => Some(weak(chao, senses, rng)),
        0x561FB0 => Some(stroke(chao, 0x94, senses, rng)),
        0x562040 => Some(stroke(chao, 0x91, senses, rng)),
        0x5620D0 => Some(stroke(chao, 0x92, senses, rng)),
        0x562160 => Some(float(chao, rng)),
        0x562EB0 => Some(climb_out(chao)),
        _ => None,
    }
}

/// The water check (`0x561630`): true while the Chao is in the water (then the other
/// interrupts are skipped). Its surface is the AI's own per garden, not the physics'.
pub(super) fn water(chao: &mut Chao, garden: &Garden) -> bool {
    let _ = garden;
    if chao.held {
        chao.water &= !ENTERED;
        return false;
    }
    chao.body.water_y = match chao.area {
        1 => -2.2,
        2 => -1.5,
        3 => -1.0,
        _ => -10_000.0,
    };
    if f64::from(chao.body.pos[1]) + 2.0 >= f64::from(chao.body.water_y) {
        chao.water &= !(ENTERED | IN_WATER);
        return false;
    }
    if chao.water & ENTERED == 0 {
        chao.water |= ENTERED;
        chao.sound(0x1020);
        chao.set(SWIM, -1);
    }
    if chao.body.vel[1] < 0.0 {
        chao.body.vel[1] = (f64::from(chao.body.vel[1]) * 0.100_000_001_490_116_12) as f32;
    }
    chao.water |= IN_WATER;
    true
}

/// `0x562330`: a new swim target, then the stroke for its swim points (each set replaces the
/// last), and a first stroke step.
fn swim(chao: &mut Chao, garden: &Garden, senses: &dyn Senses, rng: &mut Rng) -> Next {
    let points = chao.rec.scaled(SWIM_POINTS);
    swim_target(chao, garden, rng);
    // The game also sets bit 0x10 of its collision info here (meaning not traced).
    if points < 300 {
        chao.set(WEAK, -1);
    } else {
        chao.set(if points < 2000 { STROKE } else { STRONG }, -1);
        if chao.rec.u32(ANIMALS) & 4 != 0 && rng.r() < 0.5 {
            chao.set(ANIMAL, -1);
        }
        if rng.r() < 0.2 {
            chao.set(FLOAT, -1);
        }
    }
    swim_step(chao, senses, rng);
    Next::Keep
}

/// `0x561740`: nearly quenched (THIRSTY < 100), a land point to walk out to; otherwise a
/// water point 18..60 ahead within ±100° (100 tries, then the origin).
fn swim_target(chao: &mut Chao, garden: &Garden, rng: &mut Rng) {
    if chao.rec.u16(off::THIRSTY) < 100 {
        if let Some((x, z)) = random_cell_point(garden, 1, rng) {
            chao.body.target[0] = x;
            chao.body.target[2] = z;
        }
        return;
    }
    let [x, y, z] = chao.body.pos;
    let mut point = [0.0, y, 0.0];
    for _ in 0..100 {
        let reach =
            ((f64::from(rng.r()) * 0.699_999_988_079_071 + 0.300_000_011_920_929) * 60.0) as f32;
        let turn = ((0.5 - f64::from(rng.r())) * 200.0 * -182.044_448_852_539_06) as i32;
        let a =
            f64::from(chao.body.rot_y.wrapping_sub(turn as u16)) * std::f64::consts::TAU / 65536.0;
        let p = [
            (a.sin() * f64::from(reach) + f64::from(x)) as f32,
            y,
            (a.cos() * f64::from(reach) + f64::from(z)) as f32,
        ];
        if garden.cell_at(p[0], p[2]) == Some(9) {
            point = p;
            break;
        }
    }
    chao.body.target = point;
}

/// `0x561E00`, the weakest swimmers (swim < 300): a struggling face, now and then a cry,
/// and thirst quenched ten times as fast.
fn weak(chao: &mut Chao, senses: &dyn Senses, rng: &mut Rng) -> Next {
    if chao.mode == 0 {
        chao.set_eyes(1, -1);
        if chao.mouth == 1 {
            if rng.r() < 0.25 {
                chao.set_mouth(9, -1);
            }
        } else {
            let r = rng.r();
            let mouth = match r {
                r if r < 0.25 => 6,
                r if r < 0.5 => 8,
                r if r < 0.75 => 0xB,
                _ => 4,
            };
            chao.set_mouth(mouth, -1);
        }
        chao.play(0x8E);
        chao.mode += 1;
    }
    if chao.frame.is_multiple_of(120) && rng.r() < 0.2 {
        chao.sound(0x6024);
    }
    if chao.rec.need_latch() {
        chao.rec.add_need(off::THIRSTY, -6000);
    }
    swim_step(chao, senses, rng);
    Next::Keep
}

/// `0x561FB0` (motion 0x94), `0x562040` (0x91) and `0x5620D0` (0x92): a look, the stroke,
/// then the stroke step.
fn stroke(chao: &mut Chao, motion: u16, senses: &dyn Senses, rng: &mut Rng) -> Next {
    if chao.mode == 0 {
        let r = rng.r();
        if r < 0.2 {
            chao.set_eyes(1, -1);
        } else if r < 0.4 {
            chao.set_eyes(4, -1);
        }
        chao.play(motion);
        chao.mode += 1;
    }
    swim_step(chao, senses, rng);
    Next::Keep
}

/// `0x561AF0`, every frame of a stroke. Its sets don't return, so the last one wins; the
/// counter `D+0x1C4` goes up twice a frame.
fn swim_step(chao: &mut Chao, senses: &dyn Senses, rng: &mut Rng) {
    let points = chao.rec.scaled(SWIM_POINTS);
    if chao.water & IN_WATER == 0 && chao.body.on_ground() {
        chao.set(Behaviour::Wander, -1);
    }
    tick(chao, rng);
    if chao.body.flags & FACING_WALL != 0 {
        if chao.rec.u16(off::THIRSTY) < 1000 {
            if senses.touch(0xCB).is_some() {
                chao.set(CLIMB, -1);
            }
        } else if rng.r() < 0.002 {
            chao.set(SWIM, -1);
        }
    }
    if chao.body.target_distance2() < 36.0 {
        chao.set(SWIM, -1);
    }
    let old = chao.count_b;
    chao.count_b += 1;
    if old > 600 {
        chao.set(SWIM, -1);
    }
    chao.body.turn_to_target(0x120);
    let thrust =
        (f64::from(points) * 9.999_999_974_752_427e-7 + 0.002_499_999_944_120_645_5) as f32;
    hold_under(
        chao,
        0.100_000_001_490_116_12,
        0.100_000_001_490_116_12,
        thrust,
    );
    if chao.rec.need_latch() {
        chao.rec.add_need(off::THIRSTY, -600);
        chao.rec.add_need(off::TEDIOUS, -100);
        chao.rec.add_need(off::TIRE, 100);
    }
    // `0x437590`: one sound per Chao and id, kept going while it is called.
    chao.sound(0x101F);
}

/// `D+0x1C4++`, with a `rand()` thrown away every tenth frame.
fn tick(chao: &mut Chao, rng: &mut Rng) {
    let old = chao.count_b;
    chao.count_b += 1;
    if old % 10 == 0 {
        rng.rand();
    }
}

/// Deeper than `DEPTH` under the surface: up by `rise` (not past that depth), gravity
/// cancelled, the fall damped by `damp`, and a push of `thrust` along the facing against a
/// drag of 0.05.
fn hold_under(chao: &mut Chao, rise: f64, damp: f64, thrust: f32) {
    let b = &mut chao.body;
    let limit = f64::from(b.water_y) - DEPTH;
    if f64::from(b.pos[1]) > limit {
        return;
    }
    let y = (f64::from(b.pos[1]) + rise) as f32;
    b.pos[1] = if f64::from(y) > limit {
        limit as f32
    } else {
        y
    };
    b.accel[1] = (f64::from(-GRAVITY) - f64::from(b.vel[1]) * damp) as f32;
    let (s, c) = b.facing();
    let drag = 0.050_000_000_745_058_06;
    let thrust = f64::from(thrust);
    b.accel[0] = (f64::from(s) * thrust - f64::from(b.vel[0]) * drag) as f32;
    b.accel[2] = (f64::from(c) * thrust - f64::from(b.vel[2]) * drag) as f32;
}

/// `0x562160`: float on the back for a while (600..1200 frames), then swim again. The cases
/// fall through, and mode 1 checks mode 2's motion end too.
fn float(chao: &mut Chao, rng: &mut Rng) -> Next {
    if chao.mode == 0 {
        chao.play(0x9A);
        chao.mode = 1;
    }
    if chao.mode <= 2 {
        if chao.mode == 1 {
            let old = chao.count_a;
            chao.count_a += 1;
            if old > 30 {
                chao.set_eyes(1, 120);
                chao.set_mouth(2, 120);
                chao.mode = 2;
                chao.sound(0x6025);
            }
        }
        if chao.motion_done() {
            let r = rng.r();
            if r < 0.5 {
                chao.set_eyes(3, 120);
            } else if r < 0.8 {
                chao.set_eyes(4, 120);
            }
            chao.play(0x96);
            chao.mode += 1;
            chao.count_a = 600 + (rng.r() * 601.0) as i32;
        }
    } else if chao.mode == 3 {
        let old = chao.count_a;
        chao.count_a -= 1;
        if old <= 0 {
            chao.set(SWIM, -1);
        }
    }
    float_step(chao, rng);
    Next::Keep
}

/// `0x561920`, every frame of floating: no push, a slow rise, and tiredness eased.
fn float_step(chao: &mut Chao, rng: &mut Rng) {
    if chao.water & ENTERED == 0 && chao.body.on_ground() {
        chao.set(Behaviour::Wander, -1);
    }
    tick(chao, rng);
    hold_under(
        chao,
        0.019_999_999_552_965_164,
        0.050_000_000_745_058_06,
        0.0,
    );
    if chao.rec.need_latch() {
        chao.rec.add_need(off::THIRSTY, -600);
        chao.rec.add_need(off::TEDIOUS, -100);
        chao.rec.add_need(off::TIRE, -100);
    }
    chao.sound(0x1021);
}

/// `0x562EB0`: climb out over a ledge. Face the wall while touching it, creep up and forward
/// (`0.1` along the facing, `0.08` up a frame, gravity cancelled), and once clear and on flat
/// ground, risen 2.5 or after 30 frames, wander off. Ends after 241 frames.
fn climb_out(chao: &mut Chao) -> Next {
    if chao.mode == 0 {
        chao.play(0x9B);
        chao.body.vel = [0.0; 3];
        chao.mode = 1;
    }
    let walled = chao.body.flags & (WALL_PUSHED | FACING_WALL) != 0;
    match chao.mode {
        1 => {
            if walled {
                let [nx, _, nz] = chao.body.wall_normal;
                let a = (f64::from(nx).atan2(f64::from(nz)) * -10_430.380_434_934_39) as i32;
                let face = 0x8000u16.wrapping_sub(a as u16);
                chao.body.rot_y = approach(chao.body.rot_y, face, 0x800);
                chao.body.flags &= !FACING_WALL;
                chao.count_a = 0;
            } else {
                let old = chao.count_a;
                chao.count_a += 1;
                if old > 2 {
                    chao.count_a = 0;
                    // `D+0x54`: the height it cleared the wall at.
                    chao.memo[0] = chao.body.pos[1];
                    chao.mode = 2;
                }
            }
        }
        2 => {
            chao.count_a += 1;
            if chao.count_a > 3 && chao.body.on_ground() {
                if chao.body.floor_normal[1] > 0.7 {
                    chao.set(Behaviour::Wander, -1);
                }
            } else if walled {
                chao.mode = 1;
            }
            if chao.count_a > 30 || f64::from(chao.body.pos[1]) - f64::from(chao.memo[0]) > 2.5 {
                chao.set(Behaviour::Wander, -1);
            }
        }
        _ => {}
    }
    let old = chao.count_b;
    chao.count_b += 1;
    if old > 240 {
        return Next::Pop;
    }
    let (s, c) = chao.body.facing();
    let b = &mut chao.body;
    b.pos[0] = (f64::from(s) * 0.100_000_001_490_116_12 + f64::from(b.pos[0])) as f32;
    b.pos[1] = (f64::from(b.pos[1]) + 0.079_999_998_211_860_66) as f32;
    b.pos[2] = (f64::from(c) * 0.100_000_001_490_116_12 + f64::from(b.pos[2])) as f32;
    b.accel[1] = -GRAVITY;
    Next::Keep
}

/// `0x446960`: `from` turned toward `to` by at most `rate`, the short way.
fn approach(from: u16, to: u16, rate: u16) -> u16 {
    let diff = to.wrapping_sub(from) as i16;
    if i32::from(diff) > i32::from(rate) || i32::from(diff) < -i32::from(rate) {
        if diff < 0 {
            from.wrapping_sub(rate)
        } else {
            from.wrapping_add(rate)
        }
    } else {
        to
    }
}

#[cfg(test)]
mod tests {
    use super::super::super::land::{GRID_CELLS, GRID_COLS, Tri, V3};
    use super::super::super::picker::Alone;
    use super::super::super::record::Record;
    use super::*;

    /// A pond floor at y = -8 everywhere; water cells (9) west of x = 20, land (1) east.
    fn pond() -> Garden {
        let floor = [
            Tri {
                v: [
                    [-60.0, -8.0, -60.0],
                    [60.0, -8.0, -60.0],
                    [-60.0, -8.0, 60.0],
                ],
                flags: 1,
            },
            Tri {
                v: [[60.0, -8.0, -60.0], [60.0, -8.0, 60.0], [-60.0, -8.0, 60.0]],
                flags: 1,
            },
        ];
        let grid = (0..GRID_CELLS)
            .map(|i| if i % GRID_COLS >= 18 { 1 } else { 9 })
            .collect();
        Garden {
            tris: floor.to_vec(),
            grid,
            ledges: Vec::new(),
            blockers: Vec::new(),
            toys: Vec::new(),
            tree_spots: Vec::new(),
        }
    }

    fn swimmer(points: u16, thirsty: u16) -> Chao {
        let mut rec = Record::default();
        rec.set_u16(SWIM_POINTS, points);
        rec.set_u16(off::THIRSTY, thirsty);
        rec.set_u16(off::NEED_CLOCK, 1);
        let mut chao = Chao::new([0.0, -6.0, 0.0], 0, rec);
        // Already in (the water check sets `ENTERED` once).
        chao.water = ENTERED | IN_WATER;
        chao
    }

    struct Ledge;

    impl Senses for Ledge {
        fn touch(&self, kind: u8) -> Option<V3> {
            (kind == 0xCB).then_some([0.0; 3])
        }
    }

    #[test]
    fn falling_in_starts_a_swim_once() {
        let garden = pond();
        let mut chao = swimmer(1000, 5000);
        chao.water = 0;
        chao.body.vel[1] = -1.0;
        chao.area = 1;
        assert!(water(&mut chao, &garden));
        assert_eq!(chao.sounds, [0x1020]);
        assert_eq!(chao.current(), Some(SWIM));
        assert_eq!(chao.water, ENTERED | IN_WATER);
        assert!((chao.body.vel[1] + 0.1).abs() < 1e-6);
        assert_eq!(chao.body.water_y, -2.2);
        // Still in: no new swim, no new splash.
        chao.set(STROKE, -1);
        assert!(water(&mut chao, &garden));
        assert_eq!(chao.sounds, [0x1020]);
        assert_eq!(chao.current(), Some(STROKE));
        // Just 2 under the surface is out.
        chao.body.pos[1] = -4.2;
        assert!(!water(&mut chao, &garden));
        assert_eq!(chao.water, 0);
    }

    #[test]
    fn a_held_chao_is_never_in_the_water() {
        let garden = pond();
        let mut chao = swimmer(1000, 5000);
        chao.area = 1;
        chao.water = ENTERED | IN_WATER;
        chao.held = true;
        assert!(!water(&mut chao, &garden));
        assert_eq!(chao.water, IN_WATER);
        // Each garden has its own surface; elsewhere there is no water.
        chao.held = false;
        chao.body.pos[1] = -3.2;
        chao.area = 3;
        assert!(water(&mut chao, &garden));
        chao.area = 0;
        assert!(!water(&mut chao, &garden));
    }

    #[test]
    fn swim_points_choose_the_stroke() {
        let garden = pond();
        for (points, allowed) in [
            (50, &[WEAK][..]),
            (400, &[STROKE, FLOAT][..]),
            (800, &[STRONG, FLOAT][..]),
        ] {
            let mut seen = Vec::new();
            for seed in 0..60 {
                let mut chao = swimmer(points, 5000);
                chao.set(SWIM, -1);
                chao.frame(&garden, 0, &Alone, &mut Rng::new(seed));
                let now = chao.current().unwrap();
                assert!(allowed.contains(&now), "{points}: {now:?}");
                if !seen.contains(&now) {
                    seen.push(now);
                }
            }
            assert_eq!(seen.len(), allowed.len(), "{points}: {seen:?}");
        }
        // Animal 2 absorbed: half the strong swims are the third stroke.
        let mut animal = 0;
        for seed in 0..100 {
            let mut chao = swimmer(800, 5000);
            chao.rec.set_u32(ANIMALS, 4);
            chao.set(SWIM, -1);
            chao.frame(&garden, 0, &Alone, &mut Rng::new(seed));
            animal += usize::from(chao.current() == Some(ANIMAL));
        }
        assert!((25..60).contains(&animal), "{animal}");
    }

    #[test]
    fn the_target_is_water_ahead_or_land_when_quenched() {
        let garden = pond();
        let mut rng = Rng::new(9);
        for _ in 0..20 {
            let mut chao = swimmer(400, 5000);
            swim_target(&mut chao, &garden, &mut rng);
            let [x, y, z] = chao.body.target;
            assert_eq!(garden.cell_at(x, z), Some(9));
            assert_eq!(y, -6.0);
            let d = (x * x + z * z).sqrt();
            assert!((17.9..=60.1).contains(&d), "{d}");
            assert!(z > -20.0, "ahead (+z): {z}");
        }
        let mut chao = swimmer(400, 50);
        swim_target(&mut chao, &garden, &mut rng);
        assert!(chao.body.target[0] >= 20.0, "{:?}", chao.body.target);
    }

    #[test]
    fn a_stroke_holds_it_under_the_surface_and_swims_off() {
        let garden = pond();
        let mut chao = swimmer(800, 5000);
        chao.set(STRONG, -1);
        chao.body.target = [-40.0, -6.0, 40.0];
        let mut rng = Rng::new(1);
        for _ in 0..200 {
            chao.frame(&garden, 0, &Alone, &mut rng);
        }
        assert_eq!(chao.motion, 0x91);
        assert_eq!(chao.current(), Some(STRONG));
        // Held at surface - 2.1, near enough, and moving toward the target.
        assert!((chao.body.pos[1] + 4.3).abs() < 0.05, "{:?}", chao.body.pos);
        assert!(chao.body.pos[0] < -1.0 && chao.body.pos[2] > 1.0);
        // The counter runs at two a frame: past 600 it swims somewhere new (~300 frames).
        assert_eq!(chao.count_b, 400);
        let mut frames = 0;
        while chao.current() == Some(STRONG) {
            chao.frame(&garden, 0, &Alone, &mut rng);
            frames += 1;
        }
        assert_eq!(frames, 101);
        assert_eq!(chao.current(), Some(SWIM));
    }

    #[test]
    fn swimming_quenches_thirst_once_a_need_period() {
        let garden = pond();
        let mut chao = swimmer(800, 5000);
        chao.rec.set_u16(off::TEDIOUS, 500);
        chao.rec.set_u16(off::TIRE, 500);
        chao.set(STROKE, -1);
        chao.body.target = [-40.0, -6.0, 40.0];
        let mut rng = Rng::new(2);
        chao.rec.set_u16(off::NEED_CLOCK, 0);
        chao.frame(&garden, 0, &Alone, &mut rng);
        chao.frame(&garden, 0, &Alone, &mut rng);
        assert_eq!(chao.rec.u16(off::THIRSTY), 4400);
        assert_eq!(chao.rec.u16(off::TEDIOUS), 400);
        assert_eq!(chao.rec.u16(off::TIRE), 600);
        assert!(chao.sounds.contains(&0x101F));
        // The weakest: ten times the thirst, and nothing else.
        let mut chao = swimmer(50, 7000);
        chao.set(WEAK, -1);
        chao.body.target = [-40.0, -6.0, 40.0];
        chao.rec.set_u16(off::NEED_CLOCK, 0);
        chao.frame(&garden, 0, &Alone, &mut rng);
        assert_eq!(chao.rec.u16(off::THIRSTY), 1000);
        assert_eq!(chao.rec.u16(off::TIRE), 0);
        assert_eq!((chao.motion, chao.eyes), (0x8E, 1));
        assert!([4, 6, 8, 0xB].contains(&chao.mouth));
    }

    #[test]
    fn out_of_the_water_on_the_ground_it_wanders() {
        let garden = pond();
        let mut chao = swimmer(800, 5000);
        chao.body.pos = [0.0, -8.0, 0.0];
        chao.set(STROKE, -1);
        chao.body.target = [-40.0, -6.0, 40.0];
        chao.area = 1;
        chao.body.flags |= super::super::super::physics::ON_GROUND;
        // Shallow: not in the water by the AI's surface.
        chao.body.pos[1] = -4.0;
        assert!(!water(&mut chao, &garden));
        stroke(&mut chao, 0x94, &Alone, &mut Rng::new(1));
        assert_eq!(chao.current(), Some(Behaviour::Wander));
    }

    #[test]
    fn a_thirst_quenched_swimmer_at_a_ledge_climbs_out() {
        let mut chao = swimmer(800, 500);
        chao.set(STROKE, -1);
        chao.body.target = [-40.0, -6.0, 40.0];
        chao.body.flags |= FACING_WALL;
        stroke(&mut chao, 0x94, &Ledge, &mut Rng::new(1));
        assert_eq!(chao.current(), Some(CLIMB));
        // Without a ledge (or still thirsty) it swims on.
        chao.set(STROKE, -1);
        chao.body.flags |= FACING_WALL;
        stroke(&mut chao, 0x94, &Alone, &mut Rng::new(1));
        assert_eq!(chao.current(), Some(STROKE));
    }

    #[test]
    fn climbing_out_faces_the_wall_then_creeps_up() {
        let mut chao = swimmer(800, 500);
        chao.set(CLIMB, -1);
        chao.body.vel = [1.0, 1.0, 1.0];
        // A wall to the +x, its normal facing the Chao (-x): it turns to face +x (0x4000).
        chao.body.wall_normal = [-1.0, 0.0, 0.0];
        chao.body.flags |= WALL_PUSHED;
        for _ in 0..8 {
            assert_eq!(climb_out(&mut chao), Next::Keep);
        }
        assert_eq!((chao.motion, chao.body.vel), (0x9B, [0.0; 3]));
        assert_eq!(chao.body.rot_y, 0x4000);
        assert_eq!(chao.mode, 1);
        let y = chao.body.pos[1];
        // Clear of the wall: three more frames, then it notes the height (mode 2).
        chao.body.flags &= !WALL_PUSHED;
        for _ in 0..3 {
            climb_out(&mut chao);
        }
        assert_eq!(chao.mode, 1);
        climb_out(&mut chao);
        assert_eq!(chao.mode, 2);
        assert!((chao.memo[0] - y - 0.08 * 3.0).abs() < 1e-4);
        assert!(chao.body.pos[0] > 0.5, "{:?}", chao.body.pos);
        // Risen 2.5 more: off it goes.
        while chao.current() == Some(CLIMB) {
            climb_out(&mut chao);
        }
        assert_eq!(chao.current(), Some(Behaviour::Wander));
        assert!(chao.body.pos[1] - chao.memo[0] > 2.4);
    }

    #[test]
    fn climbing_ends_after_241_frames() {
        let mut chao = swimmer(800, 500);
        chao.set(CLIMB, -1);
        chao.body.flags |= WALL_PUSHED;
        let mut frames = 1;
        while climb_out(&mut chao) == Next::Keep {
            frames += 1;
        }
        assert_eq!(frames, 242);
    }

    #[test]
    fn floating_lies_back_then_swims_again() {
        let garden = pond();
        let mut chao = swimmer(800, 5000);
        chao.set(FLOAT, -1);
        chao.body.target = [-40.0, -6.0, 40.0];
        let mut rng = Rng::new(4);
        for _ in 0..31 {
            chao.frame(&garden, 0, &Alone, &mut rng);
        }
        assert_eq!((chao.motion, chao.mode), (0x9A, 1));
        chao.frame(&garden, 0, &Alone, &mut rng);
        assert_eq!((chao.mode, chao.eyes, chao.mouth), (2, 1, 2));
        assert!(chao.sounds.contains(&0x6025));
        assert!(chao.sounds.contains(&0x1021));
        while chao.mode == 2 {
            chao.frame(&garden, 0, &Alone, &mut rng);
        }
        assert_eq!((chao.motion, chao.mode), (0x96, 3));
        assert!((600..=1200).contains(&chao.count_a));
        let lie = chao.count_a;
        let mut frames = 0;
        while chao.current() == Some(FLOAT) {
            chao.frame(&garden, 0, &Alone, &mut rng);
            frames += 1;
        }
        assert_eq!(frames, lie + 1);
        // No push while floating: it drifts at most a little.
        assert!(chao.body.pos[0].abs() < 1.0 && chao.body.pos[2].abs() < 1.0);
    }

    #[test]
    fn approach_turns_the_short_way() {
        assert_eq!(approach(0, 0x4000, 0x800), 0x800);
        assert_eq!(approach(0, 0xC000, 0x800), 0xF800);
        assert_eq!(approach(0x3C00, 0x4000, 0x800), 0x4000);
    }
}
