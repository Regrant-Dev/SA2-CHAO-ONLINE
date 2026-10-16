//! Live shared gardens (re/chao-garden-sync.md, re/chao-garden-items.md).
//!
//! - Each player's game reads their visiting Chao's live objects (position, facing, animation,
//!   held) and sends them ~30 times a second.
//! - In the other games the visitor copy follows that: its AI is off and its MainSub is wrapped
//!   so it stands where the real one is, smoothed on the ground or in the owner's (puppet's)
//!   hands while held. Visitors whose owner sends nothing (an older mod) keep their own AI.
//! - Touching a friend's Chao (petting, picking it up, hitting it, or holding a fruit, animal,
//!   drive or hat near it) switches the copy's own AI on for a few seconds, so the game plays the
//!   real reaction. Whatever that changes in the copy's record (happiness, alignment, bond,
//!   stats, lifespan, animal parts) goes to the owner as differences, and the owner's game adds
//!   them to the real Chao (`chao::changes`).
//! - While you hold a friend's Chao, their game shows their real one in your (puppet's) hands.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use crate::chao::{self, CHAO_SIZE, ChaoData};
use crate::game::{self, Vector};
use crate::log;
use crate::net::protocol::GardenChao;

/// Chao-world object registry: 0x48-byte entries, 0x1200 bytes per category; an entry's record
/// pointer is at +8 and its ObjectMaster at +0x3C (objects point back to their entry at +0x3C).
const WORLD: usize = 0x01DC_0FC0;
const WORLD_ENTRY_SIZE: usize = 0x48;
const WORLD_CATEGORY_SIZE: usize = 0x1200;
const WORLD_CHAO_MAX: usize = 32;
/// Item categories a Chao takes from the player: animals and drives, fruit, hats.
const ITEM_CATEGORIES: [usize; 3] = [2, 3, 9];
const CHAO_SLOTS: usize = 0x019F_6EC4;

const OBJ_MAIN: usize = 0x10;
const OBJ_DATA1: usize = 0x34;
/// Motion/velocity work (+0/+4/+8 velocity).
const OBJ_DATA2: usize = 0x38;
const OBJ_ENTRY: usize = 0x3C;
/// The garden Chao MainSub (set by CreateChao).
const CHAO_MAIN: usize = 0x0054_FE20;

const D_STATUS: usize = 0x04;
const D_ROTATION_Y: usize = 0x0C;
const D_POSITION: usize = 0x14;
/// The Chao's record (`ChaoData*`).
const D_RECORD: usize = 0x5C;
const D_FLAGS: usize = 0xA0;
const D_MOTION: usize = 0xAC;
const D_MOTION_CURRENT: usize = 0xB0;
const D_MOTION_NEXT: usize = 0xB4;
/// Appearance flags: bit 2 = redraw parts/colours after a change.
const D_APPEARANCE: usize = 0x674;
/// Mood display (heart, "?", "!", swirl, ...), a small state machine at Data1+0x6EC: i16 current
/// state (0..=5, function table 0x1353834), i16 requested next (-1 none; 0x53D660 and friends
/// set it, and the next update switches to it), i32 timer, i32 length. Switching clears the
/// request and the state's animation counters (u16 at +0x38, +0x3C, +0x7C, +0x80; 0x53CA88).
const D_MOOD: usize = 0x6EC;
const D_MOOD_REQUEST: usize = 0x6EE;
const D_MOOD_TIMER: usize = 0x6F0;
const D_MOOD_LENGTH: usize = 0x6F4;
const MOOD_COUNTERS: [usize; 4] = [0x38, 0x3C, 0x7C, 0x80];
const MOOD_STATES: i16 = 6;
/// Face: eyes and mouth ids (set by 0x53A4B0 / 0x53A5A0).
const D_EYES: usize = 0x692;
const D_MOUTH: usize = 0x69E;
/// Eye/mouth ids we accept from the network (they index the face texture tables).
const MAX_FACE_ID: i16 = 32;
/// Motion controller (Data1+0xAC): +0x2C current NJS_MOTION*, +0x48 the motion table
/// (0x1C-byte entries: motion*, u16 play mode, ...).
const MC_MOTION: usize = 0x2C;
const MC_TABLE: usize = 0x48;
const MOTION_ENTRY_SIZE: usize = 0x1C;
/// Motion ids we accept from the network (the shared table has a few hundred).
const MAX_MOTION_ID: u16 = 0x400;

