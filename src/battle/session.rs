//! One battle from the karate menu to the result: picking singles or doubles, your team and the
//! opponents, then the turns (your choices from menus, the opponents' from the AI) played out
//! beat by beat.
//!
//! No game access here: `update` takes the buttons pressed this frame and returns `Command`s for
//! the arena (game.rs), and `screen` says what the HUD shows.

use std::collections::VecDeque;
use std::sync::Arc;

use crate::chao::ChaoData;
use crate::gba_link::Profile;
use crate::log;

use super::ai;
use super::data::{MoveData, Target, types};
use super::effects;
use super::engine::{Action, Battle, Choice, Format, Phase};
use super::event::{Event, Outcome, Slot};
use super::fighter::{Fighter, MoveSlot};
use super::narrate::{self, Beat, Cue, Names, Sound, View};
use super::rng::Rng;
use super::team::{self, Dex};

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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Command {
    Cue(Cue),
    /// Leave the karate menu for the arena.
    OpenArena(Format),
    /// The battle is over: back to the karate menu.
    CloseArena,
    /// Closed before a battle started.
    Closed,
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

pub type ProfileOf = Box<dyn Fn(&ChaoData) -> Profile + Send>;

pub struct Session {
    dex: Arc<Dex>,
    own: Vec<Entry>,
    /// `own`'s types, ability and moves, for the picker.
    own_profiles: Vec<Profile>,
    lineups: Vec<Lineup>,
    profile_of: ProfileOf,
    seed: u64,
    state: State,
    notice: Option<String>,
}

enum State {
    Format {
        cursor: usize,
    },
    Team {
        format: Format,
        cursor: usize,
        picked: Vec<usize>,
    },
    Opponent {
        format: Format,
        picked: Vec<usize>,
        cursor: usize,
    },
    Fight(Box<Fight>),
    Done,
}

/// Moves `cursor` up or down a list of `len` (wrapping); whether it moved.
fn step(cursor: &mut usize, len: usize, pressed: u32) -> bool {
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
fn move_detail(dex: &Dex, m: &MoveData) -> String {
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
        let own_profiles = own.iter().map(|e| profile_of(&e.chao)).collect();
        Session {
            dex,
            own,
            own_profiles,
            lineups,
            profile_of,
            seed,
            state: State::Format { cursor: 0 },
            notice: None,
        }
    }

    pub fn done(&self) -> bool {
        matches!(self.state, State::Done)
    }

    /// In the arena (the menu was left for the battle).
    pub fn in_arena(&self) -> bool {
        matches!(&self.state, State::Fight(f) if f.opened)
    }

    /// The record of `side`'s team member `member`, for the arena's Chao.
    pub fn record(&self, side: usize, member: usize) -> Option<&ChaoData> {
        match &self.state {
            State::Fight(f) => f.records.get(side)?.get(member),
            _ => None,
        }
    }

    /// Opponent teams to pick from after the player's team: the lineups, then copies of the
    /// player's other Chao (or of the team itself when there are no others).
    fn opponents(&self, picked: &[usize]) -> Vec<(String, Vec<usize>)> {
        let mut out: Vec<(String, Vec<usize>)> = (0..self.lineups.len())
            .map(|i| {
                let l = &self.lineups[i];
                (
                    format!("{} ({})", l.label, l.members.len().min(TEAM_SIZE)),
                    Vec::new(),
                )
            })
            .collect();
        let others: Vec<usize> = (0..self.own.len())
            .filter(|i| !picked.contains(i))
            .take(TEAM_SIZE)
            .collect();
        let (label, members) = if others.is_empty() {
            ("Copies of your team", picked.to_vec())
        } else {
            ("Copies of your other Chao", others)
        };
        out.push((format!("{label} ({})", members.len()), members));
        out
    }

    /// The names on opponent team `index` of `opponents(picked)`.
    fn opponent_names(&self, picked: &[usize], index: usize) -> Vec<String> {
        match self.lineups.get(index) {
            Some(l) => l
                .members
                .iter()
                .take(TEAM_SIZE)
                .map(|e| e.name.clone())
                .collect(),
            None => self
                .opponents(picked)
                .last()
                .map(|(_, m)| m.iter().map(|&i| self.own[i].name.clone()).collect())
                .unwrap_or_default(),
        }
    }

    /// Runs one frame with the buttons pressed in it; `arena_ready` once the arena is set up
    /// after `OpenArena`.
    pub fn update(&mut self, pressed: u32, arena_ready: bool) -> Vec<Command> {
        let mut out = Vec::new();
        if pressed & (buttons::A | buttons::B | buttons::UP | buttons::DOWN) != 0 {
            self.notice = None;
        }
        let state = std::mem::replace(&mut self.state, State::Done);
        self.state = match state {
            State::Format { mut cursor } => {
                if step(&mut cursor, 2, pressed) {
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
                        }
                    }
                } else {
                    State::Format { cursor }
                }
            }
            State::Team {
                format,
                mut cursor,
                mut picked,
            } => {
                // The Chao, then "Done".
                let len = self.own.len() + 1;
                if step(&mut cursor, len, pressed) {
                    out.push(sound(Sound::Cursor));
                }
                if pressed & buttons::B != 0 {
                    out.push(sound(Sound::Back));
                    State::Format {
                        cursor: usize::from(format == Format::Doubles),
                    }
                } else if pressed & buttons::A != 0 && cursor == self.own.len() {
                    if picked.is_empty() {
                        self.notice = Some("Pick at least one Chao.".into());
                        State::Team {
                            format,
                            cursor,
                            picked,
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
                    State::Team {
                        format,
                        cursor,
                        picked,
                    }
                } else {
                    State::Team {
                        format,
                        cursor,
                        picked,
                    }
                }
            }
            State::Opponent {
                format,
                picked,
                mut cursor,
            } => {
                let options = self.opponents(&picked);
                if step(&mut cursor, options.len(), pressed) {
                    out.push(sound(Sound::Cursor));
                }
                if pressed & buttons::B != 0 {
                    out.push(sound(Sound::Back));
                    State::Team {
                        format,
                        cursor: self.own.len(),
                        picked,
                    }
                } else if pressed & buttons::A != 0 {
                    let theirs: Vec<Entry> = match self.lineups.get(cursor) {
                        Some(l) => l.members.iter().take(TEAM_SIZE).cloned().collect(),
                        None => options
                            .last()
                            .map(|(_, m)| m.iter().map(|&i| self.own[i].clone()).collect())
                            .unwrap_or_default(),
                    };
                    let mine: Vec<Entry> = picked.iter().map(|&i| self.own[i].clone()).collect();
                    match Fight::new(
                        format,
                        [mine, theirs],
                        self.dex.clone(),
                        &self.profile_of,
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
                } else {
                    State::Opponent {
                        format,
                        picked,
                        cursor,
                    }
                }
            }
            State::Fight(mut fight) => {
                let over = fight.update(pressed, arena_ready, &mut out);
                if over {
                    State::Done
                } else {
                    State::Fight(fight)
                }
            }
            State::Done => State::Done,
        };
        out
    }

    pub fn screen(&self) -> Screen {
        let mut s = match &self.state {
            State::Format { cursor } => Screen {
                title: "Chao battle".into(),
                items: vec!["Singles (1 on 1)".into(), "Doubles (2 on 2)".into()],
                cursor: Some(*cursor),
                detail: Some(
                    if *cursor == 0 {
                        "One Chao a side at a time."
                    } else {
                        "Two Chao a side at a time; some moves hit both foes."
                    }
                    .into(),
                ),
                hint: "A: choose  B: back",
                ..Screen::default()
            },
            State::Team { cursor, picked, .. } => {
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
                let detail = match (self.own.get(*cursor), self.own_profiles.get(*cursor)) {
                    (Some(e), Some(p)) => Some(chao_detail(&self.dex, &e.chao, p)),
                    _ => Some("Start with the Chao you picked, in that order.".into()),
                };
                Screen {
                    title: format!("Your team (up to {TEAM_SIZE})"),
                    items,
                    cursor: Some(*cursor),
                    detail,
                    hint: "A: pick or drop  B: back",
                    ..Screen::default()
                }
            }
            State::Opponent { picked, cursor, .. } => Screen {
                title: "Battle against".into(),
                items: self.opponents(picked).into_iter().map(|(l, _)| l).collect(),
                cursor: Some(*cursor),
                detail: Some(self.opponent_names(picked, *cursor).join(", ")),
                hint: "A: battle  B: back",
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
    Result,
}

struct Fight {
    battle: Battle,
    ai: Rng,
    names: FightNames,
    records: [Vec<ChaoData>; 2],
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
    fn new(
        format: Format,
        teams: [Vec<Entry>; 2],
        dex: Arc<Dex>,
        profile_of: &ProfileOf,
        seed: u64,
    ) -> Option<Fight> {
        let build = |entries: &[Entry]| -> Vec<Fighter> {
            entries
                .iter()
                .map(|e| team::fighter(&e.name, &e.chao, &profile_of(&e.chao), &dex))
                .collect()
        };
        let fighters = [build(&teams[0]), build(&teams[1])];
        let names = [0, 1].map(|s| teams[s].iter().map(|e| e.name.clone()).collect());
        let records = [0, 1].map(|s| teams[s].iter().map(|e| e.chao.clone()).collect());
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
        Some(Fight {
            battle,
            ai: Rng::new(seed ^ 0x5EED_0A11),
            names: FightNames { names, dex },
            records,
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

    fn enqueue(&mut self, events: &[Event]) {
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
            }
            Phase::Replacements => {
                let slots: Vec<Slot> = self
                    .battle
                    .pending_replacements()
                    .iter()
                    .copied()
                    .filter(|s| s.side == 0)
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
            let at = Slot::new(0, pos);
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
        let f = self.battle.team(0).get(member)?;
        if f.fainted() {
            Some(Unavailable::Fainted)
        } else if taken.contains(&member) {
            Some(Unavailable::ComingIn)
        } else if !self.battle.bench(0).contains(&member) {
            Some(Unavailable::InBattle)
        } else {
            None
        }
    }

    /// The first member that can come in, for the cursor.
    fn first_free(&self, taken: &[usize]) -> usize {
        (0..self.battle.team(0).len())
            .find(|&m| self.unavailable(m, taken).is_none())
            .unwrap_or(0)
    }

    fn has_free(&self, taken: &[usize]) -> bool {
        (0..self.battle.team(0).len()).any(|m| self.unavailable(m, taken).is_none())
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
        let at = Slot::new(0, pos);
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
                let len = self.battle.team(0).len();
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
                let len = self.battle.team(0).len();
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
                    out.push(sound(Sound::Confirm));
                    self.gave_up = true;
                    out.push(Command::Cue(Cue::Result { winner: Some(1) }));
                    self.stage = Stage::Result;
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
        if step(&mut cursor, self.battle.team(0).len(), pressed) {
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
        let f = &self.battle.team(0)[member];
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
        let len = self.battle.team(0).len();
        s.items = (0..len).map(|m| self.member_line(m, taken)).collect();
        s.disabled = (0..len)
            .filter(|&m| self.unavailable(m, taken).is_some())
            .collect();
        s.cursor = Some(cursor);
        if let Some(f) = self.battle.team(0).get(cursor) {
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
                s.title = if self.gave_up {
                    "You gave up.".into()
                } else {
                    match self.battle.phase() {
                        Phase::Over(Outcome::Won(0)) => "You won!".into(),
                        Phase::Over(Outcome::Draw) => "It's a draw.".into(),
                        _ => "You lost...".into(),
                    }
                };
                let (mine, my_team) = self.standing(0);
                let (theirs, their_team) = self.standing(1);
                s.items = vec![
                    format!("Your Chao standing: {mine} of {my_team}"),
                    format!("Foe's Chao standing: {theirs} of {their_team}"),
                    format!("Turns: {}", self.battle.turn()),
                ];
                s.message = None;
                s.hint = "A: back to the menu";
            }
            Stage::Choose { pos, menu } => {
                let at = Slot::new(0, *pos);
                s.choosing = Some(at);
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
                                if t.side == 0 {
                                    format!("{name} (ally)")
                                } else {
                                    format!("Foe {name}")
                                }
                            })
                            .collect();
                        s.aimed = targets.get(cursor).copied();
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
                    .and_then(|&slot| self.shown.at(slot))
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
            Box::new(move |_| Profile {
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
}
