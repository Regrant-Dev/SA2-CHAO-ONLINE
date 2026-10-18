//! Shared garden trees (re/chao-garden-sync.md "Trees").
//!
//! A garden has 7 tree spots; each is a GrowTree object made at garden load from an 8-byte save
//! record (kind, state, growth, life, fruit ripeness x3, facing), which it writes back every
//! frame. In a shared garden the garden's keeper (keeper.rs) keeps the trees. The
//! keeper's trees are real (in their save); they send their 7 records about once a second.
//!
//! Everyone else's game swaps its own trees for copies of the keeper's: GrowTree objects made by
//! the game's own constructor from records the mod owns, so nothing of them reaches their save,
//! and their own trees come back (from their save, untouched) when they keep the trees again.
//! A copy is remade when the keeper's tree changes (planted, grown, fruit dropped).
//!
//! What a player does to a copy goes to the keeper, whose game does it for real:
//! - Planting: a seed plants itself by marking the empty spot's registry entry (+0x34 = 5,
//!   +0x36 = seed kind), and the tree takes it on its next frame. The keeper's game is marked the
//!   same way.
//! - Shaking: each frame a tree is shaken (+0xD0) counts down its ripe fruits' timers, and a
//!   fruit drops when its timer runs out. The keeper's tree counts down the same, and drops the
//!   fruit as the game does (a new saved fruit, shared as an item). Copies never drop fruit.

use std::sync::Mutex;

use crate::game;
use crate::log;
use crate::net::protocol::{Body, TREE_PLANT, TREE_RECORD, TREE_SHAKE, TREE_SLOTS};

type Record = [u8; TREE_RECORD];

/// Tree spot positions: 7 vectors (0xC bytes) per garden (0x78), from garden 1.
const TREE_POSITIONS: usize = 0x0136_6B30;
const GARDEN_POSITIONS: usize = 0x78;
const POSITION_SIZE: usize = 0xC;
/// The save block (cdecl, no arguments); a garden's tree records at +0x20 + (garden - 1) * 0x38.
#[cfg(target_arch = "x86")]
const SAVE_BLOCK: usize = 0x0052_E440;
const SAVE_TREES: usize = 0x20;
const SAVE_GARDEN: usize = 0x38;
/// GrowTree (cdecl (position*, record*) -> object), its MainSub, and the object-deleting
/// MainSub (the object goes on its next run, through its delete sub, which leaves the record).
#[cfg(target_arch = "x86")]
const CREATE_TREE: usize = 0x0054_8210;
const TREE_MAIN: usize = 0x0054_6810;
const FREE_OBJECT: usize = 0x0046_F720;
/// The fruit drop's position (cdecl (out*, data1, slot)), a free save record (ECX =
/// category), and a fruit (cdecl (kind, position*, angle, velocity*, record)).
#[cfg(target_arch = "x86")]
const FRUIT_POSITION: usize = 0x0054_6670;
#[cfg(target_arch = "x86")]
const FREE_RECORD: usize = 0x0052_F9E0;
#[cfg(target_arch = "x86")]
const CREATE_FRUIT: usize = 0x0054_6180;
#[cfg(target_arch = "x86")]
const FRUIT_CATEGORY: u32 = 3;

/// Chao-world registry: counts per category, then 0x1200 bytes of 0x48-byte entries per
/// category. Trees are in categories 4 (grown) and 5 (growing).
const WORLD_COUNTS: usize = 0x01DC_0F80;
const WORLD: usize = 0x01DC_0FC0;
const WORLD_ENTRY_SIZE: usize = 0x48;
const WORLD_CATEGORY_SIZE: usize = 0x1200;
const WORLD_CATEGORY_SLOTS: usize = 64;
const TREE_CATEGORIES: [usize; 2] = [4, 5];
const ENTRY_OBJECT: usize = 0x3C;
/// A seed planting itself sets these on the spot's entry.
const ENTRY_TOUCH: usize = 0x34;
const ENTRY_SEED: usize = 0x36;
const TOUCH_SEED: u16 = 5;

