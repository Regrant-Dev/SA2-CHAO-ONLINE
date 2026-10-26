//! Puppets: draw-only copies of other players' characters.
//!
//! One game object (list 1, like the player) whose display callback draws every puppet with
//! the same calls Sonic's own display uses (re/characters.md): push matrix, translate,
//! rotate Z/X/Y, `DrawMotion(model, frame)` with the motion in ECX, pop. A puppet of your own
//! character uses your loaded model/textures; other characters are loaded once per stage with
//! the game's own loaders and released when the stage ends. Name tags are drawn here too.

use std::collections::BTreeSet;
use std::ptr;
use std::sync::Mutex;

use crate::game;
use crate::log;

const ROTATE_X: usize = 0x0042_74E0;
const ROTATE_Y: usize = 0x0042_75B0;
const ROTATE_Z: usize = 0x0042_7680;

/// Matrix stack: current pointer, upper limit pointer, base pointer; 0x30-byte entries.
const MATRIX_TOP: usize = 0x01A5_57FC;
const MATRIX_LIMIT: usize = 0x0267_0588;
const MATRIX_BASE: usize = 0x0267_053C;
const MATRIX_SIZE: usize = 0x30;

/// `{index, NJS_OBJECT*}` x 532 at 0x1DE9620; we index the pointer field.
const MODEL_TABLE: usize = 0x01DE_9624;
const MODEL_COUNT: usize = 532;
use game::{MOTION_COUNT, MOTION_TABLE};
const TABLE_STRIDE: usize = 8;
/// `AnimationInfo` entries: +0 AnimNum, +2 ModelNum.
const ANIM_INFO_SIZE: usize = 0x10;
/// Where each character keeps its texture list in CharObj2 (found by scanning the loaders and
/// confirming their display code; re/characters.md). `None` = not drawable yet: Tails and
/// Eggman on foot draw their models differently; mechs/walkers use several models.
fn texlist_slot(character: u8) -> Option<usize> {
    match character {
        // Sonic, Shadow, Amy, Metal Sonic (Sonic's display 0x720090).
        0 | 1 | 8 | 9 => Some(0x394),
        // Knuckles, Rouge, Tikal, Chaos (Knuckles' display 0x72EF20).
        4 | 5 | 12 | 13 => Some(0x3F0),
        // Tails and Eggman on foot (their loaders 0x74CFF7 / 0x73C312).
        2 => Some(0x3B0),
        3 => Some(0x360),
        // Mechs / walkers: the body's texlist (walker display 0x7445FF); +0x444 is the shared
        // WALKTEX for extra parts, which puppets don't draw.
        6 | 7 | 10 | 11 => Some(0x448),
        _ => None,
    }
}
/// A character's asset files (the game's own filename strings) and animation table, from its
/// loader (tools/charassets.py; re/characters.md).
struct CharacterFiles {
    texture: usize,
    models: usize,
    motions: usize,
    anim_info: usize,
}

fn character_files(character: u8) -> Option<CharacterFiles> {
    let (texture, models, motions, anim_info) = match character {
        0 => (0x0090_095C, 0x0090_0968, 0x0090_0994, 0x00A1_7C18), // Sonic
        1 => (0x0090_09C4, 0x0090_09D0, 0x0090_09FC, 0x00A1_88C8), // Shadow
        8 => (0x0090_09A4, 0x0090_09AC, 0x0090_09B8, 0x00A1_9858), // Amy
        9 => (0x0090_0A0C, 0x0090_0A1C, 0x0090_0A30, 0x00A1_A7E8), // Metal Sonic
        4 => (0x0090_0B10, 0x0090_0B1C, 0x0090_0B8C, 0x00A1_3320), // Knuckles
        5 => (0x0090_0B9C, 0x0090_0BA8, 0x0090_0C18, 0x00A1_50F0), // Rouge
        12 => (0x0090_0C28, 0x0090_0C34, 0x0090_0C44, 0x00A1_40B0), // Tikal
        13 => (0x0090_0C54, 0x0090_0C60, 0x0090_0C7C, 0x00A1_5E80), // Chaos
        2 => (0x0090_0F30, 0x0090_0F3C, 0x0090_0F4C, 0x00A0_D608), // Tails (on foot)
        3 => (0x0090_0D78, 0x0090_0D80, 0x0090_0D8C, 0x00A1_2750), // Eggman (on foot)
        // Walker loader 0x740D4A..0x741420.
        6 => (0x0090_0E18, 0x0090_0E24, 0x0090_0E50, 0x00A0_EAA0), // Tails' mech
        7 => (0x0090_0DC8, 0x0090_0DD4, 0x0090_0E08, 0x00A0_E1F0), // Eggman's mech
        10 => (0x0090_0E60, 0x0090_0E6C, 0x0090_0E7C, 0x00A0_F680), // Chao Walker
        11 => (0x0090_0E8C, 0x0090_0E98, 0x0090_0EA8, 0x00A1_02E0), // Dark Chao Walker
        _ => return None,
    };
    Some(CharacterFiles {
        texture,
        models,
        motions,
        anim_info,
    })
}

/// Another character's assets we loaded for its puppets; released at the end of the stage.
struct Loaded {
    character: u8,
    texlist: usize,
    models: usize,
    motions: usize,
}

/// Remote players further apart than this between two updates snap instead of sliding.
const SNAP_DISTANCE: f32 = 200.0;
/// Upper bound for animation indices we accept (from the network or the local player).
const MAX_ANIM: usize = 256;

/// Render state pointer; +0x20 is the texture list used for chunk models.
const RENDER_STATE: usize = 0x0267_0544;
const RENDER_TEXLIST: usize = 0x20;
/// Per-bone draw hook the player's display sets while drawing; we draw without one.
const BONE_CALLBACK: usize = 0x01A5_5834;
/// Render flags the player's display saves/changes/restores around its draw (0x720100):
/// clears 0x200 in FLAGS_A (likely visibility culling — puppets blinked without this) and sets
/// 0x2400 in FLAGS_B; STATE_C is saved and restored.
const RENDER_FLAGS_A: usize = 0x025F_02D4;
const RENDER_FLAGS_B: usize = 0x025F_02D8;
const RENDER_STATE_C: usize = 0x025F_0268;

const OBJ_DISPLAY: usize = 0x14;
const OBJ_DELETE: usize = 0x18;
const PUPPET_LIST: u32 = 1;
const PUPPET_NAME: &[u8] = b"SA2ChaoOnlinePuppets\0";

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Pose {
    pub character: u8,
    pub position: [f32; 3],
    pub rotation: [i32; 3],
    pub anim: u16,
    pub frame: f32,
    /// `game::POSE_*` bits.
    pub flags: u8,
    /// `CharObj2Base::Upgrades` (mech upgrade parts).
    pub upgrades: u32,
    /// EntityData1 action and `protocol::STATUS_*` bits (what a skin's animation follows).
    pub action: u8,
    pub status: u8,
}

/// Spin ball centres (newest last) and their texture (0/1).
type Trail = Vec<([f32; 3], usize)>;

