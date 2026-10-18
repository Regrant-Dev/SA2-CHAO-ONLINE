//! Kart Race as a room battle (re/kart.md).
//!
//! - **Pick:** whoever in the room starts a Kart Race takes the others who wait in the Kart
//!   Race course list along to the same course, each with their own driver.
//! - **Start:** the room's start hold (together.rs, without its own countdown for karts) keeps
//!   everyone frozen on the grid; released together, the game's fly-by and lights run in step.
//! - **Race:** the CPU karts can't be removed (the gates read all six cars), so in a room race
//!   they're switched off, hidden and set 10 laps behind: only players race, and each player's
//!   race ends when they cross the line. Progress (laps and gates) and the finish time go to
//!   the room through stage_race.rs, which shows positions and finishes.
//! - **Karts of the others** are drawn with the game's own kart display from their state.

use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use crate::game::{self, Vector};
use crate::hud;
use crate::log;

/// The Kart Race level (1P and 2P), and the story kart level.
pub const KART_LEVEL: i16 = 71;
pub const STORY_KART_LEVEL: i16 = 70;

/// The kart menu: CurrentMenu 10, its SubMenu (5 = course list, 0xB = starting), its work.
const CURRENT_MENU: usize = 0x01D7_BB10;
const SUB_MENU: usize = 0x01D7_BB14;
const MENU_KART: i32 = 10;
const SUB_COURSE_LIST: i32 = 5;
const SUB_START: i32 = 0xB;
const KART_MENU: usize = 0x00C7_7F34;
const K_BUSY: usize = 0x08;
const K_SPECIAL: usize = 0x39;
const K_COURSE: usize = 0x54;
const K_TWO_PLAYER: usize = 0x88;
/// What the course list does on confirm before starting (re/kart.md 1, recipe A).
const MENU_SOUND: usize = 0x01A5_59E8;
const MENU_PANELS: usize = 0x00C7_7F44;

/// The cars: car[0] is ours, 1..5 the CPUs. Work (+0x38): +0x08 rotation, +0x44 is a player,
/// +0x48 kart type, +0x68 extra yaw, +0xBC gate, +0xC4 laps, +0xD4 wheel angle.
const CARS: usize = 0x01D9_6D20;
const CAR_COUNT: usize = 6;
const OBJ_MAIN: usize = 0x10;
const OBJ_DISPLAY: usize = 0x14;
const OBJ_ENTITY: usize = 0x34;
const OBJ_WORK: usize = 0x38;
const E_POSITION: usize = 0x14;
const W_ROTATION: usize = 0x08;
const W_PLAYER: usize = 0x44;
const W_KIND: usize = 0x48;
const W_YAW: usize = 0x68;
const W_GATE: usize = 0xBC;
const W_LAPS: usize = 0xC4;
const W_WHEEL: usize = 0xD4;
/// The car's models (+0x34 drawn far, +0x38 near), texture list and wheels flag.
const W_MODEL_FAR: usize = 0x34;
const W_MODEL_NEAR: usize = 0x38;
const W_TEXLIST: usize = 0x3C;
const W_WHEELS: usize = 0x40;
/// Body tilt from jumps and bumps: the display adds +0xF0/+0xFC, +0xF4/+0x100, +0xF8/+0x104.
const W_TILT: usize = 0xF0;
const W_TILT_2: usize = 0xFC;
/// 1 while the accelerator is held (0x61D8A0): the exhaust burns bright.
const W_ACCEL: usize = 0x114;
const LAPS: i32 = 3;
const GATE_COUNT: usize = 0x01D7_C3C8;
/// The race clock (frames; stopped while held and after the goal), and the post-goal counter
/// (the game leaves at 300).
const RACE_CLOCK: usize = 0x01D9_4428;
const AFTER_GOAL: usize = 0x01A3_6C30;
const AFTER_GOAL_HOLD: u32 = 0x12A;
/// Hold the goal screen at most this long for the others.
const GOAL_WAIT: Duration = Duration::from_secs(60);

