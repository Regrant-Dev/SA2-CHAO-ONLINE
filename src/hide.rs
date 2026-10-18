//! Drawing nothing of the character while a skin is drawn in its place (re/skins.md "Hiding the
//! character").
//!
//! Everything the game draws (models, sprites, effects, shadows) reaches the GPU through two
//! entries of its renderer's function table: the renderer is a C++ object at `*0x1A557C0`,
//! entry +0x40 draws cached buffers (7 arguments) and +0x54 plain vertices (6 arguments), both
//! thiscall. While `hidden` runs, the renderer uses a copy of its table whose two draw entries
//! do nothing, so whatever a character's display draws doesn't show, whatever the character,
//! while everything it works out (node positions, hands, held things) still happens.
//!
//! Sonic's family's jump ball isn't drawn by their display: each frame of a jump spawns a
//! JumpAura effect object, drawn by its own display (0x756960; re/spin-ball.md). While our
//! player wears a skin, theirs aren't drawn.
//!
//! One model can be let through: DrawMotion (0x780870, cdecl (model, frame), ECX = motion)
//! draws it with the real table. A rigged skin's body goes in through the character's own
//! display; this lets only that body show.

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

/// The renderer object (its first word is its function table).
const RENDERER: usize = 0x01A5_57C0;
/// The draw entries (in table words) and how many arguments each takes.
#[cfg(target_arch = "x86")]
const DRAW_BUFFERS: usize = 0x40 / 4;
#[cfg(target_arch = "x86")]
const DRAW_VERTICES: usize = 0x54 / 4;
/// The most of the table copied (the game uses entries up to +0x114).
#[cfg(target_arch = "x86")]
const TABLE_MAX: usize = 256;

/// DrawMotion and its first bytes (moved into the hook).
#[cfg(target_arch = "x86")]
const DRAW_MOTION: usize = 0x0078_0870;
#[cfg(target_arch = "x86")]
const DRAW_MOTION_START: [u8; 6] = [0x51, 0xD9, 0x44, 0x24, 0x0C, 0x51];

/// The jump ball's display and its first bytes (moved into the hook); the ball's data
/// (object +0x40) holds the player's index at +1.
#[cfg(target_arch = "x86")]
const JUMP_BALL_DISPLAY: usize = 0x0075_6960;
#[cfg(target_arch = "x86")]
const JUMP_BALL_START: [u8; 7] = [0x8B, 0x44, 0x24, 0x04, 0x83, 0xEC, 0x30];
/// Our player's jump balls aren't drawn while this is set.
static NO_JUMP_BALL: AtomicBool = AtomicBool::new(false);

/// While hiding: the renderer, its own table, the quiet copy, and the model let through.
static RENDERER_NOW: AtomicUsize = AtomicUsize::new(0);
static REAL_TABLE: AtomicUsize = AtomicUsize::new(0);
static QUIET_TABLE: AtomicUsize = AtomicUsize::new(0);
static LET_THROUGH: AtomicUsize = AtomicUsize::new(0);
static LET_THROUGH_DRAWN: AtomicBool = AtomicBool::new(false);
static MOTION_HOOK: AtomicBool = AtomicBool::new(false);
/// The quiet copy, for the table it was made from.
#[cfg(target_arch = "x86")]
static QUIET: std::sync::Mutex<Option<(usize, Box<[usize]>)>> = std::sync::Mutex::new(None);

/// Puts the DrawMotion hook in (once, at start).
pub fn install() {
    #[cfg(target_arch = "x86")]
    {
        let ours = draw_motion_thunk as *const () as usize;
        let mut jump = [0x90u8; 6];
        jump[0] = 0xE9;
        jump[1..5].copy_from_slice(&(ours.wrapping_sub(DRAW_MOTION + 5) as u32).to_le_bytes());
        let ok = crate::patch::replace_code(DRAW_MOTION, &DRAW_MOTION_START, &jump);
        MOTION_HOOK.store(ok, Ordering::Relaxed);
        if !ok {
            crate::log!("hide: couldn't hook DrawMotion; rigged skins show the character's extras");
        }
        let ours = jump_ball_thunk as *const () as usize;
        let mut jump = [0x90u8; 7];
        jump[0] = 0xE9;
        jump[1..5]
            .copy_from_slice(&(ours.wrapping_sub(JUMP_BALL_DISPLAY + 5) as u32).to_le_bytes());
        if !crate::patch::replace_code(JUMP_BALL_DISPLAY, &JUMP_BALL_START, &jump) {
            crate::log!("hide: couldn't hook the jump ball; it shows under skins");
        }
    }
}