struct Puppets {
    /// The game object, or 0. Cleared by our delete callback when the game frees it.
    object: usize,
    /// Remote players' poses and names.
    /// Pose, name, and the skin they wear ("" for none).
    remotes: Vec<(Pose, String, String)>,
    /// Other characters' assets loaded this stage (and characters that failed to load).
    loaded: Vec<Loaded>,
    failed: Vec<u8>,
    /// (character, anim, reason) skips already logged.
    skipped: BTreeSet<(u8, u16, &'static str)>,
    /// Recent spin ball centres per player (newest last) and their texture (0/1).
    trails: Vec<(String, Trail)>,
    /// We asked the game to load Sonic's effect textures this stage.
    effects_requested: bool,
}

static PUPPETS: Mutex<Puppets> = Mutex::new(Puppets {
    object: 0,
    remotes: Vec::new(),
    skipped: BTreeSet::new(),
    loaded: Vec::new(),
    failed: Vec::new(),
    trails: Vec::new(),
    effects_requested: false,
});

/// Blends two consecutive network poses (`t` = 0 → `a`, 1 → `b`).
pub fn blend(a: &Pose, b: &Pose, t: f32) -> Pose {
    let distance = a
        .position
        .iter()
        .zip(b.position)
        .map(|(p, q)| (q - p) * (q - p))
        .sum::<f32>()
        .sqrt();
    // NaN distance (bad data) also snaps.
    if a.character != b.character || distance.is_nan() || distance > SNAP_DISTANCE {
        return *b;
    }
    let lerp = |p: f32, q: f32| p + (q - p) * t;
    // Angles wrap at 0x10000: turn the short way round.
    let angle = |p: i32, q: i32| {
        let delta = (q.wrapping_sub(p) as i16) as f32;
        p.wrapping_add((delta * t).round() as i32)
    };
    let (anim, frame) = if a.anim == b.anim && a.flags == b.flags && b.frame >= a.frame {
        (b.anim, lerp(a.frame, b.frame))
    } else {
        (b.anim, b.frame)
    };
    Pose {
        character: b.character,
        position: [0, 1, 2].map(|i| lerp(a.position[i], b.position[i])),
        rotation: [0, 1, 2].map(|i| angle(a.rotation[i], b.rotation[i])),
        anim,
        frame,
        flags: b.flags,
        upgrades: b.upgrades,
        action: b.action,
        status: b.status,
    }
}

/// Poses and names of the remote players to draw this frame (already filtered to our stage).
static NAME_TAGS: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(true);

/// Shows or hides the names above other players (F9 menu).
pub fn set_name_tags(on: bool) {
    NAME_TAGS.store(on, std::sync::atomic::Ordering::Relaxed);
}

pub fn set_remotes(remotes: Vec<(Pose, String, String)>) {
    PUPPETS.lock().unwrap_or_else(|e| e.into_inner()).remotes = remotes;
}

/// Call every frame: creates the puppet object once a player exists in a stage.
pub fn on_frame() {
    let mut p = PUPPETS.lock().unwrap_or_else(|e| e.into_inner());
    // Load other characters that remote players use here (once per stage; a short hitch).
    if p.object != 0
        && let Some(me) = game::local_player()
    {
        let wanted: Vec<u8> = p.remotes.iter().map(|(pose, ..)| pose.character).collect();
        for character in wanted {
            if character == me.character
                || p.loaded.iter().any(|l| l.character == character)
                || p.failed.contains(&character)
            {
                continue;
            }
            let Some(files) = character_files(character) else {
                continue;
            };
            // SAFETY: game thread; the game's own loaders with its own filename strings.
            let (texlist, models, motions) = unsafe {
                (
                    load_texlist(files.texture),
                    load_models(files.models),
                    load_motions(files.motions),
                )
            };
            if texlist == 0 || models == 0 || motions == 0 {
                log!(
                    "puppets: couldn't load character {character} ({texlist:#x} {models:#x} {motions:#x})"
                );
                // SAFETY: release whatever did load.
                unsafe {
                    if motions != 0 {
                        free_motions(motions);
                    }
                    if models != 0 {
                        free_models(models);
                    }
                    if texlist != 0 {
                        free_texlist(texlist);
                    }
                }
                p.failed.push(character);
                continue;
            }
            // The game's model/motion tables are shared by index, and another character here
            // (e.g. a mech) may already hold this one's slots: puppets therefore always draw
            // from the lists this load returned, never from the shared tables.
            log!("puppets: loaded character {character} for other players");
            p.loaded.push(Loaded {
                character,
                texlist,
                models,
                motions,
            });
        }
    }
    // One object per stage, drawing all puppets.
    if p.object != 0 {
        return;
    }
    if game::current_level() == 0 || game::local_char_obj2().is_null() {
        return;
    }
    // SAFETY: game thread (OnFrame); LoadObject's register convention is handled in load_object.
    let object = unsafe { load_object(PUPPET_LIST, PUPPET_NAME.as_ptr(), 0) };
    if object == 0 {
        log!("puppets: LoadObject failed");
        return;
    }
    // SAFETY: `object` is the 0x50-byte ObjectMaster LoadObject just returned; data pointers
    // (+0x34..+0x40) stay null because FreeObject frees whatever is stored there.
    unsafe {
        ((object + OBJ_DISPLAY) as *mut usize).write(display as *const () as usize);
        ((object + OBJ_DELETE) as *mut usize).write(delete as *const () as usize);
    }
    p.object = object;
    log!("puppets: object created at {object:#x}");
}

/// The game's shared player motion list (PLCOMMTN.PRS, loaded by 0x459469 and kept).
const COMMON_MOTIONS: usize = 0x01A5_A240;

/// The model (`{i32 index, NJS_OBJECT*}`, ends at index -1) with this index in a loaded list.
unsafe fn list_model(models: usize, index: usize) -> usize {
    unsafe {
        for i in 0..MODEL_COUNT {
            let entry = models + i * TABLE_STRIDE;
            let at = read::<i32>(entry);
            if at < 0 {
                break;
            }
            if at as usize == index {
                return read::<usize>(entry + 4);
            }
        }
    }
    0
}

/// The motion (`{u16 index, u16 count, NJS_MOTION*}`, ends at index 0xFFFF) with this index in
/// a loaded list.
unsafe fn list_motion(motions: usize, index: usize) -> usize {
    unsafe {
        for i in 0..MOTION_COUNT {
            let entry = motions + i * TABLE_STRIDE;
            let at = read::<u16>(entry);
            if at == 0xFFFF {
                break;
            }
            if usize::from(at) == index {
                return read::<usize>(entry + 4);
            }
        }
    }
    0
}

/// Creates an object in the display list whose `display` callback runs during the display
/// pass every frame and whose `delete` runs when the game frees it (end of the area).
pub(crate) fn create_display_object(
    name: &'static [u8],
    display: extern "C" fn(usize),
    delete: extern "C" fn(usize),
) -> usize {
    debug_assert!(name.ends_with(b"\0"));
    // SAFETY: game thread (OnFrame); LoadObject's register convention is handled in load_object.
    let object = unsafe { load_object(PUPPET_LIST, name.as_ptr(), 0) };
    if object != 0 {
        // SAFETY: the ObjectMaster LoadObject just returned; data pointers stay null.
        unsafe {
            ((object + OBJ_DISPLAY) as *mut usize).write(display as usize);
            ((object + OBJ_DELETE) as *mut usize).write(delete as usize);
        }
    }
    object
}

extern "C" fn delete(_object: usize) {
    let _ = std::panic::catch_unwind(|| {
        let mut p = PUPPETS.lock().unwrap_or_else(|e| e.into_inner());
        p.object = 0;
        // The stage is being torn down, like the characters' own assets in their delete
        // callbacks (e.g. Sonic's 0x71E300): release what we loaded for other characters.
        for l in p.loaded.drain(..) {
            // SAFETY: lists/texlist returned by the game's loaders and not released yet; the
            // releases only clear table entries that still point at these lists.
            unsafe {
                free_motions(l.motions);
                free_models(l.models);
                free_texlist(l.texlist);
            }
            log!("puppets: released character {} assets", l.character);
        }
        p.failed.clear();
        p.trails.clear();
        p.effects_requested = false;
        log!("puppets: object deleted by the game");
    });
}

/// Display frames so far (the puppets' blink beat).
static BLINK_FRAMES: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

extern "C" fn display(_object: usize) {
    crate::watch::mark("puppet display");
    let _ = std::panic::catch_unwind(|| {
        // Asked before taking our lock: the online code takes its lock, then ours.
        let connected = crate::online::my_id().is_some();
        let co2 = game::local_char_obj2();
        let Some(me) = game::local_player() else {
            return;
        };
        if co2.is_null() {
            return;
        }
        // What to draw, read under our lock; the drawing (and the other modules it asks:
        // garden, room, skins, kart) happens without it.
        let poses: Vec<(Pose, String, String, Option<Source>)> = {
            let p = PUPPETS.lock().unwrap_or_else(|e| e.into_inner());
            p.remotes
                .iter()
                .map(|(pose, name, skin)| {
                    let source = if pose.character == me.character {
                        texlist_slot(pose.character).map(|slot| Source {
                            // SAFETY: the local player's CharObj2 (checked non-null above).
                            anim_info: unsafe { read::<usize>(co2 as usize + game::CO2_ANIM_INFO) },
                            texlist: unsafe { read::<usize>(co2 as usize + slot) },
                            lists: None,
                        })
                    } else {
                        p.loaded
                            .iter()
                            .find(|l| l.character == pose.character)
                            .and_then(|l| {
                                character_files(l.character).map(|f| Source {
                                    anim_info: f.anim_info,
                                    texlist: l.texlist,
                                    lists: Some((l.models, l.motions)),
                                })
                            })
                    };
                    (*pose, name.clone(), skin.clone(), source)
                })
                .collect()
        };
        // SAFETY: display callback on the game thread; the matrix stack top is the camera view.
        let tags = if NAME_TAGS.load(std::sync::atomic::Ordering::Relaxed) {
            unsafe { TagProjector::new(me.position) }
        } else {
            None
        };
        // Visiting Chao: "Player1 1" (their visitor 1), ours "You 1", plus a name if it has one.
        if let Some(tags) = &tags {
            let ours = if connected {
                crate::garden::my_visitors()
            } else {
                Vec::new()
            };
            let ours = ours
                .into_iter()
                .map(|(at, index, name)| (at, None, index, name));
            let theirs = crate::garden::visitors()
                .into_iter()
                .map(|(at, owner, index, name)| (at, Some(owner), index, name));
            let all: Vec<_> = theirs.chain(ours).collect();
            let lifts = tag_lifts(&all.iter().map(|t| t.0).collect::<Vec<_>>());
            for ((at, owner, index, name), lift) in all.into_iter().zip(lifts) {
                // One the server holds for a player who isn't here: "Player1's Chao (asleep)".
                let held = owner.and_then(|o| crate::held::label(o, index));
                let who = owner.map_or_else(|| "You".to_string(), crate::room::name_of);
                // Locked against us (nothing we do to it reaches its owner's save): a padlock
                // after the name.
                let locked = held.as_ref().is_some_and(|h| h.1)
                    || owner.is_some_and(|o| crate::garden::locked(o, index));
                let label = match (held, name) {
                    (Some((held, _)), Some(name)) => format!("{held}: {name}"),
                    (Some((held, _)), None) => held,
                    (None, Some(name)) => format!("{who} {}: {name}", index + 1),
                    (None, None) => format!("{who} {}", index + 1),
                };
                tags.draw_tag(at, CHAO_TAG_HEIGHT + lift, &label, CHAO_TAG_SCALE, locked);
            }
        }
        // Puppets blink while their player was just hurt, as characters do: every other two
        // frames (their own timer stays in their game; this keeps the same beat).
        let blink =
            game::blinks_out(BLINK_FRAMES.fetch_add(1, std::sync::atomic::Ordering::Relaxed));
        let in_kart_race = crate::kart::in_kart_stage();
        for (pose, name, skin, source) in poses {
            if let Some(tags) = &tags {
                tags.draw(pose.position, &name);
            }
            if blink && pose.status & crate::net::protocol::STATUS_INVULNERABLE != 0 {
                continue;
            }
            // Wearing a skin we have: that instead of the character (skins.rs).
            if !skin.is_empty() && !in_kart_race && crate::skins::draw_other(&name, &skin, &pose) {
                continue;
            }
            let result = match source {
                // In a Kart Race the others are drawn as karts (kart.rs); keep their name tags.
                _ if in_kart_race => Ok(()),
                // Mechs and walkers: their own walker display draws them with their upgrade parts
                // when every model it reads is loaded; otherwise just the body
                // (re/characters.md "Drawing other players' walkers").
                // SAFETY: display callback on the game thread.
                Some(source) if is_walker(pose.character) => unsafe { draw_walker(&source, &pose) },
                // SAFETY: display callback runs on the game thread inside the display pass.
                Some(source) => unsafe { draw(&source, &pose) },
                None if character_files(pose.character).is_some() => {
                    Err("character assets not loaded")
                }
                None => Err("character not drawable yet"),
            };
            // Our own bookkeeping again: the ball trail and what wasn't drawn.
            let mut p = PUPPETS.lock().unwrap_or_else(|e| e.into_inner());
            // SAFETY: display callback on the game thread.
            unsafe { spin_ball(&mut p, &name, &pose) };
            // Log each distinct reason a pose wasn't drawn, once.
            if let Err(reason) = result
                && p.skipped.insert((pose.character, pose.anim, reason))
            {
                log!(
                    "puppets: not drawing character {} anim {} (flags {:#x}): {reason}",
                    pose.character,
                    pose.anim,
                    pose.flags
                );
            }
        }
    });
}

// ---- Spin ball (Sonic's "JumpAura", re/spin-ball.md) ----
//
// While Sonic jumps, his main spawns a small effect every frame: the ball model drawn at the
// body centre with a one-texture list (s_ball_a or s_ball_b at random), scale 1.2, shrinking
// by 0.3 a frame and gone after 3 frames. So three balls are always visible: the trail.

/// Height of the ball centre above the player's position (`co2+0x140`, 5.4 for Sonic's family).
const BALL_HEIGHT: f32 = 5.4;
const BALL_SCALES: [f32; 3] = [1.2, 0.9, 0.6];
/// Sonic's / Shadow's ball model slots (model table index 19 / 84) and effect texlists.
const SONIC_BALL_MODEL: usize = 19;
const SONIC_EFFECT_TEXLIST: usize = 0x00A0_8B94;
const SHADOW_BALL_MODEL: usize = 84;
const SHADOW_EFFECT_TEXLIST: usize = 0x00A0_8D94;
/// `NJS_TEXNAME` size (filename, attr, texaddr).
const TEXNAME_SIZE: usize = 12;
/// Fog on/off byte the effect's display clears while it draws (then 0x42A870 applies it).
const FOG_ENABLED: usize = 0x025E_FFCC;

/// One-entry texture list handed to the renderer for the ball.
static mut BALL_TEXLIST: [usize; 2] = [0, 1];

/// Updates `name`'s ball trail and draws it (after the puppet itself).
unsafe fn spin_ball(p: &mut Puppets, name: &str, pose: &Pose) {
    let index = match p.trails.iter().position(|(n, _)| n == name) {
        Some(i) => i,
        None => {
            p.trails.push((name.to_string(), Vec::new()));
            p.trails.len() - 1
        }
    };
    let trail = &mut p.trails[index].1;
    if pose.flags & game::POSE_BALL == 0 {
        trail.clear();
        return;
    }
    // Ball centre: position + RotZ(rot.z) * RotX(rot.x) * (0, h, 0).
    let angle = |bams: i32| bams as f32 * std::f32::consts::TAU / 65536.0;
    let (rx, rz) = (angle(pose.rotation[0]), angle(pose.rotation[2]));
    let (y, z) = (BALL_HEIGHT * rx.cos(), BALL_HEIGHT * rx.sin());
    let centre = [
        pose.position[0] - y * rz.sin(),
        pose.position[1] + y * rz.cos(),
        pose.position[2] + z,
    ];
    let texture = (trail.len() + centre[0].to_bits() as usize) & 1;
    trail.push((centre, texture));
    if trail.len() > BALL_SCALES.len() {
        trail.remove(0);
    }
    let trail = trail.clone();

    let (model_slot, texlist) = match pose.character {
        1 | 9 => (SHADOW_BALL_MODEL, SHADOW_EFFECT_TEXLIST),
        _ => (SONIC_BALL_MODEL, SONIC_EFFECT_TEXLIST),
    };
    unsafe {
        // From Sonic's / Shadow's own list when we loaded them for puppets (the shared slot
        // may belong to another character here), else the shared table.
        let family = if matches!(pose.character, 1 | 9) {
            1
        } else {
            0
        };
        let model = match p.loaded.iter().find(|l| l.character == family) {
            Some(l) => list_model(l.models, model_slot),
            None => read::<usize>(MODEL_TABLE + model_slot * TABLE_STRIDE),
        };
        let textures = read::<usize>(texlist);
        let loaded = textures != 0 && read::<usize>(textures + 8) != 0;
        if !loaded && !p.effects_requested {
            // Not playing as Sonic: load his effect textures once (freed with the stage).
            p.effects_requested = true;
            load_sonic_effects();
            return;
        }
        if model == 0 || !loaded {
            return;
        }
        let top = read::<usize>(MATRIX_TOP);
        let limit = read::<usize>(MATRIX_LIMIT);
        let base_ptr = read::<usize>(MATRIX_BASE);
        let render = read::<usize>(RENDER_STATE);
        if render == 0 || top == 0 || top < base_ptr || top + MATRIX_SIZE > limit {
            return;
        }
        let old_texlist = read::<usize>(render + RENDER_TEXLIST);
        let old_fog = read::<u8>(FOG_ENABLED);
        write(FOG_ENABLED, 0u8);
        refresh_fog();
        // Newest ball is the biggest.
        for (age, (centre, texture)) in trail.iter().rev().enumerate() {
            let list = std::ptr::addr_of_mut!(BALL_TEXLIST);
            (*list)[0] = textures + texture * TEXNAME_SIZE;
            write(render + RENDER_TEXLIST, list as usize);
            let next = top + MATRIX_SIZE;
            ptr::copy_nonoverlapping(top as *const u8, next as *mut u8, MATRIX_SIZE);
            write(MATRIX_TOP, next);
            translate(next, *centre);
            scale(BALL_SCALES[age]);
            draw_object(model);
            write(MATRIX_TOP, top);
        }
        write(render + RENDER_TEXLIST, old_texlist);
        write(FOG_ENABLED, old_fog);
        refresh_fog();
    }
}

/// Draws one pose with the local player's model data. Skips anything out of range.
///
/// # Safety
/// Game thread, inside the display pass; `co2` is the local player's CharObj2.
/// Where a puppet's animation table and textures come from.
struct Source {
    anim_info: usize,
    texlist: usize,
    /// (model list, motion list) of a character we loaded; `None` = the local player's own
    /// character, whose models and motions are in the shared tables.
    lists: Option<(usize, usize)>,
}

impl Source {
    unsafe fn model(&self, index: usize) -> usize {
        unsafe {
            match self.lists {
                Some((models, _)) => list_model(models, index),
                None => read::<usize>(MODEL_TABLE + index * TABLE_STRIDE),
            }
        }
    }

