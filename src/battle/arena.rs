//! The karate arena as the battle's stage (re/chao-battle-arena.md): the karate main object's
//! menu held and left, Chao of our own made with `CreateChao` and kept in place, their motions,
//! the camera and sounds. Only reads and writes game memory; game.rs decides what happens.

// Game memory exists only in the 32-bit game build.
#![cfg_attr(not(target_arch = "x86"), allow(dead_code))]

use std::sync::Mutex;

use crate::chao::{ChaoData, TYPE_EGG};
use crate::game::{self, Vector};

/// Karate globals `g` (pointer) and the karate main object `M`.
const KARATE_GLOBALS: usize = 0x01A5_D148;
const KARATE_MAIN: usize = 0x01A5_D14C;
/// The karate main object's MainSub.
pub const MAIN_EXEC: usize = 0x0054_2DA0;
/// `ChaoArea_Karate`.
pub const AREA_KARATE: i32 = 10;
/// Game paused: the karate objects and Chao stop.
const PAUSED: usize = 0x0174_AFF7;
/// Menu buttons pressed this frame, player 1 (built from the pad and keyboard by 0x441AB0).
const MENU_BUTTONS: usize = 0x01DE_FAB0;
/// The byte karate's exit clears before it leaves (state 0xF, 0x542E83).
const EXIT_CLEARS: usize = 0x0174_AFD6;
/// `GoToChaoArea(area)` 0x52B5B0 (cdecl).
#[cfg(target_arch = "x86")]
const GO_TO_CHAO_AREA: usize = 0x0052_B5B0;

// Offsets in `g`.
/// Set while a menu part (the banner) is busy; the mode select reads no buttons then.
const G_BUSY: usize = 0x03;
const G_VS: usize = 0x05;
/// The mode select's cursor: 0 the fight, -1 leave.
const G_CURSOR: usize = 0x0A;
const G_CAMERA: usize = 0x38;

// Karate main states (`MD[0]`) and the mode select's sub-states (`MD[1]`).
/// No handler: the main object idles.
pub const STATE_IDLE: u8 = 1;
const STATE_MENU: u8 = 2;
pub const STATE_MODE_SELECT: u8 = 3;
const STATE_CONFIRM: u8 = 7;
const STATE_SPAWN: u8 = 9;
/// After the exit: the main object only holds `G_BUSY` set until the area goes.
const STATE_GONE: u8 = 0x10;
const SUB_INPUT: u8 = 1;
/// A sub-state that returns at once: the window stays up and reads no buttons.
const SUB_HELD: u8 = 2;

// Objects: MainSub at +0x10, Data1 at +0x34, Data2 at +0x38, work at +0x40.
const OBJ_MAIN: usize = 0x10;
const OBJ_DATA1: usize = 0x34;
const OBJ_DATA2: usize = 0x38;
const OBJ_WORK: usize = 0x40;
/// The karate camera's work: position and look-at, applied by its callback every frame.
const CAMERA_POSITION: usize = 0x08;
const CAMERA_LOOK: usize = 0x20;
/// The menu view (camera state 0's values).
const MENU_CAMERA: ([f32; 3], [f32; 3]) = ([0.0, 20.0, 127.0], [0.0, 11.5, 0.0]);

/// `CreateChao(record, copy, cpu template, &position, angle)`.
const CREATE_CHAO: usize = 0x0055_01D0;
/// The Chao MainSub `CreateChao` sets.
const CHAO_MAIN: usize = 0x0054_FE20;
/// Setting an object's MainSub to DeleteObject removes it on the next object pass.
const DELETE_OBJECT: usize = 0x0046_F720;
// Chao Data1.
const D_ROTATION_Y: usize = 0x0C;
const D_POSITION: usize = 0x14;
const D_FLAGS: usize = 0xA0;
/// The motion controller (inline).
const D_MOTION: usize = 0xAC;
/// Flags karate clears on its fighters' Chao: collision (0x8), 0x2, AI (0x10), 0x20000.
const FIGHTER_FLAGS_OFF: u32 = 0x8 | 0x2 | 0x10 | 0x2_0000;
/// Motion controller: +0x2C current motion, +0x48 the motion table (0x1C-byte entries).
const MC_MOTION: usize = 0x2C;
const MC_TABLE: usize = 0x48;
const MOTION_ENTRY_SIZE: usize = 0x1C;
const MAX_MOTION_ID: u16 = 0x400;
/// `0x793D30`: blend into a motion (EAX controller, EDX id, push blend frames).
#[cfg(target_arch = "x86")]
const BLEND_MOTION: usize = 0x0079_3D30;
/// `0x437260`: play a sound (ESI id, push 0, 0, 0).
#[cfg(target_arch = "x86")]
const PLAY_SOUND: usize = 0x0043_7260;
/// Below this height a Chao is off the ring (the fighters' ring-out line is 7).
const FLOOR_MIN: f32 = 7.5;
/// The ring's floor height (the fighters' spawn height).
pub const FLOOR: f32 = 8.0;

