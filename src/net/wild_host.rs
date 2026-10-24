//! The host's wild visitors in the gardens it runs (re/shared-visitors.md).
//!
//! While the server runs a garden (chao_host.rs), its visitors are the same for everyone there:
//! the host keeps the garden's list and moves them (roaming the grass, swimming in the pond,
//! called by a whistle, in a player's hands), on the ground players sent (`Spots`). The host
//! never has the ROM: one player in the garden, the picker (the one there longest with a ROM
//! and WildVisitors on), picks the garden's area and its creatures from their ROM when asked
//! (`WildAsk`, `WildPick`); every game shows the list (`WildList`) as copies drawn from its own
//! ROM, and only games whose ROM has the same visitor print (`WildHello`) are sent it.
//!
//! Picking a visitor up, carrying it, putting it down, carrying one in from elsewhere and
//! bringing a partner from the link machine go through here: the first claim wins. A Chao that
//! absorbs one (`WildAbsorbed`) takes it from everyone; what it learns goes to the Chao's owner
//! (`WildTaught`) only when their `ChaoLock` lets the player who handed it over change it (the
//! owner always can). A player battling a visitor reserves it (`WildBattle`) until the battle's
//! result: won, it stays; lost, it runs away. A garden whose run ends while one of its visitors
//! is battled keeps the battled ones (the others are the remaining player's own from then on)
//! for a run on the same ground, a while after the battle too.

use std::collections::HashMap;
use std::time::{Duration, Instant};

use super::protocol::{
    Body, MAX_WILD_LIST, MAX_WILD_PICK, WILD_MACHINE, WILD_PICK, WILD_SEE, WildArrive,
    WildCreature, WildEntry, WildOrigin, WildPicked, WildPlace, WildRequest, WildStep,
};
use super::referee::{Out, To};
use crate::chao_sim::land::{Blocker, Garden, Shape, V3};
use crate::chao_sim::wild::{
    ANIMAL_LIMIT, CALL_REACH, CALL_SECS, GONE_FAR, LOST_Y, MAX_VISITORS, MIN_CAP, NEXT_SECS,
    ON_ENTRY, RESERVE, Rng, Roam, STAY_SECS, Spots, Swim, dist2, floor_below, in_pond,
    land_spot_away, middle, to_pond, walk_toward,
};

/// The sender id the host's messages carry.
use super::protocol::SERVER;

/// The list goes out this often (frames).
const LIST_EVERY: u64 = 6;
/// A player's state counts for a whistle this long after it's heard.
const PLAYER_STALE: Duration = Duration::from_millis(1500);
/// The whistling action (`PlayerState::action`).
const WHISTLE: u8 = 42;
/// A held visitor with no `WildHeld` this long is put down at its holder.
const HELD_LIMIT: Duration = Duration::from_secs(2);
/// A visitor let go of this long ago may still be on its way into a Chao's hands.
const ABSORB_GRACE: Duration = Duration::from_secs(2);
/// A player takes or battles a visitor from this far at most; a Chao absorbs one from this far.
const REACH: f32 = 30.0;
const ABSORB_REACH: f32 = 25.0;
/// A let-go or put position farther than this from the holder is taken as the holder's.
const PUT_REACH: f32 = 40.0;
/// A placed visitor (a fresh pick, a partner at the machine) stays where it is when a land spot
/// or pond cell is this near.
const KEEP_NEAR: f32 = 15.0;
/// A battle reservation lasts this long at most (then it counts as given up).
const BATTLE_LIMIT: Duration = Duration::from_secs(15 * 60);
/// A run that ended mid-battle keeps its battled visitors this long after the battle, for a run
/// on the same ground (the battler coming back).
const KEEP_FROZEN: Duration = Duration::from_secs(3 * 60);
/// An ask unanswered this long is dropped.
const ASK_LIMIT: Duration = Duration::from_secs(5);
/// The first ask of a run may hand over the picker's own visitors this long after it starts.
const HANDOVER: Duration = Duration::from_secs(10);
/// Land spots this near the link machine are left out.
const MACHINE_CLEAR: f32 = 8.0;
/// A roaming visitor keeps this far from players (their bodies push it aside).
const PLAYER_ROOM: f32 = 3.0;
/// Partners of one player in one garden at once (as many as wait there, `store`).
const PARTNERS_EACH: usize = 4;
/// Where a visitor just taken is, in the holder's frame, until their first `WildHeld`.
const TAKEN_OFFSET: V3 = [0.0, 5.0, 2.0];

/// A player as the host knows them.
#[derive(Clone, Copy, Debug)]
pub struct Seen {
    pub id: u8,
    /// The garden they're in (1..=3; 0 none), and since when.
    pub area: u8,
    pub since: Instant,
    /// Loading on from a garden: not settled anywhere yet.
    pub loading: bool,
    pub position: V3,
    pub rotation_y: i32,
    pub action: u8,
    /// When their state was last heard.
    pub heard: Instant,
}

/// A Chao in a garden the host runs.
#[derive(Clone, Copy, Debug)]
pub struct ChaoSeen {
    pub position: V3,
    /// Not a sleeping held one (those keep off items).
    pub awake: bool,
}

/// An owner id with Chao in a garden: a player there, or one the host holds Chao under.
#[derive(Clone, Debug)]
pub struct OwnerSeen {
    pub owner: u8,
    pub awake: bool,
    /// The player ids its owner is online as.
    pub sessions: Vec<u8>,
}

/// What the visitors need of the host's gardens (`ChaoHost`).
pub trait Gardens {
    fn players(&self) -> Vec<Seen>;
    /// Garden `area`'s ground while the server runs it, and its fingerprint.
    fn ground(&self, area: u8) -> Option<(u32, &Garden)>;
    /// The animals and drives (category-2 items) in garden `area`.
    fn animals(&self, area: u8) -> usize;
    /// Chao (`owner`, `index`) in the run of garden `area`.
    fn chao(&self, area: u8, key: (u8, u8)) -> Option<ChaoSeen>;
    /// Chao (`owner`, `index`)'s identity and the player ids its owner is online as.
    fn owner(&self, key: (u8, u8)) -> Option<(u64, Vec<u8>)>;
    /// Nothing player `from` does to Chao (`owner`, `index`) may reach its owner.
    fn locked_against(&self, from: u8, key: (u8, u8)) -> bool;
    /// The owners of garden `area`'s Chao.
    fn owners(&self, area: u8) -> Vec<OwnerSeen>;
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Origin {
    Wild,
    Partner {
        bringer: u8,
        key: u64,
        from_save: bool,
    },
}

#[derive(Clone, Copy, Debug)]
enum Place {
    Roam(Roam),
    Swim(Swim),
    Held {
        by: u8,
        offset: V3,
        heard: Instant,
        /// Let go of, falling: where its holder's game has it.
        falling: Option<V3>,
        /// The holder's last place in the garden.
        last: V3,
    },
    /// Reserved for player `by`'s battle; swimming before (it swims on after).
    Battle {
        by: u8,
        swim: Option<Swim>,
    },
}

impl Place {
    /// Free again after a battle, where it was.
    fn after_battle(self, rng: &mut Rng) -> Self {
        match self {
            Self::Battle { swim: Some(s), .. } => Self::Swim(s),
            Self::Battle { swim: None, .. } => Self::Roam(Roam::new(rng)),
            other => other,
        }
    }
}

#[derive(Clone, Copy, Debug)]
struct Visitor {
    id: u16,
    creature: WildCreature,
    origin: Origin,
    at: V3,
    angle: u16,
    born: Instant,
    stay: Duration,
    place: Place,
    /// Called by this whistling player until then.
    called: Option<(u8, Instant)>,
    /// Who let go of it last, and when.
    let_go: Option<(u8, Instant)>,
}

impl Visitor {
    fn free(&self) -> bool {
        matches!(self.place, Place::Roam(_) | Place::Swim(_))
    }

    fn held_by(&self) -> Option<u8> {
        match self.place {
            Place::Held { by, .. } => Some(by),
            _ => None,
        }
    }
}

/// An ask out to a picker.
#[derive(Clone, Copy, Debug)]
struct Ask {
    serial: u16,
    to: u8,
    section: Option<u8>,
    land: u8,
    pond: u8,
    fresh: bool,
    at: Instant,
}

/// One garden's visitors for a run of it.
struct Visit {
    fingerprint: u32,
    spots: Spots,
    machine: Option<([f32; 3], i32)>,
    /// The print every visitor in the list is of (0 while it's empty).
    print: u32,
    /// The garden's wild area this run, and the print it was chosen with.
    section: Option<(u8, u32)>,
    cap: usize,
    came: usize,
    started: Instant,
    next: Instant,
    ask: Option<Ask>,
    list: Vec<Visitor>,
    /// The run ended while one is battled: the battled ones kept for a run on the same ground.
    frozen: bool,
    /// Frozen with no battle left since then.
    kept: Option<Instant>,
}

impl Visit {
    fn wild(&self) -> usize {
        self.list
            .iter()
            .filter(|v| v.origin == Origin::Wild)
            .count()
    }

    /// How many more fit: the game's animal entries less its own and the garden's items.
    fn room(&self, animals: usize) -> usize {
        let used = animals + RESERVE as usize + self.list.len();
        (ANIMAL_LIMIT as usize)
            .saturating_sub(used)
            .min(MAX_WILD_LIST.saturating_sub(self.list.len()))
    }

    fn print_fits(&self, print: u32) -> bool {
        print != 0 && (self.print == 0 || self.print == print)
    }

    /// The print of the list's visitors (an empty list: its area's).
    fn list_print(&self) -> u32 {
        if self.list.is_empty() {
            self.section.map_or(0, |(_, p)| p)
        } else {
            self.print
        }
    }
}

/// A battle reservation (one a player).
#[derive(Clone, Copy, Debug)]
struct Reserved {
    area: u8,
    id: u16,
    since: Instant,
}

pub struct WildHost {
    /// Each player's `WildHello` (flags, print).
    hello: HashMap<u8, (u8, u32)>,
    gardens: [Option<Visit>; 3],
    battles: HashMap<u8, Reserved>,
    /// Each player's last carry-in: the `since` of the garden entry it was made in.
    carried: HashMap<u8, Instant>,
    ids: u16,
    /// The serial of the last list sent.
    lists: u16,
    asks: u16,
    rng: Rng,
    frames: u64,
    /// Answers decided between frames, sent with the next.
    out: Vec<Out>,
}

impl Default for WildHost {
    fn default() -> Self {
        Self {
            hello: HashMap::new(),
            gardens: [None, None, None],
            battles: HashMap::new(),
            carried: HashMap::new(),
            ids: 0,
            lists: 0,
            asks: 0,
            rng: Rng::new(0x5A2C_57AD),
            frames: 0,
            out: Vec::new(),
        }
    }
}

/// Where a visitor goes once on the ground at `at`: swimming in a pond cell under the surface,
/// otherwise on the floor (or the nearest land spot when there's none under it).
fn settle(spots: &Spots, garden: Option<&Garden>, at: V3, rng: &mut Rng) -> (V3, Place) {
    let floor = garden.and_then(|g| floor_below(&g.tris, [at[0], at[1] + 5.0, at[2]]));
    if let Some(y) = floor {
        let on = [at[0], y, at[2]];
        if in_pond(spots, on)
            && let Some(swim) = Swim::start(spots, (at[0], at[2]))
        {
            let surface = spots.water_at(swim.to).unwrap_or(at[1]);
            return ([at[0], surface, at[2]], Place::Swim(swim));
        }
        return (on, Place::Roam(Roam::new(rng)));
    }
    let at = spots.land_near((at[0], at[2]), f32::MAX).unwrap_or(at);
    (at, Place::Roam(Roam::new(rng)))
}

/// Where the server puts a new visitor (`arrive`): on the grass away from `players`, at a pond
/// entry, or (placed) where it is when a spot is near.
fn arrive_at(
    spots: &Spots,
    garden: &Garden,
    arrive: WildArrive,
    players: &[(f32, f32)],
    rng: &mut Rng,
) -> Option<(V3, u16, Place)> {
    let angle = rng.below(0x10000) as u16;
    let land = |rng: &mut Rng| {
        let s = land_spot_away(spots, players, rng)?;
        Some((s, angle, Place::Roam(Roam::new(rng))))
    };
    let pond = |rng: &mut Rng| {
        let entries = spots.pond_entries();
        let cell = *entries.get(rng.below(entries.len() as u64) as usize)?;
        let (x, z) = middle(cell);
        let swim = Swim::start(spots, (x, z))?;
        let surface = spots.water_at(cell)?;
        Some(([x, surface, z], angle, Place::Swim(swim)))
    };
    match arrive {
        WildArrive::Land => land(rng),
        WildArrive::Pond => pond(rng).or_else(|| land(rng)),
        WildArrive::LandAt { at, angle } => match spots.land_near((at[0], at[2]), KEEP_NEAR) {
            Some(spot) => {
                let y = floor_below(&garden.tris, [at[0], at[1] + 5.0, at[2]])
                    .filter(|y| (y - spot[1]).abs() < 10.0)
                    .unwrap_or(spot[1]);
                Some(([at[0], y, at[2]], angle, Place::Roam(Roam::new(rng))))
            }
            None => land(rng),
        },
        WildArrive::SwimAt { at, angle } => match spots.pond_near((at[0], at[2]), KEEP_NEAR) {
            Some(cell) => {
                let swim = Swim::start(spots, (at[0], at[2]))?;
                let surface = spots.water_at(cell)?;
                Some(([at[0], surface, at[2]], angle, Place::Swim(swim)))
            }
            None => pond(rng).or_else(|| land(rng)),
        },
    }
}

/// The machine's spot in garden `area`'s ground, as every game finds it (`machine::find_spot`,
/// or the fixed one).
fn machine_spot(area: u8, garden: &Garden) -> Option<([f32; 3], i32)> {
    use crate::gba_link::machine;
    let toys: Vec<[f32; 3]> = garden.toys.iter().map(|t| t.pos).collect();
    machine::find_spot(area, &garden.tris, &garden.tree_spots, &toys).or_else(|| {
        machine::SPOTS
            .get(usize::from(area).checked_sub(1)?)
            .copied()
    })
}

fn distance(a: V3, b: V3) -> f32 {
    dist2((a[0], a[2]), (b[0], b[2])).sqrt()
}

/// Where something held at `offset` (in the holder's frame) is.
fn in_hands(pos: V3, rotation_y: i32, offset: V3) -> V3 {
    let a = rotation_y as f32 * std::f32::consts::TAU / 65536.0;
    let (s, c) = (a.sin(), a.cos());
    [
        pos[0] + offset[0] * c + offset[2] * s,
        pos[1] + offset[1],
        pos[2] - offset[0] * s + offset[2] * c,
    ]
}

/// `at`, or `from` when `at` is farther than `PUT_REACH` from it.
fn near(at: V3, from: V3) -> V3 {
    let d = (0..3)
        .map(|i| (at[i] - from[i]).powi(2))
        .sum::<f32>()
        .sqrt();
    if d <= PUT_REACH { at } else { from }
}

impl WildHost {
    fn delay(&mut self) -> Duration {
        Duration::from_secs(self.rng.between(NEXT_SECS.0, NEXT_SECS.1))
    }

