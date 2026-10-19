//! Chao Race with friends' Chao: the CPU racers of your next races are replaced by Chao other
//! players brought online (their "My Chao", see online.rs). See re/chao-stadium.md.
//!
//! Every racer is a normal 0x800-byte Chao record. The race spawn (0x558690) first has the
//! course's CPU racers built into a scratch buffer of 7 records at 0x1DB9540 (call at
//! 0x5586A0), then puts each record whose type byte (+0x80) is at least 2 into a lane and
//! creates it. We let the game build its CPU racers, then copy friends' records over the first
//! lanes. The buffer is never saved, so nothing reaches anyone's save.
//!
//! Each player runs their own race (races aren't streamed between games).

#![cfg_attr(not(target_arch = "x86"), allow(dead_code))]

use std::sync::Mutex;

use crate::chao::{CHAO_SIZE, ChaoData};
use crate::hud;
use crate::log;

/// The race spawn's call that builds the CPU racers.
const BUILD_CPU_RACERS_CALL: usize = 0x0055_86A0;
const BUILD_CPU_RACERS: usize = 0x0059_6140;
/// Scratch records for the CPU racers (lanes 1..=7).
const CPU_RECORDS: usize = 0x01DB_9540;
const CPU_LANES: usize = 7;
/// The racers' records (ChaoData*), lane 0 = yours; lanes 1..=7 point into CPU_RECORDS.
const RACER_RECORDS: usize = 0x01A5_CC00;
const LANES: usize = 8;
/// `ObjectMaster::Data1` and `EntityData1::Position`.
const OBJ_DATA1: usize = 0x34;
const DATA1_POSITION: usize = 0x14;
/// How far above a Chao its label sits.
const LABEL_HEIGHT: f32 = 6.0;
const TAGS_NAME: &[u8] = b"SA2ChaoOnlineRaceTags\0";

struct Race {
    enabled: bool,
    /// Friends' Chao to race against: (label, record).
    racers: Vec<(String, ChaoData, u8)>,
    /// Labels of lanes 1.. in the race being set up / run.
    lanes: Vec<(String, u8)>,
    /// Our object that draws the labels during a race (0 = none).
    tags_object: usize,
    /// We've been in the race area since the labels were set up.
    in_race: bool,
}

static RACE: Mutex<Race> = Mutex::new(Race {
    enabled: true,
    racers: Vec::new(),
    lanes: Vec::new(),
    tags_object: 0,
    in_race: false,
});

/// `ChaoArea_Race`: the race course itself (the race is set up while the entrance is loaded).
const AREA_RACE: i32 = 4;

pub fn set_enabled(enabled: bool) {
    RACE.lock().unwrap_or_else(|e| e.into_inner()).enabled = enabled;
}

/// Called every frame with the Chao other players brought (in a stable order).
pub fn set_racers(racers: Vec<(String, ChaoData, u8)>) {
    RACE.lock().unwrap_or_else(|e| e.into_inner()).racers = racers;
}

/// Hooks the CPU racer setup. Call once at startup.
pub fn install() {
    let target = build_racers as extern "C" fn() as usize;
    match crate::patch::redirect_call(BUILD_CPU_RACERS_CALL, target) {
        Some(BUILD_CPU_RACERS) => {}
        Some(original) => {
            crate::patch::redirect_call(BUILD_CPU_RACERS_CALL, original);
            log!("race: {BUILD_CPU_RACERS_CALL:#x} is already hooked; online races are off");
        }
        None => log!("race: can't hook {BUILD_CPU_RACERS_CALL:#x}; online races are off"),
    }
}

