//! Partners in the garden (re/chao-animals.md): each waiting partner is a garden animal object
//! (`0x548D30`, id 3, no save record) with our display, which draws its picture as a
//! camera-facing quad, and our main around the animal's, which lets Chao notice it while it
//! roams. A Chao that takes one absorbs it as usual (motions, sparkles, the status balloon),
//! but at `0x56708E` a partner skips the animal effects for ours (`super::absorbed`); so does a
//! wild visitor (`super::wild`), which makes its objects with the same helpers.

#[cfg(target_arch = "x86")]
pub(super) use game::{
    D_MODE, D_POSITION, DELETE_OBJECT, MODE_PLAYER, MODE_ROAM, OBJ_MAIN, create, data1,
    enable_touch, hooked, room, to_come,
};
#[cfg(target_arch = "x86")]
pub use game::{install, is_partner_object, on_frame};

/// Where partners come in from, by key: set by `arrive_at`, taken when the partner appears.
static ARRIVALS: std::sync::Mutex<Vec<(u64, [f32; 3], i32)>> = std::sync::Mutex::new(Vec::new());

/// The partner `key` (just brought in) comes out at `at`, hopping forward along `angle`
/// (BAMS) instead of walking in ahead of the player.
pub fn arrive_at(key: u64, at: [f32; 3], angle: i32) {
    let mut a = ARRIVALS.lock().unwrap_or_else(|e| e.into_inner());
    a.retain(|(k, _, _)| *k != key);
    a.push((key, at, angle));
}

/// Takes where the partner `key` comes out, if it was set.
#[cfg_attr(not(target_arch = "x86"), allow(dead_code))]
fn take_arrival(key: u64) -> Option<([f32; 3], i32)> {
    let mut a = ARRIVALS.lock().unwrap_or_else(|e| e.into_inner());
    let i = a.iter().position(|(k, _, _)| *k == key)?;
    let (_, at, angle) = a.remove(i);
    Some((at, angle))
}

/// The hop of a partner that comes out at a point: forward along `angle` and up.
#[cfg_attr(not(target_arch = "x86"), allow(dead_code))]
fn hop(angle: i32) -> [f32; 3] {
    let a = angle as f32 * std::f32::consts::TAU / 65536.0;
    let (s, c) = a.sin_cos();
    [s * HOP_FORWARD, HOP_UP, c * HOP_FORWARD]
}

#[cfg_attr(not(target_arch = "x86"), allow(dead_code))]
const HOP_FORWARD: f32 = 0.3;
#[cfg_attr(not(target_arch = "x86"), allow(dead_code))]
const HOP_UP: f32 = 0.8;

#[cfg(not(target_arch = "x86"))]
pub fn is_partner_object(_object: usize) -> bool {
    false
}

#[cfg(not(target_arch = "x86"))]
pub fn on_frame() {}

#[cfg(not(target_arch = "x86"))]
pub fn install() {}

/// Partners walk about this far apart where they come in.
#[cfg_attr(not(target_arch = "x86"), allow(dead_code))]
const SPREAD: f32 = 4.0;

/// Where the `i`th partner comes in: in front of the player (at `x, z`, facing `angle` in
/// BAMS, the game's sin/cos convention), side by side.
#[cfg_attr(not(target_arch = "x86"), allow(dead_code))]
fn arrival(x: f32, z: f32, angle: i32, i: usize) -> (f32, f32) {
    let a = angle as f32 * std::f32::consts::TAU / 65536.0;
    let (s, c) = a.sin_cos();
    let ahead = 6.0;
    let side = (i as f32 - 1.5) * SPREAD;
    (x + s * ahead + c * side, z + c * ahead - s * side)
}

#[cfg(target_arch = "x86")]
mod game {
    use std::sync::Mutex;

    use super::super::store::Partner;
    use super::super::{absorbed, learned_wild, rom, wild, with_store};
    use super::{arrival, hop, take_arrival};
    use crate::game::{self as sa2, Vector};
    use crate::log;

