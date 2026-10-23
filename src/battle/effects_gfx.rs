//! The ROM's battle animation sprites in the arena (re/chao-battle-visuals.md 3, 5, 6).
//!
//! - `Book`: which sprites show for each move and status, read once from the ROM: a move's
//!   commonest sheets (up to `PICKS`), flying from the user or bursting at the target as its own
//!   script places them; a move whose script makes none takes the commonest sheet of its type.
//! - The game side: each sprite frame becomes a game texture (the same GVM path as the garden's
//!   pictures, `gba_link::billboard`), drawn as a camera-facing quad from a display object while
//!   a battle is on, with the white flash over the screen. The textures last until the battle
//!   ends. It also makes the arena's own effects: sparkles and other particles, and the Chao's
//!   emotion balls.

// Off the game (Linux builds) only the tests use the game side's helpers.
#![cfg_attr(not(target_arch = "x86"), allow(dead_code))]

use std::collections::HashMap;

pub use crate::gba::battle_anim::{Frame, Template};
use crate::gba::battle_anim::{HIT_SCRIPT, STATUS_COUNT};
use crate::gba::rom::{MOVE_COUNT, Rom, TYPE_COUNT};

/// Sheets shown for one move at most.
const PICKS: usize = 3;

/// Status condition scripts by index (the ROM's order).
pub mod status {
    pub const POISON: u8 = 0;
    pub const CONFUSION: u8 = 1;
    pub const BURN: u8 = 2;
    pub const SLEEP: u8 = 4;
    pub const PARALYSIS: u8 = 5;
    pub const FREEZE: u8 = 6;
}

/// What shows for one move.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MoveLook {
    /// Sprites that fly from the user toward the target.
    pub fly: Vec<Template>,
    /// Sprites that burst at the target.
    pub burst: Vec<Template>,
}

impl MoveLook {
    pub fn is_empty(&self) -> bool {
        self.fly.is_empty() && self.burst.is_empty()
    }
}

/// The sprites for every move and status, by id.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Book {
    moves: Vec<MoveLook>,
    hit: Option<Template>,
    status: Vec<Option<Template>>,
}

/// One sprite a script makes: its template and whether it starts at the target.
type Made = (Template, bool);

/// The sprites with a sheet that a script makes.
fn made(rom: &Rom, script: &crate::gba::battle_anim::Script) -> Vec<Made> {
    script
        .created
        .iter()
        .filter_map(|c| Some((rom.anim_template(c.template)?, c.at_target)))
        .collect()
}

/// The template met most often among `templates` (the first of equals).
fn commonest<'a>(templates: impl Iterator<Item = &'a Template>) -> Option<Template> {
    let mut counts: Vec<(&Template, usize)> = Vec::new();
    for t in templates {
        match counts.iter_mut().find(|(c, _)| *c == t) {
            Some((_, n)) => *n += 1,
            None => counts.push((t, 1)),
        }
    }
    let best = counts.iter().map(|&(_, n)| n).max()?;
    counts
        .into_iter()
        .find(|&(_, n)| n == best)
        .map(|(t, _)| t.clone())
}

/// A move's look from the sprites its script makes: its `PICKS` commonest sheets other than the
/// plain hit's (`hit`; in the order first met among equals), each with its commonest template,
/// flying from the user unless most of them start at the target.
pub fn pick(sprites: &[Made], hit: Option<u16>) -> MoveLook {
    let mut tags: Vec<(u16, usize)> = Vec::new();
    for (t, _) in sprites.iter().filter(|(t, _)| Some(t.tag) != hit) {
        match tags.iter_mut().find(|(tag, _)| *tag == t.tag) {
            Some((_, n)) => *n += 1,
            None => tags.push((t.tag, 1)),
        }
    }
    // Stable: equals keep the order they were met in.
    tags.sort_by_key(|&(_, n)| std::cmp::Reverse(n));
    let mut look = MoveLook::default();
    for &(tag, count) in tags.iter().take(PICKS) {
        let of_tag = || sprites.iter().filter(move |(t, _)| t.tag == tag);
        let Some(template) = commonest(of_tag().map(|(t, _)| t)) else {
            continue;
        };
        let at_target = of_tag().filter(|(_, at)| *at).count();
        if at_target * 2 > count {
            look.burst.push(template);
        } else {
            look.fly.push(template);
        }
    }
    look
}

