//! The link machine (re/chao-garden-objects.md): one machine in each garden, near its
//! transporter (Neutral plain, Hero white and gold, Dark spiky; models in `assets/link_machine/`,
//! `machine_model`). Walking up to it and pressing the action button opens the pick list: the
//! GBA save's creatures, the ones the mod keeps and, without a save, the one-time gift (the
//! rows of `Page`). The machine's screen shows the picture of the one highlighted, and the one
//! picked hops out of the machine's hatch into the garden (`super::bring_out`). Its first row,
//! "Moves", lists the player's Chao by name and then one's moves, to choose the four it battles
//! with (`Page::chao_list`, `Page::chao_moves`).
//!
//! The machine is an object of list 0, which runs after the pads are read and before the
//! player: the press that opens the list is taken out of the frame's input there, so the
//! garden's own actions (whistle, pick up) never see it. It only takes it from a player
//! standing in front of the machine, facing it, with nothing to pick up closer to them. While the list is open the game's
//! controls are off and the list reads the menu buttons and the keyboard. The player stands
//! against a box registered as dynamic land collision, which the garden's own Chao also see.

// Off the game (Linux builds) only the tests use the garden side.
#![cfg_attr(not(target_arch = "x86"), allow(dead_code))]

use std::sync::atomic::{AtomicBool, Ordering};

use super::machine_model::Machine;
use super::{Action, Page};

/// config.ini `LinkMachine`.
static ENABLED: AtomicBool = AtomicBool::new(true);

pub fn set_enabled(on: bool) {
    ENABLED.store(on, Ordering::Relaxed);
}

pub fn enabled() -> bool {
    ENABLED.load(Ordering::Relaxed)
}

/// Each garden's machine (Neutral, Hero, Dark), built by src/bin/machineconv.rs.
const MODELS: [&[u8]; 3] = [
    include_bytes!("../../assets/link_machine/neutral.lmc"),
    include_bytes!("../../assets/link_machine/hero.lmc"),
    include_bytes!("../../assets/link_machine/dark.lmc"),
];

/// Where each garden's machine stands (its floor) and the Y angle (BAMS) its front faces: 20
/// to 25 units from the transporter on flat ground, the front toward the garden.
const SPOTS: [([f32; 3], i32); 3] = [
    ([50.0, 10.0, 22.0], 0x96F8),
    ([117.0, 0.0, 117.0], 0xA6C0),
    ([-98.0, 3.5, -98.0], 0x10D9),
];

/// The player can use the machine from this far from its box (across, any side) and this far
/// from its floor (up).
const REACH: f32 = 4.0;
const REACH_UP: f32 = 5.0;
/// The picture fills this much of the screen's height, and stands this far in front of it.
const PICTURE_FILL: f32 = 0.9;
const PICTURE_OUT: f32 = 0.06;

/// The garden's machine spot: (floor position, angle), for gardens 1..=3.
pub fn spot(garden: u8) -> Option<([f32; 3], i32)> {
    SPOTS.get(usize::from(garden).checked_sub(1)?).copied()
}

/// The machine model of garden `garden` (1..=3).
pub fn model(garden: u8) -> Option<Machine> {
    let bytes = MODELS.get(usize::from(garden).checked_sub(1)?)?;
    Machine::parse(bytes).ok()
}

/// `p` (in the machine's own axes, its front +Z) turned by `angle` (BAMS) the way the game
/// turns a model's +Z toward (sin, cos).
pub fn turn(p: [f32; 3], angle: i32) -> [f32; 3] {
    let a = angle as f32 * std::f32::consts::TAU / 65536.0;
    let (s, c) = a.sin_cos();
    [p[0] * c + p[2] * s, p[1], p[2] * c - p[0] * s]
}

/// `p` of the machine standing at `at` facing `angle`, in the world.
pub fn world(at: [f32; 3], angle: i32, p: [f32; 3]) -> [f32; 3] {
    let t = turn(p, angle);
    [at[0] + t[0], at[1] + t[1], at[2] + t[2]]
}

/// The player at `player` is within reach of the machine at `at` (facing `angle`, its box
/// `min`..`max` in its own axes), on any side and whichever way they face: how far they are
/// from its box.
pub fn in_reach(at: [f32; 3], angle: i32, (min, max): Bounds, player: [f32; 3]) -> Option<f32> {
    let (dx, dz) = (player[0] - at[0], player[2] - at[2]);
    let a = angle as f32 * std::f32::consts::TAU / 65536.0;
    let (s, c) = a.sin_cos();
    // Back into the machine's own axes (`turn` the other way).
    let (x, z) = (dx * c - dz * s, dx * s + dz * c);
    let ox = (min[0] - x).max(x - max[0]).max(0.0);
    let oz = (min[2] - z).max(z - max[2]).max(0.0);
    let out = (ox * ox + oz * oz).sqrt();
    (out < REACH && (player[1] - at[1]).abs() < REACH_UP).then_some(out)
}

/// A machine's collision box: its corners (min, max) in its own axes.
type Bounds = ([f32; 3], [f32; 3]);

/// Garden `garden`'s machine box (its model's), read once.
fn machine_box(garden: u8) -> Option<Bounds> {
    static BOXES: std::sync::OnceLock<[Option<Bounds>; 3]> = std::sync::OnceLock::new();
    let boxes = BOXES.get_or_init(|| {
        std::array::from_fn(|i| model(i as u8 + 1).map(|m| (m.box_min, m.box_max)))
    });
    *boxes.get(usize::from(garden).checked_sub(1)?)?
}