const OBJ_MAIN: usize = 0x10;
const OBJ_DATA1: usize = 0x34;
const OBJ_ENTRY: usize = 0x3C;
/// GrowTree Data1: mode (0 empty spot, 7 grown with fruit), kind, state, position, fruit slots
/// (3 x 0x20: ripeness f32, its target f32, drop timer i32), shaken this frame, garden, record.
const D_MODE: usize = 0x00;
#[cfg(target_arch = "x86")]
const D_KIND: usize = 0x30;
#[cfg(target_arch = "x86")]
const D_STATE: usize = 0x31;
#[cfg(target_arch = "x86")]
const D_POSITION: usize = 0x14;
const D_FRUIT: usize = 0x44;
const FRUIT_SIZE: usize = 0x20;
const FRUITS: usize = 3;
const D_SHAKEN: usize = 0xD0;
#[cfg(target_arch = "x86")]
const D_GARDEN: usize = 0xE8;
const D_RECORD: usize = 0xEC;
const MODE_EMPTY: u8 = 0;
const MODE_GROWN: u8 = 7;
/// The garden's own tree (its fruit is the garden's kind).
#[cfg(target_arch = "x86")]
const STATE_GARDEN_TREE: u8 = 5;
/// A fruit is ripe above this; a copy's fruit stays below the game's own drop (1.9) and its
/// timers never run out.
const RIPE: f32 = 0.9;
const COPY_RIPENESS_MAX: f32 = 1.85;
const COPY_TIMER: i32 = 100;

/// The keeper sends their trees this often (frames).
const SEND_EVERY: u32 = 60;
/// After asking the keeper to plant, a copy isn't remade from older news this long (frames).
const PLANT_QUIET: u32 = 180;

/// Records the copies are made from (the game keeps pointers into them while they live, and
/// writes them every frame).
static mut COPY_RECORDS: [Record; TREE_SLOTS] = [[0; TREE_RECORD]; TREE_SLOTS];

struct Trees {
    area: u8,
    frame: u32,
    /// The copies: object per spot, and the record each was made from.
    copies: [usize; TREE_SLOTS],
    made_from: [Record; TREE_SLOTS],
    /// Frame until which each copy keeps its own state (a plant was just asked for).
    quiet_until: [u32; TREE_SLOTS],
    /// The latest trees per garden from each keeper: (sender, area, records).
    latest: Vec<(u8, u8, [Record; TREE_SLOTS])>,
    /// What the others asked our trees to do: (area, slot, action, kind).
    asked: Vec<(u8, u8, u8, u8)>,
    /// What players did to our copies, to send: (slot, action, kind).
    done: Vec<(u8, u8, u8)>,
}

static TREES: Mutex<Trees> = Mutex::new(Trees {
    area: 0,
    frame: 0,
    copies: [0; TREE_SLOTS],
    made_from: [[0; TREE_RECORD]; TREE_SLOTS],
    quiet_until: [0; TREE_SLOTS],
    latest: Vec::new(),
    asked: Vec::new(),
    done: Vec::new(),
});

fn lock() -> std::sync::MutexGuard<'static, Trees> {
    TREES.lock().unwrap_or_else(|e| e.into_inner())
}

/// Network: player `from` keeps garden `area`'s trees, these.
pub fn received_trees(from: u8, area: u8, trees: [Record; TREE_SLOTS]) {
    let mut s = lock();
    s.latest.retain(|&(f, a, _)| (f, a) != (from, area));
    s.latest.push((from, area, trees));
}

/// Network: someone planted or shook tree spot `slot` of garden `area` in their copy.
pub fn received_action(area: u8, slot: u8, action: u8, kind: u8) {
    if usize::from(slot) < TREE_SLOTS {
        lock().asked.push((area, slot, action, kind));
    }
}

