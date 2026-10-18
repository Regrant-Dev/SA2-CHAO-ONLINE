//! Skins: a model drawn in place of a character or a kart (re/skins.md).
//!
//! A skin lives in `<mod>/skins/<id>/`: `skin.ini`, `model.sa2mdl` (a Ninja chunk model with
//! pointers as file offsets, made by tools/skinconv.py) and `textures/` (an SA2 Mod Loader
//! texture pack: index.txt and PNGs). The textures are copied to `<mod>/textures/SKIN_<ID>/`,
//! where the mod loader looks for texture packs, and loaded by that name with the game's own
//! loader.
//!
//! A solid skin (one model, like a car) rides on a character: the mod draws
//! it at the character's position and angle instead of the character, so the character's moves
//! (running, grinding, homing) are all still there. In kart races it's drawn as our kart.
//!
//! Every model file is checked before the game sees it: every offset in range, every chunk a
//! kind the converter writes, every vertex index within its vertex block. A bad file is
//! refused, never drawn.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};

use crate::game;
use crate::hide;
use crate::log;

/// SA Tools' "SA2MDL" magic with format version 3 in the top byte.
const SA2MDL: u64 = 0x4C44_4D32_4153 | (3 << 56);
const MAX_FILE: usize = 8 << 20;
const MAX_OBJECTS: usize = 512;
/// Slots of the game's chunk vertex buffer (0x25EFE48, allocated 0x40000 bytes at 0x43502E:
/// 8192 of 32 bytes; the 0x1000 stored beside it at 0x25EFE4C is never read) a model may use:
/// each block goes in at its offset, and later nodes' strips may use earlier nodes' vertices
/// (how Knuckles' body joins at the joints; re/skins.md).
const VERTEX_SLOTS: usize = 8192;
const MAX_TEXTURES: usize = 64;

const OBJECT_SIZE: usize = 0x34;
const OBJ_MODEL: usize = 0x04;
const OBJ_CHILD: usize = 0x2C;
const OBJ_SIBLING: usize = 0x30;
const MODEL_SIZE: usize = 0x18;

/// The model table: `{index, NJS_OBJECT*}` x 532; we use the pointer field.
const MODEL_TABLE: usize = 0x01DE_9624;
const MODEL_COUNT: usize = 532;
const TEXNAME_SIZE: usize = 12;
const MAX_CHARACTER_TEXTURES: usize = 1024;

/// CharObj2 upgrade bits; the displays draw upgrade parts from them.
const CO2_UPGRADES: usize = game::CO2_UPGRADES;

/// EntityData1 scale (+0x20, x/y/z), which characters' displays apply to the whole body.
const ENTITY_SCALE: usize = 0x20;

/// The player's ObjectMaster (re/characters.md): its display draws the character.
const PLAYER_OBJECT: usize = 0x01DE_A6E0;
const OBJ_DISPLAY: usize = 0x14;
/// The object's later display passes (the object list runs them after every display, 0x470010):
/// what a character draws there (Sonic's +0x1C draws a model at the player) is hidden too.
const OBJ_LATER_DISPLAYS: [usize; 3] = [0x1C, 0x20, 0x24];

/// What `skin.ini` says.
#[derive(Clone, Debug, PartialEq)]
pub struct Info {
    pub id: String,
    pub name: String,
    /// Characters it rides on (`LocalPlayer::character` ids); empty = any.
    pub characters: Vec<u8>,
    /// Size on a character (karts are drawn at 1).
    pub character_scale: f32,
    /// Extra turn on a character (BAMS) to face the way it runs.
    pub turn: i32,
    /// Also our kart in kart races.
    pub kart: bool,
    /// The model's height (at scale 1), to flip and lean about its middle.
    pub height: f32,
    /// How much it leans into turns: 1 normal, 0 not at all, -1 the other way.
    pub lean: f32,
    /// Built on characters' own skeletons (tools/skinrig.py): one model per character, drawn
    /// by that character's own display. Nodes in `keep_nodes` stay the character's.
    pub rigged: bool,
    pub rigs: Vec<Rig>,
    pub keep_nodes: Vec<usize>,
    pub textures: usize,
    /// On its own skeleton with its own animations (tools/skinanim.py): `motions.sa2mot`,
    /// and which of them plays when (`ROLES` order), by index.
    pub animated: bool,
    pub roles: [Option<usize>; ROLES],
    /// How far each motion's cycle goes (game units at size 1; 0 = in place), so walks and
    /// runs play as fast as the character moves.
    pub strides: Vec<f32>,
    pub pace: Pace,
}

/// Animation roles: what an animated skin's character is doing. The garden ones come from the
/// player's action and holding something (re/skins.md "Garden actions").
const ROLES: usize = 14;
const IDLE: usize = 0;
const WALK: usize = 1;
const RUN: usize = 2;
const JUMP: usize = 3;
const FALL: usize = 4;
const PICKUP: usize = 5;
const CARRY: usize = 6;
const CARRY_WALK: usize = 7;
const CARRY_RUN: usize = 8;
const THROW: usize = 9;
const PUT_DOWN: usize = 10;
const PET: usize = 11;
const SHAKE: usize = 12;
const WHISTLE: usize = 13;
const ROLE_NAMES: [&str; ROLES] = [
    "idle",
    "walk",
    "run",
    "jump",
    "fall",
    "pickup",
    "carry",
    "carry_walk",
    "carry_run",
    "throw",
    "put_down",
    "pet",
    "shake",
    "whistle",
];

/// The role a player action plays, and whether it plays once (holding its last frame) rather
/// than looping.
fn action_role(action: u8) -> Option<(usize, bool)> {
    match action {
        // Pick up: at hand height, off the ground, a kind-0xE object.
        19 | 41 | 49 => Some((PICKUP, true)),
        // Throw, on the ground or in the air.
        26 | 33 => Some((THROW, true)),
        20 => Some((PUT_DOWN, true)),
        43 => Some((PET, false)),
        // Reach for a tree, hold it, shake it.
        23 | 34 | 35 => Some((SHAKE, false)),
        42 => Some((WHISTLE, true)),
        _ => None,
    }
}

/// What the character is doing, for its animated skin.
#[derive(Clone, Copy, Debug, Default)]
pub struct Doing {
    pub grounded: bool,
    pub curled: bool,
    /// EntityData1 action.
    pub action: u8,
    pub holding: bool,
    /// How far through its own animation the character is (0..1): one-off actions keep up
    /// with it, so the skin finishes when the character does.
    pub progress: Option<f32>,
    /// The skin's pacing.
    pub pace: Pace,
}

/// A rigged skin's model for one character: its file, the model-table slots the character's
/// display draws its body from, the CharObj2 field holding its texture list, and how tall it
/// stands (0: the skin's `height`).
#[derive(Clone, Debug, PartialEq)]
pub struct Rig {
    pub character: u8,
    pub file: String,
    pub slots: Vec<usize>,
    pub field: usize,
    pub height: f32,
}

impl Rig {
    /// `rig_<character>=<slots>;<field>[;<height>]`, the model in `model_<character>.sa2mdl`.
    fn parse(character: &str, v: &str) -> Option<Self> {
        let mut fields = v.split(';');
        let (slots, field) = (fields.next()?, fields.next()?);
        let height = fields
            .next()
            .and_then(|h| h.trim().parse::<f32>().ok())
            .filter(|h| h.is_finite() && (0.0..1000.0).contains(h))
            .unwrap_or(0.0);
        let rig = Rig {
            character: character_id(character)?,
            file: format!("model_{}.sa2mdl", character.to_ascii_lowercase()),
            slots: numbers(slots).filter(|&n| n < MODEL_COUNT).collect(),
            field: numbers(field).next().filter(|&n| n < 0x1000)?,
            height,
        };
        (!rig.slots.is_empty() && rig.field != 0).then_some(rig)
    }
}

impl Info {
    fn parse(id: &str, text: &str) -> Option<Self> {
        let mut info = Info {
            id: id.to_string(),
            name: id.to_string(),
            characters: Vec::new(),
            character_scale: 1.0,
            turn: 0,
            kart: false,
            height: 5.0,
            lean: 1.0,
            rigged: false,
            rigs: Vec::new(),
            keep_nodes: Vec::new(),
            textures: 0,
            animated: false,
            roles: [None; ROLES],
            strides: Vec::new(),
            pace: Pace::default(),
        };
        for line in text.lines() {
            let Some((k, v)) = line.split_once('=') else {
                continue;
            };
            let v = v.trim();
            match k.trim().to_ascii_lowercase().as_str() {
                "name" if !v.is_empty() => info.name = v.chars().take(24).collect(),
                "characters" => {
                    info.characters = v
                        .split(',')
                        .filter_map(|c| character_id(c.trim()))
                        .collect()
                }
                "character_scale" => info.character_scale = v.parse().unwrap_or(1.0),
                "turn" => info.turn = (v.parse::<f32>().unwrap_or(0.0) * 65536.0 / 360.0) as i32,
                "kart" => {
                    info.kart =
                        matches!(v.to_ascii_lowercase().as_str(), "1" | "on" | "yes" | "true")
                }
                "textures" => info.textures = v.parse().unwrap_or(0),
                "height" => info.height = v.parse().unwrap_or(5.0),
                "lean" => info.lean = v.parse::<f32>().unwrap_or(1.0).clamp(-2.0, 2.0),
                "rigged" => {
                    info.rigged =
                        matches!(v.to_ascii_lowercase().as_str(), "1" | "on" | "yes" | "true")
                }
                "keep_nodes" => info.keep_nodes = numbers(v).collect(),
                "animated" => {
                    info.animated =
                        matches!(v.to_ascii_lowercase().as_str(), "1" | "on" | "yes" | "true")
                }
                "anim_walk_from" | "anim_run_from" | "anim_walk_speed" | "anim_run_speed"
                | "anim_max_rate" => {
                    let set = match k.trim().to_ascii_lowercase().as_str() {
                        "anim_walk_from" => &mut info.pace.walk_from,
                        "anim_run_from" => &mut info.pace.run_from,
                        "anim_walk_speed" => &mut info.pace.walk_speed,
                        "anim_run_speed" => &mut info.pace.run_speed,
                        _ => &mut info.pace.max_rate,
                    };
                    if let Ok(x) = v.parse::<f32>()
                        && x.is_finite()
                        && x > 0.0
                        && x < 100.0
                    {
                        *set = x;
                    }
                }
                "strides" => {
                    info.strides = v
                        .split(',')
                        .map(|x| x.trim().parse::<f32>().unwrap_or(0.0))
                        .map(|x| {
                            if x.is_finite() && x > 0.0 && x < 1.0e4 {
                                x
                            } else {
                                0.0
                            }
                        })
                        .take(MAX_MOTIONS)
                        .collect()
                }
                key if ROLE_NAMES.contains(&key) => {
                    let i = ROLE_NAMES.iter().position(|&r| r == key).unwrap_or(0);
                    info.roles[i] = v.parse().ok().filter(|&n| n < MAX_MOTIONS);
                }
                key => {
                    if let Some(character) = key.strip_prefix("rig_")
                        && let Some(rig) = Rig::parse(character, v)
                        && !info.rigs.iter().any(|r| r.character == rig.character)
                    {
                        info.rigs.push(rig);
                    }
                }
            }
        }
        let sane = |v: f32| v.is_finite() && v > 0.0 && v < 100.0;
        if info.rigged {
            // It goes on the characters it has a model for.
            info.characters = info.rigs.iter().map(|r| r.character).collect();
        }
        let rig_ok = !info.rigged || !info.rigs.is_empty();
        // An animated skin stands idle at least; it isn't rigged on a character too.
        let anim_ok = !info.animated || (info.roles[IDLE].is_some() && !info.rigged);
        (anim_ok
            && info.textures <= MAX_TEXTURES
            && sane(info.character_scale)
            && sane(info.height)
            && rig_ok)
            .then_some(info)
    }

