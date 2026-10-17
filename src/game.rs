//! Game memory addresses (sonic2app.exe, see docs/game-version.md).
//! Each address is from the community headers and/or verified in Ghidra; see re/addresses.md.

use std::ffi::c_void;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicUsize, Ordering};

use crate::chao::{CHAO_SIZE, ChaoData};

/// `CurrentLevel` (SA2Variables.h, `__int16` at 0x1934B70).
const CURRENT_LEVEL: usize = 0x0193_4B70;
/// Stage control bytes the game's own goal routine (0x43E6D0) uses: `ControllerEnabled[0]`
/// (1 = player can move) and `TimerStopped` (1 = stage timer frozen).
const CONTROLLER_ENABLED: usize = 0x0174_AFFF;
const TIMER_STOPPED: usize = 0x0174_AFDA;
/// `MissionNum` (0..=4) and `GameState` (16 = in game), SA2Variables.h; heavily used in code.
const MISSION_NUM: usize = 0x0174_AFE3;
const GAME_STATE: usize = 0x0193_4BE0;
pub const GAME_STATE_INGAME: i16 = 16;
/// `CurrentChaoArea` (SA2Variables.h, `int` at 0x134046C).
const CURRENT_CHAO_AREA: usize = 0x0134_046C;
/// `ChaoSlots` (SA2Variables.h, `ChaoData[37]` at 0x19F6EC4). We only touch the first 24.
const CHAO_SLOTS: usize = 0x019F_6EC4;
/// `MainCharObj1` (SA2Variables.h, `EntityData1*[8]` at 0x1DEA6C0).
const MAIN_CHAR_OBJ1: usize = 0x01DE_A6C0;
/// Per-player `CharObj2` pointers, filled by the player setup at 0x4599C0 (re/characters.md).
const MAIN_CHAR_OBJ2: usize = 0x01DE_9600;
/// `CreateChao` (SA2Functions.h, cdecl at 0x5501D0).
const CREATE_CHAO: usize = 0x0055_01D0;

/// `LevelIDs_ChaoWorld`.
pub const LEVEL_CHAO_WORLD: i16 = 90;
/// Garden slots: 8 per garden x 3 gardens.
pub const GARDEN_SLOTS: usize = 24;

/// `EntityData1::Position` offset (Action..Timer = 8 bytes, Rotation = 12).
const ENTITY_POSITION: usize = 0x14;
/// `EntityData1::Rotation` offset (x, y, z angles).
const ENTITY_ROTATION: usize = 0x08;
/// `EntityData1::Rotation.y` offset.
const ENTITY_ROTATION_Y: usize = 0x0C;

/// CharObj2 offsets, confirmed in the draw and setup code (re/characters.md).
pub const CO2_CHAR_ID: usize = 0x03;
/// `CharObj2Base::Upgrades` (bit flags of the character's upgrades).
pub const CO2_UPGRADES: usize = 0x24;
pub const CO2_ANIM: usize = 0x178;
pub const CO2_FRAME: usize = 0x184;
pub const CO2_ANIM_INFO: usize = 0x198;
const CO2_MOTION_MODE: usize = 0x174;
/// `{index, count, NJS_MOTION*}` x 300 at 0x1DEA700; the pointer field.
pub const MOTION_TABLE: usize = 0x01DE_A704;
pub const MOTION_COUNT: usize = 300;

/// `LocalPlayer::flags`: in a jump, with the blue spin ball (JumpAura) around the player.
pub const POSE_BALL: u8 = 1;
/// `LocalPlayer::flags`: blending into `anim` (animation state `co2+0x174 == 2`, see
/// FUN_004692A0). `frame` is then the blend progress 0..1, and the blend motion at `co2+0x19C`
/// is built per player at runtime, so puppets draw the target animation instead.
pub const POSE_BLENDING: u8 = 2;
/// `PlayerState::flags`: holding at the start line, waiting to start together.
pub const POSE_WAITING: u8 = 4;
/// `PlayerState::flags`: asked everyone in this stage (or race) to start now (F9 > Start now).
pub const POSE_GO: u8 = 8;
/// `PlayerState::flags`: in a Chao Race, the state is our racing Chao instead of our character:
/// position and facing (rotation y) are the Chao's, `frame` is its race progress.
pub const POSE_RACE_CHAO: u8 = 16;
/// `PlayerState::flags`: in a live karate fight (can't be challenged).
pub const POSE_KARATE_BUSY: u8 = 32;
/// `PlayerState::flags`: playing a stage that started together (past its countdown).
pub const POSE_PLAYING: u8 = 64;