/// The collision box's corners (offsets from the machine's spot, turned by `angle`) and its 12
/// triangles, each wound so `(v1 - v0) x (v2 - v0)` points out of the box.
pub fn box_mesh(min: [f32; 3], max: [f32; 3], angle: i32) -> ([[f32; 3]; 8], [[u16; 3]; 12]) {
    let corners = std::array::from_fn(|i| {
        let p = [
            if i & 1 == 0 { min[0] } else { max[0] },
            if i & 2 == 0 { min[1] } else { max[1] },
            if i & 4 == 0 { min[2] } else { max[2] },
        ];
        turn(p, angle)
    });
    // Corner i: bit 0 x, bit 1 y, bit 2 z.
    let tris = [
        [0, 2, 1],
        [1, 2, 3], // -z
        [4, 5, 6],
        [5, 7, 6], // +z
        [0, 4, 2],
        [2, 4, 6], // -x
        [1, 3, 5],
        [3, 7, 5], // +x
        [0, 1, 4],
        [1, 5, 4], // -y
        [2, 6, 3],
        [3, 6, 7], // +y
    ];
    (corners, tris)
}

/// Where the picture stands (the bottom middle of the screen, a little in front) and how tall
/// it is, in the machine's own axes.
pub fn picture(m: &Machine) -> ([f32; 3], f32) {
    let s = &m.screen;
    let height = s.height.min(s.width) * PICTURE_FILL;
    (
        [
            s.centre[0],
            s.centre[1] - height / 2.0,
            s.centre[2] + PICTURE_OUT,
        ],
        height,
    )
}

/// A press on the pick list.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Key {
    Up,
    Down,
    PageUp,
    PageDown,
    Pick,
    Back,
}

/// What a press did.
#[derive(Clone, Debug, PartialEq)]
pub enum Outcome {
    Stay,
    Moved,
    Close,
    Chose(Action),
}

/// The pick list: `Page`'s rows, the cursor only on rows with something to pick.
pub struct List {
    pub page: Page,
    /// The last refusal, shown under the rows.
    pub message: Option<String>,
}

fn pickable(a: &Action) -> bool {
    !matches!(a, Action::None)
}

impl List {
    pub fn new(mut page: Page) -> Self {
        page.row = page.rows.iter().position(|(_, a)| pickable(a)).unwrap_or(0);
        page.scroll = 0;
        Self {
            page,
            message: None,
        }
    }

    /// The next pickable row from the cursor, `step` rows at a time (wrapping).
    fn next(&self, forward: bool) -> usize {
        let n = self.page.rows.len();
        if n == 0 {
            return 0;
        }
        let mut r = self.page.row;
        for _ in 0..n {
            r = if forward {
                (r + 1) % n
            } else {
                (r + n - 1) % n
            };
            if pickable(&self.page.rows[r].1) {
                return r;
            }
        }
        self.page.row
    }

    /// Handles one press; `visible` rows fit on screen.
    pub fn press(&mut self, key: Key, visible: usize) -> Outcome {
        let n = self.page.rows.len();
        let before = self.page.row;
        match key {
            Key::Up => self.page.row = self.next(false),
            Key::Down => self.page.row = self.next(true),
            Key::PageUp | Key::PageDown if n > 0 => {
                let target = if key == Key::PageUp {
                    self.page.row.saturating_sub(visible.max(1))
                } else {
                    (self.page.row + visible.max(1)).min(n - 1)
                };
                // The pickable row nearest the target, on its side first.
                let mut best = self.page.row;
                let mut gap = usize::MAX;
                for (i, (_, a)) in self.page.rows.iter().enumerate() {
                    if pickable(a) && i.abs_diff(target) < gap {
                        gap = i.abs_diff(target);
                        best = i;
                    }
                }
                self.page.row = best;
            }
            Key::PageUp | Key::PageDown => {}
            Key::Back => return Outcome::Close,
            Key::Pick => {
                return match self.page.rows.get(self.page.row) {
                    Some((_, a)) if pickable(a) => Outcome::Chose(a.clone()),
                    _ => Outcome::Stay,
                };
            }
        }
        self.scroll_to(visible);
        if self.page.row != before {
            self.message = None;
            Outcome::Moved
        } else {
            Outcome::Stay
        }
    }

    /// Scrolls so the cursor's row shows among `visible` rows.
    pub fn scroll_to(&mut self, visible: usize) {
        let visible = visible.max(1);
        let p = &mut self.page;
        if p.row < p.scroll {
            p.scroll = p.row;
        } else if p.row >= p.scroll + visible {
            p.scroll = p.row + 1 - visible;
        }
    }

    /// The species of the highlighted row, for the machine's screen.
    pub fn species(&self) -> Option<u16> {
        match self.page.rows.get(self.page.row)?.1 {
            Action::Bring(ref c) => Some(c.species),
            Action::Gift(s) => Some(s),
            _ => None,
        }
    }
}

#[cfg(target_arch = "x86")]
pub use game::on_frame;

#[cfg(not(target_arch = "x86"))]
pub fn on_frame() {}

#[cfg(target_arch = "x86")]
mod game {
    use std::sync::Mutex;
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

    use super::super::billboard::{self, Textures};
    use super::super::machine_model::Machine;
    use super::super::{Action, Page};
    use super::{
        Key, List, Outcome, REACH_UP, box_mesh, enabled, machine_box, model, picture, spot, world,
    };
    use crate::game as sa2;
    use crate::hud;
    use crate::input::Hotkey;
    use crate::log;

