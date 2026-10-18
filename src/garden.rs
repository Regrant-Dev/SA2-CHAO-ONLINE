//! Live shared gardens (re/chao-garden-sync.md, re/chao-garden-items.md).
//!
//! - Each player's game reads their visiting Chao's live objects (position, facing, animation,
//!   held) and sends them ~30 times a second.
//! - In the other games the visitor copy follows that: its AI is off and its MainSub is wrapped
//!   so it stands where the real one is, smoothed on the ground or in the owner's (puppet's)
//!   hands while held. Visitors whose owner sends nothing (an older mod) keep their own AI.
//! - Touching a friend's Chao (petting, picking it up, hitting it, or holding a fruit, animal,
//!   drive or hat near it) switches the copy's own AI on for a few seconds, so the game plays the
//!   real reaction. Whatever that changes in the copy's record (happiness, alignment, bond,
//!   stats, lifespan, animal parts) goes to the owner as differences, and the owner's game adds
//!   them to the real Chao (`chao::changes`).
//! - While you hold a friend's Chao, their game shows their real one in your (puppet's) hands.
//!
//! One brain (keeper.rs): with friends in the garden, one player keeps it. The keeper's game
//! runs every Chao there with its own AI, visitors too, so they meet, play and fight for real;
//! it sends them all, and sends what happens to a visitor to its owner, whose save gets it.
//! Everyone else's game shows all of them following the keeper, their own visitors included
//! (their real records still take every change). Touching a Chao still plays out at once where
//! you are, as above, and what it changes goes to its owner and the keeper. A hold goes to the
//! keeper. Until someone keeps the garden (the first moment friends meet), each owner's game
//! runs its own as before.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use crate::chao::{self, CHAO_SIZE, ChaoData};
use crate::game::{self, Vector};
use crate::log;
use crate::net::protocol::GardenChao;

/// Chao-world object registry: 0x48-byte entries, 0x1200 bytes per category; an entry's record
/// pointer is at +8 and its ObjectMaster at +0x3C (objects point back to their entry at +0x3C).
const WORLD: usize = 0x01DC_0FC0;
const WORLD_ENTRY_SIZE: usize = 0x48;
const WORLD_CATEGORY_SIZE: usize = 0x1200;
const WORLD_CHAO_MAX: usize = 32;
/// Item categories a Chao takes from the player: animals and drives, fruit, hats.
const ITEM_CATEGORIES: [usize; 3] = [2, 3, 9];
const CHAO_SLOTS: usize = 0x019F_6EC4;

const OBJ_MAIN: usize = 0x10;
const OBJ_DATA1: usize = 0x34;
/// Motion/velocity work (+0/+4/+8 velocity).
const OBJ_DATA2: usize = 0x38;
const OBJ_ENTRY: usize = 0x3C;
/// The garden Chao MainSub (set by CreateChao).
const CHAO_MAIN: usize = 0x0054_FE20;

const D_STATUS: usize = 0x04;
const D_ROTATION_Y: usize = 0x0C;
const D_POSITION: usize = 0x14;
/// The Chao's record (`ChaoData*`).
const D_RECORD: usize = 0x5C;
const D_FLAGS: usize = 0xA0;
const D_MOTION: usize = 0xAC;
const D_MOTION_CURRENT: usize = 0xB0;
const D_MOTION_NEXT: usize = 0xB4;
/// Appearance flags: bit 2 = redraw parts/colours after a change.
const D_APPEARANCE: usize = 0x674;
/// Mood display (heart, "?", "!", swirl, ...), a small state machine at Data1+0x6EC: i16 current
/// state (0..=5, function table 0x1353834), i16 requested next (-1 none; 0x53D660 and friends
/// set it, and the next update switches to it), i32 timer, i32 length. Switching clears the
/// request and the state's animation counters (u16 at +0x38, +0x3C, +0x7C, +0x80; 0x53CA88).
const D_MOOD: usize = 0x6EC;
const D_MOOD_REQUEST: usize = 0x6EE;
const D_MOOD_TIMER: usize = 0x6F0;
const D_MOOD_LENGTH: usize = 0x6F4;
const MOOD_COUNTERS: [usize; 4] = [0x38, 0x3C, 0x7C, 0x80];
const MOOD_STATES: i16 = 6;
/// Face: eyes and mouth ids (set by 0x53A4B0 / 0x53A5A0).
const D_EYES: usize = 0x692;
const D_MOUTH: usize = 0x69E;
/// Eye/mouth ids we accept from the network (they index the face texture tables).
const MAX_FACE_ID: i16 = 32;
/// Motion controller (Data1+0xAC): +0x2C current NJS_MOTION*, +0x48 the motion table
/// (0x1C-byte entries: motion*, u16 play mode, ...).
const MC_MOTION: usize = 0x2C;
const MC_TABLE: usize = 0x48;
const MOTION_ENTRY_SIZE: usize = 0x1C;
/// Motion ids we accept from the network (the shared table has a few hundred).
const MAX_MOTION_ID: u16 = 0x400;

/// Data1 status bits a player sets: hit, held. (Bit 0x1, which the pet trigger also checks, is
/// set on every Chao, so being petted can't be told from it: being right next to the Chao
/// counts instead, and the copy's own AI then plays the petting.)
const STATUS_HIT: u16 = 0x0004;
const STATUS_HELD: u16 = 0x8000;
/// Data1+0xA0: AI runs.
const FLAG_AI: u32 = 0x10;
/// Motion controller flags: blending into the next motion.
const MOTION_BLENDING: u16 = 0x2;
/// "Held, calm" motion.
const MOTION_HELD: u16 = 0x101;

/// The local player's CharObj2 pointer; +0x94 is the object they hold.
const PLAYER_CHAR_OBJ2: usize = 0x01DE_9600;
const PLAYER_HELD: usize = 0x94;

/// Start a motion (blends): EAX = Data1+0xAC, EDX = id.
#[cfg(target_arch = "x86")]
const START_MOTION: usize = 0x0079_3C40;
#[cfg(target_arch = "x86")]
const SET_EYES: usize = 0x0053_A4B0;
#[cfg(target_arch = "x86")]
const SET_MOUTH: usize = 0x0053_A5A0;

/// After one of our visitor rows changes Chao, friends' changes for it are ignored this long:
/// their copy goes on as the old Chao until it leaves (`LEFT`) or is remade from the new one.
const ROW_CHANGE_QUIET: Duration = Duration::from_secs(10);

/// How fast a mirrored Chao catches up with where its owner's is (per frame).
const FOLLOW: f32 = 0.25;
/// The owner's updates count as paused after this long (the copy then holds still).
const STALE: Duration = Duration::from_secs(2);
/// A touched copy runs its own AI this long after the last touch or change.
const LOCAL_TIME: Duration = Duration::from_secs(5);
/// "Holding an item near it": within this distance.
const ITEM_NEAR: f32 = 25.0;
/// Standing this close to a friend's Chao lets it react to us (petting).
const PLAYER_NEAR: f32 = 12.0;
/// An item this close to a touched Chao is in its hands (eating, holding).
const BUSY_NEAR: f32 = 6.0;
/// The keeper sends what its visitors' AI changed this often (frames).
const KEPT_CHANGES_EVERY: u32 = 30;
/// Record: lifespan left (u16). At 0 the Chao's end of life starts (0x562A20 starts 0x568F60,
/// then death or reincarnation as an egg made from its own record, 0x568570); a visitor the
/// keeper runs is kept above it, so that only ever happens in its owner's game.
const C_LIFESPAN: usize = 0x8A;
/// A friend's Chao whose owner stopped sending it this long ago (visits off, left the garden,
/// gone) leaves our garden. Long enough to ride out a hiccup.
const LEFT: Duration = Duration::from_secs(8);
/// Setting an object's MainSub to DeleteObject removes it on the next object pass.
const DELETE_OBJECT: usize = 0x0046_F720;
/// A friend holding our Chao: their updates count this long.
const HELD_STALE: Duration = Duration::from_millis(500);

