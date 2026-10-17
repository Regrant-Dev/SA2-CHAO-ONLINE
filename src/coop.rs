//! Co-op rooms: an enemy, item box or breakable container destroyed by anyone in the room is
//! gone for everyone in that stage (same level and mission). See re/stage-objects.md.
//!
//! Every stage object comes from a record in `SETObjectData[0x600]`, filled in SET-file order,
//! so a record's index names the same object in every game. Each frame we apply kills from the
//! others, then compare the records with last frame: an enemy whose "alive" bit cleared, or an
//! item box that broke, is a kill to send. Kills stay applied after a death restart (which
//! revives everything locally) and are sent in full to players who enter the stage later.

use std::collections::BTreeSet;
use std::sync::Mutex;

use crate::game;
use crate::log;
use crate::together::Peer;

/// `SETObjectData[0x600]`, 0x10 bytes each: +2 u16 flags, +4 ObjectMaster*, +8 SETEntry*.
const RECORDS: usize = 0x01DD_E280;
const RECORD_SIZE: usize = 0x10;
/// i16: records in use this stage.
const RECORD_COUNT: usize = 0x01DD_E270;
/// The stage's object list: `{u32 count; ObjectListEntry* entries}`, entries 0x10 bytes with the
/// MainSub at +8. A SET entry's ID (+0, low 15 bits) indexes it.
const OBJECT_LIST: usize = 0x01DD_E268;
const MAX_RECORDS: usize = crate::net::protocol::MAX_STAGE_OBJECTS as usize;

/// Record flags: alive (the loader skips records without it), loaded now, box broken.
const ALIVE: u16 = 0x8000;
const LOADED: u16 = 0x0001;
const BROKEN: u16 = 0x0004;

/// Setting an object's MainSub (+0x10) to DeleteObject removes it on the next object pass.
const DELETE_OBJECT: usize = 0x0046_F720;
const OBJ_MAIN: usize = 0x10;

/// Object list MainSubs we sync (the same in every level).
const ENEMIES: [usize; 18] = [
    0x004F_8F90,
    0x004F_9F00,
    0x004F_C700,
    0x004F_DD00,
    0x004F_F990,
    0x0050_1530,
    0x0050_3140,
    0x0050_4390,
    0x0050_5C20,
    0x0050_6510,
    0x0050_A850,
    0x0050_C3F0,
    0x0050_D790,
    0x0050_E4D0,
    0x0050_F150,
    0x007A_0D10,
    0x007A_1600,
    0x007A_21C0,
];
/// ITEMBOX and ITEMBOX2_B: reload as an empty broken shell (they keep "alive").
const GROUND_BOXES: [usize; 2] = [0x006C_7C10, 0x006C_7CE0];
/// Air, balloon, KD and SG boxes: gone once broken.
const OTHER_BOXES: [usize; 4] = [0x006C_8EF0, 0x006D_B030, 0x006E_9350, 0x006E_ABC0];
/// Objects removed for good by KillObject (0x488DA0, clears "alive") when destroyed: every
/// object list entry whose update calls it (re/stage-objects.md "Breakables"). Rings are
/// separate (`RINGS`); treasure hunt pieces never go through here.
const BREAKABLES: [usize; 25] = [
    0x004B_6A40, // DRMCN
    0x004B_6D60, // PPSHED
    0x004D_7AB0, // MITRUCK
    0x004D_9BD0, // DRMCN
    0x004D_F5E0, // SLANE
    0x004F_DD00, // E EMI
    0x005A_E3E0, // SLANE
    0x005A_F4A0, // SLANE2
    0x005B_BAB0, // DRMCN
    0x005C_2B90, // BIGSHUTTER
    0x0062_4E10, // BALOON
    0x0063_8A60, // BOMBE
    0x0064_5B30, // DLCONT
    0x0065_ADA0, // DYNAMITE
    0x006C_D7F0, // CONTWOOD
    0x006D_63F0, // CONTIRON
    0x006D_A720, // DYNAMITE
    0x006E_3F80, // KDDOOR
    0x006E_A1E0, // WSMMLS
    0x006F_6EA0, // BCOL
    0x006F_B200, // CONTWOOD
    0x006F_BBA0, // CONTIRON
    0x0077_7250, // POISON
    0x0077_9FB0, // SGOLID
    0x0077_A950, // CGWOOD
];
/// RING and SGRING: collected by one player, gone for everyone and a ring for each.
const RINGS: [usize; 2] = [0x006C_0F80, 0x006C_1470];
/// A friend's ring pickup only counts once our stage has run this long (someone joining late
/// gets the whole list of what's gone, but not those rings).
const RING_REWARD_AFTER: std::time::Duration = std::time::Duration::from_secs(5);