#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct Vector {
    pub x: f32,
    pub y: f32,
    pub z: f32,
}

pub fn mission() -> u8 {
    // SAFETY: static byte in the game's data section.
    unsafe { (MISSION_NUM as *const u8).read_volatile() }
}

pub fn game_state() -> i16 {
    // SAFETY: static short in the game's data section.
    unsafe { (GAME_STATE as *const i16).read_volatile() }
}

/// Holds (or releases) player 1 at the start line: the whole stage frozen, movement off and
/// the stage timer stopped.
/// Turns the game's controller input off or on (without freezing anything).
pub fn set_controls(on: bool) {
    // SAFETY: a static byte the game itself toggles (goal routine 0x43E6D0).
    unsafe { (CONTROLLER_ENABLED as *mut u8).write_volatile(u8::from(on)) };
}

pub fn hold_player(hold: bool) {
    freeze_world(hold);
    // SAFETY: static bytes the game itself toggles (goal routine 0x43E6D0).
    unsafe {
        (CONTROLLER_ENABLED as *mut u8).write_volatile(u8::from(!hold));
        (TIMER_STOPPED as *mut u8).write_volatile(u8::from(hold));
    }
}

/// In-stage game state (16): its `call` to the frame update (objects, camera, timer, input).
const INGAME_UPDATE_CALL: usize = 0x0043_AB24;
/// What the pause screen runs instead: draws the stage without updating it. Returns 0.
const DRAW_ONLY: usize = 0x0043_D4B0;

/// Pause screen state (17): its `call` to draw the stage without updating it.
const PAUSE_DRAW_CALL: usize = 0x0043_D311;
/// Requests a state change from the frame update (1 = Start pressed: open the pause screen).
const STATE_REQUEST: usize = 0x01A5_58A4;

static FROZEN: AtomicBool = AtomicBool::new(false);
/// Frame updates still allowed while frozen (so a stage gets its camera before freezing).
static GRACE: AtomicU32 = AtomicU32::new(0);
static LIVE_PAUSE: AtomicBool = AtomicBool::new(false);
static INGAME_UPDATE: AtomicUsize = AtomicUsize::new(0);
static PAUSE_DRAW: AtomicUsize = AtomicUsize::new(0);

/// Freezes (or unfreezes) the whole stage like the pause screen does, without its menu:
/// nothing moves and the timer stops, but the stage (and other players) keep being drawn.
pub fn freeze_world(freeze: bool) {
    FROZEN.store(freeze, Ordering::Relaxed);
}

/// Freezes a stage that is still loading: it gets `frames` updates (camera set up), then
/// stands still from its first visible moment.
pub fn freeze_world_after(frames: u32) {
    if !FROZEN.swap(true, Ordering::Relaxed) {
        GRACE.store(frames, Ordering::Relaxed);
    }
}

/// In a stage started together, the pause screen doesn't stop the game (like online games):
/// the stage keeps running behind the menu, only our player ignores the controller.
/// Keeps the game running while its window is in the background (multiplayer).
///
/// The Mod Loader's "pause when inactive" sets `WindowActive` (0x174F5AC) to 0 when the window
/// loses focus, which stops the game loop, and stops sounds and inputs. While connected we set
/// it back to 1 (from the network thread, as the game loop isn't running then), and restore the
/// sounds when the window is focused again (`on_frame`).
pub fn keep_running_in_background() {
    // SAFETY: static int in the game's data section.
    unsafe {
        let active = WINDOW_ACTIVE as *mut i32;
        if active.read_volatile() == 0 {
            active.write_volatile(1);
            KEPT_RUNNING.store(true, Ordering::Relaxed);
        }
    }
}