/// `chao::changes::diff` output: (offset, delta or value).
type Changes = Vec<(u16, f32)>;
/// To send: (owner, index, changes) and (owner, index, offset in our frame).
type Outgoing = (Vec<(u8, u8, Changes)>, Vec<(u8, u8, [f32; 3])>);

struct Mirror {
    object: usize,
    /// One of our own Chao following the keeper (never deleted by us; its record is our save's).
    own: bool,
    /// Latest state from the Chao's brain (the keeper, or its owner), when it came and who sent it.
    target: Option<(GardenChao, Instant, u8)>,
    /// The motion we last started on it.
    motion: Option<u16>,
    /// Taken over (AI off, MainSub wrapped).
    driven: bool,
    /// The copy's record as last forwarded.
    snapshot: Option<Box<ChaoData>>,
    /// Running its own AI (touched) until then.
    local_until: Option<Instant>,
    /// Created then.
    since: Instant,
}

/// Our Chao held by a friend.
struct Held {
    holder: u8,
    offset: [f32; 3],
    at: Instant,
}

struct Garden {
    /// (owner, index) -> our visitor object for it.
    mirrors: HashMap<(u8, u8), Mirror>,
    /// Where each other player is drawn now: (position, facing).
    holders: HashMap<u8, (Vector, i32)>,
    /// Our visiting Chao: (index, save slot).
    mine: Vec<(u8, usize)>,
    /// Chao whose brain is our game that a friend holds: (owner, index).
    held: HashMap<(u8, u8), Held>,
    /// Our Chao objects whose MainSub we wrapped, and whether we switched their AI off.
    wrapped: HashMap<usize, bool>,
    /// To send: changes to friends' Chao, and friends' Chao we hold.
    changes_out: Vec<(u8, u8, Changes)>,
    held_out: Vec<(u8, u8, [f32; 3])>,
    /// Received: changes to Chao whose record we keep: ours, or visitors we run (owner, index).
    changes_in: Vec<(u8, u8, Changes)>,
    frame: u32,
    /// When each friend's Chao (owner, index) was last in their updates.
    heard: HashMap<(u8, u8), Instant>,
    /// Friends' Chao that left our garden, for `take_left`.
    left: Vec<(u8, u8)>,
    /// Friends' Chao sent again (their look changed): (owner, index) -> their data.
    looks_in: HashMap<(u8, u8), Box<ChaoData>>,
    /// Our visitor rows that now carry a different Chao, and when: friends' changes for a row
    /// are ignored for a while after, since their copy may still be the old Chao.
    rows_changed: HashMap<u8, Instant>,
}

static GARDEN: Mutex<Option<Garden>> = Mutex::new(None);

fn with<R>(f: impl FnOnce(&mut Garden) -> R) -> R {
    let mut g = GARDEN.lock().unwrap_or_else(|e| e.into_inner());
    f(g.get_or_insert_with(|| Garden {
        mirrors: HashMap::new(),
        holders: HashMap::new(),
        mine: Vec::new(),
        held: HashMap::new(),
        wrapped: HashMap::new(),
        changes_out: Vec::new(),
        held_out: Vec::new(),
        changes_in: Vec::new(),
        frame: 0,
        heard: HashMap::new(),
        left: Vec::new(),
        looks_in: HashMap::new(),
        rows_changed: HashMap::new(),
    }))
}

/// Visitors: we created `object` for player `owner`'s Chao `index`.
pub fn spawned(owner: u8, index: u8, object: usize) {
    if object != 0 {
        with(|g| {
            g.mirrors.insert(
                (owner, index),
                Mirror {
                    object,
                    own: false,
                    target: None,
                    motion: None,
                    driven: false,
                    snapshot: None,
                    local_until: None,
                    since: Instant::now(),
                },
            )
        });
    }
}

/// Visitors: we left the garden and the game deleted the Chao objects.
/// Forgets this garden: its Chao objects are gone (we left, or went to another garden), and
/// nothing queued for them may reach the next garden's.
pub fn clear() {
    with(|g| {
        g.mirrors.clear();
        g.held.clear();
        g.wrapped.clear();
        g.changes_out.clear();
        g.held_out.clear();
        g.changes_in.clear();
        g.heard.clear();
        g.left.clear();
        g.looks_in.clear();
        g.rows_changed.clear();
    });
}

/// The Chao we brought to this garden: (`Body::Chao` index, save slot).
pub fn set_mine(slots: Vec<(u8, usize)>) {
    with(|g| {
        let now = Instant::now();
        for &(index, before) in &g.mine.clone() {
            let after = slots.iter().find(|(i, _)| *i == index).map(|&(_, s)| s);
            if after != Some(before) {
                g.rows_changed.insert(index, now);
                // Whoever held the old one isn't holding this one, and the old one's follower
                // (when it's ours) goes.
                g.held.retain(|&(_, i), _| i != index);
                if let Some(me) = crate::keeper::me() {
                    release_own(g, (me, index));
                }
            }
        }
        g.mine = slots;
    });
}

/// Network: player `owner` left (or we lost everyone: `None`). Their Chao go home now rather
/// than after `LEFT`, so a new player given the same id never drives the old copies.
pub fn player_left(owner: Option<u8>) {
    let Some(long_ago) = Instant::now().checked_sub(LEFT * 2) else {
        return;
    };
    with(|g| {
        for (&(o, index), m) in g.mirrors.iter_mut() {
            if !m.own && owner.is_none_or(|id| id == o) {
                m.since = long_ago;
                g.heard.insert((o, index), long_ago);
            }
        }
    });
}

/// Network: player `from` sent player `owner`'s visiting Chao as they are in `from`'s game.
/// An owner's own say they're visiting; the ones the Chao follow come from the keeper (or, with
/// nobody keeping, from the owner).
pub fn received(from: u8, owner: u8, list: Vec<GardenChao>) {
    let now = Instant::now();
    let keeper = crate::keeper::current();
    let brain = keeper == Some(from) || (keeper.is_none() && from == owner);
    with(|g| {
        for c in list {
            if from == owner {
                g.heard.insert((owner, c.index), now);
            }
            if brain && let Some(m) = g.mirrors.get_mut(&(owner, c.index)) {
                m.target = Some((c, now, from));
            }
        }
    });
}

/// Friend `owner`'s Chao `index` is visiting: their game sends it (or just started to).
pub fn visiting(owner: u8, index: u8) -> bool {
    with(|g| {
        g.heard
            .get(&(owner, index))
            .is_some_and(|at| at.elapsed() < LEFT)
    })
}

/// Friends' Chao visiting us, for their name tags: (where, owner, index, the Chao's name if it
/// has one).
pub fn visitors() -> Vec<([f32; 3], u8, u8, Option<String>)> {
    with(|g| {
        g.mirrors
            .iter()
            .filter(|(_, m)| !m.own)
            .filter_map(|(&(owner, index), m)| {
                // SAFETY: our visitor's live object (cleared when we leave the garden); its
                // record is our own visitor buffer.
                unsafe {
                    let d = data1(m.object);
                    if d == 0 {
                        return None;
                    }
                    let p = ((d + D_POSITION) as *const Vector).read_unaligned();
                    let record = ((d + D_RECORD) as *const usize).read();
                    let name = (record != 0)
                        .then(|| crate::karate::chao_name(&(record as *const ChaoData).read()))
                        .filter(|n| n != "Chao");
                    Some(([p.x, p.y, p.z], owner, index, name))
                }
            })
            .collect()
    })
}

/// Body points behaviors attach toys to (`Data1+0x514 + 4*node` points at the node).
pub const PROP_NODES: [usize; 3] = [4, 11, 35];
const D_NODES: usize = 0x514;
/// Node fields: attached model, texture list, shown.
const N_MODEL: usize = 0x98;
const N_SHOWN: usize = 0xA4;
/// Attach `model` with the toy textures (0x5669B0: EAX = Chao object, ECX = node, EDX = model)
/// or a parameter block {model, texlist, 5 floats} (0x566A20: EDX = block).
const ATTACH_MODEL: usize = 0x0056_69B0;
const ATTACH_BLOCK: usize = 0x0056_6A20;

