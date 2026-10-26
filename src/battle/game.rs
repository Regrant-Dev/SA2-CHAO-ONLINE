//! Creature battles in an independently owned garden scene. F9 opens the picker; copied Chao
//! and imported creature actors use a scene-owned camera and floor. Vanilla Karate never
//! provides a menu, object hook, camera or fight state for these battles.
//! Scene teardown returns input/camera to the same garden without changing its save ownership.

// Native scene resources only exist in the 32-bit game build.
#![cfg_attr(not(target_arch = "x86"), allow(dead_code))]

use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::chao::{self, ChaoData};
use crate::creature::rom::Rom;
use crate::creature_link::Profile;
use crate::creature_link::store::{Moves, Traits};
use crate::hud;
use crate::log;
use crate::search::{self, Keyboard, Owner};

use super::arena::{self, Scene};
use super::effects_gfx::{self, Book};
use super::engine::Format;
use super::event::Slot;
use super::narrate::{Cue, Sound};
use super::online::{self, Incoming};
use super::session::{self, Command, Entry, Friend, Lineup, Screen, Session, VISIBLE_ITEMS};
use super::team::Dex;
use super::trainers::{self, Roster};
use super::visuals::{self, Director, Order};
use super::wild::{self, Ending, WildFoe};
use super::{figures, ladder};

mod sound {
    pub const CURSOR: u32 = 0x8000;
    pub const CONFIRM: u32 = 0x100E;
    pub const HITS: [u32; 2] = [0, 2];
    pub const LOSER: u32 = 0x6067;
}

/// The friends list and the connection are looked at again every this many frames.
const NETWORK_EVERY: u32 = 20;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Arena {
    Menu,
    Ready,
}

/// One of our Chao in the ring.
#[derive(Clone, Copy)]
struct Shown {
    object: usize,
    angle: i32,
}

struct Game {
    /// F9 requested the independent battle picker.
    requested: bool,
    close_requested: bool,
    session: Option<Session>,
    arena: Arena,
    format: Format,
    shown: [[Option<Shown>; 2]; 2],
    dex: Option<Arc<Dex>>,
    /// The catalog's effect sprites (read with the dex while `BattleEffects` is on) and the
    /// catalog their pictures come from.
    book: Option<Arc<Book>>,
    rom: Option<Arc<Rom>>,
    /// The source game's trainers (read with the dex while `BattleTrainers` is on).
    roster: Option<Arc<Roster>>,
    /// The trainers were read (or couldn't be): not again.
    roster_read: bool,
    /// What the Chao and effects do (while a battle is in the arena).
    director: Option<Director>,
    /// Varies the sounds.
    turn: u32,
    /// The HP bars as drawn (a fraction per team member, NaN before the first draw): they run
    /// down or up to the real HP over a moment.
    bars: Bars,
    /// Frames the session ran (for `NETWORK_EVERY`).
    frames: u32,
    /// The menus' keyboard: typing, Enter, Esc and PgUp/PgDn (read every frame).
    keyboard: Option<Keyboard>,
}

type Bars = [[f32; session::TEAM_SIZE]; 2];

static GAME: Mutex<Game> = Mutex::new(Game {
    requested: false,
    close_requested: false,
    session: None,
    arena: Arena::Menu,
    format: Format::Singles,
    shown: [[None; 2]; 2],
    dex: None,
    book: None,
    rom: None,
    roster: None,
    roster_read: false,
    director: None,
    turn: 0,
    bars: [[f32::NAN; session::TEAM_SIZE]; 2],
    frames: 0,
    keyboard: None,
});

fn lock() -> std::sync::MutexGuard<'static, Game> {
    GAME.lock().unwrap_or_else(|e| e.into_inner())
}

/// F9 requests the picker after its own menu has released the opening Enter press.
pub fn request_open() -> Result<(), String> {
    if !arena::available() {
        return Err("Enter a loaded Chao garden and unpause to open Creature battles".into());
    }
    if arena::active() {
        return Err("Creature battles are already open".into());
    }
    crate::creature_link::retry_import();
    let mut g = lock();
    g.requested = true;
    g.close_requested = false;
    Ok(())
}
pub fn request_close() {
    lock().close_requested = true;
}
pub fn is_open() -> bool {
    arena::active()
}

