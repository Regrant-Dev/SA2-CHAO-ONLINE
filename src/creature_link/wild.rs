//! Wild visitors (re/chao-wild-visitors.md): creatures of the garden's area for the clock's
//! 10-minute window (`rotation`) come by now and then, picked from the area's wild tables with
//! their level ranges and flatter chances than the source game's (`ways`; the garden's recent
//! species less often, `Recent`): land ones walk on the grass, water ones swim in the pond, and
//! now and then one is shiny (the catalog's shiny colours). Each is an animal object made like a
//! partner's (`scene::create`): a Chao the player hands it to absorbs it, which teaches them its
//! moves, types and ability as a partner does (`Store::learn`). Each visitor knows four moves
//! picked at random from the ones its kind learns by its level (`random_moves`), so two of a kind
//! can teach different ones. Up to 5 are in a garden at once. Visitors are never saved: they go
//! when the player leaves, except one the player carries out, which comes back in their hands in
//! the next garden. A shared online garden has none of them: the server's visitors are shown there
//! (`shared`), and when the garden is the player's alone again they stay as theirs (`adopt`). A
//! visit is kept while the player battles one of its visitors in the karate arena (`snapshot`).

// Off the game (Linux builds) only the tests use the garden side.
#![cfg_attr(not(target_arch = "x86"), allow(dead_code))]

use std::collections::VecDeque;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use super::rotation::{self, Area};
use super::store::Partner;
use crate::creature::rom::Rom;
use crate::creature::world::WildSlot;

/// config.ini `WildVisitors`.
static ENABLED: AtomicBool = AtomicBool::new(true);

pub fn set_enabled(on: bool) {
    ENABLED.store(on, Ordering::Relaxed);
}

pub fn enabled() -> bool {
    ENABLED.load(Ordering::Relaxed)
}

// Used by the game side (and its tests).
#[cfg_attr(not(target_arch = "x86"), allow(unused_imports))]
pub use crate::chao_sim::wild::{
    CALL_REACH, CALL_SECS, GONE_FAR, LOST_Y, MAX_SWIMMING, MAX_VISITORS, MIN_CAP, NEXT_SECS,
    ON_ENTRY, RESERVE, Rng, STAY_SECS, Spots, Swim, WATER_Y, dist2, in_pond, land_spot, middle,
    swim_y, to_pond, walk_toward,
};

/// Nothing comes (nor the area's name) until the garden has been up and not shared this long:
/// a shared garden is only known once friends' states or the server's Chao arrive.
const ALONE_MILLIS: u64 = 2500;

/// How long a garden that isn't shared waits before its visitors come (`ALONE_MILLIS`): online,
/// for friends' states or the server's Chao to say it's shared, unless it was shared earlier in
/// this visit (the server handed it back, or the friends left: it's known to be ours now);
/// offline nobody can turn up.
pub fn alone_wait(online: bool, was_shared: bool) -> Duration {
    if online && !was_shared {
        Duration::from_millis(ALONE_MILLIS)
    } else {
        Duration::ZERO
    }
}
/// One visitor in this many is shiny.
pub const SHINY_ODDS: u64 = 1024;
/// How long the area's name shows when entering the garden.
const TITLE_SECS: u64 = 4;
/// In water-only areas, creatures of this type id come to the grass (they fly).
const FLYING: u8 = 2;
/// A species' chance in its table is this share of the table's chances and the rest the same
/// for each of the table's species, so rare ones come more often than their table says.
const TABLE_PART: f64 = 0.6;
/// A garden's last this many visitors' species are its recent ones (`Recent`).
pub const RECENT: usize = 8;
/// For each time a species is among them, its chance is multiplied by this for the newest,
/// rising evenly toward 1 with age (`Recent::factor`).
const RECENT_NEWEST: f64 = 0.1;

/// Where in its area a visitor is from.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Where {
    /// The land tables (and, in an area without them, the flying ones of the water tables): it
    /// comes to the grass.
    Grass,
    /// The water tables: to the pond.
    Pond,
}

impl Where {
    pub fn label(self) -> &'static str {
        match self {
            Self::Grass => "grass",
            Self::Pond => "pond",
        }
    }
}

/// One slot a visitor can come from and how likely: its share of the visitors that come to the
/// grass, or to the pond (a place's add up to 1).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Way {
    pub slot: WildSlot,
    pub from: Where,
    pub odds: f64,
}

/// The species of a garden's last `RECENT` visitors, the newest last: the more recently and
/// often one came the less often it comes again (`factor`), so the same few don't come back to
/// back.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Recent(VecDeque<u16>);

impl Recent {
    pub const fn new() -> Self {
        Self(VecDeque::new())
    }

    /// What `species`' chance is multiplied by: for each time it's among them,
    /// `RECENT_NEWEST + (1 - RECENT_NEWEST) × age / RECENT` (age 0 the newest).
    pub fn factor(&self, species: u16) -> f64 {
        self.0
            .iter()
            .rev()
            .enumerate()
            .filter(|&(_, &s)| s == species)
            .map(|(age, _)| RECENT_NEWEST + (1.0 - RECENT_NEWEST) * age as f64 / RECENT as f64)
            .product()
    }

    /// `species` came.
    pub fn push(&mut self, species: u16) {
        if self.0.len() >= RECENT {
            self.0.pop_front();
        }
        self.0.push_back(species);
    }
}

/// Each garden's recent species: alone and as the picker of a shared garden alike.
static RECENTS: Mutex<[Recent; 3]> = Mutex::new([Recent::new(), Recent::new(), Recent::new()]);

/// Runs `f` on garden `garden`'s recent species (a throwaway one for no garden).
pub fn with_recent<R>(garden: u8, f: impl FnOnce(&mut Recent) -> R) -> R {
    let mut all = RECENTS.lock().unwrap_or_else(|e| e.into_inner());
    match all.get_mut(usize::from(garden).wrapping_sub(1)) {
        Some(r) => f(r),
        None => f(&mut Recent::new()),
    }
}

/// One wild visitor.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Visitor {
    pub form: u8,
    pub species: u16,
    pub level: u8,
    pub shiny: bool,
    /// Which of its species' two abilities it has.
    pub ability_slot: u8,
    /// Picks its moves (`random_moves`).
    pub seed: u64,
}