/// Call every frame: brings the sounds back after `keep_running_in_background` once the window
/// is focused again.
pub fn restore_after_background() {
    if KEPT_RUNNING.load(Ordering::Relaxed) && crate::input::game_focused() {
        KEPT_RUNNING.store(false, Ordering::Relaxed);
        // SAFETY: the game's own RestoreSounds (0x435FD0, no arguments), on the game thread,
        // as the Mod Loader calls it when the window is activated.
        unsafe { std::mem::transmute::<usize, extern "C" fn()>(RESTORE_SOUNDS)() };
    }
}

/// While online, the game's "Reboot to change the control scheme" dialog (opened when the number
/// of connected XInput pads changes) never opens: it stops the whole game update until
/// answered. Two `je` that skip arming it become `jmp` (re/control-scheme-dialog.md); the pad
/// list itself keeps updating. Safe to switch at any time.
pub fn block_reboot_dialog(block: bool) {
    const SITES: [usize; 2] = [0x0040_A93E, 0x0040_A9F8];
    let (from, to): ([u8; 2], [u8; 2]) = if block {
        ([0x74, 0x03], [0xEB, 0x03])
    } else {
        ([0xEB, 0x03], [0x74, 0x03])
    };
    if REBOOT_DIALOG_BLOCKED.swap(block, Ordering::Relaxed) == block {
        return;
    }
    for site in SITES {
        if !crate::patch::replace_code(site, &from, &to) {
            crate::log!("game: reboot-dialog patch at {site:#x} didn't apply");
        }
    }
}

/// Closes the reboot dialog if it's open (as "Reboot later" does). Game thread.
pub fn close_reboot_dialog() {
    const DIALOG: usize = 0x01AF_19F0;
    const PAUSED: usize = 0x0174_AFD9;
    // SAFETY: the game's system-dialog object (0x80 bytes) and static flags.
    unsafe {
        let dialog = (DIALOG as *const usize).read_volatile();
        if dialog == 0 || ((dialog + 0x64) as *const i32).read_volatile() != 4 {
            return;
        }
        ((dialog + 0x7C) as *mut i32).write_volatile(0);
        if (PAUSED as *const u8).read_volatile() != 0 {
            std::mem::transmute::<usize, extern "C" fn()>(RESTORE_SOUNDS)();
        }
        let idle = i32::from(((dialog + 0x70) as *const i32).read_volatile() != 0);
        ((dialog + 0x64) as *mut i32).write_volatile(idle);
    }
}

static REBOOT_DIALOG_BLOCKED: AtomicBool = AtomicBool::new(false);

const WINDOW_ACTIVE: usize = 0x0174_F5AC;
const RESTORE_SOUNDS: usize = 0x0043_5FD0;
static KEPT_RUNNING: AtomicBool = AtomicBool::new(false);

pub fn set_live_pause(live: bool) {
    LIVE_PAUSE.store(live, Ordering::Relaxed);
}

/// Hooks the in-stage frame update (for `freeze_world`) and the pause screen's drawing (for
/// `set_live_pause`). Call once at startup.
pub fn install_freeze() {
    let target = ingame_update as extern "C" fn() -> i32 as usize;
    match crate::patch::redirect_call(INGAME_UPDATE_CALL, target) {
        Some(original) => INGAME_UPDATE.store(original, Ordering::SeqCst),
        None => crate::log!("game: can't hook {INGAME_UPDATE_CALL:#x}; start sync won't freeze"),
    }
    let target = pause_draw as extern "C" fn() as usize;
    match crate::patch::redirect_call(PAUSE_DRAW_CALL, target) {
        Some(original) => PAUSE_DRAW.store(original, Ordering::SeqCst),
        None => crate::log!("game: can't hook {PAUSE_DRAW_CALL:#x}; pausing stops the game"),
    }
}