/// The karate area's live objects: `g`, the main object and its Data1 (`MD`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Karate {
    pub g: usize,
    pub main: usize,
    pub md: usize,
}

fn read<T: Copy>(address: usize) -> T {
    // SAFETY: callers pass addresses of live game data (checked non-null bases).
    unsafe { (address as *const T).read_unaligned() }
}

fn write<T>(address: usize, value: T) {
    // SAFETY: as in `read`.
    unsafe { (address as *mut T).write_unaligned(value) }
}

/// The karate area's objects, while we're there.
pub fn karate() -> Option<Karate> {
    if !cfg!(target_arch = "x86")
        || game::current_level() != game::LEVEL_CHAO_WORLD
        || game::current_chao_area() != AREA_KARATE
    {
        return None;
    }
    let g: usize = read(KARATE_GLOBALS);
    let main: usize = read(KARATE_MAIN);
    if g == 0 || main == 0 {
        return None;
    }
    let md: usize = read(main + OBJ_DATA1);
    (md != 0).then_some(Karate { g, main, md })
}

pub fn paused() -> bool {
    cfg!(target_arch = "x86") && read::<u8>(PAUSED) != 0
}

/// Menu buttons pressed this frame (player 1).
pub fn buttons() -> u32 {
    if cfg!(target_arch = "x86") {
        read(MENU_BUTTONS)
    } else {
        0
    }
}

impl Karate {
    /// The main object's state and sub-state.
    pub fn state(self) -> (u8, u8) {
        (read(self.md), read(self.md + 1))
    }

    fn set_state(self, state: u8, sub: u8) {
        write(self.md, state);
        write(self.md + 1, sub);
    }

    /// At the mode select, reading buttons.
    pub fn mode_select_input(self) -> bool {
        self.state() == (STATE_MODE_SELECT, SUB_INPUT) && read::<u8>(self.g + G_BUSY) == 0
    }

    /// At the mode select, held by us.
    pub fn mode_select_held(self) -> bool {
        self.state() == (STATE_MODE_SELECT, SUB_HELD)
    }

    /// Holds the mode select (its window stays up, it reads no buttons) or lets it go.
    pub fn hold_mode_select(self, hold: bool) {
        let (state, sub) = self.state();
        if state != STATE_MODE_SELECT {
            return;
        }
        match (hold, sub) {
            (true, SUB_INPUT) => write(self.md + 1, SUB_HELD),
            (false, SUB_HELD) => write(self.md + 1, SUB_INPUT),
            _ => {}
        }
    }

    /// The mode select's cursor: 0 the fight, -1 leave.
    pub fn cursor(self) -> i8 {
        read(self.g + G_CURSOR)
    }

    /// Leaves the mode select the way a VS fight does: the menu closes and tears itself down
    /// (states 7 and 8) and arrives at state 9, which `stop_before_fight` turns into idling.
    pub fn leave_menu(self) {
        write(self.g + G_VS, 1u8);
        self.set_state(STATE_CONFIRM, 0);
    }

    /// Call right after the main object's own MainSub: where it was about to spawn karate's
    /// fighters (state 9), it idles instead. True when it did.
    pub fn stop_before_fight(self) -> bool {
        if self.state() == (STATE_SPAWN, 0) {
            self.set_state(STATE_IDLE, 0);
            return true;
        }
        false
    }

    /// Back to the karate menu: the menu view and state 2, which rebuilds the menu.
    pub fn back_to_menu(self) {
        self.set_camera(MENU_CAMERA.0, MENU_CAMERA.1);
        write(self.g + G_VS, 0u8);
        self.set_state(STATE_MENU, 0);
    }

