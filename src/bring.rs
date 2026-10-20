//! Bring 4, hide the rest (re/chao-garden-sync.md "Bring 4, hide the rest").
//!
//! In a shared garden (online, with another player in it or the server running it) each game
//! shows only the Chao its player brings (F9 Bring Chao 1..4, `online::BRING_CHAO`), so
//! everyone sees the same set. The player's other Chao and eggs living in that garden aren't
//! there while it's shared:
//! - At garden load the spawn loop 0x531B10 calls CreateChao 0x5501D0 once per save slot living
//!   in the garden (0x531D91; eggs 0x531DFF). For a Chao left home that call returns 0; both
//!   callers allow it (0x531D99 tests it, the egg call ignores it).
//! - When the garden becomes shared mid-visit, the live ones are deleted as the game deletes
//!   every Chao on leaving: their delete subs (Chao 0x54FF30: model work 0x56CE70 and the
//!   registry 0x5307F0; eggs 0x57B9B0: the registry only) free the object's own memory and
//!   never write the record (`Data1+0x58 == 0`: it's the save slot itself).
//! - When it stops being shared (alone for `SHOW_AFTER`, offline, online off), or a left-out
//!   Chao is brought (F9), it's made again from its record with CreateChao, as the load does.
//!
//! Save records are never written here: a Chao without an object stands still, like the Chao
//! of a garden you're not in. The pre-save pass 0x52F090 (0x52E6B0 trees, 0x52E710 items,
//! 0x52EDE0 garden globals, 0x52EEE0 checksum) never walks the Chao slots, so the record is
//! saved exactly as it was, and the next garden load sees it again.