/// Whether our player's jump balls are drawn (not while a skin is drawn instead of them).
pub fn set_jump_ball_hidden(hidden: bool) {
    NO_JUMP_BALL.store(hidden, Ordering::Relaxed);
}

/// Whether one model can be let through `hidden`.
pub fn can_let_through() -> bool {
    MOTION_HOOK.load(Ordering::Relaxed)
}

/// Whether the renderer is there to hide with.
pub fn available() -> bool {
    // SAFETY: a static pointer of the game's.
    cfg!(target_arch = "x86") && unsafe { (RENDERER as *const usize).read() } != 0
}

/// Runs `f` (on the game thread, in the display pass) with nothing drawn, apart from
/// DrawMotion drawing `let_through` (0: nothing). `None` if it can't hide (and `f` didn't run);
/// otherwise whether `let_through` was drawn.
pub fn hidden(let_through: usize, f: impl FnOnce()) -> Option<bool> {
    // Already hiding (a display inside a display): as it is.
    if RENDERER_NOW.load(Ordering::Relaxed) != 0 {
        f();
        return Some(false);
    }
    let (renderer, real, quiet) = quiet_table()?;
    LET_THROUGH.store(let_through, Ordering::Relaxed);
    LET_THROUGH_DRAWN.store(false, Ordering::Relaxed);
    REAL_TABLE.store(real, Ordering::Relaxed);
    QUIET_TABLE.store(quiet, Ordering::Relaxed);
    RENDERER_NOW.store(renderer, Ordering::Relaxed);
    // SAFETY: the renderer object's table pointer, put back right after.
    unsafe { (renderer as *mut usize).write(quiet) };
    f();
    // SAFETY: as above.
    unsafe { (renderer as *mut usize).write(real) };
    RENDERER_NOW.store(0, Ordering::Relaxed);
    LET_THROUGH.store(0, Ordering::Relaxed);
    Some(LET_THROUGH_DRAWN.load(Ordering::Relaxed))
}

/// The renderer, its table, and a quiet copy of that table (made once per table).
#[cfg(target_arch = "x86")]
fn quiet_table() -> Option<(usize, usize, usize)> {
    // SAFETY: the game's renderer pointer and its object's first word.
    let renderer = unsafe { (RENDERER as *const usize).read() };
    if renderer == 0 {
        return None;
    }
    // SAFETY: as above.
    let real = unsafe { (renderer as *const usize).read() };
    if real == 0 {
        return None;
    }
    let mut quiet = QUIET.lock().unwrap_or_else(|e| e.into_inner());
    if quiet.as_ref().map(|(from, _)| *from) != Some(real) {
        let words = readable_words(real).min(TABLE_MAX);
        if words <= DRAW_VERTICES {
            return None;
        }
        // SAFETY: `words` words from `real` are readable (checked above).
        let mut table: Box<[usize]> =
            unsafe { std::slice::from_raw_parts(real as *const usize, words) }.into();
        table[DRAW_BUFFERS] = skip_draw_buffers as *const () as usize;
        table[DRAW_VERTICES] = skip_draw_vertices as *const () as usize;
        *quiet = Some((real, table));
    }
    let table = quiet.as_ref()?.1.as_ptr() as usize;
    Some((renderer, real, table))
}

#[cfg(not(target_arch = "x86"))]
fn quiet_table() -> Option<(usize, usize, usize)> {
    None
}

/// How many words from `at` can be read (to the end of its memory region).
#[cfg(target_arch = "x86")]
fn readable_words(at: usize) -> usize {
    #[repr(C)]
    #[derive(Default)]
    struct MemoryBasicInformation {
        base: usize,
        allocation_base: usize,
        allocation_protect: u32,
        size: usize,
        state: u32,
        protect: u32,
        kind: u32,
    }
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn VirtualQuery(at: usize, info: *mut MemoryBasicInformation, size: usize) -> usize;
    }
    const MEM_COMMIT: u32 = 0x1000;
    const READABLE: u32 = 0x02 | 0x04 | 0x08 | 0x20 | 0x40 | 0x80;
    let mut info = MemoryBasicInformation::default();
    // SAFETY: a query about our own process's memory into a struct of the right size.
    let got = unsafe { VirtualQuery(at, &mut info, std::mem::size_of::<MemoryBasicInformation>()) };
    if got == 0
        || info.state != MEM_COMMIT
        || info.protect & READABLE == 0
        || info.protect & 0x100 != 0
    {
        return 0;
    }
    (info.base + info.size).saturating_sub(at) / 4
}