impl Visitor {
    /// What a Chao that absorbs it gets: its types, ability and its four moves.
    pub fn partner(&self, rom: &Rom) -> Option<Partner> {
        let key = crate::creature::data::SpeciesKey {
            species: self.species,
            form: self.form,
        };
        let species = rom.species_form(key)?;
        Some(Partner {
            form: self.form,
            species: self.species,
            types: species.types,
            moves: random_moves(&rom.learnset_form(key), self.level, self.seed),
            level: self.level,
            ability: super::ability_of(&species, self.ability_slot),
        })
    }
}

/// Four different moves at random (by `seed`) of the ones `learnset` (level, move) teaches by
/// `level`, in the learnset's order; all of them when there are four or fewer.
pub fn random_moves(learnset: &[(u8, u16)], level: u8, seed: u64) -> [u16; 4] {
    let mut ids: Vec<u16> = Vec::new();
    for &(at, id) in learnset {
        if at <= level && id != 0 && !ids.contains(&id) {
            ids.push(id);
        }
    }
    let mut rng = Rng::new(seed);
    // The first four of a partial shuffle, put back in learnset order.
    let mut order: Vec<usize> = (0..ids.len()).collect();
    for i in 0..order.len().min(4) {
        let j = i + rng.below((order.len() - i) as u64) as usize;
        order.swap(i, j);
    }
    order.truncate(4);
    order.sort_unstable();
    let mut out = [0u16; 4];
    for (slot, i) in out.iter_mut().zip(order) {
        *slot = ids[i];
    }
    out
}

/// The ways of the slots of `tables` that `keep` allows, from `from`, their odds adding up to 1:
/// a table evenly among those with such a slot, then a species by its blended chance
/// (`TABLE_PART`), then one of that species' slots by their chances.
fn table_ways(tables: &[Vec<WildSlot>], keep: &impl Fn(u16) -> bool, from: Where) -> Vec<Way> {
    let kept = |t: &[WildSlot]| -> Vec<WildSlot> {
        t.iter()
            .filter(|s| s.chance > 0 && keep(s.species))
            .copied()
            .collect()
    };
    let tables: Vec<Vec<WildSlot>> = tables
        .iter()
        .map(|t| kept(t))
        .filter(|t| !t.is_empty())
        .collect();
    let mut out = Vec::new();
    for slots in &tables {
        let mut species: Vec<(u16, f64)> = Vec::new();
        for s in slots {
            match species.iter_mut().find(|(id, _)| *id == s.species) {
                Some((_, c)) => *c += f64::from(s.chance),
                None => species.push((s.species, f64::from(s.chance))),
            }
        }
        let total: f64 = species.iter().map(|(_, c)| c).sum();
        let even = (1.0 - TABLE_PART) / species.len() as f64;
        for &s in slots {
            let of = species
                .iter()
                .find(|(id, _)| *id == s.species)
                .map_or(0.0, |(_, c)| *c);
            let blended = TABLE_PART * of / total + even;
            out.push(Way {
                slot: s,
                from,
                odds: blended * f64::from(s.chance) / of / tables.len() as f64,
            });
        }
    }
    out.retain(|w| w.odds > 0.0);
    out
}

/// Every way a visitor can come to the pond (`pond`) or the grass of `area`, with its odds:
/// the land tables to the grass and the water tables to the pond. In areas without land
/// creatures the water ones of type `FLYING` come to the grass and the others to the pond.
/// `types` gives a species' type ids.
pub fn ways(area: &Area, pond: bool, types: impl Fn(u16) -> Option<[u8; 2]>) -> Vec<Way> {
    let flying = |species: u16| types(species).is_some_and(|t| t.contains(&FLYING));
    match (area.land.is_empty(), pond) {
        (false, false) => table_ways(&area.land, &|_| true, Where::Grass),
        (false, true) => table_ways(&area.water, &|_| true, Where::Pond),
        (true, false) => table_ways(&area.water, &flying, Where::Grass),
        (true, true) => table_ways(&area.water, &|s| !flying(s), Where::Pond),
    }
}

/// A number in 0..1 from `rng`.
fn unit(rng: &mut Rng) -> f64 {
    (rng.next() >> 11) as f64 / (1u64 << 53) as f64
}

/// One of `weights`' indices by its weight; none when they're all 0.
fn by_weight(weights: &[f64], rng: &mut Rng) -> Option<usize> {
    let total: f64 = weights.iter().sum();
    if total.is_nan() || total <= 0.0 {
        return None;
    }
    let mut roll = unit(rng) * total;
    for (i, &w) in weights.iter().enumerate() {
        if roll < w {
            return Some(i);
        }
        roll -= w;
    }
    weights.iter().rposition(|&w| w > 0.0)
}

/// A visitor from `area` for the pond (`pond`) or the grass (`ways`), `recent` species less
/// often; it joins them. `types` gives a species' type ids.
pub fn pick(
    area: &Area,
    pond: bool,
    types: impl Fn(u16) -> Option<[u8; 2]>,
    recent: &mut Recent,
    rng: &mut Rng,
) -> Option<Visitor> {
    let ways = ways(area, pond, types);
    let weights: Vec<f64> = ways
        .iter()
        .map(|w| w.odds * recent.factor(w.slot.species))
        .collect();
    let slot = ways[by_weight(&weights, rng)?].slot;
    recent.push(slot.species);
    Some(Visitor {
        form: slot.form,
        species: slot.species,
        level: rng.between(u64::from(slot.min_level), u64::from(slot.max_level)) as u8,
        shiny: rng.below(SHINY_ODDS) == 0,
        ability_slot: rng.below(2) as u8,
        seed: rng.next(),
    })
}

/// One species that can come to an area: where from, its level range, and its odds (of the
/// visitors that come to the grass for `Grass`, or to the pond).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Chance {
    pub species: u16,
    pub from: Where,
    pub min_level: u8,
    pub max_level: u8,
    pub odds: f64,
}