    /// Leaves the karate area for Chao World area `area` with the steps of karate's own exit
    /// (state 0xF, 0x542E83..0x542EBA), which always goes to the Neutral garden: the byte it
    /// clears, `GoToChaoArea(area)`, then state 0x10 and the menu parts held. Called from the
    /// main object's MainSub, as the game's own exit runs.
    pub fn exit_to(self, area: i32) {
        write(EXIT_CLEARS, 0u8);
        #[cfg(target_arch = "x86")]
        // SAFETY: the game's cdecl GoToChaoArea, as karate's own exit calls it from here.
        unsafe {
            let go: extern "C" fn(i32) = std::mem::transmute(GO_TO_CHAO_AREA);
            go(area);
        }
        #[cfg(not(target_arch = "x86"))]
        let _ = area;
        self.set_state(STATE_GONE, 0);
        write(self.g + G_BUSY, 1u8);
    }

    /// Points the karate camera from `position` at `look`.
    pub fn set_camera(self, position: [f32; 3], look: [f32; 3]) {
        let camera: usize = read(self.g + G_CAMERA);
        if camera == 0 {
            return;
        }
        let work: usize = read(camera + OBJ_WORK);
        if work == 0 {
            return;
        }
        write(work + CAMERA_POSITION, position);
        write(work + CAMERA_LOOK, look);
    }

    pub fn main_sub(self) -> usize {
        read(self.main + OBJ_MAIN)
    }

    pub fn set_main_sub(self, sub: usize) {
        write(self.main + OBJ_MAIN, sub);
    }
}

/// Where one of our Chao stands and faces, and how high it's lifted off the floor (0 on it).
/// Its MainSub keeps it there.
#[derive(Clone, Copy)]
struct Spot {
    object: usize,
    at: [f32; 3],
    angle: i32,
    lift: f32,
}

static SPOTS: Mutex<Vec<Spot>> = Mutex::new(Vec::new());

fn spots() -> std::sync::MutexGuard<'static, Vec<Spot>> {
    SPOTS.lock().unwrap_or_else(|e| e.into_inner())
}

/// Moves one of our Chao to `at`, facing `angle`, standing on the floor.
pub fn place(object: usize, at: [f32; 3], angle: i32) {
    place_lifted(object, at, angle, 0.0);
}

/// Moves one of our Chao to `at`, facing `angle`, `lift` above `at`'s height (a hop or a
/// glide; 0 leaves the height to the game).
pub fn place_lifted(object: usize, at: [f32; 3], angle: i32, lift: f32) {
    let spot = Spot {
        object,
        at,
        angle,
        lift,
    };
    let mut s = spots();
    match s.iter_mut().find(|s| s.object == object) {
        Some(old) => *old = spot,
        None => s.push(spot),
    }
}

/// Forgets every spot (the area went away with its objects).
pub fn forget_all() {
    spots().clear();
}

/// The object is one of our Chao and still in the area.
pub fn alive(object: usize) -> bool {
    object != 0
        && crate::registry::entries(crate::registry::CHAO)
            .iter()
            .any(|&(_, o)| o == object)
        && read::<usize>(object + OBJ_MAIN) == chao_main as extern "C" fn(usize) as usize
}

type CreateChaoFn =
    unsafe extern "C" fn(*mut ChaoData, i32, *mut core::ffi::c_void, *mut Vector, i32) -> usize;

/// Makes a Chao from a copy of `chao` at `at` facing `angle`, still, with no AI and no
/// collision, kept in place by `chao_main`. The game makes its own copy of the record, so
/// nothing of `chao` is kept or changed.
pub fn spawn(chao: &ChaoData, at: [f32; 3], angle: i32) -> Option<usize> {
    if !cfg!(target_arch = "x86") || chao.chao_type() <= TYPE_EGG || chao.validate().is_err() {
        return None;
    }
    let mut record = Box::new(chao.clone());
    let mut position = Vector {
        x: at[0],
        y: at[1],
        z: at[2],
    };
    // SAFETY: the game's CreateChao, on the game thread in Chao World; copy mode 1 copies the
    // record (type 2 or more, checked) into a buffer of its own during the call.
    let object = unsafe {
        let create: CreateChaoFn = std::mem::transmute(CREATE_CHAO);
        create(&mut *record, 1, std::ptr::null_mut(), &mut position, angle)
    };
    if object == 0 {
        return None;
    }
    let d: usize = read(object + OBJ_DATA1);
    if d == 0 {
        return None;
    }
    write(d + D_FLAGS, read::<u32>(d + D_FLAGS) & !FIGHTER_FLAGS_OFF);
    place(object, at, angle);
    write(
        object + OBJ_MAIN,
        chao_main as extern "C" fn(usize) as usize,
    );
    Some(object)
}