impl Book {
    /// Reads every move's and status's script from the ROM.
    pub fn from_rom(rom: &Rom) -> Book {
        let scripts: Vec<Vec<Made>> = (0..MOVE_COUNT)
            .map(|id| rom.move_anim(id).map(|s| made(rom, &s)).unwrap_or_default())
            .collect();
        let hit = rom
            .general_anim(HIT_SCRIPT)
            .and_then(|s| made(rom, &s).into_iter().next())
            .map(|(t, _)| t);
        let hit_tag = hit.as_ref().map(|t| t.tag);
        let mut moves: Vec<MoveLook> = scripts.iter().map(|s| pick(s, hit_tag)).collect();
        // The commonest look among each type's moves, for the moves with none of their own.
        let type_of = |id: usize| rom.move_info(id as u16).map(|m| m.type_id);
        let mut by_type: HashMap<u8, Vec<Made>> = HashMap::new();
        for (id, look) in moves.iter().enumerate() {
            let Some(t) = type_of(id) else { continue };
            let entry = by_type.entry(t).or_default();
            entry.extend(look.fly.iter().map(|f| (f.clone(), false)));
            entry.extend(look.burst.iter().map(|b| (b.clone(), true)));
        }
        let fallback: HashMap<u8, MoveLook> = (0..TYPE_COUNT)
            .filter_map(|t| {
                let mut look = pick(by_type.get(&t)?, hit_tag);
                look.fly.truncate(1);
                look.burst.truncate(usize::from(look.fly.is_empty()));
                Some((t, look))
            })
            .collect();
        for (id, look) in moves.iter_mut().enumerate() {
            if look.is_empty()
                && let Some(f) = type_of(id).and_then(|t| fallback.get(&t))
            {
                *look = f.clone();
            }
        }
        // A status's own sprite, else a sprite of the sheet its script loads for a task (one
        // of the moves', or its biggest square from the start).
        let status = (0..STATUS_COUNT)
            .map(|i| {
                let script = rom.status_anim(i)?;
                let own: Vec<Made> = made(rom, &script);
                commonest(own.iter().map(|(t, _)| t)).or_else(|| {
                    let tag = *script.loaded.first()?;
                    commonest(
                        scripts
                            .iter()
                            .flatten()
                            .map(|(t, _)| t)
                            .filter(|t| t.tag == tag),
                    )
                    .or_else(|| rom.anim_sheet(tag)?.square(tag))
                })
            })
            .collect();
        Book { moves, hit, status }
    }

    /// The look of move `id` (empty when there's none).
    pub fn for_move(&self, id: u16) -> MoveLook {
        self.moves.get(usize::from(id)).cloned().unwrap_or_default()
    }

    /// The plain hit's sprite.
    pub fn hit(&self) -> Option<&Template> {
        self.hit.as_ref()
    }

    /// Status condition `index`'s sprite (`status::*`).
    pub fn status(&self, index: u8) -> Option<&Template> {
        self.status.get(usize::from(index))?.as_ref()
    }

    /// A book with only these looks (and `status` for every status), for tests.
    #[cfg(test)]
    pub fn for_tests(
        moves: Vec<MoveLook>,
        hit: Option<Template>,
        status: Option<Template>,
    ) -> Book {
        Book {
            moves,
            hit,
            status: vec![status; usize::from(STATUS_COUNT)],
        }
    }
}

/// One sprite frame to draw: which picture, where (its centre) and how big (world units).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Draw {
    pub key: Key,
    pub at: [f32; 3],
    pub size: f32,
}