#[derive(Clone, Copy, PartialEq)]
enum Kind {
    Enemy,
    GroundBox,
    OtherBox,
    Breakable,
    Ring,
}

struct Coop {
    /// The stage these records belong to: (level, mission).
    stage: Option<(i16, u8)>,
    /// Flags as of last frame (one per record; empty until the stage is ready).
    last: Vec<u16>,
    /// Everything destroyed in this stage, by anyone.
    dead: BTreeSet<u16>,
    /// From the others, to apply.
    incoming: Vec<u16>,
    /// Ours, to send.
    outgoing: Vec<u16>,
    /// Send the whole `dead` set (someone just arrived).
    send_all: bool,
    /// Room members in this stage last frame.
    here: Vec<u8>,
    /// When we entered this stage.
    since: Option<std::time::Instant>,
}

static COOP: Mutex<Coop> = Mutex::new(Coop {
    stage: None,
    last: Vec::new(),
    dead: BTreeSet::new(),
    incoming: Vec::new(),
    outgoing: Vec::new(),
    send_all: false,
    here: Vec::new(),
    since: None,
});

fn lock() -> std::sync::MutexGuard<'static, Coop> {
    COOP.lock().unwrap_or_else(|e| e.into_inner())
}

fn record(n: usize) -> usize {
    RECORDS + n * RECORD_SIZE
}

fn flags(n: usize) -> u16 {
    // SAFETY: a record in the game's static table (n < MAX_RECORDS).
    unsafe { ((record(n) + 2) as *const u16).read_volatile() }
}

fn set_flags(n: usize, value: u16) {
    // SAFETY: as in `flags`; written on the game thread between object passes.
    unsafe { ((record(n) + 2) as *mut u16).write_volatile(value) }
}

/// What the record's object is, from its object list entry's MainSub.
fn kind(n: usize) -> Option<Kind> {
    // SAFETY: the SET entry and object list pointers are the game's for this stage; each is
    // checked for null and the list index against the list's length.
    let main = unsafe {
        let entry = ((record(n) + 8) as *const usize).read();
        let list = (OBJECT_LIST as *const usize).read();
        if entry == 0 || list == 0 {
            return None;
        }
        let id = usize::from((entry as *const u16).read() & 0x7FFF);
        let (count, entries) = (
            (list as *const u32).read() as usize,
            ((list + 4) as *const usize).read(),
        );
        if id >= count || entries == 0 {
            return None;
        }
        ((entries + id * 0x10 + 8) as *const usize).read()
    };
    if ENEMIES.contains(&main) {
        Some(Kind::Enemy)
    } else if GROUND_BOXES.contains(&main) {
        Some(Kind::GroundBox)
    } else if OTHER_BOXES.contains(&main) {
        Some(Kind::OtherBox)
    } else if BREAKABLES.contains(&main) {
        Some(Kind::Breakable)
    } else if RINGS.contains(&main) {
        Some(Kind::Ring)
    } else {
        None
    }
}

/// Destroyed in our game (as far as this kind can be).
fn is_destroyed(kind: Kind, f: u16) -> bool {
    match kind {
        Kind::Enemy | Kind::Breakable | Kind::Ring => f & ALIVE == 0,
        Kind::GroundBox => f & BROKEN != 0,
        Kind::OtherBox => f & ALIVE == 0 || f & BROKEN != 0,
    }
}

/// Destroys record `n` in our game without score, rings or animals (re/stage-objects.md 4).
fn destroy(n: usize, kind: Kind) {
    let mut f = flags(n);
    match kind {
        Kind::Enemy | Kind::Breakable | Kind::Ring => f &= !ALIVE,
        // Stays alive: the loader brings it back as the broken shell.
        Kind::GroundBox => f |= BROKEN,
        Kind::OtherBox => f = (f | BROKEN) & !ALIVE,
    }
    set_flags(n, f);
    if f & LOADED != 0 {
        // SAFETY: a loaded record's object pointer is the live ObjectMaster; DeleteObject as its
        // MainSub is how the game itself removes objects (CheckRangeOut, KillObject).
        unsafe {
            let obj = ((record(n) + 4) as *const usize).read();
            if obj != 0 {
                ((obj + OBJ_MAIN) as *mut usize).write(DELETE_OBJECT);
            }
        }
    }
}

