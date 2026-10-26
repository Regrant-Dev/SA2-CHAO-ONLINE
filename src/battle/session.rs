//! One battle from the creature battle menu to the result: picking singles or doubles, your team and the
//! opponents (or a trainer from the source game, `trainers`, a friend online, `online`, or a wild
//! visitor from a garden, `wild`), or a multi battle (each of the four places a friend's or the
//! computer's, the player choosing only for their own), then the turns (your choices from
//! menus, the opponents' from the AI or the friends' games) played out beat by beat, and the XP
//! the player's Chao earn from trainer, wild and live battles (`levels`).
//!
//! No game access here: `update` takes the buttons pressed this frame and returns `Command`s for
//! the arena (game.rs), and `screen` says what the HUD shows. A live battle's messages come in
//! through `network` and go out through `take_outgoing`. The menus' lists take the keyboard
//! (`keyboard`): typing searches the long ones by name, and Left/Right or PgUp/PgDn jump
//! (`crate::search`).

use std::collections::{BTreeMap, VecDeque};
use std::sync::Arc;

use crate::chao::ChaoData;
use crate::creature_link::Profile;
use crate::creature_link::store::{MoveRefused, Moves, TraitRefused, Traits};
use crate::log;
use crate::net::protocol::{
    ANSWER_ACCEPT, ANSWER_BUSY, ANSWER_DECLINE, BattleMember, BattlePick, Body, END_BUSY,
    END_CATALOG_DIFFERS, END_DECLINED, END_DESYNC, END_DISCONNECT, END_FORFEIT, END_REPLACED,
    END_TIMEOUT, END_WITHDRAWN, LEAVE_GAVE_UP, LEAVE_LEFT, LEAVE_OVER, MAX_MULTI_TEAM, MultiSeat,
    SEAT_ACCEPTED, SEAT_CATALOG_DIFFERS, SEAT_DECLINED, SEAT_LEFT, SEAT_TIMEOUT,
};
use crate::search::{self, Edit, Search, Typed};

use super::ai;
use super::data::{MoveData, Target};
use super::effects;
use super::engine::{Action, Battle, Choice, Format, Phase};
use super::event::{Event, Outcome, Slot};
use super::fighter::{Fighter, MoveSlot};
use super::ladder::Progress;
use super::levels::{self, Level};
use super::mirror;
use super::narrate::{self, Beat, Cue, Names, Sound, View};
use super::online::{self, Incoming, Invite, MultiStart};
use super::rng::Rng;
use super::team::{self, Dex};
use super::trainers::{self, Creature, Picker, Roster};
use super::wild::Ending;

/// Menu buttons (the game's `MenuButtons_Pressed` bits; the keyboard and the stick count).
/// Start isn't one of ours: it opens the game's pause menu.
pub mod buttons {
    pub const B: u32 = 0x02;
    pub const A: u32 = 0x04;
    pub const UP: u32 = 0x10;
    pub const DOWN: u32 = 0x20;
    pub const LEFT: u32 = 0x40;
    pub const RIGHT: u32 = 0x80;
    /// Not the game's: the keyboard's PgUp and PgDn (`Session::keyboard`).
    pub const PAGE_UP: u32 = 1 << 30;
    pub const PAGE_DOWN: u32 = 1 << 31;
    /// What moves a list's cursor.
    pub const MOVES: u32 = UP | DOWN | LEFT | RIGHT | PAGE_UP | PAGE_DOWN;
}

/// Most Chao on a team.
pub const TEAM_SIZE: usize = 6;
/// A beat with a message can be skipped with A after this many frames.
const SKIP_AFTER: u16 = 10;
/// How long "<Chao> grew to Lv N!" shows after a battle.
const LEVEL_UP_FRAMES: u16 = 90;
/// The result screen ignores A and B for this many frames, so presses that skip the last
/// messages don't close it at once.
const RESULT_WAIT: u16 = 30;
/// A trainer's challenge shows this long before the battle (skippable).
const INTRO_FRAMES: u16 = 90;
/// A live battle's end reason for a lost connection to the host (not on the wire).
const END_LOST: u8 = 0xFF;
/// A challenge or answer the host says nothing more about is given up after this many frames
/// (the host's own limit is shorter).
const WAIT_LIMIT: u32 = 150 * 60;
/// The same for a multi battle's invites (the host calls one off after 5 minutes).
const MULTI_WAIT_LIMIT: u32 = 330 * 60;
/// The setup screen of a multi battle: the team, the three other places, the level rule, then
/// starting it.
const PLAN_ROWS: usize = 6;
const PLAN_START: usize = 5;

/// A Chao to battle with: a copy of its record (the save is never touched), its name and, for
/// the player's own, every move, type and ability it got (`None` for other players' Chao: their
/// game's).
#[derive(Clone)]
pub struct Entry {
    pub name: String,
    pub chao: ChaoData,
    pub moves: Option<Moves>,
    pub traits: Option<Traits>,
}

/// A ready-made opponent team: a friend's Chao, the imported ones.
#[derive(Clone)]
pub struct Lineup {
    pub label: String,
    pub members: Vec<Entry>,
}

/// A friend online in a loaded Chao garden, who can be challenged to a live battle.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Friend {
    pub id: u8,
    pub name: String,
    /// In a battle or a challenge already.
    pub busy: bool,
}

/// One team in the "Battle against" list.
enum Against {
    /// `friends[i]`, live.
    Live(usize),
    /// Not a team: switches live battles between "Level 50" and the Chao's actual levels.
    LevelRule,
    Lineup(usize),
    /// Copies of these of the player's Chao.
    Copies(Vec<usize>),
}

/// Who has one of a multi battle's other places (ally, foe 1, foe 2) as its starter sets it up.
#[derive(Clone)]
enum Holder {
    /// A friend online in a Chao garden, invited.
    Friend { id: u8, name: String },
    /// The computer, with this team.
    Computer { label: String, team: Vec<Entry> },
}

impl Holder {
    fn label(&self) -> String {
        match self {
            Holder::Friend { name, .. } => format!("{name} (live)"),
            Holder::Computer { label, team } => format!("Computer: {label} ({})", team.len()),
        }
    }

    fn name(&self) -> String {
        match self {
            Holder::Friend { name, .. } => name.clone(),
            Holder::Computer { .. } => "the computer".into(),
        }
    }
}

/// An invited place's answer so far.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Answer {
    Waiting,
    Accepted,
    /// Not taken (`SEAT_*`): the starter can give it to the computer.
    Refused(u8),
    /// The computer's.
    Computer,
}

/// A multi battle as its starter sets it up: their own team (up to `MAX_MULTI_TEAM`), then who
/// has each other place.
#[derive(Clone, Default)]
struct Plan {
    picked: Vec<usize>,
    /// The ally's place, then the foes' (places 1..=3); set once the team is picked.
    seats: Option<[Holder; 3]>,
    /// Once the invites are out: each of those places' answer.
    answers: Option<[Answer; 3]>,
    /// Frames waited for the answers.
    waited: u32,
}

impl Plan {
    fn seat(&self, place: usize) -> Option<&Holder> {
        self.seats.as_ref()?.get(place.checked_sub(1)?)
    }

    fn friends(&self) -> bool {
        self.seats
            .iter()
            .flatten()
            .any(|h| matches!(h, Holder::Friend { .. }))
    }
}

const PLACE_NAMES: [&str; 3] = ["Ally", "Foe 1", "Foe 2"];

/// "Can't battle now" to player `to`'s challenge.
pub fn busy_answer(to: u8) -> Body {
    answer(to, ANSWER_BUSY)
}

/// Leaving live battle `battle` that nobody here plays any more.
pub fn left(battle: u16) -> Body {
    leave(battle, LEAVE_LEFT)
}

fn answer(to: u8, answer: u8) -> Body {
    Body::BattleAnswer {
        to,
        answer,
        dex: 0,
        team: Vec::new(),
    }
}

fn leave(battle: u16, reason: u8) -> Body {
    Body::BattleLeave { battle, reason }
}

/// Whether `event` ends a challenge with player `with` before its battle starts, and why.
fn ended(event: &Incoming, with: u8) -> Option<u8> {
    match event {
        Incoming::End {
            battle: 0, reason, ..
        } => Some(*reason),
        Incoming::Left(id) if *id == with => Some(END_DISCONNECT),
        Incoming::Lost => Some(END_LOST),
        _ => None,
    }
}

/// What the end of a multi battle's invite to us (`END_*`, or `END_LOST`) says.
fn invite_note(reason: u8, name: &str) -> String {
    match reason {
        END_WITHDRAWN => format!("{name} called the multi battle off."),
        END_DISCONNECT => format!("{name} left."),
        END_TIMEOUT => "Time ran out waiting for the multi battle.".into(),
        END_BUSY => "The multi battle couldn't take that team.".into(),
        _ => end_note(reason, name),
    }
}

/// What the host said about our invite to a multi battle's place (`SEAT_*`).
fn seat_note(status: u8, name: &str) -> String {
    match status {
        SEAT_ACCEPTED => format!("{name} accepted."),
        SEAT_DECLINED => format!("{name} turned the battle down."),
        SEAT_TIMEOUT => format!("{name} didn't answer in time."),
        SEAT_LEFT => format!("{name} left."),
        SEAT_CATALOG_DIFFERS => format!("{name}'s creature link comes from a different game file."),
        _ => format!("{name} can't battle right now."),
    }
}

/// Why an invited place wasn't taken (`SEAT_*`), for the waiting list.
fn refused(status: u8) -> &'static str {
    match status {
        SEAT_DECLINED => "turned it down",
        SEAT_TIMEOUT => "didn't answer",
        SEAT_LEFT => "left",
        SEAT_CATALOG_DIFFERS => "has a creature link from another game file",
        _ => "can't battle now",
    }
}

/// "A and B".
fn pair(a: &str, b: &str) -> String {
    format!("{a} and {b}")
}

/// What a live battle's end (`END_*`, or `END_LOST`) says.
fn end_note(reason: u8, name: &str) -> String {
    match reason {
        END_BUSY => format!("{name} can't battle right now."),
        END_DECLINED => format!("{name} turned the battle down."),
        END_CATALOG_DIFFERS => {
            format!(
                "{name}'s creature link comes from a different game file: live battles need the same one."
            )
        }
        END_FORFEIT => format!("{name} gave up."),
        END_DISCONNECT => format!("{name} left the battle."),
        END_TIMEOUT => "Time ran out waiting for an answer.".into(),
        END_DESYNC => "The two games' battles went apart, so it doesn't count.".into(),
        END_WITHDRAWN => format!("{name} took the challenge back."),
        END_LOST => "The connection to the host was lost.".into(),
        _ => "The battle ended.".into(),
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Command {
    Cue(Cue),
    /// Leave the creature battle menu for the arena.
    OpenArena(Format),
    /// The battle is over: back to the creature battle menu.
    CloseArena,
    /// Closed before a battle started.
    Closed,
    /// A challenge that opened the menu by itself ended unanswered or turned down: the menu
    /// closes and the mode select goes on as before it (`Session::take_farewell` says why).
    Released,
    /// The trainer ladder's progress changed (`Session::progress`): a win.
    Progress,
    /// The player's Chao earned battle XP (`Session::levels`).
    Levels,
    /// The moves of the player's Chao with this identity changed (`Session::own_moves`): a
    /// milestone move taught or the four it battles with chosen.
    Moves(u64),
    /// The types or ability the player's Chao with this identity battles with were chosen
    /// (`Session::own_traits`).
    Traits(u64),
}

/// One fighter's box on the HUD.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FighterLine {
    pub slot: Slot,
    pub member: usize,
    pub name: String,
    pub hp: u16,
    pub max_hp: u16,
    pub ailment: Option<&'static str>,
    /// The level it battles at.
    pub level: u8,
    /// For the player's own Chao: how far its battle level is toward the next, in thousandths.
    pub xp: Option<u16>,
}

/// The XP one of the player's Chao earned from a battle.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Gain {
    pub name: String,
    pub identity: u64,
    pub xp: u32,
    /// Its level after it, and whether it went up.
    pub level: u8,
    pub grew: bool,
}

/// What the HUD shows now.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Screen {
    pub title: String,
    pub items: Vec<String>,
    /// Items that can't be picked now (drawn dim).
    pub disabled: Vec<usize>,
    pub cursor: Option<usize>,
    pub message: Option<String>,
    /// More about the item under the cursor: a move's numbers and description, a Chao's types
    /// and moves.
    pub detail: Option<String>,
    pub fighters: Vec<FighterLine>,
    /// The player's fighter whose choice it is.
    pub choosing: Option<Slot>,
    /// The fighter the target menu's cursor is on.
    pub aimed: Option<Slot>,
    pub hint: &'static str,
    /// XP bars beside items: (item, thousandths of the way to the next battle level).
    pub bars: Vec<(usize, u16)>,
    /// A list without a cursor: the first of its items shown (`VISIBLE_ITEMS` at once).
    pub scroll: usize,
    /// A list typing searches: what's typed (empty for nothing yet). Its items are the ones
    /// that match.
    pub search: Option<String>,
}

/// Menu items shown at once (longer lists scroll).
pub const VISIBLE_ITEMS: usize = 7;

/// A Chao's profile: of the moves, types and ability given (the player's own Chao's), else of
/// the ones it's known by.
pub type ProfileOf = Box<dyn Fn(&ChaoData, Option<&Moves>, Option<&Traits>) -> Profile + Send>;

pub struct Session {
    dex: Arc<Dex>,
    /// `online::dex_print` of `dex`, for live battles.
    dex_print: u32,
    own: Vec<Entry>,
    /// `own`'s types, ability, moves and levels, for the picker.
    own_profiles: Vec<Profile>,
    lineups: Vec<Lineup>,
    profile_of: ProfileOf,
    seed: u64,
    state: State,
    notice: Option<String>,
    /// The source game's trainers ("Trainers" in the format menu), when on.
    trainers: Option<Arc<Roster>>,
    progress: Progress,
    /// Friends to challenge to a live battle (`set_friends`).
    friends: Vec<Friend>,
    /// Live battle messages for the host (`take_outgoing`).
    outbox: Vec<Body>,
    /// Commands from `network`, returned by the next `update`.
    queued: Vec<Command>,
    /// The live battle the host has us in, kept outside `state` so leaving still tells the
    /// host after a battle stopped part-way.
    live_battle: Option<u16>,
    /// A friend's challenge opened this menu (`set_opened_by_offer`).
    offer_opened: bool,
    /// Our challenges' level rule: each Chao at its own level (else all at level 50).
    actual_levels: bool,
    /// The level rule of the challenge being answered.
    offer_actual_levels: bool,
    /// The challenge being answered is an invite to a multi battle.
    offer_invite: Option<Invite>,
    /// The player's Chao's battle levels, by identity (the ones not here are at a new Chao's).
    levels: BTreeMap<u64, Level>,
    /// The notice to show as the menu closes after `Command::Released`.
    farewell: Option<String>,
    /// A battle with a wild visitor (`with_wild`): the menu is only its team screen and fight.
    wild: Option<Wild>,
    /// What's typed to search the list showing (cleared when another list shows).
    search: Search,
}

/// The wild visitor a session battles, and how it ended for it (once decided).
struct Wild {
    creature: Creature,
    shiny: bool,
    ending: Option<Ending>,
}

/// Frames a challenge shows before buttons answer it (a press already on its way doesn't).
const OFFER_GRACE: u16 = 15;

/// A live battle's challenge on its way: sent and waiting for the answer (`challenger`), or
/// accepted and waiting for the battle.
struct Waiting {
    with: u8,
    name: String,
    format: Format,
    picked: Vec<usize>,
    team: Vec<BattleMember>,
    challenger: bool,
    /// The challenge's level rule.
    actual_levels: bool,
    /// Frames waited.
    waited: u32,
    /// Accepted an invite to a multi battle (from `with`, its starter).
    multi: bool,
}

enum State {
    Format {
        cursor: usize,
    },
    Trainers(Picker),
    Team {
        format: Format,
        cursor: usize,
        picked: Vec<usize>,
        /// Against this trainer (no opponent menu).
        foe: Option<u16>,
        /// Answering this friend's challenge (id, name).
        live: Option<(u8, String)>,
        /// Choosing a Chao's moves ("Choose moves").
        moves: Option<Choosing>,
        /// Picking the team for a multi battle being set up.
        multi: Option<Box<Plan>>,
    },
    /// Setting up a multi battle (`PLAN_ROWS`).
    Multi {
        plan: Box<Plan>,
        cursor: usize,
    },
    /// Choosing who has place `place` (1..=3) of the multi battle: a friend or a computer team;
    /// only the computer's once the invites are out (a place nobody took).
    Seat {
        plan: Box<Plan>,
        place: usize,
        cursor: usize,
    },
    /// The multi battle's invites are out: each place's answer, and calling it off.
    MultiWait {
        plan: Box<Plan>,
        cursor: usize,
    },
    Opponent {
        format: Format,
        picked: Vec<usize>,
        cursor: usize,
    },
    /// A friend's challenge to a live battle: accept or decline.
    Offer {
        from: u8,
        name: String,
        format: Format,
        cursor: usize,
        /// Frames shown.
        age: u16,
    },
    Waiting(Box<Waiting>),
    Fight(Box<Fight>),
    Done,
}

/// Choosing what one of the player's Chao battles with: which Chao (`chao` none), then its
/// moves, types or ability (`section` none), then that list.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Choosing {
    chao: Option<usize>,
    section: Option<Section>,
    cursor: usize,
}

/// A list of "Choose moves, type & ability".
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Section {
    /// Four of every move it learned.
    Moves,
    /// One or two of the types its partners gave it.
    Types,
    /// One of the abilities its partners gave it, or none.
    Ability,
}

impl Section {
    const ALL: [Section; 3] = [Section::Moves, Section::Types, Section::Ability];

    fn row(self) -> usize {
        self as usize
    }
}

/// Moves `cursor` up or down a list of `len` (wrapping); whether it moved.
pub(super) fn step(cursor: &mut usize, len: usize, pressed: u32) -> bool {
    if len == 0 {
        return false;
    }
    let old = *cursor;
    if pressed & buttons::UP != 0 {
        *cursor = (*cursor + len - 1) % len;
    } else if pressed & buttons::DOWN != 0 {
        *cursor = (*cursor + 1) % len;
    }
    *cursor != old
}

fn sound(s: Sound) -> Command {
    Command::Cue(Cue::Sound(s))
}

/// "Fire" or "Fire/Flying".
fn type_names(dex: &Dex, types: [u8; 2]) -> String {
    if types[0] == types[1] {
        dex.type_name(types[0])
    } else {
        format!("{}/{}", dex.type_name(types[0]), dex.type_name(types[1]))
    }
}

/// A Chao for the picker: its battle level and XP, types, ability, moves and battle stats (at
/// the level it battles at, `profile.level`).
fn chao_detail(dex: &Dex, chao: &ChaoData, profile: &Profile, level: Level) -> String {
    let ability = if profile.ability == 0 {
        "no ability".to_string()
    } else {
        dex.ability_name(profile.ability)
    };
    let moves: Vec<String> = profile
        .moves
        .iter()
        .filter(|&&id| dex.move_data(id).is_some())
        .map(|&id| dex.move_name(id))
        .collect();
    let s = team::chao_stats(chao, profile.level).stats;
    let xp = match levels::step(level.level) {
        0 => String::new(),
        step => format!(" (XP {}/{step})", level.xp),
    };
    let at = if profile.level == level.level {
        String::new()
    } else {
        format!(", battling at Lv {}", profile.level)
    };
    format!(
        "Lv {}{xp}{at}. {}, {ability}. Moves: {}. HP {} Atk {} Def {} SpA {} SpD {} Spe {}",
        level.level,
        type_names(dex, profile.types),
        if moves.is_empty() {
            "none".to_string()
        } else {
            moves.join(", ")
        },
        s.hp,
        s.attack,
        s.defense,
        s.sp_attack,
        s.sp_defense,
        s.speed
    )
}

/// A move for the move menu: its kind, power, accuracy, who it hits and the catalog's description.
pub(super) fn move_detail(dex: &Dex, m: &MoveData) -> String {
    let mut parts = vec![
        if m.category == super::data::Category::Status {
            "Status"
        } else if m.category == super::data::Category::Special {
            "Special"
        } else {
            "Physical"
        }
        .to_string(),
    ];
    match m.power {
        0 => {}
        1 => parts.push("power varies".into()),
        p => parts.push(format!("power {p}")),
    }
    parts.push(if m.accuracy == 0 {
        "never misses".into()
    } else {
        format!("accuracy {}", m.accuracy)
    });
    match m.target {
        Target::BothFoes => parts.push("hits both foes".into()),
        Target::AllOthers => parts.push("hits everyone else".into()),
        _ => {}
    }
    let about = dex.move_description(m.id).trim();
    if about.is_empty() {
        format!("{}.", parts.join(", "))
    } else {
        format!("{}. {about}", parts.join(", "))
    }
}

impl Session {
    /// The picker, with the player's own Chao (`own`, copies) and the ready-made opponent teams.
    /// `profile_of` gives a Chao's types, ability and moves; `seed` decides the battle's rolls.
    pub fn open(
        dex: Arc<Dex>,
        own: Vec<Entry>,
        lineups: Vec<Lineup>,
        profile_of: ProfileOf,
        seed: u64,
    ) -> Session {
        let own_profiles = own
            .iter()
            .map(|e| profile_of(&e.chao, e.moves.as_ref(), e.traits.as_ref()))
            .collect();
        Session {
            dex_print: online::dex_print(&dex),
            dex,
            own,
            own_profiles,
            lineups,
            profile_of,
            seed,
            state: State::Format { cursor: 0 },
            notice: None,
            trainers: None,
            progress: Progress::default(),
            friends: Vec::new(),
            outbox: Vec::new(),
            queued: Vec::new(),
            live_battle: None,
            offer_opened: false,
            actual_levels: false,
            offer_actual_levels: false,
            offer_invite: None,
            levels: BTreeMap::new(),
            farewell: None,
            wild: None,
            search: Search::default(),
        }
    }

    /// The menu for a battle with the wild visitor `creature` (`shiny`: in its shiny colours):
    /// it opens at the team screen for singles against it; B there closes it unfought.
    pub fn with_wild(mut self, creature: Creature, shiny: bool) -> Session {
        self.wild = Some(Wild {
            creature,
            shiny,
            ending: None,
        });
        self.state = State::Team {
            format: Format::Singles,
            cursor: 0,
            picked: Vec::new(),
            foe: None,
            live: None,
            moves: None,
            multi: None,
        };
        self
    }

    /// A battle with a wild visitor.
    pub fn is_wild(&self) -> bool {
        self.wild.is_some()
    }

    /// How the battle with the wild visitor ended for it, once decided.
    pub fn wild_ending(&self) -> Option<Ending> {
        self.wild.as_ref()?.ending
    }

    /// A friend's challenge opened this menu: turning it down (or its end) closes the menu.
    pub fn set_opened_by_offer(&mut self) {
        self.offer_opened = true;
    }

    /// The notice to show as the menu closes after `Command::Released`.
    pub fn take_farewell(&mut self) -> Option<String> {
        self.farewell.take()
    }

    /// The challenge ended without a battle: back to the format menu, or out of the menu when
    /// the challenge opened it.
    fn offer_over(&mut self, out: &mut Vec<Command>) -> State {
        if self.offer_opened {
            self.farewell = self.notice.take();
            out.push(Command::Released);
        }
        State::Format { cursor: 0 }
    }

    /// Notes the live battle `state` is in (an open link), if any.
    fn note_live(&mut self) {
        self.live_battle = match &self.state {
            State::Fight(f) => f.link.as_ref().filter(|l| l.open()).map(|l| l.battle),
            _ => None,
        };
    }

    /// The friends to challenge to a live battle now (none while live battles are off).
    pub fn set_friends(&mut self, friends: Vec<Friend>) {
        self.friends = if online::enabled() {
            friends
        } else {
            Vec::new()
        };
    }

    /// Live battle messages for the host.
    pub fn take_outgoing(&mut self) -> Vec<Body> {
        let mut out = std::mem::take(&mut self.outbox);
        if let State::Fight(f) = &mut self.state
            && let Some(link) = &mut f.link
        {
            out.append(&mut link.outbox);
        }
        out
    }

    /// In a battle or a live battle's challenge, or battling a wild visitor (others see us
    /// busy).
    pub fn busy(&self) -> bool {
        self.wild.is_some()
            || matches!(
                self.state,
                State::Offer { .. } | State::Waiting(_) | State::Fight(_) | State::MultiWait { .. }
            )
            || matches!(&self.state, State::Team { live: Some(_), .. })
            || matches!(&self.state, State::Seat { plan, .. } if plan.answers.is_some())
    }

    /// Waiting on the host: a challenge, an answer or a live battle still going.
    pub fn waits_on_host(&self) -> bool {
        match &self.state {
            State::Offer { .. } | State::Waiting(_) | State::MultiWait { .. } => true,
            State::Seat { plan, .. } => plan.answers.is_some(),
            State::Team { live, .. } => live.is_some(),
            State::Fight(f) => f.link.as_ref().is_some_and(|l| l.open()),
            _ => false,
        }
    }

    /// The session closes before its end (the player left the creature battle scene or the menu went):
    /// a challenge is taken back or turned down, a live battle left (and lost).
    pub fn abandon(&mut self) {
        let live = self.live_battle.take();
        match std::mem::replace(&mut self.state, State::Done) {
            State::Offer { from, .. }
            | State::Team {
                live: Some((from, _)),
                ..
            } => {
                self.outbox.push(answer(from, ANSWER_DECLINE));
            }
            State::Waiting(_) | State::MultiWait { .. } => self.outbox.push(Body::BattleLeave {
                battle: 0,
                reason: LEAVE_LEFT,
            }),
            State::Seat { plan, .. } if plan.answers.is_some() => {
                self.outbox.push(leave(0, LEAVE_LEFT));
            }
            State::Fight(mut f) => {
                f.abandon();
                if let Some(link) = &mut f.link {
                    self.outbox.append(&mut link.outbox);
                }
            }
            // A live battle that stopped part-way (a panic left no fight): the host is told.
            _ => {
                if let Some(battle) = live {
                    self.outbox.push(leave(battle, LEAVE_LEFT));
                }
            }
        }
    }

    /// A message from the host about a live battle.
    pub fn network(&mut self, event: Incoming) {
        if self.wild.is_some() && !matches!(self.state, State::Fight(_)) {
            // Battling a wild visitor: no challenge is taken.
            match event {
                Incoming::Offer { from, .. } => self.outbox.push(answer(from, ANSWER_BUSY)),
                Incoming::Start(start) => self.outbox.push(leave(start.battle, LEAVE_LEFT)),
                Incoming::MultiStart(start) => self.outbox.push(leave(start.battle, LEAVE_LEFT)),
                _ => {}
            }
            return;
        }
        let state = std::mem::replace(&mut self.state, State::Done);
        self.state = match (state, event) {
            (State::Fight(mut f), event) => {
                if let Incoming::Offer { from, .. } = event {
                    self.outbox.push(answer(from, ANSWER_BUSY));
                } else if let Incoming::Start(start) = event {
                    self.outbox.push(leave(start.battle, LEAVE_LEFT));
                } else if let Incoming::MultiStart(start) = event {
                    self.outbox.push(leave(start.battle, LEAVE_LEFT));
                } else {
                    f.network(event, &mut self.queued);
                }
                State::Fight(f)
            }
            (
                state @ (State::Format { .. }
                | State::Trainers(_)
                | State::Opponent { .. }
                | State::Multi { .. }
                | State::Seat { .. }
                | State::Team { live: None, .. }),
                Incoming::Offer {
                    from,
                    name,
                    format,
                    actual_levels,
                    multi,
                },
            ) if online::enabled()
                && !matches!(&state, State::Seat { plan, .. } if plan.answers.is_some()) =>
            {
                log!(
                    "battle: {name} challenges us ({format:?}, actual levels {actual_levels}, \
                     multi {multi:?})"
                );
                drop(state);
                self.offer_actual_levels = actual_levels;
                self.offer_invite = multi;
                self.queued.push(Command::Cue(Cue::Sound(Sound::Confirm)));
                State::Offer {
                    from,
                    name,
                    format,
                    cursor: 0,
                    age: 0,
                }
            }
            (state, Incoming::Offer { from, .. }) => {
                self.outbox.push(answer(from, ANSWER_BUSY));
                state
            }
            (State::MultiWait { plan, cursor }, event) => {
                self.plan_event(plan, event, |plan| State::MultiWait { plan, cursor })
            }
            (
                State::Seat {
                    plan,
                    place,
                    cursor,
                },
                event,
            ) if plan.answers.is_some() => self.plan_event(plan, event, |plan| State::Seat {
                plan,
                place,
                cursor,
            }),
            (State::Waiting(w), Incoming::Start(start)) if start.opponent == w.with && !w.multi => {
                self.start_live(*w, start)
            }
            (State::Waiting(w), Incoming::MultiStart(start)) if w.multi => {
                self.start_multi(start, &w.picked)
            }
            (state, Incoming::Start(start)) => {
                // A battle nobody here waits for any more.
                self.outbox.push(leave(start.battle, LEAVE_LEFT));
                state
            }
            (state, Incoming::MultiStart(start)) => {
                self.outbox.push(leave(start.battle, LEAVE_LEFT));
                state
            }
            (State::Waiting(w), event) => match ended(&event, w.with) {
                Some(reason) => {
                    self.notice = Some(if w.multi {
                        invite_note(reason, &w.name)
                    } else {
                        end_note(reason, &w.name)
                    });
                    if w.challenger {
                        State::Opponent {
                            format: w.format,
                            picked: w.picked,
                            cursor: 0,
                        }
                    } else {
                        State::Format { cursor: 0 }
                    }
                }
                None => State::Waiting(w),
            },
            (
                State::Offer { from, name, .. }
                | State::Team {
                    live: Some((from, name)),
                    ..
                },
                event,
            ) if ended(&event, from).is_some() => {
                let reason = ended(&event, from).unwrap_or(END_DISCONNECT);
                self.notice = Some(if self.offer_invite.is_some() {
                    invite_note(reason, &name)
                } else {
                    end_note(reason, &name)
                });
                let mut out = Vec::new();
                let state = self.offer_over(&mut out);
                self.queued.append(&mut out);
                state
            }
            (state, _) => state,
        };
        self.note_live();
    }