/// Every game frame, independently of the Karate area's objects or DLC.
pub fn on_frame() {
    wild::on_frame();
    arena::on_frame();
    let mut g = lock();
    if std::mem::take(&mut g.close_requested) {
        if let Some(scene) = arena::scene() {
            end_battle(&mut g, scene);
        } else {
            forget(&mut g);
        }
        g.requested = false;
    }
    if (std::mem::take(&mut g.requested) || wild::wants_menu()) && !arena::active() {
        match arena::begin() {
            Ok(scene) => {
                if wild::wants_menu() {
                    open_wild(&mut g);
                } else {
                    open(&mut g);
                }
                if g.session.is_none() {
                    end_battle(&mut g, scene);
                }
            }
            Err(message) => {
                hud::toast(message);
                if wild::underway() {
                    let _ = wild::ended(Some(Ending::Cancelled));
                }
            }
        }
    }
    let Some(scene) = arena::scene() else {
        if g.session.is_some() {
            forget(&mut g);
        }
        online::set_busy(false);
        search::take_keyboard(Owner::Battle, false);
        // A challenge may open an available garden's independent picker.
        network(&mut g);
        return;
    };
    online::set_busy(g.session.as_ref().is_some_and(Session::busy));
    if arena::paused() {
        return;
    }
    network(&mut g);
    if g.session.as_ref().is_some_and(Session::done) {
        end_battle(&mut g, scene);
        return;
    }
    if g.session.is_none() {
        arena::end();
        return;
    }
    match crate::creature::assets::prepare_venue() {
        Ok(false) => {
            hud::center("Preparing battle venue...");
            return;
        }
        Err(error) => {
            hud::toast(format!("Battle venue: {error}"));
            if wild::underway() {
                let _ = wild::ended(Some(Ending::Cancelled));
            }
            end_battle(&mut g, scene);
            return;
        }
        Ok(true) => {}
    }
    if g.arena == Arena::Ready {
        match figures::ready() {
            Ok(false) => {
                hud::center("Preparing creature models...");
                return;
            }
            Err(error) => {
                hud::toast(format!("Creature models: {error}"));
                // A failed asset import must not punish a reserved visitor as a lost fight.
                if wild::underway() {
                    let _ = wild::ended(Some(Ending::Cancelled));
                }
                end_battle(&mut g, scene);
                return;
            }
            Ok(true) => {}
        }
    }
    let pressed = pressed(&mut g);
    run_session(&mut g, scene, pressed);
    if arena::active() {
        update_camera(&g, scene);
    }
}

/// Drops the session; a live battle or challenge in it is left (the host is told).
fn close_session(g: &mut Game) {
    if let Some(mut s) = g.session.take() {
        s.abandon();
        online::send(s.take_outgoing());
    }
}

/// Network invitations can open only our own garden scene, never a vanilla menu.
fn network(g: &mut Game) {
    for event in online::take_incoming() {
        if g.session.is_none() {
            match &event {
                Incoming::Offer { from, .. } => {
                    if online::enabled()
                        && !wild::underway()
                        && arena::available()
                        && !crate::menu::is_open()
                        && !crate::chat::typing()
                        && arena::begin().is_ok()
                    {
                        open(g);
                        if let Some(session) = g.session.as_mut() {
                            session.set_opened_by_offer();
                        } else {
                            arena::end();
                        }
                    }
                    if g.session.is_none() {
                        online::send(vec![session::busy_answer(*from)]);
                        continue;
                    }
                }
                Incoming::Start(start) => {
                    online::send(vec![session::left(start.battle)]);
                    continue;
                }
                Incoming::MultiStart(start) => {
                    online::send(vec![session::left(start.battle)]);
                    continue;
                }
                _ => continue,
            }
        }
        if let Some(session) = g.session.as_mut() {
            session.network(event);
        }
    }
}