/// A sprite frame's picture: the sheet, the frame's size and its tiles.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Key {
    pub tag: u16,
    pub width: u8,
    pub height: u8,
    pub frame: Frame,
}

/// Particles of the arena's own (`0x6EFF10` and friends): sparkles, splashes, flames, snow and
/// dust.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Particle {
    Sparkle,
    Splash,
    Flame,
    Snow,
    Dust,
}

/// A `w` x `h` RGBA8 picture in the middle of a clear square (the quads are square), and the
/// square's side.
pub fn pad_square(rgba: &[u8], w: usize, h: usize) -> (Vec<u8>, usize) {
    let side = w.max(h);
    let (left, top) = ((side - w) / 2, (side - h) / 2);
    let mut out = vec![0u8; side * side * 4];
    for y in 0..h {
        let from = y * w * 4;
        let to = ((top + y) * side + left) * 4;
        out[to..to + w * 4].copy_from_slice(&rgba[from..from + w * 4]);
    }
    (out, side)
}

/// What's drawn this frame (set by the battle every frame, drawn by the display object).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Scene {
    pub draws: Vec<Draw>,
    /// The white flash's opacity, 0..=1.
    pub flash: f32,
}

#[cfg(target_arch = "x86")]
pub use game::{emote, end, particles, show};

/// The battle shows `scene` from now on (it starts the display object when it needs one).
#[cfg(not(target_arch = "x86"))]
pub fn show(_scene: Scene, _rom: Option<std::sync::Arc<Rom>>) {}

/// The battle is over (`area_gone`: the arena went with its objects): textures, particle
/// textures and the display object go.
#[cfg(not(target_arch = "x86"))]
pub fn end(_area_gone: bool) {}

/// `count` particles of `kind` around `at`.
#[cfg(not(target_arch = "x86"))]
pub fn particles(_kind: Particle, _at: [f32; 3], _count: u32, _seed: u32) {}

/// Shows emotion ball `state` over the Chao `object` for `frames`.
#[cfg(not(target_arch = "x86"))]
pub fn emote(_object: usize, _state: u8, _frames: u32) {}

#[cfg(target_arch = "x86")]
mod game {
    use std::sync::{Arc, Mutex};

    use super::{Key, Particle, Scene, pad_square};
    use crate::gba::rom::Rom;
    use crate::gba_link::billboard::{camera, facing, gvm, quad};
    use crate::log;