/// Data1 status bits a player sets: hit, held. (Bit 0x1, which the pet trigger also checks, is
/// set on every Chao, so being petted can't be told from it: being right next to the Chao
/// counts instead, and the copy's own AI then plays the petting.)
const STATUS_HIT: u16 = 0x0004;
const STATUS_HELD: u16 = 0x8000;
/// Data1+0xA0: AI runs.
const FLAG_AI: u32 = 0x10;
/// Motion controller flags: blending into the next motion.
const MOTION_BLENDING: u16 = 0x2;
/// "Held, calm" motion.
const MOTION_HELD: u16 = 0x101;

/// The local player's CharObj2 pointer; +0x94 is the object they hold.
const PLAYER_CHAR_OBJ2: usize = 0x01DE_9600;
const PLAYER_HELD: usize = 0x94;

/// Start a motion (blends): EAX = Data1+0xAC, EDX = id.
#[cfg(target_arch = "x86")]
const START_MOTION: usize = 0x0079_3C40;
#[cfg(target_arch = "x86")]
const SET_EYES: usize = 0x0053_A4B0;
#[cfg(target_arch = "x86")]
const SET_MOUTH: usize = 0x0053_A5A0;

/// How fast a mirrored Chao catches up with where its owner's is (per frame).
const FOLLOW: f32 = 0.25;
/// Stop following when the owner's updates stop for this long.
const STALE: Duration = Duration::from_secs(2);
/// A touched copy runs its own AI this long after the last touch or change.
const LOCAL_TIME: Duration = Duration::from_secs(5);
/// "Holding an item near it": within this distance.
const ITEM_NEAR: f32 = 25.0;
/// Standing this close to a friend's Chao lets it react to us (petting).
const PLAYER_NEAR: f32 = 12.0;
/// An item this close to a touched Chao is in its hands (eating, holding).
const BUSY_NEAR: f32 = 6.0;
/// A friend holding our Chao: their updates count this long.
const HELD_STALE: Duration = Duration::from_millis(500);

/// `chao::changes::diff` output: (offset, delta or value).
type Changes = Vec<(u16, f32)>;
/// To send: (owner, index, changes) and (owner, index, offset in our frame).
type Outgoing = (Vec<(u8, u8, Changes)>, Vec<(u8, u8, [f32; 3])>);

struct Mirror {
    object: usize,
    /// Latest state from the owner, and when it came.
    target: Option<(GardenChao, Instant)>,
    /// The motion we last started on it.
    motion: Option<u16>,
    /// Taken over (AI off, MainSub wrapped).
    driven: bool,
    /// The copy's record as last forwarded.
    snapshot: Option<Box<ChaoData>>,
    /// Running its own AI (touched) until then.
    local_until: Option<Instant>,
}

/// Our Chao held by a friend.
struct Held {
    holder: u8,
    offset: [f32; 3],
    at: Instant,
}

struct Garden {
    /// (owner, index) -> our visitor object for it.
    mirrors: HashMap<(u8, u8), Mirror>,
    /// Where each other player is drawn now: (position, facing).
    holders: HashMap<u8, (Vector, i32)>,
    /// Our visiting Chao: (index, save slot).
    mine: Vec<(u8, usize)>,
    /// Our Chao (by index) a friend holds.
    held: HashMap<u8, Held>,
    /// Our Chao objects whose MainSub we wrapped, and whether we switched their AI off.
    wrapped: HashMap<usize, bool>,
    /// To send: changes to friends' Chao, and friends' Chao we hold.
    changes_out: Vec<(u8, u8, Changes)>,
    held_out: Vec<(u8, u8, [f32; 3])>,
    /// Received: changes to our Chao (by index).
    changes_in: Vec<(u8, Changes)>,
}