/// Who can come to `area` (the link machine's list): each species by where it's from, with the
/// odds `pick` picks with before its recent species count, by place (grass, then pond), the
/// likeliest first.
pub fn who_comes(area: &Area, types: impl Fn(u16) -> Option<[u8; 2]>) -> Vec<Chance> {
    let mut out: Vec<Chance> = Vec::new();
    for w in ways(area, false, &types)
        .into_iter()
        .chain(ways(area, true, &types))
    {
        let s = w.slot;
        match out
            .iter_mut()
            .find(|c| c.species == s.species && c.from == w.from)
        {
            Some(c) => {
                c.min_level = c.min_level.min(s.min_level);
                c.max_level = c.max_level.max(s.max_level);
                c.odds += w.odds;
            }
            None => out.push(Chance {
                species: s.species,
                from: w.from,
                min_level: s.min_level,
                max_level: s.max_level,
                odds: w.odds,
            }),
        }
    }
    out.sort_by(|a, b| {
        a.from
            .cmp(&b.from)
            .then(b.odds.total_cmp(&a.odds))
            .then(a.species.cmp(&b.species))
    });
    out
}

/// Garden `garden`'s area for clock window `slot` when that's not the one it has (`section`, for
/// window `of`; none on entering): the area, and whether its name shows (on entering, or when
/// it's another area than before). `None` while it has `slot`'s, or there's no area for it.
pub fn next_area(
    areas: &[Area],
    garden: u8,
    section: Option<u8>,
    of: Option<u32>,
    slot: u32,
) -> Option<(&Area, bool)> {
    if of == Some(slot) {
        return None;
    }
    let area = rotation::area_at(areas, garden, slot)?;
    Some((area, section != Some(area.section)))
}

/// One of the player's own visitors where it is: handed over to a garden that turned shared, or
/// kept while the player battles one of them (`snapshot`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Placed {
    pub visitor: Visitor,
    pub partner: Partner,
    pub at: [f32; 3],
    pub angle: u16,
    pub swimming: bool,
}