    unsafe fn motion(&self, index: usize) -> usize {
        unsafe {
            match self.lists {
                // Their own file, then the shared player motions (PLCOMMTN: walking, standing...,
                // loaded once by the game for every character, list at 0x1A5A240).
                Some((_, motions)) => match list_motion(motions, index) {
                    0 => match read::<usize>(COMMON_MOTIONS) {
                        0 => 0,
                        common => list_motion(common, index),
                    },
                    motion => motion,
                },
                None => read::<usize>(MOTION_TABLE + index * TABLE_STRIDE),
            }
        }
    }
}

// ---- Mechs and walkers (re/characters.md "Drawing other players' walkers") ----
//
// Each walker type has its own display (Eggman's 0x7444F0, Tails' 0x747DD0, ...): it draws the
// body, then the upgrade parts from bone matrices its per-bone callback records, reading the
// part models straight from the model table. A puppet gets a fresh CharObj2 and EntityData1
// with only what the display reads (its type, upgrades, animation, tables), drawn by its own
// type's display once every model slot that display reads unchecked is loaded; otherwise just
// the body. The globals the display and its callback write are saved and restored, so our own
// character is untouched. (Drawing a Tails walker with Eggman's display, whose models a Tails
// stage never loads, crashed at 0x744ACA and 0x42EB40.)

/// Walker CharObj2 size (the walker loader allocates 0x458 bytes, 0x740CD9).
const WALKER_CO2_SIZE: usize = 0x458;
/// EntityData1: angles, position, scale.
const ENTITY_SIZE: usize = 0x40;
const ENTITY_ROTATION: usize = 0x08;
const ENTITY_POSITION: usize = 0x14;
const ENTITY_SCALE: usize = 0x20;
const OBJ_DATA1: usize = 0x34;
const OBJ_DATA2: usize = 0x40;
/// CharObj2: player number, character (+1 and +3), animation block (mode, requested animation),
/// body texlist.
const CO2_PLAYER: usize = 0x00;
const CO2_CHARACTER: usize = 0x01;
const CO2_MOTION_MODE: usize = 0x174;
const CO2_NEXT_ANIM: usize = 0x176;
const CO2_TEXLIST: usize = 0x448;
/// Globals the walker displays and their bone callbacks write: (address, length).
const WALKER_GLOBALS: [(usize, usize); 6] = [
    (0x01A5_1D00, 0x450), // bone matrices and the drawn CharObj2 (Tails' callback to 0x1A52120)
    (0x01A5_57FC, 0x50),  // matrix stack top, bone callback, draw flags (0x1A557FC..0x1A5584C)
    (0x01DE_4660, 0x08),  // per-player light index
    (0x01DE_B6A0, 0x10),  // render flags it saves
    (0x025E_FFCC, 0x14),  // fog switch and colour
    (0x01D1_9CC0, 0x70),  // Chao walkers' extra matrix (Dark at +0, Chao at +0x40)
];

#[repr(C, align(16))]
struct WalkerCopy {
    co2: [u8; WALKER_CO2_SIZE + 8],
    entity: [u8; ENTITY_SIZE],
    object: [u8; 0x50],
}

static mut WALKER: WalkerCopy = WalkerCopy {
    co2: [0; WALKER_CO2_SIZE + 8],
    entity: [0; ENTITY_SIZE],
    object: [0; 0x50],
};

fn is_walker(character: u8) -> bool {
    matches!(character, 6 | 7 | 10 | 11)
}

/// The display of a walker type: Tails', Eggman's, the Chao Walker's and the Dark one's.
fn walker_display_of(character: u8) -> Option<usize> {
    match character {
        6 => Some(0x0074_7DD0),
        7 => Some(0x0074_44F0),
        10 => Some(0x0074_6290),
        11 => Some(0x0074_70D0),
        _ => None,
    }
}

/// CharObj2 +1 and +3 for a walker type: the Chao walkers are Tails'/Eggman's frames (+1)
/// with their own id (+3: 0xD / 0xE).
fn walker_ids(character: u8) -> (u8, u8) {
    match character {
        10 => (6, 0x0D),
        11 => (7, 0x0E),
        c => (c, c),
    }
}

/// Motion slots a walker display reads without a null check (only the Chao walkers' throw).
fn walker_motion_slots(character: u8, anim: u16) -> Vec<usize> {
    match (character, anim) {
        (10, 0xB4) => vec![193],
        (11, 0xB4) => vec![198],
        _ => Vec::new(),
    }
}

/// Every model slot `character`'s display reads without a null check, for these upgrades and
/// this animation (re/characters.md). The animation-dependent slots (thrown parts, boost) are
/// left out by keeping those CharObj2 counters at 0.
fn walker_slots(character: u8, upgrades: u32, anim: u16) -> Vec<usize> {
    let pick = |bit: u32, set: usize, clear: usize| if upgrades & bit != 0 { set } else { clear };
    let mut slots = Vec::new();
    match character {
        // Eggman's walker.
        7 => {
            slots.push(pick(0x40_0000, 260, 258));
            if upgrades & 0x20_0000 != 0 {
                slots.extend([262, 263]);
            } else {
                slots.extend([265, 266]);
            }
            if anim == 0x80 {
                slots.push(268);
            }
            if upgrades & 0x10_0000 != 0 {
                slots.extend([274, 276]);
            }
            if upgrades & 0x80_0000 != 0 {
                slots.extend([278, 291]);
            }
        }
        // Tails' walker.
        6 => {
            slots.push(pick(0x100, 306, 304));
            slots.push(pick(0x80, 309, 312));
            if anim == 0xB4 {
                slots.push(313);
            }
            if upgrades & 0x40 != 0 {
                slots.extend([319, 321]);
            }
        }
        // Chao Walker: no upgrade parts; its bone callback reads 442 unchecked.
        10 => {
            slots.push(442);
            if anim == 0xB4 {
                slots.push(451);
            }
        }
        // Dark Chao Walker.
        11 => {
            slots.push(465);
            if anim == 0xB4 {
                slots.push(474);
            }
        }
        _ => {}
    }
    slots
}

/// Draws a walker `pose` with its own type's display, or just its body when that display can't
/// be used safely.
///
/// # Safety
/// Display pass on the game thread.
unsafe fn draw_walker(source: &Source, pose: &Pose) -> Result<(), &'static str> {
    unsafe {
        let (_, _, frame) = resolve(source, pose)?;
        let Some(display) = walker_display_of(pose.character) else {
            return draw(source, pose);
        };
        let loaded = |slot: usize| read::<usize>(MODEL_TABLE + slot * TABLE_STRIDE) != 0;
        let motion_loaded = |slot: usize| read::<usize>(MOTION_TABLE + slot * TABLE_STRIDE) != 0;
        if !walker_slots(pose.character, pose.upgrades, pose.anim)
            .into_iter()
            .all(loaded)
            || !walker_motion_slots(pose.character, pose.anim)
                .into_iter()
                .all(motion_loaded)
        {
            return draw(source, pose);
        }
        let walker = &mut *std::ptr::addr_of_mut!(WALKER);
        walker.co2 = [0; WALKER_CO2_SIZE + 8];
        walker.entity = [0; ENTITY_SIZE];
        walker.object = [0; 0x50];
        let c = walker.co2.as_mut_ptr() as usize;
        write(c + CO2_PLAYER, 0u8);
        let (frames, id) = walker_ids(pose.character);
        write(c + CO2_CHARACTER, frames);
        write(c + game::CO2_CHAR_ID, id);
        write(c + game::CO2_UPGRADES, pose.upgrades);
        write(c + CO2_MOTION_MODE, 0u16);
        write(c + CO2_NEXT_ANIM, pose.anim);
        write(c + game::CO2_ANIM, pose.anim);
        write(c + game::CO2_FRAME, frame);
        write(c + game::CO2_ANIM_INFO, source.anim_info);
        write(c + CO2_TEXLIST, source.texlist);
        let e = walker.entity.as_mut_ptr() as usize;
        write(e + ENTITY_ROTATION, pose.rotation);
        write(e + ENTITY_POSITION, pose.position);
        write(e + ENTITY_SCALE, [1.0f32; 3]);
        let object = walker.object.as_mut_ptr() as usize;
        write(object + OBJ_DATA1, e);
        write(object + OBJ_DATA2, c);

        let saved: Vec<Vec<u8>> = WALKER_GLOBALS
            .iter()
            .map(|&(at, len)| std::slice::from_raw_parts(at as *const u8, len).to_vec())
            .collect();
        let render = read::<usize>(RENDER_STATE);
        let old_texlist = if render != 0 {
            read::<usize>(render + RENDER_TEXLIST)
        } else {
            0
        };
        std::mem::transmute::<usize, extern "C" fn(usize)>(display)(object);
        for (&(at, len), bytes) in WALKER_GLOBALS.iter().zip(&saved) {
            ptr::copy_nonoverlapping(bytes.as_ptr(), at as *mut u8, len);
        }
        if render != 0 {
            write(render + RENDER_TEXLIST, old_texlist);
        }
        refresh_fog();
    }
    Ok(())
}

