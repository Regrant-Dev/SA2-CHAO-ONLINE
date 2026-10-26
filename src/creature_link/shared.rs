//! Shared wild visitors (re/shared-visitors.md §4, §5, §7): in a garden the server runs, its
//! wild visitors and the partners players bring there are the server's (net/wild_host.rs), the
//! same for everyone in it. Each game shows them as copies, animal objects made as its own
//! visitors are (`scene::create`) and drawn from its own catalog, and only while the garden's
//! list has its catalog's visitor print (`net::wild_print`): without a catalog, with
//! SharedVisitors off or with another catalog there are none, nor a link machine.
//!
//! The player there longest (the picker) picks new visitors from their catalog when the server
//! asks (`WildAsk`, `WildPick`). Picking one up, carrying it about, putting it down, carrying one
//! in from elsewhere and bringing a partner from the link machine go through the server, the
//! first claim winning. A Chao that absorbs a copy takes it from everyone; the game that handed it
//! over never writes its own store: the Chao's owner's game is taught (`WildTaught`), when their
//! lock lets the giver change their Chao, and checks it against its own catalog first.

// Off the game (Linux builds) only the tests use the pure side.
#![cfg_attr(not(target_arch = "x86"), allow(dead_code))]

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use super::rotation::{self, Area};
use super::store::Partner;
use super::wild::{self, MAX_VISITORS, Placed, Recent, Rng, Visitor};
use crate::creature::rom::{ABILITY_COUNT, MOVE_COUNT, Rom, SPECIES_COUNT};
use crate::net::protocol::{
    Body, MAX_WILD_PICK, SERVER, WILD_MACHINE, WILD_PICK, WILD_SEE, WildArrive, WildCreature,
    WildOrigin, WildPicked, WildPlace,
};

/// config.ini `SharedVisitors`.
static ENABLED: AtomicBool = AtomicBool::new(true);

pub fn set_enabled(on: bool) {
    ENABLED.store(on, Ordering::Relaxed);
}

pub fn enabled() -> bool {
    ENABLED.load(Ordering::Relaxed)
}

/// A take, carry or battle the host doesn't answer this long after is given up.
const CLAIM_LIMIT: Duration = Duration::from_secs(3);
/// A copy let go of is put down (`WildPut`) once it's lain this long, and stays ours this long
/// after while the host takes it on.
const PUT_AFTER: Duration = Duration::from_secs(1);
const PUT_WAIT: Duration = Duration::from_millis(1500);
/// A copy in our hands that the host has on the ground this long is taken again.
const ODD_LIMIT: Duration = Duration::from_secs(1);

/// What `WildHello` says: (flags, print). Seeing needs the print; picking and the machine need
/// seeing.
pub fn hello(see: bool, pick: bool, machine: bool, print: Option<u32>) -> (u8, u32) {
    match print.filter(|&p| see && p != 0) {
        Some(p) => (
            WILD_SEE | if pick { WILD_PICK } else { 0 } | if machine { WILD_MACHINE } else { 0 },
            p,
        ),
        None => (0, 0),
    }
}

/// Whether `c` passes the wire's checks (a species, level, ability and moves the catalog can
/// have, at least one move and none twice).
pub fn wire_ok(c: &WildCreature) -> bool {
    let twice = (0..4).any(|i| c.moves[i] != 0 && c.moves[..i].contains(&c.moves[i]));
    (1..SPECIES_COUNT).contains(&c.species)
        && crate::creature::data::ordinary_form(crate::creature::data::SpeciesKey {
            species: c.species,
            form: c.form,
        })
        && (1..=100).contains(&c.level)
        && c.ability < ABILITY_COUNT
        && c.moves.iter().all(|&m| m < MOVE_COUNT)
        && c.moves != [0; 4]
        && !twice
}

/// A creature as ids: what `partner` is and teaches, and whether it's shiny (`None` when it
/// can't go on the wire).
pub fn creature(partner: &Partner, shiny: bool) -> Option<WildCreature> {
    let c = WildCreature {
        form: partner.form,
        species: partner.species,
        level: partner.level,
        ability: partner.ability,
        shiny,
        moves: partner.moves,
    };
    wire_ok(&c).then_some(c)
}

/// One of our own visitors as ids.
pub fn creature_of(visitor: &Visitor, partner: &Partner) -> Option<WildCreature> {
    creature(partner, visitor.shiny)
}

/// What a creature teaches, by our catalog's species (its types), with its own moves and ability.
pub fn partner_of(rom: &Rom, c: &WildCreature) -> Option<Partner> {
    let s = rom.species_form(crate::creature::data::SpeciesKey {
        species: c.species,
        form: c.form,
    })?;
    Some(Partner {
        form: c.form,
        species: c.species,
        types: s.types,
        moves: c.moves,
        level: c.level,
        ability: c.ability,
    })
}

/// A creature as one of our own visitors (its partner, from `partner_of`, has its moves).
pub fn visitor_of(c: &WildCreature) -> Visitor {
    Visitor {
        form: c.form,
        species: c.species,
        level: c.level,
        shiny: c.shiny,
        ability_slot: 0,
        seed: 0,
    }
}

/// What a Chao of ours learns from `WildTaught`, rebuilt from our catalog: `None` when the creature
/// doesn't fit it (an ability not its species', a wild one's move it can't know by its level, a
/// move the catalog hasn't).
pub fn taught(rom: &Rom, origin: WildOrigin, c: &WildCreature) -> Option<Partner> {
    if !wire_ok(c) {
        return None;
    }
    let species = rom.species_form(crate::creature::data::SpeciesKey {
        species: c.species,
        form: c.form,
    })?;
    if c.ability == 0 || !species.abilities.contains(&c.ability) {
        return None;
    }
    let mut moves = c.moves.iter().copied().filter(|&m| m != 0);
    let fits = match origin {
        WildOrigin::Wild => {
            let learnset = rom.learnset_form(crate::creature::data::SpeciesKey {
                species: c.species,
                form: c.form,
            });
            moves.all(|m| learnset.iter().any(|&(at, id)| id == m && at <= c.level))
        }
        WildOrigin::Partner => moves.all(|m| rom.move_info(m).is_some()),
    };
    fits.then(|| partner_of(rom, c)).flatten()
}

/// `species` comes in the wild at `level` somewhere in `areas` (a slot of it whose level range
/// has it).
pub fn in_the_wild(areas: &[Area], species: u16, level: u8) -> bool {
    areas.iter().any(|a| {
        a.slots()
            .any(|s| s.species == species && (s.min_level..=s.max_level).contains(&level))
    })
}

/// A shared visitor as our catalog has it: what it teaches (`taught`), a wild one also coming in
/// our catalog's wild `areas` at its level (none: it doesn't fit, nor show).
pub fn fits(
    rom: &Rom,
    areas: Option<&[Area]>,
    origin: WildOrigin,
    c: &WildCreature,
) -> Option<Partner> {
    if origin == WildOrigin::Wild && !areas.is_some_and(|a| in_the_wild(a, c.species, c.level)) {
        return None;
    }
    taught(rom, origin, c)
}

/// `partner` with an ability: one with none recorded (an older store's line) has its species'
/// first, as its battles do.
pub fn with_ability(rom: Option<&Rom>, mut partner: Partner) -> Partner {
    if partner.ability == 0
        && let Some(s) = rom.and_then(|r| {
            r.species_form(crate::creature::data::SpeciesKey {
                species: partner.species,
                form: partner.form,
            })
        })
    {
        partner.ability = super::ability_of(&s, 0);
    }
    partner
}

/// What the server asks the picker for (`Body::WildAsk`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Asked {
    pub section: Option<u8>,
    pub land: u8,
    pub pond: u8,
    pub fresh: bool,
    /// The server's clock window.
    pub slot: u32,
}

/// The picker's answer to `ask` for garden `garden`: the section the visitors come from (the
/// asked one, else the garden's area in the server's window) and the visitors: on a fresh ask
/// the handed-over ones first where they are (whatever area they came from), then picked from
/// that area's tables (the garden's `recent` species less often, `wild::pick`). `None` when
/// there's no area to pick from.
pub fn answer(
    areas: &[Area],
    rom: &Rom,
    garden: u8,
    ask: Asked,
    handover: Option<(u8, Vec<Placed>)>,
    recent: &mut Recent,
    rng: &mut Rng,
) -> Option<(u8, Vec<WildPicked>)> {
    let handover = handover.filter(|_| ask.fresh);
    let section = match ask.section {
        Some(s) => s,
        None => rotation::area_at(areas, garden, ask.slot)?.section,
    };
    let mut list = Vec::new();
    for p in handover.map(|(_, l)| l).unwrap_or_default() {
        if list.len() >= MAX_VISITORS {
            break;
        }
        let Some(creature) = creature_of(&p.visitor, &p.partner) else {
            continue;
        };
        let (at, angle) = (p.at, p.angle);
        let arrive = if p.swimming {
            WildArrive::SwimAt { at, angle }
        } else {
            WildArrive::LandAt { at, angle }
        };
        list.push(WildPicked { creature, arrive });
    }
    let Some(area) = areas.iter().find(|a| a.section == section) else {
        return Some((section, list));
    };
    let wanted = (usize::from(ask.land) + usize::from(ask.pond)).saturating_sub(list.len());
    let pond = usize::from(ask.pond).min(wanted);
    let land = (wanted - pond).min(usize::from(ask.land));
    let types = |s: u16| rom.species(s).map(|i| i.types);
    for in_pond in std::iter::repeat_n(true, pond).chain(std::iter::repeat_n(false, land)) {
        if list.len() >= MAX_WILD_PICK {
            break;
        }
        let Some(v) = wild::pick(area, in_pond, types, recent, rng) else {
            continue;
        };
        let Some(creature) = v.partner(rom).and_then(|p| creature_of(&v, &p)) else {
            continue;
        };
        let arrive = if in_pond {
            WildArrive::Pond
        } else {
            WildArrive::Land
        };
        list.push(WildPicked { creature, arrive });
    }
    Some((section, list))
}

