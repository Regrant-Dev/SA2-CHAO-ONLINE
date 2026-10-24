//! Where wild visitors can be in a garden and how they move (re/chao-wild-visitors.md,
//! re/shared-visitors.md): land spots on the grass, pond cells to swim in, a whistle calling
//! them, a land visitor roaming about. Plain Rust on a garden's ground (`land::Garden`): the
//! mod's own visitors (`gba_link::wild`) and the server's shared ones (`net::wild_host`) both
//! use it.

use super::land::{GRID_COLS, GRID_ROWS, Garden, Tri, V3, cell_of, probe};

/// Most visitors in a garden at once; each visit to a garden allows `MIN_CAP..=MAX_VISITORS`.
pub const MAX_VISITORS: usize = 7;
pub const MIN_CAP: usize = 6;
/// The game's category-2 entries (animals and drives, `0x8AB840`: 10 in the game, raised to
/// this by the mod at start, `registry::raise_animal_limit`; the registry has room for 64), and
/// how many of them are left for the game's own.
pub const ANIMAL_LIMIT: u32 = 20;
pub const RESERVE: u32 = 2;
/// Visitors that come at once when the garden loads; the others come one by one.
pub const ON_ENTRY: usize = 4;
const _: () = assert!(ON_ENTRY <= MIN_CAP && MIN_CAP <= MAX_VISITORS);
/// Seconds between visitors, and how long one stays before it goes once out of the way.
pub const NEXT_SECS: (u64, u64) = (20, 50);
pub const STAY_SECS: (u64, u64) = (300, 480);
/// A visitor that stayed long enough goes once the player is this far away.
pub const GONE_FAR: f32 = 60.0;
/// New visitors come at least this far from the player when a spot allows.
pub const COME_FAR: f32 = 25.0;
/// Of ten visitors, about this many go to the pond (when the area has water creatures).
pub const POND_IN_TEN: u64 = 3;
pub const MAX_SWIMMING: usize = 3;
/// The water surface of each garden (Neutral, Hero, Dark) and how far below it a swimmer is.
pub const WATER_Y: [f32; 3] = [-2.0, -1.5, -1.0];
const SWIM_DEPTH: f32 = 1.5;
const BOB: f32 = 0.2;
/// Units a frame.
const SWIM_SPEED: f32 = 0.06;
/// A pond spot has its floor at least this far below the surface.
const POND_DEPTH: f32 = 3.0;
/// Grid values.
pub const LAND: i32 = 1;
pub const WATER: i32 = 9;
/// Below this a visitor fell out of the garden.
pub const LOST_Y: f32 = -50.0;
/// A whistle calls visitors for this long (it starts again while the player whistles), from
/// this far away; they come to within `CALL_NEAR` of the player.
pub const CALL_SECS: u64 = 6;
pub const CALL_REACH: f32 = 100.0;
pub const CALL_NEAR: f32 = 4.0;
/// How fast a called visitor walks (units a frame).
pub const WALK_SPEED: f32 = 0.3;
/// A roaming visitor walks at the game's animal speed (`0x165DFD8`) to a land spot this near,
/// after standing this many frames.
pub const ROAM_SPEED: f32 = 0.15;
pub const ROAM_REACH: f32 = 30.0;
pub const ROAM_WAIT: (u64, u64) = (60, 240);

/// splitmix64: a well-mixed 64-bit value from `x`.
pub fn mix(x: u64) -> u64 {
    let mut z = x.wrapping_add(0x9E37_79B9_7F4A_7C15);
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

/// A small random source.
#[derive(Clone, Debug)]
pub struct Rng(u64);

impl Rng {
    pub fn new(seed: u64) -> Self {
        Self(seed)
    }

    #[allow(clippy::should_implement_trait)]
    pub fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        mix(self.0)
    }

    /// 0..n (0 when n is 0).
    pub fn below(&mut self, n: u64) -> u64 {
        if n == 0 { 0 } else { self.next() % n }
    }

    /// lo..=hi.
    pub fn between(&mut self, lo: u64, hi: u64) -> u64 {
        lo + self.below(hi.saturating_sub(lo) + 1)
    }
}

/// A grid cell (column, row).
pub type Cell = (usize, usize);

/// The middle of a cell (re/chao-wild-visitors.md §1).
pub fn middle((col, row): Cell) -> (f32, f32) {
    (
        (col as f32 - 16.0 + 0.5) * 10.0,
        (row as f32 - 20.0 + 0.5) * 10.0,
    )
}

