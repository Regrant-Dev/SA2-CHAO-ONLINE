//! A Chao's body in the garden: velocity, gravity, the floor and the walls (re/chao-ai.md
//! §5.2; the game's MOVE_WORK at `obj+0x38`).

use super::land::{Garden, Hit, V3, probe};

/// Gravity per frame (`0x55034E`, `*0x908548`).
pub const GRAVITY: f32 = -0.055;
/// The probe starts this far above the feet (`W+0xAC`).
const PROBE_HEIGHT: f32 = 3.0;
/// Wall radius (`W+0xB8`).
const WALL_RADIUS: f32 = 2.2;
/// Floor bounce (`W+0xC8`), wall bounce (`W+0xC4`) and ground friction (`W+0xD0`).
const FLOOR_BOUNCE: f32 = 0.7;
const WALL_BOUNCE: f32 = 0.8;
const FRICTION: f32 = 0.6;
/// Below this speed a Chao on the ground comes to rest (`*0x109E000`).
const REST_SPEED: f32 = 0.01;

/// `W+0x40` bits the AI reads.
pub const ON_GROUND: u16 = 0x400;
pub const WALL_PUSHED: u16 = 0x800;
pub const IN_WATER_POLY: u16 = 0x2000;
pub const FACING_WALL: u16 = 0x4000;
const MAY_REST: u16 = 0x8;
const RESTING: u16 = 0x10;

/// The water surface of each garden as the physics writes it (`0x53DF00`; N, H, D).
pub const WATER_Y: [f32; 3] = [-2.0, -1.5, -1.0];

#[derive(Clone, Debug)]
pub struct Body {
    pub pos: V3,
    pub vel: V3,
    /// Set by behaviours, used and cleared by `step` (`W+0xC..0x14`).
    pub accel: V3,
    /// Facing, BAMS (`D+0xC`): 0 faces +z.
    pub rot_y: u16,
    /// `W+0x40`.
    pub flags: u16,
    /// Where the walk is headed (`W+0x54..0x5C`).
    pub target: V3,
    /// The floor under the probe (`W+0x13C`) and its normal (`W+0x140`).
    pub floor_y: f32,
    pub floor_normal: V3,
    /// The water surface (`W+0xDC`).
    pub water_y: f32,
    /// The normal of the last wall it faced (`W+0x9C..0xA4`, kept with `FACING_WALL`).
    pub wall_normal: V3,
}

impl Body {
    pub fn new(pos: V3, rot_y: u16) -> Self {
        Self {
            pos,
            vel: [0.0; 3],
            accel: [0.0; 3],
            rot_y,
            flags: MAY_REST,
            target: pos,
            floor_y: -1e6,
            floor_normal: [0.0, 1.0, 0.0],
            water_y: -2_000_000.0,
            wall_normal: [0.0; 3],
        }
    }

    /// Moved from outside (held, or run by a player's game): no longer resting, so `step`
    /// moves it again (a resting body ignores gravity).
    pub fn wake(&mut self) {
        self.flags &= !RESTING;
    }

    pub fn on_ground(&self) -> bool {
        self.flags & ON_GROUND != 0
    }

    /// (sin, cos) of the facing: the x and z of a step forward.
    pub fn facing(&self) -> (f32, f32) {
        let a = f32::from(self.rot_y) * std::f32::consts::TAU / 65536.0;
        (a.sin(), a.cos())
    }

    /// Accelerates forward along the facing on the ground, as the walks do.
    pub fn drive(&mut self, k: f32) {
        if self.on_ground() {
            let (s, c) = self.facing();
            self.accel[0] = s * k;
            self.accel[2] = c * k;
        }
    }

    /// Turns toward `target` by at most `rate` BAMS; returns what's left (`0x796910`).
    pub fn turn_to_target(&mut self, rate: u16) -> u16 {
        let (dx, dz) = (self.target[0] - self.pos[0], self.target[2] - self.pos[2]);
        let want = (dx.atan2(dz) * 10430.378) as i32 as u16;
        let diff = want.wrapping_sub(self.rot_y) as i16;
        if diff.unsigned_abs() <= rate {
            self.rot_y = want;
        } else {
            self.rot_y = self
                .rot_y
                .wrapping_add(if diff > 0 { rate } else { rate.wrapping_neg() });
        }
        (want.wrapping_sub(self.rot_y) as i16).unsigned_abs()
    }

    /// Squared XZ distance to the target.
    pub fn target_distance2(&self) -> f32 {
        let (dx, dz) = (self.target[0] - self.pos[0], self.target[2] - self.pos[2]);
        dx * dx + dz * dz
    }

