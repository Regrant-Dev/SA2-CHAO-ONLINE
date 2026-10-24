//! Live battles against a friend online (`BattleOnline` setting): a challenge from the battle
//! menu, both teams sent whole, then the turns. Both games run the same battle (the challenger
//! is side 0, the accepter side 1, who sees it with the sides swapped: `mirror`); the host
//! (net/battles.rs) holds each step's choices until both are in, compares the two games'
//! checksums and gives the step's rolls a new seed, so neither game can know the other's choice
//! or the rolls ahead.
//!
//! - Teams go as `BattleMember`s (a copy of each Chao's record and its battle profile, with its
//!   battle level) and both games build both teams' fighters from them the same way
//!   (`fighter_of`), under the challenge's level rule: every Chao at level 50, or each at its
//!   own level.
//! - Choices go as `BattlePick`s; a step plays the same in both games (`apply_step`), a side
//!   whose choices the battle refuses getting the computer's instead.
//! - The mailbox between the network thread (online.rs) and the battle menu (game.rs, on the
//!   game thread): what came in and what goes out. The network thread holds the lobby's lock
//!   while it calls in here, so nothing here waits on the battle.

use std::fmt::Write as _;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};

use crate::chao::ChaoData;
use crate::gba_link::Profile;
use crate::net::protocol::{
    BattleMember, BattlePick, Body, PICK_MOVE, PICK_NONE, PICK_OUT_OF_PP, PICK_REPLACE,
    PICK_SWITCH, SERVER,
};

use super::ai;
use super::engine::{Action, Battle, BattleError, Choice, Format, Phase};
use super::event::{Event, Outcome, Slot};
use super::fighter::Fighter;
use super::levels::{MAX_LEVEL, MIN_LEVEL};
use super::rng::Rng;
use super::team::{self, Dex};

static ENABLED: AtomicBool = AtomicBool::new(true);

/// The `BattleOnline` setting.
pub fn set_enabled(on: bool) {
    ENABLED.store(on, Ordering::Relaxed);
}

pub fn enabled() -> bool {
    ENABLED.load(Ordering::Relaxed)
}

/// Mixed into a step's seed for the computer's choices for a side whose own were refused.
const AI_SALT: u64 = 0x0A11_C0DE_5EED_0B47;

pub fn format_of(wire: u8) -> Format {
    if wire == 1 {
        Format::Doubles
    } else {
        Format::Singles
    }
}

pub fn format_wire(format: Format) -> u8 {
    u8::from(format == Format::Doubles)
}

/// FNV-1a over formatted text, so state is hashed without building the text.
struct Fnv(u32);

impl Fnv {
    fn new() -> Fnv {
        Fnv(0x811C_9DC5)
    }
}

impl std::fmt::Write for Fnv {
    fn write_str(&mut self, s: &str) -> std::fmt::Result {
        for b in s.bytes() {
            self.0 = (self.0 ^ u32::from(b)).wrapping_mul(0x0100_0193);
        }
        Ok(())
    }
}

/// The ROM data a battle runs on (every move record, the type chart, the out-of-PP move and
/// turn limit, the sound moves): two games battle only when theirs are the same.
pub fn dex_print(dex: &Dex) -> u32 {
    let mut h = Fnv::new();
    let _ = write!(
        h,
        "{:?}|{:?}|{:?}|{:?}",
        dex.table(),
        dex.chart(),
        dex.rules(),
        dex.sound_moves()
    );
    h.0
}

/// The battle's state (both teams without their names, who stands where, the field, the turn
/// and phase): two games that played the same steps have the same.
pub fn checksum(battle: &Battle) -> u32 {
    let mut h = Fnv::new();
    for side in 0..2 {
        for f in battle.team(side) {
            let f = Fighter {
                name: String::new(),
                ..f.clone()
            };
            let _ = write!(h, "{f:?}|");
        }
    }
    let active: Vec<Option<usize>> = battle
        .slots()
        .into_iter()
        .map(|s| battle.member(s))
        .collect();
    let _ = write!(
        h,
        "{active:?}|{:?}|{:?}|{}|{:?}",
        battle.field(),
        battle.phase(),
        battle.turn(),
        battle.pending_replacements()
    );
    h.0
}

