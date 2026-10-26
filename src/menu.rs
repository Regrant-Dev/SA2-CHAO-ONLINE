//! The one in-game menu (F9): "Start now" while held at a start, connection and play
//! settings (saved to config.ini), live karate and the Chao tools, and Start/Apply/Stop.
//!
//! Keyboard: Up/Down pick a row, Left/Right change a choice, type into text rows (Backspace
//! deletes), Enter runs an action, Esc or F9 closes. The game also reads letter keys as
//! movement, so pausing first is easiest when typing.

use std::fs;
use std::path::Path;
use std::sync::Mutex;

use crate::audio;
use crate::chao_details::{self, View as ChaoView};
use crate::hud;
use crate::input::{self, Hotkey};
use crate::karate;
use crate::net::protocol::{ChaoLock, MAX_CARRIED_CHAO};
use crate::online;
use crate::puppet;
use crate::race;
use crate::room;
use crate::search::{self, Keyboard, Owner, Typed};
use crate::together;
use crate::voice;

const VK_BACK: i32 = 0x08;
const VK_RETURN: i32 = 0x0D;
const VK_SHIFT: i32 = 0x10;
const VK_ESCAPE: i32 = 0x1B;
const VK_SPACE: i32 = 0x20;
const VK_LEFT: i32 = 0x25;
const VK_UP: i32 = 0x26;
const VK_RIGHT: i32 = 0x27;
const VK_DOWN: i32 = 0x28;
const VK_PRIOR: i32 = 0x21;
const VK_NEXT: i32 = 0x22;
const VK_DECIMAL: i32 = 0x6E;
const VK_OEM_1: i32 = 0xBA; // ;:
const VK_OEM_MINUS: i32 = 0xBD; // -_
const VK_OEM_PERIOD: i32 = 0xBE; // .>

const MODES: [&str; 3] = ["Off", "Host", "Join"];
#[derive(Clone, Copy, Debug, PartialEq)]
enum Row {
    Mode,
    Name,
    Host,
    Port,
    RoomInfo,
    NameTags,
    /// Our skin (a model drawn in place of our character or kart).
    Skin,
    /// How loud skins' voices are.
    VoiceVolume,
    /// How loud the whole game is.
    GameVolume,
    Room,
    RoomMode,
    GoTo,
    Karate,
    KarateOpponent,
    Race,
    MyChao,
    ChaoDetails,
    SaveChao,
    /// Creature link: the starter gift or the starter taken, to bring into the garden
    /// (creature_link.rs).
    CreatureLink,
    CreatureBattles,
    /// Visitor 1..4: which Chao we bring (Auto, a save slot, or None).
    Visitor(usize),
    /// When others may change our Chao (`ChaoLock`).
    Lock,
    StartStop,
    Apply,
    StartNow,
}

/// Rows shown right now: "Start now" first while held at a start, then the connection and
/// play settings, then actions, and connecting/stopping at the bottom. `mode` (index into
/// `MODES`) picks the address row: Host address only when joining, Port only when hosting.
fn rows(mode: usize) -> Vec<Row> {
    let mut rows = Vec::new();
    if together::is_holding() || race::is_holding() || crate::karate_live::is_holding() {
        rows.push(Row::StartNow);
    }
    // Connection, then the room (live play together) ...
    rows.extend([Row::Mode, Row::Name]);
    match MODES[mode.min(MODES.len() - 1)] {
        "Host" => rows.push(Row::Port),
        "Join" => rows.push(Row::Host),
        _ => {}
    }
    rows.extend([
        Row::NameTags,
        Row::Skin,
        Row::VoiceVolume,
        Row::GameVolume,
        Row::Room,
    ]);
    if room::summary().is_some() {
        rows.extend([Row::RoomInfo, Row::RoomMode]);
    }
    if !online::others().is_empty() {
        rows.push(Row::GoTo);
    }
    if !online::karate_opponents().is_empty() {
        rows.push(Row::KarateOpponent);
    }
    // ... then your Chao, the copy-based options (your own game plays friends' Chao), files.
    rows.extend([
        Row::MyChao,
        Row::ChaoDetails,
        Row::SaveChao,
        Row::CreatureLink,
        Row::CreatureBattles,
    ]);
    rows.extend((0..online::BRING_CHAO).map(Row::Visitor));
    rows.push(Row::Lock);
    rows.extend([Row::Race, Row::Karate]);
    if online::status().is_some() {
        rows.extend([Row::Apply, Row::StartStop]);
    } else {
        rows.push(Row::StartStop);
    }
    rows
}
const MAX_NAME: usize = crate::net::protocol::MAX_NAME;
/// Skin voice and game volume change by this much a press (and keep going while held).
const VOLUME_STEP: u32 = 1;
const MAX_HOST: usize = 64;

const COLOR_TITLE: i32 = 0xFFFF_E080u32 as i32;
const COLOR_TEXT: i32 = 0xFFE0_E0E0u32 as i32;
const COLOR_SELECTED: i32 = 0xFF80_FFFFu32 as i32;
const COLOR_HINT: i32 = 0xFFA0_A0A0u32 as i32;

#[derive(Clone, Debug, PartialEq)]
pub struct Settings {
    pub mode: usize,
    pub name: String,
    pub host: String,
    pub port: String,
    pub name_tags: bool,
    pub chao_race: bool,
    /// Save slot of "My Chao" (-1 = the first hatched one).
    pub my_chao: i32,
    /// Visitor 1..4: a save slot, `online::VISITOR_AUTO` or `online::VISITOR_NONE`.
    pub visitors: [i32; MAX_CARRIED_CHAO],
    /// When others may change our Chao.
    pub lock: ChaoLock,
    /// Mode of rooms we create.
    pub room_mode: room::Mode,
    /// Our skin's id (`skins::list`), empty for none.
    pub skin: String,
    /// Skin voice volume, percent (`voice::set_volume`).
    pub voice_volume: u32,
    /// The game's volume, percent (`audio::set_volume`).
    pub game_volume: u32,
    /// The party-stage enemy, boss and lock-on hooks (`coop::stage_sync`): Off is a safety
    /// switch if one of them misbehaves.
    pub stage_sync: bool,
    /// Our player key as the file has it (online.rs makes and keeps it).
    pub player_key: String,
    /// Local source image and private asset cache (never an external game save).
    pub creature: crate::creature_link::Paths,
    /// Wild visitors come to the gardens (`creature_link::set_wild_visitors`).
    pub wild_visitors: bool,
    /// Gardens the server runs show their shared wild visitors and link machine
    /// (`creature_link::set_shared_visitors`).
    pub shared_visitors: bool,
    /// Battles show moves with effects and status on the Chao (`battle::visuals`).
    pub battle_effects: bool,
    /// Each garden has its link machine (`creature_link::set_link_machine`).
    pub link_machine: bool,
    /// "Trainers" in the battle menu (`battle::trainers`).
    pub battle_trainers: bool,
    /// Live battles against friends online (`battle::online`).
    pub battle_online: bool,
    /// The Black Market restocks every 10 minutes of play (`market_refresh`); off, on the
    /// game's own 3 to 7 minutes.
    pub market_refresh: bool,
}