    /// One frame of movement: integrate (`0x796780`), then collide (`0x53DF00` mode 0:
    /// `0x798300`, then the garden's water height).
    pub fn step(&mut self, garden: &Garden, garden_index: usize) {
        if self.integrate() {
            self.collide(garden);
        }
        if let Some(&y) = WATER_Y.get(garden_index) {
            self.water_y = y;
        }
    }

    /// A frame with `D+0xA0 & 2` clear (riding the horse): no `0x796780`, only `0x53DF00`'s
    /// collision (skipped while resting, `0x53DF1A`) and the water height.
    pub fn step_unmoved(&mut self, garden: &Garden, garden_index: usize) {
        if self.flags & RESTING == 0 {
            self.collide(garden);
        }
        if let Some(&y) = WATER_Y.get(garden_index) {
            self.water_y = y;
        }
    }

    /// `0x796780`: gravity, velocity, resting. False when resting (the game returns before
    /// moving and `0x53DF00` skips the collision too).
    fn integrate(&mut self) -> bool {
        self.accel[1] += GRAVITY;
        for (v, a) in self.vel.iter_mut().zip(&mut self.accel) {
            *v += *a;
            *a = 0.0;
        }
        if self.flags & MAY_REST != 0 {
            let speed = length(self.vel);
            if self.flags & RESTING == 0 {
                if self.flags & (ON_GROUND | WALL_PUSHED) == ON_GROUND && speed < REST_SPEED {
                    self.vel = [0.0; 3];
                    self.flags |= RESTING;
                    return false;
                }
            } else if speed <= REST_SPEED {
                return false;
            } else {
                self.flags &= !RESTING;
            }
        }
        for (p, v) in self.pos.iter_mut().zip(self.vel) {
            *p += v;
        }
        true
    }

    /// `0x798300`: probe (`0x797E10`), floor (`0x796B20`), walls (`0x796CA0`).
    fn collide(&mut self, garden: &Garden) {
        self.flags &= 0xA0FF;
        let q = [self.pos[0], self.pos[1] + PROBE_HEIGHT, self.pos[2]];
        let slots = probe(&garden.tris, q);
        // Water never counts as floor; a water hit below sets the surface (`0x797E10`).
        let floor = slots[2].filter(|h| !h.water());
        if let Some(w) = slots[2].filter(Hit::water) {
            self.water_y = w.at;
            self.flags |= IN_WATER_POLY;
        }
        self.floor_y = floor.map_or(-1e6, |h| h.at);
        // The floor's own normal, unflipped: `0x796B20` tests its raw y (an underside faces down).
        self.floor_normal = floor.map_or([0.0, 1.0, 0.0], |h| h.normal);
        self.land_on_floor();
        self.push_from_walls(q, &slots);
        // Pillars, fences and graves are solid to a Chao (garden-collision §8.4).
        for b in &garden.blockers {
            if let Some(out) = b.push(self.pos) {
                for (p, o) in self.pos.iter_mut().zip(out) {
                    *p += o;
                }
            }
        }
    }

    /// `0x796B20`: below the floor puts the Chao on it, bounces and slows it.
    fn land_on_floor(&mut self) {
        self.flags &= !ON_GROUND;
        if self.pos[1] >= self.floor_y {
            return;
        }
        self.pos[1] = self.floor_y;
        let n = self.floor_normal;
        if n[1] > 0.78 {
            self.vel[1] -= 2.0 * self.vel[1] * FLOOR_BOUNCE;
        } else {
            let along = dot(self.vel, n);
            for (v, n) in self.vel.iter_mut().zip(n) {
                *v -= 2.0 * along * FLOOR_BOUNCE * n;
            }
        }
        self.vel[0] *= FRICTION;
        self.vel[2] *= FRICTION;
        self.flags |= ON_GROUND;
    }

    /// `0x796CA0`: the four side probes push the Chao out to its radius, bounce it, and
    /// mark a wall it faces.
    fn push_from_walls(&mut self, q: V3, slots: &[Option<Hit>; 6]) {
        self.flags &= !(WALL_PUSHED | FACING_WALL);
        // -X, +X, -Z, +Z; the slot's axis and which side of the Chao it is on.
        for (slot, axis, side) in [(0, 0, -1.0), (1, 0, 1.0), (4, 2, -1.0), (5, 2, 1.0)] {
            let Some(hit) = slots[slot].filter(|h| !h.water()) else {
                continue;
            };
            // Only walls more or less across this probe (`±0.2`).
            if hit.normal[axis].abs() <= 0.2 {
                continue;
            }
            let n = toward(hit.normal, axis, -side);
            let mut at = q;
            at[axis] = hit.at;
            let distance = dot(sub(self.pos, at), n);
            if distance >= WALL_RADIUS {
                continue;
            }
            let push = (WALL_RADIUS - distance) * 0.95;
            for (p, n) in self.pos.iter_mut().zip(n) {
                *p += n * push;
            }
            self.flags |= WALL_PUSHED;
            let (s, c) = self.facing();
            if n[0] * s + n[2] * c < -0.070_711_36 {
                self.flags |= FACING_WALL;
                self.wall_normal = n;
            }
            let along = dot(self.vel, n);
            if along < 0.0 {
                for (v, n) in self.vel.iter_mut().zip(n) {
                    *v -= 2.0 * along * WALL_BOUNCE * n;
                }
            }
        }
    }
}