/// A pose's model, motion and the frame to show (checked against the animation tables).
unsafe fn resolve(source: &Source, pose: &Pose) -> Result<(usize, usize, f32), &'static str> {
    let blending = pose.flags & game::POSE_BLENDING != 0;
    unsafe {
        let anim = pose.anim as usize;
        let anim_info = source.anim_info;
        if anim >= MAX_ANIM || anim_info == 0 {
            return Err("animation index out of range");
        }
        let entry = anim_info + anim * ANIM_INFO_SIZE;
        let base = read::<i16>(entry);
        let model_index = read::<i16>(entry + 2);
        if base < 0 || base as usize >= MAX_ANIM || model_index < 0 {
            return Err("bad animation entry");
        }
        let motion_index = read::<i16>(anim_info + base as usize * ANIM_INFO_SIZE);
        let (model_index, motion_index) = (model_index as usize, motion_index as usize);
        if model_index >= MODEL_COUNT || motion_index >= MOTION_COUNT {
            return Err("model/motion index out of range");
        }
        let model = source.model(model_index);
        let motion = source.motion(motion_index);
        if model == 0 {
            return Err("model not loaded");
        }
        if motion == 0 {
            return Err("motion not loaded");
        }
        let frames = read::<u32>(motion + 4);
        // While blending, `frame` is the blend progress: show the start of the target animation.
        let frame = if blending {
            0.0
        } else if frames > 0 && pose.frame.is_finite() {
            pose.frame.rem_euclid(frames as f32)
        } else {
            0.0
        };
        Ok((model, motion, frame))
    }
}

