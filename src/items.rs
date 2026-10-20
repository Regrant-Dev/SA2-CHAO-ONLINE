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

/// Chao-world object registry: registry.rs.
/// Item categories: animals and drives, fruit, seeds, hats. 0 is Chao.
const CATEGORIES: [usize; 4] = [2, 3, 7, 9];
const CHAO_CATEGORY: usize = 0;
/// Entry: +0 u16 category, +6 u16 flags (2 = held by a Chao), +8 save record, +0x40 partner.
const ENTRY_FLAGS: usize = 0x06;
const ENTRY_HELD_BY_CHAO: u16 = 0x2;
const ENTRY_RECORD: usize = 0x08;
const ENTRY_PARTNER: usize = 0x40;

const OBJ_MAIN: usize = 0x10;
const OBJ_DATA1: usize = 0x34;
const OBJ_ENTRY: usize = 0x3C;
const D_POSITION: usize = 0x14;
/// Fruit: Data1 byte 0 is its mode (4 = being eaten); +8 kind; +0x10 bites left.
const FRUIT: usize = 3;
const FRUIT_BEING_EATEN: u8 = 4;
const D_KIND: usize = 0x08;
const D_ANIMAL_KIND: usize = 0x03;
const D_BITES: usize = 0x10;
/// A fruit's ALW entry radius (`+0x28`) is bites left × 0.5 (2.0 from 0x546180, then set by
/// the eat mode 0x545C20 after each bite); its height (`+0x2C`) is 1.0 (0x546180). The Chao
/// holds it that far out and that much lower (0x56CFF0).
const FRUIT_HOLD_RADIUS_PER_BITE: f32 = 0.5;
const FRUIT_HOLD_HEIGHT: f32 = 1.0;

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
    /// How it moves by its owner's reports (animals walk with it).
    walk: Walk,
}

/// An item's latest reported position, when it came and how fast it went since the one
/// before (per second).
#[derive(Clone, Copy, Debug)]
struct Walk {
    at: [f32; 3],
    when: Instant,
    velocity: [f32; 3],
}

impl Walk {
    fn new(at: [f32; 3], now: Instant) -> Self {
        Self {
            at,
            when: now,
            velocity: [0.0; 3],
        }
    }

    /// A report at `at` came `now` (only when it moved: the same report is read every frame).
    fn report(&mut self, at: [f32; 3], now: Instant) {
        let dt = now.duration_since(self.when).as_secs_f32();
        self.velocity = if (0.01..=STILL_AFTER.as_secs_f32()).contains(&dt) {
            std::array::from_fn(|i| (at[i] - self.at[i]) / dt)
        } else {
            [0.0; 3]
        };
        self.at = at;
        self.when = now;
    }

    /// Its speed now: none once its reports stopped moving.
    fn velocity(&self, now: Instant) -> [f32; 3] {
        if now.duration_since(self.when) > STILL_AFTER {
            [0.0; 3]
        } else {
            self.velocity
        }
    }

    /// Where it is by `now`: the latest report moved on by its speed (a report's worth at most).
    fn predicted(&self, now: Instant) -> [f32; 3] {
        let ahead = now
            .duration_since(self.when)
            .min(PREDICT_AT_MOST)
            .as_secs_f32();
        let v = self.velocity(now);
        std::array::from_fn(|i| self.at[i] + v[i] * ahead)
    }

    /// Whether it walks: faster than `WALKING` across the ground.
    fn walking(&self, now: Instant) -> bool {
        let [x, _, z] = self.velocity(now);
        x * x + z * z > WALKING * WALKING
    }
}