    const OBJ_MAIN: usize = 0x10;
    const OBJ_DISPLAY: usize = 0x14;
    const OBJ_DELETE: usize = 0x18;
    const OBJ_DATA1: usize = 0x34;
    const D_ANGLE: usize = 0x0C;
    const D_POSITION: usize = 0x14;
    /// Writing this into an object's main has the game delete it on its next update.
    const DELETE_OBJECT: usize = 0x0046_F720;
    /// The object lists' heads (8 circular lists, linked through +0).
    const OBJECT_LISTS: usize = 0x01A5_A254;
    /// A zeroed Data1 from the game heap (no arguments; EAX).
    const ALLOC_DATA1: usize = 0x0047_0B40;
    /// Dynamic land collision: register (EAX flags, EDX object, ESI NJS_OBJECT) and
    /// unregister (EDI object, EBX NJS_OBJECT).
    const REGISTER_LAND: usize = 0x0047_D6B0;
    const UNREGISTER_LAND: usize = 0x0047_D760;
    /// COL flag: solid.
    const SOLID: u32 = 1;
    /// Player 1's "action button (B or X) newly pressed" byte and the gameplay pad copy's
    /// pressed word, read by the player's update after list 0 runs.
    const ACTION_PRESSED: usize = 0x01DE_FB00;
    const PAD_PRESSED: usize = 0x01DE_FC10;
    const ACTION_BUTTONS: u32 = 0x402;
    /// Menu buttons (`MenuButtons_Pressed`): B back, A pick, up, down.
    const BACK: u32 = 0x02;
    const PICK: u32 = 0x04;
    const UP: u32 = 0x10;
    const DOWN: u32 = 0x20;
    /// The pause screen's GameState.
    const PAUSED: i16 = 17;
    /// Registry categories the player can pick up: Chao, eggs, animals and drives, fruit,
    /// seeds, hats.
    const LIFTABLE: [usize; 6] = [0, 1, 2, 3, 7, 9];
    /// Something to pick up this much farther from the player than the machine's face is still
    /// closer than the machine.
    const LIFT_MARGIN: f32 = 1.0;
    /// The swatch textures' global index (billboard's pictures and battle effects are below).
    const GBIX: u32 = 0x7E5C_0000;
    const PLAY_SOUND: usize = 0x0043_7260;
    /// Menu sounds (as the battle menus use them).
    mod sound {
        pub const CURSOR: u32 = 0x8000;
        pub const CONFIRM: u32 = 0x100E;
    }

    const VK_BACK: i32 = 0x08;
    const VK_RETURN: i32 = 0x0D;
    const VK_ESCAPE: i32 = 0x1B;
    const VK_PRIOR: i32 = 0x21;
    const VK_NEXT: i32 = 0x22;
    const VK_UP: i32 = 0x26;
    const VK_DOWN: i32 = 0x28;
    const KEYS: [i32; 7] = [
        VK_BACK, VK_RETURN, VK_ESCAPE, VK_PRIOR, VK_NEXT, VK_UP, VK_DOWN,
    ];

    const COLOR_TITLE: i32 = 0xFFFF_E080u32 as i32;
    const COLOR_TEXT: i32 = 0xFFE0_E0E0u32 as i32;
    const COLOR_SELECTED: i32 = 0xFF80_FFFFu32 as i32;
    const COLOR_HINT: i32 = 0xFFA0_A0A0u32 as i32;
    const COLOR_MESSAGE: i32 = 0xFFFF_A060u32 as i32;

    /// Our object (0: none), and the collision NJS_OBJECT registered for it.
    static OBJECT: AtomicUsize = AtomicUsize::new(0);
    static COLLISION: AtomicUsize = AtomicUsize::new(0);
    /// The pick list is open; the action button was taken to open it.
    static OPEN: AtomicBool = AtomicBool::new(false);
    static REQUESTED: AtomicBool = AtomicBool::new(false);

    /// A machine model as the game draws it: a hidden root whose children are the pieces.
    struct Drawn {
        root: Box<[u32; 13]>,
        _nodes: Vec<[u32; 13]>,
        _models: Vec<[u32; 6]>,
        _lists: Vec<Box<[u32]>>,
        machine: Machine,
    }

    /// A box the player stands against: NJS_OBJECT, basic model, mesh set, material, points,
    /// normals and indices.
    struct Solid {
        object: Box<[u32; 13]>,
        _model: Box<[u32; 10]>,
        _meshset: Box<[u32; 6]>,
        _material: Box<[u32; 5]>,
        _points: Box<[[f32; 3]; 8]>,
        _normals: Box<[[f32; 3]; 8]>,
        _indices: Box<[u16; 36]>,
    }

    struct State {
        garden: u8,
        /// Built once per garden and kept (the game may still hold their addresses).
        drawn: [Option<Drawn>; 3],
        solids: [Option<Solid>; 3],
        textures: Option<(u8, Textures)>,
        /// Textures that failed in this scene (not tried again).
        textures_failed: Option<Scene>,
        list: Option<List>,
        /// The rows picked to open the pages over the first one ("Moves", then a Chao).
        trail: Vec<Action>,
        /// Frames since the list opened or the game was last paused (the opening and resuming
        /// presses aren't picks).
        age: u32,
        /// The game was paused at the list's last frame.
        paused: bool,
        /// The picture on the screen while the list is closed.
        shown: Option<u16>,
        keys: Vec<Hotkey>,
    }

    // SAFETY: the raw addresses are only used on the game thread.
    unsafe impl Send for State {}

    static STATE: Mutex<State> = Mutex::new(State {
        garden: 0,
        drawn: [None, None, None],
        solids: [None, None, None],
        textures: None,
        textures_failed: None,
        list: None,
        trail: Vec::new(),
        age: 0,
        paused: false,
        shown: None,
        keys: Vec::new(),
    });

    type Scene = (i16, u32, i32);

    fn scene() -> Scene {
        (
            sa2::current_level(),
            sa2::stage_run(),
            sa2::current_chao_area(),
        )
    }

