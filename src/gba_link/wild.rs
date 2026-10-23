//! Wild visitors (re/chao-wild-visitors.md): creatures of the garden's area of the day
//! (`rotation`) come by now and then, picked from the area's wild tables with their chances and
//! level ranges: land ones walk on the grass, water ones swim in the pond, and now and then one
//! is shiny (the ROM's shiny colours). Each is an animal object made like a partner's
//! (`scene::create`), so Chao notice it, pick it up and absorb it, which teaches them its moves,
//! types and ability as a partner does (`Store::learn`). Visitors are never saved: they go when
//! the player leaves, except one the player carries out, which comes back in their hands in the
//! next garden.

// Off the game (Linux builds) only the tests use the garden side.
#![cfg_attr(not(target_arch = "x86"), allow(dead_code))]

use std::sync::atomic::{AtomicBool, Ordering};

use super::rotation::{Area, mix};
use super::store::Partner;
use crate::chao_sim::land::{GRID_COLS, GRID_ROWS, Garden, Tri, V3, cell_of, probe};
use crate::gba::rom::Rom;
use crate::gba::world::WildSlot;

/// config.ini `WildVisitors`.
static ENABLED: AtomicBool = AtomicBool::new(true);

pub fn set_enabled(on: bool) {
    ENABLED.store(on, Ordering::Relaxed);
}

pub fn enabled() -> bool {
    ENABLED.load(Ordering::Relaxed)
}

/// Most visitors in a garden at once; each visit to a garden allows `MIN_CAP..=MAX_VISITORS`.
pub const MAX_VISITORS: usize = 8;
const MIN_CAP: usize = 6;
/// Category-2 entries left for the game's own animals and drives.
const RESERVE: u32 = 2;
/// Visitors that come at once when the garden loads; the others come one by one.
const ON_ENTRY: usize = 4;
/// Nothing comes (nor the area's name) until the garden has been up and not shared this long:
/// a shared garden is only known once friends' states or the server's Chao arrive.
const ALONE_MILLIS: u64 = 2500;
/// Seconds between visitors, and how long one stays before it goes once out of the way.
const NEXT_SECS: (u64, u64) = (20, 50);
const STAY_SECS: (u64, u64) = (300, 480);
/// A visitor that stayed long enough goes once the player is this far away.
const GONE_FAR: f32 = 60.0;
/// New visitors come at least this far from the player when a spot allows.
const COME_FAR: f32 = 25.0;
/// Of ten visitors, about this many go to the pond (when the area has water creatures).
const POND_IN_TEN: u64 = 3;
const MAX_SWIMMING: usize = 3;
/// One visitor in this many is shiny.
pub const SHINY_ODDS: u64 = 1024;
/// How long the area's name shows when entering the garden.
const TITLE_SECS: u64 = 4;
/// In water-only areas, creatures of this type id come to the grass (they fly).
const FLYING: u8 = 2;
/// Of a land visitor's picks, one in this many is from the rock table, when the area has one.
const ROCKS_ONE_IN: u64 = 4;
/// Of a pond visitor's picks, one in this many is from the fishing tables, when the area has
/// both.
const FISHING_ONE_IN: u64 = 4;
/// The water surface of each garden (Neutral, Hero, Dark) and how far below it a swimmer is.
pub const WATER_Y: [f32; 3] = [-2.0, -1.5, -1.0];
const SWIM_DEPTH: f32 = 1.5;
const BOB: f32 = 0.2;
/// Units a frame.
const SWIM_SPEED: f32 = 0.06;
/// A pond spot has its floor at least this far below the surface.
const POND_DEPTH: f32 = 3.0;
/// Grid values.
const LAND: i32 = 1;
const WATER: i32 = 9;
/// Below this a visitor fell out of the garden.
const LOST_Y: f32 = -50.0;

/// A small random source.
#[derive(Clone, Debug)]
pub struct Rng(u64);

impl Rng {
    pub fn new(seed: u64) -> Self {
        Self(seed)
    }

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

/// One wild visitor.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Visitor {
    pub species: u16,
    pub level: u8,
    pub shiny: bool,
    /// Which of its species' two abilities it has.
    pub ability_slot: u8,
}

impl Visitor {
    /// What a Chao that absorbs it gets: its types, ability and the moves it knows at its level.
    pub fn partner(&self, rom: &Rom) -> Option<Partner> {
        let species = rom.species(self.species)?;
        Some(Partner {
            species: self.species,
            types: species.types,
            moves: rom.moves_at_level(self.species, self.level),
            level: self.level,
            ability: super::ability_of(&species, self.ability_slot),
            from_save: false,
        })
    }
}

