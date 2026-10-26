//! A look at a Chao without picking it up: in a garden, C (or D-pad up) opens the game's own
//! status balloon (ParamFukidasi, balloon.rs adds our rows) over the Chao in front of player 1.
//! Pressing again moves on to the next nearest Chao not yet shown; after the last one it
//! closes. A balloon closes by itself after `SHOWN`.
//!
//! Any Chao object in the garden counts: ours, and the copies of friends' Chao (garden.rs),
//! whose records carry the server's live values. The game's own balloons (held Chao, offered
//! fruit, absorbed animal) win: one of theirs replacing ours ends our round, and we never open
//! over one of theirs or while the player holds something.
//!
//! Buttons: re/chao-details.md "Controller buttons in the garden".

use std::sync::Mutex;
use std::time::{Duration, Instant};

use crate::game::{self, Vector};
use crate::input::Hotkey;

const VK_C: i32 = 0x43;
/// Player 1's raw pad (`*0x1DEFB60`, filled before ControllerEnabled gates the game's copy):
/// `+0x08` held buttons, `+0x1C`/`+0x1E` stick x/y (i16).
const PAD1: usize = 0x01DE_FB60;
const PAD_HELD: usize = 0x08;
const PAD_STICK_X: usize = 0x1C;
const PAD_STICK_Y: usize = 0x1E;
/// D-pad up. No garden action reads it; the keyboard's movement keys set it too, but with the
/// stick at full tilt, so it counts only while the stick rests (the game's own dead zone).
const BUTTON_UP: u32 = 0x10;
const STICK_DEAD: i16 = 0x27;

/// The current balloon (0 when none) and its data (`obj+0x40`): state byte, Chao object +0x38.
const BALLOON: usize = 0x01AE_D28C;
const BALLOON_DATA: usize = 0x40;
const BALLOON_CHAO: usize = 0x38;
/// Opens the balloon over a Chao (usercall EAX = Chao object; closes any old one first) and
/// closes it (ECX = Chao object: only if the current balloon is over that Chao; state 7).
#[cfg(target_arch = "x86")]
const OPEN: usize = 0x0056_59E0;
#[cfg(target_arch = "x86")]
const CLOSE: usize = 0x0056_5B90;

const OBJ_DATA1: usize = 0x34;
const D_POSITION: usize = 0x14;
/// The Chao's record, which the balloon reads.
const D_RECORD: usize = 0x5C;

/// The first Chao: the nearest in the player's view cone within `AHEAD`, else the nearest
/// within `NEAR` all round.
const AHEAD: f32 = 40.0;
const NEAR: f32 = 25.0;
/// Half the cone (the game's own "in front" for the action button, 0x486BB0: 0x2000 BAMS).
const CONE_COS: f32 = std::f32::consts::FRAC_1_SQRT_2;
/// How long a balloon of ours stays up.
const SHOWN: Duration = Duration::from_secs(6);

struct Peek {
    /// The balloon object we opened and the Chao it's over (compared, never read once gone).
    balloon: usize,
    chao: usize,
    since: Instant,
    /// Chao shown this round.
    shown: Vec<usize>,
}

struct State {
    key: Hotkey,
    pad_up: bool,
    peek: Option<Peek>,
}

static STATE: Mutex<State> = Mutex::new(State {
    key: Hotkey::new(VK_C),
    pad_up: false,
    peek: None,
});

/// Where the player is and which way it faces (x, z), as the game's "in front" test has it:
/// a target straight ahead is at atan2(dx, dz) = 0x4000 − rotY.
fn forward(rot_y: i32) -> (f32, f32) {
    let a = rot_y as f32 * (std::f32::consts::TAU / 65536.0);
    (a.cos(), a.sin())
}

fn distance(a: Vector, b: Vector) -> f32 {
    ((a.x - b.x).powi(2) + (a.y - b.y).powi(2) + (a.z - b.z).powi(2)).sqrt()
}

