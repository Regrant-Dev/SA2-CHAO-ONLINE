//! Battles in the game: the BATTLE button in the karate mode select, the picker and the battle
//! (session.rs) run from the karate main object's MainSub, the Chao in the arena (arena.rs) and
//! the HUD (a box per fighter in the ring, and a panel for messages and menus).
//!
//! The button sits below the mode select's own two: Down on its last one (leave) moves to it,
//! Up or B moves back. While it's chosen the mode select is held, so the game reads none of our
//! presses. Everything runs inside the main object's MainSub (wrapped while we're in the karate
//! area), before the game's own update: what we hold or change is what it then sees, and the
//! battle stops with the game while it's paused. Leaving the area deletes every object there;
//! `on_frame` notices and forgets ours.
//!
//! The battle uses copies of the Chao records: no battle ever changes the save.

// The hook only exists in the 32-bit game build.
#![cfg_attr(not(target_arch = "x86"), allow(dead_code))]

use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::chao::{self, ChaoData};
use crate::gba_link::Profile;
use crate::hud;
use crate::log;

use super::arena::{self, Karate};
use super::engine::Format;
use super::event::Slot;
use super::narrate::{Cue, Sound};
use super::session::{self, Command, Entry, Lineup, Screen, Session, buttons};
use super::team::Dex;

/// The arena gets this many frames (15 seconds of running game) to come up after the menu
/// closes.
const LEAVE_LIMIT: u32 = 15 * 60;
/// An attack's step toward its target: how far and how many frames out and back.
const STEP_DISTANCE: f32 = 3.0;
const STEP_FRAMES: u16 = 30;
/// Frames after an attack or a hit before the fighting stance comes back.
const STANCE_AFTER: u16 = 50;
/// Frames after a Chao appears before its stance starts.
const FIRST_STANCE: u16 = 3;

/// Chao motions (karate's motion table, re/chao-battle-arena.md 4).
mod motion {
    pub const STANCE: u16 = 0x255;
    pub const WIND_UP: u16 = 0x242;
    pub const ATTACKS: [u16; 4] = [0x243, 0x244, 0x246, 0x247];
    pub const HIT: [u16; 3] = [0x249, 0x24A, 0x24B];
    pub const DOWN: u16 = 0x24C;
    pub const WON: [u16; 2] = [0x24D, 0x24E];
    pub const LOST: [u16; 2] = [0x24F, 0x250];
    pub const DRAW: u16 = 0x1E0;
}

mod sound {
    pub const CURSOR: u32 = 0x8000;
    pub const CONFIRM: u32 = 0x100E;
    pub const HITS: [u32; 2] = [0, 2];
    pub const ATTACK_VOICES: [u32; 3] = [0x6070, 0x6071, 0x606F];
    pub const LOSER: u32 = 0x6067;
}

/// Our button below the mode select's two, in the game's 640x480 space.
const BUTTON: (f32, f32, f32, f32) = (320.0, 432.0, 200.0, 34.0);

#[derive(Clone, Copy, PartialEq, Eq)]
enum Arena {
    /// At the karate menu.
    Menu,
    /// The menu is closing (frames since; paused frames don't count).
    Leaving(u32),
    /// The main object idles and our Chao are in the ring.
    Ready,
}

/// One of our Chao in the ring.
#[derive(Clone, Copy)]
struct Shown {
    object: usize,
    home: [f32; 3],
    angle: i32,
    /// An attack's step toward this point: frames played.
    step: Option<([f32; 3], u16)>,
    /// Frames until the stance comes back (0 = leave its motion alone).
    stance_in: u16,
}

struct Game {
    /// The karate main object we wrapped.
    main: usize,
    /// Our button is chosen in the mode select.
    on_button: bool,
    /// Let the mode select go next frame (so the press that left our button isn't read twice).
    release_next: bool,
    session: Option<Session>,
    arena: Arena,
    format: Format,
    shown: [[Option<Shown>; 2]; 2],
    dex: Option<Arc<Dex>>,
    /// Varies the voices and motions.
    turn: u32,
    /// The HP bars as drawn (a fraction per team member, NaN before the first draw): they run
    /// down or up to the real HP over a moment.
    bars: Bars,
}

type Bars = [[f32; session::TEAM_SIZE]; 2];

