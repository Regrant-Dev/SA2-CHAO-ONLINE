//! Shared garden toys: the TV, the radio and the jack-in-the-box (re/chao-garden-sync.md
//! "Toys").
//!
//! Each is one object (object list 4) whose Data1 byte 0 is a mode, stepped by its MainSub:
//! - TV (0x55C540): 0 off, 1 switching on, then channels (odd modes start one, the next even
//!   one plays it), 3 switching off; a touch (hit 0xAF while off, 0xB1 while on) switches it on
//!   or over, and after switching on it picks a channel at random.
//! - Radio (0x57C840): 0 off, 1 on, songs 4 to 6 (picked at random), 3 off; touches as the TV.
//! - Box (0x5801C0): 0 shut, a touch (0xAF) starts 1, which pops it open (2 to 4) and back to 0.
//!
//! A mode a toy's update jumps to (anything but the next mode) is the same state however it got
//! there: the update clears the counters (+1, +3, +6) as it jumps. So jumps are what's shared:
//! one player keeps the toys (the lowest player id in the garden, as with the trees) and sends
//! every jump their toys make; everyone else's toys jump the same. A touch in someone else's
//! game (by them or by a Chao) goes to the keeper, whose toy jumps for real and tells everyone.
//! The steps in between run in every game alike. Only these mode bytes are shared, never the
//! objects' pointers.

use std::sync::Mutex;

use crate::game;
use crate::net::protocol::{Body, TOY_BOX, TOY_MAX_MODE, TOY_RADIO, TOY_TV};

const OBJECT_LISTS: usize = 0x01A5_A254;
/// The toys' object list (their constructors make them in list 4).
const TOY_LIST: usize = 4;
const OBJ_NEXT: usize = 0x04;
const OBJ_MAIN: usize = 0x10;
const OBJ_DATA1: usize = 0x34;
/// Toy Data1: mode, two counters and a timer (u16) the update clears as it jumps.
const D_MODE: usize = 0x00;
const D_COUNT: usize = 0x01;
const D_FRAMES: usize = 0x03;
const D_TIMER: usize = 0x06;
/// The object's collision hits from `kind` (cdecl (object, kind) -> hit or null; the hit's +4
/// is who hit it).
#[cfg(target_arch = "x86")]
const HIT_BY: usize = 0x0048_6760;
#[cfg(target_arch = "x86")]
const HIT_WHO: usize = 0x04;
/// Hit kinds: a touch on a toy that's off (or shut), and on one that's on.
#[cfg(target_arch = "x86")]
const TOUCH_OFF: u32 = 0xAF;
#[cfg(target_arch = "x86")]
const TOUCH_ON: u32 = 0xB1;

/// The box's mode 1 counts its timer down to opening: a touch sets 30 less a little at random.
const BOX_OPENING: u16 = 20;
const BOX_OPEN: u8 = 1;

const TOYS: [Toy; 3] = [
    Toy {
        id: TOY_TV,
        main: 0x0055_C540,
        wrapped: toy_main::<0>,
    },
    Toy {
        id: TOY_RADIO,
        main: 0x0057_C840,
        wrapped: toy_main::<1>,
    },
    Toy {
        id: TOY_BOX,
        main: 0x0058_01C0,
        wrapped: toy_main::<2>,
    },
];

struct Toy {
    id: u8,
    main: usize,
    wrapped: extern "C" fn(usize),
}

struct Toys {
    area: u8,
    /// Whether we keep the toys.
    keeper: bool,
    /// Each toy's object (0 when it's not found yet).
    objects: [usize; 3],
    /// Jumps to make our toys do (toy, mode), and jumps to send (toy, mode).
    to_make: Vec<(u8, u8)>,
    to_send: Vec<(u8, u8)>,
}

static TOYS_NOW: Mutex<Toys> = Mutex::new(Toys {
    area: 0,
    keeper: true,
    objects: [0; 3],
    to_make: Vec::new(),
    to_send: Vec::new(),
});

fn lock() -> std::sync::MutexGuard<'static, Toys> {
    TOYS_NOW.lock().unwrap_or_else(|e| e.into_inner())
}

/// Network: player `from`'s toy jumped to `mode` in garden `area`; `keeper` is whether they
/// keep the toys. The keeper's jumps are made here; others' (their touches) only by the keeper.
pub fn received(keeper: bool, area: u8, toy: u8, mode: u8) {
    let mut s = lock();
    if s.area == area && area != 0 && (keeper || s.keeper) && mode <= TOY_MAX_MODE {
        s.to_make.push((toy, mode));
    }
}

/// Whether our toy at `now` should jump to `mode`: not when it's there already, or just past it
/// (it started what `mode` starts).
pub fn should_jump(now: u8, mode: u8) -> bool {
    now != mode && (mode == 0 || now != mode + 1)
}