unsafe fn draw(source: &Source, pose: &Pose) -> Result<(), &'static str> {
    unsafe {
        let (model, motion, frame) = resolve(source, pose)?;

        let top = read::<usize>(MATRIX_TOP);
        let limit = read::<usize>(MATRIX_LIMIT);
        let base_ptr = read::<usize>(MATRIX_BASE);
        let next = top + MATRIX_SIZE;
        if top == 0 || top < base_ptr || next > limit {
            return Err("matrix stack full");
        }
        ptr::copy_nonoverlapping(top as *const u8, next as *mut u8, MATRIX_SIZE);
        write(MATRIX_TOP, next);

        let render = read::<usize>(RENDER_STATE);
        let old_texlist = if render != 0 {
            let old = read::<usize>(render + RENDER_TEXLIST);
            write(render + RENDER_TEXLIST, source.texlist);
            old
        } else {
            0
        };
        let old_bone = read::<usize>(BONE_CALLBACK);
        write(BONE_CALLBACK, 0usize);
        let old_flags_a = read::<u32>(RENDER_FLAGS_A);
        let old_state_c = read::<u32>(RENDER_STATE_C);
        write(RENDER_FLAGS_A, old_flags_a & !0x200);
        write(RENDER_FLAGS_B, read::<u32>(RENDER_FLAGS_B) | 0x2400);

        translate(next, pose.position);
        let [rx, ry, rz] = pose.rotation;
        if rz != 0 {
            rotate(ROTATE_Z, next, rz);
        }
        if rx != 0 {
            rotate(ROTATE_X, next, rx);
        }
        let ry = 0x8000i32.wrapping_sub(ry);
        if ry != 0 {
            rotate(ROTATE_Y, next, ry);
        }
        draw_motion(motion, model, frame);

        write(RENDER_FLAGS_B, read::<u32>(RENDER_FLAGS_B) & !0x2400);
        write(RENDER_FLAGS_A, old_flags_a);
        write(RENDER_STATE_C, old_state_c);
        write(BONE_CALLBACK, old_bone);
        if render != 0 {
            write(render + RENDER_TEXLIST, old_texlist);
        }
        write(MATRIX_TOP, top);
    }
    Ok(())
}