    /// Animal create (cdecl `(i8 id, pos*, angle, vel*, save*)` -> object).
    const CREATE_ANIMAL: usize = 0x0054_8D30;
    /// The animal id used: one with no cry (ids 3, 12, 14, 16, 19 are silent).
    const ANIMAL_ID: i32 = 3;
    pub const DELETE_OBJECT: usize = 0x0046_F720;
    /// Category-2 registry count and the game's limit for it (`0x8AB838[2]`), shared by
    /// animals and Chaos Drives.
    const ANIMAL_COUNT: usize = 0x01DC_0F88;
    const ANIMAL_LIMIT: usize = 0x008A_B840;
    /// Enable collision entry ECX of object EAX.
    const ENABLE_COLLISION: usize = 0x0048_6CD0;
    /// The Chao-touch collision entry (id 0x87) Chao notice animals by.
    const TOUCH_ENTRY: u32 = 2;
    /// AddHappiness (ESI = Chao, EDI = delta, push f32 alignment; caller pops 4).
    const ADD_HAPPINESS: usize = 0x0053_6320;

    pub const OBJ_MAIN: usize = 0x10;
    const OBJ_DISPLAY: usize = 0x14;
    const OBJ_DATA1: usize = 0x34;
    const OBJ_ENTRY: usize = 0x3C;
    /// Animal Data1: +0 mode (0 roam, 1 held by the player, 2 held by a Chao, 3 deleted),
    /// +0x14 position, +0xF4 set once a Chao let go of it.
    pub const D_MODE: usize = 0x00;
    pub const D_POSITION: usize = 0x14;
    pub const MODE_ROAM: u8 = 0;
    pub const MODE_PLAYER: u8 = 1;
    const MODE_DELETED: u8 = 3;
    /// The Chao record: Chao Data1 + 0x5C.
    const CHAO_RECORD: usize = 0x5C;

    /// The absorb's animal branch (`jge 0x567241` after `cmp ebx, 0x15`): EBP = Chao, EDI =
    /// the animal, EBX = its id.
    const ABSORB_SITE: usize = 0x0056_708E;
    const ABSORB_BYTES: [u8; 6] = [0x0F, 0x8D, 0xAD, 0x01, 0x00, 0x00];
    /// Where the animal effects go on (animals; drives at `ABSORB_DRIVE`) and where they end
    /// (`xor ebx, ebx`, then the balloon and unlinking).
    const ABSORB_ANIMAL: usize = 0x0056_7094;
    const ABSORB_DRIVE: usize = 0x0056_7241;
    const ABSORB_DONE: usize = 0x0056_74CC;

    struct Spawned {
        object: usize,
        garden: u8,
        key: u64,
        species: u16,
        /// The animal's own MainSub (ours wraps it).
        main: usize,
    }

    struct Scene {
        spawned: Vec<Spawned>,
        hooked: bool,
        /// In a garden at the last frame (pictures go once on leaving it).
        in_garden: bool,
        /// The ROM couldn't be read: not tried again before this.
        rom_retry: Option<std::time::Instant>,
    }

    /// How long a failed ROM read waits before it's tried again.
    const ROM_RETRY: std::time::Duration = std::time::Duration::from_secs(30);

    static SCENE: Mutex<Scene> = Mutex::new(Scene {
        spawned: Vec::new(),
        hooked: false,
        in_garden: false,
        rom_retry: None,
    });

    fn lock() -> std::sync::MutexGuard<'static, Scene> {
        SCENE.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub fn install() {
        let target = absorb_hook as unsafe extern "C" fn() as usize;
        let mut jump = [0xE9, 0, 0, 0, 0, 0x90];
        jump[1..5].copy_from_slice(&(target.wrapping_sub(ABSORB_SITE + 5) as u32).to_le_bytes());
        let ok = crate::patch::replace_code(ABSORB_SITE, &ABSORB_BYTES, &jump);
        lock().hooked = ok;
        if ok {
            log!("gba link: absorb hook installed");
        } else {
            log!("gba link: the absorb code isn't as expected; partners stay away");
        }
    }

    /// The absorb hook is in: Chao absorbing our animals reaches us, not the Chao save.
    pub fn hooked() -> bool {
        lock().hooked
    }

    /// Partners waiting in garden `garden` that have no object yet.
    pub fn to_come(garden: u8) -> usize {
        let scene = lock();
        with_store(|st| {
            st.waiting_in(garden)
                .filter(|w| {
                    !scene
                        .spawned
                        .iter()
                        .any(|s| s.garden == garden && s.key == w.key)
                })
                .count()
        })
    }