/// What a toy is in the game: a model attached directly, or through a parameter block.
#[derive(Clone, Copy)]
enum Prop {
    Model(usize),
    Block { model: usize, block: usize },
}

impl Prop {
    fn model(self) -> usize {
        match self {
            Self::Model(m) | Self::Block { model: m, .. } => m,
        }
    }
}

/// Every toy and instrument behaviors put in a Chao's hands (the 15 callers of 0x5669B0 and
/// the 7 of 0x566A20; two blocks whose model is set at run time are left out). Indices travel
/// over the network (`GardenChao::props`), never addresses.
const PROPS: [Prop; crate::net::protocol::MAX_CHAO_PROP as usize] = [
    Prop::Model(0x012F_E33C),
    Prop::Model(0x012F_E574),
    Prop::Model(0x012F_BC5C),
    Prop::Model(0x012F_A174),
    Prop::Model(0x012F_6D54),
    Prop::Model(0x012F_8364),
    Prop::Model(0x012F_B41C),
    Prop::Model(0x011C_505C),
    Prop::Model(0x011C_54F4),
    Prop::Model(0x011C_6674),
    Prop::Model(0x011C_6A14),
    Prop::Model(0x011C_62C4),
    Prop::Model(0x011C_8964),
    Prop::Model(0x011C_8FFC),
    Prop::Model(0x011C_9AE4),
    Prop::Block {
        model: 0x011C_5B3C,
        block: 0x011C_9B54,
    },
    Prop::Block {
        model: 0x011C_73A4,
        block: 0x011C_9B74,
    },
    Prop::Block {
        model: 0x011C_7A9C,
        block: 0x011C_9B94,
    },
    Prop::Block {
        model: 0x012F_CD74,
        block: 0x011B_B93C,
    },
    Prop::Block {
        model: 0x012F_D044,
        block: 0x011B_B95C,
    },
];

/// The drum: `Data1+0x660` model, +0x664 texture list, +0x668 size (it grows in and out),
/// +0x66C (1.0), set by the drum behavior (0x55E4F9) and drawn by 0x540E68.
const D_DRUM: usize = 0x660;
const DRUM_MODEL: usize = 0x012F_D9DC;
const TOY_TEXLIST: usize = 0x0136_6AFC;

/// A node pointer of this Chao's Data1 (0 when the Chao has none there).
///
/// # Safety
/// `d` is a live Chao's Data1.
unsafe fn node(d: usize, n: usize) -> usize {
    unsafe { ((d + D_NODES + n * 4) as *const usize).read() }
}

/// The Chao's cocoon (Data1+0x34): a child object made by 0x568CD0 (EAX Chao object, stack
/// kind 0..=2), kind at its Data1+3; its delete (0x568CB0) clears the Chao's pointer.
const D_COCOON: usize = 0x34;
const CREATE_COCOON: usize = 0x0056_8CD0;

/// Our Chao's cocoon for `GardenChao`: 0 none, else 1 + kind.
///
/// # Safety
/// `d` is a live Chao's Data1.
unsafe fn cocoon_of(d: usize) -> u8 {
    unsafe {
        let cocoon = ((d + D_COCOON) as *const usize).read();
        if cocoon == 0 {
            return 0;
        }
        let cd = ((cocoon + OBJ_DATA1) as *const usize).read();
        if cd == 0 {
            return 0;
        }
        ((cd + 3) as *const u8).read().min(2) + 1
    }
}

/// A friend's Chao copy gets (or loses) the cocoon their real one is in.
///
/// # Safety
/// `object` is our live copy and `d` its Data1, on the game thread.
unsafe fn show_cocoon(object: usize, d: usize, want: u8) {
    let have = unsafe { cocoon_of(d) };
    if have == want {
        return;
    }
    let cocoon = unsafe { ((d + D_COCOON) as *const usize).read() };
    if cocoon != 0 {
        // Gone (or another kind): its delete clears our pointer for the next one.
        unsafe { ((cocoon + OBJ_MAIN) as *mut usize).write(DELETE_OBJECT) };
        return;
    }
    if want == 0 {
        return;
    }
    let kind = usize::from(want - 1);
    #[cfg(target_arch = "x86")]
    // SAFETY: the game's cocoon creator as its behaviours call it (EAX object, one stack arg the
    // caller pops), on the game thread.
    unsafe {
        std::arch::asm!(
            "push {kind}",
            "call {f}",
            "add esp, 4",
            kind = in(reg) kind,
            f = in(reg) CREATE_COCOON,
            inout("eax") object => _,
            clobber_abi("C"),
        );
    }
    #[cfg(not(target_arch = "x86"))]
    let _ = (object, kind, CREATE_COCOON);
}

/// What our Chao holds now, for `GardenChao`: props per node and the drum's size.
///
/// # Safety
/// `d` is a live Chao's Data1, on the game thread.
unsafe fn props_of(d: usize) -> ([u8; 3], u8) {
    let mut props = [0u8; 3];
    for (i, &n) in PROP_NODES.iter().enumerate() {
        let node = unsafe { node(d, n) };
        if node == 0 || unsafe { ((node + N_SHOWN) as *const u32).read() } == 0 {
            continue;
        }
        let model = unsafe { ((node + N_MODEL) as *const usize).read() };
        if let Some(k) = PROPS.iter().position(|p| p.model() == model) {
            props[i] = k as u8 + 1;
        }
    }
    let drum = if unsafe { ((d + D_DRUM) as *const usize).read() } == DRUM_MODEL {
        let size = unsafe { ((d + D_DRUM + 8) as *const f32).read() };
        (size.clamp(0.0, 1.0) * 255.0).round() as u8
    } else {
        0
    };
    (props, drum)
}

/// Shows on a friend's Chao copy what their real one holds (`props_of` on their side).
///
/// # Safety
/// `object` is our live copy and `d` its Data1, on the game thread.
unsafe fn show_props(object: usize, d: usize, props: [u8; 3], drum: u8) {
    for (i, &n) in PROP_NODES.iter().enumerate() {
        let node = unsafe { node(d, n) };
        if node == 0 {
            continue;
        }
        let (shown, model) = unsafe {
            (
                ((node + N_SHOWN) as *const u32).read() != 0,
                ((node + N_MODEL) as *const usize).read(),
            )
        };
        let want = props[i]
            .checked_sub(1)
            .and_then(|k| PROPS.get(usize::from(k)).copied());
        match want {
            Some(p) if !(shown && model == p.model()) => unsafe { attach(object, n, p) },
            None if shown && PROPS.iter().any(|p| p.model() == model) => unsafe {
                ((node + N_SHOWN) as *mut u32).write(0)
            },
            _ => {}
        }
    }
    unsafe {
        if drum > 0 {
            ((d + D_DRUM) as *mut usize).write(DRUM_MODEL);
            ((d + D_DRUM + 4) as *mut usize).write(TOY_TEXLIST);
            ((d + D_DRUM + 8) as *mut f32).write(f32::from(drum) / 255.0);
            ((d + D_DRUM + 12) as *mut f32).write(1.0);
        } else if ((d + D_DRUM) as *const usize).read() == DRUM_MODEL {
            ((d + D_DRUM) as *mut usize).write(0);
        }
    }
}

/// Attaches `prop` at node `n` of Chao `object` the way the game's behaviors do.
///
/// # Safety
/// `object` is a live Chao whose node `n` exists, on the game thread.
unsafe fn attach(object: usize, n: usize, prop: Prop) {
    let (func, arg) = match prop {
        Prop::Model(m) => (ATTACH_MODEL, m),
        Prop::Block { block, .. } => (ATTACH_BLOCK, block),
    };
    #[cfg(target_arch = "x86")]
    // SAFETY: the game's usercalls (EAX object, ECX node, EDX model or block); they only write
    // the node's fields and save the other registers they use.
    unsafe {
        std::arch::asm!(
            "call {f}",
            f = in(reg) func,
            inout("eax") object => _,
            inout("ecx") n => _,
            inout("edx") arg => _,
            clobber_abi("C"),
        );
    }
    #[cfg(not(target_arch = "x86"))]
    let _ = (object, n, func, arg);
}