    fn stay(&mut self) -> Duration {
        Duration::from_secs(self.rng.between(STAY_SECS.0, STAY_SECS.1))
    }

    /// A new visitor id: never 0, never one in use.
    fn new_id(&mut self) -> u16 {
        loop {
            self.ids = self.ids.wrapping_add(1).max(1);
            let id = self.ids;
            if !self
                .gardens
                .iter()
                .flatten()
                .any(|g| g.list.iter().any(|v| v.id == id))
            {
                return id;
            }
        }
    }

    /// Whether player `id` sees visitors, and their print.
    fn sees(&self, id: u8) -> Option<u32> {
        self.hello
            .get(&id)
            .filter(|(flags, _)| flags & WILD_SEE != 0)
            .map(|&(_, print)| print)
    }

    fn flags(&self, id: u8) -> u8 {
        self.hello.get(&id).map_or(0, |h| h.0)
    }

    /// The print garden `index`'s visitors are of, or are to be: its list's, or while that has
    /// none its picker's (0: no picker either).
    fn garden_print(&self, index: usize, players: &[Seen]) -> u32 {
        let Some(g) = self.gardens[index].as_ref() else {
            return 0;
        };
        match g.list_print() {
            0 => self
                .picker(index, players)
                .and_then(|p| self.sees(p))
                .unwrap_or(0),
            p => p,
        }
    }

    /// The garden (1..=3) and place in its list of visitor `id`, in a garden being run.
    fn find(&self, id: u16) -> Option<(u8, usize)> {
        self.gardens.iter().enumerate().find_map(|(i, g)| {
            let g = g.as_ref().filter(|g| !g.frozen)?;
            let at = g.list.iter().position(|v| v.id == id)?;
            Some((i as u8 + 1, at))
        })
    }

    fn answer(&mut self, to: u8, kind: WildRequest, id: u16, key: u64, ok: bool) {
        self.out.push(Out {
            to: To::One(to),
            from: SERVER,
            body: Body::WildAnswer { kind, id, key, ok },
        });
    }

    /// Visitor `at` of garden `area` goes for everyone (its reservation with it).
    fn remove(&mut self, area: u8, at: usize) -> Option<Visitor> {
        let g = self
            .gardens
            .get_mut(usize::from(area).checked_sub(1)?)?
            .as_mut()?;
        if at >= g.list.len() {
            return None;
        }
        let v = g.list.remove(at);
        if g.list.is_empty() {
            g.print = 0;
        }
        self.battles.retain(|_, r| r.id != v.id);
        Some(v)
    }

    /// Sees player `from`'s message for the visitors.
    pub fn observe(&mut self, from: u8, body: &Body, now: Instant, gardens: &impl Gardens) {
        let players = gardens.players();
        let player = players.iter().find(|p| p.id == from).copied();
        match *body {
            Body::WildHello { flags, print } => {
                let before = self.sees(from);
                self.hello.insert(from, (flags, print));
                if before.is_some() && self.sees(from) != before {
                    self.out_of_sight(from, player, gardens);
                }
            }
            Body::WildPick {
                area,
                ask,
                section,
                ref list,
            } => self.picked(from, area, ask, section, list, now, &players, gardens),
            Body::WildBring {
                area,
                key,
                from_save,
                creature,
                at,
            } => {
                let ok = self.bring(
                    from, player, area, key, from_save, creature, at, now, gardens,
                );
                let id = ok.unwrap_or(0);
                self.answer(from, WildRequest::Bring, id, key, ok.is_some());
            }
            Body::WildCarry {
                area,
                creature,
                offset,
            } => {
                let ok = self.carry(from, player, area, creature, offset, now, gardens);
                self.answer(from, WildRequest::Carry, ok.unwrap_or(0), 0, ok.is_some());
            }
            Body::WildTake { id } => {
                let ok = self.take(from, player, id, now);
                self.answer(from, WildRequest::Take, id, 0, ok);
            }
            Body::WildHeld { id, let_go, v } => {
                if let Some((area, at)) = self.find(id)
                    && let Some(g) = self.gardens[usize::from(area) - 1].as_mut()
                    && let Place::Held {
                        by,
                        offset,
                        heard,
                        falling,
                        last,
                    } = &mut g.list[at].place
                    && *by == from
                {
                    *heard = now;
                    if let_go {
                        *falling = Some(near(v, *last));
                    } else {
                        *offset = v;
                        *falling = None;
                    }
                }
            }
            Body::WildPut { id, at, angle } => {
                if let Some((area, i)) = self.find(id) {
                    let ground = gardens.ground(area).map(|(_, g)| g);
                    let g = self.gardens[usize::from(area) - 1].as_mut();
                    if let Some(g) = g
                        && let Place::Held { by, last, .. } = g.list[i].place
                        && by == from
                    {
                        let (pos, place) = settle(&g.spots, ground, near(at, last), &mut self.rng);
                        let v = &mut g.list[i];
                        (v.at, v.place, v.angle) = (pos, place, angle);
                        v.let_go = Some((from, now));
                        v.called = None;
                    }
                }
            }
            Body::WildAbsorbed { id, owner, index } => {
                self.absorbed(from, id, (owner, index), now, gardens);
            }
            Body::WildBattle { id, step } => self.battle(from, player, id, step, now),
            _ => {}
        }
    }

    /// Player `from` no longer sees visitors as they did (switched off, another ROM): theirs
    /// go, and one in their hands is put down.
    fn out_of_sight(&mut self, from: u8, player: Option<Seen>, gardens: &impl Gardens) {
        for (i, g) in self.gardens.iter_mut().enumerate() {
            let Some(g) = g.as_mut() else { continue };
            let ground = gardens.ground(i as u8 + 1).map(|(_, g)| g);
            for v in &mut g.list {
                if let Place::Held { by, last, .. } = v.place
                    && by == from
                {
                    let at = player.map_or(last, |p| p.position);
                    (v.at, v.place) = settle(&g.spots, ground, at, &mut self.rng);
                }
            }
            let before = g.list.len();
            g.list.retain(
                |v| !matches!(v.origin, Origin::Partner { bringer, .. } if bringer == from),
            );
            if g.list.len() != before && g.list.is_empty() {
                g.print = 0;
            }
        }
        self.forget_battles();
    }

    #[allow(clippy::too_many_arguments)]
    fn picked(
        &mut self,
        from: u8,
        area: u8,
        serial: u16,
        section: u8,
        list: &[WildPicked],
        now: Instant,
        players: &[Seen],
        gardens: &impl Gardens,
    ) {
        let index = usize::from(area) - 1;
        let print = self.sees(from);
        let delay = self.delay();
        let stay: Vec<Duration> = (0..list.len()).map(|_| self.stay()).collect();
        let ids: Vec<u16> = (0..list.len()).map(|_| self.new_id()).collect();
        let (Some(g), Some((_, ground))) = (self.gardens[index].as_mut(), gardens.ground(area))
        else {
            return;
        };
        let Some(ask) = g
            .ask
            .filter(|a| a.serial == serial && a.to == from && !g.frozen)
        else {
            return;
        };
        g.ask = None;
        g.next = now + delay;
        let Some(print) = print.filter(|&p| g.print_fits(p)) else {
            return;
        };
        let count = |f: fn(&WildArrive) -> bool| list.iter().filter(|p| f(&p.arrive)).count();
        let land = count(|a| matches!(a, WildArrive::Land));
        let pond = count(|a| matches!(a, WildArrive::Pond));
        let placed = list.len() - land - pond;
        let fits = land <= usize::from(ask.land)
            && pond <= usize::from(ask.pond)
            && ask.section.is_none_or(|s| s == section)
            && if ask.fresh {
                list.len() <= MAX_VISITORS
            } else {
                placed == 0
            };
        if !fits {
            crate::log!("wild host: player {from}'s pick for garden {area} didn't fit its ask");
            return;
        }
        g.section = Some((section, print));
        g.came += list.len().max(1);
        let around: Vec<(f32, f32)> = players
            .iter()
            .filter(|p| p.area == area)
            .map(|p| (p.position[0], p.position[2]))
            .collect();
        let animals = gardens.animals(area);
        for ((p, stay), id) in list.iter().zip(stay).zip(ids) {
            if g.wild() >= g.cap.max(placed.min(MAX_VISITORS)) || g.room(animals) == 0 {
                break;
            }
            let Some((at, angle, place)) =
                arrive_at(&g.spots, ground, p.arrive, &around, &mut self.rng)
            else {
                continue;
            };
            g.print = print;
            g.list.push(Visitor {
                id,
                creature: p.creature,
                origin: Origin::Wild,
                at,
                angle,
                born: now,
                stay,
                place,
                called: None,
                let_go: None,
            });
        }
    }

    /// Player `from` brings a partner: its id, or none when refused.
    #[allow(clippy::too_many_arguments)]
    fn bring(
        &mut self,
        from: u8,
        player: Option<Seen>,
        area: u8,
        key: u64,
        from_save: bool,
        creature: WildCreature,
        at: Option<([f32; 3], u16)>,
        now: Instant,
        gardens: &impl Gardens,
    ) -> Option<u16> {
        let p = player.filter(|p| p.area == area)?;
        let print = self.sees(from)?;
        if self.flags(from) & WILD_MACHINE == 0 {
            return None;
        }
        let (_, ground) = gardens.ground(area)?;
        let index = usize::from(area) - 1;
        let everyone = gardens.players();
        let expected = self.garden_print(index, &everyone);
        if expected != 0 && expected != print {
            return None;
        }
        let g = self.gardens[index].as_ref().filter(|g| !g.frozen)?;
        let theirs =
            |v: &&Visitor| matches!(v.origin, Origin::Partner { bringer, .. } if bringer == from);
        let mine: Vec<&Visitor> = g.list.iter().filter(theirs).collect();
        if !g.print_fits(print)
            || mine.len() >= PARTNERS_EACH
            || mine
                .iter()
                .any(|v| matches!(v.origin, Origin::Partner { key: k, .. } if k == key))
        {
            return None;
        }
        if g.room(gardens.animals(area)) == 0 && !self.send_one_away(area, gardens) {
            return None;
        }
        let id = self.new_id();
        let players: Vec<(f32, f32)> = everyone
            .iter()
            .filter(|s| s.area == area)
            .map(|s| (s.position[0], s.position[2]))
            .collect();
        let g = self.gardens[index].as_mut()?;
        let by_machine = |v: [f32; 3]| g.machine.is_some_and(|(m, _)| distance(v, m) <= KEEP_NEAR);
        // At the machine when the floor is there (just under where the game has it).
        let floor = at.filter(|&(v, _)| by_machine(v)).and_then(|(v, angle)| {
            let y = floor_below(&ground.tris, [v[0], v[1] + 5.0, v[2]])
                .filter(|y| (y - v[1]).abs() < 10.0)?;
            Some(([v[0], y, v[2]], angle))
        });
        let placed = match floor {
            Some((at, angle)) => Some((at, angle, Place::Roam(Roam::new(&mut self.rng)))),
            None => arrive_at(&g.spots, ground, WildArrive::Land, &players, &mut self.rng),
        };
        let (at, angle, place) = placed.or_else(|| {
            let (at, place) = settle(&g.spots, Some(ground), p.position, &mut self.rng);
            Some((at, 0, place))
        })?;
        g.print = print;
        g.list.push(Visitor {
            id,
            creature,
            origin: Origin::Partner {
                bringer: from,
                key,
                from_save,
            },
            at,
            angle,
            born: now,
            stay: Duration::MAX,
            place,
            called: None,
            let_go: None,
        });
        crate::log!("wild host: player {from} brought a partner (visitor {id}) to garden {area}");
        Some(id)
    }