    /// Global texture indices of ours: `BASE + n`, `n` counting up (the garden pictures' are
    /// at 0x7E5A_0000).
    const GBIX_BASE: u32 = 0x7E5B_0000;
    const GBIX_SPAN: u32 = 0x1_0000;
    /// Textures kept at once; the one drawn longest ago goes first.
    const TEXTURES: usize = 64;
    /// Textures made in one frame at most (the rest wait for the next).
    const MADE_PER_FRAME: usize = 6;
    /// The GVM parser (EAX = archive, EBX = NJS_TEXLIST; 1 when done).
    const PARSE_GVM: usize = 0x0042_FC30;
    /// Releases a texture list's textures (cdecl NJS_TEXLIST*).
    const RELEASE: usize = 0x0077_F9F0;
    /// Texture records (0x18 bytes; free when +0xC is 0) and infos (0x28 bytes; free when +0 is
    /// -1): base and count pointers.
    const RECORDS: usize = 0x0267_0590;
    const RECORD_COUNT: usize = 0x0267_058C;
    const INFOS: usize = 0x025F_0278;
    const INFO_COUNT: usize = 0x025E_FFE0;
    /// Info +4: the texture's format. 0xE draws its alpha strips with alpha test and depth
    /// writes (as the garden pictures), so a sprite in front of a Chao stays in front.
    const INFO_FORMAT: usize = 4;
    const ALPHA_TEST_FORMAT: u32 = 0xE;
    /// The game's matrix stack top (the view matrix in a display).
    const MATRIX_TOP: usize = 0x01A5_57FC;
    /// Untextured rectangle (u32 0xAABBGGRR, x0, y0, x1, y1, z), cdecl, in the 640x480 space.
    const DRAW_RECT: usize = 0x0055_9720;
    /// The HUD's depth.
    const RECT_Z: f32 = -1.2;
    /// The flash's rectangle, wide enough for any screen shape.
    const FLASH_RECT: [f32; 4] = [-480.0, -60.0, 1120.0, 540.0];
    /// Emotion ball: ECX = Chao object, DL = state, push frames (caller pops).
    const EMOTE: usize = 0x0053_D660;
    /// The texture list the arena loads (`efftex_common`, `{NJS_TEXNAME*, count}`) and the
    /// particle sets bound to it here: `{frames*, count}`, frames of `{name*, attributes,
    /// texture}` (12 bytes). The arena binds only the sparkles itself.
    const EFFECT_TEXTURES: usize = 0x0104_D80C;
    const BIND_SET: usize = 0x0044_C880;
    const SETS: [usize; 4] = [SPLASH_SET, FLAME_SET, SNOW_SET, DUST_SET];
    const SPLASH_SET: usize = 0x00B0_AC50;
    const FLAME_SET: usize = 0x00B0_AD98;
    const SNOW_SET: usize = 0x00B0_AE50;
    const DUST_SET: usize = 0x00B4_8068;
    /// Spawners, cdecl (&position, &velocity, f32 scale).
    const SPARKLE: usize = 0x006E_FF10;
    const SPLASH: usize = 0x006E_D1C0;
    const FLAME: usize = 0x006E_CA30;
    const SNOW: usize = 0x006E_BE50;
    const DUST: usize = 0x006C_ACF0;
    const DELETE_OBJECT: usize = 0x0046_F720;
    const OBJ_MAIN: usize = 0x10;
    const NAME: &[u8] = b"SA2ChaoOnlineBattleFx\0";
    const TEXNAME: usize = 12;

    /// One texture of ours.
    struct Texture {
        key: Key,
        /// NJS_TEXNAME (name, attributes, texture) and the list over it.
        name: Box<[u32; 3]>,
        list: Box<[u32; 2]>,
        /// The scene it was made in, and its record then.
        scene: (i16, u32, i32),
        record: [usize; 4],
        /// Display frame it was last drawn in.
        drawn: u32,
    }

    /// The quad model: NJS_OBJECT, NJS_CNK_MODEL and the two chunk lists (1 unit, feet at 0).
    struct Model {
        object: Box<[u32; 13]>,
        _model: Box<[u32; 6]>,
        _vlist: Box<[u32]>,
        _plist: Box<[u32]>,
    }

    struct Fx {
        scene: Scene,
        rom: Option<Arc<Rom>>,
        textures: Vec<Texture>,
        /// Keys that couldn't be made this battle.
        failed: Vec<Key>,
        sheets: Vec<(u16, Arc<crate::gba::battle_anim::Sheet>)>,
        model: Option<Model>,
        object: usize,
        next_gbix: u32,
        frame: u32,
        /// The particle frames we bound this battle: (frame entry, texture we put there).
        bound: Option<Vec<(usize, u32)>>,
    }

    // SAFETY: the raw words are only used on the game thread.
    unsafe impl Send for Fx {}

    static FX: Mutex<Fx> = Mutex::new(Fx {
        scene: Scene {
            draws: Vec::new(),
            flash: 0.0,
        },
        rom: None,
        textures: Vec::new(),
        failed: Vec::new(),
        sheets: Vec::new(),
        model: None,
        object: 0,
        next_gbix: 0,
        frame: 0,
        bound: None,
    });

    fn lock() -> std::sync::MutexGuard<'static, Fx> {
        FX.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn read<T: Copy>(address: usize) -> T {
        // SAFETY: callers pass addresses of live game data.
        unsafe { (address as *const T).read_unaligned() }
    }

    fn write<T>(address: usize, value: T) {
        // SAFETY: as in `read`.
        unsafe { (address as *mut T).write_unaligned(value) }
    }