/// The area a shared garden's title names: the list's, or while it has none (a new run, or the
/// server's window just changed) the garden's area in our own window `slot`.
pub fn shown_area(listed: Option<u8>, areas: Option<&[Area]>, garden: u8, slot: u32) -> Option<u8> {
    listed.or_else(|| Some(rotation::area_at(areas?, garden, slot)?.section))
}

/// Who has a copy as far as our game goes: nobody (it follows the list), or the player, from
/// picking it up (asking the host) to a moment after putting it down.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Hands {
    Free,
    /// Picked up then; the host hasn't said yes yet.
    Asked(Instant),
    /// In our hands, the host agreeing (since when the host has it elsewhere, if it does).
    Held {
        odd: Option<Instant>,
    },
    /// Let go of then (lying, falling or in a Chao's hands); put down (`WildPut`) then.
    LetGo {
        since: Instant,
        put: Option<Instant>,
    },
}

/// What our game sees of a copy now.
#[derive(Clone, Copy, Debug)]
pub struct Seen {
    pub in_hands: bool,
    /// A Chao holds it (taking it to absorb).
    pub by_chao: bool,
    /// Where the newest list has it.
    pub listed: WildPlace,
    /// The host said no to our taking it.
    pub refused: bool,
}

/// What to do for a copy (`Hands::step`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Do {
    Nothing,
    /// `WildTake`.
    Take,
    /// Drop it from the player's hands.
    Drop,
    /// `WildHeld` in our hands, with its offset.
    Hold,
    /// `WildHeld` let go of, with where it is.
    Fall,
    /// `WildPut` where it is.
    Put,
}

impl Hands {
    /// Our game moves it (not the list).
    pub fn local(self) -> bool {
        self != Self::Free
    }

    /// One report's step (about 10 a second) for player `me`.
    pub fn step(self, seen: Seen, me: u8, now: Instant) -> (Self, Do) {
        let ours = seen.listed == WildPlace::Held { by: me };
        let taken = match seen.listed {
            WildPlace::Held { by } => by != me,
            WildPlace::Battle { .. } => true,
            WildPlace::Roam | WildPlace::Swim => false,
        };
        let dropped = if seen.in_hands { Do::Drop } else { Do::Nothing };
        let since = |at: Instant| now.saturating_duration_since(at);
        let let_go = (
            Self::LetGo {
                since: now,
                put: None,
            },
            Do::Fall,
        );
        match self {
            Self::Free if seen.in_hands => (Self::Asked(now), Do::Take),
            // The host has it in our hands but we don't: it goes down here.
            Self::Free if ours => (Self::Free, Do::Put),
            Self::Free => (Self::Free, Do::Nothing),
            Self::Asked(_) if ours => {
                if seen.in_hands && !seen.by_chao {
                    (Self::Held { odd: None }, Do::Hold)
                } else {
                    let_go
                }
            }
            Self::Asked(at) if seen.refused || taken || since(at) > CLAIM_LIMIT => {
                (Self::Free, dropped)
            }
            Self::Asked(_) => (self, Do::Nothing),
            Self::Held { .. } if taken => (Self::Free, dropped),
            Self::Held { .. } if !seen.in_hands || seen.by_chao => let_go,
            Self::Held { .. } if ours => (Self::Held { odd: None }, Do::Hold),
            Self::Held { odd } => {
                // The host has it on the ground (it stopped hearing us): taken again after a
                // moment, as the list lags our reports.
                let odd = odd.unwrap_or(now);
                if since(odd) > ODD_LIMIT {
                    (Self::Asked(now), Do::Take)
                } else {
                    (Self::Held { odd: Some(odd) }, Do::Hold)
                }
            }
            Self::LetGo { .. } if taken => (Self::Free, dropped),
            Self::LetGo { put, .. } if seen.in_hands && !seen.by_chao => match put {
                None => (Self::Held { odd: None }, Do::Hold),
                Some(_) => (Self::Asked(now), Do::Take),
            },
            Self::LetGo { put: Some(at), .. } => {
                if since(at) >= PUT_WAIT {
                    (Self::Free, Do::Nothing)
                } else {
                    (self, Do::Nothing)
                }
            }
            Self::LetGo { .. } if seen.by_chao => let_go,
            Self::LetGo { since: at, .. } if since(at) >= PUT_AFTER => (
                Self::LetGo {
                    since: at,
                    put: Some(now),
                },
                Do::Put,
            ),
            Self::LetGo { .. } => (self, Do::Fall),
        }
    }
}

/// Each axis of a held copy's offset within what the wire allows.
pub fn clamp_offset(v: [f32; 3]) -> [f32; 3] {
    use crate::net::protocol::MAX_WILD_OFFSET;
    v.map(|x| {
        if x.is_finite() {
            x.clamp(-MAX_WILD_OFFSET, MAX_WILD_OFFSET)
        } else {
            0.0
        }
    })
}

/// The loaded catalog's visitor print, worked out once per catalog on a background thread: (the
/// catalog, its print).
static PRINT: Mutex<Option<(usize, Option<u32>)>> = Mutex::new(None);
static WORKING: AtomicBool = AtomicBool::new(false);

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/// The loaded catalog's visitor print (none while there's no catalog or it's being worked out).
pub fn print() -> Option<u32> {
    let rom = super::loaded_rom()?;
    let key = Arc::as_ptr(&rom) as usize;
    if let Some((k, print)) = *lock(&PRINT)
        && k == key
    {
        return print;
    }
    if !WORKING.swap(true, Ordering::Relaxed) {
        let spawned = std::thread::Builder::new()
            .name("visitor-print".into())
            .spawn(move || {
                let print = crate::net::wild_print::visitor_print(&rom);
                match print {
                    Some(p) => crate::log!("creature link: the catalog's visitor print is {p:08x}"),
                    None => {
                        crate::log!(
                            "creature link: no visitor print (the catalog's data can't be read)"
                        )
                    }
                }
                *lock(&PRINT) = Some((key, print));
                WORKING.store(false, Ordering::Relaxed);
            });
        if let Err(error) = spawned {
            // Tried again on a later frame; said once.
            static SAID: AtomicBool = AtomicBool::new(false);
            if !SAID.swap(true, Ordering::Relaxed) {
                crate::log!("creature link: can't start the visitor print worker: {error}");
            }
            WORKING.store(false, Ordering::Relaxed);
        }
    }
    None
}

/// The host's messages for us, read on the next frame.
static INBOX: Mutex<Vec<Body>> = Mutex::new(Vec::new());
const INBOX_MAX: usize = 256;
/// Ours to the host: (body, reliable).
static OUTBOX: Mutex<Vec<(Body, bool)>> = Mutex::new(Vec::new());
const OUTBOX_MAX: usize = 128;
/// `WildHello` goes again (connected anew).
static HELLO_AGAIN: AtomicBool = AtomicBool::new(true);

/// Network: one of the host's visitor messages (anyone else's is dropped).
pub fn received(from: u8, body: Body) {
    if from != SERVER
        || !matches!(
            body,
            Body::WildList { .. }
                | Body::WildAsk { .. }
                | Body::WildAnswer { .. }
                | Body::WildTaught { .. }
                | Body::WildUsed { .. }
        )
    {
        return;
    }
    let mut inbox = lock(&INBOX);
    if inbox.len() >= INBOX_MAX {
        inbox.remove(0);
    }
    inbox.push(body);
}

fn send(body: Body, reliable: bool) {
    let mut out = lock(&OUTBOX);
    if out.len() >= OUTBOX_MAX {
        out.remove(0);
    }
    out.push((body, reliable));
}

/// Network: what to send the host now.
pub fn take_outgoing() -> Vec<(Body, bool)> {
    std::mem::take(&mut *lock(&OUTBOX))
}

/// Network: connected (or disconnected): `WildHello` goes again.
pub fn connected() {
    HELLO_AGAIN.store(true, Ordering::Relaxed);
    lock(&OUTBOX).clear();
}