/// Kart types (2..=7): the table of models and texlists (0x1C bytes each), and the display.
const KART_TYPES: usize = 0x01D9_6F70;
const KART_TYPE_SIZE: usize = 0x1C;
const KART_DISPLAY: usize = 0x0061_C6C0;
const ENTITY_SIZE: usize = 0x30;
const WORK_SIZE: usize = 0x2F8;
/// Others' karts count this long after their last state.
const KART_STALE: Duration = Duration::from_secs(1);
/// Others' karts are drawn this many of their frames in the past, between two known states
/// (smooth whatever the network timing), and run on a little past the newest when late.
const DELAY_FRAMES: f64 = 6.0;
const EXTRAPOLATE_FRAMES: f64 = 6.0;
/// Their recent states kept (frames).
const KEEP_FRAMES: f64 = 60.0;
/// Our clock for their frames eases toward theirs at this rate, and jumps when this far off.
const CLOCK_EASE: f64 = 0.05;
const CLOCK_SNAP: f64 = 30.0;

#[derive(Clone, Copy)]
struct KartState {
    rotation: [i32; 3],
    yaw: i32,
    wheel: i32,
    kind: u8,
    accel: bool,
    tilt: [i32; 3],
    /// When it came.
    at: Instant,
    /// The sender's frame stamp.
    frame: u16,
}

/// Another player's kart: its latest state, and recent positions by their frame (unwrapped).
struct Other {
    latest: KartState,
    samples: VecDeque<(f64, [f32; 3])>,
    /// The states of `samples` (same order), for the angles at the drawn frame.
    states: VecDeque<KartState>,
    /// Their frame count, unwrapped.
    frames: f64,
    /// The frame we draw them at, and when it was last advanced.
    clock: Option<(f64, Instant)>,
}

/// The position at frame `at` from `samples` (oldest first): between the two around it, a
/// little past the newest along its last motion, or the oldest.
fn sample_at(samples: &VecDeque<(f64, [f32; 3])>, at: f64) -> Option<[f32; 3]> {
    let (&(f0, p0), &(fl, pl)) = (samples.front()?, samples.back()?);
    if at <= f0 {
        return Some(p0);
    }
    if at >= fl {
        let n = samples.len();
        if n < 2 {
            return Some(pl);
        }
        let (fp, pp) = samples[n - 2];
        let t = ((at - fl).min(EXTRAPOLATE_FRAMES) / (fl - fp)) as f32;
        return Some(std::array::from_fn(|i| pl[i] + (pl[i] - pp[i]) * t));
    }
    let i = samples.iter().position(|(f, _)| *f > at)?;
    let ((fa, pa), (fb, pb)) = (samples[i - 1], samples[i]);
    let t = ((at - fa) / (fb - fa)) as f32;
    Some(std::array::from_fn(|k| pa[k] + (pb[k] - pa[k]) * t))
}

/// A kart as sent: position, rotation, extra yaw and wheel angle (BAMS), type (2..=7),
/// accelerator held, body tilt.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MyKart {
    pub position: [f32; 3],
    pub rotation: [i32; 3],
    pub yaw: i32,
    pub wheel: i32,
    pub kind: u8,
    pub accel: bool,
    pub tilt: [i32; 3],
}

struct Kart {
    /// A pick we sent for this start (so it's sent once).
    picked: bool,
    /// A room pick to apply: (course, who).
    incoming: Option<(u8, String)>,
    outgoing: Option<u8>,
    /// CPU cars we switched off.
    hidden: HashSet<usize>,
    /// Our finish: (time in centiseconds, when).
    finish: Option<(u32, Instant)>,
    /// Others' karts as last sent, and where we draw them.
    others: HashMap<u8, Other>,
    display: usize,
    /// Our latest state to send, and the frame counter stamping it.
    mine: Option<(MyKart, u16)>,
    frame: u16,
}

static KART: Mutex<Option<Kart>> = Mutex::new(None);

fn with<R>(f: impl FnOnce(&mut Kart) -> R) -> R {
    let mut k = KART.lock().unwrap_or_else(|e| e.into_inner());
    f(k.get_or_insert_with(|| Kart {
        picked: false,
        incoming: None,
        outgoing: None,
        hidden: HashSet::new(),
        finish: None,
        others: HashMap::new(),
        display: 0,
        mine: None,
        frame: 0,
    }))
}

fn read<T: Copy>(address: usize) -> T {
    // SAFETY: the game's static globals and live objects (callers check pointers).
    unsafe { (address as *const T).read_volatile() }
}

fn write<T>(address: usize, value: T) {
    // SAFETY: as in `read`, on the game thread.
    unsafe { (address as *mut T).write_volatile(value) }
}

/// In a Kart Race (the room battle level).
pub fn in_kart_race() -> bool {
    game::current_level() == KART_LEVEL && !game::two_player()
}