extern "C" fn build_racers() {
    // SAFETY: the game's own function that this call site called (no arguments).
    unsafe { std::mem::transmute::<usize, extern "C" fn()>(BUILD_CPU_RACERS)() };
    let mut r = RACE.lock().unwrap_or_else(|e| e.into_inner());
    if !r.enabled || r.racers.is_empty() {
        return;
    }
    let mut names = Vec::new();
    let mut lanes = Vec::new();
    for (lane, (label, chao, owner)) in r.racers.iter().take(CPU_LANES).enumerate() {
        // SAFETY: the game's static scratch buffer of CPU_LANES records, filled just now.
        unsafe {
            let dest = (CPU_RECORDS + lane * CHAO_SIZE) as *mut u8;
            std::ptr::copy_nonoverlapping(chao.bytes.as_ptr(), dest, CHAO_SIZE);
        }
        names.push(label.clone());
        lanes.push((label.clone(), *owner));
    }
    log!("race: friends' Chao in lanes 2..: {}", names.join(", "));
    hud::toast(format!("Chao Race with {}", names.join(", ")));
    r.lanes = lanes;
    r.in_race = false;
}

/// Call every frame: keeps the race labels up while a race with friends runs. The game frees
/// all objects when the race course loads, so the label object is (re)made in the race area.
pub fn on_frame() {
    let mut r = RACE.lock().unwrap_or_else(|e| e.into_inner());
    if r.lanes.is_empty() {
        return;
    }
    let area = crate::game::current_chao_area();
    if area == AREA_RACE {
        r.in_race = true;
        if r.tags_object == 0 && !racer_positions().is_empty() {
            r.tags_object =
                crate::puppet::create_display_object(TAGS_NAME, display_tags, delete_tags);
            log!("race: labels on ({} friends' Chao)", r.lanes.len());
        }
    } else if r.in_race || crate::game::current_level() != crate::game::LEVEL_CHAO_WORLD {
        // The race is over (or we left Chao World before it started).
        r.lanes.clear();
        r.in_race = false;
    }
}

extern "C" fn delete_tags(_object: usize) {
    let _ = std::panic::catch_unwind(|| {
        RACE.lock().unwrap_or_else(|e| e.into_inner()).tags_object = 0;
    });
}

/// Heads of the game's 8 object lists (circular, linked through `NextObject` at +4).
const OBJECT_LISTS: usize = 0x01A5_A254;
const OBJECT_LIST_COUNT: usize = 8;
const OBJ_NEXT: usize = 0x04;
const OBJ_NAME: usize = 0x44;
/// `CreateChao` names its objects with this string (0x550263).
const CHAO_OBJECT_NAME: usize = 0x0131_3620;
/// `ChaoData1` + 0x5C: the Chao's record.
const DATA1_RECORD: usize = 0x5C;

/// Positions of the racing Chao by lane.
fn racer_positions() -> Vec<(usize, crate::game::Vector)> {
    racers()
        .into_iter()
        .map(|r| {
            // SAFETY: Data1 of a live race Chao (found just now).
            let position = unsafe {
                ((r.data1 + DATA1_POSITION) as *const crate::game::Vector).read_unaligned()
            };
            (r.lane, position)
        })
        .collect()
}

/// A racing Chao: its lane, object and Data1.
#[derive(Clone, Copy)]
struct Racer {
    lane: usize,
    object: usize,
    data1: usize,
}

/// The racing Chao, found by their records (lane table 0x1A5CC00) among the Chao objects in
/// the object lists.
fn racers() -> Vec<Racer> {
    // SAFETY: the game's static tables and live object lists, read on the game thread; only
    // objects named like CreateChao's are looked into.
    unsafe {
        let records: Vec<usize> = (0..LANES)
            .map(|lane| ((RACER_RECORDS + lane * 4) as *const usize).read())
            .collect();
        let mut found = Vec::new();
        for list in 0..OBJECT_LIST_COUNT {
            let head = ((OBJECT_LISTS + list * 4) as *const usize).read();
            let mut object = head;
            for _ in 0..4096 {
                if object == 0 {
                    break;
                }
                if ((object + OBJ_NAME) as *const usize).read() == CHAO_OBJECT_NAME {
                    let data1 = ((object + OBJ_DATA1) as *const usize).read();
                    if data1 != 0 {
                        let record = ((data1 + DATA1_RECORD) as *const usize).read();
                        if let Some(lane) = records.iter().position(|&r| r != 0 && r == record) {
                            found.push(Racer {
                                lane,
                                object,
                                data1,
                            });
                        }
                    }
                }
                object = ((object + OBJ_NEXT) as *const usize).read();
                if object == head {
                    break;
                }
            }
        }
        found
    }
}