    fn rig_for(&self, character: u8) -> Option<&Rig> {
        self.rigs.iter().find(|r| r.character == character)
    }

    /// The texture pack's name (`<mod>/textures/<name>/`).
    pub fn archive(&self) -> String {
        format!("SKIN_{}", self.id.to_ascii_uppercase())
    }
}

/// Numbers in a comma-separated list (decimal or 0x hex).
fn numbers(v: &str) -> impl Iterator<Item = usize> + '_ {
    v.split(',').filter_map(|n| {
        let n = n.trim();
        match n.strip_prefix("0x") {
            Some(h) => usize::from_str_radix(h, 16).ok(),
            None => n.parse().ok(),
        }
    })
}

/// "sonic", "shadow", ... or a number, as `LocalPlayer::character` counts them.
fn character_id(name: &str) -> Option<u8> {
    let names = [
        "sonic",
        "shadow",
        "tails",
        "eggman",
        "knuckles",
        "rouge",
        "mechtails",
        "mecheggman",
        "amy",
        "metalsonic",
        "chaowalker",
        "darkchaowalker",
    ];
    names
        .iter()
        .position(|n| n.eq_ignore_ascii_case(name))
        .map(|i| i as u8)
        .or_else(|| name.parse().ok())
}

/// A model checked and moved into memory: pointers now point into `words`.
struct Model {
    words: Box<[u32]>,
    /// Objects in depth-first order (object, its children, then its siblings: the order
    /// motions use), as byte offsets in `words`.
    nodes: Vec<usize>,
    /// Byte offsets of the texture ids (tiny chunks' second u16), and what's added to them now.
    tex_ids: Vec<usize>,
    tex_base: u16,
}

impl Model {
    fn root(&self) -> usize {
        // SAFETY: `load` made word 2 (+8) the relocated root object.
        self.words[2] as usize
    }

    /// Node `i` (depth-first) in memory.
    fn node(&self, i: usize) -> Option<usize> {
        self.nodes
            .get(i)
            .map(|&at| self.words.as_ptr() as usize + at)
    }

    /// Makes the texture ids count from `base` (a rigged skin's textures go after the
    /// character's own in one list).
    fn set_texture_base(&mut self, base: u16) {
        if base == self.tex_base {
            return;
        }
        let bytes = self.words.as_mut_ptr().cast::<u8>();
        for &at in &self.tex_ids {
            // SAFETY: `at` is a texture id inside `words` (checked by `load_model`).
            unsafe {
                let p = bytes.add(at).cast::<u16>();
                let v = p.read_unaligned();
                let id = (v & 0x1FFF).wrapping_sub(self.tex_base).wrapping_add(base) & 0x1FFF;
                p.write_unaligned((v & !0x1FFF) | id);
            }
        }
        self.tex_base = base;
    }
}

/// Checks a `.sa2mdl` and relocates it; `None` (and why) if anything is off.
fn load_model(bytes: &[u8], textures: usize) -> Result<Model, String> {
    if bytes.len() < 16 || bytes.len() > MAX_FILE {
        return Err(format!("size {}", bytes.len()));
    }
    let u16_at = |at: usize| -> Option<u16> {
        bytes
            .get(at..at + 2)
            .map(|b| u16::from_le_bytes([b[0], b[1]]))
    };
    let u32_at = |at: usize| -> Option<u32> {
        bytes
            .get(at..at + 4)
            .map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    };
    let magic = u64::from(u32_at(0).unwrap_or(0)) | u64::from(u32_at(4).unwrap_or(0)) << 32;
    if magic != SA2MDL {
        return Err("not an SA2 model (version 3)".into());
    }
    let in_range =
        |at: usize, size: usize| at.is_multiple_of(4) && at >= 16 && at + size <= bytes.len();

    // Pointer fields to relocate (byte offsets in the file).
    let mut pointers: Vec<usize> = vec![8];
    let mut objects: Vec<usize> = Vec::new();
    let mut seen_objects = HashSet::new();
    let mut seen_models = HashSet::new();
    let mut tex_ids = Vec::new();
    let mut vertices = 0;
    let mut plists = Vec::new();
    let mut todo = vec![u32_at(8).unwrap_or(0) as usize];
    while let Some(obj) = todo.pop() {
        if obj == 0 || !seen_objects.insert(obj) {
            continue;
        }
        if seen_objects.len() > MAX_OBJECTS || !in_range(obj, OBJECT_SIZE) {
            return Err(format!("object at {obj:#x}"));
        }
        objects.push(obj);
        for field in [OBJ_MODEL, OBJ_CHILD, OBJ_SIBLING] {
            if u32_at(obj + field) != Some(0) {
                pointers.push(obj + field);
            }
        }
        // Floats for position/scale must be sane; angles are anything.
        for f in [8usize, 12, 16, 0x20, 0x24, 0x28] {
            let v = f32::from_bits(u32_at(obj + f).unwrap_or(0));
            if !v.is_finite() || v.abs() > 1.0e5 {
                return Err(format!("object at {obj:#x}: bad transform"));
            }
        }
        todo.push(u32_at(obj + OBJ_CHILD).unwrap_or(0) as usize);
        todo.push(u32_at(obj + OBJ_SIBLING).unwrap_or(0) as usize);
        let model = u32_at(obj + OBJ_MODEL).unwrap_or(0) as usize;
        if model == 0 || !seen_models.insert(model) {
            continue;
        }
        if !in_range(model, MODEL_SIZE) {
            return Err(format!("model at {model:#x}"));
        }
        let (vlist, plist) = (
            u32_at(model).unwrap_or(0) as usize,
            u32_at(model + 4).unwrap_or(0) as usize,
        );
        if vlist != 0 {
            pointers.push(model);
            vertices = vertices.max(check_vertices(&u32_at, &in_range, vlist)?);
        }
        if plist != 0 {
            pointers.push(model + 4);
            plists.push(plist);
        }
    }
    // Strips may use any slot the model's vertex blocks fill.
    for plist in plists {
        check_polygons(&u16_at, &in_range, plist, vertices, textures, &mut tex_ids)?;
    }
    if objects.is_empty() {
        return Err("no objects".into());
    }
    // Depth-first order, as the game walks the tree.
    let mut nodes = Vec::new();
    let mut seen = HashSet::new();
    let mut stack = vec![u32_at(8).unwrap_or(0) as usize];
    while let Some(mut o) = stack.pop() {
        // Siblings after this object's children: keep the sibling for later.
        while o != 0 && seen.insert(o) {
            nodes.push(o);
            let (child, sibling) = (
                u32_at(o + OBJ_CHILD).unwrap_or(0) as usize,
                u32_at(o + OBJ_SIBLING).unwrap_or(0) as usize,
            );
            if child != 0 {
                stack.push(sibling);
                o = child;
            } else {
                o = sibling;
            }
        }
    }

    // Copy into words (4-byte aligned for the game) and relocate.
    let mut words: Box<[u32]> = vec![0u32; bytes.len().div_ceil(4)].into_boxed_slice();
    for (i, w) in words.iter_mut().enumerate() {
        let at = i * 4;
        let mut b = [0u8; 4];
        let end = (at + 4).min(bytes.len());
        b[..end - at].copy_from_slice(&bytes[at..end]);
        *w = u32::from_le_bytes(b);
    }
    let base = words.as_ptr() as usize;
    for at in pointers {
        let w = &mut words[at / 4];
        *w = (base + *w as usize) as u32;
    }
    Ok(Model {
        words,
        nodes,
        tex_ids,
        tex_base: 0,
    })
}

/// Vertex blocks: plain (`CnkV_VN`, 0x29: position, normal) and weighted (`CnkV_VN_NF`, 0x2C:
/// position, normal, then slot | weight << 16, the slot counted from the block's offset; the
/// weight status, 0 start / 1 middle / 2 end, in the header's flags; re/skins.md "Weighted
/// vertices").
const VERTICES_PLAIN: u32 = 0x29;
const VERTICES_WEIGHTED: u32 = 0x2C;

/// A vertex list: only plain and weighted blocks within `VERTEX_SLOTS`, then the end marker.
/// Returns the end of the highest slot filled.
fn check_vertices(
    u32_at: &dyn Fn(usize) -> Option<u32>,
    in_range: &dyn Fn(usize, usize) -> bool,
    vlist: usize,
) -> Result<usize, String> {
    let mut at = vlist;
    let mut count = 0;
    for _ in 0..1024 {
        if !in_range(at, 4) {
            return Err(format!("vertices at {vlist:#x}"));
        }
        let h1 = u32_at(at).unwrap_or(0);
        if h1 & 0xFF == 0xFF {
            return Ok(count);
        }
        let h2 = u32_at(at + 4).unwrap_or(0);
        let (kind, status, words, offset, n) = (
            h1 & 0xFF,
            (h1 >> 8) & 0x7F,
            (h1 >> 16) as usize,
            (h2 & 0xFFFF) as usize,
            (h2 >> 16) as usize,
        );
        let stride = match kind {
            VERTICES_PLAIN if offset + n <= VERTEX_SLOTS => 6,
            VERTICES_WEIGHTED if status <= 2 => 7,
            _ => return Err(format!("vertex block at {at:#x}")),
        };
        if words != 1 + stride * n {
            return Err(format!("vertex block at {at:#x}"));
        }
        if !in_range(at, 4 + words * 4) {
            return Err(format!("vertex block at {at:#x} runs off the file"));
        }
        for v in 0..n {
            let entry = at + 8 + v * stride * 4;
            for i in 0..6 {
                let f = f32::from_bits(u32_at(entry + i * 4).unwrap_or(0));
                if !f.is_finite() || f.abs() > 1.0e5 {
                    return Err(format!("vertex block at {at:#x}: bad number"));
                }
            }
            if stride == 7 {
                let flags = u32_at(entry + 24).unwrap_or(u32::MAX);
                let slot = offset + (flags & 0xFFFF) as usize;
                if slot >= VERTEX_SLOTS || flags >> 16 > 255 {
                    return Err(format!(
                        "vertex block at {at:#x}: weighted vertex out of range"
                    ));
                }
                count = count.max(slot + 1);
            }
        }
        if stride == 6 {
            count = count.max(offset + n);
        }
        at += 4 + words * 4;
    }
    Err(format!("vertices at {vlist:#x}: no end"))
}

