//! Shared garden items (re/chao-garden-items.md 3, 4; the host's referee is net/referee.rs).
//!
//! Every fruit, animal, Chaos Drive, seed and hat in a shared garden has one owner: the player
//! whose game has it for real (with a save record, in their save). Each game sends its own
//! items ~10 times a second: lying where, or held by them or by one of their Chao.
//!
//! The others show a **copy** of each: a real game item with no save record, so it is never
//! saved. A copy on the ground can be picked up, and Chao can take it. The moment we pick one
//! up (or one of the Chao here takes it), we claim it from the referee. When it's granted the
//! copy becomes ours (it gets a record in our save) and the old owner's game turns theirs into a
//! copy (freeing its record). When it's refused (someone else was first, or the owner was using
//! it), we let go of it and it goes back to being a copy.
//!
//! While its owner (or their Chao) holds it, the copy is only drawn in their hands: its game
//! logic doesn't run, so nobody here can take it.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use crate::game::{self, Vector};
use crate::garden;
use crate::log;
use crate::net::protocol::{GardenItem, ItemId, ItemPlace, MAX_GARDEN_ITEMS};

/// Chao-world object registry (shared with garden.rs): counts, then 0x1200 bytes per category
/// of 0x48-byte entries.
const WORLD_COUNTS: usize = 0x01DC_0F80;
const WORLD: usize = 0x01DC_0FC0;
const WORLD_ENTRY_SIZE: usize = 0x48;
const WORLD_CATEGORY_SIZE: usize = 0x1200;
const WORLD_CATEGORY_SLOTS: usize = 64;
/// Item categories: animals and drives, fruit, seeds, hats. 0 is Chao.
const CATEGORIES: [usize; 4] = [2, 3, 7, 9];
const CHAO_CATEGORY: usize = 0;
/// Entry: +0 u16 category, +6 u16 flags (2 = held by a Chao), +8 save record, +0x40 partner.
const ENTRY_FLAGS: usize = 0x06;
const ENTRY_HELD_BY_CHAO: u16 = 0x2;
#[cfg(target_arch = "x86")]
const ENTRY_RECORD: usize = 0x08;
const ENTRY_PARTNER: usize = 0x40;

const OBJ_MAIN: usize = 0x10;
const OBJ_DATA1: usize = 0x34;
const OBJ_ENTRY: usize = 0x3C;
const D_ROTATION_Y: usize = 0x0C;
const D_POSITION: usize = 0x14;
/// Fruit: Data1 byte 0 is its mode (4 = being eaten); +8 kind; +0x10 bites left.
const FRUIT: usize = 3;
const FRUIT_BEING_EATEN: u8 = 4;
const D_KIND: usize = 0x08;
const D_ANIMAL_KIND: usize = 0x03;
const D_BITES: usize = 0x10;

/// Item create functions (cdecl, return the ObjectMaster), all called without a save record.
const CREATE_FRUIT: usize = 0x0054_6180;
const CREATE_ANIMAL: usize = 0x0054_8D30;
const CREATE_DRIVE: usize = 0x0054_5600;
const CREATE_SEED: usize = 0x0054_9B00;
const CREATE_HAT: usize = 0x0054_A540;
const DELETE_OBJECT: usize = 0x0046_F720;
/// Animal/drive kinds from here on are Chaos Drives (colour + 0x15).
const DRIVE_KIND: u8 = 0x15;
/// Save record helpers: a free record of a category (ECX = category; NULL when full), and
/// freeing one (EAX = record).
#[cfg(target_arch = "x86")]
const FREE_RECORD: usize = 0x0052_F9E0;
#[cfg(target_arch = "x86")]
const RELEASE_RECORD: usize = 0x0052_F680;
/// Drop what the player holds (EAX = player index, EDX = player Data1).
#[cfg(target_arch = "x86")]
const DROP_HELD: usize = 0x0046_E5E0;