static GARDEN: Mutex<Option<Garden>> = Mutex::new(None);

fn with<R>(f: impl FnOnce(&mut Garden) -> R) -> R {
    let mut g = GARDEN.lock().unwrap_or_else(|e| e.into_inner());
    f(g.get_or_insert_with(|| Garden {
        mirrors: HashMap::new(),
        holders: HashMap::new(),
        mine: Vec::new(),
        held: HashMap::new(),
        wrapped: HashMap::new(),
        changes_out: Vec::new(),
        held_out: Vec::new(),
        changes_in: Vec::new(),
    }))
}

/// Visitors: we created `object` for player `owner`'s Chao `index`.
pub fn spawned(owner: u8, index: u8, object: usize) {
    if object != 0 {
        with(|g| {
            g.mirrors.insert(
                (owner, index),
                Mirror {
                    object,
                    target: None,
                    motion: None,
                    driven: false,
                    snapshot: None,
                    local_until: None,
                },
            )
        });
    }
}

/// Visitors: we left the garden and the game deleted the Chao objects.
pub fn clear() {
    with(|g| {
        g.mirrors.clear();
        g.held.clear();
        g.wrapped.clear();
        g.changes_in.clear();
    });
}

/// The Chao we brought to this garden: (`Body::Chao` index, save slot).
pub fn set_mine(slots: Vec<(u8, usize)>) {
    with(|g| g.mine = slots);
}

/// Network: a player's visiting Chao as they are in their game.
pub fn received(owner: u8, list: Vec<GardenChao>) {
    let now = Instant::now();
    with(|g| {
        for c in list {
            if let Some(m) = g.mirrors.get_mut(&(owner, c.index)) {
                m.target = Some((c, now));
            }
        }
    });
}

/// Network: a friend's interaction changed our Chao `index`.
pub fn received_changes(index: u8, changes: Changes) {
    with(|g| g.changes_in.push((index, changes)));
}

/// Network: friend `holder` is holding our Chao `index`.
pub fn received_held(holder: u8, index: u8, offset: [f32; 3]) {
    with(|g| {
        g.held.insert(
            index,
            Held {
                holder,
                offset,
                at: Instant::now(),
            },
        )
    });
}

/// Where other players are drawn this frame (their puppets), for Chao they hold.
pub fn set_holders(holders: HashMap<u8, (Vector, i32)>) {
    with(|g| g.holders = holders);
}

/// To send: (changes to friends' Chao: owner, index, changes; friends' Chao we hold: owner,
/// index, offset).
pub fn take_outgoing() -> Outgoing {
    with(|g| {
        (
            std::mem::take(&mut g.changes_out),
            std::mem::take(&mut g.held_out),
        )
    })
}

/// Where friend `owner` is drawn now (their puppet): (position, facing).
pub fn holder(owner: u8) -> Option<(Vector, i32)> {
    with(|g| g.holders.get(&owner).copied())
}

/// Our copy of friend `owner`'s Chao `index`, when it follows them (not being touched by us).
pub fn following_copy(owner: u8, index: u8) -> Option<usize> {
    with(|g| {
        g.mirrors
            .get(&(owner, index))
            .filter(|m| m.local_until.is_none())
            .map(|m| m.object)
    })
}

/// Our visiting Chao now in this garden: (index, registry entry, object).
pub fn my_chao_entries() -> Vec<(u8, usize, usize)> {
    let mine = with(|g| g.mine.clone());
    mine.into_iter()
        .filter_map(|(index, slot)| {
            let entry = live_entry(slot)?;
            // SAFETY: a live registry entry's object pointer.
            let object = unsafe { ((entry + OBJ_ENTRY) as *const usize).read() };
            Some((index, entry, object))
        })
        .collect()
}

/// Rotates (x, z) by a BAMS angle about Y; `-angle` undoes it.
pub fn rotate(v: [f32; 3], angle: i32) -> [f32; 3] {
    let a = angle as f32 * (std::f32::consts::TAU / 65536.0);
    let (s, c) = a.sin_cos();
    [v[0] * c + v[2] * s, v[1], -v[0] * s + v[2] * c]
}