/// The garden we're in shows its shared visitors now (and so has the link machine).
static HERE: AtomicBool = AtomicBool::new(false);

pub fn here() -> bool {
    HERE.load(Ordering::Relaxed)
}

/// The garden we're in shows shared visitors' copies, a gap in its lists included (while the
/// copies wait to see whether the run goes on): the link machine stays meanwhile.
static SHOWN: AtomicBool = AtomicBool::new(false);

pub fn showing() -> bool {
    SHOWN.load(Ordering::Relaxed)
}

/// A partner of ours that comes into a shared garden: an import the host must take first.
pub struct Bring {
    pub key: u64,
    pub garden: u8,
    pub partner: Partner,
    pub name: String,
    pub at: Option<([f32; 3], i32)>,
}

#[cfg(target_arch = "x86")]
pub use game::{bring, is_copy, keeps_off_visitor, on_frame, take};

#[cfg(not(target_arch = "x86"))]
pub fn is_copy(_object: usize) -> bool {
    false
}

#[cfg(not(target_arch = "x86"))]
pub fn keeps_off_visitor(_chao: usize) -> bool {
    false
}

#[cfg(not(target_arch = "x86"))]
pub fn on_frame() {}

#[cfg(not(target_arch = "x86"))]
pub fn bring(_b: Bring) -> Result<(), String> {
    Err("Not in a shared garden".into())
}

#[cfg(target_arch = "x86")]
mod game {
    use std::time::{Duration, Instant};

    use super::super::scene::{
        D_MODE, D_POSITION, DELETE_OBJECT, MODE_CHAO, MODE_PLAYER, OBJ_DISPLAY, OBJ_MAIN, create,
        data1, hooked, quietly, room,
    };
    use super::super::wild_battle::{self, SharedCopy};
    use super::super::{ascii, loaded_rom, machine, wild, with_store, write_store};
    use super::*;
    use crate::game::{self as sa2, Vector};
    use crate::items::Walk;
    use crate::log;
    use crate::net::protocol::{WildEntry, WildRequest};

    /// A list this old (or none) and the copies go.
    const STALE: Duration = Duration::from_millis(1500);
    /// A garden still run with no list this long has no shared visitors for us.
    const RUN_ENDING: Duration = Duration::from_secs(4);
    /// Report what we hold every this many frames (about 10 a second).
    const REPORT_EVERY: u32 = 6;
    /// A visitor absorbed here isn't made again this long (the host takes it).
    const GONE_FOR: Duration = Duration::from_secs(3);
    /// A copy made from an answer waits this long for the list to have it.
    const LIST_GRACE: Duration = Duration::from_secs(1);
    /// A waiting partner the host had no room for is offered again this often.
    const BRING_AGAIN: Duration = Duration::from_secs(20);
    /// How long the area's name shows when entering the garden.
    const TITLE_SECS: u64 = 4;
    /// Where a visitor carried in sits in the player's frame until they report it.
    const CARRY_OFFSET: [f32; 3] = [0.0, 5.0, 2.0];
    /// Animal Data1 +0xC: facing (BAMS); Data2 (obj+0x38) +0: velocity.
    const D_ANGLE: usize = 0x0C;
    const OBJ_DATA2: usize = 0x38;

    struct List {
        area: u8,
        serial: u16,
        section: Option<u8>,
        learners: u16,
        entries: Vec<WildEntry>,
        heard: Instant,
    }

    struct Copy {
        id: u16,
        object: usize,
        /// The animal's own MainSub (ours wraps it).
        main: usize,
        entry: WildEntry,
        /// The last ground position the list had, and its walk.
        last: [f32; 3],
        walk: Walk,
        hands: Hands,
        refused: bool,
        born: Instant,
    }

    /// Carrying a visitor in: the object if it's our own in the player's hands (else the one
    /// carried from elsewhere), and its name.
    struct Carry {
        object: Option<usize>,
        name: String,
    }

    struct Shared {
        me: Option<u8>,
        hello: Option<(u8, u32)>,
        /// The garden we're in (0: none).
        garden: u8,
        list: Option<List>,
        copies: Vec<Copy>,
        gone: Vec<(u16, Instant)>,
        /// Listed visitors that don't fit our catalog (`fits`): never shown.
        unfit: Vec<u16>,
        frame: u32,
        carry: Option<Carry>,
        carry_tried: bool,
        /// Our waiting partners offered to this garden: (key, when, the answer).
        brought: Vec<(u64, Instant, Option<bool>)>,
        /// A partner from the link machine the host is asked to take, and when.
        machine: Option<(Bring, Instant)>,
        /// The area whose name showed last.
        titled: Option<u8>,
        /// The area's name, the line under it, and until when they show.
        title: Option<(String, String, Instant)>,
        rng: Rng,
    }

    // SAFETY: the object addresses are only used on the game thread.
    unsafe impl Send for Shared {}

    static SHARED: Mutex<Option<Shared>> = Mutex::new(None);

    fn with<R>(f: impl FnOnce(&mut Shared) -> R) -> R {
        let mut s = lock(&SHARED);
        let s = s.get_or_insert_with(|| {
            let seed = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(1, |d| d.as_nanos() as u64);
            Shared {
                me: None,
                hello: None,
                garden: 0,
                list: None,
                copies: Vec::new(),
                gone: Vec::new(),
                unfit: Vec::new(),
                frame: 0,
                carry: None,
                carry_tried: false,
                brought: Vec::new(),
                machine: None,
                titled: None,
                title: None,
                rng: Rng::new(seed ^ 0x5EED),
            }
        });
        f(s)
    }

    fn live_objects() -> Vec<usize> {
        crate::registry::entries(2)
            .into_iter()
            .map(|(_, o)| o)
            .collect()
    }

    /// The copy `object` (Data1 `d`) is in the player's hands.
    ///
    /// # Safety
    /// Game thread; `object` is live with Data1 `d` (or 0).
    unsafe fn in_hands(object: usize, d: usize) -> bool {
        // SAFETY: as the caller's.
        object == crate::garden::player_held()
            || (d != 0 && unsafe { ((d + D_MODE) as *const u8).read() } == MODE_PLAYER)
    }

    /// # Safety
    /// Game thread; `object` is a live animal.
    unsafe fn delete(object: usize) {
        // SAFETY: as the caller's; the game deletes it on its next pass.
        unsafe { ((object + OBJ_MAIN) as *mut usize).write(DELETE_OBJECT) };
    }

    /// Our game sees visitors (SharedVisitors on, the absorb hook in, the print known): its
    /// print.
    fn seeing() -> Option<u32> {
        if !enabled() || !hooked() {
            return None;
        }
        super::print()
    }

    pub fn on_frame() {
        let inbox = std::mem::take(&mut *lock(&INBOX));
        let me = crate::online::my_id();
        let shown = with(|s| {
            s.me = me;
            frame(s, inbox);
            s.copies
                .iter()
                .map(|c| SharedCopy {
                    object: c.object,
                    id: c.id,
                    free: !c.hands.local()
                        && matches!(c.entry.place, WildPlace::Roam | WildPlace::Swim),
                    wild: c.entry.bringer.is_none(),
                })
                .collect()
        });
        wild_battle::set_shared(shown);
    }

    fn frame(s: &mut Shared, inbox: Vec<Body>) {
        s.frame = s.frame.wrapping_add(1);
        let print = seeing();
        if HELLO_AGAIN.swap(false, Ordering::Relaxed) {
            s.hello = None;
        }
        let said = super::hello(print.is_some(), wild::enabled(), machine::enabled(), print);
        if s.me.is_some() && s.hello != Some(said) {
            log!(
                "creature link: shared visitors: flags {}, print {:08x}",
                said.0,
                said.1
            );
            send(
                Body::WildHello {
                    flags: said.0,
                    print: said.1,
                },
                true,
            );
            s.hello = Some(said);
        }
        let garden = if sa2::in_garden() && !sa2::between_areas() {
            sa2::current_chao_area() as u8
        } else {
            0
        };
        if garden != s.garden {
            leave(s);
            s.garden = garden;
        }
        for body in inbox {
            heard(s, body, print);
        }
        let fresh = s
            .list
            .as_ref()
            .is_some_and(|l| l.area == garden && l.heard.elapsed() < STALE);
        let on = garden != 0 && print.is_some() && fresh && crate::garden::server_runs();
        HERE.store(on, Ordering::Relaxed);
        if !on {
            // The server's Chao stop a moment after its lists when a run ends: the copies wait
            // for that (to stay as ours in a garden that is ours again).
            let ending = print.is_some()
                && crate::garden::server_runs()
                && s.list
                    .as_ref()
                    .is_some_and(|l| l.heard.elapsed() < RUN_ENDING);
            SHOWN.store(garden != 0 && ending, Ordering::Relaxed);
            if garden != 0 && !ending && (s.list.is_some() || !s.copies.is_empty()) {
                ended(s, garden);
            }
            return;
        }
        SHOWN.store(true, Ordering::Relaxed);
        let Some(me) = s.me else { return };
        sync(s, me);
        if s.frame.is_multiple_of(REPORT_EVERY) {
            hands(s, me);
        }
        carry_in(s, garden);
        bring_waiting(s, garden);
        title(s);
    }