/// Where visitors can be in a garden: the middles of land cells on solid ground, and water
/// cells over a pond deep enough to swim in (with the surface's height).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Spots {
    pub land: Vec<V3>,
    pub water: Vec<(Cell, f32)>,
}

impl Spots {
    pub fn read(garden: &Garden) -> Self {
        let mut out = Self::default();
        for row in 0..GRID_ROWS {
            for col in 0..GRID_COLS {
                let value = garden.grid[row * GRID_COLS + col];
                if value != LAND && value != WATER {
                    continue;
                }
                let (x, z) = middle((col, row));
                // The highest surface under the middle (probes see 220 up).
                let Some(top) = [60.0, 180.0]
                    .into_iter()
                    .filter_map(|y| probe(&garden.tris, [x, y, z])[2])
                    .max_by(|a, b| a.at.total_cmp(&b.at))
                else {
                    continue;
                };
                match (value, top.water()) {
                    (LAND, false) => out.land.push([x, top.at, z]),
                    (WATER, true)
                        if floor_below(&garden.tris, [x, top.at - 0.01, z])
                            .is_some_and(|y| y <= top.at - POND_DEPTH) =>
                    {
                        out.water.push(((col, row), top.at))
                    }
                    _ => {}
                }
            }
        }
        out
    }

    /// Without the land spots within `clear` of `at` (across).
    pub fn clear_of(mut self, at: V3, clear: f32) -> Self {
        self.land
            .retain(|s| dist2((s[0], s[2]), (at[0], at[2])) >= clear * clear);
        self
    }

    /// The pond cell `cell`'s surface, if it is one.
    pub fn water_at(&self, cell: Cell) -> Option<f32> {
        self.water.iter().find(|(c, _)| *c == cell).map(|(_, y)| *y)
    }

    /// The pond cells next to `cell` (8 around).
    pub fn neighbours(&self, (col, row): Cell) -> Vec<Cell> {
        self.water
            .iter()
            .map(|(c, _)| *c)
            .filter(|&(c, r)| c.abs_diff(col) <= 1 && r.abs_diff(row) <= 1 && (c, r) != (col, row))
            .collect()
    }

    /// Pond cells a visitor can come in at: those with water on at least three sides.
    pub fn pond_entries(&self) -> Vec<Cell> {
        self.water
            .iter()
            .map(|(c, _)| *c)
            .filter(|&c| self.neighbours(c).len() >= 3)
            .collect()
    }

    /// The land spot nearest (x, z) within `reach` (across), if any.
    pub fn land_near(&self, at: (f32, f32), reach: f32) -> Option<V3> {
        self.land
            .iter()
            .filter(|s| dist2((s[0], s[2]), at) <= reach * reach)
            .min_by(|a, b| dist2((a[0], a[2]), at).total_cmp(&dist2((b[0], b[2]), at)))
            .copied()
    }

    /// The pond cell nearest (x, z) within `reach` (across), if any.
    pub fn pond_near(&self, at: (f32, f32), reach: f32) -> Option<Cell> {
        self.water
            .iter()
            .map(|(c, _)| *c)
            .filter(|&c| dist2(middle(c), at) <= reach * reach)
            .min_by(|&a, &b| dist2(middle(a), at).total_cmp(&dist2(middle(b), at)))
    }
}

/// The solid floor below `q`, through water surfaces.
pub fn floor_below(tris: &[Tri], q: V3) -> Option<f32> {
    let mut q = q;
    for _ in 0..4 {
        let hit = probe(tris, q)[2]?;
        if !hit.water() {
            return Some(hit.at);
        }
        q[1] = hit.at - 0.01;
    }
    None
}

/// A swimming visitor: where it is and the pond cell it swims to.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Swim {
    pub at: (f32, f32),
    pub to: Cell,
    pub from: Option<Cell>,
}

impl Swim {
    /// Starts swimming at `at`, toward the nearest pond cell.
    pub fn start(spots: &Spots, at: (f32, f32)) -> Option<Self> {
        let to = spots
            .water
            .iter()
            .map(|(c, _)| *c)
            .min_by(|&a, &b| dist2(middle(a), at).total_cmp(&dist2(middle(b), at)))?;
        Some(Self { at, to, from: None })
    }

