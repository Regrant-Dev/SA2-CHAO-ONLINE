//! An owned creature battle venue inside an already loaded garden. The scene has its own
//! camera-event lease, display object and private Chao copies. It never enters, hooks or
//! changes vanilla Karate. Garden objects, save ownership and networking continue normally.
#![cfg_attr(not(target_arch = "x86"), allow(dead_code))]

use crate::chao::{ChaoData, TYPE_EGG};
use crate::game::{self, Vector};
use std::sync::Mutex;

const MENU_BUTTONS: usize = 0x01DE_FAB0;
const OBJ_MAIN: usize = 0x10;
const OBJ_DATA1: usize = 0x34;
const OBJ_DATA2: usize = 0x38;
const D_ROTATION_Y: usize = 0x0C;
const D_POSITION: usize = 0x14;
const D_FLAGS: usize = 0xA0;
const D_MOTION: usize = 0xAC;
const MC_MOTION: usize = 0x2C;
const MC_TABLE: usize = 0x48;
const MOTION_ENTRY_SIZE: usize = 0x1C;
const MAX_MOTION_ID: u16 = 0x400;
const CREATE_CHAO: usize = 0x0055_01D0;
const CHAO_MAIN: usize = 0x0054_FE20;
const DELETE_OBJECT: usize = 0x0046_F720;
const FIGHTER_FLAGS_OFF: u32 = 0x8 | 0x2 | 0x10 | 0x2_0000;
#[cfg(target_arch = "x86")]
const BLEND_MOTION: usize = 0x0079_3D30;
#[cfg(target_arch = "x86")]
const PLAY_SOUND: usize = 0x0043_7260;