/// Whether a copy made from `made` should be remade for the keeper's `now`: planted, grown or
/// dying (kind, state), grown on a good way, or a fruit dropped (its ripeness fell).
pub fn changed(made: &Record, now: &Record) -> bool {
    made[0] != now[0]
        || made[1] != now[1]
        || made[2].abs_diff(now[2]) > 15
        || (4..7).any(|i| now[i].saturating_add(20) < made[i])
}

/// Call every frame (game thread): us and our garden (1..=3, or 0). The garden's keeper
/// (keeper.rs) keeps the trees. Returns what to send.
pub fn on_frame(me: Option<u8>, area: u8) -> Vec<Body> {
    let mut out = Vec::new();
    let mut s = lock();
    s.frame = s.frame.wrapping_add(1);
    let (Some(me), true) = (me, area != 0 && game::in_garden()) else {
        // Out of the garden: its objects (copies and all) went with it.
        if s.area != 0 {
            s.copies = [0; TREE_SLOTS];
            s.area = 0;
            s.latest.clear();
            s.asked.clear();
            s.done.clear();
        }
        return out;
    };
    if s.area != area {
        s.copies = [0; TREE_SLOTS];
        s.area = area;
        s.asked.clear();
        s.done.clear();
    }
    // Not before the garden's trees are all there (it makes them as it loads).
    if trees_alive().len() < TREE_SLOTS {
        return out;
    }
    // Copies that went (the garden reloaded) are forgotten.
    for slot in 0..TREE_SLOTS {
        let object = s.copies[slot];
        // SAFETY: an object we made; checked to still run our MainSub.
        if object != 0
            && unsafe { ((object + OBJ_MAIN) as *const usize).read() }
                != copy_main as *const () as usize
        {
            s.copies[slot] = 0;
        }
    }
    // Nobody keeps the garden yet (friends just met): as they are for now.
    let Some(keeper) = crate::keeper::current() else {
        return out;
    };
    if keeper == me {
        // Ours are the real ones: back from copies if we had them.
        if s.copies.iter().any(|&o| o != 0) {
            for slot in 0..TREE_SLOTS {
                let object = std::mem::take(&mut s.copies[slot]);
                if object != 0 {
                    // SAFETY: a live copy of ours; it goes on its next run.
                    unsafe { ((object + OBJ_MAIN) as *mut usize).write(FREE_OBJECT) };
                }
                if real_tree(area, slot).is_none() {
                    make_real(area, slot);
                }
            }
            log!("trees: keeping garden {area}'s trees");
        }
        let asked: Vec<_> = std::mem::take(&mut s.asked);
        for (a, slot, action, kind) in asked {
            if a == area {
                act(area, usize::from(slot), action, kind);
            }
        }
        if s.frame.is_multiple_of(SEND_EVERY)
            && let Some(trees) = saved_trees(area)
        {
            out.push(Body::GardenTrees { area, trees });
        }
        return out;
    }
    s.asked.clear();
    // The keeper's trees: copies of them in place of ours.
    let Some(trees) = s
        .latest
        .iter()
        .find(|&&(f, a, _)| f == keeper && a == area)
        .map(|&(_, _, t)| t)
    else {
        return out;
    };
    let frame = s.frame;
    for (slot, record) in trees.iter().enumerate() {
        let current = s.copies[slot];
        if current != 0 && (frame < s.quiet_until[slot] || !changed(&s.made_from[slot], record)) {
            continue;
        }
        if current != 0 {
            // SAFETY: a live copy of ours; it goes on its next run.
            unsafe { ((current + OBJ_MAIN) as *mut usize).write(FREE_OBJECT) };
        } else if let Some(real) = real_tree(area, slot) {
            // SAFETY: our own tree; it goes on its next run, its save record untouched.
            unsafe { ((real + OBJ_MAIN) as *mut usize).write(FREE_OBJECT) };
        }
        s.copies[slot] = make_copy(area, slot, record);
        s.made_from[slot] = *record;
    }
    for (slot, action, kind) in std::mem::take(&mut s.done) {
        if action == TREE_PLANT {
            s.quiet_until[usize::from(slot)] = frame + PLANT_QUIET;
        }
        out.push(Body::TreeAction {
            area,
            slot,
            action,
            kind,
        });
    }
    out
}