    fn lock() -> std::sync::MutexGuard<'static, State> {
        STATE.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn drawn(m: Machine) -> Drawn {
        let one = 1f32.to_bits();
        let lists: Vec<Box<[u32]>> = m
            .pieces
            .iter()
            .flat_map(|p| [p.vlist.clone(), p.plist.clone()])
            .map(Vec::into_boxed_slice)
            .collect();
        // Radius 0: never culled by its sphere. Both vectors are complete before their
        // elements' addresses are taken, and never grow after.
        let models: Vec<[u32; 6]> = lists
            .chunks(2)
            .map(|l| [l[0].as_ptr() as u32, l[1].as_ptr() as u32, 0, 0, 0, 0])
            .collect();
        let mut nodes: Vec<[u32; 13]> = models
            .iter()
            .map(|model| {
                // Unit position, angle and scale, no children.
                let mut node = [0u32; 13];
                node[0] = 0x17;
                node[1] = model.as_ptr() as u32;
                node[8] = one;
                node[9] = one;
                node[10] = one;
                node
            })
            .collect();
        for i in 1..nodes.len() {
            let next = nodes[i].as_ptr() as u32;
            nodes[i - 1][12] = next;
        }
        let mut root = Box::new([0u32; 13]);
        // Hidden, unit transforms; its children are the pieces.
        root[0] = 0x0F;
        root[8] = one;
        root[9] = one;
        root[10] = one;
        root[11] = nodes.first().map_or(0, |n| n.as_ptr() as u32);
        Drawn {
            root,
            _nodes: nodes,
            _models: models,
            _lists: lists,
            machine: m,
        }
    }

    fn solid(m: &Machine, at: [f32; 3], angle: i32) -> Solid {
        let (corners, tris) = box_mesh(m.box_min, m.box_max, angle);
        let points = Box::new(corners);
        let normals = Box::new([[0.0, 1.0, 0.0]; 8]);
        let mut indices = Box::new([0u16; 36]);
        for (i, t) in tris.iter().enumerate() {
            indices[i * 3..i * 3 + 3].copy_from_slice(t);
        }
        let centre: [f32; 3] = std::array::from_fn(|i| {
            corners.iter().map(|c| c[i]).sum::<f32>() / corners.len() as f32
        });
        let radius = corners
            .iter()
            .map(|c| {
                (0..3)
                    .map(|i| (c[i] - centre[i]).powi(2))
                    .sum::<f32>()
                    .sqrt()
            })
            .fold(0.0f32, f32::max);
        let material = Box::new([0xFFFF_FFFFu32, 0, 0, 0, 0]);
        // Type 0 (triangles), material 0; 12 triangles.
        let meshset = Box::new([12 << 16, indices.as_ptr() as u32, 0, 0, 0, 0]);
        let model = Box::new([
            points.as_ptr() as u32,
            normals.as_ptr() as u32,
            8,
            meshset.as_ptr() as u32,
            material.as_ptr() as u32,
            1 | 1 << 16,
            centre[0].to_bits(),
            centre[1].to_bits(),
            centre[2].to_bits(),
            radius.to_bits(),
        ]);
        let one = 1f32.to_bits();
        let mut object = Box::new([0u32; 13]);
        // Its position applies (the corners are already turned): no scale, no children.
        object[0] = 0x14;
        object[1] = model.as_ptr() as u32;
        object[2] = at[0].to_bits();
        object[3] = at[1].to_bits();
        object[4] = at[2].to_bits();
        object[8] = one;
        object[9] = one;
        object[10] = one;
        Solid {
            object,
            _model: model,
            _meshset: meshset,
            _material: material,
            _points: points,
            _normals: normals,
            _indices: indices,
        }
    }

    /// `object` is in object list 0 (it wasn't freed with an old area).
    fn in_list0(object: usize) -> bool {
        // SAFETY: the game's list heads and their circular links (+0), on the game thread.
        unsafe {
            let head = (OBJECT_LISTS as *const usize).read();
            let mut o = head;
            for _ in 0..4096 {
                if o == 0 {
                    return false;
                }
                if o == object {
                    return true;
                }
                o = (o as *const usize).read();
                if o == head {
                    return false;
                }
            }
        }
        false
    }

    fn spawn(s: &mut State, garden: u8) -> bool {
        let Some((at, angle)) = spot(garden) else {
            return false;
        };
        let i = usize::from(garden - 1);
        if s.drawn[i].is_none() {
            let Some(m) = model(garden) else {
                log!("gba link: the link machine model of garden {garden} is broken");
                return false;
            };
            s.solids[i] = Some(solid(&m, at, angle));
            s.drawn[i] = Some(drawn(m));
        }
        let Some(collision) = s.solids[i].as_ref().map(|c| c.object.as_ptr() as usize) else {
            return false;
        };
        // SAFETY: the game's LoadObject and Data1 allocator on the game thread, in a running
        // garden; our own object's fields.
        unsafe {
            let object = crate::lockon::load_object(
                0,
                c"LinkMachine".as_ptr().cast(),
                main as *const () as usize,
            );
            if object == 0 {
                return false;
            }
            ((object + OBJ_DISPLAY) as *mut usize).write(display as *const () as usize);
            ((object + OBJ_DELETE) as *mut usize).write(delete as *const () as usize);
            let d = std::mem::transmute::<usize, extern "C" fn() -> usize>(ALLOC_DATA1)();
            if d != 0 {
                ((object + OBJ_DATA1) as *mut usize).write(d);
                ((d + D_ANGLE) as *mut i32).write(angle);
                ((d + D_POSITION) as *mut [f32; 3]).write_unaligned(at);
                std::arch::asm!(
                    "push esi",
                    "mov esi, ecx",
                    "call edi",
                    "pop esi",
                    in("edi") REGISTER_LAND,
                    inout("eax") SOLID => _,
                    inout("edx") object => _,
                    inout("ecx") collision => _,
                    clobber_abi("C"),
                );
                COLLISION.store(collision, Ordering::Relaxed);
            }
            OBJECT.store(object, Ordering::Relaxed);
        }
        s.garden = garden;
        log!("gba link: link machine in garden {garden}");
        true
    }

