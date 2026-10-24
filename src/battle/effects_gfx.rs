//! The ROM's battle animation sprites in the arena (re/chao-battle-visuals.md 3, 5, 6).
//!
//! - `Book`: what shows for each move and status, read once from the ROM: each move's main
//!   sprite and how it moves (`gba::anim_kinds`), a burst beside it, and whether it shakes or
//!   tints; a move whose script shows nothing of its own borrows the look of another move of
//!   its type (picked by its id).
//! - The game side: each sprite frame becomes a game texture (the same GVM path as the garden's
//!   pictures, `gba_link::billboard`), drawn as a camera-facing quad from a display object while
//!   a battle is on, a little in front of where it is so the Chao don't hide it (the ones the
//!   GBA draws see-through are blended over the Chao, after they draw), with the flash over the
//!   screen. The textures last until the battle ends, which logs how many were made
//!   and drawn. It also makes the arena's own effects: sparkles and other particles, and the
//!   Chao's emotion balls.

// Off the game (Linux builds) only the tests use the game side's helpers.
#![cfg_attr(not(target_arch = "x86"), allow(dead_code))]

use std::collections::{HashMap, HashSet};

pub use crate::gba::anim_kinds::{Movement, Place};
use crate::gba::anim_kinds::{Plan, Source};
pub use crate::gba::battle_anim::{Frame, Template};
use crate::gba::battle_anim::{HIT_SCRIPT, SHEET_COUNT, STATUS_COUNT, Script, TAG_BASE};
use crate::gba::rom::{MOVE_COUNT, Rom};

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
    pub movement: Movement,
    /// Where a movement that stays in one place shows.
    pub place: Place,
    /// Its main sprite, and a burst on the target beside it.
    pub main: Option<Template>,
    pub impact: Option<Template>,
    /// Sprites its script makes of the main one, and the frames between them.
    pub count: u16,
    pub gap: u16,
    /// The main shot's frames and arc height (GBA pixels), when its script gives them.
    pub duration: Option<u16>,
    pub height: Option<i16>,
    /// The target shakes; the screen shakes; the tint colour (BGR555).
    pub shake: bool,
    pub big: bool,
    pub flash: Option<u16>,
    /// Borrowed from another move of its type.
    pub borrowed: bool,
}

impl MoveLook {
    pub fn is_empty(&self) -> bool {
        self.main.is_none() && self.impact.is_none()
    }

    /// A look of `movement` with `main`, for tests.
    #[cfg(test)]
    pub fn of(movement: Movement, main: Template) -> MoveLook {
        MoveLook {
            movement,
            main: Some(main),
            count: 1,
            ..MoveLook::default()
        }
    }
}

/// The sprites for every move and status, by id.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Book {
    moves: Vec<MoveLook>,
    hit: Option<Template>,
    status: Vec<Option<Template>>,
}

