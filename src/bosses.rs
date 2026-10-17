//! Co-op rooms: one boss HP for everyone (re/bosses.md).
//!
//! Every boss shows its HP through the boss gauge, which holds a pointer to the boss's own HP
//! float. When ours drops (our hit) we send the new value; a lower value from a friend lowers ours.
//! It never goes to 0 from outside: the game only defeats a boss inside its hit handler, so the
//! last hit is always a real one in each player's game, and the bar is left with a sliver.

use std::sync::Mutex;
use std::time::{Duration, Instant};

use crate::game;
use crate::log;

const OBJECT_LISTS: usize = 0x01A5_A254;
const LISTS: usize = 8;
const OBJ_NEXT: usize = 0x04;
const OBJ_CHILD: usize = 0x0C;
const OBJ_MAIN: usize = 0x10;
const OBJ_DATA1: usize = 0x34;
/// "BossPowerGuageExec": Data1 +8 is the boss's `float *hp`, +0x28 the HP it started with.
const GAUGE_MAIN: usize = 0x0076_1C30;
const GAUGE_HP: usize = 0x08;
const GAUGE_FULL: usize = 0x28;
/// Rival fights: the rival is player 2 and its life is CharObj2 +0x48 (the mech fights' gauge
/// shows a half-size copy, so use the real one). Boss stages have no player 2.
const PLAYER_2_CHAR_OBJ2: usize = 0x01DE_9604;
const RIVAL_LIFE: usize = 0x48;
/// Looked for this often until a stage's gauge shows up (it's made a little after the start).
const SEARCH_EVERY: u32 = 30;
/// What's left of a bar a friend emptied: the next real hit finishes it.
const SLIVER: f32 = 0.01;
/// Resent this often while the boss is hurt, for players who arrive later (or died: a restart
/// brings the boss back whole).
const RESEND: Duration = Duration::from_secs(2);
/// Sent at most this often (Final Hazard takes a small hit every frame).
const SEND_GAP: Duration = Duration::from_millis(100);

struct Bosses {
    stage: Option<(i16, u8)>,
    /// The gauge object, once found, and frames since we last looked for it.
    gauge: usize,
    frames: u32,
    /// HP as of last frame (after our writes), the bar's full value, and the HP when we found
    /// it (the mech rivals' bar shows half their life).
    last: Option<f32>,
    full: f32,
    start: f32,
    /// Our hit, not sent yet.
    unsent: bool,
    /// A friend's lowest HP for this boss: (hp, full).
    incoming: Option<(f32, f32)>,
    outgoing: Option<(i16, u8, f32, f32)>,
    sent: Option<Instant>,
}

static BOSSES: Mutex<Bosses> = Mutex::new(Bosses::new());

impl Bosses {
    const fn new() -> Self {
        Bosses {
            stage: None,
            gauge: 0,
            frames: 0,
            last: None,
            full: 0.0,
            start: 0.0,
            unsent: false,
            incoming: None,
            outgoing: None,
            sent: None,
        }
    }
}

fn lock() -> std::sync::MutexGuard<'static, Bosses> {
    BOSSES.lock().unwrap_or_else(|e| e.into_inner())
}

/// Network: a room player's boss is down to `hp` (of `full`).
pub fn received(level: i16, mission: u8, hp: f32, full: f32) {
    let mut b = lock();
    if b.stage == Some((level, mission)) {
        let lower = b.incoming.is_none_or(|(h, f)| f != full || hp < h);
        if lower {
            b.incoming = Some((hp, full));
        }
    }
}

/// Our boss HP to send: (level, mission, hp, full).
pub fn take_outgoing() -> Option<(i16, u8, f32, f32)> {
    lock().outgoing.take()
}

/// The boss gauge's `float *hp` and full value, if `object` is a gauge.
fn gauge(object: usize) -> Option<(usize, f32)> {
    // SAFETY: a live object (from the game's lists, or our cached gauge checked by its main
    // first); Data1 checked for null.
    unsafe {
        if ((object + OBJ_MAIN) as *const usize).read() != GAUGE_MAIN {
            return None;
        }
        let d1 = ((object + OBJ_DATA1) as *const usize).read();
        if d1 == 0 {
            return None;
        }
        let hp = ((d1 + GAUGE_HP) as *const usize).read();
        (hp != 0).then(|| (hp, ((d1 + GAUGE_FULL) as *const f32).read()))
    }
}