/// Labels over your Chao ("You") and friends' Chao; CPU racers have none.
extern "C" fn display_tags(_object: usize) {
    crate::watch::mark("race tags");
    let _ = std::panic::catch_unwind(|| {
        let r = RACE.lock().unwrap_or_else(|e| e.into_inner());
        if crate::game::current_chao_area() != AREA_RACE || r.lanes.is_empty() {
            return;
        }
        let racers = racer_positions();
        let Some(&(_, reference)) = racers.first() else {
            return;
        };
        // SAFETY: display callback (game thread, display pass).
        let Some(tags) = (unsafe { crate::puppet::TagProjector::new(reference) }) else {
            return;
        };
        for (lane, p) in racers {
            let label = match lane {
                0 => Some("You"),
                lane => r.lanes.get(lane - 1).map(|(label, _)| label.as_str()),
            };
            if let Some(label) = label {
                tags.draw_at([p.x, p.y, p.z], LABEL_HEIGHT, label);
            }
        }
    });
}

// ---- Synced race start (rooms) ----
//
// The race manager (`*0x1A5AF18`) counts beats in `Data1+6` while `Data1[0] == 1` and creates
// the starter (starting gun) at about 15-16 beats (`Data1+3` = frames per beat). Holding the
// counter at 14 beats keeps every Chao idle at the line, after the line-up intro, until
// everyone in the room is in the race (re/chao-race-sync.md section 2).

use std::time::{Duration, Instant};

use crate::game::{POSE_GO, POSE_WAITING};
use crate::together::Peer;

const RACE_MANAGER: usize = 0x01A5_AF18;
const MANAGER_COUNTING: u8 = 1;
const HOLD_BEATS: u16 = 14;
const GO_FLAG_TIME: Duration = Duration::from_secs(2);

#[derive(Clone, Copy, PartialEq)]
enum Start {
    Idle,
    Holding,
    Released { go_until: Instant },
}

struct StartState {
    phase: Start,
    start_now: bool,
}

static START: Mutex<StartState> = Mutex::new(StartState {
    phase: Start::Idle,
    start_now: false,
});

/// F9 > Start now while holding at the race start.
pub fn start_now() {
    START.lock().unwrap_or_else(|e| e.into_inner()).start_now = true;
}

pub fn is_holding() -> bool {
    matches!(
        START.lock().unwrap_or_else(|e| e.into_inner()).phase,
        Start::Holding
    )
}

/// `POSE_*` flags for our next player state.
pub fn flags() -> u8 {
    match START.lock().unwrap_or_else(|e| e.into_inner()).phase {
        Start::Holding => POSE_WAITING,
        Start::Released { go_until } if Instant::now() < go_until => POSE_GO,
        _ => 0,
    }
}

/// The race manager's Data1, while it counts beats before the start.
fn manager_counting() -> Option<usize> {
    // SAFETY: static pointer and the live manager object in the race area.
    unsafe {
        let manager = (RACE_MANAGER as *const usize).read();
        if manager == 0 {
            return None;
        }
        let data1 = ((manager + OBJ_DATA1) as *const usize).read();
        (data1 != 0 && (data1 as *const u8).read() == MANAGER_COUNTING).then_some(data1)
    }
}