/// A report this much after the last that moved means it stood still meanwhile.
const STILL_AFTER: Duration = Duration::from_millis(300);
/// A copy is shown at most this far ahead of its last report.
const PREDICT_AT_MOST: Duration = Duration::from_millis(120);
/// Animals walk at up to 0.15 a frame (9 a second: `0x165DFD8`, state 2 of 0x57BD40); slower
/// than this is standing.
const WALKING: f32 = 1.5;
/// A copy goes this much of the way to where its owner has it each frame; further than
/// `SNAP` it jumps there.
const FOLLOW: f32 = 0.3;
const SNAP: f32 = 10.0;

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
    /// What a Chao the server runs did to our items (`ItemSteer`): item as it is now, gone.
    steer_in: Vec<(GardenItem, bool)>,
    /// Our items a server Chao has: as the server says, and the item's own MainSub (ours
    /// stands in while it's in that Chao's hands).
    steered: HashMap<ItemId, (GardenItem, usize)>,
    /// Our items in no one's save that the mod made so (fruit from the server's trees): only
    /// these are reported unsaved. Ones the game itself made without a record (an egg's shell
    /// with the hat list full) aren't the server's to keep.
    unsaved: Vec<usize>,
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
        steer_in: Vec::new(),
        steered: HashMap::new(),
        unsaved: Vec::new(),
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

/// Our new item `object` was made in no one's save on purpose (trees.rs: the server's trees'
/// fruit): it's reported unsaved, and stays in the garden when we leave it.
pub fn mark_unsaved(object: usize) {
    let mut s = lock();
    if !s.unsaved.contains(&object) {
        s.unsaved.push(object);
    }
}

/// The server, as an items' owner: its items come from the referee (`ServerItems`), not from a
/// player (re/chao-garden-items.md 7).
pub use crate::net::protocol::SERVER;

/// Network (the referee): the server's own items in garden `area`.
pub fn received_server(area: u8, items: Vec<GardenItem>) {
    received(SERVER, area, items);
}

/// Network (the referee): item `id` belongs to `owner`.
pub fn received_owner(id: ItemId, owner: u8) {
    lock().owner_in.push((id, owner));
}

/// Network (the server): a Chao it runs borrowed our item; it's now as `item` says (in that
/// Chao's hands, or put down), or `gone` (eaten, worn). It stays ours, in our save.
pub fn received_steer(item: GardenItem, gone: bool) {
    lock().steer_in.push((item, gone));
}

/// Claims to send to the host.
pub fn take_claims() -> Vec<ItemId> {
    std::mem::take(&mut lock().claims_out)
}

// --- Reading the registry ---------------------------------------------------------------

/// Live items: (entry, object). Every entry (registry.rs: the count can miss some), found by
/// the entry's object pointer, never the object's own back-pointer (Chaos Drives don't set it).
fn live_items() -> Vec<(usize, usize)> {
    CATEGORIES
        .iter()
        .flat_map(|&cat| crate::registry::entries(cat))
        // SAFETY: a registry entry's live object.
        .filter(|&(_, object)| unsafe { data1(object) } != 0)
        .collect()
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
    crate::registry::category_of(entry)
}

