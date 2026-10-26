//! Co-op rooms: one boss HP for everyone (re/bosses.md).
//!
//! Every boss shows its HP through the boss gauge, which holds a pointer to the boss's own HP
//! float. When ours drops (our hit) we send the new value; a lower value from a friend lowers ours.
//! Finishing hits are broadcast too. King Boom Boo enters his native defeat sequence on every
//! client; bosses without a verified remote defeat path still wait for a local final hit.

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
const OBJ_PHYSICS: usize = 0x38;
const OBJ_WORK: usize = 0x40;
const BOO_MAIN: usize = 0x0061_29C0;
const BOO_SET_MODE: usize = 0x0060_CE40;
const BOO_HP: usize = 0x28;
const BOO_DEFEAT: u8 = 12;
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
    defeated: bool,
    completion: Option<(i16, u8)>,
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
            defeated: false,
            completion: None,
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

/// All retail boss arenas, including both Golems and the five character rival fights.
pub fn is_boss_stage(level: i16) -> bool {
    matches!(level, 19 | 20 | 29 | 33 | 42 | 60..=67)
}

pub fn should_share_clear(
    level: i16,
    party_member: bool,
    cooperative: bool,
    finished: bool,
) -> bool {
    finished && party_member && cooperative && is_boss_stage(level)
}

/// The existing reliable RaceStatus finish is a confirmed native clear, unlike an HP phase.
/// The network caller checks that the sender is in our co-op party.
pub fn received_clear(level: i16, mission: u8) {
    if is_boss_stage(level) {
        let mut b = lock();
        if b.stage.is_none_or(|stage| stage == (level, mission)) {
            b.completion = Some((level, mission));
        }
    }
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
    find_live(|o| gauge(o).is_some())
}

fn find_live(mut matches: impl FnMut(usize) -> bool) -> Option<usize> {
    (0..LISTS).find_map(|n| {
        // SAFETY: the game's static list heads.
        let head = unsafe { ((OBJECT_LISTS + n * 4) as *const usize).read() };
        walk(head, &mut |object| {
            matches(object).then_some(object).or_else(|| {
                // SAFETY: as in `walk`.
                let child = unsafe { ((object + OBJ_CHILD) as *const usize).read() };
                walk(child, &mut |c| matches(c).then_some(c))
            })
        })
    })
}

/// Run exactly the native last-hit setup, then let mode 12 animate, clear and save normally.
/// Never call the hit handler itself: it also changes the local player's physics/camera.
fn finish_boo(hp_slot: usize) -> bool {
    if !cfg!(all(windows, target_arch = "x86")) {
        return false;
    }
    let Some(object) = find_live(|object| unsafe {
        if ((object + OBJ_MAIN) as *const usize).read() != BOO_MAIN {
            return false;
        }
        let work = ((object + OBJ_WORK) as *const usize).read();
        work != 0 && work + BOO_HP == hp_slot
    }) else {
        return false;
    };
    // SAFETY: the boss was found in this frame's live lists; all required native work is
    // checked, including the HP pointer identifying the current gauge and the helper bytes.
    unsafe {
        let data = ((object + OBJ_DATA1) as *const usize).read();
        let physics = ((object + OBJ_PHYSICS) as *const usize).read();
        let work = ((object + OBJ_WORK) as *const usize).read();
        if data == 0
            || physics == 0
            || ((data + 0x2C) as *const usize).read() == 0
            || ((work + 0x10) as *const usize).read() == 0
            || ((work + 0x0C) as *const usize).read() == 0
        {
            return false;
        }
        if (data as *const u8).read() == BOO_DEFEAT {
            return true;
        }
        if std::slice::from_raw_parts(BOO_SET_MODE as *const u8, 8)
            != [0x55, 0x8B, 0xEC, 0x83, 0xEC, 0x14, 0x8B, 0x45]
        {
            return false;
        }
        (hp_slot as *mut f32).write_volatile(0.0);
        (physics as *mut [f32; 6]).write([0.0, 1.0, 0.0, 0.0, 0.0, 0.0]);
        disable_boo_collision(object);
        let flags = (data + 4) as *mut u16;
        flags.write_unaligned(flags.read_unaligned() & !0x804);
        std::mem::transmute::<usize, extern "C" fn(usize, u8)>(BOO_SET_MODE)(object, BOO_DEFEAT);
    }
    log!("bosses: King Boom Boo defeated by a friend; running native ending");
    true
}