/// A polygon list: diffuse materials (0x11), texture ids (0x08) below `textures`, triangle
/// strips (0x40/0x41) whose indices are below `vertices`, then the end marker.
fn check_polygons(
    u16_at: &dyn Fn(usize) -> Option<u16>,
    in_range: &dyn Fn(usize, usize) -> bool,
    plist: usize,
    vertices: usize,
    textures: usize,
    tex_ids: &mut Vec<usize>,
) -> Result<(), String> {
    let mut at = plist;
    for _ in 0..4096 {
        if !in_range(at & !3, 4) || at + 2 > plist + MAX_FILE {
            return Err(format!("polygons at {plist:#x}"));
        }
        let head = u16_at(at).ok_or("polygons run off the file")?;
        match head & 0xFF {
            0xFF => return Ok(()),
            0x08 => {
                let tex = usize::from(u16_at(at + 2).ok_or("texture id")? & 0x1FFF);
                if tex >= textures.max(1) {
                    return Err(format!("texture {tex} of {textures}"));
                }
                tex_ids.push(at + 2);
                at += 4;
            }
            0x11 => {
                if u16_at(at + 2) != Some(2) {
                    return Err(format!("material at {at:#x}"));
                }
                at += 8;
            }
            kind @ (0x40 | 0x41) => {
                let size = usize::from(u16_at(at + 2).ok_or("strip size")?);
                let end = at + 4 + size * 2;
                let header = u16_at(at + 4).ok_or("strip header")?;
                let stride = if kind == 0x40 { 2 } else { 6 };
                let mut q = at + 6;
                for _ in 0..(header & 0x3FFF) {
                    let len = u16_at(q).ok_or("strip")? as i16;
                    q += 2;
                    let n = usize::from(len.unsigned_abs());
                    if n < 3 || q + n * stride > end {
                        return Err(format!("strip at {at:#x}"));
                    }
                    for v in 0..n {
                        if usize::from(u16_at(q + v * stride).unwrap_or(u16::MAX)) >= vertices {
                            return Err(format!("strip at {at:#x}: vertex out of range"));
                        }
                    }
                    q += n * stride;
                }
                if !in_range(end & !3, 0) || end > plist + MAX_FILE {
                    return Err(format!("strip at {at:#x} runs off the file"));
                }
                at = end;
            }
            other => return Err(format!("chunk type {other:#x} at {at:#x}")),
        }
    }
    Err(format!("polygons at {plist:#x}: no end"))
}

/// tools/skinanim.py's motion file: magic, motion count (+8), offset of the table of motion
/// offsets (+12); pointers are file offsets.
const SA2COMOT: &[u8; 8] = b"SA2COMOT";
const MAX_MOTIONS: usize = 64;
const MAX_FRAMES: u32 = 20_000;
/// NJS_MOTION: mdata, frame count, type (1 position | 2 angle), key arrays per node.
const MOTION_SIZE: usize = 12;
const MOTION_TYPE: u16 = 3;
const MOTION_ARRAYS: u16 = 2;
/// Per node: position keys, angle keys, then their counts.
const MDATA_SIZE: usize = 16;
/// A key: frame, then three floats (position) or three BAMS angles.
const KEY_SIZE: usize = 16;

/// Motions checked and moved into memory, for a model with a given node count.
struct Motions {
    words: Box<[u32]>,
    /// Each NJS_MOTION's byte offset in `words`, and its frame count.
    list: Vec<(usize, u32)>,
}

impl Motions {
    /// Motion `i` in memory and its frame count.
    fn get(&self, i: usize) -> Option<(usize, u32)> {
        self.list
            .get(i)
            .map(|&(at, frames)| (self.words.as_ptr() as usize + at, frames))
    }
}

/// Checks a `.sa2mot` for a model with `nodes` nodes and relocates it.
fn load_motions(bytes: &[u8], nodes: usize) -> Result<Motions, String> {
    if bytes.len() < 16 || bytes.len() > MAX_FILE || &bytes[..8] != SA2COMOT {
        return Err("not a motion file".into());
    }
    let u16_at = |at: usize| -> u16 {
        bytes
            .get(at..at + 2)
            .map_or(0, |b| u16::from_le_bytes([b[0], b[1]]))
    };
    let u32_at = |at: usize| -> u32 {
        bytes
            .get(at..at + 4)
            .map_or(0, |b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    };
    let in_range = |at: usize, size: usize| {
        at.is_multiple_of(4) && at >= 16 && at.checked_add(size).is_some_and(|e| e <= bytes.len())
    };
    let count = u32_at(8) as usize;
    let table = u32_at(12) as usize;
    if count == 0 || count > MAX_MOTIONS || !in_range(table, count * 4) {
        return Err(format!("{count} motions, table at {table:#x}"));
    }
    let mut pointers = Vec::new();
    let mut list = Vec::new();
    for i in 0..count {
        let m = u32_at(table + i * 4) as usize;
        if !in_range(m, MOTION_SIZE) {
            return Err(format!("motion {i} at {m:#x}"));
        }
        let (mdata, frames) = (u32_at(m) as usize, u32_at(m + 4));
        if u16_at(m + 8) != MOTION_TYPE || u16_at(m + 10) & 0x3F != MOTION_ARRAYS {
            return Err(format!("motion {i}: not position + angle keys"));
        }
        if !(2..=MAX_FRAMES).contains(&frames) || !in_range(mdata, nodes * MDATA_SIZE) {
            return Err(format!("motion {i}: {frames} frames, data at {mdata:#x}"));
        }
        pointers.push(m);
        for n in 0..nodes {
            let entry = mdata + n * MDATA_SIZE;
            for (kind, floats) in [(0, true), (1, false)] {
                let keys = u32_at(entry + kind * 4) as usize;
                let len = u32_at(entry + 8 + kind * 4);
                if keys == 0 && len == 0 {
                    continue;
                }
                if len == 0 || len > MAX_FRAMES || !in_range(keys, len as usize * KEY_SIZE) {
                    return Err(format!("motion {i} node {n}: keys at {keys:#x}"));
                }
                // Frames in order within the motion; positions finite and sane.
                let mut last = None;
                for k in 0..len as usize {
                    let at = keys + k * KEY_SIZE;
                    let frame = u32_at(at);
                    if frame >= frames || last.is_some_and(|l| frame <= l) {
                        return Err(format!("motion {i} node {n}: key frames"));
                    }
                    last = Some(frame);
                    if floats
                        && (1..4).any(|c| {
                            let v = f32::from_bits(u32_at(at + c * 4));
                            !v.is_finite() || v.abs() > 1.0e5
                        })
                    {
                        return Err(format!("motion {i} node {n}: bad position"));
                    }
                }
                pointers.push(entry + kind * 4);
            }
        }
        list.push((m, frames));
    }
    let mut words: Box<[u32]> = vec![0u32; bytes.len().div_ceil(4)].into_boxed_slice();
    for (i, w) in words.iter_mut().enumerate() {
        *w = u32_at(i * 4);
    }
    let tail = bytes.len() % 4;
    if tail != 0 {
        let mut b = [0u8; 4];
        b[..tail].copy_from_slice(&bytes[bytes.len() - tail..]);
        words[bytes.len() / 4] = u32::from_le_bytes(b);
    }
    let base = words.as_ptr() as usize;
    for at in pointers {
        let w = &mut words[at / 4];
        *w = (base + *w as usize) as u32;
    }
    Ok(Motions { words, list })
}

struct Loaded {
    id: String,
    model: Model,
    /// The texture list, and the scene it was loaded in (`scene`).
    texlist: usize,
    level: (i16, u32, i32),
    /// Rigged skins: the character's textures then ours, as one list (entries, then the
    /// NJS_TEXLIST pointing at them), and what it was built from.
    combined: Box<[u8]>,
    combined_list: Box<[u32; 2]>,
    combined_from: (usize, usize),
    /// An animated skin's motions.
    motions: Option<Motions>,
    /// When it was last used (`Skins::clock`), to drop the longest unused first.
    used: u64,
}

/// Skins kept loaded at once (ours and other players').
const MAX_LOADED: usize = 6;

struct Skins {
    dir: PathBuf,
    list: Vec<Info>,
    /// Our skin (id), from the config / F9.
    mine: Option<String>,
    loaded: Vec<Loaded>,
    /// Counts lookups (for `Loaded::used`).
    clock: u64,
    /// Skins that failed to load (not tried again this session).
    failed: HashSet<String>,
    /// Other players' skins move on their own (player key -> its animator).
    others: std::collections::HashMap<String, (Animator, Moves)>,
}

static SKINS: Mutex<Option<Skins>> = Mutex::new(None);
/// The player's own display, while ours draws the skin instead.
static PLAYER_DISPLAY: AtomicUsize = AtomicUsize::new(0);

fn with<R>(f: impl FnOnce(&mut Skins) -> R) -> Option<R> {
    SKINS
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .as_mut()
        .map(f)
}

/// At start: finds the skins in `<mod>/skins/` and puts their textures where the mod loader
/// looks for texture packs.
/// Our skin while it's on, for the others (online.rs sends it).
static WORN: Mutex<Option<String>> = Mutex::new(None);

pub fn worn() -> Option<String> {
    WORN.lock().unwrap_or_else(|e| e.into_inner()).clone()
}

/// Until when the main menu says new skins need a restart (their texture packs are only
/// found by the mod loader at start).
static RESTART_HINT: Mutex<Option<std::time::Instant>> = Mutex::new(None);

pub fn restart_hint() -> bool {
    RESTART_HINT
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .is_some_and(|until| std::time::Instant::now() < until)
}

/// The skins folder read again (shared skins came down): new and changed skins load.
fn rescan() {
    let Some(mod_path) = with(|s| s.dir.parent().map(Path::to_path_buf)).flatten() else {
        return;
    };
    let list = scan(&mod_path.to_string_lossy());
    with(|s| {
        let new = list.iter().any(|i| !s.list.iter().any(|o| o.id == i.id));
        if new {
            *RESTART_HINT.lock().unwrap_or_else(|e| e.into_inner()) =
                Some(std::time::Instant::now() + std::time::Duration::from_secs(20));
        }
        release(s);
        s.failed.clear();
        s.list = list;
    });
}

/// The skins in `<mod>/skins/` (their texture packs copied where the mod loader finds them).
fn scan(mod_path: &str) -> Vec<Info> {
    let dir = Path::new(mod_path).join("skins");
    let mut list = Vec::new();
    if let Ok(entries) = std::fs::read_dir(&dir) {
        for e in entries.flatten() {
            let id = e.file_name().to_string_lossy().to_ascii_lowercase();
            if !id.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') || id.len() > 32 {
                continue;
            }
            let Ok(text) = std::fs::read_to_string(e.path().join("skin.ini")) else {
                continue;
            };
            match Info::parse(&id, &text) {
                Some(info) => {
                    install_textures(mod_path, &e.path(), &info);
                    log!("skins: found '{}' ({id})", info.name);
                    list.push(info);
                }
                None => log!("skins: {id}/skin.ini isn't usable"),
            }
        }
    }
    list.sort_by(|a, b| a.name.cmp(&b.name));
    list
}

