//! The host's part in live Chao battles (battle/online.rs): it has no ROM and runs no battle,
//! but it pairs the players up, passes the teams on, holds each step's choices until both
//! players' are in, compares their games' checksums and gives every step a fresh seed, so
//! neither game knows the other's choice or the rolls before it chooses.
//!
//! - A challenge (`BattleChallenge`) goes to its target as an offer; their answer starts the
//!   battle (`BattleStart` to both, each with the other's team) or ends the challenge.
//! - A player is in at most one challenge or battle at a time; a challenge to someone busy, or
//!   not in the lobby, ends at once, and so does one to the same player within
//!   `CHALLENGE_GAP` of the last.
//! - Each step: both players' `BattleChoice`, then `BattleStep` to both. Different checksums
//!   end the battle (desync, no winner).
//! - Ends: giving up loses (forfeit), so does leaving the battle or the lobby (disconnect) and
//!   taking more than `STEP_LIMIT` to choose once the other player has (timeout); an
//!   unanswered offer runs out after `OFFER_LIMIT`. Leaving before anyone chose for the first
//!   step has no winner (the challenge taken back, or turned down). A battle ends for good
//!   once both games say it's over; a player still choosing after the other said so gets a
//!   desync end.
//!
//! Its messages come "from" the host (`SERVER`); players' copies of them are dropped.

use std::time::{Duration, Instant};

use super::protocol::{
    ANSWER_ACCEPT, ANSWER_BUSY, BattleMember, BattlePick, Body, END_BUSY, END_DECLINED, END_DESYNC,
    END_DISCONNECT, END_FORFEIT, END_ROM_DIFFERS, END_TIMEOUT, END_WITHDRAWN, LEAVE_GAVE_UP,
    LEAVE_OVER, MAX_PLAYERS, NO_WINNER, SERVER,
};
use super::referee::{Out, To};

/// An offer waits this long for its answer (the target picks their team first).
pub const OFFER_LIMIT: Duration = Duration::from_secs(120);
/// Once one player has chosen for a step, the other has this long.
pub const STEP_LIMIT: Duration = Duration::from_secs(120);
/// A step nobody chooses for in this long ends the battle with no winner.
pub const IDLE_LIMIT: Duration = Duration::from_secs(600);
/// A player can challenge the same player again after this long.
pub const CHALLENGE_GAP: Duration = Duration::from_secs(5);

struct Challenge {
    from: u8,
    to: u8,
    format: u8,
    dex: u32,
    team: Vec<BattleMember>,
    since: Instant,
}

struct Live {
    id: u16,
    /// Side 0 (the challenger) and side 1.
    players: [u8; 2],
    step: u16,
    /// Each side's checksum and picks for `step`.
    choices: [Option<(u32, Vec<BattlePick>)>; 2],
    /// The step opened, and the first choice for it came.
    opened: Instant,
    first: Option<Instant>,
    /// Each side's game said the battle is over.
    over: [bool; 2],
}

impl Live {
    fn side_of(&self, player: u8) -> usize {
        usize::from(self.players[1] == player)
    }
}

#[derive(Default)]
pub struct Battles {
    present: [bool; MAX_PLAYERS],
    challenges: Vec<Challenge>,
    live: Vec<Live>,
    next_id: u16,
    seeds: u64,
    /// Ends for players whose opponent left, sent on the next `tick`.
    queued: Vec<Out>,
    /// Recent challenges: challenger, target, when.
    recent: Vec<(u8, u8, Instant)>,
}

fn end(to: u8, battle: u16, reason: u8, winner: u8) -> Out {
    Out {
        to: To::One(to),
        from: SERVER,
        body: Body::BattleEnd {
            battle,
            reason,
            winner,
        },
    }
}

impl Battles {
    pub fn joined(&mut self, id: u8) {
        if let Some(p) = self.present.get_mut(usize::from(id)) {
            *p = true;
        }
    }