    /// One of the host's messages.
    fn heard(s: &mut Shared, body: Body, print: Option<u32>) {
        match body {
            Body::WildList {
                area,
                serial,
                section,
                print: of,
                learners,
                list,
                ..
            } => {
                if area != s.garden || print.is_none() || (of != 0 && Some(of) != print) {
                    return;
                }
                // One sent before the newest we have, arriving late.
                let older = s.list.as_ref().is_some_and(|l| {
                    l.area == area
                        && l.heard.elapsed() < STALE
                        && serial.wrapping_sub(l.serial) as i16 <= 0
                });
                if older {
                    return;
                }
                s.list = Some(List {
                    area,
                    serial,
                    section,
                    learners,
                    entries: list,
                    heard: Instant::now(),
                });
            }
            Body::WildAsk {
                area,
                ask,
                section,
                land,
                pond,
                fresh,
                slot,
            } => {
                if area != s.garden || print.is_none() || !wild::enabled() {
                    return;
                }
                let (Some(rom), Some(areas)) = (loaded_rom(), wild::areas()) else {
                    return;
                };
                let asked = Asked {
                    section,
                    land,
                    pond,
                    fresh,
                    slot,
                };
                let handover = if fresh {
                    wild::take_handover(area)
                } else {
                    None
                };
                let rng = &mut s.rng;
                let Some((section, list)) = wild::with_recent(area, |recent| {
                    super::answer(&areas, &rom, area, asked, handover, recent, rng)
                }) else {
                    return;
                };
                log!(
                    "creature link: picked {} shared visitors for garden {area} from section {section}",
                    list.len()
                );
                send(
                    Body::WildPick {
                        area,
                        ask,
                        section,
                        list,
                    },
                    true,
                );
            }
            Body::WildAnswer { kind, id, key, ok } => answered(s, kind, id, key, ok),
            Body::WildTaught {
                identity,
                origin,
                creature,
            } => learn(identity, origin, &creature),
            Body::WildUsed { garden, key } => {
                s.brought.retain(|b| b.0 != key);
                if with_store(|st| st.drop_waiting(garden, key)) {
                    write_store();
                    log!("creature link: our partner waiting in garden {garden} was absorbed");
                }
            }
            _ => {}
        }
    }

    fn answered(s: &mut Shared, kind: WildRequest, id: u16, key: u64, ok: bool) {
        match kind {
            WildRequest::Take => {
                if let Some(c) = s.copies.iter_mut().find(|c| c.id == id)
                    && !ok
                {
                    c.refused = true;
                }
            }
            WildRequest::Carry => {
                let Some(carry) = s.carry.take() else { return };
                s.carry_tried = true;
                if ok {
                    carried_in(s, id, carry);
                } else if carry.object.is_none() {
                    crate::hud::toast(format!(
                        "No room for {} here: it waits for your next garden",
                        carry.name
                    ));
                }
            }
            WildRequest::Bring => {
                if let Some((b, _)) = s.machine.take_if(|(b, _)| b.key == key) {
                    machine_answer(s, b, ok);
                } else if let Some(b) = s.brought.iter_mut().find(|b| b.0 == key) {
                    b.2 = Some(ok);
                }
            }
            WildRequest::Battle => wild_battle::answered(id, ok),
        }
    }

    /// The host took (or didn't) the partner from the link machine: only now is it recorded as
    /// brought (the waiting entry and the day's import).
    fn machine_answer(s: &mut Shared, b: Bring, ok: bool) {
        if !ok {
            crate::hud::toast("The garden is full of animals right now");
            return;
        }
        let day = super::super::today();
        if with_store(|st| st.import(b.key, b.garden, day, b.partner)).is_ok() {
            write_store();
        }
        s.brought.retain(|x| x.0 != b.key);
        s.brought.push((b.key, Instant::now(), Some(true)));
        log!(
            "creature link: species {} (level {}) came to shared garden {}",
            b.partner.species,
            b.partner.level,
            b.garden
        );
        crate::hud::toast(format!("{} came out of the link machine", b.name));
    }

    /// The host has the visitor we carried in (`id`) in our hands.
    fn carried_in(s: &mut Shared, id: u16, carry: Carry) {
        let now = Instant::now();
        let Some(rom) = loaded_rom() else { return };
        let made = match carry.object {
            // Our own in the player's hands: the same object is the copy from now on.
            Some(object) => {
                if !live_objects().contains(&object) {
                    return;
                }
                // SAFETY: game thread; the object is live (in the registry this frame).
                if !unsafe { in_hands(object, data1(object)) } {
                    return;
                }
                let Some((main, visitor, partner)) = wild::release(object) else {
                    return;
                };
                // SAFETY: our live animal (`scene::create`), on the game thread.
                unsafe {
                    ((object + OBJ_MAIN) as *mut usize)
                        .write(copy_main as extern "C" fn(usize) as usize);
                    ((object + OBJ_DISPLAY) as *mut usize)
                        .write(copy_display as extern "C" fn(usize) as usize);
                }
                super::creature_of(&visitor, &partner).map(|c| (object, main, c))
            }
            // One carried from elsewhere: made in front of the player and given to them.
            None => {
                let Some((visitor, partner)) = wild::pending() else {
                    return;
                };
                wild::take_pending();
                let creature = super::creature_of(&visitor, &partner);
                let Some(c) = creature.filter(|c| rom.species(c.species).is_some()) else {
                    return;
                };
                let Some((player, angle)) = sa2::player_position() else {
                    return;
                };
                if !room(0) {
                    return;
                }
                let a = angle as f32 * std::f32::consts::TAU / 65536.0;
                let at = Vector {
                    x: player.x + 3.0 * a.sin(),
                    y: player.y,
                    z: player.z + 3.0 * a.cos(),
                };
                // SAFETY: game thread, in a running garden with room in category 2.
                let Some((object, main)) =
                    (unsafe { create(at, angle, [0.0; 3], copy_main, copy_display) })
                else {
                    return;
                };
                // SAFETY: our live object, on the game thread.
                unsafe { wild::give_to_player(object) };
                crate::hud::toast(format!("{} came along", carry.name));
                Some((object, main, c))
            }
        };
        let Some((object, main, creature)) = made else {
            return;
        };
        log!("creature link: carried a visitor (shared {id}) into the garden");
        let me = s.me.unwrap_or(SERVER);
        // A copy the list made of it meanwhile goes: this one is it.
        let held = crate::garden::player_held();
        let live = live_objects();
        s.copies.retain(|c| {
            if c.id != id || c.object == object {
                return c.id != id;
            }
            if live.contains(&c.object) {
                // SAFETY: our live object, on the game thread.
                unsafe {
                    if c.object == held {
                        crate::items::drop_held();
                    }
                    delete(c.object);
                }
            }
            false
        });
        s.copies.push(Copy {
            id,
            object,
            main,
            entry: WildEntry {
                id,
                creature,
                bringer: None,
                place: WildPlace::Held { by: me },
                v: CARRY_OFFSET,
                angle: 0,
            },
            last: [0.0; 3],
            walk: Walk::new([0.0; 3], now),
            hands: Hands::Held { odd: None },
            refused: false,
            born: now,
        });
    }

    /// `WildTaught`: one of our Chao absorbed a shared visitor (in anyone's game): it learns it,
    /// if it's one of ours and the creature fits our catalog.
    fn learn(identity: u64, origin: WildOrigin, creature: &WildCreature) {
        let Some(rom) = loaded_rom() else { return };
        let Some((_, name)) = super::super::own_chao()
            .into_iter()
            .find(|(id, _)| *id == identity)
        else {
            log!("creature link: taught a Chao that isn't ours ({identity:016x}); left out");
            return;
        };
        let areas = wild::areas();
        let Some(partner) =
            super::fits(&rom, areas.as_deref().map(Vec::as_slice), origin, creature)
        else {
            log!(
                "creature link: a shared visitor (species {}) doesn't fit our catalog; nothing learned",
                creature.species
            );
            return;
        };
        super::super::learned_wild(identity, partner, &name);
    }

    /// Out of the garden: a wild visitor in the player's hands comes along to the next one (the
    /// host takes it out of this one once we're elsewhere); the copies go with the garden.
    fn leave(s: &mut Shared) {
        if let (Some(rom), Some(me)) = (loaded_rom(), s.me) {
            let areas = wild::areas();
            let areas = areas.as_deref().map(Vec::as_slice);
            for c in &s.copies {
                // In our hands for us and for the host alike.
                if matches!(c.hands, Hands::Held { .. })
                    && c.entry.place == (WildPlace::Held { by: me })
                    && c.entry.bringer.is_none()
                    && let Some(partner) =
                        super::fits(&rom, areas, WildOrigin::Wild, &c.entry.creature)
                {
                    log!("creature link: carrying shared visitor {} out", c.id);
                    wild::carry_out(super::visitor_of(&c.entry.creature), partner);
                }
            }
        }
        // (Ones still running delete themselves in `copy_main`.)
        s.copies.clear();
        reset(s);
        HERE.store(false, Ordering::Relaxed);
        SHOWN.store(false, Ordering::Relaxed);
    }

