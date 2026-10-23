//! Battles against the GBA game's trainers: "Trainers" in the battle menu (the `BattleTrainers`
//! setting). The ladder's order, locks and progress are in `ladder`; this file makes the
//! trainers' creatures into fighters from the player's own ROM and runs the trainer menu.
//!
//! A trainer's creatures fight at their own level with the game's stat formula (base stats from
//! the ROM, the trainer's IV for every stat, no EVs, the nature the game gives them), their own
//! moves, types and ability. The menu lists the ladder (`ladder::Plan::rungs`), each trainer's
//! team, and the TM moves won from gym leaders, which can be taught to the player's Chao.
//!
//! No game access here.

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, Ordering};

use crate::gba::ladder::PartyMember;
use crate::gba::rom::Rom;
use crate::gba_link::{Profile, ascii};

use super::fighter::{Fighter, Stats, stat_from_base};
use super::ladder::{Plan, Progress, Rung};
use super::narrate::Sound;
use super::session::{Command, Entry, Screen, buttons, move_detail, step};
use super::team::Dex;

static ENABLED: AtomicBool = AtomicBool::new(true);

/// The `BattleTrainers` setting.
pub fn set_enabled(on: bool) {
    ENABLED.store(on, Ordering::Relaxed);
}

pub fn enabled() -> bool {
    ENABLED.load(Ordering::Relaxed)
}

/// Stats from a species' base stats (ROM order: HP, attack, defense, speed, sp. attack,
/// sp. defense) with `iv` in every stat, no EVs, at `level`, with `nature` (0..25) raising
/// stat `nature / 5` and lowering stat `nature % 5` (attack, defense, speed, sp. attack,
/// sp. defense) by 10%, rounded down. A base HP of 1 always has 1 HP.
pub fn stats(base: [u8; 6], iv: u8, level: u8, nature: u8) -> Stats {
    let iv = u16::from(iv);
    let (up, down) = (usize::from(nature / 5), usize::from(nature % 5));
    let other = |i: usize| {
        let v = u32::from(stat_from_base(u16::from(base[i + 1]), iv, 0, level, false));
        let v = match i {
            _ if up == down => v,
            _ if i == up => v * 110 / 100,
            _ if i == down => v * 90 / 100,
            _ => v,
        };
        v.min(u32::from(u16::MAX)) as u16
    };
    Stats {
        hp: if base[0] == 1 {
            1
        } else {
            stat_from_base(u16::from(base[0]), iv, 0, level, true)
        },
        attack: other(0),
        defense: other(1),
        speed: other(2),
        sp_attack: other(3),
        sp_defense: other(4),
    }
}

/// One of a trainer's creatures.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Creature {
    pub species: u16,
    /// Its species' XP yield (`levels::trainer_xp`).
    pub exp_yield: u8,
    pub fighter: Fighter,
}

/// A trainer as the menu and the battle use them.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Foe {
    pub id: u16,
    /// Class and name, as the ROM has them.
    pub name: String,
    pub double: bool,
    pub party: Vec<Creature>,
}

impl Foe {
    /// The level of the strongest creature.
    pub fn top(&self) -> u8 {
        self.party
            .iter()
            .map(|c| c.fighter.level)
            .max()
            .unwrap_or(1)
    }

    /// "Lv 12 NAME, Lv 14 NAME".
    fn team_line(&self) -> String {
        self.party
            .iter()
            .map(|c| format!("Lv {} {}", c.fighter.level, c.fighter.name))
            .collect::<Vec<_>>()
            .join(", ")
    }
}

/// The ladder with every trainer on it, read from the ROM once.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Roster {
    pub plan: Plan,
    pub foes: BTreeMap<u16, Foe>,
    /// Per gym leader: where they stand, and the TM move they give.
    pub towns: Vec<String>,
    pub tms: Vec<Option<u16>>,
}

/// A trainer's creature as a fighter (`None` when the ROM lacks its species or every move).
pub fn creature(rom: &Rom, dex: &Dex, m: &PartyMember) -> Option<Fighter> {
    let s = rom.species(m.mon.species)?;
    let name = ascii(&rom.species_name(m.mon.species)?);
    let moves: Vec<_> = m
        .mon
        .moves
        .iter()
        .filter_map(|&id| dex.move_data(id))
        .collect();
    if moves.is_empty() {
        return None;
    }
    let stats = stats(s.base, m.mon.iv, m.mon.level, m.nature);
    let mut f = Fighter::new(&name, m.mon.level, stats, s.types, m.ability, &moves);
    f.friendship = m.friendship;
    Some(f)
}