/// A Chao's wire form: its record and battle profile.
pub fn member(chao: &ChaoData, profile: &Profile) -> BattleMember {
    BattleMember {
        chao: Box::new(chao.clone()),
        species: profile.partner.unwrap_or(0),
        types: profile.types,
        ability: profile.ability,
        moves: profile.moves,
        level: profile.level.clamp(MIN_LEVEL, MAX_LEVEL),
    }
}

/// The battle profile a team member came with.
pub fn profile_of(m: &BattleMember) -> Profile {
    Profile {
        types: m.types,
        ability: m.ability,
        moves: m.moves,
        partner: (m.species != 0).then_some(m.species),
        level: m.level,
    }
}

/// A team member's fighter, the same in both games: at its own battle level with
/// `actual_levels` (the challenge's rule), else at `team::FAIR_LEVEL`.
pub fn fighter_of(m: &BattleMember, dex: &Dex, actual_levels: bool) -> Fighter {
    let profile = Profile {
        level: if actual_levels {
            m.level
        } else {
            team::FAIR_LEVEL
        },
        ..profile_of(m)
    };
    team::fighter(&crate::karate::chao_name(&m.chao), &m.chao, &profile, dex)
}

fn target_wire(t: Option<Slot>) -> u8 {
    t.map_or(PICK_NONE, |t| (t.side * 2 + t.pos) as u8)
}

fn target_of(wire: u8) -> Option<Slot> {
    (wire != PICK_NONE).then(|| Slot::new(usize::from(wire / 2), usize::from(wire % 2)))
}

/// A turn's choices for one side (and who its passing moves hand on to) as picks.
pub fn choice_picks(choices: &[Choice], passes: &[(Slot, usize)]) -> Vec<BattlePick> {
    choices
        .iter()
        .map(|c| {
            let pass = passes
                .iter()
                .find(|p| p.0 == c.at)
                .map_or(PICK_NONE, |p| p.1 as u8);
            let (kind, value, target) = match c.action {
                Action::Move { index, target } => (PICK_MOVE, index as u8, target_wire(target)),
                Action::OutOfPp => (PICK_OUT_OF_PP, 0, PICK_NONE),
                Action::Switch { member } => (PICK_SWITCH, member as u8, PICK_NONE),
            };
            BattlePick {
                pos: c.at.pos as u8,
                kind,
                value,
                target,
                pass,
            }
        })
        .collect()
}

/// Replacements for fainted fighters as picks.
pub fn replace_picks(picks: &[(Slot, usize)]) -> Vec<BattlePick> {
    picks
        .iter()
        .map(|&(at, member)| BattlePick {
            pos: at.pos as u8,
            kind: PICK_REPLACE,
            value: member as u8,
            target: PICK_NONE,
            pass: PICK_NONE,
        })
        .collect()
}

/// Side `side`'s turn choices and passes from its picks (replacements left out).
fn turn_choices(side: usize, picks: &[BattlePick]) -> (Vec<Choice>, Vec<(Slot, usize)>) {
    let mut choices = Vec::new();
    let mut passes = Vec::new();
    for p in picks {
        let at = Slot::new(side, usize::from(p.pos));
        let action = match p.kind {
            PICK_MOVE => Action::Move {
                index: usize::from(p.value),
                target: target_of(p.target),
            },
            PICK_OUT_OF_PP => Action::OutOfPp,
            PICK_SWITCH => Action::Switch {
                member: usize::from(p.value),
            },
            _ => continue,
        };
        choices.push(Choice { at, action });
        if p.pass != PICK_NONE {
            passes.push((at, usize::from(p.pass)));
        }
    }
    (choices, passes)
}

fn replacements_of(side: usize, picks: &[BattlePick]) -> Vec<(Slot, usize)> {
    picks
        .iter()
        .filter(|p| p.kind == PICK_REPLACE)
        .map(|p| (Slot::new(side, usize::from(p.pos)), usize::from(p.value)))
        .collect()
}