use std::sync::Mutex;
use std::sync::atomic::{AtomicU8, AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use crate::chao::{CHAO_SIZE, ChaoData};
use crate::game::{self, Vector};
use crate::log;

/// The calls to CreateChao in the garden load's spawn loop 0x531B10: Chao, eggs.
const SPAWN_CALLS: [usize; 2] = [0x0053_1D91, 0x0053_1DFF];
const CREATE_CHAO: usize = 0x0055_01D0;
/// The save's Chao records (24 of 0x800 bytes), in each of its two copies (0x173D06C picks).
const CHAO_SLOTS: usize = 0x019F_6EC4;
const SAVE_SLOTS: usize = 24;
const SAVE_COPY_SIZE: usize = 0xCA6C;
/// Record: type (0 none, 1 egg, 2+ hatched), garden (1..=3).
const C_TYPE: usize = 0x80;
const C_GARDEN: usize = 0x81;
/// Garden load spawn points: 16 per garden (12-byte vectors), `(garden - 1) * 16 + n`.
const SPAWN_POINTS: usize = 0x0136_6260;
const SPAWN_POINTS_PER_GARDEN: usize = 16;

/// Registry categories (registry.rs): Chao, eggs.
const EGG_CATEGORY: usize = 1;
const OBJ_MAIN: usize = 0x10;
const OBJ_DATA1: usize = 0x34;
/// Registry entry: the record pointer.
const ENTRY_RECORD: usize = 0x08;
const CHAO_MAIN: usize = 0x0054_FE20;
const EGG_MAIN: usize = 0x0057_B520;
/// Chao Data1+4 status: held. Egg Data1+0 mode: 4 and up hatch (eggs.rs).
const D_STATUS: usize = 0x04;
const STATUS_HELD: u16 = 0x8000;
const D_EGG_MODE: usize = 0x00;
const EGG_HATCHING_FROM: u8 = 4;

/// Shared this long (a friend came in) before ours are taken home mid-visit.
const HIDE_AFTER: Duration = Duration::from_secs(1);
/// Alone this long before they come back: a friend's short reconnect doesn't bring them back.
const SHOW_AFTER: Duration = Duration::from_secs(20);
/// After arriving, wait this long before deleting or making Chao (the garden is still setting
/// up), and look again every this many frames.
const SETTLE: Duration = Duration::from_secs(2);
const CHECK_EVERY: u32 = 15;

/// Gardens (bit = area 1..=3) other players are in now, from their states (online.rs).
static FRIEND_GARDENS: AtomicU8 = AtomicU8::new(0);
/// The CreateChao the spawn calls went to before (0 until hooked).
static ORIGINAL: AtomicUsize = AtomicUsize::new(0);

struct Visit {
    area: i32,
    /// Ours not brought stay home now.
    hiding: bool,
    /// Whether the garden was shared at the last look, and since when that holds.
    shared: bool,
    since: Instant,
    /// Our records with no object this visit because of us (we skipped or deleted them).
    hidden: Vec<usize>,
    /// The garden load's spawn loop may still run (no frame since).
    loading: bool,
    arrived: Instant,
    frame: u32,
    /// Spreads remade Chao over the spawn points.
    next_point: usize,
}

impl Visit {
    fn new(area: i32, shared: bool) -> Self {
        let now = Instant::now();
        Self {
            area,
            hiding: shared,
            shared,
            since: now,
            hidden: Vec::new(),
            loading: true,
            arrived: now,
            frame: 0,
            next_point: 0,
        }
    }
}

static VISIT: Mutex<Option<Visit>> = Mutex::new(None);

fn with<R>(f: impl FnOnce(&mut Option<Visit>) -> R) -> R {
    f(&mut VISIT.lock().unwrap_or_else(|e| e.into_inner()))
}

/// Network (online.rs, each frame): the gardens other players are in (bit = area), 0 offline.
pub fn set_friend_gardens(mask: u8) {
    FRIEND_GARDENS.store(mask, Ordering::Relaxed);
}

/// Whether garden `area` is shared now: someone else is in it, or the server runs it.
fn shared(area: i32) -> bool {
    ((1..=3).contains(&area) && FRIEND_GARDENS.load(Ordering::Relaxed) & (1 << area) != 0)
        || crate::garden::server_runs()
}

/// Hooks the garden load's CreateChao calls. Call once at startup.
pub fn install() {
    let target = spawn_from_save as SpawnFn as usize;
    for site in SPAWN_CALLS {
        match crate::patch::redirect_call(site, target) {
            Some(original) if original == CREATE_CHAO => {
                ORIGINAL.store(original, Ordering::Relaxed)
            }
            Some(original) => {
                // Someone else hooked it first: theirs stays, and ours are all shown.
                crate::patch::redirect_call(site, original);
                log!("bring: {site:#x} is already hooked; Chao left home will still show");
            }
            None => log!("bring: can't hook {site:#x}; Chao left home will still show"),
        }
    }
}

type SpawnFn = extern "C" fn(usize, i32, i32, usize, i32) -> usize;

/// The spawn loop's CreateChao (cdecl: record, copy, flag, position, angle): 0 for our Chao
/// that stay home, else the game's.
extern "C" fn spawn_from_save(
    record: usize,
    copy: i32,
    flag: i32,
    position: usize,
    angle: i32,
) -> usize {
    // A panic must not unwind into the game: then it's shown, as the game would.
    if std::panic::catch_unwind(|| left_home_at_load(record)).unwrap_or(false) {
        return 0;
    }
    let original = match ORIGINAL.load(Ordering::Relaxed) {
        0 => CREATE_CHAO,
        f => f,
    };
    // SAFETY: the game's CreateChao, with the arguments its spawn loop passed us, on the game
    // thread.
    unsafe { std::mem::transmute::<usize, SpawnFn>(original)(record, copy, flag, position, angle) }
}

/// Garden load: whether our Chao at `record` stays home (the garden is shared and it isn't
/// brought). Remembered, to make it again when that changes.
fn left_home_at_load(record: usize) -> bool {
    let area = game::current_chao_area();
    let Some(slot) = slot_of(record) else {
        return false;
    };
    if !(1..=3).contains(&area) {
        return false;
    }
    let shared = shared(area);
    let brought = crate::online::brought_slots(area);
    with(|v| {
        // The first call of a load (no frame since the last one) starts a new visit.
        if !v.as_ref().is_some_and(|v| v.loading && v.area == area) {
            *v = Some(Visit::new(area, shared));
        }
        let Some(v) = v.as_mut() else {
            return false;
        };
        let home = v.hiding && stays_home(slot, &brought, &[]);
        if home {
            v.hidden.push(record);
            log!(
                "bring: our Chao in slot {} stays home (shared garden)",
                slot + 1
            );
        }
        home
    })
}

/// Call every frame (game thread).
pub fn on_frame() {
    let area = if game::in_garden() {
        game::current_chao_area()
    } else {
        -1
    };
    with(|v| {
        if area <= 0 {
            // The garden went and its objects with it; the records were never touched.
            *v = None;
            return;
        }
        let now_shared = shared(area);
        let visit = match v {
            Some(visit) if visit.area == area => visit,
            // In a garden without its load passing here (hook missing): everything shows.
            _ => v.insert(Visit::new(area, false)),
        };
        visit.loading = false;
        if now_shared != visit.shared {
            visit.shared = now_shared;
            visit.since = Instant::now();
        }
        let before = visit.hiding;
        visit.hiding = next_mode(visit.hiding, now_shared, visit.since.elapsed());
        if visit.hiding != before {
            log!(
                "bring: garden {area} {}",
                if visit.hiding {
                    "is shared: only the Chao we bring stay"
                } else {
                    "isn't shared any more: all our Chao come back"
                }
            );
        }
        visit.frame = visit.frame.wrapping_add(1);
        if visit.frame % CHECK_EVERY != 0
            || visit.arrived.elapsed() < SETTLE
            || game::game_state() != game::GAME_STATE_INGAME
            || game::local_entity().is_null()
        {
            return;
        }
        settle(visit);
    });
}

/// Our Chao in this garden: the ones that stay home go (when they can now), the ones that
/// may come are made again.
fn settle(v: &mut Visit) {
    let brought = crate::online::brought_slots(v.area);
    // Chao the shared garden still runs as ours (garden.rs) stay until it lets them go.
    let registered: Vec<usize> = crate::garden::mine().iter().map(|&(_, s)| s).collect();
    let live = live_records();
    if v.hiding {
        let held = crate::garden::player_held();
        for &(object, record, egg) in &live {
            let Some(slot) = slot_of(record) else {
                continue;
            };
            // SAFETY: a save slot's record (static), and a live registry object.
            let here = unsafe { i32::from(((record + C_GARDEN) as *const u8).read()) } == v.area;
            if !here || !stays_home(slot, &brought, &registered) || object == held {
                continue;
            }
            // SAFETY: a live registry object on the game thread; Data1 is checked for null.
            let free = unsafe {
                let main = ((object + OBJ_MAIN) as *const usize).read();
                let d = ((object + OBJ_DATA1) as *const usize).read();
                d != 0
                    && if egg {
                        main == EGG_MAIN
                            && ((d + D_EGG_MODE) as *const u8).read() < EGG_HATCHING_FROM
                    } else {
                        // Not held, nor run by something else (a whistle's call ends first).
                        main == CHAO_MAIN
                            && ((d + D_STATUS) as *const u16).read() & STATUS_HELD == 0
                    }
            };
            if !free {
                continue;
            }
            // Deleted as on leaving the garden: the object goes, the record stays.
            crate::garden::delete_chao(object);
            if !v.hidden.contains(&record) {
                v.hidden.push(record);
            }
            log!(
                "bring: our Chao in slot {} went home (shared garden)",
                slot + 1
            );
        }
    }
    let mut keep = Vec::new();
    for record in std::mem::take(&mut v.hidden) {
        let Some(slot) = slot_of(record) else {
            continue;
        };
        if v.hiding && stays_home(slot, &brought, &registered) {
            keep.push(record);
            continue;
        }
        if live.iter().any(|&(_, r, _)| r == record) {
            continue;
        }
        // SAFETY: a save slot's record (static memory).
        let (kind, garden) = unsafe {
            (
                ((record + C_TYPE) as *const u8).read(),
                i32::from(((record + C_GARDEN) as *const u8).read()),
            )
        };
        // Moved away or gone meanwhile: nothing to make here.
        if kind == 0 || garden != v.area {
            continue;
        }
        let point = (v.area as usize - 1) * SPAWN_POINTS_PER_GARDEN
            + v.next_point % SPAWN_POINTS_PER_GARDEN;
        v.next_point += 5;
        // SAFETY: the game's spawn point table (static data).
        let at = unsafe { ((SPAWN_POINTS + point * 12) as *const Vector).read_unaligned() };
        let angle = (slot as i32 * 0x2493) & 0xFFFF;
        // SAFETY: on the game thread in the garden it lives in, with no object for it; the
        // record is a save slot (static), as the garden load passes it.
        let object = unsafe { game::create_chao(record as *mut ChaoData, at, angle) };
        log!(
            "bring: our Chao in slot {} came back ({object:p})",
            slot + 1
        );
    }
    v.hidden = keep;
}

/// Our save's Chao and eggs with an object now: (object, record, egg).
fn live_records() -> Vec<(usize, usize, bool)> {
    [crate::registry::CHAO, EGG_CATEGORY]
        .into_iter()
        .flat_map(|category| {
            crate::registry::entries(category)
                .into_iter()
                // SAFETY: a live registry entry's record pointer.
                .map(move |(entry, object)| {
                    let record = unsafe { ((entry + ENTRY_RECORD) as *const usize).read() };
                    (object, record, category == EGG_CATEGORY)
                })
        })
        .filter(|&(_, record, _)| slot_of(record).is_some())
        .collect()
}

/// The save slot (0..24) of a record in either copy of the save; `None` for any other record
/// (a visitor's buffer).
fn slot_of(record: usize) -> Option<usize> {
    (0..2).find_map(|copy| {
        let offset = record.checked_sub(CHAO_SLOTS + copy * SAVE_COPY_SIZE)?;
        (offset % CHAO_SIZE == 0 && offset / CHAO_SIZE < SAVE_SLOTS).then_some(offset / CHAO_SIZE)
    })
}

/// While the garden is shared, our Chao in `slot` stays home: it isn't brought, nor still run
/// as one of ours by the shared garden (`registered`, until it catches up with a change).
fn stays_home(slot: usize, brought: &[usize], registered: &[usize]) -> bool {
    !brought.contains(&slot) && !registered.contains(&slot)
}

/// Hiding or not after the garden has been `shared` (or not) for `steady`: on soon after it's
/// shared, off only after being alone a good while.
fn next_mode(hiding: bool, shared: bool, steady: Duration) -> bool {
    match (hiding, shared) {
        (false, true) => steady >= HIDE_AFTER,
        (true, false) => steady < SHOW_AFTER,
        (hiding, _) => hiding,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn records_map_to_their_save_slots() {
        assert_eq!(slot_of(CHAO_SLOTS), Some(0));
        assert_eq!(slot_of(CHAO_SLOTS + 23 * CHAO_SIZE), Some(23));
        assert_eq!(
            slot_of(CHAO_SLOTS + SAVE_COPY_SIZE + 5 * CHAO_SIZE),
            Some(5)
        );
        assert_eq!(slot_of(CHAO_SLOTS + 24 * CHAO_SIZE), None);
        assert_eq!(slot_of(CHAO_SLOTS - CHAO_SIZE), None);
        assert_eq!(slot_of(CHAO_SLOTS + 4), None);
        // A visitor's buffer (heap) isn't ours.
        assert_eq!(slot_of(0x0800_0000), None);
    }

    #[test]
    fn only_the_brought_stay() {
        let brought = [3, 0];
        assert!(!stays_home(3, &brought, &[]));
        assert!(!stays_home(0, &brought, &[]));
        assert!(stays_home(1, &brought, &[]));
        // Still run as ours by the shared garden until it hears of the change.
        assert!(!stays_home(1, &brought, &[1]));
        // Every row on None: nobody comes.
        assert!(stays_home(0, &[], &[]));
    }

    #[test]
    fn hiding_starts_soon_and_ends_late() {
        let s = Duration::from_secs;
        assert!(!next_mode(false, true, Duration::ZERO));
        assert!(next_mode(false, true, HIDE_AFTER));
        assert!(next_mode(true, false, s(5)));
        assert!(!next_mode(true, false, SHOW_AFTER));
        assert!(next_mode(true, true, s(0)));
        assert!(!next_mode(false, false, s(100)));
    }
}