/// DrawMotion is about to draw the model let through: the real table for it.
#[cfg(target_arch = "x86")]
extern "C" fn show() {
    let renderer = RENDERER_NOW.load(Ordering::Relaxed);
    if renderer != 0 {
        // SAFETY: the renderer we're hiding with; its table goes back to quiet right after.
        unsafe { (renderer as *mut usize).write(REAL_TABLE.load(Ordering::Relaxed)) };
        LET_THROUGH_DRAWN.store(true, Ordering::Relaxed);
    }
}

#[cfg(target_arch = "x86")]
extern "C" fn quiet_again() {
    let renderer = RENDERER_NOW.load(Ordering::Relaxed);
    if renderer != 0 {
        // SAFETY: as in `show`.
        unsafe { (renderer as *mut usize).write(QUIET_TABLE.load(Ordering::Relaxed)) };
    }
}

/// The quiet draw entries: thiscall, the callee pops its arguments; S_OK.
#[cfg(target_arch = "x86")]
#[unsafe(naked)]
unsafe extern "C" fn skip_draw_buffers() {
    std::arch::naked_asm!("xor eax, eax", "ret 0x1c")
}

#[cfg(target_arch = "x86")]
#[unsafe(naked)]
unsafe extern "C" fn skip_draw_vertices() {
    std::arch::naked_asm!("xor eax, eax", "ret 0x18")
}

/// DrawMotion: the model let through is drawn for real (`draw_motion_visible`), anything else
/// as usual (the moved prologue, then on at +6).
#[cfg(target_arch = "x86")]
#[unsafe(naked)]
unsafe extern "C" fn draw_motion_thunk() {
    std::arch::naked_asm!(
        "cmp dword ptr [{now}], 0",
        "je 2f",
        "mov eax, dword ptr [esp+4]",
        "test eax, eax",
        "je 2f",
        "cmp eax, dword ptr [{through}]",
        "jne 2f",
        "jmp {visible}",
        "2:",
        "push ecx",
        "fld dword ptr [esp+0xc]",
        "push ecx",
        "push {resume}",
        "ret",
        now = sym RENDERER_NOW,
        through = sym LET_THROUGH,
        visible = sym draw_motion_visible,
        resume = const DRAW_MOTION + 6,
    )
}

/// DrawMotion's own body, called (cdecl (model, frame), ECX = motion).
#[cfg(target_arch = "x86")]
#[unsafe(naked)]
unsafe extern "C" fn draw_motion_original() {
    std::arch::naked_asm!(
        "push ecx",
        "fld dword ptr [esp+0xc]",
        "push ecx",
        "push {resume}",
        "ret",
        resume = const DRAW_MOTION + 6,
    )
}

/// The let-through model drawn with the real table, then quiet again.
#[cfg(target_arch = "x86")]
#[unsafe(naked)]
unsafe extern "C" fn draw_motion_visible() {
    std::arch::naked_asm!(
        "push ecx",
        "call {show}",
        "pop ecx",
        "push dword ptr [esp+8]",
        "push dword ptr [esp+8]",
        "call {original}",
        "add esp, 8",
        "push eax",
        "call {quiet}",
        "pop eax",
        "ret",
        show = sym show,
        original = sym draw_motion_original,
        quiet = sym quiet_again,
    )
}

/// The jump ball's display: nothing for our player's (index 0) while hidden, else as usual
/// (the moved instructions, then on at +7).
#[cfg(target_arch = "x86")]
#[unsafe(naked)]
unsafe extern "C" fn jump_ball_thunk() {
    std::arch::naked_asm!(
        "mov eax, dword ptr [esp+4]",
        "cmp byte ptr [{hidden}], 0",
        "je 2f",
        "mov edx, dword ptr [eax+0x40]",
        "test edx, edx",
        "je 2f",
        "cmp byte ptr [edx+1], 0",
        "jne 2f",
        "ret",
        "2:",
        "sub esp, 0x30",
        "push {resume}",
        "ret",
        hidden = sym NO_JUMP_BALL,
        resume = const JUMP_BALL_DISPLAY + 7,
    )
}