fn foe(rom: &Rom, dex: &Dex, id: u16) -> Option<Foe> {
    let t = rom.trainer(id)?;
    let party = rom
        .trainer_party(id)?
        .iter()
        .map(|m| {
            Some(Creature {
                species: m.mon.species,
                exp_yield: rom.species(m.mon.species)?.exp_yield,
                fighter: creature(rom, dex, m)?,
            })
        })
        .collect::<Option<Vec<_>>>()?;
    let class = rom
        .trainer_class_name(t.class)
        .map(|c| ascii(&c))
        .unwrap_or_default();
    let name = format!("{} {}", class.trim(), ascii(&t.name).trim())
        .trim()
        .to_string();
    Some(Foe {
        id,
        name,
        double: t.double,
        party,
    })
}

impl Roster {
    /// The ladder from the ROM; `None` when it can't be read whole (the leaders, the league and
    /// the champion must all be there).
    pub fn from_rom(rom: &Rom, dex: &Dex) -> Option<Roster> {
        let ladder = rom.ladder()?;
        let plan = Plan::from_rom(&ladder);
        let mut foes = BTreeMap::new();
        let main: Vec<u16> = plan
            .leaders
            .iter()
            .chain(&plan.league)
            .chain([&plan.champion])
            .copied()
            .collect();
        for &id in &main {
            foes.insert(id, foe(rom, dex, id)?);
        }
        let mut plan = plan;
        for tier in &mut plan.route {
            tier.retain(|&id| match foe(rom, dex, id) {
                Some(f) => {
                    foes.insert(id, f);
                    true
                }
                None => false,
            });
        }
        Some(Roster {
            towns: ladder
                .leaders
                .iter()
                .map(|l| l.town.as_deref().map(ascii).unwrap_or_default())
                .collect(),
            tms: ladder
                .leaders
                .iter()
                .map(|l| l.tm.and_then(|i| rom.tmhm_move(i)))
                .collect(),
            plan,
            foes,
        })
    }

    /// The trainer's name, or "Trainer <id>".
    pub fn name(&self, id: u16) -> String {
        self.foes
            .get(&id)
            .map_or_else(|| format!("Trainer {id}"), |f| f.name.clone())
    }

    /// The TM moves won so far (from beaten leaders), in gym order.
    pub fn won_moves(&self, progress: &Progress) -> Vec<u16> {
        let mut out = Vec::new();
        for i in self.plan.badges(progress) {
            if let Some(Some(m)) = self.tms.get(i)
                && !out.contains(m)
            {
                out.push(*m);
            }
        }
        out
    }

    /// What a win against `id` brings, as lines for the result screen (`first`: the first win).
    pub fn reward(&self, id: u16, first: bool, progress: &Progress, dex: &Dex) -> Vec<String> {
        let mut out = Vec::new();
        let rung = self.plan.rung_of(id);
        if let Some(Rung::Leader(i)) = rung {
            if first {
                out.push(format!(
                    "You earned badge {} of {}!",
                    i + 1,
                    self.plan.leaders.len()
                ));
            }
            if let Some(Some(m)) = self.tms.get(i) {
                out.push(format!(
                    "TM move: {} (teach it in Trainers)",
                    dex.move_name(*m)
                ));
            }
        }
        if rung == Some(Rung::Champion) && first {
            out.push("You beat the champion!".into());
        }
        if first {
            let opened: Vec<String> = self
                .plan
                .rungs()
                .into_iter()
                .filter(|&r| self.plan.needs(r) == Some(id) && self.plan.open(r, progress))
                .map(|r| self.rung_label(r))
                .collect();
            if !opened.is_empty() {
                out.push(format!("Now open: {}", opened.join("; ")));
            }
        }
        out
    }