    fn reset(s: &mut Shared) {
        s.list = None;
        s.gone.clear();
        s.unfit.clear();
        s.carry = None;
        s.carry_tried = false;
        s.brought.clear();
        s.machine = None;
        s.titled = None;
        s.title = None;
    }

    /// The garden's shared visitors stopped (the server no longer runs it, or doesn't send us
    /// its list): back in a garden of their own, the player's game keeps the wild ones as its own
    /// visitors; otherwise the copies go.
    fn ended(s: &mut Shared, garden: u8) {
        let alone = !crate::garden::server_runs()
            && !crate::bring::friends_in(i32::from(garden))
            && wild::enabled();
        let section = s.list.as_ref().and_then(|l| l.section);
        let live = live_objects();
        let held = crate::garden::player_held();
        let rom = loaded_rom();
        let areas = wild::areas();
        let areas = areas.as_deref().map(Vec::as_slice);
        let mut adopted = 0;
        for c in std::mem::take(&mut s.copies) {
            if !live.contains(&c.object) {
                continue;
            }
            let others = matches!(
                c.entry.place,
                WildPlace::Held { by } | WildPlace::Battle { by } if Some(by) != s.me
            );
            let partner = rom
                .as_ref()
                .and_then(|r| super::fits(r, areas, WildOrigin::Wild, &c.entry.creature));
            if alone
                && c.entry.bringer.is_none()
                && !others
                && let Some(partner) = partner
            {
                // SAFETY: our live animal, on the game thread.
                unsafe {
                    wild::adopt(
                        c.object,
                        c.main,
                        super::visitor_of(&c.entry.creature),
                        partner,
                    )
                };
                adopted += 1;
                continue;
            }
            // SAFETY: our live object, on the game thread.
            unsafe {
                if c.object == held {
                    crate::items::drop_held();
                }
                delete(c.object);
            }
        }
        if adopted > 0 {
            log!("creature link: the garden is ours again: {adopted} shared visitors stay");
            wild::adopt_visit(garden, section);
        }
        reset(s);
    }

    /// The copies follow the newest list: new ones made, gone ones deleted.
    fn sync(s: &mut Shared, me: u8) {
        let now = Instant::now();
        s.gone.retain(|(_, at)| at.elapsed() < GONE_FOR);
        let live = live_objects();
        s.copies.retain(|c| live.contains(&c.object));
        let Some(list) = s.list.as_ref() else { return };
        let held = crate::garden::player_held();
        let mut keep = Vec::new();
        for mut c in std::mem::take(&mut s.copies) {
            match list.entries.iter().find(|e| e.id == c.id) {
                Some(e) => {
                    if matches!(e.place, WildPlace::Roam | WildPlace::Swim) && e.v != c.last {
                        c.walk.report(e.v, now);
                        c.last = e.v;
                    }
                    c.entry = *e;
                    keep.push(c);
                }
                None if c.born.elapsed() < LIST_GRACE => keep.push(c),
                // Gone for everyone (absorbed elsewhere, carried out, ran away).
                None => {
                    // SAFETY: our live object, on the game thread.
                    unsafe {
                        if c.object == held {
                            crate::items::drop_held();
                        }
                        delete(c.object);
                    }
                }
            }
        }
        s.copies = keep;
        let Some(rom) = loaded_rom() else { return };
        let carrying = s.carry.is_some();
        let new: Vec<&WildEntry> = list
            .entries
            .iter()
            .filter(|e| !s.copies.iter().any(|c| c.id == e.id))
            .filter(|e| !s.gone.iter().any(|(id, _)| *id == e.id))
            .filter(|e| !s.unfit.contains(&e.id))
            // The one being carried in is made from its answer.
            .filter(|e| !(carrying && e.place == WildPlace::Held { by: me }))
            .collect();
        let areas = if new.is_empty() { None } else { wild::areas() };
        let areas = areas.as_deref().map(Vec::as_slice);
        let mut unfit = Vec::new();
        let entries: Vec<WildEntry> = new
            .into_iter()
            .filter(|e| {
                let origin = match e.bringer {
                    None => WildOrigin::Wild,
                    Some(_) => WildOrigin::Partner,
                };
                let fits = super::fits(&rom, areas, origin, &e.creature).is_some();
                if !fits {
                    log!(
                        "creature link: shared visitor {} (species {}) doesn't fit our catalog; not shown",
                        e.id,
                        e.creature.species
                    );
                    unfit.push(e.id);
                }
                fits
            })
            .copied()
            .collect();
        s.unfit
            .retain(|id| list.entries.iter().any(|e| e.id == *id));
        s.unfit.extend(unfit);
        for e in entries {
            if !room(wild::RESERVE) {
                break;
            }
            let at = match e.place {
                WildPlace::Held { by } if by == me => {
                    let Some((p, _)) = sa2::player_position() else {
                        continue;
                    };
                    p
                }
                WildPlace::Held { by } => {
                    let Some((p, angle)) = crate::garden::holder(by) else {
                        continue;
                    };
                    crate::garden::in_hands(p, angle, e.v)
                }
                _ => Vector {
                    x: e.v[0],
                    y: e.v[1],
                    z: e.v[2],
                },
            };
            // SAFETY: game thread, in a running garden with room in category 2.
            let Some((object, main)) =
                (unsafe { create(at, i32::from(e.angle), [0.0; 3], copy_main, copy_display) })
            else {
                continue;
            };
            s.copies.push(Copy {
                id: e.id,
                object,
                main,
                entry: e,
                last: e.v,
                walk: Walk::new(e.v, now),
                hands: Hands::Free,
                refused: false,
                born: now,
            });
        }
    }

    /// What the player does with the copies, reported to the host.
    fn hands(s: &mut Shared, me: u8) {
        let now = Instant::now();
        let player = sa2::player_position();
        for c in &mut s.copies {
            // SAFETY: our live copy (checked against the registry this frame), game thread.
            let (in_hands, mode, at, angle) = unsafe {
                let d = data1(c.object);
                if d == 0 {
                    continue;
                }
                (
                    in_hands(c.object, d),
                    ((d + D_MODE) as *const u8).read(),
                    ((d + D_POSITION) as *const [f32; 3]).read_unaligned(),
                    ((d + D_ANGLE) as *const i32).read_unaligned(),
                )
            };
            let seen = Seen {
                in_hands,
                by_chao: mode == MODE_CHAO,
                listed: c.entry.place,
                refused: std::mem::take(&mut c.refused),
            };
            let (next, act) = c.hands.step(seen, me, now);
            c.hands = next;
            let id = c.id;
            match act {
                Do::Nothing => {}
                Do::Take => send(Body::WildTake { id }, true),
                // SAFETY: game thread.
                Do::Drop => unsafe { crate::items::drop_held() },
                Do::Hold => {
                    if let Some((p, angle)) = player {
                        let offset = crate::garden::rotate(
                            [at[0] - p.x, at[1] - p.y, at[2] - p.z],
                            angle.wrapping_neg(),
                        );
                        let v = super::clamp_offset(offset);
                        let body = Body::WildHeld {
                            id,
                            let_go: false,
                            v,
                        };
                        send(body, false);
                    }
                }
                Do::Fall => send(
                    Body::WildHeld {
                        id,
                        let_go: true,
                        v: at,
                    },
                    false,
                ),
                Do::Put => send(
                    Body::WildPut {
                        id,
                        at,
                        angle: angle as u16,
                    },
                    true,
                ),
            }
        }
    }

    /// A visitor the player carries in (from another garden, or their own in hand when the
    /// garden turned shared) goes into the garden's list, once a visit.
    fn carry_in(s: &mut Shared, garden: u8) {
        // (One asked waits for its answer, however late: the host may have it already.)
        if s.carry.is_some() || s.carry_tried {
            return;
        }
        let rom = loaded_rom();
        let name = |species: u16| {
            rom.as_ref()
                .and_then(|r| r.species_name(species))
                .map(|n| ascii(&n))
                .unwrap_or_else(|| "Your visitor".into())
        };
        let (object, visitor, partner, offset) = match wild::local_in_hands() {
            Some((object, visitor, partner)) => {
                let (Some((p, angle)), Some(at)) = (sa2::player_position(), position(object))
                else {
                    return;
                };
                let offset = crate::garden::rotate(
                    [at[0] - p.x, at[1] - p.y, at[2] - p.z],
                    angle.wrapping_neg(),
                );
                (Some(object), visitor, partner, super::clamp_offset(offset))
            }
            None => match wild::pending() {
                Some((visitor, partner)) if crate::garden::player_held() == 0 => {
                    (None, visitor, partner, CARRY_OFFSET)
                }
                _ => return,
            },
        };
        let Some(creature) = super::creature_of(&visitor, &partner) else {
            s.carry_tried = true;
            return;
        };
        send(
            Body::WildCarry {
                area: garden,
                creature,
                offset,
            },
            true,
        );
        s.carry = Some(Carry {
            object,
            name: name(visitor.species),
        });
    }