    fn scene_id() -> (i16, u32, i32) {
        (
            crate::game::current_level(),
            crate::game::stage_run(),
            crate::game::current_chao_area(),
        )
    }

    /// A texture record's (+0, +8, +0xC).
    fn record_of(record: usize) -> [usize; 4] {
        if record == 0 {
            return [0; 4];
        }
        [record, read(record), read(record + 8), read(record + 0xC)]
    }

    fn still_ours(t: &Texture) -> bool {
        t.scene == scene_id() && t.record[3] != 0 && record_of(t.name[2] as usize) == t.record
    }

    /// Free texture records and infos, at least.
    fn room(n: usize) -> bool {
        let free = |base: usize, count: usize, stride: usize, at: usize, free: usize| {
            let base: usize = read(base);
            let count = read::<u32>(count) as usize;
            if base == 0 {
                return 0;
            }
            (0..count.min(0x2000))
                .filter(|i| read::<usize>(base + i * stride + at) == free)
                .count()
        };
        free(RECORDS, RECORD_COUNT, 0x18, 0xC, 0) >= n
            && free(INFOS, INFO_COUNT, 0x28, 0, usize::MAX) >= n
    }

    fn release(t: &Texture) {
        if still_ours(t) {
            // SAFETY: our list; its record is still ours in this scene.
            unsafe {
                std::mem::transmute::<usize, extern "C" fn(usize)>(RELEASE)(t.list.as_ptr() as usize)
            };
        }
    }

    fn model() -> Model {
        let (vlist, plist) = quad(1.0);
        let vlist = vlist.into_boxed_slice();
        let plist = plist.into_boxed_slice();
        let model = Box::new([
            vlist.as_ptr() as u32,
            plist.as_ptr() as u32,
            0,
            0.5f32.to_bits(),
            0,
            0, // radius 0: never culled by its sphere
        ]);
        let one = 1f32.to_bits();
        let mut object = Box::new([0u32; 13]);
        // Unit position, angle and scale, and no children (0x10).
        object[0] = 0x17;
        object[1] = model.as_ptr() as u32;
        object[8] = one;
        object[9] = one;
        object[10] = one;
        Model {
            object,
            _model: model,
            _vlist: vlist,
            _plist: plist,
        }
    }

    /// Makes the texture for `key` (its picture from the ROM's sheet).
    fn make(fx: &mut Fx, key: Key) -> Option<Texture> {
        let rom = fx.rom.clone()?;
        let sheet = match fx.sheets.iter().find(|(tag, _)| *tag == key.tag) {
            Some((_, s)) => s.clone(),
            None => {
                let s = Arc::new(rom.anim_sheet(key.tag)?);
                fx.sheets.push((key.tag, s.clone()));
                s
            }
        };
        let rgba = sheet.frame(key.width, key.height, key.frame)?;
        let (square, side) = pad_square(&rgba, usize::from(key.width), usize::from(key.height));
        if !room(4) {
            return None;
        }
        let gbix = GBIX_BASE + fx.next_gbix;
        fx.next_gbix = (fx.next_gbix + 1) % GBIX_SPAN;
        let archive = gvm(&[square], side, side, gbix);
        let mut name = Box::new([0u32; 3]);
        let list = Box::new([name.as_mut_ptr() as u32, 1]);
        let result: i32;
        // SAFETY: a checked archive we built (the parser byte-swaps its header in place and
        // keeps no pointer into it), a list with one entry, on the game thread.
        unsafe {
            std::arch::asm!(
                "push ebx",
                "mov ebx, {list}",
                "call {f}",
                "pop ebx",
                list = in(reg) list.as_ptr(),
                f = in(reg) PARSE_GVM,
                inlateout("eax") archive.as_ptr() => result,
                clobber_abi("C"),
            );
        }
        if result != 1 || name[2] == 0 {
            return None;
        }
        let info: usize = read(name[2] as usize + 0xC);
        if info != 0 {
            write(info + INFO_FORMAT, ALPHA_TEST_FORMAT);
        }
        let record = record_of(name[2] as usize);
        Some(Texture {
            key,
            name,
            list,
            scene: scene_id(),
            record,
            drawn: fx.frame,
        })
    }

