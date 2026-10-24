//! How a move's effect sprites move in the arena: a path of our own for each movement its ROM
//! script calls for (`gba::anim_kinds::Movement`), with as many sprites as the script makes
//! (within limits) and timed to fit the attack's beat. No game memory here.
//!
//! The arena's x runs from the player's side (left) to the foes', y is up and +z is toward the
//! camera. Points are a fighter's chest unless said otherwise.

use super::effects_gfx::{MoveLook, Movement, Place};

/// Every sprite of a move is gone this many frames after it starts.
pub const MAX_FRAMES: u16 = 60;
/// Sprites one move shows on one target at most.
pub const MAX_SHOTS: usize = 10;
/// How far in front of a fighter's chest its shots start and end (toward the other fighter).
const START_AHEAD: f32 = 0.9;
const END_AHEAD: f32 = 0.5;
/// Heights from the chest: the feet, and above the head.
const TO_FEET: f32 = -1.1;
const ABOVE: f32 = 5.0;

/// A sprite's path over its life.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Way {
    /// From `from` to `to`: `height` arcs it up, `sway` waves it up and down, `twist` circles
    /// it round the line (that far out).
    Line { height: f32, sway: f32, twist: f32 },
    /// Swirls in onto `to` from `radius` out, rising a little (`phase`: where it starts, in
    /// turns).
    SpiralIn { radius: f32, phase: f32 },
    /// Circles `to` `turns` times, `radius` out.
    Orbit { radius: f32, phase: f32, turns: f32 },
    /// Falls onto `to` from `height` above.
    Fall { height: f32 },
    /// Rises `height` from `to`, swaying, shrinking at the end.
    Rise { height: f32 },
    /// Closes in on `to` from `radius` out, shrinking.
    Close { radius: f32, phase: f32 },
    /// Bursts at `to`, growing.
    Burst,
    /// Shows at `to`, pulsing.
    Glow,
}

/// Where a sprite on `way` from `from` to `to` is `t` (0..=1) through its life, and its size
/// there (times its own).
pub fn place(way: Way, from: [f32; 3], to: [f32; 3], t: f32) -> ([f32; 3], f32) {
    let t = t.clamp(0.0, 1.0);
    let tau = std::f32::consts::TAU;
    let pi = std::f32::consts::PI;
    match way {
        Way::Line {
            height,
            sway,
            twist,
        } => {
            let mut p = lerp(from, to, t);
            p[1] += 4.0 * t * (1.0 - t) * height;
            p[1] += (t * tau * 1.5).sin() * sway;
            if twist > 0.0 {
                // Round the line, fading in and out at its ends.
                let a = t * tau * 2.0;
                let r = twist * (t * pi).sin();
                p[1] += a.cos() * r;
                p[2] += a.sin() * r;
            }
            (p, 1.0)
        }
        Way::SpiralIn { radius, phase } => {
            let a = (phase + t * 1.5) * tau;
            let r = radius * (1.0 - t);
            (
                [
                    to[0] + a.cos() * r,
                    to[1] - 0.6 * (1.0 - t),
                    to[2] + a.sin() * r * 0.5,
                ],
                0.8 + 0.4 * t,
            )
        }
        Way::Orbit {
            radius,
            phase,
            turns,
        } => {
            let a = (phase + t * turns) * tau;
            (
                [
                    to[0] + a.cos() * radius,
                    to[1] + (a * 2.0).sin() * 0.2,
                    to[2] + a.sin() * radius * 0.4,
                ],
                1.0,
            )
        }
        Way::Fall { height } => {
            // Speeding up as it falls.
            let drop = 1.0 - t * t;
            ([to[0], to[1] + height * drop, to[2]], 1.0)
        }
        Way::Rise { height } => (
            [to[0] + (t * tau).sin() * 0.25, to[1] + height * t, to[2]],
            if t > 0.7 { 1.0 - (t - 0.7) } else { 1.0 },
        ),
        Way::Close { radius, phase } => {
            let a = phase * tau;
            let r = radius * (1.0 - t);
            (
                [to[0] + a.cos() * r, to[1] + a.sin() * r * 0.8, to[2]],
                1.0 - 0.5 * t,
            )
        }
        Way::Burst => (to, 0.8 + 0.45 * t),
        Way::Glow => (to, 1.0 + (t * tau * 2.0).sin() * 0.08),
    }
}