extern "C" fn pause_draw() {
    let update = INGAME_UPDATE.load(Ordering::SeqCst);
    if LIVE_PAUSE.load(Ordering::Relaxed) && !FROZEN.load(Ordering::Relaxed) && update != 0 {
        // SAFETY: static game variables, and the game's own cdecl frame update without
        // arguments. The player ignores the pad (it drives the menu now), and a Start press
        // seen by the update must not ask for the pause screen again.
        unsafe {
            let controller = (CONTROLLER_ENABLED as *mut u8).read_volatile();
            let request = (STATE_REQUEST as *mut u16).read_volatile();
            (CONTROLLER_ENABLED as *mut u8).write_volatile(0);
            std::mem::transmute::<usize, extern "C" fn() -> i32>(update)();
            (CONTROLLER_ENABLED as *mut u8).write_volatile(controller);
            (STATE_REQUEST as *mut u16).write_volatile(request);
        }
    } else {
        // SAFETY: the game's own cdecl draw-only function, which this call site called.
        unsafe {
            std::mem::transmute::<usize, extern "C" fn()>(PAUSE_DRAW.load(Ordering::SeqCst))()
        }
    }
}

extern "C" fn ingame_update() -> i32 {
    let grace = GRACE.load(Ordering::Relaxed);
    if grace > 0 {
        GRACE.store(grace - 1, Ordering::Relaxed);
    }
    let function = if FROZEN.load(Ordering::Relaxed) && grace == 0 {
        DRAW_ONLY
    } else {
        INGAME_UPDATE.load(Ordering::SeqCst)
    };
    // SAFETY: both are the game's own cdecl functions without arguments, returning in EAX.
    unsafe { std::mem::transmute::<usize, extern "C" fn() -> i32>(function)() }
}

/// A local two-player game (split screen) is running. Online play steps aside then: the mod
/// assumes one local player.
pub fn two_player() -> bool {
    // SAFETY: static byte in the game's data section (`TwoPlayerMode`).
    unsafe { (TWO_PLAYER_MODE as *const u8).read_volatile() != 0 }
}

const TWO_PLAYER_MODE: usize = 0x0174_AFDE;

/// While online, the main menu's 2P entry (ROOM BATTLES) does nothing in the game itself:
/// its `jz` at 0x6650D9 becomes a `jmp` to the end of the "can't select" path, past the sound
/// (0x6654A4 cleans the 3 pushed arguments and returns), and `battles` opens its picker.
pub fn block_two_player_menu(online: bool) {
    const SITE: usize = 0x0066_50D9;
    const ORIGINAL: [u8; 6] = [0x0F, 0x84, 0xBB, 0x03, 0x00, 0x00];
    const SILENT: [u8; 6] = [0x90, 0xE9, 0xC5, 0x03, 0x00, 0x00];
    if TWO_PLAYER_BLOCKED.swap(online, Ordering::Relaxed) != online {
        let (from, to) = if online {
            (ORIGINAL, SILENT)
        } else {
            (SILENT, ORIGINAL)
        };
        if !crate::patch::replace_code(SITE, &from, &to) {
            crate::log!("game: 2P menu patch at {SITE:#x} didn't apply");
        }
    }
}

static TWO_PLAYER_BLOCKED: AtomicBool = AtomicBool::new(false);

const MAIN_MENU: usize = 0x00C7_7F14;
const MENU_HEADER: usize = 0x00C7_7F00;
const CURRENT_SUB_MENU: usize = 0x01D7_BB14;
const SUB_MAIN_LIST: i32 = 2;
/// Past the main menu's SubMenu jump table (0..0x1B): its update does nothing then.
const SUB_WAITING: i32 = 0x1C;

/// The menu buttons pressed this frame (`MenuButtons_Pressed`: 0x10 up, 0x20 down, 0x0C
/// A/Start, 0x02 B; the stick counts).
pub fn menu_buttons_pressed() -> u32 {
    // SAFETY: static int in the game's data section.
    unsafe { (0x01DE_FAB0 as *const u32).read_volatile() }
}

