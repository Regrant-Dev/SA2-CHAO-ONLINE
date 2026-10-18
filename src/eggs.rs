//! Eggs visiting a shared garden (re/chao-garden-sync.md "Eggs").
//!
//! Eggs are Chao records too (type 1) and visit like hatched Chao: the owner's game sends the
//! record and friends' games make a copy with CreateChao, which gives an egg object (the egg
//! spawner 0x57B9C0; registry category 1, MainSub 0x57B520) instead of a Chao. The egg's own
//! update is a 10-mode state machine; modes 4, 6 and 8 hatch it (CreateChao, and its shell as
//! a hat).
//!
//! - The owner's game reports where its visiting eggs are (or that it holds one).
//! - A copy doesn't run its update: it follows the owner's egg, so it never hatches here. While
//!   a friend holds a copy it does run (so it's in their hands), but never into a hatching mode;
//!   their game tells the owner's, which holds the real egg where they are until they put it
//!   down there.
//! - When the owner's egg hatches it's no longer reported as an egg: the copy goes, and the
//!   hatched Chao comes back as a visitor (garden.rs).

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use crate::chao::CHAO_SIZE;
use crate::game::Vector;
use crate::net::protocol::Body;

/// Chao-world registry: eggs are category 1.
const WORLD_COUNTS: usize = 0x01DC_0F80;
const WORLD: usize = 0x01DC_0FC0;
const WORLD_ENTRY_SIZE: usize = 0x48;
const WORLD_CATEGORY_SIZE: usize = 0x1200;
const EGG_CATEGORY: usize = 1;
const EGG_SLOTS: usize = 16;
const ENTRY_OBJECT: usize = 0x3C;
const OBJ_MAIN: usize = 0x10;
const OBJ_DATA1: usize = 0x34;
/// Egg Data1: mode, position, its Chao record.
const D_MODE: usize = 0x00;
const D_POSITION: usize = 0x14;
const D_RECORD: usize = 0x5C;
const EGG_MAIN: usize = 0x0057_B520;
const HATCHING: [u8; 3] = [4, 6, 8];
/// The object goes on its next run.
const DELETE_OBJECT: usize = 0x0046_F720;
/// The save's Chao records.
const CHAO_SLOTS: usize = 0x019F_6EC4;
/// Players' CharObj2 pointers; +0x94 what they hold.
const PLAYER_CHAR_OBJ2: usize = 0x01DE_9600;
const HELD_OBJECT: usize = 0x94;

/// Report our eggs every this many frames (~10 per second).
const SEND_EVERY: u32 = 6;
/// A copy goes when its owner stops reporting it for this long.
const STALE: Duration = Duration::from_millis(2000);
/// A copy put down keeps its own fall this long before following the owner's again.
const SETTLE_FRAMES: u32 = 60;
/// How fast a copy follows (share of the gap a frame), and where a friend holds ours.
const FOLLOW: f32 = 0.3;
const HANDS_UP: f32 = 4.0;

struct Copy {
    object: usize,
    target: Vector,
    seen: Instant,
    /// We hold it; frames since we put it down.
    held: bool,
    settle: u32,
}

#[derive(Default)]
struct Eggs {
    frame: u32,
    /// Our visiting eggs: index -> save slot.
    mine: Vec<(u8, usize)>,
    /// Our eggs a friend holds: index -> friend.
    held_by: HashMap<u8, u8>,
    /// Where friends are (for the eggs they hold).
    friends: HashMap<u8, Vector>,
    copies: HashMap<(u8, u8), Copy>,
    /// Reports: (owner, index) -> (where, held by its owner, when).
    reports: HashMap<(u8, u8), (Vector, bool, Instant)>,
    /// Copies gone: their visitor buffers can be reused.
    left: Vec<(u8, u8)>,
}

static EGGS: Mutex<Option<Eggs>> = Mutex::new(None);

fn with<R>(f: impl FnOnce(&mut Eggs) -> R) -> R {
    let mut g = EGGS.lock().unwrap_or_else(|e| e.into_inner());
    f(g.get_or_insert_with(Eggs::default))
}

/// Our visiting Chao (index, save slot); the eggs among them are reported.
pub fn set_mine(slots: Vec<(u8, usize)>) {
    with(|e| e.mine = slots);
}

/// A friend's egg `index` was made here as `object` (visitors.rs).
pub fn spawned(owner: u8, index: u8, object: usize) {
    if object == 0 {
        return;
    }
    // SAFETY: the egg just made: our MainSub instead of its own (which we call while held).
    unsafe { ((object + OBJ_MAIN) as *mut usize).write(copy_main as *const () as usize) };
    let target = position(object);
    with(|e| {
        e.copies.insert(
            (owner, index),
            Copy {
                object,
                target,
                seen: Instant::now(),
                held: false,
                settle: 0,
            },
        )
    });
}