/// Friends in loaded gardens, for independent live creature battles.
fn friends() -> Vec<Friend> {
    crate::online::others()
        .into_iter()
        .filter_map(|(id, name, state)| {
            let s = state?;
            let here = s.level == crate::game::LEVEL_CHAO_WORLD
                && (1..=3).contains(&s.chao_area)
                && s.flags & crate::game::POSE_LOADING == 0;
            here.then_some(Friend {
                id,
                name,
                busy: s.flags & crate::game::POSE_KARATE_BUSY != 0,
            })
        })
        .collect()
}

/// Drops the battle without touching game memory (the area and its objects are gone).
fn forget(g: &mut Game) {
    if g.session.is_some() {
        log!("battle: independent garden scene ended");
    }
    close_session(g);
    g.requested = false;
    g.close_requested = false;
    g.arena = Arena::Menu;
    g.shown = [[None; 2]; 2];
    let effects = g.director.take().is_some_and(|d| d.effects());
    arena::forget_all();
    effects_gfx::end(true, effects);
    figures::clear(true);
}

/// Search and controller input belong exclusively to our picker; the opening frame and
/// following frame swallow the F9 action so Enter cannot also select a battle option.
fn pressed(g: &mut Game) -> u32 {
    let typed = g.keyboard.get_or_insert_with(Keyboard::default).read();
    let typing = g.session.as_ref().is_some_and(Session::typing) && g.arena == Arena::Menu;
    search::take_keyboard(Owner::Battle, typing);
    if g.frames < 2
        || !crate::input::game_focused()
        || crate::chat::typing()
        || crate::menu::is_open()
    {
        return 0;
    }
    let buttons = if typing && typed.held {
        0
    } else {
        arena::buttons()
    };
    match g.session.as_mut() {
        Some(session) => buttons | session.keyboard(&typed),
        None => buttons,
    }
}

fn update_camera(g: &Game, scene: Scene) {
    let (mut position, mut look) = camera(g.format);
    if let Some(director) = &g.director {
        let shake = director.shake();
        for i in 0..3 {
            position[i] += shake[i];
            look[i] += shake[i];
        }
    }
    scene.set_camera(position, look);
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
    (arena::world([x, arena::FLOOR, z]), angle)
}

/// Where every slot stands in `format`.
fn homes(format: Format) -> [[[f32; 3]; 2]; 2] {
    [0, 1].map(|side| [0, 1].map(|pos| home(format, Slot::new(side, pos)).0))
}

/// How the wild visitor's battle in the session ended for it (`Some(None)`: not decided), or
/// `None` when the session isn't one.
fn wild_ending(g: &Game) -> Option<Option<Ending>> {
    g.session
        .as_ref()
        .filter(|s| s.is_wild())
        .map(Session::wild_ending)
}

/// A reserved garden visitor opens in the same owned scene; no area/save transition.
fn open_wild(g: &mut Game) {
    wild::opened();
    open_with(g, wild::trip_foe());
    if g.session.is_none() {
        let _ = wild::ended(Some(Ending::Cancelled));
    }
}

/// The player has a hatched Chao to battle with.
pub fn has_chao() -> bool {
    (0..crate::game::GARDEN_SLOTS)
        .filter_map(crate::game::read_chao_slot)
        .any(|c| usable(&c))
}

/// Opens the picker: the catalog's data, our Chao and the opponents to choose from.
fn open(g: &mut Game) {
    open_with(g, None);
}