impl Settings {
    /// From `key=value` lines (config.ini or online.ini); unknown/missing keys get defaults.
    pub fn parse(text: &str) -> Self {
        let mut s = Self {
            mode: 0,
            name: "Player".into(),
            host: "127.0.0.1:43902".into(),
            port: online::DEFAULT_PORT.to_string(),
            name_tags: true,
            chao_race: true,
            my_chao: -1,
            visitors: [online::VISITOR_AUTO; MAX_CARRIED_CHAO],
            lock: ChaoLock::Always,
            room_mode: room::Mode::Race,
            skin: String::new(),
            voice_volume: 100,
            game_volume: 100,
            stage_sync: true,
            player_key: String::new(),
            creature: crate::creature_link::Paths::default(),
            wild_visitors: true,
            shared_visitors: true,
            battle_effects: true,
            link_machine: true,
            battle_trainers: true,
            battle_online: true,
            market_refresh: false,
        };
        for (k, v) in crate::online::config_lines(text) {
            let v = v.to_string();
            match k.to_ascii_lowercase().as_str() {
                "mode" => {
                    s.mode = match v.to_ascii_lowercase().as_str() {
                        "host" => 1,
                        "join" | "client" => 2,
                        _ => 0,
                    }
                }
                "name" if !v.is_empty() => s.name = v,
                "host" if !v.is_empty() => s.host = v,
                "port" if !v.is_empty() => s.port = v,
                "mychao" => s.my_chao = v.parse().unwrap_or(-1),
                // Older files' `VisitWith` (visits on/off) is dropped: online, we always bring.
                "otherschangechao" => s.lock = ChaoLock::parse(&v),
                k if k.starts_with("visitor") => {
                    if let Some(i) = k["visitor".len()..]
                        .parse::<usize>()
                        .ok()
                        .and_then(|n| n.checked_sub(1))
                        .filter(|&i| i < MAX_CARRIED_CHAO)
                    {
                        s.visitors[i] = online::parse_visitor(&v);
                    }
                }
                "roommode" => s.room_mode = room::Mode::parse(&v),
                "skin" => s.skin = v.to_ascii_lowercase(),
                "playerkey" => s.player_key = v,
                "sourceimage" => s.creature.source = v,
                "assetcache" => s.creature.cache = v,
                "wildvisitors" => {
                    s.wild_visitors = !matches!(
                        v.to_ascii_lowercase().as_str(),
                        "off" | "0" | "false" | "no"
                    )
                }
                "sharedvisitors" => {
                    s.shared_visitors = !matches!(
                        v.to_ascii_lowercase().as_str(),
                        "off" | "0" | "false" | "no"
                    )
                }
                "linkmachine" => {
                    s.link_machine = !matches!(
                        v.to_ascii_lowercase().as_str(),
                        "off" | "0" | "false" | "no"
                    )
                }
                "battleeffects" => {
                    s.battle_effects = !matches!(
                        v.to_ascii_lowercase().as_str(),
                        "off" | "0" | "false" | "no"
                    )
                }
                "battletrainers" => {
                    s.battle_trainers = !matches!(
                        v.to_ascii_lowercase().as_str(),
                        "off" | "0" | "false" | "no"
                    )
                }
                "battleonline" => {
                    s.battle_online = !matches!(
                        v.to_ascii_lowercase().as_str(),
                        "off" | "0" | "false" | "no"
                    )
                }
                "blackmarketrefresh" => {
                    s.market_refresh =
                        matches!(v.to_ascii_lowercase().as_str(), "on" | "1" | "true" | "yes")
                }
                "gamevolume" => s.game_volume = v.parse::<u32>().map_or(100, |p| p.min(100)),
                "skinvoicevolume" => {
                    s.voice_volume = v.parse::<u32>().map_or(100, |p| p.min(voice::MAX_VOLUME))
                }
                "chaorace" => {
                    s.chao_race = !matches!(
                        v.to_ascii_lowercase().as_str(),
                        "off" | "0" | "false" | "no"
                    )
                }
                "stagesync" => {
                    s.stage_sync = !matches!(
                        v.to_ascii_lowercase().as_str(),
                        "off" | "0" | "false" | "no"
                    )
                }
                "nametags" => {
                    s.name_tags = !matches!(
                        v.to_ascii_lowercase().as_str(),
                        "off" | "0" | "false" | "no"
                    )
                }
                _ => {}
            }
        }
        s.visitors = online::tidy_visitors(s.visitors);
        s
    }

    /// `config.ini` in the format SA Mod Manager writes for configschema.xml.
    pub fn to_ini(&self) -> String {
        format!(
            "[Multiplayer]\nMode={}\nName={}\nHost={}\nPort={}\nNameTags={}\nChaoRace={}\nMyChao={}\n{}OthersChangeChao={}\nRoomMode={}\nSkin={}\nSkinVoiceVolume={}\nGameVolume={}\nStageSync={}\nPlayerKey={}\n\n[CreatureLink]\nSourceImage={}\nAssetCache={}\nWildVisitors={}\nSharedVisitors={}\nBattleEffects={}\nLinkMachine={}\nBattleTrainers={}\nBattleOnline={}\n\n[ChaoWorld]\nBlackMarketRefresh={}\n",
            MODES[self.mode],
            self.name,
            self.host,
            self.port,
            if self.name_tags { "On" } else { "Off" },
            if self.chao_race { "On" } else { "Off" },
            self.my_chao,
            self.visitors
                .iter()
                .enumerate()
                .map(|(i, v)| format!("Visitor{}={v}\n", i + 1))
                .collect::<String>(),
            self.lock.ini(),
            self.room_mode.ini(),
            self.skin,
            self.voice_volume,
            self.game_volume,
            if self.stage_sync { "On" } else { "Off" },
            // Kept as it is (online.rs made it): the server knows our Chao by it.
            online::known_key()
                .map(|k| crate::net::roster::key_hex(&k))
                .unwrap_or_else(|| self.player_key.clone()),
            self.creature.source,
            self.creature.cache,
            if self.wild_visitors { "On" } else { "Off" },
            if self.shared_visitors { "On" } else { "Off" },
            if self.battle_effects { "On" } else { "Off" },
            if self.link_machine { "On" } else { "Off" },
            if self.battle_trainers { "On" } else { "Off" },
            if self.battle_online { "On" } else { "Off" },
            if self.market_refresh { "On" } else { "Off" },
        )
    }
}

pub fn load(mod_path: &str) -> Settings {
    let dir = Path::new(mod_path);
    let settings = ["config.ini", "online.ini"]
        .iter()
        .find_map(|f| fs::read_to_string(dir.join(f)).ok())
        .map(|t| Settings::parse(&t))
        .unwrap_or_else(|| Settings::parse(""));
    crate::creature_link::set_paths(settings.creature.clone());
    crate::creature_link::set_wild_visitors(settings.wild_visitors);
    crate::creature_link::set_shared_visitors(settings.shared_visitors);
    crate::battle::visuals::set_enabled(settings.battle_effects);
    crate::creature_link::set_link_machine(settings.link_machine);
    crate::battle::trainers::set_enabled(settings.battle_trainers);
    crate::battle::online::set_enabled(settings.battle_online);
    crate::market_refresh::set_enabled(settings.market_refresh);
    settings
}