    /// The texture list (one entry) for `key`, made when needed; `None` when it can't be.
    fn texture(fx: &mut Fx, key: Key, made: &mut usize) -> Option<usize> {
        if let Some(t) = fx.textures.iter_mut().find(|t| t.key == key) {
            t.drawn = fx.frame;
            return Some(t.list.as_ptr() as usize);
        }
        if fx.failed.contains(&key) || *made >= MADE_PER_FRAME {
            return None;
        }
        if fx.textures.len() >= TEXTURES {
            // The one drawn longest ago goes, never one drawn this frame.
            let oldest = (0..fx.textures.len()).min_by_key(|&i| fx.textures[i].drawn)?;
            if fx.textures[oldest].drawn == fx.frame {
                return None;
            }
            let old = fx.textures.remove(oldest);
            release(&old);
        }
        *made += 1;
        match make(fx, key) {
            Some(t) => {
                let list = t.list.as_ptr() as usize;
                fx.textures.push(t);
                Some(list)
            }
            None => {
                log!("battle: no texture for sheet {}", key.tag);
                fx.failed.push(key);
                None
            }
        }
    }

    /// Binds the particle sets the arena leaves empty to its loaded effect textures
    /// (`0x44C880`), once a battle, keeping what we put there.
    fn bind_particles(fx: &mut Fx) {
        if fx.bound.is_some() {
            return;
        }
        let names: usize = read(EFFECT_TEXTURES);
        if names == 0 || read::<u32>(names + 8) == 0 {
            // Not loaded (yet).
            return;
        }
        let mut bound = Vec::new();
        for set in SETS {
            let frames: usize = read(set);
            let count = (read::<u32>(set + 4) as usize).min(16);
            let entries = || (0..count).map(|i| frames + i * TEXNAME);
            if frames == 0 || entries().any(|e| read::<u32>(e + 8) != 0) {
                continue;
            }
            // SAFETY: the game's set binder (EAX = set, push NJS_TEXLIST*, caller pops) with a
            // static set and the arena's loaded list, on the game thread.
            unsafe {
                std::arch::asm!(
                    "push {list}",
                    "call {f}",
                    "add esp, 4",
                    list = in(reg) EFFECT_TEXTURES,
                    f = in(reg) BIND_SET,
                    inout("eax") set => _,
                    clobber_abi("C"),
                );
            }
            bound.extend(
                entries()
                    .map(|e| (e, read::<u32>(e + 8)))
                    .filter(|&(_, texture)| texture != 0),
            );
        }
        fx.bound = Some(bound);
    }

    /// Takes our particle textures back: frames that still hold what we put there go empty
    /// (a frame with no texture isn't drawn).
    fn unbind_particles(fx: &mut Fx) {
        for (entry, texture) in fx.bound.take().unwrap_or_default() {
            if read::<u32>(entry + 8) == texture {
                write(entry + 4, 0u32);
                write(entry + 8, 0u32);
            }
        }
    }

    pub fn show(scene: Scene, rom: Option<Arc<Rom>>) {
        let mut fx = lock();
        if rom.is_some() {
            fx.rom = rom;
        }
        fx.scene = scene;
        bind_particles(&mut fx);
        if fx.object == 0 {
            fx.object = crate::puppet::create_display_object(NAME, display, deleted);
        }
    }

    pub fn end(area_gone: bool) {
        let mut fx = lock();
        if !area_gone {
            for t in &fx.textures {
                release(t);
            }
            if fx.object != 0 {
                write(fx.object + OBJ_MAIN, DELETE_OBJECT);
            }
        }
        fx.textures.clear();
        fx.failed.clear();
        fx.sheets.clear();
        fx.scene = Scene::default();
        // The particle frames are the game's statics: emptied either way.
        unbind_particles(&mut fx);
        fx.object = 0;
    }

