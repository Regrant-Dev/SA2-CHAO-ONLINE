//! The link machine (re/chao-garden-objects.md): one machine in each garden, near its
//! transporter (Neutral plain, Hero white and gold, Dark spiky; models in `assets/link_machine/`,
//! `machine_model`). Walking up to it and pressing the action button opens the pick list: the
//! GBA save's creatures, the ones the mod keeps and, without a save, the one-time gift (the
//! rows of `Page`). The machine's screen shows the picture of the one highlighted, and the one
//! picked hops out of the machine's hatch into the garden (`super::bring_out`). Its first row,
//! "Moves, type & ability", lists the player's Chao by name, then what one battles with: its
//! moves (four), types (one or two) and ability (or none), each a page to choose from
//! (`Page::chao_list`, `Page::chao_choices`). Its second, "Who can come here", lists each
//! garden's wild area now and next and every creature that can come from it with its odds
//! (`Page::who_gardens`, `Page::who_comes`). Every page takes the keyboard search and jumps of
//! `crate::search`. A shared garden has it while it shows its
//! shared visitors (`super::shared`): each player uses it for themselves, and the one picked
//! comes out as a shared visitor once the server takes it.
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
use crate::chao_sim::land::{Tri, probe};
use crate::search::{self, Edit, Search, Typed};

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

/// Where each garden's machine stands (its floor) and the Y angle (BAMS) its front faces when
/// the garden's ground can't be searched (`find_spot`): 20 to 25 units from the transporter on
/// flat ground, the front toward the garden.
pub(crate) const SPOTS: [([f32; 3], i32); 3] = [
    ([50.0, 10.0, 22.0], 0x96F8),
    ([117.0, 0.0, 117.0], 0xA6C0),
    ([-98.0, 3.5, -98.0], 0x10D9),
];

/// The player can use the machine from this far from the circle round its box (across, any
/// side) and this far from its floor (up).
const REACH: f32 = 3.5;
const REACH_UP: f32 = 5.0;
/// The picture fills this much of the screen's height, and stands this far in front of it.
const PICTURE_FILL: f32 = 0.9;
const PICTURE_OUT: f32 = 0.06;

/// Each garden's transporter (re/chao-garden-objects.md §2) and the middle of its Chao land.
const TRANSPORTERS: [[f32; 3]; 3] = [
    [72.0, 10.0, 28.0],
    [135.0, 0.0, 133.0],
    [-116.0, 3.5, -113.0],
];
const MIDDLES: [[f32; 2]; 3] = [[-5.0, -65.0], [-75.0, -20.0], [-55.0, 0.0]];

/// Where `find_spot` looks: this far from the transporter, and turned this far (degrees, either
/// way) from the line toward the garden's middle, where the transporter's doorway is.
const SPOT_RADII: (u32, u32) = (24, 40);
const SPOT_TURNS: (u32, u32) = (70, 110);
/// The machine's footprint (across) must be flat floor this far round its spot, at most this
/// far from the transporter's floor (up), clear of trees and toys, with no roof over it.
const FOOT: f32 = 4.5;
const FLAT: f32 = 0.6;
const LEVEL: f32 = 2.0;
const TREE_CLEAR: f32 = 12.0;
const TOY_CLEAR: f32 = 8.0;
const ROOF_CLEAR: f32 = 15.0;

/// The spots found in each garden's ground (`find_spot`), used over `SPOTS` once known.
static FOUND: std::sync::Mutex<[Option<Spot>; 3]> = std::sync::Mutex::new([None; 3]);

/// A machine's floor position and the Y angle (BAMS) its front faces.
type Spot = ([f32; 3], i32);

/// The garden's machine spot: (floor position, angle), for gardens 1..=3.
pub fn spot(garden: u8) -> Option<([f32; 3], i32)> {
    let i = usize::from(garden).checked_sub(1)?;
    let found = FOUND
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .get(i)
        .copied()
        .flatten();
    found.or_else(|| SPOTS.get(i).copied())
}

/// Garden `garden`'s spot has been decided (searched for, or given up on).
pub fn decided(garden: u8) -> bool {
    usize::from(garden).checked_sub(1).is_some_and(|i| {
        FOUND
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(i)
            .is_some_and(Option::is_some)
    })
}

/// Sets garden `garden`'s spot from its ground (`None`: the fixed one).
pub fn set_found(garden: u8, spot: Option<([f32; 3], i32)>) {
    if let Some(i) = usize::from(garden).checked_sub(1) {
        let mut f = FOUND.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(slot) = f.get_mut(i) {
            *slot = spot.or_else(|| SPOTS.get(i).copied());
        }
    }
}

/// The floor under (x, z) seen from `y` (down): its height, if it's solid, level ground (not
/// water).
fn floor(tris: &[Tri], x: f32, y: f32, z: f32) -> Option<f32> {
    let hit = probe(tris, [x, y, z])[2]?;
    (!hit.water() && hit.normal[1].abs() > 0.97).then_some(hit.at)
}