struct Menu {
    open: bool,
    row: usize,
    /// Keep the same setting selected when live party/player rows appear or disappear.
    selected: Row,
    scroll: usize,
    /// Which room choice the Room row shows.
    room_choice: usize,
    /// Which player the Go to row shows.
    goto_choice: usize,
    details: Option<ChaoView>,
    /// Chao details has the player's controls off and reads the pad (`details_buttons`).
    pad_taken: bool,
    /// This frame's menu buttons for Chao details.
    pad: u32,
    /// The creature link page, while it's open.
    creature: Option<crate::creature_link::machine::List>,
    keyboard: Option<Keyboard>,
    settings: Settings,
    message: String,
    keys: Vec<Hotkey>,
}

impl Menu {
    fn pressed(&mut self, key: i32) -> bool {
        match self.keys.iter_mut().find(|k| k.key() == key) {
            Some(k) => k.pressed(),
            None => {
                let mut k = Hotkey::new(key);
                let p = k.pressed();
                self.keys.push(k);
                p
            }
        }
    }

    /// `key` is held down and repeating this frame (after `pressed(key)` this frame).
    fn repeating(&self, key: i32) -> bool {
        self.keys
            .iter()
            .find(|k| k.key() == key)
            .is_some_and(Hotkey::repeating)
    }
}

static MENU: Mutex<Menu> = Mutex::new(Menu {
    open: false,
    row: 0,
    selected: Row::Mode,
    scroll: 0,
    room_choice: 0,
    goto_choice: 0,
    details: None,
    pad_taken: false,
    pad: 0,
    creature: None,
    keyboard: None,
    settings: Settings {
        mode: 0,
        name: String::new(),
        host: String::new(),
        port: String::new(),
        name_tags: true,
        chao_race: true,
        my_chao: -1,
        visitors: [-1; MAX_CARRIED_CHAO],
        lock: ChaoLock::Always,
        room_mode: crate::room::Mode::Race,
        skin: String::new(),
        voice_volume: 100,
        game_volume: 100,
        stage_sync: true,
        player_key: String::new(),
        creature: crate::creature_link::Paths {
            source: String::new(),
            cache: String::new(),
        },
        wild_visitors: true,
        shared_visitors: true,
        battle_effects: true,
        link_machine: true,
        battle_trainers: true,
        battle_online: true,
        market_refresh: false,
    },
    message: String::new(),
    keys: Vec::new(),
});

/// The F9 menu is open.
pub fn is_open() -> bool {
    MENU.lock().unwrap_or_else(|e| e.into_inner()).open
}

/// F9: open/close the menu.
pub fn toggle() {
    let mut m = MENU.lock().unwrap_or_else(|e| e.into_inner());
    m.open = !m.open;
    if m.open {
        m.details = None;
        m.creature = None;
        m.settings = load(online::mod_path());
        m.row = 0;
        m.selected = Row::Mode;
        m.scroll = 0;
        m.message.clear();
        // Swallow keys already held so the opening press doesn't type.
        for key in KEYS_TO_PRIME {
            let _ = m.pressed(key);
        }
        m.keyboard = Some(Keyboard::default());
        let _ = m.keyboard.as_mut().expect("just set").read();
    }
}

const KEYS_TO_PRIME: [i32; 10] = [
    VK_RETURN, VK_ESCAPE, VK_UP, VK_DOWN, VK_LEFT, VK_RIGHT, VK_BACK, VK_SPACE, VK_PRIOR, VK_NEXT,
];

/// The character a key types (with Shift state), for the text rows.
fn typed(key: i32, shift: bool) -> Option<char> {
    match key {
        0x30..=0x39 if !shift => Some((b'0' + (key - 0x30) as u8) as char),
        0x60..=0x69 => Some((b'0' + (key - 0x60) as u8) as char),
        0x41..=0x5A => {
            let c = (b'a' + (key - 0x41) as u8) as char;
            Some(if shift { c.to_ascii_uppercase() } else { c })
        }
        VK_SPACE => Some(' '),
        VK_DECIMAL => Some('.'),
        VK_OEM_PERIOD if !shift => Some('.'),
        VK_OEM_1 if shift => Some(':'),
        VK_OEM_MINUS => Some(if shift { '_' } else { '-' }),
        _ => None,
    }
}

const TYPING_KEYS: [i32; 6] = [
    VK_SPACE,
    VK_DECIMAL,
    VK_OEM_PERIOD,
    VK_OEM_1,
    VK_OEM_MINUS,
    0,
];