/// Plays one step of a live battle, as both games do: `seed` gives its rolls, and `picks` are
/// both sides' choices (side, pick). A side whose choices the battle refuses gets the
/// computer's (from the same seed, so both games pick the same).
pub fn apply_step(battle: &mut Battle, seed: u64, picks: &[(u8, BattlePick)]) -> Vec<Event> {
    battle.reseed(seed);
    let of = |side: usize| -> Vec<BattlePick> {
        picks
            .iter()
            .filter(|(s, _)| usize::from(*s) == side)
            .map(|p| p.1)
            .collect()
    };
    let mine = [of(0), of(1)];
    let mut computer = [false; 2];
    let ai_rng = |side: usize| Rng::new(seed ^ AI_SALT ^ side as u64);
    for _ in 0..3 {
        let result = match battle.phase() {
            Phase::Actions => {
                let mut choices = Vec::new();
                for side in 0..2 {
                    if computer[side] {
                        choices.extend(ai::choose(battle, side, &mut ai_rng(side)));
                    } else {
                        let (c, passes) = turn_choices(side, &mine[side]);
                        choices.extend(c);
                        for (at, member) in passes {
                            battle.pick_pass(at, member);
                        }
                    }
                }
                battle.play_turn(&choices)
            }
            Phase::Replacements => {
                let mut all = Vec::new();
                for side in 0..2 {
                    if computer[side] {
                        all.extend(ai::replacements(battle, side));
                    } else {
                        all.extend(replacements_of(side, &mine[side]));
                    }
                }
                battle.replace(&all)
            }
            Phase::Over(_) => return Vec::new(),
        };
        match result {
            Ok(events) => return events,
            Err(BattleError::Invalid(at) | BattleError::Missing(at)) if !computer[at.side] => {
                crate::log!(
                    "battle: side {}'s choices refused; the computer chooses",
                    at.side
                );
                computer[at.side] = true;
            }
            Err(e) if computer != [true; 2] => {
                crate::log!("battle: step refused ({e:?}); the computer chooses");
                computer = [true; 2];
            }
            Err(e) => {
                crate::log!("battle: step refused ({e:?})");
                return Vec::new();
            }
        }
    }
    Vec::new()
}

/// A team of `chao` with no partners (Normal, the basic moves) at level 50, for a test player
/// (`fakepeer`).
pub fn plain_team(rom: &crate::gba::rom::Rom, chao: &[ChaoData]) -> Vec<BattleMember> {
    let profile = crate::gba_link::profile::profile(
        rom,
        None,
        &crate::gba_link::store::Traits::default(),
        &crate::gba_link::store::Moves::default(),
        true,
        team::FAIR_LEVEL,
    );
    chao.iter()
        .take(crate::net::protocol::MAX_BATTLE_TEAM)
        .map(|c| member(c, &profile))
        .collect()
}

/// One side of a live battle run by the computer, for a test player (`fakepeer`): the same
/// battle as the other game's, with choices from the battle AI.
pub struct Duel {
    battle: Battle,
    pub id: u16,
    pub side: usize,
    step: u16,
    sent: bool,
    ai: Rng,
}

impl Duel {
    /// The battle `start` begins, with `mine` (the team this side sent), under the challenge's
    /// level rule.
    pub fn new(
        dex: &Dex,
        start: &Start,
        mine: &[BattleMember],
        actual_levels: bool,
    ) -> Option<Duel> {
        let team = |t: &[BattleMember]| -> Vec<Fighter> {
            t.iter()
                .map(|m| fighter_of(m, dex, actual_levels))
                .collect()
        };
        let (mine, theirs) = (team(mine), team(&start.team));
        let teams = if start.side == 0 {
            [mine, theirs]
        } else {
            [theirs, mine]
        };
        let mut battle = Battle::new(
            start.format,
            teams,
            dex.chart().clone(),
            dex.rules(),
            start.seed,
        )
        .ok()?;
        battle.set_move_table(dex.table());
        battle.set_sound_moves(dex.sound_moves().to_vec());
        battle.start();
        Some(Duel {
            battle,
            id: start.battle,
            side: start.side,
            step: 1,
            sent: false,
            ai: Rng::new(start.seed ^ AI_SALT),
        })
    }

