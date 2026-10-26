//! Co-op rooms: an enemy, item box or supported breakable destroyed by anyone in the room is
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
/// Objects removed for good when destroyed: object list entries whose update calls
/// KillObject (0x488DA0, clears "alive"), and those that clear it themselves where they give
/// the destroy score (re/stage-objects.md "Breakables", "Stage sync audit"). Rings are separate
/// (`RINGS`, `RING_GROUPS`); treasure hunt pieces never go through here.
/// Standard CONTWOOD/CONTIRON (0x6CD7F0, 0x6D63F0, 0x6FB200, 0x6FBBA0) stay per player:
/// crates can be steps needed to progress, notably Hidden Base records 273/308. Sharing their
/// destruction removes those steps for slower players and re-deletes them after every death.
const BREAKABLES: [usize; 32] = [
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
    0x006D_A720, // DYNAMITE
    0x006E_3F80, // KDDOOR
    0x006E_A1E0, // WSMMLS
    0x006F_6EA0, // BCOL
    0x0077_7250, // POISON
    0x0077_9FB0, // SGOLID
    0x0077_A950, // CGWOOD
    0x006D_E3B0, // CONTCHAO (its contents: `chao_box_reward`)
    0x005C_3B50, // MDCONTBOX
    0x0060_60C0, // BOMBE
    0x0060_79E0, // TANK
    0x0063_7980, // TANK
    0x0069_C980, // TANK
    0x0069_EF30, // BOMBE
    0x006D_29F0, // ORI
    0x0076_D760, // SHUT
    0x0077_C870, // ORI
    0x0078_BA60, // TANK
];
/// RING and SGRING: collected by one player, gone for everyone and a ring for each.
const RINGS: [usize; 2] = [0x006C_0F80, 0x006C_1470];
/// A friend's ring pickup only counts once our stage has run this long (someone joining late
/// gets the whole list of what's gone, but not those rings).
const RING_REWARD_AFTER: std::time::Duration = std::time::Duration::from_secs(5);
/// Ring lines and circles (re/stage-objects.md "Ring groups"): RINGLINE, RINGCIRC and the
/// Knuckles/Rouge ones (KDDRNGL/C, SWDRNGL/C). A group spawns up to 8 rings of its own (RING's
/// MainSub, no record): its Data2 (+0x38) is 8-byte slots, +0 bit 2 collected, +4 the ring
/// (0 while there's none), and Data1+3 how many. While it's unloaded, the record's byte +1
/// keeps which were collected (bit 7 - ring); it clears "alive" once all of them are.
const RING_GROUPS: [usize; 6] = [
    0x006C_5ED0,
    0x006C_6180,
    0x006E_4830,
    0x006E_4FB0,
    0x006E_CAE0,
    0x006E_CE10,
];
const GROUP_RINGS: u8 = 8;
const SLOT_SIZE: usize = 8;
const SLOT_COLLECTED: u32 = 2;
const OBJ_DATA1: usize = 0x34;
const OBJ_DATA2: usize = 0x38;
/// A ring a group spawned: its slot (Data2), which it marks when it goes.
const RING_SLOT: usize = 0x38;

#[derive(Clone, Copy, Debug, PartialEq)]
enum Kind {
    Enemy,
    GroundBox,
    OtherBox,
    Breakable,
    Ring,
}

struct Coop {
    /// History belongs to this party and local stage visit. Death restarts keep the visit;
    /// joining another party or replaying a stage must not carry old kills/checkpoints along.
    scope: Option<(u8, u32)>,
    /// The stage these records belong to: (level, mission).
    stage: Option<(i16, u8)>,
    /// Flags as of last frame (one per record; empty until the stage is ready).
    last: Vec<u16>,
    /// Everything destroyed in this stage, by anyone.
    dead: BTreeSet<u16>,
    /// From the others for the stage we're in or loading, applied once it runs.
    inbox: Vec<(u8, i16, u8, Inbound)>,
    /// Ours, to send.
    outgoing: Vec<u16>,
    /// Send the whole `dead` set (someone just arrived).
    send_all: bool,
    /// Room members in this stage last frame.
    here: Vec<u8>,
    /// When we entered this stage.
    since: Option<std::time::Instant>,
    /// Stage keys (0/1) that went into their keyhole, in anyone's game; ours to send.
    keys: [bool; 2],
    keys_out: Vec<u8>,
    /// Switch flags as last seen (bit 0 = on), switches anyone turned on in this stage, and
    /// ours to send.
    switches_seen: Option<[u8; SWITCHES]>,
    switches_on: u32,
    switches_out: Vec<u8>,
    /// Checkpoint records anyone passed in this stage, ours to send, and the latest one (for
    /// players who arrive later).
    checkpoints: BTreeSet<u16>,
    checkpoints_out: Vec<Checkpoint>,
    checkpoint_last: Option<Checkpoint>,
    /// We died (or restarted) and the stage is coming back: set the room's switches again.
    restarting: bool,
    /// Upgrades: the flags as last seen, our CharObj2 upgrade bits as last seen, and ours to send.
    upgrades_seen: Option<[u8; UPGRADES]>,
    upgrade_bits_seen: u32,
    upgrades_out: Vec<Upgrade>,
    /// This stage's ring groups (records; found for `groups_of` records), group rings anyone
    /// collected, and ours to send.
    groups: Vec<u16>,
    groups_of: Option<usize>,
    group_rings: BTreeSet<GroupRing>,
    group_rings_out: Vec<GroupRing>,
}

/// Something a room player did in a stage.
enum Inbound {
    Kills(Vec<u16>),
    Key(u8),
    Switch(u8),
    Checkpoint(Checkpoint),
    GroupRings(Vec<GroupRing>),
    Upgrade(Upgrade),
}

/// A ring of a ring group: (the group's SET record, which ring 0..8).
pub type GroupRing = (u16, u8);

/// Most queued from the others while our stage loads.
const INBOX_MAX: usize = 512;

/// A passed checkpoint: (SET record, player position, player rotation).
pub type Checkpoint = (u16, [f32; 3], [i32; 3]);

