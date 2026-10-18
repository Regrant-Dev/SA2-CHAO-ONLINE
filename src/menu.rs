//! The one in-game menu (F9): "Start now" while held at a start, connection and play
//! settings (saved to config.ini), live karate and the Chao tools, and Start/Apply/Stop.
//!
//! Keyboard: Up/Down pick a row, Left/Right change a choice, type into text rows (Backspace
//! deletes), Enter runs an action, Esc or F9 closes. The game also reads letter keys as
//! movement, so pausing first is easiest when typing.

use std::fs;
use std::path::Path;
use std::sync::Mutex;

use crate::hud;
use crate::input::{self, Hotkey};
use crate::karate;
use crate::online;
use crate::puppet;
use crate::race;
use crate::room;
use crate::together;

const VK_BACK: i32 = 0x08;
const VK_RETURN: i32 = 0x0D;
const VK_SHIFT: i32 = 0x10;
const VK_ESCAPE: i32 = 0x1B;
const VK_SPACE: i32 = 0x20;
const VK_LEFT: i32 = 0x25;
const VK_UP: i32 = 0x26;
const VK_RIGHT: i32 = 0x27;
const VK_DOWN: i32 = 0x28;
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
    Room,
    RoomMode,
    GoTo,
    Karate,
    KarateOpponent,
    Race,
    MyChao,
    Visits,
    /// Visitor 1 or 2: which Chao visits (Auto, a save slot, or None).
    Visitor(usize),
    StartStop,
    Apply,
    StartNow,
}