pub fn init(mod_path: &str) {
    let dir = Path::new(mod_path).join("skins");
    let list = scan(mod_path);
    // Our pick from config.ini (`Skin=<id>`).
    let mine = std::fs::read_to_string(Path::new(mod_path).join("config.ini"))
        .ok()
        .and_then(|t| {
            t.lines().find_map(|l| {
                let (k, v) = l.split_once('=')?;
                k.trim()
                    .eq_ignore_ascii_case("skin")
                    .then(|| v.trim().to_ascii_lowercase())
            })
        })
        .filter(|id| list.iter().any(|i| &i.id == id));
    if let Some(id) = &mine {
        log!("skins: wearing {id}");
    }
    *SKINS.lock().unwrap_or_else(|e| e.into_inner()) = Some(Skins {
        dir,
        list,
        mine,
        loaded: Vec::new(),
        clock: 0,
        failed: HashSet::new(),
        others: std::collections::HashMap::new(),
    });
}

/// Copies a skin's texture pack to `<mod>/textures/SKIN_<ID>/` when it isn't there or differs.
fn install_textures(mod_path: &str, skin: &Path, info: &Info) {
    let from = skin.join("textures");
    let to = Path::new(mod_path).join("textures").join(info.archive());
    let Ok(files) = std::fs::read_dir(&from) else {
        return;
    };
    let _ = std::fs::create_dir_all(&to);
    for f in files.flatten() {
        let target = to.join(f.file_name());
        let same = std::fs::read(f.path()).ok() == std::fs::read(&target).ok();
        if !same && let Err(e) = std::fs::copy(f.path(), &target) {
            log!("skins: can't copy {}: {e}", f.path().display());
        }
    }
}

/// The skins found: (id, name).
pub fn list() -> Vec<(String, String)> {
    with(|s| {
        s.list
            .iter()
            .map(|i| (i.id.clone(), i.name.clone()))
            .collect()
    })
    .unwrap_or_default()
}

/// Our skin (an id from `list`, or none).
pub fn set_mine(id: Option<String>) {
    with(|s| s.mine = id.filter(|id| s.list.iter().any(|i| &i.id == id)));
}

/// Our skin's name, for the menu.
pub fn mine_name() -> Option<String> {
    with(|s| {
        let id = s.mine.as_ref()?;
        s.list.iter().find(|i| &i.id == id).map(|i| i.name.clone())
    })
    .flatten()
}

/// A skin ready to draw.
struct Ready {
    root: usize,
    /// An animated skin: each role's motion (0 = none) and how it plays.
    poses: Option<[(usize, Cycle); ROLES]>,
    texlist: usize,
    scale: f32,
    turn: i32,
    height: f32,
    lean: f32,
    pace: Pace,
}

/// Our skin, loaded, if it applies to `character` now.
fn ready_for(s: &mut Skins, character: Option<u8>) -> Option<Ready> {
    let id = s.mine.clone()?;
    ready(s, &id, character).map(|(r, _)| r)
}

/// Skin `id`, loaded (and where in `s.loaded`), if it applies to `character` now.
fn ready(s: &mut Skins, id: &str, character: Option<u8>) -> Option<(Ready, usize)> {
    let id = id.to_string();
    let info = s.list.iter().find(|i| i.id == id)?.clone();
    if let Some(c) = character
        && !info.characters.is_empty()
        && !info.characters.contains(&c)
    {
        return None;
    }
    let level = scene();
    // A rigged skin's model for this character; otherwise the one solid model.
    let file = match character {
        Some(c) if info.rigged => info.rig_for(c)?.file.clone(),
        _ if info.rigged => return None,
        _ => "model.sa2mdl".to_string(),
    };
    let key = format!("{id}/{file}");
    s.clock += 1;
    if !s.loaded.iter().any(|l| l.id == key) {
        if s.failed.contains(&key) {
            return None;
        }
        if s.loaded.len() >= MAX_LOADED
            && let Some(oldest) = (0..s.loaded.len()).min_by_key(|&i| s.loaded[i].used)
        {
            let l = s.loaded.swap_remove(oldest);
            if l.texlist != 0 && texlist_ok(l.texlist) {
                crate::puppet::release_texlist(l.texlist);
            }
        }
        let path = s.dir.join(&id).join(&file);
        let animated = info.animated;
        let motions_path = s.dir.join(&id).join("motions.sa2mot");
        match std::fs::read(&path)
            .map_err(|e| e.to_string())
            .and_then(|b| load_model(&b, info.textures))
            .and_then(|model| {
                if !animated {
                    return Ok((model, None));
                }
                let motions = std::fs::read(&motions_path)
                    .map_err(|e| format!("motions: {e}"))
                    .and_then(|b| load_motions(&b, model.nodes.len()))?;
                // Every role names a motion there is.
                if info
                    .roles
                    .iter()
                    .flatten()
                    .any(|&r| r >= motions.list.len())
                {
                    return Err("a role names a missing motion".into());
                }
                Ok((model, Some(motions)))
            }) {
            Ok((model, motions)) => {
                log!("skins: '{}' loaded", info.name);
                s.loaded.push(Loaded {
                    id: key.clone(),
                    model,
                    texlist: 0,
                    level: (0, 0, 0),
                    combined: Box::new([]),
                    combined_list: Box::new([0; 2]),
                    combined_from: (0, 0),
                    motions,
                    used: 0,
                });
            }
            Err(e) => {
                log!("skins: '{}' ({file}) refused: {e}", info.name);
                s.failed.insert(key);
                return None;
            }
        }
    }
    let index = s.loaded.iter().position(|l| l.id == key)?;
    let clock = s.clock;
    let loaded = &mut s.loaded[index];
    loaded.used = clock;
    // Again for each scene, and whenever the game has freed them since (it frees textures
    // between Chao World's areas within one level): drawing with freed ones crashes.
    if loaded.texlist == 0 || loaded.level != level || !texlist_ok(loaded.texlist) {
        // Ours to release only while they're still there.
        if loaded.texlist != 0 && texlist_ok(loaded.texlist) {
            crate::puppet::release_texlist(loaded.texlist);
        }
        let name = std::ffi::CString::new(info.archive()).ok()?;
        loaded.texlist = crate::puppet::load_texlist_named(name.as_ptr() as usize);
        loaded.level = level;
        log!(
            "skins: textures {} {}",
            info.archive(),
            if loaded.texlist == 0 {
                "not found"
            } else {
                "loaded"
            }
        );
    }
    if loaded.texlist == 0 || !texlist_ok(loaded.texlist) {
        loaded.texlist = 0;
        return None;
    }
    let scale = if character.is_some() {
        info.character_scale
    } else {
        1.0
    };
    let poses = loaded.motions.as_ref().map(|m| {
        info.roles.map(|r| {
            r.and_then(|i| {
                let (motion, frames) = m.get(i)?;
                let stride = info.strides.get(i).copied().unwrap_or(0.0);
                Some((motion, Cycle { frames, stride }))
            })
            .unwrap_or((0, Cycle::NONE))
        })
    });
    let ready = Ready {
        root: loaded.model.root(),
        poses,
        texlist: loaded.texlist,
        scale,
        turn: info.turn,
        height: info.height,
        lean: info.lean,
        pace: info.pace,
    };
    Some((ready, index))
}

/// Where textures last: the level, the stage run, the Chao World area (game.rs).
fn scene() -> (i16, u32, i32) {
    (
        game::current_level(),
        game::stage_run(),
        game::current_chao_area(),
    )
}

/// Every texture of an NJS_TEXLIST still loaded (the game hasn't freed it): each entry's
/// texture (+8) and its surface (+0xC there), as the draw reads them (0x42C4D0).
fn texlist_ok(texlist: usize) -> bool {
    // SAFETY: a texture list we loaded; its entries point at the game's texture records.
    unsafe {
        let entries = (texlist as *const usize).read();
        let count = ((texlist + 4) as *const u32).read() as usize;
        if entries == 0 || count == 0 || count > MAX_CHARACTER_TEXTURES {
            return false;
        }
        (0..count).all(|i| {
            let texture = ((entries + i * TEXNAME_SIZE + 8) as *const usize).read();
            texture != 0 && ((texture + 0xC) as *const usize).read() != 0
        })
    }
}

fn release(s: &mut Skins) {
    for l in s.loaded.drain(..) {
        if l.texlist != 0 && texlist_ok(l.texlist) {
            crate::puppet::release_texlist(l.texlist);
        }
    }
}

/// Call every frame (game thread): puts our skin's display on the player while it applies.
pub fn on_frame() {
    // Shared skins came down: look again (skin_sync.rs).
    if crate::skin_sync::take_changed() {
        rescan();
    }
    // SAFETY: the static player object pointer, and its display field, on the game thread.
    let object = unsafe { (PLAYER_OBJECT as *const usize).read_volatile() };
    let in_stage = game::current_level() > 0 && !game::two_player();
    let character = game::local_player().map(|p| p.character);
    let ours = player_display as extern "C" fn(usize) as usize;
    let wanted = in_stage
        && object != 0
        && !crate::kart::in_kart_race()
        && with(|s| ready_for(s, character).is_some()).unwrap_or(false);
    // What the others see us wearing.
    *WORN.lock().unwrap_or_else(|e| e.into_inner()) = if wanted {
        with(|s| s.mine.clone()).flatten()
    } else {
        None
    };
    // The skin's own voice while it's drawn (skins/<id>/sounds/).
    let voice = if wanted {
        with(|s| s.mine.as_ref().map(|id| s.dir.join(id))).flatten()
    } else {
        None
    };
    crate::voice::frame(voice.as_deref());
    hide::set_jump_ball_hidden(wanted);
    if object == 0 {
        return;
    }
    let later: [extern "C" fn(usize); 3] = [later_display_0, later_display_1, later_display_2];
    let fields = std::iter::once((OBJ_DISPLAY, ours, &PLAYER_DISPLAY)).chain(
        OBJ_LATER_DISPLAYS
            .iter()
            .zip(later)
            .zip(&LATER_DISPLAYS)
            .map(|((&at, f), saved)| (at, f as usize, saved)),
    );
    for (at, ours, saved) in fields {
        let field = (object + at) as *mut usize;
        // SAFETY: the live player object's display fields.
        unsafe {
            let now = field.read();
            if wanted && now != ours && now != 0 {
                saved.store(now, Ordering::Relaxed);
                field.write(ours);
            } else if !wanted && now == ours {
                field.write(saved.load(Ordering::Relaxed));
            }
        }
    }
}