/// Our garden's tree records, as our save has them.
fn saved_trees(area: u8) -> Option<[Record; TREE_SLOTS]> {
    let first = save_record(area, 0)?;
    let mut trees = [[0; TREE_RECORD]; TREE_SLOTS];
    for (slot, t) in trees.iter_mut().enumerate() {
        // SAFETY: the save block's tree records for this garden (7 x 8 bytes).
        *t = unsafe { ((first + slot * TREE_RECORD) as *const Record).read() };
    }
    Some(trees)
}

/// Our save's record for tree spot `slot` of garden `area`.
fn save_record(area: u8, slot: usize) -> Option<usize> {
    let block = save_block()?;
    Some(block + SAVE_TREES + usize::from(area - 1) * SAVE_GARDEN + slot * TREE_RECORD)
}

#[cfg(target_arch = "x86")]
fn save_block() -> Option<usize> {
    // SAFETY: the game's own getter, no arguments.
    let block = unsafe { std::mem::transmute::<usize, extern "C" fn() -> usize>(SAVE_BLOCK)() };
    (block != 0).then_some(block)
}

#[cfg(not(target_arch = "x86"))]
fn save_block() -> Option<usize> {
    None
}

fn spot(area: u8, slot: usize) -> usize {
    TREE_POSITIONS + usize::from(area - 1) * GARDEN_POSITIONS + slot * POSITION_SIZE
}

/// Our real tree object at spot `slot` (its record is our save's).
fn real_tree(area: u8, slot: usize) -> Option<usize> {
    let record = save_record(area, slot)?;
    trees_alive()
        .into_iter()
        .find(|&(object, data1)| {
            // SAFETY: a live tree's Data1 (0x100 bytes).
            let r = unsafe { ((data1 + D_RECORD) as *const usize).read() };
            r == record && object != 0
        })
        .map(|(object, _)| object)
}

/// Live tree objects (with their Data1), ours and copies.
fn trees_alive() -> Vec<(usize, usize)> {
    let mut out = Vec::new();
    for cat in TREE_CATEGORIES {
        // SAFETY: the game's static registry; the count is clamped and objects are checked.
        unsafe {
            let count = ((WORLD_COUNTS + cat * 4) as *const u32).read() as usize;
            for i in 0..count.min(WORLD_CATEGORY_SLOTS) {
                let entry = WORLD + cat * WORLD_CATEGORY_SIZE + i * WORLD_ENTRY_SIZE;
                let object = ((entry + ENTRY_OBJECT) as *const usize).read();
                if object == 0 {
                    continue;
                }
                let main = ((object + OBJ_MAIN) as *const usize).read();
                let data1 = ((object + OBJ_DATA1) as *const usize).read();
                if data1 != 0 && (main == TREE_MAIN || main == copy_main as *const () as usize) {
                    out.push((object, data1));
                }
            }
        }
    }
    out
}

/// Makes our real tree at spot `slot` again, from our save, as the garden's load does.
fn make_real(area: u8, slot: usize) {
    if let Some(record) = save_record(area, slot) {
        // SAFETY: game thread, in this garden; the spot's position and our save's record.
        unsafe { create_tree(spot(area, slot), record) };
    }
}

/// A copy of the keeper's tree `record` at spot `slot` (0 if it couldn't be made).
fn make_copy(area: u8, slot: usize, record: &Record) -> usize {
    // SAFETY: our own records, only touched on the game thread.
    let at = unsafe {
        let records = &raw mut COPY_RECORDS;
        (*records)[slot] = *record;
        (&raw mut (*records)[slot]) as usize
    };
    // SAFETY: game thread, in this garden; a record that outlives the copy (static).
    let object = unsafe { create_tree(spot(area, slot), at) };
    if object != 0 {
        // SAFETY: the object just made: our MainSub instead of its own (which we call).
        unsafe { ((object + OBJ_MAIN) as *mut usize).write(copy_main as *const () as usize) };
    }
    object
}