static GAME: Mutex<Game> = Mutex::new(Game {
    main: 0,
    on_button: false,
    release_next: false,
    session: None,
    arena: Arena::Menu,
    format: Format::Singles,
    shown: [[None; 2]; 2],
    dex: None,
    turn: 0,
    bars: [[f32::NAN; session::TEAM_SIZE]; 2],
});

fn lock() -> std::sync::MutexGuard<'static, Game> {
    GAME.lock().unwrap_or_else(|e| e.into_inner())
}

/// Every frame: wraps the karate main object while we're in the karate area, and forgets the
/// battle when the area goes.
pub fn on_frame() {
    let mut g = lock();
    let Some(k) = arena::karate() else {
        if g.main != 0 {
            forget(&mut g);
            g.main = 0;
        }
        return;
    };
    if k.main != g.main {
        forget(&mut g);
        g.main = 0;
    }
    let ours = main_exec as extern "C" fn(usize) as usize;
    let live = crate::karate_live::live();
    if live {
        // Live karate wraps the main object itself; our button and picker let go.
        if (g.session.is_some() || g.on_button) && g.arena == Arena::Menu {
            g.session = None;
            g.on_button = false;
            k.hold_mode_select(false);
        }
        if g.session.is_none() && k.main_sub() == ours {
            k.set_main_sub(arena::MAIN_EXEC);
            g.main = 0;
        }
        return;
    }
    if k.main_sub() == arena::MAIN_EXEC {
        k.set_main_sub(ours);
        g.main = k.main;
    }
}

/// Drops the battle without touching game memory (the area and its objects are gone).
fn forget(g: &mut Game) {
    if g.session.is_some() {
        log!("battle: left the karate area");
    }
    g.session = None;
    g.on_button = false;
    g.release_next = false;
    g.arena = Arena::Menu;
    g.shown = [[None; 2]; 2];
    arena::forget_all();
}

/// MainSub of the karate main object while we're in the area.
extern "C" fn main_exec(object: usize) {
    let run = !arena::paused();
    if run {
        let _ = std::panic::catch_unwind(|| before(object));
    }
    // SAFETY: the karate main object's own MainSub, which this object had.
    unsafe { std::mem::transmute::<usize, extern "C" fn(usize)>(arena::MAIN_EXEC)(object) };
    if run {
        let _ = std::panic::catch_unwind(|| after(object));
    }
}

/// Our buttons this frame: none while the game window isn't focused, chat is being typed or
/// the F9 menu is open.
fn pressed() -> u32 {
    if !crate::input::game_focused() || crate::chat::typing() || crate::menu::is_open() {
        return 0;
    }
    arena::buttons()
}

fn before(object: usize) {
    let Some(k) = arena::karate().filter(|k| k.main == object) else {
        return;
    };
    let mut g = lock();
    let pressed = pressed();
    if std::mem::take(&mut g.release_next) {
        k.hold_mode_select(false);
    }
    if g.session.is_some() && g.arena == Arena::Menu && !k.mode_select_held() {
        // The picker lost the mode select (live karate started from it): let it go.
        log!("battle: the karate menu moved on; picker closed");
        g.session = None;
        g.on_button = false;
    }
    if g.session.as_ref().is_some_and(Session::done) {
        // Done without closing: it failed part-way (a panic caught in `main_exec`).
        log!("battle: the battle stopped; back to the menu");
        if g.arena == Arena::Menu {
            g.session = None;
            g.on_button = false;
            k.hold_mode_select(false);
        } else {
            end_battle(&mut g, k);
        }
        return;
    }
    if g.session.is_some() {
        run_session(&mut g, k, pressed);
        return;
    }
    // Our button in the mode select.
    if !g.on_button {
        if k.mode_select_input() && k.cursor() < 0 && pressed & buttons::DOWN != 0 {
            g.on_button = true;
            k.hold_mode_select(true);
            arena::sound(sound::CURSOR);
        }
    } else if !k.mode_select_held() {
        g.on_button = false;
    } else if pressed & (buttons::UP | buttons::B) != 0 {
        g.on_button = false;
        g.release_next = true;
        arena::sound(sound::CURSOR);
    } else if pressed & buttons::A != 0 {
        open(&mut g);
    }
    if k.mode_select_input() || k.mode_select_held() {
        let (x, y, w, h) = BUTTON;
        hud::menu_label(x, y, w, h, "BATTLE", g.on_button);
    }
}