/// The Chao to show first: (object, position) candidates, the player's position and facing.
fn first(chao: &[(usize, Vector)], at: Vector, rot_y: i32) -> Option<usize> {
    let (fx, fz) = forward(rot_y);
    let nearest = |pick: &dyn Fn(Vector, f32) -> bool| {
        chao.iter()
            .map(|&(o, p)| (o, p, distance(at, p)))
            .filter(|&(_, p, d)| pick(p, d))
            .min_by(|a, b| a.2.total_cmp(&b.2))
            .map(|(o, _, _)| o)
    };
    let ahead = |p: Vector, d: f32| {
        let (dx, dz) = (p.x - at.x, p.z - at.z);
        let flat = (dx * dx + dz * dz).sqrt();
        d <= AHEAD && (flat < 1.0 || (dx * fx + dz * fz) / flat >= CONE_COS)
    };
    nearest(&ahead).or_else(|| nearest(&|_, d| d <= NEAR))
}

/// The next Chao this round: the nearest not shown yet.
fn next(chao: &[(usize, Vector)], at: Vector, shown: &[usize]) -> Option<usize> {
    chao.iter()
        .filter(|(o, _)| !shown.contains(o))
        .min_by(|a, b| distance(at, a.1).total_cmp(&distance(at, b.1)))
        .map(|&(o, _)| o)
}

/// The garden's Chao objects the balloon can show (Data1 and record set), with positions.
fn garden_chao() -> Vec<(usize, Vector)> {
    crate::registry::entries(crate::registry::CHAO)
        .into_iter()
        .filter_map(|(_, object)| {
            // SAFETY: a live registry entry's object and its Data1 (null-checked), on the game
            // thread.
            unsafe {
                let d = ((object + OBJ_DATA1) as *const usize).read();
                if d == 0 || ((d + D_RECORD) as *const usize).read() == 0 {
                    return None;
                }
                Some((object, ((d + D_POSITION) as *const Vector).read_unaligned()))
            }
        })
        .collect()
}

fn current_balloon() -> usize {
    // SAFETY: the game's static balloon pointer.
    unsafe { (BALLOON as *const usize).read_volatile() }
}

/// The balloon showing now is still ours (same object, over the same Chao).
fn still_ours(peek: &Peek) -> bool {
    let balloon = current_balloon();
    balloon != 0
        && balloon == peek.balloon
        // SAFETY: the current balloon is a live object while the game's global holds it; its
        // data is set at creation (null-checked).
        && unsafe {
            let d = ((balloon + BALLOON_DATA) as *const usize).read();
            d != 0 && ((d + BALLOON_CHAO) as *const usize).read() == peek.chao
        }
}

/// Opens the balloon over `chao` (a live garden Chao with a record); the balloon object.
#[cfg(target_arch = "x86")]
fn open(chao: usize) -> usize {
    // SAFETY: the game's usercall (EAX = Chao object; saves EBX/ESI/EDI) on the game thread,
    // for a Chao the registry lists with Data1 and record set (checked by the caller).
    unsafe {
        std::arch::asm!(
            "call {f}",
            f = in(reg) OPEN,
            inout("eax") chao => _,
            clobber_abi("C"),
        );
    }
    current_balloon()
}

#[cfg(not(target_arch = "x86"))]
fn open(_chao: usize) -> usize {
    0
}

/// Closes the balloon if it's over `chao` (only compares the pointer).
#[cfg(target_arch = "x86")]
fn close(chao: usize) {
    // SAFETY: the game's close (ECX = Chao object, compared to the current balloon's; uses only
    // EAX), on the game thread.
    unsafe {
        std::arch::asm!(
            "call {f}",
            f = in(reg) CLOSE,
            inout("ecx") chao => _,
            out("eax") _,
            clobber_abi("C"),
        );
    }
}

#[cfg(not(target_arch = "x86"))]
fn close(_chao: usize) {}

/// D-pad up held on player 1's pad, with the stick at rest.
fn pad_up_held() -> bool {
    // SAFETY: the game's pad pointer (null-checked) and its fields.
    unsafe {
        let pad = (PAD1 as *const usize).read_volatile();
        if pad == 0 {
            return false;
        }
        let held = ((pad + PAD_HELD) as *const u32).read_volatile();
        let x = ((pad + PAD_STICK_X) as *const i16).read_volatile();
        let y = ((pad + PAD_STICK_Y) as *const i16).read_volatile();
        held & BUTTON_UP != 0 && x.abs() <= STICK_DEAD && y.abs() <= STICK_DEAD
    }
}