/// What a visiting copy has to match: its type (form) and animal parts with their mask.
pub fn look(chao: &ChaoData) -> Vec<u8> {
    let mut look = vec![chao.chao_type()];
    look.extend_from_slice(&chao.bytes[LOOK_PARTS]);
    look.extend_from_slice(&chao.bytes[LOOK_DRESS]);
    // Colour and body shape: run/power, fly/swim, alignment and evolution progress, in steps
    // of 0.05.
    for at in LOOK_SHAPE {
        let v = f32::from_le_bytes(chao.bytes[at..at + 4].try_into().unwrap_or_default());
        look.push((v.clamp(-1.0, 1.2) * 20.0).round() as i8 as u8);
    }
    look
}

/// Hat (+0xD5), texture/jewel colour (+0xD8) and the other appearance bytes. They change on
/// hatching, absorbing an animal or drive (0x566DF0) and taking a hat on or off.
const LOOK_DRESS: std::ops::Range<usize> = 0xD3..0xDC;
/// Run/power, fly/swim, alignment and evolution progress (`chao::changes` fields): the shape
/// and colour 0x5A49C0 computes from the record when `Data1+0x674` bit 2 asks for it.
const LOOK_SHAPE: [usize; 4] = [0xA8, 0xAC, 0xB0, 0xC0];
/// `look` bytes: type, parts and dress (a change makes a new copy); then the shape.
const LOOK_FORM_LEN: usize = 1 + (0x124 - 0x118) + (0xDC - 0xD3);
/// `Data1+0x674` bit 2: redo shape and colour (0x5A49C0 clears it when done).
const D_RESHAPE: usize = 0x674;

/// Animal part mask and parts (re/chao-garden-items.md: 0x118 mask, 0x11C..0x123 parts).
const LOOK_PARTS: std::ops::Range<usize> = 0x118..0x124;

/// Network: friend `owner` sent Chao `index` again; a copy of it here takes on its look.
pub fn received_look(owner: u8, index: u8, chao: &ChaoData) {
    let chao = Box::new(chao.clone());
    with(|g| {
        g.looks_in.insert((owner, index), chao);
    });
}

/// Applies looks from `received_look` (game thread): a copy whose parts or form changed is made
/// again from the new data.
fn looks_frame(g: &mut Garden) {
    let mut later = Vec::new();
    for ((owner, index), new) in std::mem::take(&mut g.looks_in) {
        let Some(m) = g.mirrors.get_mut(&(owner, index)).filter(|m| !m.own) else {
            continue;
        };
        // SAFETY: our visitor's live object and its record (our own visitor buffer).
        unsafe {
            let d = data1(m.object);
            if d == 0 {
                continue;
            }
            let record = ((d + D_RECORD) as *const usize).read();
            if record == 0 {
                continue;
            }
            let copy = &mut *(record as *mut ChaoData);
            let (now, want) = (self::look(copy), self::look(&new));
            if now == want {
                continue;
            }
            if now[..LOOK_FORM_LEN] == want[..LOOK_FORM_LEN] {
                // Only colour/shape: the new values, and the game redoes them. The snapshot
                // takes them too, or the next diff would send the owner's own change back.
                for at in LOOK_SHAPE {
                    copy.bytes[at..at + 4].copy_from_slice(&new.bytes[at..at + 4]);
                    if let Some(before) = m.snapshot.as_mut() {
                        before.bytes[at..at + 4].copy_from_slice(&new.bytes[at..at + 4]);
                    }
                }
                let flags = (d + D_RESHAPE) as *mut u16;
                flags.write(flags.read() | 2);
                continue;
            }
            // Parts or form: patching the parts in place and asking for a redraw (`+0x674`)
            // didn't show them (2026-10-17); a fresh copy from the new data does. Not while we
            // hold or touch it.
            if player_held() == m.object || m.local_until.is_some() {
                later.push(((owner, index), new));
                continue;
            }
            ((m.object + OBJ_MAIN) as *mut usize).write(DELETE_OBJECT);
            log!("garden: player {owner}'s Chao {index} changed its look; making a new copy");
            g.mirrors.remove(&(owner, index));
            g.heard.remove(&(owner, index));
            g.left.push((owner, index));
        }
    }
    g.looks_in.extend(later);
}

/// Our visiting Chao: (index, save slot).
pub fn mine() -> Vec<(u8, usize)> {
    with(|g| g.mine.clone())
}

/// Our Chao visiting friends, for their tags: (where, index, the Chao's name if it has one).
pub fn my_visitors() -> Vec<([f32; 3], u8, Option<String>)> {
    if !game::in_garden() {
        return Vec::new();
    }
    let mine = with(|g| g.mine.clone());
    mine.into_iter()
        .filter_map(|(index, slot)| {
            let object = live_object(slot)?;
            // SAFETY: our Chao's live object (from the game's registry).
            let d = unsafe { data1(object) };
            if d == 0 {
                return None;
            }
            // SAFETY: as above.
            let p = unsafe { ((d + D_POSITION) as *const Vector).read_unaligned() };
            let name = game::read_chao_slot(slot)
                .map(|c| crate::karate::chao_name(&c))
                .filter(|n| n != "Chao");
            Some(([p.x, p.y, p.z], index, name))
        })
        .collect()
}

/// Friends' Chao that left our garden since last asked: (owner, index).
pub fn take_left() -> Vec<(u8, u8)> {
    with(|g| std::mem::take(&mut g.left))
}

/// Network: something a friend did (or the keeper's game) changed player `owner`'s Chao
/// `index`. Taken when it's ours (our save) or we keep the garden (our visitor copy).
pub fn received_changes(owner: u8, index: u8, changes: Changes) {
    with(|g| g.changes_in.push((owner, index, changes)));
}

/// Network: friend `holder` is holding player `owner`'s Chao `index`. Taken when our game is
/// its brain (see `brain_here`).
pub fn received_held(holder: u8, owner: u8, index: u8, offset: [f32; 3]) {
    with(|g| {
        g.held.insert(
            (owner, index),
            Held {
                holder,
                offset,
                at: Instant::now(),
            },
        )
    });
}

/// Whether our game runs player `owner`'s Chao: we keep the garden, or nobody does and it's
/// ours.
pub fn brain_here(owner: u8) -> bool {
    match crate::keeper::current() {
        Some(keeper) => crate::keeper::me() == Some(keeper),
        None => crate::keeper::me() == Some(owner),
    }
}

/// Where other players are drawn this frame (their puppets), for Chao they hold.
pub fn set_holders(holders: HashMap<u8, (Vector, i32)>) {
    with(|g| g.holders = holders);
}

/// To send: (changes to friends' Chao: owner, index, changes; friends' Chao we hold: owner,
/// index, offset).
pub fn take_outgoing() -> Outgoing {
    with(|g| {
        (
            std::mem::take(&mut g.changes_out),
            std::mem::take(&mut g.held_out),
        )
    })
}

/// Where friend `owner` is drawn now (their puppet): (position, facing).
pub fn holder(owner: u8) -> Option<(Vector, i32)> {
    with(|g| g.holders.get(&owner).copied())
}

/// Our copy of friend `owner`'s Chao `index`, when it follows them (not being touched by us).
pub fn following_copy(owner: u8, index: u8) -> Option<usize> {
    with(|g| {
        g.mirrors
            .get(&(owner, index))
            .filter(|m| m.local_until.is_none() && !m.own)
            .map(|m| m.object)
    })
}