fn lerp(a: [f32; 3], b: [f32; 3], t: f32) -> [f32; 3] {
    std::array::from_fn(|i| a[i] + (b[i] - a[i]) * t)
}

/// `at` moved `by` along the floor toward `toward` (toward the camera when they're one place).
fn ahead(at: [f32; 3], toward: [f32; 3], by: f32) -> [f32; 3] {
    let (dx, dz) = (toward[0] - at[0], toward[2] - at[2]);
    let length = (dx * dx + dz * dz).sqrt();
    if length < 1e-3 {
        return [at[0], at[1], at[2] + by];
    }
    let by = by.min(length * 0.4);
    [at[0] + dx / length * by, at[1], at[2] + dz / length * by]
}

/// A small spread for the `i`th sprite (each in -1..=1), the same every time.
fn spread(i: usize, seed: u16) -> (f32, f32) {
    let h = (i as u32 + 1)
        .wrapping_mul(0x9E37_79B9)
        .wrapping_add(u32::from(seed).wrapping_mul(0x85EB_CA6B));
    let h = h ^ (h >> 15);
    let a = (h & 0xFFFF) as f32 / 32767.5 - 1.0;
    let b = (h >> 16) as f32 / 32767.5 - 1.0;
    (a, b)
}

/// One sprite of a move: its path, when it starts (frames after the move does) and how long it
/// lasts.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Shot {
    pub way: Way,
    pub from: [f32; 3],
    pub to: [f32; 3],
    pub delay: u16,
    pub life: u16,
    /// It goes the other way (from the target back to the user).
    pub back: bool,
}

/// A move's sprites on one target, and the frame they land (when the target feels it).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Volley {
    pub shots: Vec<Shot>,
    pub impact: Option<Shot>,
    pub lands: u16,
}