/// Holds the main menu's entry list still (a SubMenu its update ignores) while one of ours is
/// shown, or lets it go again. Holding needs the list idle; returns whether it's held.
pub fn hold_main_menu(hold: bool) -> bool {
    if !on_main_menu() {
        return false;
    }
    // SAFETY: the SubMenu int and the main menu's data (pointer checked).
    unsafe {
        let sub = (CURRENT_SUB_MENU as *mut i32).read_volatile();
        let menu = (MAIN_MENU as *const usize).read_volatile();
        if hold
            && sub == SUB_MAIN_LIST
            && menu != 0
            && ((menu + 8) as *const u32).read_volatile() == 0
        {
            (CURRENT_SUB_MENU as *mut i32).write_volatile(SUB_WAITING);
            return true;
        }
        if !hold && sub == SUB_WAITING {
            (CURRENT_SUB_MENU as *mut i32).write_volatile(SUB_MAIN_LIST);
        }
    }
    false
}

/// The 1P panel's entries `leave_main_menu` can go to.
pub const PANEL_STAGE_SELECT: u8 = 1;
pub const PANEL_KART: u8 = 2;

/// Leaves the main menu for a 1P panel entry the way that panel does (re/main-menu.md): the
/// panels slide out (0x664DA0, 0x671B80), then SubMenu 5 opens the entry under the panel's
/// cursor (0x1D1BC00) once they're idle. Kart Race goes 1P (kart work +0x88 = 0); Stage
/// Select clears the stage panel's +0x14 like its handler does. No unlock checks.
pub fn leave_main_menu(entry: u8) {
    const HIDE_MAIN: usize = 0x0066_4DA0;
    const HIDE_HEADER: usize = 0x0067_1B80;
    const KART_WORK: usize = 0x00C7_7F34;
    const STAGE_PANEL: usize = 0x00C7_7F44;
    // SAFETY: the menus' data pointers (checked) and two argument-less cdecl functions the
    // game's own handlers call from this update.
    unsafe {
        let menu = (MAIN_MENU as *const usize).read_volatile();
        if menu == 0 || (MENU_HEADER as *const usize).read_volatile() == 0 {
            return;
        }
        let other = match entry {
            PANEL_KART => (KART_WORK, 0x88),
            _ => (STAGE_PANEL, 0x14),
        };
        let work = (other.0 as *const usize).read_volatile();
        if work == 0 {
            return;
        }
        ((menu + 0x40) as *mut u8).write_volatile(entry);
        std::mem::transmute::<usize, extern "C" fn()>(HIDE_MAIN)();
        std::mem::transmute::<usize, extern "C" fn()>(HIDE_HEADER)();
        if entry == PANEL_KART {
            ((work + other.1) as *mut u8).write_volatile(0);
        } else {
            ((work + other.1) as *mut i32).write_volatile(0);
        }
        (CURRENT_SUB_MENU as *mut i32).write_volatile(5);
    }
}

/// Where the main menu's 1P label is drawn right now (its centre, in the game's 640x480 menu
/// space; it slides in with the menu), and whether the cursor is on it, while the main menu's
/// entry list is showing.
pub fn main_menu_1p_label() -> Option<(f32, f32, bool)> {
    main_menu_label(0)
}

/// The same for the 2P entry (the live positions are 12 bytes apart, 1P first).
pub fn main_menu_2p_label() -> Option<(f32, f32, bool)> {
    main_menu_label(1)
}

fn main_menu_label(entry: usize) -> Option<(f32, f32, bool)> {
    const CURRENT_SUB_MENU: usize = 0x01D7_BB14;
    const CURSOR: usize = 0x01D1_BBE4;
    const ENTRY_SIZE: usize = 12;
    let label_x = 0x01A3_D678 + entry * ENTRY_SIZE;
    let label_y = label_x + 4;
    if !on_main_menu() {
        return None;
    }
    // SAFETY: static values in the game's data section.
    let (sub, x, y, cursor) = unsafe {
        (
            (CURRENT_SUB_MENU as *const i32).read_volatile(),
            (label_x as *const f32).read_volatile(),
            (label_y as *const f32).read_volatile(),
            (CURSOR as *const i8).read_volatile(),
        )
    };
    let on_screen = (-300.0..940.0).contains(&x) && (-100.0..580.0).contains(&y);
    (sub == 2 && on_screen).then_some((x, y, cursor as usize == entry))
}

