//! Game memory addresses (sonic2app.exe, see docs/game-version.md).
//! Each address is from the community headers and/or verified in Ghidra; see re/addresses.md.

use std::ffi::c_void;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicUsize, Ordering};

use crate::chao::{CHAO_SIZE, ChaoData};

/// `CurrentLevel` (SA2Variables.h, `__int16` at 0x1934B70).
const CURRENT_LEVEL: usize = 0x0193_4B70;
/// Stage control bytes the game's own goal routine (0x43E6D0) uses: `ControllerEnabled[0]`
/// (1 = player can move) and `TimerStopped` (1 = stage timer frozen).
pub(crate) const CONTROLLER_ENABLED: usize = 0x0174_AFFF;
const TIMER_STOPPED: usize = 0x0174_AFDA;
/// `MissionNum` (0..=4) and `GameState` (16 = in game), SA2Variables.h; heavily used in code.
const MISSION_NUM: usize = 0x0174_AFE3;
pub(crate) const GAME_STATE: usize = 0x0193_4BE0;
/// GameModeHandler 0x434160: 5 runs the stage; 7/8 load/run EVENT, 15/16 ENDING.
pub(crate) const GAME_MODE: usize = 0x0174_B040;
pub const GAME_STATE_INGAME: i16 = 16;
/// `CurrentChaoArea` (SA2Variables.h, `int` at 0x134046C).
const CURRENT_CHAO_AREA: usize = 0x0134_046C;
/// `ChaoSlots` (SA2Variables.h, `ChaoData[37]` at 0x19F6EC4) in the Chao save's first copy;
/// the second's are 0xCA6C on (`chao_slot_address`). We only touch the first 24.
const CHAO_SLOTS: usize = 0x019F_6EC4;
/// Which of the Chao save's two copies in memory is in use: the second only when this is
/// exactly 1 (GetSaveData 0x52E440, the garden load 0x531B10, 0x568630 all test `== 1`).
const CHAO_SAVE_IN_USE: usize = 0x0173_D06C;
/// Native menu return selector; zero initializes fresh title/menu panels (re/goto-travel.md).
const MENU_RETURN: usize = 0x01A5_59E8;
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
/// EntityData1 status bits (+4) and scale (+0x20).
const ENTITY_STATUS: usize = 0x04;
/// EntityData1 status: holding something (re/chao-garden-sync.md).
const STATUS_HOLDING: u16 = 0x800;
const ENTITY_SCALE: usize = 0x20;
/// CharObj2 bits Sonic's display checks (with 0x11) before drawing the spin ball.
const CO2_BALL_FLAGS: usize = 0x360;

/// CharObj2 offsets, confirmed in the draw and setup code (re/characters.md).
pub const CO2_CHAR_ID: usize = 0x03;
/// `CharObj2Base::Upgrades` (bit flags of the character's upgrades).
pub const CO2_UPGRADES: usize = 0x24;
pub const CO2_ANIM: usize = 0x178;
/// The character's speed (3 f32: forward, up, sideways, in its own frame), what the game's own
/// movement and animations go by (0x45B610 compares +0x64 with the physics' speeds at +0xE8..);
/// a hit sets it to the knock-back (0x46C340).
const CO2_SPEED: usize = 0x64;
pub const CO2_FRAME: usize = 0x184;
pub const CO2_ANIM_INFO: usize = 0x198;
const CO2_MOTION_MODE: usize = 0x174;
/// `{index, count, NJS_MOTION*}` x 300 at 0x1DEA700; the pointer field.
pub const MOTION_TABLE: usize = 0x01DE_A704;
pub const MOTION_COUNT: usize = 300;

/// `LocalPlayer::flags`: curled up (jump, spin dash, homing), with the blue spin ball around the
/// player.
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
/// `PlayerState::flags`: between places, loading or setting one up (`between_areas`): where
/// the state says it is isn't settled yet.
pub const POSE_LOADING: u8 = 128;

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

/// Game states inside a stage: in game, paused, a death's fade and restart, and the restarts
/// (re/stage-race.md 3, 4).
pub const STAGE_STATES: [i16; 7] = [GAME_STATE_INGAME, GAME_STATE_INGAME + 1, 5, 7, 9, 13, 15];

/// Stage runs: +1 each time a stage starts after we were out of one (no player, outside the
/// stage states) for a moment. The level number alone can't tell a replay of the same stage.
static RUN: std::sync::Mutex<(u32, Option<std::time::Instant>, bool)> =
    std::sync::Mutex::new((0, None, false));

/// Call once per frame (game thread).
pub fn track_run() {
    let out = local_entity().is_null() && !STAGE_STATES.contains(&game_state());
    let mut r = RUN.lock().unwrap_or_else(|e| e.into_inner());
    let (run, since, left) = &mut *r;
    if out {
        let at = *since.get_or_insert_with(std::time::Instant::now);
        if at.elapsed() >= std::time::Duration::from_secs(1) {
            *left = true;
        }
    } else {
        *since = None;
        if *left && game_state() == GAME_STATE_INGAME {
            *left = false;
            *run = run.wrapping_add(1);
        }
    }
}

