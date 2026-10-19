//! Chao carried around Chao World outside the gardens (lobby, kindergarten, entrances).
//!
//! A player who walks out of their garden with a Chao in their hands carries it on: the game
//! makes it again in the next area from its record (0x1A5CA5C, re/chao-garden-items.md 5.4).
//! The others saw the puppet's carrying pose with empty hands. Now the carrier's game sends the
//! Chao's record (`CarriedChao`), and the others make a display-only copy (no AI, no collision,
//! as the game's menus make theirs) in the puppet's hands, gone when it's put down or the
//! carrier leaves the area.

use std::collections::HashMap;
use std::sync::Mutex;

use crate::chao::ChaoData;
use crate::game::{self, Vector};
use crate::log;
use crate::net::protocol::Body;

const OBJ_MAIN: usize = 0x10;
const OBJ_DATA1: usize = 0x34;
const OBJ_DATA2: usize = 0x38;
/// A Chao's MainSub.
const CHAO_MAIN: usize = 0x0054_FE20;
/// Data1: status (0x8000 held: drawn in hands, can't be picked up), facing, position, record,
/// flags (as the menus' display-only Chao: no collision 0x8, 0x2, no AI 0x10, 0x20, 0x80000),
/// motion controller.
const D_STATUS: usize = 0x04;
const STATUS_HELD: u16 = 0x8000;
const D_ROTATION_Y: usize = 0x0C;
const D_POSITION: usize = 0x14;
const D_RECORD: usize = 0x5C;
const D_FLAGS: usize = 0xA0;
const DISPLAY_ONLY_CLEAR: u32 = 0x8 | 0x2 | 0x10 | 0x20 | 0x8_0000;
const D_MOTION: usize = 0xAC;
/// Held calmly (re/chao-garden-sync.md 2), started with SetMotionLink (EAX = controller, EDX =
/// id).
const MOTION_HELD: u32 = 0x101;
#[cfg(target_arch = "x86")]
const START_MOTION: usize = 0x0079_3C40;

/// While carrying, our Chao goes out this often (frames), with where it is in our hands.
const SEND_EVERY: u32 = 60;

struct Carried {
    /// What we carry: its record's address (0 for nothing), and frames since sent.
    ours: usize,
    frame: u32,
    /// The others': id -> (their Chao, offset in their frame, our copy object or 0).
    theirs: HashMap<u8, Theirs>,
}

struct Theirs {
    chao: Box<ChaoData>,
    offset: [f32; 3],
    object: usize,
}

static CARRIED: Mutex<Option<Carried>> = Mutex::new(None);
/// Where each copy goes this frame: object -> (position, facing).
static PLACES: Mutex<Vec<(usize, Vector, i32)>> = Mutex::new(Vec::new());

fn with<R>(f: impl FnOnce(&mut Carried) -> R) -> R {
    let mut c = CARRIED.lock().unwrap_or_else(|e| e.into_inner());
    f(c.get_or_insert_with(|| Carried {
        ours: 0,
        frame: 0,
        theirs: HashMap::new(),
    }))
}

/// Network: player `from` carries `chao` (or nothing now), `offset` from them.
pub fn received(from: u8, chao: Option<Box<ChaoData>>, offset: [f32; 3]) {
    with(|c| match chao {
        Some(chao) => {
            let same = c
                .theirs
                .get(&from)
                .is_some_and(|t| t.chao.bytes == chao.bytes);
            if same {
                if let Some(t) = c.theirs.get_mut(&from) {
                    t.offset = offset;
                }
            } else {
                let object = c.theirs.remove(&from).map_or(0, |t| t.object);
                delete(object);
                c.theirs.insert(
                    from,
                    Theirs {
                        chao,
                        offset,
                        object: 0,
                    },
                );
            }
        }
        None => {
            if let Some(t) = c.theirs.remove(&from) {
                delete(t.object);
            }
        }
    });
}

/// Player `id` left: their copy goes.
pub fn player_left(id: u8) {
    received(id, None, [0.0; 3]);
}

/// Call every frame (game thread): `outside` is whether we're in Chao World but not in a
/// garden; `holders` the others here (id, position, facing). Returns what to send.
pub fn on_frame(outside: bool, holders: &[(u8, Vector, i32)]) -> Option<Body> {
    let send = ours(outside);
    let mut places = Vec::new();
    with(|c| {
        for (&id, t) in &mut c.theirs {
            // SAFETY: an object we made; it went if it no longer runs our MainSub (the area
            // changed and the game deleted it).
            if t.object != 0
                && unsafe { ((t.object + OBJ_MAIN) as *const usize).read() }
                    != copy_main as extern "C" fn(usize) as usize
            {
                t.object = 0;
            }
            let holder = holders.iter().find(|&&(h, _, _)| h == id);
            match (outside, holder) {
                (true, Some(&(_, at, angle))) => {
                    let o = crate::garden::rotate(t.offset, angle);
                    let spot = Vector {
                        x: at.x + o[0],
                        y: at.y + o[1],
                        z: at.z + o[2],
                    };
                    if t.object == 0 {
                        t.object = make_copy(&t.chao, spot, angle);
                        if t.object != 0 {
                            log!("carried: player {id} carries a Chao here; showing it");
                        }
                    }
                    if t.object != 0 {
                        places.push((t.object, spot, angle));
                    }
                }
                _ => {
                    delete(std::mem::take(&mut t.object));
                }
            }
        }
    });
    *PLACES.lock().unwrap_or_else(|e| e.into_inner()) = places;
    send
}