/// This Chao is actually holding or eating an item, by the item's registry partner.
pub fn chao_busy(chao: usize) -> bool {
    let Some((chao_entry, _)) = crate::registry::entries(CHAO_CATEGORY)
        .into_iter()
        .find(|&(_, object)| object == chao)
    else {
        return false;
    };
    live_items()
        .into_iter()
        .any(|(entry, object)| chao_with(entry, object) == Some(chao_entry))
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

/// Being used here: held by our player, or a Chao here holds or eats it (not a copy locked
/// against us: it lets go at once, and we don't claim what it touched).
fn used_here(entry: usize, object: usize) -> bool {
    garden::player_held() == object
        || chao_with(entry, object).is_some_and(|chao| !garden::keeps_off_items(chao))
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
        ItemPlace::Chao {
            owner: chao_owner,
            index,
        } => {
            // A fruit goes where the game itself keeps it: at the copy's animated hands, so it
            // follows the motion (eating) rather than a fixed spot (re/chao-garden-items.md).
            if usize::from(item.category) == FRUIT
                && let Some(at) = garden::hold_point(
                    chao_owner,
                    index,
                    f32::from(item.bites) * FRUIT_HOLD_RADIUS_PER_BITE,
                    FRUIT_HOLD_HEIGHT,
                )
            {
                return Some((at, true));
            }
            let (at, angle) = garden::following_pose(chao_owner, index)?;
            Some((garden::in_hands(at, angle, item.position), true))
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
            unsteer_all(&mut s);
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
    s.unsaved.retain(|&o| alive(o));

    apply_owners(&mut s, me, area);
    apply_steers(&mut s);

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
            if item.place != c.item.place {
                c.walk = Walk::new(item.position, now);
            } else if item.position != c.item.position {
                c.walk.report(item.position, now);
            }
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
                walk: Walk::new(item.position, now),
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
    let chao: Vec<(u8, u8, usize, usize)> = garden::run_here();
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
        let (place, position) = if let Some((steered, _)) = s.steered.get(&id) {
            // A server Chao has it: where the server says.
            (steered.place, steered.position)
        } else if garden::player_held() == object
            && let Some((at, angle)) = player
        {
            (
                ItemPlace::Player,
                garden::rotate([p.x - at.x, p.y - at.y, p.z - at.z], angle.wrapping_neg()),
            )
        } else if let Some(partner) = chao_with(entry, object) {
            match chao.iter().find_map(|&(owner, index, entry, object)| {
                (entry == partner)
                    .then(|| garden::live_pose(object).map(|pose| (owner, index, pose)))
                    .flatten()
            }) {
                Some((owner, index, (c, angle))) => (
                    ItemPlace::Chao { owner, index },
                    garden::rotate([p.x - c.x, p.y - c.y, p.z - c.z], angle.wrapping_neg()),
                ),
                None => (ItemPlace::Busy, [p.x, p.y, p.z]),
            }
        } else {
            (ItemPlace::Ground, [p.x, p.y, p.z])
        };
        // SAFETY: a live registry entry: its save record (0 for none).
        let mut unsaved = s.unsaved.contains(&object)
            && unsafe { ((entry + ENTRY_RECORD) as *const usize).read() } == 0;
        if unsaved && used_here(entry, object) {
            // One in no one's save (fruit from the server's trees) that we or our Chao now use
            // is ours: in our save from now on.
            // SAFETY: our live item without a record, on the game thread.
            unsafe { adopt(entry, object, area) };
            // SAFETY: as above.
            unsaved = unsafe { ((entry + ENTRY_RECORD) as *const usize).read() } == 0;
            if !unsaved {
                s.unsaved.retain(|&o| o != object);
            }
        }
        out.push(GardenItem {
            id,
            category,
            kind,
            bites,
            place,
            position,
            unsaved,
        });
    }
    out.chunks(MAX_GARDEN_ITEMS)
        .map(|c| (area, c.to_vec()))
        .collect()
}

/// What a server Chao did to our items: ours follow (in its hands, put down, or gone).
fn apply_steers(s: &mut Items) {
    for (item, gone) in std::mem::take(&mut s.steer_in) {
        let Some((&object, _)) = s.mine.iter().find(|(_, m)| **m == item.id) else {
            continue;
        };
        if gone {
            // Eaten up or worn: out of our save, as when one of our Chao does it.
            if let Some(entry) = entry_of(object) {
                // SAFETY: our live item; game thread.
                unsafe { release(entry) };
            }
            unsteer(s, item.id, object);
            delete(object);
            s.mine.remove(&object);
            log!(
                "items: our item {}:{} was used up by a Chao",
                item.id.origin,
                item.id.serial
            );
            continue;
        }
        // SAFETY: our live item's Data1, on the game thread.
        unsafe {
            let d = data1(object);
            if d != 0 && item.category == FRUIT as u8 {
                ((d + D_BITES) as *mut i32).write(i32::from(item.bites));
            }
        }
        if item.place == ItemPlace::Ground {
            unsteer(s, item.id, object);
            let [x, y, z] = item.position;
            // SAFETY: as above.
            unsafe {
                let d = data1(object);
                if d != 0 {
                    ((d + D_POSITION) as *mut Vector).write_unaligned(Vector { x, y, z });
                }
            }
            continue;
        }
        let main = match s.steered.get(&item.id) {
            Some(&(_, main)) => main,
            None => {
                // SAFETY: our live item; its MainSub swapped for ours while a Chao has it.
                unsafe {
                    let at = (object + OBJ_MAIN) as *mut usize;
                    let main = at.read();
                    at.write(steered_main as extern "C" fn(usize) as usize);
                    main
                }
            }
        };
        s.steered.insert(item.id, (item, main));
    }
}

/// Our item `id` (`object`) is ours to run again: its own MainSub back.
fn unsteer(s: &mut Items, id: ItemId, object: usize) {
    if let Some((_, main)) = s.steered.remove(&id) {
        // SAFETY: our live item, still running our stand-in.
        unsafe {
            let at = (object + OBJ_MAIN) as *mut usize;
            if at.read() == steered_main as extern "C" fn(usize) as usize {
                at.write(main);
            }
        }
    }
}

/// Leaving the garden: our items in server Chao's hands lie where they are, ours again.
fn unsteer_all(s: &mut Items) {
    let ids: Vec<(ItemId, usize)> = s
        .mine
        .iter()
        .filter(|(_, id)| s.steered.contains_key(id))
        .map(|(&object, &id)| (id, object))
        .collect();
    for (id, object) in ids {
        unsteer(s, id, object);
    }
    s.steered.clear();
}

/// MainSub of our item while a Chao the server runs has it: in that Chao's hands (as a copy is
/// shown), its own logic off.
extern "C" fn steered_main(object: usize) {
    let spot = std::panic::catch_unwind(|| {
        let s = lock();
        let id = s.mine.get(&object)?;
        let (item, _) = s.steered.get(id)?;
        place(SERVER, item)
    })
    .ok()
    .flatten();
    if let Some((at, _)) = spot {
        // SAFETY: our live item's Data1, on the game thread.
        unsafe {
            let d = data1(object);
            if d != 0 {
                ((d + D_POSITION) as *mut Vector).write_unaligned(at);
            }
        }
    }
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
                        unsaved: false,
                    },
                    seen: Instant::now(),
                    main,
                    claimed: None,
                    walk: Walk::new([p.x, p.y, p.z], Instant::now()),
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
            // A Chao here has it (eating it, putting it on): it would use up what isn't ours, so
            // the copy goes now; the owner's next report makes it again.
            if entry_of(c.object).is_some_and(|entry| chao_with(entry, c.object).is_some()) {
                delete(c.object);
                s.copies.remove(&id);
                log!(
                    "items: item {}:{} was in a Chao's hands here; its copy goes",
                    id.origin,
                    id.serial
                );
                continue;
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
/// picked up and eaten), placed where its owner has it; only drawn while they hold it. An
/// animal on the ground walks there: its own wandering is held still (`hold_still`) and it
/// faces and walks (or stands) the way its owner's moves (`walk_with`).
extern "C" fn copy_main(object: usize) {
    let step = std::panic::catch_unwind(|| {
        let s = lock();
        let c = s.copies.values().find(|c| c.object == object)?;
        let local =
            c.claimed.is_some() || entry_of(object).is_some_and(|entry| used_here(entry, object));
        let spot = if local { None } else { place(c.owner, &c.item) };
        Some((c.main, local, spot, c.item, c.walk))
    })
    .ok()
    .flatten();
    let Some((main, local, spot, item, walk)) = step else {
        // A copy we no longer know (we left the garden): it goes.
        delete(object);
        return;
    };
    if !attached(object, main) {
        // Eaten or absorbed here: its update would crash; the copy just goes.
        delete(object);
        return;
    }
    let walks = !local
        && usize::from(item.category) == ANIMAL
        && item.kind < DRIVE_KIND
        && item.place == ItemPlace::Ground;
    // SAFETY: the copy's Data1 (checked for null), on the game thread.
    let d = unsafe { data1(object) };
    if walks && d != 0 {
        // SAFETY: as above; an animal's Data1.
        unsafe { hold_still(d) };
    }
    let in_hands = spot.is_some_and(|(_, held)| held);
    if local || !in_hands {
        // SAFETY: the item's own MainSub, which this object had.
        unsafe { std::mem::transmute::<usize, extern "C" fn(usize)>(main)(object) };
    }
    // SAFETY: the object stays readable until the game's next object pass.
    let deleted = unsafe { ((object + OBJ_MAIN) as *const usize).read() } == DELETE_OBJECT;
    // SAFETY: as above.
    if walks && d != 0 && !deleted && unsafe { data1(object) } == d {
        // SAFETY: still the live animal with this Data1 (its update didn't delete it).
        unsafe { walk_with(d, &walk, Instant::now()) };
        return;
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

/// Animal category (animals and drives).
const ANIMAL: usize = 2;
/// Animal Data1 (0x57BD40, the wandering every animal uses): `+1` i8 its state, `+6` u16 that
/// state's timer, `+0xC` facing (BAMS; it walks along (sin, cos)), `+0x30` f32 speed, `+0x38`
/// its motion controller.
const D_ANIMAL_STATE: usize = 0x01;
const D_ANIMAL_TIMER: usize = 0x06;
const D_ANIMAL_FACING: usize = 0x0C;
const D_ANIMAL_SPEED: usize = 0x30;
const D_ANIMAL_MOTION: usize = 0x38;
/// States that wander on the ground: 1 decide, 2 walk, 3 slow down, 4 wait (timer), 8/9 turn.
/// 0 sets the animal up; 5..7 are the water ones (falling in, swimming).
const WANDER_STATES: [i8; 6] = [1, 2, 3, 4, 8, 9];
const WAIT_STATE: i8 = 4;
const WAIT_FRAMES: u16 = 2;
/// Its motions: 0 stand, 1 walk (0x57BD40 starts only these on the ground).
const MOTION_STAND: u32 = 0;
const MOTION_WALK: u32 = 1;
/// Motion controller: `+0` u16 flags (2 = blending), `+4` current, `+8` next. 0x793D30 blends
/// into a motion: EAX = controller, EDX = motion, one stack argument (frames, caller pops).
const MOTION_FLAGS: usize = 0x0;
const MOTION_CURRENT: usize = 0x4;
const MOTION_NEXT: usize = 0x8;
const MOTION_BLENDING: u16 = 0x2;
#[cfg(target_arch = "x86")]
const BLEND_MOTION: usize = 0x0079_3D30;

/// Keeps an animal copy from wandering by itself: it waits (state 4) and doesn't move. The
/// wait is a few frames, renewed every frame, so one that becomes ours wanders again at once.
///
/// # Safety
/// `d` is a live animal's Data1, on the game thread.
unsafe fn hold_still(d: usize) {
    unsafe {
        let state = (d + D_ANIMAL_STATE) as *mut i8;
        if WANDER_STATES.contains(&state.read()) {
            state.write(WAIT_STATE);
            ((d + D_ANIMAL_TIMER) as *mut u16).write(WAIT_FRAMES);
            ((d + D_ANIMAL_SPEED) as *mut f32).write(0.0);
        }
    }
}

/// Puts an animal copy where its owner's is by now, facing the way it goes, walking while it
/// moves and standing while it doesn't.
///
/// # Safety
/// `d` is a live animal's Data1, on the game thread.
unsafe fn walk_with(d: usize, walk: &Walk, now: Instant) {
    unsafe {
        let p = (d + D_POSITION) as *mut Vector;
        let here = p.read_unaligned();
        let [x, y, z] = walk.predicted(now);
        let (dx, dy, dz) = (x - here.x, y - here.y, z - here.z);
        let amount = if dx * dx + dy * dy + dz * dz > SNAP * SNAP {
            1.0
        } else {
            FOLLOW
        };
        p.write_unaligned(Vector {
            x: here.x + dx * amount,
            y: here.y + dy * amount,
            z: here.z + dz * amount,
        });
        let walking = walk.walking(now);
        if walking {
            let [vx, _, vz] = walk.velocity(now);
            ((d + D_ANIMAL_FACING) as *mut i32).write(facing(vx, vz));
        }
        // Only on the ground states it wanders in: in the water its own motions play.
        if !WANDER_STATES.contains(&((d + D_ANIMAL_STATE) as *const i8).read()) {
            return;
        }
        let motion = d + D_ANIMAL_MOTION;
        let want = if walking { MOTION_WALK } else { MOTION_STAND };
        let blending = ((motion + MOTION_FLAGS) as *const u16).read() & MOTION_BLENDING != 0;
        let (current, next) = (
            ((motion + MOTION_CURRENT) as *const u32).read(),
            ((motion + MOTION_NEXT) as *const u32).read(),
        );
        if if blending {
            next != want
        } else {
            current != want
        } {
            blend_motion(motion, want);
        }
    }
}

/// BAMS facing of a walk along (`x`, `z`): the animal moves along (sin, cos) of it.
fn facing(x: f32, z: f32) -> i32 {
    (x.atan2(z) * 65536.0 / std::f32::consts::TAU).round() as i32
}

/// `0x793D30`: blend the controller at `motion` into motion `id` over 15 frames (as the
/// animals' own wandering does).
///
/// # Safety
/// `motion` is a live animal's motion controller and `id` one of its motions, on the game
/// thread.
#[cfg(target_arch = "x86")]
unsafe fn blend_motion(motion: usize, id: u32) {
    // SAFETY: the game's usercall (EAX controller, EDX motion, one cdecl argument); it writes
    // only the controller and keeps ESI.
    unsafe {
        std::arch::asm!(
            "push 15",
            "call {f}",
            "add esp, 4",
            f = in(reg) BLEND_MOTION,
            inout("eax") motion => _,
            inout("edx") id => _,
            clobber_abi("C"),
        );
    }
}

#[cfg(not(target_arch = "x86"))]
unsafe fn blend_motion(_motion: usize, _id: u32) {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_animal_walks_the_way_its_reports_go_and_stands_when_they_stop() {
        let start = Instant::now();
        let mut walk = Walk::new([0.0, 0.0, 0.0], start);
        assert!(!walk.walking(start));
        let t = start + Duration::from_millis(100);
        walk.report([0.5, 0.0, 0.0], t);
        assert!(walk.walking(t));
        assert!((walk.velocity(t)[0] - 5.0).abs() < 1e-3);
        // Shown a little ahead, never more than a report's worth.
        let later = t + Duration::from_millis(50);
        assert!((walk.predicted(later)[0] - 0.75).abs() < 1e-3);
        let much_later = t + Duration::from_millis(250);
        assert!((walk.predicted(much_later)[0] - 1.1).abs() < 1e-3);
        // No new position for a while: it stands.
        let still = t + STILL_AFTER + Duration::from_millis(1);
        assert!(!walk.walking(still));
        assert_eq!(walk.predicted(still), [0.5, 0.0, 0.0]);
        // A report after a long gap isn't a walk.
        walk.report([3.0, 0.0, 0.0], t + Duration::from_secs(2));
        assert_eq!(walk.velocity(t + Duration::from_secs(2)), [0.0; 3]);
    }

    #[test]
    fn facing_points_along_the_walk() {
        assert_eq!(facing(0.0, 1.0), 0);
        assert_eq!(facing(1.0, 0.0), 0x4000);
        assert_eq!(facing(0.0, -1.0), 0x8000);
        assert_eq!(facing(-1.0, 0.0), -0x4000);
    }
}