impl Coop {
    const fn new() -> Self {
        Coop {
            scope: None,
            stage: None,
            last: Vec::new(),
            dead: BTreeSet::new(),
            inbox: Vec::new(),
            outgoing: Vec::new(),
            send_all: false,
            here: Vec::new(),
            since: None,
            keys: [false; 2],
            keys_out: Vec::new(),
            switches_seen: None,
            switches_on: 0,
            switches_out: Vec::new(),
            checkpoints: BTreeSet::new(),
            checkpoints_out: Vec::new(),
            checkpoint_last: None,
            restarting: false,
            upgrades_seen: None,
            upgrade_bits_seen: 0,
            upgrades_out: Vec::new(),
            groups: Vec::new(),
            groups_of: None,
            group_rings: BTreeSet::new(),
            group_rings_out: Vec::new(),
        }
    }

    fn enter_scope(&mut self, party: u8, run: u32) {
        if self.scope == Some((party, run)) {
            return;
        }
        let inbox = std::mem::take(&mut self.inbox)
            .into_iter()
            .filter(|(owner, _, _, _)| *owner == party)
            .collect();
        *self = Self::new();
        self.scope = Some((party, run));
        self.inbox = inbox;
    }
}

static COOP: Mutex<Coop> = Mutex::new(Coop::new());

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
/// Record `n`'s object name and MainSub (object list entry +0xC / +8), for the log.
fn describe(n: usize) -> (String, usize) {
    // SAFETY: as in `kind`; the name is a NUL-terminated string in the exe.
    unsafe {
        let entry = ((record(n) + 8) as *const usize).read();
        let list = (OBJECT_LIST as *const usize).read();
        if entry == 0 || list == 0 {
            return ("?".into(), 0);
        }
        let id = usize::from((entry as *const u16).read() & 0x7FFF);
        let (count, entries) = (
            (list as *const u32).read() as usize,
            ((list + 4) as *const usize).read(),
        );
        if id >= count || entries == 0 {
            return ("?".into(), 0);
        }
        let e = entries + id * 0x10;
        let (main, name) = (
            ((e + 8) as *const usize).read(),
            ((e + 0xC) as *const *const std::ffi::c_char).read(),
        );
        let name = if name.is_null() {
            "?".into()
        } else {
            std::ffi::CStr::from_ptr(name)
                .to_string_lossy()
                .trim()
                .to_string()
        };
        (name, main)
    }
}

/// Record `n`'s MainSub, from its object list entry.
fn main_of(n: usize) -> Option<usize> {
    // SAFETY: the SET entry and object list pointers are the game's for this stage; each is
    // checked for null and the list index against the list's length.
    unsafe {
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
        Some(((entries + id * 0x10 + 8) as *const usize).read())
    }
}

fn kind(n: usize) -> Option<Kind> {
    let main = main_of(n)?;
    kind_for_main(main)
}