    /// Has our object deleted on its next update (its delete callback unregisters the box).
    fn remove() {
        let object = OBJECT.load(Ordering::Relaxed);
        if object != 0 && in_list0(object) {
            // SAFETY: our live object.
            unsafe { ((object + OBJ_MAIN) as *mut usize).write(DELETE_OBJECT) };
        } else {
            OBJECT.store(0, Ordering::Relaxed);
        }
    }

    /// The object's main (list 0, before the player): takes the action button's press when the
    /// player stands at the machine with nothing in their hands.
    extern "C" fn main(object: usize) {
        if object != OBJECT.load(Ordering::Relaxed) {
            // SAFETY: the object the game runs; not ours any more.
            unsafe { ((object + OBJ_MAIN) as *mut usize).write(DELETE_OBJECT) };
            return;
        }
        let _ = std::panic::catch_unwind(take_press);
    }

    fn take_press() {
        if OPEN.load(Ordering::Relaxed) || REQUESTED.load(Ordering::Relaxed) || !usable() {
            return;
        }
        // SAFETY: the game's input bytes (statics), on the game thread.
        unsafe {
            if (ACTION_PRESSED as *const u8).read_volatile() == 0 {
                return;
            }
            (ACTION_PRESSED as *mut u8).write_volatile(0);
            let pressed = (PAD_PRESSED as *const u32).read_volatile();
            (PAD_PRESSED as *mut u32).write_volatile(pressed & !ACTION_BUTTONS);
        }
        REQUESTED.store(true, Ordering::Relaxed);
        log!("gba link: link machine pressed");
    }

    /// The player stands at the machine's front facing it, hands empty, nothing to pick up
    /// closer to them, no other menu open.
    fn usable() -> bool {
        let garden = sa2::current_chao_area() as u8;
        let (Some((at, angle)), Some(bounds), Some((p, _))) =
            (spot(garden), machine_box(garden), sa2::player_position())
        else {
            return false;
        };
        let player = [p.x, p.y, p.z];
        let Some(out) = super::in_reach(at, angle, bounds, player) else {
            return false;
        };
        crate::garden::player_held() == 0
            && !crate::menu::is_open()
            && !crate::chat::typing()
            && sa2::game_state() != PAUSED
            && !liftable_near(player, out + LIFT_MARGIN)
    }

    /// Something the player can pick up is within `distance` of them (across).
    fn liftable_near(player: [f32; 3], distance: f32) -> bool {
        LIFTABLE
            .iter()
            .flat_map(|&c| crate::registry::entries(c))
            .any(|(_, object)| {
                // SAFETY: a registry entry's live object and its Data1, on the game thread.
                let at = unsafe {
                    let d = ((object + OBJ_DATA1) as *const usize).read();
                    if d == 0 {
                        return false;
                    }
                    ((d + D_POSITION) as *const [f32; 3]).read_unaligned()
                };
                let (dx, dz) = (at[0] - player[0], at[2] - player[2]);
                dx * dx + dz * dz < distance * distance && (at[1] - player[1]).abs() < REACH_UP
            })
    }

    /// The game deletes our object (its area goes, or we asked): the box goes with it.
    extern "C" fn delete(object: usize) {
        if object != OBJECT.load(Ordering::Relaxed) {
            return;
        }
        let collision = COLLISION.swap(0, Ordering::Relaxed);
        if collision != 0 {
            // SAFETY: the game's unregister with our object and the NJS_OBJECT it registered
            // (both still valid here), on the game thread.
            unsafe {
                std::arch::asm!(
                    "push ebx",
                    "mov ebx, ecx",
                    "call eax",
                    "pop ebx",
                    in("edi") object,
                    inout("ecx") collision => _,
                    inout("eax") UNREGISTER_LAND => _,
                    clobber_abi("C"),
                );
            }
        }
        OBJECT.store(0, Ordering::Relaxed);
    }

    /// Draws the machine and, on its screen, the picture of the highlighted (or last picked)
    /// creature.
    extern "C" fn display(object: usize) {
        let _ = std::panic::catch_unwind(|| {
            if object != OBJECT.load(Ordering::Relaxed) {
                return;
            }
            let mut s = lock();
            let garden = s.garden;
            let Some((at, angle)) = spot(garden) else {
                return;
            };
            let i = usize::from(garden - 1);
            let now = scene();
            if s.textures
                .as_ref()
                .is_some_and(|(g, t)| *g != garden || !t.ours())
                && let Some((_, t)) = s.textures.take()
            {
                t.release();
            }
            if s.textures.is_none() && s.textures_failed != Some(now) {
                let Some(d) = s.drawn[i].as_ref() else {
                    return;
                };
                let (w, h) = d.machine.texture_size;
                let frame = vec![d.machine.texture.clone()];
                match Textures::make(&frame, usize::from(w), usize::from(h), GBIX, false) {
                    Some(t) => s.textures = Some((garden, t)),
                    None => {
                        log!("gba link: the link machine's texture wasn't made");
                        s.textures_failed = Some(now);
                    }
                }
            }
            let (Some(d), Some((_, t))) = (s.drawn[i].as_ref(), s.textures.as_ref()) else {
                return;
            };
            // SAFETY: display pass on the game thread; our checked model and its textures.
            unsafe {
                crate::puppet::draw_solid(
                    d.root.as_ptr() as usize,
                    t.list(),
                    at,
                    [0, 0x8000i32.wrapping_sub(angle), 0],
                    1.0,
                    crate::puppet::Tilt::default(),
                    None,
                );
            }
            let species = match &s.list {
                Some(l) => l.species(),
                None => s.shown,
            };
            if let Some(species) = species {
                let (feet, height) = picture(&d.machine);
                billboard::draw_panel(species, world(at, angle, feet), angle, height);
            }
        });
    }