/// One slot of `slots` that `keep` allows, by the slots' chances.
fn by_chance(slots: &[WildSlot], keep: &impl Fn(u16) -> bool, rng: &mut Rng) -> Option<WildSlot> {
    let kept: Vec<&WildSlot> = slots.iter().filter(|s| keep(s.species)).collect();
    let total: u64 = kept.iter().map(|s| u64::from(s.chance)).sum();
    if total == 0 {
        return None;
    }
    let mut roll = rng.below(total);
    for s in kept {
        let c = u64::from(s.chance);
        if roll < c {
            return Some(*s);
        }
        roll -= c;
    }
    None
}

/// A slot from one of `tables` (a table at random among those with a slot `keep` allows).
fn from_tables(
    tables: &[&Vec<WildSlot>],
    keep: &impl Fn(u16) -> bool,
    rng: &mut Rng,
) -> Option<WildSlot> {
    let usable: Vec<&&Vec<WildSlot>> = tables
        .iter()
        .filter(|t| t.iter().any(|s| keep(s.species)))
        .collect();
    let table = usable.get(rng.below(usable.len() as u64) as usize)?;
    by_chance(table, keep, rng)
}

/// A visitor from `area` for the pond (`pond`) or the grass. In areas without land creatures the
/// water ones of type `FLYING` come to the grass and the others to the pond. `types` gives a
/// species' type ids.
pub fn pick(
    area: &Area,
    pond: bool,
    types: impl Fn(u16) -> Option<[u8; 2]>,
    rng: &mut Rng,
) -> Option<Visitor> {
    let flying = |species: u16| types(species).is_some_and(|t| t.contains(&FLYING));
    let any = |_: u16| true;
    let slot = if !area.land.is_empty() {
        if pond {
            water_slot(area, &any, rng)?
        } else if !area.rocks.is_empty() && rng.below(ROCKS_ONE_IN) == 0 {
            from_tables(&all(&area.rocks), &any, rng)?
        } else {
            from_tables(&all(&area.land), &any, rng)?
        }
    } else if pond {
        water_slot(area, &|s| !flying(s), rng)?
    } else {
        water_slot(area, &flying, rng)?
    };
    Some(Visitor {
        species: slot.species,
        level: rng.between(u64::from(slot.min_level), u64::from(slot.max_level)) as u8,
        shiny: rng.below(SHINY_ODDS) == 0,
        ability_slot: rng.below(2) as u8,
    })
}

fn all(tables: &[Vec<WildSlot>]) -> Vec<&Vec<WildSlot>> {
    tables.iter().collect()
}

/// From the water tables, or now and then (or when they have none `keep` allows) the fishing
/// ones.
fn water_slot(area: &Area, keep: &impl Fn(u16) -> bool, rng: &mut Rng) -> Option<WildSlot> {
    let (water, fishing) = (all(&area.water), all(&area.fishing));
    let fish_first = rng.below(FISHING_ONE_IN) == 0;
    let (first, second) = if fish_first {
        (&fishing, &water)
    } else {
        (&water, &fishing)
    };
    from_tables(first, keep, rng).or_else(|| from_tables(second, keep, rng))
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
}

