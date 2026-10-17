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
    /// From the others for the stage we're in or loading, applied once it runs.
    inbox: Vec<(i16, u8, Inbound)>,
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
}

/// Something a room player did in a stage.
enum Inbound {
    Kills(Vec<u16>),
    Key(u8),
    Switch(u8),
    Checkpoint(Checkpoint),
}

/// Most queued from the others while our stage loads.
const INBOX_MAX: usize = 512;

/// A passed checkpoint: (SET record, player position, player rotation).
pub type Checkpoint = (u16, [f32; 3], [i32; 3]);

impl Coop {
    const fn new() -> Self {
        Coop {
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
        }
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

/// Queues what a room player did in the stage we're in (or loading).
fn receive(level: i16, mission: u8, what: Inbound) {
    if level == game::current_level() && mission == game::mission() {
        let mut c = lock();
        if c.inbox.len() < INBOX_MAX {
            c.inbox.push((level, mission, what));
        }
    }
}

/// Network: kills from another player in a stage.
pub fn received(level: i16, mission: u8, objects: Vec<u16>) {
    receive(level, mission, Inbound::Kills(objects));
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

pub fn active(connected: bool) -> bool {
    let level = game::current_level();
    connected
        && crate::room::in_room()
        && crate::room::mode() == crate::room::Mode::Coop
        && level > 0
        && level != game::LEVEL_CHAO_WORLD
        && !game::two_player()
        && game::game_state() == game::GAME_STATE_INGAME
}

/// Game states that keep a stage's shared state: in game, paused, a death's fade and restart
/// (9 -> 5 -> 7 -> 16), the time-limit restart (13 -> 7) and the pause menu's restart (15 -> 7)
/// (re/stage-race.md 3, 4). A co-op restart keeps what the room destroyed and switched.
const KEEP_STATES: [i16; 7] = game::STAGE_STATES;

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
    if !in_stage || !KEEP_STATES.contains(&state) || c.stage.is_some_and(|s| s != stage) {
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
    for (l, m, what) in std::mem::take(&mut c.inbox) {
        if (l, m) != stage {
            continue;
        }
        match what {
            Inbound::Kills(objects) => {
                for n in objects {
                    if usize::from(n) < count && c.dead.insert(n) {
                        // A ring a friend just picked up: one for us too, unless we took it
                        // ourselves at the same moment (it's gone here already, and that one
                        // paid).
                        if rewards
                            && kind(usize::from(n)) == Some(Kind::Ring)
                            && !is_destroyed(Kind::Ring, flags(usize::from(n)))
                        {
                            game::add_rings(1);
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

    keys_frame(&mut c);
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
    let here: Vec<u8> = peers
        .iter()
        .filter(|p| members.contains(&p.id) && (p.level, p.mission) == stage)
        .map(|p| p.id)
        .collect();
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
    }
    c.here = here;
}