    /// This step's choice, once (then none until its step is played, or after the end).
    pub fn choice(&mut self) -> Option<Body> {
        if self.sent {
            return None;
        }
        let picks = match self.battle.phase() {
            Phase::Actions => choice_picks(&ai::choose(&self.battle, self.side, &mut self.ai), &[]),
            Phase::Replacements => replace_picks(&ai::replacements(&self.battle, self.side)),
            Phase::Over(_) => return None,
        };
        self.sent = true;
        Some(Body::BattleChoice {
            battle: self.id,
            step: self.step,
            check: checksum(&self.battle),
            picks,
        })
    }

    /// Plays the host's step (when it's the one this side waits for).
    pub fn step(&mut self, step: u16, seed: u64, picks: &[(u8, BattlePick)]) -> Vec<Event> {
        if step != self.step || !self.sent {
            return Vec::new();
        }
        self.step = self.step.wrapping_add(1);
        self.sent = false;
        apply_step(&mut self.battle, seed, picks)
    }

    pub fn over(&self) -> Option<Outcome> {
        match self.battle.phase() {
            Phase::Over(o) => Some(o),
            _ => None,
        }
    }

    pub fn turn(&self) -> u32 {
        self.battle.turn()
    }
}

/// What came in from the host for the battle menu.
#[derive(Clone, Debug, PartialEq)]
pub enum Incoming {
    /// Player `from` (`name`) challenges us.
    Offer {
        from: u8,
        name: String,
        format: Format,
        /// The challenge's rules (`Body::BattleChallenge`).
        save_partners: bool,
        actual_levels: bool,
    },
    Start(Start),
    Step {
        battle: u16,
        step: u16,
        seed: u64,
        picks: Vec<(u8, BattlePick)>,
    },
    End {
        battle: u16,
        reason: u8,
        winner: u8,
    },
    /// A player left the lobby.
    Left(u8),
    /// The connection to the host is gone.
    Lost,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Start {
    pub battle: u16,
    pub format: Format,
    pub side: usize,
    pub opponent: u8,
    pub seed: u64,
    pub team: Vec<BattleMember>,
}

#[derive(Default)]
struct Mailbox {
    incoming: Vec<Incoming>,
    outgoing: Vec<Body>,
    busy: bool,
}

/// Most messages kept for the battle menu: it takes them every frame it runs; ones past this
/// (a game paused for long) are dropped and the host times them out.
const MAX_INCOMING: usize = 64;

static MAIL: Mutex<Mailbox> = Mutex::new(Mailbox {
    incoming: Vec::new(),
    outgoing: Vec::new(),
    busy: false,
});

fn mail() -> std::sync::MutexGuard<'static, Mailbox> {
    MAIL.lock().unwrap_or_else(|e| e.into_inner())
}

fn push(event: Incoming) {
    let mut m = mail();
    if m.incoming.len() < MAX_INCOMING {
        m.incoming.push(event);
    }
}

/// A battle message from the host as the battle menu takes it (`name` gives a player's name).
pub fn incoming(body: Body, name: impl Fn(u8) -> String) -> Option<Incoming> {
    Some(match body {
        Body::BattleOffer {
            from,
            format,
            save_partners,
            actual_levels,
        } => Incoming::Offer {
            from,
            name: name(from),
            format: format_of(format),
            save_partners,
            actual_levels,
        },
        Body::BattleStart {
            battle,
            format,
            side,
            opponent,
            seed,
            team,
        } => Incoming::Start(Start {
            battle,
            format: format_of(format),
            side: usize::from(side),
            opponent,
            seed,
            team,
        }),
        Body::BattleStep {
            battle,
            step,
            seed,
            picks,
        } => Incoming::Step {
            battle,
            step,
            seed,
            picks,
        },
        Body::BattleEnd {
            battle,
            reason,
            winner,
        } => Incoming::End {
            battle,
            reason,
            winner,
        },
        _ => return None,
    })
}