/// Call every frame (game thread): our garden (1..=3, or 0) and whether we keep its toys.
/// Returns what to send.
pub fn on_frame(area: u8, keeper: bool) -> Vec<Body> {
    let mut s = lock();
    if area == 0 || !game::in_garden() {
        if s.area != 0 {
            *s = Toys {
                area: 0,
                keeper: true,
                objects: [0; 3],
                to_make: Vec::new(),
                to_send: Vec::new(),
            };
        }
        return Vec::new();
    }
    if s.area != area {
        s.area = area;
        s.objects = [0; 3];
        s.to_make.clear();
        s.to_send.clear();
    }
    s.keeper = keeper;
    // Our toys: found in their list, and their updates wrapped (so their jumps are seen).
    for (i, toy) in TOYS.iter().enumerate() {
        let object = s.objects[i];
        // SAFETY: an object found in the list earlier; checked to still run our wrapper.
        if object != 0
            && unsafe { ((object + OBJ_MAIN) as *const usize).read() } != toy.wrapped as usize
        {
            s.objects[i] = 0;
        }
        if s.objects[i] == 0
            && let Some(object) = find(toy.main)
        {
            // SAFETY: a live toy running its own MainSub; ours calls it.
            unsafe { ((object + OBJ_MAIN) as *mut usize).write(toy.wrapped as usize) };
            s.objects[i] = object;
        }
    }
    // Jumps from the network.
    for (toy, mode) in std::mem::take(&mut s.to_make) {
        let Some(i) = TOYS.iter().position(|t| t.id == toy) else {
            continue;
        };
        let object = s.objects[i];
        if object == 0 {
            continue;
        }
        // SAFETY: a live toy's Data1 (checked above this frame).
        unsafe {
            let data1 = ((object + OBJ_DATA1) as *const usize).read();
            if data1 == 0 || !should_jump((data1 as *const u8).read(), mode) {
                continue;
            }
            jump(data1, toy, mode);
        }
        // The keeper made a friend's touch: everyone's toy does it.
        if s.keeper {
            s.to_send.push((toy, mode));
        }
    }
    std::mem::take(&mut s.to_send)
        .into_iter()
        .map(|(toy, mode)| Body::ToyMode { area, toy, mode })
        .collect()
}

/// Puts a toy in `mode` as its update does when it jumps there.
///
/// # Safety
/// `data1` is a live toy's Data1.
unsafe fn jump(data1: usize, toy: u8, mode: u8) {
    let timer = if toy == TOY_BOX && mode == BOX_OPEN {
        BOX_OPENING
    } else {
        0
    };
    // SAFETY: the caller's.
    unsafe {
        ((data1 + D_MODE) as *mut u8).write(mode);
        ((data1 + D_COUNT) as *mut u8).write(0);
        ((data1 + D_FRAMES) as *mut u8).write(0);
        ((data1 + D_TIMER) as *mut u16).write_unaligned(timer);
    }
}

/// The object in the toys' list running `main`.
fn find(main: usize) -> Option<usize> {
    // SAFETY: the game's static list head and live list, walked on the game thread.
    unsafe {
        let head = ((OBJECT_LISTS + TOY_LIST * 4) as *const usize).read();
        let mut object = head;
        for _ in 0..4096 {
            if object == 0 {
                return None;
            }
            if ((object + OBJ_MAIN) as *const usize).read() == main
                && ((object + OBJ_DATA1) as *const usize).read() != 0
            {
                return Some(object);
            }
            object = ((object + OBJ_NEXT) as *const usize).read();
            if object == head {
                return None;
            }
        }
    }
    None
}

/// Whether something touched the toy this frame.
#[cfg(target_arch = "x86")]
fn touched(object: usize) -> bool {
    [TOUCH_OFF, TOUCH_ON].into_iter().any(|kind| {
        // SAFETY: the game's hit lookup on a live object; it only reads.
        unsafe {
            let hit = std::mem::transmute::<usize, extern "C" fn(usize, u32) -> usize>(HIT_BY)(
                object, kind,
            );
            hit != 0 && ((hit + HIT_WHO) as *const usize).read() != 0
        }
    })
}

#[cfg(not(target_arch = "x86"))]
fn touched(_object: usize) -> bool {
    false
}

/// A toy's MainSub: its own, watching it jump. The keeper sends every jump; the others send
/// the jumps a touch made (for the keeper to make).
extern "C" fn toy_main<const N: usize>(object: usize) {
    let toy = &TOYS[N];
    // SAFETY: a live toy's Data1 (its MainSub is running).
    let data1 = unsafe { ((object + OBJ_DATA1) as *const usize).read() };
    let touch = touched(object);
    // SAFETY: as above.
    let before = (data1 != 0).then(|| unsafe { (data1 as *const u8).read() });
    // SAFETY: the toy's own MainSub, called as the game calls it.
    unsafe { std::mem::transmute::<usize, extern "C" fn(usize)>(toy.main)(object) };
    let Some(before) = before else {
        return;
    };
    // SAFETY: as above (the update doesn't free its own Data1).
    let after = unsafe { (data1 as *const u8).read() };
    if after == before || after == before.wrapping_add(1) || after > TOY_MAX_MODE {
        return;
    }
    let mut s = lock();
    if s.keeper || touch {
        s.to_send.push((toy.id, after));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn jumps_only_when_not_there() {
        assert!(should_jump(0, 1));
        assert!(should_jump(2, 7));
        assert!(!should_jump(7, 7));
        assert!(!should_jump(8, 7));
        assert!(should_jump(1, 0));
        assert!(!should_jump(0, 0));
    }
}