/// The sprites with a sheet that a script makes.
fn made(rom: &Rom, script: &Script) -> Vec<Template> {
    script
        .created
        .iter()
        .filter_map(|c| rom.anim_template(c.template))
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

/// The template of the size met most often among `templates` (the smaller of equals), the
/// first of that size.
fn usual(templates: &[Template]) -> Option<Template> {
    let size = |t: &Template| (t.width, t.height);
    let count = |t: &Template| templates.iter().filter(|o| size(o) == size(t)).count();
    let best = templates
        .iter()
        .max_by_key(|t| {
            (
                count(t),
                std::cmp::Reverse(u16::from(t.width) * u16::from(t.height)),
            )
        })
        .map(size)?;
    templates.iter().find(|t| size(t) == best).cloned()
}

/// A move's look from its plan: `template` reads a template, `sheet` finds a template for a
/// sheet a task draws from, `hit` is the plain hit's.
pub fn look_of(
    plan: &Plan,
    template: impl Fn(usize) -> Option<Template>,
    sheet: impl Fn(u16) -> Option<Template>,
    hit: Option<&Template>,
) -> MoveLook {
    let main = match plan.movement {
        Movement::HitOnly => hit.cloned(),
        _ => match plan.main {
            Some(Source::Template(t)) => template(t),
            Some(Source::Sheet(tag)) => sheet(tag),
            None => None,
        },
    };
    MoveLook {
        movement: plan.movement,
        place: plan.place,
        main,
        impact: plan.impact.and_then(&template),
        count: match plan.movement {
            Movement::HitOnly => plan.hits.max(1),
            _ => plan.count,
        },
        gap: plan.gap,
        duration: plan.duration,
        height: plan.height,
        shake: plan.shake,
        big: plan.big,
        flash: plan.flash,
        borrowed: false,
    }
}

/// Lends move looks to the moves with none: each takes one of `own` (the looks of its type's
/// moves that have one) by its id; its shakes and tints stay its own.
pub fn lend(look: &mut MoveLook, id: u16, own: &[&MoveLook]) {
    if !look.is_empty() || own.is_empty() {
        return;
    }
    let from = own[usize::from(id) % own.len()];
    *look = MoveLook {
        shake: look.shake,
        big: look.big,
        flash: look.flash,
        borrowed: true,
        ..from.clone()
    };
}

impl Book {
    /// Reads every move's and status's script from the ROM.
    pub fn from_rom(rom: &Rom) -> Book {
        let scripts: Vec<Vec<Template>> = (0..MOVE_COUNT)
            .map(|id| rom.move_anim(id).map(|s| made(rom, &s)).unwrap_or_default())
            .collect();
        let hit = rom
            .general_anim(HIT_SCRIPT)
            .and_then(|s| made(rom, &s).into_iter().next());
        let hit_tag = hit.as_ref().map(|t| t.tag);
        // A sheet that only a task draws from: the commonest template of it in any move's
        // script, else one of the ROM's templates for it (cut the way the game cuts it), else
        // its first tile.
        let in_scripts: HashSet<u16> = scripts.iter().flatten().map(|t| t.tag).collect();
        let others: HashSet<u16> = (TAG_BASE..TAG_BASE + SHEET_COUNT)
            .filter(|t| !in_scripts.contains(t))
            .collect();
        let found = rom.sheet_templates(&others);
        let of_sheet = |tag: u16| {
            commonest(scripts.iter().flatten().filter(|t| t.tag == tag))
                .or_else(|| found.get(&tag).and_then(|ts| usual(ts)))
                .or_else(|| rom.anim_sheet(tag).map(|_| Template::first_tile(tag)))
        };
        let mut moves: Vec<MoveLook> = (0..MOVE_COUNT)
            .map(|id| {
                rom.move_plan(id, hit_tag)
                    .map(|plan| look_of(&plan, |t| rom.anim_template(t), of_sheet, hit.as_ref()))
                    .unwrap_or_default()
            })
            .collect();
        let type_of = |id: usize| rom.move_info(id as u16).map(|m| m.type_id);
        let mut by_type: HashMap<u8, Vec<MoveLook>> = HashMap::new();
        for (id, look) in moves.iter().enumerate() {
            if let Some(t) = type_of(id)
                && !look.is_empty()
                && look.movement != Movement::HitOnly
            {
                by_type.entry(t).or_default().push(look.clone());
            }
        }
        for (id, look) in moves.iter_mut().enumerate().skip(1) {
            if let Some(own) = type_of(id).and_then(|t| by_type.get(&t)) {
                let own: Vec<&MoveLook> = own.iter().collect();
                lend(look, id as u16, &own);
            }
        }
        // A status's own sprite, else a sprite of the sheet its script loads for a task.
        let status = (0..STATUS_COUNT)
            .map(|i| {
                let script = rom.status_anim(i)?;
                commonest(made(rom, &script).iter()).or_else(|| of_sheet(*script.loaded.first()?))
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

    /// One line for the log: how many moves have a look of their own, borrowed or none, and
    /// how many of each movement.
    pub fn summary(&self) -> String {
        let moves = self.moves.iter().skip(1);
        let own = moves
            .clone()
            .filter(|l| !l.is_empty() && !l.borrowed)
            .count();
        let borrowed = moves.clone().filter(|l| l.borrowed).count();
        let none = moves.clone().filter(|l| l.is_empty()).count();
        let mut counts: Vec<(Movement, usize)> = Vec::new();
        for l in moves.filter(|l| !l.is_empty()) {
            match counts.iter_mut().find(|(m, _)| *m == l.movement) {
                Some((_, n)) => *n += 1,
                None => counts.push((l.movement, 1)),
            }
        }
        counts.sort_by_key(|&(_, n)| std::cmp::Reverse(n));
        let each: Vec<String> = counts.iter().map(|(m, n)| format!("{m:?} {n}")).collect();
        format!(
            "{own} moves with their own look, {borrowed} borrowed from their type, {none} with \
             none; {}",
            each.join(", ")
        )
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

/// A sprite frame's picture: the sheet, the frame's size and its tiles (the frame's duration
/// 0: it doesn't change the picture), and whether it's see-through.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Key {
    pub tag: u16,
    pub width: u8,
    pub height: u8,
    pub frame: Frame,
    pub blended: bool,
}

/// The opacity of a see-through sprite's pixels (4 of RGB5A3's 7 steps).
pub const BLENDED_ALPHA: u8 = 0x92;

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
    /// The flash's opacity, 0..=1, and colour (0xRRGGBB).
    pub flash: f32,
    pub flash_colour: u32,
}

/// World units a sprite is drawn nearer the camera than where it is, so the Chao and the
/// trainers' figures it's on don't hide it.
pub const NEARER: f32 = 1.8;

/// `at` moved `by` toward `camera` (not past it).
pub fn nearer(at: [f32; 3], camera: [f32; 3], by: f32) -> [f32; 3] {
    let d: [f32; 3] = std::array::from_fn(|i| camera[i] - at[i]);
    let length = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt();
    if length < 1e-3 {
        return at;
    }
    let k = by.min(length * 0.5) / length;
    std::array::from_fn(|i| at[i] + d[i] * k)
}

#[cfg(target_arch = "x86")]
pub use game::{emote, end, particles, show};

/// The battle shows `scene` from now on (it starts the display object when it needs one).
#[cfg(not(target_arch = "x86"))]
pub fn show(_scene: Scene, _rom: Option<std::sync::Arc<Rom>>) {}

/// The battle is over (`area_gone`: the arena went with its objects; `effects`: it ran with
/// the full show): textures, particle textures and the display object go.
#[cfg(not(target_arch = "x86"))]
pub fn end(_area_gone: bool, _effects: bool) {}

/// `count` particles of `kind` around `at`.
#[cfg(not(target_arch = "x86"))]
pub fn particles(_kind: Particle, _at: [f32; 3], _count: u32, _seed: u32) {}

/// Shows emotion ball `state` over the Chao `object` for `frames`.
#[cfg(not(target_arch = "x86"))]
pub fn emote(_object: usize, _state: u8, _frames: u32) {}

#[cfg(target_arch = "x86")]
mod game {
    use std::sync::{Arc, Mutex};

    use super::{BLENDED_ALPHA, Key, NEARER, Particle, Scene, nearer, pad_square};
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
    /// writes (as the garden pictures), so a sprite in front of a Chao stays in front; a
    /// see-through sprite keeps the parser's, which blends without depth writes.
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
    /// The later display, run after every object list's own displays (so after the arena
    /// Chao's): the see-through sprites blend over them there.
    const OBJ_LATER_DISPLAY: usize = 0x1C;
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

    /// Frames before a texture that couldn't be made for want of room is tried again.
    const RETRY_AFTER: u32 = 30;

    /// Why a texture wasn't made.
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    enum Fail {
        NoRom,
        Frame,
        Room,
        Parse,
    }

    /// What happened this battle, for its log line.
    #[derive(Clone, Copy, Debug, Default)]
    struct Stats {
        made: u32,
        no_rom: u32,
        frame: u32,
        room: u32,
        parse: u32,
        evicted: u32,
        /// Draws skipped for the per-frame limit or a full cache.
        waited: u32,
        displays: u32,
        drawn: u32,
        peak: usize,
        /// Draws the battle asked for, summed over its frames.
        asked: u32,
        /// Frames whose draws no display drew.
        unseen: u32,
        no_object: u32,
        deleted: u32,
    }

    struct Fx {
        scene: Scene,
        rom: Option<Arc<Rom>>,
        textures: Vec<Texture>,
        /// Keys that couldn't be made, and the display frame to try again (never for good).
        failed: Vec<(Key, u32)>,
        sheets: Vec<(u16, Arc<crate::gba::battle_anim::Sheet>)>,
        model: Option<Model>,
        object: usize,
        next_gbix: u32,
        frame: u32,
        /// Free texture records and infos counted this display frame.
        free: Option<(usize, usize)>,
        /// The scene's draws are still to be drawn.
        pending: bool,
        /// Solid sprites drawn and textures made this display frame.
        solid: usize,
        made: usize,
        /// A battle with the full show ran since the last `end`.
        on: bool,
        stats: Stats,
        /// The particle frames we bound this battle: (frame entry, texture we put there).
        bound: Option<Vec<(usize, u32)>>,
    }

    // SAFETY: the raw words are only used on the game thread.
    unsafe impl Send for Fx {}

    static FX: Mutex<Fx> = Mutex::new(Fx {
        scene: Scene {
            draws: Vec::new(),
            flash: 0.0,
            flash_colour: 0,
        },
        rom: None,
        textures: Vec::new(),
        failed: Vec::new(),
        sheets: Vec::new(),
        model: None,
        object: 0,
        next_gbix: 0,
        frame: 0,
        free: None,
        pending: false,
        solid: 0,
        made: 0,
        on: false,
        stats: Stats {
            made: 0,
            no_rom: 0,
            frame: 0,
            room: 0,
            parse: 0,
            evicted: 0,
            waited: 0,
            displays: 0,
            drawn: 0,
            peak: 0,
            asked: 0,
            unseen: 0,
            no_object: 0,
            deleted: 0,
        },
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

    /// Free texture records and infos (each table read at most 0x2000 entries far).
    fn free_slots() -> (usize, usize) {
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
        (
            free(RECORDS, RECORD_COUNT, 0x18, 0xC, 0),
            free(INFOS, INFO_COUNT, 0x28, 0, usize::MAX),
        )
    }

    /// At least `n` free texture records and infos (counted once a display frame; each texture
    /// made takes one of each).
    fn room(fx: &mut Fx, n: usize) -> bool {
        let (records, infos) = *fx.free.get_or_insert_with(free_slots);
        records >= n && infos >= n
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

    /// `key`'s picture from the ROM's sheet, as a square, and the square's side.
    fn picture(fx: &mut Fx, key: Key) -> Result<(Vec<u8>, usize), Fail> {
        let rom = fx.rom.clone().ok_or(Fail::NoRom)?;
        let sheet = match fx.sheets.iter().find(|(tag, _)| *tag == key.tag) {
            Some((_, s)) => s.clone(),
            None => {
                let s = Arc::new(rom.anim_sheet(key.tag).ok_or(Fail::Frame)?);
                fx.sheets.push((key.tag, s.clone()));
                s
            }
        };
        let mut rgba = sheet
            .frame(key.width, key.height, key.frame)
            .ok_or(Fail::Frame)?;
        if key.blended {
            for p in rgba.chunks_mut(4).filter(|p| p[3] != 0) {
                p[3] = BLENDED_ALPHA;
            }
        }
        Ok(pad_square(
            &rgba,
            usize::from(key.width),
            usize::from(key.height),
        ))
    }

    /// Makes the texture for `key` from its `square` picture.
    fn make(fx: &mut Fx, key: Key, square: Vec<u8>, side: usize) -> Result<Texture, Fail> {
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
            // Count again: what it took is unknown.
            fx.free = None;
            return Err(Fail::Parse);
        }
        // It took a record and an info.
        if let Some((records, infos)) = fx.free.as_mut() {
            *records = records.saturating_sub(1);
            *infos = infos.saturating_sub(1);
        }
        let info: usize = read(name[2] as usize + 0xC);
        if info != 0 && !key.blended {
            write(info + INFO_FORMAT, ALPHA_TEST_FORMAT);
        }
        let record = record_of(name[2] as usize);
        Ok(Texture {
            key,
            name,
            list,
            scene: scene_id(),
            record,
            drawn: fx.frame,
        })
    }

    /// Notes that `key` couldn't be made: for good when its picture can't be, else (no ROM yet,
    /// no room, the parser said no) to try again a little later.
    fn failed(fx: &mut Fx, key: Key, why: Fail) {
        let s = &mut fx.stats;
        let again = match why {
            Fail::NoRom => {
                s.no_rom += 1;
                fx.frame.wrapping_add(RETRY_AFTER)
            }
            Fail::Frame => {
                s.frame += 1;
                u32::MAX
            }
            Fail::Room => {
                s.room += 1;
                fx.frame.wrapping_add(RETRY_AFTER)
            }
            Fail::Parse => {
                s.parse += 1;
                fx.frame.wrapping_add(RETRY_AFTER)
            }
        };
        // Once: the others are tried again and counted for the battle's line.
        if why == Fail::Frame {
            log!("battle: no texture for sheet {} (its frame)", key.tag);
        }
        fx.failed.retain(|(k, _)| *k != key);
        fx.failed.push((key, again));
    }

    /// The texture list (one entry) for `key`, made when needed; `None` when it can't be (yet).
    fn texture(fx: &mut Fx, key: Key, made: &mut usize) -> Option<usize> {
        let frame = fx.frame;
        if let Some(t) = fx.textures.iter_mut().find(|t| t.key == key) {
            t.drawn = frame;
            return Some(t.list.as_ptr() as usize);
        }
        if let Some(&(_, again)) = fx.failed.iter().find(|(k, _)| *k == key) {
            if again == u32::MAX || frame < again {
                return None;
            }
            fx.failed.retain(|(k, _)| *k != key);
        }
        if *made >= MADE_PER_FRAME {
            fx.stats.waited += 1;
            return None;
        }
        // The one drawn longest ago, never one drawn this frame.
        let oldest = |fx: &Fx| {
            (0..fx.textures.len())
                .filter(|&i| fx.textures[i].drawn != frame)
                .min_by_key(|&i| fx.textures[i].drawn)
        };
        if fx.textures.len() >= TEXTURES && oldest(fx).is_none() {
            fx.stats.waited += 1;
            return None;
        }
        *made += 1;
        let (square, side) = match picture(fx, key) {
            Ok(p) => p,
            Err(why) => {
                failed(fx, key, why);
                return None;
            }
        };
        // Room in the game's pools, and in our cache: our oldest goes when that makes it.
        let full = fx.textures.len() >= TEXTURES;
        if (full || !room(fx, 4))
            && let Some(i) = oldest(fx)
        {
            let old = fx.textures.remove(i);
            release(&old);
            fx.stats.evicted += 1;
            // Count again: what the release gave back is unknown.
            fx.free = None;
        }
        if !room(fx, 4) {
            failed(fx, key, Fail::Room);
            return None;
        }
        match make(fx, key, square, side) {
            Ok(t) => {
                let list = t.list.as_ptr() as usize;
                fx.textures.push(t);
                fx.stats.made += 1;
                Some(list)
            }
            Err(why) => {
                failed(fx, key, why);
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
        fx.on = true;
        if rom.is_some() {
            fx.rom = rom;
        }
        if fx.pending {
            fx.stats.unseen += 1;
        }
        fx.pending = !scene.draws.is_empty();
        fx.stats.asked += scene.draws.len() as u32;
        fx.scene = scene;
        bind_particles(&mut fx);
        if fx.object == 0 {
            fx.object = crate::puppet::create_display_object(NAME, display, deleted);
            if fx.object == 0 {
                fx.stats.no_object += 1;
            } else {
                write(
                    fx.object + OBJ_LATER_DISPLAY,
                    display_later as *const () as usize,
                );
            }
        }
    }

    /// The battle's line in the log: textures made, failed and drawn (for every battle with
    /// the full show, even one that drew nothing).
    fn report(fx: &Fx, effects: bool) {
        let s = fx.stats;
        if !effects && !fx.on && s.asked == 0 && s.made == 0 {
            return;
        }
        log!(
            "battle: effect sprites: {} textures made ({} failed: {} no ROM, {} bad frame, {} no \
             room, {} not parsed; {} evicted, {} waited), {} drawn in {} displays (at most {} at \
             once), {} asked for, {} frames unseen, display object failed {} times, deleted {} \
             times",
            s.made,
            s.no_rom + s.frame + s.room + s.parse,
            s.no_rom,
            s.frame,
            s.room,
            s.parse,
            s.evicted,
            s.waited,
            s.drawn,
            s.displays,
            s.peak,
            s.asked,
            s.unseen,
            s.no_object,
            s.deleted,
        );
    }

    pub fn end(area_gone: bool, effects: bool) {
        let mut fx = lock();
        report(&fx, effects);
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
        fx.stats = Stats::default();
        fx.pending = false;
        fx.solid = 0;
        fx.on = false;
        fx.free = None;
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
                fx.stats.deleted += 1;
            }
        });
    }

    /// Draws the scene's sprites that are see-through (`blended`) or not; how many.
    fn draw_sprites(fx: &mut Fx, blended: bool) -> usize {
        let draws: Vec<_> = fx
            .scene
            .draws
            .iter()
            .filter(|d| d.key.blended == blended)
            .copied()
            .collect();
        let top: usize = read(MATRIX_TOP);
        if draws.is_empty() || top == 0 {
            return 0;
        }
        if fx.model.is_none() {
            fx.model = Some(model());
        }
        let cam = camera(&read::<[f32; 12]>(top));
        // The textures made this frame so far (both passes share the limit).
        let mut made = fx.made;
        let mut drawn = 0;
        for d in draws {
            let Some(list) = texture(fx, d.key, &mut made) else {
                continue;
            };
            let Some(object) = fx.model.as_ref().map(|m| m.object.as_ptr() as usize) else {
                break;
            };
            let at = nearer(d.at, cam, NEARER);
            let feet = [at[0], at[1] - d.size / 2.0, at[2]];
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
            drawn += 1;
        }
        fx.made = made;
        fx.stats.drawn += drawn as u32;
        drawn
    }

    /// The display object's display: the solid sprites, then the flash.
    extern "C" fn display(_object: usize) {
        crate::watch::mark("battle fx");
        let _ = std::panic::catch_unwind(|| {
            let mut fx = lock();
            fx.frame = fx.frame.wrapping_add(1);
            fx.free = None;
            fx.made = 0;
            fx.stats.displays += 1;
            fx.pending = false;
            let now = scene_id();
            fx.textures.retain(|t| t.scene == now);
            let drawn = draw_sprites(&mut fx, false);
            fx.solid = drawn;
            fx.stats.peak = fx.stats.peak.max(drawn);
            let (flash, colour) = (fx.scene.flash, fx.scene.flash_colour);
            // The flash only lasts while the battle runs (not through a pause).
            if flash > 0.0 && !crate::battle::arena::paused() {
                let alpha = (flash.clamp(0.0, 1.0) * 255.0) as u32;
                let [r, g, b] = [colour >> 16 & 0xFF, colour >> 8 & 0xFF, colour & 0xFF];
                let colour = alpha.min(0xFE) << 24 | b << 16 | g << 8 | r;
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

    /// The later display: the see-through sprites, over the Chao.
    extern "C" fn display_later(_object: usize) {
        crate::watch::mark("battle fx later");
        let _ = std::panic::catch_unwind(|| {
            let mut fx = lock();
            let drawn = draw_sprites(&mut fx, true);
            let all = fx.solid + drawn;
            fx.stats.peak = fx.stats.peak.max(all);
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
            blended: false,
        }
    }

    #[test]
    fn a_plan_becomes_a_look_and_moves_without_one_borrow() {
        let plan = Plan {
            movement: Movement::Beam,
            main: Some(Source::Template(7)),
            impact: Some(8),
            count: 9,
            gap: 2,
            ..Plan::default()
        };
        let read = |t: usize| Some(template(10_000 + t as u16, 0));
        let look = look_of(&plan, read, |_| None, None);
        assert_eq!(look.main, Some(template(10_007, 0)));
        assert_eq!(look.impact, Some(template(10_008, 0)));
        assert_eq!(
            (look.movement, look.count, look.gap),
            (Movement::Beam, 9, 2)
        );
        // The plain hit only: the hit's sprite, once a hit.
        let hit = template(10_135, 0);
        let only = Plan {
            movement: Movement::HitOnly,
            hits: 2,
            shake: true,
            ..Plan::default()
        };
        let look = look_of(&only, read, |_| None, Some(&hit));
        assert_eq!((look.main.as_ref(), look.count), (Some(&hit), 2));
        // A task's sheet.
        let task = Plan {
            movement: Movement::Rain,
            main: Some(Source::Sheet(10_050)),
            ..Plan::default()
        };
        let look = look_of(&task, read, |tag| Some(template(tag, 4)), None);
        assert_eq!(look.main, Some(template(10_050, 4)));
        // Nothing of its own: one of its type's, by its id; its shake stays.
        let a = MoveLook::of(Movement::Arc, template(10_001, 0));
        let b = MoveLook::of(Movement::Rise, template(10_002, 0));
        let own = [&a, &b];
        let mut none = MoveLook {
            shake: true,
            ..MoveLook::default()
        };
        lend(&mut none, 3, &own);
        assert_eq!(none.main, b.main);
        assert!(none.borrowed && none.shake);
        let mut other = MoveLook::default();
        lend(&mut other, 4, &own);
        assert_eq!(other.main, a.main);
        // A look of its own stays.
        let mut kept = a.clone();
        lend(&mut kept, 3, &own);
        assert_eq!(kept, a);
    }

    #[test]
    fn sprites_draw_nearer_the_camera() {
        let at = nearer([0.0, 1.0, 0.0], [0.0, 1.0, 10.0], NEARER);
        assert_eq!(at, [0.0, 1.0, NEARER]);
        // Never past the camera, nor anywhere for a camera on top of it.
        assert_eq!(nearer([0.0; 3], [0.0, 0.0, 1.0], NEARER), [0.0, 0.0, 0.5]);
        assert_eq!(nearer([1.0; 3], [1.0; 3], NEARER), [1.0; 3]);
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
        // Every move has something to show (its own or its type's, or only a shudder for a
        // move with no sprites and no type-mates), and few borrow.
        let mut borrowed = 0;
        let mut shudders = 0;
        let mut blended = 0;
        let mut looks = std::collections::HashSet::new();
        for id in 1..MOVE_COUNT {
            let look = book.for_move(id);
            assert!(!look.is_empty() || look.shake || look.big, "move {id}");
            shudders += usize::from(look.is_empty());
            blended += usize::from(look.main.as_ref().is_some_and(|t| t.blended));
            borrowed += usize::from(look.borrowed);
            for t in look.main.iter().chain(&look.impact) {
                let sheet = rom.anim_sheet(t.tag).expect("sheet");
                assert!(sheet.frame(t.width, t.height, t.frames[0]).is_some());
            }
            looks.insert((look.movement, look.main.map(|t| t.tag)));
        }
        assert!(borrowed < 50, "{borrowed}");
        assert!(shudders < 5, "{shudders}");
        assert!(blended > 20, "{blended}");
        assert!(looks.len() > 200, "{}", looks.len());
        // Sheets only tasks draw from are cut the way the ROM's own templates for them are.
        let tags = HashSet::from([10_115, 10_211, 10_253]);
        let found = rom.sheet_templates(&tags);
        let size = |tag| usual(&found[&tag]).map(|t| (t.width, t.height));
        assert_eq!(size(10_115), Some((16, 32)));
        assert_eq!(size(10_211), Some((8, 8)));
        assert_eq!(size(10_253), Some((16, 32)));
        let freeze = book.status(status::FREEZE).expect("ice");
        assert_eq!((freeze.width, freeze.height), (64, 64));
        let drops = book.for_move(240).main.expect("drops");
        assert_eq!((drops.width, drops.height), (16, 32));
        println!("{}", book.summary());
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