fn after(object: usize) {
    let Some(k) = arena::karate().filter(|k| k.main == object) else {
        return;
    };
    let mut g = lock();
    if let Arena::Leaving(frames) = g.arena {
        g.arena = Arena::Leaving(frames + 1);
        if k.stop_before_fight() {
            g.arena = Arena::Ready;
            log!("battle: in the arena");
        } else if frames > LEAVE_LIMIT {
            log!(
                "battle: the menu didn't close (state {:?}); back",
                k.state()
            );
            end_battle(&mut g, k);
        }
    }
    if g.arena == Arena::Ready {
        let (position, look) = camera(g.format);
        k.set_camera(position, look);
    }
}

/// The fight view: in front of the ring, a little further back for doubles.
fn camera(format: Format) -> ([f32; 3], [f32; 3]) {
    match format {
        Format::Singles => ([0.0, 13.0, 37.0], [0.0, 11.0, 19.0]),
        Format::Doubles => ([0.0, 15.0, 43.0], [0.0, 10.5, 19.0]),
    }
}

/// Where `slot` stands and faces: the player's side on the left facing right, the foes on the
/// right; doubles side by side across the ring.
fn home(format: Format, slot: Slot) -> ([f32; 3], i32) {
    let x = if slot.side == 0 { -5.0 } else { 5.0 };
    let z = match (format, slot.pos) {
        (Format::Singles, _) => 19.0,
        (Format::Doubles, 0) => 16.0,
        (Format::Doubles, _) => 22.0,
    };
    let angle = if slot.side == 0 { 0x4000 } else { 0xC000 };
    ([x, arena::FLOOR, z], angle)
}

/// Opens the picker: the ROM's data, our Chao and the opponents to choose from.
fn open(g: &mut Game) {
    let dex = match g.dex.clone() {
        Some(dex) => dex,
        None => match crate::gba_link::rom().and_then(|rom| Dex::from_rom(&rom)) {
            Ok(dex) => {
                let dex = Arc::new(dex);
                g.dex = Some(dex.clone());
                dex
            }
            Err(e) => {
                log!("battle: {e}");
                hud::toast(format!("Battle: {e}"));
                return;
            }
        },
    };
    arena::sound(sound::CONFIRM);
    let own = own_chao();
    let lineups = lineups();
    log!(
        "battle: picker with {} Chao, {} opponent teams",
        own.len(),
        lineups.len() + 1
    );
    let seed = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(1, |d| d.as_nanos() as u64);
    g.session = Some(Session::open(dex, own, lineups, Box::new(profile_of), seed));
    g.bars = [[f32::NAN; session::TEAM_SIZE]; 2];
}

/// A Chao's types, ability and moves (Normal with no moves if the ROM can't be read).
fn profile_of(chao: &ChaoData) -> Profile {
    crate::gba_link::battle_profile(chao.identity()).unwrap_or_else(|e| {
        log!("battle: no profile ({e})");
        Profile {
            types: [0, 0],
            ability: 0,
            moves: [0; 4],
            partner: None,
        }
    })
}

/// A hatched Chao that's fine to show (an egg or a damaged record isn't).
fn usable(chao: &ChaoData) -> bool {
    chao.chao_type() > chao::TYPE_EGG && chao.validate().is_ok()
}

fn entry(chao: ChaoData) -> Entry {
    Entry {
        name: crate::karate::chao_name(&chao),
        chao,
    }
}

/// Copies of the Chao in our save.
fn own_chao() -> Vec<Entry> {
    (0..crate::game::GARDEN_SLOTS)
        .filter_map(crate::game::read_chao_slot)
        .filter(usable)
        .map(entry)
        .collect()
}