/// On the game's main menu (the 1P / 2P / options screen).
pub fn on_main_menu() -> bool {
    const GAME_MODE: usize = 0x0174_B040;
    const CURRENT_MENU: usize = 0x01D7_BB10;
    // SAFETY: static ints in the game's data section.
    unsafe {
        (GAME_MODE as *const i32).read_volatile() == 0xE
            && (CURRENT_MENU as *const i32).read_volatile() == 1
    }
}

/// Player 1 pressed Start this frame (pad `*0x1DEFB60`, `+0x10` = pressed, bit 8 = Start).
pub fn start_pressed() -> bool {
    const PAD1: usize = 0x01DE_FB60;
    // SAFETY: the game's pad pointer and its pressed-buttons field.
    unsafe {
        let pad = (PAD1 as *const usize).read_volatile();
        pad != 0 && ((pad + 0x10) as *const u32).read_volatile() & 8 != 0
    }
}

pub fn current_level() -> i16 {
    // SAFETY: fixed static address in the game's data section, always mapped.
    unsafe { (CURRENT_LEVEL as *const i16).read_volatile() }
}

/// `ChaoArea`: 0 lobby, 1 neutral, 2 hero, 3 dark, 4 race, 5 entrance, 6 kindergarten, 9 stadium, 10 karate.
pub fn current_chao_area() -> i32 {
    // SAFETY: fixed static address, always mapped.
    unsafe { (CURRENT_CHAO_AREA as *const i32).read_volatile() }
}

/// True when the player is standing in one of the three gardens.
pub fn in_garden() -> bool {
    current_level() == LEVEL_CHAO_WORLD && (1..=3).contains(&current_chao_area())
}

/// Copies one garden slot out of game memory (read-only).
pub fn read_chao_slot(index: usize) -> Option<ChaoData> {
    if index >= GARDEN_SLOTS {
        return None;
    }
    // The game loads the Chao into memory when Chao World first opens; until then, the save
    // file has them.
    if !chao_in_memory() {
        return saved_chao().get(index).cloned().flatten();
    }
    let ptr = (CHAO_SLOTS + index * CHAO_SIZE) as *const u8;
    // SAFETY: ChaoSlots is a static array in the game's data section; index is bounds-checked.
    let bytes = unsafe { std::slice::from_raw_parts(ptr, CHAO_SIZE) };
    ChaoData::from_bytes(bytes)
}

/// Any Chao (or egg) in memory: Chao World has loaded the Chao save.
fn chao_in_memory() -> bool {
    (0..GARDEN_SLOTS).any(|n| {
        // SAFETY: the `ChaoType` byte of each static slot.
        unsafe { ((CHAO_SLOTS + n * CHAO_SIZE + 0x80) as *const u8).read_volatile() != 0 }
    })
}

static CHAO_SAVE_PATH: std::sync::OnceLock<String> = std::sync::OnceLock::new();
/// The Chao save as last read: (file time, slots).
type SavedChao = (Option<std::time::SystemTime>, Vec<Option<ChaoData>>);
static SAVED_CHAO: std::sync::Mutex<Option<SavedChao>> = std::sync::Mutex::new(None);
/// Where the Chao slots are in the save file (memory's 0x19F6EC4, in the file's layout).
const FILE_CHAO_SLOTS: usize = 0x3AA4;

/// From the loader at startup: where the Chao save is (the loader gives its folder).
pub fn set_chao_save_path(path: String) {
    let path = std::path::Path::new(&path);
    let file = if path.is_dir() {
        path.join("SONIC2B__ALF")
    } else {
        path.to_path_buf()
    };
    let _ = CHAO_SAVE_PATH.set(file.to_string_lossy().into_owned());
}