    /// # Safety
    /// `object` is live.
    pub unsafe fn data1(object: usize) -> usize {
        unsafe { ((object + OBJ_DATA1) as *const usize).read() }
    }

    /// Room for one more animal in category 2, keeping `reserve` entries free.
    pub fn room(reserve: u32) -> bool {
        // SAFETY: the game's registry count and limit (statics).
        unsafe {
            (ANIMAL_COUNT as *const u32).read().saturating_add(reserve)
                < (ANIMAL_LIMIT as *const u32).read()
        }
    }

    /// Lets Chao notice `object` (its collision entry 2) this frame.
    ///
    /// # Safety
    /// Game thread; `object` is a live animal.
    pub unsafe fn enable_touch(object: usize) {
        unsafe {
            std::arch::asm!(
                "call {f}",
                f = in(reg) ENABLE_COLLISION,
                inout("eax") object => _,
                inout("ecx") TOUCH_ENTRY => _,
                clobber_abi("C"),
            );
        }
    }

    /// Makes a silent animal with no save record at `at` (moving at `velocity`) whose main is
    /// `main` and display `display`. Returns it and its own main (which `main` should call).
    ///
    /// # Safety
    /// Game thread, in a running garden with `room` in category 2.
    pub unsafe fn create(
        at: Vector,
        angle: i32,
        velocity: [f32; 3],
        main: extern "C" fn(usize),
        display: extern "C" fn(usize),
    ) -> Option<(usize, usize)> {
        let velocity = Vector {
            x: velocity[0],
            y: velocity[1],
            z: velocity[2],
        };
        type Create = extern "C" fn(i32, *const Vector, i32, *const Vector, usize) -> usize;
        unsafe {
            let object = std::mem::transmute::<usize, Create>(CREATE_ANIMAL)(
                ANIMAL_ID, &at, angle, &velocity, 0,
            );
            if object == 0 {
                return None;
            }
            if ((object + OBJ_ENTRY) as *const usize).read() == 0 {
                // Not registered: the game's own display and Chao code expect an entry.
                ((object + OBJ_MAIN) as *mut usize).write(DELETE_OBJECT);
                return None;
            }
            let own = ((object + OBJ_MAIN) as *const usize).read();
            ((object + OBJ_MAIN) as *mut usize).write(main as usize);
            ((object + OBJ_DISPLAY) as *mut usize).write(display as usize);
            Some((object, own))
        }
    }

    pub fn on_frame() {
        let mut scene = lock();
        let in_garden = sa2::in_garden();
        if std::mem::replace(&mut scene.in_garden, in_garden) && !in_garden {
            // Once on leaving (other areas, as the karate area's battles, draw their own).
            super::super::billboard::forget();
        }
        if !scene.hooked {
            return;
        }
        if !in_garden {
            // Gone with the garden: the game keeps list-2 objects across areas, so ours go
            // (those still registered; the others went with the garden).
            let live: Vec<usize> = crate::registry::entries(2)
                .into_iter()
                .map(|(_, o)| o)
                .collect();
            for s in scene.spawned.drain(..) {
                if live.contains(&s.object) {
                    // SAFETY: our live object; the game deletes it on its next pass.
                    unsafe { ((s.object + OBJ_MAIN) as *mut usize).write(DELETE_OBJECT) };
                }
            }
            return;
        }
        if sa2::between_areas() || crate::garden::server_runs() {
            return;
        }
        let garden = sa2::current_chao_area() as u8;
        let live: Vec<usize> = crate::registry::entries(2)
            .into_iter()
            .map(|(_, o)| o)
            .collect();
        scene
            .spawned
            .retain(|s| live.contains(&s.object) && s.garden == garden);
        let waiting: Vec<(u64, u16)> = with_store(|st| {
            st.waiting_in(garden)
                .map(|w| (w.key, w.partner.species))
                .collect()
        });
        // Absorbed or no longer waiting: gone.
        for s in &scene.spawned {
            if !waiting.iter().any(|(k, _)| *k == s.key) {
                // SAFETY: our live object.
                unsafe { ((s.object + OBJ_MAIN) as *mut usize).write(DELETE_OBJECT) };
            }
        }
        scene
            .spawned
            .retain(|s| waiting.iter().any(|(k, _)| *k == s.key));
        let missing: Vec<(u64, u16)> = waiting
            .into_iter()
            .filter(|(k, _)| !scene.spawned.iter().any(|s| s.key == *k))
            .collect();
        if missing.is_empty() {
            return;
        }
        if scene
            .rom_retry
            .is_some_and(|at| std::time::Instant::now() < at)
        {
            return;
        }
        if let Err(e) = rom() {
            log!("gba link: partners wait for the ROM: {e}");
            scene.rom_retry = Some(std::time::Instant::now() + ROM_RETRY);
            return;
        }
        let Some((player, angle)) = sa2::player_position() else {
            return;
        };
        for (key, species) in missing {
            if !room(0) {
                break;
            }
            let (at, angle, velocity) = match take_arrival(key) {
                Some((from, angle)) => (
                    Vector {
                        x: from[0],
                        y: from[1],
                        z: from[2],
                    },
                    angle,
                    hop(angle),
                ),
                None => {
                    let (x, z) = arrival(player.x, player.z, angle, scene.spawned.len());
                    let at = Vector {
                        x,
                        y: player.y + 2.0,
                        z,
                    };
                    // Facing the player.
                    (at, angle.wrapping_add(0x8000), [0.0; 3])
                }
            };
            // SAFETY: game thread, in a running garden with room in category 2.
            if let Some(s) = unsafe { spawn(at, angle, velocity, garden, key, species) } {
                log!("gba link: partner (species {species}) is in garden {garden}");
                scene.spawned.push(s);
            }
        }
    }

