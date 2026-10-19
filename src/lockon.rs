//! Co-op rooms: a friend's mech lock-on, drawn here (re/stage-objects.md "Mech lock-on").
//!
//! Holding the shot, a mech locks on to what's in front of it: each target gets a marker, a
//! child of the player object (main 0x74C260) whose data (+0x40) holds +0x10 where it aims and
//! +0x1C the target object. Letting go turns each marker into a laser (main 0x74B920) that
//! homes in from the gun (CharObj2+0x23C) and hits as that player.
//!
//! Ours are sent: the locked SET records while we hold, and the ones we fire at. A friend's are
//! drawn with the game's own marker displays (the cursor 0x74BEC0, the laser 0x74B620) on
//! objects of ours whose updates only move them. They never hit anything and their "target" is
//! a stand-in object of ours. What the friend's lasers destroy comes as their kills.
//!
//! The displays take the mech's effect textures from a player's CharObj2 (+0x444, "WALKTEX",
//! loaded by the walker's loader 0x740DDE). As a mech we lend them ours; otherwise we load
//! WALKTEX ourselves and, only while one of these displays runs, player 2's slots (`PLAYERS`
//! +4, `CHAR_OBJ2S` +4: empty in 1P but for a rival fight's AI, and put back right after)
//! point at a stand-in player whose CharObj2 holds it.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use crate::net::protocol::MAX_LOCK_ON;

/// Players' ObjectMaster pointers and CharObj2 pointers (player 1 first).
const PLAYER_OBJECTS: usize = 0x01DE_A6E0;
const CHAR_OBJ2S: usize = 0x01DE_9600;
/// The mech effect textures' slot in a CharObj2, and how big a stand-in CharObj2 must be.
const CO2_EFFECT_TEXLIST: usize = 0x444;
const CO2_SIZE: usize = 0x448;
const WALKTEX: &[u8] = b"WALKTEX\0";
const OBJ_NEXT: usize = 0x00;
const OBJ_CHILD: usize = 0x0C;
const OBJ_MAIN: usize = 0x10;
const OBJ_DELETE: usize = 0x18;
const OBJ_DISPLAY: usize = 0x1C;
const OBJ_DATA1: usize = 0x34;
const OBJ_CHAR_OBJ2: usize = 0x40;
const OBJ_EXTRA: usize = 0x40;
const D_POSITION: usize = 0x14;
const DATA1_SIZE: usize = 0x30;
/// Marker data (+0x40): +8 player index, +0xE cursor frames (7 = fully shown), +0x10 where it
/// aims, +0x1C the target object. +0 and +4 chain other markers (none here).
const MARKER_SIZE: usize = 0x20;
const M_PLAYER: usize = 0x08;
const M_FRAMES: usize = 0x0E;
const M_AIM: usize = 0x10;
const M_TARGET: usize = 0x1C;
const MARKER_LOCKED: usize = 0x0074_C260;
const MARKER_FLYING: usize = 0x0074_B920;
const CURSOR_DISPLAY: usize = 0x0074_BEC0;
const LASER_DISPLAY: usize = 0x0074_B620;
const CO2_GUN: usize = 0x23C;
const DELETE_OBJECT: usize = 0x0046_F720;
/// LoadObject: ESI = list, EDI = main, name on the stack.
const LOAD_OBJECT: usize = 0x0046_F680;
const ALLOCATOR: usize = 0x01D1_9CAC;
const LIST: u32 = 3;
const NAME: &[u8] = b"SA2ChaoOnlineLockOn\0";

/// Laser speed (units a frame) and how long one flies at most.
const SPEED: f32 = 12.0;
const LIFE_FRAMES: u32 = 120;
/// Locked targets are sent this often while they change.
const SEND_EVERY: u32 = 6;
/// A friend's cursors go when they haven't said anything for this long.
const QUIET: Duration = Duration::from_secs(2);

/// Lock-on targets: (SET record, where it aims).
pub type Aims = Vec<(u16, [f32; 3])>;
/// A lock-on: (level, mission, gun, locked, fired).
pub type LockOn = (i16, u8, [f32; 3], Aims, Aims);

#[derive(Clone, Copy)]
enum Kind {
    Cursor,
    Laser { age: u32 },
}

struct Shown {
    friend: u8,
    record: u16,
    aim: [f32; 3],
    kind: Kind,
}