/// Kart `i` (0 = ours), only while the stage runs (in game or paused): the table keeps
/// pointers to freed karts once the race is left (a crash after finishing).
fn car(i: usize) -> usize {
    let state = game::game_state();
    if state != game::GAME_STATE_INGAME && state != game::GAME_STATE_INGAME + 1 {
        return 0;
    }
    read::<usize>(CARS + i * 4)
}

fn our_work() -> Option<usize> {
    let c = car(0);
    (c != 0)
        .then(|| read::<usize>(c + OBJ_WORK))
        .filter(|&w| w != 0)
}

/// Our progress along the race (0..=1): laps and gates.
pub fn progress() -> Option<f32> {
    let w = our_work()?;
    let gates = read::<u32>(GATE_COUNT).max(1) as f32;
    let laps = read::<i32>(w + W_LAPS).clamp(0, LAPS) as f32;
    let gate = read::<i32>(w + W_GATE).max(0) as f32;
    Some(((laps * gates + gate) / (LAPS as f32 * gates)).clamp(0.0, 1.0))
}

/// Our finish time (centiseconds), once we crossed the line after the last lap.
pub fn finished() -> Option<u32> {
    with(|k| k.finish.map(|(t, _)| t))
}

/// Network: someone in our room started a Kart Race on `course`.
pub fn received_pick(course: u8, from: u8, name: &str) {
    if crate::room::shares_room(from) {
        with(|k| k.incoming = Some((course, name.to_string())));
    }
}

/// Network: another player's kart, taken on their game frame `frame`.
pub fn received_state(from: u8, kart: MyKart, frame: u16) {
    let MyKart {
        position,
        rotation,
        yaw,
        wheel,
        kind,
        accel,
        tilt,
    } = kart;
    with(|k| {
        let latest = KartState {
            rotation,
            yaw,
            wheel,
            kind,
            accel,
            tilt,
            at: Instant::now(),
            frame,
        };
        let o = k.others.entry(from).or_insert_with(|| Other {
            latest,
            samples: VecDeque::new(),
            states: VecDeque::new(),
            frames: f64::from(frame),
            clock: None,
        });
        // Frames since their last one: repeats and late arrivals are dropped; a jump back or
        // a long gap starts over (a new race).
        let step = frame.wrapping_sub(o.latest.frame) as i16;
        if !o.samples.is_empty() && step <= 0 && step > -600 {
            return;
        }
        if o.samples.is_empty() || !(1..=120).contains(&step) {
            o.samples.clear();
            o.states.clear();
            o.clock = None;
        } else {
            o.frames += f64::from(step);
        }
        o.latest = latest;
        o.samples.push_back((o.frames, position));
        while o
            .samples
            .front()
            .is_some_and(|(f, _)| o.frames - f > KEEP_FRAMES)
        {
            o.samples.pop_front();
        }
    });
}

/// A course pick to send.
pub fn take_pick() -> Option<u8> {
    with(|k| k.outgoing.take())
}

/// Our kart's latest state, taken on the game thread, and its frame stamp, once each (the
/// network thread sends it).
pub fn take_mine() -> Option<(MyKart, u16)> {
    with(|k| k.mine.take())
}

/// Our kart's state, while racing.
fn my_state() -> Option<MyKart> {
    if !in_kart_race() {
        return None;
    }
    let c = car(0);
    let w = our_work()?;
    let e = read::<usize>(c + OBJ_ENTITY);
    if e == 0 {
        return None;
    }
    let p = read::<Vector>(e + E_POSITION);
    let kind = read::<i32>(w + W_KIND);
    let (a, b) = (read::<[i32; 3]>(w + W_TILT), read::<[i32; 3]>(w + W_TILT_2));
    (2..=7).contains(&kind).then(|| MyKart {
        position: [p.x, p.y, p.z],
        rotation: read::<[i32; 3]>(w + W_ROTATION),
        yaw: read::<i32>(w + W_YAW),
        wheel: read::<i32>(w + W_WHEEL),
        kind: kind as u8,
        accel: read::<i32>(w + W_ACCEL) == 1,
        tilt: [
            a[0].wrapping_add(b[0]),
            a[1].wrapping_add(b[1]),
            a[2].wrapping_add(b[2]),
        ],
    })
}

extern "C" fn nothing(_object: usize) {}

