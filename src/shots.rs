//! Co-op rooms: the mechs' shots show in everyone's game (re/characters.md "Walker shots").
//!
//! A walker's bullet and bazooka shell are objects in list 3 (`WalkerBulletExecutor`, main
//! 0x75D520; `WalkerBazookaExecutor`, main 0x75D9F0) with the position at Data1+0x14 and the
//! direction at Data1+0x20. New ones in our game are sent; a friend's are fired here with the
//! game's own creators. These are real shots that can hit things, so only in Co-op, where what
//! anyone destroys is gone for everyone anyway.

use std::sync::Mutex;

use crate::game::Vector;

const OBJECT_LIST_3: usize = 0x01A5_A254 + 3 * 4;
const OBJ_NEXT: usize = 0x04;
const OBJ_MAIN: usize = 0x10;
const OBJ_DATA1: usize = 0x34;
const D_POSITION: usize = 0x14;
const D_DIRECTION: usize = 0x20;
const BULLET_MAIN: usize = 0x0075_D520;
const BAZOOKA_MAIN: usize = 0x0075_D9F0;
/// Creators: bullet (EAX parent or 0, ECX &direction, stack &position, caller pops),
/// bazooka (EAX parent or 0, EBX &position, stack &direction).
const FIRE_BULLET: usize = 0x0075_D7D0;
const FIRE_BAZOOKA: usize = 0x0075_DCA0;

/// A shot: kind (0 bullet, 1 bazooka), where from, which way.
pub type Shot = (u8, [f32; 3], [f32; 3]);

struct Shots {
    /// Shot objects already here last frame (ours sent, friends' fired by us).
    seen: Vec<usize>,
    incoming: Vec<Shot>,
    outgoing: Vec<Shot>,
}

static SHOTS: Mutex<Shots> = Mutex::new(Shots {
    seen: Vec::new(),
    incoming: Vec::new(),
    outgoing: Vec::new(),
});

/// Shot objects in list 3 now: (object, kind).
fn live_shots() -> Vec<(usize, u8)> {
    let mut out = Vec::new();
    // SAFETY: the game's object list 3 (circular through +4), read on the game thread.
    unsafe {
        let head = (OBJECT_LIST_3 as *const usize).read();
        let mut object = head;
        for _ in 0..4096 {
            if object == 0 {
                break;
            }
            match ((object + OBJ_MAIN) as *const usize).read() {
                BULLET_MAIN => out.push((object, 0)),
                BAZOOKA_MAIN => out.push((object, 1)),
                _ => {}
            }
            object = ((object + OBJ_NEXT) as *const usize).read();
            if object == head {
                break;
            }
        }
    }
    out
}

/// Network: a room player in our stage fired.
pub fn received(shot: Shot) {
    let mut s = SHOTS.lock().unwrap_or_else(|e| e.into_inner());
    if s.incoming.len() < 64 {
        s.incoming.push(shot);
    }
}

/// Our shots to send.
pub fn take_outgoing() -> Vec<Shot> {
    std::mem::take(&mut SHOTS.lock().unwrap_or_else(|e| e.into_inner()).outgoing)
}

/// Call every frame (game thread).
pub fn on_frame(connected: bool) {
    let mut s = SHOTS.lock().unwrap_or_else(|e| e.into_inner());
    if !crate::coop::active(connected) {
        s.seen.clear();
        s.incoming.clear();
        return;
    }
    // New since last frame: fired by our mech this frame.
    let now = live_shots();
    for &(object, kind) in &now {
        if s.seen.contains(&object) {
            continue;
        }
        // SAFETY: a live shot object's Data1.
        unsafe {
            let d = ((object + OBJ_DATA1) as *const usize).read();
            if d != 0 {
                let p = ((d + D_POSITION) as *const Vector).read_unaligned();
                let v = ((d + D_DIRECTION) as *const Vector).read_unaligned();
                s.outgoing.push((kind, [p.x, p.y, p.z], [v.x, v.y, v.z]));
            }
        }
    }
    // Friends' shots, fired here; then everything here now counts as seen, so those aren't
    // sent back. Only while we're a mech too: shots draw with the mech's models and textures,
    // which the game loads only for a walker player.
    let walker =
        crate::game::local_player().is_some_and(|p| matches!(p.character, 6 | 7 | 10 | 11));
    for (kind, p, v) in std::mem::take(&mut s.incoming) {
        if walker {
            fire(kind, p, v);
        }
    }
    s.seen = live_shots().into_iter().map(|(o, _)| o).collect();
}

fn fire(kind: u8, position: [f32; 3], direction: [f32; 3]) {
    let (p, v) = (
        Vector {
            x: position[0],
            y: position[1],
            z: position[2],
        },
        Vector {
            x: direction[0],
            y: direction[1],
            z: direction[2],
        },
    );
    let (pp, vp) = (&raw const p as usize, &raw const v as usize);
    #[cfg(target_arch = "x86")]
    // SAFETY: the game's shot creators as its walker code calls them (see the constants), on
    // the game thread, with pointers to our stack copies; no parent (EAX = 0).
    unsafe {
        if kind == 0 {
            std::arch::asm!(
                "push {pos}",
                "call {f}",
                "add esp, 4",
                pos = in(reg) pp,
                f = in(reg) FIRE_BULLET,
                inout("eax") 0usize => _,
                inout("ecx") vp => _,
                clobber_abi("C"),
            );
        } else {
            std::arch::asm!(
                "push ebx",
                "mov ebx, ecx",
                "push edx",
                "call edi",
                "add esp, 4",
                "pop ebx",
                inout("ecx") pp => _,
                inout("edx") vp => _,
                inout("edi") FIRE_BAZOOKA => _,
                inout("eax") 0usize => _,
                clobber_abi("C"),
            );
        }
    }
    #[cfg(not(target_arch = "x86"))]
    let _ = (kind, pp, vp, FIRE_BULLET, FIRE_BAZOOKA);
}