struct LockOns {
    /// Ours: what we sent last, lasers already reported, and frames.
    sent: Vec<u16>,
    flying: Vec<usize>,
    outgoing: Option<LockOn>,
    frame: u32,
    /// Friends' lock-ons to draw, and when each friend last said anything.
    incoming: Vec<(u8, LockOn)>,
    heard: HashMap<u8, Instant>,
    /// Our drawing objects.
    shown: HashMap<usize, Shown>,
    /// The stand-in target every drawing object points at: object, Data1, collision, its info.
    stand_in: usize,
    /// The stand-in player 2 (object, CharObj2) and the WALKTEX we loaded for it, with the
    /// stage it's for.
    lender: (usize, usize),
    texlist: Option<(usize, (i16, u8))>,
}

static LOCK_ONS: Mutex<Option<LockOns>> = Mutex::new(None);

fn with<R>(f: impl FnOnce(&mut LockOns) -> R) -> R {
    let mut l = LOCK_ONS.lock().unwrap_or_else(|e| e.into_inner());
    f(l.get_or_insert_with(|| LockOns {
        sent: Vec::new(),
        flying: Vec::new(),
        outgoing: None,
        frame: 0,
        incoming: Vec::new(),
        heard: HashMap::new(),
        shown: HashMap::new(),
        stand_in: stand_in_target(),
        lender: lender(),
        texlist: None,
    }))
}

/// A target the displays can read and never draw anything on: the cursor asks (0x74C4E0) for
/// its Data1, Data1+0x2C collision, its +0xC info's flags (+4) and Data1+2. Ours, never freed.
fn stand_in_target() -> usize {
    let info: &'static mut [usize; 4] = Box::leak(Box::new([0; 4]));
    let collision: &'static mut [usize; 4] = Box::leak(Box::new([0; 4]));
    collision[3] = info.as_ptr() as usize;
    let data1: &'static mut [usize; DATA1_SIZE / 4] = Box::leak(Box::new([0; DATA1_SIZE / 4]));
    data1[0x2C / 4] = collision.as_ptr() as usize;
    let object: &'static mut [usize; 0x50 / 4] = Box::leak(Box::new([0; 0x50 / 4]));
    object[OBJ_DATA1 / 4] = data1.as_ptr() as usize;
    object.as_ptr() as usize
}

/// A player 2 for the displays: an object whose CharObj2 (+0x40) is zero but for the effect
/// textures (set when loaded). Ours, never freed.
fn lender() -> (usize, usize) {
    let co2: &'static mut [u8; CO2_SIZE] = Box::leak(Box::new([0; CO2_SIZE]));
    let object: &'static mut [usize; 0x50 / 4] = Box::leak(Box::new([0; 0x50 / 4]));
    object[OBJ_CHAR_OBJ2 / 4] = co2.as_ptr() as usize;
    (object.as_ptr() as usize, co2.as_ptr() as usize)
}

/// Network: a room player's lock-on.
pub fn received(from: u8, lock_on: LockOn) {
    with(|l| {
        l.heard.insert(from, Instant::now());
        if l.incoming.len() < 64 {
            l.incoming.push((from, lock_on));
        }
    });
}

/// Our lock-on to send, if it changed.
pub fn take_outgoing() -> Option<LockOn> {
    with(|l| l.outgoing.take())
}

fn mech() -> bool {
    crate::game::local_player().is_some_and(|p| matches!(p.character, 6 | 7 | 10 | 11))
}

/// Our player object (null without one).
fn player_object() -> usize {
    // SAFETY: the game's static player array.
    unsafe { (PLAYER_OBJECTS as *const usize).read_volatile() }
}

/// Our markers now: (marker object, flying, target record, where it aims).
fn our_markers() -> Vec<(usize, bool, u16, [f32; 3])> {
    let player = player_object();
    let mut out = Vec::new();
    if player == 0 {
        return out;
    }
    // SAFETY: the player's children, a circular list through +0 (0x470C00), read on the game
    // thread; each marker's data and target are checked for null.
    unsafe {
        let first = ((player + OBJ_CHILD) as *const usize).read();
        let mut child = first;
        for _ in 0..256 {
            if child == 0 {
                break;
            }
            let main = ((child + OBJ_MAIN) as *const usize).read();
            if main == MARKER_LOCKED || main == MARKER_FLYING {
                let data = ((child + OBJ_EXTRA) as *const usize).read();
                if data != 0 {
                    let target = ((data + M_TARGET) as *const usize).read();
                    let aim = ((data + M_AIM) as *const [f32; 3]).read_unaligned();
                    if let Some(record) = (target != 0)
                        .then(|| crate::coop::record_of(target))
                        .flatten()
                    {
                        out.push((child, main == MARKER_FLYING, record, aim));
                    }
                }
            }
            child = ((child + OBJ_NEXT) as *const usize).read();
            if child == first {
                break;
            }
        }
    }
    out
}

