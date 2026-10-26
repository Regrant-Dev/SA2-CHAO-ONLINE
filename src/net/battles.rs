//! The host's part in live Chao battles (battle/online.rs): it has no catalog and runs no battle,
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
//! Multi battles (doubles, each of the four places a player's or the computer's):
//!
//! - The starter's `MultiInvite` goes to each invited player as `MultiOffer`; each answers
//!   with `BattleAnswer` (to the starter). The starter hears of each answer (`MultiStatus`),
//!   and fills a place nobody took with the computer (`MultiFill`) or calls the battle off
//!   (`BattleLeave` 0). It starts (`MultiStart` to every player, with all four teams) once every
//!   place is filled; an invite runs out after `OFFER_LIMIT`, the whole setup after
//!   `SETUP_LIMIT`.
//! - Each step waits for the choices of every player still in it, then goes to them all; a
//!   place the computer plays has no choices in it, and every game picks the computer's from
//!   the step's seed. Different checksums end it for everyone (desync).
//! - A player who gives up, leaves or takes more than `STEP_LIMIT` to choose once someone has
//!   is out (the slow one gets `BattleEnd` replaced): the others hear of it (`MultiTakeover`)
//!   and the computer plays that place from then on. The battle ends in the games as any does,
//!   when a side has no Chao left.
//!
//! Its messages come "from" the host (`SERVER`); players' copies of them are dropped.

use std::time::{Duration, Instant};