    fn position(object: usize) -> Option<[f32; 3]> {
        if !live_objects().contains(&object) {
            return None;
        }
        // SAFETY: a live object (in the registry this frame), on the game thread.
        unsafe {
            let d = data1(object);
            (d != 0).then(|| ((d + D_POSITION) as *const [f32; 3]).read_unaligned())
        }
    }

    /// Our partners waiting in this garden come into its list (the host puts them on the
    /// grass); one it had no room for is offered again a while later.
    fn bring_waiting(s: &mut Shared, garden: u8) {
        if !machine::enabled() {
            return;
        }
        let rom = loaded_rom();
        let waiting: Vec<(u64, Partner)> = with_store(|st| {
            st.waiting_in(garden)
                .map(|w| (w.key, super::with_ability(rom.as_deref(), w.partner)))
                .collect()
        });
        let now = Instant::now();
        for (key, partner) in waiting {
            let asked = s.brought.iter().find(|b| b.0 == key);
            if asked.is_some_and(|b| b.2 != Some(false) || b.1.elapsed() < BRING_AGAIN)
                || s.machine.as_ref().is_some_and(|(b, _)| b.key == key)
            {
                continue;
            }
            let Some(creature) = super::creature(&partner, false) else {
                continue;
            };
            send(
                Body::WildBring {
                    area: garden,
                    key,
                    creature,
                    at: None,
                },
                true,
            );
            s.brought.retain(|b| b.0 != key);
            s.brought.push((key, now, None));
        }
    }

    /// The garden's area, on entering and when it changes (`shown_area`).
    fn title(s: &mut Shared) {
        let now = Instant::now();
        let section = s.list.as_ref().and_then(|l| {
            let areas = if l.section.is_none() {
                wild::areas()
            } else {
                None
            };
            let areas = areas.as_deref().map(Vec::as_slice);
            super::shown_area(l.section, areas, s.garden, rotation::slot_now())
        });
        if let Some(section) = section
            && s.titled != Some(section)
            && let Some(rom) = loaded_rom()
        {
            s.titled = Some(section);
            let name = ascii(&rom.map_section_name(section).unwrap_or_default());
            let line = rotation::until_next(rotation::unix_now());
            if !name.is_empty() {
                s.title = Some((name, line, now + Duration::from_secs(TITLE_SECS)));
            }
        }
        if let Some((name, line, until)) = &s.title {
            if now < *until {
                crate::hud::top_title(name, line);
            } else {
                s.title = None;
            }
        }
    }

    /// A partner of ours comes into the shared garden we're in (from the link machine): the
    /// host is asked, and only once it takes it is it recorded as brought.
    pub fn bring(b: Bring) -> Result<(), String> {
        if !super::here() {
            return Err("The garden's visitors aren't here yet".into());
        }
        let partner = super::with_ability(loaded_rom().as_deref(), b.partner);
        let creature =
            super::creature(&partner, false).ok_or("This one can't come to a shared garden")?;
        with(|s| {
            if s.machine
                .as_ref()
                .is_some_and(|(_, at)| at.elapsed() < CLAIM_LIMIT)
            {
                return Err("The link machine is busy".to_string());
            }
            send(
                Body::WildBring {
                    area: b.garden,
                    key: b.key,
                    creature,
                    at: b.at.map(|(at, angle)| (at, angle as u16)),
                },
                true,
            );
            s.machine = Some((b, Instant::now()));
            Ok(())
        })
    }

    /// One of the shared visitors' copies.
    pub fn is_copy(object: usize) -> bool {
        object != 0 && with(|s| s.copies.iter().any(|c| c.object == object))
    }

    /// The player holds a shared visitor, and Chao `chao` can't learn from it (its owner doesn't
    /// see these visitors, or we can't tell whose it is): it keeps off it.
    pub fn keeps_off_visitor(chao: usize) -> bool {
        let held = crate::garden::player_held();
        if held == 0 {
            return false;
        }
        let learners = with(|s| {
            s.copies
                .iter()
                .any(|c| c.object == held)
                .then(|| s.list.as_ref().map_or(0, |l| l.learners))
        });
        let Some(learners) = learners else {
            return false;
        };
        match crate::garden::chao_of(chao) {
            Some((owner, _)) if owner < 16 => learners & (1 << owner) == 0,
            _ => true,
        }
    }

    /// A Chao (`chao`) absorbs the copy `animal`: the host is told (it takes it from everyone and
    /// teaches the Chao's owner); our store is never written. The copy's own main, to put back.
    pub fn take(chao: usize, animal: usize) -> Option<usize> {
        if !is_copy(animal) {
            return None;
        }
        let key = crate::garden::chao_of(chao);
        with(|s| {
            let i = s.copies.iter().position(|c| c.object == animal)?;
            let c = s.copies.remove(i);
            s.gone.push((c.id, Instant::now()));
            match key {
                Some((owner, index)) if owner < 16 && index < 4 => {
                    log!(
                        "creature link: Chao ({owner}, {index}) absorbs shared visitor {} (species {})",
                        c.id,
                        c.entry.creature.species
                    );
                    send(
                        Body::WildAbsorbed {
                            id: c.id,
                            owner,
                            index,
                        },
                        true,
                    );
                }
                _ => log!(
                    "creature link: a Chao we can't name absorbs shared visitor {}",
                    c.id
                ),
            }
            Some(c.main)
        })
    }

    /// How a copy shows when nobody here moves it.
    #[derive(Clone, Copy)]
    enum Show {
        /// Walking on the ground along the list.
        Walk,
        Swim([f32; 3], u16),
        /// In another player's hands.
        Hands(u8, [f32; 3]),
        /// Standing where it is (battled).
        Stand([f32; 3], u16),
    }

    fn show(e: &WildEntry) -> Show {
        match e.place {
            WildPlace::Roam => Show::Walk,
            WildPlace::Swim => Show::Swim(e.v, e.angle),
            WildPlace::Held { by } => Show::Hands(by, e.v),
            WildPlace::Battle { .. } => Show::Stand(e.v, e.angle),
        }
    }

    /// MainSub of a copy: the animal's own main (silenced) for its motions; unless the player
    /// has it, it's kept from wandering and put where the list has it.
    extern "C" fn copy_main(object: usize) {
        let step = std::panic::catch_unwind(|| {
            with(|s| {
                let c = s.copies.iter().find(|c| c.object == object)?;
                Some((c.main, c.hands, c.entry, c.walk, c.born))
            })
        })
        .ok()
        .flatten();
        let Some((main, hands, entry, walk, born)) = step else {
            // Not ours any more: let the game delete it.
            // SAFETY: the object the game runs.
            unsafe { delete(object) };
            return;
        };
        // SAFETY: the object the game runs, on the game thread.
        let d = unsafe { data1(object) };
        // SAFETY: as above.
        let local = hands.local() || unsafe { in_hands(object, d) };
        let show = (!local && d != 0).then(|| show(&entry));
        if matches!(show, Some(Show::Walk | Show::Stand(..))) {
            // SAFETY: an animal's Data1, on the game thread.
            unsafe { crate::items::hold_still(d) };
        }
        if !matches!(show, Some(Show::Hands(..))) {
            // SAFETY: the animal's own main with its object, on the game thread.
            quietly(|| unsafe { std::mem::transmute::<usize, extern "C" fn(usize)>(main)(object) });
        }
        // SAFETY: the object stays readable until the game's next pass; its main may have
        // deleted it (MainSub changed) or rebuilt its Data1.
        unsafe {
            if ((object + OBJ_MAIN) as *const usize).read()
                != copy_main as extern "C" fn(usize) as usize
                || data1(object) != d
            {
                return;
            }
        }
        let Some(show) = show else { return };
        let now = Instant::now();
        // SAFETY: still the live animal with Data1 `d`, on the game thread.
        unsafe {
            let still = |at: [f32; 3], angle: u16| {
                ((d + D_POSITION) as *mut [f32; 3]).write_unaligned(at);
                ((d + D_ANGLE) as *mut i32).write_unaligned(i32::from(angle));
                let d2 = ((object + OBJ_DATA2) as *const usize).read();
                if d2 != 0 {
                    (d2 as *mut [f32; 3]).write_unaligned([0.0; 3]);
                }
            };
            match show {
                Show::Walk => crate::items::walk_with(d, &walk, now),
                Show::Swim(v, angle) => {
                    let y = wild::swim_y(v[1], now.duration_since(born).as_secs_f32());
                    still([v[0], y, v[2]], angle);
                }
                Show::Hands(by, offset) => {
                    if let Some((p, angle)) = crate::garden::holder(by) {
                        let at = crate::garden::in_hands(p, angle, offset);
                        ((d + D_POSITION) as *mut Vector).write_unaligned(at);
                    }
                }
                Show::Stand(v, angle) => still(v, angle),
            }
        }
    }