/// Removes one of our Chao (if it's still there).
pub fn remove(object: usize) {
    if alive(object) {
        write(object + OBJ_MAIN, DELETE_OBJECT);
    }
    spots().retain(|s| s.object != object);
}

/// `id` is a real entry of the motion table at `controller` (0x793D30 doesn't check, and a bad
/// entry crashes the Chao's drawing).
fn motion_ok(controller: usize, id: u16) -> bool {
    if id >= MAX_MOTION_ID {
        return false;
    }
    let table: usize = read(controller + MC_TABLE);
    let current: usize = read(controller + MC_MOTION);
    if table == 0 || current == 0 {
        return false;
    }
    let entry = table + usize::from(id) * MOTION_ENTRY_SIZE;
    let motion: usize = read(entry);
    let mode: u16 = read(entry + 4);
    // Every Chao motion lives in the same module as the one it plays now.
    motion != 0 && mode <= 7 && motion.abs_diff(current) < 0x0100_0000
}

/// Plays Chao motion `id` on one of our Chao, blending over `blend` frames.
pub fn motion(object: usize, id: u16, blend: u32) {
    if !alive(object) {
        return;
    }
    let d: usize = read(object + OBJ_DATA1);
    if d == 0 || !motion_ok(d + D_MOTION, id) {
        return;
    }
    // SAFETY: a live Chao's motion controller and a checked id, on the game thread.
    unsafe { blend_motion(d + D_MOTION, id, blend) };
}

#[cfg(target_arch = "x86")]
unsafe fn blend_motion(controller: usize, id: u16, blend: u32) {
    // SAFETY: usercall EAX = controller, EDX = id, one cdecl argument (re/chao-battle-arena.md 4).
    unsafe {
        std::arch::asm!(
            "push {blend}",
            "call {f}",
            "add esp, 4",
            blend = in(reg) blend,
            f = in(reg) BLEND_MOTION,
            inout("eax") controller => _,
            inout("edx") u32::from(id) => _,
            clobber_abi("C"),
        );
    }
}

#[cfg(not(target_arch = "x86"))]
unsafe fn blend_motion(_controller: usize, _id: u16, _blend: u32) {}

/// Plays sound `id`.
pub fn sound(id: u32) {
    #[cfg(target_arch = "x86")]
    // SAFETY: the game's sound call (ESI = id, three zero cdecl arguments), on the game thread.
    unsafe {
        std::arch::asm!(
            "push esi",
            "mov esi, ecx",
            "push 0",
            "push 0",
            "push 0",
            "call eax",
            "add esp, 12",
            "pop esi",
            inout("ecx") id => _,
            inout("eax") PLAY_SOUND => _,
            clobber_abi("C"),
        );
    }
    #[cfg(not(target_arch = "x86"))]
    let _ = id;
}

/// MainSub of our Chao: the game's own update between two writes of its spot, so nothing moves
/// it but us (the height stays the game's, so it stands on the floor).
extern "C" fn chao_main(object: usize) {
    let spot = std::panic::catch_unwind(|| spots().iter().find(|s| s.object == object).copied())
        .ok()
        .flatten();
    let d: usize = read(object + OBJ_DATA1);
    if let Some(spot) = spot
        && d != 0
    {
        put(d, spot);
    }
    // SAFETY: the Chao's own MainSub, which this object had.
    unsafe { std::mem::transmute::<usize, extern "C" fn(usize)>(CHAO_MAIN)(object) };
    if let Some(spot) = spot
        && d != 0
        && read::<usize>(object + OBJ_MAIN) == chao_main as extern "C" fn(usize) as usize
        && read::<usize>(object + OBJ_DATA1) == d
    {
        put(d, spot);
        let d2: usize = read(object + OBJ_DATA2);
        if d2 != 0 {
            write(d2, [0.0f32; 3]);
        }
    }
}

/// Writes x, z and facing; the height is the game's unless it's lifted or fell off the ring.
fn put(d: usize, spot: Spot) {
    let mut p: Vector = read(d + D_POSITION);
    p.x = spot.at[0];
    p.z = spot.at[2];
    if spot.lift > 0.0 {
        p.y = spot.at[1] + spot.lift;
    } else if !p.y.is_finite() || p.y < FLOOR_MIN {
        p.y = spot.at[1];
    }
    write(d + D_POSITION, p);
    write(d + D_ROTATION_Y, spot.angle);
}