/// Opens the picker; against `wild` (a visitor from a garden), only its team screen.
fn open_with(g: &mut Game, wild: Option<WildFoe>) {
    let from = if wild.is_some() {
        "the visitor's card"
    } else {
        "F9 > Creature battles"
    };
    let rom = match crate::creature_link::rom_from(from) {
        Ok(rom) => rom,
        Err(e) => {
            log!("battle: {e}");
            hud::toast(format!("Battle: {e}"));
            return;
        }
    };
    if g.rom
        .as_ref()
        .is_none_or(|previous| previous.fingerprint() != rom.fingerprint())
    {
        g.dex = None;
        g.book = None;
        g.roster = None;
        g.roster_read = false;
    }
    let dex = match g.dex.clone() {
        Some(dex) => dex,
        None => match Dex::from_rom(&rom) {
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
    let foe = match wild {
        Some(foe) => match wild::creature(&rom, &dex, &foe) {
            Some(creature) => Some((creature, foe.shiny)),
            None => {
                log!(
                    "battle: the wild visitor (species {}) can't battle",
                    foe.species
                );
                hud::toast("That visitor can't battle");
                return;
            }
        },
        None => None,
    };
    if visuals::enabled() && g.book.is_none() {
        let book = Book::from_rom(&rom);
        log!("battle: effect sprites read: {}", book.summary());
        g.book = Some(Arc::new(book));
    }
    if trainers::enabled() && !g.roster_read {
        g.roster_read = true;
        match Roster::from_rom(&rom, &dex) {
            Some(roster) => g.roster = Some(Arc::new(roster)),
            None => log!("battle: the catalog's trainers couldn't be read"),
        }
    }
    g.rom = Some(rom);
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
    let mut session = Session::open(dex, own, lineups, Box::new(profile_of), seed)
        .with_levels(crate::creature_link::levels());
    if trainers::enabled() {
        session = match g.roster.clone() {
            Some(roster) => session.with_trainers(roster, ladder::load()),
            None => session.with_progress(ladder::load()),
        };
    }
    match foe {
        Some((creature, shiny)) => session = session.with_wild(creature, shiny),
        None => session.set_friends(friends()),
    }
    g.session = Some(session);
    g.frames = 0;
    g.keyboard = Some(Keyboard::default());
    let _ = g.keyboard.as_mut().expect("just set").read();
    g.bars = [[f32::NAN; session::TEAM_SIZE]; 2];
}

/// A Chao's types, ability, moves (of `moves` and `traits` when given) and battle level (Normal
/// with no moves at a new Chao's level if the catalog can't be read).
fn profile_of(chao: &ChaoData, moves: Option<&Moves>, traits: Option<&Traits>) -> Profile {
    crate::creature_link::battle_profile(chao.identity(), moves, traits).unwrap_or_else(|e| {
        log!("battle: no profile ({e})");
        Profile {
            form: 0,
            types: [0, 0],
            ability: 0,
            moves: [0; 4],
            partner: None,
            level: crate::creature_link::battle_level(
                chao.identity(),
                moves.is_some() || traits.is_some(),
            )
            .0
            .level,
        }
    })
}

/// A hatched Chao that's fine to show (an egg or a damaged record isn't).
fn usable(chao: &ChaoData) -> bool {
    chao.chao_type() > chao::TYPE_EGG && chao.validate().is_ok()
}

/// Another player's Chao (its moves are their game's).
fn entry(chao: ChaoData) -> Entry {
    Entry {
        name: crate::karate::chao_name(&chao),
        chao,
        moves: None,
        traits: None,
    }
}

/// Copies of the Chao in our save, with every move, type and ability each one got.
fn own_chao() -> Vec<Entry> {
    (0..crate::game::GARDEN_SLOTS)
        .filter_map(crate::game::read_chao_slot)
        .filter(usable)
        .map(|chao| Entry {
            moves: Some(crate::creature_link::moves_of(chao.identity())),
            traits: Some(crate::creature_link::traits_of(chao.identity())),
            ..entry(chao)
        })
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

fn run_session(g: &mut Game, scene: Scene, pressed: u32) {
    let ready = g.arena == Arena::Ready;
    g.frames = g.frames.wrapping_add(1);
    let look = g.frames.is_multiple_of(NETWORK_EVERY);
    let Some(session) = g.session.as_mut() else {
        return;
    };
    if look {
        session.set_friends(friends());
        if session.waits_on_host() && crate::online::my_id().is_none() {
            session.network(Incoming::Lost);
        }
    }
    let commands = session.update(pressed, ready);
    online::send(session.take_outgoing());
    let screen = session.screen();
    for c in commands {
        match c {
            Command::Cue(cue) => cue_frame(g, cue),
            Command::OpenArena(format) => {
                log!("battle: starting ({format:?})");
                if g.session.as_ref().is_some_and(Session::is_wild) {
                    wild::fought();
                }
                g.format = format;
                let effects = visuals::enabled() && g.book.is_some();
                g.director = Some(Director::new(
                    effects,
                    homes(format),
                    g.dex.clone(),
                    g.book.clone().filter(|_| effects),
                ));
                g.arena = Arena::Ready;
            }
            Command::CloseArena => {
                end_battle(g, scene);
                return;
            }
            Command::Closed => {
                end_battle(g, scene);
                return;
            }
            Command::Released => {
                if let Some(note) = g.session.as_mut().and_then(Session::take_farewell) {
                    hud::toast(note);
                }
                end_battle(g, scene);
                return;
            }
            Command::Progress => {
                if let Some(s) = &g.session {
                    ladder::save(s.progress());
                }
            }
            Command::Levels => {
                // Friends' copies of our Chao take the levels too (`set_levels` tells them).
                if let Some(s) = &g.session {
                    crate::creature_link::set_levels(s.levels());
                }
            }
            Command::Moves(identity) => {
                if let Some(moves) = g.session.as_ref().and_then(|s| s.own_moves(identity)) {
                    crate::creature_link::set_moves(identity, moves.clone());
                }
            }
            Command::Traits(identity) => {
                if let Some(traits) = g.session.as_ref().and_then(|s| s.own_traits(identity)) {
                    crate::creature_link::set_traits(identity, traits.clone());
                }
            }
        }
    }
    animate(g);
    {
        let in_arena = g.arena == Arena::Ready;
        if in_arena {
            draw_fighters(&screen, &mut g.bars);
        }
        draw_menu(&screen);
    }
}

/// Dispose only our actor copies/effects/camera, then return to the unchanged garden.
fn end_battle(g: &mut Game, _scene: Scene) {
    let ending = wild_ending(g);
    for row in &mut g.shown {
        for shown in row.iter_mut().filter_map(Option::take) {
            arena::remove(shown.object);
        }
    }
    g.arena = Arena::Menu;
    close_session(g);
    let effects = g.director.take().is_some_and(|d| d.effects());
    effects_gfx::end(false, effects);
    figures::clear(false);
    arena::end();
    search::take_keyboard(Owner::Battle, false);
    if let Some(ending) = ending {
        let _ = wild::ended(ending);
    }
    log!("battle: returned input and camera to the garden");
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
            let species = g
                .session
                .as_ref()
                .and_then(|s| s.species(slot.side, member));
            let shiny = g
                .session
                .as_ref()
                .is_some_and(|s| s.shiny(slot.side, member));
            let form = g.session.as_ref().map_or(0, |s| s.form(slot.side, member));
            figures::show_form(slot, species, form, shiny, home(g.format, slot).0);
            if species.is_some() {
                let orders = g.director.as_mut().map(|d| d.cue(&cue)).unwrap_or_default();
                carry_out(g, &orders);
                return;
            }
            let record = g
                .session
                .as_ref()
                .and_then(|s| s.record(slot.side, member))
                .cloned();
            let Some(record) = record else { return };
            let (at, angle) = home(g.format, slot);
            match arena::spawn(&record, at, angle) {
                Some(object) => g.shown[slot.side][slot.pos] = Some(Shown { object, angle }),
                None => {
                    log!("battle: couldn't make the Chao for {slot:?}");
                    if let Some(d) = g.director.as_mut() {
                        d.cue(&Cue::Hide { slot });
                    }
                    return;
                }
            }
        }
        Cue::Hide { slot } => {
            hide(g, slot);
            figures::show(slot, None, false, [0.0; 3]);
        }
        Cue::Sound(s) => {
            arena::sound(match s {
                Sound::Cursor | Sound::Back => sound::CURSOR,
                Sound::Confirm => sound::CONFIRM,
                Sound::Hit => sound::HITS[pick % 2],
                Sound::Faint => sound::LOSER,
            });
            return;
        }
        Cue::Result { winner } if winner != Some(0) => arena::sound(sound::LOSER),
        Cue::Attack { move_id, .. } => {
            if let Some(line) = g.director.as_mut().and_then(|d| d.told(move_id)) {
                log!("battle: {line}");
            }
        }
        _ => {}
    }
    let orders = g.director.as_mut().map(|d| d.cue(&cue)).unwrap_or_default();
    carry_out(g, &orders);
}

/// Does what the director asks.
fn carry_out(g: &Game, orders: &[Order]) {
    for order in orders {
        match *order {
            Order::Motion { slot, id, blend } => {
                if let Some(object) = object_at(g, slot) {
                    arena::motion(object, id, blend);
                } else {
                    figures::play(slot, id);
                }
            }
            Order::Sound(id) => arena::sound(id),
            Order::Ball {
                slot,
                state,
                frames,
            } => {
                if let Some(object) = object_at(g, slot) {
                    effects_gfx::emote(object, state, frames);
                }
            }
            Order::Particles { kind, at, count } => effects_gfx::particles(kind, at, count, g.turn),
        }
    }
}

/// One frame of the Chao and the effects: where each stands, what starts, what's drawn.
fn animate(g: &mut Game) {
    let Some(d) = g.director.as_mut() else {
        return;
    };
    let out = d.frame();
    let scene = d.effects().then(|| d.scene());
    for &(slot, at, lift) in &out.places {
        let shown = g
            .shown
            .get(slot.side)
            .and_then(|s| s.get(slot.pos))
            .copied()
            .flatten();
        match shown {
            Some(s) => arena::place_lifted(s.object, at, s.angle, lift),
            None => figures::place(slot, [at[0], at[1] + lift, at[2]]),
        }
    }
    carry_out(g, &out.orders);
    if let Some(scene) = scene
        && g.arena == Arena::Ready
    {
        effects_gfx::show(scene, g.rom.clone());
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
/// XP bar fill (toward the next battle level).
const XP_FILL: u32 = 0xFF50_A0F0;
/// A fighter box's width in text cells.
const BOX_COLS: usize = 20;
/// An XP bar beside a menu item: at most this many cells wide, and none narrower than
/// `MIN_XP_COLS`.
const XP_COLS: usize = 10;
const MIN_XP_COLS: usize = 4;
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
/// stand), each with its name, status, HP bar and level, and for the player's own the HP and a
/// thin bar of the XP toward the next battle level.
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
        let rows = 3;
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
        let level = format!("Lv {}", line.level);
        let lx = if ours {
            x + (BOX_COLS - level.len()) as f32 * size
        } else {
            x + size
        };
        hud::text(lx, y + size * 2.0, size, TEXT, &level);
        if let Some(xp) = line.xp {
            hud::bar(
                x,
                y + size * 3.0,
                BOX_COLS as f32 * size,
                size * 0.2,
                f32::from(xp) / 1000.0,
                XP_FILL,
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
    if let Some(text) = screen.search.as_deref() {
        lines.push((format!("Search: {text}_"), GOLD));
    }
    let first = match screen.cursor {
        Some(cursor) => first_visible(screen.items.len(), cursor),
        None => screen
            .scroll
            .min(screen.items.len().saturating_sub(VISIBLE_ITEMS)),
    };
    let last = (first + VISIBLE_ITEMS).min(screen.items.len());
    if first > 0 {
        lines.push(("  ...".into(), DIM));
    }
    // XP bars start past the longest item that has one.
    let bar_col = screen
        .bars
        .iter()
        .filter_map(|&(i, _)| screen.items.get(i))
        .map(|item| item.chars().count() + 4)
        .max()
        .unwrap_or(0);
    let bar_cols = XP_COLS.min(cols.saturating_sub(bar_col));
    let mut bars = Vec::new();
    for (i, item) in screen.items.iter().enumerate().take(last).skip(first) {
        if let Some(&(_, xp)) = screen.bars.iter().find(|(b, _)| *b == i)
            && bar_cols >= MIN_XP_COLS
        {
            bars.push((lines.len(), xp));
        }
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
    if screen.search.is_some() {
        lines.push(("Left/Right: jump 10  Type: search  Enter: pick".into(), DIM));
        lines.push((
            if screen
                .search
                .as_deref()
                .is_some_and(|text| !text.is_empty())
            {
                "Backspace: delete  Esc: clear search  B: back"
            } else {
                "Esc / B: back"
            }
            .into(),
            DIM,
        ));
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
    for (row, xp) in bars {
        hud::bar(
            x + bar_col as f32 * size,
            top + size * (row as f32 + 0.3),
            bar_cols as f32 * size,
            size * 0.4,
            f32::from(xp) / 1000.0,
            XP_FILL,
        );
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