fn kind_for_main(main: usize) -> Option<Kind> {
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

fn record_count() -> usize {
    // SAFETY: the game's record count for this stage.
    unsafe { (RECORD_COUNT as *const i16).read_volatile() }.clamp(0, MAX_RECORDS as i16) as usize
}

/// SET record `n`'s live object, while it's loaded and alive.
pub fn record_object(n: u16) -> Option<usize> {
    let n = usize::from(n);
    if n >= record_count() || flags(n) & (ALIVE | LOADED) != ALIVE | LOADED {
        return None;
    }
    // SAFETY: a loaded record's object pointer, in the game's static table.
    let obj = unsafe { ((record(n) + 4) as *const usize).read() };
    (obj != 0).then_some(obj)
}

/// The SET record whose loaded object is `object`, if any.
pub fn record_of(object: usize) -> Option<u16> {
    (0..record_count())
        .find(|&n| {
            // SAFETY: as in `record_object`.
            flags(n) & LOADED != 0 && unsafe { ((record(n) + 4) as *const usize).read() } == object
        })
        .map(|n| n as u16)
}

/// The enemies loaded in our stage now: (object, its list entry's MainSub, still alive, its SET
/// record).
pub fn live_enemies() -> Vec<(usize, usize, bool, u16)> {
    // SAFETY: the game's record count for this stage.
    let count = unsafe { (RECORD_COUNT as *const i16).read_volatile() }.clamp(0, MAX_RECORDS as i16)
        as usize;
    (0..count)
        .filter(|&n| flags(n) & LOADED != 0)
        .filter_map(|n| {
            let main = main_of(n).filter(|m| ENEMIES.contains(m))?;
            // SAFETY: a loaded record's object pointer, in the game's static table.
            let obj = unsafe { ((record(n) + 4) as *const usize).read() };
            (obj != 0).then_some((obj, main, flags(n) & ALIVE != 0, n as u16))
        })
        .collect()
}

/// Item boxes (re/stage-objects.md "Shared rewards"): the item is the SET entry's scale X
/// (+0x14) truncated to 0..=10 (inits 0x6C7DB0, 0x6C7F20, 0x6C9022, 0x6DB136, 0x6EAC93), an
/// index into its kind's table (8 bytes each: +0 the icon, +4 the reward, cdecl (box, player)).
/// Breaking one shows the icon (0x6DF440, cdecl (player, icon)) and calls the reward (0x6C8101,
/// 0x6C9266, 0x6DB28B, 0x6EADF4). KDITEMBOX shares the air box's break but not its item.
const ITEM_TABLES: [(usize, usize); 5] = [
    (0x006C_7C10, 0x00B4_D120), // ITEMBOX
    (0x006C_7CE0, 0x00B4_D120), // ITEMBOX2_B
    (0x006C_8EF0, 0x00B4_93A0), // ITEMBOXAIR
    (0x006D_B030, 0x00B1_9928), // ITEMBOXBALLOON
    (0x006E_ABC0, 0x00B0_AFA0), // SGITEMBOX
];

fn item_table(main: usize) -> Option<usize> {
    ITEM_TABLES
        .iter()
        .find(|&&(m, _)| m == main)
        .map(|&(_, table)| table)
}
const SHOW_ITEM: usize = 0x006D_F440;
const ITEM_LAST: i32 = 10;
/// Items not given again: 6 blows up the enemies around the box (those kills come as kills),
/// 9 is nothing; 2, the 1-up, isn't while the record says it was taken (flag 0x20).
const ITEM_BLAST: usize = 6;
const ITEM_NOTHING: usize = 9;
const ITEM_ONE_UP: usize = 2;
const ONE_UP_TAKEN: u16 = 0x20;

/// A box for the rewards to look at: an object whose Data1 is where the box is, with Data1+1
/// 0 and a record (+0x30) of its own (a 1-up marks it taken: some don't check for none).
/// Ours, never freed.
fn reward_box(at: [f32; 3]) -> usize {
    static BOX: std::sync::OnceLock<(usize, usize)> = std::sync::OnceLock::new();
    let &(object, data1) = BOX.get_or_init(|| {
        let data1: &'static mut [u8; 0x30] = Box::leak(Box::new([0; 0x30]));
        let record: &'static mut [u8; RECORD_SIZE] = Box::leak(Box::new([0; RECORD_SIZE]));
        let object: &'static mut [usize; 0x50 / 4] = Box::leak(Box::new([0; 0x50 / 4]));
        object[0x34 / 4] = data1.as_ptr() as usize;
        object[0x30 / 4] = record.as_ptr() as usize;
        (object.as_ptr() as usize, data1.as_ptr() as usize)
    });
    // SAFETY: our own Data1 block (0x30 bytes).
    unsafe { ((data1 + 0x14) as *mut [f32; 3]).write_unaligned(at) };
    object
}

/// Gives us the item in item box `n` (a friend opened it), as breaking it would, from its
/// kind's item table.
fn box_reward(n: usize, table: usize) {
    // SAFETY: the record's SET entry (the game's for this stage, checked for null) and the
    // game's item table.
    let (item, at) = unsafe {
        let entry = ((record(n) + 8) as *const usize).read();
        if entry == 0 {
            return;
        }
        let param = ((entry + 0x14) as *const f32).read_unaligned();
        (
            (param as i32).clamp(0, ITEM_LAST) as usize,
            ((entry + 8) as *const [f32; 3]).read_unaligned(),
        )
    };
    if item == ITEM_BLAST
        || item == ITEM_NOTHING
        || (item == ITEM_ONE_UP && flags(n) & ONE_UP_TAKEN != 0)
    {
        return;
    }
    // SAFETY: the game's item table entry and its own icon and reward functions, called as the
    // box calls them (0x6C810B..), for player 0 (us), on the game thread.
    unsafe {
        let icon = ((table + item * 8) as *const u32).read();
        let reward = ((table + item * 8 + 4) as *const usize).read();
        let show: extern "C" fn(i32, u32) = std::mem::transmute(SHOW_ITEM);
        show(0, icon);
        let give: extern "C" fn(usize, i32) = std::mem::transmute(reward);
        give(reward_box(at), 0);
    }
    log!("coop: a friend opened item box {n}; item {item} for us too");
}

/// Chao boxes (CONTCHAO 0x6DE3B0, opened at 0x6DE5E8..): while this stage's Chao key isn't got
/// (0x6E9C20: `0x174B004` != 1 and `0x1AEFF64` == 0) it lets out the key (0x6E9D10:
/// "ObjectChaoKeyExec", EBX = &position, cdecl (height, 2.5)); otherwise small animals
/// (0x48ACD0: ESI = kind, cdecl (x, y, z)), four around it the first time in a stage and one
/// the second (bits 0x100 / 0x200 of `0x1A5A32C`). Then its record gets 0x4 | 0x20 and loses
/// "alive".
const CHAO_BOX: usize = 0x006D_E3B0;
const KEY_GOT: usize = 0x0174_B004;
const KEY_GOT_STAGE: usize = 0x01AE_FF64;
const LET_OUT_KEY: usize = 0x006E_9D10;
const LET_OUT_ANIMAL: usize = 0x0048_ACD0;
const CHAO_BOXES_OPENED: usize = 0x01A5_A32C;
const OPENED_FIRST: u32 = 0x100;
const OPENED_SECOND: u32 = 0x200;
const CHAO_BOX_MARKS: u16 = 0x0024;

/// A friend opened Chao box `n`: what it holds comes out here too (the key, or animals), for
/// us to pick up.
fn chao_box_reward(n: usize) {
    // SAFETY: the record's SET entry (checked for null) and the game's static stage flags.
    let (at, key_got, opened) = unsafe {
        let entry = ((record(n) + 8) as *const usize).read();
        if entry == 0 {
            return;
        }
        (
            ((entry + 8) as *const [f32; 3]).read_unaligned(),
            (KEY_GOT as *const u8).read_volatile() == 1
                || (KEY_GOT_STAGE as *const u32).read_volatile() != 0,
            (CHAO_BOXES_OPENED as *const u32).read_volatile(),
        )
    };
    set_flags(n, flags(n) | CHAO_BOX_MARKS);
    if !key_got {
        let_out_key(at);
        log!("coop: a friend opened Chao box {n}; its key is out here too");
        return;
    }
    let animals: Vec<(i32, [f32; 3])> = if opened & OPENED_FIRST == 0 {
        (0..4)
            .map(|i| {
                let a = i as f32 * std::f32::consts::FRAC_PI_2;
                let kind = if i % 2 == 0 { 2 } else { 1 };
                (
                    kind,
                    [at[0] + a.sin() * 10.0, at[1] + 10.0, at[2] + a.cos() * 10.0],
                )
            })
            .collect()
    } else if opened & OPENED_SECOND == 0 {
        vec![(0, [at[0], at[1] + 10.0, at[2]])]
    } else {
        Vec::new()
    };
    let bit = if opened & OPENED_FIRST == 0 {
        OPENED_FIRST
    } else {
        OPENED_SECOND
    };
    // SAFETY: the game's static flag word, on the game thread.
    unsafe { (CHAO_BOXES_OPENED as *mut u32).write_volatile(opened | bit) };
    for (kind, p) in animals {
        let_out_animal(kind, p);
    }
    log!("coop: a friend opened Chao box {n}; its animals are out here too");
}

#[cfg(target_arch = "x86")]
fn let_out_key(at: [f32; 3]) {
    let position = at;
    let (height, spin) = (at[1], 2.5f32);
    // SAFETY: the game's key maker as CONTCHAO calls it (0x6DE624..): EBX = &position, two
    // floats on the stack (caller pops), on the game thread.
    unsafe {
        std::arch::asm!(
            "push ebx",
            "mov ebx, ecx",
            "push edx",
            "push eax",
            "mov eax, {f}",
            "call eax",
            "add esp, 8",
            "pop ebx",
            f = const LET_OUT_KEY,
            inout("ecx") &raw const position as usize => _,
            inout("eax") height.to_bits() => _,
            inout("edx") spin.to_bits() => _,
            clobber_abi("C"),
        );
    }
}

#[cfg(not(target_arch = "x86"))]
fn let_out_key(_: [f32; 3]) {
    let _ = LET_OUT_KEY;
}

#[cfg(target_arch = "x86")]
fn let_out_animal(kind: i32, at: [f32; 3]) {
    let [x, y, z] = at.map(f32::to_bits);
    // SAFETY: the game's animal maker as CONTCHAO calls it (0x6DE711): ESI = kind, x, y, z on
    // the stack (caller pops), on the game thread.
    unsafe {
        std::arch::asm!(
            "push esi",
            "mov esi, ecx",
            "push edi",
            "push edx",
            "push eax",
            "mov eax, {f}",
            "call eax",
            "add esp, 12",
            "pop esi",
            f = const LET_OUT_ANIMAL,
            inout("edi") z => _,
            inout("ecx") kind => _,
            inout("eax") x => _,
            inout("edx") y => _,
            clobber_abi("C"),
        );
    }
}

#[cfg(not(target_arch = "x86"))]
fn let_out_animal(_: i32, _: [f32; 3]) {
    let _ = LET_OUT_ANIMAL;
}

/// Ring group `n`'s slots while it's loaded: (Data2, how many).
fn group_slots(n: usize) -> Option<(usize, u8)> {
    if flags(n) & LOADED == 0 {
        return None;
    }
    // SAFETY: a loaded record's live object and its Data1/Data2, each checked for null.
    unsafe {
        let obj = ((record(n) + 4) as *const usize).read();
        if obj == 0 {
            return None;
        }
        let (d1, d2) = (
            ((obj + OBJ_DATA1) as *const usize).read(),
            ((obj + OBJ_DATA2) as *const usize).read(),
        );
        (d1 != 0 && d2 != 0).then(|| (d2, ((d1 + 3) as *const u8).read().min(GROUP_RINGS)))
    }
}

/// Which of ring group `n`'s rings are collected in our game (bit = ring).
fn group_collected(n: usize) -> u8 {
    if let Some((d2, count)) = group_slots(n) {
        (0..count)
            .filter(|&i| {
                // SAFETY: slot `i` of the group's Data2 (i < its count, at most 8).
                let slot = unsafe { ((d2 + usize::from(i) * SLOT_SIZE) as *const u32).read() };
                slot & SLOT_COLLECTED != 0
            })
            .fold(0, |m, i| m | 1 << i)
    } else {
        // SAFETY: the record's byte +1, in the game's static table.
        let kept = unsafe { ((record(n) + 1) as *const u8).read_volatile() };
        kept.reverse_bits()
    }
}

/// Ring `i` of group `n` is gone in our game, as if collected (no ring for us): the ring goes
/// and its slot is marked, the way the ring does it itself (0x6C1425); or, unloaded, the record
/// keeps it for when the group loads.
fn collect_group_ring(n: usize, i: u8) {
    if let Some((d2, count)) = group_slots(n) {
        if i >= count {
            return;
        }
        let slot = d2 + usize::from(i) * SLOT_SIZE;
        // SAFETY: the group's slot `i` and the ring it spawned (its own object, checked for
        // null); DeleteObject as its MainSub removes it on the next object pass.
        unsafe {
            let ring = ((slot + 4) as *const usize).read();
            if ring != 0 {
                ((ring + RING_SLOT) as *mut usize).write(0);
                ((ring + OBJ_MAIN) as *mut usize).write(DELETE_OBJECT);
            }
            (slot as *mut u32).write((slot as *const u32).read() | SLOT_COLLECTED);
            ((slot + 4) as *mut usize).write(0);
        }
    } else {
        let kept = (record(n) + 1) as *mut u8;
        // SAFETY: the record's byte +1, on the game thread.
        unsafe { kept.write_volatile(kept.read_volatile() | 0x80 >> i) };
    }
}

/// Queues what a room player did in the stage we're in (or loading).
fn receive(level: i16, mission: u8, what: Inbound) {
    if level == game::current_level() && mission == game::mission() {
        let party = crate::room::wire().0;
        let mut c = lock();
        if c.inbox.len() < INBOX_MAX {
            c.inbox.push((party, level, mission, what));
        }
    }
}

/// Network: kills from another player in a stage.
pub fn received(level: i16, mission: u8, objects: Vec<u16>) {
    receive(level, mission, Inbound::Kills(objects));
}

/// Network: rings of ring groups another player collected in a stage.
pub fn received_group_rings(level: i16, mission: u8, rings: Vec<GroupRing>) {
    let rings: Vec<GroupRing> = rings
        .into_iter()
        .filter(|&(_, i)| i < GROUP_RINGS)
        .collect();
    receive(level, mission, Inbound::GroupRings(rings));
}

/// An upgrade someone picked up: (its flag, the character it's for, its CharObj2 bits).
pub type Upgrade = (u8, u8, u32);

/// Upgrades (LEVUPDAI 0x6D8540, re/stage-objects.md "Shared rewards"): picking one up sets its
/// byte in the upgrade flags 0x1DEB300 (the save's, copied in at 0x68DBA1; 0x1E of them) and
/// ORs its bits into the player's CharObj2+0x24 when it's their character's (CharObj2+1).
const UPGRADE_FLAGS: usize = 0x01DE_B300;
const UPGRADES: usize = 0x1E;
const CHAR_OBJ2S: usize = 0x01DE_9600;
const CO2_CHARACTER: usize = 0x01;
const CO2_UPGRADES: usize = 0x24;

/// Network: a party player picked up an upgrade.
pub fn received_upgrade(level: i16, mission: u8, upgrade: Upgrade) {
    if usize::from(upgrade.0) < UPGRADES {
        receive(level, mission, Inbound::Upgrade(upgrade));
    }
}

/// Our upgrades picked up, to send: (level, mission, upgrade).
pub fn take_upgrades() -> Vec<(i16, u8, Upgrade)> {
    let mut c = lock();
    let Some((level, mission)) = c.stage else {
        return Vec::new();
    };
    std::mem::take(&mut c.upgrades_out)
        .into_iter()
        .map(|u| (level, mission, u))
        .collect()
}

fn upgrade_flags() -> [u8; UPGRADES] {
    // SAFETY: the game's static upgrade flags.
    unsafe { (UPGRADE_FLAGS as *const [u8; UPGRADES]).read_volatile() }
}

/// Our CharObj2 (null without a player).
fn char_obj2() -> usize {
    // SAFETY: the game's static player array.
    unsafe { (CHAR_OBJ2S as *const usize).read_volatile() }
}

/// Ours picked up are sent (with the bits they gave our character).
fn upgrades_frame(c: &mut Coop) {
    let now = upgrade_flags();
    let co2 = char_obj2();
    // SAFETY: our live CharObj2 (checked for null).
    let (bits, character) = if co2 == 0 {
        (0, 0)
    } else {
        unsafe {
            (
                ((co2 + CO2_UPGRADES) as *const u32).read_unaligned(),
                ((co2 + CO2_CHARACTER) as *const u8).read(),
            )
        }
    };
    if let Some(before) = c.upgrades_seen {
        for i in 0..UPGRADES {
            if before[i] == 0 && now[i] != 0 {
                let mask = bits & !c.upgrade_bits_seen;
                c.upgrades_out.push((i as u8, character, mask));
                log!("coop: we got upgrade {i}");
            }
        }
    }
    c.upgrades_seen = Some(now);
    c.upgrade_bits_seen = bits;
}

/// A friend's upgrade is ours too: its flag (kept in our save) and, when it's for our
/// character, its bits now.
fn give_upgrade(c: &mut Coop, (i, character, mask): Upgrade) {
    let flag = (UPGRADE_FLAGS + usize::from(i)) as *mut u8;
    // SAFETY: one of the game's static upgrade flags (i < UPGRADES).
    unsafe {
        if flag.read_volatile() != 0 {
            return;
        }
        flag.write_volatile(1);
    }
    let co2 = char_obj2();
    if co2 != 0 {
        // SAFETY: our live CharObj2.
        unsafe {
            if ((co2 + CO2_CHARACTER) as *const u8).read() == character {
                let at = (co2 + CO2_UPGRADES) as *mut u32;
                at.write_unaligned(at.read_unaligned() | mask);
            }
        }
    }
    c.upgrades_seen = Some(upgrade_flags());
    c.upgrade_bits_seen = if co2 == 0 {
        0
    } else {
        // SAFETY: as above.
        unsafe { ((co2 + CO2_UPGRADES) as *const u32).read_unaligned() }
    };
    log!("coop: a friend got upgrade {i}; ours too");
}

/// Our group rings to send, or all of them when someone joined the stage.
pub fn take_group_rings() -> Option<(i16, u8, Vec<GroupRing>)> {
    let mut c = lock();
    let (level, mission) = c.stage?;
    let rings = std::mem::take(&mut c.group_rings_out);
    (!rings.is_empty()).then_some((level, mission, rings))
}

/// Stage switch flags (re/stage-objects.md "Switches"): one byte per switch id, bit 0 on
/// (pressing sets 3); doors and gates read them, and a switch shows itself pressed from them.
const SWITCH_FLAGS: usize = 0x01DE_9460;
const SWITCHES: usize = 32;

/// Network: a room player's switch `id` turned on (off isn't shared).
pub fn received_switch(level: i16, mission: u8, id: u8, on: bool) {
    if on && usize::from(id) < SWITCHES {
        receive(level, mission, Inbound::Switch(id));
    }
}

/// Our switches turned on, to send: (level, mission, id).
pub fn take_switches() -> Vec<(i16, u8, u8)> {
    let mut c = lock();
    let Some((level, mission)) = c.stage else {
        return Vec::new();
    };
    std::mem::take(&mut c.switches_out)
        .into_iter()
        .map(|id| (level, mission, id))
        .collect()
}

fn switch_flags() -> [u8; SWITCHES] {
    // SAFETY: the game's static switch flag bytes.
    unsafe { (SWITCH_FLAGS as *const [u8; SWITCHES]).read_volatile() }
}

fn set_switch_on(id: usize) {
    let at = (SWITCH_FLAGS + id) as *mut u8;
    // SAFETY: as in `switch_flags`, on the game thread.
    unsafe { at.write_volatile(at.read_volatile() | 3) };
}

/// Our switches that turned on are sent. Only "on" is shared: a death restart clears every flag
/// (0x47E4B0 from 0x43C822/0x43C90F), and that must not switch things off for the others; once
/// the stage runs again, `on_frame` sets everything the room turned on again.
fn switches_frame(c: &mut Coop) {
    let now = switch_flags();
    let before = c.switches_seen.unwrap_or(now);
    for (id, (&was, &is)) in before.iter().zip(&now).enumerate() {
        if was & 1 == 0 && is & 1 != 0 && c.switches_on & (1 << id) == 0 {
            c.switches_on |= 1 << id;
            c.switches_out.push(id as u8);
        }
    }
    c.switches_seen = Some(now);
}

/// Network: a room player's stage key `key` went into its keyhole.
pub fn received_key(level: i16, mission: u8, key: u8) {
    if key < 2 {
        receive(level, mission, Inbound::Key(key));
    }
}

/// Our keys that went in, to send: (level, mission, key).
pub fn take_keys() -> Vec<(i16, u8, u8)> {
    let mut c = lock();
    let Some((level, mission)) = c.stage else {
        return Vec::new();
    };
    std::mem::take(&mut c.keys_out)
        .into_iter()
        .map(|k| (level, mission, k))
        .collect()
}

/// Stage keys (re/stage-objects.md "Keys"): each registers at `KEYS[type]`; its Data2 (+0x40)
/// runs a state machine: +0 state, +4 next state (-1 none). 0 on the ground, 1 carried, 2 going
/// into the keyhole, 3 in (the keyhole and door react), 4 back to where it started.
const KEYS: usize = 0x01AF_00A8;
/// Keyholes register here the same way (cleared to 0 when unloaded, 0x6FDDD4).
const KEYHOLES: usize = 0x01AF_00B0;
const KEY_GROUND: i32 = 0;
const KEY_INSERT: i32 = 2;
const KEY_IN: i32 = 3;
const KEY_RETURN: i32 = 4;

/// A friend's key went in: ours goes in too (only from the ground, never out of our hands), so
/// our door opens the game's own way. Ours going in is sent to them.
fn keys_frame(c: &mut Coop) {
    for k in 0..2 {
        // SAFETY: the game's key slots and the live key object's Data2, on the game thread.
        unsafe {
            let key = ((KEYS + k * 4) as *const usize).read();
            if key == 0 {
                continue;
            }
            let d2 = ((key + 0x40) as *const usize).read();
            if d2 == 0 {
                continue;
            }
            let (state, next) = ((d2 as *const i32).read(), ((d2 + 4) as *const i32).read());
            if state == KEY_IN && !c.keys[k] {
                c.keys[k] = true;
                c.keys_out.push(k as u8);
                log!("coop: our key {k} went in");
            } else if c.keys[k]
                && next == -1
                && matches!(state, KEY_GROUND | KEY_RETURN)
                // Going in reads the keyhole's slot unchecked; it's 0 while out of range.
                && ((KEYHOLES + k * 4) as *const usize).read() != 0
            {
                ((d2 + 4) as *mut i32).write(KEY_INSERT);
                log!("coop: key {k} was used by a friend; ours goes in too");
            }
        }
    }
}

/// Checkpoints (re/stage-race.md 3): SAVEPOINT's MainSub. Touching one saves the restart point
/// and marks its record passed; a passed one loads already lit (state 3).
const SAVEPOINT: usize = 0x006D_99A0;
const PASSED: u16 = 0x40;
const CHECKPOINT_IDLE: u8 = 1;
const CHECKPOINT_LIT: u8 = 3;
/// Player 1's restart point: +0 level, +2 checkpoint index (-1 none), +8 position, +0x14
/// rotation.
const RESTART_POINT: usize = 0x0193_4B88;
/// Saves a restart point: cdecl (player, index), EBX = &position, EDI = &rotation.
const SAVE_RESTART: usize = 0x0043_E520;

/// Network: a room player passed checkpoint `record`, standing at `position`/`rotation`.
pub fn received_checkpoint(
    level: i16,
    mission: u8,
    record: u16,
    position: [f32; 3],
    rotation: [i32; 3],
) {
    receive(
        level,
        mission,
        Inbound::Checkpoint((record, position, rotation)),
    );
}

/// Our checkpoints to send: (level, mission, checkpoints).
pub fn take_checkpoints() -> Option<(i16, u8, Vec<Checkpoint>)> {
    let mut c = lock();
    let (level, mission) = c.stage?;
    let out = std::mem::take(&mut c.checkpoints_out);
    (!out.is_empty()).then_some((level, mission, out))
}

/// Where we'd restart now, if a checkpoint in this level set it.
fn restart_point() -> Option<[f32; 3]> {
    // SAFETY: the game's static restart point for player 1.
    unsafe {
        let level = (RESTART_POINT as *const i16).read_volatile();
        let index = ((RESTART_POINT + 2) as *const i16).read_volatile();
        (level == game::current_level() && index != -1)
            .then(|| ((RESTART_POINT + 8) as *const [f32; 3]).read_volatile())
    }
}

/// Our restart point as the game just saved it: (position, rotation).
fn saved_restart() -> ([f32; 3], [i32; 3]) {
    // SAFETY: as in `restart_point`.
    unsafe {
        (
            ((RESTART_POINT + 8) as *const [f32; 3]).read_volatile(),
            ((RESTART_POINT + 0x14) as *const [i32; 3]).read_volatile(),
        )
    }
}

fn save_restart(position: [f32; 3], rotation: [i32; 3]) {
    let (pp, rp) = (&raw const position as usize, &raw const rotation as usize);
    #[cfg(target_arch = "x86")]
    // SAFETY: the game's own restart save as SAVEPOINT calls it (0x6DA306), on the game thread,
    // with pointers to our copies; index 0 like every 1P checkpoint.
    unsafe {
        std::arch::asm!(
            "push ebx",
            "mov ebx, ecx",
            "push 0",
            "push 0",
            "call eax",
            "add esp, 8",
            "pop ebx",
            inout("ecx") pp => _,
            inout("eax") SAVE_RESTART => _,
            inout("edi") rp => _,
            clobber_abi("C"),
        );
    }
    #[cfg(not(target_arch = "x86"))]
    let _ = (pp, rp, SAVE_RESTART);
}

/// A friend's checkpoint lights ours and becomes our restart point when we have none yet, or
/// it's further along the stage's route than ours.
fn apply_checkpoint(c: &mut Coop, count: usize, cp: Checkpoint) {
    {
        let (n, position, rotation) = cp;
        let i = usize::from(n);
        if i >= count || main_of(i) != Some(SAVEPOINT) {
            return;
        }
        c.checkpoints.insert(n);
        let f = flags(i);
        if f & PASSED != 0 {
            // We passed it ourselves.
            return;
        }
        set_flags(i, f | PASSED);
        c.last[i] = f | PASSED;
        if f & LOADED != 0 {
            // SAFETY: a loaded record's live object; Data1 byte 0 is its state.
            unsafe {
                let obj = ((record(i) + 4) as *const usize).read();
                let d1 = if obj == 0 {
                    0
                } else {
                    ((obj + 0x34) as *const usize).read()
                };
                if d1 != 0 && (d1 as *const u8).read() == CHECKPOINT_IDLE {
                    (d1 as *mut u8).write(CHECKPOINT_LIT);
                }
            }
        }
        if crate::stage_race::is_further(restart_point(), position) {
            save_restart(position, rotation);
            log!("coop: checkpoint {n} passed by a friend; we restart there now");
        } else {
            log!("coop: checkpoint {n} passed by a friend (ours is further)");
        }
        c.checkpoint_last = Some(cp);
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

/// The party-stage enemy, boss and lock-on hooks are on (config.ini `StageSync`, default On).
/// Off, enemies.rs, boss_follow.rs and lockon.rs leave the game's objects alone: a safety
/// switch if one of them misbehaves.
static STAGE_SYNC: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(true);

/// At startup: `StageSync` from config.ini.
pub fn init(mod_path: &str) {
    let text = std::fs::read_to_string(std::path::Path::new(mod_path).join("config.ini"))
        .unwrap_or_default();
    let on = crate::menu::Settings::parse(&text).stage_sync;
    STAGE_SYNC.store(on, std::sync::atomic::Ordering::Relaxed);
    if !on {
        log!("coop: StageSync=Off: enemies, bosses and lock-on aren't synced");
    }
}

/// The enemy, boss and lock-on hooks may run (`init`).
pub fn stage_sync() -> bool {
    STAGE_SYNC.load(std::sync::atomic::Ordering::Relaxed)
}

pub fn active(connected: bool) -> bool {
    let level = game::current_level();
    connected
        && crate::room::in_room()
        && crate::room::mode() == crate::room::Mode::Coop
        && level > 0
        && level != game::LEVEL_CHAO_WORLD
        && !game::two_player()
        && matches!(game::game_mode(), 5 | 10)
        && !game::local_entity().is_null()
        && game::game_state() == game::GAME_STATE_INGAME
}

/// Game states that keep a stage's shared state: in game, paused, a death's fade and restart
/// (9 -> 5 -> 7 -> 16), the time-limit restart (13 -> 7) and the pause menu's restart (15 -> 7)
/// (re/stage-race.md 3, 4). A co-op restart keeps what the room destroyed and switched.
const KEEP_STATES: [i16; 7] = game::STAGE_STATES;

/// Wait until a matching party member has finished loading before sending their history.
/// A level number is set before the objects and the receiver's stage inbox are ready.
fn settled_members(peers: &[Peer], members: &[u8], stage: (i16, u8)) -> Vec<u8> {
    peers
        .iter()
        .filter(|p| members.contains(&p.id) && (p.level, p.mission) == stage)
        .filter(|p| p.flags & game::POSE_LOADING == 0)
        .filter(|p| p.flags & (game::POSE_WAITING | game::POSE_PLAYING) != 0)
        .map(|p| p.id)
        .collect()
}

/// Call every frame with the other players' latest states.
pub fn on_frame(peers: &[Peer], connected: bool) {
    let mut c = lock();
    let level = game::current_level();
    let state = game::game_state();
    let stage = (level, game::mission());
    let in_stage = connected
        && crate::room::in_room()
        && crate::room::mode() == crate::room::Mode::Coop
        && level > 0
        && level != game::LEVEL_CHAO_WORLD
        && !game::two_player();
    let party = crate::room::wire().0;
    if in_stage {
        c.enter_scope(party, game::stage_run());
    }
    if !in_stage
        || !matches!(game::game_mode(), 5 | 10)
        || !KEEP_STATES.contains(&state)
        || c.stage.is_some_and(|s| s != stage)
    {
        // Left the stage (or co-op): forget it. What the others sent for the stage we're
        // loading waits in the inbox.
        if c.stage.is_some() || !in_stage {
            let inbox = if in_stage {
                std::mem::take(&mut c.inbox)
            } else {
                Vec::new()
            };
            *c = Coop::new();
            c.inbox = inbox;
        }
        return;
    }
    if state != game::GAME_STATE_INGAME {
        // Paused, or dying and restarting: keep everything.
        if state != game::GAME_STATE_INGAME + 1 && c.stage.is_some() {
            c.restarting = true;
        }
        return;
    }
    if game::local_entity().is_null() {
        return;
    }
    // SAFETY: the game's record count for this stage.
    let count = unsafe { (RECORD_COUNT as *const i16).read_volatile() }.clamp(0, MAX_RECORDS as i16)
        as usize;
    if c.stage.is_none() {
        log!(
            "coop: stage {} mission {}, {count} objects",
            stage.0,
            stage.1
        );
        c.stage = Some(stage);
        c.since = Some(std::time::Instant::now());
        c.last = (0..count).map(flags).collect();
    }
    if c.last.len() != count {
        c.last = (0..count).map(flags).collect();
    }
    if c.groups_of != Some(count) {
        c.groups_of = Some(count);
        c.groups = (0..count)
            .filter(|&n| main_of(n).is_some_and(|main| RING_GROUPS.contains(&main)))
            .map(|n| n as u16)
            .collect();
    }
    if std::mem::take(&mut c.restarting) {
        for id in 0..SWITCHES {
            if c.switches_on & (1 << id) != 0 {
                set_switch_on(id);
            }
        }
        c.switches_seen = Some(switch_flags());
        log!("coop: restarted; the room's switches are on again");
    }

    // From the others: kills, keys, switches and checkpoints.
    let rewards = c.since.is_some_and(|at| at.elapsed() >= RING_REWARD_AFTER);
    for (owner, l, m, what) in std::mem::take(&mut c.inbox) {
        if owner != party || (l, m) != stage {
            continue;
        }
        match what {
            Inbound::Kills(objects) => {
                for n in objects {
                    if usize::from(n) < count
                        // Older peers can still send platform crates or unknown records.
                        // Never retain/rebroadcast those events, including late-join history.
                        && kind(usize::from(n)).is_some()
                        && c.dead.insert(n)
                    {
                        // A ring a friend just picked up: one for us too, unless we took it
                        // ourselves at the same moment (it's gone here already, and that one
                        // paid).
                        if rewards
                            && kind(usize::from(n)) == Some(Kind::Ring)
                            && !is_destroyed(Kind::Ring, flags(usize::from(n)))
                        {
                            game::add_rings(1);
                        }
                        // An item box a friend opened: its item for us too, unless we opened
                        // it ourselves.
                        let i = usize::from(n);
                        if rewards
                            && let Some(table) = main_of(i).and_then(item_table)
                            && kind(i).is_some_and(|k| !is_destroyed(k, flags(i)))
                        {
                            box_reward(i, table);
                        }
                        if rewards
                            && main_of(i) == Some(CHAO_BOX)
                            && !is_destroyed(Kind::Breakable, flags(i))
                        {
                            chao_box_reward(i);
                        }
                    }
                }
            }
            Inbound::Key(k) => c.keys[usize::from(k)] = true,
            Inbound::Switch(id) => {
                if c.switches_on & (1 << id) == 0 {
                    c.switches_on |= 1 << id;
                    log!("coop: switch {id} on by a friend");
                }
                set_switch_on(usize::from(id));
            }
            Inbound::Checkpoint(cp) => apply_checkpoint(&mut c, count, cp),
            Inbound::Upgrade(upgrade) => give_upgrade(&mut c, upgrade),
            Inbound::GroupRings(rings) => {
                for (n, i) in rings {
                    if c.groups.contains(&n) && c.group_rings.insert((n, i)) {
                        // One for us too, as for a single ring.
                        if rewards && group_collected(usize::from(n)) & 1 << i == 0 {
                            game::add_rings(1);
                        }
                    }
                }
            }
        }
    }
    // Everything destroyed by anyone stays destroyed here (a death restart brings it back).
    let dead: Vec<u16> = c.dead.iter().copied().collect();
    for n in dead {
        let n = usize::from(n);
        if let Some(kind) = kind(n)
            && !is_destroyed(kind, flags(n))
        {
            destroy(n, kind);
        }
    }

    // Group rings anyone collected stay collected here; ours are sent.
    for &(n, i) in &c.group_rings {
        if group_collected(usize::from(n)) & 1 << i == 0 {
            collect_group_ring(usize::from(n), i);
        }
    }
    for n in c.groups.clone() {
        let collected = group_collected(usize::from(n));
        for i in 0..GROUP_RINGS {
            if collected & 1 << i != 0 && c.group_rings.insert((n, i)) {
                c.group_rings_out.push((n, i));
            }
        }
    }

    keys_frame(&mut c);
    upgrades_frame(&mut c);
    switches_frame(&mut c);
    // Our own kills: records that became destroyed since last frame.
    for n in 0..count {
        let now = flags(n);
        let before = std::mem::replace(&mut c.last[n], now);
        if now == before {
            continue;
        }
        if main_of(n) == Some(SAVEPOINT) {
            if now & PASSED != 0 && before & PASSED == 0 && c.checkpoints.insert(n as u16) {
                let (position, rotation) = saved_restart();
                let cp = (n as u16, position, rotation);
                c.checkpoints_out.push(cp);
                c.checkpoint_last = Some(cp);
                log!("coop: we passed checkpoint {n}");
            }
            continue;
        }
        if c.groups.contains(&(n as u16)) {
            // Its rings are shared one by one (above).
            continue;
        }
        let Some(kind) = kind(n) else {
            // Diagnostic: objects we don't share yet (keys, doors, switches...) changing state.
            // Loading in and out (0x1) happens all the time and isn't worth a line.
            if (now ^ before) & !LOADED != 0 {
                let (name, main) = describe(n);
                log!(
                    "coop: unshared {name} (record {n}, main {main:#x}) flags {before:#06x} -> {now:#06x}"
                );
            }
            continue;
        };
        if is_destroyed(kind, now) && !is_destroyed(kind, before) && c.dead.insert(n as u16) {
            c.outgoing.push(n as u16);
        }
    }

    // Someone in the room entered this stage: send them everything destroyed so far.
    let members = crate::room::members();
    let here = settled_members(peers, &members, stage);
    if here.iter().any(|id| !c.here.contains(id)) {
        if !c.dead.is_empty() {
            c.send_all = true;
        }
        for k in 0..2u8 {
            if c.keys[usize::from(k)] && !c.keys_out.contains(&k) {
                c.keys_out.push(k);
            }
        }
        for id in 0..SWITCHES {
            if c.switches_on & (1 << id) != 0 {
                c.switches_out.push(id as u8);
            }
        }
        if let Some(cp) = c.checkpoint_last {
            c.checkpoints_out.push(cp);
        }
        c.group_rings_out = c.group_rings.iter().copied().collect();
    }
    c.here = here;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn platform_crates_cannot_enter_shared_destruction_history() {
        // Hidden Base's step at (-3969.7, 840, -4677.6) is CONTWOOD 0x6FB200.
        // Both native sizes of wooden and iron crates must survive friends' kill snapshots.
        for main in [0x6CD7F0, 0x6D63F0, 0x6FB200, 0x6FBBA0] {
            assert_eq!(kind_for_main(main), None);
        }
        assert_eq!(kind_for_main(0x6FC330), None); // adjacent SOLIDBOX
        assert_eq!(kind_for_main(0x4FC700), Some(Kind::Enemy));
        assert_eq!(kind_for_main(0x6C7C10), Some(Kind::GroundBox));
        assert_eq!(kind_for_main(0x6DE3B0), Some(Kind::Breakable)); // Chao reward box
        assert_eq!(kind_for_main(0x6C0F80), Some(Kind::Ring));
    }

    #[test]
    fn switching_parties_clears_previous_history_and_keeps_only_new_party_inbox() {
        let mut c = Coop::new();
        c.enter_scope(1, 10);
        c.stage = Some((13, 0));
        c.dead.insert(7);
        c.checkpoint_last = Some((8, [1.0; 3], [0; 3]));
        c.keys[0] = true;
        c.inbox.push((1, 13, 0, Inbound::Kills(vec![9])));
        c.inbox.push((2, 13, 0, Inbound::Kills(vec![10])));
        c.enter_scope(2, 10);
        assert!(c.stage.is_none());
        assert!(c.dead.is_empty());
        assert!(c.checkpoint_last.is_none());
        assert_eq!(c.keys, [false; 2]);
        assert_eq!(c.inbox.len(), 1);
        assert_eq!(c.inbox[0].0, 2);
    }

    #[test]
    fn death_restart_keeps_history_but_a_new_stage_visit_resets_it() {
        let mut c = Coop::new();
        c.enter_scope(1, 10);
        c.dead.insert(7);
        c.enter_scope(1, 10);
        assert!(c.dead.contains(&7));
        c.inbox.push((1, 13, 0, Inbound::Kills(vec![10])));
        c.enter_scope(1, 11);
        assert!(c.dead.is_empty());
        // History received while the new stage loaded still applies once it is ready.
        assert_eq!(c.inbox.len(), 1);
    }

    #[test]
    fn history_is_sent_when_a_matching_party_member_settles() {
        let make = |id, level, mission, flags| Peer {
            id,
            level,
            mission,
            chao_area: 0,
            flags,
        };
        let mut peers = vec![
            make(1, 13, 0, game::POSE_LOADING),
            make(2, 13, 1, game::POSE_PLAYING),
            make(3, 14, 0, game::POSE_PLAYING),
            make(4, 13, 0, game::POSE_PLAYING),
        ];
        assert!(settled_members(&peers, &[1, 2, 3], (13, 0)).is_empty());
        peers[0].flags = 0;
        assert!(settled_members(&peers, &[1, 2, 3], (13, 0)).is_empty());
        peers[0].flags = game::POSE_WAITING;
        assert_eq!(settled_members(&peers, &[1, 2, 3], (13, 0)), vec![1]);
    }
}