/// Call every frame: handles keys and draws the menu while it's open.
pub fn on_frame() {
    let mut m = MENU.lock().unwrap_or_else(|e| e.into_inner());
    search::take_keyboard(Owner::Menu, m.open && m.creature.is_some());
    m.pad = details_buttons(&mut m);
    if !m.open {
        return;
    }
    let typed = m.keyboard.get_or_insert_with(Keyboard::default).read();
    if m.details.is_some() {
        details_frame(&mut m);
        return;
    }
    if m.creature.is_some() {
        creature_frame(&mut m, &typed);
        return;
    }
    if m.pressed(VK_ESCAPE) {
        m.open = false;
        return;
    }
    let rows = rows(m.settings.mode);
    m.row = rows
        .iter()
        .position(|row| *row == m.selected)
        .unwrap_or(m.row.min(rows.len() - 1));
    if m.pressed(VK_UP) || m.repeating(VK_UP) {
        m.row = (m.row + rows.len() - 1) % rows.len();
    }
    if m.pressed(VK_DOWN) || m.repeating(VK_DOWN) {
        m.row = (m.row + 1) % rows.len();
    }
    let visible = menu_visible();
    if typed.page_up {
        m.row = m.row.saturating_sub(visible);
    }
    if typed.page_down {
        m.row = (m.row + visible).min(rows.len() - 1);
    }
    m.scroll = scroll_to(m.scroll, m.row, rows.len(), visible);
    let row = rows[m.row];
    m.selected = row;
    let (mut left, mut right) = (m.pressed(VK_LEFT), m.pressed(VK_RIGHT));
    // The volumes go a step at a time: holding the key keeps them going.
    if matches!(row, Row::VoiceVolume | Row::GameVolume) {
        left |= m.repeating(VK_LEFT);
        right |= m.repeating(VK_RIGHT);
    }
    match row {
        Row::Mode if left => m.settings.mode = (m.settings.mode + MODES.len() - 1) % MODES.len(),
        Row::Mode if right => m.settings.mode = (m.settings.mode + 1) % MODES.len(),
        Row::GoTo if left || right => {
            let n = online::others().len().max(1);
            m.goto_choice = (m.goto_choice + if left { n - 1 } else { 1 }) % n;
        }
        Row::Room if left || right => {
            // Browse the room choices; Enter applies the one shown.
            let n = room::choices().len().max(1);
            m.room_choice = (m.room_choice + if left { n - 1 } else { 1 }) % n;
        }
        Row::MyChao if left || right => {
            // Auto, then each hatched Chao in the save.
            let mut options = vec![-1];
            options.extend(online::hatched_chao().iter().map(|(slot, _)| *slot as i32));
            let at = options
                .iter()
                .position(|&o| o == m.settings.my_chao)
                .unwrap_or(0);
            let n = options.len();
            m.settings.my_chao = options[(at + if left { n - 1 } else { 1 }) % n];
            online::set_my_chao(m.settings.my_chao);
            let _ = fs::write(
                Path::new(online::mod_path()).join("config.ini"),
                m.settings.to_ini(),
            );
        }
        Row::Visitor(i) if left || right => {
            // Auto, then each hatched Chao no other row has picked, then None.
            let others: Vec<i32> = (0..online::BRING_CHAO)
                .filter(|&j| j != i)
                .map(|j| m.settings.visitors[j])
                .collect();
            let mut options = vec![online::VISITOR_AUTO];
            options.extend(
                online::garden_chao()
                    .iter()
                    .map(|(slot, _)| *slot as i32)
                    .filter(|slot| !others.contains(slot)),
            );
            options.push(online::VISITOR_NONE);
            let at = options
                .iter()
                .position(|&o| o == m.settings.visitors[i])
                .unwrap_or(0);
            let n = options.len();
            m.settings.visitors[i] = options[(at + if left { n - 1 } else { 1 }) % n];
            online::set_visitor(i, m.settings.visitors[i]);
            let _ = fs::write(
                Path::new(online::mod_path()).join("config.ini"),
                m.settings.to_ini(),
            );
        }
        Row::Lock if left || right => {
            let all = ChaoLock::ALL;
            let at = all.iter().position(|l| *l == m.settings.lock).unwrap_or(0);
            let n = all.len();
            m.settings.lock = all[(at + if left { n - 1 } else { 1 }) % n];
            online::set_lock(m.settings.lock);
            let _ = fs::write(
                Path::new(online::mod_path()).join("config.ini"),
                m.settings.to_ini(),
            );
        }
        Row::KarateOpponent if left || right => {
            let options = online::karate_opponents();
            let current = crate::karate_live::opponent();
            let at = options
                .iter()
                .position(|(id, _)| *id == current)
                .unwrap_or(0);
            let n = options.len().max(1);
            let next = (at + if left { n - 1 } else { 1 }) % n;
            crate::karate_live::set_opponent(options.get(next).and_then(|(id, _)| *id));
        }
        // Only the room's creator sets its mode; members see the creator's.
        Row::RoomMode if (left || right) && room::is_leader() => {
            let all = room::Mode::ALL;
            let at = all
                .iter()
                .position(|x| *x == m.settings.room_mode)
                .unwrap_or(0);
            let n = all.len();
            m.settings.room_mode = all[(at + if left { n - 1 } else { 1 }) % n];
            room::set_mode(m.settings.room_mode);
            let _ = fs::write(
                Path::new(online::mod_path()).join("config.ini"),
                m.settings.to_ini(),
            );
        }
        Row::Race if left || right => {
            m.settings.chao_race = !m.settings.chao_race;
            race::set_enabled(m.settings.chao_race);
            let _ = fs::write(
                Path::new(online::mod_path()).join("config.ini"),
                m.settings.to_ini(),
            );
        }
        Row::Karate if (left || right) && !karate::available() => {}
        Row::Karate if left || right => {
            // CPU, then every Chao other players brought online.
            let choices = online::karate_choices();
            let current = karate::opponent_label();
            let at = current
                .as_ref()
                .and_then(|l| choices.iter().position(|(c, _)| c == l))
                .map_or(0, |i| i + 1);
            let n = choices.len() + 1;
            let next = (at + if left { n - 1 } else { 1 }) % n;
            karate::set_opponent(next.checked_sub(1).map(|i| choices[i].clone()));
        }
        Row::Skin if left || right => {
            // None, then each skin found in <mod>/skins.
            let list = crate::skins::list();
            let at = list
                .iter()
                .position(|(id, _)| *id == m.settings.skin)
                .map_or(0, |i| i + 1);
            let n = list.len() + 1;
            let next = (at + if left { n - 1 } else { 1 }) % n;
            m.settings.skin = next
                .checked_sub(1)
                .map_or_else(String::new, |i| list[i].0.clone());
            crate::skins::set_mine((!m.settings.skin.is_empty()).then(|| m.settings.skin.clone()));
            let _ = fs::write(
                Path::new(online::mod_path()).join("config.ini"),
                m.settings.to_ini(),
            );
        }
        Row::VoiceVolume if left || right => {
            let v = m.settings.voice_volume;
            m.settings.voice_volume = if left {
                v.saturating_sub(VOLUME_STEP)
            } else {
                (v + VOLUME_STEP).min(voice::MAX_VOLUME)
            };
            voice::set_volume(m.settings.voice_volume);
            let _ = fs::write(
                Path::new(online::mod_path()).join("config.ini"),
                m.settings.to_ini(),
            );
        }
        Row::GameVolume if left || right => {
            let v = m.settings.game_volume;
            m.settings.game_volume = if left {
                v.saturating_sub(VOLUME_STEP)
            } else {
                (v + VOLUME_STEP).min(100)
            };
            audio::set_volume(m.settings.game_volume);
            let _ = fs::write(
                Path::new(online::mod_path()).join("config.ini"),
                m.settings.to_ini(),
            );
        }
        Row::NameTags if left || right => {
            m.settings.name_tags = !m.settings.name_tags;
            puppet::set_name_tags(m.settings.name_tags);
            let _ = fs::write(
                Path::new(online::mod_path()).join("config.ini"),
                m.settings.to_ini(),
            );
        }
        _ => {}
    }
    if matches!(row, Row::Name | Row::Host | Row::Port) {
        edit_text(&mut m, row);
    }
    if m.pressed(VK_RETURN) {
        if row == Row::CreatureLink {
            if crate::creature_link::link_machine() {
                crate::creature_link::retry_import();
                hud::toast(match crate::creature_link::rom_from("F9 > Creature link") {
                    Ok(_) => {
                        "Use the link machine in a garden: walk up, press the action button".into()
                    }
                    Err(message) => message,
                });
            } else {
                m.creature = Some(crate::creature_link::machine::List::new(
                    crate::creature_link::Page::open("F9 > Creature link"),
                ));
            }
            return;
        }
        if row == Row::ChaoDetails {
            let choices = online::garden_chao();
            let preferred = crate::garden::held_save_slot()
                .or_else(|| usize::try_from(m.settings.my_chao).ok())
                .or_else(|| {
                    choices
                        .iter()
                        .find(|(_, c)| i32::from(c.garden()) == crate::game::current_chao_area())
                        .map(|(s, _)| *s)
                });
            let mut view = ChaoView {
                slot: preferred,
                ..Default::default()
            };
            view.retain_slot(&choices.iter().map(|(s, _)| *s).collect::<Vec<_>>());
            draw_details(&mut view, &choices);
            m.details = Some(view);
            return;
        }
        let message = match row {
            Row::Room => room::choices()
                .get(m.room_choice)
                .map(|(choice, _)| room::choose(*choice)),
            Row::StartStop => Some(start_stop(&mut m)),
            Row::SaveChao => Some(crate::saving::save_now()),
            Row::CreatureBattles => Some(if crate::battle::game::is_open() {
                crate::battle::game::request_close();
                "Returning to the garden".into()
            } else {
                match crate::battle::game::request_open() {
                    Ok(()) => "Opening Creature battles".into(),
                    Err(message) => message,
                }
            }),
            Row::GoTo => {
                let others = online::others();
                others
                    .get(m.goto_choice.min(others.len().saturating_sub(1)))
                    .map(|(id, _, _)| crate::goto::go_to(*id))
            }
            Row::Apply => {
                online::stop();
                Some(start_stop(&mut m))
            }
            Row::StartNow => {
                together::start_now();
                race::start_now();
                crate::karate_live::start_now();
                Some("Starting...".to_string())
            }
            _ => None,
        };
        // Close right away: Enter is also the game's Start button, so the next press (to
        // unpause) must not run an action again. Results show up as toasts.
        if let Some(message) = message {
            hud::toast(message);
            m.open = false;
            return;
        }
    }
    draw(&m, &rows);
}