/// Send our items every this many frames (~10 per second).
const SEND_EVERY: u32 = 6;
/// A copy goes when its owner stops reporting it for this long.
const STALE: Duration = Duration::from_millis(1500);
/// Give up on a claim the referee never answered.
const CLAIM_TIMEOUT: Duration = Duration::from_secs(3);

/// A copy of someone else's item.
struct Copy {
    object: usize,
    owner: u8,
    item: GardenItem,
    seen: Instant,
    /// The item's own MainSub (our wrapper replaces it).
    main: usize,
    /// We claimed it (picked up / one of the Chao here took it), waiting for the referee.
    claimed: Option<Instant>,
}

struct Items {
    me: Option<u8>,
    /// The garden we're in (1..3), or 0.
    area: u8,
    serial: u16,
    frame: u32,
    /// What our player held last frame (to log pickups).
    held: usize,
    /// Our items: object -> id.
    mine: HashMap<usize, ItemId>,
    /// Copies of the others' items.
    copies: HashMap<ItemId, Copy>,
    /// Who the referee last said owns an item.
    owners: HashMap<ItemId, u8>,
    /// Latest reports from the others: id -> (reporter, area, item, when).
    reports: HashMap<ItemId, (u8, u8, GardenItem, Instant)>,
    claims_out: Vec<ItemId>,
    owner_in: Vec<(ItemId, u8)>,
}

static ITEMS: Mutex<Option<Items>> = Mutex::new(None);

