//! Party players share the Hero and Dark Gardens (re/chao.md "Garden unlocks").
//!
//! A garden opens when one of the save's Chao first evolves toward it (0x568630, the
//! evolution: Hero sets bit 0x10, Dark 0x40 of the Chao save's flags at `0x19F646C` + 0xCA6C x
//! the save in use, `0x173D06C`; with both open it also unlocks Steam achievement 7). The lobby
//! makes each garden's gate from those bits (0x54C162 / 0x54C1B2). Ours are sent to the party
//! (`GardenUnlocks`) when they change and now and then; a party player's open gardens open in our
//! save too (kept with it when the game next saves the Chao World).

use std::sync::Mutex;

use crate::net::protocol::{GARDEN_DARK, GARDEN_HERO};

const CHAO_SAVE_FLAGS: usize = 0x019F_646C;
const CHAO_SAVE_SIZE: usize = 0xCA6C;
const CHAO_SAVE_IN_USE: usize = 0x0173_D06C;
const GARDENS: u8 = GARDEN_HERO | GARDEN_DARK;
/// Ours go out again this often (frames), for party players who joined since.
const SEND_EVERY: u32 = 600;

struct Unlocks {
    sent: u8,
    frame: u32,
    incoming: u8,
}

static UNLOCKS: Mutex<Unlocks> = Mutex::new(Unlocks {
    sent: 0,
    frame: 0,
    incoming: 0,
});

/// The flags dword of the Chao save in use (the game picks save 1 or 0 the same way).
fn flags_at() -> usize {
    // SAFETY: the game's static "which Chao save" value.
    let save = unsafe { (CHAO_SAVE_IN_USE as *const u32).read_volatile() };
    CHAO_SAVE_FLAGS + if save == 1 { CHAO_SAVE_SIZE } else { 0 }
}

fn gardens() -> u8 {
    // SAFETY: the Chao save's flags, in the game's static save data.
    (unsafe { (flags_at() as *const u32).read_volatile() } as u8) & GARDENS
}

/// One of our Chao evolved toward `garden` (`GARDEN_HERO`, `GARDEN_DARK`) while the server
/// ran it (held.rs): our save opens that garden, as the evolution does in the game (game
/// thread).
pub fn open(garden: u8) {
    let at = flags_at() as *mut u32;
    // SAFETY: as in `gardens`, on the game thread; the evolution code sets them the same way.
    unsafe { at.write_volatile(at.read_volatile() | u32::from(garden & GARDENS)) };
}

/// Network: a party player's open gardens.
pub fn received(gardens: u8) {
    UNLOCKS.lock().unwrap_or_else(|e| e.into_inner()).incoming |= gardens & GARDENS;
}

/// Call every frame (game thread) with whether we're in a party. Returns our open gardens to
/// send, when they changed or now and then.
pub fn on_frame(in_party: bool) -> Option<u8> {
    let mut u = UNLOCKS.lock().unwrap_or_else(|e| e.into_inner());
    u.frame = u.frame.wrapping_add(1);
    let incoming = std::mem::take(&mut u.incoming);
    if !in_party {
        return None;
    }
    let ours = gardens();
    let new = incoming & !ours;
    if new != 0 {
        let at = flags_at() as *mut u32;
        // SAFETY: as in `gardens`, on the game thread; the evolution code sets them the same way.
        unsafe { at.write_volatile(at.read_volatile() | u32::from(new)) };
        crate::log!("unlocks: a party player opened gardens {new:#04x}; ours too");
        crate::hud::toast(match new {
            GARDEN_HERO => "The Hero Garden is open (a party player opened it)",
            GARDEN_DARK => "The Dark Garden is open (a party player opened it)",
            _ => "The Hero and Dark Gardens are open (a party player opened them)",
        });
    }
    let ours = gardens();
    if ours != 0 && (ours != u.sent || u.frame.is_multiple_of(SEND_EVERY)) {
        u.sent = ours;
        return Some(ours);
    }
    None
}
