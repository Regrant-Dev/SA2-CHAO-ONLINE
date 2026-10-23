//! One battle from the karate menu to the result: picking singles or doubles, your team and the
//! opponents (or a trainer from the GBA game, `trainers`, or a friend online, `online`), then the
//! turns (your choices from menus, the opponents' from the AI or the friend's game) played out
//! beat by beat.
//!
//! No game access here: `update` takes the buttons pressed this frame and returns `Command`s for
//! the arena (game.rs), and `screen` says what the HUD shows. A live battle's messages come in
//! through `network` and go out through `take_outgoing`.

use std::collections::VecDeque;
use std::sync::Arc;

use crate::chao::ChaoData;
use crate::gba_link::Profile;
use crate::log;
use crate::net::protocol::{
    ANSWER_ACCEPT, ANSWER_BUSY, ANSWER_DECLINE, BattleMember, BattlePick, Body, END_BUSY,
    END_DECLINED, END_DESYNC, END_DISCONNECT, END_FORFEIT, END_ROM_DIFFERS, END_TIMEOUT,
    END_WITHDRAWN, LEAVE_GAVE_UP, LEAVE_LEFT, LEAVE_OVER,
};

use super::ai;
use super::data::{MoveData, Target, types};
use super::effects;
use super::engine::{Action, Battle, Choice, Format, Phase};
use super::event::{Event, Outcome, Slot};
use super::fighter::{Fighter, MoveSlot};
use super::ladder::Progress;
use super::mirror;
use super::narrate::{self, Beat, Cue, Names, Sound, View};
use super::online::{self, Incoming};
use super::rng::Rng;
use super::team::{self, Dex};
use super::trainers::{self, Picker, Roster};

/// Menu buttons (the game's `MenuButtons_Pressed` bits; the keyboard and the stick count).
/// Start isn't one of ours: it opens the game's pause menu.
pub mod buttons {
    pub const B: u32 = 0x02;
    pub const A: u32 = 0x04;
    pub const UP: u32 = 0x10;
    pub const DOWN: u32 = 0x20;
}

/// Most Chao on a team.
pub const TEAM_SIZE: usize = 6;
/// A beat with a message can be skipped with A after this many frames.
const SKIP_AFTER: u16 = 10;
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

/// A Chao to battle with: a copy of its record (the save is never touched) and its name.
#[derive(Clone)]
pub struct Entry {
    pub name: String,
    pub chao: ChaoData,
}

/// A ready-made opponent team: a friend's Chao, the imported ones.
#[derive(Clone)]
pub struct Lineup {
    pub label: String,
    pub members: Vec<Entry>,
}

/// A friend online at the Stadium's karate, who can be challenged to a live battle.
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
    /// Not a team: switches whether partners from a GBA save count in live battles.
    SaveRule,
    Lineup(usize),
    /// Copies of these of the player's Chao.
    Copies(Vec<usize>),
}

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