/// Call every frame (game thread). `racing_others`: room players still racing here.
pub fn on_frame(racing_others: bool) {
    let mine = my_state();
    // Our kart skin: its model and textures in our car's work, no wheels (they're the
    // skin's own), every frame of the race (re/skins.md).
    if in_kart_race()
        && let Some(w) = our_work()
        && let Some((model, texlist)) = crate::skins::kart()
    {
        write(w + W_MODEL_FAR, model);
        write(w + W_MODEL_NEAR, model);
        write(w + W_TEXLIST, texlist);
        write(w + W_WHEELS, 0u32);
    }
    with(|k| {
        k.frame = k.frame.wrapping_add(1);
        if let Some(state) = mine {
            k.mine = Some((state, k.frame));
        }
        menu_frame(k);
        if !in_kart_race() {
            k.hidden.clear();
            k.finish = None;
            k.others.clear();
            k.display = 0;
            return;
        }
        if !crate::room::in_room() {
            return;
        }
        if car(0) == 0 {
            // Between runs (a retry makes new karts, maybe at the same addresses).
            k.hidden.clear();
            return;
        }
        // The CPU karts: off, hidden, and never finishing.
        for i in 1..CAR_COUNT {
            let c = car(i);
            if c == 0 || k.hidden.contains(&c) {
                continue;
            }
            let w = read::<usize>(c + OBJ_WORK);
            let e = read::<usize>(c + OBJ_ENTITY);
            if w == 0 || e == 0 || read::<i32>(w + W_PLAYER) != 0 {
                continue;
            }
            write(c + OBJ_MAIN, nothing as extern "C" fn(usize) as usize);
            write(c + OBJ_DISPLAY, nothing as extern "C" fn(usize) as usize);
            write(e + E_POSITION + 4, -100_000.0f32);
            write(w + W_LAPS, -10i32);
            k.hidden.insert(c);
        }
        if k.hidden.len() == CAR_COUNT - 1 && k.display == 0 {
            log!("kart: CPU karts off; racing the room");
        }
        // Our finish, and holding the goal screen while the others still race.
        if let Some(w) = our_work()
            && k.finish.is_none()
            && read::<i32>(w + W_LAPS) >= LAPS
        {
            let frames = read::<u32>(RACE_CLOCK);
            k.finish = Some((frames * 100 / 60, Instant::now()));
            log!("kart: finished in {frames} frames");
        }
        if let Some((_, at)) = k.finish
            && racing_others
            && at.elapsed() < GOAL_WAIT
            && read::<u32>(AFTER_GOAL) > AFTER_GOAL_HOLD
        {
            write(AFTER_GOAL, AFTER_GOAL_HOLD);
        }
        // The others' karts.
        k.others.retain(|_, o| o.latest.at.elapsed() < KART_STALE);
        if k.display == 0 && !k.others.is_empty() {
            k.display = crate::puppet::create_display_object(DISPLAY_NAME, display, deleted);
        }
    });
}

/// Kart menu: send our start as a room pick; follow someone else's.
fn menu_frame(k: &mut Kart) {
    let on_menu = read::<i32>(CURRENT_MENU) == MENU_KART && read::<usize>(KART_MENU) != 0;
    let sub = read::<i32>(SUB_MENU);
    let menu = read::<usize>(KART_MENU);
    if on_menu && sub == SUB_START && read::<u32>(menu + K_BUSY) == 0 {
        if !k.picked && crate::room::in_room() && k.incoming.is_none() {
            let course = read::<i32>(menu + K_COURSE).clamp(0, 2) as u8;
            k.outgoing = Some(course);
            hud::toast("Room: taking everyone along");
            log!("kart: picked course {course}, taking the room along");
        }
        k.picked = true;
    } else if sub != SUB_START {
        k.picked = false;
    }
    let Some((course, who)) = k.incoming.take() else {
        return;
    };
    if !(on_menu && sub == SUB_COURSE_LIST && read::<u32>(menu + K_BUSY) == 0) {
        hud::toast(format!(
            "Room: {who} started a Kart Race (open the Kart Race course list to go along)"
        ));
        return;
    }
    log!("kart: {who} picked course {course}, going along");
    hud::toast(format!("Room: {who} picked a course"));
    write(menu + K_COURSE, i32::from(course));
    write(menu + K_SPECIAL, 0u8);
    write(menu + K_TWO_PLAYER, 0i32);
    write(MENU_SOUND, 4i32);
    let panels = read::<usize>(MENU_PANELS);
    if panels != 0 {
        write(panels, 0i32);
        write(panels + 0x14, 0i32);
    }
    write(SUB_MENU, SUB_START);
    k.picked = true;
}

const DISPLAY_NAME: &[u8] = b"SA2ChaoOnlineKarts\0";