fn record_of(slot: usize) -> usize {
    CHAO_SLOTS + slot * CHAO_SIZE
}

/// The registry entry of the Chao in save slot `slot`, if it's in this garden.
fn live_entry(slot: usize) -> Option<usize> {
    let record = record_of(slot);
    // SAFETY: the game's static Chao object table (category 0); entries are checked for null.
    unsafe {
        (0..WORLD_CHAO_MAX)
            .map(|i| WORLD + i * WORLD_ENTRY_SIZE)
            .find(|&e| ((e + 8) as *const usize).read() == record)
            .filter(|&e| ((e + OBJ_ENTRY) as *const usize).read() != 0)
    }
}

/// The live object of the Chao in save slot `slot`, if it's in this garden.
fn live_object(slot: usize) -> Option<usize> {
    // SAFETY: a live registry entry's object pointer.
    live_entry(slot).map(|e| unsafe { ((e + OBJ_ENTRY) as *const usize).read() })
}

/// # Safety
/// `object` is a live object.
unsafe fn data1(object: usize) -> usize {
    unsafe { ((object + OBJ_DATA1) as *const usize).read() }
}

/// The object the local player holds, or 0.
pub fn player_held() -> usize {
    // SAFETY: a static pointer to the player's CharObj2 (null without a player).
    unsafe {
        let c = (PLAYER_CHAR_OBJ2 as *const usize).read_volatile();
        if c == 0 {
            0
        } else {
            ((c + PLAYER_HELD) as *const usize).read()
        }
    }
}

/// The local player is within `distance` of `at`.
fn player_near(at: Vector, distance: f32) -> bool {
    game::player_position().is_some_and(|(p, _)| {
        let (dx, dy, dz) = (p.x - at.x, p.y - at.y, p.z - at.z);
        dx * dx + dy * dy + dz * dz < distance * distance
    })
}

/// The local player holds a fruit, animal, drive or hat within reach of `at`.
fn holding_item_near(at: Vector) -> bool {
    let held = player_held();
    if held == 0 {
        return false;
    }
    // SAFETY: the held object is live; its registry entry pointer is checked against the
    // registry's range before use.
    let entry = unsafe { ((held + OBJ_ENTRY) as *const usize).read() };
    let in_category = |c: usize| {
        let base = WORLD + c * WORLD_CATEGORY_SIZE;
        (base..base + WORLD_CATEGORY_SIZE).contains(&entry)
    };
    ITEM_CATEGORIES.into_iter().any(in_category) && player_near(at, ITEM_NEAR)
}

/// Our visiting Chao (`(index, save slot)`) as they are now, to send.
pub fn my_chao() -> Vec<GardenChao> {
    let slots = with(|g| g.mine.clone());
    let player = game::player_position();
    slots
        .iter()
        .filter_map(|&(index, slot)| {
            let object = live_object(slot)?;
            // SAFETY: a live Chao object's Data1, read on the game thread.
            unsafe {
                let d = data1(object);
                if d == 0 {
                    return None;
                }
                let p = ((d + D_POSITION) as *const Vector).read_unaligned();
                // Blending: the motion it's going to, unless there's none yet (-1).
                let motion_flags = ((d + D_MOTION) as *const u16).read();
                let current = ((d + D_MOTION_CURRENT) as *const i32).read();
                let next = ((d + D_MOTION_NEXT) as *const i32).read();
                let motion = if motion_flags & MOTION_BLENDING != 0 && next >= 0 {
                    next
                } else {
                    current
                };
                // Held by us (not shown in a friend's hands by us).
                let held = player_held() == object;
                let offset = match (held, player) {
                    (true, Some((at, angle))) => {
                        rotate([p.x - at.x, p.y - at.y, p.z - at.z], angle.wrapping_neg())
                    }
                    _ => [0.0; 3],
                };
                Some(GardenChao {
                    index,
                    position: [p.x, p.y, p.z],
                    rotation_y: ((d + D_ROTATION_Y) as *const i32).read(),
                    motion: motion as u16,
                    held: held && player.is_some(),
                    offset,
                    bubble: (
                        ((d + D_MOOD) as *const i16).read(),
                        ((d + D_MOOD_TIMER) as *const i32).read(),
                        ((d + D_MOOD_LENGTH) as *const i32).read(),
                    ),
                    eyes: ((d + D_EYES) as *const i16).read(),
                    mouth: ((d + D_MOUTH) as *const i16).read(),
                })
            }
        })
        .collect()
}