    /// One frame on: toward its cell, and from there to a neighbouring one (not straight back
    /// when it can help it; toward `called`, the whistling player, when it's called, staying
    /// put at the pond cell nearest them). Returns the heading (BAMS, the game's sin/cos
    /// convention).
    pub fn step(&mut self, spots: &Spots, rng: &mut Rng, called: Option<(f32, f32)>) -> i32 {
        let goal = middle(self.to);
        let (dx, dz) = (goal.0 - self.at.0, goal.1 - self.at.1);
        let d = (dx * dx + dz * dz).sqrt();
        if d <= SWIM_SPEED {
            self.at = goal;
            if let Some(player) = called {
                let here = dist2(goal, player);
                let nearer = spots
                    .neighbours(self.to)
                    .into_iter()
                    .filter(|&c| dist2(middle(c), player) < here)
                    .min_by(|&a, &b| dist2(middle(a), player).total_cmp(&dist2(middle(b), player)));
                if let Some(n) = nearer {
                    self.from = Some(self.to);
                    self.to = n;
                }
                return heading(player.0 - self.at.0, player.1 - self.at.1);
            }
            let mut next = spots.neighbours(self.to);
            if next.len() > 1 {
                next.retain(|c| Some(*c) != self.from);
            }
            if let Some(&n) = next.get(rng.below(next.len() as u64) as usize) {
                self.from = Some(self.to);
                self.to = n;
            }
        } else {
            self.at.0 += dx / d * SWIM_SPEED;
            self.at.1 += dz / d * SWIM_SPEED;
        }
        let goal = middle(self.to);
        heading(goal.0 - self.at.0, goal.1 - self.at.1)
    }
}

/// A land visitor walking about as the game's animals do (0x57BD40): it stands a while, then
/// walks to a land spot nearby, turning to another when its next step is onto water.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Roam {
    /// Frames it still stands.
    pub wait: u32,
    /// Where it walks to.
    pub goal: Option<(f32, f32)>,
}

impl Roam {
    pub fn new(rng: &mut Rng) -> Self {
        Self {
            wait: rng.between(ROAM_WAIT.0, ROAM_WAIT.1) as u32,
            goal: None,
        }
    }

    /// One frame on for a visitor at `at` in a garden with `spots` and cell grid `grid`: where
    /// it steps to and its heading, or `None` while it stands.
    pub fn step(
        &mut self,
        spots: &Spots,
        grid: &[i32],
        at: (f32, f32),
        rng: &mut Rng,
    ) -> Option<((f32, f32), i32)> {
        if self.wait > 0 {
            self.wait -= 1;
            return None;
        }
        let Some(goal) = self.goal else {
            let near: Vec<&V3> = spots
                .land
                .iter()
                .filter(|s| {
                    let d = dist2((s[0], s[2]), at);
                    d > 1.0 && d <= ROAM_REACH * ROAM_REACH
                })
                .collect();
            match near.get(rng.below(near.len() as u64) as usize) {
                Some(s) => self.goal = Some((s[0], s[2])),
                None => *self = Self::new(rng),
            }
            return None;
        };
        let (dx, dz) = (goal.0 - at.0, goal.1 - at.1);
        let d = (dx * dx + dz * dz).sqrt();
        if d <= ROAM_SPEED {
            *self = Self::new(rng);
            return Some((goal, heading(dx, dz)));
        }
        let next = (at.0 + dx / d * ROAM_SPEED, at.1 + dz / d * ROAM_SPEED);
        let value = cell_of(next.0, next.1).and_then(|(c, r)| grid.get(r * GRID_COLS + c));
        if value.is_none_or(|&v| v == WATER) {
            // Not into the pond (nor off the grid): it stands a moment, then turns elsewhere.
            self.goal = None;
            self.wait = ROAM_WAIT.0 as u32 / 2;
            return None;
        }
        Some((next, heading(dx, dz)))
    }
}

pub fn dist2(a: (f32, f32), b: (f32, f32)) -> f32 {
    (a.0 - b.0).powi(2) + (a.1 - b.1).powi(2)
}

/// The BAMS angle that walks along (dx, dz) as the game does (sin, cos).
pub fn heading(dx: f32, dz: f32) -> i32 {
    (dx.atan2(dz) * 65536.0 / std::f32::consts::TAU) as i32
}

/// One frame of a called visitor at `at` walking to `player`: where it is next and its heading,
/// or `None` once it's near enough.
pub fn walk_toward(at: (f32, f32), player: (f32, f32)) -> Option<((f32, f32), i32)> {
    let (dx, dz) = (player.0 - at.0, player.1 - at.1);
    let d = (dx * dx + dz * dz).sqrt();
    if d <= CALL_NEAR {
        return None;
    }
    let step = WALK_SPEED.min(d - CALL_NEAR);
    Some((
        (at.0 + dx / d * step, at.1 + dz / d * step),
        heading(dx, dz),
    ))
}