    /// The roaming wild visitor farthest from every player in garden `area` goes, making room.
    fn send_one_away(&mut self, area: u8, gardens: &impl Gardens) -> bool {
        let players: Vec<V3> = gardens
            .players()
            .iter()
            .filter(|p| p.area == area)
            .map(|p| p.position)
            .collect();
        let Some(g) = self.gardens[usize::from(area) - 1].as_ref() else {
            return false;
        };
        let nearest = |v: &Visitor| {
            players
                .iter()
                .map(|&p| distance(p, v.at))
                .fold(f32::MAX, f32::min)
        };
        let farthest = g
            .list
            .iter()
            .enumerate()
            .filter(|(_, v)| v.origin == Origin::Wild && matches!(v.place, Place::Roam(_)))
            .max_by(|a, b| nearest(a.1).total_cmp(&nearest(b.1)))
            .map(|(i, _)| i);
        let Some(i) = farthest else { return false };
        self.remove(area, i);
        crate::log!("wild host: a visitor in garden {area} made way for a partner");
        true
    }

    #[allow(clippy::too_many_arguments)]
    fn carry(
        &mut self,
        from: u8,
        player: Option<Seen>,
        area: u8,
        creature: WildCreature,
        offset: V3,
        now: Instant,
        gardens: &impl Gardens,
    ) -> Option<u16> {
        let p = player.filter(|p| p.area == area)?;
        let print = self.sees(from)?;
        gardens.ground(area)?;
        let index = usize::from(area) - 1;
        let holding = self
            .gardens
            .iter()
            .flatten()
            .any(|g| g.list.iter().any(|v| v.held_by() == Some(from)));
        // One a garden entry: it came from where they were before.
        let again = self.carried.get(&from) == Some(&p.since);
        let expected = self.garden_print(index, &gardens.players());
        let g = self.gardens[index].as_ref().filter(|g| !g.frozen)?;
        if holding
            || again
            || (expected != 0 && expected != print)
            || !g.print_fits(print)
            || g.wild() >= MAX_VISITORS
            || g.room(gardens.animals(area)) == 0
        {
            return None;
        }
        self.carried.insert(from, p.since);
        let (id, stay) = (self.new_id(), self.stay());
        let g = self.gardens[usize::from(area) - 1].as_mut()?;
        g.print = print;
        g.list.push(Visitor {
            id,
            creature,
            origin: Origin::Wild,
            at: in_hands(p.position, p.rotation_y, offset),
            angle: p.rotation_y as u16,
            born: now,
            stay,
            place: Place::Held {
                by: from,
                offset,
                heard: now,
                falling: None,
                last: p.position,
            },
            called: None,
            let_go: None,
        });
        crate::log!("wild host: player {from} carried a visitor ({id}) into garden {area}");
        Some(id)
    }

    fn take(&mut self, from: u8, player: Option<Seen>, id: u16, now: Instant) -> bool {
        let Some((area, i)) = self.find(id) else {
            return false;
        };
        let print = self.sees(from);
        let holding = self
            .gardens
            .iter()
            .flatten()
            .any(|g| g.list.iter().any(|v| v.held_by() == Some(from)));
        let Some(g) = self.gardens[usize::from(area) - 1].as_mut() else {
            return false;
        };
        let Some(p) = player.filter(|p| p.area == area) else {
            return false;
        };
        let v = &mut g.list[i];
        if holding || !v.free() || print != Some(g.print) || distance(p.position, v.at) > REACH {
            return false;
        }
        v.place = Place::Held {
            by: from,
            offset: TAKEN_OFFSET,
            heard: now,
            falling: None,
            last: p.position,
        };
        v.called = None;
        true
    }

    fn absorbed(&mut self, from: u8, id: u16, key: (u8, u8), now: Instant, gardens: &impl Gardens) {
        let Some((area, i)) = self.find(id) else {
            return;
        };
        let Some(g) = self.gardens[usize::from(area) - 1].as_ref() else {
            return;
        };
        let v = g.list[i];
        // Theirs in hands, or let go of a moment ago and nobody's since.
        let theirs = v.held_by() == Some(from)
            || (v.free()
                && v.let_go
                    .is_some_and(|(by, at)| by == from && now.duration_since(at) < ABSORB_GRACE));
        let reached = gardens
            .chao(area, key)
            .is_some_and(|c| c.awake && distance(c.position, v.at) <= ABSORB_REACH);
        if !theirs || !reached {
            return;
        }
        let print = g.print;
        self.remove(area, i);
        let delay = self.delay();
        if let Some(g) = self.gardens[usize::from(area) - 1].as_mut() {
            g.next = g.next.max(now + delay);
        }
        crate::log!(
            "wild host: Chao {key:?} absorbed visitor {id} (species {}) from player {from} in garden {area}",
            v.creature.species
        );
        let origin = match v.origin {
            Origin::Wild => WildOrigin::Wild,
            Origin::Partner {
                bringer,
                key: k,
                from_save,
                ..
            } => {
                self.out.push(Out {
                    to: To::One(bringer),
                    from: SERVER,
                    body: Body::WildUsed {
                        garden: area,
                        key: k,
                    },
                });
                if from_save {
                    WildOrigin::SavePartner
                } else {
                    WildOrigin::Partner
                }
            }
        };
        if gardens.locked_against(from, key) {
            return;
        }
        let Some((identity, sessions)) = gardens.owner(key) else {
            return;
        };
        for s in sessions {
            if self.sees(s) == Some(print) {
                self.out.push(Out {
                    to: To::One(s),
                    from: SERVER,
                    body: Body::WildTaught {
                        identity,
                        origin,
                        creature: v.creature,
                    },
                });
            }
        }
    }

    fn battle(&mut self, from: u8, player: Option<Seen>, id: u16, step: WildStep, now: Instant) {
        if step == WildStep::Reserve {
            let ok = self.reserve(from, player, id, now);
            self.answer(from, WildRequest::Battle, id, 0, ok);
            return;
        }
        let Some(r) = self.battles.get(&from).copied().filter(|r| r.id == id) else {
            return;
        };
        self.battles.remove(&from);
        let index = usize::from(r.area) - 1;
        let Some(g) = self.gardens[index].as_mut() else {
            return;
        };
        let Some(i) = g.list.iter().position(|v| v.id == id) else {
            return;
        };
        match step {
            WildStep::Lost => {
                crate::log!("wild host: visitor {id} ran away from player {from}'s battle");
                self.remove(r.area, i);
            }
            _ => {
                let v = &mut g.list[i];
                v.place = v.place.after_battle(&mut self.rng);
            }
        }
    }

    fn reserve(&mut self, from: u8, player: Option<Seen>, id: u16, now: Instant) -> bool {
        if self.battles.contains_key(&from) {
            return false;
        }
        let Some((area, i)) = self.find(id) else {
            return false;
        };
        let print = self.sees(from);
        let Some(g) = self.gardens[usize::from(area) - 1].as_mut() else {
            return false;
        };
        let Some(p) = player.filter(|p| p.area == area) else {
            return false;
        };
        let v = &mut g.list[i];
        if v.origin != Origin::Wild
            || !v.free()
            || print != Some(g.print)
            || distance(p.position, v.at) > REACH
        {
            return false;
        }
        let swim = match v.place {
            Place::Swim(s) => Some(s),
            _ => None,
        };
        v.place = Place::Battle { by: from, swim };
        v.called = None;
        self.battles.insert(
            from,
            Reserved {
                area,
                id,
                since: now,
            },
        );
        crate::log!("wild host: player {from} battles visitor {id} in garden {area}");
        true
    }

    /// Player `id` left: what they held is put down where they last were, a visitor they
    /// battle is free again, and their partners go.
    pub fn left(&mut self, id: u8, gardens: &impl Gardens) {
        self.hello.remove(&id);
        self.battles.remove(&id);
        self.carried.remove(&id);
        for (i, g) in self.gardens.iter_mut().enumerate() {
            let Some(g) = g.as_mut() else { continue };
            let ground = gardens.ground(i as u8 + 1).map(|(_, g)| g);
            for v in &mut g.list {
                match v.place {
                    Place::Held { by, last, .. } if by == id => {
                        (v.at, v.place) = settle(&g.spots, ground, last, &mut self.rng);
                    }
                    Place::Battle { by, .. } if by == id => {
                        v.place = v.place.after_battle(&mut self.rng);
                    }
                    _ => {}
                }
            }
            g.list
                .retain(|v| !matches!(v.origin, Origin::Partner { bringer, .. } if bringer == id));
            if g.list.is_empty() {
                g.print = 0;
            }
            if g.ask.is_some_and(|a| a.to == id) {
                g.ask = None;
            }
        }
        self.forget_battles();
    }

    /// Reservations of visitors no longer in any list go.
    fn forget_battles(&mut self) {
        let gardens = &self.gardens;
        self.battles.retain(|_, r| {
            gardens[usize::from(r.area) - 1]
                .as_ref()
                .is_some_and(|g| g.list.iter().any(|v| v.id == r.id))
        });
    }

    /// The link machine's box in garden `area`, for the server's Chao to walk round: while its
    /// visitors are shared and someone there has the machine.
    pub fn machine_blocker(&self, area: u8, players: &[Seen]) -> Option<Blocker> {
        let g = self
            .gardens
            .get(usize::from(area).checked_sub(1)?)?
            .as_ref()?;
        let wanted = players.iter().any(|p| {
            p.area == area
                && self.flags(p.id) & (WILD_SEE | WILD_MACHINE) == WILD_SEE | WILD_MACHINE
        });
        if !wanted || g.frozen {
            return None;
        }
        let (at, angle) = g.machine?;
        let (min, max) = crate::gba_link::machine::machine_box(area)?;
        let mid = std::array::from_fn(|i| (min[i] + max[i]) / 2.0);
        Some(Blocker {
            shape: Shape::Box {
                half: std::array::from_fn(|i| (max[i] - min[i]) / 2.0),
            },
            pos: crate::gba_link::machine::world(at, angle, mid),
            axis: [0.0, 1.0, 0.0],
            rot_y: angle as u16 as i16,
        })
    }

    /// One frame (60 a second) of every garden's visitors; what to send goes to `out`.
    pub fn frame(&mut self, now: Instant, gardens: &impl Gardens, out: &mut Vec<Out>) {
        self.frames += 1;
        out.append(&mut self.out);
        let players = gardens.players();
        // Battles that went on too long count as given up.
        let over: Vec<(u8, Reserved)> = self
            .battles
            .iter()
            .filter(|(_, r)| now.duration_since(r.since) > BATTLE_LIMIT)
            .map(|(&p, &r)| (p, r))
            .collect();
        for (p, r) in over {
            crate::log!(
                "wild host: player {p}'s battle with visitor {} ran out of time",
                r.id
            );
            self.battles.remove(&p);
            if let Some(i) = self.gardens[usize::from(r.area) - 1]
                .as_ref()
                .and_then(|g| g.list.iter().position(|v| v.id == r.id))
            {
                self.remove(r.area, i);
            }
        }
        for index in 0..3 {
            let area = index as u8 + 1;
            if !self.keep(index, now, gardens) {
                continue;
            }
            let Some((_, ground)) = gardens.ground(area) else {
                continue;
            };
            self.hands(index, &players, ground, now);
            self.calls(index, &players, now);
            self.step(index, &players, ground);
            self.leave(index, &players, now);
            self.ask(index, &players, now, gardens, out);
            if self.frames.is_multiple_of(LIST_EVERY) {
                self.send(index, &players, gardens, out);
            }
        }
    }