    /// A message from the host while our multi battle's invites are out; `stay` is the state
    /// to go on in.
    fn plan_event(
        &mut self,
        mut plan: Box<Plan>,
        event: Incoming,
        stay: impl FnOnce(Box<Plan>) -> State,
    ) -> State {
        match event {
            Incoming::Status { place, status } if (1..=3).contains(&place) => {
                let name = plan.seat(place).map_or_else(String::new, Holder::name);
                if let Some(answers) = plan.answers.as_mut()
                    && answers[place - 1] != Answer::Computer
                {
                    answers[place - 1] = if status == SEAT_ACCEPTED {
                        Answer::Accepted
                    } else {
                        self.notice = Some(seat_note(status, &name));
                        Answer::Refused(status)
                    };
                    self.queued.push(sound(Sound::Cursor));
                }
                stay(plan)
            }
            Incoming::MultiStart(start) => self.start_multi(start, &plan.picked),
            Incoming::Start(start) => {
                self.outbox.push(leave(start.battle, LEAVE_LEFT));
                stay(plan)
            }
            Incoming::End {
                battle: 0, reason, ..
            } => {
                self.notice = Some(match reason {
                    END_TIMEOUT => "Time ran out waiting for the multi battle.".into(),
                    END_LOST => end_note(END_LOST, ""),
                    _ => "The multi battle can't start right now.".into(),
                });
                plan.answers = None;
                State::Multi {
                    plan,
                    cursor: PLAN_START,
                }
            }
            Incoming::Lost => {
                self.notice = Some(end_note(END_LOST, ""));
                plan.answers = None;
                State::Multi {
                    plan,
                    cursor: PLAN_START,
                }
            }
            _ => stay(plan),
        }
    }

    /// The multi battle's setup screen.
    fn plan_screen(&self, plan: &Plan, cursor: usize) -> Screen {
        let seats = plan
            .seats
            .clone()
            .unwrap_or_else(|| self.default_seats(&plan.picked));
        let team: Vec<String> = plan
            .picked
            .iter()
            .filter_map(|&i| Some(self.own.get(i)?.name.clone()))
            .collect();
        let mut items = vec![format!("Your team: {}", team.join(", "))];
        for (name, h) in PLACE_NAMES.iter().zip(&seats) {
            items.push(format!("{name}: {}", h.label()));
        }
        items.push(self.level_rule_label());
        items.push(if plan.friends() {
            "Send the invites".into()
        } else {
            "Start".into()
        });
        let detail = match cursor {
            0 => format!("A: pick your team again (up to {MAX_MULTI_TEAM} Chao)."),
            1..=3 => match &seats[cursor - 1] {
                Holder::Friend { name, .. } => format!(
                    "{name} picks up to {MAX_MULTI_TEAM} of their Chao and chooses for them from \
                     their game. A: change."
                ),
                Holder::Computer { team, .. } => format!(
                    "The computer plays this place with copies of {}. A: change.",
                    team.iter()
                        .map(|e| e.name.clone())
                        .collect::<Vec<_>>()
                        .join(", ")
                ),
            },
            4 => self.opponent_detail(&Against::LevelRule),
            _ => format!(
                "You and {} against {}.{}",
                seats[0].name(),
                pair(&seats[1].name(), &seats[2].name()),
                if plan.friends() {
                    " The battle starts once every friend in it accepts."
                } else {
                    ""
                }
            ),
        };
        Screen {
            title: "Multi battle".into(),
            items,
            cursor: Some(cursor),
            detail: Some(detail),
            hint: "A: choose  B: back",
            ..Screen::default()
        }
    }

    /// Most Chao on the team being picked: `MAX_MULTI_TEAM` for a multi battle.
    fn team_limit(&self, multi: bool) -> usize {
        if multi { MAX_MULTI_TEAM } else { TEAM_SIZE }
    }

    /// The computer's teams for a multi battle's places: copies of the player's other Chao
    /// (of the team itself when there are no others), then each ready-made team, up to
    /// `MAX_MULTI_TEAM` to a team.
    fn computer_teams(&self, picked: &[usize]) -> Vec<Holder> {
        let mut out = Vec::new();
        let mut add = |label: &str, members: Vec<Entry>| {
            let chunks = members.len().div_ceil(MAX_MULTI_TEAM);
            for (i, team) in members.chunks(MAX_MULTI_TEAM).enumerate() {
                out.push(Holder::Computer {
                    label: if chunks > 1 {
                        format!("{label} {}", i + 1)
                    } else {
                        label.to_string()
                    },
                    team: team.to_vec(),
                });
            }
        };
        let others: Vec<Entry> = (0..self.own.len())
            .filter(|i| !picked.contains(i))
            .map(|i| self.own[i].clone())
            .collect();
        if others.is_empty() {
            let team = picked.iter().map(|&i| self.own[i].clone()).collect();
            add("Copies of your team", team);
        } else {
            add("Copies of your other Chao", others);
        }
        for l in &self.lineups {
            add(&l.label, l.members.clone());
        }
        out
    }

    /// Who has the other places of a new multi battle: the computer's first teams.
    fn default_seats(&self, picked: &[usize]) -> [Holder; 3] {
        let teams = self.computer_teams(picked);
        std::array::from_fn(|i| teams[i % teams.len()].clone())
    }

    /// Who can have one of the multi battle's places: the friends in a Chao garden, then the
    /// computer's teams (only those once the invites are out).
    fn seat_options(&self, plan: &Plan) -> Vec<Holder> {
        let mut out: Vec<Holder> = Vec::new();
        if plan.answers.is_none() {
            out.extend(self.friends.iter().map(|f| Holder::Friend {
                id: f.id,
                name: f.name.clone(),
            }));
        }
        out.extend(self.computer_teams(&plan.picked));
        out
    }

    /// Chao as they go to a live battle, at the levels here.
    fn wire_entries(&self, entries: &[Entry]) -> Vec<BattleMember> {
        entries
            .iter()
            .map(|e| online::member(&e.chao, &self.profile(e)))
            .collect()
    }

    /// Sends the multi battle's invites to the friends in it.
    fn invite(&mut self, plan: &mut Plan) {
        let Some(seats) = plan.seats.clone() else {
            return;
        };
        let wire = seats.each_ref().map(|h| match h {
            Holder::Friend { id, .. } => MultiSeat::Player(*id),
            Holder::Computer { team, .. } => MultiSeat::Computer(self.wire_entries(team)),
        });
        let team = self.wire_team(&plan.picked);
        self.outbox.push(Body::MultiInvite {
            seats: wire,
            actual_levels: self.actual_levels,
            dex: self.dex_print,
            team,
        });
        log!(
            "battle: multi battle invites to {}",
            seats
                .iter()
                .map(Holder::name)
                .collect::<Vec<_>>()
                .join(", ")
        );
        plan.answers = Some(seats.each_ref().map(|h| match h {
            Holder::Friend { .. } => Answer::Waiting,
            Holder::Computer { .. } => Answer::Computer,
        }));
        plan.waited = 0;
    }

    /// A multi battle with no friends in it: the player and the computer's three places, here
    /// alone.
    fn local_multi(&self, plan: &Plan) -> Option<Fight> {
        let seats = plan.seats.as_ref()?;
        let team = |h: &Holder| match h {
            Holder::Computer { team, .. } => self.chao_members(team),
            Holder::Friend { .. } => Vec::new(),
        };
        let mine: Vec<Entry> = plan.picked.iter().map(|&i| self.own[i].clone()).collect();
        let mut ours = self.chao_members(&mine);
        let first = ours.len();
        ours.extend(team(&seats[0]));
        let mut theirs = team(&seats[1]);
        let their_first = theirs.len();
        theirs.extend(team(&seats[2]));
        let mut fight = Fight::new(Format::Doubles, [ours, theirs], self.dex.clone(), self.seed)?;
        fight.battle.set_split(0, first);
        fight.battle.set_split(1, their_first);
        fight.mine = Some(0);
        fight.intro = Some(format!(
            "A multi battle: you and {} against {}!",
            seats[0].name(),
            pair(&seats[1].name(), &seats[2].name())
        ));
        self.take_part(&mut fight, &plan.picked);
        Some(fight)
    }

    /// The host started the multi battle we set up or accepted (the `picked` Chao are ours).
    fn start_multi(&mut self, start: MultiStart, picked: &[usize]) -> State {
        let at = online::place_slot(start.place);
        let places_ok = start.seats[start.place].player.is_some()
            && start.seats.iter().all(|s| !s.team.is_empty());
        let members = |side: usize| -> Vec<Member> {
            start.seats[side * 2..side * 2 + 2]
                .iter()
                .flat_map(|s| self.live_members(&s.team, start.actual_levels))
                .collect()
        };
        let teams = [members(0), members(1)];
        let fight = places_ok
            .then(|| {
                Fight::from_side(
                    Format::Doubles,
                    teams,
                    self.dex.clone(),
                    start.seed,
                    at.side,
                )
            })
            .flatten();
        let Some(mut fight) = fight else {
            self.outbox.push(leave(start.battle, LEAVE_LEFT));
            self.notice = Some("That battle can't start.".into());
            return State::Format { cursor: 0 };
        };
        self.live_battle = Some(start.battle);
        for (side, first) in start.splits().into_iter().enumerate() {
            fight.battle.set_split(side, first);
        }
        fight.mine = Some(at.pos);
        fight.own_first = if at.pos == 0 {
            0
        } else {
            start.seats[at.side * 2].team.len()
        };
        let name = |place: usize| -> String {
            let s = &start.seats[place];
            match s.player {
                _ if place == start.place => "you".into(),
                Some(_) => s.name.clone(),
                None => "the computer".into(),
            }
        };
        let (ally, foes) = (at.side * 2 + 1 - at.pos, 2 - at.side * 2);
        fight.intro = Some(format!(
            "A multi battle: you and {} against {}!",
            name(ally),
            pair(&name(foes), &name(foes + 1))
        ));
        fight.foe_levels = start.seats[foes..foes + 2]
            .iter()
            .flat_map(|s| s.team.iter().map(|m| m.level))
            .collect();
        fight.fair = !start.actual_levels;
        self.take_part(&mut fight, picked);
        let others: Vec<(usize, String)> = (0..4)
            .filter(|&p| p != start.place && start.seats[p].player.is_some())
            .map(|p| (p, start.seats[p].name.clone()))
            .collect();
        log!(
            "battle: multi battle {} at place {} with {:?}",
            start.battle,
            start.place,
            others
        );
        fight.link = Some(Link {
            battle: start.battle,
            opponent: crate::net::protocol::SERVER,
            name: others
                .iter()
                .map(|o| o.1.clone())
                .collect::<Vec<_>>()
                .join(", "),
            step: 1,
            ready: None,
            outbox: Vec::new(),
            ended: None,
            closed: false,
            others,
            notes: Vec::new(),
        });
        self.queued.push(Command::Cue(Cue::Sound(Sound::Confirm)));
        State::Fight(Box::new(fight))
    }

    /// A team of the picked Chao as it goes to a live battle.
    fn wire_team(&self, picked: &[usize]) -> Vec<BattleMember> {
        picked
            .iter()
            .filter_map(|&i| {
                let e = self.own.get(i)?;
                Some(online::member(&e.chao, &self.profile(e)))
            })
            .collect()
    }

    /// A live team's fighters under the challenge's level rule.
    fn live_members(&self, team: &[BattleMember], actual_levels: bool) -> Vec<Member> {
        team.iter()
            .map(|m| Member {
                fighter: online::fighter_of(m, &self.dex, actual_levels),
                record: Some((*m.chao).clone()),
                species: None,
                exp_yield: 0,
            })
            .collect()
    }

    /// The identities of the picked Chao, in order.
    fn own_ids(&self, picked: &[usize]) -> Vec<u64> {
        picked
            .iter()
            .filter_map(|&i| Some(self.own.get(i)?.chao.identity()))
            .collect()
    }

    /// A battle against a ready-made team or copies.
    fn local_fight(
        &mut self,
        format: Format,
        picked: Vec<usize>,
        cursor: usize,
        against: Against,
        out: &mut Vec<Command>,
    ) -> State {
        let mine: Vec<Entry> = picked.iter().map(|&i| self.own[i].clone()).collect();
        let theirs: Vec<Entry> = match against {
            Against::Live(_) | Against::LevelRule => Vec::new(),
            Against::Lineup(l) => self.lineups[l]
                .members
                .iter()
                .take(TEAM_SIZE)
                .cloned()
                .collect(),
            Against::Copies(m) => m.iter().map(|&i| self.own[i].clone()).collect(),
        };
        match Fight::new(
            format,
            [self.chao_members(&mine), self.chao_members(&theirs)],
            self.dex.clone(),
            self.seed,
        ) {
            Some(fight) => {
                out.push(sound(Sound::Confirm));
                State::Fight(Box::new(fight))
            }
            None => {
                self.notice = Some("Those teams can't battle.".into());
                State::Opponent {
                    format,
                    picked,
                    cursor,
                }
            }
        }
    }

    /// Challenges `friend` to a live battle with the picked team.
    fn challenge(
        &mut self,
        friend: Friend,
        format: Format,
        picked: Vec<usize>,
        cursor: usize,
        out: &mut Vec<Command>,
    ) -> State {
        if friend.busy {
            self.notice = Some(format!("{} is in another battle.", friend.name));
            return State::Opponent {
                format,
                picked,
                cursor,
            };
        }
        let team = self.wire_team(&picked);
        self.outbox.push(Body::BattleChallenge {
            to: friend.id,
            format: online::format_wire(format),
            actual_levels: self.actual_levels,
            dex: self.dex_print,
            team: team.clone(),
        });
        log!("battle: challenging {} ({format:?})", friend.name);
        out.push(sound(Sound::Confirm));
        State::Waiting(Box::new(Waiting {
            with: friend.id,
            name: friend.name,
            format,
            picked,
            team,
            challenger: true,
            actual_levels: self.actual_levels,
            waited: 0,
            multi: false,
        }))
    }

    /// The host started the live battle we waited for.
    fn start_live(&mut self, w: Waiting, start: online::Start) -> State {
        self.live_battle = Some(start.battle);
        let mine = self.live_members(&w.team, w.actual_levels);
        let theirs = self.live_members(&start.team, w.actual_levels);
        let teams = if start.side == 0 {
            [mine, theirs]
        } else {
            [theirs, mine]
        };
        log!(
            "battle: live battle {} against {} as side {}",
            start.battle,
            w.name,
            start.side
        );
        match Fight::from_side(
            start.format,
            teams,
            self.dex.clone(),
            start.seed,
            start.side,
        ) {
            Some(mut fight) => {
                fight.intro = Some(format!("A live battle against {}!", w.name));
                fight.foe_levels = start.team.iter().map(|m| m.level).collect();
                fight.fair = !w.actual_levels;
                self.take_part(&mut fight, &w.picked);
                fight.link = Some(Link {
                    battle: start.battle,
                    opponent: start.opponent,
                    name: w.name,
                    step: 1,
                    ready: None,
                    outbox: Vec::new(),
                    ended: None,
                    closed: false,
                    others: Vec::new(),
                    notes: Vec::new(),
                });
                self.queued.push(Command::Cue(Cue::Sound(Sound::Confirm)));
                State::Fight(Box::new(fight))
            }
            None => {
                self.outbox.push(leave(start.battle, LEAVE_LEFT));
                self.notice = Some("That battle can't start.".into());
                State::Format { cursor: 0 }
            }
        }
    }

    /// Adds "Trainers" to the format menu: the ladder of `roster` with the player's `progress`
    /// (whose milestone moves the player's Chao then use).
    pub fn with_trainers(mut self, roster: Arc<Roster>, progress: Progress) -> Session {
        self.trainers = Some(roster);
        self.progress = progress;
        self.refresh_profiles();
        self
    }

    /// The player's ladder `progress` without the ladder (the catalog's trainers couldn't be read):
    /// the Chao keep the milestone moves they were taught.
    pub fn with_progress(mut self, progress: Progress) -> Session {
        self.progress = progress;
        self.refresh_profiles();
        self
    }

    /// The trainer ladder's progress, to keep after `Command::Progress`.
    pub fn progress(&self) -> &Progress {
        &self.progress
    }

    /// The player's Chao's battle levels (by identity), which battles add XP to.
    pub fn with_levels(mut self, levels: BTreeMap<u64, Level>) -> Session {
        self.levels = levels;
        self.refresh_profiles();
        self
    }

    /// The player's Chao's battle levels, to keep after `Command::Levels`.
    pub fn levels(&self) -> &BTreeMap<u64, Level> {
        &self.levels
    }

    /// The battle level of the player's Chao with `identity`.
    pub fn level_of(&self, identity: u64) -> Level {
        self.levels.get(&identity).copied().unwrap_or_default()
    }

    /// Every move the player's Chao with `identity` learned, to keep after `Command::Moves`.
    pub fn own_moves(&self, identity: u64) -> Option<&Moves> {
        self.own
            .iter()
            .find(|e| e.chao.identity() == identity)?
            .moves
            .as_ref()
    }

    /// Every type and ability the player's Chao with `identity` got, to keep after
    /// `Command::Traits`.
    pub fn own_traits(&self, identity: u64) -> Option<&Traits> {
        self.own
            .iter()
            .find(|e| e.chao.identity() == identity)?
            .traits
            .as_ref()
    }

    /// Every move the player's Chao `own[chao]` learned, oldest first.
    fn known_moves(&self, chao: usize) -> Vec<u16> {
        self.own
            .get(chao)
            .and_then(|e| e.moves.as_ref())
            .map_or_else(Vec::new, |m| m.known.clone())
    }

    /// Every type and ability the player's Chao `own[chao]` got (none for one without).
    fn traits(&self, chao: usize) -> Traits {
        self.own
            .get(chao)
            .and_then(|e| e.traits.clone())
            .unwrap_or_default()
    }

    /// The rows of a "Choose moves, type & ability" list after its items, "Done" last: the
    /// moves, types or abilities (with "None" first).
    fn section_len(&self, chao: usize, section: Section) -> usize {
        match section {
            Section::Moves => self.known_moves(chao).len() + 1,
            Section::Types => self.traits(chao).types.len() + 1,
            Section::Ability => self.traits(chao).abilities.len() + 2,
        }
    }

    /// A frame of "Choose moves, type & ability" on the team screen: which Chao, then its
    /// moves, types or ability, then that list, where A switches a move or type on or off or
    /// picks the ability. `None` once it's back to the team.
    fn choose(
        &mut self,
        mut c: Choosing,
        pressed: u32,
        out: &mut Vec<Command>,
    ) -> Option<Choosing> {
        let Some(chao) = c.chao else {
            if step(&mut c.cursor, self.own.len(), pressed) {
                out.push(sound(Sound::Cursor));
            }
            if pressed & buttons::B != 0 {
                out.push(sound(Sound::Back));
                return None;
            }
            if pressed & buttons::A != 0 && c.cursor < self.own.len() {
                out.push(sound(Sound::Confirm));
                return Some(Choosing {
                    chao: Some(c.cursor),
                    section: None,
                    cursor: 0,
                });
            }
            return Some(c);
        };
        let Some(section) = c.section else {
            // Moves, type, ability, then "Done".
            if step(&mut c.cursor, Section::ALL.len() + 1, pressed) {
                out.push(sound(Sound::Cursor));
            }
            let back = Choosing {
                chao: None,
                section: None,
                cursor: chao,
            };
            if pressed & buttons::B != 0 {
                out.push(sound(Sound::Back));
                return Some(back);
            }
            if pressed & buttons::A != 0 {
                out.push(sound(Sound::Confirm));
                return Some(match Section::ALL.get(c.cursor) {
                    Some(&section) => Choosing {
                        section: Some(section),
                        cursor: 0,
                        ..c
                    },
                    None => back,
                });
            }
            return Some(c);
        };
        if step(&mut c.cursor, self.section_len(chao, section), pressed) {
            out.push(sound(Sound::Cursor));
        }
        let back = Choosing {
            chao: Some(chao),
            section: None,
            cursor: section.row(),
        };
        if pressed & buttons::B != 0 {
            out.push(sound(Sound::Back));
            return Some(back);
        }
        if pressed & buttons::A == 0 {
            return Some(c);
        }
        let traits = self.traits(chao);
        let done = match section {
            Section::Moves => self.known_moves(chao).get(c.cursor).map(|&id| {
                self.toggle_move(chao, id)
                    .map_err(|why| format!("{}.", why.message()))
            }),
            Section::Types => traits.types.get(c.cursor).map(|&t| {
                self.toggle_type(chao, t)
                    .map_err(|why| format!("{}.", why.message()))
            }),
            Section::Ability => match c.cursor {
                0 => Some(0),
                i => traits.abilities.get(i - 1).copied(),
            }
            .map(|a| {
                self.choose_ability(chao, a)
                    .map(|()| true)
                    .map_err(|why| format!("{}.", why.message()))
            }),
        };
        match done {
            // "Done".
            None => {
                out.push(sound(Sound::Confirm));
                return Some(back);
            }
            Some(Ok(on)) => out.push(sound(if on { Sound::Confirm } else { Sound::Back })),
            Some(Err(why)) => self.notice = Some(why),
        }
        Some(c)
    }

    /// "Lv 12. <types>, <ability>. Battles with: ..." for the player's Chao `own[chao]`.
    fn battles_with(&self, chao: usize) -> Option<String> {
        let p = self.own_profiles.get(chao)?;
        let names: Vec<String> = p
            .moves
            .iter()
            .filter(|&&id| self.dex.move_data(id).is_some())
            .map(|&id| self.dex.move_name(id))
            .collect();
        let ability = match p.ability {
            0 => "no ability".to_string(),
            a => self.dex.ability_name(a),
        };
        Some(format!(
            "{}, {ability}. Battles with: {}.",
            type_names(&self.dex, p.types),
            names.join(", ")
        ))
    }

    /// The screen of "Choose moves, type & ability" (`choose`).
    fn choosing_screen(&self, c: &Choosing) -> Screen {
        let Some(chao) = c.chao else {
            return Screen {
                title: "Moves, type & ability: which Chao?".into(),
                items: self
                    .own
                    .iter()
                    .map(|e| {
                        let m = e.moves.clone().unwrap_or_default();
                        let on = m.active.iter().filter(|&&id| id != 0).count();
                        format!("{:<7}  {on} of {} moves", e.name, m.known.len())
                    })
                    .collect(),
                cursor: Some(c.cursor),
                detail: self.battles_with(c.cursor),
                hint: "A: choose  B: back",
                ..Screen::default()
            };
        };
        let name = self.own.get(chao).map_or("", |e| e.name.as_str());
        let traits = self.traits(chao);
        let Some(section) = c.section else {
            let m = self
                .own
                .get(chao)
                .and_then(|e| e.moves.clone())
                .unwrap_or_default();
            let on = m.active.iter().filter(|&&id| id != 0).count();
            let types: Vec<String> = traits
                .active_type_list()
                .into_iter()
                .map(|t| self.dex.type_name(t))
                .collect();
            let items = vec![
                format!("Moves: {on} of {} on", m.known.len()),
                if types.is_empty() {
                    "Type: Normal (none from partners yet)".into()
                } else {
                    format!("Type: {}", types.join("/"))
                },
                match traits.ability {
                    0 => "Ability: none".into(),
                    a => format!("Ability: {}", self.dex.ability_name(a)),
                },
                "Done".into(),
            ];
            let detail = match Section::ALL.get(c.cursor) {
                Some(Section::Moves) => {
                    "Up to four moves from every move it has learned; empty slots take basic moves."
                }
                Some(Section::Types) => "One or two of the types its partners gave it.",
                Some(Section::Ability) => "One of the abilities its partners gave it, or none.",
                None => "Back to your Chao.",
            };
            return Screen {
                title: format!("{name}: moves, type & ability"),
                items,
                cursor: Some(c.cursor),
                detail: Some(format!(
                    "{detail} {}",
                    self.battles_with(chao).unwrap_or_default()
                )),
                hint: "A: choose  B: back",
                ..Screen::default()
            };
        };
        let mut items: Vec<String>;
        let detail;
        let title;
        let hint;
        match section {
            Section::Moves => {
                let known = self.known_moves(chao);
                let moves = self.own.get(chao).and_then(|e| e.moves.as_ref());
                items = known
                    .iter()
                    .map(|&id| {
                        let on = if moves.is_some_and(|m| m.is_active(id)) {
                            "[x]"
                        } else {
                            "[ ]"
                        };
                        let about = match self.dex.move_data(id) {
                            Some(d) if d.power > 1 => {
                                format!("{} {}", self.dex.type_name(d.type_id), d.power)
                            }
                            Some(d) => self.dex.type_name(d.type_id),
                            None => "(unavailable)".into(),
                        };
                        format!("{on} {}  {about}", self.dex.move_name(id))
                    })
                    .collect();
                detail = match known.get(c.cursor) {
                    Some(&id) => self.dex.move_data(id).map_or_else(
                        || "Creature battles can't use this move. A: switch it off.".into(),
                        |m| move_detail(&self.dex, &m),
                    ),
                    None if known.is_empty() => {
                        "It hasn't learned any moves yet, so it battles with basic moves.".into()
                    }
                    None => "Up to four moves on; empty slots take basic moves.".into(),
                };
                title = format!("{name}'s moves (4 at most)");
                hint = "A: use or drop  B: back";
            }
            Section::Types => {
                items = traits
                    .types
                    .iter()
                    .map(|&t| {
                        let on = if traits.is_type_active(t) {
                            "[x]"
                        } else {
                            "[ ]"
                        };
                        format!("{on} {}", self.dex.type_name(t))
                    })
                    .collect();
                detail = match traits.types.get(c.cursor) {
                    Some(_) => "One or two types on.".into(),
                    None if traits.types.is_empty() => {
                        "Its partners haven't given it a type yet, so it battles as Normal.".into()
                    }
                    None => "One or two types on.".into(),
                };
                title = format!("{name}'s type (2 at most)");
                hint = "A: use or drop  B: back";
            }
            Section::Ability => {
                let mark = |on: bool| if on { "(o)" } else { "( )" };
                items = vec![format!("{} None", mark(traits.ability == 0))];
                items.extend(traits.abilities.iter().map(|&a| {
                    format!("{} {}", mark(traits.ability == a), self.dex.ability_name(a))
                }));
                detail = match c.cursor.checked_sub(1).map(|i| traits.abilities.get(i)) {
                    None => "It battles without an ability.".into(),
                    Some(Some(&a)) => self.dex.ability_description(a).trim().to_string(),
                    Some(None) => "The ability it battles with, or none.".into(),
                };
                title = format!("{name}'s ability");
                hint = "A: choose  B: back";
            }
        }
        items.push("Done".into());
        Screen {
            title,
            items,
            cursor: Some(c.cursor),
            detail: Some(detail),
            hint,
            ..Screen::default()
        }
    }

    /// Switches move `id` of the player's Chao `own[chao]` on or off for battles
    /// (`Moves::toggle`); whether it's on now. The change goes out as `Command::Moves`.
    pub fn toggle_move(&mut self, chao: usize, id: u16) -> Result<bool, MoveRefused> {
        let usable = self.dex.move_data(id).is_some();
        let e = self.own.get_mut(chao).ok_or(MoveRefused::Unknown)?;
        let m = e.moves.as_mut().ok_or(MoveRefused::Unknown)?;
        let on = if usable {
            m.toggle(id)?
        } else if m.is_active(id) {
            // One battles can't use only switches off, even the last one.
            m.drop_unusable(|other| other != id);
            false
        } else if m.knows(id) {
            return Err(MoveRefused::Unsupported);
        } else {
            return Err(MoveRefused::Unknown);
        };
        let identity = self.moves_changed(chao);
        self.queued.push(Command::Moves(identity));
        Ok(on)
    }