/// The Chao in the save file, re-read when it changes.
fn saved_chao() -> Vec<Option<ChaoData>> {
    let Some(path) = CHAO_SAVE_PATH.get() else {
        return Vec::new();
    };
    let modified = std::fs::metadata(path).and_then(|m| m.modified()).ok();
    let mut cache = SAVED_CHAO.lock().unwrap_or_else(|e| e.into_inner());
    if let Some((at, slots)) = cache.as_ref()
        && *at == modified
    {
        return slots.clone();
    }
    let data = std::fs::read(path).unwrap_or_default();
    crate::log!("chao save: read {} bytes from {path}", data.len());
    let slots: Vec<Option<ChaoData>> = (0..GARDEN_SLOTS)
        .map(|n| {
            let at = FILE_CHAO_SLOTS + n * CHAO_SIZE;
            data.get(at..at + CHAO_SIZE).and_then(ChaoData::from_bytes)
        })
        .collect();
    *cache = Some((modified, slots.clone()));
    slots
}

/// The Chao we picked for a Chao Race, while at the race entrance or in the race (lane 0's
/// record, set by the entrance's Chao select; re/chao-race-sync.md).
pub fn race_chao() -> Option<ChaoData> {
    const LANE0_RECORD: usize = 0x01A5_CC00;
    const AREA_RACE: i32 = 4;
    const AREA_ENTRANCE: i32 = 5;
    if current_level() != LEVEL_CHAO_WORLD
        || !matches!(current_chao_area(), AREA_RACE | AREA_ENTRANCE)
    {
        return None;
    }
    // SAFETY: static pointer; when set it points at a full Chao record in the game's memory.
    unsafe {
        let record = (LANE0_RECORD as *const usize).read_volatile();
        if record == 0 {
            return None;
        }
        ChaoData::from_bytes(std::slice::from_raw_parts(record as *const u8, CHAO_SIZE))
    }
    .filter(|c| c.validate().is_ok())
}

/// Player 1's position and facing, if the player object exists.
pub fn player_position() -> Option<(Vector, i32)> {
    // SAFETY: MainCharObj1 is a static pointer array; entries are null when no player exists.
    unsafe {
        let entity = (MAIN_CHAR_OBJ1 as *const *const u8).read_volatile();
        if entity.is_null() {
            return None;
        }
        let pos = (entity.add(ENTITY_POSITION) as *const Vector).read_unaligned();
        let rot_y = (entity.add(ENTITY_ROTATION_Y) as *const i32).read_unaligned();
        Some((pos, rot_y))
    }
}

/// Everything a puppet needs to copy the local player's pose.
#[derive(Clone, Copy, Debug)]
pub struct LocalPlayer {
    pub character: u8,
    pub position: Vector,
    pub rotation: [i32; 3],
    pub anim: i16,
    pub frame: f32,
    /// `POSE_*` bits.
    pub flags: u8,
    /// `CharObj2Base::Upgrades`.
    pub upgrades: u32,
}

/// Player 1's EntityData1 (position, angles, action), or null when no player exists.
/// Moves our player to `at` (teleport).
pub fn set_player_position(at: Vector) {
    let entity = local_entity();
    if !entity.is_null() {
        // SAFETY: the player's live EntityData1, on the game thread.
        unsafe { (entity.add(ENTITY_POSITION) as *mut Vector).write_unaligned(at) };
    }
}

pub fn local_entity() -> *const u8 {
    // SAFETY: static pointer array in the game's data section.
    unsafe { (MAIN_CHAR_OBJ1 as *const *const u8).read_volatile() }
}

/// Player 1's CharObj2, or null when no player exists.
pub fn local_char_obj2() -> *const u8 {
    // SAFETY: static pointer array in the game's data section.
    unsafe { (MAIN_CHAR_OBJ2 as *const *const u8).read_volatile() }
}