    /// The menu line of a rung (without its lock or badge mark).
    pub fn rung_label(&self, rung: Rung) -> String {
        match rung {
            Rung::Route(t) => {
                let tier = self.plan.route.get(t).map_or(&[][..], Vec::as_slice);
                let level = |id: Option<&u16>| id.and_then(|id| self.foes.get(id)).map(Foe::top);
                match (level(tier.first()), level(tier.last())) {
                    (Some(lo), Some(hi)) if lo < hi => {
                        format!("Route trainers Lv {lo}-{hi} ({})", tier.len())
                    }
                    (Some(lv), Some(_)) => format!("Route trainers Lv {lv} ({})", tier.len()),
                    _ => "Route trainers".into(),
                }
            }
            Rung::Leader(i) => {
                let id = self.plan.leaders.get(i).copied().unwrap_or(0);
                match self.towns.get(i).filter(|t| !t.is_empty()) {
                    Some(town) => format!("Gym {}: {} ({town})", i + 1, self.name(id)),
                    None => format!("Gym {}: {}", i + 1, self.name(id)),
                }
            }
            Rung::League(i) => {
                let id = self.plan.league.get(i).copied().unwrap_or(0);
                format!("League {}: {}", i + 1, self.name(id))
            }
            Rung::Champion => format!("Champion: {}", self.name(self.plan.champion)),
        }
    }
}

/// What the trainer menu needs from the session.
pub struct Ctx<'a> {
    pub roster: &'a Roster,
    pub progress: &'a Progress,
    pub dex: &'a Dex,
    pub own: &'a [Entry],
    /// `own`'s battle profiles (their moves and levels).
    pub profiles: &'a [Profile],
}