    /// Plays sound `id` (`0x437260`: ESI id, three zero cdecl arguments).
    fn play(id: u32) {
        // SAFETY: the game's sound call, on the game thread.
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
    }

    fn pressed(s: &mut State, key: i32) -> bool {
        match s.keys.iter_mut().find(|k| k.key() == key) {
            Some(k) => k.pressed(),
            None => {
                let mut k = Hotkey::new(key);
                let p = k.pressed();
                s.keys.push(k);
                p
            }
        }
    }

    fn repeating(s: &State, key: i32) -> bool {
        s.keys
            .iter()
            .find(|k| k.key() == key)
            .is_some_and(Hotkey::repeating)
    }

    /// This frame's presses on the list, from the menu buttons and the keyboard.
    fn keys(s: &mut State) -> Vec<Key> {
        let buttons = sa2::menu_buttons_pressed();
        fn held(s: &mut State, key: i32) -> bool {
            pressed(s, key) || repeating(s, key)
        }
        let mut out = Vec::new();
        if buttons & UP != 0 || held(s, VK_UP) {
            out.push(Key::Up);
        }
        if buttons & DOWN != 0 || held(s, VK_DOWN) {
            out.push(Key::Down);
        }
        if held(s, VK_PRIOR) {
            out.push(Key::PageUp);
        }
        if held(s, VK_NEXT) {
            out.push(Key::PageDown);
        }
        // Each key's state is read every frame, so a press isn't seen late.
        let (enter, escape, back) = (
            pressed(s, VK_RETURN),
            pressed(s, VK_ESCAPE),
            pressed(s, VK_BACK),
        );
        if buttons & PICK != 0 || enter {
            out.push(Key::Pick);
        }
        if buttons & BACK != 0 || escape || back {
            out.push(Key::Back);
        }
        out
    }

    fn close(s: &mut State) {
        if (s.list.take().is_some() || OPEN.load(Ordering::Relaxed))
            && !crate::together::is_holding()
        {
            sa2::set_controls(true);
        }
        OPEN.store(false, Ordering::Relaxed);
        REQUESTED.store(false, Ordering::Relaxed);
    }

    /// Rows that fit on screen.
    fn visible() -> usize {
        let size = hud::menu_font();
        let (_, h) = hud::screen_size();
        ((h / size) as usize).saturating_sub(12).clamp(3, 16)
    }

    /// The player was in reach last frame (for the log).
    static NEAR: AtomicBool = AtomicBool::new(false);

    pub fn on_frame() {
        let mut s = lock();
        let garden = sa2::current_chao_area();
        let here = enabled() && sa2::in_garden() && !crate::garden::server_runs();
        if !here {
            close(&mut s);
            remove();
            if !sa2::in_garden() {
                s.shown = None;
            }
            return;
        }
        if sa2::between_areas() {
            return;
        }
        let garden = garden as u8;
        let object = OBJECT.load(Ordering::Relaxed);
        if object != 0 && !in_list0(object) {
            // Freed with an old area without its delete: the box went with that area's ground.
            OBJECT.store(0, Ordering::Relaxed);
            COLLISION.store(0, Ordering::Relaxed);
        }
        if OBJECT.load(Ordering::Relaxed) != 0 && s.garden != garden {
            close(&mut s);
            s.shown = None;
            remove();
            return;
        }
        if OBJECT.load(Ordering::Relaxed) == 0 {
            close(&mut s);
            s.shown = None;
            if !spawn(&mut s, garden) {
                return;
            }
        }
        if REQUESTED.swap(false, Ordering::Relaxed) && s.list.is_none() {
            open(&mut s);
        }
        if s.list.is_some() {
            list_frame(&mut s, garden);
        } else {
            let near = usable();
            if near {
                hud::banner("Link machine: press the action button");
            }
            if NEAR.swap(near, Ordering::Relaxed) != near {
                log!(
                    "gba link: {} the link machine",
                    if near { "at" } else { "away from" }
                );
            }
        }
    }

    /// The page that `opened` (the last of the trail) opens; the first page without one.
    fn page(opened: Option<&Action>) -> Page {
        match opened {
            Some(Action::Moves) => Page::chao_list(),
            Some(&Action::ChaoMoves(identity)) => Page::chao_moves(identity),
            _ => {
                let mut page = Page::open();
                page.rows.insert(
                    0,
                    (
                        "Moves: choose the four moves each Chao battles with".into(),
                        Action::Moves,
                    ),
                );
                page.hints = vec![
                    "A / Enter: it comes out of the machine as a partner".into(),
                    "for your Chao (each once a day per garden, 4 at a time).".into(),
                    "Up/Down (PgUp/PgDn) pick. B / Esc / Backspace close.".into(),
                ];
                page
            }
        }
    }

    /// Back to the page before, its cursor on the row that left it; closes from the first.
    /// Whether the list is still open.
    fn up(s: &mut State) -> bool {
        let Some(left) = s.trail.pop() else {
            close(s);
            return false;
        };
        let mut list = List::new(page(s.trail.last()));
        if let Some(row) = list.page.rows.iter().position(|(_, a)| *a == left) {
            list.page.row = row;
        }
        s.list = Some(list);
        play(sound::CURSOR);
        true
    }

    fn open(s: &mut State) {
        s.trail.clear();
        s.list = Some(List::new(page(None)));
        s.age = 0;
        OPEN.store(true, Ordering::Relaxed);
        sa2::set_controls(false);
        // The keys already down don't count.
        for key in KEYS {
            let _ = pressed(s, key);
        }
        play(sound::CONFIRM);
    }