/// Whether friend `owner`'s egg `index` is here (reported lately), so it may be made.
pub fn visiting(owner: u8, index: u8) -> bool {
    with(|e| {
        e.reports
            .get(&(owner, index))
            .is_some_and(|&(_, _, at)| at.elapsed() < STALE)
    })
}

/// Network: friend `owner`'s visiting eggs.
pub fn received(owner: u8, eggs: Vec<(u8, [f32; 3], bool)>) {
    let now = Instant::now();
    with(|e| {
        for (index, [x, y, z], held) in eggs {
            e.reports
                .insert((owner, index), (Vector { x, y, z }, held, now));
        }
    });
}

/// Network: friend `from` picked up or put down our egg `index`.
pub fn received_held(from: u8, index: u8, held: bool, [x, y, z]: [f32; 3]) {
    with(|e| {
        let Some(&(_, slot)) = e.mine.iter().find(|&&(i, _)| i == index) else {
            return;
        };
        let Some(object) = real_egg(slot) else {
            return;
        };
        if held {
            e.held_by.insert(index, from);
            // SAFETY: our live egg: held where the friend is, by our MainSub, until let go.
            unsafe {
                ((object + OBJ_MAIN) as *mut usize).write(friend_held_main as *const () as usize)
            };
        } else if e.held_by.get(&index) == Some(&from) {
            e.held_by.remove(&index);
            // SAFETY: our live egg, back to its own update, where they put it down.
            unsafe {
                set_position(object, Vector { x, y, z });
                ((object + OBJ_MAIN) as *mut usize).write(EGG_MAIN);
            }
        }
    });
}

/// Copies that went (their owner took them home, or they hatched there): their visitor buffers
/// can be reused.
pub fn take_left() -> Vec<(u8, u8)> {
    with(|e| std::mem::take(&mut e.left))
}

/// We left the garden: its objects went with it.
pub fn clear() {
    with(|e| {
        e.copies.clear();
        e.held_by.clear();
        e.reports.clear();
    });
}

/// Call every frame (game thread) in garden `area` (1..=3): where friends are. Returns what to
/// send.
pub fn on_frame(area: u8, friends: &HashMap<u8, Vector>) -> Vec<Body> {
    let mut out = Vec::new();
    with(|e| {
        e.frame = e.frame.wrapping_add(1);
        e.friends = friends.clone();
        // Friends who left drop what they held of ours where they were.
        let gone: Vec<u8> = e
            .held_by
            .iter()
            .filter(|(_, f)| !friends.contains_key(f))
            .map(|(&i, _)| i)
            .collect();
        for index in gone {
            e.held_by.remove(&index);
            if let Some(&(_, slot)) = e.mine.iter().find(|&&(i, _)| i == index)
                && let Some(object) = real_egg(slot)
            {
                // SAFETY: our live egg, back to its own update.
                unsafe { ((object + OBJ_MAIN) as *mut usize).write(EGG_MAIN) };
            }
        }
        // Ours, to everyone.
        if e.frame.is_multiple_of(SEND_EVERY) {
            let held = held_object();
            let eggs: Vec<(u8, [f32; 3], bool)> = e
                .mine
                .iter()
                .filter_map(|&(index, slot)| {
                    let object = real_egg(slot)?;
                    let p = position(object);
                    Some((index, [p.x, p.y, p.z], held == object))
                })
                .collect();
            if !eggs.is_empty() {
                out.push(Body::GardenEggs { area, eggs });
            }
        }
        // Copies: gone when no longer reported; otherwise following, or in our hands.
        let held = held_object();
        let now = Instant::now();
        let mut gone = Vec::new();
        for (&(owner, index), copy) in &mut e.copies {
            let report = e.reports.get(&(owner, index));
            match report {
                Some(&(at, _, when)) if now.duration_since(when) < STALE => {
                    copy.target = at;
                    copy.seen = when;
                }
                _ => {
                    gone.push((owner, index));
                    continue;
                }
            }
            let ours = held == copy.object;
            if ours != copy.held {
                copy.held = ours;
                copy.settle = 0;
                let p = position(copy.object);
                out.push(Body::EggHeld {
                    owner,
                    index,
                    held: ours,
                    position: [p.x, p.y, p.z],
                });
            }
            if !ours && copy.settle < SETTLE_FRAMES {
                copy.settle += 1;
            }
        }
        for key in gone {
            if let Some(copy) = e.copies.remove(&key) {
                // SAFETY: our copy; it goes on its next run.
                unsafe { ((copy.object + OBJ_MAIN) as *mut usize).write(DELETE_OBJECT) };
                e.left.push(key);
            }
        }
    });
    out
}