/// The Y angle (BAMS) that turns a model's +Z from `at` toward `to` (as `turn`).
fn facing(at: [f32; 3], to: [f32; 2]) -> i32 {
    let a = (to[0] - at[0]).atan2(to[1] - at[2]);
    ((a * 65536.0 / std::f32::consts::TAU) as i32) & 0xFFFF
}

/// A spot for garden `garden`'s machine in its ground `tris` (with its trees and toys): beside
/// the transporter, not in front of its doorway, on flat solid floor at the transporter's level
/// with room round it and no roof, its front toward the garden's middle. The one nearest a
/// quarter turn from the doorway and a short walk from the transporter.
pub fn find_spot(
    garden: u8,
    tris: &[Tri],
    trees: &[[f32; 3]],
    toys: &[[f32; 3]],
) -> Option<([f32; 3], i32)> {
    let i = usize::from(garden).checked_sub(1)?;
    let (t, middle) = (*TRANSPORTERS.get(i)?, *MIDDLES.get(i)?);
    let toward = (middle[0] - t[0]).atan2(middle[1] - t[2]);
    let mut best: Option<(f32, [f32; 3])> = None;
    for r in (SPOT_RADII.0..=SPOT_RADII.1).step_by(2) {
        for turn in (SPOT_TURNS.0..=SPOT_TURNS.1).step_by(10) {
            for side in [-1.0f32, 1.0] {
                let a = toward + side * (turn as f32).to_radians();
                let (x, z) = (t[0] + a.sin() * r as f32, t[2] + a.cos() * r as f32);
                let Some(y) = floor(tris, x, t[1] + 8.0, z) else {
                    continue;
                };
                if (y - t[1]).abs() > LEVEL {
                    continue;
                }
                let flat = (0..8).all(|k| {
                    let b = k as f32 * std::f32::consts::TAU / 8.0;
                    floor(tris, x + b.sin() * FOOT, y + 3.0, z + b.cos() * FOOT)
                        .is_some_and(|h| (h - y).abs() < FLAT)
                });
                let roof = probe(tris, [x, y + 0.5, z])[3].is_some_and(|h| h.at < y + ROOF_CLEAR);
                let near = |list: &[[f32; 3]], clear: f32| {
                    list.iter()
                        .any(|p| (p[0] - x).powi(2) + (p[2] - z).powi(2) < clear * clear)
                };
                if !flat || roof || near(trees, TREE_CLEAR) || near(toys, TOY_CLEAR) {
                    continue;
                }
                let score = (turn as f32 - 90.0).abs() / 10.0 + (r as f32 - 28.0).abs() / 2.0;
                if best.is_none_or(|(s, _)| score < s) {
                    best = Some((score, [x, y, z]));
                }
            }
        }
    }
    best.map(|(_, at)| (at, facing(at, middle)))
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
/// from the circle round its box (across). The circle doesn't depend on which way the box
/// turns, so the reach matches the machine as drawn whatever its turn.
pub fn in_reach(at: [f32; 3], angle: i32, (min, max): Bounds, player: [f32; 3]) -> Option<f32> {
    let mid = world(at, angle, std::array::from_fn(|i| (min[i] + max[i]) / 2.0));
    let half = [(max[0] - min[0]) / 2.0, (max[2] - min[2]) / 2.0];
    let radius = (half[0] * half[0] + half[1] * half[1]).sqrt();
    let (dx, dz) = (player[0] - mid[0], player[2] - mid[2]);
    let out = ((dx * dx + dz * dz).sqrt() - radius).max(0.0);
    (out < REACH && (player[1] - at[1]).abs() < REACH_UP).then_some(out)
}

/// A machine's collision box: its corners (min, max) in its own axes.
type Bounds = ([f32; 3], [f32; 3]);

/// Garden `garden`'s machine box (its model's), read once.
pub(crate) fn machine_box(garden: u8) -> Option<Bounds> {
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
    /// Left and Right: `search::JUMP` rows up or down.
    JumpUp,
    JumpDown,
    Pick,
    Back,
}

/// What a press did.
#[derive(Clone, Debug, PartialEq)]
pub enum Outcome {
    Stay,
    Moved,
    /// The search changed (the cursor may have moved with it).
    Searched,
    Close,
    Chose(Action),
}

/// The pick list: `Page`'s rows that match the search, the cursor only on rows with something
/// to pick.
pub struct List {
    pub page: Page,
    /// The last refusal, shown under the rows.
    pub message: Option<String>,
    pub search: Search,
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
            search: Search::default(),
        }
    }

    /// The rows that show (the search's).
    pub fn shown(&self) -> Vec<usize> {
        self.search
            .shown((0..self.page.rows.len()).map(|i| self.page.name(i)))
    }

    /// The rows that show and can be picked.
    fn candidates(&self) -> Vec<usize> {
        self.shown()
            .into_iter()
            .filter(|&i| pickable(&self.page.rows[i].1))
            .collect()
    }

    /// The cursor's row shows and can be picked.
    fn on_candidate(&self) -> bool {
        self.candidates().contains(&self.page.row)
    }

    /// `by` shown rows up or down from the cursor (stopping at the ends), then the pickable row
    /// nearest there, on the earlier side first.
    fn toward(&self, by: usize, forward: bool) -> usize {
        let shown = self.shown();
        let Some(last) = shown.len().checked_sub(1) else {
            return self.page.row;
        };
        let at = shown
            .iter()
            .position(|&i| i == self.page.row)
            .unwrap_or_else(|| shown.partition_point(|&i| i < self.page.row).min(last));
        let target = if forward {
            (at + by).min(last)
        } else {
            at.saturating_sub(by)
        };
        let mut best = self.page.row;
        let mut gap = usize::MAX;
        for (place, &i) in shown.iter().enumerate() {
            if pickable(&self.page.rows[i].1) && place.abs_diff(target) < gap {
                gap = place.abs_diff(target);
                best = i;
            }
        }
        best
    }

    /// Handles one press; `visible` rows fit on screen.
    pub fn press(&mut self, key: Key, visible: usize) -> Outcome {
        let before = self.page.row;
        match key {
            Key::Up | Key::Down => {
                self.page.row = search::step(self.page.row, &self.candidates(), key == Key::Down);
            }
            Key::PageUp | Key::PageDown => {
                self.page.row = self.toward(visible.max(1), key == Key::PageDown);
            }
            Key::JumpUp | Key::JumpDown => {
                self.page.row = self.toward(search::JUMP, key == Key::JumpDown);
            }
            Key::Back => return Outcome::Close,
            Key::Pick => {
                return match self.page.rows.get(self.page.row) {
                    Some((_, a)) if pickable(a) && self.on_candidate() => Outcome::Chose(a.clone()),
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

    /// This frame's typing (`search::Search::edit`; a fresh Backspace with nothing to delete,
    /// like Esc with nothing to clear, closes the page). The cursor stays on a row that shows.
    pub fn typed(&mut self, typed: &Typed, visible: usize) -> Outcome {
        match self.search.edit(typed, true) {
            Edit::None => Outcome::Stay,
            Edit::Back => Outcome::Close,
            Edit::Changed => {
                self.page.row = search::settle(self.page.row, &self.candidates());
                self.page.scroll = 0;
                self.message = None;
                self.scroll_to(visible);
                Outcome::Searched
            }
        }
    }

    /// Scrolls so the cursor's row shows among `visible` rows (`scroll` counts the rows that
    /// show).
    pub fn scroll_to(&mut self, visible: usize) {
        let visible = visible.max(1);
        let shown = self.shown();
        let at = shown.iter().position(|&i| i == self.page.row).unwrap_or(0);
        let p = &mut self.page;
        p.scroll = p.scroll.min(shown.len().saturating_sub(1));
        if at < p.scroll {
            p.scroll = at;
        } else if at >= p.scroll + visible {
            p.scroll = at + 1 - visible;
        }
    }

    /// The species of the highlighted row, for the machine's screen.
    pub fn species(&self) -> Option<u16> {
        if !self.on_candidate() {
            return None;
        }
        match self.page.rows.get(self.page.row)?.1 {
            Action::Bring(ref c) => Some(c.species),
            Action::Gift(s) | Action::Look(s) => Some(s),
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
    use super::super::{Action, Page, nav_hints};
    use super::{
        Key, List, Outcome, REACH_UP, box_mesh, enabled, machine_box, model, picture, spot, world,
    };
    use crate::game as sa2;
    use crate::hud;
    use crate::input::Hotkey;
    use crate::log;
    use crate::search::{self, Keyboard, Owner, Typed};

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
    /// Menu buttons (`MenuButtons_Pressed`): B back, A pick, up, down, left, right.
    const BACK: u32 = 0x02;
    const PICK: u32 = 0x04;
    const UP: u32 = 0x10;
    const DOWN: u32 = 0x20;
    const LEFT: u32 = 0x40;
    const RIGHT: u32 = 0x80;
    /// The pause screen's GameState.
    const PAUSED: i16 = 17;
    /// Registry categories the player can pick up: Chao, eggs, animals and drives, fruit,
    /// seeds, hats.
    const LIFTABLE: [usize; 6] = [0, 1, 2, 3, 7, 9];
    /// Something to pick up this close to the player (across) takes the action button instead
    /// of the machine.
    const LIFT_NEAR: f32 = 2.0;
    /// The swatch textures' global index (billboard's pictures and battle effects are below).
    const GBIX: u32 = 0x7E5C_0000;
    const PLAY_SOUND: usize = 0x0043_7260;
    /// Menu sounds (as the battle menus use them).
    mod sound {
        pub const CURSOR: u32 = 0x8000;
        pub const CONFIRM: u32 = 0x100E;
    }

    const VK_LEFT: i32 = 0x25;
    const VK_UP: i32 = 0x26;
    const VK_RIGHT: i32 = 0x27;
    const VK_DOWN: i32 = 0x28;
    const KEYS: [i32; 4] = [VK_LEFT, VK_UP, VK_RIGHT, VK_DOWN];

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
        /// The rows picked to open the pages over the first one ("Moves, type & ability", a
        /// Chao, then one of its lists).
        trail: Vec<Action>,
        /// Frames since the list opened or the game was last paused (the opening and resuming
        /// presses aren't picks).
        age: u32,
        /// The game was paused at the list's last frame.
        paused: bool,
        /// The picture on the screen while the list is closed.
        shown: Option<u16>,
        keys: Vec<Hotkey>,
        /// Typing, Enter, Esc and PgUp/PgDn (read every frame).
        keyboard: Option<Keyboard>,
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
        keyboard: None,
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

    /// Frames the garden's ground has been unreadable while its spot waits for it.
    static GROUND_WAIT: AtomicUsize = AtomicUsize::new(0);
    /// Frames to wait for the ground before taking the fixed spot.
    const GROUND_WAIT_MAX: usize = 300;

    /// Searches garden `garden`'s ground for the machine's spot once: false while the ground
    /// can't be read yet.
    fn decide_spot(garden: u8) -> bool {
        if super::decided(garden) {
            return true;
        }
        match crate::land_share::read(usize::from(garden - 1)) {
            Some(g) => {
                let toys: Vec<[f32; 3]> = g.toys.iter().map(|t| t.pos).collect();
                let found = super::find_spot(garden, &g.tris, &g.tree_spots, &toys);
                match found {
                    Some((at, angle)) => log!(
                        "gba link: link machine spot in garden {garden}: ({:.0}, {:.1}, {:.0}) {angle:#x}",
                        at[0],
                        at[1],
                        at[2]
                    ),
                    None => log!(
                        "gba link: no free ground found in garden {garden}; machine at its fixed spot"
                    ),
                }
                super::set_found(garden, found);
                GROUND_WAIT.store(0, Ordering::Relaxed);
                true
            }
            None if GROUND_WAIT.fetch_add(1, Ordering::Relaxed) >= GROUND_WAIT_MAX => {
                log!("gba link: garden {garden}'s ground can't be read; machine at its fixed spot");
                super::set_found(garden, None);
                GROUND_WAIT.store(0, Ordering::Relaxed);
                true
            }
            None => false,
        }
    }

    fn spawn(s: &mut State, garden: u8) -> bool {
        if !decide_spot(garden) {
            return false;
        }
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
        if super::in_reach(at, angle, bounds, player).is_none() {
            return false;
        }
        crate::garden::player_held() == 0
            && !crate::menu::is_open()
            && !crate::chat::typing()
            && sa2::game_state() != PAUSED
            && !liftable_near(player, LIFT_NEAR)
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

    /// This frame's presses on the list, from the menu buttons and the keyboard, and its
    /// typing. While a typing key is down the menu buttons don't count (the game's letter keys
    /// press them too).
    fn keys(s: &mut State) -> (Vec<Key>, Typed) {
        let typed = s.keyboard.get_or_insert_with(Keyboard::default).read();
        let buttons = if typed.held {
            0
        } else {
            sa2::menu_buttons_pressed()
        };
        fn held(s: &mut State, key: i32) -> bool {
            pressed(s, key) || repeating(s, key)
        }
        let mut out = Vec::new();
        // Each key's state is read every frame, so a press isn't seen late.
        let arrows = KEYS.map(|k| held(s, k));
        let [left, up, right, down] = arrows;
        if buttons & UP != 0 || up {
            out.push(Key::Up);
        }
        if buttons & DOWN != 0 || down {
            out.push(Key::Down);
        }
        if buttons & LEFT != 0 || left {
            out.push(Key::JumpUp);
        }
        if buttons & RIGHT != 0 || right {
            out.push(Key::JumpDown);
        }
        if typed.page_up {
            out.push(Key::PageUp);
        }
        if typed.page_down {
            out.push(Key::PageDown);
        }
        if buttons & PICK != 0 || typed.enter {
            out.push(Key::Pick);
        }
        if buttons & BACK != 0 {
            out.push(Key::Back);
        }
        (out, typed)
    }

    fn close(s: &mut State) {
        if (s.list.take().is_some() || OPEN.load(Ordering::Relaxed))
            && !crate::together::is_holding()
        {
            sa2::set_controls(true);
        }
        OPEN.store(false, Ordering::Relaxed);
        REQUESTED.store(false, Ordering::Relaxed);
        search::take_keyboard(Owner::LinkMachine, false);
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
        // A shared garden has it while it shows its shared visitors (`shared`).
        let shared = crate::garden::server_runs() || crate::bring::friends_in(garden);
        let here = enabled() && sa2::in_garden() && (!shared || super::super::shared::showing());
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
            Some(Action::ChaoList) => Page::chao_list(),
            Some(&Action::Chao(identity)) => Page::chao_choices(identity),
            Some(&Action::ChaoMoves(identity)) => Page::chao_moves(identity),
            Some(&Action::ChaoTypes(identity)) => Page::chao_types(identity),
            Some(&Action::ChaoAbilities(identity)) => Page::chao_abilities(identity),
            Some(Action::WhoComes) => Page::who_gardens(sa2::current_chao_area() as u8),
            Some(&Action::WhoNow(garden)) => Page::who_comes_now(garden, false),
            Some(&Action::WhoNext(garden)) => Page::who_comes_now(garden, true),
            _ => {
                let mut page = Page::open();
                page.push_front(
                    "Who can come here: each garden's wild visitors".into(),
                    Action::WhoComes,
                );
                page.push_front(
                    "Moves, type & ability: choose what each Chao battles with".into(),
                    Action::ChaoList,
                );
                page.hints = [
                    "A / Enter: it comes out of the machine as a partner".into(),
                    "for your Chao (each once a day per garden, 4 at a time).".into(),
                ]
                .into_iter()
                .chain(nav_hints("close"))
                .collect();
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
        search::take_keyboard(Owner::LinkMachine, true);
        // The keys already down don't count.
        for key in KEYS {
            let _ = pressed(s, key);
        }
        let _ = s.keyboard.get_or_insert_with(Keyboard::default).read();
        play(sound::CONFIRM);
    }

    fn list_frame(s: &mut State, garden: u8) {
        if crate::menu::is_open() || crate::chat::typing() {
            close(s);
            return;
        }
        // Keep the controls off (something else ending mustn't turn them on mid-pick).
        sa2::set_controls(false);
        search::take_keyboard(Owner::LinkMachine, true);
        let (keys, typed) = keys(s);
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
            let typing = s
                .list
                .as_mut()
                .map_or(Outcome::Stay, |l| l.typed(&typed, visible));
            if !outcome(s, garden, typing) {
                return;
            }
            for key in keys {
                let Some(list) = s.list.as_mut() else {
                    return;
                };
                let done = list.press(key, visible);
                if !outcome(s, garden, done) {
                    return;
                }
            }
        }
        if let Some(list) = s.list.as_mut() {
            list.scroll_to(visible);
            draw(list, visible);
        }
    }

    /// What a press did on the list. Whether the list is still open.
    fn outcome(s: &mut State, garden: u8, done: Outcome) -> bool {
        use super::super::{choose_ability, toggle_move, toggle_type};
        match done {
            Outcome::Stay | Outcome::Searched => {}
            Outcome::Moved => play(sound::CURSOR),
            Outcome::Close | Outcome::Chose(Action::Back) => return up(s),
            // A creature that can come: the machine's screen shows it, nothing more.
            Outcome::Chose(Action::Look(_)) => {}
            Outcome::Chose(action) if action.opens_page() => {
                s.list = Some(List::new(page(Some(&action))));
                s.trail.push(action);
                play(sound::CONFIRM);
            }
            Outcome::Chose(Action::Toggle { identity, id }) => {
                changed(s, toggle_move(identity, id).map_err(|e| e.message()));
            }
            Outcome::Chose(Action::ToggleType { identity, id }) => {
                changed(s, toggle_type(identity, id).map_err(|e| e.message()));
            }
            Outcome::Chose(Action::Ability { identity, id }) => {
                let result = choose_ability(identity, id).map(|()| true);
                changed(s, result.map_err(|e| e.message()));
            }
            Outcome::Chose(action) => {
                if chose(s, garden, action) {
                    close(s);
                    return false;
                }
            }
        }
        true
    }

    /// After a Chao's move, type or ability was switched (kept at once): the page shows the
    /// change with its cursor, scroll and search as they were, or says why it can't (`result`:
    /// whether it's on now, or why not).
    fn changed(s: &mut State, result: Result<bool, &'static str>) {
        let fresh = page(s.trail.last());
        let Some(list) = s.list.as_mut() else {
            return;
        };
        match result {
            Ok(on) => {
                let (row, scroll) = (list.page.row, list.page.scroll);
                let search = std::mem::take(&mut list.search);
                *list = List::new(fresh);
                list.search = search;
                list.page.row = row.min(list.page.rows.len().saturating_sub(1));
                list.page.scroll = scroll;
                play(if on { sound::CONFIRM } else { sound::CURSOR });
            }
            Err(why) => list.message = Some(why.to_string()),
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
        let shown = list.shown();
        let mut lines: Vec<(String, i32)> = vec![(format!("== {} ==", page.title), COLOR_TITLE)];
        if !list.search.is_empty() {
            lines.push((format!("Search: {}_", list.search.text()), COLOR_TITLE));
        }
        for &i in shown.iter().skip(page.scroll).take(visible) {
            let selected = i == page.row;
            lines.push((
                format!("{} {}", if selected { ">" } else { " " }, page.rows[i].0),
                if selected { COLOR_SELECTED } else { COLOR_TEXT },
            ));
        }
        if shown.is_empty() {
            lines.push(("  No matches".into(), COLOR_HINT));
        }
        if shown.len() > visible {
            lines.push((
                format!(
                    "Showing {}-{} of {}",
                    page.scroll + 1,
                    (page.scroll + visible).min(shown.len()),
                    shown.len()
                ),
                COLOR_HINT,
            ));
        }
        if let Some(note) = page.note(page.row).filter(|_| shown.contains(&page.row)) {
            lines.push((note.to_string(), COLOR_TEXT));
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
            names: (0..rows.len()).map(|i| Some(format!("Row {i}"))).collect(),
            notes: Vec::new(),
            rows: rows
                .into_iter()
                .enumerate()
                .map(|(i, a)| (format!("row {i}"), a))
                .collect(),
            row: 3,
            scroll: 2,
        })
    }

    fn typed(text: &str) -> Typed {
        Typed {
            text: text.into(),
            ..Typed::default()
        }
    }

    #[test]
    fn who_can_come_asks_which_garden_ours_first() {
        let page = Page::who_gardens(2);
        let actions: Vec<Action> = page.rows.iter().map(|(_, a)| a.clone()).collect();
        assert_eq!(
            actions,
            [Action::WhoNow(2), Action::WhoNow(1), Action::WhoNow(3)]
        );
        assert!(page.rows[0].0.ends_with("(here)"));
        assert_eq!(page.name(1), Some("Neutral Garden"));
        assert_eq!(Page::who_gardens(1).rows[0].1, Action::WhoNow(1));
        // Out of a garden: all three, in order.
        assert_eq!(Page::who_gardens(0).rows.len(), 3);
        assert_eq!(Page::who_gardens(0).rows[0].1, Action::WhoNow(1));
        // These open pages; a creature that can come doesn't, and shows on the screen.
        assert!(Action::WhoComes.opens_page() && Action::WhoNow(1).opens_page());
        assert!(Action::WhoNext(3).opens_page() && !Action::Look(5).opens_page());
        let mut l = list(vec![Action::None, Action::Look(7), Action::Look(8)]);
        assert_eq!(l.species(), Some(7));
        assert_eq!(l.press(Key::Pick, 5), Outcome::Chose(Action::Look(7)));
        // Searching for the garden: by its name.
        let mut l = List::new(Page::who_gardens(3));
        assert_eq!(l.typed(&typed("hero"), 5), Outcome::Searched);
        assert_eq!(l.shown(), [2]);
        assert_eq!(l.page.rows[l.page.row].1, Action::WhoNow(2));
    }

    fn rom() -> Option<crate::gba::rom::Rom> {
        let home = std::env::var_os("HOME")?;
        let path = std::path::Path::new(&home).join("Games/gba/emerald.gba");
        if !path.exists() {
            println!("skipped: no GBA ROM at {}", path.display());
            return None;
        }
        Some(crate::gba::rom::Rom::open(&path).expect("ROM opens"))
    }

    #[test]
    fn rom_who_can_come_lists_the_area_now_and_next() {
        use super::super::{rotation, wild};
        let Some(rom) = rom() else { return };
        let areas = rotation::areas(&rom).unwrap();
        let types = |s: u16| rom.species(s).map(|i| i.types);
        // 3 minutes 20 into a window: 7 minutes left (rounded up).
        let start = 1_792_800_000 / rotation::SLOT_SECS * rotation::SLOT_SECS;
        let unix = start + 200;
        let slot = rotation::slot_of(unix);
        for garden in 1..=3u8 {
            let page = Page::who_comes(&rom, &areas, garden, false, unix);
            let area = rotation::area_at(&areas, garden, slot).unwrap();
            let after = rotation::area_at(&areas, garden, slot + 1).unwrap();
            let name =
                |a: &rotation::Area| super::super::ascii(&rom.map_section_name(a.section).unwrap());
            assert_eq!(
                page.rows[0],
                (
                    format!("{}: for the next 7 minutes", name(area)),
                    Action::None
                )
            );
            assert_eq!(
                page.rows[1],
                (
                    format!("Next: {} in 7 minutes (A: who comes)", name(after)),
                    Action::WhoNext(garden)
                )
            );
            // Then every species that can come, as the picking has them, each with its name
            // to search by and its types and abilities in its note.
            let chances = wild::who_comes(area, types);
            assert_eq!(page.rows.len(), 2 + chances.len());
            for (i, c) in chances.iter().enumerate() {
                let (text, action) = &page.rows[2 + i];
                assert_eq!(*action, Action::Look(c.species));
                let species = super::super::ascii(&rom.species_name(c.species).unwrap());
                assert_eq!(page.name(2 + i), Some(species.as_str()));
                assert!(text.starts_with(&species) && text.contains(c.from.label()));
                assert!(text.contains(&format!("Lv{}", c.min_level)) && text.ends_with('%'));
                let note = page.note(2 + i).unwrap();
                let first = rom.species(c.species).unwrap().types[0];
                assert!(note.contains(&super::super::ascii(&rom.type_name(first).unwrap())));
            }
            assert!(page.note(0).is_none() && page.note(1).is_none());
            // The next window's list: its area, from when, and no further.
            let next = Page::who_comes(&rom, &areas, garden, true, unix);
            assert_eq!(
                next.rows[0].0,
                format!("{}: in 7 minutes, for 10 minutes", name(after))
            );
            let later = wild::who_comes(after, types);
            assert_eq!(next.rows.len(), 1 + later.len());
            assert!(next.title.ends_with(" next"));
        }
        // Search and jumps on a long list: find a window whose area has many kinds.
        let (unix, page) = (0..400u64)
            .map(|w| start + w * rotation::SLOT_SECS)
            .map(|t| (t, Page::who_comes(&rom, &areas, 1, false, t)))
            .find(|(_, p)| p.rows.len() > 2 + 14)
            .expect("a long list");
        let mut l = List::new(page);
        // The cursor starts on the next area's row; the area's own line can't be picked.
        assert_eq!(l.page.row, 1);
        assert_eq!(l.press(Key::JumpDown, 8), Outcome::Moved);
        assert_eq!(l.page.row, 11);
        assert!(l.species().is_some());
        l.press(Key::JumpUp, 8);
        assert_eq!(l.page.row, 1);
        // Typing a creature's name keeps its rows and the unnamed lines.
        let target = l.page.name(5).unwrap().to_string();
        assert_eq!(l.typed(&typed(&target[..3]), 8), Outcome::Searched);
        let shown = l.shown();
        assert!(shown.contains(&0) && shown.contains(&1) && shown.contains(&5));
        assert!(shown.iter().filter_map(|&i| l.page.name(i)).all(|n| {
            n.to_ascii_lowercase()
                .contains(&target[..3].to_ascii_lowercase())
        }));
        // Esc clears the search, then goes back.
        let escape = Typed {
            escape: true,
            ..Typed::default()
        };
        assert_eq!(l.typed(&escape, 8), Outcome::Searched);
        assert_eq!(l.typed(&escape, 8), Outcome::Close);
        assert!(unix >= start);
    }

    #[test]
    fn left_and_right_jump_ten_rows() {
        let mut rows: Vec<Action> = (0..25).map(|i| bring(i + 1)).collect();
        rows[10] = Action::None;
        let mut l = list(rows);
        assert_eq!(l.press(Key::JumpDown, 5), Outcome::Moved);
        // Row 10 can't be picked: the nearest one, the earlier first.
        assert_eq!(l.page.row, 9);
        l.press(Key::JumpDown, 5);
        assert_eq!(l.page.row, 19);
        assert_eq!(l.page.scroll, 15);
        l.press(Key::JumpDown, 5);
        assert_eq!(l.page.row, 24);
        assert_eq!(l.press(Key::JumpDown, 5), Outcome::Stay);
        l.press(Key::JumpUp, 5);
        assert_eq!(l.page.row, 14);
        l.press(Key::JumpUp, 5);
        l.press(Key::JumpUp, 5);
        assert_eq!(l.page.row, 0);
        // PgUp/PgDn still move a page.
        l.press(Key::PageDown, 5);
        assert_eq!(l.page.row, 5);
    }

    #[test]
    fn typing_searches_the_rows_by_name() {
        let mut rows: Vec<Action> = (0..30).map(|i| bring(i + 1)).collect();
        rows[0] = Action::ChaoList;
        let mut l = list(rows);
        l.page.names[0] = None;
        l.press(Key::Down, 5);
        assert_eq!(l.page.row, 1);
        // "Row 2", "Row 12" and "Row 20".."Row 29" match "2"; the row without a name stays.
        assert_eq!(l.typed(&typed("2"), 5), Outcome::Searched);
        assert_eq!(
            l.shown(),
            [0, 2, 12, 20, 21, 22, 23, 24, 25, 26, 27, 28, 29]
        );
        // The cursor went to a row that shows (the nearest, the earlier first).
        assert_eq!(l.page.row, 0);
        l.press(Key::Down, 5);
        assert_eq!((l.page.row, l.species()), (2, Some(3)));
        l.press(Key::JumpDown, 5);
        assert_eq!(l.page.row, 28);
        assert_eq!(l.page.scroll, 7);
        l.press(Key::Down, 5);
        assert_eq!((l.page.row, l.page.scroll), (29, 8));
        l.press(Key::Down, 5);
        assert_eq!(l.page.row, 0);
        l.typed(&typed("1"), 5);
        assert_eq!(l.search.text(), "21");
        assert_eq!(l.shown(), [0, 21]);
        // Nothing matches: no row to pick, until Backspace.
        l.typed(&typed("x"), 5);
        assert_eq!(l.shown(), [0]);
        l.page.names[0] = Some("Moves".into());
        assert!(l.shown().is_empty());
        assert_eq!(l.press(Key::Pick, 5), Outcome::Stay);
        assert_eq!(l.press(Key::Down, 5), Outcome::Stay);
        assert_eq!(l.species(), None);
        let erase = Typed {
            erase: true,
            erase_fresh: true,
            ..Typed::default()
        };
        assert_eq!(l.typed(&erase, 5), Outcome::Searched);
        assert_eq!(l.shown(), [21]);
        assert_eq!(l.page.row, 21);
        assert_eq!(l.press(Key::Pick, 5), Outcome::Chose(bring(22)));
        // Esc clears the search, then closes; a fresh Backspace with none closes too.
        let escape = Typed {
            escape: true,
            ..Typed::default()
        };
        assert_eq!(l.typed(&escape, 5), Outcome::Searched);
        assert_eq!(l.shown().len(), 30);
        assert_eq!(l.page.row, 21);
        assert_eq!(l.typed(&escape, 5), Outcome::Close);
        assert_eq!(l.typed(&erase, 5), Outcome::Close);
        assert_eq!(l.typed(&Typed::default(), 5), Outcome::Stay);
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
            // The player reaches it from every side alike, however the box turns.
            let bounds = (m.box_min, m.box_max);
            let mid = world(
                at,
                angle,
                std::array::from_fn(|i| (m.box_min[i] + m.box_max[i]) / 2.0),
            );
            let (hx, hz) = (
                (m.box_max[0] - m.box_min[0]) / 2.0,
                (m.box_max[2] - m.box_min[2]) / 2.0,
            );
            let radius = (hx * hx + hz * hz).sqrt();
            for k in 0..8 {
                let a = k as f32 * std::f32::consts::TAU / 8.0;
                let around = |d: f32| [mid[0] + a.sin() * d, at[1], mid[2] + a.cos() * d];
                let out = in_reach(at, angle, bounds, around(radius + 2.0)).unwrap();
                assert!((out - 2.0).abs() < 1e-3, "{k}");
                assert!(in_reach(at, angle, bounds, around(radius + 5.0)).is_none());
                let mut high = around(radius + 1.0);
                high[1] += 6.0;
                assert!(in_reach(at, angle, bounds, high).is_none());
            }
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

    /// A square of ground `half` across round (cx, cz) at height `y`, with `flags`.
    fn square(cx: f32, cz: f32, half: f32, y: f32, flags: u32) -> Vec<Tri> {
        let (a, b) = ([cx - half, y, cz - half], [cx + half, y, cz - half]);
        let (c, d) = ([cx + half, y, cz + half], [cx - half, y, cz + half]);
        vec![
            Tri {
                v: [a, b, c],
                flags,
            },
            Tri {
                v: [a, c, d],
                flags,
            },
        ]
    }

    /// The turn (degrees) from the line toward the garden's middle to `at`, seen from the
    /// transporter, and the distance.
    fn from_doorway(garden: u8, at: [f32; 3]) -> (f32, f32) {
        let i = usize::from(garden - 1);
        let (t, m) = (TRANSPORTERS[i], MIDDLES[i]);
        let toward = (m[0] - t[0]).atan2(m[1] - t[2]);
        let to = (at[0] - t[0]).atan2(at[2] - t[2]);
        let mut d = (to - toward).to_degrees();
        while d > 180.0 {
            d -= 360.0;
        }
        while d < -180.0 {
            d += 360.0;
        }
        (
            d.abs(),
            ((at[0] - t[0]).powi(2) + (at[2] - t[2]).powi(2)).sqrt(),
        )
    }

    #[test]
    fn the_spot_is_beside_the_transporter_not_in_its_doorway() {
        for garden in 1..=3u8 {
            let t = TRANSPORTERS[usize::from(garden - 1)];
            let ground = square(t[0], t[2], 120.0, t[1], 0);
            let (at, angle) = find_spot(garden, &ground, &[], &[]).unwrap();
            assert!((at[1] - t[1]).abs() < 1e-3);
            let (turn, r) = from_doorway(garden, at);
            assert!((turn - 90.0).abs() < 1.0, "{garden}: {turn}");
            assert!((r - 28.0).abs() < 1.0, "{garden}: {r}");
            // Its front faces the garden's middle.
            let m = MIDDLES[usize::from(garden - 1)];
            let f = turn_front(angle);
            let (dx, dz) = (m[0] - at[0], m[1] - at[2]);
            let len = (dx * dx + dz * dz).sqrt();
            assert!(f[0] * dx / len + f[2] * dz / len > 0.999, "{garden}");
        }
    }

    fn turn_front(angle: i32) -> [f32; 3] {
        turn([0.0, 0.0, 1.0], angle)
    }

    #[test]
    fn the_spot_keeps_off_water_trees_toys_slopes_and_roofs() {
        let garden = 2u8;
        let t = TRANSPORTERS[1];
        let ground = square(t[0], t[2], 120.0, t[1], 0);
        let (first, _) = find_spot(garden, &ground, &[], &[]).unwrap();
        // A pond over the first spot: the other side.
        let mut wet = square(
            first[0],
            first[2],
            12.0,
            t[1] + 0.5,
            crate::chao_sim::land::WATER,
        );
        wet.extend(ground.clone());
        let (other, _) = find_spot(garden, &wet, &[], &[]).unwrap();
        assert!((other[0] - first[0]).powi(2) + (other[2] - first[2]).powi(2) > 400.0);
        // A tree or a toy there does the same.
        let (by_tree, _) = find_spot(garden, &ground, &[first], &[]).unwrap();
        assert!((by_tree[0] - first[0]).powi(2) + (by_tree[2] - first[2]).powi(2) > 100.0);
        let (by_toy, _) = find_spot(garden, &ground, &[], &[first]).unwrap();
        assert!((by_toy[0] - first[0]).powi(2) + (by_toy[2] - first[2]).powi(2) > 50.0);
        // A roof over everything, or ground far below the transporter: nowhere.
        let mut roofed = ground.clone();
        roofed.extend(square(t[0], t[2], 120.0, t[1] + 6.0, 0));
        assert!(find_spot(garden, &roofed, &[], &[]).is_none());
        let low = square(t[0], t[2], 120.0, t[1] - 10.0, 0);
        assert!(find_spot(garden, &low, &[], &[]).is_none());
        assert!(find_spot(4, &ground, &[], &[]).is_none());
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