/// Height above a character's position (its middle, not its feet) for the name tag, in world
/// units: just above the head (22 was far too high in game).
const TAG_HEIGHT: f32 = 10.0;
/// Friends' Chao: tags just over their heads, smaller than players'.
const CHAO_TAG_HEIGHT: f32 = 6.0;
const CHAO_TAG_SCALE: f32 = 0.7;
/// Chao closer than this have their tags stacked, each this much over the one before.
const TAG_CROWD: f32 = 4.0;
const TAG_STEP: f32 = 2.0;

/// How much higher each Chao's tag goes (in order) so tags of Chao at about the same spot
/// don't print over each other.
fn tag_lifts(at: &[[f32; 3]]) -> Vec<f32> {
    let near = |a: [f32; 3], b: [f32; 3]| {
        let (x, y, z) = (a[0] - b[0], a[1] - b[1], a[2] - b[2]);
        x * x + y * y + z * z < TAG_CROWD * TAG_CROWD
    };
    let mut lifts: Vec<f32> = Vec::with_capacity(at.len());
    for (i, &here) in at.iter().enumerate() {
        let mut lift = 0.0;
        // Above the highest of the earlier ones it crowds.
        for (j, &there) in at[..i].iter().enumerate() {
            if near(here, there) {
                lift = f32::max(lift, lifts[j] + TAG_STEP);
            }
        }
        lifts.push(lift);
    }
    lifts
}
/// Hide tags beyond this distance (world units).
const TAG_RANGE: f32 = 1500.0;
/// Camera depth up to which tags are drawn at full size (about the usual follow distance).
const TAG_FULL_SIZE_DEPTH: f32 = 60.0;

/// Projects world points with the game's own projection (0x77FB00, the one
/// `EnemyCheckFrameIn` uses), so it follows whatever camera is drawing — including Chao
/// World's separate camera system. Results are in the game's 640x480 space.
pub(crate) struct TagProjector {
    /// View matrix (top of the matrix stack during the display pass).
    view: usize,
    /// +1 or -1: view-space z sign of points in front of the camera, from the local player.
    front: f32,
    me: [f32; 3],
}

/// The game's virtual screen used by its projection.
const VIRTUAL_WIDTH: f32 = 640.0;
const VIRTUAL_HEIGHT: f32 = 480.0;

impl TagProjector {
    /// # Safety
    /// Display pass on the game thread (the matrix stack top is the camera view).
    pub(crate) unsafe fn new(me: game::Vector) -> Option<Self> {
        // SAFETY: static global.
        let view = unsafe { read::<usize>(MATRIX_TOP) };
        if view == 0 {
            return None;
        }
        let me = [me.x, me.y + TAG_HEIGHT * 0.5, me.z];
        // SAFETY: valid matrix; our stack buffers.
        let z = unsafe { calc_point(view, me) }[2];
        Some(Self {
            view,
            front: if z > 0.0 { 1.0 } else { -1.0 },
            me,
        })
    }

    fn draw(&self, feet: [f32; 3], name: &str) {
        self.draw_at(feet, TAG_HEIGHT, name);
    }

    /// Draws `name` centred `height` units above world point `feet`.
    pub(crate) fn draw_at(&self, feet: [f32; 3], height: f32, name: &str) {
        self.draw_scaled(feet, height, name, 1.0);
    }

    /// `draw_at`, with the text `scale` times the usual size.
    pub(crate) fn draw_scaled(&self, feet: [f32; 3], height: f32, name: &str, scale: f32) {
        self.draw_tag(feet, height, name, scale, false);
    }

    /// `draw_scaled`, with a padlock after the name when `locked`.
    pub(crate) fn draw_tag(
        &self,
        feet: [f32; 3],
        height: f32,
        name: &str,
        scale: f32,
        locked: bool,
    ) {
        let head = [feet[0], feet[1] + height, feet[2]];
        let distance = (0..3)
            .map(|i| (head[i] - self.me[i]).powi(2))
            .sum::<f32>()
            .sqrt();
        if distance > TAG_RANGE {
            return;
        }
        // SAFETY: as in `new`; the view matrix is unchanged during this display call.
        let (z, screen) = unsafe { (calc_point(self.view, head)[2], project_screen(head)) };
        if z * self.front <= 1.0 {
            return; // behind the camera
        }
        // Smaller with distance from the camera: full size up close, down to half far away.
        let depth = (z * self.front).max(1.0);
        let shrink = (TAG_FULL_SIZE_DEPTH / depth).clamp(0.5, 1.0);
        let [x, y] = screen;
        if !(0.0..=VIRTUAL_WIDTH).contains(&x) || !(0.0..=VIRTUAL_HEIGHT).contains(&y) {
            return;
        }
        // The 640x480 HUD space is the largest 4:3 area that fits the screen, centred.
        let (sx, sy) = crate::hud::from_virtual(x, y);
        crate::hud::name_tag(sx, sy, shrink * scale, name, locked);
    }
}

unsafe fn read<T: Copy>(address: usize) -> T {
    unsafe { (address as *const T).read_unaligned() }
}

unsafe fn write<T>(address: usize, value: T) {
    unsafe { (address as *mut T).write_unaligned(value) }
}

/// Calls into the game with its register conventions. Only meaningful in the 32-bit game.
#[cfg(target_arch = "x86")]
mod ffi {
    use std::arch::asm;

    const LOAD_OBJECT: usize = 0x0046_F680;
    const DRAW_MOTION: usize = 0x0078_0870;
    const TRANSLATE: usize = 0x0042_7470;
    const CALC_POINT: usize = 0x0042_6CC0;
    const PROJECT_SCREEN: usize = 0x0077_FB00;
    const LOAD_TEXLIST: usize = 0x0044_C510;
    const LOAD_MODELS: usize = 0x0045_9590;
    const LOAD_MOTIONS: usize = 0x0045_9740;
    const FREE_MODELS: usize = 0x0045_96D0;
    const FREE_MOTIONS: usize = 0x0045_9890;
    const RELEASE_TEXLIST: usize = 0x0077_F9F0;
    const ALLOCATOR: usize = 0x01D1_9CAC;

    /// `LoadObject`: ESI = list, EDI = main (0 = no-op), name on the stack (caller pops).
    pub unsafe fn load_object(list: u32, name: *const u8, main: usize) -> usize {
        let object: usize;
        unsafe {
            asm!(
                "push esi",
                "push edi",
                "mov esi, ecx",
                "mov edi, edx",
                "push eax",
                "mov eax, {func}",
                "call eax",
                "add esp, 4",
                "pop edi",
                "pop esi",
                func = const LOAD_OBJECT,
                inout("eax") name => object,
                inout("ecx") list => _,
                inout("edx") main => _,
                clobber_abi("C"),
            );
        }
        object
    }

    /// Translate the matrix at `matrix` (EAX) by x, y, z (stack, caller pops).
    pub unsafe fn translate(matrix: usize, position: [f32; 3]) {
        let args = position.map(f32::to_bits);
        unsafe {
            asm!(
                "push dword ptr [{args} + 8]",
                "push dword ptr [{args} + 4]",
                "push dword ptr [{args}]",
                "mov ecx, {func}",
                "call ecx",
                "add esp, 12",
                args = in(reg) args.as_ptr(),
                func = const TRANSLATE,
                inout("eax") matrix => _,
                clobber_abi("C"),
            );
        }
    }

    const SCALE: usize = 0x0042_7750;
    const DRAW_OBJECT: usize = 0x0042_E730;
    const REFRESH_FOG: usize = 0x0042_A870;
    const LOAD_SONIC_EFFECTS: usize = 0x0075_5DA0;

    /// `njScale(x, y, z)` 0x427750 on the current matrix (cdecl).
    pub unsafe fn scale(s: f32) {
        unsafe { std::mem::transmute::<usize, extern "C" fn(f32, f32, f32)>(SCALE)(s, s, s) }
    }