    /// Switches type `t` of the player's Chao `own[chao]` on or off for battles
    /// (`Traits::toggle_type`); whether it's on now. The change goes out as `Command::Traits`.
    pub fn toggle_type(&mut self, chao: usize, t: u8) -> Result<bool, TraitRefused> {
        let e = self.own.get_mut(chao).ok_or(TraitRefused::Unknown)?;
        let on = e
            .traits
            .as_mut()
            .ok_or(TraitRefused::Unknown)?
            .toggle_type(t)?;
        self.traits_changed(chao);
        Ok(on)
    }

    /// The player's Chao `own[chao]` battles with ability `a` (0: none) from now on
    /// (`Traits::choose_ability`). The change goes out as `Command::Traits`.
    pub fn choose_ability(&mut self, chao: usize, a: u8) -> Result<(), TraitRefused> {
        let e = self.own.get_mut(chao).ok_or(TraitRefused::Unknown)?;
        e.traits
            .as_mut()
            .ok_or(TraitRefused::Unknown)?
            .choose_ability(a)?;
        self.traits_changed(chao);
        Ok(())
    }

    /// After `own[chao]`'s types or ability changed: copies of that Chao take them too, and
    /// the change goes out.
    fn traits_changed(&mut self, chao: usize) {
        let identity = self.own[chao].chao.identity();
        let traits = self.own[chao].traits.clone();
        for e in &mut self.own {
            if e.chao.identity() == identity {
                e.traits.clone_from(&traits);
            }
        }
        self.refresh_profiles();
        self.queued.push(Command::Traits(identity));
    }

    /// After `own[chao]`'s moves changed: copies of that Chao (the same identity) take them
    /// too. Its identity.
    fn moves_changed(&mut self, chao: usize) -> u64 {
        let identity = self.own[chao].chao.identity();
        let moves = self.own[chao].moves.clone();
        for e in &mut self.own {
            if e.chao.identity() == identity {
                e.moves.clone_from(&moves);
            }
        }
        self.refresh_profiles();
        identity
    }

    /// A Chao's types, ability, moves and level. The player's own Chao are at the levels here.
    fn profile(&self, e: &Entry) -> Profile {
        let mut p = (self.profile_of)(&e.chao, e.moves.as_ref(), e.traits.as_ref());
        p.level = self.level_of(e.chao.identity()).level;
        p
    }

    fn refresh_profiles(&mut self) {
        self.own_profiles = self.own.iter().map(|e| self.profile(e)).collect();
    }

    /// Notes the player's Chao on `fight`'s side (the `picked` ones, in order), so the ones that
    /// take part earn XP.
    fn take_part(&self, fight: &mut Fight, picked: &[usize]) {
        fight.own_ids = self.own_ids(picked);
        fight.own_levels = fight.own_ids.iter().map(|&id| self.level_of(id)).collect();
    }

    /// The format menu's items: singles, doubles, the multi battle, and the trainers when on.
    fn formats(&self) -> usize {
        if self.trainers.is_some() { 4 } else { 3 }
    }

    /// The player's picked Chao as fighters.
    fn chao_members(&self, entries: &[Entry]) -> Vec<Member> {
        entries
            .iter()
            .map(|e| Member {
                fighter: team::fighter(&e.name, &e.chao, &self.profile(e), &self.dex),
                record: Some(e.chao.clone()),
                species: None,
                exp_yield: 0,
            })
            .collect()
    }

    /// A fight of the picked Chao against trainer `id`.
    fn trainer_fight(&self, picked: &[usize], id: u16) -> Option<Fight> {
        let roster = self.trainers.as_ref()?;
        if !roster.plan.can_battle(id, &self.progress) {
            return None;
        }
        let foe = roster.foes.get(&id)?;
        let format = if foe.double {
            Format::Doubles
        } else {
            Format::Singles
        };
        let mine: Vec<Entry> = picked.iter().map(|&i| self.own[i].clone()).collect();
        let theirs = foe
            .party
            .iter()
            .map(|c| Member {
                fighter: c.fighter.clone(),
                record: None,
                species: Some(crate::creature::data::SpeciesKey {
                    species: c.species,
                    form: c.form,
                }),
                exp_yield: c.exp_yield,
            })
            .collect();
        let mut fight = Fight::new(
            format,
            [self.chao_members(&mine), theirs],
            self.dex.clone(),
            self.seed,
        )?;
        fight.foe = Some(id);
        fight.intro = Some(format!("{} wants to battle!", foe.name));
        self.take_part(&mut fight, picked);
        Some(fight)
    }

    /// A singles fight of the picked Chao against the wild visitor.
    fn wild_fight(&self, picked: &[usize]) -> Option<Fight> {
        let wild = self.wild.as_ref()?;
        let mine: Vec<Entry> = picked.iter().map(|&i| self.own[i].clone()).collect();
        let theirs = vec![Member {
            fighter: wild.creature.fighter.clone(),
            record: None,
            species: Some(crate::creature::data::SpeciesKey {
                species: wild.creature.species,
                form: wild.creature.form,
            }),
            exp_yield: wild.creature.exp_yield,
        }];
        let mut fight = Fight::new(
            Format::Singles,
            [self.chao_members(&mine), theirs],
            self.dex.clone(),
            self.seed,
        )?;
        fight.wild = true;
        fight.intro = Some(format!(
            "Wild {} wants to battle!",
            wild.creature.fighter.name
        ));
        self.take_part(&mut fight, picked);
        Some(fight)
    }

    /// After a frame of a battle, once it's decided (once): a trainer win records progress and
    /// its reward, and the player's Chao that took part in a trainer or live battle earn XP
    /// (`levels`), with "<Chao> grew to Lv N!" after the battle's last messages.
    fn record_result(&mut self, fight: &mut Fight, out: &mut Vec<Command>) {
        if fight.recorded {
            return;
        }
        let Some(decided) = fight.decided() else {
            return;
        };
        fight.recorded = true;
        // A wild visitor's trip that went on too long counted as given up: it ran away.
        let overdue = self.wild.is_some() && super::wild::overdue();
        let won = decided == Decided::Won && !overdue;
        if let Some(wild) = self.wild.as_mut() {
            let ending = if won { Ending::Won } else { Ending::RanAway };
            wild.ending = Some(ending);
            let name = &wild.creature.fighter.name;
            fight.reward.push(if won {
                format!("{name} stays in the garden.")
            } else {
                format!("{name} ran away.")
            });
        }
        if let Some(id) = fight.foe.filter(|_| won) {
            let first = self.progress.beat(id);
            if let Some(roster) = &self.trainers {
                fight.reward = roster.reward(id, first, &self.progress, &self.dex);
            }
            if first {
                out.push(Command::Progress);
            }
        }
        if !matches!(decided, Decided::Won | Decided::Lost) {
            return;
        }
        let gains = fight.earn(won, &mut self.levels);
        if gains.is_empty() {
            return;
        }
        out.push(Command::Levels);
        // At the end of the beats when the battle ended by itself (its last ones are queued).
        let playing = fight.stage != Stage::Result;
        for g in &gains {
            fight.xp_lines.push(if g.grew {
                format!("{} gained {} XP and grew to Lv {}!", g.name, g.xp, g.level)
            } else {
                format!("{} gained {} XP.", g.name, g.xp)
            });
            if g.grew && playing {
                fight.queue.push_back(Beat {
                    text: Some(format!("{} grew to Lv {}!", g.name, g.level)),
                    cues: Vec::new(),
                    updates: Vec::new(),
                    frames: LEVEL_UP_FRAMES,
                });
            }
        }
        fight.gains = gains;
    }

    /// What the player's Chao earned in the battle shown now (none before it's decided).
    pub fn gains(&self) -> &[Gain] {
        match &self.state {
            State::Fight(f) => &f.gains,
            _ => &[],
        }
    }

    pub fn done(&self) -> bool {
        matches!(self.state, State::Done)
    }

    /// In the arena (the menu was left for the battle).
    pub fn in_arena(&self) -> bool {
        matches!(&self.state, State::Fight(f) if f.opened)
    }

    /// The record of `side`'s team member `member`, for the arena's Chao (none for a
    /// trainer's creature).
    pub fn record(&self, side: usize, member: usize) -> Option<&ChaoData> {
        match &self.state {
            State::Fight(f) => f.records.get(side)?.get(member)?.as_ref(),
            _ => None,
        }
    }

    /// `side`'s team member `member` is the wild visitor in its shiny colours.
    pub fn shiny(&self, side: usize, member: usize) -> bool {
        side == 1 && member == 0 && self.wild.as_ref().is_some_and(|w| w.shiny)
    }

    /// The species of `side`'s team member `member` when it's a trainer's creature, for its
    /// picture in the arena.
    pub fn species(&self, side: usize, member: usize) -> Option<u16> {
        match &self.state {
            State::Fight(f) => f.species.get(side)?.get(member)?.map(|key| key.species),
            _ => None,
        }
    }

    pub fn form(&self, side: usize, member: usize) -> u8 {
        match &self.state {
            State::Fight(f) => f
                .species
                .get(side)
                .and_then(|s| s.get(member))
                .and_then(|key| *key)
                .map_or(0, |key| key.form),
            _ => 0,
        }
    }

    /// Opponent teams to pick from after the player's team: friends for a live battle, the
    /// lineups, then copies of the player's other Chao (or of the team itself when there are
    /// no others).
    fn opponents(&self, picked: &[usize]) -> Vec<(String, Against)> {
        let mut out: Vec<(String, Against)> = self
            .friends
            .iter()
            .enumerate()
            .map(|(i, f)| {
                let label = if f.busy {
                    format!("{} (live, busy)", f.name)
                } else {
                    format!("{} (live)", f.name)
                };
                (label, Against::Live(i))
            })
            .collect();
        if !out.is_empty() {
            out.push((self.level_rule_label(), Against::LevelRule));
        }
        out.extend(self.lineups.iter().enumerate().map(|(i, l)| {
            (
                format!("{} ({})", l.label, l.members.len().min(TEAM_SIZE)),
                Against::Lineup(i),
            )
        }));
        let others: Vec<usize> = (0..self.own.len())
            .filter(|i| !picked.contains(i))
            .take(TEAM_SIZE)
            .collect();
        let (label, members) = if others.is_empty() {
            ("Copies of your team", picked.to_vec())
        } else {
            ("Copies of your other Chao", others)
        };
        out.push((
            format!("{label} ({})", members.len()),
            Against::Copies(members),
        ));
        out
    }

    /// The live-challenge level rule's row.
    fn level_rule_label(&self) -> String {
        if self.actual_levels {
            "Live rule: actual levels".into()
        } else {
            "Live rule: Level 50".into()
        }
    }

    /// About an opponent team: its Chao's names, or who a live battle is against.
    fn opponent_detail(&self, against: &Against) -> String {
        match against {
            Against::LevelRule => if self.actual_levels {
                "A: switch. In your live challenges, each Chao battles at its own battle level."
            } else {
                "A: switch. In your live challenges, every Chao on both teams battles at level 50."
            }
            .into(),
            Against::Live(i) => match self.friends.get(*i) {
                Some(f) if f.busy => format!("{} is in another battle.", f.name),
                Some(f) => format!(
                    "A live battle: {} picks their own team and chooses their own moves.",
                    f.name
                ),
                None => String::new(),
            },
            Against::Lineup(i) => self.lineups.get(*i).map_or_else(String::new, |l| {
                l.members
                    .iter()
                    .take(TEAM_SIZE)
                    .map(|e| e.name.clone())
                    .collect::<Vec<_>>()
                    .join(", ")
            }),
            Against::Copies(m) => m
                .iter()
                .map(|&i| self.own[i].name.clone())
                .collect::<Vec<_>>()
                .join(", "),
        }
    }

    /// The menu list showing, by its rows' search names (`None`: the row always shows, as
    /// "Done" does), one per item of its screen; `None` when no list with a cursor shows.
    fn list_names(&self) -> Option<Vec<Option<String>>> {
        let named = |names: Vec<String>, after: usize| -> Vec<Option<String>> {
            names
                .into_iter()
                .map(Some)
                .chain(std::iter::repeat_n(None, after))
                .collect()
        };
        let own_names = || self.own.iter().map(|e| e.name.clone()).collect::<Vec<_>>();
        Some(match &self.state {
            State::Format { .. } => vec![None; self.formats()],
            State::Trainers(picker) => picker.names(&trainers::Ctx {
                roster: self.trainers.as_deref()?,
                progress: &self.progress,
                dex: &self.dex,
                own: &self.own,
                profiles: &self.own_profiles,
            }),
            State::Team { moves: None, .. } => named(own_names(), 2),
            State::Team { moves: Some(c), .. } => match (c.chao, c.section) {
                (None, _) => named(own_names(), 0),
                (Some(_), None) => vec![None; Section::ALL.len() + 1],
                (Some(chao), Some(Section::Moves)) => named(
                    self.known_moves(chao)
                        .iter()
                        .map(|&id| self.dex.move_name(id))
                        .collect(),
                    1,
                ),
                (Some(chao), Some(Section::Types)) => named(
                    self.traits(chao)
                        .types
                        .iter()
                        .map(|&t| self.dex.type_name(t))
                        .collect(),
                    1,
                ),
                (Some(chao), Some(Section::Ability)) => std::iter::once(None)
                    .chain(named(
                        self.traits(chao)
                            .abilities
                            .iter()
                            .map(|&a| self.dex.ability_name(a))
                            .collect(),
                        1,
                    ))
                    .collect(),
            },
            State::Opponent { picked, .. } => self
                .opponents(picked)
                .into_iter()
                .map(|(label, against)| match against {
                    Against::Live(i) => self.friends.get(i).map(|f| f.name.clone()),
                    Against::Lineup(_) => Some(label),
                    _ => None,
                })
                .collect(),
            // A press on its way as the challenge came doesn't move it either.
            State::Offer { age, .. } if *age >= OFFER_GRACE => vec![None; 2],
            State::Multi { .. } => vec![None; PLAN_ROWS],
            State::Seat { plan, .. } => self
                .seat_options(plan)
                .into_iter()
                .map(|h| {
                    Some(match h {
                        Holder::Friend { name, .. } => name,
                        Holder::Computer { label, .. } => label,
                    })
                })
                .collect(),
            State::MultiWait { .. } => vec![None; 4],
            _ => return None,
        })
    }

    /// The cursor of the list showing (`list_names`).
    fn cursor_mut(&mut self) -> Option<&mut usize> {
        match &mut self.state {
            State::Format { cursor }
            | State::Opponent { cursor, .. }
            | State::Offer { cursor, .. }
            | State::Multi { cursor, .. }
            | State::Seat { cursor, .. }
            | State::MultiWait { cursor, .. }
            | State::Team {
                moves: None,
                cursor,
                ..
            } => Some(cursor),
            State::Team { moves: Some(c), .. } => Some(&mut c.cursor),
            State::Trainers(picker) => Some(picker.cursor_mut()),
            _ => None,
        }
    }

    /// Which list shows: another one clears the search.
    fn list_key(&self) -> Option<(u8, usize, usize)> {
        Some(match &self.state {
            State::Format { .. } => (0, 0, 0),
            State::Trainers(picker) => {
                let (view, of) = picker.view_key();
                (1, view, of)
            }
            State::Team { moves: None, .. } => (2, 0, 0),
            State::Team { moves: Some(c), .. } => match (c.chao, c.section) {
                (None, _) => (3, 0, 0),
                (Some(chao), None) => (4, chao, 0),
                (Some(chao), Some(section)) => (5, chao, section.row()),
            },
            State::Opponent { .. } => (6, 0, 0),
            State::Offer { .. } => (7, 0, 0),
            State::Multi { .. } => (8, 0, 0),
            State::Seat { place, .. } => (9, *place, 0),
            State::MultiWait { .. } => (10, 0, 0),
            _ => return None,
        })
    }

    /// The list showing takes typing to search it: it has rows with names.
    pub fn typing(&self) -> bool {
        self.list_names()
            .is_some_and(|names| names.iter().any(Option::is_some))
    }

    /// The rows of the list showing that match the search (`None` without a list).
    fn shown_rows(&self) -> Option<Vec<usize>> {
        let names = self.list_names()?;
        Some(self.search.shown(names.iter().map(Option::as_deref)))
    }

    /// This frame's keyboard on the menus: typing searches a list that has names (Backspace
    /// deletes a letter, Esc clears the search, or goes back when there's none), Enter is A and
    /// PgUp/PgDn jump. The buttons it presses, for `update`.
    pub fn keyboard(&mut self, typed: &Typed) -> u32 {
        let mut extra = 0;
        for (pressed, button) in [
            (typed.enter, buttons::A),
            (typed.up, buttons::UP),
            (typed.down, buttons::DOWN),
            (typed.left, buttons::LEFT),
            (typed.right, buttons::RIGHT),
            (typed.page_up, buttons::PAGE_UP),
            (typed.page_down, buttons::PAGE_DOWN),
        ] {
            if pressed {
                extra |= button;
            }
        }
        if self.list_names().is_none() || !self.typing() {
            if typed.escape {
                extra |= buttons::B;
            }
            return extra;
        }
        match self.search.edit(typed, false) {
            Edit::None => {}
            Edit::Back => extra |= buttons::B,
            Edit::Changed => {
                self.notice = None;
                let names = self.list_names().unwrap_or_default();
                let cursor = self.cursor_mut().map(|cursor| *cursor);
                let focused = cursor.map(|cursor| {
                    self.search.focus(
                        cursor,
                        names
                            .iter()
                            .enumerate()
                            .map(|(i, name)| (i, name.as_deref())),
                    )
                });
                if let Some(cursor) = self.cursor_mut() {
                    *cursor = focused.unwrap_or(*cursor);
                }
            }
        }
        extra
    }

    /// Moves the cursor of the list showing among the rows that match the search: Up/Down a
    /// row (wrapping), Left/Right and PgUp/PgDn `search::JUMP` rows. The buttons left for the
    /// list itself (A does nothing when no row matches).
    fn navigate(&mut self, pressed: u32, out: &mut Vec<Command>) -> u32 {
        let Some(shown) = self.shown_rows() else {
            return pressed;
        };
        let Some(cursor) = self.cursor_mut() else {
            return pressed;
        };
        let old = *cursor;
        let mut c = search::settle(old, &shown);
        if pressed & buttons::UP != 0 {
            c = search::step(c, &shown, false);
        } else if pressed & buttons::DOWN != 0 {
            c = search::step(c, &shown, true);
        } else if pressed & (buttons::LEFT | buttons::PAGE_UP) != 0 {
            c = search::jump(c, &shown, false, search::JUMP);
        } else if pressed & (buttons::RIGHT | buttons::PAGE_DOWN) != 0 {
            c = search::jump(c, &shown, true, search::JUMP);
        }
        *cursor = c;
        if c != old && pressed & buttons::MOVES != 0 {
            out.push(sound(Sound::Cursor));
            if let State::Trainers(picker) = &mut self.state {
                picker.clear_notice();
            }
        }
        let rest = pressed & !buttons::MOVES;
        if shown.is_empty() {
            rest & !buttons::A
        } else {
            rest
        }
    }

    /// `s` (the screen of the list showing) with only the rows that match the search, and the
    /// search's line.
    fn searched(&self, mut s: Screen) -> Screen {
        if !self.typing() {
            return s;
        }
        s.search = Some(self.search.text().to_string());
        let Some(shown) = self.shown_rows().filter(|_| !self.search.is_empty()) else {
            return s;
        };
        if shown.is_empty() {
            s.items = vec!["No matches".into()];
            s.disabled = vec![0];
            s.cursor = None;
            s.detail = None;
            s.bars.clear();
            s.scroll = 0;
            return s;
        }
        let place = |i: usize| shown.iter().position(|&r| r == i);
        s.items = shown
            .iter()
            .filter_map(|&i| s.items.get(i).cloned())
            .collect();
        s.disabled = s.disabled.iter().filter_map(|&i| place(i)).collect();
        s.bars = s
            .bars
            .iter()
            .filter_map(|&(i, xp)| Some((place(i)?, xp)))
            .collect();
        s.cursor = s.cursor.and_then(place);
        s
    }

    /// Runs one frame with the buttons pressed in it; `arena_ready` once the arena is set up
    /// after `OpenArena`.
    pub fn update(&mut self, pressed: u32, arena_ready: bool) -> Vec<Command> {
        let mut out = std::mem::take(&mut self.queued);
        if pressed & (buttons::A | buttons::B | buttons::MOVES) != 0 {
            self.notice = None;
        }
        let list = self.list_key();
        let pressed = self.navigate(pressed, &mut out);
        let state = std::mem::replace(&mut self.state, State::Done);
        self.state = match state {
            State::Format { mut cursor } => {
                if step(&mut cursor, self.formats(), pressed) {
                    out.push(sound(Sound::Cursor));
                }
                if pressed & buttons::B != 0 {
                    out.push(sound(Sound::Back));
                    out.push(Command::Closed);
                    State::Done
                } else if pressed & buttons::A != 0 {
                    if self.own.is_empty() {
                        self.notice = Some("You have no Chao to battle with.".into());
                        State::Format { cursor }
                    } else if cursor == 3
                        && let Some(roster) = &self.trainers
                    {
                        out.push(sound(Sound::Confirm));
                        State::Trainers(Picker::open(roster, &self.progress))
                    } else {
                        out.push(sound(Sound::Confirm));
                        let format = if cursor == 0 {
                            Format::Singles
                        } else {
                            Format::Doubles
                        };
                        State::Team {
                            format,
                            cursor: 0,
                            picked: Vec::new(),
                            foe: None,
                            live: None,
                            moves: None,
                            multi: (cursor == 2).then(Box::default),
                        }
                    }
                } else {
                    State::Format { cursor }
                }
            }
            State::Trainers(mut picker) => match self.trainers.clone() {
                None => State::Format { cursor: 0 },
                Some(roster) => {
                    let ctx = trainers::Ctx {
                        roster: &roster,
                        progress: &self.progress,
                        dex: &self.dex,
                        own: &self.own,
                        profiles: &self.own_profiles,
                    };
                    match picker.update(&ctx, pressed, &mut out) {
                        trainers::Step::Stay => State::Trainers(picker),
                        trainers::Step::Back => State::Format { cursor: 3 },
                        trainers::Step::Fight(id) => {
                            let double = roster.foes.get(&id).is_some_and(|f| f.double);
                            State::Team {
                                format: if double {
                                    Format::Doubles
                                } else {
                                    Format::Singles
                                },
                                cursor: 0,
                                picked: Vec::new(),
                                foe: Some(id),
                                live: None,
                                moves: None,
                                multi: None,
                            }
                        }
                        trainers::Step::Taught {
                            chao,
                            slot,
                            move_id,
                        } => {
                            let replacing = self
                                .own_profiles
                                .get(chao)
                                .and_then(|p| p.moves.get(slot))
                                .copied()
                                .unwrap_or(0);
                            if let Some(e) = self.own.get_mut(chao) {
                                e.moves
                                    .get_or_insert_with(Moves::default)
                                    .teach_milestone(move_id, replacing);
                                out.push(Command::Moves(self.moves_changed(chao)));
                            }
                            State::Trainers(picker)
                        }
                    }
                }
            },
            State::Team {
                format,
                cursor,
                picked,
                foe,
                live,
                moves: Some(choosing),
                multi,
            } => State::Team {
                format,
                cursor,
                picked,
                foe,
                live,
                moves: self.choose(choosing, pressed, &mut out),
                multi,
            },
            State::Team {
                format,
                mut cursor,
                mut picked,
                foe,
                live,
                moves: None,
                multi,
            } => {
                // The Chao, "Choose moves", then "Done".
                let (moves_row, done) = (self.own.len(), self.own.len() + 1);
                let len = done + 1;
                if step(&mut cursor, len, pressed) {
                    out.push(sound(Sound::Cursor));
                }
                let limit = self
                    .team_limit(multi.is_some() || (live.is_some() && self.offer_invite.is_some()));
                let stay = |cursor, picked, foe, live, multi| State::Team {
                    format,
                    cursor,
                    picked,
                    foe,
                    live,
                    moves: None,
                    multi,
                };
                if pressed & buttons::B != 0 && self.wild.is_some() {
                    out.push(sound(Sound::Back));
                    out.push(Command::Closed);
                    State::Done
                } else if pressed & buttons::B != 0 {
                    out.push(sound(Sound::Back));
                    match (foe, &self.trainers, live) {
                        (_, _, Some((from, name))) => State::Offer {
                            from,
                            name,
                            format,
                            cursor: 0,
                            age: OFFER_GRACE,
                        },
                        (Some(id), Some(roster), None) => State::Trainers(Picker::at(roster, id)),
                        _ if multi.is_some() => State::Format { cursor: 2 },
                        _ => State::Format {
                            cursor: usize::from(format == Format::Doubles),
                        },
                    }
                } else if pressed & buttons::A != 0 && cursor == moves_row {
                    out.push(sound(Sound::Confirm));
                    State::Team {
                        format,
                        cursor,
                        picked,
                        foe,
                        live,
                        moves: Some(Choosing {
                            chao: None,
                            section: None,
                            cursor: 0,
                        }),
                        multi,
                    }
                } else if pressed & buttons::A != 0 && cursor == done {
                    if picked.is_empty() {
                        self.notice = Some("Pick at least one Chao.".into());
                        stay(cursor, picked, foe, live, multi)
                    } else if let Some((from, name)) = live {
                        // Accept the challenge (or the multi battle's invite) with this team.
                        let team = self.wire_team(&picked);
                        self.outbox.push(Body::BattleAnswer {
                            to: from,
                            answer: ANSWER_ACCEPT,
                            dex: self.dex_print,
                            team: team.clone(),
                        });
                        out.push(sound(Sound::Confirm));
                        State::Waiting(Box::new(Waiting {
                            with: from,
                            name,
                            format,
                            picked,
                            team,
                            challenger: false,
                            actual_levels: self.offer_actual_levels,
                            waited: 0,
                            multi: self.offer_invite.is_some(),
                        }))
                    } else if let Some(mut plan) = multi {
                        out.push(sound(Sound::Confirm));
                        if plan.seats.is_none() {
                            plan.seats = Some(self.default_seats(&picked));
                        }
                        plan.picked = picked;
                        State::Multi { plan, cursor: 1 }
                    } else if self.wild.is_some() {
                        match self.wild_fight(&picked) {
                            Some(fight) => {
                                out.push(sound(Sound::Confirm));
                                State::Fight(Box::new(fight))
                            }
                            None => {
                                self.notice = Some("That battle can't start.".into());
                                stay(cursor, picked, foe, None, None)
                            }
                        }
                    } else if let Some(id) = foe {
                        match self.trainer_fight(&picked, id) {
                            Some(fight) => {
                                out.push(sound(Sound::Confirm));
                                State::Fight(Box::new(fight))
                            }
                            None => {
                                self.notice = Some("That battle can't start.".into());
                                stay(cursor, picked, foe, None, None)
                            }
                        }
                    } else {
                        out.push(sound(Sound::Confirm));
                        State::Opponent {
                            format,
                            picked,
                            cursor: 0,
                        }
                    }
                } else if pressed & buttons::A != 0 {
                    if let Some(i) = picked.iter().position(|&p| p == cursor) {
                        picked.remove(i);
                        out.push(sound(Sound::Back));
                    } else if picked.len() < limit {
                        picked.push(cursor);
                        out.push(sound(Sound::Confirm));
                        // A full team goes straight to "Done".
                        if picked.len() == limit {
                            cursor = done;
                        }
                    } else {
                        self.notice = Some(format!("A team has at most {limit} Chao."));
                    }
                    stay(cursor, picked, foe, live, multi)
                } else {
                    stay(cursor, picked, foe, live, multi)
                }
            }
            State::Multi {
                mut plan,
                mut cursor,
            } => {
                if step(&mut cursor, PLAN_ROWS, pressed) {
                    out.push(sound(Sound::Cursor));
                }
                if pressed & buttons::B != 0 {
                    out.push(sound(Sound::Back));
                    State::Team {
                        format: Format::Doubles,
                        cursor: self.own.len() + 1,
                        picked: plan.picked.clone(),
                        foe: None,
                        live: None,
                        moves: None,
                        multi: Some(plan),
                    }
                } else if pressed & buttons::A == 0 {
                    State::Multi { plan, cursor }
                } else {
                    match cursor {
                        0 => {
                            out.push(sound(Sound::Confirm));
                            State::Team {
                                format: Format::Doubles,
                                cursor: 0,
                                picked: plan.picked.clone(),
                                foe: None,
                                live: None,
                                moves: None,
                                multi: Some(plan),
                            }
                        }
                        1..=3 => {
                            out.push(sound(Sound::Confirm));
                            State::Seat {
                                plan,
                                place: cursor,
                                cursor: 0,
                            }
                        }
                        4 => {
                            self.actual_levels = !self.actual_levels;
                            out.push(sound(Sound::Cursor));
                            State::Multi { plan, cursor }
                        }
                        _ if plan.friends() => {
                            out.push(sound(Sound::Confirm));
                            self.invite(&mut plan);
                            State::MultiWait { plan, cursor: 0 }
                        }
                        _ => match self.local_multi(&plan) {
                            Some(fight) => {
                                out.push(sound(Sound::Confirm));
                                State::Fight(Box::new(fight))
                            }
                            None => {
                                self.notice = Some("Those teams can't battle.".into());
                                State::Multi { plan, cursor }
                            }
                        },
                    }
                }
            }
            State::Seat {
                mut plan,
                place,
                mut cursor,
            } => {
                let mut options = self.seat_options(&plan);
                if step(&mut cursor, options.len(), pressed) {
                    out.push(sound(Sound::Cursor));
                }
                let back = |plan: Box<Plan>| {
                    if plan.answers.is_some() {
                        State::MultiWait {
                            plan,
                            cursor: place - 1,
                        }
                    } else {
                        State::Multi {
                            plan,
                            cursor: place,
                        }
                    }
                };
                if pressed & buttons::B != 0 {
                    out.push(sound(Sound::Back));
                    back(plan)
                } else if pressed & buttons::A != 0 && cursor < options.len() {
                    let holder = options.swap_remove(cursor);
                    let elsewhere = |id: u8| {
                        (1..=3).any(|p| {
                            p != place
                                && matches!(plan.seat(p), Some(Holder::Friend { id: o, .. }) if *o == id)
                        })
                    };
                    match &holder {
                        Holder::Friend { id, name }
                            if self.friends.iter().any(|f| f.id == *id && f.busy) =>
                        {
                            self.notice = Some(format!("{name} is in another battle."));
                            State::Seat {
                                plan,
                                place,
                                cursor,
                            }
                        }
                        Holder::Friend { id, name } if elsewhere(*id) => {
                            self.notice = Some(format!("{name} already has a place."));
                            State::Seat {
                                plan,
                                place,
                                cursor,
                            }
                        }
                        _ => {
                            out.push(sound(Sound::Confirm));
                            if let Some(answers) = plan.answers.as_mut()
                                && let Holder::Computer { team, .. } = &holder
                            {
                                // A place nobody took goes to the computer.
                                let team = self.wire_entries(team);
                                self.outbox.push(Body::MultiFill {
                                    place: place as u8,
                                    team,
                                });
                                answers[place - 1] = Answer::Computer;
                            }
                            if let Some(seats) = plan.seats.as_mut() {
                                seats[place - 1] = holder;
                            }
                            back(plan)
                        }
                    }
                } else {
                    State::Seat {
                        plan,
                        place,
                        cursor,
                    }
                }
            }
            State::MultiWait {
                mut plan,
                mut cursor,
            } => {
                plan.waited += 1;
                if step(&mut cursor, 4, pressed) {
                    out.push(sound(Sound::Cursor));
                }
                let gone = plan.waited > MULTI_WAIT_LIMIT;
                let refused = cursor < 3
                    && matches!(
                        plan.answers.as_ref().map(|a| a[cursor]),
                        Some(Answer::Refused(_))
                    );
                if gone {
                    self.notice = Some("Time ran out waiting for the multi battle.".into());
                }
                if gone || pressed & buttons::B != 0 || (pressed & buttons::A != 0 && cursor == 3) {
                    out.push(sound(Sound::Back));
                    self.outbox.push(leave(0, LEAVE_LEFT));
                    plan.answers = None;
                    State::Multi {
                        plan,
                        cursor: PLAN_START,
                    }
                } else if pressed & buttons::A != 0 && refused {
                    out.push(sound(Sound::Confirm));
                    State::Seat {
                        plan,
                        place: cursor + 1,
                        cursor: 0,
                    }
                } else {
                    State::MultiWait { plan, cursor }
                }
            }
            State::Opponent {
                format,
                picked,
                mut cursor,
            } => {
                let mut options = self.opponents(&picked);
                if step(&mut cursor, options.len(), pressed) {
                    out.push(sound(Sound::Cursor));
                }
                if pressed & buttons::B != 0 {
                    out.push(sound(Sound::Back));
                    State::Team {
                        format,
                        cursor: self.own.len() + 1,
                        picked,
                        foe: None,
                        live: None,
                        moves: None,
                        multi: None,
                    }
                } else if pressed & buttons::A != 0 && cursor < options.len() {
                    match options.swap_remove(cursor).1 {
                        Against::LevelRule => {
                            self.actual_levels = !self.actual_levels;
                            out.push(sound(Sound::Cursor));
                            State::Opponent {
                                format,
                                picked,
                                cursor,
                            }
                        }
                        Against::Live(i) => {
                            let friend = self.friends[i].clone();
                            self.challenge(friend, format, picked, cursor, &mut out)
                        }
                        against => self.local_fight(format, picked, cursor, against, &mut out),
                    }
                } else {
                    State::Opponent {
                        format,
                        picked,
                        cursor,
                    }
                }
            }
            State::Offer {
                from,
                name,
                format,
                mut cursor,
                age,
            } => {
                // A press on its way as the challenge came doesn't answer it.
                let pressed = if age < OFFER_GRACE { 0 } else { pressed };
                if step(&mut cursor, 2, pressed) {
                    out.push(sound(Sound::Cursor));
                }
                let accept = pressed & buttons::A != 0 && cursor == 0;
                if accept && self.own.is_empty() {
                    self.outbox.push(answer(from, ANSWER_DECLINE));
                    self.notice = Some("You have no Chao to battle with.".into());
                    out.push(sound(Sound::Back));
                    self.offer_over(&mut out)
                } else if accept {
                    out.push(sound(Sound::Confirm));
                    State::Team {
                        format,
                        cursor: 0,
                        picked: Vec::new(),
                        foe: None,
                        live: Some((from, name)),
                        moves: None,
                        multi: None,
                    }
                } else if pressed & (buttons::A | buttons::B) != 0 {
                    self.outbox.push(answer(from, ANSWER_DECLINE));
                    out.push(sound(Sound::Back));
                    self.offer_over(&mut out)
                } else {
                    State::Offer {
                        from,
                        name,
                        format,
                        cursor,
                        age: age.saturating_add(1),
                    }
                }
            }
            State::Waiting(mut w) => {
                w.waited += 1;
                // A multi battle waits for every place to be filled.
                let limit = if w.multi {
                    MULTI_WAIT_LIMIT
                } else {
                    WAIT_LIMIT
                };
                let gone = w.waited > limit;
                if gone {
                    self.notice = Some(if w.multi {
                        invite_note(END_TIMEOUT, &w.name)
                    } else {
                        end_note(END_TIMEOUT, &w.name)
                    });
                }
                if gone || pressed & buttons::B != 0 {
                    out.push(sound(Sound::Back));
                    self.outbox.push(leave(0, LEAVE_LEFT));
                    if w.challenger {
                        State::Opponent {
                            format: w.format,
                            picked: w.picked,
                            cursor: 0,
                        }
                    } else {
                        State::Format { cursor: 0 }
                    }
                } else {
                    State::Waiting(w)
                }
            }
            State::Fight(mut fight) => {
                let over = fight.update(pressed, arena_ready, &mut out);
                self.record_result(&mut fight, &mut out);
                if over {
                    State::Done
                } else {
                    State::Fight(fight)
                }
            }
            State::Done => State::Done,
        };
        if self.list_key() != list {
            self.search.clear();
        }
        self.note_live();
        out
    }