/// The player object's own later displays, while ours stand in for them.
static LATER_DISPLAYS: [AtomicUsize; 3] = [const { AtomicUsize::new(0) }; 3];

extern "C" fn later_display_0(object: usize) {
    later_display(0, object);
}

extern "C" fn later_display_1(object: usize) {
    later_display(1, object);
}

extern "C" fn later_display_2(object: usize) {
    later_display(2, object);
}

/// A later display of the character's, with nothing of it drawn (the skin is drawn in its
/// main one).
fn later_display(i: usize, object: usize) {
    let original = LATER_DISPLAYS[i].load(Ordering::Relaxed);
    if original != 0 && hide::hidden(0, || call_display(original, object)).is_none() {
        call_display(original, object);
    }
}

/// A solid skin's own movement, from what its character does: flips while curled up (jump,
/// spin dash, homing), and leans into turns.
pub struct Moves {
    spin: i32,
    lean: f32,
    last_yaw: Option<i32>,
}

/// One flip per this many frames.
const FLIP_FRAMES: i32 = 18;
/// Lean per BAMS of turn in a frame, the most it leans, and how fast it follows.
const LEAN_PER_TURN: f32 = 6.0;
const LEAN_MAX: f32 = 2500.0;
const LEAN_EASE: f32 = 0.2;

impl Moves {
    pub const fn new() -> Self {
        Moves {
            spin: 0,
            lean: 0.0,
            last_yaw: None,
        }
    }

    /// One drawn frame: curled up or not, the facing (BAMS), the model's drawn height.
    pub fn step(&mut self, curled: bool, yaw: i32, height: f32) -> crate::puppet::Tilt {
        let step = 0x10000 / FLIP_FRAMES;
        let at = self.spin & 0xFFFF;
        self.spin = if curled {
            at + step
        } else if at == 0 {
            0
        } else {
            // Finish the flip upright rather than snapping back.
            (at + step).min(0x10000) & 0xFFFF
        };
        let turned = self.last_yaw.map_or(0, |y| yaw.wrapping_sub(y) as i16);
        self.last_yaw = Some(yaw);
        let want = (f32::from(turned) * LEAN_PER_TURN).clamp(-LEAN_MAX, LEAN_MAX);
        self.lean += (want - self.lean) * LEAN_EASE;
        crate::puppet::Tilt {
            spin: self.spin & 0xFFFF,
            lean: self.lean as i32,
            lift: height / 2.0,
        }
    }
}

static OUR_MOVES: Mutex<Moves> = Mutex::new(Moves::new());

/// An animated skin's animation, from how its character moves: idle, walk or run on the
/// ground by speed, jump on leaving it (then fall), fall when dropping off.
pub struct Animator {
    role: usize,
    frame: f32,
    last: Option<[f32; 3]>,
    speed: f32,
}

/// The skins' animations are 30 frames a second; the game draws 60.
const ANIM_STEP: f32 = 0.5;
/// Back to walking below this share of the speed it started running at.
const RUN_UNTIL: f32 = 0.8;

/// How an animated skin's walks and runs keep up with the character: `anim_*` lines in its
/// skin.ini (docs/skins.md), each defaulting to what suits most skins.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Pace {
    /// Ground speed (units a frame) to start walking (`anim_walk_from`).
    pub walk_from: f32,
    /// To run, when the walk and run don't say how far they go (`anim_run_from`).
    pub run_from: f32,
    /// Speeds in-place walk and run animations look right at, at size 1 (`anim_walk_speed`,
    /// `anim_run_speed`).
    pub walk_speed: f32,
    pub run_speed: f32,
    /// The fastest a walk or run may play, times its own pace (`anim_max_rate`): a run only
    /// blurs so much.
    pub max_rate: f32,
}

impl Default for Pace {
    fn default() -> Self {
        Pace {
            walk_from: 0.15,
            run_from: 2.0,
            walk_speed: 0.8,
            run_speed: 3.0,
            max_rate: 6.0,
        }
    }
}

/// The slowest a walk or run plays, times its own pace.
const MIN_RATE: f32 = 0.3;

/// One animation's length (frames; 0 = none) and how far its cycle goes (0 = in place).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Cycle {
    pub frames: u32,
    pub stride: f32,
}

impl Cycle {
    pub const NONE: Cycle = Cycle {
        frames: 0,
        stride: 0.0,
    };

    /// The ground speed (units a frame) it plays at its own pace, at `size`.
    fn pace(self, size: f32) -> Option<f32> {
        (self.stride > 0.0 && self.frames > 1)
            // The last frame is the first again: a cycle is `frames - 1` long.
            .then(|| self.stride * size / (self.frames - 1) as f32 * ANIM_STEP)
    }
}
/// More than this in a frame is a teleport, not movement.
const TELEPORT: f32 = 60.0;
const SPEED_EASE: f32 = 0.3;

impl Animator {
    pub const fn new() -> Self {
        Animator {
            role: IDLE,
            frame: 0.0,
            last: None,
            speed: 0.0,
        }
    }

    /// One drawn frame: where the character is, what it's doing, the skin's size, and each
    /// role's animation. Returns the role and frame.
    pub fn step(
        &mut self,
        at: [f32; 3],
        doing: Doing,
        size: f32,
        cycles: [Cycle; ROLES],
    ) -> (usize, f32) {
        let Doing {
            grounded, curled, ..
        } = doing;
        let frames = cycles.map(|c| c.frames);
        let (moved, rise) = match self.last {
            Some(l) => {
                let (dx, dy, dz) = (at[0] - l[0], at[1] - l[1], at[2] - l[2]);
                let d = (dx * dx + dz * dz).sqrt();
                if d > TELEPORT || dy.abs() > TELEPORT || !d.is_finite() || !dy.is_finite() {
                    (0.0, 0.0)
                } else {
                    (d, dy)
                }
            }
            None => (0.0, 0.0),
        };
        self.last = Some(at);
        self.speed += (moved - self.speed) * SPEED_EASE;
        let size = size.max(0.1);
        // Run from halfway between the walk's and the run's own paces (carrying, the carrying
        // ones'), or at a set speed without both.
        let (walk_role, run_role) = if doing.holding && frames[CARRY_WALK] > 1 {
            (CARRY_WALK, CARRY_RUN)
        } else {
            (WALK, RUN)
        };
        let run_from = match (cycles[walk_role].pace(size), cycles[run_role].pace(size)) {
            (Some(w), Some(r)) if r > w => (w + r) / 2.0,
            _ => doing.pace.run_from,
        };
        let running = self.role == RUN || self.role == CARRY_RUN;
        let moving = if !grounded || curled {
            match self.role {
                // A jump plays out, then it falls.
                JUMP if self.frame + ANIM_STEP < frames[JUMP].saturating_sub(1) as f32 => JUMP,
                JUMP | FALL => FALL,
                _ if rise > 0.0 || curled => JUMP,
                _ => FALL,
            }
        } else if self.speed < doing.pace.walk_from {
            IDLE
        } else if self.speed > run_from || (running && self.speed > run_from * RUN_UNTIL) {
            RUN
        } else {
            WALK
        };
        // Carrying something: the same, holding it.
        let moving = match moving {
            IDLE if doing.holding => CARRY,
            WALK if doing.holding => CARRY_WALK,
            RUN if doing.holding => CARRY_RUN,
            m => m,
        };
        // A garden action, if the skin has an animation for it; else how it's moving.
        let action = action_role(doing.action).filter(|&(r, _)| frames[r] > 1);
        // Missing animations: the nearest one there is.
        let fallbacks: [&[usize]; ROLES] = [
            &[IDLE],
            &[WALK, RUN, IDLE],
            &[RUN, WALK, IDLE],
            &[JUMP, FALL, IDLE],
            &[FALL, JUMP, IDLE],
            &[PICKUP],
            &[CARRY, IDLE],
            &[CARRY_WALK, WALK, RUN, IDLE],
            &[CARRY_RUN, CARRY_WALK, RUN, WALK, IDLE],
            &[THROW],
            &[PUT_DOWN],
            &[PET],
            &[SHAKE],
            &[WHISTLE],
        ];
        let want = action.map_or(moving, |(r, _)| r);
        let role = fallbacks[want]
            .iter()
            .chain(fallbacks[moving])
            .copied()
            .find(|&r| frames[r] > 1)
            .unwrap_or(IDLE);
        if role != self.role {
            self.role = role;
            self.frame = 0.0;
        }
        // Walks and runs keep pace with the character: one cycle per stride it covers.
        let rate = match role {
            WALK | RUN | CARRY_WALK | CARRY_RUN => {
                let pace = cycles[role].pace(size).unwrap_or(
                    if role == WALK || role == CARRY_WALK {
                        doing.pace.walk_speed
                    } else {
                        doing.pace.run_speed
                    } * size,
                );
                (self.speed / pace).clamp(MIN_RATE, doing.pace.max_rate.max(MIN_RATE))
            }
            _ => 1.0,
        };
        let last = frames[role].saturating_sub(1).max(1) as f32;
        self.frame += ANIM_STEP * rate;
        let once = role == JUMP || action.is_some_and(|(r, once)| r == role && once);
        if once
            && role != JUMP
            && let Some(p) = doing.progress
        {
            // At least its own pace, caught up to where the character is; never backwards
            // (one action can play more than one game animation).
            self.frame = self.frame.max(p * last).min(last);
        } else if once {
            // Holds its last frame.
            self.frame = self.frame.min(last);
        } else if self.frame >= last {
            self.frame %= last;
        }
        (role, self.frame)
    }
}

static OUR_ANIM: Mutex<Animator> = Mutex::new(Animator::new());

/// A model's nodes in depth-first order, from the game's own tree (bounded, each once).
///
/// # Safety
/// `root` is a live NJS_OBJECT tree (a model-table entry).
unsafe fn game_tree(root: usize) -> Vec<usize> {
    let mut out = Vec::new();
    let mut seen = HashSet::new();
    let mut stack = vec![root];
    while let Some(mut o) = stack.pop() {
        while o != 0 && out.len() < MAX_OBJECTS && seen.insert(o) {
            out.push(o);
            // SAFETY: as above.
            let (child, sibling) = unsafe {
                (
                    ((o + OBJ_CHILD) as *const usize).read(),
                    ((o + OBJ_SIBLING) as *const usize).read(),
                )
            };
            if child != 0 {
                stack.push(sibling);
                o = child;
            } else {
                o = sibling;
            }
        }
    }
    out
}

/// A rigged skin on our character this frame: the writes that put it in place for the
/// character's own display (address, value), undone right after it draws, and its model.
struct Rigged {
    writes: Vec<(usize, usize)>,
    root: usize,
    texlist: usize,
    scale: f32,
    height: f32,
}