/// The sprites `look` shows from `user` to `target` (one place for a move on the user itself),
/// varied a little by `seed`.
pub fn volley(look: &MoveLook, user: [f32; 3], target: [f32; 3], seed: u16) -> Volley {
    let on_self = user == target;
    let mut movement = look.movement;
    if on_self && movement.travels() {
        // A shot at oneself: it rises round the user instead.
        movement = Movement::Rise;
    }
    let at = match look.place {
        Place::User => user,
        Place::Target => target,
    };
    let start = ahead(user, target, START_AHEAD);
    let end = ahead(target, user, END_AHEAD);
    let count = usize::from(look.count);
    let gap = |lo: u16, hi: u16| look.gap.clamp(lo, hi);
    let travel = look.duration.map_or(22, |d| d.clamp(14, 30));
    let lob = look.height.map_or(1.8, |h| {
        (f32::from(h.unsigned_abs()) * 0.05).clamp(0.8, 3.0)
    });
    let line = |height: f32, sway: f32, twist: f32| Way::Line {
        height,
        sway,
        twist,
    };
    let shot = |way: Way, from: [f32; 3], to: [f32; 3], delay: u16, life: u16| Shot {
        way,
        from,
        to,
        delay,
        life,
        back: false,
    };
    let mut shots = Vec::new();
    // Frames after the start that it lands.
    let mut lands = 0;
    match movement {
        Movement::None => {}
        Movement::Straight => {
            shots.push(shot(line(0.25, 0.0, 0.0), start, end, 0, travel));
            lands = travel;
        }
        Movement::Arc => {
            shots.push(shot(line(lob, 0.0, 0.0), start, end, 0, travel + 4));
            lands = travel + 4;
        }
        Movement::Corkscrew => {
            let life = travel + 6;
            for i in 0..count.clamp(1, 3) {
                shots.push(shot(line(0.0, 0.0, 0.55), start, end, i as u16 * 5, life));
            }
            lands = life;
        }
        Movement::Beam => {
            for i in 0..9u16 {
                let sway = if i % 2 == 0 { 0.12 } else { -0.12 };
                shots.push(shot(line(0.0, sway, 0.0), start, end, i * 3, 14));
            }
            lands = 14;
        }
        Movement::Multi | Movement::MultiArc => {
            let life = travel.min(20);
            let g = gap(3, 8);
            for i in 0..count.clamp(2, 5) {
                let (a, b) = spread(i, seed);
                let height = if movement == Movement::MultiArc {
                    lob * (0.75 + 0.25 * a)
                } else {
                    0.2 + 0.3 * a
                };
                let to = [end[0], end[1] + 0.4 * b, end[2] + 0.3 * a];
                shots.push(shot(line(height, 0.0, 0.0), start, to, i as u16 * g, life));
            }
            lands = life;
        }
        Movement::Drain => {
            let from = ahead(target, user, END_AHEAD);
            let to = ahead(user, target, END_AHEAD);
            for i in 0..count.clamp(2, 4) {
                let sway = if i % 2 == 0 { 0.5 } else { -0.5 };
                let mut s = shot(line(0.6, sway, 0.0), from, to, i as u16 * 5, 26);
                s.back = true;
                shots.push(s);
            }
            lands = 0;
        }
        Movement::Spiral => {
            let n = count.clamp(2, 4);
            for i in 0..n {
                let phase = i as f32 / n as f32;
                let way = Way::SpiralIn { radius: 1.8, phase };
                shots.push(shot(way, at, at, i as u16 * 2, 28));
            }
            lands = 28;
        }
        Movement::Orbit => {
            let n = count.clamp(2, 4);
            for i in 0..n {
                let way = Way::Orbit {
                    radius: 1.6,
                    phase: i as f32 / n as f32,
                    turns: 1.25,
                };
                shots.push(shot(way, at, at, 0, 36));
            }
            lands = 18;
        }
        Movement::Rain => {
            for i in 0..count.clamp(4, 6) {
                let (a, b) = spread(i, seed);
                let to = [at[0] + 1.0 * a, at[1] - 0.4, at[2] + 0.6 * b];
                shots.push(shot(Way::Fall { height: ABOVE }, to, to, i as u16 * 4, 18));
            }
            lands = 18;
        }
        Movement::Rise => {
            for i in 0..count.clamp(3, 4) {
                let (a, b) = spread(i, seed);
                let to = [at[0] + 0.8 * a, at[1] + TO_FEET + 0.2, at[2] + 0.5 * b];
                shots.push(shot(Way::Rise { height: 2.8 }, to, to, i as u16 * 5, 26));
            }
            lands = 10;
        }
        Movement::Gather => {
            let n = count.clamp(3, 5);
            for i in 0..n {
                let way = Way::Close {
                    radius: 2.4,
                    phase: i as f32 / n as f32 + 0.125,
                };
                shots.push(shot(way, user, user, i as u16, 22));
            }
            lands = 22;
        }
        Movement::Burst | Movement::HitOnly => {
            let g = gap(4, 8);
            for i in 0..count.clamp(1, 3) {
                let (a, b) = spread(i, seed);
                let to = if i == 0 {
                    at
                } else {
                    [at[0] + 0.5 * a, at[1] + 0.5 * b, at[2]]
                };
                shots.push(shot(Way::Burst, to, to, i as u16 * g, 18));
            }
        }
        Movement::Aura => {
            for i in 0..count.clamp(1, 2) {
                shots.push(shot(Way::Glow, at, at, i as u16 * 8, 34));
            }
            lands = 8;
        }
    }
    shots.truncate(MAX_SHOTS);
    let impact = look.impact.as_ref().map(|_| {
        let when = if movement == Movement::Drain {
            0
        } else {
            lands
        };
        shot(Way::Burst, target, target, when, 18)
    });
    let mut v = Volley {
        shots,
        impact,
        lands,
    };
    fit(&mut v);
    v
}