/// Ready-made opponent teams: each friend's Chao brought online, then the Chao in
/// `chao/import`, six to a team.
fn lineups() -> Vec<Lineup> {
    let mut out: Vec<Lineup> = Vec::new();
    for (label, chao) in crate::online::karate_choices() {
        if !usable(&chao) {
            continue;
        }
        let suffix = format!("'s {}", crate::karate::chao_name(&chao));
        let owner = label.strip_suffix(&suffix).unwrap_or(&label).to_string();
        let team_label = format!("{owner}'s Chao");
        match out.last_mut() {
            Some(l) if l.label == team_label && l.members.len() < session::TEAM_SIZE => {
                l.members.push(entry(chao));
            }
            _ => out.push(Lineup {
                label: team_label,
                members: vec![entry(chao)],
            }),
        }
    }
    let imported = imported_chao();
    let teams = imported.len().div_ceil(session::TEAM_SIZE);
    for (i, members) in imported.chunks(session::TEAM_SIZE).enumerate() {
        let label = if teams > 1 {
            format!("Imported Chao {}", i + 1)
        } else {
            "Imported Chao".into()
        };
        out.push(Lineup {
            label,
            members: members.to_vec(),
        });
    }
    out
}

/// The `.sa2chao` files in `chao/import`.
fn imported_chao() -> Vec<Entry> {
    let dir = PathBuf::from(crate::online::mod_path())
        .join("chao")
        .join("import");
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return Vec::new();
    };
    let mut files: Vec<PathBuf> = entries
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| {
            p.extension()
                .is_some_and(|x| x.eq_ignore_ascii_case("sa2chao"))
        })
        .collect();
    files.sort();
    files
        .into_iter()
        .filter_map(|path| {
            let data = std::fs::read(&path).ok()?;
            match chao::file::decode(&data) {
                Ok(chao) if usable(&chao) => Some(entry(chao)),
                Ok(_) => None,
                Err(e) => {
                    log!("battle: {} skipped: {e}", path.display());
                    None
                }
            }
        })
        .collect()
}

fn run_session(g: &mut Game, k: Karate, pressed: u32) {
    let ready = g.arena == Arena::Ready;
    let Some(session) = g.session.as_mut() else {
        return;
    };
    let commands = session.update(pressed, ready);
    let screen = session.screen();
    for c in commands {
        match c {
            Command::Cue(cue) => cue_frame(g, cue),
            Command::OpenArena(format) => {
                log!("battle: starting ({format:?})");
                g.format = format;
                g.on_button = false;
                g.arena = Arena::Leaving(0);
                k.leave_menu();
            }
            Command::CloseArena => {
                end_battle(g, k);
                return;
            }
            Command::Closed => {
                // Back to our button in the held mode select.
                g.session = None;
                return;
            }
        }
    }
    animate(g);
    if !matches!(g.arena, Arena::Leaving(_)) {
        let in_arena = g.arena == Arena::Ready;
        if in_arena {
            draw_fighters(&screen, &mut g.bars);
        }
        draw_menu(&screen);
    }
}

/// The battle is over or failed: our Chao go and the karate menu comes back.
fn end_battle(g: &mut Game, k: Karate) {
    for row in &mut g.shown {
        for s in row.iter_mut() {
            if let Some(shown) = s.take() {
                arena::remove(shown.object);
            }
        }
    }
    if g.arena != Arena::Menu {
        k.back_to_menu();
    }
    g.arena = Arena::Menu;
    g.session = None;
    g.on_button = false;
    log!("battle: back to the menu");
}

fn shown_mut(g: &mut Game, slot: Slot) -> Option<&mut Shown> {
    g.shown
        .get_mut(slot.side)
        .and_then(|s| s.get_mut(slot.pos))
        .and_then(Option::as_mut)
}

fn object_at(g: &Game, slot: Slot) -> Option<usize> {
    g.shown
        .get(slot.side)
        .and_then(|s| s.get(slot.pos))
        .copied()
        .flatten()
        .map(|s| s.object)
}

fn hide(g: &mut Game, slot: Slot) {
    if let Some(s) = g
        .shown
        .get_mut(slot.side)
        .and_then(|s| s.get_mut(slot.pos))
        .and_then(Option::take)
    {
        arena::remove(s.object);
    }
}