/// What the trainer menu did this frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Step {
    Stay,
    /// Back to the battle menu.
    Back,
    /// Battle this trainer.
    Fight(u16),
    /// Teach own Chao `chao` the TM move `move_id` in move slot `slot`.
    Taught {
        chao: usize,
        slot: usize,
        move_id: u16,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum View {
    /// The rungs, then "Teach a TM move".
    List(usize),
    /// A route tier's trainers.
    Tier { tier: usize, cursor: usize },
    /// The TM moves won.
    TeachMove(usize),
    /// Who learns it.
    TeachChao { move_id: u16, cursor: usize },
    /// In place of which move.
    TeachSlot {
        move_id: u16,
        chao: usize,
        cursor: usize,
    },
}

/// The trainer menu.
pub struct Picker {
    view: View,
    notice: Option<String>,
}

fn sound(s: Sound) -> Command {
    Command::Cue(super::narrate::Cue::Sound(s))
}

impl Picker {
    /// The menu with the cursor on the next trainer to beat.
    pub fn open(roster: &Roster, progress: &Progress) -> Picker {
        let rungs = roster.plan.rungs();
        let next = rungs.iter().position(|&r| {
            roster
                .plan
                .trainer(r)
                .is_some_and(|id| roster.plan.open(r, progress) && !progress.beaten.contains(&id))
        });
        Picker {
            view: View::List(next.unwrap_or(0)),
            notice: None,
        }
    }

    /// The menu with the cursor on trainer `id` (back from picking a team).
    pub fn at(roster: &Roster, id: u16) -> Picker {
        let view = match roster.plan.rung_of(id) {
            Some(Rung::Route(tier)) => View::Tier {
                tier,
                cursor: roster.plan.route[tier]
                    .iter()
                    .position(|&t| t == id)
                    .unwrap_or(0),
            },
            Some(r) => View::List(
                roster
                    .plan
                    .rungs()
                    .iter()
                    .position(|&x| x == r)
                    .unwrap_or(0),
            ),
            None => View::List(0),
        };
        Picker { view, notice: None }
    }

    /// Says something under the menu (until the next press).
    pub fn notice(&mut self, text: String) {
        self.notice = Some(text);
    }

    fn list_len(ctx: &Ctx) -> usize {
        ctx.roster.plan.rungs().len() + 1
    }

    pub fn update(&mut self, ctx: &Ctx, pressed: u32, out: &mut Vec<Command>) -> Step {
        if pressed & (buttons::A | buttons::B | buttons::UP | buttons::DOWN) != 0 {
            self.notice = None;
        }
        let a = pressed & buttons::A != 0;
        let b = pressed & buttons::B != 0;
        let plan = &ctx.roster.plan;
        let len = match self.view {
            View::List(_) => Self::list_len(ctx),
            View::Tier { tier, .. } => plan.route.get(tier).map_or(0, Vec::len),
            View::TeachMove(_) => ctx.roster.won_moves(ctx.progress).len(),
            View::TeachChao { .. } => ctx.own.len(),
            View::TeachSlot { .. } => 4,
        };
        let cursor = match &mut self.view {
            View::List(c) | View::TeachMove(c) => c,
            View::Tier { cursor, .. }
            | View::TeachChao { cursor, .. }
            | View::TeachSlot { cursor, .. } => cursor,
        };
        if step(cursor, len, pressed) {
            out.push(sound(Sound::Cursor));
        }
        let cursor = *cursor;
        match self.view {
            View::List(_) if b => {
                out.push(sound(Sound::Back));
                return Step::Back;
            }
            View::List(_) if a => {
                let rungs = plan.rungs();
                match rungs.get(cursor) {
                    Some(&r) if !plan.open(r, ctx.progress) => {
                        self.notice = Some(self.lock_text(ctx, r));
                    }
                    Some(&Rung::Route(tier)) => {
                        out.push(sound(Sound::Confirm));
                        self.view = View::Tier { tier, cursor: 0 };
                    }
                    Some(&r) => {
                        if let Some(id) = plan.trainer(r) {
                            out.push(sound(Sound::Confirm));
                            return Step::Fight(id);
                        }
                    }
                    None if ctx.roster.won_moves(ctx.progress).is_empty() => {
                        self.notice = Some("Beat a gym leader to win a TM move.".into());
                    }
                    None if ctx.own.is_empty() => {
                        self.notice = Some("You have no Chao to teach.".into());
                    }
                    None => {
                        out.push(sound(Sound::Confirm));
                        self.view = View::TeachMove(0);
                    }
                }
            }
            View::Tier { tier, .. } if b => {
                out.push(sound(Sound::Back));
                self.view = View::List(tier);
            }
            View::Tier { tier, .. } if a => {
                if let Some(&id) = plan.route.get(tier).and_then(|t| t.get(cursor)) {
                    out.push(sound(Sound::Confirm));
                    return Step::Fight(id);
                }
            }
            View::TeachMove(_) if b => {
                out.push(sound(Sound::Back));
                self.view = View::List(Self::list_len(ctx) - 1);
            }
            View::TeachMove(_) if a => {
                if let Some(&move_id) = ctx.roster.won_moves(ctx.progress).get(cursor) {
                    out.push(sound(Sound::Confirm));
                    self.view = View::TeachChao { move_id, cursor: 0 };
                }
            }
            View::TeachChao { move_id, .. } if b => {
                out.push(sound(Sound::Back));
                let won = ctx.roster.won_moves(ctx.progress);
                self.view = View::TeachMove(won.iter().position(|&m| m == move_id).unwrap_or(0));
            }
            View::TeachChao { move_id, .. } if a => {
                if let Some(p) = ctx.profiles.get(cursor) {
                    if p.moves.contains(&move_id) {
                        self.notice = Some(format!(
                            "{} already knows {}.",
                            ctx.own[cursor].name,
                            ctx.dex.move_name(move_id)
                        ));
                    } else {
                        out.push(sound(Sound::Confirm));
                        self.view = View::TeachSlot {
                            move_id,
                            chao: cursor,
                            cursor: 0,
                        };
                    }
                }
            }
            View::TeachSlot { move_id, chao, .. } if b => {
                out.push(sound(Sound::Back));
                self.view = View::TeachChao {
                    move_id,
                    cursor: chao,
                };
            }
            View::TeachSlot { move_id, chao, .. } if a => {
                out.push(sound(Sound::Confirm));
                let old = ctx
                    .profiles
                    .get(chao)
                    .and_then(|p| p.moves.get(cursor))
                    .copied()
                    .filter(|&m| m != 0);
                let name = ctx.own.get(chao).map_or("?", |e| e.name.as_str());
                self.notice = Some(match old {
                    Some(old) => format!(
                        "{name} learned {} in place of {}!",
                        ctx.dex.move_name(move_id),
                        ctx.dex.move_name(old)
                    ),
                    None => format!("{name} learned {}!", ctx.dex.move_name(move_id)),
                });
                self.view = View::TeachChao {
                    move_id,
                    cursor: chao,
                };
                return Step::Taught {
                    chao,
                    slot: cursor,
                    move_id,
                };
            }
            _ => {}
        }
        Step::Stay
    }

    /// Why `rung` is locked.
    fn lock_text(&self, ctx: &Ctx, rung: Rung) -> String {
        match ctx.roster.plan.needs(rung) {
            Some(id) => format!("Beat {} first.", ctx.roster.name(id)),
            None => "Not open yet.".into(),
        }
    }

    /// A trainer's team, format and prize.
    fn foe_detail(&self, ctx: &Ctx, id: u16) -> String {
        let roster = ctx.roster;
        let Some(foe) = roster.foes.get(&id) else {
            return String::new();
        };
        let mut s = format!(
            "{}. {}.",
            foe.team_line(),
            if foe.double { "Doubles" } else { "Singles" }
        );
        if let Some(Rung::Leader(i)) = roster.plan.rung_of(id) {
            let tm = roster
                .tms
                .get(i)
                .copied()
                .flatten()
                .map(|m| format!(" and TM move {}", ctx.dex.move_name(m)))
                .unwrap_or_default();
            if ctx.progress.beaten.contains(&id) {
                s.push_str(&format!(" Won: badge {}{tm}.", i + 1));
            } else {
                s.push_str(&format!(" Prize: badge {}{tm}.", i + 1));
            }
        } else if ctx.progress.beaten.contains(&id) {
            s.push_str(" Beaten.");
        }
        s
    }

    pub fn screen(&self, ctx: &Ctx) -> Screen {
        let roster = ctx.roster;
        let plan = &roster.plan;
        let mut s = match self.view {
            View::List(cursor) => {
                let rungs = plan.rungs();
                let mut items: Vec<String> = rungs
                    .iter()
                    .map(|&r| {
                        let mut line = roster.rung_label(r);
                        let beaten = plan
                            .trainer(r)
                            .is_some_and(|id| ctx.progress.beaten.contains(&id));
                        if beaten {
                            line.push_str(" *");
                        }
                        line
                    })
                    .collect();
                let won = roster.won_moves(ctx.progress).len();
                items.push(format!("Teach a TM move ({won} won)"));
                let mut disabled: Vec<usize> = (0..rungs.len())
                    .filter(|&i| !plan.open(rungs[i], ctx.progress))
                    .collect();
                if won == 0 {
                    disabled.push(rungs.len());
                }
                let detail = match rungs.get(cursor) {
                    Some(&r) if !plan.open(r, ctx.progress) => self.lock_text(ctx, r),
                    Some(&Rung::Route(_)) => {
                        "Warm-up battles against the game's route trainers.".into()
                    }
                    Some(&r) => plan
                        .trainer(r)
                        .map(|id| self.foe_detail(ctx, id))
                        .unwrap_or_default(),
                    None => "Teach a TM move won from a gym leader to one of your Chao, in \
                             place of one of its moves."
                        .into(),
                };
                let badges = plan.badges(ctx.progress).len();
                Screen {
                    title: format!("Trainers (badges: {badges} of {})", plan.leaders.len()),
                    items,
                    disabled,
                    cursor: Some(cursor),
                    detail: Some(detail),
                    hint: "A: battle  B: back  (* beaten)",
                    ..Screen::default()
                }
            }
            View::Tier { tier, cursor } => {
                let ids = plan.route.get(tier).map_or(&[][..], Vec::as_slice);
                let items = ids
                    .iter()
                    .map(|&id| {
                        let foe = roster.foes.get(&id);
                        let mut line =
                            format!("Lv {}  {}", foe.map_or(0, Foe::top), roster.name(id));
                        if ctx.progress.beaten.contains(&id) {
                            line.push_str(" *");
                        }
                        line
                    })
                    .collect();
                Screen {
                    title: roster.rung_label(Rung::Route(tier)),
                    items,
                    cursor: Some(cursor),
                    detail: ids.get(cursor).map(|&id| self.foe_detail(ctx, id)),
                    hint: "A: battle  B: back  (* beaten)",
                    ..Screen::default()
                }
            }
            View::TeachMove(cursor) => {
                let won = roster.won_moves(ctx.progress);
                Screen {
                    title: "Teach which TM move?".into(),
                    items: won
                        .iter()
                        .map(|&m| {
                            let kind = ctx
                                .dex
                                .move_data(m)
                                .map(|d| ctx.dex.type_name(d.type_id))
                                .unwrap_or_default();
                            format!("{}  {kind}", ctx.dex.move_name(m))
                        })
                        .collect(),
                    cursor: Some(cursor),
                    detail: won
                        .get(cursor)
                        .and_then(|&m| ctx.dex.move_data(m))
                        .map(|d| move_detail(ctx.dex, &d)),
                    hint: "A: choose  B: back",
                    ..Screen::default()
                }
            }
            View::TeachChao { move_id, cursor } => Screen {
                title: format!("Who learns {}?", ctx.dex.move_name(move_id)),
                items: ctx.own.iter().map(|e| e.name.clone()).collect(),
                disabled: (0..ctx.own.len())
                    .filter(|&i| {
                        ctx.profiles
                            .get(i)
                            .is_some_and(|p| p.moves.contains(&move_id))
                    })
                    .collect(),
                cursor: Some(cursor),
                detail: ctx.profiles.get(cursor).map(|p| {
                    let moves: Vec<String> = p
                        .moves
                        .iter()
                        .filter(|&&m| m != 0)
                        .map(|&m| ctx.dex.move_name(m))
                        .collect();
                    format!("Moves: {}", moves.join(", "))
                }),
                hint: "A: choose  B: back",
                ..Screen::default()
            },
            View::TeachSlot {
                move_id,
                chao,
                cursor,
            } => {
                let moves = ctx.profiles.get(chao).map_or([0; 4], |p| p.moves);
                Screen {
                    title: format!(
                        "{}: forget which move for {}?",
                        ctx.own.get(chao).map_or("?", |e| e.name.as_str()),
                        ctx.dex.move_name(move_id)
                    ),
                    items: moves
                        .iter()
                        .map(|&m| {
                            if m == 0 {
                                "(empty)".into()
                            } else {
                                ctx.dex.move_name(m)
                            }
                        })
                        .collect(),
                    cursor: Some(cursor),
                    detail: moves
                        .get(cursor)
                        .and_then(|&m| ctx.dex.move_data(m))
                        .map(|d| move_detail(ctx.dex, &d)),
                    hint: "A: replace  B: back",
                    ..Screen::default()
                }
            }
        };
        if let Some(n) = &self.notice {
            s.message = Some(n.clone());
        }
        s
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::battle::data::{MoveData, TypeChart};

    #[test]
    fn stats_follow_the_gen3_formula_and_nature() {
        // Base 100 everywhere, IV 31, level 50: 120 for the others and 175 HP.
        let neutral = stats([100; 6], 31, 50, 0);
        assert_eq!(neutral.hp, 175);
        assert_eq!(
            [
                neutral.attack,
                neutral.defense,
                neutral.speed,
                neutral.sp_attack,
                neutral.sp_defense
            ],
            [120; 5]
        );
        // Nature 1 raises attack and lowers defense.
        let n = stats([100; 6], 31, 50, 1);
        assert_eq!((n.attack, n.defense, n.speed), (132, 108, 120));
        // Nature 23 raises sp. defense (4) and lowers sp. attack (3); 24 changes nothing.
        let n = stats([100; 6], 31, 50, 23);
        assert_eq!((n.sp_attack, n.sp_defense), (108, 132));
        assert_eq!(stats([100; 6], 31, 50, 24), neutral);
        // HP is never changed by nature; a base HP of 1 stays at 1.
        assert_eq!(stats([100; 6], 31, 50, 0).hp, stats([100; 6], 31, 50, 1).hp);
        assert_eq!(stats([1, 90, 45, 40, 30, 30], 0, 20, 3).hp, 1);
        // Low level, low IV.
        let low = stats([45, 49, 49, 45, 65, 65], 0, 5, 12);
        assert_eq!(low.hp, 19);
        assert_eq!(low.attack, 9);
    }

    fn roster() -> (Roster, Dex) {
        let dex = Dex::for_tests(
            &[MoveData::plain(1, 0, 40), MoveData::plain(2, 0, 60)],
            TypeChart::default(),
        );
        let foe = |id: u16, level: u8, double: bool| Foe {
            id,
            name: format!("T{id}"),
            double,
            party: vec![Creature {
                exp_yield: 50,
                species: 1,
                fighter: Fighter::new(
                    "Mon",
                    level,
                    stats([50; 6], 10, level, 0),
                    [0, 0],
                    0,
                    &[MoveData::plain(1, 0, 40)],
                ),
            }],
        };
        let plan = Plan {
            route: vec![vec![10, 11], vec![12]],
            leaders: vec![100, 101],
            league: vec![200],
            champion: 300,
        };
        let mut foes = BTreeMap::new();
        for (id, lv, d) in [
            (10, 3, false),
            (11, 5, true),
            (12, 9, false),
            (100, 6, false),
            (101, 10, true),
            (200, 20, false),
            (300, 30, false),
        ] {
            foes.insert(id, foe(id, lv, d));
        }
        (
            Roster {
                plan,
                foes,
                towns: vec!["TOWN A".into(), String::new()],
                tms: vec![Some(2), None],
            },
            dex,
        )
    }

    fn entry(name: &str) -> Entry {
        Entry {
            name: name.into(),
            chao: crate::chao::ChaoData {
                bytes: [0; crate::chao::CHAO_SIZE],
            },
            moves: None,
        }
    }

    #[test]
    fn the_menu_lists_the_ladder_and_locks_what_is_not_open() {
        let (roster, dex) = roster();
        let progress = Progress::default();
        let own = [entry("Pip")];
        let profiles = [Profile {
            types: [0, 0],
            ability: 0,
            moves: [1, 0, 0, 0],
            partner: None,
            level: 5,
        }];
        let ctx = Ctx {
            roster: &roster,
            progress: &progress,
            dex: &dex,
            own: &own,
            profiles: &profiles,
        };
        let mut p = Picker::open(&roster, &progress);
        let s = p.screen(&ctx);
        assert_eq!(
            s.items,
            [
                "Route trainers Lv 3-5 (2)",
                "Route trainers Lv 9 (1)",
                "Gym 1: T100 (TOWN A)",
                "Gym 2: T101",
                "League 1: T200",
                "Champion: T300",
                "Teach a TM move (0 won)",
            ]
        );
        assert_eq!(s.disabled, [1, 3, 4, 5, 6]);
        // The cursor starts on the first leader not beaten yet.
        assert_eq!(s.cursor, Some(2));
        let mut out = Vec::new();
        assert_eq!(p.update(&ctx, buttons::A, &mut out), Step::Fight(100));
        // A locked one says who to beat first.
        p.update(&ctx, buttons::DOWN, &mut out);
        assert_eq!(p.update(&ctx, buttons::A, &mut out), Step::Stay);
        assert_eq!(p.screen(&ctx).message.as_deref(), Some("Beat T100 first."));
        // A route tier opens its list.
        let mut p = Picker::open(&roster, &progress);
        p.update(&ctx, buttons::UP, &mut out);
        p.update(&ctx, buttons::UP, &mut out);
        assert_eq!(p.update(&ctx, buttons::A, &mut out), Step::Stay);
        assert_eq!(p.screen(&ctx).items, ["Lv 3  T10", "Lv 5  T11"]);
        p.update(&ctx, buttons::DOWN, &mut out);
        assert_eq!(p.update(&ctx, buttons::A, &mut out), Step::Fight(11));
        assert_eq!(p.update(&ctx, buttons::B, &mut out), Step::Stay);
        assert_eq!(p.update(&ctx, buttons::B, &mut out), Step::Back);
        // Back from a team pick, the cursor is on the trainer.
        let p = Picker::at(&roster, 11);
        assert_eq!(p.screen(&ctx).cursor, Some(1));
        let p = Picker::at(&roster, 200);
        assert_eq!(p.screen(&ctx).cursor, Some(4));
    }

    #[test]
    fn a_won_tm_move_can_be_taught_in_place_of_a_move() {
        let (roster, dex) = roster();
        let mut progress = Progress::default();
        progress.beat(100);
        assert_eq!(roster.won_moves(&progress), [2]);
        let own = [entry("Pip"), entry("Bo")];
        let profiles = [
            Profile {
                types: [0, 0],
                ability: 0,
                moves: [2, 0, 0, 0],
                partner: None,
                level: 5,
            },
            Profile {
                types: [0, 0],
                ability: 0,
                moves: [1, 0, 0, 0],
                partner: None,
                level: 5,
            },
        ];
        let ctx = Ctx {
            roster: &roster,
            progress: &progress,
            dex: &dex,
            own: &own,
            profiles: &profiles,
        };
        let mut p = Picker::open(&roster, &progress);
        let mut out = Vec::new();
        // To the last item.
        while p.screen(&ctx).cursor != Some(p.screen(&ctx).items.len() - 1) {
            p.update(&ctx, buttons::DOWN, &mut out);
        }
        assert_eq!(
            p.screen(&ctx).items.last().unwrap(),
            "Teach a TM move (1 won)"
        );
        p.update(&ctx, buttons::A, &mut out);
        assert_eq!(p.screen(&ctx).items, ["Move 2  Type 0"]);
        p.update(&ctx, buttons::A, &mut out);
        // Pip knows it already.
        assert_eq!(p.screen(&ctx).disabled, [0]);
        assert_eq!(p.update(&ctx, buttons::A, &mut out), Step::Stay);
        p.update(&ctx, buttons::DOWN, &mut out);
        p.update(&ctx, buttons::A, &mut out);
        assert_eq!(
            p.screen(&ctx).items,
            ["Move 1", "(empty)", "(empty)", "(empty)"]
        );
        assert_eq!(
            p.update(&ctx, buttons::A, &mut out),
            Step::Taught {
                chao: 1,
                slot: 0,
                move_id: 2
            }
        );
        assert_eq!(
            p.screen(&ctx).message.as_deref(),
            Some("Bo learned Move 2 in place of Move 1!")
        );
    }

    #[test]
    fn wins_bring_badges_moves_and_open_the_next_rungs() {
        let (roster, dex) = roster();
        let mut progress = Progress::default();
        progress.beat(100);
        let lines = roster.reward(100, true, &progress, &dex);
        assert_eq!(
            lines,
            [
                "You earned badge 1 of 2!",
                "TM move: Move 2 (teach it in Trainers)",
                "Now open: Route trainers Lv 9 (1); Gym 2: T101",
            ]
        );
        // Again: the move is still shown, nothing new opens.
        assert_eq!(
            roster.reward(100, false, &progress, &dex),
            ["TM move: Move 2 (teach it in Trainers)"]
        );
        // A route trainer brings nothing.
        assert!(roster.reward(10, true, &progress, &dex).is_empty());
        progress.beat(101);
        progress.beat(200);
        progress.beat(300);
        assert_eq!(
            roster.reward(300, true, &progress, &dex),
            ["You beat the champion!"]
        );
    }

    #[test]
    fn reads_the_rom() {
        let Some(home) = std::env::var_os("HOME") else {
            return;
        };
        let path = std::path::Path::new(&home).join("Games/gba/emerald.gba");
        if !path.exists() {
            println!("skipped: no GBA ROM at {}", path.display());
            return;
        }
        let rom = Rom::open(&path).expect("ROM opens");
        let dex = Dex::from_rom(&rom).expect("dex");
        let roster = Roster::from_rom(&rom, &dex).expect("roster");
        assert_eq!(roster.plan.leaders.len(), crate::gba::ladder::LEADERS);
        assert_eq!(
            roster.tms.iter().flatten().count(),
            crate::gba::ladder::LEADERS
        );
        let route: usize = roster.plan.route.iter().map(Vec::len).sum();
        assert!(route > 400, "{route}");
        for f in roster.foes.values() {
            assert!(!f.name.is_empty() && f.name.is_ascii());
            assert!((1..=6).contains(&f.party.len()));
            for c in &f.party {
                assert!(!c.fighter.moves.is_empty());
                assert!(c.fighter.stats.hp > 0);
            }
        }
        // The first leader's team is weaker than the champion's.
        let first = &roster.foes[&roster.plan.leaders[0]];
        let last = &roster.foes[&roster.plan.champion];
        assert!(first.top() < last.top());
        assert!(first.party[0].fighter.stats.hp < last.party[0].fighter.stats.hp);
        // One leader fights doubles.
        let doubles = roster
            .plan
            .leaders
            .iter()
            .filter(|id| roster.foes[id].double)
            .count();
        assert_eq!(doubles, 1);
    }
}