/// Whether a roaming visitor at `at` in a garden (index 0..3) swims: in a pond cell, below the
/// surface.
pub fn in_pond(spots: &Spots, at: V3) -> bool {
    cell_of(at[0], at[2])
        .and_then(|c| spots.water_at(c))
        .is_some_and(|surface| at[1] < surface)
}

/// Where a swimmer is drawn: under the surface, bobbing.
pub fn swim_y(surface: f32, seconds: f32) -> f32 {
    surface - SWIM_DEPTH + BOB * (seconds * 2.0).sin()
}

/// Whether the next visitor goes to the pond.
pub fn to_pond(rng: &mut Rng, swimming: usize, pond_cells: bool) -> bool {
    pond_cells && swimming < MAX_SWIMMING && rng.below(10) < POND_IN_TEN
}

/// A land spot for a new visitor: away from the player (`from`) when one is.
pub fn land_spot(spots: &Spots, from: (f32, f32), rng: &mut Rng) -> Option<V3> {
    land_spot_away(spots, &[from], rng)
}

/// A land spot for a new visitor: away from every player at `from` when one is.
pub fn land_spot_away(spots: &Spots, from: &[(f32, f32)], rng: &mut Rng) -> Option<V3> {
    let far: Vec<&V3> = spots
        .land
        .iter()
        .filter(|s| {
            from.iter()
                .all(|&p| dist2((s[0], s[2]), p) >= COME_FAR * COME_FAR)
        })
        .collect();
    let list: Vec<&V3> = if far.is_empty() {
        spots.land.iter().collect()
    } else {
        far
    };
    list.get(rng.below(list.len() as u64) as usize).map(|s| **s)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chao_sim::land::{GRID_CELLS, LAND_EXPORT, PeFile};

    #[test]
    fn swimmers_stay_on_pond_cells() {
        // A 3x3 pond with its middle at (5, 5).
        let mut spots = Spots::default();
        for c in 4..7 {
            for r in 4..7 {
                spots.water.push(((c, r), -2.0));
            }
        }
        assert_eq!(spots.pond_entries().len(), 9);
        assert_eq!(spots.neighbours((5, 5)).len(), 8);
        assert_eq!(spots.neighbours((4, 4)).len(), 3);
        let (x, z) = middle((5, 5));
        assert!(in_pond(&spots, [x, -3.0, z]));
        assert!(!in_pond(&spots, [x, -1.0, z]));
        assert!(!in_pond(&spots, [x + 30.0, -3.0, z]));
        let mut swim = Swim::start(&spots, (x + 1.0, z)).unwrap();
        assert_eq!(swim.to, (5, 5));
        let mut rng = Rng::new(3);
        let (lo, hi) = (middle((4, 4)), middle((6, 6)));
        for _ in 0..5_000 {
            swim.step(&spots, &mut rng, None);
            assert!((lo.0 - 0.01..=hi.0 + 0.01).contains(&swim.at.0));
            assert!((lo.1 - 0.01..=hi.1 + 0.01).contains(&swim.at.1));
        }
        // Heading follows the game's (sin, cos).
        assert_eq!(heading(0.0, 1.0), 0);
        assert_eq!(heading(1.0, 0.0), 0x4000);
        let y = swim_y(-2.0, 0.0);
        assert!((y + 3.5).abs() < 1e-4);
        // Called from beyond the pond's (6, 6) corner: it swims there and stays.
        let player = (middle((6, 6)).0 + 20.0, middle((6, 6)).1 + 20.0);
        for _ in 0..2_000 {
            swim.step(&spots, &mut rng, Some(player));
        }
        assert_eq!(swim.to, (6, 6));
        let corner = middle((6, 6));
        assert!(dist2(swim.at, corner) < 0.01);
    }

    #[test]
    fn a_whistle_walks_a_visitor_to_the_player() {
        let mut at = (0.0, 0.0);
        let player = (30.0, 40.0);
        let mut steps = 0;
        while let Some((next, angle)) = walk_toward(at, player) {
            // Heading toward the player, a step at a time, never past the stopping distance.
            assert_eq!(angle, heading(player.0 - at.0, player.1 - at.1));
            assert!(dist2(next, at).sqrt() <= WALK_SPEED + 1e-4);
            at = next;
            steps += 1;
            assert!(steps < 1_000);
        }
        assert!((dist2(at, player).sqrt() - CALL_NEAR).abs() < 1e-3);
        assert_eq!(walk_toward((29.0, 40.0), player), None);
    }

    #[test]
    fn new_visitors_come_away_from_the_player() {
        let spots = Spots {
            land: vec![[0.0, 0.0, 0.0], [100.0, 1.0, 0.0]],
            water: Vec::new(),
        };
        let mut rng = Rng::new(5);
        for _ in 0..20 {
            assert_eq!(
                land_spot(&spots, (0.0, 0.0), &mut rng),
                Some([100.0, 1.0, 0.0])
            );
        }
        assert!(land_spot(&Spots::default(), (0.0, 0.0), &mut rng).is_none());
        // Away from every player: none far from both, so any.
        let both = [(0.0, 0.0), (100.0, 0.0)];
        assert!(land_spot_away(&spots, &both, &mut rng).is_some());
        // No pond cells: never the pond; three swimming: never again.
        assert!((0..100).all(|_| !to_pond(&mut rng, 0, false)));
        assert!((0..100).all(|_| !to_pond(&mut rng, MAX_SWIMMING, true)));
        assert!((0..100).any(|_| to_pond(&mut rng, 0, true)));
    }

    #[test]
    fn a_roaming_visitor_walks_between_land_spots_and_keeps_off_water() {
        // Land in columns 0..16, water from column 16 on.
        let mut grid = vec![LAND; GRID_CELLS];
        for (i, c) in grid.iter_mut().enumerate() {
            if i % GRID_COLS >= 16 {
                *c = WATER;
            }
        }
        let mut spots = Spots::default();
        for col in 0..20 {
            for row in 10..20 {
                let (x, z) = middle((col, row));
                spots.land.push([x, 0.0, z]);
            }
        }
        let mut rng = Rng::new(9);
        let mut roam = Roam::new(&mut rng);
        let mut at = middle((14, 15));
        let mut walked = 0.0;
        for _ in 0..20_000 {
            if let Some((next, _)) = roam.step(&spots, &grid, at, &mut rng) {
                let step = dist2(next, at).sqrt();
                assert!(step <= ROAM_SPEED + 1e-4);
                walked += step;
                at = next;
            }
            let (col, _) = cell_of(at.0, at.1).unwrap();
            assert!(col < 16, "on water at {at:?}");
        }
        assert!(walked > 100.0, "it walks about: {walked}");
    }

    #[test]
    fn spots_near_a_place() {
        let spots = Spots {
            land: vec![[0.0, 0.0, 0.0], [10.0, 0.0, 0.0]],
            water: vec![((16, 20), -2.0)],
        };
        assert_eq!(spots.land_near((9.0, 0.0), 5.0), Some([10.0, 0.0, 0.0]));
        assert_eq!(spots.land_near((50.0, 0.0), 5.0), None);
        assert_eq!(spots.pond_near((5.0, 5.0), 15.0), Some((16, 20)));
        assert_eq!(spots.clone().clear_of([0.0, 3.0, 1.0], 8.0).land.len(), 1);
    }

    #[test]
    fn gardens_have_spots_on_grass_and_in_the_pond() {
        let (Ok(dll), Ok(exe)) = (
            std::fs::read(
                "/mnt/SSD1/SteamLibrary/steamapps/common/Sonic Adventure 2/resource/gd_PC/DLL/Win32/Data_DLL_orig.dll",
            ),
            std::fs::read(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/re/bin/sonic2app.dumped.exe"
            )),
        ) else {
            return;
        };
        let (dll, exe) = (PeFile::parse(dll).unwrap(), PeFile::parse(exe).unwrap());
        for (i, name) in LAND_EXPORT.iter().enumerate() {
            let garden = Garden::read(&dll, dll.export(name).unwrap(), &exe, i).unwrap();
            let spots = Spots::read(&garden);
            let entries = spots.pond_entries();
            eprintln!(
                "{name}: {} land, {} pond, {} pond entries",
                spots.land.len(),
                spots.water.len(),
                entries.len()
            );
            assert!(spots.land.len() > 150, "{name}");
            assert!(entries.len() > 10, "{name}");
            for (cell, surface) in &spots.water {
                assert!(
                    (surface - WATER_Y[i]).abs() < 0.3,
                    "{name} {cell:?} {surface}"
                );
            }
            // Land spots are on the ground, not in the pond.
            for s in &spots.land {
                assert!(!in_pond(&spots, *s));
            }
        }
    }
}