/// Call every frame (game thread).
pub fn on_frame() {
    let now = Instant::now();
    with(|g| {
        mirrors_frame(g, now);
        mine_frame(g);
    });
}

/// Friends' Chao: take over copies whose owners send their Chao; let touched ones run their
/// own AI and forward what it changes.
fn mirrors_frame(g: &mut Garden, now: Instant) {
    let player = game::player_position();
    for (&(owner, index), m) in &mut g.mirrors {
        // SAFETY: our visitor's live object (cleared when we leave the garden), on the game thread.
        unsafe {
            let d = data1(m.object);
            if d == 0 {
                continue;
            }
            let record = ((d + D_RECORD) as *const usize).read();
            if !m.driven {
                let main = (m.object + OBJ_MAIN) as *mut usize;
                if m.target.is_none() || main.read() != CHAO_MAIN || record == 0 {
                    continue;
                }
                main.write(mirror_main as extern "C" fn(usize) as usize);
                m.snapshot = Some(Box::new((record as *const ChaoData).read()));
                m.driven = true;
                log!("garden: player {owner}'s Chao {index} now follows their real one");
            }

            // Touched by us: held, hit, offered an item, or we're right next to it (petting).
            // Never while its owner holds it.
            let here = ((d + D_POSITION) as *const Vector).read_unaligned();
            let status = ((d + D_STATUS) as *const u16).read();
            let held_by_us = player_held() == m.object;
            let owner_holds = m
                .target
                .is_some_and(|(t, at)| t.held && at.elapsed() < STALE);
            let touched = !owner_holds
                && (held_by_us
                    || status & STATUS_HIT != 0
                    || holding_item_near(here)
                    || player_near(here, PLAYER_NEAR));

            // Touching starts it; an item right at it (being eaten or held) keeps it going, so
            // the game never has its AI switched off halfway through.
            let busy = crate::items::near(here, BUSY_NEAR);
            if touched || (m.local_until.is_some() && busy) {
                if m.local_until.is_none() {
                    log!(
                        "garden: touching player {owner}'s Chao {index} (status {status:#06x}, \
                         held {held_by_us})"
                    );
                }
                m.local_until = Some(now + LOCAL_TIME);
            }
            let local = m.local_until.is_some();
            if m.local_until.is_some_and(|until| now >= until) {
                m.local_until = None;
                m.motion = None;
                log!("garden: done touching player {owner}'s Chao {index}; it follows again");
            }

            // What our touching changed in the copy goes to the owner. Changes the game makes on
            // its own (moods drifting over time) happen in the owner's game too: not forwarded.
            if record != 0
                && let Some(before) = m.snapshot.as_mut()
            {
                let after = (record as *const ChaoData).read();
                if local {
                    let diff = chao::changes::diff(before, &after);
                    if !diff.is_empty() {
                        log!(
                            "garden: player {owner}'s Chao {index}: {} change(s) to send",
                            diff.len()
                        );
                        g.changes_out.push((owner, index, diff));
                    }
                }
                **before = after;
            }
            let flags = (d + D_FLAGS) as *mut u32;
            let ai = m.local_until.is_some();
            flags.write(if ai {
                flags.read() | FLAG_AI
            } else {
                flags.read() & !FLAG_AI
            });

            if held_by_us && let Some((at, angle)) = player {
                let offset = rotate(
                    [here.x - at.x, here.y - at.y, here.z - at.z],
                    angle.wrapping_neg(),
                );
                g.held_out.push((owner, index, offset));
            }
        }
    }
}