/// Chao details: Left/Right turn the page, Up/Down scroll, PgUp/PgDn show another of our Chao,
/// Esc/Backspace go back; on a controller the D-pad or stick, L/R and B.
fn details_frame(m: &mut Menu) {
    let Some(mut view) = m.details.take() else {
        return;
    };
    let pad = m.pad;
    // The game's own keys also press the pad: a direction counts once while its arrow is down.
    let pad_hit = |bit: u32, key: i32| pad & bit != 0 && !input::key_down(key);
    let escape = m.pressed(VK_ESCAPE);
    let backspace = m.pressed(VK_BACK);
    if escape || backspace || pad & PAD_B != 0 {
        return;
    }
    let choices = online::garden_chao();
    let slots: Vec<usize> = choices.iter().map(|(slot, _)| *slot).collect();
    view.retain_slot(&slots);
    let held = |m: &mut Menu, key: i32| m.pressed(key) || m.repeating(key);
    let left = held(m, VK_LEFT) || pad_hit(PAD_LEFT, VK_LEFT);
    let right = held(m, VK_RIGHT) || pad_hit(PAD_RIGHT, VK_RIGHT);
    let up = held(m, VK_UP) || pad_hit(PAD_UP, VK_UP);
    let down = held(m, VK_DOWN) || pad_hit(PAD_DOWN, VK_DOWN);
    let previous = m.pressed(VK_PRIOR) || pad & PAD_L != 0;
    let next = m.pressed(VK_NEXT) || pad & PAD_R != 0;
    if left != right {
        view.turn_page(left);
    }
    if up {
        view.scroll = view.scroll.saturating_sub(1);
    }
    if down {
        // Kept in range when drawn.
        view.scroll = view.scroll.saturating_add(1);
    }
    if previous != next {
        view.select(&slots, previous);
    }
    view.pad = m.pad_taken;
    draw_details(&mut view, &choices);
    m.details = Some(view);
}

/// Menu buttons (`game::menu_buttons_pressed`): B, up, down, left, right, L, R.
const PAD_B: u32 = 0x02;
const PAD_UP: u32 = 0x10;
const PAD_DOWN: u32 = 0x20;
const PAD_LEFT: u32 = 0x40;
const PAD_RIGHT: u32 = 0x80;
const PAD_L: u32 = 0x1_0000;
const PAD_R: u32 = 0x2_0000;
/// The pause screen's game state: its menu reads the same buttons.
const PAUSED: i16 = 17;

/// Player 1's menu buttons for Chao details, with the player's controls off while it's open so
/// the same presses don't also move them. Taken only in play (a player with their controls, in
/// a stage or garden, not under one of the game's own screens or menus, which read the same
/// buttons) while the game isn't paused and has focus. Let go as soon as play ends (the game's
/// own screens then own the controls), and given back once the details close, unless the game
/// has meanwhile ended the stage. Ignored while unfocused: the pad still reads in the
/// background, and `game::run_unfocused` holds the controls then.
fn details_buttons(m: &mut Menu) -> u32 {
    use crate::game::{GAME_STATE_INGAME, game_state, local_entity, set_controls};
    let open = m.open && m.details.is_some();
    let state = game_state();
    let paused = state == PAUSED;
    let player = !local_entity().is_null();
    let focused = input::game_focused();
    if m.pad_taken && !(player && (state == GAME_STATE_INGAME || paused)) {
        m.pad_taken = false;
        return 0;
    }
    if !open {
        // While unfocused the release waits for focus, so `run_unfocused` has given the
        // controls back first.
        if m.pad_taken && focused {
            m.pad_taken = false;
            let held = together::is_holding()
                || crate::race::is_holding()
                || crate::karate_live::is_holding();
            if (state == GAME_STATE_INGAME || paused) && !crate::game::stage_cleared() && !held {
                set_controls(true);
            }
        }
        return 0;
    }
    if !m.pad_taken && state == GAME_STATE_INGAME && player && focused {
        // SAFETY: a static byte the game itself toggles (`game::set_controls`).
        let controls =
            unsafe { (crate::game::CONTROLLER_ENABLED as *const u8).read_volatile() } != 0;
        m.pad_taken = controls;
    }
    if !m.pad_taken {
        return 0;
    }
    set_controls(false);
    if paused || !focused {
        0
    } else {
        crate::game::menu_buttons_pressed()
    }
}

/// The highlight behind the current page's tab.
const TAB_HIGHLIGHT: u32 = 0xFF2C_3C5C;

fn draw_details(view: &mut ChaoView, choices: &[(usize, crate::chao::ChaoData)]) {
    let (w, h) = hud::screen_size();
    let (size, visible) = chao_details::layout(w, h);
    let index = choices
        .iter()
        .position(|(slot, _)| Some(*slot) == view.slot);
    let chosen = index.map(|i| &choices[i]);
    if let Some((_, c)) = chosen {
        let id = c.identity();
        if view.battle.as_ref().is_none_or(|(known, _)| *known != id) {
            view.battle = Some((id, battle_details(c)));
        }
    }
    let label = chosen.map_or_else(
        || "No Chao in your save".into(),
        |(slot, c)| chao_label(*slot, c),
    );
    let screen = chao_details::screen(
        chosen.map(|(_, c)| c),
        &label,
        index,
        choices.len(),
        view,
        visible,
    );
    // Everything on whole cells of the font size, so the panel, the text and the bars line up.
    let x = ((w / size / 2.0).floor() - (chao_details::PANEL_COLUMNS / 2) as f32).max(0.0) * size;
    let y = size;
    let cell = |col: usize| x + (col + 1) as f32 * size;
    let row_y = |row: usize| y + (row + 1) as f32 * size;
    hud::panel(
        x,
        y,
        size,
        chao_details::PANEL_COLUMNS,
        screen.rows.len() + 2,
    );
    let (row, col, width) = screen.tab;
    hud::rect(
        cell(col) - size * 0.4,
        row_y(row) - size * 0.1,
        cell(col + width) + size * 0.4,
        row_y(row) + size * 1.1,
        TAB_HIGHLIGHT,
    );
    for (i, line) in screen.rows.iter().enumerate() {
        let line_y = row_y(i);
        for bar in &line.bars {
            for ([x1, y1, x2, y2], color) in
                chao_details::bar_rects(bar, cell(bar.col), line_y, size)
            {
                hud::rect(x1, y1, x2, y2, color);
            }
        }
        for span in &line.spans {
            hud::text(
                cell(span.col),
                line_y,
                size,
                span.tone.colour() as i32,
                &span.text,
            );
        }
    }
}

/// What the mod keeps of our Chao's battles (`creature_link`), named from the player's catalog.
fn battle_details(c: &crate::chao::ChaoData) -> chao_details::Battle {
    let id = c.identity();
    let (level, xp_known) = crate::creature_link::battle_level(id, true);
    let rom = crate::creature_link::rom_from("F9 > Chao details").ok();
    let partner = if !crate::creature_link::has_partner(id) {
        chao_details::Partner::None
    } else {
        match &rom {
            None => chao_details::Partner::NoCatalog,
            Some(_) => crate::creature_link::partner_types(id).map_or(
                chao_details::Partner::NoCatalog,
                chao_details::Partner::Types,
            ),
        }
    };
    let moves = crate::creature_link::moves_of(id);
    let name = |m: u16| {
        rom.as_ref()
            .and_then(|r| r.move_name(m))
            .map_or_else(|| format!("Move #{m}"), |n| crate::creature_link::ascii(&n))
    };
    chao_details::Battle {
        level: level.level,
        xp: xp_known.then(|| (level.xp, crate::battle::levels::step(level.level))),
        partner,
        moves: moves
            .active
            .iter()
            .filter(|&&m| m != 0)
            .map(|&m| name(m))
            .collect(),
        known: moves.known.len(),
    }
}