    pub fn particles(kind: Particle, at: [f32; 3], count: u32, seed: u32) {
        // Spawner, scale (the absorb ring's sparkles are 1, an animal's flames 0.25), ring
        // radius, rise per frame.
        let (spawner, scale, radius, rise) = match kind {
            Particle::Sparkle => (SPARKLE, 1.0f32, 1.0f32, 0.03f32),
            Particle::Splash => (SPLASH, 0.6, 0.6, 0.08),
            Particle::Flame => (FLAME, 0.3, 0.5, 0.04),
            Particle::Snow => (SNOW, 0.5, 0.8, 0.01),
            Particle::Dust => (DUST, 0.8, 0.8, 0.02),
        };
        for i in 0..count {
            let step = 360 / count.max(1);
            let a = (seed.wrapping_mul(37).wrapping_add(i * step) % 360) as f32;
            let (s, c) = a.to_radians().sin_cos();
            let position = [at[0] + c * radius, at[1], at[2] + s * radius];
            let velocity = [c * 0.02, rise, s * 0.02];
            // SAFETY: the game's particle spawner, cdecl (&position, &velocity, f32 scale);
            // it returns at once when its pool is full or particles are off.
            unsafe {
                let f: extern "C" fn(*const [f32; 3], *const [f32; 3], f32) =
                    std::mem::transmute(spawner);
                f(&position, &velocity, scale);
            }
        }
    }

    pub fn emote(object: usize, state: u8, frames: u32) {
        if !crate::battle::arena::alive(object) {
            return;
        }
        // SAFETY: the game's emotion ball setter (ECX = Chao object, DL = state, one cdecl
        // argument) on one of our live arena Chao, on the game thread.
        unsafe {
            std::arch::asm!(
                "push {frames}",
                "call {f}",
                "add esp, 4",
                frames = in(reg) frames,
                f = in(reg) EMOTE,
                inout("ecx") object => _,
                inout("edx") u32::from(state) => _,
                clobber_abi("C"),
            );
        }
    }

    extern "C" fn deleted(object: usize) {
        let _ = std::panic::catch_unwind(|| {
            let mut fx = lock();
            if fx.object == object {
                fx.object = 0;
            }
        });
    }