/// Rows shown right now: "Start now" first while held at a start, then the connection and
/// play settings, then actions, and connecting/stopping at the bottom. `mode` (index into
/// `MODES`) picks the address row: Host address only when joining, Port only when hosting.
fn rows(mode: usize, visits: usize) -> Vec<Row> {
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
    rows.extend([Row::NameTags, Row::Skin, Row::Room]);
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
    rows.extend([Row::MyChao, Row::Visits]);
    if visits != 0 {
        rows.extend([Row::Visitor(0), Row::Visitor(1)]);
    }
    rows.extend([Row::Race, Row::Karate]);
    if online::status().is_some() {
        rows.extend([Row::Apply, Row::StartStop]);
    } else {
        rows.push(Row::StartStop);
    }
    rows
}
const MAX_NAME: usize = crate::net::protocol::MAX_NAME;
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
    /// Visit friends: 0 off, 1 on (index into `online::VISIT_MODES`).
    pub visits: usize,
    /// Visitor 1 and 2: a save slot, `online::VISITOR_AUTO` or `online::VISITOR_NONE`.
    pub visitors: [i32; 2],
    /// Mode of rooms we create.
    pub room_mode: room::Mode,
    /// Our skin's id (`skins::list`), empty for none.
    pub skin: String,
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
            visits: 1,
            visitors: [online::VISITOR_AUTO; 2],
            room_mode: room::Mode::Race,
            skin: String::new(),
        };
        for line in text.lines() {
            let line = line.split([';', '#']).next().unwrap_or("").trim();
            let Some((k, v)) = line.split_once('=') else {
                continue;
            };
            let v = v.trim().to_string();
            match k.trim().to_ascii_lowercase().as_str() {
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
                "visitwith" => s.visits = online::parse_visits(&v),
                "visitor1" => s.visitors[0] = online::parse_visitor(&v),
                "visitor2" => s.visitors[1] = online::parse_visitor(&v),
                "roommode" => s.room_mode = room::Mode::parse(&v),
                "skin" => s.skin = v.to_ascii_lowercase(),
                "chaorace" => {
                    s.chao_race = !matches!(
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
            "[Multiplayer]\nMode={}\nName={}\nHost={}\nPort={}\nNameTags={}\nChaoRace={}\nMyChao={}\nVisitWith={}\nVisitor1={}\nVisitor2={}\nRoomMode={}\nSkin={}\n",
            MODES[self.mode],
            self.name,
            self.host,
            self.port,
            if self.name_tags { "On" } else { "Off" },
            if self.chao_race { "On" } else { "Off" },
            self.my_chao,
            ["Off", "On"][self.visits.min(1)],
            self.visitors[0],
            self.visitors[1],
            self.room_mode.ini(),
            self.skin,
        )
    }
}

fn load(mod_path: &str) -> Settings {
    let dir = Path::new(mod_path);
    ["config.ini", "online.ini"]
        .iter()
        .find_map(|f| fs::read_to_string(dir.join(f)).ok())
        .map(|t| Settings::parse(&t))
        .unwrap_or_else(|| Settings::parse(""))
}

struct Menu {
    open: bool,
    row: usize,
    /// Which room choice the Room row shows.
    room_choice: usize,
    /// Which player the Go to row shows.
    goto_choice: usize,
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
}

static MENU: Mutex<Menu> = Mutex::new(Menu {
    open: false,
    row: 0,
    room_choice: 0,
    goto_choice: 0,
    settings: Settings {
        mode: 0,
        name: String::new(),
        host: String::new(),
        port: String::new(),
        name_tags: true,
        chao_race: true,
        my_chao: -1,
        visits: 1,
        visitors: [-1; 2],
        room_mode: crate::room::Mode::Race,
        skin: String::new(),
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
        m.settings = load(online::mod_path());
        m.row = 0;
        m.message.clear();
        m.message.clear();
        // Swallow keys already held so the opening press doesn't type.
        for key in KEYS_TO_PRIME {
            let _ = m.pressed(key);
        }
    }
}

const KEYS_TO_PRIME: [i32; 8] = [
    VK_RETURN, VK_ESCAPE, VK_UP, VK_DOWN, VK_LEFT, VK_RIGHT, VK_BACK, VK_SPACE,
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
    if !m.open {
        return;
    }
    if m.pressed(VK_ESCAPE) {
        m.open = false;
        return;
    }
    let rows = rows(m.settings.mode, m.settings.visits);
    m.row = m.row.min(rows.len() - 1);
    if m.pressed(VK_UP) {
        m.row = (m.row + rows.len() - 1) % rows.len();
    }
    if m.pressed(VK_DOWN) {
        m.row = (m.row + 1) % rows.len();
    }
    let row = rows[m.row];
    let (left, right) = (m.pressed(VK_LEFT), m.pressed(VK_RIGHT));
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
            // Auto, then each hatched Chao the other row hasn't picked, then None.
            let other = m.settings.visitors[1 - i];
            let mut options = vec![online::VISITOR_AUTO];
            options.extend(
                online::hatched_chao()
                    .iter()
                    .map(|(slot, _)| *slot as i32)
                    .filter(|&slot| slot != other),
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
        Row::Visits if left || right => {
            let n = online::VISIT_MODES.len();
            m.settings.visits = (m.settings.visits + if left { n - 1 } else { 1 }) % n;
            online::set_visits(m.settings.visits);
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
        let message = match row {
            Row::Room => room::choices()
                .get(m.room_choice)
                .map(|(choice, _)| room::choose(*choice)),
            Row::StartStop => Some(start_stop(&mut m)),
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
    let backspace = m.pressed(VK_BACK);
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
    format!(
        "{} ({} garden, slot {})",
        karate::chao_name(chao),
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
            let picked = online::hatched_chao()
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

fn draw(m: &Menu, rows: &[Row]) {
    let status = online::status();
    let on_off = |b: bool| if b { "On" } else { "Off" };
    let mut lines: Vec<(String, i32)> = vec![("== SA2 Chao Online ==".into(), COLOR_TITLE)];
    for (i, row) in rows.iter().enumerate() {
        let text = match row {
            Row::Mode => format!("Multiplayer:  < {} >", MODES[m.settings.mode]),
            Row::Name => format!("Your name:    {}", m.settings.name),
            Row::Host => format!("Host address: {}", m.settings.host),
            Row::Port => format!("Port:         {}", m.settings.port),
            Row::RoomInfo => format!(
                "  {}",
                room::summary().unwrap_or_else(|| "Not in a room".into())
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
                    Some((_, label)) => format!("Room:         < {label} >  (Enter)"),
                    None => "Room:         (connect first)".into(),
                }
            }
            Row::Visitor(i) => format!(
                "  Visitor {}: < {} >",
                i + 1,
                visitor_label(*i, m.settings.visitors[*i])
            ),
            Row::MyChao => format!(
                "My Chao (races, karate): < {} >",
                my_chao_label(m.settings.my_chao)
            ),
            Row::Visits => format!(
                "Visit friends: < {} >",
                online::VISIT_MODES[m.settings.visits.min(1)]
            ),
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
            Row::Apply => "[ Apply settings & reconnect ]".into(),
            Row::StartStop if status.is_some() => "[ Stop multiplayer ]".into(),
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
    lines.push((
        format!("Status: {}", status.as_deref().unwrap_or("off")),
        COLOR_TEXT,
    ));
    lines.push((
        "Up/Down select, Left/Right change, type to edit, Enter = do it".into(),
        COLOR_HINT,
    ));
    lines.push((
        "Esc/F9 close. Pause first when typing: letters also move you.".into(),
        COLOR_HINT,
    ));

    // Everything on whole cells of the font size, so the panel and the text line up.
    let size = hud::menu_font();
    let (w, _) = hud::screen_size();
    let cols = lines
        .iter()
        .map(|(t, _)| t.chars().count())
        .max()
        .unwrap_or(0)
        + 2;
    let x = ((w / size / 2.0).floor() - (cols / 2) as f32).max(0.0) * size;
    let y = 3.0 * size;
    hud::panel(x, y, size, cols, lines.len() + 2);
    for (i, (text, color)) in lines.iter().enumerate() {
        hud::text(x + size, y + (i + 1) as f32 * size, size, *color, text);
    }
}

#[cfg(test)]
mod tests {
    use super::{Settings, typed};

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
            visits: 1,
            visitors: [3, -2],
            room_mode: crate::room::Mode::Coop,
            skin: "race_car".into(),
        };
        assert_eq!(Settings::parse(&s.to_ini()), s);
        // The networking side reads the same file.
        let c = crate::online::parse_config(&s.to_ini()).unwrap();
        assert_eq!(c.name, "Regrant");
        assert_eq!(c.room_mode, crate::room::Mode::Coop);
    }

    #[test]
    fn reads_old_online_ini() {
        let s = Settings::parse("mode = client\nname = Bob\nhost = 1.2.3.4:5\n");
        assert_eq!(
            (s.mode, s.name.as_str(), s.host.as_str()),
            (2, "Bob", "1.2.3.4:5")
        );
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