/// Squeezes a volley's starts so everything is gone by `MAX_FRAMES`.
fn fit(v: &mut Volley) {
    let end = v
        .shots
        .iter()
        .chain(&v.impact)
        .map(|s| s.delay + s.life)
        .max()
        .unwrap_or(0);
    if end <= MAX_FRAMES {
        return;
    }
    let room = |s: &Shot| MAX_FRAMES.saturating_sub(s.life);
    let latest = v
        .shots
        .iter()
        .chain(&v.impact)
        .map(|s| s.delay)
        .max()
        .unwrap_or(0);
    for s in v.shots.iter_mut().chain(v.impact.as_mut()) {
        let squeezed = if latest == 0 {
            0
        } else {
            u32::from(s.delay) * u32::from(room(s)) / u32::from(latest)
        };
        s.delay = (squeezed as u16).min(room(s));
        s.life = s.life.min(MAX_FRAMES);
    }
    v.lands = v.lands.min(MAX_FRAMES);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::battle::effects_gfx::{Frame, Template};

    fn template() -> Template {
        Template {
            tag: 10_001,
            width: 16,
            height: 16,
            frames: vec![Frame {
                tile: 0,
                duration: 2,
                h_flip: false,
                v_flip: false,
            }],
            blended: false,
        }
    }

    fn look(movement: Movement, count: u16) -> MoveLook {
        MoveLook {
            count,
            ..MoveLook::of(movement, template())
        }
    }

    const USER: [f32; 3] = [-5.0, 9.3, 19.0];
    const TARGET: [f32; 3] = [5.0, 9.3, 19.0];

    fn close(a: [f32; 3], b: [f32; 3]) -> bool {
        (0..3).all(|i| (a[i] - b[i]).abs() < 0.05)
    }

    #[test]
    fn lines_start_in_front_of_the_user_and_end_in_front_of_the_target() {
        for m in [
            Movement::Straight,
            Movement::Arc,
            Movement::Beam,
            Movement::Multi,
            Movement::MultiArc,
            Movement::Corkscrew,
        ] {
            let v = volley(&look(m, 3), USER, TARGET, 7);
            assert!(!v.shots.is_empty(), "{m:?}");
            for s in &v.shots {
                let (first, _) = place(s.way, s.from, s.to, 0.0);
                let (last, _) = place(s.way, s.from, s.to, 1.0);
                assert!(first[0] > USER[0] && first[0] < -3.5, "{m:?} {first:?}");
                assert!(last[0] < TARGET[0] && last[0] > 3.5, "{m:?} {last:?}");
                assert!(!s.back);
            }
            assert!(v.lands > 10, "{m:?}");
        }
        // An arc goes up on its way; a straight shot hardly.
        let arc = volley(&look(Movement::Arc, 1), USER, TARGET, 0).shots[0];
        let straight = volley(&look(Movement::Straight, 1), USER, TARGET, 0).shots[0];
        let peak = |s: Shot| place(s.way, s.from, s.to, 0.5).0[1] - USER[1];
        assert!(peak(arc) > 1.0 && peak(straight) < 0.5);
        // A drain comes back.
        let drain = volley(&look(Movement::Drain, 2), USER, TARGET, 0);
        let s = drain.shots[0];
        assert!(s.back);
        assert!(place(s.way, s.from, s.to, 0.0).0[0] > 3.5);
        assert!(place(s.way, s.from, s.to, 1.0).0[0] < -3.5);
    }

    #[test]
    fn counts_follow_the_script_within_limits() {
        let n = |m: Movement, count: u16| volley(&look(m, count), USER, TARGET, 1).shots.len();
        assert_eq!(n(Movement::Straight, 4), 1);
        assert_eq!(n(Movement::Multi, 1), 2);
        assert_eq!(n(Movement::Multi, 3), 3);
        assert_eq!(n(Movement::Multi, 30), 5);
        assert_eq!(n(Movement::Beam, 1), 9);
        assert_eq!(n(Movement::Rain, 1), 4);
        assert_eq!(n(Movement::Rain, 20), 6);
        assert_eq!(n(Movement::Burst, 0), 1);
        assert_eq!(n(Movement::Burst, 9), 3);
        assert_eq!(n(Movement::None, 9), 0);
        assert!(n(Movement::Beam, 50) <= MAX_SHOTS);
    }

    #[test]
    fn movements_in_one_place_show_where_the_script_puts_them() {
        let mut on_user = look(Movement::Orbit, 2);
        on_user.place = Place::User;
        let v = volley(&on_user, USER, TARGET, 0);
        for s in &v.shots {
            for t in [0.0, 0.3, 0.7, 1.0] {
                let (p, _) = place(s.way, s.from, s.to, t);
                assert!((p[0] - USER[0]).abs() <= 1.7, "{p:?}");
            }
        }
        // Rain falls from above onto the target; rises go up from its feet.
        let rain = volley(&look(Movement::Rain, 4), USER, TARGET, 3);
        for s in &rain.shots {
            let (top, _) = place(s.way, s.from, s.to, 0.0);
            let (bottom, _) = place(s.way, s.from, s.to, 1.0);
            assert!(top[1] > TARGET[1] + 4.0 && bottom[1] < TARGET[1]);
            assert!((bottom[0] - TARGET[0]).abs() <= 1.0);
        }
        let rise = volley(&look(Movement::Rise, 3), USER, TARGET, 3);
        for s in &rise.shots {
            let (low, _) = place(s.way, s.from, s.to, 0.0);
            let (high, _) = place(s.way, s.from, s.to, 1.0);
            assert!(low[1] < TARGET[1] - 0.5 && high[1] > TARGET[1] + 1.5);
        }
        // A spiral ends on its fighter; a gather closes in on the user.
        let spiral = volley(&look(Movement::Spiral, 3), USER, TARGET, 0);
        let s = spiral.shots[0];
        assert!(close(place(s.way, s.from, s.to, 1.0).0, TARGET));
        assert!((place(s.way, s.from, s.to, 0.0).0[0] - TARGET[0]).abs() > 1.0);
        let gather = volley(&look(Movement::Gather, 3), USER, TARGET, 0);
        let s = gather.shots[0];
        assert!(close(place(s.way, s.from, s.to, 1.0).0, USER));
    }

    #[test]
    fn a_shot_at_oneself_rises_round_the_user() {
        let v = volley(&look(Movement::Beam, 9), USER, USER, 0);
        assert!(v.shots.iter().all(|s| matches!(s.way, Way::Rise { .. })));
        for s in &v.shots {
            assert!((s.to[0] - USER[0]).abs() <= 0.8);
        }
    }

    #[test]
    fn every_volley_ends_in_time() {
        let all = [
            Movement::None,
            Movement::HitOnly,
            Movement::Straight,
            Movement::Beam,
            Movement::Multi,
            Movement::MultiArc,
            Movement::Arc,
            Movement::Corkscrew,
            Movement::Spiral,
            Movement::Orbit,
            Movement::Rain,
            Movement::Rise,
            Movement::Drain,
            Movement::Gather,
            Movement::Burst,
            Movement::Aura,
        ];
        for m in all {
            for (count, gap, duration) in [(1, 0, None), (5, 8, Some(30)), (40, 40, Some(400))] {
                let mut l = look(m, count);
                l.gap = gap;
                l.duration = duration;
                l.impact = Some(template());
                let v = volley(&l, USER, TARGET, 5);
                for s in v.shots.iter().chain(&v.impact) {
                    assert!(s.delay + s.life <= MAX_FRAMES, "{m:?} {s:?}");
                    assert!(s.life >= 8, "{m:?} {s:?}");
                }
                assert!(v.lands <= MAX_FRAMES);
                if m != Movement::None {
                    assert!(v.impact.is_some());
                }
            }
        }
        // Squeezing keeps the order of the starts.
        let mut v = Volley {
            shots: (0..5)
                .map(|i| Shot {
                    way: Way::Burst,
                    from: USER,
                    to: USER,
                    delay: i * 20,
                    life: 20,
                    back: false,
                })
                .collect(),
            impact: None,
            lands: 0,
        };
        fit(&mut v);
        let delays: Vec<u16> = v.shots.iter().map(|s| s.delay).collect();
        assert!(delays.windows(2).all(|w| w[0] < w[1]), "{delays:?}");
        assert_eq!(*delays.last().unwrap(), MAX_FRAMES - 20);
    }

    #[test]
    fn sizes_grow_and_shrink_gently() {
        let (_, start) = place(Way::Burst, USER, USER, 0.0);
        let (_, end) = place(Way::Burst, USER, USER, 1.0);
        assert!(start < end && end < 1.5);
        let (_, low) = place(Way::Rise { height: 2.0 }, USER, USER, 1.0);
        assert!(low > 0.5 && low < 1.0);
        for t in [0.0, 0.25, 0.5] {
            let (_, s) = place(Way::Glow, USER, USER, t);
            assert!((0.9..=1.1).contains(&s));
        }
    }
}