    pub fn screen(&self) -> Screen {
        let s = match &self.state {
            State::Format { cursor } => {
                let mut items = vec![
                    "Singles (1 on 1)".into(),
                    "Doubles (2 on 2)".into(),
                    "Multi battle".into(),
                ];
                if self.trainers.is_some() {
                    items.push("Trainers".into());
                }
                Screen {
                    title: "Chao battle".into(),
                    items,
                    cursor: Some(*cursor),
                    detail: Some(
                        match cursor {
                            0 => "One Chao a side at a time.",
                            1 => "Two Chao a side at a time; some moves hit both foes.",
                            2 => {
                                "Doubles with four teams: you, an ally and two foes, each a friend \
                                 online in a Chao garden or the computer, with up to 3 Chao each. \
                                 You choose only for your own Chao."
                            }
                            _ => {
                                "A strength ladder of the game's trainers. Milestones open \
                                 one by one as you win."
                            }
                        }
                        .into(),
                    ),
                    hint: "A: choose  B: back",
                    ..Screen::default()
                }
            }
            State::Trainers(picker) => match &self.trainers {
                Some(roster) => picker.screen(&trainers::Ctx {
                    roster,
                    progress: &self.progress,
                    dex: &self.dex,
                    own: &self.own,
                    profiles: &self.own_profiles,
                }),
                None => Screen::default(),
            },
            State::Team { moves: Some(c), .. } => self.choosing_screen(c),
            State::Team {
                cursor,
                picked,
                foe,
                live,
                multi,
                ..
            } => {
                let invited = live.is_some() && self.offer_invite.is_some();
                let limit = self.team_limit(multi.is_some() || invited);
                let mut items: Vec<String> = Vec::new();
                let mut bars = Vec::new();
                for (i, e) in self.own.iter().enumerate() {
                    let mark = picked
                        .iter()
                        .position(|&p| p == i)
                        .map_or_else(|| "-".to_string(), |n| (n + 1).to_string());
                    let level = self.level_of(e.chao.identity());
                    items.push(format!("{mark} {:<7}  Lv {}", e.name, level.level));
                    bars.push((i, (level.fraction() * 1000.0) as u16));
                }
                items.push("Choose moves, type & ability".into());
                items.push(format!("Done ({} picked)", picked.len()));
                let against = foe.and_then(|id| self.trainers.as_ref()?.foes.get(&id));
                let wild = self.wild.as_ref().map(|w| w.creature.fighter.name.as_str());
                // Answering a challenge under the Level 50 rule: the stats at 50.
                let fair = live.is_some() && !self.offer_actual_levels;
                let detail = match (self.own.get(*cursor), self.own_profiles.get(*cursor)) {
                    (Some(e), Some(p)) => Some(chao_detail(
                        &self.dex,
                        &e.chao,
                        &Profile {
                            level: if fair { team::FAIR_LEVEL } else { p.level },
                            ..*p
                        },
                        self.level_of(e.chao.identity()),
                    )),
                    _ if *cursor == self.own.len() => Some(
                        "Choose what each of your Chao battles with: four of every move it has \
                         learned, one or two of the types and one of the abilities its partners \
                         gave it."
                            .into(),
                    ),
                    _ if multi.is_some() => Some(
                        "Battle with the Chao you picked, in that order: next, who has the \
                              other places."
                            .into(),
                    ),
                    _ if invited => Some(format!(
                        "Accept {}'s invite with the Chao you picked, in that order.",
                        live.as_ref().map_or("", |l| l.1.as_str())
                    )),
                    _ => Some(match (wild, against, live) {
                        (Some(name), _, _) => format!(
                            "Start with the Chao you picked, in that order, against wild {name} \
                             (singles). Win and it stays; lose or give up and it runs away."
                        ),
                        (None, Some(f), _) => format!(
                            "Start with the Chao you picked, in that order, against {} ({}).",
                            f.name,
                            if f.double { "doubles" } else { "singles" }
                        ),
                        (None, None, Some((_, name))) => format!(
                            "Accept {name}'s challenge with the Chao you picked, in that order."
                        ),
                        (None, None, None) => {
                            "Start with the Chao you picked, in that order.".into()
                        }
                    }),
                };
                Screen {
                    title: match (wild, against, live) {
                        _ if multi.is_some() => {
                            format!("Your team for the multi battle (up to {limit})")
                        }
                        (None, None, Some((_, name))) if invited => {
                            format!("Your team for {name}'s multi battle (up to {limit})")
                        }
                        (Some(name), _, _) => {
                            format!("Your team against wild {name} (up to {limit})")
                        }
                        (None, Some(f), _) => {
                            format!("Your team against {} (up to {limit})", f.name)
                        }
                        (None, None, Some((_, name))) => {
                            format!("Your team against {name} (up to {limit})")
                        }
                        (None, None, None) => format!("Your team (up to {limit})"),
                    },
                    items,
                    cursor: Some(*cursor),
                    detail,
                    hint: if wild.is_some() {
                        "A: pick or drop  B: leave"
                    } else {
                        "A: pick or drop  B: back"
                    },
                    bars,
                    ..Screen::default()
                }
            }
            State::Opponent { picked, cursor, .. } => {
                let options = self.opponents(picked);
                let disabled = options
                    .iter()
                    .enumerate()
                    .filter(|(_, (_, a))| {
                        matches!(a, Against::Live(i) if self.friends.get(*i).is_some_and(|f| f.busy))
                    })
                    .map(|(i, _)| i)
                    .collect();
                Screen {
                    title: "Battle against".into(),
                    detail: options.get(*cursor).map(|(_, a)| self.opponent_detail(a)),
                    items: options.into_iter().map(|(l, _)| l).collect(),
                    disabled,
                    cursor: Some(*cursor),
                    hint: "A: battle  B: back",
                    ..Screen::default()
                }
            }
            State::Offer { name, cursor, .. } if self.offer_invite.is_some() => {
                let invite = self.offer_invite.as_ref();
                let who = |place: usize| -> String {
                    match invite {
                        Some(i) if i.place == place => "you".into(),
                        Some(i) if i.players[place].is_some() => i.names[place].clone(),
                        _ => "the computer".into(),
                    }
                };
                let mine = invite.map_or(1, |i| i.place) / 2;
                let (ours, theirs) = (mine * 2, 2 - mine * 2);
                Screen {
                    title: format!("{name} invites you to a multi battle!"),
                    items: vec!["Accept".into(), "Decline".into()],
                    cursor: Some(*cursor),
                    detail: Some(format!(
                        "{} against {}. Accept, then pick up to {MAX_MULTI_TEAM} Chao; you choose \
                         only for yours. {}",
                        pair(&who(ours), &who(ours + 1)),
                        pair(&who(theirs), &who(theirs + 1)),
                        if self.offer_actual_levels {
                            "Rule: actual levels (each Chao at its own battle level)."
                        } else {
                            "Rule: Level 50 (every Chao at level 50)."
                        }
                    )),
                    hint: "A: choose  B: decline",
                    ..Screen::default()
                }
            }
            State::Offer {
                name,
                format,
                cursor,
                ..
            } => Screen {
                title: format!(
                    "{name} challenges you to a live {} battle!",
                    if *format == Format::Doubles {
                        "doubles"
                    } else {
                        "singles"
                    }
                ),
                items: vec!["Accept".into(), "Decline".into()],
                cursor: Some(*cursor),
                detail: Some(format!(
                    "Accept, then pick your team. {name} chooses their own moves from their game. {}",
                    if self.offer_actual_levels {
                        "Rule: actual levels (each Chao at its own battle level)."
                    } else {
                        "Rule: Level 50 (every Chao at level 50)."
                    }
                )),
                hint: "A: choose  B: decline",
                ..Screen::default()
            },
            State::Waiting(w) if w.multi => Screen {
                title: "Waiting for the others to answer...".into(),
                detail: Some(format!(
                    "{}'s multi battle starts once every place is filled.",
                    w.name
                )),
                hint: "B: leave",
                ..Screen::default()
            },
            State::Waiting(w) => Screen {
                title: if w.challenger {
                    format!("Waiting for {} to answer...", w.name)
                } else {
                    format!("Starting the battle with {}...", w.name)
                },
                detail: Some(if w.challenger {
                    format!("{} accepts and picks a team, or turns it down.", w.name)
                } else {
                    "The battle starts in a moment.".into()
                }),
                hint: if w.challenger {
                    "B: take the challenge back"
                } else {
                    "B: leave"
                },
                ..Screen::default()
            },
            State::Multi { plan, cursor } => self.plan_screen(plan, *cursor),
            State::Seat {
                plan,
                place,
                cursor,
            } => {
                let options = self.seat_options(plan);
                let disabled = options
                    .iter()
                    .enumerate()
                    .filter(|(_, h)| {
                        matches!(h, Holder::Friend { id, .. }
                            if self.friends.iter().any(|f| f.id == *id && f.busy))
                    })
                    .map(|(i, _)| i)
                    .collect();
                let detail = options.get(*cursor).map(|h| match h {
                    Holder::Friend { name, .. } => format!(
                        "{name} picks up to {MAX_MULTI_TEAM} of their Chao and chooses for them \
                         from their game."
                    ),
                    Holder::Computer { team, .. } => format!(
                        "The computer plays this place with copies of {}.",
                        team.iter()
                            .map(|e| e.name.clone())
                            .collect::<Vec<_>>()
                            .join(", ")
                    ),
                });
                let place_name = PLACE_NAMES[place.clamp(&1, &3) - 1];
                Screen {
                    title: if plan.answers.is_some() {
                        format!("{place_name}: the computer's team")
                    } else {
                        format!("{place_name} in the multi battle")
                    },
                    items: options.iter().map(Holder::label).collect(),
                    disabled,
                    cursor: Some(*cursor),
                    detail,
                    hint: "A: choose  B: back",
                    ..Screen::default()
                }
            }
            State::MultiWait { plan, cursor } => {
                let mut items: Vec<String> = (1..=3)
                    .map(|place| {
                        let holder = plan.seat(place);
                        let name = holder.map_or_else(String::new, Holder::name);
                        let answer = plan.answers.as_ref().map(|a| a[place - 1]);
                        let what = match answer {
                            Some(Answer::Accepted) => format!("{name}, accepted"),
                            Some(Answer::Refused(status)) => {
                                format!("{name}: {}", refused(status))
                            }
                            Some(Answer::Computer) => {
                                holder.map_or_else(String::new, Holder::label)
                            }
                            _ => format!("{name}, waiting..."),
                        };
                        format!("{}: {what}", PLACE_NAMES[place - 1])
                    })
                    .collect();
                items.push("Call it off".into());
                let refused_here = *cursor < 3
                    && matches!(
                        plan.answers.as_ref().map(|a| a[*cursor]),
                        Some(Answer::Refused(_))
                    );
                Screen {
                    title: "Multi battle: waiting for answers...".into(),
                    items,
                    cursor: Some(*cursor),
                    detail: Some(
                        if refused_here {
                            "A: give this place to the computer."
                        } else if *cursor == 3 {
                            "Call the battle off and go back to setting it up."
                        } else {
                            "The battle starts once every place is filled."
                        }
                        .into(),
                    ),
                    hint: "A: choose  B: call it off",
                    ..Screen::default()
                }
            }
            State::Fight(f) => f.screen(),
            State::Done => Screen::default(),
        };
        let mut s = self.searched(s);
        if let Some(n) = &self.notice {
            s.message = Some(n.clone());
        }
        s
    }
}

/// Names for the narrator: the fighters' and the dex's.
struct FightNames {
    names: [Vec<String>; 2],
    dex: Arc<Dex>,
}

impl Names for FightNames {
    fn fighter(&self, side: usize, member: usize) -> String {
        self.names
            .get(side)
            .and_then(|t| t.get(member))
            .cloned()
            .unwrap_or_else(|| "?".into())
    }
    fn move_name(&self, id: u16) -> String {
        self.dex.move_name(id)
    }
    fn ability(&self, id: u8) -> String {
        self.dex.ability_name(id)
    }
    fn type_name(&self, id: u8) -> String {
        self.dex.type_name(id)
    }
    fn is_status_move(&self, id: u16) -> bool {
        self.dex.move_data(id).is_some_and(|m| m.power == 0)
    }
}

/// A menu while choosing for the player's fighter.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Menu {
    Main(usize),
    Moves(usize),
    Targets {
        index: usize,
        cursor: usize,
    },
    /// Who move `index`, a passing move, hands on to; the cursor is on a team member.
    Pass {
        index: usize,
        cursor: usize,
    },
    /// The cursor is on a team member.
    Switch(usize),
    /// "Keep battling" (0) or "Give up" (1).
    GiveUp(usize),
}

const MAIN_ITEMS: [&str; 3] = ["Fight", "Switch", "Give up"];
const GIVE_UP_ITEMS: [&str; 2] = ["Keep battling", "Give up"];

#[derive(Clone, Debug, PartialEq, Eq)]
enum Stage {
    /// Waiting for the arena.
    Waiting,
    /// Playing beats.
    Playing,
    Choose {
        pos: usize,
        menu: Menu,
    },
    /// Picking who comes in for the player's fainted fighters; the cursor is on a team member.
    Replace {
        slots: Vec<Slot>,
        picks: Vec<(Slot, usize)>,
        cursor: usize,
    },
    /// A live battle: the player chose and waits for the friend (the step comes from the host);
    /// with the cursor of "Give up the battle?" while asking.
    Remote {
        give_up: Option<usize>,
    },
    Result,
}

/// A live battle's tie to the friend's game, through the host.
struct Link {
    battle: u16,
    opponent: u8,
    name: String,
    /// The step the next choice is for.
    step: u16,
    /// That step's seed and choices, once the host sent them.
    ready: Option<(u64, Vec<(u8, BattlePick)>)>,
    outbox: Vec<Body>,
    /// The host (or the lost connection) ended it: why (`END_*`), and the winning side.
    ended: Option<(u8, Option<usize>)>,
    /// We told the host we're done with it (its end, giving up, leaving).
    closed: bool,
    /// A multi battle: the other players still in it (place, name).
    others: Vec<(usize, String)>,
    /// What happened to the others (gone, the computer playing their places), for the result.
    notes: Vec<String>,
}

impl Link {
    /// Still going for both games.
    fn open(&self) -> bool {
        self.ended.is_none() && !self.closed
    }

    fn close(&mut self, reason: u8) {
        if self.open() {
            self.outbox.push(leave(self.battle, reason));
        }
        self.closed = true;
    }
}

/// A team member as a fight takes it: a Chao (its record, for the arena) or a trainer's
/// creature (its species, for its picture, and its XP yield).
struct Member {
    fighter: Fighter,
    record: Option<ChaoData>,
    species: Option<crate::creature::data::SpeciesKey>,
    exp_yield: u16,
}

/// How a battle ended for the player, for its XP.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Decided {
    Won,
    /// Lost, or a draw.
    Lost,
    GaveUp,
    /// A live battle that doesn't count (the games went apart, or it was cut short).
    Void,
}

struct Fight {
    battle: Battle,
    /// The player's side in `battle` (1 when they accepted a live battle). The HUD, the arena
    /// and the narrator see the player's side as side 0: `names`, `records`, `species` and the
    /// views are in that order, and events are swapped (`mirror`) before they're told.
    me: usize,
    /// A live battle (the other side is a friend's game).
    link: Option<Link>,
    /// A multi battle: the place (position on side `me`) the player plays; the computer or
    /// other players play the rest. None: the player plays their whole side.
    mine: Option<usize>,
    /// Where the player's own Chao start in side `me`'s team (a multi battle's second place).
    own_first: usize,
    ai: Rng,
    names: FightNames,
    records: [Vec<Option<ChaoData>>; 2],
    species: [Vec<Option<crate::creature::data::SpeciesKey>>; 2],
    /// The trainer battled, its challenge, and what a win brought (once recorded).
    foe: Option<u16>,
    /// Against a wild visitor.
    wild: bool,
    intro: Option<String>,
    reward: Vec<String>,
    recorded: bool,
    /// Each side's members' XP yields (trainers' creatures), in the battle's side order.
    yields: [Vec<u16>; 2],
    /// The other team's own battle levels in a live battle (whatever the level rule), for XP.
    foe_levels: Vec<u8>,
    /// A live battle under the Level 50 rule (every Chao at `team::FAIR_LEVEL`).
    fair: bool,
    /// The identities of the player's Chao on side `me` (none for a battle that gives no XP),
    /// their battle levels, and which ones were sent out.
    own_ids: Vec<u64>,
    own_levels: Vec<Level>,
    took_part: Vec<bool>,
    /// What they earned, and the result screen's lines for it.
    gains: Vec<Gain>,
    xp_lines: Vec<String>,
    /// The first of the result screen's lines shown.
    result_top: usize,
    shown: View,
    planned: View,
    queue: VecDeque<Beat>,
    /// The beat playing: frames left and played, and whether it can be skipped.
    beat: Option<(u16, u16, bool)>,
    message: Option<String>,
    stage: Stage,
    choices: Vec<Choice>,
    /// Who the chosen passing moves hand on to.
    passes: Vec<(Slot, usize)>,
    /// Frames left before the result screen takes A or B.
    result_wait: u16,
    opened: bool,
    gave_up: bool,
}

/// Why a team member can't be sent in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Unavailable {
    InBattle,
    Fainted,
    ComingIn,
}

impl Fight {
    fn new(format: Format, teams: [Vec<Member>; 2], dex: Arc<Dex>, seed: u64) -> Option<Fight> {
        Fight::from_side(format, teams, dex, seed, 0)
    }

    /// A battle between `teams` (in the battle's side order) where the player is side `me`.
    fn from_side(
        format: Format,
        teams: [Vec<Member>; 2],
        dex: Arc<Dex>,
        seed: u64,
        me: usize,
    ) -> Option<Fight> {
        let me = me.min(1);
        let shown = |s: usize| if me == 0 { s } else { mirror::side(s) };
        let names = [0, 1].map(|s| {
            teams[shown(s)]
                .iter()
                .map(|m| m.fighter.name.clone())
                .collect()
        });
        let records = [0, 1].map(|s| teams[shown(s)].iter().map(|m| m.record.clone()).collect());
        let species = [0, 1].map(|s| teams[shown(s)].iter().map(|m| m.species).collect());
        let yields = [0, 1].map(|s| teams[s].iter().map(|m| m.exp_yield).collect());
        let took_part = vec![false; teams[me].len()];
        let [mine, theirs] = teams;
        let fighters = [mine, theirs].map(|t| t.into_iter().map(|m| m.fighter).collect());
        let mut battle = match Battle::new(format, fighters, dex.chart().clone(), dex.rules(), seed)
        {
            Ok(b) => b,
            Err(e) => {
                log!("battle: can't start ({e:?})");
                return None;
            }
        };
        battle.set_move_table(dex.table());
        battle.set_sound_moves(dex.sound_moves().to_vec());
        let view = View::new(&battle);
        let view = if me == 0 { view } else { mirror::view(view) };
        Some(Fight {
            battle,
            me,
            link: None,
            mine: None,
            own_first: 0,
            ai: Rng::new(seed ^ 0x5EED_0A11),
            names: FightNames { names, dex },
            records,
            species,
            foe: None,
            wild: false,
            intro: None,
            reward: Vec::new(),
            recorded: false,
            yields,
            foe_levels: Vec::new(),
            fair: false,
            own_ids: Vec::new(),
            own_levels: Vec::new(),
            took_part,
            gains: Vec::new(),
            xp_lines: Vec::new(),
            result_top: 0,
            shown: view.clone(),
            planned: view,
            queue: VecDeque::new(),
            beat: None,
            message: None,
            stage: Stage::Waiting,
            choices: Vec::new(),
            passes: Vec::new(),
            result_wait: 0,
            opened: false,
            gave_up: false,
        })
    }

    /// The HUD's slot for a battle slot, and back.
    fn shown_slot(&self, slot: Slot) -> Slot {
        if self.me == 0 {
            slot
        } else {
            mirror::slot(slot)
        }
    }

    fn foe(&self) -> usize {
        mirror::side(self.me)
    }

    fn enqueue(&mut self, events: &[Event]) {
        for e in events {
            if let Event::SwitchedIn { at, member } = e
                && at.side == self.me
                && let Some(t) = self.took_part.get_mut(*member)
            {
                *t = true;
            }
        }
        let swapped: Vec<Event>;
        let events = if self.me == 0 {
            events
        } else {
            swapped = events.iter().map(mirror::event).collect();
            &swapped
        };
        let beats = narrate::beats(events, &mut self.planned, &self.names);
        self.queue.extend(beats);
        self.stage = Stage::Playing;
    }