extern "C" fn deleted(_object: usize) {
    let _ = std::panic::catch_unwind(|| with(|k| k.display = 0));
}

#[repr(C, align(16))]
struct Fake {
    object: [u8; 0x50],
    entity: [u8; ENTITY_SIZE],
    work: [u8; WORK_SIZE],
}

static mut FAKE: Fake = Fake {
    object: [0; 0x50],
    entity: [0; ENTITY_SIZE],
    work: [0; WORK_SIZE],
};

/// Draws the others' karts with the game's kart display (re/kart.md 4.3).
extern "C" fn display(_object: usize) {
    crate::watch::mark("kart display");
    let _ = std::panic::catch_unwind(|| {
        let karts: Vec<(KartState, [f32; 3])> = with(|k| {
            k.others
                .values_mut()
                .filter_map(|o| {
                    // Their frame now, as far as we know, minus the delay; our clock runs at
                    // 60 frames a second and eases toward it.
                    let now = Instant::now();
                    let target =
                        o.frames + o.latest.at.elapsed().as_secs_f64() * 60.0 - DELAY_FRAMES;
                    let clock = match o.clock {
                        Some((c, at)) => {
                            let c = c + now.duration_since(at).as_secs_f64() * 60.0;
                            if (target - c).abs() > CLOCK_SNAP {
                                target
                            } else {
                                c + (target - c) * CLOCK_EASE
                            }
                        }
                        None => target,
                    };
                    o.clock = Some((clock, now));
                    // Angles of the last state at or before that frame.
                    let i = o
                        .samples
                        .iter()
                        .rposition(|(f, _)| *f <= clock)
                        .unwrap_or(0);
                    let state = o.states.get(i).copied().unwrap_or(o.latest);
                    Some((state, sample_at(&o.samples, clock)?))
                })
                .collect()
        });
        for (s, at) in karts {
            let entry = KART_TYPES + usize::from(s.kind) * KART_TYPE_SIZE;
            let (far, near, texlist, wheels) = (
                read::<usize>(entry + 4),
                read::<usize>(entry + 8),
                read::<usize>(entry + 0xC),
                read::<u32>(entry + 0x18),
            );
            if far == 0 || near == 0 {
                continue;
            }
            // SAFETY: our private buffers, rebuilt for each kart; the display only reads them
            // (and the shared model's wheel nodes, which it rewrites every draw).
            unsafe {
                let f = &mut *std::ptr::addr_of_mut!(FAKE);
                f.object = [0; 0x50];
                f.entity = [0; ENTITY_SIZE];
                f.work = [0; WORK_SIZE];
                let (o, e, w) = (
                    f.object.as_mut_ptr() as usize,
                    f.entity.as_mut_ptr() as usize,
                    f.work.as_mut_ptr() as usize,
                );
                write(o + OBJ_ENTITY, e);
                write(o + OBJ_WORK, w);
                write(e + E_POSITION, at);
                write(w + W_ROTATION, s.rotation);
                write(w + 0x34, far);
                write(w + 0x38, near);
                write(w + 0x3C, texlist);
                write(w + 0x40, wheels);
                write(w + W_PLAYER, 0i32);
                write(w + W_KIND, i32::from(s.kind));
                write(w + W_YAW, s.yaw);
                write(w + 0x70, -1i32);
                write(w + W_WHEEL, s.wheel);
                write(w + W_TILT, s.tilt);
                write(w + W_ACCEL, i32::from(s.accel));
                std::mem::transmute::<usize, extern "C" fn(usize)>(KART_DISPLAY)(o);
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn karts_are_drawn_between_their_states() {
        let samples: VecDeque<(f64, [f32; 3])> = [
            (0.0, [0.0, 0.0, 0.0]),
            (2.0, [20.0, 0.0, 0.0]),
            (4.0, [40.0, 4.0, 0.0]),
        ]
        .into();
        assert_eq!(sample_at(&samples, -1.0), Some([0.0, 0.0, 0.0]));
        assert_eq!(sample_at(&samples, 1.0), Some([10.0, 0.0, 0.0]));
        assert_eq!(sample_at(&samples, 3.0), Some([30.0, 2.0, 0.0]));
        // Past the newest: on along the last motion, at most EXTRAPOLATE_FRAMES.
        assert_eq!(sample_at(&samples, 6.0), Some([60.0, 8.0, 0.0]));
        assert_eq!(sample_at(&samples, 100.0), Some([100.0, 16.0, 0.0]));
        assert_eq!(sample_at(&VecDeque::new(), 1.0), None);
    }
}