/// Our Chao: apply friends' changes, and show the ones friends hold in their hands.
fn mine_frame(g: &mut Garden) {
    let slot_of =
        |mine: &[(u8, usize)], index: u8| mine.iter().find(|(i, _)| *i == index).map(|&(_, s)| s);
    for (index, changes) in std::mem::take(&mut g.changes_in) {
        let Some(slot) = slot_of(&g.mine, index) else {
            continue;
        };
        let record = record_of(slot) as *mut ChaoData;
        // SAFETY: our own save slot in the game's Chao table, written on the game thread (the
        // live Chao object reads it in place).
        unsafe {
            let mut chao = record.read();
            chao::changes::apply(&mut chao, &changes);
            record.write(chao);
            if let Some(object) = live_object(slot) {
                let d = data1(object);
                if d != 0 {
                    let a = (d + D_APPEARANCE) as *mut u32;
                    a.write(a.read() | 2);
                }
            }
        }
        log!(
            "garden: applied {} change(s) from a friend to our Chao {index}",
            changes.len()
        );
    }

    g.held.retain(|_, h| h.at.elapsed() < HELD_STALE);
    for &(index, slot) in &g.mine {
        let Some(object) = live_object(slot) else {
            continue;
        };
        let held = g.held.contains_key(&index);
        // SAFETY: our own live Chao object; its MainSub is the garden one or our wrapper.
        unsafe {
            let main = (object + OBJ_MAIN) as *mut usize;
            if held && main.read() == CHAO_MAIN {
                main.write(held_main as extern "C" fn(usize) as usize);
                g.wrapped.insert(object, false);
                log!("garden: a friend picked up our Chao {index}");
            }
        }
    }
}

/// `0x793C40`: start motion `id` on the controller at `motion` (Data1+0xAC).
#[cfg(target_arch = "x86")]
unsafe fn start_motion(motion: usize, id: u16) {
    // SAFETY: usercall EAX = controller, EDX = id; clobbers ECX/EDX (re/chao-garden-sync.md).
    unsafe {
        std::arch::asm!(
            "call {func}",
            func = in(reg) START_MOTION,
            inout("eax") motion => _,
            inout("edx") u32::from(id) => _,
            clobber_abi("C"),
        );
    }
}

#[cfg(not(target_arch = "x86"))]
unsafe fn start_motion(_motion: usize, _id: u16) {}

/// `id` names a real entry of the motion table at `controller`: 0x793C40 doesn't check, and a
/// bad entry leaves a garbage "next motion" that crashes the Chao's drawing (0x56EB7F).
///
/// # Safety
/// `controller` is a live Chao's motion controller (Data1+0xAC).
unsafe fn motion_ok(controller: usize, id: u16) -> bool {
    if id >= MAX_MOTION_ID {
        return false;
    }
    unsafe {
        let table = ((controller + MC_TABLE) as *const usize).read();
        let current = ((controller + MC_MOTION) as *const usize).read();
        if table == 0 || current == 0 {
            return false;
        }
        let entry = table + usize::from(id) * MOTION_ENTRY_SIZE;
        let motion = (entry as *const usize).read();
        let mode = ((entry + 4) as *const u16).read();
        // Every Chao motion lives in the same module as the one it plays now.
        motion != 0 && mode <= 7 && motion.abs_diff(current) < 0x0100_0000
    }
}

/// `0x53A4B0`: eyes `id` until changed (ECX = obj, EAX = duration, EDX = id).
#[cfg(target_arch = "x86")]
unsafe fn set_eyes(object: usize, id: i16) {
    unsafe {
        std::arch::asm!(
            "call {func}",
            func = in(reg) SET_EYES,
            inout("ecx") object => _,
            inout("eax") -1i32 => _,
            inout("edx") i32::from(id) => _,
            clobber_abi("C"),
        );
    }
}