/// Player 1's full pose (position, angles, animation), if the player exists.
pub fn local_player() -> Option<LocalPlayer> {
    let co2 = local_char_obj2();
    // SAFETY: both pointers come from the game's player arrays and are checked for null;
    // offsets are from the decompiled player code.
    unsafe {
        let entity = (MAIN_CHAR_OBJ1 as *const *const u8).read_volatile();
        if entity.is_null() || co2.is_null() {
            return None;
        }
        let rot = entity.add(ENTITY_ROTATION) as *const i32;
        // The Chao walkers keep Tails'/Eggman's walker frames at +1 and 0xD/0xE at +3, which
        // would read as Chaos/... : they're walkers 10 and 11 (re/characters.md).
        let character = match (co2.add(1).read(), co2.add(CO2_CHAR_ID).read()) {
            (6, 0x0D) => 10,
            (7, 0x0E) => 11,
            (_, id) => id,
        };
        // Same test as the player's display before it swaps in the ball model.
        let blending = (co2.add(CO2_MOTION_MODE) as *const i16).read_unaligned() == 2;
        // Sonic's main spawns the blue "JumpAura" ball every frame while the action is Jump (6)
        // or the bounce bracelet (69) (case 0x718544; re/spin-ball.md). Sonic family only.
        let action = entity.read();
        let ball = matches!(character, 0 | 1 | 8 | 9) && matches!(action, 6 | 69);
        Some(LocalPlayer {
            character,
            position: (entity.add(ENTITY_POSITION) as *const Vector).read_unaligned(),
            rotation: [
                rot.read_unaligned(),
                rot.add(1).read_unaligned(),
                rot.add(2).read_unaligned(),
            ],
            anim: (co2.add(CO2_ANIM) as *const i16).read_unaligned(),
            frame: (co2.add(CO2_FRAME) as *const f32).read_unaligned(),
            flags: if ball { POSE_BALL } else { 0 } | if blending { POSE_BLENDING } else { 0 },
            upgrades: (co2.add(CO2_UPGRADES) as *const u32).read_unaligned(),
        })
    }
}

type CreateChaoFn =
    unsafe extern "C" fn(*mut ChaoData, i32, *mut c_void, *mut Vector, i32) -> *mut c_void;

/// Spawns a Chao object from `data` at `position`.
///
/// # Safety
/// Must be called from the game thread while in a garden. `data` must stay alive and
/// unmoved for as long as the Chao object exists (the game keeps the pointer).
pub unsafe fn create_chao(data: *mut ChaoData, position: Vector, angle: i32) -> *mut c_void {
    let mut position = position;
    // SAFETY: address and signature from SA2Functions.h (header status, not yet verified).
    unsafe {
        let create: CreateChaoFn = std::mem::transmute(CREATE_CHAO);
        create(data, 0, std::ptr::null_mut(), &mut position, angle)
    }
}

/// Image base of sonic2app.exe.
const IMAGE_BASE: usize = 0x0040_0000;

/// Build fingerprint: PE `TimeDateStamp` + `SizeOfImage` from the in-memory header.
/// Our build (docs/game-version.md) gives 0x50601AE8 / 0x023FC000.
pub fn game_build() -> [u8; 8] {
    // SAFETY: the exe's PE header is always mapped at its image base (no ASLR on this exe).
    unsafe {
        let e_lfanew = ((IMAGE_BASE + 0x3C) as *const u32).read_unaligned() as usize;
        let nt = IMAGE_BASE + e_lfanew;
        let timestamp = ((nt + 8) as *const u32).read_unaligned();
        let size_of_image = ((nt + 24 + 56) as *const u32).read_unaligned();
        let mut out = [0; 8];
        out[..4].copy_from_slice(&timestamp.to_le_bytes());
        out[4..].copy_from_slice(&size_of_image.to_le_bytes());
        out
    }
}

/// Fingerprint of the build in docs/game-version.md, for tools running outside the game.
pub const KNOWN_GAME_BUILD: [u8; 8] = [0xE8, 0x1A, 0x60, 0x50, 0x00, 0xC0, 0x3F, 0x02];