/// Where record `record`'s object is here, if it's loaded.
fn live_aim(record: u16) -> Option<[f32; 3]> {
    let object = crate::coop::record_object(record)?;
    // SAFETY: a loaded record's live object and its Data1.
    unsafe {
        let d1 = ((object + OBJ_DATA1) as *const usize).read();
        (d1 != 0).then(|| ((d1 + D_POSITION) as *const [f32; 3]).read_unaligned())
    }
}

/// Call every frame (game thread).
pub fn on_frame(connected: bool) {
    let active =
        crate::coop::active(connected) && crate::coop::stage_sync() && player_object() != 0;
    let stage = (crate::game::current_level(), crate::game::mission());
    with(|l| {
        l.frame = l.frame.wrapping_add(1);
        // Our WALKTEX is for one stage (the game's own go with the stage).
        if let Some((texlist, of)) = l.texlist
            && (of != stage || stage.0 <= 0)
        {
            crate::puppet::release_texlist(texlist);
            l.texlist = None;
        }
        if !active {
            l.sent.clear();
            l.flying.clear();
            l.incoming.clear();
            remove_all(l);
            return;
        }

        // Ours.
        let markers = our_markers();
        let locked: Aims = markers
            .iter()
            .filter(|m| !m.1)
            .map(|m| (m.2, m.3))
            .take(usize::from(MAX_LOCK_ON))
            .collect();
        let fired: Aims = markers
            .iter()
            .filter(|m| m.1 && !l.flying.contains(&m.0))
            .map(|m| (m.2, m.3))
            .take(usize::from(MAX_LOCK_ON))
            .collect();
        l.flying = markers.iter().filter(|m| m.1).map(|m| m.0).collect();
        let records: Vec<u16> = locked.iter().map(|&(r, _)| r).collect();
        if !fired.is_empty() || (records != l.sent && l.frame.is_multiple_of(SEND_EVERY)) {
            // SAFETY: our live player object's CharObj2 (+0x40), checked for null.
            let gun = unsafe {
                let co2 = ((player_object() + OBJ_CHAR_OBJ2) as *const usize).read();
                if co2 == 0 {
                    [0.0; 3]
                } else {
                    ((co2 + CO2_GUN) as *const [f32; 3]).read_unaligned()
                }
            };
            l.sent = records;
            l.outgoing = Some((stage.0, stage.1, gun, locked, fired));
        }

        // Friends'.
        for (friend, (level, mission, gun, locked, fired)) in std::mem::take(&mut l.incoming) {
            if (level, mission) != stage {
                continue;
            }
            if l.texlist.is_none() && !mech() {
                let texlist = crate::puppet::load_texlist_named(WALKTEX.as_ptr() as usize);
                if texlist == 0 {
                    continue;
                }
                // SAFETY: our stand-in CharObj2 (`lender`), CO2_SIZE bytes.
                unsafe { ((l.lender.1 + CO2_EFFECT_TEXLIST) as *mut usize).write(texlist) };
                l.texlist = Some((texlist, stage));
            }
            // Cursors no longer locked go; new ones come.
            let gone: Vec<usize> = l
                .shown
                .iter()
                .filter(|(_, s)| {
                    s.friend == friend
                        && matches!(s.kind, Kind::Cursor)
                        && !locked.iter().any(|&(r, _)| r == s.record)
                })
                .map(|(&o, _)| o)
                .collect();
            for object in gone {
                remove(l, object);
            }
            for (record, aim) in locked {
                let have = l.shown.values().any(|s| {
                    s.friend == friend && s.record == record && matches!(s.kind, Kind::Cursor)
                });
                if !have {
                    spawn(l, friend, record, aim, None);
                }
            }
            for (record, aim) in fired {
                spawn(l, friend, record, aim, Some(gun));
            }
        }
        // Friends gone quiet: their cursors go (lasers finish on their own).
        let quiet: Vec<usize> = l
            .shown
            .iter()
            .filter(|(_, s)| {
                matches!(s.kind, Kind::Cursor)
                    && l.heard.get(&s.friend).is_none_or(|at| at.elapsed() > QUIET)
            })
            .map(|(&o, _)| o)
            .collect();
        for object in quiet {
            remove(l, object);
        }
    });
}