use super::protocol::{
    ANSWER_ACCEPT, ANSWER_BUSY, BattleMember, BattlePick, Body, COMPUTER, END_BUSY,
    END_CATALOG_DIFFERS, END_DECLINED, END_DESYNC, END_DISCONNECT, END_FORFEIT, END_REPLACED,
    END_TIMEOUT, END_WITHDRAWN, LEAVE_GAVE_UP, LEAVE_OVER, MAX_MULTI_TEAM, MAX_PLAYERS, MultiSeat,
    NO_WINNER, SEAT_ACCEPTED, SEAT_BUSY, SEAT_CATALOG_DIFFERS, SEAT_DECLINED, SEAT_LEFT,
    SEAT_TIMEOUT, SERVER,
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
/// A multi battle that hasn't started this long after its invites is called off.
pub const SETUP_LIMIT: Duration = Duration::from_secs(300);

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

/// A multi battle place while the battle is set up.
enum Seat {
    /// A player: invited at `since` (`team` none until they accept), or in with their team.
    Player {
        id: u8,
        team: Option<Vec<BattleMember>>,
        since: Instant,
    },
    /// Its player didn't take it: the starter fills it or calls the battle off.
    Open,
    Computer(Vec<BattleMember>),
}

/// A multi battle being set up: place 0 is its starter's.
struct Setup {
    starter: u8,
    seats: [Seat; 4],
    actual_levels: bool,
    dex: u32,
    since: Instant,
}

impl Setup {
    /// The place player `id` has (invited or in).
    fn place_of(&self, id: u8) -> Option<usize> {
        self.seats
            .iter()
            .position(|s| matches!(s, Seat::Player { id: p, .. } if *p == id))
    }
}

/// A multi battle under way.
struct Multi {
    id: u16,
    /// Each place's player (side * 2 + pos); none: the computer plays it, from the start or
    /// since its player went.
    players: [Option<u8>; 4],
    step: u16,
    /// Each place's checksum and picks for `step`.
    choices: [Option<(u32, Vec<BattlePick>)>; 4],
    opened: Instant,
    first: Option<Instant>,
    /// Each place's game said the battle is over.
    over: [bool; 4],
}

impl Multi {
    fn place_of(&self, player: u8) -> Option<usize> {
        self.players.iter().position(|&p| p == Some(player))
    }

    /// The places whose players still play (their game hasn't reached the end), with them.
    fn playing(&self) -> Vec<(usize, u8)> {
        (0..4)
            .filter(|&p| !self.over[p])
            .filter_map(|p| Some((p, self.players[p]?)))
            .collect()
    }
}

#[derive(Default)]
pub struct Battles {
    present: [bool; MAX_PLAYERS],
    challenges: Vec<Challenge>,
    live: Vec<Live>,
    setups: Vec<Setup>,
    multis: Vec<Multi>,
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

fn status(to: u8, place: usize, status: u8) -> Out {
    Out {
        to: To::One(to),
        from: SERVER,
        body: Body::MultiStatus {
            place: place as u8,
            status,
        },
    }
}

impl Battles {
    pub fn joined(&mut self, id: u8) {
        if let Some(p) = self.present.get_mut(usize::from(id)) {
            *p = true;
        }
    }

    /// In a challenge, a multi battle's setup, or a battle their game hasn't finished.
    fn busy(&self, id: u8) -> bool {
        self.challenges.iter().any(|c| c.from == id || c.to == id)
            || self
                .live
                .iter()
                .any(|l| l.players.contains(&id) && !l.over[l.side_of(id)])
            || self.setups.iter().any(|s| s.place_of(id).is_some())
            || self
                .multis
                .iter()
                .any(|m| m.place_of(id).is_some_and(|p| !m.over[p]))
    }

    /// Player `id` is in a challenge, a setup or a battle (`busy`), for other modules' tests.
    #[cfg(test)]
    pub(crate) fn in_battle(&self, id: u8) -> bool {
        self.busy(id)
    }

    fn present(&self, id: u8) -> bool {
        self.present.get(usize::from(id)).copied() == Some(true)
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
            if !self.live.iter().any(|l| l.id == id) && !self.multis.iter().any(|m| m.id == id) {
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
                actual_levels,
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
                            actual_levels: *actual_levels,
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
                    self.multi_answer(from, *to, *answer, *dex, team, now, out);
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
                    out.push(end(c.from, 0, END_CATALOG_DIFFERS, NO_WINNER));
                    out.push(end(from, 0, END_CATALOG_DIFFERS, NO_WINNER));
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
                    if let Some(i) = self.multi_of(*battle, from) {
                        self.multi_choice(i, from, *step, *check, picks, now, out);
                    } else {
                        // Over already (or never ours): that game waits for nothing.
                        out.push(end(from, *battle, END_DESYNC, NO_WINNER));
                    }
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
                // A multi battle called off, or an invite turned down (or left after accepting).
                self.leave_setups(from, END_WITHDRAWN, out);
            }
            Body::BattleLeave { battle, reason } => {
                let Some(i) = self
                    .live
                    .iter()
                    .position(|l| l.id == *battle && l.players.contains(&from))
                else {
                    if let Some(i) = self.multi_of(*battle, from) {
                        self.multi_leave(i, from, *reason, now, out);
                    }
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
            Body::MultiInvite {
                seats,
                actual_levels,
                dex,
                team,
            } => {
                self.invite(from, seats, *actual_levels, *dex, team, now, out);
            }
            Body::MultiFill { place, team } => {
                let place = usize::from(*place);
                if let Some(i) = self.setups.iter().position(|s| s.starter == from)
                    && let Some(seat @ Seat::Open) = self.setups[i].seats.get_mut(place)
                {
                    *seat = Seat::Computer(team.clone());
                    self.try_start(i, now, out);
                }
            }
            // Only the host sends these.
            Body::BattleOffer { .. } | Body::BattleStart { .. } | Body::BattleStep { .. } => {}
            Body::BattleEnd { .. } => {}
            Body::MultiOffer { .. }
            | Body::MultiStatus { .. }
            | Body::MultiStart { .. }
            | Body::MultiTakeover { .. } => {}
            _ => return false,
        }
        true
    }

    /// Player `from`'s multi battle invite, with its rule (actual levels).
    #[allow(clippy::too_many_arguments)]
    fn invite(
        &mut self,
        from: u8,
        seats: &[MultiSeat; 3],
        actual_levels: bool,
        dex: u32,
        team: &[BattleMember],
        now: Instant,
        out: &mut Vec<Out>,
    ) {
        let ids: Vec<u8> = seats
            .iter()
            .filter_map(|s| match s {
                MultiSeat::Player(id) => Some(*id),
                MultiSeat::Computer(_) => None,
            })
            .collect();
        let twice = ids
            .iter()
            .enumerate()
            .any(|(i, id)| *id == from || ids[..i].contains(id));
        if self.busy(from) || twice || team.len() > MAX_MULTI_TEAM {
            out.push(end(from, 0, END_BUSY, NO_WINNER));
            return;
        }
        let mut setup = Setup {
            starter: from,
            seats: [Seat::Open, Seat::Open, Seat::Open, Seat::Open],
            actual_levels,
            dex,
            since: now,
        };
        setup.seats[0] = Seat::Player {
            id: from,
            team: Some(team.to_vec()),
            since: now,
        };
        for (i, seat) in seats.iter().enumerate() {
            setup.seats[i + 1] = match seat {
                MultiSeat::Computer(team) => Seat::Computer(team.clone()),
                MultiSeat::Player(id) if self.present(*id) && !self.busy(*id) => Seat::Player {
                    id: *id,
                    team: None,
                    since: now,
                },
                MultiSeat::Player(_) => {
                    out.push(status(from, i + 1, SEAT_BUSY));
                    Seat::Open
                }
            };
        }
        let players = setup.seats.each_ref().map(|s| match s {
            Seat::Player { id, .. } => *id,
            _ => COMPUTER,
        });
        for (place, seat) in setup.seats.iter().enumerate().skip(1) {
            if let Seat::Player { id, .. } = seat {
                out.push(Out {
                    to: To::One(*id),
                    from: SERVER,
                    body: Body::MultiOffer {
                        from,
                        place: place as u8,
                        actual_levels,
                        players,
                    },
                });
            }
        }
        self.setups.push(setup);
        let i = self.setups.len() - 1;
        self.try_start(i, now, out);
    }

    /// Player `from`'s answer to `starter`'s multi battle invite, if it's one.
    #[allow(clippy::too_many_arguments)]
    fn multi_answer(
        &mut self,
        from: u8,
        starter: u8,
        answer: u8,
        dex: u32,
        team: &[BattleMember],
        now: Instant,
        out: &mut Vec<Out>,
    ) {
        let found = self.setups.iter().enumerate().find_map(|(i, s)| {
            let place = s.place_of(from).filter(|_| s.starter == starter)?;
            matches!(s.seats[place], Seat::Player { team: None, .. }).then_some((i, place))
        });
        let Some((i, place)) = found else {
            return;
        };
        let s = &mut self.setups[i];
        let refused = if answer != ANSWER_ACCEPT {
            Some(if answer == ANSWER_BUSY {
                SEAT_BUSY
            } else {
                SEAT_DECLINED
            })
        } else if dex != s.dex {
            out.push(end(from, 0, END_CATALOG_DIFFERS, NO_WINNER));
            Some(SEAT_CATALOG_DIFFERS)
        } else if team.len() > MAX_MULTI_TEAM {
            out.push(end(from, 0, END_BUSY, NO_WINNER));
            Some(SEAT_BUSY)
        } else {
            None
        };
        out.push(status(starter, place, refused.unwrap_or(SEAT_ACCEPTED)));
        if refused.is_some() {
            s.seats[place] = Seat::Open;
        } else {
            s.seats[place] = Seat::Player {
                id: from,
                team: Some(team.to_vec()),
                since: now,
            };
            self.try_start(i, now, out);
        }
    }

    /// Player `id` leaves the multi battle setups they're in: theirs is called off (the others
    /// get `why`), and a place they were invited to opens for the starter.
    fn leave_setups(&mut self, id: u8, why: u8, out: &mut Vec<Out>) {
        self.setups.retain_mut(|s| {
            if s.starter == id {
                for seat in &s.seats[1..] {
                    if let Seat::Player { id: other, .. } = seat {
                        out.push(end(*other, 0, why, NO_WINNER));
                    }
                }
                return false;
            }
            if let Some(place) = s.place_of(id) {
                let accepted = matches!(s.seats[place], Seat::Player { team: Some(_), .. });
                out.push(status(
                    s.starter,
                    place,
                    if accepted { SEAT_LEFT } else { SEAT_DECLINED },
                ));
                s.seats[place] = Seat::Open;
            }
            true
        });
    }

    /// Starts setup `i` once every place is filled.
    fn try_start(&mut self, i: usize, now: Instant, out: &mut Vec<Out>) {
        let ready = self.setups[i]
            .seats
            .iter()
            .all(|s| matches!(s, Seat::Player { team: Some(_), .. } | Seat::Computer(_)));
        if !ready {
            return;
        }
        let s = self.setups.remove(i);
        let id = self.new_id();
        let seed = self.seed();
        let players = s.seats.each_ref().map(|seat| match seat {
            Seat::Player { id, .. } => Some(*id),
            _ => None,
        });
        let seats = s.seats.map(|seat| match seat {
            Seat::Player {
                id,
                team: Some(team),
                ..
            } => (id, team),
            Seat::Computer(team) => (COMPUTER, team),
            _ => (COMPUTER, Vec::new()),
        });
        crate::log!("battles: multi battle {id} starts ({players:?})");
        for (place, player) in players.iter().enumerate() {
            if let Some(p) = player {
                out.push(Out {
                    to: To::One(*p),
                    from: SERVER,
                    body: Body::MultiStart {
                        battle: id,
                        seed,
                        place: place as u8,
                        actual_levels: s.actual_levels,
                        seats: seats.clone(),
                    },
                });
            }
        }
        self.multis.push(Multi {
            id,
            players,
            step: 1,
            choices: Default::default(),
            opened: now,
            first: None,
            over: [false; 4],
        });
    }

    /// The multi battle `battle` player `from` is in.
    fn multi_of(&self, battle: u16, from: u8) -> Option<usize> {
        self.multis
            .iter()
            .position(|m| m.id == battle && m.place_of(from).is_some())
    }

    /// Player `from`'s choices for a multi battle step (only their own place's count).
    #[allow(clippy::too_many_arguments)]
    fn multi_choice(
        &mut self,
        i: usize,
        from: u8,
        step: u16,
        check: u32,
        picks: &[BattlePick],
        now: Instant,
        out: &mut Vec<Out>,
    ) {
        let m = &mut self.multis[i];
        let Some(place) = m.place_of(from) else {
            return;
        };
        if m.over.contains(&true) && !m.over[place] {
            // Another game finished the battle and this one plays on: they went apart.
            out.push(end(from, m.id, END_DESYNC, NO_WINNER));
            m.players[place] = None;
            self.multi_try_step(i, now, out);
            return;
        }
        if step != m.step || m.choices[place].is_some() || m.over[place] {
            return;
        }
        let own: Vec<BattlePick> = picks
            .iter()
            .filter(|p| usize::from(p.pos) == place % 2)
            .copied()
            .collect();
        m.choices[place] = Some((check, own));
        m.first.get_or_insert(now);
        self.multi_try_step(i, now, out);
    }

    /// Sends multi battle `i`'s step once every player still in it chose (or ends it when they
    /// went apart, or when nobody plays it any more).
    fn multi_try_step(&mut self, i: usize, now: Instant, out: &mut Vec<Out>) {
        let playing = self.multis[i].playing();
        if playing.is_empty() {
            self.multis.remove(i);
            return;
        }
        let m = &self.multis[i];
        if playing.iter().any(|&(p, _)| m.choices[p].is_none()) {
            return;
        }
        let checks: Vec<u32> = playing
            .iter()
            .filter_map(|&(p, _)| Some(m.choices[p].as_ref()?.0))
            .collect();
        if checks.iter().any(|&c| c != checks[0]) {
            crate::log!(
                "battles: multi battle {} went apart at step {}",
                m.id,
                m.step
            );
            for &(_, player) in &playing {
                out.push(end(player, m.id, END_DESYNC, NO_WINNER));
            }
            self.multis.remove(i);
            return;
        }
        let seed = self.seed();
        let m = &mut self.multis[i];
        let picks: Vec<(u8, BattlePick)> = playing
            .iter()
            .filter_map(|&(p, _)| Some((p, m.choices[p].as_ref()?)))
            .flat_map(|(p, (_, picks))| picks.iter().map(move |&pick| ((p / 2) as u8, pick)))
            .collect();
        for &(_, player) in &playing {
            out.push(Out {
                to: To::One(player),
                from: SERVER,
                body: Body::BattleStep {
                    battle: m.id,
                    step: m.step,
                    seed,
                    picks: picks.clone(),
                },
            });
        }
        m.step = m.step.wrapping_add(1);
        m.choices = Default::default();
        m.opened = now;
        m.first = None;
    }

    /// Player `from` is done with multi battle `i` (`LEAVE_*`).
    fn multi_leave(&mut self, i: usize, from: u8, reason: u8, now: Instant, out: &mut Vec<Out>) {
        let m = &mut self.multis[i];
        let Some(place) = m.place_of(from) else {
            return;
        };
        if reason == LEAVE_OVER {
            // At its end each game knows the result. A game that already chose for another
            // step plays on: they went apart.
            m.over[place] = true;
            for (p, player) in m.playing() {
                if m.choices[p].is_some() {
                    out.push(end(player, m.id, END_DESYNC, NO_WINNER));
                    m.players[p] = None;
                }
            }
            if m.playing().is_empty() {
                self.multis.remove(i);
            }
            return;
        }
        if m.over.contains(&true) {
            // The battle reached its end: nothing to take over.
            m.players[place] = None;
            if m.playing().is_empty() {
                self.multis.remove(i);
            }
            return;
        }
        let why = if reason == LEAVE_GAVE_UP {
            END_FORFEIT
        } else {
            END_DISCONNECT
        };
        self.take_over(i, &[place], why, now, out);
    }

    /// The computer plays multi battle `i`'s `places` from now on (their players went, `why`):
    /// the others hear of it, and the step goes on without them.
    fn take_over(&mut self, i: usize, places: &[usize], why: u8, now: Instant, out: &mut Vec<Out>) {
        let m = &mut self.multis[i];
        for &place in places {
            m.players[place] = None;
            m.choices[place] = None;
        }
        for &place in places {
            for (_, player) in m.playing() {
                out.push(Out {
                    to: To::One(player),
                    from: SERVER,
                    body: Body::MultiTakeover {
                        battle: m.id,
                        place: place as u8,
                        reason: why,
                    },
                });
            }
        }
        self.multi_try_step(i, now, out);
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
        let mut out = Vec::new();
        self.leave_setups(id, END_DISCONNECT, &mut out);
        let now = Instant::now();
        for i in (0..self.multis.len()).rev() {
            let m = &mut self.multis[i];
            let Some(place) = m.place_of(id) else {
                continue;
            };
            if m.over.contains(&true) {
                m.players[place] = None;
                if m.playing().is_empty() {
                    self.multis.remove(i);
                }
            } else {
                self.take_over(i, &[place], END_DISCONNECT, now, &mut out);
            }
        }
        self.queued.append(&mut out);
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
        self.setups.retain_mut(|s| {
            if now.duration_since(s.since) >= SETUP_LIMIT {
                for seat in &s.seats {
                    if let Seat::Player { id, .. } = seat {
                        out.push(end(*id, 0, END_TIMEOUT, NO_WINNER));
                    }
                }
                return false;
            }
            for place in 1..4 {
                if let Seat::Player {
                    id,
                    team: None,
                    since,
                } = s.seats[place]
                    && now.duration_since(since) >= OFFER_LIMIT
                {
                    out.push(end(id, 0, END_TIMEOUT, NO_WINNER));
                    out.push(status(s.starter, place, SEAT_TIMEOUT));
                    s.seats[place] = Seat::Open;
                }
            }
            true
        });
        for i in (0..self.multis.len()).rev() {
            let m = &self.multis[i];
            match m.first {
                Some(at) if now.duration_since(at) >= STEP_LIMIT => {
                    // The ones still to choose are out; the computer plays their places.
                    let slow: Vec<(usize, u8)> = m
                        .playing()
                        .into_iter()
                        .filter(|&(p, _)| m.choices[p].is_none())
                        .collect();
                    for &(_, player) in &slow {
                        out.push(end(player, m.id, END_REPLACED, NO_WINNER));
                    }
                    let places: Vec<usize> = slow.iter().map(|s| s.0).collect();
                    self.take_over(i, &places, END_REPLACED, now, out);
                }
                None if now.duration_since(m.opened) >= IDLE_LIMIT => {
                    for (_, player) in m.playing() {
                        out.push(end(player, m.id, END_TIMEOUT, NO_WINNER));
                    }
                    self.multis.remove(i);
                }
                _ => {}
            }
        }
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
            form: 0,
            chao: Box::new(c),
            species: 0,
            types: [0, 0],
            ability: 0,
            moves: [1, 0, 0, 0],
            level: 5,
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
            actual_levels: true,
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
                    actual_levels: true,
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
            actual_levels: false,
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
        // Turned down, with a different catalog, busy.
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
                    (1, 0, END_CATALOG_DIFFERS, NO_WINNER),
                    (2, 0, END_CATALOG_DIFFERS, NO_WINNER),
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
    // ---- multi battles ----

    fn mteam(n: usize) -> Vec<BattleMember> {
        vec![team()[0].clone(); n]
    }

    fn send(b: &mut Battles, from: u8, body: Body, now: Instant) -> Vec<Out> {
        let mut out = Vec::new();
        assert!(b.observe(from, &body, now, &mut out));
        out
    }

    fn statuses(out: &[Out]) -> Vec<(u8, u8, u8)> {
        out.iter()
            .filter_map(|o| match (o.to, &o.body) {
                (To::One(to), Body::MultiStatus { place, status }) => Some((to, *place, *status)),
                _ => None,
            })
            .collect()
    }

    fn takeovers(out: &[Out]) -> Vec<(u8, u8, u8)> {
        out.iter()
            .filter_map(|o| match (o.to, &o.body) {
                (To::One(to), Body::MultiTakeover { place, reason, .. }) => {
                    Some((to, *place, *reason))
                }
                _ => None,
            })
            .collect()
    }

    /// The steps sent: to whom, with which picks (side, pos).
    fn steps(out: &[Out]) -> Vec<(u8, Vec<(u8, u8)>)> {
        out.iter()
            .filter_map(|o| match (o.to, &o.body) {
                (To::One(to), Body::BattleStep { picks, .. }) => {
                    Some((to, picks.iter().map(|(s, p)| (*s, p.pos)).collect()))
                }
                _ => None,
            })
            .collect()
    }

    fn invite(seats: [MultiSeat; 3]) -> Body {
        Body::MultiInvite {
            seats,
            actual_levels: true,
            dex: 9,
            team: mteam(2),
        }
    }

    fn accept(to: u8, dex: u32, n: usize) -> Body {
        Body::BattleAnswer {
            to,
            answer: ANSWER_ACCEPT,
            dex,
            team: mteam(n),
        }
    }

    /// Players 1 to 4 in the lobby; 1 invites 2 (its ally) and 3 (foe 1), the computer foe 2.
    fn invited(b: &mut Battles, now: Instant) -> Vec<Out> {
        for id in 1..=4 {
            b.joined(id);
        }
        let seats = [
            MultiSeat::Player(2),
            MultiSeat::Player(3),
            MultiSeat::Computer(mteam(3)),
        ];
        send(b, 1, invite(seats), now)
    }

    /// ... and both accept: the battle's id.
    fn multi_started(b: &mut Battles, now: Instant) -> u16 {
        invited(b, now);
        send(b, 2, accept(1, 9, 3), now);
        let out = send(b, 3, accept(1, 9, 1), now);
        let ids: Vec<u16> = out
            .iter()
            .filter_map(|o| match &o.body {
                Body::MultiStart { battle, .. } => Some(*battle),
                _ => None,
            })
            .collect();
        assert_eq!(ids.len(), 3);
        ids[0]
    }

    fn mpick(pos: u8) -> BattlePick {
        BattlePick { pos, ..pick() }
    }

    fn mchoose(
        b: &mut Battles,
        from: u8,
        id: u16,
        step: u16,
        check: u32,
        now: Instant,
    ) -> Vec<Out> {
        let pos = u8::from(from == 2);
        let body = Body::BattleChoice {
            battle: id,
            step,
            check,
            picks: vec![mpick(pos)],
        };
        send(b, from, body, now)
    }

    #[test]
    fn a_multi_battle_starts_once_every_invited_player_accepts() {
        let now = Instant::now();
        let mut b = Battles::default();
        let out = invited(&mut b, now);
        let offers: Vec<(To, Body)> = out.into_iter().map(|o| (o.to, o.body)).collect();
        let offer = |place: u8| Body::MultiOffer {
            from: 1,
            place,
            actual_levels: true,
            players: [1, 2, 3, COMPUTER],
        };
        assert_eq!(offers, [(To::One(2), offer(1)), (To::One(3), offer(2))]);
        assert!(b.busy(1) && b.busy(2) && b.busy(3) && !b.busy(4));
        // Nobody can challenge them meanwhile.
        let challenge = Body::BattleChallenge {
            to: 2,
            format: 0,
            actual_levels: true,
            dex: 9,
            team: team(),
        };
        assert_eq!(
            ends(&send(&mut b, 4, challenge, now)),
            [(4, 0, END_BUSY, NO_WINNER)]
        );
        let out = send(&mut b, 2, accept(1, 9, 3), now);
        assert_eq!(statuses(&out), [(1, 1, SEAT_ACCEPTED)]);
        let decline = Body::BattleAnswer {
            to: 1,
            answer: ANSWER_DECLINE,
            dex: 0,
            team: Vec::new(),
        };
        let out = send(&mut b, 3, decline, now);
        assert_eq!(statuses(&out), [(1, 2, SEAT_DECLINED)]);
        assert!(!b.busy(3));
        // Only the starter fills a place, and only one nobody has.
        let fill = |place: u8| Body::MultiFill {
            place,
            team: mteam(2),
        };
        assert!(send(&mut b, 2, fill(2), now).is_empty());
        assert!(send(&mut b, 1, fill(1), now).is_empty());
        let out = send(&mut b, 1, fill(2), now);
        // To whom, the battle, its seed, their place, the players and the team sizes.
        type Started = (To, u16, u64, u8, [u8; 4], [usize; 4]);
        let starts: Vec<Started> = out
            .iter()
            .filter_map(|o| match &o.body {
                Body::MultiStart {
                    battle,
                    seed,
                    place,
                    actual_levels: true,
                    seats,
                } => Some((
                    o.to,
                    *battle,
                    *seed,
                    *place,
                    seats.each_ref().map(|s| s.0),
                    seats.each_ref().map(|s| s.1.len()),
                )),
                _ => None,
            })
            .collect();
        assert_eq!(starts.len(), 2);
        assert_eq!((starts[0].0, starts[0].3), (To::One(1), 0));
        assert_eq!((starts[1].0, starts[1].3), (To::One(2), 1));
        assert_eq!(starts[0].4, [1, 2, COMPUTER, COMPUTER]);
        assert_eq!(starts[0].5, [2, 3, 2, 3]);
        // One battle and seed for everyone.
        assert_eq!((starts[0].1, starts[0].2), (starts[1].1, starts[1].2));
        assert!(b.busy(1) && b.busy(2) && b.setups.is_empty());
    }

    #[test]
    fn multi_invites_that_cant_be_taken() {
        let now = Instant::now();
        let mut b = Battles::default();
        for id in 1..=3 {
            b.joined(id);
        }
        let computer = || MultiSeat::Computer(mteam(1));
        // Twice the same player, or themselves.
        for seats in [
            [MultiSeat::Player(2), MultiSeat::Player(2), computer()],
            [MultiSeat::Player(1), computer(), computer()],
        ] {
            let out = send(&mut b, 1, invite(seats), now);
            assert_eq!(ends(&out), [(1, 0, END_BUSY, NO_WINNER)]);
        }
        // Someone not in the lobby: their place is open at once.
        let out = send(
            &mut b,
            1,
            invite([computer(), MultiSeat::Player(6), MultiSeat::Player(2)]),
            now,
        );
        assert_eq!(statuses(&out), [(1, 2, SEAT_BUSY)]);
        // Busy already.
        let out = send(&mut b, 1, invite([computer(), computer(), computer()]), now);
        assert_eq!(ends(&out), [(1, 0, END_BUSY, NO_WINNER)]);
        // Another catalog, or too big a team.
        let out = send(&mut b, 2, accept(1, 8, 1), now);
        assert_eq!(ends(&out), [(2, 0, END_CATALOG_DIFFERS, NO_WINNER)]);
        assert_eq!(statuses(&out), [(1, 3, SEAT_CATALOG_DIFFERS)]);
        // An answer to an invite that isn't theirs.
        assert!(send(&mut b, 3, accept(1, 9, 1), now).is_empty());
        let mut b = Battles::default();
        invited(&mut b, now);
        let out = send(&mut b, 2, accept(1, 9, MAX_MULTI_TEAM + 1), now);
        assert_eq!(ends(&out), [(2, 0, END_BUSY, NO_WINNER)]);
        assert_eq!(statuses(&out), [(1, 1, SEAT_BUSY)]);
        // An all-computer battle starts at once.
        let mut b = Battles::default();
        b.joined(1);
        let out = send(&mut b, 1, invite([computer(), computer(), computer()]), now);
        assert!(matches!(
            &out[..],
            [Out {
                to: To::One(1),
                body: Body::MultiStart { place: 0, .. },
                ..
            }]
        ));
    }

    #[test]
    fn multi_invites_run_out_or_are_called_off() {
        let now = Instant::now();
        // Unanswered.
        let mut b = Battles::default();
        invited(&mut b, now);
        send(&mut b, 2, accept(1, 9, 1), now);
        let mut out = Vec::new();
        b.tick(now + OFFER_LIMIT, &mut out);
        assert_eq!(ends(&out), [(3, 0, END_TIMEOUT, NO_WINNER)]);
        assert_eq!(statuses(&out), [(1, 2, SEAT_TIMEOUT)]);
        // The whole setup.
        let mut out = Vec::new();
        b.tick(now + SETUP_LIMIT, &mut out);
        assert_eq!(
            ends(&out),
            [
                (1, 0, END_TIMEOUT, NO_WINNER),
                (2, 0, END_TIMEOUT, NO_WINNER)
            ]
        );
        assert!(!b.busy(1) && !b.busy(2));
        // Called off by the starter: the ones invited hear of it.
        let mut b = Battles::default();
        invited(&mut b, now);
        send(&mut b, 2, accept(1, 9, 1), now);
        let leave = Body::BattleLeave {
            battle: 0,
            reason: LEAVE_LEFT,
        };
        let out = send(&mut b, 1, leave.clone(), now);
        assert_eq!(
            ends(&out),
            [
                (2, 0, END_WITHDRAWN, NO_WINNER),
                (3, 0, END_WITHDRAWN, NO_WINNER)
            ]
        );
        assert!(!b.busy(2) && !b.busy(3));
        // A player who accepted and then left: their place opens for the starter.
        let mut b = Battles::default();
        invited(&mut b, now);
        send(&mut b, 2, accept(1, 9, 1), now);
        let out = send(&mut b, 2, leave, now);
        assert_eq!(statuses(&out), [(1, 1, SEAT_LEFT)]);
        assert!(!b.busy(2) && b.busy(3));
        // The starter leaves the lobby.
        b.left(1);
        let mut out = Vec::new();
        b.tick(now, &mut out);
        assert_eq!(ends(&out), [(3, 0, END_DISCONNECT, NO_WINNER)]);
        assert!(b.setups.is_empty());
    }

    #[test]
    fn a_multi_step_waits_for_every_player_in_it() {
        let now = Instant::now();
        let mut b = Battles::default();
        let id = multi_started(&mut b, now);
        assert!(mchoose(&mut b, 1, id, 1, 5, now).is_empty());
        // Player 2's pick for player 1's place doesn't count.
        let body = Body::BattleChoice {
            battle: id,
            step: 1,
            check: 5,
            picks: vec![mpick(0), mpick(1)],
        };
        assert!(send(&mut b, 2, body, now).is_empty());
        let out = mchoose(&mut b, 3, id, 1, 5, now);
        let picks = vec![(0, 0), (0, 1), (1, 0)];
        assert_eq!(
            steps(&out),
            [(1, picks.clone()), (2, picks.clone()), (3, picks)]
        );
        // Different checksums: it ends for everyone.
        mchoose(&mut b, 1, id, 2, 6, now);
        mchoose(&mut b, 2, id, 2, 6, now);
        let out = mchoose(&mut b, 3, id, 2, 7, now);
        assert_eq!(
            ends(&out),
            [
                (1, id, END_DESYNC, NO_WINNER),
                (2, id, END_DESYNC, NO_WINNER),
                (3, id, END_DESYNC, NO_WINNER)
            ]
        );
        assert!(b.multis.is_empty() && !b.busy(1));
    }

    #[test]
    fn the_computer_takes_over_a_player_who_goes() {
        let now = Instant::now();
        let mut b = Battles::default();
        let id = multi_started(&mut b, now);
        // Player 2 gives up after player 1 chose.
        mchoose(&mut b, 1, id, 1, 5, now);
        let give_up = Body::BattleLeave {
            battle: id,
            reason: LEAVE_GAVE_UP,
        };
        let out = send(&mut b, 2, give_up, now);
        assert_eq!(takeovers(&out), [(1, 1, END_FORFEIT), (3, 1, END_FORFEIT)]);
        assert!(!b.busy(2));
        // The step goes out without them.
        let out = mchoose(&mut b, 3, id, 1, 5, now);
        assert_eq!(
            steps(&out),
            [(1, vec![(0, 0), (1, 0)]), (3, vec![(0, 0), (1, 0)])]
        );
        // Player 3 takes too long.
        mchoose(&mut b, 1, id, 2, 5, now);
        let mut out = Vec::new();
        b.tick(now + STEP_LIMIT / 2, &mut out);
        assert!(out.is_empty());
        b.tick(Instant::now() + STEP_LIMIT, &mut out);
        assert_eq!(ends(&out), [(3, id, END_REPLACED, NO_WINNER)]);
        assert_eq!(takeovers(&out), [(1, 2, END_REPLACED)]);
        assert_eq!(steps(&out), [(1, vec![(0, 0)])]);
        // Choices from players no longer in it are turned away.
        let out = mchoose(&mut b, 3, id, 3, 5, now);
        assert_eq!(ends(&out), [(3, id, END_DESYNC, NO_WINNER)]);
        // The last player leaves the lobby: the battle is gone.
        b.left(1);
        let mut out = Vec::new();
        b.tick(now, &mut out);
        assert!(out.is_empty());
        assert!(b.multis.is_empty());
    }

    #[test]
    fn a_player_leaving_the_lobby_is_taken_over_and_the_end_comes_from_the_games() {
        let now = Instant::now();
        let mut b = Battles::default();
        let id = multi_started(&mut b, now);
        mchoose(&mut b, 1, id, 1, 5, now);
        mchoose(&mut b, 2, id, 1, 5, now);
        b.left(3);
        let mut out = Vec::new();
        b.tick(now, &mut out);
        assert_eq!(
            takeovers(&out),
            [(1, 2, END_DISCONNECT), (2, 2, END_DISCONNECT)]
        );
        assert_eq!(steps(&out).len(), 2);
        // One game reaches the end while the other already chose again: they went apart.
        mchoose(&mut b, 2, id, 2, 5, now);
        let over = Body::BattleLeave {
            battle: id,
            reason: LEAVE_OVER,
        };
        let out = send(&mut b, 1, over.clone(), now);
        assert_eq!(ends(&out), [(2, id, END_DESYNC, NO_WINNER)]);
        assert!(b.multis.is_empty());
        // Both reach it: it goes quietly.
        let mut b = Battles::default();
        let id = multi_started(&mut b, now);
        let over = Body::BattleLeave {
            battle: id,
            reason: LEAVE_OVER,
        };
        for p in [1, 2] {
            assert!(send(&mut b, p, over.clone(), now).is_empty());
        }
        assert!(!b.busy(1) && b.busy(3));
        assert!(send(&mut b, 3, over, now).is_empty());
        assert!(b.multis.is_empty());
    }

    #[test]
    fn hostile_multi_messages_change_nothing() {
        let now = Instant::now();
        let mut b = Battles::default();
        let id = multi_started(&mut b, now);
        // Players' copies of the host's messages.
        for body in [
            Body::MultiOffer {
                from: 2,
                place: 1,
                actual_levels: true,
                players: [COMPUTER; 4],
            },
            Body::MultiStatus {
                place: 1,
                status: SEAT_ACCEPTED,
            },
            Body::MultiStart {
                battle: id,
                seed: 1,
                place: 0,
                actual_levels: true,
                seats: std::array::from_fn(|_| (COMPUTER, mteam(1))),
            },
            Body::MultiTakeover {
                battle: id,
                place: 1,
                reason: END_FORFEIT,
            },
            Body::MultiFill {
                place: 2,
                team: mteam(1),
            },
            accept(1, 9, 1),
        ] {
            for from in [1, 2, 4] {
                assert!(send(&mut b, from, body.clone(), now).is_empty(), "{body:?}");
            }
        }
        // Someone not in it choosing, or leaving.
        let out = mchoose(&mut b, 4, id, 1, 5, now);
        assert_eq!(ends(&out), [(4, id, END_DESYNC, NO_WINNER)]);
        let leave = Body::BattleLeave {
            battle: id,
            reason: LEAVE_GAVE_UP,
        };
        assert!(send(&mut b, 4, leave, now).is_empty());
        // A step for later, or twice.
        assert!(mchoose(&mut b, 1, id, 2, 5, now).is_empty());
        assert!(mchoose(&mut b, 1, id, 1, 5, now).is_empty());
        assert!(mchoose(&mut b, 1, id, 1, 5, now).is_empty());
        mchoose(&mut b, 2, id, 1, 5, now);
        assert_eq!(steps(&mchoose(&mut b, 3, id, 1, 5, now)).len(), 3);
    }
}