fn rigged_writes(s: &mut Skins, character: u8, co2: usize) -> Option<Rigged> {
    let id = s.mine.clone()?;
    let info = s.list.iter().find(|i| i.id == id)?.clone();
    let rig = info.rig_for(character)?.clone();
    let (_, index) = ready(s, &id, Some(character))?;
    let loaded = &mut s.loaded[index];
    let read = |at: usize| -> usize {
        // SAFETY: game data the display itself reads (its CharObj2, texlists, model table).
        unsafe { (at as *const usize).read() }
    };
    let mut writes = Vec::new();

    // Textures: the character's own, then ours.
    let field = co2 + rig.field;
    let theirs = read(field);
    if theirs == 0 {
        return None;
    }
    let (their_entries, their_count) = (read(theirs), read(theirs + 4));
    let (our_entries, our_count) = (read(loaded.texlist), read(loaded.texlist + 4));
    if their_entries == 0 || our_entries == 0 || their_count > MAX_CHARACTER_TEXTURES {
        return None;
    }
    if loaded.combined_from != (theirs, loaded.texlist) {
        let mut entries = vec![0u8; (their_count + our_count.min(MAX_TEXTURES)) * TEXNAME_SIZE];
        // SAFETY: both lists have `count` 12-byte entries.
        unsafe {
            std::ptr::copy_nonoverlapping(
                their_entries as *const u8,
                entries.as_mut_ptr(),
                their_count * TEXNAME_SIZE,
            );
            std::ptr::copy_nonoverlapping(
                our_entries as *const u8,
                entries.as_mut_ptr().add(their_count * TEXNAME_SIZE),
                our_count.min(MAX_TEXTURES) * TEXNAME_SIZE,
            );
        }
        loaded.combined = entries.into_boxed_slice();
        loaded.combined_list[0] = loaded.combined.as_ptr() as usize as u32;
        loaded.combined_list[1] = (their_count + our_count.min(MAX_TEXTURES)) as u32;
        loaded.combined_from = (theirs, loaded.texlist);
        loaded.model.set_texture_base(their_count as u16);
    }
    writes.push((field, loaded.combined_list.as_ptr() as usize));

    // The body slots, and every slot pointing inside a body (hands, feet... that the bone
    // callbacks look for) at the same node of ours.
    for &slot in &rig.slots {
        let at = MODEL_TABLE + slot * 8;
        let body = read(at);
        if body == 0 {
            continue;
        }
        // SAFETY: a loaded model-table entry.
        let tree = unsafe { game_tree(body) };
        if tree.len() != loaded.model.nodes.len() {
            if s.failed.insert(format!("{id}:{slot}")) {
                log!(
                    "skins: '{}' has {} nodes, the body in slot {slot} {}: not drawn",
                    info.name,
                    loaded.model.nodes.len(),
                    tree.len()
                );
            }
            return None;
        }
        writes.push((at, loaded.model.root()));
        let index_of: std::collections::HashMap<usize, usize> =
            tree.iter().enumerate().map(|(i, &n)| (n, i)).collect();
        for other in (0..MODEL_COUNT).filter(|o| !rig.slots.contains(o)) {
            let a = MODEL_TABLE + other * 8;
            if let Some(&i) = index_of.get(&read(a))
                && i != 0
                && !info.keep_nodes.contains(&i)
            {
                writes.push((a, loaded.model.node(i)?));
            }
        }
    }
    // The character's upgrade parts (shoes, bracelets, claws...) are drawn from its upgrade
    // bits (CharObj2 +0x24) on top of the body: none on a skin. Only for the draw, so the
    // upgrades still work in play.
    writes.push((co2 + CO2_UPGRADES, 0));

    // Its size: the display scales the whole body by the entity's scale (+0x20, from the feet).
    let entity = game::local_entity() as usize;
    let scale = info.character_scale;
    if entity != 0 && scale != 1.0 {
        for axis in 0..3 {
            let at = entity + ENTITY_SCALE + axis * 4;
            // SAFETY: the player's EntityData1, read like the display reads it.
            let now = f32::from_bits(unsafe { (at as *const u32).read() });
            writes.push((at, (now * scale).to_bits() as usize));
        }
    }
    Some(Rigged {
        writes,
        root: loaded.model.root(),
        texlist: loaded.combined_list.as_ptr() as usize,
        scale,
        height: if rig.height > 0.0 {
            rig.height
        } else {
            info.height
        },
    })
}

/// Extra model-table slots a character's display may draw its body from besides its current
/// animation's (re/skins.md): Sonic's and Shadow's far-away and spin-ball bodies.
fn body_slots(character: u8) -> &'static [usize] {
    match character {
        0 => &[0, 31, 48, 6],
        1 => &[65, 104, 123, 84],
        4 => &[142],
        _ => &[],
    }
}

/// The body model its current animation draws (AnimationInfo at CharObj2 +0x198: 0x10 bytes
/// per animation, +2 the model number; the animation at +0x178).
fn current_body(co2: usize) -> Option<usize> {
    // SAFETY: the player's CharObj2 and its animation table, as the display reads them.
    unsafe {
        let table = ((co2 + game::CO2_ANIM_INFO) as *const usize).read();
        let anim = ((co2 + game::CO2_ANIM) as *const i16).read();
        if table == 0 || !(0..512).contains(&anim) {
            return None;
        }
        let model = ((table + anim as usize * 0x10 + 2) as *const i16).read();
        (0..MODEL_COUNT as i16)
            .contains(&model)
            .then_some(model as usize)
    }
}

/// Writes that hide every node of a character's body trees (NJS_OBJECT flag 8: the node is
/// placed and its callbacks run, but its mesh isn't drawn).
fn hidden_body(character: u8, co2: usize) -> Vec<(usize, usize)> {
    let mut writes = Vec::new();
    let mut slots: Vec<usize> = body_slots(character).to_vec();
    slots.extend(current_body(co2).filter(|m| !slots.contains(m)));
    for slot in slots {
        // SAFETY: the static model table; entries are null or loaded models.
        let root = unsafe { ((MODEL_TABLE + slot * 8) as *const usize).read() };
        if root == 0 {
            continue;
        }
        // SAFETY: a loaded model-table entry.
        for node in unsafe { game_tree(root) } {
            // SAFETY: a node of that tree.
            let flags = unsafe { (node as *const usize).read() };
            writes.push((node, flags | 8));
        }
    }
    writes
}

/// Makes `writes` (address, value), runs `f`, then puts every old value back in reverse order.
///
/// # Safety
/// Every address is live, writable game data of pointer size, and `f` doesn't keep them.
unsafe fn with_writes(writes: &[(usize, usize)], f: impl FnOnce()) {
    let saved: Vec<(usize, usize)> = writes
        .iter()
        .map(|&(at, value)| {
            // SAFETY: as the caller promises.
            unsafe {
                let old = (at as *const usize).read();
                (at as *mut usize).write(value);
                (at, old)
            }
        })
        .collect();
    f();
    for (at, old) in saved.into_iter().rev() {
        // SAFETY: as above.
        unsafe { (at as *mut usize).write(old) };
    }
}

fn call_display(display: usize, object: usize) {
    // SAFETY: the display this object had before ours, called as the game calls it.
    unsafe { std::mem::transmute::<usize, extern "C" fn(usize)>(display)(object) };
}

/// Another player wearing skin `skin` (from the puppets' display, game thread), drawn by its
/// own animator from what they're doing. Returns whether it drew; if not (a skin we don't
/// have, or one built on a character's skeleton) the character is drawn as usual.
pub fn draw_other(key: &str, skin: &str, pose: &crate::puppet::Pose) -> bool {
    use crate::net::protocol::{STATUS_GROUNDED, STATUS_HOLDING};
    with(|s| {
        if s.list.iter().any(|i| i.id == skin && i.rigged) {
            return false;
        }
        let Some((r, _)) = ready(s, skin, Some(pose.character)) else {
            return false;
        };
        let (anim, moves) = s
            .others
            .entry(key.to_string())
            .or_insert_with(|| (Animator::new(), Moves::new()));
        let ball = pose.flags & game::POSE_BALL != 0;
        let mut tilt = moves.step(ball, pose.rotation[1], r.height * r.scale);
        tilt.lean = (tilt.lean as f32 * r.lean) as i32;
        let motion = r.poses.map(|poses| {
            let doing = Doing {
                grounded: pose.status & STATUS_GROUNDED != 0,
                curled: ball,
                action: pose.action,
                holding: pose.status & STATUS_HOLDING != 0,
                progress: None,
                pace: r.pace,
            };
            let (role, frame) = anim.step(pose.position, doing, r.scale, poses.map(|(_, c)| c));
            if role == JUMP || role == FALL {
                tilt.spin = 0;
            }
            tilt.lean = 0;
            (poses[role].0, frame)
        });
        let mut rotation = pose.rotation;
        rotation[1] = rotation[1].wrapping_sub(r.turn);
        // SAFETY: display pass on the game thread; a checked, relocated model and motions.
        unsafe {
            crate::puppet::draw_solid(
                r.root,
                r.texlist,
                pose.position,
                rotation,
                r.scale,
                tilt,
                motion,
            )
        };
        true
    })
    .unwrap_or(false)
}