/// One of our drawing objects goes (the game frees it on its next pass).
fn remove(l: &mut LockOns, object: usize) {
    if l.shown.remove(&object).is_some() {
        // SAFETY: our live object (it's in `shown` until its delete callback runs).
        unsafe { ((object + OBJ_MAIN) as *mut usize).write(DELETE_OBJECT) };
    }
}

fn remove_all(l: &mut LockOns) {
    let all: Vec<usize> = l.shown.keys().copied().collect();
    for object in all {
        remove(l, object);
    }
}

/// A cursor on `record` (`from` None), or a laser flying there from `from`.
fn spawn(l: &mut LockOns, friend: u8, record: u16, aim: [f32; 3], from: Option<[f32; 3]>) {
    if from.is_some() {
        // That cursor turns into the laser, as the game's marker does.
        let cursor = l
            .shown
            .iter()
            .find(|(_, s)| {
                s.friend == friend && s.record == record && matches!(s.kind, Kind::Cursor)
            })
            .map(|(&o, _)| o);
        if let Some(object) = cursor {
            remove(l, object);
        }
    }
    let aim = live_aim(record).unwrap_or(aim);
    let (main, display) = match from {
        Some(_) => (
            laser_main as *const () as usize,
            laser_display as *const () as usize,
        ),
        None => (
            cursor_main as *const () as usize,
            cursor_display as *const () as usize,
        ),
    };
    // SAFETY: the game's object loader and allocator on the game thread; the blocks are the
    // game's own (with its header), so freeing the object frees them the game's way.
    let object = unsafe {
        let (data1, extra) = (game_alloc(DATA1_SIZE), game_alloc(MARKER_SIZE));
        if data1 == 0 || extra == 0 {
            return;
        }
        let object = load_object(LIST, NAME.as_ptr(), main);
        if object == 0 {
            return;
        }
        ((object + OBJ_DATA1) as *mut usize).write(data1);
        ((object + OBJ_EXTRA) as *mut usize).write(extra);
        ((object + OBJ_DELETE) as *mut usize).write(deleted as *const () as usize);
        ((object + OBJ_DISPLAY) as *mut usize).write(display);
        ((data1 + D_POSITION) as *mut [f32; 3]).write_unaligned(from.unwrap_or(aim));
        ((extra + M_FRAMES) as *mut u16).write_unaligned(7);
        ((extra + M_AIM) as *mut [f32; 3]).write_unaligned(aim);
        ((extra + M_TARGET) as *mut usize).write(l.stand_in);
        object
    };
    let kind = if from.is_some() {
        Kind::Laser { age: 0 }
    } else {
        Kind::Cursor
    };
    l.shown.insert(
        object,
        Shown {
            friend,
            record,
            aim,
            kind,
        },
    );
}

/// A cursor's update: it stays on its target (where it is here, or where the friend aimed).
extern "C" fn cursor_main(object: usize) {
    crate::watch::mark("lock-on cursor (lockon.rs)");
    with(|l| {
        let Some(s) = l.shown.get_mut(&object) else {
            return;
        };
        if let Some(at) = live_aim(s.record) {
            s.aim = at;
        }
        place(object, None, s.aim);
    });
}

/// A laser's update: it flies to its target and goes when it gets there.
extern "C" fn laser_main(object: usize) {
    crate::watch::mark("lock-on laser (lockon.rs)");
    with(|l| {
        let Some(s) = l.shown.get_mut(&object) else {
            return;
        };
        if let Some(at) = live_aim(s.record) {
            s.aim = at;
        }
        let Kind::Laser { age } = &mut s.kind else {
            return;
        };
        *age += 1;
        // SAFETY: our own object's Data1 (allocated in `spawn`).
        let here = unsafe {
            let d1 = ((object + OBJ_DATA1) as *const usize).read();
            ((d1 + D_POSITION) as *const [f32; 3]).read_unaligned()
        };
        let gap = [s.aim[0] - here[0], s.aim[1] - here[1], s.aim[2] - here[2]];
        let length = (gap[0] * gap[0] + gap[1] * gap[1] + gap[2] * gap[2]).sqrt();
        let (arrived, aged) = (length <= SPEED, *age > LIFE_FRAMES);
        let next = if arrived {
            s.aim
        } else {
            let k = SPEED / length;
            [
                here[0] + gap[0] * k,
                here[1] + gap[1] * k,
                here[2] + gap[2] * k,
            ]
        };
        let aim = s.aim;
        place(object, Some(next), aim);
        if arrived || aged {
            remove(l, object);
        }
    });
}