/// Our visiting Chao now in this garden: (index, registry entry, object).
pub fn my_chao_entries() -> Vec<(u8, usize, usize)> {
    let mine = with(|g| g.mine.clone());
    mine.into_iter()
        .filter_map(|(index, slot)| {
            let entry = live_entry(slot)?;
            // SAFETY: a live registry entry's object pointer.
            let object = unsafe { ((entry + OBJ_ENTRY) as *const usize).read() };
            Some((index, entry, object))
        })
        .collect()
}

/// Rotates (x, z) by a BAMS angle about Y; `-angle` undoes it.
pub fn rotate(v: [f32; 3], angle: i32) -> [f32; 3] {
    let a = angle as f32 * (std::f32::consts::TAU / 65536.0);
    let (s, c) = a.sin_cos();
    [v[0] * c + v[2] * s, v[1], -v[0] * s + v[2] * c]
}

fn record_of(slot: usize) -> usize {
    CHAO_SLOTS + slot * CHAO_SIZE
}

/// The registry entry of the Chao in save slot `slot`, if it's in this garden.
fn live_entry(slot: usize) -> Option<usize> {
    let record = record_of(slot);
    // SAFETY: the game's static Chao object table (category 0); entries are checked for null.
    unsafe {
        (0..WORLD_CHAO_MAX)
            .map(|i| WORLD + i * WORLD_ENTRY_SIZE)
            .find(|&e| ((e + 8) as *const usize).read() == record)
            .filter(|&e| ((e + OBJ_ENTRY) as *const usize).read() != 0)
    }
}

/// The live object of the Chao in save slot `slot`, if it's in this garden.
fn live_object(slot: usize) -> Option<usize> {
    // SAFETY: a live registry entry's object pointer.
    live_entry(slot).map(|e| unsafe { ((e + OBJ_ENTRY) as *const usize).read() })
}

/// # Safety
/// `object` is a live object.
unsafe fn data1(object: usize) -> usize {
    unsafe { ((object + OBJ_DATA1) as *const usize).read() }
}

/// The object the local player holds, or 0.
pub fn player_held() -> usize {
    // SAFETY: a static pointer to the player's CharObj2 (null without a player).
    unsafe {
        let c = (PLAYER_CHAR_OBJ2 as *const usize).read_volatile();
        if c == 0 {
            0
        } else {
            ((c + PLAYER_HELD) as *const usize).read()
        }
    }
}

/// The local player is within `distance` of `at`.
fn player_near(at: Vector, distance: f32) -> bool {
    game::player_position().is_some_and(|(p, _)| {
        let (dx, dy, dz) = (p.x - at.x, p.y - at.y, p.z - at.z);
        dx * dx + dy * dy + dz * dz < distance * distance
    })
}

/// The local player holds a fruit, animal, drive or hat within reach of `at`.
fn holding_item_near(at: Vector) -> bool {
    let held = player_held();
    if held == 0 {
        return false;
    }
    // SAFETY: the held object is live; its registry entry pointer is checked against the
    // registry's range before use.
    let entry = unsafe { ((held + OBJ_ENTRY) as *const usize).read() };
    let in_category = |c: usize| {
        let base = WORLD + c * WORLD_CATEGORY_SIZE;
        (base..base + WORLD_CATEGORY_SIZE).contains(&entry)
    };
    ITEM_CATEGORIES.into_iter().any(in_category) && player_near(at, ITEM_NEAR)
}

/// The visiting Chao to send, per owner: ours (always: that's how friends know they visit),
/// and, while we keep the garden, everyone else's that our game runs.
pub fn streams() -> Vec<(u8, Vec<GardenChao>)> {
    let Some(me) = crate::keeper::me() else {
        return Vec::new();
    };
    let (slots, kept) = with(|g| {
        let kept: Vec<(u8, u8, usize)> = if crate::keeper::current() == Some(me) {
            g.mirrors
                .iter()
                .filter(|(_, m)| !m.own)
                .map(|(&(owner, index), m)| (owner, index, m.object))
                .collect()
        } else {
            Vec::new()
        };
        (g.mine.clone(), kept)
    });
    let mut out: Vec<(u8, Vec<GardenChao>)> = vec![(
        me,
        slots
            .iter()
            .filter_map(|&(index, slot)| chao_state(index, live_object(slot)?))
            .collect(),
    )];
    for (owner, index, object) in kept {
        let Some(state) = chao_state(index, object) else {
            continue;
        };
        match out.iter_mut().find(|(o, _)| *o == owner) {
            Some((_, list)) => list.push(state),
            None => out.push((owner, vec![state])),
        }
    }
    out.retain(|(_, list)| !list.is_empty());
    out
}

/// A live garden Chao object as it is now, to send as Chao `index`.
fn chao_state(index: u8, object: usize) -> Option<GardenChao> {
    let player = game::player_position();
    // SAFETY: a live Chao object's Data1, read on the game thread.
    unsafe {
        let d = data1(object);
        if d == 0 {
            return None;
        }
        let p = ((d + D_POSITION) as *const Vector).read_unaligned();
        let (props, drum) = props_of(d);
        let cocoon = cocoon_of(d);
        // Blending: the motion it's going to, unless there's none yet (-1).
        let motion_flags = ((d + D_MOTION) as *const u16).read();
        let current = ((d + D_MOTION_CURRENT) as *const i32).read();
        let next = ((d + D_MOTION_NEXT) as *const i32).read();
        let motion = if motion_flags & MOTION_BLENDING != 0 && next >= 0 {
            next
        } else {
            current
        };
        // Held by us (not shown in a friend's hands by us).
        let held = player_held() == object;
        let offset = match (held, player) {
            (true, Some((at, angle))) => {
                rotate([p.x - at.x, p.y - at.y, p.z - at.z], angle.wrapping_neg())
            }
            _ => [0.0; 3],
        };
        Some(GardenChao {
            index,
            position: [p.x, p.y, p.z],
            rotation_y: ((d + D_ROTATION_Y) as *const i32).read(),
            motion: motion as u16,
            held: held && player.is_some(),
            offset,
            bubble: (
                ((d + D_MOOD) as *const i16).read(),
                ((d + D_MOOD_TIMER) as *const i32).read(),
                ((d + D_MOOD_LENGTH) as *const i32).read(),
            ),
            eyes: ((d + D_EYES) as *const i16).read(),
            mouth: ((d + D_MOUTH) as *const i16).read(),
            props,
            drum,
            cocoon,
        })
    }
}

/// Call every frame (game thread).
pub fn on_frame() {
    let now = Instant::now();
    with(|g| {
        g.frame = g.frame.wrapping_add(1);
        own_frame(g);
        mirrors_frame(g, now);
        looks_frame(g);
        mine_frame(g);
    });
}

/// Our own visiting Chao follow the keeper while a friend keeps the garden (wrapped like a
/// friend's copy), and are our game's own again otherwise.
fn own_frame(g: &mut Garden) {
    let Some(me) = crate::keeper::me() else {
        return;
    };
    let follow = crate::keeper::current().is_some_and(|k| k != me);
    for (index, slot) in g.mine.clone() {
        let object = live_object(slot);
        let current = g.mirrors.get(&(me, index)).map(|m| m.object);
        if !follow || object != current {
            release_own(g, (me, index));
        }
        let (true, Some(object)) = (follow, object) else {
            continue;
        };
        if g.mirrors.contains_key(&(me, index)) {
            continue;
        }
        g.mirrors.insert(
            (me, index),
            Mirror {
                object,
                own: true,
                target: None,
                motion: None,
                driven: false,
                snapshot: None,
                local_until: None,
                since: Instant::now(),
            },
        );
        log!("garden: our Chao {index} now follows the keeper");
    }
    // Rows no longer visiting (or under an id we had before).
    let gone: Vec<(u8, u8)> = g
        .mirrors
        .iter()
        .filter(|&(&(o, i), m)| m.own && (o != me || !g.mine.iter().any(|&(r, _)| r == i)))
        .map(|(&key, _)| key)
        .collect();
    for key in gone {
        release_own(g, key);
    }
}