/// The player's display while a skin rides on them: the skin instead of the character.
extern "C" fn player_display(object: usize) {
    crate::watch::mark("skins: player display");
    // A rigged skin: the character's own display draws it, with the skin swapped in for the
    // call only, so the game's tables are never left changed.
    let rigged = std::panic::catch_unwind(|| {
        let player = game::local_player()?;
        let co2 = game::local_char_obj2() as usize;
        if co2 == 0 {
            return None;
        }
        with(|s| rigged_writes(s, player.character, co2)).flatten()
    })
    .ok()
    .flatten();
    let original = PLAYER_DISPLAY.load(Ordering::Relaxed);
    if let Some(rig) = rigged {
        if original == 0 {
            return;
        }
        // Nothing of the character shows but the skin's body: not its other models (wings,
        // gloves, mouths...), upgrades or effects.
        let through = if hide::can_let_through() { rig.root } else { 0 };
        let mut drawn = None;
        // SAFETY: the addresses come from `rigged_writes` (live game data) and are restored in
        // reverse order right after the display.
        unsafe {
            with_writes(&rig.writes, || {
                drawn = if through != 0 {
                    hide::hidden(through, || call_display(original, object))
                } else {
                    None
                };
                if drawn.is_none() {
                    call_display(original, object);
                }
            })
        };
        // The display drew another body instead (a spin ball, which is no skeleton of ours):
        // the skin's own, turning over as it goes.
        let curled = game::local_player().map(|p| (p.curled, p.position, p.rotation, p.scale));
        if let Some((curled, p, rotation, size)) = curled {
            let mut tilt = OUR_MOVES.lock().unwrap_or_else(|e| e.into_inner()).step(
                curled,
                rotation[1],
                rig.height * rig.scale * size,
            );
            if drawn == Some(false) {
                // The characters face -X: rolling forward is about Z.
                tilt.lean = tilt.spin;
                tilt.spin = 0;
                // SAFETY: display pass on the game thread; a checked, relocated model with the
                // texture list it was drawn with.
                unsafe {
                    crate::puppet::draw_solid(
                        rig.root,
                        rig.texlist,
                        [p.x, p.y, p.z],
                        rotation,
                        rig.scale * size,
                        tilt,
                        None,
                    )
                };
            }
        }
        return;
    }
    let drawn = std::panic::catch_unwind(|| {
        let player = game::local_player()?;
        let co2 = game::local_char_obj2() as usize;
        // A rigged skin that couldn't go in (above) isn't drawn as a solid one either.
        let Ready {
            root,
            poses,
            texlist,
            scale,
            turn,
            height,
            lean,
            pace,
        } = with(|s| {
            let rigged = s
                .list
                .iter()
                .any(|i| Some(&i.id) == s.mine.as_ref() && i.rigged);
            if rigged {
                None
            } else {
                ready_for(s, Some(player.character))
            }
        })
        .flatten()?;
        let mut rotation = player.rotation;
        let scale = scale * player.scale;
        let mut tilt = OUR_MOVES.lock().unwrap_or_else(|e| e.into_inner()).step(
            player.curled,
            rotation[1],
            height * scale,
        );
        tilt.lean = (tilt.lean as f32 * lean) as i32;
        rotation[1] = rotation[1].wrapping_sub(turn);
        // An animated skin plays its own animations; it flips only when it has no jump.
        let motion = poses.map(|poses| {
            let doing = Doing {
                grounded: player.grounded,
                curled: player.curled,
                action: player.action,
                holding: player.holding,
                progress: game::local_anim_progress(),
                pace,
            };
            let (role, frame) = OUR_ANIM.lock().unwrap_or_else(|e| e.into_inner()).step(
                [player.position.x, player.position.y, player.position.z],
                doing,
                scale,
                poses.map(|(_, c)| c),
            );
            if role == JUMP || role == FALL {
                tilt.spin = 0;
            }
            tilt.lean = 0;
            (poses[role].0, frame)
        });
        // The character's own display still runs, with every node of its body hidden: as it
        // draws it works out where the body, hands and feet are, and moves like the light
        // dash use that. Then the skin is drawn in its place.
        // Hidden at the renderer, nothing of the character is drawn (body, wings, upgrades,
        // effects: any character); otherwise its body nodes are, for the characters we know.
        let hide = hide::available();
        let mut writes = if co2 == 0 || original == 0 {
            Vec::new()
        } else if hide {
            vec![(co2 + CO2_UPGRADES, 0)]
        } else {
            let mut body = hidden_body(player.character, co2);
            if !body.is_empty() {
                body.push((co2 + CO2_UPGRADES, 0));
            }
            body
        };
        // Only with the character hidden, or it would show too.
        if !writes.is_empty() {
            // The hidden body at the skin's size, so what it holds (a Chao, a fruit) sits up
            // where the bigger skin's hands are, not at the character's.
            let entity = game::local_entity() as usize;
            let grow = scale / player.scale.max(0.01);
            if entity != 0 && grow.is_finite() && (grow - 1.0).abs() > 0.01 {
                for axis in 0..3 {
                    let at = entity + ENTITY_SCALE + axis * 4;
                    // SAFETY: the player's EntityData1, read like the display reads it.
                    let now = f32::from_bits(unsafe { (at as *const u32).read() });
                    writes.push((at, (now * grow).to_bits() as usize));
                }
            }
            // SAFETY: live game data, restored in reverse order right after the display.
            unsafe {
                with_writes(&writes, || {
                    if !hide || hide::hidden(0, || call_display(original, object)).is_none() {
                        call_display(original, object);
                    }
                })
            };
        }
        // SAFETY: display pass on the game thread; a checked, relocated model.
        let p = player.position;
        unsafe {
            crate::puppet::draw_solid(
                root,
                texlist,
                [p.x, p.y, p.z],
                rotation,
                scale,
                tilt,
                motion,
            )
        };
        Some(())
    });
    if !matches!(drawn, Ok(Some(()))) {
        // Not ready (or gone): the character as usual.
        let original = PLAYER_DISPLAY.load(Ordering::Relaxed);
        if original != 0 {
            // SAFETY: the display this object had before ours.
            unsafe { std::mem::transmute::<usize, extern "C" fn(usize)>(original)(object) };
        }
    }
}