fn place(object: usize, position: Option<[f32; 3]>, aim: [f32; 3]) {
    // SAFETY: our own object's Data1 and marker data (allocated in `spawn`).
    unsafe {
        if let Some(p) = position {
            let d1 = ((object + OBJ_DATA1) as *const usize).read();
            ((d1 + D_POSITION) as *mut [f32; 3]).write_unaligned(p);
        }
        let extra = ((object + OBJ_EXTRA) as *const usize).read();
        ((extra + M_AIM) as *mut [f32; 3]).write_unaligned(aim);
    }
}

extern "C" fn cursor_display(object: usize) {
    draw(object, CURSOR_DISPLAY);
}

extern "C" fn laser_display(object: usize) {
    draw(object, LASER_DISPLAY);
}

/// Runs the game's marker display `display` on one of ours: with our own textures as a mech,
/// or with the stand-in player 2 lending the WALKTEX we loaded.
fn draw(object: usize, display: usize) {
    crate::watch::mark("lock-on draw (lockon.rs)");
    let lender = with(|l| l.texlist.map(|_| l.lender));
    let own = mech() && player_object() != 0;
    if !own && lender.is_none() {
        return;
    }
    // SAFETY: our own object and marker data (from `spawn`); the game's display as it calls
    // its markers' (it compares +0x1C with itself); player 2's slots (empty in 1P but for a
    // rival) are put back right after.
    unsafe {
        let extra = ((object + OBJ_EXTRA) as *const usize).read();
        let run = std::mem::transmute::<usize, extern "C" fn(usize)>(display);
        let ours = ((object + OBJ_DISPLAY) as *const usize).read();
        ((object + OBJ_DISPLAY) as *mut usize).write(display);
        match lender {
            Some((player, co2)) if !own => {
                let slots = [(PLAYER_OBJECTS + 4, player), (CHAR_OBJ2S + 4, co2)];
                let saved = slots.map(|(at, _)| (at as *const usize).read());
                for (at, value) in slots {
                    (at as *mut usize).write(value);
                }
                ((extra + M_PLAYER) as *mut u8).write(1);
                run(object);
                for ((at, _), old) in slots.into_iter().zip(saved) {
                    (at as *mut usize).write(old);
                }
            }
            _ => {
                ((extra + M_PLAYER) as *mut u8).write(0);
                run(object);
            }
        }
        ((object + OBJ_DISPLAY) as *mut usize).write(ours);
    }
}

/// Delete callback: the game frees one of ours (we removed it, or the stage ended).
extern "C" fn deleted(object: usize) {
    crate::watch::mark("lock-on deleted (lockon.rs)");
    with(|l| {
        l.shown.remove(&object);
    });
}

/// A zeroed block from the game's allocator, with the header its free expects (0x77DF40).
pub(crate) unsafe fn game_alloc(size: usize) -> usize {
    // SAFETY: the game's allocator table: +0 alloc(size, file, line).
    unsafe {
        let allocator = (ALLOCATOR as *const usize).read();
        if allocator == 0 {
            return 0;
        }
        let alloc: extern "C" fn(usize, *const u8, i32) -> usize =
            std::mem::transmute((allocator as *const usize).read());
        let block = alloc(size + 4, c"sa2-chao-online".as_ptr().cast(), 0);
        if block == 0 {
            return 0;
        }
        (block as *mut u32).write(0x1234_5678);
        std::ptr::write_bytes((block + 4) as *mut u8, 0, size);
        block + 4
    }
}

#[cfg(target_arch = "x86")]
pub(crate) unsafe fn load_object(list: u32, name: *const u8, main: usize) -> usize {
    let object: usize;
    // SAFETY: the game's LoadObject (ESI = list, EDI = main, name on the stack, caller pops).
    unsafe {
        std::arch::asm!(
            "push esi",
            "push edi",
            "mov esi, ecx",
            "mov edi, edx",
            "push eax",
            "mov eax, {func}",
            "call eax",
            "add esp, 4",
            "pop edi",
            "pop esi",
            func = const LOAD_OBJECT,
            inout("eax") name => object,
            inout("ecx") list => _,
            inout("edx") main => _,
            clobber_abi("C"),
        );
    }
    object
}

#[cfg(not(target_arch = "x86"))]
pub(crate) unsafe fn load_object(_: u32, _: *const u8, _: usize) -> usize {
    let _ = LOAD_OBJECT;
    0
}