#[cfg(target_arch = "x86")]
pub use game::{
    adopt, adopt_visit, areas, battle_outcome, carry_out, creatures, give_to_player, is_visitor,
    local_in_hands, on_frame, pending, release, room_for_partner, snapshot, take, take_handover,
    take_pending,
};

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

    use super::super::rotation;
    use super::super::scene::{
        D_MODE, D_POSITION, DELETE_OBJECT, MODE_PLAYER, MODE_ROAM, OBJ_DISPLAY, OBJ_MAIN, create,
        data1, hooked, quietly, room, to_come,
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
    /// The visitors of a visit that turned shared can be handed over to the server this long.
    const HANDOVER: Duration = Duration::from_secs(30);
    /// A visit kept for a battle comes back when the player does within this long.
    const TRIP_LIMIT: Duration = Duration::from_secs(15 * 60);
    /// A visitor brought back after a battle stays at least this long.
    const BACK_FOR: Duration = Duration::from_secs(60);

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

    /// The visit kept while the player battles one of its visitors in the karate arena: it goes
    /// on when they come back to that garden.
    struct Trip {
        garden: u8,
        area: Option<Area>,
        slot: Option<u32>,
        cap: usize,
        came: usize,
        /// Each visitor and how long it has left.
        kept: Vec<(Placed, Duration)>,
        battled: usize,
        /// Whether the battled one stays (none: the battle never ended, as given up).
        stays: Option<bool>,
        since: Instant,
    }

    struct Wild {
        /// The garden the visitors are in (0: none).
        garden: u8,
        area: Option<Area>,
        /// The clock window `area` is for (none: it's to be chosen).
        slot: Option<u32>,
        /// The area's name, the line under it, and until when they show.
        title: Option<(String, String, Instant)>,
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
        /// The garden was shared earlier in this visit.
        was_shared: bool,
        carrying: Option<Carried>,
        pending: Option<Carried>,
        /// The player whistled: visitors come to where they are until then.
        called: Option<(Instant, (f32, f32))>,
        rng: Rng,
        /// The visit's visitors when the garden turned shared: (garden, section, them, when).
        handover: Option<(u8, u8, Vec<Placed>, Instant)>,
        trip: Option<Trip>,
        /// Visitors back from a battle trip, to make again.
        restore: Vec<(Placed, Duration)>,
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
                area: None,
                slot: None,
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
                was_shared: false,
                carrying: None,
                pending: None,
                called: None,
                rng: Rng::new(seed),
                handover: None,
                trip: None,
                restore: Vec::new(),
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

    /// Our live visitors where they are: (object, it).
    fn placed(w: &Wild) -> Vec<(usize, Placed)> {
        let live = live_objects();
        w.live
            .iter()
            .filter(|v| live.contains(&v.object))
            .filter_map(|v| {
                // SAFETY: our live object (in the registry this frame), on the game thread.
                let (at, angle) = unsafe {
                    let d = data1(v.object);
                    if d == 0 {
                        return None;
                    }
                    (
                        ((d + D_POSITION) as *const [f32; 3]).read_unaligned(),
                        ((d + D_ANGLE) as *const i32).read_unaligned(),
                    )
                };
                let placed = Placed {
                    visitor: v.visitor,
                    partner: v.partner,
                    at,
                    angle: angle as u16,
                    swimming: v.swim.is_some(),
                };
                Some((v.object, placed))
            })
            .collect()
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
        log!("creature link: a wild visitor made way for a partner");
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
            if sa2::current_level() != sa2::LEVEL_CHAO_WORLD {
                w.trip = None;
            }
            return;
        }
        if !sa2::in_garden() {
            // The lobby and Chao World's other areas: a carried visitor waits for the next
            // garden, and a note says so (the hands look empty here).
            delete_all(w);
            w.garden = 0;
            if let Some(c) = w.carrying.take() {
                w.pending = Some(c);
            }
            if let Some(c) = w.pending
                && !sa2::between_areas()
                && sa2::game_state() != GAME_STATE_PAUSED
            {
                let name = super::super::loaded_rom()
                    .and_then(|rom| rom.species_name(c.visitor.species))
                    .map(|n| super::super::ascii(&n))
                    .unwrap_or_else(|| "a visitor".into());
                crate::hud::banner(&format!("Carrying {name} to your next garden"));
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
            // A shared online garden: our own visitors go (one in the player's hands goes once
            // let go of; a carried one waits for a garden of their own, unless the garden's
            // shared visitors take it, `shared`). Where they were is kept a while for the
            // server's first pick there.
            w.alone_since = None;
            w.was_shared = true;
            w.title = None;
            w.restore.clear();
            if running(w)
                && let Some(section) = w.area.as_ref().map(|a| a.section)
            {
                let held = crate::garden::player_held();
                let list: Vec<Placed> = placed(w)
                    .into_iter()
                    .filter(|(o, _)| *o != held)
                    .map(|(_, p)| p)
                    .collect();
                if !list.is_empty() {
                    w.handover = Some((garden, section, list, now));
                }
            }
            delete_all(w);
            w.carrying = carried(w);
            return;
        }
        let alone = *w.alone_since.get_or_insert(now);
        let online = crate::online::my_id().is_some();
        if now.duration_since(alone) < alone_wait(online, w.was_shared) {
            return;
        }
        // A new window: its area from now on (the visitors already here stay).
        let slot = rotation::slot_now();
        if w.slot != Some(slot) && !choose_area(w, garden, slot, now) && w.area.is_none() {
            return;
        }
        if let Some((name, line, until)) = &w.title {
            if now < *until {
                crate::hud::top_title(name, line);
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
        if !w.restore.is_empty() {
            come_back(w, garden, now);
        }
        if w.pending.is_some() && crate::garden::player_held() == 0 {
            hand_over(w, garden, now);
        }
        hear_whistle(w, now);
        go_away(w, now);
        come(w, garden, now);
    }

    /// The pause screen's GameState.
    const GAME_STATE_PAUSED: i16 = 17;

    /// Player 1's entity (PLAYERS[0]): its action byte (+0) is 42 while whistling, and its
    /// position at +0x14 (whistle.rs).
    const PLAYERS: usize = 0x01DE_A6C0;
    const ACTION_WHISTLE: u8 = 42;

    /// While the player whistles, visitors are called to them (for `CALL_SECS` after).
    fn hear_whistle(w: &mut Wild, now: Instant) {
        // SAFETY: the game's player table (a static) and player 1's entity, on the game thread.
        let whistling = unsafe {
            let p = (PLAYERS as *const usize).read();
            p != 0 && (p as *const u8).read() == ACTION_WHISTLE
        };
        if whistling && let Some((at, _)) = sa2::player_position() {
            w.called = Some((now + Duration::from_secs(CALL_SECS), (at.x, at.z)));
        } else if w.called.is_some_and(|(until, _)| now >= until) {
            w.called = None;
        }
    }

    /// Walked into garden `garden`: its visitors start anew, or go on as they were when the
    /// player comes back from battling one of them.
    fn enter(w: &mut Wild, garden: u8, now: Instant) {
        delete_all(w);
        w.was_shared = false;
        if let Some(c) = w.carrying.take() {
            w.pending = Some(c);
        }
        w.garden = garden;
        w.restore.clear();
        if let Some(t) = w
            .trip
            .take()
            .filter(|t| t.garden == garden && now.duration_since(t.since) < TRIP_LIMIT)
        {
            let stays = t.stays == Some(true);
            log!(
                "creature link: back from a battle in garden {garden}: its visit goes on{}",
                if stays {
                    ""
                } else {
                    " without the one that ran away"
                }
            );
            w.area = t.area;
            w.slot = t.slot;
            w.cap = t.cap;
            w.came = t.came;
            w.restore = t
                .kept
                .into_iter()
                .enumerate()
                .filter(|&(i, _)| i != t.battled || stays)
                .map(|(_, k)| k)
                .collect();
            w.spots = w
                .known_spots
                .get(usize::from(garden.saturating_sub(1)))
                .cloned()
                .flatten();
            w.spots_retry = None;
            w.next = Some(now + delay(&mut w.rng));
            w.alone_since = None;
            return;
        }
        w.area = None;
        w.slot = None;
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

    /// The garden's area for clock window `slot`, and its name as the title when it's entering
    /// or another area. False while the catalog isn't there (or has no area for this garden).
    fn choose_area(w: &mut Wild, garden: u8, slot: u32, now: Instant) -> bool {
        let (Some(areas), Some(rom)) = (load_areas(w), super::super::loaded_rom()) else {
            return false;
        };
        let section = w.area.as_ref().map(|a| a.section);
        let Some((area, titled)) = next_area(&areas, garden, section, w.slot, slot) else {
            return false;
        };
        if titled {
            log!(
                "creature link: garden {garden}'s wild area is section {} until the next window",
                area.section
            );
            let name = super::super::ascii(&rom.map_section_name(area.section).unwrap_or_default());
            let line = rotation::until_next(rotation::unix_now());
            w.title =
                (!name.is_empty()).then(|| (name, line, now + Duration::from_secs(TITLE_SECS)));
        }
        w.area = Some(area.clone());
        w.slot = Some(slot);
        true
    }

    /// The catalog's wild areas, read once (none while the catalog isn't there).
    fn load_areas(w: &mut Wild) -> Option<Arc<Vec<Area>>> {
        if w.areas.is_none() {
            if w.areas_failed {
                return None;
            }
            let rom = super::super::loaded_rom()?;
            match rotation::areas(&rom) {
                Some(a) => w.areas = Some(Arc::new(a)),
                None => {
                    log!(
                        "creature link: the catalog's wild tables can't be read; no wild visitors"
                    );
                    w.areas_failed = true;
                    return None;
                }
            }
        }
        w.areas.clone()
    }

    /// The catalog's wild areas (`rotation::areas`), read once.
    pub fn areas() -> Option<Arc<Vec<Area>>> {
        with(load_areas)
    }

    /// Where our visitors were when garden `garden` turned shared, with its area's section, if
    /// that was a moment ago: for the server's first pick there. Taken once.
    pub fn take_handover(garden: u8) -> Option<(u8, Vec<Placed>)> {
        with(|w| {
            let (g, section, list, at) = w.handover.take()?;
            (g == garden && at.elapsed() < HANDOVER).then_some((section, list))
        })
    }

    /// The visitor the player carried out of a shared garden waits for the next garden, as one
    /// carried out of their own.
    pub fn carry_out(visitor: Visitor, partner: Partner) {
        with(|w| w.pending = Some(Carried { visitor, partner }));
    }

    /// The visitor the player carries to the next garden, if any.
    pub fn pending() -> Option<(Visitor, Partner)> {
        with(|w| w.pending.map(|c| (c.visitor, c.partner)))
    }

    /// Takes the visitor the player carries (a shared garden has it now).
    pub fn take_pending() {
        with(|w| w.pending = None);
    }

    /// One of our visitors in the player's hands: its object, it and what it teaches.
    pub fn local_in_hands() -> Option<(usize, Visitor, Partner)> {
        with(|w| {
            let held = crate::garden::player_held();
            w.live
                .iter()
                .find(|v| held != 0 && v.object == held)
                .map(|v| (v.object, v.visitor, v.partner))
        })
    }

    /// Visitor `object` is no longer ours (a shared garden's copy now): its own main, it and
    /// what it teaches. The object stays.
    pub fn release(object: usize) -> Option<(usize, Visitor, Partner)> {
        with(|w| {
            let i = w.live.iter().position(|v| v.object == object)?;
            let v = w.live.remove(i);
            Some((v.main, v.visitor, v.partner))
        })
    }

    /// A shared garden's copy `object` (whose own main is `main`) is our visitor from now on
    /// (the garden is the player's own again).
    ///
    /// # Safety
    /// Game thread; `object` is a live animal of ours (`scene::create`).
    pub unsafe fn adopt(object: usize, main: usize, visitor: Visitor, partner: Partner) {
        // SAFETY: as the caller's.
        unsafe {
            ((object + OBJ_MAIN) as *mut usize)
                .write(visitor_main as extern "C" fn(usize) as usize);
            ((object + OBJ_DISPLAY) as *mut usize)
                .write(visitor_display as extern "C" fn(usize) as usize);
        }
        with(|w| {
            let stay = Duration::from_secs(w.rng.between(STAY_SECS.0, STAY_SECS.1));
            w.live.push(Live {
                object,
                visitor,
                partner,
                main,
                born: Instant::now(),
                stay,
                swim: None,
            });
        });
    }

    /// Garden `garden` is the player's own again, its shared visitors from map section `section`
    /// now theirs (`adopt`): the visit goes on, with the clock window's area (its title only when
    /// that's another area than `section`).
    pub fn adopt_visit(garden: u8, section: Option<u8>) {
        with(|w| {
            if w.garden != garden {
                return;
            }
            let area = section.and_then(|s| {
                let areas = load_areas(w)?;
                areas.iter().find(|a| a.section == s).cloned()
            });
            if let Some(area) = area {
                w.area = Some(area);
                w.slot = None;
            }
            w.title = None;
            w.came = w.came.max(ON_ENTRY);
            let now = Instant::now();
            w.next = Some(now + delay(&mut w.rng));
        });
    }

    /// Keeps the visit as it is for a battle with visitor `object` in the karate arena: it
    /// goes on when the player comes back to this garden (`battle_outcome` says whether that
    /// one stays). Whether `object` is one of ours.
    pub fn snapshot(object: usize) -> bool {
        with(|w| {
            if !running(w) {
                return false;
            }
            let now = Instant::now();
            let kept: Vec<(usize, Placed, Duration)> = placed(w)
                .into_iter()
                .filter_map(|(o, p)| {
                    let v = w.live.iter().find(|v| v.object == o)?;
                    let left = v.stay.saturating_sub(now.duration_since(v.born));
                    Some((o, p, left.max(BACK_FOR)))
                })
                .collect();
            let Some(battled) = kept.iter().position(|(o, _, _)| *o == object) else {
                return false;
            };
            w.trip = Some(Trip {
                garden: w.garden,
                area: w.area.clone(),
                slot: w.slot,
                cap: w.cap,
                came: w.came,
                kept: kept.into_iter().map(|(_, p, left)| (p, left)).collect(),
                battled,
                stays: None,
                since: now,
            });
            true
        })
    }

    /// The battle with the visitor kept by `snapshot` is over: whether it stays. Before the
    /// player left the garden the visit simply goes on (one that ran away goes).
    pub fn battle_outcome(stays: bool) {
        with(|w| {
            let here = running(w);
            let Some(t) = w.trip.as_mut() else { return };
            t.stays = Some(stays);
            if !(here && w.garden == t.garden) {
                return;
            }
            let battled = t.kept.get(t.battled).map(|(p, _)| p.visitor);
            w.trip = None;
            if stays {
                return;
            }
            let live = live_objects();
            if let Some(i) = w
                .live
                .iter()
                .position(|v| Some(v.visitor) == battled && live.contains(&v.object))
            {
                let v = w.live.remove(i);
                // SAFETY: our live object; the game deletes it on its next pass.
                unsafe { ((v.object + OBJ_MAIN) as *mut usize).write(DELETE_OBJECT) };
            }
        })
    }

    /// The visitors back from a battle trip come again where they were.
    fn come_back(w: &mut Wild, garden: u8, now: Instant) {
        let coming = to_come(garden) as u32;
        for (p, left) in std::mem::take(&mut w.restore) {
            if !room(RESERVE + coming) {
                break;
            }
            let at = Vector {
                x: p.at[0],
                y: p.at[1] + 0.5,
                z: p.at[2],
            };
            // SAFETY: game thread, in a running garden with room in category 2.
            if let Some(object) =
                unsafe { spawn(w, at, i32::from(p.angle), p.visitor, p.partner, now) }
                && let Some(v) = w.live.iter_mut().find(|v| v.object == object)
            {
                v.stay = left;
            }
        }
    }

    fn read_spots(w: &mut Wild, garden: u8, now: Instant) -> bool {
        if w.spots_retry.is_some_and(|at| now < at) {
            return false;
        }
        match crate::land_share::read(usize::from(garden - 1)) {
            Some(g) => {
                let spots = Spots::read(&g);
                log!(
                    "creature link: garden {garden} has {} land and {} pond spots for visitors",
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
        // SAFETY: our live object, on the game thread.
        unsafe { give_to_player(object) };
        log!(
            "creature link: a visitor (species {}) came along in the player's hands",
            c.visitor.species
        );
        let name = super::super::loaded_rom()
            .and_then(|rom| rom.species_name(c.visitor.species))
            .map(|n| super::super::ascii(&n))
            .unwrap_or_else(|| "Your visitor".into());
        crate::hud::toast(format!("{name} came along"));
    }

    /// Hands `object` to the player (the game's pickup; it checks the hands are empty).
    ///
    /// # Safety
    /// Game thread; `object` is a live animal.
    pub unsafe fn give_to_player(object: usize) {
        // SAFETY: as the caller's.
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
        let rng = &mut w.rng;
        let picked = with_recent(garden, |recent| {
            pick(&area, pond, types, recent, rng)
                .map(|v| (v, pond))
                .or_else(|| {
                    let other = !pond && !entries.is_empty() && swimming < MAX_SWIMMING;
                    pick(&area, other, types, recent, rng).map(|v| (v, other))
                })
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
                "creature link: a wild visitor (species {}, level {}{}) came to the {}",
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
        quietly(|| unsafe { std::mem::transmute::<usize, extern "C" fn(usize)>(main)(object) });
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
        }
    }

    /// A roaming visitor in the pond swims; a held one stops. Called by a whistle, one on land
    /// walks to the player and one in the pond swims to its edge nearest them.
    ///
    /// # Safety
    /// Game thread; `object` is our live visitor with Data1 `d`.
    unsafe fn swim(w: &mut Wild, object: usize, d: usize, roaming: bool) {
        let Wild {
            live,
            spots,
            rng,
            garden,
            called,
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
            let called = called
                .map(|(_, p)| p)
                .filter(|p| dist2((at[0], at[2]), *p) < CALL_REACH * CALL_REACH);
            if v.swim.is_none() && in_pond(spots, at) {
                v.swim = Swim::start(spots, (at[0], at[2]));
            }
            let Some(s) = v.swim.as_mut() else {
                if let Some(((x, z), angle)) = called.and_then(|p| walk_toward((at[0], at[2]), p)) {
                    ((d + D_POSITION) as *mut [f32; 3]).write_unaligned([x, at[1], z]);
                    ((d + D_ANGLE) as *mut i32).write_unaligned(angle);
                    let d2 = ((object + OBJ_DATA2) as *const usize).read();
                    if d2 != 0 {
                        (d2 as *mut [f32; 3]).write_unaligned([0.0; 3]);
                    }
                }
                return;
            };
            let angle = s.step(spots, rng, called);
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

    /// Draws the visitor where the animal is. A panic must not unwind into the game.
    extern "C" fn visitor_display(object: usize) {
        let _ = std::panic::catch_unwind(|| {
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
            let (at, heading, held) = unsafe {
                let d = data1(object);
                if d == 0 {
                    return;
                }
                (
                    ((d + D_POSITION) as *const [f32; 3]).read_unaligned(),
                    ((d + 0x0C) as *const i32).read_unaligned(),
                    (d as *const u8).read() == 1,
                )
            };
            super::super::billboard::draw_form(
                object, v.species, v.form, v.shiny, at, heading, held,
            );
        });
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
                "creature link: a Chao absorbed a wild visitor (species {})",
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

    fn rom() -> Option<crate::creature::rom::Rom> {
        crate::test_cache::rom()
    }

    fn slot(species: u16, chance: u8) -> WildSlot {
        WildSlot {
            form: 0,
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
            also: Vec::new(),
            land: if land {
                vec![vec![slot(10, 90), slot(11, 10)]]
            } else {
                Vec::new()
            },
            water: vec![vec![slot(20, 60), slot(21, 40)]],
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
            // A fresh memory each time: the area's own odds.
            if let Some(v) = pick(area, pond, types, &mut Recent::new(), &mut rng) {
                assert!((3..=7).contains(&v.level));
                assert!(v.ability_slot < 2);
                *out.entry(v.species).or_insert(0) += 1;
            }
        }
        out
    }

    #[test]
    fn a_garden_handed_back_has_its_visitors_at_once() {
        // Just in, online: a moment for friends' states or the server's Chao to come.
        assert_eq!(alone_wait(true, false), Duration::from_millis(ALONE_MILLIS));
        // Shared earlier this visit and ours now (the server handed it back, the friends
        // left), or offline: nothing to wait for.
        assert_eq!(alone_wait(true, true), Duration::ZERO);
        assert_eq!(alone_wait(false, false), Duration::ZERO);
    }

    fn odds_of(ways: &[Way], species: u16) -> f64 {
        ways.iter()
            .filter(|w| w.slot.species == species)
            .map(|w| w.odds)
            .sum()
    }

    fn near(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-9
    }

    #[test]
    fn picks_follow_the_chances() {
        // 90 and 10 in the table: 60% of that and 40% shared evenly, 74 and 26.
        let land = count(&area(true), false, 10_000);
        assert_eq!(land.keys().copied().collect::<Vec<_>>(), [10, 11]);
        assert!((7_100..7_700).contains(&land[&10]), "{land:?}");
        // The pond: the water table, 60 and 40 blended to 56 and 44.
        let pond = count(&area(true), true, 10_000);
        assert_eq!(pond.keys().copied().collect::<Vec<_>>(), [20, 21]);
        assert!((5_300..5_900).contains(&pond[&20]), "{pond:?}");
        // Without land creatures the flying one comes to the grass, the others to the pond.
        let grass = count(&area(false), false, 1_000);
        assert_eq!(grass.keys().copied().collect::<Vec<_>>(), [21]);
        let pond = count(&area(false), true, 1_000);
        assert!(!pond.contains_key(&21) && pond.contains_key(&20));
        // Nothing to pick: none.
        let mut dry = area(true);
        dry.water.clear();
        assert!(pick(&dry, true, types, &mut Recent::new(), &mut Rng::new(1)).is_none());
        assert!(ways(&dry, true, types).is_empty());
    }

    #[test]
    fn rare_ones_come_more_often_than_their_table_says() {
        // Uneven land chances, a species each.
        let chances = [25, 15, 15, 10, 10, 8, 5, 4, 3, 2, 2, 1];
        let mut a = area(true);
        a.land = vec![
            (0..12)
                .map(|i| slot(100 + i, chances[i as usize]))
                .collect(),
        ];
        let ways = ways(&a, false, types);
        let total: f64 = ways.iter().map(|w| w.odds).sum();
        assert!(near(total, 1.0));
        // 0.6 × chance + 0.4 / 12.
        let even = 0.4 / 12.0;
        assert!(near(odds_of(&ways, 100), 0.15 + even));
        assert!(near(odds_of(&ways, 111), 0.006 + even));
        // The rarest come about four times as often (1% to 3.9%), the commonest less often.
        assert!(odds_of(&ways, 111) > 0.038 && odds_of(&ways, 100) < 0.19);
        // A species in several slots counts once for the even part; its slots keep their
        // chances between them (for its levels).
        let mut b = area(true);
        b.land = vec![vec![slot(1, 50), slot(1, 20), slot(2, 30)]];
        let ways = super::ways(&b, false, types);
        assert!(near(odds_of(&ways, 1), 0.6 * 0.7 + 0.2));
        assert!(near(odds_of(&ways, 2), 0.6 * 0.3 + 0.2));
        assert!(near(ways[0].odds / ways[1].odds, 50.0 / 20.0));
        // Two tables (two maps): each half the time.
        let mut c = area(true);
        c.land = vec![vec![slot(1, 100)], vec![slot(2, 50), slot(3, 50)]];
        let ways = super::ways(&c, false, types);
        assert!(near(odds_of(&ways, 1), 0.5) && near(odds_of(&ways, 3), 0.25));
        assert!(ways.iter().all(|w| w.from == Where::Grass));
        // The draws follow the odds.
        let mut rng = Rng::new(21);
        let n = 40_000;
        let first = (0..n)
            .filter(|_| {
                pick(&c, false, types, &mut Recent::new(), &mut rng)
                    .unwrap()
                    .species
                    == 1
            })
            .count();
        assert!((19_400..20_600).contains(&first), "{first}");
    }

    #[test]
    fn an_area_without_land_sends_its_flyers_to_the_grass() {
        // An area without land: the flying one of the water table (and only it) to the grass,
        // the rest to the pond.
        let dry = area(false);
        let grass = super::ways(&dry, false, types);
        assert_eq!(grass.len(), 1);
        assert!(grass[0].slot.species == 21 && near(grass[0].odds, 1.0));
        assert_eq!(grass[0].from, Where::Grass);
        let pond = super::ways(&dry, true, types);
        assert!(pond.len() == 1 && near(odds_of(&pond, 20), 1.0));
        assert_eq!(pond[0].from, Where::Pond);
    }

    #[test]
    fn recent_species_come_less_often() {
        let mut r = Recent::new();
        assert!(near(r.factor(10), 1.0));
        r.push(10);
        r.push(10);
        r.push(11);
        // The newest a tenth as often; older ones less so, and twice is both.
        assert!(near(r.factor(11), 0.1) && near(r.factor(12), 1.0));
        assert!(near(
            r.factor(10),
            (0.1 + 0.9 / 8.0) * (0.1 + 0.9 * 2.0 / 8.0)
        ));
        // Only the last `RECENT` count.
        for _ in 0..RECENT - 1 {
            r.push(12);
        }
        assert!(near(r.factor(10), 1.0));
        assert!(near(r.factor(11), 0.1 + 0.9 * 7.0 / 8.0));
        r.push(12);
        assert!(near(r.factor(11), 1.0) && r.factor(12) < 1e-3);
        // Four species of equal odds: with the memory far fewer repeats back to back, and each
        // still comes.
        let mut a = area(true);
        a.land = vec![(1..=4).map(|s| slot(s, 25)).collect()];
        let run = |remember: bool| {
            let mut rng = Rng::new(8);
            let mut recent = Recent::new();
            let mut seen = std::collections::BTreeMap::new();
            let mut last = 0;
            let mut repeats = 0;
            for _ in 0..4_000 {
                if !remember {
                    recent = Recent::new();
                }
                let v = pick(&a, false, types, &mut recent, &mut rng).unwrap();
                repeats += usize::from(v.species == last);
                last = v.species;
                *seen.entry(v.species).or_insert(0) += 1;
            }
            (repeats, seen)
        };
        let (forgetful, _) = run(false);
        let (remembering, seen) = run(true);
        assert!((850..1_150).contains(&forgetful), "{forgetful}");
        assert!(remembering < 400, "{remembering}");
        assert!(
            seen.values().all(|&n| (900..1_100).contains(&n)),
            "{seen:?}"
        );
        // Only one can come: it comes anyway.
        let mut one = area(true);
        one.land = vec![vec![slot(5, 100)]];
        let mut recent = Recent::new();
        let mut rng = Rng::new(2);
        assert!((0..20).all(|_| pick(&one, false, types, &mut recent, &mut rng).is_some()));
        // Each garden keeps its own.
        with_recent(1, |r| *r = Recent::new());
        with_recent(2, |r| *r = Recent::new());
        with_recent(1, |r| r.push(7));
        assert!(with_recent(1, |r| r.factor(7)) < 1.0);
        assert!(near(with_recent(2, |r| r.factor(7)), 1.0));
        with_recent(9, |r| r.push(7));
        assert!(near(with_recent(9, |r| r.factor(7)), 1.0));
    }

    #[test]
    fn who_comes_lists_each_species_by_place_with_the_picking_odds() {
        let mut a = area(true);
        a.land = vec![vec![slot(10, 90), slot(11, 10)], vec![slot(10, 100)]];
        a.land[1][0].min_level = 9;
        a.land[1][0].max_level = 12;
        let list = who_comes(&a, types);
        let places: Vec<(u16, Where)> = list.iter().map(|c| (c.species, c.from)).collect();
        assert_eq!(
            places,
            [
                (10, Where::Grass),
                (11, Where::Grass),
                (20, Where::Pond),
                (21, Where::Pond),
            ]
        );
        // Species 10 is in both land tables: its levels and odds together.
        assert_eq!((list[0].min_level, list[0].max_level), (3, 12));
        let land = ways(&a, false, types);
        assert!(near(list[0].odds, odds_of(&land, 10)));
        // Each place's odds add up to 1, as the picking's do.
        let grass: f64 = list
            .iter()
            .filter(|c| c.from == Where::Grass)
            .map(|c| c.odds)
            .sum();
        let pond: f64 = list
            .iter()
            .filter(|c| c.from == Where::Pond)
            .map(|c| c.odds)
            .sum();
        assert!(near(grass, 1.0) && near(pond, 1.0));
        assert!(near(list[2].odds, 0.56) && near(list[3].odds, 0.44));
    }

    #[test]
    fn the_area_changes_with_the_window() {
        let areas: Vec<Area> = (1..=4)
            .map(|section| Area {
                section,
                theme: Theme::Neutral,
                ..area(true)
            })
            .collect();
        // Entering: the window's area, named.
        let (a, titled) = next_area(&areas, 1, None, None, 100).unwrap();
        assert!(titled);
        assert_eq!(a, rotation::area_at(&areas, 1, 100).unwrap());
        // The same window: it stays.
        assert!(next_area(&areas, 1, Some(a.section), Some(100), 100).is_none());
        // The next window: another area, named.
        let (b, titled) = next_area(&areas, 1, Some(a.section), Some(100), 101).unwrap();
        assert!(titled && b.section != a.section);
        assert_eq!(b, rotation::area_at(&areas, 1, 101).unwrap());
        // Back from a shared garden whose area is the window's: no name.
        let (c, titled) = next_area(&areas, 1, Some(b.section), None, 101).unwrap();
        assert!(!titled && c == b);
        // One whose area was the last window's: the new one, named.
        let (d, titled) = next_area(&areas, 1, Some(a.section), None, 101).unwrap();
        assert!(titled && d == b);
        // No area of the garden's theme.
        assert!(next_area(&areas, 2, None, None, 100).is_none());
    }

    #[test]
    fn visitors_roll_their_moves() {
        let learnset = [
            (1, 10),
            (1, 11),
            (4, 12),
            (7, 13),
            (9, 11),
            (12, 14),
            (20, 15),
        ];
        // Up to four known by the level: all of them, in order.
        assert_eq!(random_moves(&learnset, 4, 1), [10, 11, 12, 0]);
        assert_eq!(random_moves(&learnset, 9, 1), [10, 11, 12, 13]);
        assert_eq!(random_moves(&[], 9, 1), [0; 4]);
        // More: four different ones from those by the level, the same for the same seed.
        let mut seen = std::collections::BTreeSet::new();
        for seed in 0..200 {
            let m = random_moves(&learnset, 15, seed);
            assert_eq!(m, random_moves(&learnset, 15, seed));
            assert!(m.iter().all(|&id| (10..=14).contains(&id)), "{m:?}");
            for i in 0..4 {
                assert!(!m[..i].contains(&m[i]));
            }
            assert!(m.windows(2).all(|w| w[0] < w[1]), "learnset order: {m:?}");
            seen.insert(m);
        }
        // Each of the five ways to leave one out comes up.
        assert_eq!(seen.len(), 5);
        // Visitors get their own seeds.
        let mut rng = Rng::new(4);
        let mut recent = Recent::new();
        let a = pick(&area(true), false, types, &mut recent, &mut rng).unwrap();
        let b = pick(&area(true), false, types, &mut recent, &mut rng).unwrap();
        assert_ne!(a.seed, b.seed);
    }

    #[test]
    fn shinies_are_rare() {
        let mut rng = Rng::new(99);
        let n = 200_000;
        let shiny = (0..n)
            .filter(|_| {
                pick(&area(true), false, types, &mut Recent::new(), &mut rng)
                    .unwrap()
                    .shiny
            })
            .count();
        let expected = n / SHINY_ODDS as usize;
        assert!(shiny > expected / 2 && shiny < expected * 2, "{shiny}");
    }

    #[test]
    fn catalog_visitors_for_every_garden() {
        let Some(rom) = rom() else { return };
        let areas = rotation::areas(&rom).unwrap();
        let types = |s: u16| rom.species(s).map(|i| i.types);
        let mut rng = Rng::new(11);
        let mut recent = Recent::new();
        for a in &areas {
            let land = pick(a, false, types, &mut recent, &mut rng);
            let pond = pick(a, true, types, &mut recent, &mut rng);
            assert!(land.is_some() || pond.is_some(), "section {}", a.section);
            for v in [land, pond].into_iter().flatten() {
                let p = v.partner(&rom).unwrap();
                assert_eq!(p.species, v.species);
                assert_ne!(p.ability, 0);
                assert_ne!(p.moves[0], 0);
                assert!((1..=100).contains(&p.level));
                // Moves its kind learns by its level.
                let learnset = rom.learnset(v.species);
                for m in p.moves.into_iter().filter(|&m| m != 0) {
                    assert!(learnset.iter().any(|&(at, id)| id == m && at <= v.level));
                }
            }
        }
    }

    #[test]
    fn catalog_who_comes_has_every_species_of_each_area() {
        let Some(rom) = rom() else { return };
        let areas = rotation::areas(&rom).unwrap();
        let types = |s: u16| rom.species(s).map(|i| i.types);
        let mut species: [std::collections::BTreeSet<u16>; 3] = Default::default();
        let mut own: [std::collections::BTreeSet<u16>; 3] = Default::default();
        let mut rarest = 1.0f64;
        for a in &areas {
            let list = who_comes(a, types);
            // Every species of its tables can come (none is left out), and only those.
            let mut listed: Vec<u16> = list.iter().map(|c| c.species).collect();
            listed.sort();
            listed.dedup();
            let mut all: Vec<u16> = a.slots().map(|s| s.species).collect();
            all.sort();
            all.dedup();
            assert_eq!(listed, all, "section {}", a.section);
            for pond in [false, true] {
                let of: Vec<&Chance> = list
                    .iter()
                    .filter(|c| (c.from == Where::Pond) == pond)
                    .collect();
                let total: f64 = of.iter().map(|c| c.odds).sum();
                assert!(of.is_empty() || near(total, 1.0), "{} {pond}", a.section);
            }
            for c in &list {
                assert!(c.min_level <= c.max_level && c.odds > 0.0);
                assert!(a.slots().any(|s| s.species == c.species
                    && (c.min_level..=c.max_level).contains(&s.min_level)));
                rarest = rarest.min(c.odds);
            }
            for g in 0..3 {
                let theme = Theme::of_garden(g as u8 + 1).unwrap();
                if a.fits(theme) {
                    species[g].extend(all.iter().copied());
                }
                if a.theme == theme {
                    own[g].extend(all.iter().copied());
                }
            }
        }
        // Every predicted appearance has a finite positive probability.
        assert!(rarest > 0. && rarest <= 1., "{rarest}");
        // Species per garden (Neutral, Hero, Dark): with the areas it shares, and its own only.
        assert!(
            species
                .iter()
                .zip(&own)
                .all(|(shared, own)| !own.is_empty() && own.is_subset(shared))
        );
        let union: std::collections::BTreeSet<_> = species.into_iter().flatten().collect();
        assert_eq!(union, (1..=807).collect());
    }
}
