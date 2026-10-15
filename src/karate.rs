//! Chao Karate against a friend's Chao: the CPU opponent of your next karate fight is replaced
//! by a Chao another player brought online (their "My Chao" or garden Chao, see online.rs).
//!
//! Karate's fight setup (0x542250) creates both fighters with 0x578EB0 (EAX = source, then
//! position, angle, side, mode on the stack). Mode 1 creates the fighter from real Chao data
//! (EAX = the 0x800-byte record, like the game's 2-player karate), mode 0 builds a CPU Chao from
//! the opponent table at 0x1296C10. We take over the CPU opponent's call (0x542308): with a
//! friend's Chao chosen, it is created in mode 1 from a copy of their record. The copy is never
//! saved anywhere; your own Chao and save only change the way any karate fight changes them.
//!
//! Each player runs their own fight (karate isn't streamed between games).

// The hook only exists in the 32-bit game build.
#![cfg_attr(not(target_arch = "x86"), allow(dead_code))]

use std::sync::Mutex;

use crate::chao::ChaoData;
use crate::hud;
use crate::log;

/// Steam ownership of the Battle DLC, which is what puts Karate in the Chao Stadium
/// (set at startup at 0x40A67B; re/chao-stadium.md). We never change it.
const HAVE_BATTLE_DLC: usize = 0x01A5_56F4;

/// Karate exists in this game (the player owns the Battle DLC).
pub fn available() -> bool {
    // SAFETY: static dword in the game's data section.
    cfg!(target_arch = "x86") && unsafe { (HAVE_BATTLE_DLC as *const u32).read_volatile() } != 0
}

/// The fight setup's `call` that creates the CPU opponent.
const CPU_SPAWN_CALL: usize = 0x0054_2308;
/// Creates a karate fighter (see module docs).
const SPAWN_FIGHTER: usize = 0x0057_8EB0;

struct Karate {
    /// Label and Chao of the chosen opponent; `None` = the normal CPU opponent.
    opponent: Option<(String, ChaoData)>,
    /// Record handed to the game for the current fight (kept alive, never freed).
    buffer: Option<&'static mut ChaoData>,
}

static KARATE: Mutex<Karate> = Mutex::new(Karate {
    opponent: None,
    buffer: None,
});

/// Chooses the opponent for the next karate fights (`None` = CPU).
pub fn set_opponent(opponent: Option<(String, ChaoData)>) {
    KARATE.lock().unwrap_or_else(|e| e.into_inner()).opponent = opponent;
}

pub fn opponent_label() -> Option<String> {
    let k = KARATE.lock().unwrap_or_else(|e| e.into_inner());
    k.opponent.as_ref().map(|(label, _)| label.clone())
}

/// Hooks the CPU opponent's creation. Call once at startup.
pub fn install() {
    #[cfg(target_arch = "x86")]
    {
        let target = spawn_opponent as unsafe extern "C" fn() as usize;
        match crate::patch::redirect_call(CPU_SPAWN_CALL, target) {
            Some(original) if original == SPAWN_FIGHTER => {}
            Some(original) => {
                // Someone else hooked it first; put it back rather than skip their code.
                crate::patch::redirect_call(CPU_SPAWN_CALL, original);
                log!("karate: {CPU_SPAWN_CALL:#x} is already hooked; online karate is off");
            }
            None => log!("karate: can't hook {CPU_SPAWN_CALL:#x}; online karate is off"),
        }
    }
}

/// Called from the fight setup: the record to fight instead of the CPU, or null.
extern "C" fn opponent_record() -> *mut ChaoData {
    let mut guard = KARATE.lock().unwrap_or_else(|e| e.into_inner());
    let k = &mut *guard;
    let Some((label, chao)) = &k.opponent else {
        return std::ptr::null_mut();
    };
    log!("karate: fighting {label} ({})", chao.summary());
    hud::toast(format!("Karate: fighting {label}"));
    // The game keeps a pointer to the record for the whole fight, so it must stay put.
    let buffer = match k.buffer.take() {
        Some(buffer) => {
            *buffer = chao.clone();
            buffer
        }
        None => Box::leak(Box::new(chao.clone())),
    };
    let pointer: *mut ChaoData = buffer;
    k.buffer = Some(buffer);
    pointer
}

/// Replaces `call SPAWN_FIGHTER` for the CPU opponent. Stack on entry: return address,
/// position, angle, side, mode; EAX = the CPU opponent's table entry.
#[cfg(target_arch = "x86")]
#[unsafe(naked)]
unsafe extern "C" fn spawn_opponent() {
    std::arch::naked_asm!(
        "push eax",
        "call {record}",
        "test eax, eax",
        "jz 2f",
        // A friend's Chao: EAX = their record, mode = 1 (real Chao data).
        "add esp, 4",
        "mov dword ptr [esp + 16], 1",
        "push {spawn}",
        "ret",
        "2:",
        // The normal CPU opponent.
        "pop eax",
        "push {spawn}",
        "ret",
        record = sym opponent_record,
        spawn = const SPAWN_FIGHTER,
    )
}

/// Best-effort readable Chao name (the game's name alphabet is ASCII shifted down by 0x20).
pub fn chao_name(chao: &ChaoData) -> String {
    let name: String = chao
        .name_raw()
        .iter()
        .take_while(|&&b| b != 0)
        .map(|&b| match b {
            0x01..=0x5E => (b + 0x20) as char,
            _ => '?',
        })
        .collect();
    if name.trim().is_empty() {
        "Chao".into()
    } else {
        name
    }
}

#[cfg(test)]
mod tests {
    use super::chao_name;
    use crate::chao::{CHAO_SIZE, ChaoData};

    #[test]
    fn names() {
        let mut c = ChaoData {
            bytes: [0; CHAO_SIZE],
        };
        assert_eq!(chao_name(&c), "Chao");
        for (i, b) in [0x21u8, 0x42, 0x43].iter().enumerate() {
            c.bytes[0x12 + i] = *b;
        }
        assert_eq!(chao_name(&c), "Abc");
    }
}