    /// Draws a model with the current matrix and texture list, 0x42E730 (cdecl).
    pub unsafe fn draw_object(model: usize) {
        unsafe { std::mem::transmute::<usize, extern "C" fn(usize)>(DRAW_OBJECT)(model) }
    }

    /// Applies the fog setting (byte 0x25EFFCC), 0x42A870 (no arguments).
    pub unsafe fn refresh_fog() {
        unsafe { std::mem::transmute::<usize, extern "C" fn()>(REFRESH_FOG)() }
    }

    /// `LoadSonEffTex` 0x755DA0: Sonic's effect textures (no arguments; safe to call twice).
    pub unsafe fn load_sonic_effects() {
        unsafe { std::mem::transmute::<usize, extern "C" fn()>(LOAD_SONIC_EFFECTS)() }
    }

    /// Rotate the matrix at `matrix` (EBX) by `angle` (stack, caller pops).
    pub unsafe fn rotate(func: usize, matrix: usize, angle: i32) {
        unsafe {
            asm!(
                "push ebx",
                "push ecx",
                "mov ebx, edx",
                "call eax",
                "add esp, 4",
                "pop ebx",
                inout("eax") func => _,
                inout("ecx") angle => _,
                inout("edx") matrix => _,
                clobber_abi("C"),
            );
        }
    }

    /// `njCalcPoint` 0x426CC0: EAX = matrix, ECX = input point, EDX = output point,
    /// stack = additive flag (caller pops), as Knuckles' display calls it (0x72EF84).
    pub unsafe fn calc_point(matrix: usize, point: [f32; 3]) -> [f32; 3] {
        let mut out = [0.0f32; 3];
        unsafe {
            asm!(
                "push 0",
                "call {func}",
                "add esp, 4",
                func = in(reg) CALC_POINT,
                inout("eax") matrix => _,
                inout("ecx") point.as_ptr() => _,
                inout("edx") out.as_mut_ptr() => _,
                clobber_abi("C"),
            );
        }
        out
    }

    /// World point -> game screen (640x480) with the current view, 0x77FB00:
    /// ESI = input point, EBX = output [x, y] (as EnemyCheckFrameIn 0x479FA0 calls it).
    pub unsafe fn project_screen(point: [f32; 3]) -> [f32; 2] {
        let mut out = [0.0f32; 2];
        unsafe {
            asm!(
                "push ebx",
                "push esi",
                "mov esi, ecx",
                "mov ebx, edx",
                "call eax",
                "pop esi",
                "pop ebx",
                inout("eax") PROJECT_SCREEN => _,
                inout("ecx") point.as_ptr() => _,
                inout("edx") out.as_mut_ptr() => _,
                clobber_abi("C"),
            );
        }
        out
    }

    /// Texture list loader 0x44C510: ECX = name (without extension); returns the texlist.
    pub unsafe fn load_texlist(name: usize) -> usize {
        let out: usize;
        unsafe {
            asm!("call eax", inout("eax") LOAD_TEXLIST => out, inout("ecx") name => _, clobber_abi("C"));
        }
        out
    }

    /// Model list loader 0x459590: EAX = file name; fills empty model-table slots.
    pub unsafe fn load_models(name: usize) -> usize {
        let out: usize;
        unsafe {
            asm!("mov edx, {f}", "call edx", f = const LOAD_MODELS, inout("eax") name => out, clobber_abi("C"));
        }
        out
    }

    /// Motion list loader 0x459740: EAX = file name; fills empty motion-table slots.
    pub unsafe fn load_motions(name: usize) -> usize {
        let out: usize;
        unsafe {
            asm!("mov edx, {f}", "call edx", f = const LOAD_MOTIONS, inout("eax") name => out, clobber_abi("C"));
        }
        out
    }

    /// Model list release 0x4596D0: ESI = list (clears only table slots it filled, frees it).
    pub unsafe fn free_models(list: usize) {
        unsafe {
            asm!("push esi", "mov esi, ecx", "call eax", "pop esi",
                inout("eax") FREE_MODELS => _, inout("ecx") list => _, clobber_abi("C"));
        }
    }

    /// Motion list release 0x459890: ESI = list (clears only its own table slots, frees it).
    pub unsafe fn free_motions(list: usize) {
        unsafe {
            asm!("push esi", "mov esi, ecx", "call eax", "pop esi",
                inout("eax") FREE_MOTIONS => _, inout("ecx") list => _, clobber_abi("C"));
        }
    }

    /// Texlist release as Sonic's delete callback does it (0x71E33A): 0x77F9F0(texlist), then
    /// free the block through the game allocator (`(*0x1D19CAC)[1]`) with its 0x89ABCDEF tag.
    pub unsafe fn free_texlist(texlist: usize) {
        unsafe {
            let release: extern "C" fn(usize) = std::mem::transmute(RELEASE_TEXLIST);
            release(texlist);
            let block = texlist - 4;
            (block as *mut u32).write(0x89AB_CDEF);
            let allocator = (ALLOCATOR as *const usize).read();
            let free: extern "C" fn(usize, *const u8, i32) =
                std::mem::transmute(((allocator + 4) as *const usize).read());
            free(block, c"sa2-chao-online".as_ptr().cast(), 0);
        }
    }

    /// `DrawMotion`: ECX = NJS_MOTION*, cdecl(NJS_OBJECT* model, float frame).
    pub unsafe fn draw_motion(motion: usize, model: usize, frame: f32) {
        unsafe {
            asm!(
                "push edx",
                "push eax",
                "mov eax, {func}",
                "call eax",
                "add esp, 8",
                func = const DRAW_MOTION,
                inout("eax") model => _,
                inout("ecx") motion => _,
                inout("edx") frame.to_bits() => _,
                clobber_abi("C"),
            );
        }
    }
}

/// Host builds (tests, fakepeer) never run the display callback.
/// Loads a texture archive by name (no extension) with the game's loader; 0 if not found.
pub fn load_texlist_named(name: usize) -> usize {
    // SAFETY: the game's loader, on the game thread, with a NUL-terminated name.
    unsafe { load_texlist(name) }
}

/// Releases a texture list `load_texlist_named` gave.
pub fn release_texlist(texlist: usize) {
    if texlist != 0 {
        // SAFETY: a texlist the game's loader made for us, released once.
        unsafe { free_texlist(texlist) }
    }
}

/// How a solid model moves on its own: a flip (`spin`, BAMS about its side axis) and a lean
/// into turns (`lean`, BAMS about its length), both about the point `lift` above its base.
#[derive(Clone, Copy, Default)]
pub struct Tilt {
    pub spin: i32,
    pub lean: i32,
    pub lift: f32,
}

/// The matrix stack's top (the current matrix), and putting it elsewhere on the stack: a draw
/// made from an earlier matrix (game thread, display pass; `top` within the stack).
pub fn matrix_top() -> usize {
    // SAFETY: the game's matrix stack pointer.
    unsafe { read::<usize>(MATRIX_TOP) }
}

/// # Safety
/// `top` is a matrix of the game's stack (as `matrix_top` gave), put back after.
pub unsafe fn set_matrix_top(top: usize) {
    // SAFETY: as the caller promises.
    unsafe { write(MATRIX_TOP, top) };
}