    /// One frame; true once the battle is over and the arena closed.
    fn update(&mut self, pressed: u32, arena_ready: bool, out: &mut Vec<Command>) -> bool {
        if !self.opened {
            self.opened = true;
            out.push(Command::OpenArena(self.battle.format()));
            return false;
        }
        match self.stage.clone() {
            Stage::Waiting => {
                if arena_ready {
                    let events = self.battle.start();
                    self.enqueue(&events);
                    if let Some(text) = self.intro.take() {
                        self.queue.push_front(Beat {
                            text: Some(text),
                            cues: Vec::new(),
                            updates: Vec::new(),
                            frames: INTRO_FRAMES,
                        });
                    }
                    self.play(pressed, out);
                }
            }
            Stage::Playing => self.play(pressed, out),
            Stage::Choose { pos, menu } => self.choose(pos, menu, pressed, out),
            Stage::Replace {
                slots,
                picks,
                cursor,
            } => self.replace(slots, picks, cursor, pressed, out),
            Stage::Remote { give_up } => self.remote(give_up, pressed, out),
            Stage::Result => {
                // A long result scrolls.
                let more = self.result_items().len().saturating_sub(VISIBLE_ITEMS);
                let top = if pressed & buttons::UP != 0 {
                    self.result_top.saturating_sub(1)
                } else if pressed & buttons::DOWN != 0 {
                    (self.result_top + 1).min(more)
                } else {
                    self.result_top
                };
                if top != self.result_top {
                    self.result_top = top;
                    out.push(sound(Sound::Cursor));
                }
                if self.result_wait > 0 {
                    self.result_wait -= 1;
                } else if pressed & (buttons::A | buttons::B) != 0 {
                    out.push(sound(Sound::Confirm));
                    out.push(Command::CloseArena);
                    return true;
                }
            }
        }
        false
    }

    /// How the battle ended for the player, once it has.
    fn decided(&self) -> Option<Decided> {
        if self.gave_up {
            return Some(Decided::GaveUp);
        }
        // A live battle counts only when it's played to the end here: not when the games went
        // apart, and not when it was cut short (a give-up, a disconnect, a timeout).
        let ended = self.link.as_ref().and_then(|l| l.ended);
        if matches!(ended, Some((END_DESYNC, _))) {
            return Some(Decided::Void);
        }
        match self.battle.phase() {
            Phase::Over(Outcome::Won(w)) if w == self.me => Some(Decided::Won),
            Phase::Over(_) => Some(Decided::Lost),
            _ => ended.map(|_| Decided::Void),
        }
    }

    /// The XP the player's Chao that took part earn (`won`: the player won), added to `levels`:
    /// a trainer or wild battle's by its foes, a live battle's by each Chao's level step; none
    /// for other battles or Chao at level 100.
    fn earn(&mut self, won: bool, levels: &mut BTreeMap<u64, Level>) -> Vec<Gain> {
        let takers: Vec<usize> = (0..self.own_ids.len())
            .filter(|&m| self.took_part.get(self.own_first + m) == Some(&true))
            .collect();
        let k = takers.len();
        let foe_side = self.foe();
        let foes = self.battle.team(foe_side);
        let fainted: Vec<u8> = foes
            .iter()
            .enumerate()
            .filter(|(_, f)| f.fainted())
            .map(|(i, f)| self.foe_levels.get(i).copied().unwrap_or(f.level))
            .collect();
        let foe_count = foes.len();
        let foe_xp = (self.foe.is_some() || self.wild).then(|| {
            let list: Vec<levels::Foe> = foes
                .iter()
                .enumerate()
                .map(|(i, f)| levels::Foe {
                    exp_yield: self.yields[foe_side].get(i).copied().unwrap_or(0),
                    level: f.level,
                    fainted: f.fainted(),
                })
                .collect();
            if self.wild {
                list.first().map_or(0, |f| levels::wild_xp(f, won, k))
            } else {
                levels::trainer_xp(&list, won, k)
            }
        });
        let live = self.link.is_some();
        let mut out = Vec::new();
        for m in takers {
            let identity = self.own_ids[m];
            let mut level = levels.get(&identity).copied().unwrap_or_default();
            if level.level >= levels::MAX_LEVEL {
                continue;
            }
            let xp = match foe_xp {
                Some(xp) => xp,
                None if live => levels::online_xp(level.level, won, &fainted, foe_count, k),
                None => 0,
            };
            if xp == 0 {
                continue;
            }
            let grew = level.gain(xp) > 0;
            levels.insert(identity, level);
            if let Some(l) = self.own_levels.get_mut(m) {
                *l = level;
            }
            out.push(Gain {
                name: self.names.fighter(0, self.own_first + m),
                identity,
                xp,
                level: level.level,
                grew,
            });
        }
        out
    }

    /// Waiting for the friend's choice: the step once it's here, or the player giving up.
    fn remote(&mut self, give_up: Option<usize>, pressed: u32, out: &mut Vec<Command>) {
        let ready = self.link.as_mut().and_then(|l| {
            let ready = l.ready.take()?;
            l.step = l.step.wrapping_add(1);
            Some(ready)
        });
        if let Some((seed, picks)) = ready {
            let events = self.apply(seed, &picks);
            self.enqueue(&events);
            self.play(0, out);
            return;
        }
        let a = pressed & buttons::A != 0;
        let b = pressed & buttons::B != 0;
        match give_up {
            None if b => {
                out.push(sound(Sound::Confirm));
                self.stage = Stage::Remote { give_up: Some(0) };
            }
            None => {}
            Some(mut cursor) => {
                if step(&mut cursor, GIVE_UP_ITEMS.len(), pressed) {
                    out.push(sound(Sound::Cursor));
                }
                if b || (a && cursor == 0) {
                    out.push(sound(Sound::Back));
                    self.stage = Stage::Remote { give_up: None };
                } else if a {
                    self.give_up(out);
                } else {
                    self.stage = Stage::Remote {
                        give_up: Some(cursor),
                    };
                }
            }
        }
    }

    /// A step of a live battle (its seed and every player's picks).
    fn apply(&mut self, seed: u64, picks: &[(u8, BattlePick)]) -> Vec<Event> {
        if self.mine.is_some() {
            online::apply_multi_step(&mut self.battle, seed, picks)
        } else {
            online::apply_step(&mut self.battle, seed, picks)
        }
    }

    /// A seed for a step of a multi battle played here alone.
    fn local_seed(&mut self) -> u64 {
        u64::from(self.ai.next_u32()) << 32 | u64::from(self.ai.next_u32())
    }

    /// The player gives up: a loss (the friend's game is told; in a multi battle the computer
    /// plays their place).
    fn give_up(&mut self, out: &mut Vec<Command>) {
        out.push(sound(Sound::Confirm));
        self.gave_up = true;
        let winner = self.mine.is_none().then_some(1);
        out.push(Command::Cue(Cue::Result { winner }));
        self.stage = Stage::Result;
        if let Some(link) = &mut self.link {
            link.close(LEAVE_GAVE_UP);
        }
    }

    /// A message from the host about this live battle.
    fn network(&mut self, event: Incoming, out: &mut Vec<Command>) {
        let Some(link) = &mut self.link else { return };
        match event {
            Incoming::Step {
                battle,
                step,
                seed,
                picks,
            } if battle == link.battle && step == link.step && link.open() => {
                link.ready = Some((seed, picks));
            }
            Incoming::End {
                battle,
                reason,
                winner,
            } if battle == link.battle => {
                let winner = (usize::from(winner) < 2).then_some(usize::from(winner));
                self.remote_end(reason, winner, out);
            }
            Incoming::Left(id) if id == link.opponent => {
                self.remote_end(END_DISCONNECT, Some(self.me), out)
            }
            Incoming::Takeover { battle, place, why } if battle == link.battle && link.open() => {
                let Some(i) = link.others.iter().position(|o| o.0 == place) else {
                    return;
                };
                let (_, name) = link.others.remove(i);
                let note = match why {
                    END_FORFEIT => format!("{name} gave up; the computer plays their place."),
                    END_REPLACED => {
                        format!("{name} took too long to choose; the computer plays their place.")
                    }
                    _ => format!("{name} left the battle; the computer plays their place."),
                };
                log!("battle: {note}");
                link.notes.push(note.clone());
                if self.stage == Stage::Playing {
                    self.queue.push_back(Beat {
                        text: Some(note),
                        cues: Vec::new(),
                        updates: Vec::new(),
                        frames: INTRO_FRAMES,
                    });
                } else if self.stage != Stage::Result {
                    self.message = Some(note);
                }
            }
            Incoming::Lost => self.remote_end(END_LOST, None, out),
            _ => {}
        }
    }

    /// The host ended the live battle before its end (a forfeit, a disconnect, time out).
    fn remote_end(&mut self, reason: u8, winner: Option<usize>, out: &mut Vec<Command>) {
        let me = self.me;
        let Some(link) = &mut self.link else { return };
        if !link.open() || matches!(self.battle.phase(), Phase::Over(_)) {
            return;
        }
        log!("battle: live battle {} ended ({reason})", link.battle);
        link.ended = Some((reason, winner));
        self.queue.clear();
        self.beat = None;
        self.message = None;
        self.stage = Stage::Result;
        self.result_wait = RESULT_WAIT;
        let shown = winner.map(|w| usize::from(w != me));
        out.push(Command::Cue(Cue::Result { winner: shown }));
    }

    /// The session closes before the end: the friend's game is told we left.
    fn abandon(&mut self) {
        let over = matches!(self.battle.phase(), Phase::Over(_));
        if let Some(link) = &mut self.link {
            link.close(if over { LEAVE_OVER } else { LEAVE_LEFT });
        }
    }

    /// The player's choices (or nothing to choose) for a live battle's step: to the host, then
    /// wait for the friend's.
    fn send_choice(&mut self, picks: Vec<BattlePick>) {
        let check = online::checksum(&self.battle);
        if let Some(link) = &mut self.link {
            link.outbox.push(Body::BattleChoice {
                battle: link.battle,
                step: link.step,
                check,
                picks,
            });
        }
        self.choices.clear();
        self.passes.clear();
        self.message = None;
        self.stage = Stage::Remote { give_up: None };
    }

    fn play(&mut self, pressed: u32, out: &mut Vec<Command>) {
        if let Some((left, played, skippable)) = self.beat {
            let skip = skippable && played >= SKIP_AFTER && pressed & buttons::A != 0;
            if left > 1 && !skip {
                self.beat = Some((left - 1, played + 1, skippable));
                return;
            }
            self.beat = None;
        }
        match self.queue.pop_front() {
            Some(beat) => {
                for u in &beat.updates {
                    self.shown.apply(u);
                }
                out.extend(beat.cues.iter().map(|&c| Command::Cue(c)));
                if beat.text.is_some() {
                    self.message = beat.text.clone();
                }
                self.beat = Some((beat.frames.max(1), 0, beat.text.is_some()));
            }
            None => self.next(),
        }
    }

    /// After the beats: the result, replacements or the next turn's choices.
    fn next(&mut self) {
        match self.battle.phase() {
            Phase::Over(_) => {
                self.stage = Stage::Result;
                self.result_wait = RESULT_WAIT;
                if let Some(link) = &mut self.link {
                    link.close(LEAVE_OVER);
                }
            }
            Phase::Replacements => {
                let slots: Vec<Slot> = self
                    .battle
                    .pending_replacements()
                    .iter()
                    .copied()
                    .filter(|s| s.side == self.me && self.mine.is_none_or(|p| p == s.pos))
                    .collect();
                if slots.is_empty() {
                    self.do_replace(Vec::new());
                } else {
                    self.message = None;
                    let cursor = self.first_free(&[]);
                    self.stage = Stage::Replace {
                        slots,
                        picks: Vec::new(),
                        cursor,
                    };
                }
            }
            Phase::Actions => {
                self.message = None;
                self.choices.clear();
                self.passes.clear();
                self.next_choice(0);
            }
        }
    }

    /// Choosing for the player's next fighter from `pos` on, or the turn once all have chosen.
    fn next_choice(&mut self, from: usize) {
        for pos in from..self.battle.format().slots() {
            let at = Slot::new(self.me, pos);
            let ours = self.mine.is_none_or(|p| p == pos);
            if ours && self.battle.alive(at) && !self.battle.forced(at) {
                self.stage = Stage::Choose {
                    pos,
                    menu: Menu::Main(0),
                };
                return;
            }
        }
        self.submit();
    }

    fn submit(&mut self) {
        if self.link.is_some() {
            let picks = online::choice_picks(&self.choices, &self.passes);
            self.send_choice(picks);
            return;
        }
        if self.mine.is_some() {
            // A multi battle here alone: the computer plays the other places.
            let picks: Vec<(u8, BattlePick)> = online::choice_picks(&self.choices, &self.passes)
                .into_iter()
                .map(|p| (self.me as u8, p))
                .collect();
            let seed = self.local_seed();
            let events = self.apply(seed, &picks);
            self.choices.clear();
            self.passes.clear();
            self.enqueue(&events);
            return;
        }
        let mut all = self.choices.clone();
        all.extend(ai::choose(&self.battle, 1, &mut self.ai));
        for &(at, member) in &self.passes {
            self.battle.pick_pass(at, member);
        }
        let events = match self.battle.play_turn(&all) {
            Ok(events) => events,
            Err(e) => {
                // Shouldn't happen (the menus offer legal choices only): let the AI choose.
                log!("battle: turn refused ({e:?}); choosing for the player");
                let mut all = ai::choose(&self.battle, 0, &mut self.ai);
                all.extend(ai::choose(&self.battle, 1, &mut self.ai));
                self.battle.play_turn(&all).unwrap_or_default()
            }
        };
        self.choices.clear();
        self.passes.clear();
        self.enqueue(&events);
    }

    fn do_replace(&mut self, mut picks: Vec<(Slot, usize)>) {
        if self.link.is_some() {
            self.send_choice(online::replace_picks(&picks));
            return;
        }
        if self.mine.is_some() {
            let picks: Vec<(u8, BattlePick)> = online::replace_picks(&picks)
                .into_iter()
                .map(|p| (self.me as u8, p))
                .collect();
            let seed = self.local_seed();
            let events = self.apply(seed, &picks);
            self.enqueue(&events);
            return;
        }
        picks.extend(ai::replacements(&self.battle, 1));
        let events = match self.battle.replace(&picks) {
            Ok(events) => events,
            Err(e) => {
                log!("battle: replacements refused ({e:?}); choosing for the player");
                let mut picks = ai::replacements(&self.battle, 0);
                picks.extend(ai::replacements(&self.battle, 1));
                self.battle.replace(&picks).unwrap_or_default()
            }
        };
        self.enqueue(&events);
    }

    /// Why the player's team member `member` can't come in now (`taken`: picked to come in by
    /// an earlier choice), or None when it can.
    fn unavailable(&self, member: usize, taken: &[usize]) -> Option<Unavailable> {
        let f = self.battle.team(self.me).get(member)?;
        let bench = match self.mine {
            Some(pos) => self.battle.bench_for(Slot::new(self.me, pos)),
            None => self.battle.bench(self.me),
        };
        if f.fainted() {
            Some(Unavailable::Fainted)
        } else if taken.contains(&member) {
            Some(Unavailable::ComingIn)
        } else if !bench.contains(&member) {
            Some(Unavailable::InBattle)
        } else {
            None
        }
    }

    /// The members of side `me` the player sends in and switches to: their own place's in a
    /// multi battle.
    fn roster(&self) -> std::ops::Range<usize> {
        let len = self.battle.team(self.me).len();
        match self.mine {
            Some(pos) => match self.battle.split(self.me) {
                Some(first) if pos == 0 => 0..first.min(len),
                Some(first) => first.min(len)..len,
                None => 0..len,
            },
            None => 0..len,
        }
    }

    /// Moves `cursor` (a member of `roster`) up or down it; whether it moved.
    fn step_member(&self, cursor: &mut usize, pressed: u32) -> bool {
        let r = self.roster();
        let mut row = cursor
            .saturating_sub(r.start)
            .min(r.len().saturating_sub(1));
        let moved = step(&mut row, r.len(), pressed);
        *cursor = r.start + row;
        moved
    }

    /// The first member that can come in, for the cursor.
    fn first_free(&self, taken: &[usize]) -> usize {
        let r = self.roster();
        let start = r.start;
        r.into_iter()
            .find(|&m| self.unavailable(m, taken).is_none())
            .unwrap_or(start)
    }

    fn has_free(&self, taken: &[usize]) -> bool {
        self.roster().any(|m| self.unavailable(m, taken).is_none())
    }

    /// Members already picked to come in by a switch or a passing move.
    fn switching_in(&self) -> Vec<usize> {
        self.choices
            .iter()
            .filter_map(|c| match c.action {
                Action::Switch { member } => Some(member),
                _ => None,
            })
            .chain(self.passes.iter().map(|p| p.1))
            .collect()
    }

    fn chosen(&mut self, at: Slot, action: Action, out: &mut Vec<Command>) {
        out.push(sound(Sound::Confirm));
        self.message = None;
        self.choices.push(Choice { at, action });
        self.next_choice(at.pos + 1);
    }

    /// Whether move `index` of the fighter at `at` passes its state on to a member that can
    /// still come in, who the player then picks.
    fn passes_on(&self, at: Slot, index: usize) -> bool {
        self.battle
            .fighter(at)
            .and_then(|f| f.moves.get(index))
            .is_some_and(|m| m.data.effect == effects::PASS_STATE)
            && self.has_free(&self.switching_in())
    }

    /// Says why `member` can't come in.
    fn refuse(&mut self, member: usize, why: Unavailable) {
        let name = self.names.fighter(0, member);
        self.message = Some(match why {
            Unavailable::InBattle => format!("{name} is already battling."),
            Unavailable::Fainted => format!("{name} has fainted."),
            Unavailable::ComingIn => format!("{name} is already coming in."),
        });
    }

    fn choose(&mut self, pos: usize, menu: Menu, pressed: u32, out: &mut Vec<Command>) {
        let at = Slot::new(self.me, pos);
        let set = |f: &mut Fight, menu: Menu| f.stage = Stage::Choose { pos, menu };
        let a = pressed & buttons::A != 0;
        let b = pressed & buttons::B != 0;
        if a || b {
            self.message = None;
        }
        match menu {
            Menu::Main(mut cursor) => {
                if step(&mut cursor, MAIN_ITEMS.len(), pressed) {
                    out.push(sound(Sound::Cursor));
                }
                set(self, Menu::Main(cursor));
                if b {
                    // Back to the previous fighter's choice (doubles).
                    if let Some(last) = self.choices.pop() {
                        self.passes.retain(|p| p.0 != last.at);
                        out.push(sound(Sound::Back));
                        self.stage = Stage::Choose {
                            pos: last.at.pos,
                            menu: Menu::Main(0),
                        };
                    }
                } else if a {
                    match cursor {
                        0 if self.battle.usable_moves(at).is_empty() => {
                            self.chosen(at, Action::OutOfPp, out);
                        }
                        0 => {
                            out.push(sound(Sound::Confirm));
                            let first = self.battle.usable_moves(at)[0];
                            set(self, Menu::Moves(first));
                        }
                        1 if !self.battle.can_switch(at) => {
                            self.message = Some("It can't switch out now!".into());
                        }
                        1 if !self.has_free(&self.switching_in()) => {
                            self.message = Some("There's nobody to switch to.".into());
                        }
                        1 => {
                            out.push(sound(Sound::Confirm));
                            let first = self.first_free(&self.switching_in());
                            set(self, Menu::Switch(first));
                        }
                        _ => {
                            out.push(sound(Sound::Confirm));
                            set(self, Menu::GiveUp(0));
                        }
                    }
                }
            }
            Menu::Moves(mut cursor) => {
                let len = self.battle.fighter(at).map_or(0, |f| f.moves.len());
                if step(&mut cursor, len, pressed) {
                    out.push(sound(Sound::Cursor));
                }
                set(self, Menu::Moves(cursor));
                if b {
                    out.push(sound(Sound::Back));
                    set(self, Menu::Main(0));
                } else if a {
                    let pp = self
                        .battle
                        .fighter(at)
                        .and_then(|f| f.moves.get(cursor))
                        .map_or(0, |m| m.pp);
                    if pp == 0 {
                        self.message = Some("There's no PP left for that move!".into());
                    } else if !self.battle.usable(at, cursor) {
                        self.message = Some("That move can't be used now!".into());
                    } else if self.passes_on(at, cursor) {
                        out.push(sound(Sound::Confirm));
                        let first = self.first_free(&self.switching_in());
                        set(
                            self,
                            Menu::Pass {
                                index: cursor,
                                cursor: first,
                            },
                        );
                    } else if self.battle.choosable_targets(at, cursor).is_empty() {
                        self.chosen(
                            at,
                            Action::Move {
                                index: cursor,
                                target: None,
                            },
                            out,
                        );
                    } else {
                        out.push(sound(Sound::Confirm));
                        set(
                            self,
                            Menu::Targets {
                                index: cursor,
                                cursor: 0,
                            },
                        );
                    }
                }
            }
            Menu::Targets { index, mut cursor } => {
                let targets = self.battle.choosable_targets(at, index);
                if step(&mut cursor, targets.len(), pressed) {
                    out.push(sound(Sound::Cursor));
                }
                set(self, Menu::Targets { index, cursor });
                if b {
                    out.push(sound(Sound::Back));
                    set(self, Menu::Moves(index));
                } else if a && let Some(&target) = targets.get(cursor) {
                    self.chosen(
                        at,
                        Action::Move {
                            index,
                            target: Some(target),
                        },
                        out,
                    );
                }
            }
            Menu::Pass { index, mut cursor } => {
                if self.step_member(&mut cursor, pressed) {
                    out.push(sound(Sound::Cursor));
                }
                set(self, Menu::Pass { index, cursor });
                if b {
                    out.push(sound(Sound::Back));
                    set(self, Menu::Moves(index));
                } else if a {
                    match self.unavailable(cursor, &self.switching_in()) {
                        None => {
                            self.passes.push((at, cursor));
                            self.chosen(
                                at,
                                Action::Move {
                                    index,
                                    target: None,
                                },
                                out,
                            );
                        }
                        Some(why) => self.refuse(cursor, why),
                    }
                }
            }
            Menu::Switch(mut cursor) => {
                if self.step_member(&mut cursor, pressed) {
                    out.push(sound(Sound::Cursor));
                }
                set(self, Menu::Switch(cursor));
                if b {
                    out.push(sound(Sound::Back));
                    set(self, Menu::Main(1));
                } else if a {
                    match self.unavailable(cursor, &self.switching_in()) {
                        None => self.chosen(at, Action::Switch { member: cursor }, out),
                        Some(why) => self.refuse(cursor, why),
                    }
                }
            }
            Menu::GiveUp(mut cursor) => {
                if step(&mut cursor, GIVE_UP_ITEMS.len(), pressed) {
                    out.push(sound(Sound::Cursor));
                }
                set(self, Menu::GiveUp(cursor));
                if b || (a && cursor == 0) {
                    out.push(sound(Sound::Back));
                    set(self, Menu::Main(2));
                } else if a {
                    self.give_up(out);
                }
            }
        }
    }

    fn replace(
        &mut self,
        slots: Vec<Slot>,
        mut picks: Vec<(Slot, usize)>,
        mut cursor: usize,
        pressed: u32,
        out: &mut Vec<Command>,
    ) {
        let taken: Vec<usize> = picks.iter().map(|p| p.1).collect();
        if self.step_member(&mut cursor, pressed) {
            out.push(sound(Sound::Cursor));
        }
        if pressed & (buttons::A | buttons::B) != 0 {
            self.message = None;
        }
        if pressed & buttons::B != 0 && picks.pop().is_some() {
            out.push(sound(Sound::Back));
            let taken: Vec<usize> = picks.iter().map(|p| p.1).collect();
            cursor = self.first_free(&taken);
        } else if pressed & buttons::A != 0
            && let Some(&slot) = slots.get(picks.len())
        {
            match self.unavailable(cursor, &taken) {
                None => {
                    out.push(sound(Sound::Confirm));
                    picks.push((slot, cursor));
                    if picks.len() == slots.len() {
                        self.do_replace(picks);
                        return;
                    }
                    let taken: Vec<usize> = picks.iter().map(|p| p.1).collect();
                    cursor = self.first_free(&taken);
                }
                Some(why) => self.refuse(cursor, why),
            }
        }
        self.stage = Stage::Replace {
            slots,
            picks,
            cursor,
        };
    }

    fn fighter_lines(&self) -> Vec<FighterLine> {
        let mut out = Vec::new();
        for side in 0..2 {
            for pos in 0..self.battle.format().slots() {
                let slot = Slot::new(side, pos);
                let Some(member) = self.shown.at(slot) else {
                    continue;
                };
                let Some((hp, max_hp)) = self.shown.hp_at(slot) else {
                    continue;
                };
                let battle_side = if side == 0 { self.me } else { self.foe() };
                let level = self
                    .battle
                    .team(battle_side)
                    .get(member)
                    .map_or(0, |f| f.level);
                out.push(FighterLine {
                    slot,
                    member,
                    name: self.names.fighter(side, member),
                    hp,
                    max_hp,
                    ailment: self.shown.ailment[side]
                        .get(member)
                        .copied()
                        .flatten()
                        .map(narrate::ailment_tag),
                    level,
                    // Only at the Chao's own level (not under the Level 50 rule).
                    xp: (side == 0)
                        .then(|| self.own_levels.get(member.checked_sub(self.own_first)?))
                        .flatten()
                        .filter(|_| !self.fair)
                        .map(|l| (l.fraction() * 1000.0) as u16),
                });
            }
        }
        out
    }

    /// A team member in the switch and send-in lists: HP, status, and why it can't come in.
    fn member_line(&self, member: usize, taken: &[usize]) -> String {
        let f = &self.battle.team(self.me)[member];
        let mut line = format!("{}  Lv {}  HP {}/{}", f.name, f.level, f.hp, f.stats.hp);
        if let Some(a) = f.ailment.filter(|_| !f.fainted()) {
            line.push_str("  ");
            line.push_str(narrate::ailment_tag(a));
        }
        match self.unavailable(member, taken) {
            Some(Unavailable::InBattle) => line.push_str("  (battling)"),
            Some(Unavailable::Fainted) => line.push_str("  (fainted)"),
            Some(Unavailable::ComingIn) => line.push_str("  (coming in)"),
            None => {}
        }
        line
    }

    /// The team list for switching or sending in.
    fn team_list(&self, s: &mut Screen, taken: &[usize], cursor: usize) {
        let r = self.roster();
        s.items = r.clone().map(|m| self.member_line(m, taken)).collect();
        s.disabled = r
            .clone()
            .filter(|&m| self.unavailable(m, taken).is_some())
            .map(|m| m - r.start)
            .collect();
        s.cursor = Some(cursor.saturating_sub(r.start));
        if let Some(f) = self.battle.team(self.me).get(cursor) {
            let types = type_names(&self.names.dex, f.types);
            let moves: Vec<String> = f
                .moves
                .iter()
                .map(|m| format!("{} {}/{}", self.names.move_name(m.data.id), m.pp, m.data.pp))
                .collect();
            s.detail = Some(format!("{types}. {}", moves.join(", ")));
        }
    }

    fn move_item(&self, m: &MoveSlot) -> String {
        format!(
            "{}  {}  PP {}/{}",
            self.names.move_name(m.data.id),
            self.names.type_name(m.data.type_id),
            m.pp,
            m.data.pp
        )
    }

    /// Fighters left standing on `side`, of the whole team.
    fn standing(&self, side: usize) -> (usize, usize) {
        let team = self.battle.team(side);
        (team.iter().filter(|f| !f.fainted()).count(), team.len())
    }

    /// The result screen's lines: who's left standing, the turns, how a live battle ended, the
    /// trainer's reward and the XP earned.
    fn result_items(&self) -> Vec<String> {
        let ended = self.link.as_ref().and_then(|l| Some((l.ended?, &l.name)));
        let (mine, my_team) = self.standing(self.me);
        let (theirs, their_team) = self.standing(self.foe());
        let foes = if self.foe.is_some() || self.wild {
            "team"
        } else {
            "Chao"
        };
        let mut items = if self.mine.is_some() {
            vec![
                format!("Your side's Chao standing: {mine} of {my_team}"),
                format!("Foes' Chao standing: {theirs} of {their_team}"),
            ]
        } else {
            vec![
                format!("Your Chao standing: {mine} of {my_team}"),
                format!("Foe's {foes} standing: {theirs} of {their_team}"),
            ]
        };
        items.push(format!("Turns: {}", self.battle.turn()));
        if let Some(link) = &self.link {
            items.extend(link.notes.iter().cloned());
        }
        if let Some(((reason, winner), name)) = ended {
            items.push(match (reason, winner) {
                (END_REPLACED, _) => {
                    "You took too long to choose; the computer played your place.".into()
                }
                (END_TIMEOUT, Some(w)) if w == self.me => {
                    format!("{name} took too long to choose.")
                }
                (END_TIMEOUT, Some(_)) => "You took too long to choose.".into(),
                (END_TIMEOUT, None) => "Nobody chose for too long.".into(),
                _ => end_note(reason, name),
            });
        }
        items.extend(self.reward.iter().cloned());
        items.extend(self.xp_lines.iter().cloned());
        items
    }