    fn list_frame(s: &mut State, garden: u8) {
        if crate::menu::is_open() || crate::chat::typing() {
            close(s);
            return;
        }
        // Keep the controls off (something else ending mustn't turn them on mid-pick).
        sa2::set_controls(false);
        let keys = keys(s);
        if sa2::game_state() == PAUSED {
            // Hidden under the pause menu; the press that leaves it isn't a pick.
            s.paused = true;
            return;
        }
        if std::mem::take(&mut s.paused) {
            s.age = 0;
        }
        s.age += 1;
        let visible = visible();
        if s.age > 2 {
            for key in keys {
                let Some(list) = s.list.as_mut() else {
                    return;
                };
                match list.press(key, visible) {
                    Outcome::Stay => {}
                    Outcome::Moved => play(sound::CURSOR),
                    Outcome::Close => {
                        if !up(s) {
                            return;
                        }
                    }
                    Outcome::Chose(Action::Back) => {
                        if !up(s) {
                            return;
                        }
                    }
                    Outcome::Chose(action @ (Action::Moves | Action::ChaoMoves(_))) => {
                        s.list = Some(List::new(page(Some(&action))));
                        s.trail.push(action);
                        play(sound::CONFIRM);
                    }
                    Outcome::Chose(Action::Toggle { identity, id }) => toggle(s, identity, id),
                    Outcome::Chose(action) => {
                        if chose(s, garden, action) {
                            close(s);
                            return;
                        }
                    }
                }
            }
        }
        if let Some(list) = s.list.as_mut() {
            list.scroll_to(visible);
            draw(list, visible);
        }
    }

    /// Switches a Chao's move on or off for battles (kept at once); the page stays as it was
    /// with the change, or says why it can't.
    fn toggle(s: &mut State, identity: u64, id: u16) {
        let result = super::super::toggle_move(identity, id);
        let Some(list) = s.list.as_mut() else {
            return;
        };
        match result {
            Ok(on) => {
                let (row, scroll) = (list.page.row, list.page.scroll);
                *list = List::new(page(s.trail.last()));
                list.page.row = row.min(list.page.rows.len().saturating_sub(1));
                list.page.scroll = scroll;
                play(if on { sound::CONFIRM } else { sound::CURSOR });
            }
            Err(why) => list.message = Some(why.message().to_string()),
        }
    }

    /// Brings out the one picked (taking the gift first). Whether the list closes.
    fn chose(s: &mut State, garden: u8, action: Action) -> bool {
        let Some((at, angle)) = spot(garden) else {
            return true;
        };
        let Some(exit) = s.drawn[usize::from(garden - 1)]
            .as_ref()
            .map(|d| world(at, angle, d.machine.exit))
        else {
            return true;
        };
        let gift = matches!(action, Action::Gift(_));
        let result = match action {
            Action::Bring(c) => super::super::bring_out(&c, exit, angle),
            Action::Gift(species) => super::super::take_gift_candidate(species)
                .and_then(|c| super::super::bring_out(&c, exit, angle)),
            _ => return false,
        };
        match result {
            Ok(message) => {
                play(sound::CONFIRM);
                s.shown = s.list.as_ref().and_then(List::species);
                hud::toast(message);
                true
            }
            Err(e) => {
                if gift {
                    // A gift taken but not brought out is among the kept ones now.
                    s.list = Some(List::new(page(None)));
                }
                if let Some(list) = s.list.as_mut() {
                    list.message = Some(e);
                }
                false
            }
        }
    }