/// Our own carried Chao, to send when it changes and now and then.
fn ours(outside: bool) -> Option<Body> {
    let held = if outside {
        crate::garden::player_held()
    } else {
        0
    };
    // SAFETY: the object our player holds (null-checked), read on the game thread.
    let (record, offset) = unsafe {
        if held != 0 && ((held + OBJ_MAIN) as *const usize).read() == CHAO_MAIN {
            let d = ((held + OBJ_DATA1) as *const usize).read();
            let record = if d == 0 {
                0
            } else {
                ((d + D_RECORD) as *const usize).read()
            };
            let offset = match (d, game::player_position()) {
                (0, _) | (_, None) => [0.0; 3],
                (_, Some((at, angle))) => {
                    let p = ((d + D_POSITION) as *const Vector).read_unaligned();
                    crate::garden::rotate(
                        [p.x - at.x, p.y - at.y, p.z - at.z],
                        angle.wrapping_neg(),
                    )
                }
            };
            (record, offset)
        } else {
            (0, [0.0; 3])
        }
    };
    with(|c| {
        c.frame = c.frame.wrapping_add(1);
        if record == c.ours && (record == 0 || !c.frame.is_multiple_of(SEND_EVERY)) {
            return None;
        }
        let changed = record != c.ours;
        c.ours = record;
        if record == 0 {
            return Some(Body::CarriedChao { chao: None, offset });
        }
        // SAFETY: the held Chao's record (a full Chao record in the game's memory).
        let chao = unsafe { (record as *const ChaoData).read() };
        if changed {
            log!("carried: carrying a Chao out of the garden; showing it to the others");
        }
        chao.validate().is_ok().then(|| Body::CarriedChao {
            chao: Some(Box::new(chao)),
            offset,
        })
    })
}

/// A display-only Chao made from `chao` (its record kept for the object's life), or 0.
fn make_copy(chao: &ChaoData, at: Vector, angle: i32) -> usize {
    // The game keeps a pointer to the record while the Chao lives; one leaked buffer per carry
    // (2 KB), so a copy being deleted never sees the next one's data.
    let data: &'static mut ChaoData = Box::leak(Box::new(chao.clone()));
    // SAFETY: game thread, in Chao World (the game makes carried Chao here itself); the record
    // outlives the object.
    let object = unsafe { game::create_chao(data, at, angle) } as usize;
    if object == 0 {
        return 0;
    }
    // SAFETY: the Chao object just made.
    unsafe {
        let d = ((object + OBJ_DATA1) as *const usize).read();
        if d != 0 {
            let flags = (d + D_FLAGS) as *mut u32;
            flags.write(flags.read() & !DISPLAY_ONLY_CLEAR);
            start_motion(d + D_MOTION, MOTION_HELD);
        }
        ((object + OBJ_MAIN) as *mut usize).write(copy_main as extern "C" fn(usize) as usize);
    }
    object
}

fn delete(object: usize) {
    if object != 0 {
        // A Chao: the garden's safe delete (nothing left pointing at it).
        crate::garden::delete_chao(object);
    }
}

#[cfg(target_arch = "x86")]
unsafe fn start_motion(controller: usize, id: u32) {
    // SAFETY: the game's SetMotionLink (EAX = motion controller, EDX = id).
    unsafe {
        std::arch::asm!(
            "call {f}",
            f = in(reg) START_MOTION,
            inout("eax") controller => _,
            inout("edx") id => _,
            clobber_abi("C"),
        );
    }
}

#[cfg(not(target_arch = "x86"))]
unsafe fn start_motion(_controller: usize, _id: u32) {}

/// Writes where the copy goes and that it's held (drawn in hands, can't be picked up).
///
/// # Safety
/// `d` is the copy's Data1.
unsafe fn put(d: usize, at: Vector, angle: i32) {
    unsafe {
        ((d + D_POSITION) as *mut Vector).write_unaligned(at);
        ((d + D_ROTATION_Y) as *mut i32).write(angle);
        let status = (d + D_STATUS) as *mut u16;
        status.write(status.read() | STATUS_HELD);
    }
}

/// MainSub of a copy: in the carrier's hands, then the game's own update (motion, face), then
/// in their hands again (its physics can't move it).
extern "C" fn copy_main(object: usize) {
    let place = PLACES
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .iter()
        .find(|p| p.0 == object)
        .map(|&(_, at, angle)| (at, angle));
    // SAFETY: our copy's Data1 (checked for null), on the game thread; then the Chao's own
    // MainSub, which it had.
    unsafe {
        let d = ((object + OBJ_DATA1) as *const usize).read();
        if d == 0 {
            return;
        }
        if let Some((at, angle)) = place {
            put(d, at, angle);
        }
        std::mem::transmute::<usize, extern "C" fn(usize)>(CHAO_MAIN)(object);
        if let Some((at, angle)) = place {
            put(d, at, angle);
            let d2 = ((object + OBJ_DATA2) as *const usize).read();
            if d2 != 0 {
                (d2 as *mut [f32; 3]).write_unaligned([0.0; 3]);
            }
        }
    }
}