    /// Garden `index`'s visit for its run, made, kept, frozen or dropped. Whether it's on.
    fn keep(&mut self, index: usize, now: Instant, gardens: &impl Gardens) -> bool {
        let area = index as u8 + 1;
        let battled = |g: &Visit| {
            g.list
                .iter()
                .any(|v| matches!(v.place, Place::Battle { .. }))
        };
        match (gardens.ground(area), self.gardens[index].as_mut()) {
            (None, None) => false,
            (None, Some(g)) => {
                if battled(g) {
                    if !g.frozen {
                        // The others stay as the remaining player's own (their game keeps
                        // them); the battled ones wait for the battler.
                        crate::log!("wild host: garden {area}'s battled visitors wait");
                        g.frozen = true;
                        g.ask = None;
                        g.list.retain(|v| matches!(v.place, Place::Battle { .. }));
                    }
                    g.kept = None;
                } else if g.frozen
                    && !g.list.is_empty()
                    && g.kept.is_none_or(|at| now.duration_since(at) < KEEP_FROZEN)
                {
                    g.kept.get_or_insert(now);
                } else {
                    self.gardens[index] = None;
                    self.battles.retain(|_, r| r.area != area);
                }
                false
            }
            (Some((fingerprint, _)), Some(g)) if g.fingerprint == fingerprint => {
                if g.frozen {
                    // Back with its battled visitors: the first pick may hand over the
                    // picker's own around them.
                    crate::log!("wild host: garden {area}'s battled visitors are back");
                    g.frozen = false;
                    g.kept = None;
                    g.started = now;
                    g.came = 0;
                    g.next = now;
                }
                true
            }
            (Some((fingerprint, ground)), _) => {
                let machine = machine_spot(area, ground);
                let mut spots = Spots::read(ground);
                if let Some((at, _)) = machine {
                    spots = spots.clear_of(at, MACHINE_CLEAR);
                }
                crate::log!(
                    "wild host: garden {area} has {} land and {} pond spots for shared visitors",
                    spots.land.len(),
                    spots.water.len()
                );
                self.battles.retain(|_, r| r.area != area);
                self.gardens[index] = Some(Visit {
                    fingerprint,
                    spots,
                    machine,
                    print: 0,
                    section: None,
                    cap: self.rng.between(MIN_CAP as u64, MAX_VISITORS as u64) as usize,
                    came: 0,
                    started: now,
                    next: now,
                    ask: None,
                    list: Vec::new(),
                    frozen: false,
                    kept: None,
                });
                true
            }
        }
    }

    /// Held visitors: in their holders' hands, put down when they go quiet, carried out when
    /// they're settled elsewhere.
    fn hands(&mut self, index: usize, players: &[Seen], ground: &Garden, now: Instant) {
        let area = index as u8 + 1;
        let Some(g) = self.gardens[index].as_mut() else {
            return;
        };
        let mut out = Vec::new();
        for (i, v) in g.list.iter_mut().enumerate() {
            let Place::Held {
                by,
                offset,
                heard,
                falling,
                last,
            } = v.place
            else {
                continue;
            };
            match players.iter().find(|p| p.id == by) {
                Some(p) if p.area == area => {
                    v.at = falling.unwrap_or_else(|| in_hands(p.position, p.rotation_y, offset));
                    v.angle = p.rotation_y as u16;
                    if now.duration_since(heard) > HELD_LIMIT {
                        let at = falling.unwrap_or(p.position);
                        (v.at, v.place) = settle(&g.spots, Some(ground), at, &mut self.rng);
                        v.let_go = Some((by, now));
                    } else {
                        v.place = Place::Held {
                            by,
                            offset,
                            heard,
                            falling,
                            last: p.position,
                        };
                    }
                }
                // Loading on from here: it waits in their hands.
                Some(p) if p.loading => {}
                // Settled elsewhere with a wild one still in hands: carried out.
                Some(_) if v.origin == Origin::Wild && falling.is_none() => out.push(i),
                // Gone, a partner, or one let go of (falling, in a Chao's hands): it stays
                // where it was.
                _ => {
                    let at = falling.unwrap_or(last);
                    (v.at, v.place) = settle(&g.spots, Some(ground), at, &mut self.rng);
                    v.let_go = Some((by, now));
                }
            }
        }
        for i in out.into_iter().rev() {
            if let Some(v) = self.remove(area, i) {
                crate::log!(
                    "wild host: visitor {} was carried out of garden {area}",
                    v.id
                );
            }
        }
    }

    /// Whistling players call the visitors near them (the nearest whistler wins).
    fn calls(&mut self, index: usize, players: &[Seen], now: Instant) {
        let area = index as u8 + 1;
        let Some(g) = self.gardens[index].as_mut() else {
            return;
        };
        let whistling: Vec<&Seen> = players
            .iter()
            .filter(|p| {
                p.area == area && p.action == WHISTLE && now.duration_since(p.heard) < PLAYER_STALE
            })
            .collect();
        for v in &mut g.list {
            if !v.free() {
                v.called = None;
                continue;
            }
            let nearest = whistling
                .iter()
                .map(|p| (p.id, distance(p.position, v.at)))
                .filter(|(_, d)| *d <= CALL_REACH)
                .min_by(|a, b| a.1.total_cmp(&b.1));
            if let Some((id, _)) = nearest {
                v.called = Some((id, now + Duration::from_secs(CALL_SECS)));
            } else if v.called.is_some_and(|(_, until)| now >= until) {
                v.called = None;
            }
        }
    }

    /// One frame of walking and swimming.
    fn step(&mut self, index: usize, players: &[Seen], ground: &Garden) {
        let area = index as u8 + 1;
        let Some(g) = self.gardens[index].as_mut() else {
            return;
        };
        let here: Vec<&Seen> = players.iter().filter(|p| p.area == area).collect();
        for v in &mut g.list {
            let caller = v
                .called
                .and_then(|(id, _)| here.iter().find(|p| p.id == id))
                .map(|p| (p.position[0], p.position[2]));
            let xz = (v.at[0], v.at[2]);
            match &mut v.place {
                Place::Roam(roam) => {
                    let moved = match caller {
                        Some(to) => {
                            roam.goal = None;
                            walk_toward(xz, to)
                        }
                        None => roam.step(&g.spots, &ground.grid, xz, &mut self.rng),
                    };
                    let mut at = v.at;
                    if let Some((next, heading)) = moved {
                        (at[0], at[2]) = next;
                        v.angle = heading as u16;
                    }
                    for p in &here {
                        push_out(&mut at, p.position);
                    }
                    if at != v.at {
                        let floor = floor_below(&ground.tris, [at[0], v.at[1] + 5.0, at[2]]);
                        match floor {
                            Some(y)
                                if (y - v.at[1]).abs() < 10.0
                                    && !in_pond(&g.spots, [at[0], y, at[2]]) =>
                            {
                                v.at = [at[0], y, at[2]];
                            }
                            // Off the floor or into the pond: it stays where it was.
                            _ => roam.goal = None,
                        }
                    }
                }
                Place::Swim(swim) => {
                    v.angle = swim.step(&g.spots, &mut self.rng, caller) as u16;
                    let surface = g.spots.water_at(swim.to).unwrap_or(v.at[1]);
                    v.at = [swim.at.0, surface, swim.at.1];
                }
                Place::Held { .. } | Place::Battle { .. } => {}
            }
        }
    }

    /// Wild visitors that stayed long enough go once every player is away from them; ones that
    /// fell out of the garden go at once; partners go with their bringers.
    fn leave(&mut self, index: usize, players: &[Seen], now: Instant) {
        let area = index as u8 + 1;
        let Some(g) = self.gardens[index].as_ref() else {
            return;
        };
        let here: Vec<&Seen> = players.iter().filter(|p| p.area == area).collect();
        let gone: Vec<usize> = g
            .list
            .iter()
            .enumerate()
            .filter(|(_, v)| {
                let lost = v.at[1] < LOST_Y;
                let far = here.iter().all(|p| distance(p.position, v.at) >= GONE_FAR);
                let done = now.duration_since(v.born) > v.stay && v.free() && far;
                let left = match v.origin {
                    Origin::Wild => false,
                    Origin::Partner { bringer, .. } => {
                        self.sees(bringer).is_none()
                            || players
                                .iter()
                                .find(|p| p.id == bringer)
                                .is_none_or(|p| p.area != area && !p.loading)
                    }
                };
                lost || (v.origin == Origin::Wild && done) || left
            })
            .map(|(i, _)| i)
            .collect();
        if gone.is_empty() {
            return;
        }
        let delay = self.delay();
        for i in gone.into_iter().rev() {
            self.remove(area, i);
        }
        if let Some(g) = self.gardens[index].as_mut() {
            g.next = g.next.max(now + delay);
        }
    }

    /// The garden's picker now: of the players there who see and pick visitors with the
    /// garden's print (any while it has none), the one there longest (the lower id on a tie).
    fn picker(&self, index: usize, players: &[Seen]) -> Option<u8> {
        let g = self.gardens[index].as_ref()?;
        players
            .iter()
            .filter(|p| p.area == index as u8 + 1)
            .filter(|p| self.flags(p.id) & WILD_PICK != 0)
            .filter(|p| self.sees(p.id).is_some_and(|print| g.print_fits(print)))
            .min_by_key(|p| (p.since, p.id))
            .map(|p| p.id)
    }

    /// Asks the picker for the next visitors when they're due and there's room.
    fn ask(
        &mut self,
        index: usize,
        players: &[Seen],
        now: Instant,
        gardens: &impl Gardens,
        out: &mut Vec<Out>,
    ) {
        let area = index as u8 + 1;
        let picker = self.picker(index, players);
        let delay = self.delay();
        let animals = gardens.animals(area);
        let picker_print = picker.and_then(|p| self.sees(p));
        let Some(g) = self.gardens[index].as_mut() else {
            return;
        };
        if let Some(a) = g.ask {
            if Some(a.to) != picker {
                // The picker changed: the new one is asked at once.
                g.ask = None;
                g.next = g.next.min(now);
            } else if now.duration_since(a.at) > ASK_LIMIT {
                g.ask = None;
                g.next = now + delay;
            }
        }
        let (Some(to), Some(print)) = (picker, picker_print) else {
            return;
        };
        if g.ask.is_some() || g.frozen || now < g.next {
            return;
        }
        let (wild, room) = (g.wild(), g.room(animals));
        if wild >= g.cap || room == 0 {
            g.next = now + delay;
            return;
        }
        let count = if g.came == 0 { ON_ENTRY } else { 1 }
            .min(g.cap - wild)
            .min(room)
            .min(MAX_WILD_PICK);
        let pond_cells = !g.spots.pond_entries().is_empty();
        let mut swimming = g
            .list
            .iter()
            .filter(|v| matches!(v.place, Place::Swim(_)))
            .count();
        let mut pond = 0u8;
        for _ in 0..count {
            if to_pond(&mut self.rng, swimming, pond_cells) {
                pond += 1;
                swimming += 1;
            }
        }
        let fresh = g.came == 0 && now.duration_since(g.started) < HANDOVER;
        let section = g.section.filter(|&(_, p)| p == print).map(|(s, _)| s);
        self.asks = self.asks.wrapping_add(1);
        let ask = Ask {
            serial: self.asks,
            to,
            section,
            land: count as u8 - pond,
            pond,
            fresh,
            at: now,
        };
        g.ask = Some(ask);
        out.push(Out {
            to: To::One(to),
            from: SERVER,
            body: Body::WildAsk {
                area,
                ask: ask.serial,
                section,
                land: ask.land,
                pond: ask.pond,
                fresh,
            },
        });
    }