/// The current stage run (see `track_run`).
pub fn stage_run() -> u32 {
    RUN.lock().unwrap_or_else(|e| e.into_inner()).0
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
/// Requests a stage transition (2 = death, 5 = time-limit restart, 7 = full restart).
/// Start/pause is instead the frame update's return value (REA: re/addresses.md).
const STATE_REQUEST: usize = 0x01A5_58A4;

static FROZEN: AtomicBool = AtomicBool::new(false);
/// Frame updates still allowed while frozen (so a stage gets its camera before freezing).
static GRACE: AtomicU32 = AtomicU32::new(0);
static LIVE_PAUSE: AtomicBool = AtomicBool::new(false);
static INGAME_UPDATE: AtomicUsize = AtomicUsize::new(0);
static PAUSE_DRAW: AtomicUsize = AtomicUsize::new(0);

/// A transition raised by objects while live-paused must reach the native state-16
/// dispatcher after Resume. Running more object updates can consume a one-shot death event.
#[derive(Default)]
struct PauseTransition {
    pending: Option<((i16, u8), u16)>,
}

impl PauseTransition {
    fn queue(&mut self, stage: (i16, u8), request: u16) -> bool {
        if matches!(request, 1..=5 | 7 | 8) && self.pending.is_none() {
            self.pending = Some((stage, request));
            return true;
        }
        false
    }

    fn retain(&mut self, stage: (i16, u8), state: i16) {
        if !matches!(state, GAME_STATE_INGAME | GAME_STATE_PAUSED)
            || self.pending.is_some_and(|(of, _)| of != stage)
        {
            self.pending = None;
        }
    }

    fn take(&mut self, stage: (i16, u8)) -> Option<u16> {
        self.pending
            .take()
            .filter(|(of, _)| *of == stage)
            .map(|(_, request)| request)
    }
}

static PAUSE_TRANSITION: std::sync::Mutex<PauseTransition> =
    std::sync::Mutex::new(PauseTransition { pending: None });

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum FriendExit {
    Stage,
    Chao,
    ChaoRace,
}

fn friend_exit(
    mode: i32,
    state: i16,
    level: i16,
    area: i32,
    player: bool,
    two_player: bool,
) -> Option<FriendExit> {
    if !matches!(mode, 5 | 10)
        || !matches!(state, GAME_STATE_INGAME | GAME_STATE_PAUSED)
        || !player
        || two_player
        || level <= 0
    {
        return None;
    }
    if level == LEVEL_CHAO_WORLD {
        return (area >= 0).then_some(if area == 4 {
            FriendExit::ChaoRace
        } else {
            FriendExit::Chao
        });
    }
    Some(FriendExit::Stage)
}

/// Begin the game's own exit to fresh menus to follow a friend. Game thread only.
/// Chao World finishes its manager's save before unloading. A Chao Race first returns to
/// its entrance; call again once that area is live to leave Chao World altogether.
pub fn leave_for_friend() -> bool {
    if !cfg!(all(windows, target_arch = "x86")) {
        return false;
    }
    let Some(exit) = friend_exit(
        game_mode(),
        game_state(),
        current_level(),
        current_chao_area(),
        !local_entity().is_null(),
        two_player(),
    ) else {
        return false;
    };
    const CHAO_MANAGER: usize = 0x01A5_AF04;
    const CHAO_NEXT_AREA: usize = 0x01DE_46A0;
    const BEGIN_EXIT: usize = 0x0043_C340;
    // SAFETY: the loaded game's static pointers. Chao saves need the manager, and the race
    // uses its registered area-change callback, exactly as native paused Quit does.
    unsafe {
        if exit == FriendExit::Chao && (CHAO_MANAGER as *const usize).read_volatile() == 0 {
            return false;
        }
        let change_area = (CHAO_NEXT_AREA as *const usize).read_volatile();
        if exit == FriendExit::ChaoRace && change_area == 0 {
            return false;
        }
        PAUSE_TRANSITION
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .pending = None;
        FROZEN.store(false, Ordering::Relaxed);
        GRACE.store(0, Ordering::Relaxed);
        LIVE_PAUSE.store(false, Ordering::Relaxed);
        (MENU_RETURN as *mut i32).write_volatile(0);
        (STATE_REQUEST as *mut u16).write_volatile(0);
        match exit {
            FriendExit::Stage => {
                // Native request 8's dispatch branch (0x43AB54), valid while paused too:
                // fade state 14 then native object/resource cleanup and menu return.
                (GAME_STATE as *mut i16).write_volatile(14);
                std::mem::transmute::<usize, extern "C" fn()>(BEGIN_EXIT)();
            }
            FriendExit::Chao => {
                // Native Chao Quit (0x43B334): the manager saves and raises request 1 when
                // complete. An ordinary stage exit would skip that save flow.
                (GAME_STATE as *mut i16).write_volatile(29);
            }
            FriendExit::ChaoRace => {
                // Native race Quit (0x43B33B): return to the race entrance, then fade via
                // state 12. Its own area loader and cleanup stay in charge.
                std::mem::transmute::<usize, extern "C" fn(i32)>(change_area)(5);
                (GAME_STATE as *mut i16).write_volatile(12);
                std::mem::transmute::<usize, extern "C" fn()>(BEGIN_EXIT)();
            }
        }
    }
    true
}

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

// Tabbing out (re/addresses.md "Running while unfocused"): under Proton the window is minimized
// on every focus loss (client 160x31), so the game can't draw then and idles (`frame_skip`);
// the network thread keeps the session alive meanwhile. Coming back is the risky part: with the
// Mod Loader's "pause when inactive" off, returning hung in the first frame's
// drawing (last mark "puppet display"), while with it on returning never hung. Its own
// reactivation handling (in its Present hook, 0x867AE0 → the loader) is what that setting
// switches, so `prepare_loader_profile` keeps it on. Forcing `WindowActive` (0x174F5AC) from the
// mod froze the game too.

/// At start: reads the Mod Loader's current display mode without changing it
/// (`mods/.modloader/profiles`: `Profiles.json` names it). "Pause when inactive" is kept on
/// from the next launch (an earlier build turned it off: that's undone), because the loader's
/// handling of the window coming back depends on it (above).
pub fn prepare_loader_profile(mod_path: &str) {
    let profiles = std::path::Path::new(mod_path)
        .parent()
        .unwrap_or(std::path::Path::new("mods"))
        .join(".modloader")
        .join("profiles");
    let Some(path) = std::fs::read_to_string(profiles.join("Profiles.json"))
        .ok()
        .and_then(|list| current_profile(&list))
        .map(|file| profiles.join(file))
    else {
        crate::log!("game: no Mod Loader profile found; tabbing out pauses the game");
        return;
    };
    let Ok(profile) = std::fs::read_to_string(&path) else {
        return;
    };
    let mode = json_value(&profile, "ScreenMode");
    let pauses = json_value(&profile, "EnablePauseOnInactive");
    // SA2 Mod Manager: 0 windowed, 1 borderless, 2 custom window, 3 fullscreen.
    // `run_unfocused` may keep the game going without focus only when this launch isn't
    // fullscreen and the loader's own pause is off (with it on, the loader handles inactivity
    // its own way and we stay out of it).
    WINDOWED.store(
        matches!(mode, Some("0" | "1" | "2")) && pauses == Some("false"),
        Ordering::Relaxed,
    );
    crate::log!("game: Mod Loader screen mode {mode:?}, pause when inactive {pauses:?}");
    let mut changed = profile.clone();
    if pauses == Some("false") {
        changed = set_json_value(&changed, "EnablePauseOnInactive", "true").unwrap_or(changed);
    }
    if changed == profile {
        return;
    }
    match std::fs::write(&path, changed) {
        Ok(()) => {
            crate::log!("game: Mod Loader profile set to pause when inactive on");
            crate::hud::toast(
                "From your next launch the game pauses while tabbed out \
                 (the others keep playing)",
            );
        }
        Err(e) => crate::log!("game: can't update {}: {e}", path.display()),
    }
}

/// WinMain's loop asks 0x425DA0 before each frame (its call at 0x401DE0) whether to skip it:
/// yes while `WindowActive` (0x174F5AC, kept by the game's own window procedure on
/// WM_ACTIVATEAPP, 0x40186C) is 0, otherwise whether the D3D device is lost (0x1A5570B, reset
/// there). `frame_skip` stands in for that call: online, in a windowed or borderless game, it
/// lets the frame run without focus, but only when the window can take a frame (not minimized,
/// a client area to draw to), the device isn't lost and the game isn't paused. `WindowActive`
/// itself is never touched (forcing it hangs the game).
const SKIP_FRAME_CALL: usize = 0x0040_1DE0;
const WINDOW_ACTIVE: usize = 0x0174_F5AC;
const DEVICE_LOST: usize = 0x01A5_570B;
pub(crate) const GAME_STATE_PAUSED: i16 = 17;

static WINDOWED: AtomicBool = AtomicBool::new(false);
static RUNS_UNFOCUSED: AtomicBool = AtomicBool::new(false);
static SKIP_FRAME: AtomicUsize = AtomicUsize::new(0);
/// `WindowActive` as `frame_skip` last saw it (for its log line on each focus change).
static LAST_ACTIVE: AtomicU32 = AtomicU32::new(1);
/// While we keep the game running without focus: the controller flag as we found it when focus
/// went (restored when it comes back), or None while focused.
static UNFOCUSED_CONTROLS: std::sync::Mutex<Option<u8>> = std::sync::Mutex::new(None);

/// Hooks WinMain's frame-skip check (`run_unfocused`). Call once at startup.
pub fn install_frame_skip() {
    let target = frame_skip as extern "C" fn() -> u8 as usize;
    match crate::patch::redirect_call(SKIP_FRAME_CALL, target) {
        Some(original) => SKIP_FRAME.store(original, Ordering::SeqCst),
        None => crate::log!("game: can't hook {SKIP_FRAME_CALL:#x}; tabbing out stops the game"),
    }
}

/// Returns nonzero to skip the frame (WinMain then only idles: 0x401E53).
extern "C" fn frame_skip() -> u8 {
    let original = SKIP_FRAME.load(Ordering::SeqCst);
    // SAFETY: the game's own check, which this call site called (no arguments, result in AL).
    let skip = unsafe { std::mem::transmute::<usize, extern "C" fn() -> u8>(original)() };
    // SAFETY: static game variables, read on the game thread.
    let (active, lost) = unsafe {
        (
            (WINDOW_ACTIVE as *const u32).read_volatile(),
            (DEVICE_LOST as *const u8).read_volatile(),
        )
    };
    // Each focus change: why the next frames run or not (this runs every loop pass, idle too).
    if LAST_ACTIVE.swap(active, Ordering::Relaxed) != active {
        let (iconic, (width, height)) = crate::input::game_window_state();
        crate::log!(
            "game: focus {}: online {}, not fullscreen {}, device lost {lost}, game state {}, \
             minimized {iconic}, client {width}x{height}, game skips {skip}",
            if active != 0 { "back" } else { "gone" },
            RUNS_UNFOCUSED.load(Ordering::Relaxed),
            WINDOWED.load(Ordering::Relaxed),
            game_state(),
        );
    }
    if skip == 0 || !RUNS_UNFOCUSED.load(Ordering::Relaxed) {
        return skip;
    }
    // Skipped for another reason than focus, or nothing to draw to: as the game decides.
    if active != 0 || lost != 0 || game_state() == GAME_STATE_PAUSED {
        return skip;
    }
    if !crate::input::game_window_drawable() {
        return skip;
    }
    0
}

/// Online, a windowed or borderless game keeps running while another window has focus (the
/// player stands still: their pad and keyboard are ignored meanwhile). Fullscreen is left
/// alone: there D3D9 loses its device on focus loss and the game must wait.
pub fn run_unfocused(on: bool) {
    let on = on && WINDOWED.load(Ordering::Relaxed);
    RUNS_UNFOCUSED.store(on, Ordering::Relaxed);
    // Without focus our player ignores the controller (the pad still reads in the background).
    let focused = !on || crate::input::game_focused();
    let mut held = UNFOCUSED_CONTROLS.lock().unwrap_or_else(|e| e.into_inner());
    // SAFETY: a static byte the game itself toggles (goal routine 0x43E6D0).
    unsafe {
        let at = CONTROLLER_ENABLED as *mut u8;
        match (focused, *held) {
            (false, None) => {
                *held = Some(at.read_volatile());
                at.write_volatile(0);
            }
            (true, Some(was)) => {
                // Unless the game changed it meanwhile (a goal, a cutscene).
                if at.read_volatile() == 0 {
                    at.write_volatile(was);
                }
                *held = None;
            }
            _ => {}
        }
    }
}

/// The value after `"key":` in a JSON text (the first one), when it's a number or a word.
fn json_value<'a>(text: &'a str, key: &str) -> Option<&'a str> {
    let (start, len) = json_value_at(text, key)?;
    Some(&text[start..start + len])
}