/// Battle coordinates are isolated above the ordinary garden. Copies are pinned to their
/// own floor, so native garden land and its Chao never become battle collision objects.
pub const ORIGIN: [f32; 3] = [0.0, 1000.0, 0.0];
pub const FLOOR: f32 = 8.0;
pub fn world(at: [f32; 3]) -> [f32; 3] {
    std::array::from_fn(|i| ORIGIN[i] + at[i])
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Context {
    level: i16,
    area: i32,
    run: u32,
    player: usize,
}
impl Context {
    fn current() -> Option<Self> {
        if !cfg!(target_arch = "x86")
            || game::two_player()
            || !game::in_garden()
            || !matches!(game::game_state(), 16 | 17)
        {
            return None;
        }
        let player = game::local_entity() as usize;
        (player != 0).then(|| Self {
            level: game::current_level(),
            area: game::current_chao_area(),
            run: game::stage_run(),
            player,
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Scene {
    epoch: u64,
}

fn read<T: Copy>(address: usize) -> T {
    // SAFETY: internal callers use checked live game data on the game thread.
    unsafe { (address as *const T).read_unaligned() }
}
fn write<T>(address: usize, value: T) {
    // SAFETY: as in read.
    unsafe { (address as *mut T).write_unaligned(value) }
}
pub fn paused() -> bool {
    // TimeStopped (0x174AFF7) also changes during ordinary garden interactions.
    // Only the native pause-menu state should suspend the owned battle UI.
    cfg!(target_arch = "x86") && game::game_state() == game::GAME_STATE_PAUSED
}
pub fn buttons() -> u32 {
    if cfg!(target_arch = "x86") {
        read(MENU_BUTTONS)
    } else {
        0
    }
}

pub fn available() -> bool {
    Context::current().is_some() && !paused()
}

#[cfg(target_arch = "x86")]
pub use native::{begin, end, on_frame, scene};
#[cfg(not(target_arch = "x86"))]
pub fn begin() -> Result<Scene, String> {
    Err("Enter a loaded Chao garden to open Creature battles".into())
}
#[cfg(not(target_arch = "x86"))]
pub fn end() {}
#[cfg(not(target_arch = "x86"))]
pub fn on_frame() {}
#[cfg(not(target_arch = "x86"))]
pub fn scene() -> Option<Scene> {
    None
}
pub fn active() -> bool {
    scene().is_some()
}

impl Scene {
    pub fn set_camera(self, position: [f32; 3], look: [f32; 3]) {
        #[cfg(target_arch = "x86")]
        native::set_camera(self, position, look);
        #[cfg(not(target_arch = "x86"))]
        let _ = (position, look);
    }
}

#[cfg(target_arch = "x86")]
mod native {
    use super::*;
    use crate::creature_link::billboard::{self, Textures};
    use crate::puppet::{self, Tilt};
    use std::sync::atomic::{AtomicU64, Ordering};

    const CAMERA_SLOT: usize = 0x01DD_0194;
    const REGISTER_CAMERA: usize = 0x004E_BC50;
    const REMOVE_CAMERA: usize = 0x004E_BD30;
    // This native callback only copies the position/look pointers at event +0x14/+0x18
    // to the camera output. It reads no Karate state (verified 0x5743A0..0x574415).
    const CAMERA_CALLBACK: usize = 0x0057_43A0;
    const NAME: &[u8] = b"SA2ChaoOnlineCreatureVenue\0";
    const GBIX: u32 = 0x7E5E_0000;
    static NEXT_EPOCH: AtomicU64 = AtomicU64::new(1);
    static SCENE: Mutex<Option<Owned>> = Mutex::new(None);

    struct CameraLease {
        slot: i32,
        entry: usize,
        pose: Box<[[f32; 3]; 2]>,
    }
    impl CameraLease {
        fn new() -> Option<Self> {
            let previous: i32 = read(CAMERA_SLOT);
            if !(0..15).contains(&previous) {
                return None;
            }
            let slot = previous + 1;
            let pose = Box::new([world([0.0, 15.0, 43.0]), world([0.0, 10.5, 19.0])]);
            let entry: usize;
            // SAFETY: generic native camera registration: context EAX=0, priority ESI,
            // callback cdecl argument. This adds a new event without clearing the old one.
            unsafe {
                std::arch::asm!(
                    "push esi", "mov esi, ecx", "push {callback}", "mov edx, {register}", "call edx",
                    "add esp, 4", "pop esi",
                    callback = const CAMERA_CALLBACK,
                    register = const REGISTER_CAMERA,
                    inout("eax") 0usize => entry,
                    inout("ecx") slot => _,
                    clobber_abi("C"),
                );
            }
            if entry == 0 {
                return None;
            }
            write(entry + 0x14, pose[0].as_ptr() as usize);
            write(entry + 0x18, pose[1].as_ptr() as usize);
            Some(Self { slot, entry, pose })
        }
        fn ours(&self) -> bool {
            read::<usize>(self.entry + 0x0C) == CAMERA_CALLBACK
                && read::<usize>(self.entry + 0x14) == self.pose[0].as_ptr() as usize
                && read::<usize>(self.entry + 0x18) == self.pose[1].as_ptr() as usize
        }
        fn release(&self) {
            if !self.ours() {
                return;
            }
            // SAFETY: only our still-owned event is removed, restoring the previous native
            // camera if ours was current. Never remove a reused event after an area change.
            unsafe {
                std::arch::asm!("call {remove}", remove = in(reg) REMOVE_CAMERA,
                    inout("eax") self.slot => _, inout("ecx") 0usize => _, clobber_abi("C"));
            }
            write(self.entry + 0x14, 0usize);
            write(self.entry + 0x18, 0usize);
        }
    }
    // SAFETY: all game addresses are only read/written on the game thread.
    unsafe impl Send for CameraLease {}

    struct Mesh {
        object: Box<[u32; 13]>,
        _model: Box<[u32; 6]>,
        _vertices: Box<[u32]>,
        _polygons: Box<[u32]>,
    }
    impl Mesh {
        fn quad(size: f32, texture: u16) -> Self {
            let (vertices, mut polygons) = billboard::quad(size);
            // The generated quad's texture command names one of our procedural swatches.
            polygons[2] = (polygons[2] & 0xFFFF) | (u32::from(texture) << 16);
            let vertices = vertices.into_boxed_slice();
            let polygons = polygons.into_boxed_slice();
            let model = Box::new([
                vertices.as_ptr() as u32,
                polygons.as_ptr() as u32,
                0,
                0,
                0,
                0,
            ]);
            let mut object = Box::new([0u32; 13]);
            object[0] = 0x17;
            object[1] = model.as_ptr() as u32;
            for field in [8, 9, 10] {
                object[field] = 1.0f32.to_bits();
            }
            Self {
                object,
                _model: model,
                _vertices: vertices,
                _polygons: polygons,
            }
        }
    }
    struct Owned {
        epoch: u64,
        context: Context,
        object: usize,
        controls: u8,
        player_position: [f32; 3],
        camera: CameraLease,
        textures: Textures,
        floor: Mesh,
        backdrop: Mesh,
    }
    // SAFETY: native pointers are owned by this scene and used only on the game thread.
    unsafe impl Send for Owned {}
    impl Owned {
        fn release(self, restore: bool, delete_object: bool) {
            self.camera.release();
            self.textures.release();
            if delete_object {
                write(self.object + OBJ_MAIN, DELETE_OBJECT);
            }
            if restore && Context::current() == Some(self.context) {
                let [x, y, z] = self.player_position;
                game::set_player_position(Vector { x, y, z });
                // A lost-focus handler owns input until focus returns; don't override it.
                if crate::input::game_focused() {
                    game::set_controls(self.controls != 0);
                }
            }
        }
    }
    fn lock() -> std::sync::MutexGuard<'static, Option<Owned>> {
        SCENE.lock().unwrap_or_else(|e| e.into_inner())
    }
    fn swatches() -> [Vec<u8>; 2] {
        std::array::from_fn(|which| {
            let mut pixels = Vec::with_capacity(16 * 16 * 4);
            for y in 0..16 {
                for x in 0..16 {
                    let color = if which == 0 {
                        if x == 7 || x == 8 {
                            [215, 225, 232, 255]
                        } else if x < 8 {
                            [45, 90, 120, 255]
                        } else {
                            [108, 65, 90, 255]
                        }
                    } else {
                        [27 + y * 2, 38 + y * 2, 57 + y * 2, 255]
                    };
                    pixels.extend(color);
                }
            }
            pixels
        })
    }
    pub fn begin() -> Result<Scene, String> {
        let context = Context::current().filter(|_| !paused()).ok_or_else(|| {
            "Open Creature battles in a loaded Chao garden, with the game unpaused".to_string()
        })?;
        if let Some(existing) = scene() {
            return Ok(existing);
        }
        if crate::garden::player_held() != 0 {
            return Err("Put down what you're holding before opening Creature battles".into());
        }
        let count = crate::registry::entries(crate::registry::CHAO).len();
        if count > 28 {
            return Err("This garden needs four free Chao display slots for a battle".into());
        }
        let textures = Textures::make(&swatches(), 16, 16, GBIX, false)
            .ok_or_else(|| "Couldn't prepare the creature venue textures".to_string())?;
        let Some(camera) = CameraLease::new() else {
            textures.release();
            return Err(
                "The current camera is busy; try Creature battles again after it settles".into(),
            );
        };
        let object = puppet::create_display_object(NAME, display, deleted);
        if object == 0 {
            camera.release();
            textures.release();
            return Err("Couldn't create the creature venue".into());
        }
        let epoch = NEXT_EPOCH.fetch_add(1, Ordering::Relaxed);
        let player_position: [f32; 3] = read(context.player + D_POSITION);
        let controls: u8 = read(game::CONTROLLER_ENABLED);
        *lock() = Some(Owned {
            epoch,
            context,
            object,
            controls,
            player_position,
            camera,
            textures,
            floor: Mesh::quad(120.0, 0),
            backdrop: Mesh::quad(300.0, 1),
        });
        game::set_controls(false);
        crate::log!(
            "battle: opened independent creature scene in garden {} (owned safety floor)",
            context.area
        );
        Ok(Scene { epoch })
    }
    pub fn scene() -> Option<Scene> {
        let context = Context::current()?;
        lock()
            .as_ref()
            .filter(|s| s.context == context)
            .map(|s| Scene { epoch: s.epoch })
    }
    pub fn end() {
        let old = lock().take();
        if let Some(old) = old {
            old.release(true, true);
        }
    }
    pub fn on_frame() {
        let current = Context::current();
        let mut state = lock();
        if state.as_ref().is_some_and(|s| Some(s.context) != current) {
            let old = state.take();
            drop(state);
            if let Some(old) = old {
                old.release(false, false);
            }
            return;
        }
        if let Some(scene) = state.as_ref() {
            game::set_controls(false);
            let [x, y, z] = scene.player_position;
            game::set_player_position(Vector { x, y, z });
        }
    }
    pub fn set_camera(scene: Scene, position: [f32; 3], look: [f32; 3]) {
        if !position.into_iter().chain(look).all(f32::is_finite) {
            return;
        }
        if let Some(owned) = lock().as_mut().filter(|s| s.epoch == scene.epoch) {
            owned.camera.pose[0] = world(position);
            owned.camera.pose[1] = world(look);
        }
    }
    extern "C" fn deleted(object: usize) {
        let _ = std::panic::catch_unwind(|| {
            let mut state = lock();
            if state.as_ref().is_some_and(|s| s.object == object) {
                let old = state.take();
                drop(state);
                if let Some(old) = old {
                    old.release(false, false);
                }
            }
        });
    }
    extern "C" fn display(object: usize) {
        let _ = std::panic::catch_unwind(|| {
            let state = lock();
            let Some(s) = state.as_ref().filter(|s| {
                s.object == object && s.context == Context::current().unwrap_or(s.context)
            }) else {
                return;
            };
            if !s.textures.ours() {
                return;
            }
            // The imported static field supplies the actual venue. During preparation only,
            // show our own neutral backdrop; game.rs never advances a battle on that fallback.
            let imported = crate::creature::assets::draw_venue(world([0.0, FLOOR, 19.0])).is_ok();
            // SAFETY: owned, generated chunk models and current-scene textures, display pass.
            unsafe {
                if !imported {
                    puppet::draw_solid(
                        s.backdrop.object.as_ptr() as usize,
                        s.textures.list(),
                        world([0.0, -40.0, -45.0]),
                        [0, 0x8000, 0],
                        1.0,
                        Tilt::default(),
                        None,
                    );
                }
                puppet::draw_solid(
                    s.floor.object.as_ptr() as usize,
                    s.textures.list(),
                    world([0.0, FLOOR - if imported { 0.5 } else { 0.0 }, -60.0]),
                    [0x4000, 0x8000, 0],
                    1.0,
                    Tilt::default(),
                    None,
                );
            }
        });
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
        write(object + OBJ_MAIN, DELETE_OBJECT);
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
    // Battle copies have a scene-owned floor; native garden gravity/land must never pull
    // them back into the live garden or mutate a saved Chao record.
    let p = Vector {
        x: spot.at[0],
        y: spot.at[1] + spot.lift,
        z: spot.at[2],
    };
    write(d + D_POSITION, p);
    write(d + D_ROTATION_Y, spot.angle);
}