fn cue_frame(g: &mut Game, cue: Cue) {
    g.turn = g.turn.wrapping_add(1);
    let pick = g.turn as usize;
    match cue {
        Cue::Show { slot, member } => {
            hide(g, slot);
            let record = g
                .session
                .as_ref()
                .and_then(|s| s.record(slot.side, member))
                .cloned();
            let Some(record) = record else { return };
            let (at, angle) = home(g.format, slot);
            match arena::spawn(&record, at, angle) {
                Some(object) => {
                    // The stance once its first update has set its motions up.
                    g.shown[slot.side][slot.pos] = Some(Shown {
                        object,
                        home: at,
                        angle,
                        step: None,
                        stance_in: FIRST_STANCE,
                    });
                }
                None => log!("battle: couldn't make the Chao for {slot:?}"),
            }
        }
        Cue::Hide { slot } => hide(g, slot),
        Cue::Attack { by, toward, status } => {
            let target = toward.map(|t| home(g.format, t).0);
            let Some(s) = shown_mut(g, by) else { return };
            if status {
                arena::motion(s.object, motion::WIND_UP, 20);
            } else {
                arena::motion(s.object, motion::ATTACKS[pick % motion::ATTACKS.len()], 15);
                arena::sound(sound::ATTACK_VOICES[pick % sound::ATTACK_VOICES.len()]);
                s.step = target.map(|t| (t, 0));
            }
            s.stance_in = STANCE_AFTER;
        }
        Cue::Hurt { target, heavy } => {
            let Some(s) = shown_mut(g, target) else {
                return;
            };
            let id = motion::HIT[usize::from(heavy).min(motion::HIT.len() - 1)];
            arena::motion(s.object, id, 1);
            s.stance_in = STANCE_AFTER;
        }
        Cue::Faint { at } => {
            if let Some(s) = shown_mut(g, at) {
                arena::motion(s.object, motion::DOWN, 1);
                s.stance_in = 0;
            }
        }
        Cue::Result { winner } => {
            for side in 0..2 {
                for pos in 0..2 {
                    let Some(object) = object_at(g, Slot::new(side, pos)) else {
                        continue;
                    };
                    let id = match winner {
                        None => motion::DRAW,
                        Some(w) if w == side => motion::WON[pick % 2],
                        Some(_) => motion::LOST[pick % 2],
                    };
                    arena::motion(object, id, 20);
                    if let Some(s) = shown_mut(g, Slot::new(side, pos)) {
                        s.stance_in = 0;
                        s.step = None;
                    }
                }
            }
            if winner != Some(0) {
                arena::sound(sound::LOSER);
            }
        }
        Cue::Sound(s) => arena::sound(match s {
            Sound::Cursor | Sound::Back => sound::CURSOR,
            Sound::Confirm => sound::CONFIRM,
            Sound::Hit => sound::HITS[pick % 2],
            Sound::Faint => sound::LOSER,
        }),
    }
}

/// Steps toward targets and back, and the stance after a motion.
fn animate(g: &mut Game) {
    for row in &mut g.shown {
        for s in row.iter_mut().flatten() {
            let mut at = s.home;
            if let Some((target, frame)) = s.step {
                let t = f32::from(frame) / f32::from(STEP_FRAMES);
                let (dx, dz) = (target[0] - s.home[0], target[2] - s.home[2]);
                let length = (dx * dx + dz * dz).sqrt().max(1.0);
                let reach = (t * std::f32::consts::PI).sin() * STEP_DISTANCE;
                at[0] += dx / length * reach;
                at[2] += dz / length * reach;
                s.step = (frame < STEP_FRAMES).then_some((target, frame + 1));
            }
            arena::place(s.object, at, s.angle);
            if s.stance_in > 0 {
                s.stance_in -= 1;
                if s.stance_in == 0 {
                    arena::motion(s.object, motion::STANCE, 25);
                }
            }
        }
    }
}

const TEXT: i32 = 0xFFFF_FFFFu32 as i32;
const DIM: i32 = 0xFF80_8898u32 as i32;
const GOLD: i32 = 0xFFF0_C030u32 as i32;
/// The cursor on an item that can't be picked.
const GOLD_DIM: i32 = 0xFF90_7828u32 as i32;
const DETAIL: i32 = 0xFFB0_D0F0u32 as i32;
/// HP bar fills: more than half, more than a fifth, the rest.
const HP_HIGH: u32 = 0xFF40_C060;
const HP_MID: u32 = 0xFFE8_C030;
const HP_LOW: u32 = 0xFFE0_4030;
/// A fighter box's width in text cells.
const BOX_COLS: usize = 20;
/// Menu items shown at once (longer lists scroll with the cursor).
const VISIBLE_ITEMS: usize = 7;
/// How far a drawn HP bar moves toward the real HP each frame (a full bar in about a second).
const BAR_STEP: f32 = 1.0 / 60.0;