/// `0x53A5A0`: mouth `id` until changed (ECX = obj, EDX = duration, push id; caller pops).
#[cfg(target_arch = "x86")]
unsafe fn set_mouth(object: usize, id: i16) {
    unsafe {
        std::arch::asm!(
            "push {id}",
            "call {func}",
            "add esp, 4",
            id = in(reg) i32::from(id),
            func = in(reg) SET_MOUTH,
            inout("ecx") object => _,
            inout("edx") -1i32 => _,
            clobber_abi("C"),
        );
    }
}

#[cfg(not(target_arch = "x86"))]
unsafe fn set_eyes(_object: usize, _id: i16) {}
#[cfg(not(target_arch = "x86"))]
unsafe fn set_mouth(_object: usize, _id: i16) {}

/// Where a mirrored Chao should be this frame, or None to leave it to the game.
fn place(g: &mut Garden, object: usize, d: usize) -> Option<(Vector, i32, bool)> {
    let holders = &g.holders;
    let (&(owner, _), m) = g.mirrors.iter_mut().find(|(_, m)| m.object == object)?;
    if m.local_until.is_some() {
        return None;
    }
    let (t, at) = m.target?;
    if at.elapsed() > STALE {
        return None;
    }
    if m.motion != Some(t.motion) {
        m.motion = Some(t.motion);
        // SAFETY: the Chao's own motion controller; the id comes from the owner's game, which
        // uses the same shared motion table, and is checked against it first.
        unsafe {
            if motion_ok(d + D_MOTION, t.motion) {
                start_motion(d + D_MOTION, t.motion);
            } else {
                log!(
                    "garden: player {owner}'s Chao: motion {:#x} skipped (not in the table)",
                    t.motion
                );
            }
        }
    }
    // SAFETY: the copy's own Data1 and face functions, on the game thread.
    unsafe {
        // The mood shown over it (heart, "?", "!", ...): switch to the owner's current one the
        // way the game does, then let it play here.
        let (mood, timer, length) = t.bubble;
        if (0..MOOD_STATES).contains(&mood) && ((d + D_MOOD) as *const i16).read() != mood {
            ((d + D_MOOD) as *mut i16).write(mood);
            ((d + D_MOOD_REQUEST) as *mut i16).write(-1);
            ((d + D_MOOD_TIMER) as *mut i32).write(timer);
            ((d + D_MOOD_LENGTH) as *mut i32).write(length);
            for counter in MOOD_COUNTERS {
                ((d + D_MOOD + counter) as *mut u16).write(0);
            }
        }
        if (0..MAX_FACE_ID).contains(&t.eyes) && ((d + D_EYES) as *const i16).read() != t.eyes {
            set_eyes(object, t.eyes);
        }
        if (0..MAX_FACE_ID).contains(&t.mouth) && ((d + D_MOUTH) as *const i16).read() != t.mouth {
            set_mouth(object, t.mouth);
        }
    }
    if t.held
        && let Some(&(holder, angle)) = holders.get(&owner)
    {
        return Some((in_hands(holder, angle, t.offset), t.rotation_y, true));
    }
    // SAFETY: the visitor's Data1 (read on the game thread inside its MainSub).
    let here = unsafe { ((d + D_POSITION) as *const Vector).read_unaligned() };
    let [x, y, z] = t.position;
    Some((
        Vector {
            x: here.x + (x - here.x) * FOLLOW,
            y: here.y + (y - here.y) * FOLLOW,
            z: here.z + (z - here.z) * FOLLOW,
        },
        t.rotation_y,
        false,
    ))
}

pub fn in_hands(holder: Vector, angle: i32, offset: [f32; 3]) -> Vector {
    let o = rotate(offset, angle);
    Vector {
        x: holder.x + o[0],
        y: holder.y + o[1],
        z: holder.z + o[2],
    }
}

/// Writes the place, and the held bit (draw offset; can't be picked up while held).
///
/// # Safety
/// `d` is the Chao's Data1.
unsafe fn put(d: usize, (at, rotation_y, held): (Vector, i32, bool)) {
    unsafe {
        ((d + D_POSITION) as *mut Vector).write_unaligned(at);
        ((d + D_ROTATION_Y) as *mut i32).write(rotation_y);
        let status = (d + D_STATUS) as *mut u16;
        let s = status.read();
        status.write(if held {
            s | STATUS_HELD
        } else {
            s & !STATUS_HELD
        });
    }
}