/// Our Chao `key` (our id, its index) stops following the keeper: our game runs it again.
fn release_own(g: &mut Garden, key: (u8, u8)) {
    if !g.mirrors.get(&key).is_some_and(|m| m.own) {
        return;
    }
    let Some(m) = g.mirrors.remove(&key) else {
        return;
    };
    let index = key.1;
    g.wrapped.remove(&m.object);
    // SAFETY: our own Chao's object when it's still the one in its registry entry (checked);
    // its own MainSub back and its AI on.
    unsafe {
        let still = g
            .mine
            .iter()
            .any(|&(i, slot)| i == index && live_object(slot) == Some(m.object));
        let main = (m.object + OBJ_MAIN) as *mut usize;
        if still && main.read() != DELETE_OBJECT {
            main.write(CHAO_MAIN);
            let d = data1(m.object);
            if d != 0 {
                let flags = (d + D_FLAGS) as *mut u32;
                flags.write(flags.read() | FLAG_AI);
                let status = (d + D_STATUS) as *mut u16;
                status.write(status.read() & !STATUS_HELD);
            }
        }
    }
    log!("garden: our Chao {index} runs in our game again");
}

/// Friends' Chao (and ours, while a friend keeps the garden): when our game is their brain, run
/// them and send their owners what that changes; otherwise follow the brain, let touched ones
/// run their own AI for a while and forward what that changes.
fn mirrors_frame(g: &mut Garden, now: Instant) {
    let player = game::player_position();
    let me = crate::keeper::me();
    let keeping = me.is_some() && crate::keeper::current() == me;
    let send_kept = g.frame.is_multiple_of(KEPT_CHANGES_EVERY);
    let mut gone = Vec::new();
    for (&(owner, index), m) in &mut g.mirrors {
        // SAFETY: our visitor's live object (cleared when we leave the garden), on the game thread.
        unsafe {
            let d = data1(m.object);
            if d == 0 {
                continue;
            }
            let main = (m.object + OBJ_MAIN) as *mut usize;
            // Its owner took it home (visits off, left, disconnected): it leaves, unless we're
            // holding or still touching it (then once we let go).
            let heard = g
                .heard
                .get(&(owner, index))
                .map_or(m.since, |at| (*at).max(m.since));
            if !m.own
                && heard.elapsed() > LEFT
                && player_held() != m.object
                && m.local_until.is_none()
            {
                main.write(DELETE_OBJECT);
                log!("garden: player {owner}'s Chao {index} went home; removing its copy");
                gone.push((owner, index));
                continue;
            }
            let record = ((d + D_RECORD) as *const usize).read();
            if record == 0 {
                continue;
            }
            let flags = (d + D_FLAGS) as *mut u32;

            // We keep the garden: a friend's Chao is ours to run, AI and all (a hold by a friend
            // wraps it in `mine_frame`). What its AI changes goes to its owner now and then.
            if keeping && !m.own {
                if m.driven {
                    m.driven = false;
                    m.local_until = None;
                    m.motion = None;
                    if main.read() == mirror_main as extern "C" fn(usize) as usize {
                        main.write(CHAO_MAIN);
                    }
                    flags.write(flags.read() | FLAG_AI);
                    let status = (d + D_STATUS) as *mut u16;
                    status.write(status.read() & !STATUS_HELD);
                    log!("garden: running player {owner}'s Chao {index} here (we keep the garden)");
                }
                let lifespan = (record + C_LIFESPAN) as *mut u16;
                if lifespan.read_unaligned() == 0 {
                    lifespan.write_unaligned(1);
                    if let Some(before) = m.snapshot.as_mut() {
                        before.bytes[C_LIFESPAN..C_LIFESPAN + 2]
                            .copy_from_slice(&1u16.to_le_bytes());
                    }
                }
                let before = m
                    .snapshot
                    .get_or_insert_with(|| Box::new((record as *const ChaoData).read()));
                if send_kept {
                    let after = (record as *const ChaoData).read();
                    let diff = chao::changes::diff(before, &after);
                    if !diff.is_empty() {
                        g.changes_out.push((owner, index, diff));
                    }
                    **before = after;
                }
                continue;
            }

            if !m.driven {
                // Whatever runs it now (its own MainSub, a hold or a whistle), it follows.
                if m.target.is_none() || main.read() == DELETE_OBJECT {
                    continue;
                }
                main.write(mirror_main as extern "C" fn(usize) as usize);
                g.wrapped.remove(&m.object);
                m.snapshot = Some(Box::new((record as *const ChaoData).read()));
                m.driven = true;
                m.motion = None;
                if m.own {
                    log!("garden: our Chao {index} follows the keeper's");
                } else {
                    log!("garden: player {owner}'s Chao {index} now follows its brain");
                }
            }

            // Touched by us: held, hit, offered an item, or we're right next to it (petting).
            // Never while its brain's player holds it.
            let here = ((d + D_POSITION) as *const Vector).read_unaligned();
            let status = ((d + D_STATUS) as *const u16).read();
            let held_by_us = player_held() == m.object;
            let brain_holds = m
                .target
                .is_some_and(|(t, at, _)| t.held && at.elapsed() < STALE);
            let touched = !brain_holds
                && (held_by_us
                    || status & STATUS_HIT != 0
                    || holding_item_near(here)
                    || player_near(here, PLAYER_NEAR));

            // Touching starts it; an item right at it (being eaten or held) keeps it going, so
            // the game never has its AI switched off halfway through.
            let busy = crate::items::near(here, BUSY_NEAR);
            if touched || (m.local_until.is_some() && busy) {
                if m.local_until.is_none() {
                    log!(
                        "garden: touching player {owner}'s Chao {index} (status {status:#06x}, \
                         held {held_by_us})"
                    );
                }
                m.local_until = Some(now + LOCAL_TIME);
            }
            let local = m.local_until.is_some();
            if m.local_until.is_some_and(|until| now >= until) {
                m.local_until = None;
                m.motion = None;
                log!("garden: done touching player {owner}'s Chao {index}; it follows again");
            }

            // What our touching changed goes to the owner and the keeper. Changes the game makes
            // on its own (moods drifting over time) happen in the brain's game too and are only
            // forwarded while we touch it, a few seconds at a time.
            if let Some(before) = m.snapshot.as_mut() {
                let after = (record as *const ChaoData).read();
                if local {
                    let diff = chao::changes::diff(before, &after);
                    if !diff.is_empty() {
                        log!(
                            "garden: player {owner}'s Chao {index}: {} change(s) to send",
                            diff.len()
                        );
                        g.changes_out.push((owner, index, diff));
                    }
                }
                **before = after;
            }
            let ai = m.local_until.is_some();
            flags.write(if ai {
                flags.read() | FLAG_AI
            } else {
                flags.read() & !FLAG_AI
            });

            if held_by_us && let Some((at, angle)) = player {
                let offset = rotate(
                    [here.x - at.x, here.y - at.y, here.z - at.z],
                    angle.wrapping_neg(),
                );
                g.held_out.push((owner, index, offset));
            }
        }
    }
    for key in gone {
        g.mirrors.remove(&key);
        g.heard.remove(&key);
        g.left.push(key);
    }
}