    /// The list, to the players in the garden who see it with its print (an empty one with no
    /// area: its picker's; none: anyone's).
    fn send(&mut self, index: usize, players: &[Seen], gardens: &impl Gardens, out: &mut Vec<Out>) {
        let area = index as u8 + 1;
        let print = self.garden_print(index, players);
        self.lists = self.lists.wrapping_add(1);
        let Some(g) = self.gardens[index].as_ref() else {
            return;
        };
        let receivers: Vec<u8> = players
            .iter()
            .filter(|p| p.area == area)
            .filter(|p| self.sees(p.id).is_some_and(|s| print == 0 || s == print))
            .map(|p| p.id)
            .collect();
        if receivers.is_empty() {
            return;
        }
        let learners = gardens
            .owners(area)
            .iter()
            .filter(|o| o.owner < 16 && o.awake)
            .filter(|o| o.sessions.iter().any(|&s| self.sees(s) == Some(g.print)))
            .fold(0u16, |bits, o| bits | 1 << o.owner);
        let list: Vec<WildEntry> = g
            .list
            .iter()
            .take(MAX_WILD_LIST)
            .map(|v| {
                let (place, at) = match v.place {
                    Place::Roam(_) => (WildPlace::Roam, v.at),
                    Place::Swim(_) => (WildPlace::Swim, v.at),
                    Place::Held {
                        falling: Some(at), ..
                    } => (WildPlace::Roam, at),
                    Place::Held { by, offset, .. } => (WildPlace::Held { by }, offset),
                    Place::Battle { by, .. } => (WildPlace::Battle { by }, v.at),
                };
                WildEntry {
                    id: v.id,
                    creature: v.creature,
                    bringer: match v.origin {
                        Origin::Wild => None,
                        Origin::Partner { bringer, .. } => Some(bringer),
                    },
                    place,
                    v: at,
                    angle: v.angle,
                }
            })
            .collect();
        let body = Body::WildList {
            area,
            serial: self.lists,
            section: g.section.map(|(s, _)| s),
            print,
            picker: self.picker(index, players),
            learners,
            list,
        };
        for to in receivers {
            out.push(Out {
                to: To::One(to),
                from: SERVER,
                body: body.clone(),
            });
        }
    }
}

/// Moves a visitor at `pos` out of the room of a player at `player`, along the ground.
fn push_out(pos: &mut V3, player: V3) {
    if (pos[1] - player[1]).abs() > 10.0 {
        return;
    }
    let (dx, dz) = (pos[0] - player[0], pos[2] - player[2]);
    let d = (dx * dx + dz * dz).sqrt();
    if d >= PLAYER_ROOM {
        return;
    }
    let (nx, nz) = if d > 1e-3 {
        (dx / d, dz / d)
    } else {
        (1.0, 0.0)
    };
    pos[0] = player[0] + nx * PLAYER_ROOM;
    pos[2] = player[2] + nz * PLAYER_ROOM;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chao_sim::land::{GRID_CELLS, GRID_COLS, Tri};
    use crate::chao_sim::wild::{LAND, WATER};
    use std::collections::HashSet;

    const FRAME: Duration = Duration::from_micros(16_667);
    const PRINT: u32 = 0xC0FFEE;
    const ALL: u8 = WILD_SEE | WILD_PICK | WILD_MACHINE;

    /// A quad over cell (col, row) at height `y`.
    fn cell(col: usize, row: usize, y: f32, flags: u32) -> [Tri; 2] {
        let (x0, z0) = ((col as f32 - 16.0) * 10.0, (row as f32 - 20.0) * 10.0);
        let (x1, z1) = (x0 + 10.0, z0 + 10.0);
        [
            Tri {
                v: [[x0, y, z0], [x1, y, z0], [x0, y, z1]],
                flags,
            },
            Tri {
                v: [[x1, y, z0], [x1, y, z1], [x0, y, z1]],
                flags,
            },
        ]
    }

    fn pond_cell(col: usize, row: usize) -> bool {
        (14..17).contains(&col) && (18..21).contains(&row)
    }

    /// Grass on cells (8..18, 15..25), x -80..20 and z -50..50, with a 3x3 pond (x -20..10,
    /// z -20..10) whose surface is at -2 and floor at -6.
    fn ground() -> Garden {
        let mut grid = vec![0; GRID_CELLS];
        let mut tris = Vec::new();
        for col in 8..18 {
            for row in 15..25 {
                if pond_cell(col, row) {
                    grid[row * GRID_COLS + col] = WATER;
                    tris.extend(cell(col, row, -6.0, 1));
                    tris.extend(cell(col, row, -2.0, 0x2003));
                } else {
                    grid[row * GRID_COLS + col] = LAND;
                    tris.extend(cell(col, row, 0.0, 1));
                }
            }
        }
        Garden {
            tris,
            grid,
            ledges: Vec::new(),
            blockers: Vec::new(),
            toys: Vec::new(),
            tree_spots: Vec::new(),
        }
    }

    struct Fake {
        players: Vec<Seen>,
        ground: Option<(u32, Garden)>,
        animals: usize,
        chao: HashMap<(u8, u8), ChaoSeen>,
        owner: HashMap<(u8, u8), (u64, Vec<u8>)>,
        locked: HashSet<(u8, (u8, u8))>,
        owners: Vec<OwnerSeen>,
    }

    impl Gardens for Fake {
        fn players(&self) -> Vec<Seen> {
            self.players.clone()
        }
        fn ground(&self, area: u8) -> Option<(u32, &Garden)> {
            self.ground
                .as_ref()
                .filter(|_| area == 1)
                .map(|(f, g)| (*f, g))
        }
        fn animals(&self, _area: u8) -> usize {
            self.animals
        }
        fn chao(&self, area: u8, key: (u8, u8)) -> Option<ChaoSeen> {
            self.chao.get(&key).copied().filter(|_| area == 1)
        }
        fn owner(&self, key: (u8, u8)) -> Option<(u64, Vec<u8>)> {
            self.owner.get(&key).cloned()
        }
        fn locked_against(&self, from: u8, key: (u8, u8)) -> bool {
            self.locked.contains(&(from, key))
        }
        fn owners(&self, _area: u8) -> Vec<OwnerSeen> {
            self.owners.clone()
        }
    }

    fn seen(id: u8, at: V3, since: Instant) -> Seen {
        Seen {
            id,
            area: 1,
            since,
            loading: false,
            position: at,
            rotation_y: 0,
            action: 0,
            heard: since,
        }
    }

    /// Players 0 and 1 in garden 1 (0 there first), far apart on the grass, both seeing and
    /// picking with `PRINT`.
    fn two(t0: Instant) -> (WildHost, Fake) {
        let fake = Fake {
            players: vec![
                seen(0, [-70.0, 0.0, -40.0], t0),
                seen(1, [-70.0, 0.0, 40.0], t0 + Duration::from_secs(1)),
            ],
            ground: Some((7, ground())),
            animals: 0,
            chao: HashMap::new(),
            owner: HashMap::new(),
            locked: HashSet::new(),
            owners: Vec::new(),
        };
        let mut host = WildHost::default();
        for id in [0, 1] {
            hello(&mut host, &fake, id, ALL, PRINT, t0);
        }
        (host, fake)
    }

    fn hello(host: &mut WildHost, fake: &Fake, id: u8, flags: u8, print: u32, now: Instant) {
        host.observe(id, &Body::WildHello { flags, print }, now, fake);
    }

    /// Steps `frames` frames from `*now`; what went out.
    fn run(host: &mut WildHost, fake: &mut Fake, frames: u32, now: &mut Instant) -> Vec<Out> {
        let mut out = Vec::new();
        for _ in 0..frames {
            *now += FRAME;
            for p in &mut fake.players {
                p.heard = *now;
            }
            host.frame(*now, fake, &mut out);
        }
        out
    }

    fn asks(out: &[Out]) -> Vec<(u8, Body)> {
        out.iter()
            .filter_map(|o| match (o.to, &o.body) {
                (To::One(to), b @ Body::WildAsk { .. }) => Some((to, b.clone())),
                _ => None,
            })
            .collect()
    }

    fn creature(species: u16) -> WildCreature {
        WildCreature {
            species,
            level: 5,
            ability: 9,
            shiny: false,
            moves: [84, 0, 0, 0],
        }
    }

    /// Player `to`'s answer to the ask in `out`: as many land and pond visitors as it asks.
    fn answer(host: &mut WildHost, fake: &Fake, out: &[Out], now: Instant) -> u8 {
        let Some((
            to,
            Body::WildAsk {
                area,
                ask,
                land,
                pond,
                ..
            },
        )) = asks(out).pop()
        else {
            panic!("no ask in {out:?}");
        };
        let list = (0..land)
            .map(|i| WildPicked {
                creature: creature(10 + u16::from(i)),
                arrive: WildArrive::Land,
            })
            .chain((0..pond).map(|i| WildPicked {
                creature: creature(20 + u16::from(i)),
                arrive: WildArrive::Pond,
            }))
            .collect();
        let pick = Body::WildPick {
            area,
            ask,
            section: 33,
            list,
        };
        host.observe(to, &pick, now, fake);
        to
    }

    /// A list as heard: section, print, picker, learners, visitors.
    type Heard = (Option<u8>, u32, Option<u8>, u16, Vec<WildEntry>);

    /// The last list player `to` got.
    fn list_to(out: &[Out], to: u8) -> Option<Heard> {
        out.iter().rev().find_map(|o| match (&o.to, &o.body) {
            (
                To::One(t),
                Body::WildList {
                    section,
                    print,
                    picker,
                    learners,
                    list,
                    ..
                },
            ) if *t == to => Some((*section, *print, *picker, *learners, list.clone())),
            _ => None,
        })
    }

    fn entries(out: &[Out], to: u8) -> Vec<WildEntry> {
        list_to(out, to).map(|l| l.4).unwrap_or_default()
    }

    fn answers(out: &[Out], to: u8) -> Vec<(WildRequest, u16, bool)> {
        out.iter()
            .filter_map(|o| match (&o.to, &o.body) {
                (To::One(t), Body::WildAnswer { kind, id, ok, .. }) if *t == to => {
                    Some((*kind, *id, *ok))
                }
                _ => None,
            })
            .collect()
    }

    /// Two players in a garden with the first visitors there.
    fn with_visitors(t0: Instant) -> (WildHost, Fake, Instant, Vec<Out>) {
        let (mut host, mut fake) = two(t0);
        let mut now = t0 + Duration::from_secs(2);
        let out = run(&mut host, &mut fake, 2, &mut now);
        assert_eq!(answer(&mut host, &fake, &out, now), 0);
        let out = run(&mut host, &mut fake, 12, &mut now);
        (host, fake, now, out)
    }

    #[test]
    fn the_first_one_there_picks_and_the_next_takes_over() {
        let t0 = Instant::now();
        let (mut host, mut fake) = two(t0);
        let mut now = t0 + Duration::from_secs(2);
        let out = run(&mut host, &mut fake, 2, &mut now);
        let a = asks(&out);
        assert_eq!(a.len(), 1);
        let (
            to,
            Body::WildAsk {
                section,
                land,
                pond,
                fresh,
                ..
            },
        ) = &a[0]
        else {
            unreachable!()
        };
        // The one there longest; a new run's first ask chooses the area, three at once, and
        // may hand over the picker's own visitors.
        assert_eq!(*to, 0);
        assert_eq!(*section, None);
        assert_eq!(land + pond, ON_ENTRY as u8);
        assert!(*fresh);
        answer(&mut host, &fake, &out, now);
        let out = run(&mut host, &mut fake, 12, &mut now);
        let (section, print, picker, _, list) = list_to(&out, 1).unwrap();
        assert_eq!((section, print, picker), (Some(33), PRINT, Some(0)));
        assert_eq!(list.len(), ON_ENTRY);
        // Player 0 goes to the lobby: player 1 picks now, and the visitors stay.
        fake.players[0].area = 0;
        let out = run(&mut host, &mut fake, 12, &mut now);
        let (section, _, picker, _, list) = list_to(&out, 1).unwrap();
        assert_eq!((section, picker), (Some(33), Some(1)));
        assert_eq!(list.len(), ON_ENTRY);
        assert!(
            list_to(&out, 0).is_none(),
            "only those in the garden hear of it"
        );
        // The next ask is theirs, from the garden's area.
        let out = run(&mut host, &mut fake, 60 * 60, &mut now);
        let a = asks(&out);
        assert!(!a.is_empty());
        assert!(a.iter().all(|(to, b)| *to == 1
            && matches!(b, Body::WildAsk { section: Some(33), land, pond, fresh: false, .. } if land + pond == 1)));
    }

    #[test]
    fn a_player_without_the_pick_flag_or_print_never_picks() {
        let t0 = Instant::now();
        let (mut host, mut fake) = two(t0);
        hello(&mut host, &fake, 0, WILD_SEE, PRINT, t0);
        hello(&mut host, &fake, 1, 0, 0, t0);
        let mut now = t0 + Duration::from_secs(2);
        assert!(asks(&run(&mut host, &mut fake, 10, &mut now)).is_empty());
        // Another print picks while the list is empty.
        hello(&mut host, &fake, 1, ALL, PRINT + 1, now);
        let out = run(&mut host, &mut fake, 10, &mut now);
        assert_eq!(asks(&out)[0].0, 1);
        answer(&mut host, &fake, &out, now);
        let out = run(&mut host, &mut fake, 12, &mut now);
        // Player 0 has another print: no list for them; player 1 has it.
        assert!(list_to(&out, 0).is_none());
        assert_eq!(list_to(&out, 1).unwrap().1, PRINT + 1);
    }

    #[test]
    fn picks_that_dont_fit_their_ask_are_dropped() {
        let t0 = Instant::now();
        let (mut host, mut fake) = two(t0);
        let mut now = t0 + Duration::from_secs(2);
        let out = run(&mut host, &mut fake, 2, &mut now);
        let Some((_, Body::WildAsk { ask, land, .. })) = asks(&out).pop() else {
            unreachable!()
        };
        let land_one = |n: usize| -> Vec<WildPicked> {
            vec![
                WildPicked {
                    creature: creature(1),
                    arrive: WildArrive::Land,
                };
                n
            ]
        };
        let pick = |ask: u16, list: Vec<WildPicked>| Body::WildPick {
            area: 1,
            ask,
            section: 5,
            list,
        };
        // From someone else, or for another ask: nothing, and the ask is still out.
        host.observe(1, &pick(ask, land_one(1)), now, &fake);
        host.observe(0, &pick(ask.wrapping_add(1), land_one(1)), now, &fake);
        let out = run(&mut host, &mut fake, 12, &mut now);
        assert!(entries(&out, 0).is_empty());
        // More land ones than asked: the whole pick goes, and the ask with it.
        host.observe(0, &pick(ask, land_one(usize::from(land) + 1)), now, &fake);
        let out = run(&mut host, &mut fake, 12, &mut now);
        assert!(entries(&out, 0).is_empty());
        host.observe(0, &pick(ask, land_one(1)), now, &fake);
        assert!(entries(&run(&mut host, &mut fake, 12, &mut now), 0).is_empty());
        // The next ask is not fresh: placed ones and another section are refused.
        let out = run(&mut host, &mut fake, 60 * 60, &mut now);
        let Some((_, Body::WildAsk { ask, fresh, .. })) = asks(&out).pop() else {
            panic!("asked again")
        };
        assert!(!fresh);
        let placed = vec![WildPicked {
            creature: creature(1),
            arrive: WildArrive::LandAt {
                at: [-50.0, 0.0, 0.0],
                angle: 0,
            },
        }];
        host.observe(0, &pick(ask, placed), now, &fake);
        assert!(entries(&run(&mut host, &mut fake, 12, &mut now), 0).is_empty());
    }

    #[test]
    fn a_fresh_pick_keeps_its_visitors_where_they_are() {
        let t0 = Instant::now();
        let (mut host, mut fake) = two(t0);
        let mut now = t0 + Duration::from_secs(2);
        let out = run(&mut host, &mut fake, 2, &mut now);
        let Some((_, Body::WildAsk { ask, .. })) = asks(&out).pop() else {
            unreachable!()
        };
        let list = vec![
            WildPicked {
                creature: creature(1),
                arrive: WildArrive::LandAt {
                    at: [-41.0, 3.0, 2.0],
                    angle: 0x1234,
                },
            },
            WildPicked {
                creature: creature(2),
                arrive: WildArrive::SwimAt {
                    at: [-4.0, -3.0, -6.0],
                    angle: 0,
                },
            },
            // Nowhere near a spot: the host puts it.
            WildPicked {
                creature: creature(3),
                arrive: WildArrive::LandAt {
                    at: [500.0, 0.0, 500.0],
                    angle: 0,
                },
            },
        ];
        host.observe(
            0,
            &Body::WildPick {
                area: 1,
                ask,
                section: 9,
                list,
            },
            now,
            &fake,
        );
        let out = run(&mut host, &mut fake, 1, &mut now);
        let out = [out, run(&mut host, &mut fake, 11, &mut now)].concat();
        let list = entries(&out, 0);
        assert_eq!(list.len(), 3);
        let one = list.iter().find(|e| e.creature.species == 1).unwrap();
        assert!(distance(one.v, [-41.0, 0.0, 2.0]) < 1.0, "{one:?}");
        assert!(one.v[1].abs() < 0.01, "on the floor");
        let two = list.iter().find(|e| e.creature.species == 2).unwrap();
        assert_eq!(two.place, WildPlace::Swim);
        assert!((two.v[1] + 2.0).abs() < 0.01, "at the surface");
        let three = list.iter().find(|e| e.creature.species == 3).unwrap();
        assert!(three.v[0] < 30.0 && three.v[2] < 60.0, "{three:?}");
    }

    #[test]
    fn visitors_never_pass_the_cap_nor_the_games_animal_room() {
        let t0 = Instant::now();
        let (mut host, mut fake) = two(t0);
        // 16 animals and drives already: room for 2 (20 - 2 kept - 16).
        fake.animals = 16;
        let mut now = t0 + Duration::from_secs(2);
        let out = run(&mut host, &mut fake, 2, &mut now);
        let Some((_, Body::WildAsk { land, pond, .. })) = asks(&out).first().cloned() else {
            panic!("asked")
        };
        assert_eq!(land + pond, 2);
        answer(&mut host, &fake, &out, now);
        let out = run(&mut host, &mut fake, 60 * 120, &mut now);
        assert!(asks(&out).is_empty(), "no room for more");
        assert_eq!(entries(&out, 0).len(), 2);
        // With room, up to the cap (6 or 7) and never more, over a long time.
        fake.animals = 0;
        let cap = host.gardens[0].as_ref().unwrap().cap;
        assert!((MIN_CAP..=MAX_VISITORS).contains(&cap));
        let mut most = 0;
        for _ in 0..20 * 60 {
            let out = run(&mut host, &mut fake, 60, &mut now);
            if !asks(&out).is_empty() {
                answer(&mut host, &fake, &out, now);
            }
            let n = host.gardens[0].as_ref().unwrap().list.len();
            assert!(n <= cap, "{n}");
            most = most.max(n);
        }
        assert_eq!(most, cap);
    }

    fn bring(host: &mut WildHost, fake: &Fake, from: u8, key: u64, now: Instant) {
        let body = Body::WildBring {
            area: 1,
            key,
            from_save: true,
            creature: creature(50),
            at: None,
        };
        host.observe(from, &body, now, fake);
    }

    #[test]
    fn a_partner_with_no_room_sends_a_wild_one_away_or_is_refused() {
        let t0 = Instant::now();
        let (mut host, mut fake, mut now, _) = with_visitors(t0);
        assert_eq!(host.gardens[0].as_ref().unwrap().list.len(), ON_ENTRY);
        // Room for none more: a partner sends the farthest roaming wild one away.
        fake.animals = 15;
        bring(&mut host, &fake, 1, 77, now);
        let out = run(&mut host, &mut fake, 12, &mut now);
        let a = answers(&out, 1);
        assert!(
            matches!(a[..], [(WildRequest::Bring, id, true)] if id != 0),
            "{a:?}"
        );
        let list = entries(&out, 1);
        assert_eq!(list.len(), ON_ENTRY);
        assert_eq!(list.iter().filter(|e| e.bringer == Some(1)).count(), 1);
        // The same key again, or without the machine flag: refused.
        bring(&mut host, &fake, 1, 77, now);
        hello(&mut host, &fake, 0, WILD_SEE | WILD_PICK, PRINT, now);
        bring(&mut host, &fake, 0, 78, now);
        let out = run(&mut host, &mut fake, 1, &mut now);
        assert_eq!(answers(&out, 1), [(WildRequest::Bring, 0, false)]);
        assert_eq!(answers(&out, 0), [(WildRequest::Bring, 0, false)]);
        // Nothing roaming left to send away: refused.
        fake.animals = 18;
        for v in &mut host.gardens[0].as_mut().unwrap().list {
            if v.origin == Origin::Wild {
                v.place = Place::Battle { by: 5, swim: None };
            }
        }
        bring(&mut host, &fake, 1, 79, now);
        let out = run(&mut host, &mut fake, 1, &mut now);
        assert_eq!(answers(&out, 1), [(WildRequest::Bring, 0, false)]);
        // Its bringer leaving takes it out of the list.
        fake.players[1].area = 0;
        run(&mut host, &mut fake, 1, &mut now);
        assert!(
            host.gardens[0]
                .as_ref()
                .unwrap()
                .list
                .iter()
                .all(|v| v.origin == Origin::Wild)
        );
    }

    /// Player `id` stands next to visitor `v`.
    fn next_to(fake: &mut Fake, id: u8, v: &WildEntry) {
        let p = fake.players.iter_mut().find(|p| p.id == id).unwrap();
        p.position = [v.v[0] + 2.0, v.v[1], v.v[2]];
    }

    fn roaming(out: &[Out]) -> WildEntry {
        *entries(out, 0)
            .iter()
            .find(|e| e.place == WildPlace::Roam)
            .expect("a roaming visitor")
    }

    #[test]
    fn the_first_to_take_a_visitor_has_it_and_it_goes_down_where_they_put_it() {
        let t0 = Instant::now();
        let (mut host, mut fake, mut now, out) = with_visitors(t0);
        let v = roaming(&out);
        next_to(&mut fake, 0, &v);
        next_to(&mut fake, 1, &v);
        host.observe(1, &Body::WildTake { id: v.id }, now, &fake);
        host.observe(0, &Body::WildTake { id: v.id }, now, &fake);
        let out = run(&mut host, &mut fake, 12, &mut now);
        assert_eq!(answers(&out, 1), [(WildRequest::Take, v.id, true)]);
        assert_eq!(answers(&out, 0), [(WildRequest::Take, v.id, false)]);
        let e = entries(&out, 0).into_iter().find(|e| e.id == v.id).unwrap();
        assert_eq!(e.place, WildPlace::Held { by: 1 });
        // Only the holder moves it; a put far from them lands by them.
        host.observe(
            0,
            &Body::WildPut {
                id: v.id,
                at: [0.0; 3],
                angle: 0,
            },
            now,
            &fake,
        );
        let held = Body::WildHeld {
            id: v.id,
            let_go: false,
            v: [0.0, 4.0, 3.0],
        };
        host.observe(1, &held, now, &fake);
        let out = run(&mut host, &mut fake, 12, &mut now);
        let e = entries(&out, 0).into_iter().find(|e| e.id == v.id).unwrap();
        assert_eq!((e.place, e.v), (WildPlace::Held { by: 1 }, [0.0, 4.0, 3.0]));
        // Put down in the pond, it swims.
        let put = Body::WildPut {
            id: v.id,
            at: [-4.0, -3.0, -6.0],
            angle: 0,
        };
        fake.players[1].position = [-12.0, 0.0, -6.0];
        run(&mut host, &mut fake, 1, &mut now);
        host.observe(1, &put, now, &fake);
        let out = run(&mut host, &mut fake, 12, &mut now);
        let e = entries(&out, 0).into_iter().find(|e| e.id == v.id).unwrap();
        assert_eq!(e.place, WildPlace::Swim);
        // Taking needs to be near it.
        fake.players[0].position = [-70.0, 0.0, 40.0];
        host.observe(0, &Body::WildTake { id: v.id }, now, &fake);
        let out = run(&mut host, &mut fake, 1, &mut now);
        assert_eq!(answers(&out, 0), [(WildRequest::Take, v.id, false)]);
    }

    #[test]
    fn a_held_visitor_with_no_word_from_its_holder_is_put_down() {
        let t0 = Instant::now();
        let (mut host, mut fake, mut now, out) = with_visitors(t0);
        let v = roaming(&out);
        next_to(&mut fake, 1, &v);
        host.observe(1, &Body::WildTake { id: v.id }, now, &fake);
        let out = run(&mut host, &mut fake, 60, &mut now);
        assert!(
            entries(&out, 0)
                .iter()
                .any(|e| e.place == WildPlace::Held { by: 1 })
        );
        let out = run(&mut host, &mut fake, 120, &mut now);
        let e = entries(&out, 0).into_iter().find(|e| e.id == v.id).unwrap();
        assert_eq!(e.place, WildPlace::Roam);
        assert!(distance(e.v, fake.players[1].position) <= PLAYER_ROOM + 0.5);
    }

    #[test]
    fn carried_out_a_wild_one_goes_and_a_partner_stays() {
        let t0 = Instant::now();
        let (mut host, mut fake, mut now, out) = with_visitors(t0);
        let v = roaming(&out);
        next_to(&mut fake, 1, &v);
        host.observe(1, &Body::WildTake { id: v.id }, now, &fake);
        // Loading out: it waits in their hands; settled in the lobby: gone for everyone.
        fake.players[1].area = 0;
        fake.players[1].loading = true;
        let out = run(&mut host, &mut fake, 12, &mut now);
        assert!(entries(&out, 0).iter().any(|e| e.id == v.id));
        fake.players[1].loading = false;
        let out = run(&mut host, &mut fake, 12, &mut now);
        assert!(!entries(&out, 0).iter().any(|e| e.id == v.id));
        // A partner (player 0's) carried out by player 1 is put back where they left.
        fake.players[1].area = 1;
        bring(&mut host, &fake, 0, 5, now);
        let out = run(&mut host, &mut fake, 12, &mut now);
        let p = *entries(&out, 0)
            .iter()
            .find(|e| e.bringer == Some(0))
            .unwrap();
        next_to(&mut fake, 1, &p);
        host.observe(1, &Body::WildTake { id: p.id }, now, &fake);
        run(&mut host, &mut fake, 2, &mut now);
        let left_at = fake.players[1].position;
        fake.players[1].area = 0;
        let out = run(&mut host, &mut fake, 12, &mut now);
        let e = entries(&out, 0).into_iter().find(|e| e.id == p.id).unwrap();
        assert_eq!(e.place, WildPlace::Roam);
        assert!(distance(e.v, left_at) < 1.0);
    }

    #[test]
    fn coming_in_carrying_one_adds_it_in_their_hands() {
        let t0 = Instant::now();
        let (mut host, mut fake, mut now, _) = with_visitors(t0);
        let carry = Body::WildCarry {
            area: 1,
            creature: creature(60),
            offset: [0.0, 4.0, 2.0],
        };
        host.observe(1, &carry, now, &fake);
        // Not twice at once, and not with another print.
        host.observe(1, &carry, now, &fake);
        hello(&mut host, &fake, 0, ALL, PRINT + 1, now);
        host.observe(0, &carry, now, &fake);
        let out = run(&mut host, &mut fake, 12, &mut now);
        let a = answers(&out, 1);
        assert!(
            matches!(a[..], [(WildRequest::Carry, id, true), (WildRequest::Carry, 0, false)] if id != 0),
            "{a:?}"
        );
        assert_eq!(answers(&out, 0), [(WildRequest::Carry, 0, false)]);
        let list = entries(&out, 1);
        assert!(
            list.iter()
                .any(|e| e.creature.species == 60 && e.place == WildPlace::Held { by: 1 })
        );
    }

    /// With a roaming visitor held by player 1 next to Chao (0, 0) (player 0's).
    fn holding(t0: Instant) -> (WildHost, Fake, Instant, WildEntry) {
        let (mut host, mut fake, mut now, out) = with_visitors(t0);
        let v = roaming(&out);
        next_to(&mut fake, 1, &v);
        host.observe(1, &Body::WildTake { id: v.id }, now, &fake);
        run(&mut host, &mut fake, 2, &mut now);
        fake.chao.insert(
            (0, 0),
            ChaoSeen {
                position: fake.players[1].position,
                awake: true,
            },
        );
        fake.owner.insert((0, 0), (0xABCD, vec![0]));
        (host, fake, now, v)
    }

    fn taught(out: &[Out]) -> Vec<(u8, u64, WildOrigin, WildCreature)> {
        out.iter()
            .filter_map(|o| match (&o.to, &o.body) {
                (
                    To::One(to),
                    Body::WildTaught {
                        identity,
                        origin,
                        creature,
                    },
                ) => Some((*to, *identity, *origin, *creature)),
                _ => None,
            })
            .collect()
    }

    fn absorb(host: &mut WildHost, fake: &Fake, from: u8, id: u16, now: Instant) {
        let body = Body::WildAbsorbed {
            id,
            owner: 0,
            index: 0,
        };
        host.observe(from, &body, now, fake);
    }

    #[test]
    fn a_chao_absorbing_a_visitor_teaches_its_owner_and_takes_it_from_everyone() {
        let t0 = Instant::now();
        let (mut host, mut fake, mut now, v) = holding(t0);
        // Not the holder: nothing.
        absorb(&mut host, &fake, 0, v.id, now);
        let out = run(&mut host, &mut fake, 12, &mut now);
        assert!(taught(&out).is_empty());
        assert!(entries(&out, 0).iter().any(|e| e.id == v.id));
        absorb(&mut host, &fake, 1, v.id, now);
        let out = run(&mut host, &mut fake, 12, &mut now);
        assert_eq!(taught(&out), [(0, 0xABCD, WildOrigin::Wild, v.creature)]);
        assert!(!entries(&out, 0).iter().any(|e| e.id == v.id));
        assert!(!entries(&out, 1).iter().any(|e| e.id == v.id));
        // Gone: a second absorb does nothing.
        absorb(&mut host, &fake, 1, v.id, now);
        assert!(taught(&run(&mut host, &mut fake, 1, &mut now)).is_empty());
    }

    #[test]
    fn a_locked_chao_absorbs_but_its_owner_learns_nothing() {
        let t0 = Instant::now();
        let (mut host, mut fake, mut now, v) = holding(t0);
        fake.locked.insert((1, (0, 0)));
        absorb(&mut host, &fake, 1, v.id, now);
        let out = run(&mut host, &mut fake, 12, &mut now);
        assert!(taught(&out).is_empty());
        assert!(!entries(&out, 0).iter().any(|e| e.id == v.id));
    }

    #[test]
    fn an_owner_with_another_print_or_not_seeing_learns_nothing() {
        for flags in [0, WILD_SEE] {
            let t0 = Instant::now();
            let (mut host, mut fake, mut now, v) = holding(t0);
            let print = if flags == 0 { 0 } else { PRINT + 1 };
            hello(&mut host, &fake, 0, flags, print, now);
            absorb(&mut host, &fake, 1, v.id, now);
            let out = run(&mut host, &mut fake, 12, &mut now);
            assert!(taught(&out).is_empty());
            assert!(!entries(&out, 1).iter().any(|e| e.id == v.id));
        }
    }

    #[test]
    fn a_sleeping_or_far_chao_never_absorbs() {
        let t0 = Instant::now();
        let (mut host, mut fake, mut now, v) = holding(t0);
        fake.chao.get_mut(&(0, 0)).unwrap().awake = false;
        absorb(&mut host, &fake, 1, v.id, now);
        let out = run(&mut host, &mut fake, 12, &mut now);
        assert!(entries(&out, 0).iter().any(|e| e.id == v.id));
        fake.chao.get_mut(&(0, 0)).unwrap().awake = true;
        fake.chao.get_mut(&(0, 0)).unwrap().position[0] += 40.0;
        absorb(&mut host, &fake, 1, v.id, now);
        let out = run(&mut host, &mut fake, 12, &mut now);
        assert!(entries(&out, 0).iter().any(|e| e.id == v.id));
        assert!(taught(&out).is_empty());
    }

    #[test]
    fn an_absorbed_partner_is_used_up_for_its_bringer() {
        let t0 = Instant::now();
        let (mut host, mut fake, mut now, _) = with_visitors(t0);
        bring(&mut host, &fake, 0, 0x55, now);
        let out = run(&mut host, &mut fake, 12, &mut now);
        let p = *entries(&out, 1)
            .iter()
            .find(|e| e.bringer == Some(0))
            .unwrap();
        next_to(&mut fake, 1, &p);
        host.observe(1, &Body::WildTake { id: p.id }, now, &fake);
        // Let go: it may still go into a Chao's hands a moment after.
        let put = Body::WildPut {
            id: p.id,
            at: fake.players[1].position,
            angle: 0,
        };
        host.observe(1, &put, now, &fake);
        fake.chao.insert(
            (0, 1),
            ChaoSeen {
                position: fake.players[1].position,
                awake: true,
            },
        );
        fake.owner.insert((0, 1), (0x99, vec![0]));
        let body = Body::WildAbsorbed {
            id: p.id,
            owner: 0,
            index: 1,
        };
        host.observe(1, &body, now, &fake);
        let out = run(&mut host, &mut fake, 1, &mut now);
        assert!(out.iter().any(|o| o.to == To::One(0)
            && o.body
                == Body::WildUsed {
                    garden: 1,
                    key: 0x55
                }));
        assert_eq!(taught(&out)[0].2, WildOrigin::SavePartner);
    }

    fn battle(host: &mut WildHost, fake: &Fake, from: u8, id: u16, step: WildStep, now: Instant) {
        host.observe(from, &Body::WildBattle { id, step }, now, fake);
    }

    #[test]
    fn a_battled_visitor_is_reserved_until_the_result() {
        let t0 = Instant::now();
        let (mut host, mut fake, mut now, out) = with_visitors(t0);
        let v = roaming(&out);
        next_to(&mut fake, 0, &v);
        next_to(&mut fake, 1, &v);
        battle(&mut host, &fake, 1, v.id, WildStep::Reserve, now);
        battle(&mut host, &fake, 0, v.id, WildStep::Reserve, now);
        host.observe(0, &Body::WildTake { id: v.id }, now, &fake);
        let out = run(&mut host, &mut fake, 12, &mut now);
        assert_eq!(answers(&out, 1), [(WildRequest::Battle, v.id, true)]);
        assert_eq!(
            answers(&out, 0),
            [
                (WildRequest::Battle, v.id, false),
                (WildRequest::Take, v.id, false)
            ]
        );
        let e = entries(&out, 0).into_iter().find(|e| e.id == v.id).unwrap();
        assert_eq!(e.place, WildPlace::Battle { by: 1 });
        // The battler goes to the arena: it stays reserved.
        fake.players[1].area = 0;
        run(&mut host, &mut fake, 60, &mut now);
        // A result from someone else is dropped; won, it roams again.
        battle(&mut host, &fake, 0, v.id, WildStep::Lost, now);
        battle(&mut host, &fake, 1, v.id, WildStep::Won, now);
        let out = run(&mut host, &mut fake, 12, &mut now);
        let e = entries(&out, 0).into_iter().find(|e| e.id == v.id).unwrap();
        assert_eq!(e.place, WildPlace::Roam);
        // Lost: it runs away.
        fake.players[1].area = 1;
        next_to(&mut fake, 1, &e);
        battle(&mut host, &fake, 1, v.id, WildStep::Reserve, now);
        run(&mut host, &mut fake, 1, &mut now);
        battle(&mut host, &fake, 1, v.id, WildStep::Lost, now);
        let out = run(&mut host, &mut fake, 12, &mut now);
        assert!(!entries(&out, 0).iter().any(|e| e.id == v.id));
    }

    #[test]
    fn a_battlers_leaving_frees_it_and_a_long_battle_counts_as_given_up() {
        let t0 = Instant::now();
        let (mut host, mut fake, mut now, out) = with_visitors(t0);
        let v = roaming(&out);
        next_to(&mut fake, 1, &v);
        battle(&mut host, &fake, 1, v.id, WildStep::Reserve, now);
        run(&mut host, &mut fake, 1, &mut now);
        host.left(1, &fake);
        fake.players.retain(|p| p.id != 1);
        let out = run(&mut host, &mut fake, 12, &mut now);
        let e = entries(&out, 0).into_iter().find(|e| e.id == v.id).unwrap();
        assert_eq!(e.place, WildPlace::Roam);
        next_to(&mut fake, 0, &e);
        battle(&mut host, &fake, 0, v.id, WildStep::Reserve, now);
        run(&mut host, &mut fake, 1, &mut now);
        now += BATTLE_LIMIT;
        let out = run(&mut host, &mut fake, 12, &mut now);
        assert!(!entries(&out, 0).iter().any(|e| e.id == v.id));
    }

    #[test]
    fn a_runs_end_mid_battle_keeps_the_battled_one_for_the_same_ground() {
        for same in [true, false] {
            let t0 = Instant::now();
            let (mut host, mut fake, mut now, out) = with_visitors(t0);
            let v = roaming(&out);
            next_to(&mut fake, 1, &v);
            battle(&mut host, &fake, 1, v.id, WildStep::Reserve, now);
            run(&mut host, &mut fake, 1, &mut now);
            let ground = fake.ground.take();
            let out = run(&mut host, &mut fake, 60, &mut now);
            assert!(list_to(&out, 0).is_none(), "frozen: nothing sent");
            assert!(host.gardens[0].as_ref().is_some_and(|g| g.frozen));
            fake.ground = ground.map(|(f, g)| (if same { f } else { f + 1 }, g));
            let out = run(&mut host, &mut fake, 12, &mut now);
            // Only the battled one waited (the others were player 0's own meanwhile), and the
            // picker may hand theirs over around it.
            let list = entries(&out, 0);
            assert_eq!(list.len(), usize::from(same), "same ground: {same}");
            let fresh = asks(&out)
                .iter()
                .any(|(_, b)| matches!(b, Body::WildAsk { fresh: true, .. }));
            assert!(fresh, "same ground: {same}");
            battle(&mut host, &fake, 1, v.id, WildStep::Won, now);
            let out = run(&mut host, &mut fake, 12, &mut now);
            assert_eq!(entries(&out, 0).iter().any(|e| e.id == v.id), same);
        }
        // Won with no run (the battler away, the other alone in the garden): it waits for the
        // battler's coming back a while, then goes.
        for back in [true, false] {
            let t0 = Instant::now();
            let (mut host, mut fake, mut now, out) = with_visitors(t0);
            let v = roaming(&out);
            next_to(&mut fake, 1, &v);
            battle(&mut host, &fake, 1, v.id, WildStep::Reserve, now);
            let ground = fake.ground.take();
            run(&mut host, &mut fake, 2, &mut now);
            battle(&mut host, &fake, 1, v.id, WildStep::Won, now);
            run(&mut host, &mut fake, 60, &mut now);
            assert!(host.gardens[0].as_ref().is_some_and(|g| g.list.len() == 1));
            if !back {
                now += KEEP_FROZEN;
                run(&mut host, &mut fake, 2, &mut now);
                assert!(host.gardens[0].is_none());
                continue;
            }
            fake.ground = ground;
            let out = run(&mut host, &mut fake, 12, &mut now);
            let e = entries(&out, 0).into_iter().find(|e| e.id == v.id).unwrap();
            assert_eq!(e.place, WildPlace::Roam);
        }
        // Lost with no run: nothing is left to wait.
        let t0 = Instant::now();
        let (mut host, mut fake, mut now, out) = with_visitors(t0);
        let v = roaming(&out);
        next_to(&mut fake, 1, &v);
        battle(&mut host, &fake, 1, v.id, WildStep::Reserve, now);
        fake.ground = None;
        run(&mut host, &mut fake, 2, &mut now);
        battle(&mut host, &fake, 1, v.id, WildStep::Lost, now);
        run(&mut host, &mut fake, 2, &mut now);
        assert!(host.gardens[0].is_none());
    }

    #[test]
    fn one_let_go_of_stays_when_its_holder_leaves() {
        let t0 = Instant::now();
        let (mut host, mut fake, mut now, v) = holding(t0);
        let at = fake.players[1].position;
        let fall = Body::WildHeld {
            id: v.id,
            let_go: true,
            v: at,
        };
        host.observe(1, &fall, now, &fake);
        fake.players[1].area = 0;
        let out = run(&mut host, &mut fake, 12, &mut now);
        let e = entries(&out, 0).into_iter().find(|e| e.id == v.id).unwrap();
        assert_eq!(e.place, WildPlace::Roam);
        assert!(distance(e.v, at) < 1.0);
    }

    #[test]
    fn a_let_go_grace_never_takes_one_from_another() {
        let t0 = Instant::now();
        let (mut host, mut fake, mut now, v) = holding(t0);
        let at = fake.players[1].position;
        host.observe(
            1,
            &Body::WildPut {
                id: v.id,
                at,
                angle: 0,
            },
            now,
            &fake,
        );
        run(&mut host, &mut fake, 1, &mut now);
        fake.players[0].position = at;
        host.observe(0, &Body::WildTake { id: v.id }, now, &fake);
        run(&mut host, &mut fake, 1, &mut now);
        absorb(&mut host, &fake, 1, v.id, now);
        let out = run(&mut host, &mut fake, 12, &mut now);
        assert!(taught(&out).is_empty());
        let e = entries(&out, 1).into_iter().find(|e| e.id == v.id).unwrap();
        assert_eq!(e.place, WildPlace::Held { by: 0 });
    }

    #[test]
    fn partners_are_never_battled_and_land_on_the_floor() {
        let t0 = Instant::now();
        let (mut host, mut fake, mut now, _) = with_visitors(t0);
        let machine = host.gardens[0].as_ref().unwrap().machine;
        let (mx, mz) = machine.map_or((-60.0, 0.0), |(m, _)| (m[0] + 2.0, m[2]));
        let body = Body::WildBring {
            area: 1,
            key: 5,
            from_save: false,
            creature: creature(50),
            at: Some(([mx, 1.0e5, mz], 0)),
        };
        host.observe(0, &body, now, &fake);
        let out = run(&mut host, &mut fake, 12, &mut now);
        let p = *entries(&out, 0)
            .iter()
            .find(|e| e.bringer == Some(0))
            .unwrap();
        assert!(p.v[1].abs() < 1.0, "{p:?}");
        next_to(&mut fake, 1, &p);
        battle(&mut host, &fake, 1, p.id, WildStep::Reserve, now);
        let out = run(&mut host, &mut fake, 12, &mut now);
        assert_eq!(answers(&out, 1), [(WildRequest::Battle, p.id, false)]);
    }

    #[test]
    fn one_carried_in_each_entry_within_the_cap() {
        let t0 = Instant::now();
        let (mut host, mut fake, mut now, _) = with_visitors(t0);
        let carry = Body::WildCarry {
            area: 1,
            creature: creature(60),
            offset: [0.0, 4.0, 2.0],
        };
        let mut took = 0;
        for entry in 0..10 {
            for _ in 0..2 {
                host.observe(1, &carry, now, &fake);
                let out = run(&mut host, &mut fake, 2, &mut now);
                for (_, id, ok) in answers(&out, 1) {
                    if ok {
                        took += 1;
                        let at = fake.players[1].position;
                        host.observe(1, &Body::WildPut { id, at, angle: 0 }, now, &fake);
                    }
                }
            }
            assert!(took <= entry + 1);
            fake.players[1].since = now;
        }
        let g = host.gardens[0].as_ref().unwrap();
        assert_eq!(g.wild(), MAX_VISITORS);
        assert!(took > 0);
    }

    #[test]
    fn at_a_runs_start_the_pickers_print_holds_the_garden() {
        let t0 = Instant::now();
        let (mut host, mut fake) = two(t0);
        let mut now = t0 + Duration::from_secs(2);
        hello(&mut host, &fake, 1, ALL, PRINT + 1, now);
        let first = run(&mut host, &mut fake, 12, &mut now);
        assert!(list_to(&first, 1).is_none());
        assert_eq!(list_to(&first, 0).map(|l| l.1), Some(PRINT));
        bring(&mut host, &fake, 1, 5, now);
        let carry = Body::WildCarry {
            area: 1,
            creature: creature(60),
            offset: [0.0, 4.0, 2.0],
        };
        host.observe(1, &carry, now, &fake);
        let out = run(&mut host, &mut fake, 2, &mut now);
        assert_eq!(
            answers(&out, 1),
            [
                (WildRequest::Bring, 0, false),
                (WildRequest::Carry, 0, false)
            ]
        );
        assert_eq!(answer(&mut host, &fake, &first, now), 0);
        let out = run(&mut host, &mut fake, 12, &mut now);
        assert!(!entries(&out, 0).is_empty());
        // Each list's serial is newer than the one before.
        let serials: Vec<u16> = out
            .iter()
            .filter_map(|o| match o.body {
                Body::WildList { serial, .. } => Some(serial),
                _ => None,
            })
            .collect();
        assert!(serials.len() >= 2 && serials.windows(2).all(|w| w[1] > w[0]));
    }

    #[test]
    fn a_whistle_calls_visitors_within_reach() {
        let t0 = Instant::now();
        let (mut host, mut fake, mut now, out) = with_visitors(t0);
        let v = roaming(&out);
        let start = v.v;
        // Whistling 120 away: not called.
        fake.players[0].position = [start[0] + 120.0, 0.0, start[2]];
        fake.players[0].action = WHISTLE;
        run(&mut host, &mut fake, 30, &mut now);
        assert!(
            host.gardens[0]
                .as_ref()
                .unwrap()
                .list
                .iter()
                .all(|v| v.called.is_none())
        );
        // From 60 (still on the grass): it walks over.
        fake.players[0].position = [-75.0, 0.0, start[2].clamp(-45.0, 45.0)];
        let before = distance(start, fake.players[0].position);
        let out = run(&mut host, &mut fake, 120, &mut now);
        let e = entries(&out, 0).into_iter().find(|e| e.id == v.id).unwrap();
        assert!(
            distance(e.v, fake.players[0].position) < before - 10.0,
            "{before} {e:?}"
        );
    }

    #[test]
    fn roaming_ones_stay_on_the_grass_and_swimmers_in_the_pond() {
        let t0 = Instant::now();
        let (mut host, mut fake, mut now, _) = with_visitors(t0);
        for _ in 0..30 {
            let out = run(&mut host, &mut fake, 120, &mut now);
            for e in entries(&out, 0) {
                let (col, row) = crate::chao_sim::land::cell_of(e.v[0], e.v[2]).unwrap();
                match e.place {
                    WildPlace::Roam => {
                        assert!(!pond_cell(col, row), "{e:?}");
                        assert!(e.v[1].abs() < 0.01);
                    }
                    WildPlace::Swim => assert!(pond_cell(col, row), "{e:?}"),
                    _ => {}
                }
            }
        }
    }

    #[test]
    fn visitors_go_after_their_stay_once_everyone_is_away() {
        let t0 = Instant::now();
        let (mut host, mut fake, mut now, _) = with_visitors(t0);
        let ids: Vec<u16> = host.gardens[0]
            .as_ref()
            .unwrap()
            .list
            .iter()
            .map(|v| v.id)
            .collect();
        // Someone stands on each: they stay however long.
        for v in &mut host.gardens[0].as_mut().unwrap().list {
            v.born = now - Duration::from_secs(STAY_SECS.1 + 1);
        }
        fake.players[0].position = host.gardens[0].as_ref().unwrap().list[0].at;
        run(&mut host, &mut fake, 1, &mut now);
        let left: Vec<u16> = host.gardens[0]
            .as_ref()
            .unwrap()
            .list
            .iter()
            .map(|v| v.id)
            .collect();
        assert!(left.contains(&ids[0]));
        // Everyone far: gone. One fallen out goes at once.
        fake.players[0].position = [500.0, 0.0, 500.0];
        fake.players[1].position = [500.0, 0.0, 500.0];
        run(&mut host, &mut fake, 1, &mut now);
        assert!(
            host.gardens[0]
                .as_ref()
                .unwrap()
                .list
                .iter()
                .all(|v| !ids.contains(&v.id))
        );
    }

    #[test]
    fn learners_are_the_owners_whose_games_see_the_print() {
        let t0 = Instant::now();
        let (mut host, mut fake, mut now, _) = with_visitors(t0);
        fake.owners = vec![
            OwnerSeen {
                owner: 0,
                awake: true,
                sessions: vec![0],
            },
            OwnerSeen {
                owner: 1,
                awake: true,
                sessions: vec![1],
            },
            // A held Chao whose owner is online with the print, and a sleeping one.
            OwnerSeen {
                owner: 9,
                awake: true,
                sessions: vec![0],
            },
            OwnerSeen {
                owner: 10,
                awake: false,
                sessions: vec![1],
            },
        ];
        hello(&mut host, &fake, 1, WILD_SEE, PRINT + 1, now);
        let out = run(&mut host, &mut fake, 12, &mut now);
        assert_eq!(list_to(&out, 0).unwrap().3, 1 << 0 | 1 << 9);
    }

    #[test]
    fn the_machine_box_is_there_for_the_servers_chao_while_someone_has_the_machine() {
        let t0 = Instant::now();
        let (mut host, mut fake) = two(t0);
        let mut now = t0;
        run(&mut host, &mut fake, 1, &mut now);
        let b = host.machine_blocker(1, &fake.players).expect("a box");
        assert!(
            matches!(b.shape, Shape::Box { half } if half.iter().all(|h| *h > 0.5 && *h < 20.0))
        );
        for id in [0, 1] {
            hello(&mut host, &fake, id, WILD_SEE | WILD_PICK, PRINT, now);
        }
        assert!(host.machine_blocker(1, &fake.players).is_none());
    }

    #[test]
    fn hostile_messages_change_nothing_and_never_panic() {
        let t0 = Instant::now();
        let (mut host, mut fake, mut now, out) = with_visitors(t0);
        let before = entries(&out, 0);
        let mut rng = Rng::new(1);
        let mut f = || rng.next() as f32 % 2000.0 - 1000.0;
        for i in 0..2000u32 {
            let id = (i % 70) as u16;
            let from = (i % 9) as u8;
            let v = [f(), f(), f()];
            let bodies = [
                Body::WildTake { id },
                Body::WildHeld {
                    id,
                    let_go: i % 2 == 0,
                    v,
                },
                Body::WildPut {
                    id,
                    at: v,
                    angle: 1,
                },
                Body::WildAbsorbed {
                    id,
                    owner: (i % 16) as u8,
                    index: (i % 4) as u8,
                },
                Body::WildBattle {
                    id,
                    step: [WildStep::Won, WildStep::Lost, WildStep::Cancelled][i as usize % 3],
                },
                Body::WildPick {
                    area: (i % 3 + 1) as u8,
                    ask: i as u16,
                    section: 1,
                    list: vec![WildPicked {
                        creature: creature(1),
                        arrive: WildArrive::SwimAt { at: v, angle: 0 },
                    }],
                },
                Body::WildCarry {
                    area: 2,
                    creature: creature(1),
                    offset: [0.0; 3],
                },
                Body::WildBring {
                    area: 3,
                    key: 1,
                    from_save: false,
                    creature: creature(1),
                    at: Some((v, 0)),
                },
            ];
            // Players 2..8 aren't in any garden; 0 and 1 hold nothing and battle nothing.
            if from >= 2 {
                for b in &bodies {
                    host.observe(from, b, now, &fake);
                }
            } else {
                for b in &bodies[1..5] {
                    host.observe(from, b, now, &fake);
                }
            }
            if i % 100 == 0 {
                run(&mut host, &mut fake, 1, &mut now);
            }
        }
        let out = run(&mut host, &mut fake, 12, &mut now);
        let after = entries(&out, 0);
        assert!(taught(&out).is_empty());
        assert_eq!(after.len(), before.len());
        let ids = |l: &[WildEntry]| l.iter().map(|e| e.id).collect::<Vec<_>>();
        assert_eq!(ids(&after), ids(&before));
    }
}