    /// # Safety
    /// Game thread, in a running garden.
    unsafe fn spawn(
        at: Vector,
        angle: i32,
        velocity: [f32; 3],
        garden: u8,
        key: u64,
        species: u16,
    ) -> Option<Spawned> {
        // SAFETY: as the caller's.
        let (object, main) = unsafe { create(at, angle, velocity, partner_main, partner_display)? };
        Some(Spawned {
            object,
            garden,
            key,
            species,
            main,
        })
    }

    /// The animal's own main, then: while it roams and the player holds nothing, Chao can
    /// notice it (the game only lets them while the player carries an animal).
    extern "C" fn partner_main(object: usize) {
        let main = lock()
            .spawned
            .iter()
            .find(|s| s.object == object)
            .map(|s| s.main);
        let Some(main) = main else {
            // Not ours any more: let the game delete it.
            // SAFETY: the object the game runs.
            unsafe { ((object + OBJ_MAIN) as *mut usize).write(DELETE_OBJECT) };
            return;
        };
        // SAFETY: the animal's own main with its object, on the game thread.
        unsafe { std::mem::transmute::<usize, extern "C" fn(usize)>(main)(object) };
        // SAFETY: as above; the main may have deleted it (MainSub changed, mode 3).
        unsafe {
            if ((object + OBJ_MAIN) as *const usize).read()
                != partner_main as extern "C" fn(usize) as usize
            {
                return;
            }
            let d = data1(object);
            if d != 0 && (d as *const u8).read() == MODE_ROAM && crate::garden::player_held() == 0 {
                enable_touch(object);
            }
        }
    }

    /// Draws the partner's picture where the animal is.
    extern "C" fn partner_display(object: usize) {
        let species = lock()
            .spawned
            .iter()
            .find(|s| s.object == object)
            .map(|s| s.species);
        let Some(species) = species else {
            return;
        };
        // SAFETY: the object the game draws, on the game thread.
        let (at, held) = unsafe {
            let d = data1(object);
            if d == 0 {
                return;
            }
            (
                ((d + D_POSITION) as *const [f32; 3]).read_unaligned(),
                (d as *const u8).read() == MODE_PLAYER,
            )
        };
        super::super::billboard::draw(species, at, held);
    }

    /// One of our partners' objects.
    pub fn is_partner_object(object: usize) -> bool {
        lock().spawned.iter().any(|s| s.object == object)
    }

    /// Hook: whether the animal a Chao absorbs is a partner or a wild visitor (cdecl).
    extern "C" fn is_partner(_chao: usize, animal: usize) -> u32 {
        let partner = lock().spawned.iter().any(|s| s.object == animal);
        u32::from(partner || wild::is_visitor(animal))
    }

    /// What a Chao absorbed.
    enum Took {
        Partner { garden: u8, key: u64 },
        Wild(Partner),
    }