/// Network thread: a battle message from `from`. Only the host's own count; the host never
/// passes on players' ones.
pub fn received(from: u8, body: Body, name: impl Fn(u8) -> String) {
    if from == SERVER
        && let Some(event) = incoming(body, name)
    {
        push(event);
    }
}

/// Network thread: player `id` left the lobby.
pub fn peer_left(id: u8) {
    push(Incoming::Left(id));
}

/// Network thread: the connection to the host is gone.
pub fn lost() {
    push(Incoming::Lost);
}

/// Game thread: what came in since last time.
pub fn take_incoming() -> Vec<Incoming> {
    std::mem::take(&mut mail().incoming)
}

/// Game thread: messages for the host.
pub fn send(bodies: Vec<Body>) {
    if !bodies.is_empty() {
        mail().outgoing.extend(bodies);
    }
}

/// Network side (online.rs, each frame): the messages to send, reliably.
pub fn take_outgoing() -> Vec<Body> {
    std::mem::take(&mut mail().outgoing)
}

/// The battle menu is in a battle or a challenge (others see us busy).
pub fn set_busy(busy: bool) {
    mail().busy = busy;
}

/// `PlayerState.flags` bits: busy while in a battle or a challenge.
pub fn flags() -> u8 {
    if mail().busy {
        crate::game::POSE_KARATE_BUSY
    } else {
        0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::battle::data::{MoveData, Target, TypeChart};
    use crate::battle::fighter::Stats;

    fn fighter(name: &str, speed: u16, moves: &[MoveData]) -> Fighter {
        let stats = Stats {
            hp: 160,
            attack: 90,
            defense: 80,
            speed,
            sp_attack: 90,
            sp_defense: 80,
        };
        Fighter::new(name, 50, stats, [0, 0], 0, moves)
    }

    fn moves() -> Vec<MoveData> {
        let mut spread = MoveData::plain(3, 0, 45);
        spread.target = Target::BothFoes;
        let mut weak = MoveData::plain(4, 0, 20);
        weak.accuracy = 70;
        vec![
            MoveData::plain(1, 0, 60),
            MoveData::plain(2, 0, 90),
            spread,
            weak,
        ]
    }

    fn battle(format: Format, seed: u64) -> Battle {
        let m = moves();
        let team = |side: usize| -> Vec<Fighter> {
            (0..4)
                .map(|i| fighter(&format!("S{side}M{i}"), 60 + (i as u16 * 7) % 20, &m))
                .collect()
        };
        let dex = Dex::for_tests(&m, TypeChart::default());
        let mut b = Battle::new(
            format,
            [team(0), team(1)],
            dex.chart().clone(),
            dex.rules(),
            seed,
        )
        .unwrap();
        b.set_move_table(dex.table());
        b
    }

    /// Side `side`'s picks for this step, as its player would choose them.
    fn picks_for(b: &Battle, side: usize, rng: &mut Rng) -> Vec<BattlePick> {
        match b.phase() {
            Phase::Actions => choice_picks(&ai::choose(b, side, rng), &[]),
            Phase::Replacements => replace_picks(&ai::replacements(b, side)),
            Phase::Over(_) => Vec::new(),
        }
    }

    #[test]
    fn two_games_fed_the_same_seeds_and_choices_play_the_same_battle() {
        for format in [Format::Singles, Format::Doubles] {
            let mut a = battle(format, 99);
            let mut b = battle(format, 99);
            assert_eq!(a.start(), b.start());
            // Each player chooses in their own game; the host's seeds come from elsewhere.
            let mut players = [Rng::new(1), Rng::new(2)];
            let mut host = Rng::new(3);
            let mut steps = 0;
            while !matches!(a.phase(), Phase::Over(_)) {
                assert_eq!(checksum(&a), checksum(&b), "step {steps}");
                // Side 0 chooses in game A, side 1 in game B.
                let mut picks = Vec::new();
                for (side, (game, rng)) in [&a, &b].into_iter().zip(&mut players).enumerate() {
                    let mine = picks_for(game, side, rng);
                    picks.extend(mine.into_iter().map(|p| (side as u8, p)));
                }
                let seed = u64::from(host.next_u32()) << 32 | u64::from(host.next_u32());
                let ea = apply_step(&mut a, seed, &picks);
                let eb = apply_step(&mut b, seed, &picks);
                assert_eq!(ea, eb);
                assert!(!ea.is_empty());
                steps += 1;
                assert!(steps < 2000);
            }
            assert_eq!(a.phase(), b.phase());
            assert_eq!(checksum(&a), checksum(&b));
        }
    }

    #[test]
    fn a_different_seed_or_choice_changes_the_checksum() {
        let mut a = battle(Format::Singles, 5);
        let mut b = battle(Format::Singles, 5);
        a.start();
        b.start();
        let pick = |value: u8| BattlePick {
            pos: 0,
            kind: PICK_MOVE,
            value,
            target: PICK_NONE,
            pass: PICK_NONE,
        };
        apply_step(&mut a, 1, &[(0, pick(1)), (1, pick(0))]);
        apply_step(&mut b, 1, &[(0, pick(0)), (1, pick(0))]);
        assert_ne!(checksum(&a), checksum(&b));
    }

    #[test]
    fn refused_choices_are_made_by_the_computer_the_same_way() {
        let mut a = battle(Format::Doubles, 8);
        let mut b = battle(Format::Doubles, 8);
        a.start();
        b.start();
        // Side 1 switches both fighters to the same member, and side 0 chooses nothing.
        let switch = |pos: u8| BattlePick {
            pos,
            kind: PICK_SWITCH,
            value: 2,
            target: PICK_NONE,
            pass: PICK_NONE,
        };
        let picks = [(1, switch(0)), (1, switch(1))];
        let ea = apply_step(&mut a, 77, &picks);
        let eb = apply_step(&mut b, 77, &picks);
        assert_eq!(ea, eb);
        assert_eq!(a.turn(), 1);
        assert_eq!(checksum(&a), checksum(&b));
    }

    #[test]
    fn picks_carry_targets_and_passes() {
        let choices = [
            Choice {
                at: Slot::new(1, 0),
                action: Action::Move {
                    index: 2,
                    target: Some(Slot::new(0, 1)),
                },
            },
            Choice {
                at: Slot::new(1, 1),
                action: Action::Switch { member: 5 },
            },
        ];
        let passes = [(Slot::new(1, 0), 3)];
        let picks = choice_picks(&choices, &passes);
        assert_eq!(picks[0].target, 1);
        assert_eq!(picks[0].pass, 3);
        assert_eq!(turn_choices(1, &picks), (choices.to_vec(), passes.to_vec()));
        let replace = [(Slot::new(0, 1), 4)];
        assert_eq!(replacements_of(0, &replace_picks(&replace)), replace);
    }

    #[test]
    fn the_dex_print_follows_the_rom_data() {
        let m = moves();
        let a = Dex::for_tests(&m, TypeChart::default());
        let b = Dex::for_tests(&m, TypeChart::default());
        assert_eq!(dex_print(&a), dex_print(&b));
        let mut other = m.clone();
        other[1].power = 95;
        assert_ne!(
            dex_print(&a),
            dex_print(&Dex::for_tests(&other, TypeChart::default()))
        );
    }

    #[test]
    fn two_computer_sides_play_a_battle_through_the_host() {
        use crate::chao::CHAO_SIZE;
        use crate::net::battles::Battles;
        use crate::net::protocol::ANSWER_ACCEPT;
        let dex = Dex::for_tests(&moves(), TypeChart::default());
        let team = |points: i16| -> Vec<BattleMember> {
            (0..3)
                .map(|i| {
                    let mut c = ChaoData {
                        bytes: [0; CHAO_SIZE],
                    };
                    c.bytes[0x80] = 2;
                    c.bytes[0x81] = 1;
                    for s in 0..7 {
                        let v = points + i * 50;
                        c.bytes[0x38 + s * 2..0x3A + s * 2].copy_from_slice(&v.to_le_bytes());
                    }
                    BattleMember {
                        chao: Box::new(c),
                        species: 0,
                        types: [0, 0],
                        ability: 0,
                        moves: [1, 2, 3, 4],
                        level: 5,
                    }
                })
                .collect()
        };
        let teams = [team(300), team(600)];
        let now = std::time::Instant::now();
        let mut host = Battles::default();
        host.joined(1);
        host.joined(2);
        let mut out = Vec::new();
        let challenge = Body::BattleChallenge {
            to: 2,
            format: 1,
            save_partners: true,
            actual_levels: false,
            dex: dex_print(&dex),
            team: teams[0].clone(),
        };
        host.observe(1, &challenge, now, &mut out);
        let answer = Body::BattleAnswer {
            to: 1,
            answer: ANSWER_ACCEPT,
            dex: dex_print(&dex),
            team: teams[1].clone(),
        };
        out.clear();
        host.observe(2, &answer, now, &mut out);
        let mut duels: Vec<Duel> = out
            .drain(..)
            .map(|o| match incoming(o.body, |_| String::new()) {
                Some(Incoming::Start(start)) => {
                    Duel::new(&dex, &start, &teams[start.side], false).unwrap()
                }
                other => panic!("{other:?}"),
            })
            .collect();
        let mut steps = 0;
        while duels.iter().all(|d| d.over().is_none()) {
            for (d, id) in duels.iter_mut().zip([1, 2]) {
                if let Some(body) = d.choice() {
                    host.observe(id, &body, now, &mut out);
                }
            }
            assert_eq!(out.len(), 2, "step {steps}: {out:?}");
            for (o, d) in out.drain(..).zip(&mut duels) {
                let Some(Incoming::Step {
                    step, seed, picks, ..
                }) = incoming(o.body, |_| String::new())
                else {
                    panic!("no step");
                };
                d.step(step, seed, &picks);
            }
            steps += 1;
            assert!(steps < 2000);
        }
        assert_eq!(duels[0].over(), duels[1].over());
        assert_eq!(duels[0].turn(), duels[1].turn());
        assert_eq!(duels[0].over(), Some(Outcome::Won(1)));
    }

    #[test]
    fn only_the_hosts_messages_reach_the_menu() {
        take_incoming();
        let offer = Body::BattleOffer {
            from: 1,
            format: 1,
            save_partners: false,
            actual_levels: true,
        };
        received(2, offer.clone(), |_| "x".into());
        assert!(take_incoming().is_empty());
        received(SERVER, offer, |id| format!("P{id}"));
        assert_eq!(
            take_incoming(),
            [Incoming::Offer {
                from: 1,
                name: "P1".into(),
                format: Format::Doubles,
                save_partners: false,
                actual_levels: true,
            }]
        );
    }

    #[test]
    fn the_level_rule_sets_every_fighters_level() {
        use crate::chao::CHAO_SIZE;
        let dex = Dex::for_tests(&moves(), TypeChart::default());
        let mut chao = ChaoData {
            bytes: [0; CHAO_SIZE],
        };
        chao.bytes[0x80] = 2;
        let profile = Profile {
            types: [0, 0],
            ability: 0,
            moves: [1, 0, 0, 0],
            partner: None,
            level: 12,
        };
        let m = member(&chao, &profile);
        assert_eq!(m.level, 12);
        assert_eq!(profile_of(&m), profile);
        assert_eq!(fighter_of(&m, &dex, true).level, 12);
        assert_eq!(fighter_of(&m, &dex, false).level, team::FAIR_LEVEL);
        // Only levels in range go out.
        let m = member(
            &chao,
            &Profile {
                level: 0,
                ..profile
            },
        );
        assert_eq!(m.level, MIN_LEVEL);
    }
}