#[cfg(target_arch = "x86")]
unsafe fn create_tree(position: usize, record: usize) -> usize {
    // SAFETY: as the caller promises.
    unsafe {
        std::mem::transmute::<usize, extern "C" fn(usize, usize) -> usize>(CREATE_TREE)(
            position, record,
        )
    }
}

#[cfg(not(target_arch = "x86"))]
unsafe fn create_tree(_position: usize, _record: usize) -> usize {
    0
}

/// A copy's MainSub: the tree's own, but its fruit never drops here; planting in it and shaking
/// it are noted for the keeper.
extern "C" fn copy_main(object: usize) {
    // SAFETY: the copy is a live GrowTree (Data1 0x100 bytes, its registry entry).
    unsafe {
        let data1 = ((object + OBJ_DATA1) as *const usize).read();
        let entry = ((object + OBJ_ENTRY) as *const usize).read();
        if data1 == 0 {
            return;
        }
        let slot = lock().copies.iter().position(|&o| o == object);
        if let Some(slot) = slot
            && entry != 0
            && ((data1 + D_MODE) as *const u8).read() == MODE_EMPTY
            && ((entry + ENTRY_TOUCH) as *const u16).read() == TOUCH_SEED
        {
            let kind = ((entry + ENTRY_SEED) as *const u16).read() as u8;
            lock().done.push((slot as u8, TREE_PLANT, kind));
        }
        for i in 0..FRUITS {
            let fruit = data1 + D_FRUIT + i * FRUIT_SIZE;
            let ripeness = (fruit as *mut f32).read();
            if ripeness > COPY_RIPENESS_MAX {
                (fruit as *mut f32).write(COPY_RIPENESS_MAX);
            }
            ((fruit + 8) as *mut i32).write(COPY_TIMER);
        }
        std::mem::transmute::<usize, extern "C" fn(usize)>(TREE_MAIN)(object);
        if let Some(slot) = slot
            && ((data1 + D_SHAKEN) as *const u32).read() != 0
        {
            lock().done.push((slot as u8, TREE_SHAKE, 0));
        }
    }
}

/// Does what another player did to their copy of our tree at spot `slot`.
fn act(area: u8, slot: usize, action: u8, kind: u8) {
    let Some(object) = real_tree(area, slot) else {
        return;
    };
    // SAFETY: our live real tree (Data1 0x100 bytes, its registry entry).
    unsafe {
        let data1 = ((object + OBJ_DATA1) as *const usize).read();
        let entry = ((object + OBJ_ENTRY) as *const usize).read();
        match action {
            TREE_PLANT if entry != 0 && ((data1 + D_MODE) as *const u8).read() == MODE_EMPTY => {
                // As a seed planting itself does: the tree takes it on its next frame.
                ((entry + ENTRY_SEED) as *mut u16).write(u16::from(kind));
                ((entry + ENTRY_TOUCH) as *mut u16).write(TOUCH_SEED);
                log!("trees: planted kind {kind} at spot {slot} for a friend");
            }
            TREE_SHAKE if ((data1 + D_MODE) as *const u8).read() == MODE_GROWN => {
                shake(object, data1);
            }
            _ => {}
        }
    }
}