fn status_color(tag: &str) -> i32 {
    (match tag {
        "SLP" => 0xFFA0_A8C0u32,
        "PSN" => 0xFFC0_70E0,
        "BRN" => 0xFFF0_7040,
        "FRZ" => 0xFF80_D0F0,
        "PAR" => 0xFFF0_D040,
        _ => 0xFFFF_FFFF,
    }) as i32
}

fn hp_color(fraction: f32) -> u32 {
    if fraction > 0.5 {
        HP_HIGH
    } else if fraction > 0.2 {
        HP_MID
    } else {
        HP_LOW
    }
}

/// Moves a drawn bar (`shown`, NaN when new) toward `target`.
fn ease(shown: f32, target: f32) -> f32 {
    if !shown.is_finite() {
        return target;
    }
    if (target - shown).abs() <= BAR_STEP {
        target
    } else {
        shown + BAR_STEP * (target - shown).signum()
    }
}

/// `text` broken into lines of at most `cols` characters, at spaces where it can.
fn wrap(text: &str, cols: usize) -> Vec<String> {
    let cols = cols.max(1);
    let mut lines = Vec::new();
    let mut line = String::new();
    for word in text.split_whitespace() {
        let mut word: Vec<char> = word.chars().collect();
        while word.len() > cols {
            if !line.is_empty() {
                lines.push(std::mem::take(&mut line));
            }
            lines.push(word.drain(..cols).collect());
        }
        let word: String = word.into_iter().collect();
        let len = line.chars().count();
        if len > 0 && len + 1 + word.chars().count() > cols {
            lines.push(std::mem::take(&mut line));
        }
        if !line.is_empty() {
            line.push(' ');
        }
        line.push_str(&word);
    }
    if !line.is_empty() {
        lines.push(line);
    }
    lines
}

/// The first of `VISIBLE_ITEMS` items shown so the cursor stays in view.
fn first_visible(len: usize, cursor: usize) -> usize {
    if len <= VISIBLE_ITEMS {
        return 0;
    }
    cursor
        .saturating_sub(VISIBLE_ITEMS / 2)
        .min(len - VISIBLE_ITEMS)
}

/// A box per fighter in the ring: the player's on the left, the foes on the right (as they
/// stand), each with its name, status and HP bar, and the HP for the player's own.
fn draw_fighters(screen: &Screen, bars: &mut Bars) {
    let (w, _) = hud::screen_size();
    let size = hud::menu_font();
    let cols = (w / size) as usize;
    for line in &screen.fighters {
        let Slot { side, pos } = line.slot;
        let ours = side == 0;
        let col = if ours {
            2
        } else {
            cols.saturating_sub(BOX_COLS + 2)
        };
        let rows = if ours { 3 } else { 2 };
        let (x, y) = (col as f32 * size, (1 + pos * 5) as f32 * size);
        hud::panel(x - size * 0.5, y - size * 0.5, size, BOX_COLS + 1, rows + 1);

        let marked = screen.choosing == Some(line.slot) || screen.aimed == Some(line.slot);
        let mark = if marked { ">" } else { " " };
        let name: String = line.name.chars().take(BOX_COLS - 5).collect();
        hud::text(
            x,
            y,
            size,
            if marked { GOLD } else { TEXT },
            &format!("{mark}{name}"),
        );
        if let Some(tag) = line.ailment {
            let tx = x + (BOX_COLS - tag.len()) as f32 * size;
            hud::text(tx, y, size, status_color(tag), tag);
        }

        let target = f32::from(line.hp) / f32::from(line.max_hp.max(1));
        let bar = bars
            .get_mut(side)
            .and_then(|b| b.get_mut(line.member))
            .map_or(target, |b| {
                *b = ease(*b, target);
                *b
            });
        hud::bar(
            x,
            y + size * 1.25,
            BOX_COLS as f32 * size,
            size * 0.5,
            bar,
            hp_color(bar),
        );
        if ours {
            // The numbers count along with the bar.
            let hp = (bar * f32::from(line.max_hp)).round() as u16;
            let hp = if bar == target { line.hp } else { hp };
            hud::text(
                x,
                y + size * 2.0,
                size,
                TEXT,
                &format!(" HP {hp}/{}", line.max_hp),
            );
        }
    }
}