    /// Draws the copy where the animal is. A panic must not unwind into the game.
    extern "C" fn copy_display(object: usize) {
        let _ = std::panic::catch_unwind(|| {
            let found = with(|s| {
                s.copies.iter().find(|c| c.object == object).map(|c| {
                    let held = matches!(c.entry.place, WildPlace::Held { .. });
                    (
                        c.entry.creature.species,
                        c.entry.creature.form,
                        c.entry.creature.shiny,
                        held,
                    )
                })
            });
            let Some((species, form, shiny, held_listed)) = found else {
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
                    ((d + D_MODE) as *const u8).read() == MODE_PLAYER,
                )
            };
            super::super::billboard::draw_form(
                object,
                species,
                form,
                shiny,
                at,
                heading,
                held || held_listed,
            );
        });
    }

    /// The copies for their info card: (object, position, its list entry).
    pub fn creatures() -> Vec<(usize, [f32; 3], WildEntry)> {
        let live = live_objects();
        with(|s| {
            s.copies
                .iter()
                .filter(|c| live.contains(&c.object))
                .filter_map(|c| {
                    // SAFETY: our live copy's Data1, on the game thread.
                    let at = unsafe {
                        let d = data1(c.object);
                        if d == 0 {
                            return None;
                        }
                        ((d + D_POSITION) as *const [f32; 3]).read_unaligned()
                    };
                    Some((c.object, at, c.entry))
                })
                .collect()
        })
    }

    /// Our player id, as the copies' list knows it.
    pub fn me() -> Option<u8> {
        with(|s| s.me)
    }
}

#[cfg(target_arch = "x86")]
pub use game::{creatures, me};

#[cfg(test)]
mod tests {
    use super::*;
    use crate::creature::rom::Species;

    fn partner(moves: [u16; 4]) -> Partner {
        Partner {
            form: 0,
            species: 25,
            types: [13, 13],
            moves,
            level: 12,
            ability: 9,
        }
    }

    #[test]
    fn hello_needs_the_print_to_see_and_seeing_to_pick() {
        assert_eq!(
            hello(true, true, true, Some(7)),
            (WILD_SEE | WILD_PICK | WILD_MACHINE, 7)
        );
        assert_eq!(
            hello(true, false, true, Some(7)),
            (WILD_SEE | WILD_MACHINE, 7)
        );
        assert_eq!(hello(true, true, true, None), (0, 0));
        assert_eq!(hello(false, true, true, Some(7)), (0, 0));
    }

    #[test]
    fn only_creatures_the_wire_takes_are_sent() {
        let c = creature(&partner([10, 20, 0, 0]), true).unwrap();
        assert_eq!((c.species, c.level, c.ability, c.shiny), (25, 12, 9, true));
        assert_eq!(c.moves, [10, 20, 0, 0]);
        assert!(creature(&partner([0; 4]), false).is_none());
        assert!(creature(&partner([10, 10, 0, 0]), false).is_none());
        assert!(creature(&partner([MOVE_COUNT, 0, 0, 0]), false).is_none());
        let mut p = partner([1, 0, 0, 0]);
        p.level = 0;
        assert!(creature(&p, false).is_none());
        p.level = 5;
        p.species = SPECIES_COUNT;
        assert!(creature(&p, false).is_none());
        assert_eq!(clamp_offset([20.0, -20.0, f32::NAN]), [10.0, -10.0, 0.0]);
    }

    const ME: u8 = 2;

    fn seen(in_hands: bool, listed: WildPlace) -> Seen {
        Seen {
            in_hands,
            by_chao: false,
            listed,
            refused: false,
        }
    }

    #[test]
    fn picking_a_copy_up_asks_the_host_and_its_no_drops_it() {
        let t = Instant::now();
        let mine = WildPlace::Held { by: ME };
        let (h, act) = Hands::Free.step(seen(true, WildPlace::Roam), ME, t);
        assert_eq!((h, act), (Hands::Asked(t), Do::Take));
        // Still waiting.
        assert_eq!(h.step(seen(true, WildPlace::Roam), ME, t).1, Do::Nothing);
        // Yes: ours.
        let (held, act) = h.step(seen(true, mine), ME, t);
        assert_eq!((held, act), (Hands::Held { odd: None }, Do::Hold));
        // No, someone else has it, or no answer: dropped.
        let no = Seen {
            refused: true,
            ..seen(true, WildPlace::Roam)
        };
        assert_eq!(h.step(no, ME, t), (Hands::Free, Do::Drop));
        let theirs = WildPlace::Held { by: 5 };
        assert_eq!(h.step(seen(true, theirs), ME, t), (Hands::Free, Do::Drop));
        let battled = WildPlace::Battle { by: 5 };
        assert_eq!(h.step(seen(true, battled), ME, t), (Hands::Free, Do::Drop));
        let late = t + CLAIM_LIMIT + Duration::from_millis(1);
        assert_eq!(
            h.step(seen(true, WildPlace::Roam), ME, late),
            (Hands::Free, Do::Drop)
        );
        // A yes for one we no longer hold: it goes down.
        assert_eq!(
            Hands::Free.step(seen(false, mine), ME, t),
            (Hands::Free, Do::Put)
        );
        assert_eq!(
            Hands::Free.step(seen(false, WildPlace::Roam), ME, t),
            (Hands::Free, Do::Nothing)
        );
    }

    #[test]
    fn a_copy_let_go_of_is_put_down_after_a_moment_unless_a_chao_has_it() {
        let t = Instant::now();
        let mine = WildPlace::Held { by: ME };
        let held = Hands::Held { odd: None };
        let (h, act) = held.step(seen(false, mine), ME, t);
        assert_eq!(act, Do::Fall);
        // Falling: reported where it is; the list shows it on the ground meanwhile.
        assert_eq!(h.step(seen(false, WildPlace::Roam), ME, t).1, Do::Fall);
        // A Chao takes it to absorb: still let go of, not put down.
        let chao = Seen {
            by_chao: true,
            ..seen(false, WildPlace::Roam)
        };
        let later = t + PUT_AFTER * 3;
        let (h2, act) = h.step(chao, ME, later);
        assert_eq!(act, Do::Fall);
        assert!(matches!(h2, Hands::LetGo { put: None, .. }));
        // Lying a moment: put down, then the list's again.
        let (put, act) = h.step(seen(false, WildPlace::Roam), ME, t + PUT_AFTER);
        assert_eq!(act, Do::Put);
        assert!(put.local());
        assert_eq!(
            put.step(seen(false, WildPlace::Roam), ME, t + PUT_AFTER).1,
            Do::Nothing
        );
        let done = t + PUT_AFTER + PUT_WAIT;
        assert_eq!(
            put.step(seen(false, WildPlace::Roam), ME, done),
            (Hands::Free, Do::Nothing)
        );
        // Picked up again before it was put down: still ours; after: asked again.
        assert_eq!(h.step(seen(true, WildPlace::Roam), ME, t).0, held);
        assert_eq!(
            put.step(seen(true, WildPlace::Roam), ME, t + PUT_AFTER),
            (Hands::Asked(t + PUT_AFTER), Do::Take)
        );
    }

    #[test]
    fn a_held_copy_the_host_lost_is_taken_again_and_one_taken_away_is_dropped() {
        let t = Instant::now();
        let held = Hands::Held { odd: None };
        let (h, act) = held.step(seen(true, WildPlace::Roam), ME, t);
        assert_eq!((h, act), (Hands::Held { odd: Some(t) }, Do::Hold));
        let later = t + ODD_LIMIT + Duration::from_millis(1);
        assert_eq!(
            h.step(seen(true, WildPlace::Roam), ME, later),
            (Hands::Asked(later), Do::Take)
        );
        // Back in our hands on the host: fine again.
        assert_eq!(
            h.step(seen(true, WildPlace::Held { by: ME }), ME, later),
            (held, Do::Hold)
        );
        assert_eq!(
            held.step(seen(true, WildPlace::Held { by: 4 }), ME, t),
            (Hands::Free, Do::Drop)
        );
    }

    fn species(abilities: [u8; 2]) -> Species {
        Species {
            types: [0, 0],
            base: [0; 6],
            growth: 0,
            abilities: [abilities[0], abilities[1], 0],
            exp_yield: 0,
        }
    }

    #[test]
    fn a_visitor_is_the_same_creature_on_the_wire_and_back() {
        let p = partner([33, 45, 0, 0]);
        let v = Visitor {
            form: 0,
            species: 25,
            level: 12,
            shiny: true,
            ability_slot: 1,
            seed: 9,
        };
        let c = creature_of(&v, &p).unwrap();
        let back = visitor_of(&c);
        assert_eq!((back.species, back.level, back.shiny), (25, 12, true));
        // The ability check `taught` makes.
        let s = species([5, 9]);
        assert!(s.abilities.contains(&c.ability));
        assert!(!species([5, 0]).abilities.contains(&c.ability));
    }