/// Changes for Chao whose record we keep (our save's, or a visitor copy we run), and holds of
/// Chao our game runs: shown in the friend's (puppet's) hands.
fn mine_frame(g: &mut Garden) {
    let slot_of =
        |mine: &[(u8, usize)], index: u8| mine.iter().find(|(i, _)| *i == index).map(|&(_, s)| s);
    let now = Instant::now();
    let me = crate::keeper::me();
    let keeping = me.is_some() && crate::keeper::current() == me;
    g.rows_changed
        .retain(|_, at| now.duration_since(*at) < ROW_CHANGE_QUIET);
    for (owner, index, changes) in std::mem::take(&mut g.changes_in) {
        if Some(owner) == me {
            if g.rows_changed.contains_key(&index) {
                log!("garden: changes for our Chao {index} dropped (that row was just changed)");
                continue;
            }
            let Some(slot) = slot_of(&g.mine, index) else {
                continue;
            };
            // SAFETY: our own save slot in the game's Chao table, written on the game thread
            // (the live Chao object reads it in place).
            unsafe { apply_changes(record_of(slot), live_object(slot), &changes) };
            // While it follows the keeper, the change isn't ours to send back.
            if let Some(before) = g
                .mirrors
                .get_mut(&(owner, index))
                .and_then(|m| m.snapshot.as_mut())
            {
                chao::changes::apply(before, &changes);
            }
            log!(
                "garden: applied {} change(s) from a friend to our Chao {index}",
                changes.len()
            );
        } else if keeping && let Some(m) = g.mirrors.get_mut(&(owner, index)) {
            // SAFETY: our visitor copy's live object and its record (our visitor buffer).
            unsafe {
                let d = data1(m.object);
                let record = if d == 0 {
                    0
                } else {
                    ((d + D_RECORD) as *const usize).read()
                };
                if record == 0 {
                    continue;
                }
                apply_changes(record, Some(m.object), &changes);
            }
            if let Some(before) = m.snapshot.as_mut() {
                chao::changes::apply(before, &changes);
            }
        }
    }

    g.held.retain(|_, h| h.at.elapsed() < HELD_STALE);
    let mut runs_here: Vec<((u8, u8), usize)> = Vec::new();
    if let Some(me) = me.filter(|&me| brain_here(me)) {
        for &(index, slot) in &g.mine {
            if let Some(object) = live_object(slot) {
                runs_here.push(((me, index), object));
            }
        }
    }
    if keeping {
        for (&key, m) in &g.mirrors {
            if !m.own {
                runs_here.push((key, m.object));
            }
        }
    }
    for (key, object) in runs_here {
        if !g.held.contains_key(&key) {
            continue;
        }
        // SAFETY: a live Chao object our game runs; its MainSub is the garden one or a wrapper.
        unsafe {
            let main = (object + OBJ_MAIN) as *mut usize;
            if main.read() == CHAO_MAIN {
                main.write(held_main as extern "C" fn(usize) as usize);
                g.wrapped.insert(object, false);
                log!(
                    "garden: a friend picked up player {}'s Chao {}",
                    key.0,
                    key.1
                );
            }
        }
    }
}

/// Adds `changes` to the Chao record at `record` (with the game's limits), and has its live
/// object (if any) redo its look.
///
/// # Safety
/// `record` is a live Chao record and `object` (if given) its live object, on the game thread.
unsafe fn apply_changes(record: usize, object: Option<usize>, changes: &Changes) {
    unsafe {
        let record = record as *mut ChaoData;
        let mut chao = record.read();
        chao::changes::apply(&mut chao, changes);
        record.write(chao);
        if let Some(object) = object {
            let d = data1(object);
            if d != 0 {
                let a = (d + D_APPEARANCE) as *mut u32;
                a.write(a.read() | 2);
            }
        }
    }
}

/// `0x793C40`: start motion `id` on the controller at `motion` (Data1+0xAC).
#[cfg(target_arch = "x86")]
unsafe fn start_motion(motion: usize, id: u16) {
    // SAFETY: usercall EAX = controller, EDX = id; clobbers ECX/EDX (re/chao-garden-sync.md).
    unsafe {
        std::arch::asm!(
            "call {func}",
            func = in(reg) START_MOTION,
            inout("eax") motion => _,
            inout("edx") u32::from(id) => _,
            clobber_abi("C"),
        );
    }
}

#[cfg(not(target_arch = "x86"))]
unsafe fn start_motion(_motion: usize, _id: u16) {}

/// `id` names a real entry of the motion table at `controller`: 0x793C40 doesn't check, and a
/// bad entry leaves a garbage "next motion" that crashes the Chao's drawing (0x56EB7F).
///
/// # Safety
/// `controller` is a live Chao's motion controller (Data1+0xAC).
unsafe fn motion_ok(controller: usize, id: u16) -> bool {
    if id >= MAX_MOTION_ID {
        return false;
    }
    unsafe {
        let table = ((controller + MC_TABLE) as *const usize).read();
        let current = ((controller + MC_MOTION) as *const usize).read();
        if table == 0 || current == 0 {
            return false;
        }
        let entry = table + usize::from(id) * MOTION_ENTRY_SIZE;
        let motion = (entry as *const usize).read();
        let mode = ((entry + 4) as *const u16).read();
        // Every Chao motion lives in the same module as the one it plays now.
        motion != 0 && mode <= 7 && motion.abs_diff(current) < 0x0100_0000
    }
}

/// `0x53A4B0`: eyes `id` until changed (ECX = obj, EAX = duration, EDX = id).
#[cfg(target_arch = "x86")]
unsafe fn set_eyes(object: usize, id: i16) {
    unsafe {
        std::arch::asm!(
            "call {func}",
            func = in(reg) SET_EYES,
            inout("ecx") object => _,
            inout("eax") -1i32 => _,
            inout("edx") i32::from(id) => _,
            clobber_abi("C"),
        );
    }
}

/// `0x53A5A0`: mouth `id` until changed (ECX = obj, EDX = duration, push id; caller pops).
#[cfg(target_arch = "x86")]
unsafe fn set_mouth(object: usize, id: i16) {
    unsafe {
        std::arch::asm!(
            "push {id}",
            "call {func}",
            "add esp, 4",
            id = in(reg) i32::from(id),
            func = in(reg) SET_MOUTH,
            inout("ecx") object => _,
            inout("edx") -1i32 => _,
            clobber_abi("C"),
        );
    }
}

#[cfg(not(target_arch = "x86"))]
unsafe fn set_eyes(_object: usize, _id: i16) {}
#[cfg(not(target_arch = "x86"))]
unsafe fn set_mouth(_object: usize, _id: i16) {}

/// Where a mirrored Chao should be this frame, or None to leave it to the game.
fn place(g: &mut Garden, object: usize, d: usize) -> Option<(Vector, i32, bool)> {
    let holders = &g.holders;
    let (&(owner, _), m) = g.mirrors.iter_mut().find(|(_, m)| m.object == object)?;
    if m.local_until.is_some() {
        return None;
    }
    let (t, at, brain) = m.target?;
    if at.elapsed() > STALE {
        // Updates paused: it stays where it last was (left to the game with its AI off, it
        // sank into the ground). It goes home once they stop for good (`LEFT`).
        let [x, y, z] = t.position;
        return Some((Vector { x, y, z }, t.rotation_y, false));
    }
    // What the copy plays now: the motion it's on, or the one it's blending into. The game's
    // own update can switch it (to standing, say), and the copy then slid around in the wrong
    // pose until the owner's Chao changed motion again: restart it whenever it differs.
    // SAFETY: the copy's Data1 motion fields, on the game thread.
    let playing = unsafe {
        let (current, next) = (
            ((d + D_MOTION_CURRENT) as *const i32).read(),
            ((d + D_MOTION_NEXT) as *const i32).read(),
        );
        current == i32::from(t.motion) || next == i32::from(t.motion)
    };
    if m.motion != Some(t.motion) || !playing {
        let new = m.motion != Some(t.motion);
        m.motion = Some(t.motion);
        // SAFETY: the Chao's own motion controller; the id comes from the owner's game, which
        // uses the same shared motion table, and is checked against it first.
        unsafe {
            if motion_ok(d + D_MOTION, t.motion) {
                start_motion(d + D_MOTION, t.motion);
            } else if new {
                log!(
                    "garden: player {owner}'s Chao: motion {:#x} skipped (not in the table)",
                    t.motion
                );
            }
        }
    }
    // SAFETY: the copy's own Data1 and face functions, on the game thread.
    unsafe {
        // The mood shown over it (heart, "?", "!", ...): switch to the owner's current one the
        // way the game does, then let it play here.
        let (mood, timer, length) = t.bubble;
        if (0..MOOD_STATES).contains(&mood) && ((d + D_MOOD) as *const i16).read() != mood {
            ((d + D_MOOD) as *mut i16).write(mood);
            ((d + D_MOOD_REQUEST) as *mut i16).write(-1);
            ((d + D_MOOD_TIMER) as *mut i32).write(timer);
            ((d + D_MOOD_LENGTH) as *mut i32).write(length);
            for counter in MOOD_COUNTERS {
                ((d + D_MOOD + counter) as *mut u16).write(0);
            }
        }
        if (0..MAX_FACE_ID).contains(&t.eyes) && ((d + D_EYES) as *const i16).read() != t.eyes {
            set_eyes(object, t.eyes);
        }
        if (0..MAX_FACE_ID).contains(&t.mouth) && ((d + D_MOUTH) as *const i16).read() != t.mouth {
            set_mouth(object, t.mouth);
        }
        // Toys and instruments in its hands, and the drum.
        show_props(object, d, t.props, t.drum);
        show_cocoon(object, d, t.cocoon);
    }
    if t.held
        && let Some(&(holder, angle)) = holders.get(&brain)
    {
        return Some((in_hands(holder, angle, t.offset), t.rotation_y, true));
    }
    // SAFETY: the visitor's Data1 (read on the game thread inside its MainSub).
    let here = unsafe { ((d + D_POSITION) as *const Vector).read_unaligned() };
    let [x, y, z] = t.position;
    Some((
        Vector {
            x: here.x + (x - here.x) * FOLLOW,
            y: here.y + (y - here.y) * FOLLOW,
            z: here.z + (z - here.z) * FOLLOW,
        },
        t.rotation_y,
        false,
    ))
}