#[cfg(target_arch = "x86")]
unsafe fn disable_boo_collision(object: usize) {
    // REA: native finishing hit loops over all eleven collision slots. Both helpers have
    // one caller-popped stack argument; index in EAX, object in ECX / EDX respectively.
    for index in 0..11usize {
        unsafe {
            std::arch::asm!("push 0", "call edx", "add esp, 4",
                in("eax") index, in("ecx") object, in("edx") 0x0048_6A60usize,
                clobber_abi("C"));
            std::arch::asm!("push 3", "call ecx", "add esp, 4",
                in("eax") index, in("edx") object, in("ecx") 0x0048_6A80usize,
                clobber_abi("C"));
        }
    }
}

#[cfg(not(target_arch = "x86"))]
unsafe fn disable_boo_collision(_object: usize) {}

fn hp_announcement(last: Option<f32>, hp: f32, defeated: bool, due: bool) -> Option<f32> {
    let finishing_hit = hp <= 0.0 && !defeated && last.is_some_and(|last| last > 0.0);
    (finishing_hit || due).then_some(hp.max(0.0))
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
        let completion = b
            .completion
            .filter(|s| connected && coop_room && *s == stage);
        *b = Bosses::new();
        b.completion = completion;
    }
    // Paused, dying or loading: keep what we know; the gauge is checked again after.
    if !crate::coop::active(connected) {
        return;
    }
    b.stage = Some(stage);
    if b.completion == Some(stage) {
        if game::stage_cleared() {
            b.completion = None;
        } else if game::complete_shared_boss() {
            b.completion = None;
            log!(
                "bosses: party cleared level {}; native victory/results started",
                stage.0
            );
            return;
        }
    } else if b.completion.is_some() {
        b.completion = None;
    }
    if game::stage_cleared() {
        return;
    }
    // The gauge we found, while it's still one; else look again now and then.
    // Never dereference a cached gauge after its native object has been freed.
    if b.gauge != 0 && (find_live(|o| o == b.gauge).is_none() || gauge(b.gauge).is_none()) {
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
    if !hp.is_finite() || !full.is_finite() || full <= 0.0 {
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
    if let Some(announcement) = hp_announcement(b.last, hp, b.defeated, due) {
        b.outgoing = Some((stage.0, stage.1, announcement, full));
        b.sent = Some(Instant::now());
        b.unsent = false;
        if hp <= 0.0 && !b.defeated {
            log!("bosses: finishing hit shared for level {}", stage.0);
        }
        b.defeated |= hp <= 0.0;
    }
    // Shared damage, with Boo's traced native defeat path. Other bosses finish from the
    // confirmed stage-clear message rather than treating a potentially intermediate HP phase as victory.
    if let Some((theirs, their_full)) = b.incoming.take()
        && their_full == full
        && hp > 0.0
    {
        if theirs <= 0.0 && stage.0 == 63 && finish_boo(at) {
            hp = 0.0;
            b.defeated = true;
        } else {
            let to = theirs.max(SLIVER);
            if to < hp {
                // SAFETY: as above, on the game thread.
                unsafe { (at as *mut f32).write_volatile(to) };
                log!("bosses: a friend's hit: hp {hp} -> {to}");
                hp = to;
                b.sent.get_or_insert_with(Instant::now);
            }
            if theirs <= 0.0 && stage.0 == 63 {
                b.incoming = Some((theirs, their_full)); // retry if native work was not ready yet
            }
        }
    }
    b.last = Some(hp);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finishing_hits_are_sent_immediately_including_negative_native_hp() {
        assert_eq!(hp_announcement(Some(1.0), 0.0, false, false), Some(0.0));
        assert_eq!(hp_announcement(Some(0.01), -0.99, false, false), Some(0.0));
        assert_eq!(hp_announcement(Some(2.0), 1.0, false, false), None);
        assert_eq!(hp_announcement(Some(2.0), 1.0, false, true), Some(1.0));
    }

    #[test]
    fn boss_intro_zero_is_not_a_defeat_and_terminal_resends_remain_possible() {
        assert_eq!(hp_announcement(None, 0.0, false, false), None);
        assert_eq!(hp_announcement(Some(0.0), 0.0, false, false), None);
        assert_eq!(hp_announcement(Some(1.0), 0.0, true, false), None);
        assert_eq!(hp_announcement(Some(0.0), 0.0, true, true), Some(0.0));
    }

    #[test]
    fn confirmed_party_wins_finish_bosses_without_finishing_races_or_hunts() {
        for level in [63, 64, 67, 65, 66, 19] {
            assert!(should_share_clear(level, true, true, true));
            assert!(!should_share_clear(level, false, true, true)); // another party
            assert!(!should_share_clear(level, true, false, true)); // competitive race
            assert!(!should_share_clear(level, true, true, false)); // still fighting
        }
        assert!(!should_share_clear(25, true, true, true)); // hunt keeps its own shared pieces
        assert!(!should_share_clear(23, true, true, true)); // co-op platforming is not auto-finished
    }
}