    fn rom() -> Option<crate::creature::rom::Rom> {
        crate::test_cache::rom()
    }

    #[test]
    fn the_title_names_the_lists_area_or_our_windows() {
        let areas: Vec<Area> = (1..=3)
            .map(|section| Area {
                section,
                theme: rotation::Theme::Dark,
                also: Vec::new(),
                land: Vec::new(),
                water: Vec::new(),
            })
            .collect();
        assert_eq!(shown_area(Some(9), Some(&areas), 3, 7), Some(9));
        assert_eq!(shown_area(Some(9), None, 3, 7), Some(9));
        // Between the server's window change and its next pick: our window's area.
        let ours = rotation::area_at(&areas, 3, 7).unwrap().section;
        assert_eq!(shown_area(None, Some(&areas), 3, 7), Some(ours));
        assert_ne!(shown_area(None, Some(&areas), 3, 8), Some(ours));
        assert_eq!(shown_area(None, None, 3, 7), None);
        assert_eq!(shown_area(None, Some(&areas), 1, 7), None);
    }

    #[test]
    fn catalog_picks_answer_their_asks_from_each_gardens_areas() {
        let Some(rom) = rom() else { return };
        let areas = rotation::areas(&rom).unwrap();
        let mut rng = Rng::new(3);
        let mut recent = Recent::new();
        for garden in 1..=3u8 {
            for slot in 2_988_000..2_988_006 {
                let ask = Asked {
                    section: None,
                    land: 2,
                    pond: 1,
                    fresh: false,
                    slot,
                };
                let (section, list) =
                    answer(&areas, &rom, garden, ask, None, &mut recent, &mut rng).unwrap();
                // The area of the server's window.
                let area = rotation::area_at(&areas, garden, slot).unwrap();
                assert_eq!(section, area.section);
                assert!(list.len() <= 3);
                let land = list.iter().filter(|p| p.arrive == WildArrive::Land).count();
                let pond = list.iter().filter(|p| p.arrive == WildArrive::Pond).count();
                assert!(land <= 2 && pond <= 1 && land + pond == list.len());
                for p in &list {
                    assert!(wire_ok(&p.creature));
                    // What the owner's game takes back: the same creature.
                    let taught = taught(&rom, WildOrigin::Wild, &p.creature).unwrap();
                    assert_eq!(taught.moves, p.creature.moves);
                    assert_eq!(taught.ability, p.creature.ability);
                }
                // The asked section is kept, whatever the window.
                let ask = Asked {
                    section: Some(section),
                    land: 1,
                    pond: 0,
                    fresh: false,
                    slot: slot + 1,
                };
                let (again, list) =
                    answer(&areas, &rom, 3, ask, None, &mut recent, &mut rng).unwrap();
                assert_eq!(again, section);
                assert!(list.iter().all(|p| p.arrive == WildArrive::Land));
            }
        }
    }

    #[test]
    fn catalog_a_fresh_ask_hands_over_the_visit_where_it_is() {
        let Some(rom) = rom() else { return };
        let areas = rotation::areas(&rom).unwrap();
        let area = rotation::area_at(&areas, 1, 0).unwrap();
        let types = |s: u16| rom.species(s).map(|i| i.types);
        let mut rng = Rng::new(5);
        let mut recent = Recent::new();
        let visitor = (0..50)
            .find_map(|_| wild::pick(area, false, types, &mut recent, &mut rng))
            .unwrap();
        let placed = Placed {
            visitor,
            partner: visitor.partner(&rom).unwrap(),
            at: [1.0, 2.0, 3.0],
            angle: 0x1234,
            swimming: false,
        };
        let ask = Asked {
            section: None,
            land: 3,
            pond: 0,
            fresh: true,
            slot: 0,
        };
        let handover = Some((area.section, vec![placed; 2]));
        let (section, list) = answer(
            &areas,
            &rom,
            1,
            ask,
            handover.clone(),
            &mut recent,
            &mut rng,
        )
        .unwrap();
        assert_eq!(section, area.section);
        assert_eq!(list.len(), 3);
        assert_eq!(
            list[0].arrive,
            WildArrive::LandAt {
                at: [1.0, 2.0, 3.0],
                angle: 0x1234
            }
        );
        assert_eq!(list[2].arrive, WildArrive::Land);
        // Handed over from the last window's area: they come as they are, and the section is
        // the server's window's.
        let next = rotation::area_at(&areas, 1, 1).unwrap();
        assert_ne!(next.section, area.section);
        let (section, list) = answer(
            &areas,
            &rom,
            1,
            Asked { slot: 1, ..ask },
            handover.clone(),
            &mut recent,
            &mut rng,
        )
        .unwrap();
        assert_eq!(section, next.section);
        assert_eq!(list.len(), 3);
        assert!(matches!(list[1].arrive, WildArrive::LandAt { .. }));
        // Not a fresh ask: nothing handed over.
        let (_, list) = answer(
            &areas,
            &rom,
            1,
            Asked {
                fresh: false,
                ..ask
            },
            handover,
            &mut recent,
            &mut rng,
        )
        .unwrap();
        assert!(list.iter().all(|p| p.arrive == WildArrive::Land));
    }

    #[test]
    fn catalog_taught_creatures_must_fit_the_catalog() {
        let Some(rom) = rom() else { return };
        let species = rom.starters().unwrap()[0];
        let info = rom.species(species).unwrap();
        let moves = rom.moves_at_level(species, 5);
        let ok = WildCreature {
            form: 0,
            species,
            level: 5,
            ability: info.abilities[0],
            shiny: false,
            moves,
        };
        let p = taught(&rom, WildOrigin::Wild, &ok).unwrap();
        assert_eq!((p.species, p.types, p.level), (species, info.types, 5));
        // Another species' ability, a move above its level or one it never learns.
        let other = (1..SPECIES_COUNT)
            .filter_map(|s| rom.species(s).map(|i| i.abilities[0]))
            .find(|a| *a != 0 && !info.abilities.contains(a))
            .unwrap();
        let bad = WildCreature {
            ability: other,
            ..ok
        };
        assert!(taught(&rom, WildOrigin::Wild, &bad).is_none());
        let later = rom
            .learnset(species)
            .into_iter()
            .find(|&(at, id)| at > 5 && !moves.contains(&id))
            .unwrap()
            .1;
        let early = WildCreature {
            moves: [moves[0], later, 0, 0],
            ..ok
        };
        assert!(taught(&rom, WildOrigin::Wild, &early).is_none());
        // A partner may know it (a move taught on the link machine): any move the catalog has.
        assert!(taught(&rom, WildOrigin::Partner, &early).is_some());
    }

    #[test]
    fn catalog_a_wild_one_must_come_in_the_catalogs_wild_areas_at_its_level() {
        let Some(rom) = rom() else { return };
        let areas = rotation::areas(&rom).unwrap();
        let slot = areas
            .iter()
            .flat_map(|a| a.land.iter().flatten())
            .next()
            .copied()
            .unwrap();
        let creature = |species: u16, level: u8| WildCreature {
            form: 0,
            species,
            level,
            ability: rom.species(species).unwrap().abilities[0],
            shiny: false,
            moves: rom.moves_at_level(species, level),
        };
        let wild = creature(slot.species, slot.min_level);
        assert!(fits(&rom, Some(&areas[..]), WildOrigin::Wild, &wild).is_some());
        // Without the areas there's nothing to tell by.
        assert!(fits(&rom, None, WildOrigin::Wild, &wild).is_none());
        // Levels remain constrained by the mod habitat policy; starters are eligible too.
        let level = (1..=100)
            .find(|&l| !in_the_wild(&areas, slot.species, l))
            .unwrap();
        let odd = creature(slot.species, level);
        assert!(taught(&rom, WildOrigin::Wild, &odd).is_some());
        assert!(fits(&rom, Some(&areas[..]), WildOrigin::Wild, &odd).is_none());
        let starter = creature(rom.starters().unwrap()[0], 5);
        assert!(fits(&rom, Some(&areas[..]), WildOrigin::Wild, &starter).is_some());
        // A partner needn't.
        assert!(fits(&rom, None, WildOrigin::Partner, &starter).is_some());
    }

    #[test]
    fn catalog_a_partner_with_no_ability_recorded_has_its_species_first() {
        let Some(rom) = rom() else { return };
        let mut p = partner([1, 0, 0, 0]);
        p.ability = 0;
        assert_eq!(with_ability(None, p).ability, 0);
        let first = rom.species(p.species).unwrap().abilities[0];
        assert_eq!(with_ability(Some(&rom), p).ability, first);
        p.ability = 7;
        assert_eq!(with_ability(Some(&rom), p).ability, 7);
    }
}