/// The solid floor below `q`, through water surfaces.
fn floor_below(tris: &[Tri], q: V3) -> Option<f32> {
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
    /// when it can help it). Returns the heading (BAMS, the game's sin/cos convention).
    pub fn step(&mut self, spots: &Spots, rng: &mut Rng) -> i32 {
        let goal = middle(self.to);
        let (dx, dz) = (goal.0 - self.at.0, goal.1 - self.at.1);
        let d = (dx * dx + dz * dz).sqrt();
        if d <= SWIM_SPEED {
            self.at = goal;
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

fn dist2(a: (f32, f32), b: (f32, f32)) -> f32 {
    (a.0 - b.0).powi(2) + (a.1 - b.1).powi(2)
}

/// The BAMS angle that walks along (dx, dz) as the game does (sin, cos).
fn heading(dx: f32, dz: f32) -> i32 {
    (dx.atan2(dz) * 65536.0 / std::f32::consts::TAU) as i32
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
    let far: Vec<&V3> = spots
        .land
        .iter()
        .filter(|s| dist2((s[0], s[2]), from) >= COME_FAR * COME_FAR)
        .collect();
    let list: Vec<&V3> = if far.is_empty() {
        spots.land.iter().collect()
    } else {
        far
    };
    list.get(rng.below(list.len() as u64) as usize).map(|s| **s)
}

#[cfg(target_arch = "x86")]
pub use game::{creatures, is_visitor, on_frame, room_for_partner, take};

#[cfg(not(target_arch = "x86"))]
pub fn is_visitor(_object: usize) -> bool {
    false
}

#[cfg(not(target_arch = "x86"))]
pub fn room_for_partner(_garden: u8) -> bool {
    true
}

#[cfg(not(target_arch = "x86"))]
pub fn take(_object: usize) -> Option<(usize, Partner)> {
    None
}

#[cfg(not(target_arch = "x86"))]
pub fn on_frame() {}

#[cfg(target_arch = "x86")]
mod game {
    use std::sync::{Arc, Mutex};
    use std::time::{Duration, Instant};

    use super::super::rotation::{self, Area};
    use super::super::scene::{
        D_MODE, D_POSITION, DELETE_OBJECT, MODE_PLAYER, MODE_ROAM, OBJ_MAIN, create, data1,
        enable_touch, hooked, room, to_come,
    };
    use super::super::store::Partner;
    use super::*;
    use crate::game::{self as sa2, Vector};
    use crate::log;

    /// Hands an object to the player (ECX = player index, push object; caller pops 4).
    const GIVE_TO_PLAYER: usize = 0x0046_E5B0;
    /// Animal Data1 +0xC: facing (BAMS); Data2 (obj+0x38) +0: velocity.
    const D_ANGLE: usize = 0x0C;
    const OBJ_DATA2: usize = 0x38;
    /// How long a failed garden read waits before it's tried again.
    const RETRY: Duration = Duration::from_secs(2);

    struct Live {
        object: usize,
        visitor: Visitor,
        partner: Partner,
        /// The animal's own MainSub (ours wraps it).
        main: usize,
        born: Instant,
        stay: Duration,
        swim: Option<Swim>,
    }

    /// A visitor the player carries, kept across the area change.
    #[derive(Clone, Copy)]
    struct Carried {
        visitor: Visitor,
        partner: Partner,
    }

    struct Wild {
        /// The garden the visitors are in (0: none).
        garden: u8,
        /// The day `area` is for (0: not chosen yet).
        day: u32,
        area: Option<Area>,
        title: Option<(String, Instant)>,
        areas: Option<Arc<Vec<Area>>>,
        areas_failed: bool,
        spots: Option<Spots>,
        spots_retry: Option<Instant>,
        /// Each garden's spots once read (they don't change).
        known_spots: [Option<Spots>; 3],
        live: Vec<Live>,
        cap: usize,
        came: usize,
        next: Option<Instant>,
        /// Since when the garden is up and not shared.
        alone_since: Option<Instant>,
        carrying: Option<Carried>,
        pending: Option<Carried>,
        rng: Rng,
    }

    // SAFETY: the object addresses are only used on the game thread.
    unsafe impl Send for Wild {}

    static WILD: Mutex<Option<Wild>> = Mutex::new(None);

    fn with<R>(f: impl FnOnce(&mut Wild) -> R) -> R {
        let mut w = WILD.lock().unwrap_or_else(|e| e.into_inner());
        let w = w.get_or_insert_with(|| {
            let seed = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(1, |d| d.as_nanos() as u64);
            Wild {
                garden: 0,
                day: 0,
                area: None,
                title: None,
                areas: None,
                areas_failed: false,
                spots: None,
                spots_retry: None,
                known_spots: [None, None, None],
                live: Vec::new(),
                cap: MIN_CAP,
                came: 0,
                next: None,
                alone_since: None,
                carrying: None,
                pending: None,
                rng: Rng::new(seed),
            }
        });
        f(w)
    }

    fn live_objects() -> Vec<usize> {
        crate::registry::entries(2)
            .into_iter()
            .map(|(_, o)| o)
            .collect()
    }

    /// Visitors' garden is the one running now.
    fn running(w: &Wild) -> bool {
        sa2::in_garden() && !sa2::between_areas() && sa2::current_chao_area() == i32::from(w.garden)
    }

    /// The player holds visitor `object` (Data1 `d`).
    ///
    /// # Safety
    /// Game thread; `object` is live with Data1 `d` (or 0).
    unsafe fn in_hands(object: usize, d: usize) -> bool {
        // SAFETY: as the caller's.
        object == crate::garden::player_held()
            || (d != 0 && unsafe { ((d + D_MODE) as *const u8).read() } == MODE_PLAYER)
    }

    /// Our visitors go: in their running garden the game deletes them on its next pass, except
    /// one in the player's hands (the game keeps pointing at it), which stays ours until it's
    /// let go of; after an area change they went with the area (and one still running deletes
    /// itself in `visitor_main`), so they're only forgotten.
    fn delete_all(w: &mut Wild) {
        if w.live.is_empty() {
            return;
        }
        if !running(w) {
            w.live.clear();
            return;
        }
        let live = live_objects();
        w.live.retain(|v| {
            if !live.contains(&v.object) {
                return false;
            }
            // SAFETY: our live object (in the registry this frame), on the game thread.
            unsafe {
                if in_hands(v.object, data1(v.object)) {
                    return true;
                }
                ((v.object + OBJ_MAIN) as *mut usize).write(DELETE_OBJECT);
            }
            false
        });
    }

    /// The visitor in the player's hands, if any.
    fn carried(w: &Wild) -> Option<Carried> {
        let held = crate::garden::player_held();
        if held == 0 {
            return None;
        }
        w.live.iter().find(|v| v.object == held).map(|v| Carried {
            visitor: v.visitor,
            partner: v.partner,
        })
    }

    /// One roaming visitor (the farthest from the player) goes, making room. Whether one did.
    fn send_one_away(w: &mut Wild, now: Instant) -> bool {
        if !running(w) {
            return false;
        }
        let Some((player, _)) = sa2::player_position() else {
            return false;
        };
        let live = live_objects();
        let mut farthest: Option<(usize, f32)> = None;
        for v in w.live.iter().filter(|v| live.contains(&v.object)) {
            // SAFETY: our live object (in the registry this frame).
            let (mode, at) = unsafe {
                let d = data1(v.object);
                if d == 0 {
                    continue;
                }
                (
                    ((d + D_MODE) as *const u8).read(),
                    ((d + D_POSITION) as *const [f32; 3]).read_unaligned(),
                )
            };
            let far = (at[0] - player.x).powi(2) + (at[2] - player.z).powi(2);
            if mode == MODE_ROAM && farthest.is_none_or(|(_, f)| far > f) {
                farthest = Some((v.object, far));
            }
        }
        let Some((object, _)) = farthest else {
            return false;
        };
        // SAFETY: as above; the game deletes it on its next pass.
        unsafe { ((object + OBJ_MAIN) as *mut usize).write(DELETE_OBJECT) };
        w.live.retain(|v| v.object != object);
        w.next = Some(now + delay(&mut w.rng));
        log!("gba link: a wild visitor made way for a partner");
        true
    }

    /// Room in garden `garden` for one more partner (from the link machine) besides the ones
    /// still to come there: the reserve stays free while a visitor can make way for it.
    pub fn room_for_partner(garden: u8) -> bool {
        let coming = to_come(garden) as u32;
        room(RESERVE + coming) || with(|w| send_one_away(w, Instant::now())) || room(coming)
    }

    pub fn on_frame() {
        with(frame);
    }

    fn frame(w: &mut Wild) {
        // Without the absorb hook a Chao absorbing a visitor would change the Chao save.
        if !enabled() || !hooked() || sa2::current_level() != sa2::LEVEL_CHAO_WORLD {
            delete_all(w);
            if w.live.is_empty() {
                // (One still in the player's hands stays this garden's until it's let go of.)
                w.garden = 0;
            }
            w.carrying = None;
            w.pending = None;
            return;
        }
        if !sa2::in_garden() {
            // The lobby and Chao World's other areas: a carried visitor waits for the next
            // garden.
            delete_all(w);
            w.garden = 0;
            if let Some(c) = w.carrying.take() {
                w.pending = Some(c);
            }
            return;
        }
        if sa2::between_areas() {
            w.alone_since = None;
            return;
        }
        let garden = sa2::current_chao_area() as u8;
        let now = Instant::now();
        if w.garden != garden {
            enter(w, garden, now);
        }
        if crate::garden::server_runs() || crate::bring::friends_in(i32::from(garden)) {
            // A shared online garden: no visitors there (one in the player's hands goes once
            // let go of; a carried one waits for a garden of their own).
            w.alone_since = None;
            w.title = None;
            delete_all(w);
            w.carrying = carried(w);
            return;
        }
        let alone = *w.alone_since.get_or_insert(now);
        if now.duration_since(alone) < Duration::from_millis(ALONE_MILLIS) {
            return;
        }
        let day = super::super::today();
        if w.day != day && !choose_area(w, garden, day, now) {
            return;
        }
        if let Some((name, until)) = &w.title {
            if now < *until {
                crate::hud::center2(name, "Wild visitors come from here today");
            } else {
                w.title = None;
            }
        }
        if w.spots.is_none() && !read_spots(w, garden, now) {
            return;
        }
        let live = live_objects();
        w.live.retain(|v| live.contains(&v.object));
        w.carrying = carried(w);
        if w.pending.is_some() && crate::garden::player_held() == 0 {
            hand_over(w, garden, now);
        }
        go_away(w, now);
        come(w, garden, now);
    }

    /// Walked into garden `garden`: its visitors start anew.
    fn enter(w: &mut Wild, garden: u8, now: Instant) {
        delete_all(w);
        if let Some(c) = w.carrying.take() {
            w.pending = Some(c);
        }
        w.garden = garden;
        w.day = 0;
        w.spots = w
            .known_spots
            .get(usize::from(garden.saturating_sub(1)))
            .cloned()
            .flatten();
        w.spots_retry = None;
        w.cap = w.rng.between(MIN_CAP as u64, MAX_VISITORS as u64) as usize;
        w.came = 0;
        w.next = Some(now);
        w.alone_since = None;
    }

    /// The garden's area for `day`, and its name as the title. False while the ROM isn't
    /// there (or has no area for this garden).
    fn choose_area(w: &mut Wild, garden: u8, day: u32, now: Instant) -> bool {
        if w.areas.is_none() {
            if w.areas_failed {
                return false;
            }
            let Some(rom) = super::super::loaded_rom() else {
                return false;
            };
            match rotation::areas(&rom) {
                Some(a) => w.areas = Some(Arc::new(a)),
                None => {
                    log!("gba link: the ROM's wild tables can't be read; no wild visitors");
                    w.areas_failed = true;
                    return false;
                }
            }
        }
        let (Some(areas), Some(rom)) = (w.areas.clone(), super::super::loaded_rom()) else {
            return false;
        };
        let Some(area) = rotation::area_of_day(&areas, garden, day) else {
            return false;
        };
        let name = super::super::ascii(&rom.map_section_name(area.section).unwrap_or_default());
        log!(
            "gba link: garden {garden}'s wild area today is section {}",
            area.section
        );
        w.title = (!name.is_empty()).then(|| (name, now + Duration::from_secs(TITLE_SECS)));
        w.area = Some(area.clone());
        w.day = day;
        true
    }

    fn read_spots(w: &mut Wild, garden: u8, now: Instant) -> bool {
        if w.spots_retry.is_some_and(|at| now < at) {
            return false;
        }
        match crate::land_share::read(usize::from(garden - 1)) {
            Some(g) => {
                let spots = Spots::read(&g);
                log!(
                    "gba link: garden {garden} has {} land and {} pond spots for visitors",
                    spots.land.len(),
                    spots.water.len()
                );
                if let Some(known) = w.known_spots.get_mut(usize::from(garden - 1)) {
                    *known = Some(spots.clone());
                }
                w.spots = Some(spots);
                true
            }
            None => {
                w.spots_retry = Some(now + RETRY);
                false
            }
        }
    }

    /// The visitor the player carried out comes back in their hands, in front of them (another
    /// visitor makes way when the garden is full; it stays to come until it's made).
    fn hand_over(w: &mut Wild, garden: u8, now: Instant) {
        let Some((player, angle)) = sa2::player_position() else {
            return;
        };
        let coming = to_come(garden) as u32;
        if !room(RESERVE + coming) && send_one_away(w, now) {
            // Its entry is free after the game's next pass.
            return;
        }
        if !room(coming) {
            return;
        }
        let Some(c) = w.pending else {
            return;
        };
        let a = angle as f32 * std::f32::consts::TAU / 65536.0;
        let (s, co) = a.sin_cos();
        let at = Vector {
            x: player.x + 3.0 * s,
            y: player.y,
            z: player.z + 3.0 * co,
        };
        // SAFETY: game thread, in a running garden with room in category 2.
        let Some(object) = (unsafe { spawn(w, at, angle, c.visitor, c.partner, now) }) else {
            return;
        };
        w.pending = None;
        // SAFETY: the player's pickup with our live object (it checks the hands are empty).
        unsafe {
            std::arch::asm!(
                "push {object}",
                "call {f}",
                "add esp, 4",
                object = in(reg) object,
                f = in(reg) GIVE_TO_PLAYER,
                inout("ecx") 0u32 => _,
                clobber_abi("C"),
            );
        }
        log!(
            "gba link: a visitor (species {}) came along in the player's hands",
            c.visitor.species
        );
    }

    /// Visitors that stayed long enough go once the player is away from them.
    fn go_away(w: &mut Wild, now: Instant) {
        let Some((player, _)) = sa2::player_position() else {
            return;
        };
        let mut gone = Vec::new();
        for v in &w.live {
            // SAFETY: our live object (checked against the registry this frame).
            let (mode, at) = unsafe {
                let d = data1(v.object);
                if d == 0 {
                    continue;
                }
                (
                    ((d + D_MODE) as *const u8).read(),
                    ((d + D_POSITION) as *const [f32; 3]).read_unaligned(),
                )
            };
            let far = (at[0] - player.x).powi(2) + (at[2] - player.z).powi(2) > GONE_FAR * GONE_FAR;
            let old = now.duration_since(v.born) > v.stay;
            if at[1] < LOST_Y || (mode == MODE_ROAM && old && far) {
                gone.push(v.object);
            }
        }
        for object in gone {
            // SAFETY: as above.
            unsafe { ((object + OBJ_MAIN) as *mut usize).write(DELETE_OBJECT) };
            w.live.retain(|v| v.object != object);
            w.next = Some(now + delay(&mut w.rng));
        }
    }

    fn delay(rng: &mut Rng) -> Duration {
        Duration::from_secs(rng.between(NEXT_SECS.0, NEXT_SECS.1))
    }

    /// A new visitor when it's time and there's room (besides the reserve and the partners
    /// still to come).
    fn come(w: &mut Wild, garden: u8, now: Instant) {
        if w.next.is_none_or(|at| now < at) {
            return;
        }
        if w.live.len() >= w.cap || !room(RESERVE + to_come(garden) as u32) {
            w.next = Some(now + delay(&mut w.rng));
            return;
        }
        let (Some(area), Some(spots), Some(rom)) =
            (w.area.clone(), w.spots.clone(), super::super::loaded_rom())
        else {
            return;
        };
        let Some((player, _)) = sa2::player_position() else {
            return;
        };
        let swimming = w.live.iter().filter(|v| v.swim.is_some()).count();
        let entries = spots.pond_entries();
        let pond = to_pond(&mut w.rng, swimming, !entries.is_empty());
        let types = |s: u16| rom.species(s).map(|i| i.types);
        let picked = pick(&area, pond, types, &mut w.rng)
            .map(|v| (v, pond))
            .or_else(|| {
                let other = !pond && !entries.is_empty() && swimming < MAX_SWIMMING;
                pick(&area, other, types, &mut w.rng).map(|v| (v, other))
            });
        w.came += 1;
        w.next = Some(if w.came < ON_ENTRY {
            now
        } else {
            now + delay(&mut w.rng)
        });
        let Some((visitor, pond)) = picked else {
            return;
        };
        let Some(partner) = visitor.partner(&rom) else {
            return;
        };
        let at = if pond {
            let cell = entries[w.rng.below(entries.len() as u64) as usize];
            let (x, z) = middle(cell);
            let surface = spots.water_at(cell).unwrap_or(0.0);
            [x, swim_y(surface, 0.0), z]
        } else {
            let Some(s) = land_spot(&spots, (player.x, player.z), &mut w.rng) else {
                return;
            };
            [s[0], s[1] + 1.0, s[2]]
        };
        let angle = w.rng.below(0x10000) as i32;
        let at = Vector {
            x: at[0],
            y: at[1],
            z: at[2],
        };
        // SAFETY: game thread, in a running garden with room in category 2.
        if unsafe { spawn(w, at, angle, visitor, partner, now) }.is_some() {
            log!(
                "gba link: a wild visitor (species {}, level {}{}) came to the {}",
                visitor.species,
                visitor.level,
                if visitor.shiny { ", shiny" } else { "" },
                if pond { "pond" } else { "grass" }
            );
        }
    }

    /// # Safety
    /// Game thread, in a running garden with room in category 2.
    unsafe fn spawn(
        w: &mut Wild,
        at: Vector,
        angle: i32,
        visitor: Visitor,
        partner: Partner,
        now: Instant,
    ) -> Option<usize> {
        // SAFETY: as the caller's.
        let (object, main) = unsafe { create(at, angle, [0.0; 3], visitor_main, visitor_display)? };
        let stay = Duration::from_secs(w.rng.between(STAY_SECS.0, STAY_SECS.1));
        w.live.push(Live {
            object,
            visitor,
            partner,
            main,
            born: now,
            stay,
            swim: None,
        });
        Some(object)
    }

    /// The animal's own main, then: in the pond it swims (at the surface, along the pond's
    /// cells); roaming with the player's hands empty, Chao can notice it.
    extern "C" fn visitor_main(object: usize) {
        let main = with(|w| w.live.iter().find(|v| v.object == object).map(|v| v.main));
        let Some(main) = main else {
            // Not ours any more: let the game delete it.
            // SAFETY: the object the game runs.
            unsafe { ((object + OBJ_MAIN) as *mut usize).write(DELETE_OBJECT) };
            return;
        };
        // SAFETY: the animal's own main with its object, on the game thread.
        unsafe { std::mem::transmute::<usize, extern "C" fn(usize)>(main)(object) };
        // SAFETY: as above; the main may have deleted it (MainSub changed, mode 3).
        unsafe {
            if ((object + OBJ_MAIN) as *const usize).read()
                != visitor_main as extern "C" fn(usize) as usize
            {
                return;
            }
            let d = data1(object);
            if d == 0 {
                return;
            }
            let roaming = ((d + D_MODE) as *const u8).read() == MODE_ROAM;
            with(|w| swim(w, object, d, roaming));
            if roaming && crate::garden::player_held() == 0 {
                enable_touch(object);
            }
        }
    }

    /// A roaming visitor in the pond swims; a held one stops.
    ///
    /// # Safety
    /// Game thread; `object` is our live visitor with Data1 `d`.
    unsafe fn swim(w: &mut Wild, object: usize, d: usize, roaming: bool) {
        let Wild {
            live,
            spots,
            rng,
            garden,
            ..
        } = w;
        let (Some(v), Some(spots)) = (live.iter_mut().find(|v| v.object == object), spots) else {
            return;
        };
        if !roaming {
            v.swim = None;
            return;
        }
        // SAFETY: as the caller's.
        unsafe {
            let at = ((d + D_POSITION) as *const [f32; 3]).read_unaligned();
            if v.swim.is_none() && in_pond(spots, at) {
                v.swim = Swim::start(spots, (at[0], at[2]));
            }
            let Some(s) = v.swim.as_mut() else {
                return;
            };
            let angle = s.step(spots, rng);
            let surface = spots
                .water_at(s.to)
                .unwrap_or(WATER_Y[usize::from(garden.saturating_sub(1)).min(2)]);
            let y = swim_y(surface, v.born.elapsed().as_secs_f32());
            ((d + D_POSITION) as *mut [f32; 3]).write_unaligned([s.at.0, y, s.at.1]);
            ((d + D_ANGLE) as *mut i32).write_unaligned(angle);
            let d2 = ((object + OBJ_DATA2) as *const usize).read();
            if d2 != 0 {
                (d2 as *mut [f32; 3]).write_unaligned([0.0; 3]);
            }
        }
    }

    /// Draws the visitor's picture where the animal is.
    extern "C" fn visitor_display(object: usize) {
        let visitor = with(|w| {
            w.live
                .iter()
                .find(|v| v.object == object)
                .map(|v| v.visitor)
        });
        let Some(v) = visitor else {
            return;
        };
        // SAFETY: the object the game draws, on the game thread.
        let (at, held) = unsafe {
            let d = data1(object);
            if d == 0 {
                return;
            }
            (
                ((d + D_POSITION) as *const [f32; 3]).read_unaligned(),
                (d as *const u8).read() == 1,
            )
        };
        super::super::billboard::draw_picture(v.species, v.shiny, at, held);
    }

    /// Our visitors: (object, position, what it teaches, shiny), for their info card.
    pub fn creatures() -> Vec<(usize, [f32; 3], Partner, bool)> {
        with(|w| {
            w.live
                .iter()
                .filter_map(|v| {
                    // SAFETY: our live visitor's Data1, on the game thread.
                    let at = unsafe {
                        let d = data1(v.object);
                        if d == 0 {
                            return None;
                        }
                        ((d + D_POSITION) as *const [f32; 3]).read_unaligned()
                    };
                    Some((v.object, at, v.partner, v.visitor.shiny))
                })
                .collect()
        })
    }

    /// One of our visitors' objects.
    pub fn is_visitor(object: usize) -> bool {
        with(|w| w.live.iter().any(|v| v.object == object))
    }

    /// A Chao absorbs visitor `object`: it's no longer ours. Its own main (to put back) and
    /// what it teaches.
    pub fn take(object: usize) -> Option<(usize, Partner)> {
        with(|w| {
            let i = w.live.iter().position(|v| v.object == object)?;
            let v = w.live.remove(i);
            log!(
                "gba link: a Chao absorbed a wild visitor (species {})",
                v.visitor.species
            );
            w.next = Some(Instant::now() + delay(&mut w.rng));
            Some((v.main, v.partner))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::super::rotation::{self, Theme};
    use super::*;
    use crate::chao_sim::land::{LAND_EXPORT, PeFile};
    use std::path::Path;

    fn rom() -> Option<Rom> {
        let home = std::env::var_os("HOME")?;
        let path = Path::new(&home).join("Games/gba/emerald.gba");
        if !path.exists() {
            println!("skipped: no GBA ROM at {}", path.display());
            return None;
        }
        Some(Rom::open(&path).expect("ROM opens"))
    }

    fn slot(species: u16, chance: u8) -> WildSlot {
        WildSlot {
            species,
            min_level: 3,
            max_level: 7,
            chance,
        }
    }

    fn area(land: bool) -> Area {
        Area {
            section: 1,
            theme: Theme::Hero,
            land: if land {
                vec![vec![slot(10, 90), slot(11, 10)]]
            } else {
                Vec::new()
            },
            water: vec![vec![slot(20, 60), slot(21, 40)]],
            rocks: Vec::new(),
            fishing: vec![vec![slot(30, 100)]],
        }
    }

    /// Species 21 flies; the others don't.
    fn types(species: u16) -> Option<[u8; 2]> {
        Some(if species == 21 {
            [11, FLYING]
        } else {
            [11, 11]
        })
    }

    fn count(area: &Area, pond: bool, n: usize) -> std::collections::BTreeMap<u16, usize> {
        let mut rng = Rng::new(7);
        let mut out = std::collections::BTreeMap::new();
        for _ in 0..n {
            if let Some(v) = pick(area, pond, types, &mut rng) {
                assert!((3..=7).contains(&v.level));
                assert!(v.ability_slot < 2);
                *out.entry(v.species).or_insert(0) += 1;
            }
        }
        out
    }

    #[test]
    fn picks_follow_the_chances() {
        let land = count(&area(true), false, 10_000);
        assert_eq!(land.keys().copied().collect::<Vec<_>>(), [10, 11]);
        assert!((8_700..9_300).contains(&land[&10]), "{land:?}");
        // The pond: the water table, and about a quarter of the time fishing.
        let pond = count(&area(true), true, 10_000);
        assert!((2_100..2_900).contains(&pond[&30]), "{pond:?}");
        assert!(pond[&20] > pond[&21]);
        // Without land creatures the flying one comes to the grass, the others to the pond.
        let grass = count(&area(false), false, 1_000);
        assert_eq!(grass.keys().copied().collect::<Vec<_>>(), [21]);
        let pond = count(&area(false), true, 1_000);
        assert!(!pond.contains_key(&21) && pond.contains_key(&20));
        // Nothing to pick: none.
        let mut dry = area(true);
        dry.water.clear();
        dry.fishing.clear();
        assert!(pick(&dry, true, types, &mut Rng::new(1)).is_none());
    }

    #[test]
    fn shinies_are_rare() {
        let mut rng = Rng::new(99);
        let n = 200_000;
        let shiny = (0..n)
            .filter(|_| pick(&area(true), false, types, &mut rng).unwrap().shiny)
            .count();
        let expected = n / SHINY_ODDS as usize;
        assert!(shiny > expected / 2 && shiny < expected * 2, "{shiny}");
    }

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
            swim.step(&spots, &mut rng);
            assert!((lo.0 - 0.01..=hi.0 + 0.01).contains(&swim.at.0));
            assert!((lo.1 - 0.01..=hi.1 + 0.01).contains(&swim.at.1));
        }
        // Heading follows the game's (sin, cos).
        assert_eq!(heading(0.0, 1.0), 0);
        assert_eq!(heading(1.0, 0.0), 0x4000);
        let y = swim_y(-2.0, 0.0);
        assert!((y + 3.5).abs() < 1e-4);
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
        // No pond cells: never the pond; three swimming: never again.
        assert!((0..100).all(|_| !to_pond(&mut rng, 0, false)));
        assert!((0..100).all(|_| !to_pond(&mut rng, MAX_SWIMMING, true)));
        assert!((0..100).any(|_| to_pond(&mut rng, 0, true)));
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

    #[test]
    fn rom_visitors_for_every_garden() {
        let Some(rom) = rom() else { return };
        let areas = rotation::areas(&rom).unwrap();
        let types = |s: u16| rom.species(s).map(|i| i.types);
        let mut rng = Rng::new(11);
        for a in &areas {
            let land = pick(a, false, types, &mut rng);
            let pond = pick(a, true, types, &mut rng);
            assert!(land.is_some() || pond.is_some(), "section {}", a.section);
            for v in [land, pond].into_iter().flatten() {
                let p = v.partner(&rom).unwrap();
                assert_eq!(p.species, v.species);
                assert_ne!(p.ability, 0);
                assert_ne!(p.moves[0], 0);
                assert!((1..=100).contains(&p.level));
            }
        }
    }
}