    fn draw(list: &List, visible: usize) {
        let size = hud::menu_font();
        let page = &list.page;
        let mut lines: Vec<(String, i32)> = vec![(format!("== {} ==", page.title), COLOR_TITLE)];
        for (i, (text, _)) in page.rows.iter().enumerate().skip(page.scroll).take(visible) {
            let selected = i == page.row;
            lines.push((
                format!("{} {text}", if selected { ">" } else { " " }),
                if selected { COLOR_SELECTED } else { COLOR_TEXT },
            ));
        }
        if page.rows.len() > visible {
            lines.push((
                format!(
                    "Showing {}-{} of {}",
                    page.scroll + 1,
                    (page.scroll + visible).min(page.rows.len()),
                    page.rows.len()
                ),
                COLOR_HINT,
            ));
        }
        if let Some(m) = &list.message {
            lines.push((m.clone(), COLOR_MESSAGE));
        }
        lines.extend(page.hints.iter().map(|h| (h.clone(), COLOR_HINT)));
        let cols = lines
            .iter()
            .map(|(t, _)| t.chars().count())
            .max()
            .unwrap_or(0)
            + 2;
        let (x, y) = (size * 2.0, size * 3.0);
        hud::panel(x, y, size, cols, lines.len() + 2);
        for (i, (text, color)) in lines.iter().enumerate() {
            hud::text(x + size, y + (i + 1) as f32 * size, size, *color, text);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::{Candidate, Source};
    use super::*;

    fn bring(species: u16) -> Action {
        Action::Bring(Candidate {
            source: Source::Kept,
            key: u64::from(species),
            species,
            level: 5,
            moves: [0; 4],
            ability_slot: 0,
        })
    }

    fn list(rows: Vec<Action>) -> List {
        List::new(Page {
            title: String::new(),
            hints: Vec::new(),
            rows: rows
                .into_iter()
                .enumerate()
                .map(|(i, a)| (format!("row {i}"), a))
                .collect(),
            row: 3,
            scroll: 2,
        })
    }

    #[test]
    fn the_cursor_only_lands_on_rows_to_pick() {
        let mut l = list(vec![
            Action::None,
            bring(10),
            Action::None,
            bring(20),
            Action::Gift(30),
        ]);
        assert_eq!((l.page.row, l.page.scroll), (1, 0));
        assert_eq!(l.species(), Some(10));
        assert_eq!(l.press(Key::Down, 10), Outcome::Moved);
        assert_eq!(l.species(), Some(20));
        l.press(Key::Down, 10);
        assert_eq!(l.species(), Some(30));
        // Wraps past the message rows.
        l.press(Key::Down, 10);
        assert_eq!(l.page.row, 1);
        l.press(Key::Up, 10);
        assert_eq!(l.page.row, 4);
        assert_eq!(l.press(Key::Pick, 10), Outcome::Chose(Action::Gift(30)));
        assert_eq!(l.press(Key::Back, 10), Outcome::Close);
    }

    #[test]
    fn nothing_to_pick() {
        let mut l = list(vec![Action::None, Action::None]);
        assert_eq!(l.page.row, 0);
        assert_eq!(l.species(), None);
        assert_eq!(l.press(Key::Down, 5), Outcome::Stay);
        assert_eq!(l.press(Key::Pick, 5), Outcome::Stay);
        assert_eq!(list(Vec::new()).press(Key::PageDown, 5), Outcome::Stay);
    }

    #[test]
    fn pages_and_scrolling() {
        let mut rows: Vec<Action> = (0..20).map(|i| bring(i + 1)).collect();
        rows[5] = Action::None;
        let mut l = list(rows);
        l.press(Key::PageDown, 5);
        // Row 5 can't be picked: the nearest one is 4.
        assert_eq!(l.page.row, 4);
        l.press(Key::PageDown, 5);
        assert_eq!(l.page.row, 9);
        assert_eq!(l.page.scroll, 5);
        for _ in 0..5 {
            l.press(Key::PageDown, 5);
        }
        assert_eq!(l.page.row, 19);
        assert_eq!(l.page.scroll, 15);
        l.press(Key::PageUp, 5);
        assert_eq!(l.page.row, 14);
        l.press(Key::Up, 5);
        l.press(Key::Up, 5);
        l.press(Key::Up, 5);
        l.press(Key::Up, 5);
        l.press(Key::Up, 5);
        assert_eq!(l.page.row, 9);
        assert_eq!(l.page.scroll, 9);
    }

    #[test]
    fn every_garden_has_a_machine() {
        for garden in 1..=3 {
            let m = model(garden).unwrap();
            let (at, angle) = spot(garden).unwrap();
            // The screen faces the front, inside the box; things come out in front of it.
            assert!(m.screen.width > 2.0 && m.screen.height > 2.0);
            assert!(m.screen.centre[2] < m.box_max[2] && m.screen.centre[2] > m.box_min[2]);
            assert!(m.exit[2] > m.box_max[2]);
            assert!(m.box_min[1] == 0.0 && m.box_max[1] > 8.0);
            // The player reaches it from any side, whichever way they face.
            let bounds = (m.box_min, m.box_max);
            let front = world(at, angle, [0.0, 0.0, m.box_max[2] + 2.0]);
            let out = in_reach(at, angle, bounds, front).unwrap();
            assert!((out - 2.0).abs() < 1e-3);
            let beside = world(at, angle, [m.box_max[0] + 2.0, 0.0, 0.0]);
            assert!(in_reach(at, angle, bounds, beside).is_some());
            let behind = world(at, angle, [0.0, 0.0, m.box_min[2] - 2.0]);
            assert!(in_reach(at, angle, bounds, behind).is_some());
            // Not too far, nor up high.
            let far = world(at, angle, [0.0, 0.0, m.box_max[2] + 6.0]);
            assert!(in_reach(at, angle, bounds, far).is_none());
            let corner = world(at, angle, [m.box_max[0] + 3.0, 0.0, m.box_max[2] + 3.0]);
            assert!(in_reach(at, angle, bounds, corner).is_none());
            let high = world(at, angle, [0.0, 6.0, m.box_max[2] + 2.0]);
            assert!(in_reach(at, angle, bounds, high).is_none());
            assert_eq!(machine_box(garden), Some(bounds));
            let (feet, height) = picture(&m);
            assert!(feet[2] > m.screen.centre[2] && height <= m.screen.height);
        }
        assert!(spot(0).is_none() && spot(4).is_none() && model(4).is_none());
    }

    #[test]
    fn the_front_turns_like_the_game_turns_models() {
        // Angle 0x4000: +Z goes to +X, +X to -Z.
        let t = turn([0.0, 1.0, 1.0], 0x4000);
        assert!((t[0] - 1.0).abs() < 1e-5 && t[1] == 1.0 && t[2].abs() < 1e-5);
        let t = turn([1.0, 0.0, 0.0], 0x4000);
        assert!(t[0].abs() < 1e-5 && (t[2] + 1.0).abs() < 1e-5);
        // Neutral's machine faces the middle of its Chao land, (-5, -65).
        let (at, angle) = spot(1).unwrap();
        let f = turn([0.0, 0.0, 1.0], angle);
        let (dx, dz) = (-5.0 - at[0], -65.0 - at[2]);
        let len = (dx * dx + dz * dz).sqrt();
        assert!((f[0] - dx / len).abs() < 1e-3 && (f[2] - dz / len).abs() < 1e-3);
    }

    #[test]
    fn the_box_faces_point_out() {
        let (c, tris) = box_mesh([-1.0, 0.0, -2.0], [1.0, 3.0, 2.0], 0x1234);
        let mid: [f32; 3] = std::array::from_fn(|i| c.iter().map(|p| p[i]).sum::<f32>() / 8.0);
        for t in tris {
            let [a, b, d] = t.map(|i| c[usize::from(i)]);
            let u = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
            let v = [d[0] - a[0], d[1] - a[1], d[2] - a[2]];
            let n = [
                u[1] * v[2] - u[2] * v[1],
                u[2] * v[0] - u[0] * v[2],
                u[0] * v[1] - u[1] * v[0],
            ];
            let centre: [f32; 3] = std::array::from_fn(|i| (a[i] + b[i] + d[i]) / 3.0 - mid[i]);
            let dot: f32 = (0..3).map(|i| n[i] * centre[i]).sum();
            assert!(dot > 0.0, "{t:?}");
        }
    }
}