pub fn in_hands(holder: Vector, angle: i32, offset: [f32; 3]) -> Vector {
    let o = rotate(offset, angle);
    Vector {
        x: holder.x + o[0],
        y: holder.y + o[1],
        z: holder.z + o[2],
    }
}

/// Writes the place, and the held bit (draw offset; can't be picked up while held).
///
/// # Safety
/// `d` is the Chao's Data1.
unsafe fn put(d: usize, (at, rotation_y, held): (Vector, i32, bool)) {
    unsafe {
        ((d + D_POSITION) as *mut Vector).write_unaligned(at);
        ((d + D_ROTATION_Y) as *mut i32).write(rotation_y);
        let status = (d + D_STATUS) as *mut u16;
        let s = status.read();
        status.write(if held {
            s | STATUS_HELD
        } else {
            s & !STATUS_HELD
        });
    }
}

/// Runs the game's Chao update for `object` between two `put`s of `spot` (when given), so
/// physics and gravity can't move it.
fn update_at(object: usize, d: usize, spot: Option<(Vector, i32, bool)>) {
    if let Some(spot) = spot {
        // SAFETY: the Chao's Data1, on the game thread.
        unsafe { put(d, spot) };
    }
    // SAFETY: the Chao's original MainSub, which this object had.
    unsafe { std::mem::transmute::<usize, extern "C" fn(usize)>(CHAO_MAIN)(object) };
    if let Some(spot) = spot {
        // SAFETY: as above; Data2's first three floats are the velocity.
        unsafe {
            put(d, spot);
            let d2 = ((object + OBJ_DATA2) as *const usize).read();
            if d2 != 0 {
                (d2 as *mut [f32; 3]).write_unaligned([0.0; 3]);
            }
        }
    }
}

/// MainSub of a friend's Chao copy: where the owner's real one is (unless we're touching it),
/// then the game's own update.
extern "C" fn mirror_main(object: usize) {
    crate::watch::mark("garden: friend's Chao");
    // SAFETY: a visitor Chao we wrapped; its Data1 is checked for null.
    let d = unsafe { data1(object) };
    let spot = if d == 0 {
        None
    } else {
        std::panic::catch_unwind(|| {
            let mut g = GARDEN.lock().unwrap_or_else(|e| e.into_inner());
            g.as_mut().and_then(|g| place(g, object, d))
        })
        .ok()
        .flatten()
    };
    update_at(object, d, spot);
}

/// MainSub of a Chao our game runs (ours, or a visitor while we keep the garden) while a friend
/// may hold it: in their (puppet's) hands with its AI off; back to normal when they let go.
extern "C" fn held_main(object: usize) {
    crate::watch::mark("garden: held Chao");
    // SAFETY: one of our Chao objects we wrapped; its Data1 is checked for null.
    let d = unsafe { data1(object) };
    let spot = if d == 0 {
        None
    } else {
        std::panic::catch_unwind(|| {
            let mut g = GARDEN.lock().unwrap_or_else(|e| e.into_inner());
            let g = g.as_mut()?;
            let me = crate::keeper::me();
            let index = g
                .mine
                .iter()
                .find(|&&(_, slot)| live_object(slot) == Some(object))
                .and_then(|&(i, _)| Some((me?, i)))
                .or_else(|| {
                    g.mirrors
                        .iter()
                        .find(|(_, m)| m.object == object && !m.own)
                        .map(|(&key, _)| key)
                });
            // Held by a friend we can see. Otherwise (let go, the friend left, or this Chao no
            // longer visits) it's put down.
            let held = index.and_then(|key| g.held.get(&key)).and_then(|h| {
                g.holders
                    .get(&h.holder)
                    .map(|&(at, angle)| (at, angle, h.offset))
            });
            let flags = (d + D_FLAGS) as *mut u32;
            let ai_off = g.wrapped.entry(object).or_insert(false);
            match held {
                Some((holder, angle, offset)) => {
                    // SAFETY: our Chao's Data1 and motion controller, on the game thread.
                    unsafe {
                        if !*ai_off {
                            *ai_off = true;
                            flags.write(flags.read() & !FLAG_AI);
                            start_motion(d + D_MOTION, MOTION_HELD);
                        }
                        let rotation = ((d + D_ROTATION_Y) as *const i32).read();
                        Some((in_hands(holder, angle, offset), rotation, true))
                    }
                }
                None => {
                    if *ai_off {
                        *ai_off = false;
                        // SAFETY: as above; let go: drop it where it is with its AI back on.
                        unsafe {
                            flags.write(flags.read() | FLAG_AI);
                            let status = (d + D_STATUS) as *mut u16;
                            status.write(status.read() & !STATUS_HELD);
                        }
                        log!("garden: a friend put a Chao down ({index:?})");
                    }
                    None
                }
            }
        })
        .ok()
        .flatten()
    };
    update_at(object, d, spot);
}

#[cfg(test)]
mod tests {
    use super::{ChaoData, LOOK_FORM_LEN, look, rotate};

    #[test]
    fn looks_follow_colour_and_parts_but_not_tiny_changes() {
        let mut a = ChaoData {
            bytes: [0; crate::chao::CHAO_SIZE],
        };
        a.bytes[0x80] = 2;
        let base = look(&a);
        a.bytes[0xB0..0xB4].copy_from_slice(&0.01f32.to_le_bytes());
        assert_eq!(look(&a), base);
        a.bytes[0xB0..0xB4].copy_from_slice(&0.3f32.to_le_bytes());
        let tinted = look(&a);
        assert_ne!(tinted, base);
        assert_eq!(tinted[..LOOK_FORM_LEN], base[..LOOK_FORM_LEN]);
        a.bytes[0x11C] = 5;
        assert_ne!(look(&a)[..LOOK_FORM_LEN], base[..LOOK_FORM_LEN]);
    }

    #[test]
    fn offset_round_trips_through_the_holders_frame() {
        let v = [3.0, 4.0, -7.5];
        for angle in [0i32, 0x4000, 0x8000, -0x2345, 0x1_2345] {
            let back = rotate(rotate(v, angle.wrapping_neg()), angle);
            for i in 0..3 {
                assert!((back[i] - v[i]).abs() < 1e-3, "{angle:#x}: {back:?}");
            }
        }
    }
}