/// `n` turned so its `axis` component has the sign of `sign`, so a wall's normal faces the
/// Chao. (The game branches on the sign instead, with a distance `0x780310` not yet decoded;
/// this is the push for a wall facing the Chao, applied to both.)
fn toward(n: V3, axis: usize, sign: f32) -> V3 {
    if n[axis] * sign < 0.0 {
        [-n[0], -n[1], -n[2]]
    } else {
        n
    }
}

fn dot(a: V3, b: V3) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn sub(a: V3, b: V3) -> V3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn length(v: V3) -> f32 {
    dot(v, v).sqrt()
}

#[cfg(test)]
mod tests {
    use super::super::land::{GRID_CELLS, Tri};
    use super::*;

    /// A flat floor at y = 0 and a wall at x = 10 facing -x.
    fn room() -> Garden {
        let floor = [
            Tri {
                v: [[-50.0, 0.0, -50.0], [50.0, 0.0, -50.0], [-50.0, 0.0, 50.0]],
                flags: 1,
            },
            Tri {
                v: [[50.0, 0.0, -50.0], [50.0, 0.0, 50.0], [-50.0, 0.0, 50.0]],
                flags: 1,
            },
        ];
        let wall = [
            Tri {
                v: [
                    [10.0, -10.0, -50.0],
                    [10.0, 40.0, -50.0],
                    [10.0, -10.0, 50.0],
                ],
                flags: 1,
            },
            Tri {
                v: [[10.0, 40.0, -50.0], [10.0, 40.0, 50.0], [10.0, -10.0, 50.0]],
                flags: 1,
            },
        ];
        Garden {
            tris: [floor, wall].concat(),
            grid: vec![0; GRID_CELLS],
            ledges: Vec::new(),
            blockers: Vec::new(),
            toys: Vec::new(),
            tree_spots: Vec::new(),
        }
    }

    #[test]
    fn a_dropped_chao_lands_and_comes_to_rest() {
        let garden = room();
        let mut body = Body::new([0.0, 10.0, 0.0], 0);
        for _ in 0..200 {
            body.step(&garden, 0);
        }
        assert!(body.on_ground() || body.flags & RESTING != 0);
        assert!(body.pos[1].abs() < 0.01, "{:?}", body.pos);
    }

    #[test]
    fn walking_into_a_wall_stops_at_its_radius_and_faces_it() {
        let garden = room();
        // Facing +x (0x4000) and driving into the wall at x = 10.
        let mut body = Body::new([0.0, 0.0, 0.0], 0x4000);
        let mut faced = false;
        for _ in 0..300 {
            body.drive(0.055);
            body.step(&garden, 0);
            faced |= body.flags & FACING_WALL != 0;
        }
        assert!(faced);
        assert!(
            body.pos[0] < 10.0 - 2.0 && body.pos[0] > 10.0 - 2.5,
            "{:?}",
            body.pos
        );
    }

    #[test]
    fn steady_walking_speed_is_about_one_and_a_half_drives() {
        let garden = room();
        let mut body = Body::new([-40.0, 0.0, 0.0], 0);
        for _ in 0..200 {
            body.drive(0.055);
            body.step(&garden, 0);
        }
        let speed = body.vel[2];
        // re/chao-ai.md §5.5: ≈ 1.5·k with friction 0.6 on every contact.
        assert!((speed - 0.0825).abs() < 0.01, "{speed}");
    }

    #[test]
    fn turning_goes_the_short_way_and_snaps() {
        let mut body = Body::new([0.0; 3], 0);
        body.target = [10.0, 0.0, 0.0]; // +x is 0x4000
        let left = body.turn_to_target(0x120);
        assert_eq!(body.rot_y, 0x120);
        assert_eq!(left, 0x4000 - 0x120);
        body.rot_y = 0x3F00;
        assert_eq!(body.turn_to_target(0x120), 0);
        assert_eq!(body.rot_y, 0x4000);
        body.target = [0.0, 0.0, -10.0]; // 0x8000: either way, by 0x120
        body.rot_y = 0xF000;
        body.turn_to_target(0x120);
        assert_eq!(body.rot_y, 0xF000 - 0x120);
    }
}