/// What a live battle's end (`END_*`, or `END_LOST`) says.
fn end_note(reason: u8, name: &str) -> String {
    match reason {
        END_BUSY => format!("{name} can't battle right now."),
        END_DECLINED => format!("{name} turned the battle down."),
        END_ROM_DIFFERS => {
            format!("{name}'s GBA ROM is a different one: live battles need the same game.")
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
    /// Leave the karate menu for the arena.
    OpenArena(Format),
    /// The battle is over: back to the karate menu.
    CloseArena,
    /// Closed before a battle started.
    Closed,
    /// A challenge that opened the menu by itself ended unanswered or turned down: the menu
    /// closes and the mode select goes on as before it (`Session::take_farewell` says why).
    Released,
    /// The trainer ladder's progress changed (`Session::progress`): a win or a taught move.
    Progress,
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
}

/// A Chao's profile; the flag is a live battle's rule (`Body::BattleChallenge::save_partners`).
pub type ProfileOf = Box<dyn Fn(&ChaoData, bool) -> Profile + Send>;

pub struct Session {
    dex: Arc<Dex>,
    /// `online::dex_print` of `dex`, for live battles.
    dex_print: u32,
    own: Vec<Entry>,
    /// `own`'s types, ability and moves (with their TM moves), for the picker.
    own_profiles: Vec<Profile>,
    lineups: Vec<Lineup>,
    profile_of: ProfileOf,
    seed: u64,
    state: State,
    notice: Option<String>,
    /// The GBA game's trainers ("Trainers" in the format menu), when on.
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
    /// Our challenges' rule: partners from a GBA save count.
    save_partners: bool,
    /// The rule of the challenge being answered.
    offer_save_partners: bool,
    /// The notice to show as the menu closes after `Command::Released`.
    farewell: Option<String>,
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
    /// Frames waited.
    waited: u32,
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

/// A Chao for the picker: its types, ability, moves and battle stats.
fn chao_detail(dex: &Dex, chao: &ChaoData, profile: &Profile) -> String {
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
    let s = team::chao_stats(chao).stats;
    format!(
        "{}, {ability}. Moves: {}. HP {} Atk {} Def {} SpA {} SpD {} Spe {}",
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

/// A move for the move menu: its kind, power, accuracy, who it hits and the ROM's description.
pub(super) fn move_detail(dex: &Dex, m: &MoveData) -> String {
    let mut parts = vec![
        if m.power == 0 {
            "Status"
        } else if types::is_special(m.type_id) {
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
        let own_profiles = own.iter().map(|e| profile_of(&e.chao, true)).collect();
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
            save_partners: true,
            offer_save_partners: true,
            farewell: None,
        }
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

    /// In a battle or a live battle's challenge (others see us busy).
    pub fn busy(&self) -> bool {
        matches!(
            self.state,
            State::Offer { .. } | State::Waiting(_) | State::Fight(_)
        ) || matches!(&self.state, State::Team { live: Some(_), .. })
    }

    /// Waiting on the host: a challenge, an answer or a live battle still going.
    pub fn waits_on_host(&self) -> bool {
        match &self.state {
            State::Offer { .. } | State::Waiting(_) => true,
            State::Team { live, .. } => live.is_some(),
            State::Fight(f) => f.link.as_ref().is_some_and(|l| l.open()),
            _ => false,
        }
    }

    /// The session closes before its end (the player left the karate area or the menu went):
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
            State::Waiting(_) => self.outbox.push(Body::BattleLeave {
                battle: 0,
                reason: LEAVE_LEFT,
            }),
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
        let state = std::mem::replace(&mut self.state, State::Done);
        self.state = match (state, event) {
            (State::Fight(mut f), event) => {
                if let Incoming::Offer { from, .. } = event {
                    self.outbox.push(answer(from, ANSWER_BUSY));
                } else if let Incoming::Start(start) = event {
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
                | State::Team { live: None, .. }),
                Incoming::Offer {
                    from,
                    name,
                    format,
                    save_partners,
                },
            ) if online::enabled() => {
                log!("battle: {name} challenges us ({format:?}, save partners {save_partners})");
                drop(state);
                self.offer_save_partners = save_partners;
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
            (State::Waiting(w), Incoming::Start(start)) if start.opponent == w.with => {
                self.start_live(*w, start)
            }
            (state, Incoming::Start(start)) => {
                // A battle nobody here waits for any more.
                self.outbox.push(leave(start.battle, LEAVE_LEFT));
                state
            }
            (State::Waiting(w), event) => match ended(&event, w.with) {
                Some(reason) => {
                    self.notice = Some(end_note(reason, &w.name));
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
                self.notice = Some(end_note(reason, &name));
                let mut out = Vec::new();
                let state = self.offer_over(&mut out);
                self.queued.append(&mut out);
                state
            }
            (state, _) => state,
        };
        self.note_live();
    }

    /// A team of the picked Chao as it goes to a live battle (`save_partners`: its rule).
    fn wire_team(&self, picked: &[usize], save_partners: bool) -> Vec<BattleMember> {
        picked
            .iter()
            .filter_map(|&i| {
                let chao = &self.own.get(i)?.chao;
                Some(online::member(
                    chao,
                    &self.profile_ruled(chao, save_partners),
                ))
            })
            .collect()
    }

    fn live_members(&self, team: &[BattleMember]) -> Vec<Member> {
        team.iter()
            .map(|m| Member {
                fighter: online::fighter_of(m, &self.dex),
                record: Some((*m.chao).clone()),
                species: None,
            })
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
            Against::Live(_) | Against::SaveRule => Vec::new(),
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
        let team = self.wire_team(&picked, self.save_partners);
        self.outbox.push(Body::BattleChallenge {
            to: friend.id,
            format: online::format_wire(format),
            save_partners: self.save_partners,
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
            waited: 0,
        }))
    }

    /// The host started the live battle we waited for.
    fn start_live(&mut self, w: Waiting, start: online::Start) -> State {
        self.live_battle = Some(start.battle);
        let mine = self.live_members(&w.team);
        let theirs = self.live_members(&start.team);
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
                fight.link = Some(Link {
                    battle: start.battle,
                    opponent: start.opponent,
                    name: w.name,
                    step: 1,
                    ready: None,
                    outbox: Vec::new(),
                    ended: None,
                    closed: false,
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
    /// (whose TM moves the player's Chao then use).
    pub fn with_trainers(mut self, roster: Arc<Roster>, progress: Progress) -> Session {
        self.trainers = Some(roster);
        self.progress = progress;
        self.refresh_profiles();
        self
    }

    /// The player's ladder `progress` without the ladder (the ROM's trainers couldn't be read):
    /// the Chao keep the TM moves they were taught.
    pub fn with_progress(mut self, progress: Progress) -> Session {
        self.progress = progress;
        self.refresh_profiles();
        self
    }

    /// The trainer ladder's progress, to keep after `Command::Progress`.
    pub fn progress(&self) -> &Progress {
        &self.progress
    }

    /// A Chao's types, ability and moves, with the TM moves it was taught.
    fn profile(&self, chao: &ChaoData) -> Profile {
        self.profile_ruled(chao, true)
    }

    /// `profile`, under a live battle's rule.
    fn profile_ruled(&self, chao: &ChaoData, save_partners: bool) -> Profile {
        self.progress
            .apply(chao.identity(), (self.profile_of)(chao, save_partners))
    }

    fn refresh_profiles(&mut self) {
        self.own_profiles = self.own.iter().map(|e| self.profile(&e.chao)).collect();
    }

    /// The format menu's items: singles, doubles, and the trainers when on.
    fn formats(&self) -> usize {
        if self.trainers.is_some() { 3 } else { 2 }
    }

    /// The player's picked Chao as fighters.
    fn chao_members(&self, entries: &[Entry]) -> Vec<Member> {
        entries
            .iter()
            .map(|e| Member {
                fighter: team::fighter(&e.name, &e.chao, &self.profile(&e.chao), &self.dex),
                record: Some(e.chao.clone()),
                species: None,
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
                species: Some(c.species),
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
        Some(fight)
    }

    /// After a frame of a trainer battle: a first win records progress and its reward.
    fn record_win(&mut self, fight: &mut Fight, out: &mut Vec<Command>) {
        let Some(id) = fight.foe else { return };
        if fight.recorded || !fight.won() {
            return;
        }
        fight.recorded = true;
        let first = self.progress.beat(id);
        if let Some(roster) = &self.trainers {
            fight.reward = roster.reward(id, first, &self.progress, &self.dex);
        }
        if first {
            out.push(Command::Progress);
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

    /// The species of `side`'s team member `member` when it's a trainer's creature, for its
    /// picture in the arena.
    pub fn species(&self, side: usize, member: usize) -> Option<u16> {
        match &self.state {
            State::Fight(f) => *f.species.get(side)?.get(member)?,
            _ => None,
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
            out.push((self.save_rule_label(), Against::SaveRule));
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

    /// The live-challenge rule's row.
    fn save_rule_label(&self) -> String {
        format!(
            "Live rule: GBA save partners {}",
            if self.save_partners {
                "allowed"
            } else {
                "not allowed"
            }
        )
    }

    /// About an opponent team: its Chao's names, or who a live battle is against.
    fn opponent_detail(&self, against: &Against) -> String {
        match against {
            Against::SaveRule => if self.save_partners {
                "A: switch. In your live challenges, Chao use what partners from a GBA save taught them."
            } else {
                "A: switch. In your live challenges, both teams leave out what partners from a GBA save taught (moves, types, ability)."
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

    /// Runs one frame with the buttons pressed in it; `arena_ready` once the arena is set up
    /// after `OpenArena`.
    pub fn update(&mut self, pressed: u32, arena_ready: bool) -> Vec<Command> {
        let mut out = std::mem::take(&mut self.queued);
        if pressed & (buttons::A | buttons::B | buttons::UP | buttons::DOWN) != 0 {
            self.notice = None;
        }
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
                    } else if cursor == 2
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
                        trainers::Step::Back => State::Format { cursor: 2 },
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
                            }
                        }
                        trainers::Step::Taught {
                            chao,
                            slot,
                            move_id,
                        } => {
                            if let Some(e) = self.own.get(chao) {
                                self.progress.teach(e.chao.identity(), slot, move_id);
                                self.refresh_profiles();
                                out.push(Command::Progress);
                            }
                            State::Trainers(picker)
                        }
                    }
                }
            },
            State::Team {
                format,
                mut cursor,
                mut picked,
                foe,
                live,
            } => {
                // The Chao, then "Done".
                let len = self.own.len() + 1;
                if step(&mut cursor, len, pressed) {
                    out.push(sound(Sound::Cursor));
                }
                let stay = |cursor, picked, foe, live| State::Team {
                    format,
                    cursor,
                    picked,
                    foe,
                    live,
                };
                if pressed & buttons::B != 0 {
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
                        _ => State::Format {
                            cursor: usize::from(format == Format::Doubles),
                        },
                    }
                } else if pressed & buttons::A != 0 && cursor == self.own.len() {
                    if picked.is_empty() {
                        self.notice = Some("Pick at least one Chao.".into());
                        stay(cursor, picked, foe, live)
                    } else if let Some((from, name)) = live {
                        // Accept the challenge with this team.
                        let team = self.wire_team(&picked, self.offer_save_partners);
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
                            waited: 0,
                        }))
                    } else if let Some(id) = foe {
                        match self.trainer_fight(&picked, id) {
                            Some(fight) => {
                                out.push(sound(Sound::Confirm));
                                State::Fight(Box::new(fight))
                            }
                            None => {
                                self.notice = Some("That battle can't start.".into());
                                stay(cursor, picked, foe, None)
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
                    } else if picked.len() < TEAM_SIZE {
                        picked.push(cursor);
                        out.push(sound(Sound::Confirm));
                        // A full team goes straight to "Done".
                        if picked.len() == TEAM_SIZE {
                            cursor = self.own.len();
                        }
                    } else {
                        self.notice = Some(format!("A team has at most {TEAM_SIZE} Chao."));
                    }
                    stay(cursor, picked, foe, live)
                } else {
                    stay(cursor, picked, foe, live)
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
                        cursor: self.own.len(),
                        picked,
                        foe: None,
                        live: None,
                    }
                } else if pressed & buttons::A != 0 && cursor < options.len() {
                    match options.swap_remove(cursor).1 {
                        Against::SaveRule => {
                            self.save_partners = !self.save_partners;
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
                let gone = w.waited > WAIT_LIMIT;
                if gone {
                    self.notice = Some(end_note(END_TIMEOUT, &w.name));
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
                self.record_win(&mut fight, &mut out);
                if over {
                    State::Done
                } else {
                    State::Fight(fight)
                }
            }
            State::Done => State::Done,
        };
        self.note_live();
        out
    }

    pub fn screen(&self) -> Screen {
        let mut s = match &self.state {
            State::Format { cursor } => {
                let mut items = vec!["Singles (1 on 1)".into(), "Doubles (2 on 2)".into()];
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
                            _ => {
                                "The GBA game's trainers: route trainers, the gym leaders, the \
                                 league and the champion, each opening once the one before is \
                                 beaten."
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
            State::Team {
                cursor,
                picked,
                foe,
                live,
                ..
            } => {
                let mut items: Vec<String> = self
                    .own
                    .iter()
                    .enumerate()
                    .map(|(i, e)| match picked.iter().position(|&p| p == i) {
                        Some(n) => format!("{} {}", n + 1, e.name),
                        None => format!("- {}", e.name),
                    })
                    .collect();
                items.push(format!("Done ({} picked)", picked.len()));
                let against = foe.and_then(|id| self.trainers.as_ref()?.foes.get(&id));
                let detail = match (self.own.get(*cursor), self.own_profiles.get(*cursor)) {
                    (Some(e), Some(p)) => Some(chao_detail(&self.dex, &e.chao, p)),
                    _ => Some(match (against, live) {
                        (Some(f), _) => format!(
                            "Start with the Chao you picked, in that order, against {} ({}).",
                            f.name,
                            if f.double { "doubles" } else { "singles" }
                        ),
                        (None, Some((_, name))) => format!(
                            "Accept {name}'s challenge with the Chao you picked, in that order."
                        ),
                        (None, None) => "Start with the Chao you picked, in that order.".into(),
                    }),
                };
                Screen {
                    title: match (against, live) {
                        (Some(f), _) => {
                            format!("Your team against {} (up to {TEAM_SIZE})", f.name)
                        }
                        (None, Some((_, name))) => {
                            format!("Your team against {name} (up to {TEAM_SIZE})")
                        }
                        (None, None) => format!("Your team (up to {TEAM_SIZE})"),
                    },
                    items,
                    cursor: Some(*cursor),
                    detail,
                    hint: "A: pick or drop  B: back",
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
                    "Accept, then pick your team. {name} chooses their own moves from their game.{}",
                    if self.offer_save_partners {
                        ""
                    } else {
                        " Rule: GBA save partners not allowed (what they taught is left out)."
                    }
                )),
                hint: "A: choose  B: decline",
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
            State::Fight(f) => f.screen(),
            State::Done => Screen::default(),
        };
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
/// creature (its species, for its picture).
struct Member {
    fighter: Fighter,
    record: Option<ChaoData>,
    species: Option<u16>,
}

struct Fight {
    battle: Battle,
    /// The player's side in `battle` (1 when they accepted a live battle). The HUD, the arena
    /// and the narrator see the player's side as side 0: `names`, `records`, `species` and the
    /// views are in that order, and events are swapped (`mirror`) before they're told.
    me: usize,
    /// A live battle (the other side is a friend's game).
    link: Option<Link>,
    ai: Rng,
    names: FightNames,
    records: [Vec<Option<ChaoData>>; 2],
    species: [Vec<Option<u16>>; 2],
    /// The trainer battled, its challenge, and what a win brought (once recorded).
    foe: Option<u16>,
    intro: Option<String>,
    reward: Vec<String>,
    recorded: bool,
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
            ai: Rng::new(seed ^ 0x5EED_0A11),
            names: FightNames { names, dex },
            records,
            species,
            foe: None,
            intro: None,
            reward: Vec::new(),
            recorded: false,
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

    /// Over with the player's side the winner (not given up).
    fn won(&self) -> bool {
        self.stage == Stage::Result
            && !self.gave_up
            && self.battle.phase() == Phase::Over(Outcome::Won(self.me))
    }

    /// Waiting for the friend's choice: the step once it's here, or the player giving up.
    fn remote(&mut self, give_up: Option<usize>, pressed: u32, out: &mut Vec<Command>) {
        let ready = self.link.as_mut().and_then(|l| {
            let ready = l.ready.take()?;
            l.step = l.step.wrapping_add(1);
            Some(ready)
        });
        if let Some((seed, picks)) = ready {
            let events = online::apply_step(&mut self.battle, seed, &picks);
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

    /// The player gives up: a loss (the friend's game is told).
    fn give_up(&mut self, out: &mut Vec<Command>) {
        out.push(sound(Sound::Confirm));
        self.gave_up = true;
        out.push(Command::Cue(Cue::Result { winner: Some(1) }));
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
                    .filter(|s| s.side == self.me)
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
            if self.battle.alive(at) && !self.battle.forced(at) {
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
        if f.fainted() {
            Some(Unavailable::Fainted)
        } else if taken.contains(&member) {
            Some(Unavailable::ComingIn)
        } else if !self.battle.bench(self.me).contains(&member) {
            Some(Unavailable::InBattle)
        } else {
            None
        }
    }

    /// The first member that can come in, for the cursor.
    fn first_free(&self, taken: &[usize]) -> usize {
        (0..self.battle.team(self.me).len())
            .find(|&m| self.unavailable(m, taken).is_none())
            .unwrap_or(0)
    }

    fn has_free(&self, taken: &[usize]) -> bool {
        (0..self.battle.team(self.me).len()).any(|m| self.unavailable(m, taken).is_none())
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
                let len = self.battle.team(self.me).len();
                if step(&mut cursor, len, pressed) {
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
                let len = self.battle.team(self.me).len();
                if step(&mut cursor, len, pressed) {
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
        if step(&mut cursor, self.battle.team(self.me).len(), pressed) {
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
                });
            }
        }
        out
    }

    /// A team member in the switch and send-in lists: HP, status, and why it can't come in.
    fn member_line(&self, member: usize, taken: &[usize]) -> String {
        let f = &self.battle.team(self.me)[member];
        let mut line = format!("{}  HP {}/{}", f.name, f.hp, f.stats.hp);
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
        let len = self.battle.team(self.me).len();
        s.items = (0..len).map(|m| self.member_line(m, taken)).collect();
        s.disabled = (0..len)
            .filter(|&m| self.unavailable(m, taken).is_some())
            .collect();
        s.cursor = Some(cursor);
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
                let (mine, my_team) = self.standing(self.me);
                let (theirs, their_team) = self.standing(self.foe());
                let foes = if self.foe.is_some() { "team" } else { "Chao" };
                s.items = vec![
                    format!("Your Chao standing: {mine} of {my_team}"),
                    format!("Foe's {foes} standing: {theirs} of {their_team}"),
                    format!("Turns: {}", self.battle.turn()),
                ];
                if let Some(((reason, winner), name)) = ended {
                    s.items.push(match (reason, winner) {
                        (END_TIMEOUT, Some(w)) if w == self.me => {
                            format!("{name} took too long to choose.")
                        }
                        (END_TIMEOUT, Some(_)) => "You took too long to choose.".into(),
                        (END_TIMEOUT, None) => "Nobody chose for too long.".into(),
                        _ => end_note(reason, name),
                    });
                }
                s.items.extend(self.reward.iter().cloned());
                s.message = None;
                s.hint = "A: back to the menu";
            }
            Stage::Remote { give_up } => {
                let name = self.link.as_ref().map_or("", |l| l.name.as_str());
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
        c
    }

    fn entry(name: &str, points: i16) -> Entry {
        Entry {
            name: name.into(),
            chao: chao(points),
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
        let own = (0..own)
            .map(|i| entry(&format!("Own{i}"), 200 + i as i16 * 100))
            .collect();
        Session::open(
            Arc::new(dex),
            own,
            lineups,
            Box::new(move |_, _| Profile {
                types: [0, 0],
                ability: 0,
                moves,
                partner: None,
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
    fn b_on_the_first_screen_closes() {
        let mut s = session(2, Vec::new());
        assert_eq!(s.screen().items.len(), 2);
        let out = s.update(buttons::B, false);
        assert!(out.contains(&Command::Closed));
        assert!(s.done());
    }

    #[test]
    fn without_chao_there_is_nothing_to_pick() {
        let mut s = session(0, Vec::new());
        s.update(buttons::A, false);
        assert!(s.screen().message.is_some());
        assert_eq!(s.screen().items.len(), 2);
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
        assert_eq!(s.screen().items.len(), 4);
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
        assert_eq!(s.screen().items[0], "1 Own0");
        assert_eq!(s.screen().items[2], "2 Own2");
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
        assert_eq!(s.screen().cursor, Some(3));
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
        assert_eq!(screen.cursor, Some(8));
        assert_eq!(screen.items[8], "Done (6 picked)");
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

    /// A ladder of one-creature trainers: route trainer 10, leader 100 (TM move 2), league 200
    /// and champion 300; leader 100's creature is weak, 200's fights doubles.
    fn roster() -> Roster {
        use crate::battle::ladder::Plan;
        use crate::battle::trainers::{Creature, Foe};
        let foe = |id: u16, double: bool| Foe {
            id,
            name: format!("T{id}"),
            double,
            party: vec![Creature {
                species: 7,
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
            tms: vec![Some(2)],
        }
    }

    #[test]
    fn without_trainers_the_menu_has_no_trainers_item() {
        let s = session(1, Vec::new());
        assert_eq!(s.screen().items, ["Singles (1 on 1)", "Doubles (2 on 2)"]);
        let s = session(1, Vec::new()).with_trainers(Arc::new(roster()), Progress::default());
        assert_eq!(s.screen().items.len(), 3);
        assert_eq!(s.screen().items[2], "Trainers");
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
                .contains(&"You earned badge 1 of 1!".to_string())
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
    }

    #[test]
    fn a_taught_tm_move_is_kept_and_used() {
        let mut progress = Progress::default();
        progress.beat(100);
        let mut s = session(1, Vec::new()).with_trainers(Arc::new(roster()), progress);
        s.update(buttons::UP, false);
        s.update(buttons::A, false);
        // "Teach a TM move", the last item (from the league trainer).
        s.update(buttons::DOWN, false);
        s.update(buttons::DOWN, false);
        assert_eq!(
            s.screen().items[s.screen().cursor.unwrap()],
            "Teach a TM move (1 won)"
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
        assert!(out.contains(&Command::Progress));
        assert_eq!(s.progress().taught[&identity], [0, 2, 0, 0]);
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

    #[test]
    fn a_live_battle_plays_the_same_in_both_games() {
        for doubles in [false, true] {
            let mut l = Lobby::new(3);
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
        // Player 2 leaves the area mid-battle (its session is dropped).
        let mut l = Lobby::new(2);
        l.start(false, 1);
        until_waiting(&mut l, 1);
        l.s[1].abandon();
        l.pump();
        assert_eq!(l.s[0].screen().title, "You won!");
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
    }

    #[test]
    fn a_challenge_that_opened_the_menu_closes_it_when_it_ends() {
        let offer = || Incoming::Offer {
            from: 1,
            name: "P1".into(),
            format: Format::Singles,
            save_partners: true,
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
        s.update(buttons::DOWN, false);
        s.update(buttons::A, false);
        assert_eq!(s.screen().items[0], "Busy (live, busy)");
        assert_eq!(s.screen().disabled, [0]);
        s.update(buttons::A, false);
        assert!(s.take_outgoing().is_empty());
        assert_eq!(s.screen().title, "Battle against");
    }

    #[test]
    fn the_save_partner_rule_goes_with_the_challenge_to_both_teams() {
        let mut l = Lobby::new(1);
        // Type 1 marks a profile made without save partners.
        for s in &mut l.s {
            s.profile_of = Box::new(|_, save_partners| Profile {
                types: [u8::from(!save_partners); 2],
                ability: 0,
                moves: [1, 0, 0, 0],
                partner: None,
            });
        }
        let s = &mut l.s[0];
        s.update(buttons::A, false);
        s.update(buttons::A, false);
        s.update(buttons::UP, false);
        s.update(buttons::A, false);
        assert_eq!(s.screen().items[0], "P2 (live)");
        assert_eq!(s.screen().items[1], "Live rule: GBA save partners allowed");
        s.update(buttons::DOWN, false);
        s.update(buttons::A, false);
        assert_eq!(
            s.screen().items[1],
            "Live rule: GBA save partners not allowed"
        );
        assert!(s.take_outgoing().is_empty());
        s.update(buttons::UP, false);
        s.update(buttons::A, false);
        let sent = s.take_outgoing();
        let [
            Body::BattleChallenge {
                save_partners: false,
                team,
                ..
            },
        ] = &sent[..]
        else {
            panic!("{sent:?}");
        };
        assert_eq!(team[0].types, [1, 1]);
        let mut out = Vec::new();
        l.host.observe(IDS[0], &sent[0], l.now, &mut out);
        l.deliver(out);
        let s = &mut l.s[1];
        assert!(
            s.screen()
                .detail
                .unwrap()
                .contains("GBA save partners not allowed")
        );
        see_offer(s);
        s.update(buttons::A, false);
        s.update(buttons::A, false);
        s.update(buttons::UP, false);
        s.update(buttons::A, false);
        let sent = s.take_outgoing();
        let [Body::BattleAnswer { team, .. }] = &sent[..] else {
            panic!("{sent:?}");
        };
        assert_eq!(team[0].types, [1, 1]);
        // The picker itself shows the Chao as they are.
        assert_eq!(s.own_profiles[0].types, [0, 0]);
    }
}