    fn screen(&self) -> Screen {
        let mut s = Screen {
            fighters: self.fighter_lines(),
            message: self.message.clone(),
            ..Screen::default()
        };
        match &self.stage {
            Stage::Waiting | Stage::Playing => {
                s.hint = "A: next";
            }
            Stage::Result => {
                let ended = self.link.as_ref().and_then(|l| Some((l.ended?, &l.name)));
                s.title = match ended {
                    _ if self.gave_up => "You gave up.".into(),
                    Some(((_, Some(w)), _)) if w == self.me => "You won!".into(),
                    Some(((_, Some(_)), _)) => "You lost...".into(),
                    Some(((_, None), _)) => "The battle ended.".into(),
                    None => match self.battle.phase() {
                        Phase::Over(Outcome::Won(w)) if w == self.me => "You won!".into(),
                        Phase::Over(Outcome::Draw) => "It's a draw.".into(),
                        _ => "You lost...".into(),
                    },
                };
                s.items = self.result_items();
                s.scroll = self.result_top;
                s.message = None;
                s.hint = if s.items.len() > VISIBLE_ITEMS {
                    "A: back to the menu  Up/Down: more"
                } else {
                    "A: back to the menu"
                };
            }
            Stage::Remote { give_up } => {
                let name = match &self.link {
                    Some(l) if self.mine.is_some() => {
                        let names: Vec<&str> = l.others.iter().map(|o| o.1.as_str()).collect();
                        match names[..] {
                            [] => "the host".to_string(),
                            [one] => one.to_string(),
                            [ref rest @ .., last] => pair(&rest.join(", "), last),
                        }
                    }
                    Some(l) => l.name.clone(),
                    None => String::new(),
                };
                match give_up {
                    None => {
                        s.title = format!("Waiting for {name}...");
                        s.hint = "B: give up";
                    }
                    Some(cursor) => {
                        s.title = "Give up the battle?".into();
                        s.items = GIVE_UP_ITEMS.iter().map(|&i| i.into()).collect();
                        s.cursor = Some(*cursor);
                        s.hint = "A: choose  B: back";
                    }
                }
            }
            Stage::Choose { pos, menu } => {
                let at = Slot::new(self.me, *pos);
                s.choosing = Some(self.shown_slot(at));
                let name = self
                    .battle
                    .fighter(at)
                    .map_or_else(String::new, |f| f.name.clone());
                s.title = format!("What will {name} do?");
                s.hint = if self.choices.is_empty() {
                    "A: choose"
                } else {
                    "A: choose  B: back to the other Chao"
                };
                match *menu {
                    Menu::Main(cursor) => {
                        s.items = MAIN_ITEMS.iter().map(|&i| i.into()).collect();
                        s.cursor = Some(cursor);
                    }
                    Menu::Moves(cursor) => {
                        if let Some(f) = self.battle.fighter(at) {
                            s.items = f.moves.iter().map(|m| self.move_item(m)).collect();
                            s.disabled = (0..f.moves.len())
                                .filter(|&i| !self.battle.usable(at, i))
                                .collect();
                            s.detail = f
                                .moves
                                .get(cursor)
                                .map(|m| move_detail(&self.names.dex, &m.data));
                        }
                        s.cursor = Some(cursor);
                        s.hint = "A: use  B: back";
                    }
                    Menu::Targets { cursor, index } => {
                        let move_name = self
                            .battle
                            .fighter(at)
                            .and_then(|f| f.moves.get(index))
                            .map(|m| self.names.move_name(m.data.id))
                            .unwrap_or_default();
                        s.title = format!("Use {move_name} on");
                        let targets = self.battle.choosable_targets(at, index);
                        s.items = targets
                            .iter()
                            .map(|&t| {
                                let name = self.battle.fighter(t).map_or("?", |f| f.name.as_str());
                                if t.side == self.me {
                                    format!("{name} (ally)")
                                } else {
                                    format!("Foe {name}")
                                }
                            })
                            .collect();
                        s.aimed = targets.get(cursor).map(|&t| self.shown_slot(t));
                        s.cursor = Some(cursor);
                        s.hint = "A: aim  B: back";
                    }
                    Menu::Pass { cursor, .. } => {
                        s.title = format!("Who does {name} pass to?");
                        self.team_list(&mut s, &self.switching_in(), cursor);
                        s.hint = "A: pass  B: back";
                    }
                    Menu::Switch(cursor) => {
                        s.title = format!("Switch {name} for");
                        self.team_list(&mut s, &self.switching_in(), cursor);
                        s.hint = "A: switch  B: back";
                    }
                    Menu::GiveUp(cursor) => {
                        s.title = "Give up the battle?".into();
                        s.items = GIVE_UP_ITEMS.iter().map(|&i| i.into()).collect();
                        s.cursor = Some(cursor);
                        s.hint = "A: choose  B: back";
                    }
                }
            }
            Stage::Replace {
                slots,
                picks,
                cursor,
            } => {
                let taken: Vec<usize> = picks.iter().map(|p| p.1).collect();
                let fainted = slots
                    .get(picks.len())
                    .and_then(|&slot| self.shown.at(self.shown_slot(slot)))
                    .map(|m| self.names.fighter(0, m));
                s.title = match fainted {
                    Some(name) => format!("Who replaces {name}?"),
                    None => "Send in".into(),
                };
                self.team_list(&mut s, &taken, *cursor);
                s.hint = if picks.is_empty() {
                    "A: send in"
                } else {
                    "A: send in  B: back"
                };
            }
        }
        s
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::battle::data::{MoveData, Target, TypeChart};
    use crate::chao::CHAO_SIZE;

    fn chao(points: i16) -> ChaoData {
        let mut c = ChaoData {
            bytes: [0; CHAO_SIZE],
        };
        c.bytes[0x80] = 2;
        c.bytes[0x81] = 1;
        for i in 0..7 {
            c.bytes[0x28 + i] = 2;
            c.bytes[0x30 + i] = 10;
            c.bytes[0x38 + i * 2..0x3A + i * 2].copy_from_slice(&points.to_le_bytes());
        }
        // Its own DNA, so Chao with other points are other Chao.
        c.bytes[0x438 + 2..0x438 + 4].copy_from_slice(&points.to_le_bytes());
        c
    }

    fn entry(name: &str, points: i16) -> Entry {
        Entry {
            name: name.into(),
            chao: chao(points),
            moves: None,
            traits: None,
        }
    }

    fn session(own: usize, lineups: Vec<Lineup>) -> Session {
        session_with(own, lineups, &[], [1, 2, 3, 0])
    }

    /// A session whose Chao know `moves`, out of moves 1 to 3 and `extra`.
    fn session_with(
        own: usize,
        lineups: Vec<Lineup>,
        extra: &[MoveData],
        moves: [u16; 4],
    ) -> Session {
        let mut spread = MoveData::plain(3, 0, 40);
        spread.target = Target::BothFoes;
        let mut all = vec![MoveData::plain(1, 0, 60), MoveData::plain(2, 0, 90), spread];
        all.extend_from_slice(extra);
        let dex = Dex::for_tests(&all, TypeChart::default());
        let known = Moves {
            known: moves.iter().copied().filter(|&m| m != 0).collect(),
            active: moves,
        };
        let own = (0..own)
            .map(|i| Entry {
                moves: Some(known.clone()),
                traits: Some(Traits::default()),
                ..entry(&format!("Own{i}"), 200 + i as i16 * 100)
            })
            .collect();
        // The game's profile at level 50; the player's own Chao are at their levels.
        Session::open(
            Arc::new(dex),
            own,
            lineups,
            Box::new(move |_, chosen, traits| {
                let (types, ability) = traits.map_or(([0, 0], 0), |t| {
                    crate::creature_link::profile::traits_ruled(t)
                });
                Profile {
                    form: 0,
                    types,
                    ability,
                    moves: chosen.map_or(moves, |m| m.active),
                    partner: None,
                    level: team::FAIR_LEVEL,
                }
            }),
            7,
        )
    }

    /// Picks the format (`doubles`), the first `count` Chao and the last opponent team.
    fn start(s: &mut Session, doubles: bool, count: usize) -> Vec<Command> {
        let mut out = Vec::new();
        if doubles {
            out.extend(s.update(buttons::DOWN, true));
        }
        out.extend(s.update(buttons::A, true));
        for _ in 0..count {
            out.extend(s.update(buttons::A, true));
            out.extend(s.update(buttons::DOWN, true));
        }
        // Up from the top wraps to "Done".
        while !s.screen().items[s.screen().cursor.unwrap()].starts_with("Done") {
            out.extend(s.update(buttons::UP, true));
        }
        out.extend(s.update(buttons::A, true));
        while s.screen().cursor != Some(s.screen().items.len() - 1) {
            out.extend(s.update(buttons::DOWN, true));
        }
        out.extend(s.update(buttons::A, true));
        out
    }

    /// Presses `script`'s buttons in turn, one a frame, until the session is done; returns every
    /// command and whether the battle itself came to an end (not given up).
    fn run(s: &mut Session, script: &[u32], limit: usize) -> (Vec<Command>, bool) {
        let mut all = Vec::new();
        let mut over = false;
        for frame in 0..limit {
            if s.done() {
                return (all, over);
            }
            if let State::Fight(f) = &s.state {
                over |= matches!(f.battle.phase(), Phase::Over(_));
            }
            let pressed = script[frame % script.len()];
            all.extend(s.update(pressed, true));
        }
        panic!("not done after {limit} frames: {:?}", s.screen());
    }

    /// Runs frames with A until the player chooses.
    fn to_choice(s: &mut Session) -> Vec<Command> {
        let mut out = Vec::new();
        for _ in 0..10_000 {
            if matches!(&s.state, State::Fight(f) if matches!(f.stage, Stage::Choose { .. })) {
                return out;
            }
            out.extend(s.update(buttons::A, true));
        }
        panic!("no choice: {:?}", s.screen());
    }

    #[test]
    fn keyboard_alone_can_pick_a_team_and_choose_a_move_in_combat() {
        fn key(s: &mut Session, typed: Typed) -> Vec<Command> {
            let buttons = s.keyboard(&typed);
            s.update(buttons, true)
        }
        let enter = || Typed {
            enter: true,
            ..Typed::default()
        };
        let mut s = session(2, Vec::new());
        key(&mut s, enter()); // Singles.
        key(&mut s, enter()); // Own0.
        key(
            &mut s,
            Typed {
                up: true,
                ..Typed::default()
            },
        ); // Done.
        key(&mut s, enter());
        key(&mut s, enter()); // Other Chao.
        assert!(matches!(&s.state, State::Fight(_)), "{:?}", s.screen());
        to_choice(&mut s);
        key(&mut s, enter()); // Fight.
        key(
            &mut s,
            Typed {
                down: true,
                ..Typed::default()
            },
        );
        assert!(
            matches!(&s.state, State::Fight(f) if matches!(f.stage, Stage::Choose { menu: Menu::Moves(1), .. }))
        );
        let mut out = key(&mut s, enter());
        for _ in 0..1000 {
            if out
                .iter()
                .any(|c| matches!(c, Command::Cue(Cue::Attack { move_id: 2, .. })))
            {
                break;
            }
            out.extend(key(&mut s, enter()));
        }
        assert!(
            out.iter()
                .any(|c| matches!(c, Command::Cue(Cue::Attack { move_id: 2, .. })))
        );
    }

    #[test]
    fn b_on_the_first_screen_closes() {
        let mut s = session(2, Vec::new());
        assert_eq!(s.screen().items.len(), 3);
        let out = s.update(buttons::B, false);
        assert!(out.contains(&Command::Closed));
        assert!(s.done());
    }

    #[test]
    fn without_chao_there_is_nothing_to_pick() {
        let mut s = session(0, Vec::new());
        s.update(buttons::A, false);
        assert!(s.screen().message.is_some());
        assert_eq!(s.screen().items.len(), 3);
    }

    #[test]
    fn picking_a_team_and_the_opponents() {
        let lineup = Lineup {
            label: "Friend's Chao".into(),
            members: vec![entry("F0", 500), entry("F1", 500)],
        };
        let mut s = session(3, vec![lineup]);
        s.update(buttons::DOWN, false);
        s.update(buttons::A, false); // doubles
        assert_eq!(s.screen().items.len(), 5);
        // The highlighted Chao's types, moves and stats.
        let detail = s.screen().detail.unwrap();
        assert!(detail.contains("Move 1, Move 2, Move 3") && detail.contains("HP "));
        // Done with nobody picked isn't allowed.
        s.update(buttons::UP, false);
        s.update(buttons::A, false);
        assert!(s.screen().title.starts_with("Your team"));
        assert!(s.screen().message.is_some());
        s.update(buttons::DOWN, false);
        s.update(buttons::A, false); // Own0
        s.update(buttons::DOWN, false);
        s.update(buttons::DOWN, false);
        s.update(buttons::A, false); // Own2
        s.update(buttons::A, false); // Own2 again: dropped
        s.update(buttons::A, false); // and back
        // With each one's battle level and XP bar.
        assert_eq!(s.screen().items[0], "1 Own0     Lv 5");
        assert_eq!(s.screen().items[2], "2 Own2     Lv 5");
        assert_eq!(s.screen().items[3], "Choose moves, type & ability");
        assert_eq!(s.screen().bars, [(0, 0), (1, 0), (2, 0)]);
        s.update(buttons::DOWN, false);
        s.update(buttons::DOWN, false);
        s.update(buttons::A, false); // Done
        let screen = s.screen();
        assert_eq!(
            screen.items,
            ["Friend's Chao (2)", "Copies of your other Chao (1)"]
        );
        assert_eq!(screen.detail.as_deref(), Some("F0, F1"));
        // B goes back to the team with the cursor on "Done".
        s.update(buttons::B, false);
        assert_eq!(s.screen().cursor, Some(4));
        s.update(buttons::A, false);
        // The friend's team.
        let out = s.update(buttons::A, false);
        assert!(out.contains(&Command::Cue(Cue::Sound(Sound::Confirm))));
        let out = s.update(0, false);
        assert_eq!(out, [Command::OpenArena(Format::Doubles)]);
        assert!(s.in_arena());
        assert_eq!(s.record(1, 1).map(|c| c.points(0)), Some(500));
        // Nothing happens until the arena is ready.
        assert!(s.update(0, false).is_empty());
        let out = s.update(0, true);
        assert!(out.iter().any(|c| matches!(
            c,
            Command::Cue(Cue::Show {
                slot: Slot { side: 0, pos: 0 },
                member: 0
            })
        )));
    }

    #[test]
    fn choosing_the_moves_a_chao_battles_with_on_the_team_screen() {
        let extra = [MoveData::plain(4, 0, 70), MoveData::plain(5, 0, 50)];
        let mut s = session_with(2, Vec::new(), &extra, [1, 2, 3, 0]);
        let id = s.own[0].chao.identity();
        // One more it learned but doesn't battle with.
        s.own[0].moves.as_mut().unwrap().known.push(4);
        s.update(buttons::A, false); // singles
        s.update(buttons::UP, false);
        s.update(buttons::UP, false);
        assert_eq!(s.screen().items[2], "Choose moves, type & ability");
        assert!(
            s.screen()
                .detail
                .unwrap()
                .starts_with("Choose what each of your Chao battles with")
        );
        s.update(buttons::A, false);
        let screen = s.screen();
        assert_eq!(screen.title, "Moves, type & ability: which Chao?");
        assert_eq!(screen.items[0], "Own0     3 of 4 moves");
        s.update(buttons::A, false); // Own0
        let screen = s.screen();
        assert_eq!(screen.title, "Own0: moves, type & ability");
        assert_eq!(
            screen.items,
            [
                "Moves: 3 of 4 on",
                "Type: Normal (none from partners yet)",
                "Ability: none",
                "Done"
            ]
        );
        s.update(buttons::A, false); // Moves
        let screen = s.screen();
        assert_eq!(screen.title, "Own0's moves (4 at most)");
        assert_eq!(screen.items.len(), 5);
        assert!(screen.items[0].starts_with("[x] Move 1"));
        assert!(screen.items[3].starts_with("[ ] Move 4"));
        assert_eq!(screen.items[4], "Done");
        // Move 4 joins the empty slot; then four are on.
        s.update(buttons::UP, false);
        s.update(buttons::UP, false);
        let out = s.update(buttons::A, false);
        assert!(out.contains(&Command::Cue(Cue::Sound(Sound::Confirm))));
        let out = s.update(0, false);
        assert_eq!(out, [Command::Moves(id)]);
        assert_eq!(s.own_moves(id).unwrap().active, [1, 2, 3, 4]);
        assert_eq!(s.own_profiles[0].moves, [1, 2, 3, 4]);
        assert!(s.screen().items[3].starts_with("[x] Move 4"));
        // Move 1 off, and on again into its slot.
        s.update(buttons::DOWN, false);
        s.update(buttons::DOWN, false);
        s.update(buttons::A, false); // Move 1 off
        assert_eq!(s.own_moves(id).unwrap().active, [0, 2, 3, 4]);
        s.update(buttons::A, false); // and on again
        assert_eq!(s.own_moves(id).unwrap().active, [1, 2, 3, 4]);
        // Four on: one more is refused, with why.
        s.own[0].moves.as_mut().unwrap().known.push(5);
        s.update(buttons::UP, false);
        s.update(buttons::UP, false);
        s.update(buttons::A, false);
        assert_eq!(
            s.screen().message.as_deref(),
            Some("4 moves already: turn one off first.")
        );
        // B: back to its lists, the Chao, then the team on its row; Own1 kept its moves.
        s.update(buttons::B, false);
        assert_eq!(s.screen().cursor, Some(0));
        assert_eq!(s.screen().items[0], "Moves: 4 of 5 on");
        s.update(buttons::B, false);
        assert_eq!(s.screen().cursor, Some(0));
        s.update(buttons::B, false);
        assert_eq!(s.screen().cursor, Some(2));
        assert!(s.screen().title.starts_with("Your team"));
        assert_eq!(
            s.own_moves(s.own[1].chao.identity()).unwrap().active,
            [1, 2, 3, 0]
        );
    }

    /// One partner gave type 3 and ability 30, another types 5 and 7 and ability 40.
    fn traits() -> Traits {
        let mut t = Traits::default();
        let partner = |types, ability| crate::creature_link::store::Partner {
            form: 0,
            species: 1,
            types,
            moves: [0; 4],
            level: 5,
            ability,
        };
        t.learn(&partner([3, 3], 30));
        t.learn(&partner([5, 7], 40));
        t
    }

    #[test]
    fn choosing_the_type_and_ability_a_chao_battles_with() {
        let mut s = session(2, Vec::new());
        s.own[0].traits = Some(traits());
        // A copy of Own0 shares what it chooses.
        s.own[1].chao = s.own[0].chao.clone();
        s.own[1].traits = Some(traits());
        s.refresh_profiles();
        let id = s.own[0].chao.identity();
        s.update(buttons::A, false); // singles
        s.update(buttons::UP, false);
        s.update(buttons::UP, false);
        s.update(buttons::A, false); // Choose moves, type & ability
        s.update(buttons::A, false); // Own0
        assert_eq!(s.screen().items[1], "Type: Type 3");
        assert_eq!(s.screen().items[2], "Ability: Ability 30");
        s.update(buttons::DOWN, false);
        s.update(buttons::A, false); // Type
        let screen = s.screen();
        assert_eq!(screen.title, "Own0's type (2 at most)");
        assert_eq!(
            screen.items,
            ["[x] Type 3", "[ ] Type 5", "[ ] Type 7", "Done"]
        );
        assert_eq!(screen.detail.as_deref(), Some("One or two types on."));
        // A second type, then a third is refused; the last can't go off.
        s.update(buttons::DOWN, false);
        s.update(buttons::A, false);
        assert_eq!(s.update(0, false), [Command::Traits(id)]);
        assert_eq!(s.own_traits(id).unwrap().active_types, [3, 5]);
        assert_eq!(s.own_profiles[0].types, [3, 5]);
        assert_eq!(s.own_profiles[1].types, [3, 5]);
        s.update(buttons::DOWN, false);
        s.update(buttons::A, false);
        assert_eq!(
            s.screen().message.as_deref(),
            Some("2 types already: turn one off first.")
        );
        s.update(buttons::UP, false);
        s.update(buttons::UP, false);
        s.update(buttons::A, false); // Type 3 off
        assert_eq!(s.own_traits(id).unwrap().active_types, [5, 5]);
        s.update(buttons::DOWN, false);
        s.update(buttons::A, false);
        assert_eq!(
            s.screen().message.as_deref(),
            Some("A Chao needs at least one type.")
        );
        // Done: back to its lists on "Type".
        s.update(buttons::DOWN, false);
        s.update(buttons::DOWN, false);
        s.update(buttons::A, false);
        assert_eq!(s.screen().cursor, Some(1));
        assert_eq!(s.screen().items[1], "Type: Type 5");
        // The ability: none, or one of its own.
        s.update(buttons::DOWN, false);
        s.update(buttons::A, false);
        let screen = s.screen();
        assert_eq!(screen.title, "Own0's ability");
        assert_eq!(
            screen.items,
            ["( ) None", "(o) Ability 30", "( ) Ability 40", "Done"]
        );
        assert_eq!(
            screen.detail.as_deref(),
            Some("It battles without an ability.")
        );
        s.update(buttons::A, false);
        assert_eq!(s.own_traits(id).unwrap().ability, 0);
        assert_eq!(s.own_profiles[0].ability, 0);
        s.update(buttons::DOWN, false);
        s.update(buttons::DOWN, false);
        s.update(buttons::A, false);
        assert_eq!(s.own_traits(id).unwrap().ability, 40);
        assert_eq!(s.own_profiles[1].ability, 40);
        assert!(s.screen().items[2].starts_with("(o)"));
        let out = s.update(buttons::B, false);
        assert!(out.contains(&Command::Traits(id)));
        assert_eq!(s.screen().items[2], "Ability: Ability 40");
        // A Chao without any types or abilities: nothing to pick but "None" and "Done".
        assert_eq!(s.toggle_type(5, 1), Err(TraitRefused::Unknown));
        assert_eq!(s.choose_ability(0, 99), Err(TraitRefused::Unknown));
    }

    /// Types `text` on the keyboard.
    fn type_text(s: &mut Session, text: &str) -> u32 {
        s.keyboard(&Typed {
            text: text.into(),
            ..Typed::default()
        })
    }

    #[test]
    fn typing_searches_the_team_picker_and_left_right_jump() {
        let mut s = session(25, Vec::new());
        s.update(buttons::A, false); // singles
        assert!(s.typing());
        assert_eq!(s.screen().search.as_deref(), Some(""));
        // Right jumps ten, Left back; PgDn (from the keyboard) too, stopping at the end.
        s.update(buttons::RIGHT, false);
        assert_eq!(s.screen().cursor, Some(10));
        let pgdn = s.keyboard(&Typed {
            page_down: true,
            ..Typed::default()
        });
        assert_eq!(pgdn, buttons::PAGE_DOWN);
        s.update(pgdn, false);
        assert_eq!(s.screen().cursor, Some(20));
        s.update(buttons::RIGHT, false);
        assert_eq!(s.screen().cursor, Some(26));
        assert!(s.screen().items[26].starts_with("Done"));
        s.update(buttons::LEFT, false);
        assert_eq!(s.screen().cursor, Some(16));
        // "own1" matches Own1 and Own10..Own19; the rows without names stay.
        assert_eq!(type_text(&mut s, "OWN1"), 0);
        let screen = s.screen();
        assert_eq!(screen.search.as_deref(), Some("OWN1"));
        assert_eq!(screen.items.len(), 13);
        assert!(screen.items[0].contains("Own1 "));
        assert!(screen.items[1].contains("Own10"));
        assert_eq!(screen.items[11], "Choose moves, type & ability");
        // The cursor stayed on Own16, which matches.
        assert!(screen.items[screen.cursor.unwrap()].contains("Own16"));
        assert_eq!(screen.bars.len(), 11);
        s.update(buttons::A, false);
        assert!(s.screen().items[7].starts_with("1 Own16"));
        // Down from the last match goes on to the rows that always show, and wraps.
        s.update(buttons::RIGHT, false);
        assert_eq!(s.screen().cursor, Some(12));
        s.update(buttons::DOWN, false);
        assert_eq!(s.screen().cursor, Some(0));
        // Backspace deletes a letter; Esc clears the search, and then goes back.
        let erase = Typed {
            erase: true,
            erase_fresh: true,
            ..Typed::default()
        };
        assert_eq!(s.keyboard(&erase), 0);
        assert_eq!(s.screen().search.as_deref(), Some("OWN"));
        assert_eq!(s.screen().items.len(), 27);
        type_text(&mut s, "z");
        let screen = s.screen();
        assert_eq!(screen.items.len(), 2);
        let escape = Typed {
            escape: true,
            ..Typed::default()
        };
        assert_eq!(s.keyboard(&escape), 0);
        assert_eq!(s.screen().items.len(), 27);
        // Backspace on an empty search does nothing here; Esc goes back, Enter is A.
        assert_eq!(s.keyboard(&erase), 0);
        assert_eq!(s.keyboard(&escape), buttons::B);
        let enter = Typed {
            enter: true,
            ..Typed::default()
        };
        assert_eq!(s.keyboard(&enter), buttons::A);
        // Nothing matches: A picks nothing.
        type_text(&mut s, "qq");
        let screen = s.screen();
        assert_eq!((screen.items.len(), screen.cursor), (2, Some(0)));
        // Another list clears the search.
        s.update(buttons::B, false);
        assert_eq!(s.screen().title, "Chao battle");
        assert!(!s.typing());
        assert_eq!(s.screen().search, None);
        s.update(buttons::A, false);
        assert_eq!(s.screen().search.as_deref(), Some(""));
    }

    #[test]
    fn a_search_with_no_match_shows_none_and_picks_nothing() {
        let mut s = session(3, Vec::new());
        s.update(buttons::A, false); // singles
        s.update(buttons::UP, false);
        s.update(buttons::UP, false);
        s.update(buttons::A, false); // Choose moves, type & ability
        assert_eq!(s.screen().title, "Moves, type & ability: which Chao?");
        type_text(&mut s, "own2");
        let screen = s.screen();
        assert_eq!(screen.items.len(), 1);
        assert_eq!(screen.cursor, Some(0));
        type_text(&mut s, "x");
        let screen = s.screen();
        assert_eq!(screen.items, ["No matches"]);
        assert_eq!((screen.cursor, screen.detail), (None, None));
        let out = s.update(buttons::A, false);
        assert!(out.is_empty());
        assert_eq!(s.screen().items, ["No matches"]);
        // Backspace: Own2 again, and A picks it.
        s.keyboard(&Typed {
            erase: true,
            ..Typed::default()
        });
        s.update(buttons::A, false);
        assert_eq!(s.screen().title, "Own2: moves, type & ability");
        // Its move list searches by move name.
        s.update(buttons::A, false);
        type_text(&mut s, "move 3");
        assert_eq!(s.screen().items.len(), 2);
        assert!(s.screen().items[0].contains("Move 3"));
        // Its lists without names (moves, type, ability, done) don't take typing.
        s.update(buttons::B, false);
        assert!(!s.typing());
        assert_eq!(s.screen().search, None);
    }

    #[test]
    fn the_opponent_list_searches_by_team_name() {
        let lineup = |label: &str| Lineup {
            label: label.into(),
            members: vec![entry("F0", 500)],
        };
        let mut s = session(1, vec![lineup("Amy's Chao"), lineup("Bob's Chao")]);
        s.update(buttons::A, false);
        s.update(buttons::A, false);
        s.update(buttons::UP, false);
        s.update(buttons::A, false);
        assert_eq!(s.screen().title, "Battle against");
        assert_eq!(s.screen().items.len(), 3);
        type_text(&mut s, "bob");
        let screen = s.screen();
        assert_eq!(screen.items, ["Bob's Chao (1)", "Copies of your team (1)"]);
        assert_eq!(screen.cursor, Some(0));
        assert_eq!(screen.detail.as_deref(), Some("F0"));
    }

    #[test]
    fn a_full_team_moves_the_cursor_to_done() {
        let mut s = session(8, Vec::new());
        s.update(buttons::A, false);
        for _ in 0..TEAM_SIZE - 1 {
            s.update(buttons::A, false);
            s.update(buttons::DOWN, false);
        }
        assert_eq!(s.screen().cursor, Some(TEAM_SIZE - 1));
        s.update(buttons::A, false);
        let screen = s.screen();
        assert_eq!(screen.cursor, Some(9));
        assert_eq!(screen.items[9], "Done (6 picked)");
    }

    #[test]
    fn a_singles_battle_plays_to_the_end_with_a() {
        let mut s = session(2, Vec::new());
        // Singles, both Chao, copies of the team, then A all the way.
        let mut out = start(&mut s, false, 2);
        assert!(matches!(s.state, State::Fight(_)));
        let (rest, over) = run(&mut s, &[buttons::A, 0], 200_000);
        out.extend(rest);
        assert!(over);
        assert!(out.contains(&Command::CloseArena));
        assert!(
            out.iter()
                .any(|c| matches!(c, Command::Cue(Cue::Result { .. })))
        );
        assert!(
            out.iter()
                .any(|c| matches!(c, Command::Cue(Cue::Attack { .. })))
        );
    }

    #[test]
    fn the_result_screen_waits_before_taking_a() {
        let mut s = session(1, Vec::new());
        start(&mut s, false, 1);
        let at_result =
            |s: &Session| matches!(&s.state, State::Fight(f) if f.stage == Stage::Result);
        for _ in 0..200_000 {
            if at_result(&s) {
                break;
            }
            s.update(buttons::A, true);
        }
        assert!(at_result(&s));
        assert!(!s.update(buttons::A, true).contains(&Command::CloseArena));
        for _ in 0..RESULT_WAIT {
            s.update(0, true);
        }
        assert!(s.update(buttons::A, true).contains(&Command::CloseArena));
    }

    #[test]
    fn the_move_menu_shows_type_pp_and_details() {
        let mut s = session(1, Vec::new());
        start(&mut s, false, 1);
        to_choice(&mut s);
        let screen = s.screen();
        assert_eq!(screen.items, MAIN_ITEMS);
        assert_eq!(screen.choosing, Some(Slot::new(0, 0)));
        assert_eq!(screen.message, None);
        s.update(buttons::A, true);
        let screen = s.screen();
        assert_eq!(screen.items.len(), 3);
        assert!(
            screen.items[0].starts_with("Move 1  "),
            "{:?}",
            screen.items
        );
        assert!(screen.items[0].ends_with("PP 20/20"), "{:?}", screen.items);
        assert!(screen.disabled.is_empty());
        let detail = screen.detail.unwrap();
        assert!(detail.starts_with("Physical, power 60, accuracy 100"));
        s.update(buttons::UP, true);
        assert!(s.screen().detail.unwrap().contains("hits both foes"));
        // B goes back to the main menu.
        s.update(buttons::B, true);
        assert_eq!(s.screen().items, MAIN_ITEMS);
    }

    #[test]
    fn a_doubles_battle_with_targets_and_replacements_plays_out() {
        let mut s = session(5, Vec::new());
        let mut out = start(&mut s, true, 3);
        assert!(matches!(s.state, State::Fight(_)));
        out.extend(to_choice(&mut s));
        // Fight, the first move, then a target: either foe or the ally.
        s.update(buttons::A, true);
        s.update(buttons::A, true);
        let screen = s.screen();
        assert_eq!(screen.title, "Use Move 1 on");
        assert_eq!(screen.items.len(), 3);
        assert_eq!(screen.aimed, Some(Slot::new(1, 0)));
        s.update(buttons::A, true);
        // The second Chao's turn to choose; B goes back to the first.
        assert_eq!(s.screen().choosing, Some(Slot::new(0, 1)));
        s.update(buttons::B, true);
        assert_eq!(s.screen().choosing, Some(Slot::new(0, 0)));
        let (rest, over) = run(&mut s, &[buttons::A, 0], 400_000);
        out.extend(rest);
        assert!(over);
        assert!(out.contains(&Command::CloseArena));
        let shows = out
            .iter()
            .filter(|c| matches!(c, Command::Cue(Cue::Show { .. })))
            .count();
        assert!(shows >= 4, "{shows}");
    }

    #[test]
    fn switching_brings_the_other_member_in() {
        let mut s = session(2, Vec::new());
        start(&mut s, false, 2);
        to_choice(&mut s);
        s.update(buttons::DOWN, true);
        s.update(buttons::A, true);
        let screen = s.screen();
        assert!(screen.title.starts_with("Switch Own0"));
        // The whole team, the one battling greyed out, the cursor on the other.
        assert_eq!(screen.items.len(), 2);
        assert!(screen.items[0].ends_with("(battling)"));
        assert_eq!(screen.disabled, [0]);
        assert_eq!(screen.cursor, Some(1));
        // The one battling can't be picked.
        s.update(buttons::UP, true);
        s.update(buttons::A, true);
        assert_eq!(
            s.screen().message.as_deref(),
            Some("Own0 is already battling.")
        );
        s.update(buttons::DOWN, true);
        s.update(buttons::A, true);
        let mut out = Vec::new();
        for _ in 0..400 {
            out.extend(s.update(0, true));
        }
        let me = Slot::new(0, 0);
        let hide = out
            .iter()
            .position(|c| *c == Command::Cue(Cue::Hide { slot: me }));
        let show = out.iter().position(|c| {
            *c == Command::Cue(Cue::Show {
                slot: me,
                member: 1,
            })
        });
        assert!(hide.is_some() && show > hide, "{out:?}");
    }

    #[test]
    fn a_passing_move_asks_who_gets_it() {
        let pass = MoveData {
            effect: effects::PASS_STATE,
            target: Target::User,
            ..MoveData::plain(4, 0, 0)
        };
        let mut s = session_with(3, Vec::new(), &[pass], [4, 1, 0, 0]);
        start(&mut s, false, 3);
        to_choice(&mut s);
        s.update(buttons::A, true);
        s.update(buttons::A, true);
        let screen = s.screen();
        assert_eq!(screen.title, "Who does Own0 pass to?");
        assert_eq!(screen.disabled, [0]);
        assert_eq!(screen.cursor, Some(1));
        // B goes back to the moves.
        s.update(buttons::B, true);
        assert!(s.screen().items[0].starts_with("Move 4"));
        s.update(buttons::A, true);
        s.update(buttons::DOWN, true);
        s.update(buttons::A, true);
        to_choice(&mut s);
        let State::Fight(f) = &s.state else {
            panic!("no fight");
        };
        assert_eq!(f.battle.member(Slot::new(0, 0)), Some(2));
    }

    #[test]
    fn giving_up_asks_first_then_loses() {
        let mut s = session(1, Vec::new());
        start(&mut s, false, 1);
        // Play the opening beats out.
        to_choice(&mut s);
        s.update(buttons::UP, true); // wraps to "Give up"
        s.update(buttons::A, true);
        assert_eq!(s.screen().title, "Give up the battle?");
        // "Keep battling" goes back.
        s.update(buttons::A, true);
        assert_eq!(s.screen().items, MAIN_ITEMS);
        assert_eq!(s.screen().cursor, Some(2));
        s.update(buttons::A, true);
        s.update(buttons::DOWN, true);
        let out = s.update(buttons::A, true);
        assert!(out.contains(&Command::Cue(Cue::Result { winner: Some(1) })));
        let screen = s.screen();
        assert_eq!(screen.title, "You gave up.");
        assert_eq!(screen.items[0], "Your Chao standing: 1 of 1");
        let out = s.update(buttons::A, true);
        assert!(out.contains(&Command::CloseArena));
        assert!(s.done());
    }

    /// A ladder of one-creature trainers: route trainer 10, milestone 100 (milestone move 2), league 200
    /// and champion 300; leader 100's creature is weak, 200's fights doubles.
    fn roster() -> Roster {
        use crate::battle::ladder::Plan;
        use crate::battle::trainers::{Creature, Foe};
        let foe = |id: u16, double: bool| Foe {
            id,
            name: format!("T{id}"),
            double,
            party: vec![Creature {
                form: 0,
                species: 7,
                exp_yield: 64,
                fighter: Fighter::new(
                    "Mon",
                    1,
                    trainers::stats([5; 6], 0, 1, 0),
                    [0, 0],
                    0,
                    &[MoveData::plain(1, 0, 10)],
                ),
            }],
        };
        Roster {
            plan: Plan {
                route: vec![vec![10]],
                leaders: vec![100],
                league: vec![200],
                champion: 300,
            },
            foes: [(10, false), (100, false), (200, true), (300, false)]
                .into_iter()
                .map(|(id, d)| (id, foe(id, d)))
                .collect(),
            towns: vec![String::new()],
            milestone_moves: vec![Some(2)],
        }
    }

    #[test]
    fn the_trainer_lists_search_by_name_and_jump() {
        let mut s = session(1, Vec::new()).with_trainers(Arc::new(roster()), Progress::default());
        s.update(buttons::UP, false);
        s.update(buttons::A, false);
        assert!(s.screen().title.starts_with("Trainers"));
        assert_eq!(s.screen().items.len(), 5);
        // Right stops at the last row.
        s.update(buttons::RIGHT, false);
        assert_eq!(s.screen().cursor, Some(4));
        type_text(&mut s, "t200");
        let screen = s.screen();
        assert_eq!(
            screen.items,
            ["Finalist 1: T200", "Teach a milestone move (0 won)"]
        );
        assert_eq!(screen.disabled, [0, 1]);
        // Typing from the final utility row focuses the matching trainer.
        assert_eq!(s.screen().cursor, Some(0));
        // Locked: it says why, as without a search.
        s.update(buttons::A, false);
        assert!(s.screen().message.unwrap().starts_with("Beat "));
        // Into the route trainers: another list, so the search starts again.
        s.keyboard(&Typed {
            escape: true,
            ..Typed::default()
        });
        type_text(&mut s, "practice");
        assert_eq!(s.screen().items.len(), 2);
        assert_eq!(s.screen().cursor, Some(0));
        s.update(buttons::A, false);
        let screen = s.screen();
        assert_eq!(screen.search.as_deref(), Some(""));
        assert_eq!(screen.items.len(), 1);
        assert!(screen.items[0].contains("T10"));
    }

    #[test]
    fn without_trainers_the_menu_has_no_trainers_item() {
        let s = session(1, Vec::new());
        assert_eq!(
            s.screen().items,
            ["Singles (1 on 1)", "Doubles (2 on 2)", "Multi battle"]
        );
        let s = session(1, Vec::new()).with_trainers(Arc::new(roster()), Progress::default());
        assert_eq!(s.screen().items.len(), 4);
        assert_eq!(s.screen().items[3], "Trainers");
    }

    #[test]
    fn a_trainer_battle_is_won_and_recorded() {
        let mut s = session(2, Vec::new()).with_trainers(Arc::new(roster()), Progress::default());
        s.update(buttons::UP, false); // wraps to "Trainers"
        s.update(buttons::A, false);
        let screen = s.screen();
        assert!(screen.title.starts_with("Trainers"), "{screen:?}");
        // The cursor is on the first leader; the league is locked.
        assert_eq!(screen.cursor, Some(1));
        assert_eq!(screen.disabled, [2, 3, 4]);
        s.update(buttons::A, false);
        assert_eq!(
            s.screen().title,
            format!("Your team against T100 (up to {TEAM_SIZE})")
        );
        // B goes back to the trainer, A again to the team.
        s.update(buttons::B, false);
        assert_eq!(s.screen().cursor, Some(1));
        s.update(buttons::A, false);
        s.update(buttons::A, false); // Own0
        s.update(buttons::UP, false); // wraps to "Done"
        let out = s.update(buttons::A, false);
        assert!(out.contains(&Command::Cue(Cue::Sound(Sound::Confirm))));
        assert_eq!(s.update(0, false), [Command::OpenArena(Format::Singles)]);
        // Their side is the trainer's creature: a picture, no Chao record.
        assert_eq!(s.species(1, 0), Some(7));
        assert!(s.record(1, 0).is_none());
        assert!(s.record(0, 0).is_some() && s.species(0, 0).is_none());
        s.update(0, true);
        assert_eq!(s.screen().message.as_deref(), Some("T100 wants to battle!"));

        let mut out = Vec::new();
        for _ in 0..20_000 {
            if matches!(&s.state, State::Fight(f) if f.stage == Stage::Result) {
                break;
            }
            out.extend(s.update(buttons::A, true));
        }
        let screen = s.screen();
        assert_eq!(screen.title, "You won!");
        assert_eq!(screen.items[1], "Foe's team standing: 0 of 1");
        assert!(
            screen
                .items
                .contains(&"You cleared milestone 1 of 1!".to_string())
        );
        assert_eq!(out.iter().filter(|&&c| c == Command::Progress).count(), 1);
        assert!(s.progress().beaten.contains(&100));
        let (_, _) = run(&mut s, &[buttons::A], 100);
    }

    #[test]
    fn a_doubles_trainer_picks_doubles_and_a_lost_battle_records_nothing() {
        let mut progress = Progress::default();
        progress.beat(100);
        let mut s = session(2, Vec::new()).with_trainers(Arc::new(roster()), progress);
        s.update(buttons::UP, false);
        s.update(buttons::A, false);
        // The cursor is on the league trainer, now open.
        assert_eq!(s.screen().cursor, Some(2));
        s.update(buttons::A, false);
        s.update(buttons::A, false);
        s.update(buttons::UP, false);
        s.update(buttons::A, false);
        assert_eq!(s.update(0, false), [Command::OpenArena(Format::Doubles)]);
        to_choice(&mut s);
        // Give up: no progress.
        s.update(buttons::UP, true);
        s.update(buttons::A, true);
        s.update(buttons::DOWN, true);
        let out = s.update(buttons::A, true);
        assert!(!out.contains(&Command::Progress));
        let out = s.update(buttons::A, true);
        assert!(!out.contains(&Command::Progress));
        assert!(!s.progress().beaten.contains(&200));
        // Nor XP.
        assert!(!out.contains(&Command::Levels));
        assert!(s.gains().is_empty() && s.levels().is_empty());
    }

    #[test]
    fn trainer_battles_give_the_chao_that_took_part_xp() {
        let ids: Vec<u64> = (0..2).map(|i| chao(200 + i * 100).identity()).collect();
        assert_ne!(ids[0], ids[1]);
        // Own0 one XP short of level 6.
        let near = Level::new(5, levels::step(5) - 1);
        let mut s = session(2, Vec::new())
            .with_trainers(Arc::new(roster()), Progress::default())
            .with_levels(BTreeMap::from([(ids[0], near)]));
        assert_eq!(s.own_profiles[0].level, 5);
        // Without a level: a new Chao's, whatever the game's profile says.
        assert_eq!(s.own_profiles[1].level, levels::START_LEVEL);
        assert!(s.screen().detail.is_some());
        s.update(buttons::UP, false);
        s.update(buttons::A, false);
        s.update(buttons::A, false); // the first leader
        assert!(s.screen().detail.unwrap().starts_with("Lv 5 (XP 20/21)."));
        s.update(buttons::A, false); // Own0
        s.update(buttons::DOWN, false);
        s.update(buttons::A, false); // Own1
        s.update(buttons::DOWN, false);
        s.update(buttons::DOWN, false);
        s.update(buttons::A, false); // Done
        let mut out = Vec::new();
        let mut grew = false;
        for _ in 0..20_000 {
            if matches!(&s.state, State::Fight(f) if f.stage == Stage::Result) {
                break;
            }
            out.extend(s.update(buttons::A, true));
            grew |= s.screen().message.as_deref() == Some("Own0 grew to Lv 6!");
        }
        assert!(grew, "no level-up message");
        // The weak foe fell to Own0 alone: 64 * 1 / 7 = 9, times 1.5.
        assert_eq!(out.iter().filter(|&&c| c == Command::Levels).count(), 1);
        assert_eq!(s.levels()[&ids[0]], Level { level: 6, xp: 12 });
        assert!(!s.levels().contains_key(&ids[1]));
        let screen = s.screen();
        assert!(
            screen
                .items
                .contains(&"Own0 gained 13 XP and grew to Lv 6!".to_string()),
            "{screen:?}"
        );
        // The HP box shows the level and the XP toward the next.
        let box0 = &screen.fighters[0];
        assert_eq!(box0.level, 5);
        assert_eq!(
            box0.xp,
            Some((12.0 / levels::step(6) as f32 * 1000.0) as u16)
        );
        assert_eq!(screen.fighters[1].level, 1);
        assert_eq!(screen.fighters[1].xp, None);
        // A long result scrolls.
        let more = screen.items.len().saturating_sub(VISIBLE_ITEMS);
        s.update(buttons::DOWN, true);
        assert_eq!(s.screen().scroll, more.min(1));
        s.update(buttons::UP, true);
        assert_eq!(s.screen().scroll, 0);
        let (_, _) = run(&mut s, &[buttons::A], 100);
        // A rematch gives XP again (and no new progress).
        let (progress, levels) = (s.progress().clone(), s.levels().clone());
        let mut s = session(2, Vec::new())
            .with_trainers(Arc::new(roster()), progress)
            .with_levels(levels);
        s.update(buttons::UP, false);
        s.update(buttons::A, false);
        s.update(buttons::UP, false); // back up to the leader
        s.update(buttons::A, false);
        assert_eq!(
            s.screen().title,
            format!("Your team against T100 (up to {TEAM_SIZE})")
        );
        s.update(buttons::A, false);
        s.update(buttons::UP, false);
        s.update(buttons::A, false);
        let (out, _) = run(&mut s, &[buttons::A], 20_000);
        assert!(out.contains(&Command::Levels) && !out.contains(&Command::Progress));
        assert_eq!(s.levels()[&ids[0]], Level { level: 6, xp: 25 });
    }

    /// A weak wild visitor (species 9, XP yield 70, level 14) for `with_wild`.
    fn visitor() -> Creature {
        Creature {
            form: 0,
            species: 9,
            exp_yield: 70,
            fighter: Fighter::new(
                "Wildmon",
                14,
                trainers::stats([5; 6], 0, 1, 0),
                [0, 0],
                0,
                &[MoveData::plain(1, 0, 10)],
            ),
        }
    }

    #[test]
    fn a_wild_visitor_opens_at_the_team_screen_and_b_leaves_unfought() {
        let mut s = session(2, Vec::new()).with_wild(visitor(), true);
        assert!(s.is_wild() && s.busy());
        let screen = s.screen();
        assert_eq!(
            screen.title,
            format!("Your team against wild Wildmon (up to {TEAM_SIZE})")
        );
        assert_eq!(screen.hint, "A: pick or drop  B: leave");
        // A challenge meanwhile is turned down.
        s.network(Incoming::Offer {
            from: 4,
            name: "Friend".into(),
            format: Format::Singles,
            actual_levels: false,
            multi: None,
        });
        assert_eq!(s.take_outgoing(), [busy_answer(4)]);
        assert!(s.screen().title.contains("wild Wildmon"));
        let out = s.update(buttons::B, false);
        assert!(out.contains(&Command::Closed));
        assert!(s.done());
        assert_eq!(s.wild_ending(), None);
    }

    #[test]
    fn a_won_wild_battle_gives_wild_xp_and_it_stays() {
        let id = chao(200).identity();
        let mut s = session(2, Vec::new()).with_wild(visitor(), true);
        s.update(buttons::A, false); // Own0
        s.update(buttons::UP, false); // wraps to "Done"
        s.update(buttons::A, false);
        assert_eq!(s.update(0, false), [Command::OpenArena(Format::Singles)]);
        // It stands as its picture, in its shiny colours.
        assert_eq!(s.species(1, 0), Some(9));
        assert!(s.shiny(1, 0) && !s.shiny(0, 0));
        s.update(0, true);
        assert_eq!(
            s.screen().message.as_deref(),
            Some("Wild Wildmon wants to battle!")
        );
        let mut out = Vec::new();
        for _ in 0..20_000 {
            if matches!(&s.state, State::Fight(f) if f.stage == Stage::Result) {
                break;
            }
            out.extend(s.update(buttons::A, true));
        }
        assert_eq!(s.wild_ending(), Some(Ending::Won));
        // 70 * 14 / 7 = 140, with no trainer bonus; no ladder progress.
        assert!(out.contains(&Command::Levels) && !out.contains(&Command::Progress));
        let mut expect = Level::default();
        expect.gain(140);
        assert_eq!(s.levels()[&id], expect);
        let screen = s.screen();
        assert_eq!(screen.title, "You won!");
        assert!(
            screen
                .items
                .contains(&"Wildmon stays in the garden.".to_string()),
            "{screen:?}"
        );
        let (out, _) = run(&mut s, &[buttons::A], 100);
        assert!(out.contains(&Command::CloseArena));
        assert_eq!(s.wild_ending(), Some(Ending::Won));
    }

    #[test]
    fn giving_up_on_a_wild_visitor_lets_it_run_away_without_xp() {
        let mut s = session(1, Vec::new()).with_wild(visitor(), false);
        assert!(!s.shiny(1, 0));
        s.update(buttons::A, false);
        s.update(buttons::UP, false);
        s.update(buttons::A, false);
        to_choice(&mut s);
        s.update(buttons::UP, true); // wraps to "Give up"
        s.update(buttons::A, true);
        s.update(buttons::DOWN, true);
        let out = s.update(buttons::A, true);
        assert!(!out.contains(&Command::Levels));
        assert_eq!(s.wild_ending(), Some(Ending::RanAway));
        assert!(s.levels().is_empty() && s.gains().is_empty());
        assert!(s.screen().items.contains(&"Wildmon ran away.".to_string()));
    }

    #[test]
    fn the_moves_a_chao_battles_with_can_be_switched() {
        let mut s = session_with(1, Vec::new(), &[], [1, 2, 0, 0]);
        let id = s.own[0].chao.identity();
        s.own[0].moves.as_mut().unwrap().known.push(3);
        assert_eq!(s.toggle_move(0, 3), Ok(true));
        assert_eq!(s.own_profiles[0].moves, [1, 2, 3, 0]);
        assert_eq!(s.toggle_move(0, 1), Ok(false));
        assert_eq!(s.own_profiles[0].moves, [0, 2, 3, 0]);
        assert_eq!(s.toggle_move(0, 9), Err(MoveRefused::Unknown));
        assert_eq!(s.toggle_move(5, 1), Err(MoveRefused::Unknown));
        let out = s.update(0, false);
        assert_eq!(out.iter().filter(|&&c| c == Command::Moves(id)).count(), 2);
        assert_eq!(s.own_moves(id).unwrap().active, [0, 2, 3, 0]);
    }

    #[test]
    fn copies_of_a_chao_share_its_moves() {
        let mut s = session_with(2, Vec::new(), &[], [1, 2, 0, 0]);
        s.own[1].chao = s.own[0].chao.clone();
        let id = s.own[0].chao.identity();
        for e in &mut s.own {
            e.moves.as_mut().unwrap().known.push(3);
        }
        // Switched on for the second copy: the first has it too, and that's what's kept.
        assert_eq!(s.toggle_move(1, 3), Ok(true));
        assert_eq!(s.own_moves(id).unwrap().active, [1, 2, 3, 0]);
        assert_eq!(s.own_profiles[0].moves, s.own_profiles[1].moves);
    }

    #[test]
    fn a_taught_milestone_move_is_kept_and_used() {
        let mut progress = Progress::default();
        progress.beat(100);
        let mut s = session(1, Vec::new()).with_trainers(Arc::new(roster()), progress);
        s.update(buttons::UP, false);
        s.update(buttons::A, false);
        // "Teach a milestone move", the last item (from the league trainer).
        s.update(buttons::DOWN, false);
        s.update(buttons::DOWN, false);
        assert_eq!(
            s.screen().items[s.screen().cursor.unwrap()],
            "Teach a milestone move (1 won)"
        );
        s.update(buttons::A, false);
        s.update(buttons::A, false); // move 2
        s.update(buttons::A, false); // Own0 knows 1, 2, 3: refused
        assert!(s.screen().message.unwrap().contains("already knows"));
        let identity = s.own[0].chao.identity();
        // Teach move 2 again after a ladder where the Chao doesn't know it.
        let mut s = session_with(1, Vec::new(), &[], [1, 3, 0, 0])
            .with_trainers(Arc::new(roster()), s.progress().clone());
        s.update(buttons::UP, false);
        s.update(buttons::A, false);
        s.update(buttons::DOWN, false);
        s.update(buttons::DOWN, false);
        s.update(buttons::A, false);
        s.update(buttons::A, false);
        s.update(buttons::A, false);
        s.update(buttons::DOWN, false); // slot 2
        let out = s.update(buttons::A, false);
        // In place of move 3, which stays in its move list.
        assert!(out.contains(&Command::Moves(identity)));
        assert!(!out.contains(&Command::Progress));
        let moves = s.own_moves(identity).unwrap();
        assert_eq!(moves.active, [1, 2, 0, 0]);
        assert!(moves.knows(3) && moves.knows(2));
        assert_eq!(s.own_profiles[0].moves, [1, 2, 0, 0]);
    }

    /// Two players' games with a host between them: player 1's session (`s[0]`) and player
    /// 2's (`s[1]`), whose Chao are stronger.
    struct Lobby {
        host: crate::net::battles::Battles,
        s: [Session; 2],
        /// Every command each session gave.
        out: [Vec<Command>; 2],
        /// The host's clock.
        now: std::time::Instant,
    }

    /// Frames for a challenge to show before buttons answer it.
    fn see_offer(s: &mut Session) {
        for _ in 0..OFFER_GRACE {
            s.update(0, false);
        }
    }

    const IDS: [u8; 2] = [1, 2];

    impl Lobby {
        fn new(own: usize) -> Lobby {
            let mut host = crate::net::battles::Battles::default();
            host.joined(1);
            host.joined(2);
            let mut s = [session(own, Vec::new()), session(own, Vec::new())];
            for (i, e) in s[1].own.iter_mut().enumerate() {
                e.chao = chao(900 + i as i16 * 100);
            }
            for (i, s) in s.iter_mut().enumerate() {
                s.set_friends(vec![Friend {
                    id: IDS[1 - i],
                    name: format!("P{}", IDS[1 - i]),
                    busy: false,
                }]);
            }
            Lobby {
                host,
                s,
                out: [Vec::new(), Vec::new()],
                now: std::time::Instant::now(),
            }
        }

        /// Messages both ways until there are none.
        fn pump(&mut self) {
            let now = self.now;
            loop {
                let mut out = Vec::new();
                for (i, s) in self.s.iter_mut().enumerate() {
                    for body in s.take_outgoing() {
                        assert!(self.host.observe(IDS[i], &body, now, &mut out));
                    }
                }
                self.host.tick(now, &mut out);
                if out.is_empty() {
                    return;
                }
                self.deliver(out);
            }
        }

        fn deliver(&mut self, out: Vec<crate::net::referee::Out>) {
            use crate::net::referee::To;
            for o in out {
                let To::One(to) = o.to else { panic!("{o:?}") };
                let i = IDS.iter().position(|&id| id == to).unwrap();
                let event = online::incoming(o.body, |id| format!("P{id}")).unwrap();
                self.s[i].network(event);
            }
        }

        /// A frame of both games with these buttons.
        fn frame(&mut self, pressed: [u32; 2]) {
            for ((s, out), pressed) in self.s.iter_mut().zip(&mut self.out).zip(pressed) {
                out.extend(s.update(pressed, true));
            }
            self.pump();
        }

        /// Player 1 challenges player 2 with `count` Chao, and player 2 accepts with theirs.
        fn start(&mut self, doubles: bool, count: usize) {
            // Format, team, then the friend at the top of the opponents.
            let s = &mut self.s[0];
            if doubles {
                s.update(buttons::DOWN, false);
            }
            s.update(buttons::A, false);
            for _ in 0..count {
                s.update(buttons::A, false);
                s.update(buttons::DOWN, false);
            }
            while !s.screen().items[s.screen().cursor.unwrap()].starts_with("Done") {
                s.update(buttons::UP, false);
            }
            s.update(buttons::A, false);
            assert_eq!(s.screen().items[0], "P2 (live)");
            s.update(buttons::A, false);
            assert_eq!(s.screen().title, "Waiting for P2 to answer...");
            self.pump();
            let s = &mut self.s[1];
            assert_eq!(
                s.screen().title,
                format!(
                    "P1 challenges you to a live {} battle!",
                    if doubles { "doubles" } else { "singles" }
                )
            );
            see_offer(s);
            s.update(buttons::A, false);
            assert_eq!(s.screen().title, "Your team against P1 (up to 6)");
            for _ in 0..count {
                s.update(buttons::A, false);
                s.update(buttons::DOWN, false);
            }
            while !s.screen().items[s.screen().cursor.unwrap()].starts_with("Done") {
                s.update(buttons::UP, false);
            }
            s.update(buttons::A, false);
            self.pump();
            for s in &self.s {
                assert!(matches!(&s.state, State::Fight(f) if f.link.is_some()));
            }
        }

        fn fight(&self, i: usize) -> &Fight {
            match &self.s[i].state {
                State::Fight(f) => f,
                _ => panic!("no fight: {:?}", self.s[i].screen()),
            }
        }
    }

    /// Every Chao in the lobby at `level`.
    fn at_level(l: &mut Lobby, level: u8) {
        for s in &mut l.s {
            let ids: Vec<u64> = s.own.iter().map(|e| e.chao.identity()).collect();
            s.levels = ids
                .into_iter()
                .map(|id| (id, Level::new(level, 0)))
                .collect();
            s.refresh_profiles();
        }
    }

    #[test]
    fn a_live_battle_plays_the_same_in_both_games() {
        for doubles in [false, true] {
            let mut l = Lobby::new(3);
            at_level(&mut l, 30);
            l.start(doubles, 3);
            assert_eq!((l.fight(0).me, l.fight(1).me), (0, 1));
            // Each game shows its own Chao on its side: player 2's stronger ones.
            l.frame([0, 0]);
            l.frame([0, 0]);
            assert_eq!(l.s[0].record(0, 0).map(|c| c.points(0)), Some(200));
            assert_eq!(l.s[1].record(0, 0).map(|c| c.points(0)), Some(900));
            assert_eq!(l.s[1].record(1, 0).map(|c| c.points(0)), Some(200));
            let mut waited = false;
            for _ in 0..400_000 {
                if l.s
                    .iter()
                    .all(|s| matches!(&s.state, State::Fight(f) if f.stage == Stage::Result))
                {
                    break;
                }
                waited |= l.s[1].screen().title == "Waiting for P1...";
                l.frame([buttons::A, buttons::A]);
            }
            assert!(waited);
            let (a, b) = (l.fight(0), l.fight(1));
            assert!(matches!(a.battle.phase(), Phase::Over(_)));
            assert_eq!(online::checksum(&a.battle), online::checksum(&b.battle));
            assert_eq!(a.battle.turn(), b.battle.turn());
            let titles = [l.s[0].screen().title, l.s[1].screen().title];
            assert!(
                titles == ["You won!", "You lost..."]
                    || titles == ["You lost...", "You won!"]
                    || titles == ["It's a draw.", "It's a draw."],
                "{titles:?}"
            );
            // The stronger team won.
            assert_eq!(titles[1], "You won!");
            // Both earned XP, the winner more.
            let xp = |i: usize| l.s[i].gains().iter().map(|g| g.xp).sum::<u32>();
            assert!(xp(1) > xp(0) && xp(0) > 0, "{} {}", xp(1), xp(0));
            assert!(l.out.iter().all(|o| o.contains(&Command::Levels)));
            // Both say so in the arena, from their own side.
            for (i, out) in l.out.iter().enumerate() {
                let won = usize::from(i == 0);
                assert!(
                    out.contains(&Command::Cue(Cue::Result { winner: Some(won) })),
                    "{i}"
                );
            }
            // Leaving: both were told the host the battle ended; nothing more goes out.
            for _ in 0..RESULT_WAIT + 1 {
                l.frame([0, 0]);
            }
            l.frame([buttons::A, buttons::A]);
            assert!(l.s.iter().all(Session::done));
            for s in &mut l.s {
                s.abandon();
                assert!(s.take_outgoing().is_empty());
            }
        }
    }

    /// Frames with A in game 1 (`chooser`) until game `waiter` waits for it.
    fn until_waiting(l: &mut Lobby, waiter: usize) {
        for _ in 0..10_000 {
            if matches!(&l.s[waiter].state, State::Fight(f) if matches!(f.stage, Stage::Remote { .. }))
            {
                return;
            }
            let mut pressed = [0, 0];
            pressed[waiter] = buttons::A;
            l.frame(pressed);
        }
        panic!("not waiting");
    }

    #[test]
    fn giving_up_or_leaving_a_live_battle_lets_the_friend_win() {
        // Player 1 gives up while waiting for player 2's choice.
        let mut l = Lobby::new(2);
        l.start(false, 2);
        until_waiting(&mut l, 0);
        assert_eq!(l.s[0].screen().title, "Waiting for P2...");
        l.frame([buttons::B, 0]);
        assert_eq!(l.s[0].screen().title, "Give up the battle?");
        l.frame([buttons::DOWN, 0]);
        l.frame([buttons::A, 0]);
        assert_eq!(l.s[0].screen().title, "You gave up.");
        let screen = l.s[1].screen();
        assert_eq!(screen.title, "You won!");
        assert!(
            screen.items.contains(&"P1 gave up.".to_string()),
            "{screen:?}"
        );
        // No XP for either: a battle cut short doesn't count.
        l.frame([0, 0]);
        for i in 0..2 {
            assert!(l.s[i].gains().is_empty() && !l.out[i].contains(&Command::Levels));
        }
        // Player 2 leaves the area mid-battle (its session is dropped).
        let mut l = Lobby::new(2);
        l.start(false, 1);
        until_waiting(&mut l, 1);
        l.s[1].abandon();
        l.pump();
        assert_eq!(l.s[0].screen().title, "You won!");
        l.frame([0, 0]);
        assert!(l.s[0].gains().is_empty() && !l.out[0].contains(&Command::Levels));
        // Player 2's game drops out of the lobby.
        let mut l = Lobby::new(2);
        l.start(true, 2);
        l.host.left(2);
        l.pump();
        let screen = l.s[0].screen();
        assert_eq!(screen.title, "You won!");
        assert!(screen.items.contains(&"P2 left the battle.".to_string()));
        // The arena shows it with the next frame.
        l.frame([0, 0]);
        assert!(l.out[0].contains(&Command::Cue(Cue::Result { winner: Some(0) })));
        // The connection to the host is lost: no winner.
        let mut l = Lobby::new(1);
        l.start(false, 1);
        l.s[1].network(Incoming::Lost);
        assert_eq!(l.s[1].screen().title, "The battle ended.");
        // It doesn't count.
        l.s[1].update(0, true);
        assert!(l.s[1].gains().is_empty());
    }

    #[test]
    fn the_level_rule_goes_with_the_challenge_to_both_teams() {
        // The rule's row switches.
        let mut l = Lobby::new(1);
        let s = &mut l.s[0];
        s.update(buttons::A, false);
        s.update(buttons::A, false);
        s.update(buttons::UP, false);
        s.update(buttons::A, false);
        assert_eq!(s.screen().items[1], "Live rule: Level 50");
        s.update(buttons::DOWN, false);
        assert!(s.screen().detail.unwrap().contains("level 50"));
        s.update(buttons::A, false);
        assert_eq!(s.screen().items[1], "Live rule: actual levels");
        assert!(s.take_outgoing().is_empty());
        for actual in [false, true] {
            // Player 1's Chao at level 12, player 2's at 70.
            let mut l = Lobby::new(1);
            for (s, level) in l.s.iter_mut().zip([12, 70]) {
                let id = s.own[0].chao.identity();
                s.levels.insert(id, Level::new(level, 0));
                s.refresh_profiles();
            }
            l.s[0].actual_levels = actual;
            l.start(false, 1);
            assert_eq!(l.s[1].offer_actual_levels, actual);
            let want = if actual { [12, 70] } else { [50, 50] };
            for i in 0..2 {
                let b = &l.fight(i).battle;
                assert_eq!([b.team(0)[0].level, b.team(1)[0].level], want, "{actual}");
            }
            assert_eq!(
                online::checksum(&l.fight(0).battle),
                online::checksum(&l.fight(1).battle)
            );
            // Each box shows the level fought at.
            l.frame([0, 0]);
            l.frame([0, 0]);
            for _ in 0..200 {
                if !l.s[0].screen().fighters.is_empty() {
                    break;
                }
                l.frame([0, 0]);
            }
            let boxes = l.s[0].screen().fighters;
            assert_eq!(boxes[0].level, want[0]);
            // The XP bar only at the Chao's own level.
            assert_eq!(boxes[0].xp.is_some(), actual);
        }
    }

    #[test]
    fn a_challenge_that_opened_the_menu_closes_it_when_it_ends() {
        let offer = || Incoming::Offer {
            from: 1,
            name: "P1".into(),
            format: Format::Singles,
            actual_levels: false,
            multi: None,
        };
        // Turned down.
        let mut s = session(1, Vec::new());
        s.set_opened_by_offer();
        s.network(offer());
        see_offer(&mut s);
        let out = s.update(buttons::B, false);
        assert!(out.contains(&Command::Released));
        assert_eq!(s.take_farewell(), None);
        // Taken back: the notice goes with it.
        let mut s = session(1, Vec::new());
        s.set_opened_by_offer();
        s.network(offer());
        s.network(Incoming::End {
            battle: 0,
            reason: END_WITHDRAWN,
            winner: crate::net::protocol::NO_WINNER,
        });
        assert!(s.update(0, false).contains(&Command::Released));
        assert_eq!(
            s.take_farewell().as_deref(),
            Some("P1 took the challenge back.")
        );
        // Opened by the player: back to the format menu.
        let mut s = session(1, Vec::new());
        s.network(offer());
        see_offer(&mut s);
        assert!(!s.update(buttons::B, false).contains(&Command::Released));
        assert!(matches!(s.state, State::Format { .. }));
    }

    #[test]
    fn a_live_battle_that_stopped_part_way_still_tells_the_host() {
        let mut l = Lobby::new(1);
        l.start(false, 1);
        let State::Fight(f) = &l.s[1].state else {
            panic!("no fight");
        };
        let battle = f.link.as_ref().unwrap().battle;
        // A panic in its update leaves the state at `Done`.
        l.s[1].state = State::Done;
        l.s[1].abandon();
        let sent = l.s[1].take_outgoing();
        assert_eq!(
            sent,
            [Body::BattleLeave {
                battle,
                reason: LEAVE_LEFT
            }]
        );
        let mut out = Vec::new();
        l.host.observe(IDS[1], &sent[0], l.now, &mut out);
        l.deliver(out);
        assert!(!matches!(&l.s[0].state, State::Fight(f) if f.link.as_ref().unwrap().open()));
    }

    #[test]
    fn a_challenge_can_be_turned_down_or_taken_back() {
        // Declined.
        let mut l = Lobby::new(2);
        l.s[0].update(buttons::A, false);
        l.s[0].update(buttons::A, false);
        l.s[0].update(buttons::UP, false);
        l.s[0].update(buttons::A, false);
        l.s[0].update(buttons::A, false);
        l.pump();
        // A press as it comes doesn't answer it.
        l.s[1].update(buttons::A, false);
        assert!(l.s[1].screen().title.starts_with("P1 challenges you"));
        assert!(l.s[1].take_outgoing().is_empty());
        see_offer(&mut l.s[1]);
        l.s[1].update(buttons::B, false);
        l.pump();
        let screen = l.s[0].screen();
        assert_eq!(screen.title, "Battle against");
        assert_eq!(
            screen.message.as_deref(),
            Some("P2 turned the battle down.")
        );
        assert!(!l.s[1].busy());
        // Taken back before the answer (a while later: the host waits between challenges).
        l.now += crate::net::battles::CHALLENGE_GAP;
        l.s[0].update(buttons::A, false);
        l.pump();
        assert!(l.s[1].screen().title.starts_with("P1 challenges you"));
        l.s[0].update(buttons::B, false);
        l.pump();
        assert_eq!(
            l.s[1].screen().message.as_deref(),
            Some("P1 took the challenge back.")
        );
        // A game in another battle turns it down as busy.
        let mut l = Lobby::new(2);
        start(&mut l.s[1], false, 1);
        l.s[0].update(buttons::A, false);
        l.s[0].update(buttons::A, false);
        l.s[0].update(buttons::UP, false);
        l.s[0].update(buttons::A, false);
        l.s[0].update(buttons::A, false);
        l.pump();
        assert_eq!(
            l.s[0].screen().message.as_deref(),
            Some("P2 can't battle right now.")
        );
        // A busy friend can't be picked.
        let mut s = session(1, Vec::new());
        s.set_friends(vec![Friend {
            id: 4,
            name: "Busy".into(),
            busy: true,
        }]);
        s.update(buttons::A, false);
        s.update(buttons::A, false);
        s.update(buttons::UP, false);
        s.update(buttons::A, false);
        assert_eq!(s.screen().items[0], "Busy (live, busy)");
        assert_eq!(s.screen().disabled, [0]);
        s.update(buttons::A, false);
        assert!(s.take_outgoing().is_empty());
        assert_eq!(s.screen().title, "Battle against");
    }

    // ---- multi battles ----

    /// Picks the first `count` Chao on the team screen showing and chooses "Done".
    fn pick_team(s: &mut Session, count: usize) {
        for _ in 0..count {
            s.update(buttons::A, false);
            s.update(buttons::DOWN, false);
        }
        while !s.screen().items[s.screen().cursor.unwrap()].starts_with("Done") {
            s.update(buttons::UP, false);
        }
        s.update(buttons::A, false);
    }

    /// From the format menu to the multi battle's setup with the first `count` Chao.
    fn to_plan(s: &mut Session, count: usize) {
        s.update(buttons::DOWN, false);
        s.update(buttons::DOWN, false);
        s.update(buttons::A, false);
        assert_eq!(
            s.screen().title,
            format!("Your team for the multi battle (up to {MAX_MULTI_TEAM})")
        );
        pick_team(s, count);
        assert_eq!(s.screen().title, "Multi battle");
    }

    /// Moves the cursor of the list showing to row `row`.
    fn cursor_to(s: &mut Session, row: usize) {
        for _ in 0..20 {
            if s.screen().cursor == Some(row) {
                return;
            }
            s.update(buttons::DOWN, false);
        }
        panic!("no row {row}: {:?}", s.screen());
    }

    #[test]
    fn a_multi_battle_with_the_computer_plays_here_alone() {
        // A team is at most 3: the third goes straight to "Done", a fourth is refused.
        let mut s = session(4, Vec::new());
        s.update(buttons::DOWN, false);
        s.update(buttons::DOWN, false);
        s.update(buttons::A, false);
        for _ in 0..3 {
            s.update(buttons::A, false);
            s.update(buttons::DOWN, false);
        }
        assert_eq!(s.screen().cursor, Some(0));
        cursor_to(&mut s, 3);
        s.update(buttons::A, false);
        assert_eq!(
            s.screen().message.as_deref(),
            Some("A team has at most 3 Chao.")
        );
        let mut s = session(4, Vec::new());
        to_plan(&mut s, 3);
        let screen = s.screen();
        assert_eq!(screen.items[0], "Your team: Own0, Own1, Own2");
        assert_eq!(
            screen.items[1..4],
            [
                "Ally: Computer: Copies of your other Chao (1)",
                "Foe 1: Computer: Copies of your other Chao (1)",
                "Foe 2: Computer: Copies of your other Chao (1)",
            ]
        );
        assert_eq!(screen.items[PLAN_START], "Start");
        assert!(s.take_outgoing().is_empty());
        cursor_to(&mut s, PLAN_START);
        s.update(buttons::A, false);
        let State::Fight(f) = &s.state else {
            panic!("no fight: {:?}", s.screen());
        };
        assert_eq!(
            (f.mine, f.battle.split(0), f.battle.split(1)),
            (Some(0), Some(3), Some(1))
        );
        let mut over = false;
        for _ in 0..200_000 {
            if s.done() {
                break;
            }
            if let State::Fight(f) = &s.state {
                over |= matches!(f.battle.phase(), Phase::Over(_));
                // The player chooses only for their own place.
                if let Stage::Choose { pos, .. } = f.stage {
                    assert_eq!(pos, 0);
                    assert_eq!(s.screen().choosing, Some(Slot::new(0, 0)));
                }
                if let Stage::Replace { slots, .. } = &f.stage {
                    assert!(slots.iter().all(|s| s.pos == 0));
                    // Only their own Chao are in the list.
                    assert_eq!(s.screen().items.len(), 3);
                }
            }
            s.update(buttons::A, true);
        }
        assert!(s.done() && over);
        // Nothing went to a host, and a battle with copies gives no XP.
        assert!(s.take_outgoing().is_empty());
        assert!(s.gains().is_empty());
    }

    /// Four players' games with a host between them (`IDS4`), each with the others as friends
    /// in a Chao garden.
    struct Lobby4 {
        host: crate::net::battles::Battles,
        s: Vec<Session>,
        out: Vec<Vec<Command>>,
        now: std::time::Instant,
    }

    const IDS4: [u8; 4] = [1, 2, 3, 4];

    impl Lobby4 {
        fn new() -> Lobby4 {
            let mut host = crate::net::battles::Battles::default();
            let mut s: Vec<Session> = Vec::new();
            for (i, &id) in IDS4.iter().enumerate() {
                host.joined(id);
                let mut game = session(3, Vec::new());
                for (j, e) in game.own.iter_mut().enumerate() {
                    e.chao = chao(200 + i as i16 * 200 + j as i16 * 50);
                }
                game.set_friends(
                    IDS4.iter()
                        .filter(|&&o| o != id)
                        .map(|&o| Friend {
                            id: o,
                            name: format!("P{o}"),
                            busy: false,
                        })
                        .collect(),
                );
                s.push(game);
            }
            Lobby4 {
                host,
                s,
                out: vec![Vec::new(); 4],
                now: std::time::Instant::now(),
            }
        }

        fn pump(&mut self) {
            let now = self.now;
            loop {
                let mut out = Vec::new();
                for (i, s) in self.s.iter_mut().enumerate() {
                    for body in s.take_outgoing() {
                        assert!(self.host.observe(IDS4[i], &body, now, &mut out));
                    }
                }
                self.host.tick(now, &mut out);
                if out.is_empty() {
                    return;
                }
                for o in out {
                    use crate::net::referee::To;
                    let To::One(to) = o.to else { panic!("{o:?}") };
                    let i = IDS4.iter().position(|&id| id == to).unwrap();
                    let event = online::incoming(o.body, |id| format!("P{id}")).unwrap();
                    self.s[i].network(event);
                }
            }
        }

        /// A frame of every game, pressing `pressed` in the ones in `who`.
        fn frame(&mut self, pressed: u32, who: &[usize]) {
            for (i, (s, out)) in self.s.iter_mut().zip(&mut self.out).enumerate() {
                let p = if who.contains(&i) { pressed } else { 0 };
                out.extend(s.update(p, true));
            }
            self.pump();
        }

        fn fight(&self, i: usize) -> &Fight {
            match &self.s[i].state {
                State::Fight(f) => f,
                _ => panic!("no fight: {:?}", self.s[i].screen()),
            }
        }

        /// Player 1 sets up a multi battle: player 2 its ally, player 3 foe 1, the computer foe
        /// 2, and sends the invites.
        fn invite(&mut self) {
            let s = &mut self.s[0];
            to_plan(s, 2);
            // The ally: the first friend.
            s.update(buttons::A, false);
            assert_eq!(s.screen().title, "Ally in the multi battle");
            assert_eq!(
                s.screen().items[..3],
                ["P2 (live)", "P3 (live)", "P4 (live)"]
            );
            s.update(buttons::A, false);
            // Foe 1: player 2 has a place already; player 3.
            s.update(buttons::DOWN, false);
            s.update(buttons::A, false);
            s.update(buttons::A, false);
            assert_eq!(
                s.screen().message.as_deref(),
                Some("P2 already has a place.")
            );
            s.update(buttons::DOWN, false);
            s.update(buttons::A, false);
            let screen = s.screen();
            assert_eq!(screen.items[1..3], ["Ally: P2 (live)", "Foe 1: P3 (live)"]);
            assert_eq!(screen.items[PLAN_START], "Send the invites");
            cursor_to(s, PLAN_START);
            s.update(buttons::A, false);
            assert_eq!(s.screen().title, "Multi battle: waiting for answers...");
            self.pump();
        }

        /// Player `i` accepts the invite with `count` Chao.
        fn accept(&mut self, i: usize, count: usize) {
            let s = &mut self.s[i];
            assert_eq!(s.screen().title, "P1 invites you to a multi battle!");
            see_offer(s);
            s.update(buttons::A, false);
            assert_eq!(
                s.screen().title,
                format!("Your team for P1's multi battle (up to {MAX_MULTI_TEAM})")
            );
            pick_team(s, count);
            self.pump();
        }

        /// Frames with A in every game until all show the result.
        fn play_out(&mut self, who: &[usize]) {
            for _ in 0..400_000 {
                if who.iter().all(
                    |&i| matches!(&self.s[i].state, State::Fight(f) if f.stage == Stage::Result),
                ) {
                    return;
                }
                self.frame(buttons::A, who);
            }
            panic!("not over");
        }
    }

    #[test]
    fn a_multi_battle_plays_the_same_in_every_game() {
        let mut l = Lobby4::new();
        l.invite();
        let detail = l.s[2].screen().detail.unwrap();
        assert!(
            detail.starts_with("you and the computer against P1 and P2."),
            "{detail}"
        );
        assert!(detail.contains("Rule: Level 50"));
        assert!(l.s[0].busy() && l.s[1].busy() && !l.s[3].busy());
        l.accept(1, 3);
        assert_eq!(l.s[1].screen().title, "Waiting for the others to answer...");
        assert_eq!(l.s[0].screen().items[0], "Ally: P2, accepted");
        l.accept(2, 1);
        for i in 0..3 {
            assert!(
                matches!(&l.s[i].state, State::Fight(f) if f.link.is_some()),
                "{i}"
            );
        }
        // Each game plays its own place, its side on the left.
        let places: Vec<(usize, Option<usize>, usize)> = (0..3)
            .map(|i| (l.fight(i).me, l.fight(i).mine, l.fight(i).own_first))
            .collect();
        assert_eq!(places, [(0, Some(0), 0), (0, Some(1), 2), (1, Some(0), 0)]);
        let sizes: Vec<usize> = (0..2)
            .map(|side| l.fight(0).battle.team(side).len())
            .collect();
        assert_eq!(sizes, [5, 2]);
        let mut waited = Vec::new();
        for _ in 0..400_000 {
            if (0..3).all(|i| matches!(&l.s[i].state, State::Fight(f) if f.stage == Stage::Result))
            {
                break;
            }
            let titles: Vec<String> = (0..3).map(|i| l.s[i].screen().title).collect();
            for t in titles {
                if t.starts_with("Waiting for") && !waited.contains(&t) {
                    waited.push(t);
                }
            }
            for i in 0..3 {
                if let State::Fight(f) = &l.s[i].state
                    && let Stage::Choose { pos, .. } = f.stage
                {
                    assert_eq!(Some(pos), f.mine);
                }
            }
            l.frame(buttons::A, &[0, 1, 2]);
        }
        assert!(
            waited.contains(&"Waiting for P2 and P3...".to_string()),
            "{waited:?}"
        );
        assert!(
            waited.contains(&"Waiting for P1 and P2...".to_string()),
            "{waited:?}"
        );
        let checks: Vec<u32> = (0..3)
            .map(|i| online::checksum(&l.fight(i).battle))
            .collect();
        assert!(checks.iter().all(|&c| c == checks[0]));
        assert!(matches!(l.fight(0).battle.phase(), Phase::Over(_)));
        let titles: Vec<String> = (0..3).map(|i| l.s[i].screen().title).collect();
        assert_eq!(titles[0], titles[1]);
        assert!(
            titles == ["You won!", "You won!", "You lost..."]
                || titles == ["You lost...", "You lost...", "You won!"]
                || titles.iter().all(|t| t == "It's a draw."),
            "{titles:?}"
        );
        // Each earned XP for their own Chao.
        for i in 0..3 {
            let gains = l.s[i].gains();
            assert!(!gains.is_empty(), "{i}");
            let own: Vec<u64> = l.s[i].own.iter().map(|e| e.chao.identity()).collect();
            assert!(gains.iter().all(|g| own.contains(&g.identity)));
        }
        // Done: nothing more goes out.
        for _ in 0..RESULT_WAIT + 1 {
            l.frame(0, &[]);
        }
        l.frame(buttons::A, &[0, 1, 2]);
        for s in &mut l.s[..3] {
            assert!(s.done());
            s.abandon();
            assert!(s.take_outgoing().is_empty());
        }
    }

    #[test]
    fn a_turned_down_place_goes_to_the_computer() {
        let mut l = Lobby4::new();
        l.invite();
        l.accept(1, 1);
        // Player 3 turns it down.
        see_offer(&mut l.s[2]);
        l.s[2].update(buttons::DOWN, false);
        l.s[2].update(buttons::A, false);
        l.pump();
        let screen = l.s[0].screen();
        assert_eq!(
            screen.message.as_deref(),
            Some("P3 turned the battle down.")
        );
        assert_eq!(screen.items[1], "Foe 1: P3: turned it down");
        // Give it to the computer: only its teams are offered.
        cursor_to(&mut l.s[0], 1);
        l.s[0].update(buttons::A, false);
        let screen = l.s[0].screen();
        assert_eq!(screen.title, "Foe 1: the computer's team");
        assert!(screen.items.iter().all(|i| i.starts_with("Computer: ")));
        l.s[0].update(buttons::A, false);
        let sent = l.s[0].take_outgoing();
        assert!(matches!(&sent[..], [Body::MultiFill { place: 2, team }] if team.len() == 1));
        let mut out = Vec::new();
        l.host.observe(1, &sent[0], l.now, &mut out);
        assert_eq!(out.len(), 2);
        for o in out {
            use crate::net::referee::To;
            let To::One(to) = o.to else { panic!() };
            let event = online::incoming(o.body, |id| format!("P{id}")).unwrap();
            l.s[usize::from(to) - 1].network(event);
        }
        assert!(matches!(&l.s[0].state, State::Fight(_)));
        assert!(matches!(&l.s[1].state, State::Fight(_)));
        assert!(!matches!(&l.s[2].state, State::Fight(_)));
        assert!(
            l.fight(0)
                .intro
                .as_deref()
                .unwrap()
                .contains("against the computer and")
        );
    }

    #[test]
    fn a_multi_battle_can_be_called_off_or_left_before_it_starts() {
        let mut l = Lobby4::new();
        l.invite();
        l.accept(1, 1);
        // Called off: the ones invited hear of it.
        l.s[0].update(buttons::B, false);
        l.pump();
        assert_eq!(l.s[0].screen().title, "Multi battle");
        for i in [1, 2] {
            assert_eq!(
                l.s[i].screen().message.as_deref(),
                Some("P1 called the multi battle off."),
                "{i}"
            );
            assert!(!l.s[i].busy());
        }
        // Sent again, and player 2 leaves after accepting.
        let s = &mut l.s[0];
        cursor_to(s, PLAN_START);
        s.update(buttons::A, false);
        l.pump();
        l.accept(1, 1);
        l.s[1].update(buttons::B, false);
        l.pump();
        let screen = l.s[0].screen();
        assert_eq!(screen.items[0], "Ally: P2: left");
        assert_eq!(screen.message.as_deref(), Some("P2 left."));
        // Leaving the creature battle scene calls it off.
        l.s[0].abandon();
        l.pump();
        assert_eq!(
            l.s[2].screen().message.as_deref(),
            Some("P1 called the multi battle off.")
        );
        assert!(IDS4.iter().all(|&id| !l.host.in_battle(id)));
    }

    #[test]
    fn the_computer_takes_over_a_player_who_leaves_a_multi_battle() {
        let mut l = Lobby4::new();
        l.invite();
        l.accept(1, 2);
        l.accept(2, 2);
        // Player 2 leaves once they chose.
        for _ in 0..10_000 {
            if matches!(&l.s[1].state, State::Fight(f) if matches!(f.stage, Stage::Remote { .. })) {
                break;
            }
            l.frame(buttons::A, &[1]);
        }
        l.s[1].abandon();
        l.pump();
        let note = "P2 left the battle; the computer plays their place.";
        for i in [0, 2] {
            let link = l.fight(i).link.as_ref().unwrap();
            assert_eq!(link.notes, [note], "{i}");
            assert!(link.others.iter().all(|o| o.1 != "P2"));
        }
        // The others play it to its end.
        l.play_out(&[0, 2]);
        assert!(matches!(l.fight(0).battle.phase(), Phase::Over(_)));
        assert_eq!(
            online::checksum(&l.fight(0).battle),
            online::checksum(&l.fight(2).battle)
        );
        assert!(l.s[0].screen().items.contains(&note.to_string()));
        // Player 2 earned nothing; the ones who played it to the end did.
        l.frame(0, &[]);
        assert!(l.s[1].gains().is_empty());
        assert!(!l.out[1].contains(&Command::Levels));
        assert!(!l.s[0].gains().is_empty() && !l.s[2].gains().is_empty());
    }

    #[test]
    fn giving_up_a_multi_battle_lets_the_computer_play_on() {
        let mut l = Lobby4::new();
        l.invite();
        l.accept(1, 1);
        l.accept(2, 1);
        for _ in 0..10_000 {
            if matches!(&l.s[2].state, State::Fight(f) if matches!(f.stage, Stage::Remote { .. })) {
                break;
            }
            l.frame(buttons::A, &[2]);
        }
        let s = &mut l.s[2];
        s.update(buttons::B, false);
        s.update(buttons::DOWN, false);
        s.update(buttons::A, false);
        assert_eq!(s.screen().title, "You gave up.");
        l.pump();
        assert_eq!(
            l.fight(0).link.as_ref().unwrap().notes,
            ["P3 gave up; the computer plays their place."]
        );
        l.play_out(&[0, 1]);
        l.frame(0, &[]);
        assert!(l.s[2].gains().is_empty());
    }
}