    /// Hook: a Chao absorbs partner or visitor `animal`: our effect, then the animal goes.
    extern "C" fn partner_absorbed(chao: usize, animal: usize) {
        let _ = std::panic::catch_unwind(|| {
            let spawned = {
                let mut scene = lock();
                let at = scene.spawned.iter().position(|s| s.object == animal);
                at.map(|i| scene.spawned.remove(i))
            };
            let (main, took) = match spawned {
                Some(s) => (
                    s.main,
                    Took::Partner {
                        garden: s.garden,
                        key: s.key,
                    },
                ),
                None => match wild::take(animal) {
                    Some((main, partner)) => (main, Took::Wild(partner)),
                    None => return,
                },
            };
            // SAFETY: the Chao and animal the absorb behaviour has, on the game thread. The
            // animal isn't freed here (the behaviour still writes its registry entry): mode 3
            // has its own main delete it next frame.
            unsafe {
                let d = data1(animal);
                if d != 0 {
                    ((d + D_MODE) as *mut u8).write(MODE_DELETED);
                }
                ((animal + OBJ_MAIN) as *mut usize).write(main);
                let cd = data1(chao);
                if cd == 0 {
                    return;
                }
                let record = ((cd + CHAO_RECORD) as *const usize).read();
                if record == 0 {
                    return;
                }
                let rec = std::slice::from_raw_parts(record as *const u8, crate::chao::CHAO_SIZE);
                let Some(identity) = crate::chao::ChaoData::from_bytes(rec).map(|c| c.identity())
                else {
                    return;
                };
                match took {
                    Took::Partner { garden, key } => absorbed(garden, key, identity),
                    Took::Wild(partner) => learned_wild(identity, partner),
                }
                // Happiness +1 as for an animal; no alignment change.
                std::arch::asm!(
                    "push esi",
                    "mov esi, ecx",
                    "push 0",
                    "call eax",
                    "add esp, 4",
                    "pop esi",
                    inout("eax") ADD_HAPPINESS => _,
                    inout("ecx") chao => _,
                    inout("edi") 1u32 => _,
                    clobber_abi("C"),
                );
            }
        });
    }

    // At ABSORB_SITE: flags from `cmp ebx, 0x15`; EBP = Chao, EDI = animal; FPU empty.
    #[unsafe(naked)]
    unsafe extern "C" fn absorb_hook() {
        std::arch::naked_asm!(
            "pushad",
            "mov esi, esp",
            "and esp, -16",
            "sub esp, 8",
            "push edi",
            "push ebp",
            "call {is_partner}",
            "mov esp, esi",
            "test eax, eax",
            "popad",
            "jz 2f",
            "pushad",
            "mov esi, esp",
            "and esp, -16",
            "sub esp, 8",
            "push edi",
            "push ebp",
            "call {absorbed}",
            "mov esp, esi",
            "popad",
            "push {done}",
            "ret",
            "2:",
            "cmp ebx, 0x15",
            "jge 3f",
            "push {animal}",
            "ret",
            "3:",
            "push {drive}",
            "ret",
            is_partner = sym is_partner,
            absorbed = sym partner_absorbed,
            done = const ABSORB_DONE,
            animal = const ABSORB_ANIMAL,
            drive = const ABSORB_DRIVE,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::{arrival, arrive_at, hop, take_arrival};

    #[test]
    fn a_partner_can_come_out_at_a_point() {
        arrive_at(0x77, [1.0, 2.0, 3.0], 0x4000);
        arrive_at(0x77, [4.0, 5.0, 6.0], 0);
        assert_eq!(take_arrival(0x77), Some(([4.0, 5.0, 6.0], 0)));
        assert_eq!(take_arrival(0x77), None);
        // Facing 0x4000: forward is +x.
        let v = hop(0x4000);
        assert!(v[0] > 0.0 && v[1] > 0.0 && v[2].abs() < 1e-4);
    }

    #[test]
    fn partners_arrive_ahead_side_by_side() {
        // Facing 0: ahead is +z (sin 0, cos 1).
        let (x, z) = arrival(0.0, 0.0, 0, 0);
        assert!((z - 6.0).abs() < 1e-4 && x < 0.0);
        let (x1, _) = arrival(0.0, 0.0, 0, 1);
        let (x2, _) = arrival(0.0, 0.0, 0, 2);
        assert!((x2 - x1 - super::SPREAD).abs() < 1e-4);
    }
}