/// Network: kills from another player in a stage.
pub fn received(level: i16, mission: u8, objects: Vec<u16>) {
    if level == game::current_level() && mission == game::mission() {
        lock().incoming.extend(objects);
    }
}

/// Our new kills to send, or all of them when someone joined the stage: (level, mission, ids).
pub fn take_outgoing() -> Option<(i16, u8, Vec<u16>)> {
    let mut c = lock();
    let (level, mission) = c.stage?;
    let objects: Vec<u16> = if std::mem::take(&mut c.send_all) {
        c.outgoing.clear();
        c.dead.iter().copied().collect()
    } else {
        std::mem::take(&mut c.outgoing)
    };
    (!objects.is_empty()).then_some((level, mission, objects))
}

fn active(connected: bool) -> bool {
    let level = game::current_level();
    connected
        && crate::room::in_room()
        && crate::room::mode() == crate::room::Mode::Coop
        && level > 0
        && level != game::LEVEL_CHAO_WORLD
        && !game::two_player()
        && game::game_state() == game::GAME_STATE_INGAME
}

/// Call every frame with the other players' latest states.
pub fn on_frame(peers: &[Peer], connected: bool) {
    let mut c = lock();
    if !active(connected) {
        // Leaving the stage (or co-op) forgets it; a new entry starts fresh.
        if game::game_state() != game::GAME_STATE_INGAME || !connected {
            *c = Coop {
                stage: None,
                last: Vec::new(),
                dead: BTreeSet::new(),
                incoming: Vec::new(),
                outgoing: Vec::new(),
                send_all: false,
                here: Vec::new(),
                since: None,
            };
        }
        return;
    }
    let stage = (game::current_level(), game::mission());
    // SAFETY: the game's record count for this stage.
    let count = unsafe { (RECORD_COUNT as *const i16).read_volatile() }.clamp(0, MAX_RECORDS as i16)
        as usize;
    if c.stage != Some(stage) {
        log!(
            "coop: stage {} mission {}, {count} objects",
            stage.0,
            stage.1
        );
        c.stage = Some(stage);
        c.since = Some(std::time::Instant::now());
        c.dead.clear();
        c.outgoing.clear();
        c.here.clear();
        // Kills that arrived while we were loading stay queued in `incoming`.
        c.last = (0..count).map(flags).collect();
    }
    if c.last.len() != count {
        c.last = (0..count).map(flags).collect();
    }

    // Kills from the others, and anything a death restart brought back.
    let incoming = std::mem::take(&mut c.incoming);
    let rewards = c.since.is_some_and(|at| at.elapsed() >= RING_REWARD_AFTER);
    for n in incoming {
        if usize::from(n) < count && c.dead.insert(n) {
            // A ring a friend just picked up: one for us too.
            if rewards && kind(usize::from(n)) == Some(Kind::Ring) {
                game::add_rings(1);
            }
        }
    }
    let dead: Vec<u16> = c.dead.iter().copied().collect();
    for n in dead {
        let n = usize::from(n);
        if let Some(kind) = kind(n)
            && !is_destroyed(kind, flags(n))
        {
            destroy(n, kind);
        }
    }

    // Our own kills: records that became destroyed since last frame.
    for n in 0..count {
        let now = flags(n);
        let before = std::mem::replace(&mut c.last[n], now);
        if now == before {
            continue;
        }
        let Some(kind) = kind(n) else { continue };
        if is_destroyed(kind, now) && !is_destroyed(kind, before) && c.dead.insert(n as u16) {
            c.outgoing.push(n as u16);
        }
    }

    // Someone in the room entered this stage: send them everything destroyed so far.
    let members = crate::room::members();
    let here: Vec<u8> = peers
        .iter()
        .filter(|p| members.contains(&p.id) && (p.level, p.mission) == stage)
        .map(|p| p.id)
        .collect();
    if here.iter().any(|id| !c.here.contains(id)) && !c.dead.is_empty() {
        c.send_all = true;
    }
    c.here = here;
}