/// Call every frame with the other players' states.
pub fn start_frame(peers: &[Peer], connected: bool) {
    let mut s = START.lock().unwrap_or_else(|e| e.into_inner());
    let pressed = std::mem::take(&mut s.start_now);
    let in_race = crate::game::current_level() == crate::game::LEVEL_CHAO_WORLD
        && crate::game::current_chao_area() == AREA_RACE;
    if !in_race {
        s.phase = Start::Idle;
        return;
    }
    let now = Instant::now();
    // The room's players in the race.
    let members = crate::room::members();
    let others: Vec<&Peer> = peers
        .iter()
        .filter(|p| {
            members.contains(&p.id)
                && p.level == crate::game::LEVEL_CHAO_WORLD
                && i32::from(p.chao_area) == AREA_RACE
        })
        .collect();
    match s.phase {
        Start::Idle => {
            if crate::room::in_room() && connected && manager_counting().is_some() {
                s.phase = Start::Holding;
                log!("race: holding the start for the room");
            }
        }
        Start::Holding => {
            let Some(data1) = manager_counting() else {
                s.phase = Start::Released { go_until: now };
                return;
            };
            // Anyone in this race goes for everyone in it (F8 or F9 > Start now).
            let someone_go = others.iter().any(|p| p.flags & POSE_GO != 0);
            if pressed || someone_go || !connected {
                log!(
                    "race: start ({})",
                    if pressed {
                        "we started"
                    } else if someone_go {
                        "someone started"
                    } else {
                        "disconnected"
                    }
                );
                s.phase = Start::Released {
                    go_until: now + GO_FLAG_TIME,
                };
                hud::toast("Race: go!");
                return;
            }
            // SAFETY: the manager's Data1 (checked above): beat length byte and beat counter.
            unsafe {
                let beat = u16::from(((data1 + 3) as *const u8).read());
                let counter = (data1 + 6) as *mut u16;
                if beat > 0 && counter.read_unaligned() > HOLD_BEATS * beat {
                    counter.write_unaligned(HOLD_BEATS * beat);
                }
            }
            hud::center2(
                &crate::together::opponents_here(others.iter().map(|p| p.id)),
                "Press F8 to go",
            );
        }
        Start::Released { .. } => {}
    }
}

// ---- Real-time race (rooms) ----
//
// Each player sends their own racing Chao (lane 0) instead of their character while in a race:
// position, facing and race progress (`ChaoData1+0xA8`, waypoint index + fraction). In our race,
// a friend's lane keeps the game's race behaviour (MainSub 0x53FC10: animation, effects, goal
// check, progress) but, while the race runs, after each update is put where their real Chao
// is, shifted by the gap between their lane in their game and in ours (measured when the race
// starts), so speed-ups and the finish happen when they really did (re/chao-race-sync.md 3).
// Only the position and the animation are set: facing and progress stay the game's, which
// keeps the camera and ranking sane. The animation is their Chao's (sent in the state's `anim`)
// whenever theirs changes, so a friend's Chao resting, tripping or cheering in their race does
// it in ours too, not just the run its lane's own behaviour would play.

const RACE_CHAO_MAIN: usize = 0x0053_FC10;
const OBJ_MAIN: usize = 0x10;
const DATA1_ROTATION_Y: usize = 0x0C;
const DATA1_PROGRESS: usize = 0xA8;
/// The race manager's main state (`*(int*)mgr->Data2`): 3 = running.
const OBJ_DATA2: usize = 0x40;
const RACE_RUNNING: i32 = 3;
/// Stop following a friend's Chao when their updates stop for this long.
const DRIVE_STALE: Duration = Duration::from_secs(1);

/// A friend's racing Chao as last seen: when, where, and its animation.
#[derive(Clone, Copy)]
struct Target {
    at: Instant,
    position: crate::game::Vector,
    motion: u16,
}

/// Data1: the motion controller (flags +0, bit 2 blending; current +4, next +8; table +0x48 of
/// 0x1C-byte entries) and SetMotionLink (EAX = controller, EDX = id).
const DATA1_MOTION: usize = 0xAC;
const MC_FLAGS: usize = 0x00;
const MC_CURRENT: usize = 0x04;
const MC_NEXT: usize = 0x08;
const MC_TABLE: usize = 0x48;
const MOTION_ENTRY_SIZE: usize = 0x1C;
const MOTION_BLENDING: u16 = 0x2;
const MAX_MOTION_ID: u16 = 0x400;
#[cfg(target_arch = "x86")]
const START_MOTION: usize = 0x0079_3C40;