/// The creature link page: Up/Down (PgUp/PgDn) pick, Enter brings the one picked, Esc/Backspace
/// back.
fn creature_frame(m: &mut Menu, typed: &Typed) {
    use crate::creature_link::machine::{Key, Outcome};
    // The shared keyboard owns these presses here. Track them in the parent too, so the
    // same held Esc doesn't close F9 on the frame after it returned from this list.
    for key in [VK_ESCAPE, VK_BACK, VK_RETURN] {
        let _ = m.pressed(key);
    }
    let Some(mut list) = m.creature.take() else {
        return;
    };
    let size = hud::menu_font();
    let (w, h) = hud::screen_size();
    let visible = (((h / size) as usize).saturating_sub(14)).clamp(3, 20);
    if list.typed(typed, visible) == Outcome::Close {
        return;
    }
    if m.pressed(VK_UP) || m.repeating(VK_UP) {
        list.press(Key::Up, visible);
    }
    if m.pressed(VK_DOWN) || m.repeating(VK_DOWN) {
        list.press(Key::Down, visible);
    }
    if m.pressed(VK_LEFT) || m.repeating(VK_LEFT) {
        list.press(Key::JumpUp, visible);
    }
    if m.pressed(VK_RIGHT) || m.repeating(VK_RIGHT) {
        list.press(Key::JumpDown, visible);
    }
    if typed.page_up {
        list.press(Key::PageUp, visible);
    }
    if typed.page_down {
        list.press(Key::PageDown, visible);
    }
    if typed.enter {
        let message = match list.press(Key::Pick, visible) {
            Outcome::Chose(crate::creature_link::Action::Bring(c)) => {
                Some(crate::creature_link::import(&c))
            }
            Outcome::Chose(crate::creature_link::Action::Gift(species)) => {
                Some(crate::creature_link::take_gift(species))
            }
            _ => None,
        };
        if let Some(message) = message {
            hud::toast(message);
            m.open = false;
            return;
        }
    }
    list.scroll_to(visible);
    let page = &list.page;
    let shown = list.shown();
    let mut lines: Vec<(String, i32)> = vec![
        ("== Creature link ==".into(), COLOR_TITLE),
        (format!("Search: {}_", list.search.text()), COLOR_TITLE),
    ];
    for &i in shown.iter().skip(page.scroll).take(visible) {
        let (text, action) = &page.rows[i];
        let selected = i == page.row;
        let color = if selected { COLOR_SELECTED } else { COLOR_TEXT };
        if *action == crate::creature_link::Action::None {
            // A message (why the list can't show): every word of it, on as many lines.
            let parts = crate::creature_link::machine::wrap(text, MESSAGE_COLS);
            for (n, part) in parts.into_iter().enumerate() {
                let lead = if n == 0 && selected { ">" } else { " " };
                lines.push((format!("{lead} {part}"), color));
            }
        } else {
            lines.push((
                format!("{} {text}", if selected { ">" } else { " " }),
                color,
            ));
        }
    }
    if shown.is_empty() {
        lines.push(("No matches. Esc clears the search.".into(), COLOR_HINT));
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
    lines.extend([
        (
            "Enter brings the one picked into this garden as a partner".into(),
            COLOR_HINT,
        ),
        (
            "for your Chao (each once a day per garden, 4 at a time).".into(),
            COLOR_HINT,
        ),
        (
            "Up/Down pick. Left/Right jump 10. PgUp/PgDn a page.".into(),
            COLOR_HINT,
        ),
        (
            "Type to search. Backspace deletes. Esc clears / goes back.".into(),
            COLOR_HINT,
        ),
    ]);
    let cols = (lines
        .iter()
        .map(|(t, _)| t.chars().count())
        .max()
        .unwrap_or(0)
        + 2)
    .min(((w / size) as usize).saturating_sub(2).max(4));
    let x = ((w / size / 2.0).floor() - (cols / 2) as f32).max(0.0) * size;
    let y = 3.0 * size;
    hud::panel(x, y, size, cols, lines.len() + 2);
    for (i, (text, color)) in lines.iter().enumerate() {
        hud::text(
            x + size,
            y + (i + 1) as f32 * size,
            size,
            *color,
            &clip_line(text, cols.saturating_sub(2)),
        );
    }
    m.creature = Some(list);
}

/// Longest line of a message on the creature link page before it goes on to the next line.
const MESSAGE_COLS: usize = 58;

fn edit_text(m: &mut Menu, row: Row) {
    let shift = input::key_down(VK_SHIFT);
    let keys = (0x30..=0x39)
        .chain(0x41..=0x5A)
        .chain(0x60..=0x69)
        .chain(TYPING_KEYS);
    let mut typed_now = Vec::new();
    for key in keys.filter(|&k| k != 0) {
        if m.pressed(key)
            && let Some(c) = typed(key, shift)
        {
            typed_now.push(c);
        }
    }
    let backspace = m.pressed(VK_BACK) || m.repeating(VK_BACK);
    let (field, max, digits) = match row {
        Row::Name => (&mut m.settings.name, MAX_NAME, false),
        Row::Host => (&mut m.settings.host, MAX_HOST, false),
        _ => (&mut m.settings.port, 5, true),
    };
    if backspace {
        field.pop();
    }
    for c in typed_now {
        if field.len() < max && (!digits || c.is_ascii_digit()) {
            field.push(c);
        }
    }
}

fn start_stop(m: &mut Menu) -> String {
    if online::status().is_some() {
        online::stop();
        return "Stopped".into();
    }
    let path = Path::new(online::mod_path()).join("config.ini");
    match fs::write(&path, m.settings.to_ini()) {
        Err(e) => format!("Can't save settings: {e}"),
        Ok(()) if m.settings.mode == 0 => "Saved. Multiplayer is Off.".into(),
        Ok(()) => match online::restart() {
            Ok(()) => "Saved and started".into(),
            Err(e) => e,
        },
    }
}

/// "Name (Hero garden)" for a save slot, or "first Chao" for -1.
/// A Chao for the menu: "Chao (Neutral garden, slot 2)".
fn chao_label(slot: usize, chao: &crate::chao::ChaoData) -> String {
    const GARDENS: [&str; 4] = ["?", "Neutral", "Hero", "Dark"];
    let name = if chao.chao_type() == crate::chao::TYPE_EGG {
        "Egg".into()
    } else {
        karate::chao_name(chao)
    };
    format!(
        "{} ({} garden, slot {})",
        name,
        GARDENS[usize::from(chao.garden()).min(3)],
        slot + 1
    )
}

/// Visitor row `row` set to `slot`: what it comes to in this garden ("empty" when nobody).
fn visitor_label(row: usize, slot: i32) -> String {
    let now = online::visitor_preview().map(|rows| rows[row].clone());
    let shown = |c: &Option<(usize, crate::chao::ChaoData)>| match c {
        Some((slot, chao)) => chao_label(*slot, chao),
        None => "empty".into(),
    };
    match slot {
        online::VISITOR_NONE => "None".into(),
        online::VISITOR_AUTO => match now {
            Some(c) => format!("Auto: {}", shown(&c)),
            None => "Auto".into(),
        },
        slot => {
            let picked = online::garden_chao()
                .into_iter()
                .find(|(s, _)| *s as i32 == slot)
                .map_or_else(|| format!("slot {}", slot + 1), |(s, c)| chao_label(s, &c));
            // In a garden it doesn't live in, it stays home.
            match now {
                Some(None) => format!("{picked}: not in this garden"),
                _ => picked,
            }
        }
    }
}

fn my_chao_label(slot: i32) -> String {
    const GARDENS: [&str; 4] = ["?", "Neutral", "Hero", "Dark"];
    let hatched = online::hatched_chao();
    match hatched.iter().find(|(s, _)| *s as i32 == slot) {
        Some((s, chao)) => format!(
            "{} ({} garden, slot {})",
            karate::chao_name(chao),
            GARDENS[usize::from(chao.garden()).min(3)],
            s + 1
        ),
        // Automatic: the first hatched Chao.
        None => match hatched.first() {
            Some((s, chao)) => format!("Auto: {} (slot {})", karate::chao_name(chao), s + 1),
            None => "Auto (no hatched Chao yet)".into(),
        },
    }
}

/// Keep the selected row visible and avoid blank space after the last page.
fn scroll_to(scroll: usize, row: usize, len: usize, visible: usize) -> usize {
    let visible = visible.max(1);
    let scroll = scroll.min(len.saturating_sub(visible));
    if row < scroll {
        row
    } else if row >= scroll + visible {
        row + 1 - visible
    } else {
        scroll
    }
}

fn menu_visible() -> usize {
    let (_, h) = hud::screen_size();
    ((h / hud::menu_font()) as usize)
        .saturating_sub(12)
        .clamp(3, 28)
}

fn clip_line(text: &str, cols: usize) -> String {
    let len = text.chars().count();
    if len <= cols {
        return text.into();
    }
    if cols < 4 {
        return text.chars().take(cols).collect();
    }
    // Editable rows end with their cursor. Keep it and the newest text visible, along with
    // the label, while a long address is being entered.
    let head = if text.ends_with('_') && cols >= 12 {
        (cols / 2).min(16)
    } else {
        cols - 3
    };
    let mut clipped: String = text.chars().take(head).collect();
    clipped.push_str("...");
    if head < cols - 3 {
        clipped.extend(text.chars().skip(len - (cols - head - 3)));
    }
    clipped
}

fn draw(m: &Menu, rows: &[Row]) {
    let status = online::status();
    let on_off = |b: bool| if b { "On" } else { "Off" };
    let mut lines: Vec<(String, i32)> = vec![("== SA2 Chao Online ==".into(), COLOR_TITLE)];
    let visible = menu_visible();
    for (i, row) in rows.iter().enumerate().skip(m.scroll).take(visible) {
        let text = match row {
            Row::Mode => format!("Multiplayer:  < {} >", MODES[m.settings.mode]),
            Row::Name => format!("Your name:    {}", m.settings.name),
            Row::Host => format!("Host address: {}", m.settings.host),
            Row::Port => format!("Port:         {}", m.settings.port),
            Row::RoomInfo => format!(
                "  {}",
                room::summary().unwrap_or_else(|| "Not in a party".into())
            ),
            Row::RoomMode if room::is_leader() => {
                format!("  Mode:       < {} >", room::mode().label())
            }
            Row::RoomMode => format!(
                "  Mode:       {} (set by the creator)",
                room::mode().label()
            ),
            Row::GoTo => {
                let others = online::others();
                match others.get(m.goto_choice.min(others.len().saturating_sub(1))) {
                    Some((_, name, state)) => {
                        let place = state.map_or_else(
                            || "joining".to_string(),
                            |s| crate::places::place(s.level, s.chao_area),
                        );
                        format!("Go to:        < {name} - {place} >  (Enter)")
                    }
                    None => "Go to:        (nobody else online)".into(),
                }
            }
            Row::Room => {
                let choices = room::choices();
                match choices.get(m.room_choice.min(choices.len().saturating_sub(1))) {
                    Some((_, label)) => format!("Party:        < {label} >  (Enter)"),
                    None => "Party:        (connect first)".into(),
                }
            }
            Row::Visitor(i) => format!(
                "Bring Chao {}: < {} >",
                i + 1,
                visitor_label(*i, m.settings.visitors[*i])
            ),
            Row::MyChao => format!(
                "My Chao (races, karate): < {} >",
                my_chao_label(m.settings.my_chao)
            ),
            Row::ChaoDetails => "[ Chao details: stats, personality, bonds & more ]".into(),
            Row::SaveChao => "[ Save Chao garden now ] (autosaves every 5 min)".into(),
            Row::CreatureBattles if crate::battle::game::is_open() => {
                "[ Creature battles: return to garden ]".into()
            }
            Row::CreatureBattles => "[ Creature battles: trainers, wild & friends ]".into(),
            Row::CreatureLink if crate::creature_link::link_machine() => {
                "Creature link: use the link machine in a garden".into()
            }
            Row::CreatureLink => "[ Creature link: starter partner ]".into(),
            Row::Lock => format!("Others can change my Chao: < {} >", m.settings.lock.label()),
            Row::KarateOpponent => {
                let options = online::karate_opponents();
                let current = crate::karate_live::opponent();
                let label = options
                    .iter()
                    .find(|(id, _)| *id == current)
                    .or(options.first())
                    .map_or("", |(_, l)| l.as_str());
                format!("Live karate vs: < {label} >")
            }
            Row::Race => format!(
                "Friends' Chao in races: < {} >",
                on_off(m.settings.chao_race)
            ),
            Row::Karate if !karate::available() => "Practice karate: (needs the Battle DLC)".into(),
            Row::Karate => format!(
                "Practice karate: < {} >",
                karate::opponent_label().map_or_else(|| "CPU".into(), |l| format!("{l} (copy)"))
            ),
            Row::NameTags => format!("Name tags:    < {} >", on_off(m.settings.name_tags)),
            Row::Skin if crate::skins::list().is_empty() => {
                "Skin:         (none in skins folder)".into()
            }
            Row::Skin => format!(
                "Skin:         < {} >",
                crate::skins::mine_name().unwrap_or_else(|| "None".into())
            ),
            Row::VoiceVolume => format!("Skin voices:  < {}% >", m.settings.voice_volume),
            Row::GameVolume => format!("Game volume:  < {}% >", m.settings.game_volume),
            Row::Apply => "[ Apply settings & reconnect ]".into(),
            Row::StartStop if status.is_some() => "[ Stop multiplayer ]".into(),
            Row::StartStop if m.settings.mode == 0 => "[ Save settings ]".into(),
            Row::StartStop => "[ Save & Start ]".into(),
            Row::StartNow => "[ Start now ] (everyone in this stage)".into(),
        };
        let selected = i == m.row;
        let cursor = if selected && matches!(row, Row::Name | Row::Host | Row::Port) {
            "_"
        } else {
            ""
        };
        let text = format!("{} {text}{cursor}", if selected { ">" } else { " " });
        lines.push((text, if selected { COLOR_SELECTED } else { COLOR_TEXT }));
    }
    if rows.len() > visible {
        lines.push((
            format!(
                "Showing {}-{} of {}",
                m.scroll + 1,
                (m.scroll + visible).min(rows.len()),
                rows.len()
            ),
            COLOR_HINT,
        ));
    }
    lines.push((
        format!("Status: {}", status.as_deref().unwrap_or("off")),
        COLOR_TEXT,
    ));
    lines.push((
        "Up/Down select. PgUp/PgDn a page. Esc/F9 close.".into(),
        COLOR_HINT,
    ));
    lines.push((
        match rows[m.row] {
            Row::Name | Row::Host | Row::Port => "Type to edit. Hold Backspace to erase.",
            Row::ChaoDetails
            | Row::SaveChao
            | Row::CreatureLink
            | Row::CreatureBattles
            | Row::Apply
            | Row::StartStop
            | Row::StartNow => "Enter: open / run the selected action.",
            Row::Room | Row::GoTo => "Left/Right choose. Enter: confirm.",
            Row::RoomInfo | Row::RoomMode => "Party mode is set by its creator.",
            _ => "Left/Right change the selected setting.",
        }
        .into(),
        COLOR_HINT,
    ));
    if matches!(rows[m.row], Row::Name | Row::Host | Row::Port) {
        lines.push((
            "Pause first when typing: letters also move you.".into(),
            COLOR_HINT,
        ));
    }

    // Everything on whole cells of the font size, so the panel and the text line up.
    let size = hud::menu_font();
    let (w, _) = hud::screen_size();
    let cols = (lines
        .iter()
        .map(|(t, _)| t.chars().count())
        .max()
        .unwrap_or(0)
        + 2)
    .min(((w / size) as usize).saturating_sub(2).max(4));
    let x = ((w / size / 2.0).floor() - (cols / 2) as f32).max(0.0) * size;
    let y = 3.0 * size;
    hud::panel(x, y, size, cols, lines.len() + 2);
    for (i, (text, color)) in lines.iter().enumerate() {
        hud::text(
            x + size,
            y + (i + 1) as f32 * size,
            size,
            *color,
            &clip_line(text, cols.saturating_sub(2)),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::{ChaoLock, Settings, typed};

    #[test]
    fn scrolling_keeps_both_ends_and_a_changed_list_in_view() {
        assert_eq!(super::scroll_to(0, 12, 25, 5), 8);
        assert_eq!(super::scroll_to(8, 12, 25, 5), 8);
        assert_eq!(super::scroll_to(8, 3, 25, 5), 3);
        assert_eq!(super::scroll_to(20, 22, 25, 5), 20);
        assert_eq!(super::scroll_to(20, 7, 8, 5), 3);
        assert_eq!(super::scroll_to(20, 0, 2, 5), 0);
    }

    #[test]
    fn settings_round_trip_through_config_ini() {
        let s = Settings {
            mode: 2,
            name: "Regrant".into(),
            host: "100.101.102.103:27015".into(),
            port: "27015".into(),
            name_tags: false,
            chao_race: false,
            my_chao: 5,
            visitors: [3, -2, -1, 7],
            lock: ChaoLock::ThatGarden,
            room_mode: crate::room::Mode::Coop,
            skin: "race_car".into(),
            voice_volume: 70,
            game_volume: 60,
            stage_sync: false,
            player_key: "00112233445566778899aabbccddeeff".into(),
            creature: crate::creature_link::Paths {
                source: "C:\\games\\source game.3ds".into(),
                cache: "private/cache".into(),
            },
            wild_visitors: false,
            shared_visitors: false,
            battle_effects: false,
            link_machine: false,
            battle_trainers: false,
            battle_online: false,
            market_refresh: false,
        };
        assert_eq!(Settings::parse(&s.to_ini()), s);
        // The networking side reads the same file.
        let c = crate::online::parse_config(&s.to_ini()).unwrap();
        assert_eq!(c.name, "Regrant");
        assert_eq!(c.room_mode, crate::room::Mode::Coop);
        assert_eq!(
            c.key,
            Some([
                0x00, 0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77, 0x88, 0x99, 0xAA, 0xBB, 0xCC, 0xDD,
                0xEE, 0xFF
            ])
        );
    }

    #[test]
    fn the_menu_and_the_session_read_a_file_the_same_way() {
        let text = "; a note\nMode = host\n# another\nName = Team#1;A\nPort = 4000\n";
        let s = Settings::parse(text);
        let c = crate::online::parse_config(text).unwrap();
        assert_eq!((s.mode, s.name.as_str()), (1, "Team#1;A"));
        assert_eq!(c.name, s.name);
        assert!(matches!(c.mode, crate::online::Mode::Host { port: 4000 }));
    }

    #[test]
    fn shared_visitors_are_on_unless_switched_off() {
        assert!(Settings::parse("").shared_visitors);
        let off = Settings::parse("[CreatureLink]\nSharedVisitors=Off\n");
        assert!(!off.shared_visitors && off.wild_visitors);
        assert!(off.to_ini().contains("\nSharedVisitors=Off\n"));
        assert_eq!(Settings::parse(&off.to_ini()), off);
        let on = Settings {
            shared_visitors: true,
            ..off
        };
        assert!(on.to_ini().contains("\nSharedVisitors=On\n"));
    }

    #[test]
    fn reads_old_online_ini() {
        let s = Settings::parse("mode = client\nname = Player1\nhost = 1.2.3.4:5\n");
        assert_eq!(
            (s.mode, s.name.as_str(), s.host.as_str()),
            (2, "Player1", "1.2.3.4:5")
        );
    }

    #[test]
    fn paths_keep_semicolons_and_hashes_and_only_whole_lines_are_comments() {
        let s = Settings::parse(
            "; a comment\n# another\n[CreatureLink]\nSourceImage=C:\\Games;#1\\game.3ds\n\
             AssetCache = D:\\cache #2\n  ;SourceImage=ignored\n",
        );
        assert_eq!(s.creature.source, "C:\\Games;#1\\game.3ds");
        assert_eq!(s.creature.cache, "D:\\cache #2");
    }

    #[test]
    fn old_visit_key_is_dropped_and_the_lock_is_kept() {
        let s = Settings::parse("[Multiplayer]\nVisitWith=Off\nOthersChangeChao=AnyGarden\n");
        // `AnyGarden` reads as ChaoWorld.
        assert_eq!(s.lock, ChaoLock::ChaoWorld);
        assert!(!s.to_ini().contains("VisitWith"));
        assert!(s.to_ini().contains("OthersChangeChao=ChaoWorld\n"));
        assert_eq!(Settings::parse("VisitWith=On").lock, ChaoLock::Always);
        // Every visitor row is a row of the menu, and the lock after them.
        let rows = super::rows(0);
        let at = rows.iter().position(|r| *r == super::Row::Lock).unwrap();
        assert_eq!(rows[at - 4..at], [0, 1, 2, 3].map(super::Row::Visitor));
    }

    #[test]
    fn typing_keys() {
        assert_eq!(typed(0x41, false), Some('a'));
        assert_eq!(typed(0x41, true), Some('A'));
        assert_eq!(typed(0x35, false), Some('5'));
        assert_eq!(typed(0xBA, true), Some(':'));
        assert_eq!(typed(0xBE, false), Some('.'));
        assert_eq!(typed(0x25, false), None);
    }
}