    /// In a challenge, or a battle their game hasn't finished.
    fn busy(&self, id: u8) -> bool {
        self.challenges.iter().any(|c| c.from == id || c.to == id)
            || self
                .live
                .iter()
                .any(|l| l.players.contains(&id) && !l.over[l.side_of(id)])
    }

    /// A new random seed (`RandomState` is seeded from the system's randomness).
    fn seed(&mut self) -> u64 {
        use std::hash::{BuildHasher, Hasher};
        self.seeds = self.seeds.wrapping_add(1);
        let mut h = std::collections::hash_map::RandomState::new().build_hasher();
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_nanos());
        h.write_u128(nanos);
        h.write_u64(self.seeds);
        h.finish()
    }

    fn new_id(&mut self) -> u16 {
        loop {
            self.next_id = self.next_id.wrapping_add(1).max(1);
            let id = self.next_id;
            if !self.live.iter().any(|l| l.id == id) {
                return id;
            }
        }
    }

    /// A battle message from player `from`; true when it's one (none is passed on).
    pub fn observe(&mut self, from: u8, body: &Body, now: Instant, out: &mut Vec<Out>) -> bool {
        match body {
            Body::BattleChallenge {
                to,
                format,
                save_partners,
                dex,
                team,
            } => {
                let present = self.present.get(usize::from(*to)).copied() == Some(true);
                let again = self.recent.iter().any(|&(a, b, at)| {
                    (a, b) == (from, *to) && now.duration_since(at) < CHALLENGE_GAP
                });
                if *to == from || !present || again || self.busy(from) || self.busy(*to) {
                    out.push(end(from, 0, END_BUSY, NO_WINNER));
                } else {
                    self.recent.retain(|&(a, b, _)| (a, b) != (from, *to));
                    self.recent.push((from, *to, now));
                    self.challenges.push(Challenge {
                        from,
                        to: *to,
                        format: *format,
                        dex: *dex,
                        team: team.clone(),
                        since: now,
                    });
                    out.push(Out {
                        to: To::One(*to),
                        from: SERVER,
                        body: Body::BattleOffer {
                            from,
                            format: *format,
                            save_partners: *save_partners,
                        },
                    });
                }
            }
            Body::BattleAnswer {
                to,
                answer,
                dex,
                team,
            } => {
                let Some(i) = self
                    .challenges
                    .iter()
                    .position(|c| c.from == *to && c.to == from)
                else {
                    return true;
                };
                let c = self.challenges.remove(i);
                if *answer != ANSWER_ACCEPT {
                    let reason = if *answer == ANSWER_BUSY {
                        END_BUSY
                    } else {
                        END_DECLINED
                    };
                    out.push(end(c.from, 0, reason, NO_WINNER));
                } else if *dex != c.dex {
                    out.push(end(c.from, 0, END_ROM_DIFFERS, NO_WINNER));
                    out.push(end(from, 0, END_ROM_DIFFERS, NO_WINNER));
                } else {
                    let id = self.new_id();
                    let seed = self.seed();
                    let start = |side: u8, opponent: u8, team: Vec<BattleMember>| Out {
                        to: To::One(if side == 0 { c.from } else { from }),
                        from: SERVER,
                        body: Body::BattleStart {
                            battle: id,
                            format: c.format,
                            side,
                            opponent,
                            seed,
                            team,
                        },
                    };
                    out.push(start(0, from, team.clone()));
                    out.push(start(1, c.from, c.team));
                    self.live.push(Live {
                        id,
                        players: [c.from, from],
                        step: 1,
                        choices: [None, None],
                        opened: now,
                        first: None,
                        over: [false; 2],
                    });
                }
            }
            Body::BattleChoice {
                battle,
                step,
                check,
                picks,
            } => {
                let Some(i) = self
                    .live
                    .iter()
                    .position(|l| l.id == *battle && l.players.contains(&from))
                else {
                    // Over already (or never ours): that game waits for nothing.
                    out.push(end(from, *battle, END_DESYNC, NO_WINNER));
                    return true;
                };
                let seed = self.seed();
                let l = &mut self.live[i];
                let side = l.side_of(from);
                if l.over[1 - side] {
                    // The other game finished the battle and this one plays on: they went apart.
                    out.push(end(from, l.id, END_DESYNC, NO_WINNER));
                    self.live.remove(i);
                    return true;
                }
                if *step != l.step || l.choices[side].is_some() || l.over[side] {
                    return true;
                }
                l.choices[side] = Some((*check, picks.clone()));
                l.first.get_or_insert(now);
                let [Some((a, picks0)), Some((b, picks1))] = &l.choices else {
                    return true;
                };
                if a != b {
                    crate::log!("battles: battle {} went apart at step {}", l.id, l.step);
                    for p in l.players {
                        out.push(end(p, l.id, END_DESYNC, NO_WINNER));
                    }
                    self.live.remove(i);
                    return true;
                }
                let picks: Vec<(u8, BattlePick)> = picks0
                    .iter()
                    .map(|&p| (0, p))
                    .chain(picks1.iter().map(|&p| (1, p)))
                    .collect();
                for p in l.players {
                    out.push(Out {
                        to: To::One(p),
                        from: SERVER,
                        body: Body::BattleStep {
                            battle: l.id,
                            step: l.step,
                            seed,
                            picks: picks.clone(),
                        },
                    });
                }
                l.step = l.step.wrapping_add(1);
                l.choices = [None, None];
                l.opened = now;
                l.first = None;
            }
            Body::BattleLeave { battle: 0, .. } => {
                // A challenge taken back, or an offer turned down.
                let (mine, theirs): (Vec<Challenge>, Vec<Challenge>) = self
                    .challenges
                    .drain(..)
                    .partition(|c| c.from == from || c.to == from);
                self.challenges = theirs;
                for c in mine {
                    if c.from == from {
                        out.push(end(c.to, 0, END_WITHDRAWN, NO_WINNER));
                    } else {
                        out.push(end(c.from, 0, END_DECLINED, NO_WINNER));
                    }
                }
            }
            Body::BattleLeave { battle, reason } => {
                let Some(i) = self
                    .live
                    .iter()
                    .position(|l| l.id == *battle && l.players.contains(&from))
                else {
                    return true;
                };
                let l = &mut self.live[i];
                let side = l.side_of(from);
                let other = 1 - side;
                if *reason == LEAVE_OVER {
                    // At its end each game knows the result: it goes once both say so. A game
                    // that already chose for another step plays on: they went apart.
                    l.over[side] = true;
                    if l.over[other] {
                        self.live.remove(i);
                    } else if l.choices[other].is_some() {
                        let l = self.live.remove(i);
                        out.push(end(l.players[other], l.id, END_DESYNC, NO_WINNER));
                    }
                    return true;
                }
                let l = self.live.remove(i);
                if l.over.contains(&true) {
                    return true;
                }
                let fresh = l.step == 1 && l.choices.iter().all(Option::is_none);
                let (why, winner) = if fresh && *reason != LEAVE_GAVE_UP {
                    // Left before the battle got going: the challenge taken back (a start
                    // that crossed it) or turned down.
                    let why = if side == 0 {
                        END_WITHDRAWN
                    } else {
                        END_DECLINED
                    };
                    (why, NO_WINNER)
                } else if *reason == LEAVE_GAVE_UP {
                    (END_FORFEIT, other as u8)
                } else {
                    (END_DISCONNECT, other as u8)
                };
                out.push(end(l.players[other], l.id, why, winner));
            }
            // Only the host sends these.
            Body::BattleOffer { .. } | Body::BattleStart { .. } | Body::BattleStep { .. } => {}
            Body::BattleEnd { .. } => {}
            _ => return false,
        }
        true
    }

    /// Player `id` left the lobby: their challenges and battle end (sent on the next `tick`).
    pub fn left(&mut self, id: u8) {
        if let Some(p) = self.present.get_mut(usize::from(id)) {
            *p = false;
        }
        let queued = &mut self.queued;
        self.challenges.retain(|c| {
            if c.from == id {
                queued.push(end(c.to, 0, END_DISCONNECT, NO_WINNER));
            } else if c.to == id {
                queued.push(end(c.from, 0, END_DISCONNECT, NO_WINNER));
            }
            c.from != id && c.to != id
        });
        self.live.retain(|l| {
            let Some(side) = l.players.iter().position(|&p| p == id) else {
                return true;
            };
            let other = 1 - side;
            // A game that finished the battle knows its result.
            if !l.over.contains(&true) {
                queued.push(end(l.players[other], l.id, END_DISCONNECT, other as u8));
            }
            false
        });
        self.recent.retain(|&(a, b, _)| a != id && b != id);
    }

    /// Offers and steps that waited too long.
    pub fn tick(&mut self, now: Instant, out: &mut Vec<Out>) {
        out.append(&mut self.queued);
        self.challenges.retain(|c| {
            let waiting = now.duration_since(c.since) < OFFER_LIMIT;
            if !waiting {
                out.push(end(c.from, 0, END_TIMEOUT, NO_WINNER));
                out.push(end(c.to, 0, END_TIMEOUT, NO_WINNER));
            }
            waiting
        });
        self.live.retain(|l| {
            let winner = match l.first {
                Some(at) if now.duration_since(at) >= STEP_LIMIT => {
                    if l.choices[0].is_some() {
                        0
                    } else {
                        1
                    }
                }
                // The other player has `STEP_LIMIT` however long the step was open.
                Some(_) => return true,
                None if now.duration_since(l.opened) >= IDLE_LIMIT => NO_WINNER,
                None => return true,
            };
            for (p, over) in l.players.into_iter().zip(l.over) {
                if !over {
                    out.push(end(p, l.id, END_TIMEOUT, winner));
                }
            }
            false
        });
        self.recent
            .retain(|&(_, _, at)| now.duration_since(at) < CHALLENGE_GAP);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chao::{CHAO_SIZE, ChaoData};
    use crate::net::protocol::{ANSWER_DECLINE, LEAVE_LEFT, PICK_MOVE, PICK_NONE};

    fn team() -> Vec<BattleMember> {
        let mut c = ChaoData {
            bytes: [0; CHAO_SIZE],
        };
        c.bytes[0x80] = 5;
        c.bytes[0x81] = 1;
        vec![BattleMember {
            chao: Box::new(c),
            species: 0,
            types: [0, 0],
            ability: 0,
            moves: [1, 0, 0, 0],
        }]
    }

    fn pick() -> BattlePick {
        BattlePick {
            pos: 0,
            kind: PICK_MOVE,
            value: 0,
            target: PICK_NONE,
            pass: PICK_NONE,
        }
    }

    fn ends(out: &[Out]) -> Vec<(u8, u16, u8, u8)> {
        out.iter()
            .filter_map(|o| match (o.to, &o.body) {
                (
                    To::One(to),
                    Body::BattleEnd {
                        battle,
                        reason,
                        winner,
                    },
                ) => Some((to, *battle, *reason, *winner)),
                _ => None,
            })
            .collect()
    }

    /// Players 1 and 2 in the lobby; 1 challenges 2 and 2 accepts. The battle's id.
    fn started(b: &mut Battles, now: Instant) -> u16 {
        b.joined(1);
        b.joined(2);
        let mut out = Vec::new();
        let challenge = Body::BattleChallenge {
            to: 2,
            format: 0,
            save_partners: false,
            dex: 9,
            team: team(),
        };
        assert!(b.observe(1, &challenge, now, &mut out));
        assert!(matches!(
            out[..],
            [Out {
                to: To::One(2),
                from: SERVER,
                body: Body::BattleOffer {
                    from: 1,
                    format: 0,
                    save_partners: false
                }
            }]
        ));
        out.clear();
        let answer = Body::BattleAnswer {
            to: 1,
            answer: ANSWER_ACCEPT,
            dex: 9,
            team: team(),
        };
        b.observe(2, &answer, now, &mut out);
        let starts: Vec<(To, u16, u8, u8, u64)> = out
            .iter()
            .filter_map(|o| match &o.body {
                Body::BattleStart {
                    battle,
                    side,
                    opponent,
                    seed,
                    ..
                } => Some((o.to, *battle, *side, *opponent, *seed)),
                _ => None,
            })
            .collect();
        assert_eq!(starts.len(), 2);
        assert_eq!((starts[0].0, starts[0].2, starts[0].3), (To::One(1), 0, 2));
        assert_eq!((starts[1].0, starts[1].2, starts[1].3), (To::One(2), 1, 1));
        // The same battle and seed for both.
        assert_eq!((starts[0].1, starts[0].4), (starts[1].1, starts[1].4));
        starts[0].1
    }

    fn choose(b: &mut Battles, from: u8, id: u16, step: u16, check: u32) -> Vec<Out> {
        let mut out = Vec::new();
        let body = Body::BattleChoice {
            battle: id,
            step,
            check,
            picks: vec![pick()],
        };
        assert!(b.observe(from, &body, Instant::now(), &mut out));
        out
    }

    #[test]
    fn a_step_goes_out_once_both_chose() {
        let mut b = Battles::default();
        let id = started(&mut b, Instant::now());
        assert!(choose(&mut b, 1, id, 1, 5).is_empty());
        // A second choice for the same step, or one for another step, doesn't count.
        assert!(choose(&mut b, 1, id, 1, 5).is_empty());
        assert!(choose(&mut b, 2, id, 2, 5).is_empty());
        let out = choose(&mut b, 2, id, 1, 5);
        assert_eq!(out.len(), 2);
        let mut seeds = Vec::new();
        for o in &out {
            let Body::BattleStep {
                battle,
                step,
                seed,
                picks,
            } = &o.body
            else {
                panic!("{o:?}");
            };
            assert_eq!((*battle, *step), (id, 1));
            assert_eq!(picks, &[(0, pick()), (1, pick())]);
            seeds.push(*seed);
        }
        assert_eq!(seeds[0], seeds[1]);
        // The next step.
        assert!(choose(&mut b, 2, id, 2, 6).is_empty());
        assert_eq!(choose(&mut b, 1, id, 2, 6).len(), 2);
    }

    #[test]
    fn different_checksums_end_the_battle() {
        let mut b = Battles::default();
        let id = started(&mut b, Instant::now());
        choose(&mut b, 1, id, 1, 5);
        let out = choose(&mut b, 2, id, 1, 6);
        assert_eq!(
            ends(&out),
            [
                (1, id, END_DESYNC, NO_WINNER),
                (2, id, END_DESYNC, NO_WINNER)
            ]
        );
        assert!(!b.busy(1) && !b.busy(2));
    }

    #[test]
    fn giving_up_leaving_and_disconnecting_lose() {
        for (reason, why) in [(LEAVE_GAVE_UP, END_FORFEIT), (LEAVE_LEFT, END_DISCONNECT)] {
            let mut b = Battles::default();
            let id = started(&mut b, Instant::now());
            choose(&mut b, 1, id, 1, 5);
            let mut out = Vec::new();
            b.observe(
                2,
                &Body::BattleLeave { battle: id, reason },
                Instant::now(),
                &mut out,
            );
            assert_eq!(ends(&out), [(1, id, why, 0)]);
            assert!(!b.busy(1));
        }
        let mut b = Battles::default();
        let id = started(&mut b, Instant::now());
        b.left(1);
        let mut out = Vec::new();
        b.tick(Instant::now(), &mut out);
        assert_eq!(ends(&out), [(2, id, END_DISCONNECT, 1)]);
        // The end of a battle: nothing to tell; it goes once both games say so.
        let mut b = Battles::default();
        let id = started(&mut b, Instant::now());
        let mut out = Vec::new();
        let over = Body::BattleLeave {
            battle: id,
            reason: LEAVE_OVER,
        };
        b.observe(1, &over, Instant::now(), &mut out);
        assert!(out.is_empty() && !b.busy(1) && b.busy(2));
        b.observe(2, &over, Instant::now(), &mut out);
        assert!(out.is_empty() && !b.busy(2) && b.live.is_empty());
    }

    #[test]
    fn an_end_one_game_alone_says_is_a_desync_for_the_other() {
        // The other already chose for the next step.
        let mut b = Battles::default();
        let id = started(&mut b, Instant::now());
        choose(&mut b, 2, id, 1, 5);
        let over = Body::BattleLeave {
            battle: id,
            reason: LEAVE_OVER,
        };
        let mut out = Vec::new();
        b.observe(1, &over, Instant::now(), &mut out);
        assert_eq!(ends(&out), [(2, id, END_DESYNC, NO_WINNER)]);
        assert!(!b.busy(1) && !b.busy(2));
        // Or chooses after.
        let mut b = Battles::default();
        let id = started(&mut b, Instant::now());
        let mut out = Vec::new();
        b.observe(1, &over, Instant::now(), &mut out);
        assert!(out.is_empty());
        let out = choose(&mut b, 2, id, 1, 5);
        assert_eq!(ends(&out), [(2, id, END_DESYNC, NO_WINNER)]);
        assert!(b.live.is_empty());
    }

    #[test]
    fn leaving_before_the_first_choice_has_no_winner() {
        for (from, why, to) in [(1, END_WITHDRAWN, 2), (2, END_DECLINED, 1)] {
            let mut b = Battles::default();
            let id = started(&mut b, Instant::now());
            let mut out = Vec::new();
            let leave = Body::BattleLeave {
                battle: id,
                reason: LEAVE_LEFT,
            };
            b.observe(from, &leave, Instant::now(), &mut out);
            assert_eq!(ends(&out), [(to, id, why, NO_WINNER)]);
            assert!(!b.busy(1) && !b.busy(2));
        }
        // Giving up still loses.
        let mut b = Battles::default();
        let id = started(&mut b, Instant::now());
        let mut out = Vec::new();
        let leave = Body::BattleLeave {
            battle: id,
            reason: LEAVE_GAVE_UP,
        };
        b.observe(1, &leave, Instant::now(), &mut out);
        assert_eq!(ends(&out), [(2, id, END_FORFEIT, 1)]);
    }

    #[test]
    fn a_slow_chooser_loses_and_an_idle_battle_ends() {
        let now = Instant::now();
        let mut b = Battles::default();
        let id = started(&mut b, now);
        choose(&mut b, 2, id, 1, 5);
        let mut out = Vec::new();
        b.tick(now + STEP_LIMIT / 2, &mut out);
        assert!(out.is_empty());
        b.tick(Instant::now() + STEP_LIMIT, &mut out);
        assert_eq!(
            ends(&out),
            [(1, id, END_TIMEOUT, 1), (2, id, END_TIMEOUT, 1)]
        );
        // A choice late in a long step still gives the other player their time.
        let mut b = Battles::default();
        let id = started(&mut b, now);
        let late = now + IDLE_LIMIT - STEP_LIMIT / 4;
        let body = Body::BattleChoice {
            battle: id,
            step: 1,
            check: 5,
            picks: vec![pick()],
        };
        b.observe(1, &body, late, &mut Vec::new());
        let mut out = Vec::new();
        b.tick(now + IDLE_LIMIT, &mut out);
        assert!(out.is_empty());
        b.tick(late + STEP_LIMIT, &mut out);
        assert_eq!(
            ends(&out),
            [(1, id, END_TIMEOUT, 0), (2, id, END_TIMEOUT, 0)]
        );
        let mut b = Battles::default();
        let id = started(&mut b, now);
        let mut out = Vec::new();
        b.tick(now + IDLE_LIMIT, &mut out);
        assert_eq!(
            ends(&out),
            [
                (1, id, END_TIMEOUT, NO_WINNER),
                (2, id, END_TIMEOUT, NO_WINNER)
            ]
        );
    }

    #[test]
    fn challenges_to_busy_or_absent_players_and_answers() {
        let now = Instant::now();
        let mut b = Battles::default();
        b.joined(1);
        let challenge = |to: u8| Body::BattleChallenge {
            to,
            format: 1,
            save_partners: true,
            dex: 1,
            team: team(),
        };
        let mut out = Vec::new();
        // Not in the lobby, or themselves.
        b.observe(1, &challenge(3), now, &mut out);
        b.observe(1, &challenge(1), now, &mut out);
        assert_eq!(
            ends(&out),
            [(1, 0, END_BUSY, NO_WINNER), (1, 0, END_BUSY, NO_WINNER)]
        );
        // Turned down, with a different ROM, busy.
        b.joined(2);
        b.joined(3);
        let mut t = now;
        for (answer, dex, expect) in [
            (ANSWER_DECLINE, 1, vec![(1, 0, END_DECLINED, NO_WINNER)]),
            (ANSWER_BUSY, 1, vec![(1, 0, END_BUSY, NO_WINNER)]),
            (
                ANSWER_ACCEPT,
                2,
                vec![
                    (1, 0, END_ROM_DIFFERS, NO_WINNER),
                    (2, 0, END_ROM_DIFFERS, NO_WINNER),
                ],
            ),
        ] {
            let mut out = Vec::new();
            t += CHALLENGE_GAP;
            b.observe(1, &challenge(2), t, &mut out);
            // Player 3 can't challenge either of them meanwhile.
            b.observe(3, &challenge(2), now, &mut out);
            b.observe(3, &challenge(1), now, &mut out);
            assert_eq!(ends(&out), [(3, 0, END_BUSY, NO_WINNER); 2]);
            let team = if answer == ANSWER_ACCEPT {
                team()
            } else {
                Vec::new()
            };
            let mut out = Vec::new();
            let body = Body::BattleAnswer {
                to: 1,
                answer,
                dex,
                team,
            };
            b.observe(2, &body, now, &mut out);
            assert_eq!(ends(&out), expect);
            assert!(!b.busy(1) && !b.busy(2));
        }
        // Taken back, and run out.
        let mut out = Vec::new();
        t += CHALLENGE_GAP;
        b.observe(1, &challenge(2), t, &mut out);
        let leave = Body::BattleLeave {
            battle: 0,
            reason: LEAVE_LEFT,
        };
        out.clear();
        b.observe(1, &leave, t, &mut out);
        assert_eq!(ends(&out), [(2, 0, END_WITHDRAWN, NO_WINNER)]);
        // Again at once: refused, with no offer.
        out.clear();
        b.observe(1, &challenge(2), t + CHALLENGE_GAP / 2, &mut out);
        assert_eq!(ends(&out), [(1, 0, END_BUSY, NO_WINNER)]);
        assert_eq!(out.len(), 1);
        t += CHALLENGE_GAP;
        b.observe(1, &challenge(2), t, &mut out);
        out.clear();
        b.tick(t + OFFER_LIMIT, &mut out);
        assert_eq!(
            ends(&out),
            [
                (1, 0, END_TIMEOUT, NO_WINNER),
                (2, 0, END_TIMEOUT, NO_WINNER)
            ]
        );
        // Players' own copies of the host's messages go nowhere.
        let mut out = Vec::new();
        let fake = Body::BattleEnd {
            battle: 1,
            reason: END_FORFEIT,
            winner: 0,
        };
        assert!(b.observe(2, &fake, now, &mut out));
        assert!(out.is_empty());
        assert!(!b.observe(2, &Body::Chat { text: "hi".into() }, now, &mut out));
    }
}