/// The animation a race Chao plays (or is blending into).
///
/// # Safety
/// `data1` is a live race Chao's Data1.
unsafe fn motion_of(data1: usize) -> u16 {
    unsafe {
        let mc = data1 + DATA1_MOTION;
        let next = ((mc + MC_NEXT) as *const i32).read();
        let id = if ((mc + MC_FLAGS) as *const u16).read() & MOTION_BLENDING != 0 && next >= 0 {
            next
        } else {
            ((mc + MC_CURRENT) as *const i32).read()
        };
        u16::try_from(id).unwrap_or(0)
    }
}

/// Starts animation `id` on a race Chao, if it's in the shared motion table.
///
/// # Safety
/// `data1` is a live race Chao's Data1, on the game thread.
#[cfg_attr(not(target_arch = "x86"), allow(clippy::needless_return))]
unsafe fn start_motion(data1: usize, id: u16) {
    let mc = data1 + DATA1_MOTION;
    // SAFETY: the Chao's motion controller and its table (checked for null).
    let ok = unsafe {
        let table = ((mc + MC_TABLE) as *const usize).read();
        id < MAX_MOTION_ID
            && table != 0
            && ((table + usize::from(id) * MOTION_ENTRY_SIZE) as *const usize).read() != 0
    };
    if !ok {
        return;
    }
    #[cfg(target_arch = "x86")]
    // SAFETY: the game's SetMotionLink as behaviours call it.
    unsafe {
        std::arch::asm!(
            "call {f}",
            f = in(reg) START_MOTION,
            inout("eax") mc => _,
            inout("edx") u32::from(id) => _,
            clobber_abi("C"),
        );
    }
}

struct Driving {
    /// Owner (player id) -> their racing Chao.
    targets: std::collections::HashMap<u8, Target>,
    /// Our objects for friends' lanes -> owner, and the lane gap once the race runs.
    objects: Vec<(usize, u8, Option<crate::game::Vector>)>,
    /// The animation each lane was last given from its owner's: object -> id.
    motions: std::collections::HashMap<usize, u16>,
}

static DRIVING: Mutex<Option<Driving>> = Mutex::new(None);

/// Network: a friend's racing Chao (their own lane 0) in their game.
pub fn set_target(owner: u8, position: [f32; 3], motion: u16) {
    let mut d = DRIVING.lock().unwrap_or_else(|e| e.into_inner());
    let d = d.get_or_insert_with(|| Driving {
        targets: Default::default(),
        objects: Vec::new(),
        motions: Default::default(),
    });
    let [x, y, z] = position;
    d.targets.insert(
        owner,
        Target {
            at: Instant::now(),
            position: crate::game::Vector { x, y, z },
            motion,
        },
    );
}

/// Our own racing Chao, to send instead of our character: (position, facing, progress,
/// animation).
pub fn my_racer() -> Option<([f32; 3], i32, f32, u16)> {
    let in_race = crate::game::current_level() == crate::game::LEVEL_CHAO_WORLD
        && crate::game::current_chao_area() == AREA_RACE;
    if !in_race {
        return None;
    }
    let mine = racers().into_iter().find(|r| r.lane == 0)?;
    // SAFETY: Data1 of our live race Chao.
    unsafe {
        let p = ((mine.data1 + DATA1_POSITION) as *const crate::game::Vector).read_unaligned();
        Some((
            [p.x, p.y, p.z],
            ((mine.data1 + DATA1_ROTATION_Y) as *const i32).read_unaligned(),
            ((mine.data1 + DATA1_PROGRESS) as *const f32).read_unaligned(),
            motion_of(mine.data1),
        ))
    }
}