/// What player 1 holds (0 for nothing).
fn held_object() -> usize {
    // SAFETY: the game's static player table and player 1's CharObj2 (when there is one).
    unsafe {
        let co2 = (PLAYER_CHAR_OBJ2 as *const usize).read();
        if co2 == 0 {
            0
        } else {
            ((co2 + HELD_OBJECT) as *const usize).read()
        }
    }
}

/// Our live egg object whose record is save slot `slot`.
fn real_egg(slot: usize) -> Option<usize> {
    let record = CHAO_SLOTS + slot * CHAO_SIZE;
    // SAFETY: the game's static registry; the count is clamped and objects are checked.
    unsafe {
        let count = ((WORLD_COUNTS + EGG_CATEGORY * 4) as *const u32).read() as usize;
        (0..count.min(EGG_SLOTS)).find_map(|i| {
            let entry = WORLD + EGG_CATEGORY * WORLD_CATEGORY_SIZE + i * WORLD_ENTRY_SIZE;
            let object = ((entry + ENTRY_OBJECT) as *const usize).read();
            if object == 0 {
                return None;
            }
            let data1 = ((object + OBJ_DATA1) as *const usize).read();
            (data1 != 0 && ((data1 + D_RECORD) as *const usize).read() == record).then_some(object)
        })
    }
}

fn position(object: usize) -> Vector {
    // SAFETY: a live egg's Data1.
    unsafe {
        let data1 = ((object + OBJ_DATA1) as *const usize).read();
        ((data1 + D_POSITION) as *const Vector).read_unaligned()
    }
}

/// # Safety
/// `object` is a live egg.
unsafe fn set_position(object: usize, p: Vector) {
    unsafe {
        let data1 = ((object + OBJ_DATA1) as *const usize).read();
        ((data1 + D_POSITION) as *mut Vector).write_unaligned(p);
    }
}

/// A copy's MainSub: following its owner's egg; in our hands it runs its own update, but never
/// into hatching.
extern "C" fn copy_main(object: usize) {
    let (held, settle, target) = with(|e| {
        e.copies
            .values()
            .find(|c| c.object == object)
            .map_or((false, SETTLE_FRAMES, None), |c| {
                (c.held, c.settle, Some(c.target))
            })
    });
    // SAFETY: a live egg copy of ours (its Data1 checked).
    unsafe {
        let data1 = ((object + OBJ_DATA1) as *const usize).read();
        if data1 == 0 {
            return;
        }
        if held || settle < SETTLE_FRAMES {
            let mode = ((data1 + D_MODE) as *const u8).read();
            std::mem::transmute::<usize, extern "C" fn(usize)>(EGG_MAIN)(object);
            if HATCHING.contains(&((data1 + D_MODE) as *const u8).read()) {
                ((data1 + D_MODE) as *mut u8).write(mode);
            }
            return;
        }
        if let Some(t) = target {
            let p = position(object);
            set_position(
                object,
                Vector {
                    x: p.x + (t.x - p.x) * FOLLOW,
                    y: p.y + (t.y - p.y) * FOLLOW,
                    z: p.z + (t.z - p.z) * FOLLOW,
                },
            );
        }
    }
}

/// Our egg while a friend holds it: where they are (above their head), not running its update.
extern "C" fn friend_held_main(object: usize) {
    let at = with(|e| {
        let (_, slot_index) = e
            .mine
            .iter()
            .map(|&(i, s)| (s, i))
            .find(|&(s, _)| real_egg_is(s, object))?;
        let friend = e.held_by.get(&slot_index)?;
        e.friends.get(friend).copied()
    });
    if let Some(p) = at {
        // SAFETY: our live egg.
        unsafe {
            set_position(
                object,
                Vector {
                    x: p.x,
                    y: p.y + HANDS_UP,
                    z: p.z,
                },
            )
        };
    }
}

fn real_egg_is(slot: usize, object: usize) -> bool {
    // SAFETY: a live egg's Data1.
    unsafe {
        let data1 = ((object + OBJ_DATA1) as *const usize).read();
        data1 != 0 && ((data1 + D_RECORD) as *const usize).read() == CHAO_SLOTS + slot * CHAO_SIZE
    }
}