/// Draws a solid model (a skin) at `position` with a character's `rotation`, `scale`d, with
/// its own texture list: the same transform a character's display uses (translate, rotate
/// Z, X, then Y as 0x8000 - y), then the model's own `tilt`. With a `motion` (and its
/// frame) it's drawn posed by it, the way the game draws a character's animation.
///
/// # Safety
/// Game thread, inside the display pass; `model` is a checked, relocated object tree, and
/// `motion` a checked, relocated NJS_MOTION for it.
pub unsafe fn draw_solid(
    model: usize,
    texlist: usize,
    position: [f32; 3],
    rotation: [i32; 3],
    s: f32,
    tilt: Tilt,
    motion: Option<(usize, f32)>,
) {
    unsafe {
        let top = read::<usize>(MATRIX_TOP);
        let limit = read::<usize>(MATRIX_LIMIT);
        let base_ptr = read::<usize>(MATRIX_BASE);
        let render = read::<usize>(RENDER_STATE);
        let next = top + MATRIX_SIZE;
        if render == 0 || top == 0 || top < base_ptr || next > limit {
            return;
        }
        ptr::copy_nonoverlapping(top as *const u8, next as *mut u8, MATRIX_SIZE);
        write(MATRIX_TOP, next);
        let old_texlist = read::<usize>(render + RENDER_TEXLIST);
        write(render + RENDER_TEXLIST, texlist);
        translate(next, position);
        let [rx, ry, rz] = rotation;
        if rz != 0 {
            rotate(ROTATE_Z, next, rz);
        }
        if rx != 0 {
            rotate(ROTATE_X, next, rx);
        }
        let ry = 0x8000i32.wrapping_sub(ry);
        if ry != 0 {
            rotate(ROTATE_Y, next, ry);
        }
        if tilt.spin != 0 || tilt.lean != 0 {
            translate(next, [0.0, tilt.lift, 0.0]);
            if tilt.lean != 0 {
                rotate(ROTATE_Z, next, tilt.lean);
            }
            if tilt.spin != 0 {
                rotate(ROTATE_X, next, tilt.spin);
            }
            translate(next, [0.0, -tilt.lift, 0.0]);
        }
        if s != 1.0 {
            scale(s);
        }
        match motion {
            Some((motion, frame)) => {
                // No bone callback: the character's display may have left one that records
                // its body's node positions, and these nodes aren't its body.
                let old_bone = read::<usize>(BONE_CALLBACK);
                write(BONE_CALLBACK, 0usize);
                draw_motion(motion, model, frame);
                write(BONE_CALLBACK, old_bone);
            }
            None => draw_object(model),
        }
        write(render + RENDER_TEXLIST, old_texlist);
        write(MATRIX_TOP, top);
    }
}

/// Draws an independently authored model without inheriting another display's strip flags.
///
/// The native polygon walker (0x42CAD0, override at 0x42CC47) applies
/// `(strip & *0x25F0268) | *0x25F02D4` while `*0x25F02D8 & 0x800` is set.
/// An animal or character display may leave that override active. Independent models carry
/// their own culling, alpha and lighting flags, so suspend only the override bit while they
/// draw. Other pass/shadow flags retain their original values.
///
/// # Safety
/// Same requirements as [`draw_solid`]. Call only from the game display thread.
#[cfg_attr(not(target_arch = "x86"), allow(dead_code))]
pub unsafe fn draw_independent_solid(
    model: usize,
    texlist: usize,
    position: [f32; 3],
    rotation: [i32; 3],
    s: f32,
    tilt: Tilt,
    motion: Option<(usize, f32)>,
) {
    #[cfg(target_arch = "x86")]
    {
        struct Restore {
            flags_a: u32,
            flags_b: u32,
            mask: u32,
            callback: usize,
        }
        impl Drop for Restore {
            fn drop(&mut self) {
                // SAFETY: this guard stays on the display thread; these are static globals.
                unsafe {
                    write(RENDER_FLAGS_A, self.flags_a);
                    write(RENDER_FLAGS_B, self.flags_b);
                    write(RENDER_STATE_C, self.mask);
                    write(BONE_CALLBACK, self.callback);
                }
            }
        }
        // SAFETY: caller guarantees the live game display context.
        unsafe {
            let restore = Restore {
                flags_a: read(RENDER_FLAGS_A),
                flags_b: read(RENDER_FLAGS_B),
                mask: read(RENDER_STATE_C),
                callback: read(BONE_CALLBACK),
            };
            write(RENDER_FLAGS_B, restore.flags_b & !0x800);
            // DrawObject uses its fixed 0x42D650 callback. Resetting this is defensive for
            // an optional DrawMotion and does not imply a static draw invokes that hook.
            write(BONE_CALLBACK, 0usize);
            draw_solid(model, texlist, position, rotation, s, tilt, motion);
            drop(restore);
        }
    }
    #[cfg(not(target_arch = "x86"))]
    let _ = (model, texlist, position, rotation, s, tilt, motion);
}

#[cfg(not(target_arch = "x86"))]
mod ffi {
    pub unsafe fn load_object(_: u32, _: *const u8, _: usize) -> usize {
        0
    }
    pub unsafe fn translate(_: usize, _: [f32; 3]) {}
    pub unsafe fn rotate(_: usize, _: usize, _: i32) {}
    pub unsafe fn draw_motion(_: usize, _: usize, _: f32) {}
    pub unsafe fn scale(_: f32) {}
    pub unsafe fn draw_object(_: usize) {}
    pub unsafe fn refresh_fog() {}
    pub unsafe fn load_sonic_effects() {}
    pub unsafe fn calc_point(_: usize, p: [f32; 3]) -> [f32; 3] {
        p
    }
    pub unsafe fn project_screen(_: [f32; 3]) -> [f32; 2] {
        [0.0; 2]
    }
    pub unsafe fn load_texlist(_: usize) -> usize {
        0
    }
    pub unsafe fn load_models(_: usize) -> usize {
        0
    }
    pub unsafe fn load_motions(_: usize) -> usize {
        0
    }
    pub unsafe fn free_models(_: usize) {}
    pub unsafe fn free_motions(_: usize) {}
    pub unsafe fn free_texlist(_: usize) {}
}

use ffi::{
    calc_point, draw_motion, draw_object, free_models, free_motions, free_texlist, load_models,
    load_motions, load_object, load_sonic_effects, load_texlist, project_screen, refresh_fog,
    rotate, scale, translate,
};

#[cfg(test)]
mod tests {
    #[test]
    fn tags_of_chao_on_the_same_spot_stack() {
        let lifts = super::tag_lifts(&[[0.0; 3], [0.5, 0.0, 0.0], [50.0, 0.0, 0.0], [0.0; 3]]);
        assert_eq!(lifts, vec![0.0, 2.0, 0.0, 4.0]);
    }
    use super::*;

    fn pose(x: f32, rot_y: i32, anim: u16, frame: f32) -> Pose {
        Pose {
            character: 0,
            position: [x, 0.0, 0.0],
            rotation: [0, rot_y, 0],
            anim,
            frame,
            flags: 0,
            upgrades: 0,
            action: 0,
            status: 0,
        }
    }

    #[test]
    fn blends_position_angle_and_frame() {
        let mid = blend(
            &pose(0.0, 0x1000, 3, 10.0),
            &pose(10.0, 0x2000, 3, 20.0),
            0.5,
        );
        assert_eq!(mid.position[0], 5.0);
        assert_eq!(mid.rotation[1], 0x1800);
        assert_eq!(mid.frame, 15.0);
    }

    #[test]
    fn turns_the_short_way_round() {
        // 0xFF00 -> 0x0100 (+0x200 through zero), not -0xFE00.
        let mid = blend(&pose(0.0, 0xFF00, 0, 0.0), &pose(0.0, 0x0100, 0, 0.0), 0.5);
        assert_eq!(mid.rotation[1] & 0xFFFF, 0);
    }

    #[test]
    fn snaps_on_teleport_and_new_animation() {
        let far = blend(&pose(0.0, 0, 0, 0.0), &pose(5000.0, 0, 0, 0.0), 0.5);
        assert_eq!(far.position[0], 5000.0);
        let changed = blend(&pose(0.0, 0, 1, 30.0), &pose(0.0, 0, 2, 4.0), 0.5);
        assert_eq!((changed.anim, changed.frame), (2, 4.0));
        let looped = blend(&pose(0.0, 0, 1, 58.0), &pose(0.0, 0, 1, 2.0), 0.5);
        assert_eq!(looped.frame, 2.0);
    }
}