    /// The display object's display: the sprites, then the flash.
    extern "C" fn display(_object: usize) {
        crate::watch::mark("battle fx");
        let _ = std::panic::catch_unwind(|| {
            let mut fx = lock();
            fx.frame = fx.frame.wrapping_add(1);
            let now = scene_id();
            fx.textures.retain(|t| t.scene == now);
            let draws = fx.scene.draws.clone();
            let flash = fx.scene.flash;
            if !draws.is_empty() {
                if fx.model.is_none() {
                    fx.model = Some(model());
                }
                let top: usize = read(MATRIX_TOP);
                if top == 0 {
                    return;
                }
                let cam = camera(&read::<[f32; 12]>(top));
                let mut made = 0;
                for d in draws {
                    let Some(list) = texture(&mut fx, d.key, &mut made) else {
                        continue;
                    };
                    let Some(object) = fx.model.as_ref().map(|m| m.object.as_ptr() as usize) else {
                        return;
                    };
                    let feet = [d.at[0], d.at[1] - d.size / 2.0, d.at[2]];
                    let yaw = facing(feet, cam);
                    // SAFETY: display pass on the game thread; our quad and texture list.
                    unsafe {
                        crate::puppet::draw_solid(
                            object,
                            list,
                            feet,
                            [0, 0x8000i32.wrapping_sub(yaw), 0],
                            d.size,
                            crate::puppet::Tilt::default(),
                            None,
                        );
                    }
                }
            }
            // The flash only lasts while the battle runs (not through a pause).
            if flash > 0.0 && !crate::battle::arena::paused() {
                let alpha = (flash.clamp(0.0, 1.0) * 255.0) as u32;
                let colour = alpha.min(0xFE) << 24 | 0x00FF_FFFF;
                let [x0, y0, x1, y1] = FLASH_RECT;
                // SAFETY: the game's rectangle draw (cdecl), in a display pass.
                unsafe {
                    let f: extern "C" fn(u32, f32, f32, f32, f32, f32) =
                        std::mem::transmute(DRAW_RECT);
                    f(colour, x0, y0, x1, y1, RECT_Z);
                }
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn template(tag: u16, tile: u16) -> Template {
        Template {
            tag,
            width: 16,
            height: 16,
            frames: vec![Frame {
                tile,
                duration: 4,
                h_flip: false,
                v_flip: false,
            }],
        }
    }

    #[test]
    fn a_move_shows_its_commonest_sheets_where_its_script_puts_them() {
        const HIT_TAG: u16 = 10_135;
        let sprites = vec![
            (template(HIT_TAG, 0), true),
            (template(10_001, 0), false),
            (template(10_002, 4), true),
            (template(10_002, 8), true),
            (template(10_002, 4), false),
            (template(10_003, 0), false),
            (template(10_004, 0), false),
            (template(HIT_TAG, 0), true),
            (template(HIT_TAG, 0), true),
        ];
        let look = pick(&sprites, Some(HIT_TAG));
        // 10002 most often (mostly at the target, its commonest template), then the first two
        // of the equals; never the plain hit.
        assert_eq!(look.burst, [template(10_002, 4)]);
        assert_eq!(look.fly, [template(10_001, 0), template(10_003, 0)]);
        assert!(pick(&[(template(HIT_TAG, 0), true)], Some(HIT_TAG)).is_empty());
        assert!(pick(&[], Some(HIT_TAG)).is_empty());
        assert_eq!(pick(&[(template(HIT_TAG, 0), true)], None).burst.len(), 1);
    }

    #[test]
    fn pictures_are_centred_in_a_square() {
        // A 4x2 picture of opaque pixels, numbered.
        let rgba: Vec<u8> = (0..8u8).flat_map(|i| [i, 0, 0, 255]).collect();
        let (out, side) = pad_square(&rgba, 4, 2);
        assert_eq!(side, 4);
        let px = |x: usize, y: usize| out[(y * 4 + x) * 4..(y * 4 + x) * 4 + 4].to_vec();
        assert_eq!(px(0, 0), [0, 0, 0, 0]);
        assert_eq!(px(0, 1), [0, 0, 0, 255]);
        assert_eq!(px(3, 2), [7, 0, 0, 255]);
        assert_eq!(px(3, 3), [0, 0, 0, 0]);
        let (same, side) = pad_square(&rgba[..4 * 4], 2, 2);
        assert_eq!((same.as_slice(), side), (&rgba[..16], 2));
    }

    #[test]
    fn reads_the_rom() {
        let Some(home) = std::env::var_os("HOME") else {
            return;
        };
        let path = std::path::Path::new(&home).join("Games/gba/emerald.gba");
        if !path.exists() {
            println!("skipped: no GBA ROM at {}", path.display());
            return;
        }
        let rom = Rom::open(&path).expect("ROM opens");
        let book = Book::from_rom(&rom);
        let hit = book.hit().expect("the plain hit's sprite").tag;
        assert!(rom.anim_sheet(hit).is_some());
        // Every move has something to show (its own or its type's).
        for id in 1..MOVE_COUNT {
            let look = book.for_move(id);
            assert!(!look.is_empty(), "move {id}");
            assert!(look.fly.len() + look.burst.len() <= PICKS);
            for t in look.fly.iter().chain(&look.burst) {
                let sheet = rom.anim_sheet(t.tag).expect("sheet");
                assert!(sheet.frame(t.width, t.height, t.frames[0]).is_some());
            }
        }
        assert!(book.for_move(0).is_empty());
        for index in [
            status::CONFUSION,
            status::BURN,
            status::SLEEP,
            status::PARALYSIS,
            status::FREEZE,
        ] {
            assert!(book.status(index).is_some(), "status {index}");
        }
    }
}