/// `text` with the value after `"key":` (the first one, a number or a word) replaced.
fn set_json_value(text: &str, key: &str, value: &str) -> Option<String> {
    let (start, len) = json_value_at(text, key)?;
    Some(format!("{}{value}{}", &text[..start], &text[start + len..]))
}

fn json_value_at(text: &str, key: &str) -> Option<(usize, usize)> {
    let quoted = format!("\"{key}\"");
    let key_at = text.find(&quoted)? + quoted.len();
    let rest = text[key_at..].trim_start().strip_prefix(':')?;
    let start = text.len() - rest.trim_start().len();
    let len = text[start..]
        .find(|c: char| !(c.is_ascii_alphanumeric() || c == '-'))
        .unwrap_or(text.len() - start);
    (len > 0).then_some((start, len))
}

/// The profile file `Profiles.json` points at (`ProfileIndex` into `ProfilesList`).
fn current_profile(list: &str) -> Option<String> {
    let index = usize::try_from(json_number(list, "ProfileIndex")?).ok()?;
    let mut rest = list;
    let mut files = Vec::new();
    while let Some(at) = rest.find("\"Filename\"") {
        rest = &rest[at + "\"Filename\"".len()..];
        let open = rest.find('"')? + 1;
        let len = rest[open..].find('"')?;
        files.push(rest[open..open + len].to_string());
        rest = &rest[open + len..];
    }
    files.into_iter().nth(index)
}