/// The state, created on first use.
struct Guard(std::sync::MutexGuard<'static, Option<Items>>);

impl std::ops::Deref for Guard {
    type Target = Items;
    fn deref(&self) -> &Items {
        self.0.as_ref().expect("created in lock()")
    }
}

impl std::ops::DerefMut for Guard {
    fn deref_mut(&mut self) -> &mut Items {
        self.0.as_mut().expect("created in lock()")
    }
}

fn lock() -> Guard {
    let mut g = ITEMS.lock().unwrap_or_else(|e| e.into_inner());
    g.get_or_insert_with(|| Items {
        me: None,
        area: 0,
        serial: 0,
        frame: 0,
        held: 0,
        mine: HashMap::new(),
        copies: HashMap::new(),
        owners: HashMap::new(),
        reports: HashMap::new(),
        claims_out: Vec::new(),
        owner_in: Vec::new(),
    });
    Guard(g)
}

/// Network: items player `from` owns in garden `area`.
pub fn received(from: u8, area: u8, items: Vec<GardenItem>) {
    let now = Instant::now();
    let mut s = lock();
    for item in items {
        s.reports.insert(item.id, (from, area, item, now));
    }
}

/// Network (the referee): item `id` belongs to `owner`.
pub fn received_owner(id: ItemId, owner: u8) {
    lock().owner_in.push((id, owner));
}

/// Claims to send to the host.
pub fn take_claims() -> Vec<ItemId> {
    std::mem::take(&mut lock().claims_out)
}

// --- Reading the registry ---------------------------------------------------------------

/// Live items: (entry, object).
fn live_items() -> Vec<(usize, usize)> {
    let mut out = Vec::new();
    for cat in CATEGORIES {
        // SAFETY: the game's static registry; the count is clamped and objects are checked.
        unsafe {
            let count = ((WORLD_COUNTS + cat * 4) as *const u32).read() as usize;
            for i in 0..count.min(WORLD_CATEGORY_SLOTS) {
                let entry = WORLD + cat * WORLD_CATEGORY_SIZE + i * WORLD_ENTRY_SIZE;
                let object = ((entry + OBJ_ENTRY) as *const usize).read();
                if object != 0 && data1(object) != 0 {
                    out.push((entry, object));
                }
            }
        }
    }
    out
}

/// # Safety
/// `object` is live.
unsafe fn data1(object: usize) -> usize {
    unsafe { ((object + OBJ_DATA1) as *const usize).read() }
}

fn position(object: usize) -> Vector {
    // SAFETY: a live object with Data1 (callers check).
    unsafe { ((data1(object) + D_POSITION) as *const Vector).read_unaligned() }
}

fn facing(object: usize) -> i32 {
    // SAFETY: as above.
    unsafe { ((data1(object) + D_ROTATION_Y) as *const i32).read() }
}

/// The registry entry of a live item: found in the registry (the entry pointing at it), not
/// through the object's own back-pointer, which some items (Chaos Drives) don't set.
fn entry_of(object: usize) -> Option<usize> {
    live_items()
        .into_iter()
        .find(|&(_, o)| o == object)
        .map(|(e, _)| e)
}

/// Animals' display (re/chao-garden-items.md); it reads `+0x3C` unchecked.
const ANIMAL_DISPLAY: usize = 0x0054_89D0;
/// Object list 2 holds the animals and drives (list heads at 0x1A5A254).
const OBJECT_LIST_2: usize = 0x01A5_A254 + 2 * 4;

/// An animal drawn with no item-list entry (`+0x3C` empty) crashes the game's animal display
/// (0x548BB2, twice on 2026-10-17). The normal game never has one; a mirrored animal that was
/// eaten or absorbed while we skipped its update can. Those stop being drawn.
fn hide_detached_animals() {
    if game::current_level() != game::LEVEL_CHAO_WORLD {
        return;
    }
    // SAFETY: the game's object list 2 (circular through +4), on the game thread, before the
    // display pass.
    unsafe {
        let head = (OBJECT_LIST_2 as *const usize).read();
        let mut object = head;
        for _ in 0..1024 {
            if object == 0 {
                break;
            }
            let display = (object + 0x14) as *mut usize;
            if display.read() == ANIMAL_DISPLAY
                && ((object + OBJ_ENTRY) as *const usize).read() == 0
            {
                display.write(0);
                crate::log!("items: an animal lost its item entry; not drawing it");
            }
            object = ((object + 0x04) as *const usize).read();
            if object == head {
                break;
            }
        }
    }
}

/// Animals' MainSub (re/chao-garden-items.md); their update reads `+0x3C` unchecked.
const ANIMAL_MAIN: usize = 0x0054_8C50;

/// The item is still in the garden's item list (an animal also still has its `+0x3C`; drives
/// never do). A Chao eating or absorbing it takes it out, and running the item's update after
/// that crashes (0x548BB2 in the animal update, 2026-10-17). `main` is its original MainSub.
fn attached(object: usize, main: usize) -> bool {
    if entry_of(object).is_none() {
        return false;
    }
    // SAFETY: a live object we track.
    main != ANIMAL_MAIN || unsafe { ((object + OBJ_ENTRY) as *const usize).read() != 0 }
}

fn category_of(entry: usize) -> usize {
    (entry.wrapping_sub(WORLD)) / WORLD_CATEGORY_SIZE
}

/// Some garden item is within `distance` of `at`.
pub fn near(at: Vector, distance: f32) -> bool {
    live_items().into_iter().any(|(_, o)| {
        let p = position(o);
        let (dx, dy, dz) = (p.x - at.x, p.y - at.y, p.z - at.z);
        dx * dx + dy * dy + dz * dz < distance * distance
    })
}

/// (category, kind, bites) of an item.
fn kind_of(entry: usize, object: usize) -> (u8, u8, u8) {
    let category = category_of(entry);
    // SAFETY: a live item's Data1.
    unsafe {
        let d = data1(object);
        match category {
            FRUIT => (
                3,
                ((d + D_KIND) as *const i32).read() as u8,
                ((d + D_BITES) as *const i32).read().clamp(0, 4) as u8,
            ),
            2 => (2, ((d + D_ANIMAL_KIND) as *const u8).read(), 0),
            c => (c as u8, ((d + D_KIND) as *const i32).read() as u8, 0),
        }
    }
}

/// The Chao entry holding or eating this item, if any.
fn chao_with(entry: usize, object: usize) -> Option<usize> {
    // SAFETY: a live registry entry and its object's Data1.
    unsafe {
        let partner = ((entry + ENTRY_PARTNER) as *const usize).read();
        if partner == 0 || category_of(partner) != CHAO_CATEGORY {
            return None;
        }
        let held = ((entry + ENTRY_FLAGS) as *const u16).read() & ENTRY_HELD_BY_CHAO != 0;
        let eaten =
            category_of(entry) == FRUIT && (data1(object) as *const u8).read() == FRUIT_BEING_EATEN;
        (held || eaten).then_some(partner)
    }
}

/// Being used here: held by our player, or a Chao here holds or eats it.
fn used_here(entry: usize, object: usize) -> bool {
    garden::player_held() == object || chao_with(entry, object).is_some()
}

// --- Game calls -------------------------------------------------------------------------

/// Creates a copy of `item` at `at` (no save record), or 0.
///
/// # Safety
/// Game thread, in a garden.
unsafe fn create(item: &GardenItem, at: Vector) -> usize {
    let pos = std::ptr::from_ref(&at);
    let still = Vector {
        x: 0.0,
        y: 0.0,
        z: 0.0,
    };
    let vel = std::ptr::from_ref(&still);
    let kind = i32::from(item.kind);
    type WithAngle = extern "C" fn(i32, *const Vector, i32, *const Vector, usize) -> usize;
    type NoAngle = extern "C" fn(i32, *const Vector, *const Vector, usize) -> usize;
    unsafe {
        let object = match item.category {
            3 => std::mem::transmute::<usize, WithAngle>(CREATE_FRUIT)(kind, pos, 0, vel, 0),
            2 if item.kind >= DRIVE_KIND => std::mem::transmute::<usize, NoAngle>(CREATE_DRIVE)(
                i32::from((item.kind - DRIVE_KIND).min(3)),
                pos,
                vel,
                0,
            ),
            2 => {
                std::mem::transmute::<usize, WithAngle>(CREATE_ANIMAL)(kind.min(20), pos, 0, vel, 0)
            }
            7 => std::mem::transmute::<usize, NoAngle>(CREATE_SEED)(kind, pos, vel, 0),
            9 => std::mem::transmute::<usize, WithAngle>(CREATE_HAT)(kind, pos, 0, vel, 0),
            _ => 0,
        };
        if object != 0 && item.category == 3 && data1(object) != 0 {
            ((data1(object) + D_BITES) as *mut i32).write(i32::from(item.bites));
        }
        object
    }
}

/// Gives our new item a save record (in our save), if a free one exists.
///
/// # Safety
/// Game thread; `entry`/`object` are a live item without a record.
#[cfg(target_arch = "x86")]
unsafe fn adopt(entry: usize, object: usize, area: u8) {
    let category = category_of(entry) as u32;
    let record: usize;
    unsafe {
        std::arch::asm!(
            "call {func}",
            func = in(reg) FREE_RECORD,
            inout("ecx") category => _,
            out("eax") record,
            clobber_abi("C"),
        );
        if record == 0 {
            log!("items: no free save record for category {category}; this one won't be saved");
            return;
        }
        let (_, kind, bites) = kind_of(entry, object);
        let p = position(object);
        // ItemSave: i16 kind, i16 garden, i16 param (fruit: bites), i16 age, f32 pos[3].
        (record as *mut i16).write(i16::from(kind));
        ((record + 2) as *mut i16).write(i16::from(area));
        ((record + 4) as *mut i16).write(i16::from(bites));
        ((record + 6) as *mut i16).write(0);
        ((record + 8) as *mut Vector).write_unaligned(p);
        ((entry + ENTRY_RECORD) as *mut usize).write(record);
    }
}

/// Takes our item's save record away (it's leaving our save).
///
/// # Safety
/// Game thread; `entry` is a live item.
#[cfg(target_arch = "x86")]
unsafe fn release(entry: usize) {
    unsafe {
        let record = ((entry + ENTRY_RECORD) as *const usize).read();
        if record != 0 {
            std::arch::asm!(
                "call {func}",
                func = in(reg) RELEASE_RECORD,
                inout("eax") record => _,
                clobber_abi("C"),
            );
            ((entry + ENTRY_RECORD) as *mut usize).write(0);
        }
    }
}

/// Our player drops what they hold.
///
/// # Safety
/// Game thread.
#[cfg(target_arch = "x86")]
unsafe fn drop_held() {
    let player = game::local_entity() as usize;
    if player == 0 {
        return;
    }
    unsafe {
        std::arch::asm!(
            "call {func}",
            func = in(reg) DROP_HELD,
            inout("eax") 0u32 => _,
            inout("edx") player => _,
            clobber_abi("C"),
        );
    }
}

#[cfg(not(target_arch = "x86"))]
unsafe fn adopt(_entry: usize, _object: usize, _area: u8) {}
#[cfg(not(target_arch = "x86"))]
unsafe fn release(_entry: usize) {}
#[cfg(not(target_arch = "x86"))]
unsafe fn drop_held() {}

fn wrap(object: usize) -> usize {
    // SAFETY: a live item we own the wrapper for; returns its original MainSub.
    unsafe {
        let main = (object + OBJ_MAIN) as *mut usize;
        let original = main.read();
        main.write(copy_main as extern "C" fn(usize) as usize);
        original
    }
}

fn delete(object: usize) {
    // SAFETY: a live item; the game deletes it on its next pass.
    unsafe { ((object + OBJ_MAIN) as *mut usize).write(DELETE_OBJECT) };
}

// --- Each frame -------------------------------------------------------------------------

/// Where a reported item belongs here, and whether it's in someone's hands.
fn place(owner: u8, item: &GardenItem) -> Option<(Vector, bool)> {
    let [x, y, z] = item.position;
    match item.place {
        ItemPlace::Ground | ItemPlace::Busy => Some((Vector { x, y, z }, item.busy())),
        ItemPlace::Player => {
            let (at, angle) = garden::holder(owner)?;
            Some((garden::in_hands(at, angle, item.position), true))
        }
        ItemPlace::Chao(n) => {
            let chao = garden::following_copy(owner, n)?;
            Some((
                garden::in_hands(position(chao), facing(chao), item.position),
                true,
            ))
        }
    }
}

/// Call every frame with our player id and the garden we're in (1..3, or 0).
pub fn on_frame(me: Option<u8>, area: u8) -> Vec<(u8, Vec<GardenItem>)> {
    hide_detached_animals();
    let mut s = lock();
    s.me = me;
    if area == 0 || me.is_none() {
        if s.area != 0 {
            // Left the garden: our copies go (the game doesn't delete them by itself; they'd
            // follow us to the next area). Our own items are the game's to keep or carry.
            for c in s.copies.values() {
                delete(c.object);
            }
            s.mine.clear();
            s.copies.clear();
            s.reports.clear();
            s.area = 0;
        }
        return Vec::new();
    }
    let me = me.unwrap_or(0);
    s.area = area;
    let now = Instant::now();
    let live = live_items();
    let alive = |o: usize| live.iter().any(|&(_, x)| x == o);
    s.mine.retain(|&o, _| alive(o));
    s.copies.retain(|_, c| alive(c.object));

    apply_owners(&mut s, me, area);

    let held = garden::player_held();
    if held != s.held {
        s.held = held;
        if held != 0 {
            let what = if let Some(id) = s.mine.get(&held) {
                format!("our item {}:{}", id.origin, id.serial)
            } else if let Some((id, c)) = s.copies.iter().find(|(_, c)| c.object == held) {
                format!(
                    "player {}'s item {}:{} (a copy)",
                    c.owner, id.origin, id.serial
                )
            } else {
                "something not shared".into()
            };
            let kind = entry_of(held).map(|e| kind_of(e, held));
            log!("items: picked up {what}, category/kind/bites {kind:?}");
        }
    }

    // Copies: follow their owners' reports, claim the ones we use, drop the stale ones.
    s.reports.retain(|_, (_, _, _, at)| at.elapsed() < STALE);
    let reports: Vec<(ItemId, u8, GardenItem)> = s
        .reports
        .iter()
        .filter(|(id, (from, a, _, _))| {
            *a == area && s.owners.get(id).is_none_or(|o| o == from) && *from != me
        })
        .map(|(id, (from, _, item, _))| (*id, *from, *item))
        .collect();
    for (id, owner, item) in &reports {
        if let Some(c) = s.copies.get_mut(id) {
            c.item = *item;
            c.owner = *owner;
            c.seen = now;
            continue;
        }
        if s.mine.values().any(|m| m == id) {
            continue;
        }
        let Some((at, _)) = place(*owner, item) else {
            continue;
        };
        // SAFETY: game thread, in a garden.
        let object = unsafe { create(item, at) };
        if object == 0 {
            continue;
        }
        let main = wrap(object);
        s.copies.insert(
            *id,
            Copy {
                object,
                owner: *owner,
                item: *item,
                seen: now,
                main,
                claimed: None,
            },
        );
    }
    let mut claims = Vec::new();
    let mut gone = Vec::new();
    for (id, c) in &mut s.copies {
        let used = entry_of(c.object).is_some_and(|entry| used_here(entry, c.object));
        if used && c.claimed.is_none() {
            c.claimed = Some(now);
            claims.push(*id);
            log!(
                "items: claiming player {}'s item {}:{}",
                c.owner,
                id.origin,
                id.serial
            );
        }
        if c.claimed.is_some_and(|at| at.elapsed() > CLAIM_TIMEOUT) {
            c.claimed = None;
        }
        if !used && c.claimed.is_none() && c.seen.elapsed() > STALE {
            gone.push(*id);
        }
    }
    for id in gone {
        if let Some(c) = s.copies.remove(&id) {
            delete(c.object);
        }
    }
    s.claims_out.extend(claims);

    // Our items, to send.
    s.frame = s.frame.wrapping_add(1);
    if !s.frame.is_multiple_of(SEND_EVERY) {
        return Vec::new();
    }
    let chao: Vec<(u8, usize, usize)> = garden::my_chao_entries();
    let player = game::player_position();
    let copies: Vec<usize> = s.copies.values().map(|c| c.object).collect();
    let mut out = Vec::new();
    for &(entry, object) in &live {
        if copies.contains(&object) {
            continue;
        }
        let next = s.serial;
        let id = *s.mine.entry(object).or_insert_with(|| ItemId {
            origin: me,
            serial: next,
        });
        if id.origin == me && id.serial == next {
            s.serial = s.serial.wrapping_add(1);
        }
        let (category, kind, bites) = kind_of(entry, object);
        let p = position(object);
        let (place, position) = if garden::player_held() == object
            && let Some((at, angle)) = player
        {
            (
                ItemPlace::Player,
                garden::rotate([p.x - at.x, p.y - at.y, p.z - at.z], angle.wrapping_neg()),
            )
        } else if let Some(partner) = chao_with(entry, object) {
            match chao.iter().find(|(_, e, _)| *e == partner) {
                Some(&(index, _, chao)) => {
                    let c = position(chao);
                    (
                        ItemPlace::Chao(index),
                        garden::rotate(
                            [p.x - c.x, p.y - c.y, p.z - c.z],
                            facing(chao).wrapping_neg(),
                        ),
                    )
                }
                None => (ItemPlace::Busy, [p.x, p.y, p.z]),
            }
        } else {
            (ItemPlace::Ground, [p.x, p.y, p.z])
        };
        out.push(GardenItem {
            id,
            category,
            kind,
            bites,
            place,
            position,
        });
    }
    out.chunks(MAX_GARDEN_ITEMS)
        .map(|c| (area, c.to_vec()))
        .collect()
}

/// The referee's answers: items changing hands.
fn apply_owners(s: &mut Items, me: u8, area: u8) {
    for (id, owner) in std::mem::take(&mut s.owner_in) {
        s.owners.insert(id, owner);
        if owner == me {
            // Our claim won: the copy becomes ours, saved in our save.
            if let Some(c) = s.copies.remove(&id) {
                // SAFETY: the copy's live object, checked to still be our wrapped copy in the
                // item list before its original MainSub goes back.
                unsafe {
                    let main = (c.object + OBJ_MAIN) as *mut usize;
                    if main.read() != copy_main as extern "C" fn(usize) as usize
                        || !attached(c.object, c.main)
                    {
                        log!(
                            "items: item {}:{} was used up before it became ours",
                            id.origin,
                            id.serial
                        );
                        continue;
                    }
                    main.write(c.main);
                    if let Some(entry) = entry_of(c.object) {
                        adopt(entry, c.object, area);
                    }
                }
                s.mine.insert(c.object, id);
                log!("items: item {}:{} is ours now", id.origin, id.serial);
            }
        } else if let Some((&object, _)) = s.mine.iter().find(|(_, m)| **m == id) {
            // Someone else's now: it leaves our save and becomes a copy.
            s.mine.remove(&object);
            let Some(entry) = entry_of(object) else {
                continue;
            };
            // SAFETY: our live item; game thread.
            unsafe {
                if garden::player_held() == object {
                    drop_held();
                }
                release(entry);
            }
            let (category, kind, bites) = kind_of(entry, object);
            let p = position(object);
            let main = wrap(object);
            s.copies.insert(
                id,
                Copy {
                    object,
                    owner,
                    item: GardenItem {
                        id,
                        category,
                        kind,
                        bites,
                        place: ItemPlace::Ground,
                        position: [p.x, p.y, p.z],
                    },
                    seen: Instant::now(),
                    main,
                    claimed: None,
                },
            );
            log!(
                "items: item {}:{} went to player {owner}",
                id.origin,
                id.serial
            );
        } else if let Some(c) = s.copies.get_mut(&id)
            && c.claimed.is_some()
        {
            // Refused: someone else has it. Let go of it; it stays a copy.
            c.claimed = None;
            c.owner = owner;
            if garden::player_held() == c.object {
                // SAFETY: game thread.
                unsafe { drop_held() };
            }
            log!(
                "items: item {}:{} stays with player {owner}",
                id.origin,
                id.serial
            );
        }
    }
}

/// MainSub of a copy: its own game logic while it's on the ground or used here (so it can be
/// picked up and eaten), placed where its owner has it; only drawn while they hold it.
extern "C" fn copy_main(object: usize) {
    let step = std::panic::catch_unwind(|| {
        let s = lock();
        let c = s.copies.values().find(|c| c.object == object)?;
        let local =
            c.claimed.is_some() || entry_of(object).is_some_and(|entry| used_here(entry, object));
        let spot = if local { None } else { place(c.owner, &c.item) };
        Some((c.main, local, spot, c.item))
    })
    .ok()
    .flatten();
    let Some((main, local, spot, item)) = step else {
        // A copy we no longer know (we left the garden): it goes.
        delete(object);
        return;
    };
    if !attached(object, main) {
        // Eaten or absorbed here: its update would crash; the copy just goes.
        delete(object);
        return;
    }
    let in_hands = spot.is_some_and(|(_, held)| held);
    if local || !in_hands {
        // SAFETY: the item's own MainSub, which this object had.
        unsafe { std::mem::transmute::<usize, extern "C" fn(usize)>(main)(object) };
    }
    if let Some((at, _)) = spot {
        // SAFETY: the copy's Data1, on the game thread.
        unsafe {
            let d = data1(object);
            if d != 0 {
                ((d + D_POSITION) as *mut Vector).write_unaligned(at);
                if item.category == 3 {
                    ((d + D_BITES) as *mut i32).write(i32::from(item.bites));
                }
            }
        }
    }
}