/// Our kart skin for this race, if any: (root object, texlist).
pub fn kart() -> Option<(usize, usize)> {
    with(|s| {
        let id = s.mine.clone()?;
        if !s.list.iter().any(|i| i.id == id && i.kart) {
            return None;
        }
        ready_for(s, None).map(|r| (r.root, r.texlist))
    })
    .flatten()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A one-triangle model as tools/skinconv.py writes it.
    fn tiny_model() -> Vec<u8> {
        let mut b = vec![0u8; 16];
        let put = |b: &mut Vec<u8>, v: &[u8]| {
            let at = b.len();
            b.extend_from_slice(v);
            at
        };
        // Vertex block: 3 vertices.
        let mut v = Vec::new();
        v.extend_from_slice(&(0x29u32 | (19 << 16)).to_le_bytes());
        v.extend_from_slice(&(3u32 << 16).to_le_bytes());
        for i in 0..3 {
            for f in [i as f32, 0.0, 1.0, 0.0, 1.0, 0.0] {
                v.extend_from_slice(&f32::to_le_bytes(f));
            }
        }
        v.extend_from_slice(&0xFFu32.to_le_bytes());
        let vlist = put(&mut b, &v);
        // Polygons: material, texture 0, one strip of 3, end.
        let mut p = Vec::new();
        for h in [0x2511u16, 2] {
            p.extend_from_slice(&h.to_le_bytes());
        }
        p.extend_from_slice(&0xFFFF_FFFFu32.to_le_bytes());
        for h in [0x0408u16, 0x4000] {
            p.extend_from_slice(&h.to_le_bytes());
        }
        for h in [0x1641u16, 12, 1, 3, 0, 0, 0, 1, 256, 0, 2, 0, 256, 0] {
            p.extend_from_slice(&h.to_le_bytes());
        }
        p.extend_from_slice(&0xFFu16.to_le_bytes());
        while p.len() % 4 != 0 {
            p.push(0);
        }
        let plist = put(&mut b, &p);
        let mut m = Vec::new();
        for w in [vlist as u32, plist as u32] {
            m.extend_from_slice(&w.to_le_bytes());
        }
        m.extend_from_slice(&[0u8; 16]);
        let model = put(&mut b, &m);
        let mut o = Vec::new();
        o.extend_from_slice(&0x17u32.to_le_bytes());
        o.extend_from_slice(&(model as u32).to_le_bytes());
        for f in [0.0f32, 0.0, 0.0] {
            o.extend_from_slice(&f.to_le_bytes());
        }
        o.extend_from_slice(&[0u8; 12]);
        for f in [1.0f32, 1.0, 1.0] {
            o.extend_from_slice(&f.to_le_bytes());
        }
        o.extend_from_slice(&[0u8; 8]);
        let root = put(&mut b, &o);
        b[..8].copy_from_slice(&SA2MDL.to_le_bytes());
        b[8..12].copy_from_slice(&(root as u32).to_le_bytes());
        b
    }

    /// A vertex list of one weighted block (status, offset, entries (slot, weight)), checked.
    fn weighted(status: u32, offset: u32, entries: &[(u32, u32)]) -> Result<usize, String> {
        let mut b = vec![0u8; 16];
        let n = entries.len() as u32;
        b.extend_from_slice(&(0x2C | status << 8 | (1 + 7 * n) << 16).to_le_bytes());
        b.extend_from_slice(&(offset | n << 16).to_le_bytes());
        for &(slot, weight) in entries {
            for f in [1.0f32, 2.0, 3.0, 0.0, 1.0, 0.0] {
                b.extend_from_slice(&f.to_le_bytes());
            }
            b.extend_from_slice(&(slot | weight << 16).to_le_bytes());
        }
        b.extend_from_slice(&0xFFu32.to_le_bytes());
        let u32_at = |at: usize| {
            b.get(at..at + 4)
                .map(|x| u32::from_le_bytes([x[0], x[1], x[2], x[3]]))
        };
        let in_range =
            |at: usize, size: usize| at.is_multiple_of(4) && at >= 16 && at + size <= b.len();
        check_vertices(&u32_at, &in_range, 16)
    }

    #[test]
    fn weighted_vertices_are_checked() {
        // Two shares of slot 10 and one of 8191 (the last there is), counted from offset 0.
        assert_eq!(weighted(0, 0, &[(10, 128), (8191, 255)]), Ok(8192));
        assert_eq!(weighted(2, 5, &[(10, 127)]), Ok(16));
        // A weight status that isn't start, middle or end; a slot past the buffer; a weight
        // above 255.
        assert!(weighted(3, 0, &[(1, 255)]).is_err());
        assert!(weighted(1, 5000, &[(4000, 255)]).is_err());
        assert!(weighted(1, 0, &[(1, 256)]).is_err());
    }

    #[test]
    fn a_good_model_loads_and_is_relocated() {
        let bytes = tiny_model();
        let m = load_model(&bytes, 1).unwrap();
        // Pointers are 32-bit (the game's); compare within that width so this runs anywhere.
        let base = m.words.as_ptr() as usize as u32;
        let root = m.words[2].wrapping_sub(base) as usize;
        assert!(root >= 16 && root < bytes.len());
    }

    #[test]
    fn broken_models_are_refused() {
        let good = tiny_model();
        // Wrong magic.
        let mut b = good.clone();
        b[0] ^= 1;
        assert!(load_model(&b, 1).is_err());
        // A texture the skin doesn't have.
        assert!(load_model(&good, 0).is_err() || load_model(&good, 1).is_ok());
        // Root pointing off the end.
        let mut b = good.clone();
        b[8..12].copy_from_slice(&(good.len() as u32).to_le_bytes());
        assert!(load_model(&b, 1).is_err());
        // A strip index past the vertices: patch the first index of the strip (value 0 -> 9).
        let mut b = good.clone();
        let strip = b.windows(4).position(|w| w == [0x41, 0x16, 12, 0]).unwrap();
        b[strip + 8] = 9;
        assert!(load_model(&b, 1).is_err());
        // Truncated.
        assert!(load_model(&good[..good.len() - 8], 1).is_err());
    }

    /// A real converted skin, when there's one locally (skins/ isn't in the repo).
    #[test]
    fn converted_skins_load() {
        let Ok(dirs) = std::fs::read_dir(concat!(env!("CARGO_MANIFEST_DIR"), "/skins")) else {
            return;
        };
        for d in dirs.flatten() {
            let Ok(ini) = std::fs::read_to_string(d.path().join("skin.ini")) else {
                continue;
            };
            let info = Info::parse("x", &ini).unwrap();
            let files: Vec<String> = if info.rigged {
                assert!(!info.rigs.is_empty(), "{}", d.path().display());
                info.rigs.iter().map(|r| r.file.clone()).collect()
            } else {
                vec!["model.sa2mdl".into()]
            };
            for f in files {
                let bytes = std::fs::read(d.path().join(&f)).unwrap();
                let model = match load_model(&bytes, info.textures) {
                    Ok(m) => m,
                    Err(e) => panic!("{}/{f}: {e}", d.path().display()),
                };
                if info.animated {
                    let bytes = std::fs::read(d.path().join("motions.sa2mot")).unwrap();
                    let motions = load_motions(&bytes, model.nodes.len())
                        .unwrap_or_else(|e| panic!("{}: {e}", d.path().display()));
                    assert!(info.roles.iter().flatten().all(|&r| r < motions.list.len()));
                    // The wrong model's node count is refused.
                    assert!(load_motions(&bytes, model.nodes.len() + 4000).is_err());
                }
            }
        }
    }

    #[test]
    fn broken_motions_are_refused() {
        // One motion of 2 frames for 1 node, with one angle key.
        let mut b = Vec::new();
        b.extend_from_slice(SA2COMOT);
        b.extend_from_slice(&1u32.to_le_bytes());
        b.extend_from_slice(&60u32.to_le_bytes()); // table
        // 16: key (frame 0, angles)
        b.extend_from_slice(&[0u8; 16]);
        // 32: mdata [pos 0, ang 16, 0, 1]
        for v in [0u32, 16, 0, 1] {
            b.extend_from_slice(&v.to_le_bytes());
        }
        // 48: motion
        b.extend_from_slice(&32u32.to_le_bytes());
        b.extend_from_slice(&2u32.to_le_bytes());
        b.extend_from_slice(&3u16.to_le_bytes());
        b.extend_from_slice(&2u16.to_le_bytes());
        // 60: table
        b.extend_from_slice(&48u32.to_le_bytes());
        assert!(load_motions(&b, 1).is_ok());
        assert!(load_motions(&b, 2).is_err());
        let mut bad = b.clone();
        bad[16] = 5; // key frame past the end
        assert!(load_motions(&bad, 1).is_err());
        let mut bad = b.clone();
        bad[44] = 9; // more keys than there are
        assert!(load_motions(&bad, 1).is_err());
        let mut bad = b.clone();
        bad[56] = 1; // positions only
        assert!(load_motions(&bad, 1).is_err());
        assert!(load_motions(&b[..60], 1).is_err());
    }

    #[test]
    fn animations_follow_the_movement() {
        let mut a = Animator::new();
        let frames = cycles(&[100, 30, 20, 40, 20], &[]);
        let mut x = 0.0;
        let mut go = |a: &mut Animator, dx: f32, grounded: bool, y: f32| {
            x += dx;
            a.step([x, y, 0.0], on(grounded), 1.0, frames).0
        };
        assert_eq!(go(&mut a, 0.0, true, 0.0), IDLE);
        for _ in 0..20 {
            go(&mut a, 1.0, true, 0.0);
        }
        assert_eq!(go(&mut a, 1.0, true, 0.0), WALK);
        for _ in 0..20 {
            go(&mut a, 5.0, true, 0.0);
        }
        assert_eq!(go(&mut a, 5.0, true, 0.0), RUN);
        assert_eq!(go(&mut a, 5.0, false, 1.0), JUMP);
        for _ in 0..100 {
            go(&mut a, 5.0, false, 1.0);
        }
        assert_eq!(go(&mut a, 5.0, false, 0.0), FALL);
        // A teleport isn't running.
        let mut b = Animator::new();
        b.step([0.0; 3], on(true), 1.0, frames);
        assert_eq!(b.step([500.0, 0.0, 0.0], on(true), 1.0, frames).0, IDLE);
        // No walk animation: it runs instead.
        let mut c = Animator::new();
        for i in 0..20 {
            c.step(
                [i as f32, 0.0, 0.0],
                on(true),
                1.0,
                cycles(&[100, 0, 20, 40, 20], &[]),
            );
        }
        let none = cycles(&[100, 0, 20, 40, 20], &[]);
        assert_eq!(c.step([20.0, 0.0, 0.0], on(true), 1.0, none).0, RUN);
    }

    /// The first roles' animations (the rest none).
    fn cycles(frames: &[u32], strides: &[f32]) -> [Cycle; ROLES] {
        std::array::from_fn(|i| Cycle {
            frames: frames.get(i).copied().unwrap_or(0),
            stride: strides.get(i).copied().unwrap_or(0.0),
        })
    }

    fn on(grounded: bool) -> Doing {
        Doing {
            grounded,
            ..Doing::default()
        }
    }

    #[test]
    fn garden_actions_play_their_animations() {
        let mut all = [30u32; ROLES];
        all[WALK] = 0;
        let c = cycles(&all, &[]);
        let mut a = Animator::new();
        let doing = |action, holding| Doing {
            grounded: true,
            action,
            holding,
            ..Doing::default()
        };
        assert_eq!(a.step([0.0; 3], doing(43, false), 1.0, c).0, PET);
        assert_eq!(a.step([0.0; 3], doing(35, true), 1.0, c).0, SHAKE);
        assert_eq!(a.step([0.0; 3], doing(42, false), 1.0, c).0, WHISTLE);
        assert_eq!(a.step([0.0; 3], doing(41, false), 1.0, c).0, PICKUP);
        // Holding something: carrying, standing or moving.
        assert_eq!(a.step([0.0; 3], doing(0, true), 1.0, c).0, CARRY);
        let mut x = 0.0;
        let mut role = 0;
        for _ in 0..30 {
            x += 0.5;
            role = a.step([x, 0.0, 0.0], doing(1, true), 1.0, c).0;
        }
        assert_eq!(role, CARRY_WALK);
        // A pick-up plays once and holds its last frame.
        let mut b = Animator::new();
        let mut f = 0.0;
        for _ in 0..200 {
            f = b.step([0.0; 3], doing(41, false), 1.0, c).1;
        }
        assert_eq!(f, 29.0);
        // A skin without the garden ones: as it moves.
        let plain = cycles(&[100, 30, 20, 40, 20], &[]);
        let mut p = Animator::new();
        assert_eq!(p.step([0.0; 3], doing(43, false), 1.0, plain).0, IDLE);
        assert_eq!(p.step([0.0; 3], doing(0, true), 1.0, plain).0, IDLE);
    }

    #[test]
    fn walks_and_runs_keep_pace() {
        // Walk: 10 units a 41-frame cycle = 0.125 a game frame at its own pace; run: 20 units
        // a 21-frame cycle = 0.5. Running starts halfway (0.3125).
        let c = cycles(&[100, 41, 21, 40, 20], &[0.0, 10.0, 20.0]);
        let mut a = Animator::new();
        let mut x = 0.0;
        let mut go = |a: &mut Animator, dx: f32| {
            x += dx;
            a.step([x, 0.0, 0.0], on(true), 1.0, c)
        };
        for _ in 0..30 {
            go(&mut a, 0.25);
        }
        // Walking at twice its pace: a frame a game frame.
        let (role, f0) = go(&mut a, 0.25);
        let (_, f1) = go(&mut a, 0.25);
        assert_eq!(role, WALK);
        assert!(((f1 - f0).rem_euclid(40.0) - 1.0).abs() < 0.01, "{f0} {f1}");
        for _ in 0..30 {
            go(&mut a, 1.0);
        }
        // Running at twice its pace.
        let (role, f0) = go(&mut a, 1.0);
        let (_, f1) = go(&mut a, 1.0);
        assert_eq!(role, RUN);
        assert!(((f1 - f0).rem_euclid(20.0) - 1.0).abs() < 0.01, "{f0} {f1}");
        // Fast as Sonic: as fast as it goes, not more.
        for _ in 0..30 {
            go(&mut a, 30.0);
        }
        let (_, f0) = go(&mut a, 30.0);
        let (_, f1) = go(&mut a, 30.0);
        assert!(((f1 - f0).rem_euclid(20.0) - Pace::default().max_rate * ANIM_STEP).abs() < 0.01);
    }

    #[test]
    fn flips_while_curled_and_lands_upright() {
        let mut m = Moves::new();
        for _ in 0..7 {
            m.step(true, 0, 5.0);
        }
        assert_ne!(m.step(true, 0, 5.0).spin, 0);
        // Let go mid-flip: it finishes the turn and stays upright.
        let mut last = 1;
        for _ in 0..FLIP_FRAMES {
            last = m.step(false, 0, 5.0).spin;
        }
        assert_eq!(last, 0);
        // Turning leans, then settles when it stops.
        let lean = (0..10)
            .map(|i| m.step(false, i * 400, 5.0).lean)
            .last()
            .unwrap();
        assert!(lean > 1000, "{lean}");
        let settled = (0..60)
            .map(|_| m.step(false, 3600, 5.0).lean)
            .last()
            .unwrap();
        assert!(settled.abs() < 50, "{settled}");
    }

    #[test]
    fn rigged_skin_ini() {
        let i = Info::parse(
            "hero",
            "name=Space Hero\nrigged=on\nrig_sonic=0,31,48;0x394\nrig_knuckles=142;0x3f0\nrig_nobody=1;2\nrig_eggman=230,527;0x360;14.5\ntextures=3\n",
        )
        .unwrap();
        assert_eq!(i.characters, vec![0, 4, 3]);
        let k = i.rig_for(4).unwrap();
        assert_eq!(
            (k.file.as_str(), k.slots.as_slice(), k.field, k.height),
            ("model_knuckles.sa2mdl", &[142][..], 0x3F0, 0.0)
        );
        let e = i.rig_for(3).unwrap();
        assert_eq!(
            (e.slots.as_slice(), e.field, e.height),
            (&[230, 527][..], 0x360, 14.5)
        );
        // Rigged without any usable rig: refused.
        assert!(Info::parse("x", "rigged=on\nrig_sonic=;0x394\n").is_none());
    }

    #[test]
    fn skin_ini() {
        let i = Info::parse(
            "car",
            "name=Race Car\ncharacters=sonic, 1\ncharacter_scale=0.6\nturn=90\nkart=on\ntextures=5\n",
        )
        .unwrap();
        assert_eq!(i.characters, vec![0, 1]);
        assert_eq!(i.turn, 0x4000);
        assert!(i.kart);
        assert_eq!(i.archive(), "SKIN_CAR");
        let a = Info::parse(
            "anim",
            "animated=on\nmotions=Idle:100,Run:22\nstrides=0,15.8,nan,-3\nidle=0\nrun=1\n",
        )
        .unwrap();
        assert!(a.animated);
        assert_eq!(a.roles[RUN], Some(1));
        assert_eq!(a.strides, vec![0.0, 15.8, 0.0, 0.0]);
        let paced = Info::parse(
            "x",
            "anim_walk_from=0.3\nanim_max_rate=3\nanim_run_speed=-1\n",
        )
        .unwrap();
        assert_eq!(paced.pace.walk_from, 0.3);
        assert_eq!(paced.pace.max_rate, 3.0);
        // Bad values keep the default.
        assert_eq!(paced.pace.run_speed, Pace::default().run_speed);
    }
}