/// The player can look now: in a garden, playing, controls on, no menu or chat, hands free.
fn can_peek(menu_open: bool) -> bool {
    game::in_garden()
        && game::game_state() == game::GAME_STATE_INGAME
        // SAFETY: a static byte in the game's data section.
        && unsafe { (game::CONTROLLER_ENABLED as *const u8).read_volatile() } != 0
        && !menu_open
        && !crate::chat::typing()
        && !crate::search::keyboard_taken()
        && game::local_player().is_some_and(|p| !p.holding)
}

/// Call every frame on the game thread (after the F9 menu and chat).
pub fn on_frame(menu_open: bool) {
    let mut s = STATE.lock().unwrap_or_else(|e| e.into_inner());
    // Keep both edges current every frame so an old press never fires later.
    let key = s.key.pressed();
    let up = pad_up_held();
    let pad = up && !s.pad_up;
    s.pad_up = up;

    if !game::in_garden() {
        s.peek = None;
        return;
    }
    if let Some(peek) = &s.peek {
        let alive = garden_chao().iter().any(|&(o, _)| o == peek.chao);
        if !still_ours(peek) {
            // Closed, or the game opened its own.
            s.peek = None;
        } else if !alive || peek.since.elapsed() >= SHOWN {
            close(peek.chao);
            s.peek = None;
        }
    }
    if !(key || pad) || !can_peek(menu_open) {
        return;
    }
    let Some((at, rot_y)) = game::player_position() else {
        return;
    };
    let chao = garden_chao();
    let (target, mut shown) = match s.peek.take() {
        Some(peek) => match next(&chao, at, &peek.shown) {
            Some(target) => (target, peek.shown),
            None => {
                close(peek.chao);
                return;
            }
        },
        // Never over one of the game's own.
        None if current_balloon() != 0 => return,
        None => match first(&chao, at, rot_y) {
            Some(target) => (target, Vec::new()),
            None => return,
        },
    };
    let balloon = open(target);
    if balloon != 0 {
        shown.push(target);
        s.peek = Some(Peek {
            balloon,
            chao: target,
            since: Instant::now(),
            shown,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(x: f32, z: f32) -> Vector {
        Vector { x, y: 0.0, z }
    }

    #[test]
    fn facing_matches_the_games_in_front_test() {
        // A target at atan2(dx, dz) = 0x4000 − rotY is straight ahead.
        for rot in [0, 0x2000, 0x4000, 0x9000, -0x3000] {
            let (fx, fz) = forward(rot);
            let want = (0x4000 - rot) as f32 * std::f32::consts::TAU / 65536.0;
            let got = fx.atan2(fz);
            let diff = (got - want).rem_euclid(std::f32::consts::TAU);
            assert!(diff < 1e-3 || diff > std::f32::consts::TAU - 1e-3, "{rot}");
        }
    }

    #[test]
    fn first_prefers_the_chao_in_front() {
        // Facing +x (rotY 0).
        let chao = [(1, v(-10.0, 0.0)), (2, v(30.0, 5.0)), (3, v(0.0, 35.0))];
        assert_eq!(first(&chao, v(0.0, 0.0), 0), Some(2));
        // Nothing in front: the nearest all round, if close enough.
        let behind = [(1, v(-20.0, 0.0)), (2, v(-30.0, 0.0))];
        assert_eq!(first(&behind, v(0.0, 0.0), 0), Some(1));
        assert_eq!(first(&[(1, v(-26.0, 0.0))], v(0.0, 0.0), 0), None);
        // Out of reach ahead.
        assert_eq!(first(&[(1, v(41.0, 0.0))], v(0.0, 0.0), 0), None);
    }

    #[test]
    fn next_goes_by_distance_and_skips_the_shown() {
        let chao = [(1, v(5.0, 0.0)), (2, v(50.0, 0.0)), (3, v(-20.0, 0.0))];
        assert_eq!(next(&chao, v(0.0, 0.0), &[1]), Some(3));
        assert_eq!(next(&chao, v(0.0, 0.0), &[1, 3]), Some(2));
        assert_eq!(next(&chao, v(0.0, 0.0), &[1, 2, 3]), None);
    }
}