/// Runs the game's Chao update for `object` between two `put`s of `spot` (when given), so
/// physics and gravity can't move it.
fn update_at(object: usize, d: usize, spot: Option<(Vector, i32, bool)>) {
    if let Some(spot) = spot {
        // SAFETY: the Chao's Data1, on the game thread.
        unsafe { put(d, spot) };
    }
    // SAFETY: the Chao's original MainSub, which this object had.
    unsafe { std::mem::transmute::<usize, extern "C" fn(usize)>(CHAO_MAIN)(object) };
    if let Some(spot) = spot {
        // SAFETY: as above; Data2's first three floats are the velocity.
        unsafe {
            put(d, spot);
            let d2 = ((object + OBJ_DATA2) as *const usize).read();
            if d2 != 0 {
                (d2 as *mut [f32; 3]).write_unaligned([0.0; 3]);
            }
        }
    }
}

/// MainSub of a friend's Chao copy: where the owner's real one is (unless we're touching it),
/// then the game's own update.
extern "C" fn mirror_main(object: usize) {
    // SAFETY: a visitor Chao we wrapped; its Data1 is checked for null.
    let d = unsafe { data1(object) };
    let spot = if d == 0 {
        None
    } else {
        std::panic::catch_unwind(|| {
            let mut g = GARDEN.lock().unwrap_or_else(|e| e.into_inner());
            g.as_mut().and_then(|g| place(g, object, d))
        })
        .ok()
        .flatten()
    };
    update_at(object, d, spot);
}

/// MainSub of our own Chao while a friend may hold it: in their (puppet's) hands with its AI
/// off; back to normal when they let go.
extern "C" fn held_main(object: usize) {
    // SAFETY: one of our Chao objects we wrapped; its Data1 is checked for null.
    let d = unsafe { data1(object) };
    let spot = if d == 0 {
        None
    } else {
        std::panic::catch_unwind(|| {
            let mut g = GARDEN.lock().unwrap_or_else(|e| e.into_inner());
            let g = g.as_mut()?;
            let index = g
                .mine
                .iter()
                .find(|&&(_, slot)| live_object(slot) == Some(object))
                .map(|&(i, _)| i)?;
            let flags = (d + D_FLAGS) as *mut u32;
            let ai_off = g.wrapped.entry(object).or_insert(false);
            match g.held.get(&index) {
                Some(h) => {
                    let (holder, angle) = *g.holders.get(&h.holder)?;
                    // SAFETY: our Chao's Data1 and motion controller, on the game thread.
                    unsafe {
                        if !*ai_off {
                            *ai_off = true;
                            flags.write(flags.read() & !FLAG_AI);
                            start_motion(d + D_MOTION, MOTION_HELD);
                        }
                        let rotation = ((d + D_ROTATION_Y) as *const i32).read();
                        Some((in_hands(holder, angle, h.offset), rotation, true))
                    }
                }
                None => {
                    if *ai_off {
                        *ai_off = false;
                        // SAFETY: as above; let go: drop it where it is with its AI back on.
                        unsafe {
                            flags.write(flags.read() | FLAG_AI);
                            let status = (d + D_STATUS) as *mut u16;
                            status.write(status.read() & !STATUS_HELD);
                        }
                        log!("garden: a friend put our Chao {index} down");
                    }
                    None
                }
            }
        })
        .ok()
        .flatten()
    };
    update_at(object, d, spot);
}

#[cfg(test)]
mod tests {
    use super::rotate;

    #[test]
    fn offset_round_trips_through_the_holders_frame() {
        let v = [3.0, 4.0, -7.5];
        for angle in [0i32, 0x4000, 0x8000, -0x2345, 0x1_2345] {
            let back = rotate(rotate(v, angle.wrapping_neg()), angle);
            for i in 0..3 {
                assert!((back[i] - v[i]).abs() < 1e-3, "{angle:#x}: {back:?}");
            }
        }
    }
}