/// Call every frame: hands friends' lanes over to their owners' real Chao in a room race.
pub fn drive_frame() {
    let in_race = crate::game::current_level() == crate::game::LEVEL_CHAO_WORLD
        && crate::game::current_chao_area() == AREA_RACE;
    let mut d = DRIVING.lock().unwrap_or_else(|e| e.into_inner());
    let Some(d) = d.as_mut() else { return };
    if !in_race || !crate::room::in_room() {
        // The race objects are gone (or we're not racing together): forget them.
        d.objects.clear();
        d.motions.clear();
        return;
    }
    let owners: Vec<(usize, u8)> = {
        let r = RACE.lock().unwrap_or_else(|e| e.into_inner());
        // Each friend's first lane is the one their real Chao drives.
        let mut first: Vec<(usize, u8)> = Vec::new();
        for (i, (_, owner)) in r.lanes.iter().enumerate() {
            if !first.iter().any(|(_, o)| o == owner) {
                first.push((i + 1, *owner));
            }
        }
        first
    };
    for racer in racers() {
        let Some(&(_, owner)) = owners.iter().find(|(lane, _)| *lane == racer.lane) else {
            continue;
        };
        if !d.targets.contains_key(&owner) || d.objects.iter().any(|(o, _, _)| *o == racer.object) {
            continue;
        }
        // SAFETY: a live race Chao object; we only swap its MainSub for our wrapper, which
        // calls the original first.
        unsafe {
            let main = (racer.object + OBJ_MAIN) as *mut usize;
            if main.read() == RACE_CHAO_MAIN {
                main.write(drive_main as extern "C" fn(usize) as usize);
                d.objects.push((racer.object, owner, None));
                log!(
                    "race: lane {} now follows player {owner}'s Chao",
                    racer.lane
                );
            }
        }
    }
}

/// MainSub of a friend's lane: the normal race behaviour, then their real Chao's place.
extern "C" fn drive_main(object: usize) {
    // SAFETY: the race Chao's own MainSub, which this object had.
    unsafe { std::mem::transmute::<usize, extern "C" fn(usize)>(RACE_CHAO_MAIN)(object) };
    let _ = std::panic::catch_unwind(|| {
        if !race_running() {
            return;
        }
        let mut d = DRIVING.lock().unwrap_or_else(|e| e.into_inner());
        let Some(d) = d.as_mut() else { return };
        let Some(t) = d
            .objects
            .iter()
            .find(|(o, _, _)| *o == object)
            .and_then(|(_, owner, _)| d.targets.get(owner))
            .copied()
        else {
            return;
        };
        if t.at.elapsed() > DRIVE_STALE {
            return;
        }
        let Some(entry) = d.objects.iter_mut().find(|(o, _, _)| *o == object) else {
            return;
        };
        // SAFETY: the object's Data1 (the game just ran its MainSub with it).
        unsafe {
            let data1 = ((object + OBJ_DATA1) as *const usize).read();
            if data1 == 0 {
                return;
            }
            let position = (data1 + DATA1_POSITION) as *mut crate::game::Vector;
            let here = position.read_unaligned();
            // Their lane in their game vs this lane in ours, measured once at the start.
            let gap = *entry.2.get_or_insert(crate::game::Vector {
                x: here.x - t.position.x,
                y: here.y - t.position.y,
                z: here.z - t.position.z,
            });
            position.write_unaligned(crate::game::Vector {
                x: t.position.x + gap.x,
                y: t.position.y + gap.y,
                z: t.position.z + gap.z,
            });
            // Their animation when theirs changes (and ours isn't it already).
            if d.motions.get(&object) != Some(&t.motion) {
                d.motions.insert(object, t.motion);
                if motion_of(data1) != t.motion {
                    start_motion(data1, t.motion);
                }
            }
        }
    });
}

/// In a room race that is running with friends in it (pausing doesn't stop it).
pub fn live() -> bool {
    let in_race = crate::game::current_level() == crate::game::LEVEL_CHAO_WORLD
        && crate::game::current_chao_area() == AREA_RACE;
    in_race
        && crate::room::in_room()
        && !RACE
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .lanes
            .is_empty()
}

/// The race is running (after the starting gun, before the finish).
fn race_running() -> bool {
    // SAFETY: static pointer and the live manager object in the race area.
    unsafe {
        let manager = (RACE_MANAGER as *const usize).read();
        if manager == 0 {
            return false;
        }
        let data2 = ((manager + OBJ_DATA2) as *const usize).read();
        data2 != 0 && (data2 as *const i32).read() == RACE_RUNNING
    }
}