/// Calls `f` on each object of the list starting at `head` (circular through +4) until it
/// returns something.
fn walk(head: usize, f: &mut dyn FnMut(usize) -> Option<usize>) -> Option<usize> {
    let mut object = head;
    for _ in 0..4096 {
        if object == 0 {
            break;
        }
        if let Some(found) = f(object) {
            return Some(found);
        }
        // SAFETY: a live object's next pointer, on the game thread.
        object = unsafe { ((object + OBJ_NEXT) as *const usize).read() };
        if object == head {
            break;
        }
    }
    None
}

/// The gauge object in the object lists or among their objects' children.
fn find_gauge() -> Option<usize> {
    let is_gauge = |o: usize| gauge(o).map(|_| o);
    (0..LISTS).find_map(|n| {
        // SAFETY: the game's static list heads.
        let head = unsafe { ((OBJECT_LISTS + n * 4) as *const usize).read() };
        walk(head, &mut |object| {
            is_gauge(object).or_else(|| {
                // SAFETY: as in `walk`.
                let child = unsafe { ((object + OBJ_CHILD) as *const usize).read() };
                walk(child, &mut |c| is_gauge(c))
            })
        })
    })
}

/// The boss's HP float and its bar's full value: the rival's life in a rival fight, else what
/// the gauge shows.
fn boss_hp(gauge_object: usize) -> Option<(usize, f32)> {
    let (hp, full) = gauge(gauge_object)?;
    // SAFETY: the static player pointer array; null without a player 2.
    let rival = unsafe { (PLAYER_2_CHAR_OBJ2 as *const usize).read_volatile() };
    Some(if rival != 0 {
        (rival + RIVAL_LIFE, full)
    } else {
        (hp, full)
    })
}

/// Call every frame (game thread).
pub fn on_frame(connected: bool) {
    let mut b = lock();
    let stage = (game::current_level(), game::mission());
    let coop_room = crate::room::in_room() && crate::room::mode() == crate::room::Mode::Coop;
    if !connected || !coop_room || b.stage.is_some_and(|s| s != stage) {
        *b = Bosses::new();
    }
    // Paused, dying or loading: keep what we know; the gauge is checked again after.
    if !crate::coop::active(connected) {
        return;
    }
    b.stage = Some(stage);
    // The gauge we found, while it's still one; else look again now and then.
    if b.gauge != 0 && gauge(b.gauge).is_none() {
        b.gauge = 0;
        b.last = None;
    }
    if b.gauge == 0 {
        b.frames += 1;
        if b.frames < SEARCH_EVERY {
            return;
        }
        b.frames = 0;
        let Some(found) = find_gauge() else {
            return;
        };
        b.gauge = found;
    }
    let Some((at, full)) = boss_hp(b.gauge) else {
        return;
    };
    // SAFETY: the boss's live HP float (from the gauge or the rival's CharObj2).
    let mut hp = unsafe { (at as *const f32).read_volatile() };
    if !hp.is_finite() {
        return;
    }
    if b.full != full {
        log!("bosses: boss up, hp {hp} of {full}");
        b.full = full;
        b.start = hp;
    }
    // Our hit (lower than we left it), or a resend while it's hurt.
    if b.last.is_some_and(|last| hp < last) {
        b.unsent = true;
    }
    let since = b.sent.map(|at| at.elapsed());
    let due = if b.unsent {
        since.is_none_or(|s| s >= SEND_GAP)
    } else {
        hp < b.start && since.is_some_and(|s| s >= RESEND)
    };
    if due && hp > 0.0 {
        b.outgoing = Some((stage.0, stage.1, hp, full));
        b.sent = Some(Instant::now());
        b.unsent = false;
    }
    // A friend's hit: down to theirs, never to 0 (only the boss's own hit handler defeats it).
    if let Some((theirs, their_full)) = b.incoming.take()
        && their_full == full
        && hp > 0.0
    {
        let to = theirs.max(SLIVER);
        if to < hp {
            // SAFETY: as above, on the game thread.
            unsafe { (at as *mut f32).write_volatile(to) };
            log!("bosses: a friend's hit: hp {hp} -> {to}");
            hp = to;
            b.sent.get_or_insert_with(Instant::now);
        }
    }
    b.last = Some(hp);
}