/// The panel at the bottom: the message, the menu with its cursor, more about the item under
/// the cursor, and the buttons.
fn draw_menu(screen: &Screen) {
    let (w, h) = hud::screen_size();
    let size = hud::menu_font();
    let cols = ((w / size) as usize).saturating_sub(4).clamp(20, 60);
    let x = size * 2.0;
    let mut lines: Vec<(String, i32)> = Vec::new();
    if let Some(m) = &screen.message {
        lines.extend(wrap(m, cols).into_iter().map(|l| (l, TEXT)));
    }
    if !screen.title.is_empty() {
        lines.push((screen.title.clone(), GOLD));
    }
    let first = first_visible(screen.items.len(), screen.cursor.unwrap_or(0));
    let last = (first + VISIBLE_ITEMS).min(screen.items.len());
    if first > 0 {
        lines.push(("  ...".into(), DIM));
    }
    for (i, item) in screen.items.iter().enumerate().take(last).skip(first) {
        let selected = screen.cursor == Some(i);
        let disabled = screen.disabled.contains(&i);
        let mark = if selected { "> " } else { "  " };
        let color = match (selected, disabled) {
            (true, false) => GOLD,
            (true, true) => GOLD_DIM,
            (false, false) => TEXT,
            (false, true) => DIM,
        };
        lines.push((format!("{mark}{item}"), color));
    }
    if last < screen.items.len() {
        lines.push(("  ...".into(), DIM));
    }
    if let Some(d) = &screen.detail {
        lines.extend(wrap(d, cols).into_iter().map(|l| (l, DETAIL)));
    }
    if !screen.hint.is_empty() {
        lines.push((screen.hint.to_string(), DIM));
    }
    if lines.is_empty() {
        return;
    }
    let rows = lines.len();
    let top = ((h / size).floor() - rows as f32 - 2.0).max(1.0) * size;
    hud::panel(x - size * 0.5, top - size * 0.5, size, cols + 1, rows + 1);
    for (i, (line, color)) in lines.iter().enumerate() {
        let clipped: String = line.chars().take(cols).collect();
        hud::text(x, top + size * i as f32, size, *color, &clipped);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_sides_face_each_other_across_the_ring() {
        for format in [Format::Singles, Format::Doubles] {
            for pos in 0..format.slots() {
                let (mine, a) = home(format, Slot::new(0, pos));
                let (theirs, b) = home(format, Slot::new(1, pos));
                assert!(mine[0] < 0.0 && theirs[0] > 0.0);
                assert_eq!(mine[2], theirs[2]);
                assert_eq!((a + 0x8000) & 0xFFFF, b);
            }
        }
        let (front, _) = home(Format::Doubles, Slot::new(0, 1));
        let (back, _) = home(Format::Doubles, Slot::new(0, 0));
        assert!(front[2] > back[2]);
    }

    #[test]
    fn messages_wrap_at_spaces_and_long_words_break() {
        assert_eq!(
            wrap("Foe Pip uses a move on you!", 10),
            ["Foe Pip", "uses a", "move on", "you!"]
        );
        assert_eq!(wrap("abcdefghijkl x", 5), ["abcde", "fghij", "kl x"]);
        assert!(wrap("   ", 5).is_empty());
        for line in wrap("one two three four five six seven", 9) {
            assert!(line.chars().count() <= 9);
        }
    }

    #[test]
    fn long_lists_scroll_with_the_cursor() {
        assert_eq!(first_visible(5, 4), 0);
        assert_eq!(first_visible(25, 0), 0);
        assert_eq!(first_visible(25, 10), 10 - VISIBLE_ITEMS / 2);
        assert_eq!(first_visible(25, 24), 25 - VISIBLE_ITEMS);
        for cursor in 0..25 {
            let first = first_visible(25, cursor);
            assert!((first..first + VISIBLE_ITEMS).contains(&cursor));
        }
    }

    #[test]
    fn hp_bars_run_toward_the_real_hp_and_change_color() {
        assert_eq!(ease(f32::NAN, 0.5), 0.5);
        let mut bar = 1.0;
        let mut frames = 0;
        while bar != 0.25 {
            bar = ease(bar, 0.25);
            frames += 1;
        }
        assert!((40..=50).contains(&frames), "{frames}");
        assert_eq!(hp_color(0.8), HP_HIGH);
        assert_eq!(hp_color(0.3), HP_MID);
        assert_eq!(hp_color(0.1), HP_LOW);
    }
}