/// One frame of shaking our tree, as its own MainSub does it: each ripe fruit's timer counts
/// down, and drops when it runs out.
///
/// # Safety
/// Game thread; `object` is our live grown tree, `data1` its Data1.
unsafe fn shake(object: usize, data1: usize) {
    for i in 0..FRUITS {
        let fruit = data1 + D_FRUIT + i * FRUIT_SIZE;
        // SAFETY: as the caller promises.
        unsafe {
            if (fruit as *const f32).read() <= RIPE {
                continue;
            }
            let timer = ((fruit + 8) as *const i32).read() - 1;
            if timer >= 1 {
                ((fruit + 8) as *mut i32).write(timer);
                continue;
            }
            drop_fruit(object, data1, i);
            // As the game resets a dropped fruit's slot: not ripe, a new timer.
            let unripe = -random() * 0.3;
            (fruit as *mut f32).write(unripe);
            ((fruit + 4) as *mut f32).write(unripe);
            ((fruit + 8) as *mut i32).write(11 + (random() * 13.0) as i32);
        }
    }
}

/// Drops fruit `i` of our tree as the game does: a new saved fruit, its kind from the tree.
///
/// # Safety
/// As `shake`.
#[cfg(target_arch = "x86")]
unsafe fn drop_fruit(_object: usize, data1: usize, i: usize) {
    unsafe {
        // Where the fruit hangs (the tree's own: under it if its model isn't there).
        let mut at = ((data1 + D_POSITION) as *const [f32; 3]).read_unaligned();
        at[1] += 8.0;
        let mut out = [at[0], at[1], at[2], 0.0];
        std::mem::transmute::<usize, extern "C" fn(*mut f32, usize, usize)>(FRUIT_POSITION)(
            out.as_mut_ptr(),
            data1,
            i,
        );
        let record: usize;
        std::arch::asm!(
            "call {func}",
            func = in(reg) FREE_RECORD,
            inout("ecx") FRUIT_CATEGORY => _,
            out("eax") record,
            clobber_abi("C"),
        );
        let kind = if ((data1 + D_STATE) as *const u8).read() == STATE_GARDEN_TREE {
            ((data1 + D_GARDEN) as *const i32).read().clamp(1, 3) - 1
        } else {
            i32::from(((data1 + D_KIND) as *const u8).read()) + 2
        };
        let side = if random() < 0.5 { -1.0 } else { 1.0 };
        let velocity = [
            side * (0.03 + (0.5 - random()) * 0.01),
            0.0,
            side * (0.03 + (0.5 - random()) * 0.01),
        ];
        let angle = (random() * 65536.0) as i32;
        type CreateFruit = extern "C" fn(i32, *const f32, i32, *const f32, usize) -> usize;
        std::mem::transmute::<usize, CreateFruit>(CREATE_FRUIT)(
            kind,
            out.as_ptr(),
            angle,
            velocity.as_ptr(),
            record,
        );
    }
}

#[cfg(not(target_arch = "x86"))]
unsafe fn drop_fruit(_object: usize, _data1: usize, _i: usize) {}

/// 0..1, for the fruit's drop (no need for the game's own generator).
fn random() -> f32 {
    use std::sync::atomic::{AtomicU32, Ordering};
    static STATE: AtomicU32 = AtomicU32::new(0x2545_F491);
    let mut x = STATE.load(Ordering::Relaxed);
    x ^= x << 13;
    x ^= x >> 17;
    x ^= x << 5;
    STATE.store(x, Ordering::Relaxed);
    (x >> 8) as f32 / (1u32 << 24) as f32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn copies_are_remade_only_for_real_changes() {
        let grown = [2, 3, 100, 90, 150, 40, 0, 0x70];
        // Fruit ripening, a little growth: kept.
        assert!(!changed(&grown, &[2, 3, 105, 90, 160, 50, 10, 0x70]));
        // A fruit dropped, planted, grown up, or a different kind: remade.
        assert!(changed(&grown, &[2, 3, 100, 90, 0, 40, 0, 0x70]));
        assert!(changed(
            &[0, 0, 0, 0, 0, 0, 0, 0],
            &[3, 1, 0, 0, 0, 0, 0, 0]
        ));
        assert!(changed(&grown, &[2, 4, 100, 90, 150, 40, 0, 0x70]));
        assert!(changed(&grown, &[2, 3, 140, 90, 150, 40, 0, 0x70]));
    }
}