/// The number after `"key":` in a JSON text (the first one).
fn json_number(text: &str, key: &str) -> Option<i64> {
    json_value(text, key)?.parse().ok()
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

const RESTORE_SOUNDS: usize = 0x0043_5FD0;

/// Native completion flag, set by LevelClear and reset by the game's stage/restart setup.
pub fn stage_cleared() -> bool {
    unsafe { (0x0174_B002 as *const u8).read_volatile() != 0 }
}

/// Run the same victory/results path used by a locally defeated boss. The caller must have
/// received a confirmed co-op boss clear; wait through local death, loading and the start hold.
pub fn complete_shared_boss() -> bool {
    if !cfg!(all(windows, target_arch = "x86")) || stage_cleared() {
        return false;
    }
    if game_state() != GAME_STATE_INGAME
        || two_player()
        || local_entity().is_null()
        || FROZEN.load(Ordering::Relaxed)
        || unsafe {
            (CONTROLLER_ENABLED as *const u8).read_volatile() == 0
                || (STATE_REQUEST as *const u16).read_volatile() != 0
        }
    {
        return false;
    }
    // REA: 0x43E6D0 uses ESI as player, sets the clear/control/timer flags, creates the native
    // results task and applies the player's victory action. It guards against duplicate wins.
    #[cfg(target_arch = "x86")]
    unsafe {
        if std::slice::from_raw_parts(0x0043_E6D0 as *const u8, 8)
            != [0x53, 0x33, 0xD2, 0x55, 0x57, 0x38, 0x15, 0x02]
        {
            return false;
        }
        std::arch::asm!(
            "push esi", "xor esi, esi", "call eax", "pop esi",
            in("eax") 0x0043_E6D0usize, clobber_abi("C"));
    }
    stage_cleared()
}

/// In a stage started together, the pause screen doesn't stop the game (like online games):
/// the stage keeps running behind the menu, only our player ignores the controller.
pub fn set_live_pause(live: bool) {
    LIVE_PAUSE.store(live, Ordering::Relaxed);
    PAUSE_TRANSITION
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .retain((current_level(), mission()), game_state());
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
    crate::watch::mark("pause draw");
    let update = INGAME_UPDATE.load(Ordering::SeqCst);
    let stage = (current_level(), mission());
    let pending = {
        let mut transition = PAUSE_TRANSITION.lock().unwrap_or_else(|e| e.into_inner());
        transition.retain(stage, game_state());
        // A request can already exist when the pause menu opens on the death frame.
        let request = unsafe { (STATE_REQUEST as *const u16).read_volatile() };
        transition.queue(stage, request);
        transition.pending.is_some()
    };
    if LIVE_PAUSE.load(Ordering::Relaxed)
        && !FROZEN.load(Ordering::Relaxed)
        && update != 0
        && !pending
    {
        // SAFETY: static game variables, and the game's own cdecl frame update without
        // arguments. The player ignores the pad (it drives the menu now). Ignore only the
        // returned Start press; preserve actual stage-transition requests for Resume.
        unsafe {
            let controller = (CONTROLLER_ENABLED as *mut u8).read_volatile();
            let request = (STATE_REQUEST as *mut u16).read_volatile();
            (CONTROLLER_ENABLED as *mut u8).write_volatile(0);
            std::mem::transmute::<usize, extern "C" fn() -> i32>(update)();
            let next = (STATE_REQUEST as *const u16).read_volatile();
            if PAUSE_TRANSITION
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .queue(stage, next)
            {
                crate::log!(
                    "game: queued stage request {next} during pause; Resume will finish it"
                );
            }
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
    crate::watch::mark("stage update");
    if let Some(request) = PAUSE_TRANSITION
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .take((current_level(), mission()))
    {
        // Resume already ran the native pause cleanup. Skip one object update, letting the
        // native state-16 dispatcher consume this request exactly once (including lives,
        // game-over, fade and checkpoint handling). Do not manufacture a GameState here.
        unsafe { (STATE_REQUEST as *mut u16).write_volatile(request) };
        crate::log!("game: resumed with pending stage request {request}");
        return 0;
    }
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

/// While online, the main menu's 2P entry (PARTY BATTLES) does nothing in the game itself:
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
const CURRENT_MENU: usize = 0x01D7_BB10;
const CURRENT_SUB_MENU: usize = 0x01D7_BB14;
pub const MENU_TITLE: i32 = 0;
pub const MENU_MAIN: i32 = 1;
const SUB_MAIN_LIST: i32 = 2;
/// Past the main menu's SubMenu jump table (0..0x1B): its update does nothing then.
const SUB_WAITING: i32 = 0x1C;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum FriendMenu {
    Title,
    Main,
}

fn friend_menu(menu: i32, sub: i32, save_chosen: bool, idle: bool) -> Option<FriendMenu> {
    if !idle {
        return None;
    }
    match (menu, sub) {
        (MENU_TITLE, 2) if save_chosen => Some(FriendMenu::Title),
        (MENU_MAIN, SUB_MAIN_LIST | 4 | SUB_WAITING) => Some(FriendMenu::Main),
        _ => None,
    }
}

/// Advance initialized title/main menus toward Stage Select for a pending friend join.
/// Returns true only when a native menu transition starts; retry while panels slide.
/// A title screen without a selected save remains on the native save-selection path.
pub fn open_stage_select_for_friend() -> bool {
    if !cfg!(all(windows, target_arch = "x86")) || game_mode() != 0xE || two_player() {
        return false;
    }
    const STAGE_PANEL: usize = 0x00C7_7F44;
    const SETUP_MAIN: usize = 0x0066_4B60;
    // SAFETY: native menu pointers, checked before reading their fields or calling their
    // setup. This only operates on idle title/main menus, never a loading/save screen.
    unsafe {
        let header = (MENU_HEADER as *const usize).read_volatile();
        let main = (MAIN_MENU as *const usize).read_volatile();
        if header == 0 || main == 0 || (STAGE_PANEL as *const usize).read_volatile() == 0 {
            return false;
        }
        let menu = (CURRENT_MENU as *const i32).read_volatile();
        let sub = (CURRENT_SUB_MENU as *const i32).read_volatile();
        let idle = ((header + 8) as *const u32).read_volatile() == 0
            && ((main + 8) as *const u32).read_volatile() == 0;
        let save = (CHAO_SAVE_IN_USE as *const i32).read_volatile();
        match friend_menu(menu, sub, save >= 0, idle) {
            Some(FriendMenu::Title) => {
                // Native title confirmation's save-selected branch (0x671C96/0x671E12).
                // With no save chosen the game must show its save picker instead.
                ((header + 4) as *mut u32).write_volatile(0);
                std::mem::transmute::<usize, extern "C" fn()>(SETUP_MAIN)();
                (CURRENT_MENU as *mut i32).write_volatile(MENU_MAIN);
            }
            Some(FriendMenu::Main) => leave_main_menu(PANEL_STAGE_SELECT),
            None => return false,
        }
    }
    true
}

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

/// The loader's active Chao save file, including save redirection.
pub fn chao_save_path() -> Option<&'static std::path::Path> {
    CHAO_SAVE_PATH.get().map(std::path::Path::new)
}

/// `GameMode` (0x174B040): 5 and 10 run a stage through GameState 0x43A780, 0xE the menus,
/// 4/6/9 load a stage, 7/8 an event, 15/16 the ending (GameModeHandler 0x434160).
pub fn game_mode() -> i32 {
    // SAFETY: static int in the game's data section.
    unsafe { (GAME_MODE as *const i32).read_volatile() }
}

/// Between places: a load or an event (GameMode other than 5/10 and the menus 0xE), or in a
/// stage or Chao World area that isn't running yet (GameState not 16/17, in game or paused)
/// or has no player (an area being torn down or made, as between gardens).
pub fn between_areas() -> bool {
    match game_mode() {
        0xE => false,
        5 | 10 => !matches!(game_state(), 16 | 17) || local_entity().is_null(),
        _ => true,
    }
}

/// How the Chao save can be snapshotted right now.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SaveScope {
    /// In a loaded garden: the full pre-save pass 0x52F090 (live items/Chao write-back).
    Garden,
    /// Anywhere else that isn't loading: only the checksum 0x52EEE0 over the buffer as is.
    Elsewhere,
}

/// Where a Chao save snapshot is safe now, if anywhere. Needs the Chao save in memory. In a
/// garden or stage (GameMode 5/10, Chao World's other areas included) it must be running or
/// paused (GameState 16/17) with a player; the menus (GameMode 0xE) are safe as is. Loads,
/// events, endings and the death/restart states are not.
pub fn chao_save_scope() -> Option<SaveScope> {
    if !chao_in_memory() {
        return None;
    }
    let running = matches!(game_state(), 16 | 17) && !local_entity().is_null();
    if in_garden() {
        return running.then_some(SaveScope::Garden);
    }
    match game_mode() {
        5 | 10 if running => Some(SaveScope::Elsewhere),
        0xE => Some(SaveScope::Elsewhere),
        _ => None,
    }
}

/// The two copies of the Chao save in memory (0x19F6460, +0xCA6C); `0x173D06C == 1` picks
/// the second (GetSaveData 0x52E440 returns the active one).
#[cfg(all(windows, target_arch = "x86"))]
const CHAO_SAVE_BASE: usize = 0x019F_6460;
pub const CHAO_SAVE_SIZE: usize = 0xCA6C;

/// Copies the active Chao save region after the game's own write-back/checksum. In a garden
/// that's the whole pre-save pass 0x52F090; elsewhere the garden's item registry and Chao
/// objects aren't loaded, so only the checksum 0x52EEE0 runs over the buffer as it is (the
/// game wrote the garden back into it on leaving). The old AL_Save wrapper depends on mounted
/// file descriptors that aren't ready during normal play.
pub fn chao_save_snapshot() -> Result<Vec<u8>, String> {
    let scope = chao_save_scope().ok_or("wait for Chao World data to finish loading")?;
    #[cfg(all(windows, target_arch = "x86"))]
    // SAFETY: the game's own pre-save pass, active-buffer getter and checksum, called on the
    // game thread. GetSaveData normalizes the two-copy selector itself; the checksum is only
    // run on one of the two static buffers.
    unsafe {
        if scope == SaveScope::Garden {
            std::mem::transmute::<usize, unsafe extern "C" fn()>(0x0052_F090)();
        }
        let base = std::mem::transmute::<usize, unsafe extern "C" fn() -> usize>(0x0052_E440)();
        if base != CHAO_SAVE_BASE && base != CHAO_SAVE_BASE + CHAO_SAVE_SIZE {
            return Err("the active Chao save buffer is unavailable".into());
        }
        if scope == SaveScope::Elsewhere {
            // 0x52EEE0: usercall ESI = save base; clobbers EAX/ECX/EDX (and calls rand),
            // restores the x87 control word it changes.
            std::arch::asm!(
                "push esi",
                "mov esi, ecx",
                "call eax",
                "pop esi",
                inout("ecx") base => _,
                inout("eax") 0x0052_EEE0usize => _,
                clobber_abi("C"),
            );
        }
        Ok(std::slice::from_raw_parts(base as *const u8, CHAO_SAVE_SIZE).to_vec())
    }
    #[cfg(not(all(windows, target_arch = "x86")))]
    {
        let _ = scope;
        Err("saving is only available in the game".into())
    }
}

/// Gives player 1 `count` rings the game's way (AddRings 0x44CE10: EAX = player, DX = count;
/// clamps at 999 and handles the extra lives at every 100).
pub fn add_rings(count: i16) {
    #[cfg(target_arch = "x86")]
    // SAFETY: the game's own usercall, on the game thread; it saves the registers it uses
    // besides EAX/ECX/EDX.
    unsafe {
        std::arch::asm!(
            "call {f}",
            f = in(reg) 0x0044_CE10usize,
            inout("eax") 0 => _,
            inout("edx") i32::from(count) => _,
            clobber_abi("C"),
        );
    }
    #[cfg(not(target_arch = "x86"))]
    let _ = count;
}

/// Save slot `index`'s record in the Chao save in use, the one the game's garden loads its
/// Chao from and saves (re/chao-details.md "The Chao save checksum"). Every read and write
/// of our save's Chao goes through here.
pub fn chao_slot_address(index: usize) -> usize {
    // SAFETY: the game's static "which Chao save" value.
    let second = unsafe { (CHAO_SAVE_IN_USE as *const u32).read_volatile() } == 1;
    CHAO_SLOTS + usize::from(second) * CHAO_SAVE_SIZE + index * CHAO_SIZE
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
    let ptr = chao_slot_address(index) as *const u8;
    // SAFETY: ChaoSlots is a static array in the game's data section (either save copy);
    // index is bounds-checked.
    let bytes = unsafe { std::slice::from_raw_parts(ptr, CHAO_SIZE) };
    ChaoData::from_bytes(bytes)
}

/// Any Chao (or egg) in memory: Chao World has loaded the Chao save.
pub fn chao_in_memory() -> bool {
    (0..GARDEN_SLOTS).any(|n| {
        // SAFETY: the `ChaoType` byte of each static slot.
        unsafe { ((chao_slot_address(n) + 0x80) as *const u8).read_volatile() != 0 }
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
    /// Curled into a ball, by the same test Sonic's display uses to swap in the ball model
    /// (0x720090, re/skins.md): status +4 & 0x100, not blending, not the Chao walker, and
    /// co2+0x360 & 0x11.
    pub curled: bool,
    /// The entity's scale (+0x20, normally 1).
    pub scale: f32,
    /// Standing on the ground or an object (status +4 & 3).
    pub grounded: bool,
    /// EntityData1 action (re/skins.md "Garden actions": 43 pet, 35 shake a tree, ...).
    pub action: u8,
    /// Holding something (status +4 & 0x800: a Chao, fruit, ...).
    pub holding: bool,
    /// Ground speed (units a frame), from the game's own speed (forward and sideways).
    pub speed: f32,
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

/// EntityData1 +6: the time left a character can't be hurt again (u16, counting down after a
/// hit). The characters' displays (Sonic's 0x720090) and their shadows (0x71E520) skip the body
/// while its bit 2 is set: the blink. Not while 0x174B009 is set (the split screen's view of
/// one player is being drawn).
pub const ENTITY_INVULNERABLE: usize = 0x06;
pub const SPLIT_VIEW: usize = 0x0174_B009;

/// Player 1 was just hurt and can't be hurt again yet (it blinks).
pub fn local_invulnerable() -> bool {
    let entity = local_entity();
    // SAFETY: the live player's EntityData1 (null-checked).
    !entity.is_null()
        && unsafe { (entity.add(ENTITY_INVULNERABLE) as *const u16).read_unaligned() } != 0
}

/// A character blinks out on frames where `timer` has bit 2, as the game draws them (unless the
/// split screen draws one player's view).
pub fn blinks_out(timer: u32) -> bool {
    // SAFETY: a static byte in the game's data section.
    timer & 2 != 0 && unsafe { (SPLIT_VIEW as *const u8).read_volatile() } == 0
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
        // Sonic's main spawns the blue ball every frame while the action is Jump (6), the bounce
        // bracelet (69), spin charge (3) or spin dash / rolling (4, 5), and once as the homing
        // attack starts (13) (re/spin-ball.md, re/effects.md). Sonic family only.
        let action = entity.read();
        let ball = matches!(character, 0 | 1 | 8 | 9) && matches!(action, 3..=6 | 13 | 69);
        let curled = (entity.add(ENTITY_STATUS) as *const u16).read_unaligned() & 0x100 != 0
            && !blending
            && co2.add(CO2_CHAR_ID).read() != 0x0C
            && (co2.add(CO2_BALL_FLAGS) as *const u32).read_unaligned() & 0x11 != 0;
        let scale = (entity.add(ENTITY_SCALE) as *const f32).read_unaligned();
        let status = (entity.add(ENTITY_STATUS) as *const u16).read_unaligned();
        let grounded = status & 3 != 0;
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
            curled,
            scale: if scale.is_finite() && scale > 0.0 {
                scale
            } else {
                1.0
            },
            grounded,
            action,
            holding: status & STATUS_HOLDING != 0,
            speed: {
                let v = co2.add(CO2_SPEED) as *const f32;
                let (forward, sideways) = (v.read_unaligned(), v.add(2).read_unaligned());
                let speed = (forward * forward + sideways * sideways).sqrt();
                if speed.is_finite() { speed } else { 0.0 }
            },
        })
    }
}

/// How far through its current animation the local player is (0..1), from its animation
/// table and the motion table (as the display finds the motion; re/skins.md).
pub fn local_anim_progress() -> Option<f32> {
    const ANIM_INFO_SIZE: usize = 0x10;
    const MAX_ANIM: usize = 256;
    const MOTION_STRIDE: usize = 8;
    let co2 = local_char_obj2();
    if co2.is_null() {
        return None;
    }
    // SAFETY: the live CharObj2 and the game's own tables, every index checked first.
    unsafe {
        let anim_info = (co2.add(CO2_ANIM_INFO) as *const usize).read_unaligned();
        let anim = (co2.add(CO2_ANIM) as *const i16).read_unaligned();
        let frame = (co2.add(CO2_FRAME) as *const f32).read_unaligned();
        if anim_info == 0 || anim < 0 || anim as usize >= MAX_ANIM || !frame.is_finite() {
            return None;
        }
        let base = ((anim_info + anim as usize * ANIM_INFO_SIZE) as *const i16).read_unaligned();
        if base < 0 || base as usize >= MAX_ANIM {
            return None;
        }
        let motion_index =
            ((anim_info + base as usize * ANIM_INFO_SIZE) as *const i16).read_unaligned();
        if motion_index < 0 || motion_index as usize >= MOTION_COUNT {
            return None;
        }
        let motion =
            ((MOTION_TABLE + motion_index as usize * MOTION_STRIDE) as *const usize).read();
        if motion == 0 {
            return None;
        }
        let frames = ((motion + 4) as *const u32).read_unaligned();
        (frames > 1).then(|| (frame / (frames - 1) as f32).clamp(0.0, 1.0))
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn friend_travel_uses_stage_cleanup_or_the_chao_save_flow() {
        for mode in [5, 10] {
            for state in [GAME_STATE_INGAME, GAME_STATE_PAUSED] {
                assert_eq!(
                    friend_exit(mode, state, 13, 0, true, false),
                    Some(FriendExit::Stage)
                );
                for area in [0, 1, 2, 3, 5, 9, 10] {
                    assert_eq!(
                        friend_exit(mode, state, LEVEL_CHAO_WORLD, area, true, false),
                        Some(FriendExit::Chao)
                    );
                }
                assert_eq!(
                    friend_exit(mode, state, LEVEL_CHAO_WORLD, 4, true, false),
                    Some(FriendExit::ChaoRace)
                );
            }
        }
    }

    #[test]
    fn friend_travel_waits_through_loads_deaths_and_invalid_chao_areas() {
        for mode in [0, 2, 4, 6, 7, 8, 9, 13, 14, 15, 16] {
            assert_eq!(
                friend_exit(mode, GAME_STATE_INGAME, 13, 0, true, false),
                None
            );
        }
        for state in [0, 5, 7, 9, 12, 13, 14, 15, 21, 29] {
            assert_eq!(friend_exit(5, state, 13, 0, true, false), None);
        }
        assert_eq!(friend_exit(5, GAME_STATE_INGAME, 13, 0, false, false), None);
        assert_eq!(friend_exit(5, GAME_STATE_PAUSED, 13, 0, true, true), None);
        assert_eq!(friend_exit(5, GAME_STATE_INGAME, 0, 0, true, false), None);
        for area in [-2, -1] {
            assert_eq!(
                friend_exit(5, GAME_STATE_INGAME, LEVEL_CHAO_WORLD, area, true, false),
                None
            );
        }
    }

    #[test]
    fn friend_menu_navigation_preserves_save_selection_and_waits_for_idle_panels() {
        assert_eq!(friend_menu(MENU_TITLE, 2, false, true), None);
        assert_eq!(
            friend_menu(MENU_TITLE, 2, true, true),
            Some(FriendMenu::Title)
        );
        for sub in [SUB_MAIN_LIST, 4, SUB_WAITING] {
            assert_eq!(
                friend_menu(MENU_MAIN, sub, true, true),
                Some(FriendMenu::Main)
            );
            assert_eq!(friend_menu(MENU_MAIN, sub, true, false), None);
        }
        for (menu, sub) in [
            (MENU_TITLE, 1),
            (MENU_TITLE, 3),
            (MENU_MAIN, 0),
            (MENU_MAIN, 5),
            (9, 2),
            (16, 2),
        ] {
            assert_eq!(friend_menu(menu, sub, true, true), None);
        }
    }

    #[test]
    fn live_pause_keeps_a_death_until_resume_and_delivers_it_once() {
        let stage = (23, 0);
        let mut transition = PauseTransition::default();
        assert!(!transition.queue(stage, 0));
        assert!(transition.queue(stage, 2));
        for _ in 0..30 {
            transition.retain(stage, GAME_STATE_PAUSED);
            assert!(!transition.queue(stage, 0));
        }
        assert!(!transition.queue(stage, 7)); // the first native transition wins
        transition.retain(stage, GAME_STATE_INGAME);
        assert_eq!(transition.take(stage), Some(2));
        assert_eq!(transition.take(stage), None);
    }

    #[test]
    fn queued_pause_transitions_cannot_follow_a_restart_or_level_change() {
        let mut transition = PauseTransition::default();
        for state in [5, 7, 9, 13, 15] {
            assert!(transition.queue((23, 0), 2));
            transition.retain((23, 0), state);
            assert_eq!(transition.take((23, 0)), None);
        }
        assert!(transition.queue((23, 0), 5));
        transition.retain((24, 0), GAME_STATE_INGAME);
        assert_eq!(transition.take((24, 0)), None);
        assert!(transition.queue((23, 0), 7));
        assert_eq!(transition.take((23, 1)), None);
    }

    #[test]
    fn reads_the_loader_profile() {
        let list = r#"{ "ProfileIndex": 1, "ProfilesList": [
            { "Name": "Default", "Filename": "Default.json" },
            { "Name": "Online", "Filename": "Online.json" } ] }"#;
        assert_eq!(current_profile(list).as_deref(), Some("Online.json"));
        let profile = r#"{ "Graphics": { "SelectedScreen": 1, "ScreenMode": 2 } }"#;
        assert_eq!(json_number(profile, "ScreenMode"), Some(2));
        assert_eq!(json_number(profile, "Missing"), None);
        assert_eq!(
            set_json_value(profile, "ScreenMode", "1").as_deref(),
            Some(r#"{ "Graphics": { "SelectedScreen": 1, "ScreenMode": 1 } }"#)
        );
        let pause = r#"{ "EnablePauseOnInactive": true, "X": 1 }"#;
        assert_eq!(json_value(pause, "EnablePauseOnInactive"), Some("true"));
        assert_eq!(
            set_json_value(pause, "EnablePauseOnInactive", "false").as_deref(),
            Some(r#"{ "EnablePauseOnInactive": false, "X": 1 }"#)
        );
    }
}
