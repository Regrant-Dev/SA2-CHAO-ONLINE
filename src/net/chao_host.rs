//! The host (the dedicated server, or a player hosting) runs shared gardens' Chao (chao_sim).
//!
//! Players' games send each garden's ground once (`GardenLand`); the copy most of them agree
//! on is the one used. Visiting Chao come from their owners' records (`Body::Chao`) and own
//! streams (`GardenChao`: which are out, and where they start); the server then runs them and
//! sends them to everyone in the garden as `ServerChao`, which they follow instead of their
//! owners' games. A garden with fewer than two players is left to its player's game, as before.
//!
//! The Chao sense the gardens' items, toys and trees as the referee has them (`Shared`). A
//! fruit, seed, animal or hat one of them takes becomes the server's, in its hands, until it's
//! eaten, put on or let go (`ItemAct`); a toy it touches is switched on by the toys' keeper.
//! Trees stay the trees' keeper's.
//!
//! What a Chao's AI changes in its record goes to its owner (`ChaoChanges`) now and then, like
//! a keeper's game does: not its life tick's (the owner's game ticks it too), and not while a
//! player holds or pets it (their game runs it then and sends its own). Its sounds go to
//! everyone in the garden (`ChaoSounds`).
//!
//! It also shows the Chao players bring while they aren't in that garden (net/roster.rs, kept
//! by their key): with their owner online elsewhere they're awake (run here as any, their
//! whole life going to the owner's save as `SaveChao`, evolving here too); with them offline
//! they sleep (only carried about). They go by owner ids `HELD_OWNER..` (`HeldChao` tells
//! everyone which is which), and a garden with one player runs for them too. When the owner
//! comes into that garden, their game has it again and the host's copy goes.
//!
//! A Chao its owner's `ChaoLock` locks now (`locked_against`) is the same Chao to everyone:
//! it notices, follows and answers others' whistles, looks and characters, is held, thrown and
//! petted by them, and reacts to all of it as ever. Only its save doesn't keep what they do:
//! their `ChaoChanges` are dropped (the referee doesn't pass them on either), their touches'
//! moods aren't taken, their petting and holding don't hand its changes to them, and while it
//! deals with one of them (`with_whom`) its brain's record (`Chao::rec`) lives through it but
//! what goes to its owner (`Chao::kept`) lives only its life ticks and its owner's doings;
//! once they're done the brain goes on from the kept record. It takes only its owner's and the
//! server's items (eating another's would change it and use theirs up). Everyone is told which
//! Chao are locked against them (`ChaoLocked`); an owner loading on from a garden or elsewhere
//! in Chao World still counts as there for a while (`lock_area`).

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::chao::{ChaoData, changes};
use crate::chao_sim::brain::{Chao, FruitEvent, ItemEvent, LifeEvent, TreeEvent};
use crate::chao_sim::evolve::evolve;
use crate::chao_sim::land::V3;
use crate::chao_sim::land::{Garden, fell_through};
use crate::chao_sim::motion::MotionTable;
use crate::chao_sim::picker::{Ball, Carried, Fruit, Release, Senses, Signal, ToyEvent, Tree};
use crate::chao_sim::record::{Record, off};
use crate::chao_sim::world::World;
use crate::chao_sim::world::{Around, in_view};

use super::protocol::{
    BALL_HELD, BALL_IN_WATER, Body, BroughtChao, CHAO_LIVE_FIELDS, CHAO_MOOD, ChaoLock, ChaoSound,
    GardenChao, GardenItem, HELD_OWNER, HeldChao, ItemId, ItemPlace, MAX_CARRIED_CHAO,
    MAX_CHAO_OWNER, MAX_CHAO_SOUNDS, PlayerKey, PlayerState, SAVE_CHAO_FIELDS, SOUND_AT, TOY_BOX,
    TOY_RADIO, TOY_TV, TREE_PLANT, TREE_SLOTS, TREE_WATER, lock_place,
};
use super::referee::{Out, To};
use super::roster::{Candidate, ChaoId, Roster, choose};

/// The game's frame rate.
const FRAME: Duration = Duration::from_micros(16_667);
/// At most this many frames are caught up at once (after a stall, the rest are dropped).
const MAX_CATCH_UP: u32 = 30;
/// `ServerChao` every 2 frames, like `GardenChao`.
const SEND_EVERY: u64 = 2;
/// A player's state, an owner's stream or a hold counts this long after it's heard.
const PLAYER_STALE: Duration = Duration::from_millis(1500);
const VISIT_STALE: Duration = Duration::from_secs(8);
const HELD_STALE: Duration = Duration::from_millis(500);
/// Players in a garden before the server runs it.
const MIN_PLAYERS: usize = 2;
/// A run goes on this long with too few players (one stepping out briefly, a late state).
const RUN_GRACE: Duration = Duration::from_secs(15);
/// The same sound of the same Chao goes out at most this often (frames).
const SOUND_AGAIN: u64 = 30;
/// A Chao's record changes go to its owner every second.
const CHANGES_EVERY: u64 = 60;
/// After a player holds, pets or changes a Chao, its changes are theirs to send this long.
const TOUCH_QUIET: Duration = Duration::from_secs(3);
/// The Chao's voice call (`0x4372E0`) as the behaviours make it: no holder, volume 0x6E.
const VOICE_ARGS: [i16; 3] = [0, 0, 0x6E];
/// The sender id the server's own Chao messages carry.
use super::protocol::SERVER;
/// Chao a garden holds at once (the game's ALW Chao limit `0x8AB838`): the present players'
/// own first, then held ones.
const ROOM: usize = 32;
/// Held Chao are looked over (shown, hidden, woken, put to sleep, told about) this often
/// (frames).
const HELD_EVERY: u64 = 30;
/// A player who came into a garden has their own held Chao there back in their game once
/// their stream for it comes, or after this long anyway.
const HANDOVER: Duration = Duration::from_secs(5);
/// A held Chao's owner id let go isn't given out again for this long (copies of the old Chao
/// may still be about).
const REUSE_HELD: Duration = Duration::from_secs(20);
/// Last-seen times are written and old players dropped this often (frames): a minute.
const ROSTER_EVERY: u64 = 3600;
/// A sleeping Chao: lying in the sleeping behaviour's second pose (`0x54EF10` mode 2: motion
/// 0x13, eyes 7). Held, it plays the game's held motion.
const SLEEP_MOTION: u16 = 0x13;
const SLEEP_EYES: i16 = 7;
const HELD_MOTION: u16 = 0x101;
/// A player who leaves a garden for a load (`POSE_LOADING`) still counts as in it for their
/// Chao's locks this long, or until a settled state says where they are.
const AREA_GRACE: Duration = Duration::from_secs(10);

type Key = (u8, u8);

/// A garden's ball: position, velocity, `Ball::flags`, when heard.
type BallSeen = ([f32; 3], [f32; 3], u8, Instant);

struct Player {
    area: u8,
    /// In `area` since then.
    since: Instant,
    /// Where they last were for their Chao's locks (`lock_place`) and, while they're loading
    /// on from there, since when (`lock_area`); where they are now.
    garden: u8,
    left: Option<Instant>,
    place: u8,
    position: [f32; 3],
    rotation_y: i32,
    character: u8,
    action: u8,
    at: Instant,
}

/// A garden's ground as one player sent it, and who sent the same.
struct Land {
    garden: Garden,
    voters: BTreeSet<u8>,
}

/// Parts of a garden's ground still coming from one player.
struct Upload {
    fingerprint: u32,
    parts: Vec<Option<Vec<u8>>>,
}

/// A garden being run.
struct Run {
    world: World,
    /// World slot of each visiting Chao.
    slots: BTreeMap<Key, usize>,
    /// The ground it runs on.
    fingerprint: u32,
    /// Since when too few players are here (it goes back to their games after `RUN_GRACE`).
    short: Option<Instant>,
}

#[derive(Default)]
pub struct ChaoHost {
    players: HashMap<u8, Player>,
    records: HashMap<Key, Box<ChaoData>>,
    /// Owners' own streams: where each of their Chao is in their game, and when heard.
    own: HashMap<Key, (GardenChao, Instant)>,
    held: HashMap<Key, (u8, [f32; 3], Instant)>,
    /// Where a player's game has a Chao it runs for a touch (`ChaoTouched`): position, facing,
    /// motion; and whose game that is.
    touched: HashMap<Key, ([f32; 3], i32, u16, Instant)>,
    touched_by: HashMap<Key, u8>,
    /// How each touched Chao moves between its toucher's reports (thrown: it flies).
    flights: HashMap<Key, Flight>,
    /// Each garden's ball as its kicker last sent it.
    balls: [Option<BallSeen>; 3],
    uploads: HashMap<(u8, u8), Upload>,
    lands: [Vec<(u32, Land)>; 3],
    /// The Chao motion table as players sent it, and who sent each.
    tables: Vec<(u32, Arc<MotionTable>, BTreeSet<u8>)>,
    runs: [Option<Run>; 3],
    /// Each running Chao's record as its owner last has it (sent or theirs), and until when a
    /// player's touch keeps its changes theirs.
    sent: HashMap<Key, Box<ChaoData>>,
    quiet: HashMap<Key, Instant>,
    /// The gardens' items as the referee has them (area, owner, item), kept up to date with
    /// what the Chao here do to them between its looks.
    items: Vec<(u8, u8, GardenItem)>,
    /// What each Chao carries that isn't a fruit.
    carried: HashMap<Key, (ItemId, Carried)>,
    /// What the Chao did to items, for the referee.
    acts: Vec<ItemAct>,
    /// Items a Chao threw, in the air.
    flying: Vec<Flying>,
    /// When each Chao's sound of each id last went out (frame).
    sounded: HashMap<(Key, u16), u64>,
    /// Each garden's toys' modes (`TOY_TV`, `TOY_RADIO`, `TOY_BOX`; 0 off).
    toy_modes: [[u8; 3]; 3],
    tree_states: [[u8; TREE_SLOTS]; 3],
    /// Each garden's trees that want water (a bit per spot).
    tree_thirst: [u8; 3],
    clock: Option<Instant>,
    frames: u64,
    /// Each player's key and name (`joined`), and whether their save takes `SaveChao`
    /// (`Brought::awake`).
    keys: HashMap<u8, (PlayerKey, String)>,
    awake: HashMap<u8, bool>,
    /// What everyone brings, by key.
    roster: Roster,
    /// Held Chao shown now, by their owner id and index (`HELD_OWNER..`).
    shown: BTreeMap<Key, Shown>,
    /// Held ids let go, and when.
    freed: HashMap<Key, Instant>,
    /// What each player was last told of the held Chao (`HeldChao`): id -> `Shown::version`.
    told: HashMap<u8, HashMap<Key, u64>>,
    versions: u64,
    /// Each garden has held Chao waiting for someone to be there.
    held_here: [bool; 3],
    /// Messages decided outside a tick, sent with the next.
    outbox: Vec<Out>,
    /// What each player was last told is locked against them (`ChaoLocked`).
    told_locks: HashMap<u8, Vec<Key>>,
    /// The time of the latest message or tick (locks look at where owners are then).
    now: Option<Instant>,
}

/// A Chao a player's game runs for a touch, between their reports (`ChaoTouched`).
#[derive(Clone, Copy, Debug)]
struct Flight {
    /// Where the last report put it, and when it came.
    pos: V3,
    heard: Instant,
    /// Per frame, from the last two reports: none for a jump or a gap.
    vel: V3,
}

/// A held Chao shown in a garden.
#[derive(Clone, Copy, Debug)]
struct Shown {
    id: ChaoId,
    area: u8,
    /// Its owner is offline: it sleeps (not in the world, only carried about).
    asleep: bool,
    /// Its owner's `ChaoLock` locks it (as last looked at).
    locked: bool,
    /// Changes when what players are told of it does (`HeldChao`).
    version: u64,
}

impl ChaoHost {
    pub fn new() -> Self {
        Self::default()
    }

    /// Sees player `from`'s game message (the referee passes everything on).
    pub fn observe(&mut self, from: u8, body: &Body, now: Instant) {
        self.now = Some(now);
        match body {
            // Locked against them: nothing of theirs reaches its owner.
            Body::ChaoChanges { owner, index, .. }
                if self.locked_against(from, (*owner, *index)) => {}
            Body::PlayerState(state) => self.player(from, state, now),
            Body::Chao { index, data } if usize::from(*index) < MAX_CARRIED_CHAO => {
                let key = (from, *index);
                self.records.insert(key, data.clone());
                // The owner's record, with what the server changed and hasn't sent yet, and
                // the server's own emotions and needs (its life here; the owner's game takes
                // them from `ChaoLive`, not the other way).
                let pending = self.sent.get(&key).cloned();
                if let Some(chao) = self.chao_mut(key) {
                    let rebase = |now: &Record| {
                        let mut rec = (**data).clone();
                        if let Some(sent) = &pending {
                            changes::apply(&mut rec, &changes::diff(sent, &now.0));
                        }
                        let (at, len) = CHAO_MOOD;
                        rec.bytes[at..at + len].copy_from_slice(&now.0.bytes[at..at + len]);
                        Record(rec)
                    };
                    // Both: the brain's, and the one its save keeps (`Chao::saved`).
                    chao.saved = chao.saved.as_ref().map(rebase);
                    chao.rec = rebase(&chao.rec);
                    self.sent.insert(key, data.clone());
                }
            }
            Body::GardenChao { owner, list } if *owner == from => {
                for c in list {
                    self.own.insert((from, c.index), (*c, now));
                }
            }
            Body::ChaoHeld {
                owner,
                index,
                offset,
            } => {
                self.held.insert((*owner, *index), (from, *offset, now));
            }
            Body::ChaoTouched {
                owner,
                index,
                position,
                rotation_y,
                motion,
                mood,
            } => {
                let key = (*owner, *index);
                self.touched
                    .insert(key, (*position, *rotation_y, *motion, now));
                self.touched_by.insert(key, from);
                let vel = self.flights.get(&key).map_or([0.0; 3], |f| {
                    flight_velocity(f.pos, *position, now.duration_since(f.heard))
                });
                self.flights.insert(
                    key,
                    Flight {
                        pos: *position,
                        heard: now,
                        vel,
                    },
                );
                // Their game runs it now: they let go of it (thrown, dropped), whatever the
                // last `ChaoHeld` says.
                if self.held.get(&key).is_some_and(|h| h.0 == from) {
                    self.held.remove(&key);
                }
                // How the touch left it (hit: angry, scared; petted: glad): it goes on so,
                // unless it's locked against them.
                let (at, len) = CHAO_MOOD;
                if mood.len() == len
                    && !self.locked_against(from, key)
                    && let Some(chao) = self.chao_mut(key)
                {
                    chao.change(|rec| rec.0.bytes[at..at + len].copy_from_slice(mood));
                }
            }
            // A held Chao: an awake one's life takes it (its owner gets it as `SaveChao`); a
            // sleeping one changes in nothing.
            Body::ChaoChanges {
                owner,
                index,
                changes: list,
            } if *owner >= HELD_OWNER => {
                if let Some(chao) = self.chao_mut((*owner, *index)) {
                    chao.change(|rec| changes::apply(&mut rec.0, list));
                }
            }
            Body::ChaoChanges {
                owner,
                index,
                changes: list,
            } => {
                if let Some(record) = self.records.get_mut(&(*owner, *index)) {
                    changes::apply(record, list);
                }
                let key = (*owner, *index);
                if let Some(chao) = self.chao_mut(key) {
                    chao.change(|rec| changes::apply(&mut rec.0, list));
                }
                // Its sender sends these to the owner too.
                if let Some(sent) = self.sent.get_mut(&key) {
                    changes::apply(sent, list);
                }
                self.quiet.insert(key, now + TOUCH_QUIET);
            }
            Body::Ball {
                area,
                position,
                velocity,
                flags,
                ..
            } => {
                if let Some(ball) = self.balls.get_mut(usize::from(*area).wrapping_sub(1)) {
                    *ball = Some((*position, *velocity, *flags, now));
                }
            }
            Body::GardenLand {
                garden,
                fingerprint,
                part,
                parts,
                bytes,
            } => self.land_part(from, *garden, *fingerprint, *part, *parts, bytes),
            Body::Brought { awake, lock, list } => self.brought(from, *awake, *lock, list, now),
            _ => {}
        }
    }

    fn player(&mut self, from: u8, state: &PlayerState, now: Instant) {
        let area =
            if state.level == crate::game::LEVEL_CHAO_WORLD && (1..=3).contains(&state.chao_area) {
                state.chao_area as u8
            } else {
                0
            };
        let place = lock_place(
            state.level == crate::game::LEVEL_CHAO_WORLD,
            state.chao_area,
        );
        let prev = self.players.get(&from);
        let since = prev.filter(|p| p.area == area).map_or(now, |p| p.since);
        // Loading on from a garden or elsewhere in Chao World: still there for their locks a
        // while (`lock_area`). A settled state anywhere else says where they are.
        let loading = state.flags & crate::game::POSE_LOADING != 0;
        let (garden, left) = match prev {
            _ if area != 0 => (area, None),
            Some(p) if loading && p.garden != 0 => (p.garden, Some(p.left.unwrap_or(now))),
            // Settled elsewhere in Chao World.
            _ if place != 0 => (place, None),
            _ => (0, None),
        };
        self.players.insert(
            from,
            Player {
                area,
                since,
                garden,
                left,
                place,
                position: state.position,
                rotation_y: state.rotation[1],
                character: state.character,
                action: state.action,
                at: now,
            },
        );
    }

    /// Player `id` left: their Chao go (and sleep where they were).
    pub fn left(&mut self, id: u8) {
        for index in 0..3 {
            let mine: Vec<Key> = self.runs[index]
                .iter()
                .flat_map(|r| r.slots.keys())
                .filter(|k| k.0 == id)
                .copied()
                .collect();
            for key in mine {
                self.remember(key, index as u8 + 1);
            }
        }
        self.keys.remove(&id);
        self.awake.remove(&id);
        self.told.remove(&id);
        self.told_locks.remove(&id);
        self.players.remove(&id);
        self.records.retain(|&(o, _), _| o != id);
        self.sent.retain(|&(o, _), _| o != id);
        self.quiet.retain(|&(o, _), _| o != id);
        self.own.retain(|&(o, _), _| o != id);
        self.held.retain(|_, (holder, _, _)| *holder != id);
        self.touched.retain(|&(o, _), _| o != id);
        self.touched_by.retain(|&(o, _), by| o != id && *by != id);
        self.flights.retain(|&(o, _), _| o != id);
        self.sounded.retain(|&((o, _), _), _| o != id);
        self.uploads.retain(|&(f, _), _| f != id);
        for lands in &mut self.lands {
            for (_, land) in lands.iter_mut() {
                land.voters.remove(&id);
            }
        }
        for (_, _, voters) in &mut self.tables {
            voters.remove(&id);
        }
    }

    fn land_part(
        &mut self,
        from: u8,
        garden: u8,
        fingerprint: u32,
        part: u8,
        parts: u8,
        bytes: &[u8],
    ) {
        let key = (from, garden);
        let upload = self.uploads.entry(key).or_insert_with(|| Upload {
            fingerprint,
            parts: vec![None; usize::from(parts)],
        });
        if upload.fingerprint != fingerprint || upload.parts.len() != usize::from(parts) {
            *upload = Upload {
                fingerprint,
                parts: vec![None; usize::from(parts)],
            };
        }
        upload.parts[usize::from(part)] = Some(bytes.to_vec());
        if upload.parts.iter().any(Option::is_none) {
            return;
        }
        let whole: Vec<u8> = upload.parts.iter().flatten().flatten().copied().collect();
        self.uploads.remove(&key);
        if garden == 3 {
            self.table_sent(from, fingerprint, &whole);
            return;
        }
        let Some(lands) = self.lands.get_mut(usize::from(garden)) else {
            return;
        };
        let Some(land) = Garden::from_bytes(&whole).filter(|g| g.fingerprint() == fingerprint)
        else {
            crate::log!("chao host: player {from}'s garden {garden} ground didn't check out");
            return;
        };
        for (_, l) in lands.iter_mut() {
            l.voters.remove(&from);
        }
        match lands.iter_mut().find(|(f, _)| *f == fingerprint) {
            Some((_, l)) => {
                l.voters.insert(from);
            }
            None => {
                crate::log!(
                    "chao host: garden {garden} ground {fingerprint:08x} ({} triangles) from player {from}",
                    land.tris.len()
                );
                lands.push((
                    fingerprint,
                    Land {
                        garden: land,
                        voters: BTreeSet::from([from]),
                    },
                ));
            }
        }
    }

    /// Player `from`'s motion table.
    fn table_sent(&mut self, from: u8, fingerprint: u32, bytes: &[u8]) {
        let Some(table) = MotionTable::from_bytes(bytes).filter(|t| t.fingerprint() == fingerprint)
        else {
            crate::log!("chao host: player {from}'s motion table didn't check out");
            return;
        };
        for (_, _, voters) in &mut self.tables {
            voters.remove(&from);
        }
        match self.tables.iter_mut().find(|(f, _, _)| *f == fingerprint) {
            Some((_, _, voters)) => {
                voters.insert(from);
            }
            None => {
                crate::log!(
                    "chao host: motion table {fingerprint:08x} ({} motions) from player {from}",
                    table.0.len()
                );
                self.tables
                    .push((fingerprint, Arc::new(table), BTreeSet::from([from])));
            }
        }
    }

    /// The motion table most players sent (the first sent on a tie).
    fn table(&self) -> Option<Arc<MotionTable>> {
        self.tables
            .iter()
            .filter(|(_, _, v)| !v.is_empty())
            .fold(
                None,
                |best: Option<&(u32, Arc<MotionTable>, BTreeSet<u8>)>, t| match best {
                    Some(b) if b.2.len() >= t.2.len() => Some(b),
                    _ => Some(t),
                },
            )
            .map(|(_, t, _)| t.clone())
    }

    /// The ground most players sent for garden `index` (the first sent on a tie).
    fn land(&self, index: usize) -> Option<(u32, &Garden)> {
        self.lands[index]
            .iter()
            .filter(|(_, l)| !l.voters.is_empty())
            .fold(None, |best: Option<&(u32, Land)>, l| match best {
                Some(b) if b.1.voters.len() >= l.1.voters.len() => Some(b),
                _ => Some(l),
            })
            .map(|(f, l)| (*f, &l.garden))
    }

    /// Chao `key` in whichever garden the server runs it (where its owner has it, not
    /// necessarily its home: a Chao can be carried to another garden).
    fn chao_mut(&mut self, key: Key) -> Option<&mut Chao> {
        let run = self
            .runs
            .iter_mut()
            .flatten()
            .find(|r| r.slots.contains_key(&key))?;
        let slot = *run.slots.get(&key)?;
        run.world.chao.get_mut(slot)?.as_mut()
    }

    /// What the Chao did to the gardens' items since last asked (the referee carries it out).
    pub fn take_item_acts(&mut self) -> Vec<ItemAct> {
        std::mem::take(&mut self.acts)
    }

    /// Steps the gardens to `now` and sends their Chao, with the gardens' items and toys as
    /// the referee has them.
    pub fn tick(&mut self, now: Instant, shared: Shared, out: &mut Vec<Out>) {
        self.now = Some(now);
        out.append(&mut self.outbox);
        self.items = shared.items;
        // Gone from the garden (its owner left with it, or it's used up): not in a Chao's hands.
        let items = &self.items;
        self.carried
            .retain(|_, (id, _)| items.iter().any(|(_, _, i)| i.id == *id));
        self.toy_modes = shared.toys;
        self.tree_states = shared.trees;
        self.tree_thirst = shared.thirsty;
        let clock = *self.clock.get_or_insert(now);
        let due = (now.duration_since(clock).as_micros() / FRAME.as_micros()) as u32;
        if due == 0 {
            return;
        }
        self.clock = Some(if due > MAX_CATCH_UP {
            now
        } else {
            clock + FRAME * due
        });
        for _ in 0..due.min(MAX_CATCH_UP) {
            self.frame(now, out);
        }
    }

    fn frame(&mut self, now: Instant, out: &mut Vec<Out>) {
        self.frames += 1;
        if self.frames % ROSTER_EVERY == 1 {
            self.roster_pass(now);
        }
        let look = self.frames.is_multiple_of(HELD_EVERY);
        if look {
            for index in 0..3 {
                self.held_here[index] = self.held_waiting(index as u8 + 1, now);
            }
        }
        for index in 0..3 {
            self.membership(index, now, out);
            if look && self.runs[index].is_some() {
                self.held_pass(index, now, out);
            }
            self.asleep_frame(index, now);
            // Locked ones: who may still change them (their owner's player ids).
            let locks = self.locks(index, now);
            let Some(run) = self.runs[index].as_mut() else {
                continue;
            };
            for (key, &slot) in &run.slots {
                let may = |id: &u8| locks.get(key).is_none_or(|owners| owners.contains(id));
                if let Some(chao) = run.world.chao.get_mut(slot).and_then(Option::as_mut) {
                    // A held Chao (its owner isn't here) evolves here; its owner's game runs
                    // a present one's evolution.
                    let held = key.0 >= HELD_OWNER;
                    chao.change(|rec| hold_life(rec, held));
                    if let Some(sent) = self.sent.get_mut(key).filter(|_| !held) {
                        let mut rec = Record((**sent).clone());
                        hold_life(&mut rec, false);
                        **sent = rec.0;
                    }
                    // A player holds it (their `ChaoHeld`), whoever they are; letting go
                    // clears it.
                    let holder = self
                        .held
                        .get(key)
                        .filter(|(_, _, at)| now.duration_since(*at) < HELD_STALE)
                        .map(|h| h.0);
                    chao.held = holder.is_some();
                    // Its changes are theirs to send while they touch it; not while it's
                    // locked against them (theirs are dropped: the server's go on).
                    let petted = self.players.iter().any(|(id, p)| {
                        p.action == PET
                            && may(id)
                            && now.duration_since(p.at) < PLAYER_STALE
                            && distance(p.position, chao.body.pos) < PET_RANGE
                    });
                    if holder.is_some_and(|h| may(&h)) || petted {
                        self.quiet.insert(*key, now + TOUCH_QUIET);
                    }
                }
            }
            let around = Players {
                here: self
                    .players
                    .iter()
                    .filter(|(_, p)| {
                        p.area == index as u8 + 1 && now.duration_since(p.at) < PLAYER_STALE
                    })
                    .map(|(&id, p)| Seen {
                        id,
                        pos: p.position,
                        rot_y: p.rotation_y as u16,
                        character: p.character,
                        action: p.action,
                    })
                    .collect(),
                ball: self.balls[index]
                    .filter(|(_, _, _, at)| now.duration_since(*at) < PLAYER_STALE)
                    .map(|(pos, vel, flags, _)| Ball {
                        pos,
                        moving: (vel[0] * vel[0] + vel[1] * vel[1] + vel[2] * vel[2]).sqrt() > 0.4,
                        held: flags & BALL_HELD != 0,
                        in_water: flags & BALL_IN_WATER != 0,
                    }),
                things: things(
                    &self.items,
                    &self.flying,
                    &self.players,
                    run,
                    index as u8 + 1,
                    now,
                ),
                carried: self
                    .carried
                    .iter()
                    .filter_map(|(key, (_, c))| run.slots.get(key).map(|&slot| (slot, *c)))
                    .collect(),
                thrown: self
                    .touched
                    .iter()
                    .filter(|(_, (_, _, _, at))| now.duration_since(*at) < HELD_STALE)
                    .filter_map(|(key, _)| run.slots.get(key).copied())
                    .collect(),
                holders: self
                    .held
                    .iter()
                    .filter(|(_, (_, _, at))| now.duration_since(*at) < HELD_STALE)
                    .filter_map(|(key, (by, _, _))| Some((*run.slots.get(key)?, *by)))
                    .collect(),
                focus: RefCell::new(HashMap::new()),
                playing: [(TOY_TV, 2, 0xC9), (TOY_RADIO, 3, 0xCA)]
                    .into_iter()
                    .filter(|&(toy, _, _)| self.toy_modes[index][usize::from(toy)] != 0)
                    .filter_map(|(_, kind, signal)| {
                        let t = run.world.garden.toys.iter().find(|t| t.kind == kind)?;
                        Some((signal, t.pos))
                    })
                    .collect(),
                trees: run
                    .world
                    .garden
                    .tree_spots
                    .iter()
                    .zip(self.tree_states[index])
                    .enumerate()
                    .map(|(i, (&at, state))| (at, state, self.tree_thirst[index] & 1 << i != 0))
                    .collect(),
                locks: locks
                    .iter()
                    .filter_map(|(key, owners)| Some((*run.slots.get(key)?, owners.clone())))
                    .collect(),
            };
            // Locked Chao's records as this frame finds them: their save keeps these if one
            // they're locked against has them busy by its end.
            let before: HashMap<usize, Record> = around
                .locks
                .keys()
                .filter_map(|&slot| {
                    let c = run.world.chao.get(slot)?.as_ref()?;
                    c.saved.is_none().then(|| (slot, c.rec.clone()))
                })
                .collect();
            run.world.frame(&around);
            let focus = around.focus.take();
            for (key, &slot) in &run.slots {
                let Some(chao) = run.world.chao.get_mut(slot).and_then(Option::as_mut) else {
                    continue;
                };
                let party = match with_whom(chao) {
                    Some(Party::Holder) => self.held.get(key).map(|h| h.0),
                    Some(Party::Thrower) => self
                        .touched_by
                        .get(key)
                        .copied()
                        .or_else(|| self.held.get(key).map(|h| h.0)),
                    Some(Party::Focus) => focus.get(&slot).copied(),
                    None => None,
                };
                let theirs =
                    party.is_some_and(|id| locks.get(key).is_some_and(|o| !o.contains(&id)));
                let ticked = run.world.ticked.iter().find(|(s, _)| *s == slot);
                keep_save(chao, theirs, before.get(&slot), ticked.map(|t| &t.1));
            }
            let (things, slot_locks) = (around.things, around.locks);
            // The life tick happens in the owner's game too: not theirs to be sent. A held
            // Chao's owner doesn't tick it (it isn't in their garden): theirs to be sent.
            for (slot, ticked) in run.world.ticked.drain(..) {
                if let Some(key) = run.slots.iter().find(|(_, s)| **s == slot).map(|(k, _)| k)
                    && key.0 < HELD_OWNER
                    && let Some(sent) = self.sent.get_mut(key)
                {
                    changes::apply(sent, &ticked);
                }
            }
            let mut evolved = Vec::new();
            let mut sounds = Vec::new();
            for (&key, &slot) in &run.slots {
                let Some(chao) = run.world.chao.get_mut(slot).and_then(Option::as_mut) else {
                    continue;
                };
                // Some play every frame (swimming): the game keeps one per Chao and id
                // (0x437590), so each goes once in a while, not 60 times a second.
                let frame = self.frames;
                for id in chao.sounds.drain(..) {
                    let last = self.sounded.entry((key, id)).or_insert(0);
                    if *last != 0 && frame - *last < SOUND_AGAIN {
                        continue;
                    }
                    *last = frame;
                    sounds.push(ChaoSound {
                        owner: key.0,
                        index: key.1,
                        call: SOUND_AT,
                        id,
                        offset: [0.0; 3],
                        args: VOICE_ARGS,
                    });
                }
                // A held Chao's cocoon opens: it evolves here, and its owner's save and
                // everyone's copy take the new record. A present one's evolving and dying are
                // its owner's game's (`hold_life`).
                if key.0 >= HELD_OWNER && chao.life_event == Some(LifeEvent::Evolve) {
                    // Its own doing: from the kept record.
                    if let Some(saved) = chao.saved.take() {
                        chao.rec = saved;
                    }
                    let kind = evolve(&mut chao.rec);
                    crate::log!(
                        "chao host: held Chao {key:?} in garden {} evolved into type {kind:#04x}",
                        index + 1
                    );
                    evolved.push(key);
                }
                chao.life_event = None;
                // Let go of, an item falls to the floor under its hands.
                let in_its_hands =
                    in_hands(chao.body.pos, i32::from(chao.body.rot_y), IN_CHAO_HANDS);
                let hands = on_floor(&run.world.garden, in_its_hands);
                let area = index as u8 + 1;
                for event in std::mem::take(&mut chao.fruit_events) {
                    match event {
                        FruitEvent::Took { id, .. } => self.acts.push(ItemAct::Take {
                            id: item_id(id),
                            area,
                            chao: key,
                            at: chao.body.pos,
                        }),
                        FruitEvent::Bite { id, left } => self.acts.push(ItemAct::Bite {
                            id: item_id(id),
                            left,
                        }),
                        // Thrown away: it flies from its hands (`throw_away`'s velocity).
                        FruitEvent::Dropped { id, vel: Some(v) } => {
                            self.flying.push((item_id(id), area, in_its_hands, v, 0));
                        }
                        FruitEvent::Dropped { id, vel: None } => {
                            let id = item_id(id);
                            self.acts.push(ItemAct::Drop { id, at: hands });
                        }
                    }
                }
                for event in std::mem::take(&mut chao.toy_events) {
                    // The shared toys (the horse is each game's own).
                    let toy = match event {
                        ToyEvent::Press { toy: Some(2), .. } => Some(TOY_TV),
                        ToyEvent::Press { toy: Some(3), .. } => Some(TOY_RADIO),
                        ToyEvent::Press { toy: Some(8), .. } => Some(TOY_BOX),
                        _ => None,
                    };
                    if let Some(toy) = toy {
                        self.acts.push(ItemAct::ToyOn { area, toy });
                    }
                }
                for event in std::mem::take(&mut chao.tree_events) {
                    let spots = &run.world.garden.tree_spots;
                    let nearest = |at: V3, reach: f32| {
                        spots
                            .iter()
                            .enumerate()
                            .filter(|(_, s)| distance(**s, at) < reach)
                            .min_by(|a, b| distance(*a.1, at).total_cmp(&distance(*b.1, at)))
                            .map(|(i, _)| i as u8)
                    };
                    match event {
                        TreeEvent::Plant {
                            spot_pos,
                            seed_kind,
                        } => {
                            // The seed is gone either way; planted, the keeper's spot takes it.
                            if let Some((id, _)) = self.carried.remove(&key) {
                                self.acts.push(ItemAct::Gone { id });
                            }
                            if let Some(slot) = spot_pos.and_then(|at| nearest(at, 1.0)) {
                                self.acts.push(ItemAct::Tree {
                                    area,
                                    slot,
                                    action: TREE_PLANT,
                                    kind: seed_kind,
                                });
                            }
                        }
                        TreeEvent::Water { pos, radius, .. } => {
                            // Trees whose watering sphere (r 20) the poured one reaches.
                            for (slot, spot) in spots.iter().enumerate() {
                                if distance(*spot, pos) < radius + WATER_REACH {
                                    self.acts.push(ItemAct::Tree {
                                        area,
                                        slot: slot as u8,
                                        action: TREE_WATER,
                                        kind: 0,
                                    });
                                }
                            }
                        }
                    }
                }
                for event in std::mem::take(&mut chao.items) {
                    match event {
                        ItemEvent::Take(touch) => {
                            let mut senses = ChaoSenses::at(&things, slot, chao.body.pos);
                            senses.owners = slot_locks.get(&slot).map(Vec::as_slice);
                            if let Some(t) = touch_category(touch).and_then(|c| {
                                things.iter().find(|t| {
                                    t.category == c
                                        && t.chao.is_none()
                                        && senses.may_take(t)
                                        && senses.touches(t)
                                })
                            }) {
                                self.acts.push(ItemAct::Take {
                                    id: t.id,
                                    area,
                                    chao: key,
                                    at: chao.body.pos,
                                });
                                self.carried.insert(
                                    key,
                                    (
                                        t.id,
                                        Carried {
                                            touch,
                                            kind: t.kind,
                                        },
                                    ),
                                );
                            }
                        }
                        ItemEvent::LetGo => {
                            if let Some((id, _)) = self.carried.remove(&key) {
                                self.acts.push(ItemAct::Drop { id, at: hands });
                            }
                        }
                        ItemEvent::Throw(v) => {
                            if let Some((id, _)) = self.carried.remove(&key) {
                                self.flying.push((id, area, in_its_hands, v, 0));
                            }
                        }
                        ItemEvent::Wear => {
                            if let Some((id, _)) = self.carried.remove(&key) {
                                self.acts.push(ItemAct::Gone { id });
                            }
                        }
                        ItemEvent::HatOff { kind, .. } => self.acts.push(ItemAct::New {
                            area,
                            category: 9,
                            kind,
                            at: chao.body.pos,
                        }),
                    }
                }
                if let Some(&(holder, offset, at)) = self.held.get(&key)
                    && now.duration_since(at) < HELD_STALE
                    && let Some(p) = self.players.get(&holder)
                {
                    chao.body.pos = in_hands(p.position, p.rotation_y, offset);
                    chao.body.vel = [0.0; 3];
                    // Let go of without a word (their game went quiet), it falls from there.
                    chao.body.wake();
                } else if let Some(&(pos, rot, _, at)) = self.touched.get(&key)
                    && now.duration_since(at) < HELD_STALE
                {
                    // A player's game runs it for a touch (thrown, petted, fed): it's where
                    // theirs is, flying on between their reports, and goes on from there
                    // (still flying) when they're done.
                    let (at, vel) = self.flights.get(&key).map_or((pos, [0.0; 3]), |f| {
                        let frames =
                            now.duration_since(f.heard).as_secs_f32() / FRAME.as_secs_f32();
                        flight(&run.world.garden, f.pos, f.vel, frames)
                    });
                    chao.body.pos = at;
                    chao.body.rot_y = rot as u16;
                    chao.body.vel = vel;
                    chao.body.wake();
                    if self
                        .touched_by
                        .get(&key)
                        .is_none_or(|by| locks.get(&key).is_none_or(|owners| owners.contains(by)))
                    {
                        self.quiet.insert(key, now + TOUCH_QUIET);
                    }
                } else {
                    // The players' bodies push it aside, as their collision does in their
                    // games: otherwise their copies walk into them and shove them.
                    for p in self.players.values().filter(|p| {
                        usize::from(p.area) == index + 1 && now.duration_since(p.at) < PLAYER_STALE
                    }) {
                        push_out(&mut chao.body.pos, p.position);
                    }
                    // Under the ground (in at a spot just below the floor, or through a seam):
                    // back on top, or everyone's copy shows it under the garden.
                    if self.frames.is_multiple_of(RESCUE_EVERY)
                        && let Some(at) = fell_through(&run.world.garden.tris, chao.body.pos)
                    {
                        crate::log!(
                            "chao host: {key:?} in garden {} was under the ground at {:?}; back on it",
                            index + 1,
                            chao.body.pos
                        );
                        chao.body.pos = at;
                        chao.body.vel = [0.0; 3];
                    }
                }
            }
            // The server's items lie on the floor, as the games' own do (their position is
            // the floor under them): ones that don't (dropped from a Chao's hands, or saved
            // from them) settle.
            if self.frames.is_multiple_of(SETTLE_EVERY) {
                let area = index as u8 + 1;
                for (_, _, item) in self
                    .items
                    .iter()
                    .filter(|(a, o, i)| *a == area && *o == SERVER && i.place == ItemPlace::Ground)
                    // Not one in the air: it lands by itself.
                    .filter(|(_, _, i)| !self.flying.iter().any(|f| f.0 == i.id))
                {
                    let at = on_floor(&run.world.garden, item.position);
                    if (at[1] - item.position[1]).abs() > 0.1 {
                        self.acts.push(ItemAct::Drop { id: item.id, at });
                    }
                }
            }
            fly(
                &mut self.flying,
                &run.world.garden,
                index as u8 + 1,
                &mut self.acts,
            );
            for act in &self.acts {
                act.apply(&mut self.items);
            }
            let keys: Vec<Key> = run.slots.keys().copied().collect();
            for key in evolved {
                self.versions += 1;
                if let Some(s) = self.shown.get_mut(&key) {
                    s.version = self.versions;
                }
            }
            if !sounds.is_empty() {
                self.send_sounds(index, now, sounds, out);
            }
            if self.frames.is_multiple_of(CHANGES_EVERY) {
                for &key in &keys {
                    self.send_changes(key, now, out);
                }
                self.send_live(index, &keys, now, out);
            }
            if self.frames.is_multiple_of(SEND_EVERY) {
                self.send(index, now, out);
            }
        }
        if look {
            self.relock(now);
            self.tell(out);
            self.tell_locks(now, out);
        }
    }

    /// Chao `key`'s owner (their key) and the garden it's in: a held one's from what's shown,
    /// a present one's from its player (it's in their garden).
    fn owner_of(&self, key: Key) -> Option<(PlayerKey, u8)> {
        if key.0 >= HELD_OWNER {
            return self.shown.get(&key).map(|s| (s.id.key, s.area));
        }
        let owner = self.keys.get(&key.0)?.0;
        let area = self
            .runs
            .iter()
            .position(|r| r.as_ref().is_some_and(|r| r.slots.contains_key(&key)))
            .map(|i| i as u8 + 1)
            .or_else(|| self.players.get(&key.0).map(|p| p.area))
            .unwrap_or(0);
        Some((owner, area))
    }

    /// Whether Chao `key` is locked at `now` (its owner's `ChaoLock` where they are, by
    /// `lock_area`; offline is in no garden), and the player ids its owner is online as (they
    /// may always change it).
    fn lock(&self, key: Key, now: Instant) -> Option<(bool, Vec<u8>)> {
        let (owner, area) = self.owner_of(key)?;
        let lock = self
            .roster
            .owners
            .get(&owner)
            .map_or(ChaoLock::Always, |o| o.lock);
        let ids: Vec<u8> = self
            .keys
            .iter()
            .filter(|(_, (k, _))| *k == owner)
            .map(|(&id, _)| id)
            .collect();
        let areas: Vec<u8> = ids
            .iter()
            .filter_map(|id| self.players.get(id))
            .map(|p| lock_area(p, now))
            .collect();
        let there = if areas.contains(&area) {
            area
        } else {
            areas.into_iter().find(|a| *a != 0).unwrap_or(0)
        };
        Some((lock.locks(there, area), ids))
    }

    /// Whether Chao `owner`/`index` is locked against player `from`: nothing they do to it
    /// reaches its owner (the referee doesn't pass their `ChaoChanges` on).
    pub fn locked_against(&self, from: u8, key: Key) -> bool {
        let now = self.now.unwrap_or_else(Instant::now);
        self.lock(key, now)
            .is_some_and(|(locked, owners)| locked && !owners.contains(&from))
    }

    /// Garden `index`'s locked Chao: who may still change them (their owner's player ids).
    fn locks(&self, index: usize, now: Instant) -> HashMap<Key, Vec<u8>> {
        let Some(run) = self.runs[index].as_ref() else {
            return HashMap::new();
        };
        run.slots
            .keys()
            .filter_map(|&key| match self.lock(key, now) {
                Some((true, owners)) => Some((key, owners)),
                _ => None,
            })
            .collect()
    }

    /// Held Chao whose lock changed are told of again (their name tag says so).
    fn relock(&mut self, now: Instant) {
        let keys: Vec<Key> = self.shown.keys().copied().collect();
        for key in keys {
            let locked = self.lock(key, now).is_some_and(|(l, _)| l);
            if self.shown.get(&key).is_some_and(|s| s.locked != locked) {
                self.versions += 1;
                if let Some(s) = self.shown.get_mut(&key) {
                    s.locked = locked;
                    s.version = self.versions;
                }
            }
        }
    }

    /// Tells each player which Chao are locked against them now (`ChaoLocked`), when that
    /// changed for them: every present player's brought Chao and every held one, but their
    /// own.
    fn tell_locks(&mut self, now: Instant, out: &mut Vec<Out>) {
        let keys: BTreeSet<Key> = self
            .records
            .keys()
            .chain(self.shown.keys())
            .copied()
            .collect();
        let locked: Vec<(Key, Vec<u8>)> = keys
            .into_iter()
            .filter_map(|key| match self.lock(key, now) {
                Some((true, owners)) => Some((key, owners)),
                _ => None,
            })
            .collect();
        let ids: Vec<u8> = self.keys.keys().copied().collect();
        for id in ids {
            let list: Vec<Key> = locked
                .iter()
                .filter(|(_, owners)| !owners.contains(&id))
                .map(|(key, _)| *key)
                .collect();
            if self.told_locks.get(&id) == Some(&list) {
                continue;
            }
            self.told_locks.insert(id, list.clone());
            out.push(Out {
                to: To::One(id),
                from: SERVER,
                body: Body::ChaoLocked { chao: list },
            });
        }
    }

    /// Player ids in garden `index` now.
    fn here(&self, index: usize, now: Instant) -> Vec<u8> {
        self.players
            .iter()
            .filter(|(_, p)| {
                usize::from(p.area) == index + 1 && now.duration_since(p.at) < PLAYER_STALE
            })
            .map(|(&id, _)| id)
            .collect()
    }

    fn send_sounds(&self, index: usize, now: Instant, sounds: Vec<ChaoSound>, out: &mut Vec<Out>) {
        for to in self.here(index, now) {
            for chunk in sounds.chunks(MAX_CHAO_SOUNDS) {
                out.push(Out {
                    to: To::One(to),
                    from: SERVER,
                    body: Body::ChaoSounds {
                        sounds: chunk.to_vec(),
                    },
                });
            }
        }
    }

    /// Garden `index`'s Chao as they live here (`ChaoLive`), to everyone there: their copies
    /// show it (the status balloon), and its owner's save keeps its emotions and needs.
    fn send_live(&mut self, index: usize, keys: &[Key], now: Instant, out: &mut Vec<Out>) {
        let to: Vec<u8> = self
            .players
            .iter()
            .filter(|(_, p)| {
                usize::from(p.area) == index + 1 && now.duration_since(p.at) < VISIT_STALE
            })
            .map(|(&id, _)| id)
            .collect();
        for &key in keys {
            let Some(chao) = self.chao_mut(key) else {
                continue;
            };
            let bytes: Vec<u8> = CHAO_LIVE_FIELDS
                .iter()
                .flat_map(|&(at, len)| chao.kept().0.bytes[at..at + len].iter().copied())
                .collect();
            // Its owner too: their save keeps up with its emotions and needs.
            for &id in &to {
                out.push(Out {
                    to: To::One(id),
                    from: SERVER,
                    body: Body::ChaoLive {
                        owner: key.0,
                        index: key.1,
                        bytes: bytes.clone(),
                    },
                });
            }
        }
    }

    /// What the server changed in Chao `key`'s record since its owner last had it, to them
    /// (unless a player's touch makes it theirs to send).
    fn send_changes(&mut self, key: Key, now: Instant, out: &mut Vec<Out>) {
        if key.0 >= HELD_OWNER {
            self.send_save(key, out);
            return;
        }
        let Some(rec) = self.chao_mut(key).map(|c| c.kept().0.clone()) else {
            return;
        };
        let quiet = self.quiet.get(&key).is_some_and(|until| now < *until);
        let Some(sent) = self.sent.get_mut(&key) else {
            return;
        };
        let diff = changes::diff(sent, &rec);
        **sent = rec;
        if quiet || diff.is_empty() {
            return;
        }
        out.push(Out {
            to: To::One(key.0),
            from: SERVER,
            body: Body::ChaoChanges {
                owner: key.0,
                index: key.1,
                changes: diff,
            },
        });
    }

    /// Who's in garden `index` now, whether it's run, and which Chao are in it.
    fn membership(&mut self, index: usize, now: Instant, out: &mut Vec<Out>) {
        let area = index as u8 + 1;
        let here = self.present_in(area);
        let land = self.land(index).map(|(f, g)| (f, g.clone()));
        // One player is enough when Chao held for others wait there.
        let enough = here.len() >= MIN_PLAYERS || (!here.is_empty() && self.held_here[index]);
        if let Some(run) = self.runs[index].as_mut() {
            run.short = if enough {
                None
            } else {
                Some(run.short.unwrap_or(now))
            };
        }
        let grace = self.runs[index]
            .as_ref()
            .and_then(|r| r.short)
            .is_some_and(|since| now.duration_since(since) < RUN_GRACE);
        let Some((fingerprint, garden)) = land.filter(|_| enough || grace) else {
            self.hide_all(area, now, out);
            if let Some(run) = self.runs[index].as_ref() {
                for key in run.slots.keys().copied().collect::<Vec<_>>() {
                    self.remember(key, area);
                    self.send_changes(key, now, out);
                    self.sent.remove(&key);
                    self.let_go(key);
                }
            }
            if self.runs[index].take().is_some() {
                crate::log!("chao host: garden {area} back to the players' games");
            }
            return;
        };
        if self.runs[index]
            .as_ref()
            .is_none_or(|r| r.fingerprint != fingerprint)
        {
            // New ground: held Chao come back into the new world at the next look.
            self.hide_all(area, now, out);
            crate::log!("chao host: running garden {area} for {here:?}");
            self.runs[index] = Some(Run {
                world: World::new(garden, index, 0x5A2C_0000 | area as u32),
                slots: BTreeMap::new(),
                fingerprint,
                short: None,
            });
        }
        let table = self.table();
        // Whose Chao are out: the players here (a hitch or a tab-out doesn't take theirs out of
        // the garden, and its brain with it).
        let present = here;
        let Some(run) = self.runs[index].as_ref() else {
            return;
        };
        let gone: Vec<Key> = run
            .slots
            .keys()
            .filter(|k| k.0 < HELD_OWNER && !self.wanted(k, &present))
            .copied()
            .collect();
        for &key in &gone {
            crate::log!(
                "chao host: player {}'s Chao {} leaves garden {area} (owner here {}, streamed {}, owner runs it {})",
                key.0,
                key.1,
                present.contains(&key.0),
                streaming(&self.own, &self.players, &key),
                self.own.get(&key).is_some_and(|(c, _)| c.owner_runs),
            );
            self.remember(key, area);
            self.send_changes(key, now, out);
            self.sent.remove(&key);
            self.let_go(key);
        }
        let Some(run) = self.runs[index].as_mut() else {
            return;
        };
        if run.world.motions.as_ref().map(Arc::as_ptr) != table.as_ref().map(Arc::as_ptr) {
            run.world.set_motions(table);
        }
        let wanted: BTreeMap<Key, GardenChao> = self
            .own
            .iter()
            .filter(|(key, _)| {
                Self::is_wanted(&self.own, &self.records, &self.players, key, &present)
            })
            .map(|(&key, (c, _))| (key, *c))
            .collect();
        for key in gone {
            if let Some(slot) = run.slots.remove(&key) {
                run.world.remove(slot);
            }
        }
        let mut handed = Vec::new();
        for (key, c) in wanted {
            if run.slots.contains_key(&key) {
                continue;
            }
            let Some(record) = self.records.get(&key) else {
                continue;
            };
            // From where its owner's game has it, but never under the ground.
            let at = fell_through(&run.world.garden.tris, c.position).unwrap_or(c.position);
            let chao = Chao::new(at, c.rotation_y as u16, Record((**record).clone()));
            if let Some(slot) = run.world.add(chao) {
                run.slots.insert(key, slot);
                self.sent.insert(key, record.clone());
                handed.push(key);
            }
        }
        // Its owner's game has it again: the copy held for them goes now.
        for (owner, row) in handed {
            let held = self
                .keys
                .get(&owner)
                .and_then(|(k, _)| self.roster.in_row(k, area, row))
                .and_then(|id| self.shown_key(&id));
            if let Some(held) = held {
                crate::log!(
                    "chao host: player {owner}'s Chao {row} is back with them in garden {area}"
                );
                self.hide(held, now, out);
            }
        }
    }

    /// Chao `key` leaves the server's world: what it holds drops where it is.
    fn let_go(&mut self, key: Key) {
        let Some(chao) = self.chao_mut(key) else {
            return;
        };
        let at = chao.body.pos;
        let fruit = chao
            .hands
            .take()
            .filter(|h| h.bites > 0)
            .map(|h| item_id(h.fruit));
        let carried = self.carried.remove(&key).map(|(id, _)| id);
        for id in fruit.into_iter().chain(carried) {
            let act = ItemAct::Drop { id, at };
            act.apply(&mut self.items);
            self.acts.push(act);
        }
    }

    fn wanted(&self, key: &Key, here: &[u8]) -> bool {
        Self::is_wanted(&self.own, &self.records, &self.players, key, here)
    }

    /// Out now: an owner here streams it as visiting (their game has it in this garden, its
    /// home or not) and isn't running it themselves (evolving, life ending); its record came.
    fn is_wanted(
        own: &HashMap<Key, (GardenChao, Instant)>,
        records: &HashMap<Key, Box<ChaoData>>,
        players: &HashMap<u8, Player>,
        key: &Key,
        here: &[u8],
    ) -> bool {
        here.contains(&key.0)
            && streaming(own, players, key)
            && own.get(key).is_some_and(|(c, _)| !c.owner_runs)
            && records.contains_key(key)
    }

    /// Player ids in garden `area`: where their last state put them, however long ago. A game
    /// paused on a tab-out sends nothing (the session stays up) and is still there; only a
    /// state elsewhere or leaving (`left`) takes a player out.
    fn present_in(&self, area: u8) -> Vec<u8> {
        self.players
            .iter()
            .filter(|(_, p)| p.area == area)
            .map(|(&id, _)| id)
            .collect()
    }

    /// The player ids player `key` is online as.
    fn sessions(&self, key: &PlayerKey) -> Vec<u8> {
        self.keys
            .iter()
            .filter(|(_, (k, _))| k == key)
            .map(|(&id, _)| id)
            .collect()
    }

    /// `ServerChao` for each owner's Chao in garden `index`, to each player there.
    fn send(&self, index: usize, now: Instant, out: &mut Vec<Out>) {
        let Some(run) = self.runs[index].as_ref() else {
            return;
        };
        let area = index as u8 + 1;
        let mut by_owner: BTreeMap<u8, Vec<GardenChao>> = BTreeMap::new();
        for (&(owner, chao_index), &slot) in &run.slots {
            let Some(chao) = run.world.chao.get(slot).and_then(Option::as_ref) else {
                continue;
            };
            // Never `held`: every game hears a hold from its holder (`ChaoHeld`), and the
            // holder's own game must not take it as someone else's.
            // While a player's game runs it for a touch, the motion it plays there.
            let motion = self
                .touched
                .get(&(owner, chao_index))
                .filter(|(_, _, _, at)| now.duration_since(*at) < HELD_STALE)
                .map_or(chao.motion, |(_, _, motion, _)| *motion);
            by_owner.entry(owner).or_default().push(GardenChao {
                index: chao_index,
                position: chao.body.pos,
                rotation_y: i32::from(chao.body.rot_y),
                motion,
                // The game's timer counts up to the length (`D+0x6F0`, `D+0x6F4`).
                bubble: (
                    chao.bubble.0,
                    (chao.bubble_length - chao.bubble.1).max(0),
                    chao.bubble_length,
                ),
                eyes: chao.eyes,
                mouth: chao.mouth,
                props: chao.props.map(|m| crate::garden::prop_value(m as usize)),
                drum: if chao.drum { 255 } else { 0 },
                // The client's book modes: 1 + (0 opening, 1 open, 2 turning a page).
                book: match chao.book {
                    Some(0) => 1,
                    Some(2) => 3,
                    Some(_) => 2,
                    None => 0,
                },
                // An evolution's cocoon (kind 0) while it's closed.
                cocoon: u8::from(chao.cocooned()),
                ..GardenChao::default()
            });
        }
        // Sleeping ones lie where they were left, or are where they're carried.
        for (&(owner, index), s) in self.shown.iter().filter(|(_, s)| s.area == area) {
            if !s.asleep {
                continue;
            }
            let Some((position, rotation_y)) = self.roster.get(&s.id).and_then(|c| c.place) else {
                continue;
            };
            let fresh = |at: &Instant| now.duration_since(*at) < HELD_STALE;
            let held = self.held.get(&(owner, index)).is_some_and(|h| fresh(&h.2));
            let touched = self
                .touched
                .get(&(owner, index))
                .filter(|t| fresh(&t.3))
                .map(|t| t.2);
            let motion = touched.unwrap_or(if held { HELD_MOTION } else { SLEEP_MOTION });
            by_owner.entry(owner).or_default().push(GardenChao {
                index,
                position,
                rotation_y,
                motion,
                eyes: if motion == SLEEP_MOTION {
                    SLEEP_EYES
                } else {
                    0
                },
                ..GardenChao::default()
            });
        }
        // Players whose game paused a moment (a tab-out) still get them, or they'd take the
        // Chao back meanwhile.
        let here: Vec<u8> = self
            .players
            .iter()
            .filter(|(_, p)| p.area == area && now.duration_since(p.at) < VISIT_STALE)
            .map(|(&id, _)| id)
            .collect();
        for (owner, list) in by_owner {
            for &to in &here {
                out.push(Out {
                    to: To::One(to),
                    from: if owner >= HELD_OWNER { SERVER } else { owner },
                    body: Body::ServerChao {
                        area,
                        owner,
                        list: list.clone(),
                    },
                });
            }
        }
    }
}

/// Held Chao: the ones players bring, shown while they aren't in that garden.
impl ChaoHost {
    /// Player `id` joined with key `key` (their Chao are known by it) and name `name`.
    pub fn joined(&mut self, id: u8, key: PlayerKey, name: &str) {
        self.keys.insert(id, (key, name.to_string()));
        self.awake.remove(&id);
        self.told.remove(&id);
        self.told_locks.remove(&id);
    }

    /// The Chao held for players that changed, as files (`Roster::files`).
    pub fn files(&mut self) -> Vec<(String, Option<String>)> {
        self.roster.files()
    }

    /// Takes back a file `files` wrote.
    pub fn load_file(&mut self, name: &str, text: &str) {
        if !self.roster.load(name, text) {
            crate::log!("chao host: {name} doesn't read back; left out");
        }
    }

    /// Player `from` brings `list` (read from their save): what's kept of them follows it.
    fn brought(
        &mut self,
        from: u8,
        awake: bool,
        lock: ChaoLock,
        list: &[BroughtChao],
        now: Instant,
    ) {
        let Some((key, name)) = self.keys.get(&from).cloned() else {
            return;
        };
        self.awake.insert(from, awake);
        let synced = self.roster.sync(key, &name, lock, list, unix_now());
        crate::log!(
            "chao host: player {from} brings {} Chao ({} new or changed, {} gone), others may change them: {}{}",
            list.len(),
            synced.changed.len(),
            synced.removed.len(),
            lock.ini(),
            if awake {
                ""
            } else {
                "; their save isn't loaded"
            }
        );
        let mut out = Vec::new();
        for id in &synced.removed {
            if let Some(held) = self.shown_key(id) {
                self.hide(held, now, &mut out);
            }
        }
        for id in &synced.changed {
            let Some(held) = self.shown_key(id) else {
                continue;
            };
            let Some(stored) = self.roster.get(id).cloned() else {
                continue;
            };
            if self
                .shown
                .get(&held)
                .is_some_and(|s| s.area != stored.garden)
            {
                // Moved to another garden: shown there from the next look.
                self.hide(held, now, &mut out);
                continue;
            }
            // An awake one goes on from the save's record, with what the server changed that
            // isn't in it yet.
            let sent = self.sent.get(&held).cloned();
            if let Some(sent) = sent
                && let Some(chao) = self.chao_mut(held)
            {
                // Both its records (`Chao::saved`), each with what it has that the save hasn't.
                let rebase = |now: &Record| {
                    let mut rec = (*stored.record).clone();
                    for (at, bytes) in &save_spans(&sent, &now.0) {
                        let at = usize::from(*at);
                        rec.bytes[at..at + bytes.len()].copy_from_slice(bytes);
                    }
                    Record(rec)
                };
                chao.saved = chao.saved.as_ref().map(rebase);
                chao.rec = rebase(&chao.rec);
                self.sent.insert(held, stored.record.clone());
            }
            self.versions += 1;
            if let Some(s) = self.shown.get_mut(&held) {
                s.version = self.versions;
            }
        }
        self.outbox.append(&mut out);
    }

    /// The held id Chao `id` is shown under.
    fn shown_key(&self, id: &ChaoId) -> Option<Key> {
        self.shown
            .iter()
            .find(|(_, s)| s.id == *id)
            .map(|(&k, _)| k)
    }

    /// The player id player `key` is online as (the one whose save takes changes first).
    fn session_of(&self, key: &PlayerKey) -> Option<u8> {
        self.keys
            .iter()
            .filter(|(_, (k, _))| k == key)
            .map(|(&id, _)| id)
            .max_by_key(|id| self.awake.get(id).copied().unwrap_or(false))
    }

    /// Whether a held Chao of `key` living in garden `area`, visitor row `row`, is shown now,
    /// and asleep: `None` while its owner's game has it (they're in that garden, and their
    /// stream for it came or `HANDOVER` passed).
    fn held_state(&self, key: &PlayerKey, area: u8, row: u8, now: Instant) -> Option<bool> {
        let ids = self.sessions(key);
        if ids.is_empty() {
            return Some(true);
        }
        // Any session of theirs in that garden (a reconnect can leave an old one a moment).
        for id in &ids {
            let Some(p) = self.players.get(id).filter(|p| p.area == area) else {
                continue;
            };
            let streamed = streaming(&self.own, &self.players, &(*id, row));
            if streamed || now.duration_since(p.since) >= HANDOVER {
                return None;
            }
        }
        Some(
            !ids.iter()
                .any(|id| self.awake.get(id).copied().unwrap_or(false)),
        )
    }

    /// The held Chao that would show in garden `area` now (`choose` picks among them).
    fn candidates(&self, area: u8, now: Instant) -> Vec<Candidate> {
        let mut out = Vec::new();
        for (key, owner) in &self.roster.owners {
            let online = self.session_of(key).is_some();
            for c in owner.chao.iter().filter(|c| c.garden == area) {
                let Some(asleep) = self.held_state(key, area, c.row, now) else {
                    continue;
                };
                out.push(Candidate {
                    id: ChaoId {
                        key: *key,
                        identity: c.identity,
                        nth: c.nth,
                    },
                    seen: if online { u64::MAX } else { owner.seen },
                    row: c.row,
                    asleep,
                });
            }
        }
        out
    }

    /// Held Chao wait in garden `area`: a single player there has it run. Not their own on the
    /// way back to them (or every visit alone would start a run for a moment).
    fn held_waiting(&self, area: u8, now: Instant) -> bool {
        self.candidates(area, now).iter().any(|c| {
            !self
                .sessions(&c.id.key)
                .iter()
                .any(|id| self.players.get(id).is_some_and(|p| p.area == area))
        })
    }

    /// Garden `index` (run now): which held Chao it shows, awake or asleep.
    fn held_pass(&mut self, index: usize, now: Instant, out: &mut Vec<Out>) {
        let area = index as u8 + 1;
        let present = self.runs[index]
            .as_ref()
            .map_or(0, |r| r.slots.keys().filter(|k| k.0 < HELD_OWNER).count());
        let chosen = choose(self.candidates(area, now), ROOM.saturating_sub(present));
        let going: Vec<Key> = self
            .shown
            .iter()
            .filter(|(_, s)| s.area == area && !chosen.iter().any(|c| c.id == s.id))
            .map(|(&k, _)| k)
            .collect();
        for key in going {
            self.hide(key, now, out);
        }
        for c in chosen {
            let key = match self.shown_key(&c.id) {
                Some(key) if self.shown.get(&key).is_some_and(|s| s.area == area) => key,
                Some(key) => {
                    self.hide(key, now, out);
                    continue;
                }
                None => {
                    // All held ids in use: it waits.
                    let Some(key) = self.free_key(now) else {
                        break;
                    };
                    if self.place(index, &c.id).is_none() {
                        continue;
                    }
                    self.versions += 1;
                    self.shown.insert(
                        key,
                        Shown {
                            id: c.id,
                            area,
                            asleep: true,
                            locked: false,
                            version: self.versions,
                        },
                    );
                    crate::log!(
                        "chao host: shows a Chao of {} in garden {area} as {key:?}",
                        self.roster
                            .owners
                            .get(&c.id.key)
                            .map_or("?", |o| o.name.as_str())
                    );
                    key
                }
            };
            let asleep = self.shown.get(&key).is_some_and(|s| s.asleep);
            if asleep && !c.asleep {
                self.wake(index, key);
            } else if !asleep && c.asleep {
                self.sleep(key, out);
            }
        }
    }

    /// Where held Chao `id` is in garden `index`: where it was last, or a free spot of the
    /// ground (kept from now on).
    fn place(&mut self, index: usize, id: &ChaoId) -> Option<([f32; 3], i32)> {
        if let Some(place) = self.roster.get(id)?.place {
            return Some(place);
        }
        let run = self.runs[index].as_mut()?;
        let garden = &run.world.garden;
        let at = crate::chao_sim::brain::random_cell_point(garden, 1, &mut run.world.rng)
            .map(|(x, z)| on_floor(garden, [x, 50.0, z]))
            .or_else(|| garden.tree_spots.first().copied())
            .unwrap_or([0.0; 3]);
        let place = (at, (run.world.rng.rand() & 0xFFFF) as i32);
        self.roster.get_mut(id)?.place = Some(place);
        Some(place)
    }

    /// A held id that's free now.
    fn free_key(&self, now: Instant) -> Option<Key> {
        (HELD_OWNER..=MAX_CHAO_OWNER)
            .flat_map(|o| (0..MAX_CARRIED_CHAO as u8).map(move |i| (o, i)))
            .find(|k| {
                !self.shown.contains_key(k)
                    && self
                        .freed
                        .get(k)
                        .is_none_or(|at| now.duration_since(*at) >= REUSE_HELD)
            })
    }

    /// Held Chao `key` wakes (its owner is online): it lives in garden `index`'s world from
    /// its record, the one its owner's save has.
    fn wake(&mut self, index: usize, key: Key) {
        let Some(id) = self.shown.get(&key).map(|s| s.id) else {
            return;
        };
        let (Some(stored), Some((pos, facing))) =
            (self.roster.get(&id).cloned(), self.place(index, &id))
        else {
            return;
        };
        let Some(run) = self.runs[index].as_mut() else {
            return;
        };
        let pos = fell_through(&run.world.garden.tris, pos).unwrap_or(pos);
        let chao = Chao::new(pos, facing as u16, Record((*stored.record).clone()));
        let Some(slot) = run.world.add(chao) else {
            return;
        };
        run.slots.insert(key, slot);
        self.sent.insert(key, stored.record);
        self.versions += 1;
        if let Some(s) = self.shown.get_mut(&key) {
            s.asleep = false;
            s.version = self.versions;
        }
        crate::log!(
            "chao host: held Chao {key:?} in garden {} wakes up",
            index + 1
        );
    }

    /// Held Chao `key` falls asleep where it is (its owner went offline, or it goes): out of
    /// the world, what its owner's save doesn't have yet sent first. Kept is its place and
    /// its record as their save has it.
    fn sleep(&mut self, key: Key, out: &mut Vec<Out>) {
        let Some(id) = self.shown.get(&key).map(|s| s.id) else {
            return;
        };
        if let Some(chao) = self.chao_mut(key) {
            let place = (chao.body.pos, i32::from(chao.body.rot_y));
            self.send_save(key, out);
            self.let_go(key);
            for run in self.runs.iter_mut().flatten() {
                if let Some(slot) = run.slots.remove(&key) {
                    run.world.remove(slot);
                }
            }
            let record = self.sent.remove(&key);
            if let Some(stored) = self.roster.get_mut(&id) {
                stored.place = Some(place);
                if let Some(record) = record {
                    stored.record = record;
                }
            }
        }
        self.held.remove(&key);
        self.touched.remove(&key);
        self.touched_by.remove(&key);
        self.flights.remove(&key);
        self.quiet.remove(&key);
        self.versions += 1;
        if let Some(s) = self.shown.get_mut(&key) {
            s.asleep = true;
            s.version = self.versions;
        }
    }

    /// Held Chao `key` goes from its garden (its owner has it, or it made room).
    fn hide(&mut self, key: Key, now: Instant, out: &mut Vec<Out>) {
        self.sleep(key, out);
        self.sounded.retain(|&(k, _), _| k != key);
        if self.shown.remove(&key).is_some() {
            self.freed.insert(key, now);
        }
    }

    /// Every held Chao in garden `area` goes (it isn't run any more).
    fn hide_all(&mut self, area: u8, now: Instant, out: &mut Vec<Out>) {
        let keys: Vec<Key> = self
            .shown
            .iter()
            .filter(|(_, s)| s.area == area)
            .map(|(&k, _)| k)
            .collect();
        for key in keys {
            self.hide(key, now, out);
        }
    }

    /// Sleeping Chao in garden `index` carried about: kept where they're carried (and thrown
    /// to).
    fn asleep_frame(&mut self, index: usize, now: Instant) {
        let area = index as u8 + 1;
        let asleep: Vec<(Key, ChaoId)> = self
            .shown
            .iter()
            .filter(|(_, s)| s.area == area && s.asleep)
            .map(|(&k, s)| (k, s.id))
            .collect();
        for (key, id) in asleep {
            let place = if let Some(&(holder, offset, at)) = self.held.get(&key)
                && now.duration_since(at) < HELD_STALE
                && let Some(p) = self.players.get(&holder)
            {
                Some((in_hands(p.position, p.rotation_y, offset), p.rotation_y))
            } else if let Some(&(pos, rot, _, at)) = self.touched.get(&key)
                && now.duration_since(at) < HELD_STALE
            {
                Some((pos, rot))
            } else {
                None
            };
            if let Some(place) = place
                && self.roster.get(&id).is_some_and(|c| c.place != Some(place))
                && let Some(stored) = self.roster.get_mut(&id)
            {
                stored.place = Some(place);
            }
        }
    }

    /// Tells every player what's new of the held Chao (`HeldChao`); not their own.
    fn tell(&mut self, out: &mut Vec<Out>) {
        let players: Vec<(u8, PlayerKey)> =
            self.keys.iter().map(|(&id, (k, _))| (id, *k)).collect();
        for (id, key) in players {
            let told = self.told.entry(id).or_default();
            let gone: Vec<Key> = told
                .keys()
                .filter(|k| !self.shown.get(k).is_some_and(|s| s.id.key != key))
                .copied()
                .collect();
            let mut bodies = Vec::new();
            for k in gone {
                told.remove(&k);
                bodies.push(Body::HeldChao {
                    owner: k.0,
                    index: k.1,
                    chao: None,
                });
            }
            for (&k, s) in self.shown.iter().filter(|(_, s)| s.id.key != key) {
                if told.get(&k) == Some(&s.version) {
                    continue;
                }
                let (Some(owner), Some(stored)) =
                    (self.roster.owners.get(&s.id.key), self.roster.get(&s.id))
                else {
                    continue;
                };
                // Awake, as it lives here (an evolution shows at once).
                let data = self
                    .runs
                    .iter()
                    .flatten()
                    .find_map(|r| {
                        let slot = *r.slots.get(&k)?;
                        r.world.chao.get(slot)?.as_ref().map(|c| c.kept().0.clone())
                    })
                    .map_or_else(|| stored.record.clone(), Box::new);
                told.insert(k, s.version);
                bodies.push(Body::HeldChao {
                    owner: k.0,
                    index: k.1,
                    chao: Some(HeldChao {
                        area: s.area,
                        asleep: s.asleep,
                        locked: s.locked,
                        name: owner.name.clone(),
                        data,
                    }),
                });
            }
            out.extend(bodies.into_iter().map(|body| Out {
                to: To::One(id),
                from: SERVER,
                body,
            }));
        }
    }

    /// What held Chao `key`'s life changed since its owner's save last had it, to them
    /// (`SaveChao`), when they're online with their save loaded. Otherwise it waits.
    fn send_save(&mut self, key: Key, out: &mut Vec<Out>) {
        let Some(id) = self.shown.get(&key).map(|s| s.id) else {
            return;
        };
        let Some(to) = self
            .session_of(&id.key)
            .filter(|s| self.awake.get(s).copied().unwrap_or(false))
        else {
            return;
        };
        let Some(rec) = self.chao_mut(key).map(|c| c.kept().0.clone()) else {
            return;
        };
        let Some(sent) = self.sent.get_mut(&key) else {
            return;
        };
        let spans = save_spans(sent, &rec);
        if spans.is_empty() {
            return;
        }
        **sent = rec.clone();
        if let Some(stored) = self.roster.get_mut(&id) {
            *stored.record = rec;
        }
        out.push(Out {
            to: To::One(to),
            from: SERVER,
            body: Body::SaveChao {
                identity: id.identity,
                nth: id.nth,
                spans,
            },
        });
    }

    /// Present Chao `key` (garden `area`) leaves the server's world: where it was is kept, for
    /// when it's held for its owner.
    fn remember(&mut self, key: Key, area: u8) {
        let Some(id) = self
            .keys
            .get(&key.0)
            .and_then(|(k, _)| self.roster.in_row(k, area, key.1))
        else {
            return;
        };
        let Some(place) = self
            .chao_mut(key)
            .map(|c| (c.body.pos, i32::from(c.body.rot_y)))
        else {
            return;
        };
        if let Some(stored) = self.roster.get_mut(&id) {
            stored.place = Some(place);
        }
    }

    /// Now and then: online players are seen now; ones gone a week are forgotten.
    fn roster_pass(&mut self, now: Instant) {
        let unix = unix_now();
        let online: Vec<PlayerKey> = self.keys.values().map(|(k, _)| *k).collect();
        for key in &online {
            self.roster.seen(key, unix);
        }
        let old = self.roster.expire(unix, &online);
        if !old.is_empty() {
            crate::log!(
                "chao host: forgot {} player(s) not seen for a week",
                old.len()
            );
            let keys: Vec<Key> = self
                .shown
                .iter()
                .filter(|(_, s)| old.contains(&s.id.key))
                .map(|(&k, _)| k)
                .collect();
            let mut out = Vec::new();
            for key in keys {
                self.hide(key, now, &mut out);
            }
            self.outbox.append(&mut out);
        }
    }
}

/// Whether owner stream `key` is current: heard within `VISIT_STALE` of its owner's last
/// state (their game paused, both stop alike; their game going on without it, it's gone).
fn streaming(
    own: &HashMap<Key, (GardenChao, Instant)>,
    players: &HashMap<u8, Player>,
    key: &Key,
) -> bool {
    let (Some((_, at)), Some(p)) = (own.get(key), players.get(&key.0)) else {
        return false;
    };
    p.at.saturating_duration_since(*at) < VISIT_STALE
}

/// Where player `p` counts as being for their Chao's locks at `now` (`lock_place`): the
/// garden they're in or elsewhere in Chao World, or for `AREA_GRACE` after they left there for
/// a load, that; then where the load has them (0 out of Chao World).
fn lock_area(p: &Player, now: Instant) -> u8 {
    match p.left {
        Some(at) if now.saturating_duration_since(at) >= AREA_GRACE => p.place,
        _ => p.garden,
    }
}

/// Record bytes for a save (`SaveChao`): (offset, bytes).
type Spans = Vec<(u16, Vec<u8>)>;

/// Seconds since 1970 (the roster's clock).
fn unix_now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

/// What changed from `before` to `after` in `SAVE_CHAO_FIELDS`: per field, its bytes from the
/// first change to the last.
fn save_spans(before: &ChaoData, after: &ChaoData) -> Spans {
    SAVE_CHAO_FIELDS
        .iter()
        .filter_map(|&(at, len)| {
            let differs = |i: &usize| before.bytes[*i] != after.bytes[*i];
            let first = (at..at + len).find(differs)?;
            let last = (at..at + len).rev().find(differs)?;
            Some((first as u16, after.bytes[first..=last].to_vec()))
        })
        .collect()
}

/// The gardens as the referee has them, for the Chao.
#[derive(Clone, Debug, Default)]
pub struct Shared {
    /// (area, owner, item); the server's own with owner `SERVER`.
    pub items: Vec<(u8, u8, GardenItem)>,
    /// Each garden's toys' last modes (`TOY_TV`, `TOY_RADIO`, `TOY_BOX`; 0 off or unknown).
    pub toys: [[u8; 3]; 3],
    /// Each garden's tree states by spot (record byte 1; 0 empty or unknown).
    pub trees: [[u8; TREE_SLOTS]; 3],
    /// Each garden's trees that want water, a bit per spot (`GardenTrees::thirsty`).
    pub thirsty: [u8; 3],
}

/// What a Chao the server runs did to a garden item, for the referee to carry out.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ItemAct {
    /// Chao `chao` took item `id` into its hands at `at`: it's the server's now, held there.
    Take {
        id: ItemId,
        area: u8,
        chao: Key,
        at: V3,
    },
    /// A bite of fruit `id`: `left` bites remain; at 0 it's eaten up and goes.
    Bite { id: ItemId, left: u8 },
    /// Let go of at `at`, onto the ground.
    Drop { id: ItemId, at: V3 },
    /// Used up (a hat put on).
    Gone { id: ItemId },
    /// A new item of the server's (a hat that fell off).
    New {
        area: u8,
        category: u8,
        kind: u8,
        at: V3,
    },
    /// Not an item: a Chao touched garden toy `toy` (`TOY_TV`, `TOY_RADIO`, `TOY_BOX`); if
    /// it's off (shut), it switches on (opens).
    ToyOn { area: u8, toy: u8 },
    /// Not an item: a Chao planted a seed in tree spot `slot` (`TREE_PLANT`, `kind` the
    /// seed's) or watered the tree there (`TREE_WATER`): the trees' keeper does it.
    Tree {
        area: u8,
        slot: u8,
        action: u8,
        kind: u8,
    },
}

impl ItemAct {
    /// Does it to `items` (area, owner, item), as the referee will.
    pub fn apply(&self, items: &mut Vec<(u8, u8, GardenItem)>) {
        let find = |items: &mut Vec<(u8, u8, GardenItem)>, id: ItemId| {
            items.iter().position(|(_, _, i)| i.id == id)
        };
        match *self {
            Self::Take {
                id,
                area,
                chao: (owner, index),
                ..
            } => {
                // Whoever's it is stays its owner (a player's item is only borrowed).
                if let Some(at) = find(items, id) {
                    let (a, _, item) = &mut items[at];
                    *a = area;
                    item.place = ItemPlace::Chao { owner, index };
                    item.position = IN_CHAO_HANDS;
                }
            }
            Self::Bite { id, left } => {
                if let Some(at) = find(items, id) {
                    if left == 0 {
                        items.remove(at);
                    } else {
                        items[at].2.bites = left;
                    }
                }
            }
            Self::Drop { id, at } => {
                if let Some(i) = find(items, id) {
                    items[i].2.place = ItemPlace::Ground;
                    items[i].2.position = at;
                }
            }
            Self::Gone { id } => {
                if let Some(at) = find(items, id) {
                    items.remove(at);
                }
            }
            // The referee gives it its id; the Chao see it from its next look.
            Self::New { .. } | Self::ToyOn { .. } | Self::Tree { .. } => {}
        }
    }
}

/// Garden `area`'s items as the Chao of `run` sense them: where each is (on the ground, in a
/// player's hands, in one of these Chao's hands). Ones in a player's own Chao's hands, or
/// out of sight in their game, aren't there for them.
fn things(
    items: &[(u8, u8, GardenItem)],
    flying: &[Flying],
    players: &HashMap<u8, Player>,
    run: &Run,
    area: u8,
    now: Instant,
) -> Vec<Thing> {
    items
        .iter()
        .filter(|(a, _, _)| *a == area)
        // One in the air can't be taken until it lands.
        .filter(|(_, _, i)| !flying.iter().any(|f| f.0 == i.id))
        .filter_map(|&(_, owner, item)| {
            let (pos, held, chao) = match item.place {
                ItemPlace::Ground => (item.position, false, None),
                ItemPlace::Busy => return None,
                ItemPlace::Player => {
                    let p = players
                        .get(&owner)
                        .filter(|p| now.duration_since(p.at) < PLAYER_STALE)?;
                    (
                        in_hands(p.position, p.rotation_y, item.position),
                        true,
                        None,
                    )
                }
                // In the hands of a Chao the server runs (whoever owns the item).
                ItemPlace::Chao { owner: o, index } if run.slots.contains_key(&(o, index)) => {
                    let slot = *run.slots.get(&(o, index))?;
                    let c = run.world.chao.get(slot)?.as_ref()?;
                    let pos = in_hands(c.body.pos, i32::from(c.body.rot_y), item.position);
                    (pos, false, Some(slot))
                }
                ItemPlace::Chao { .. } => return None,
            };
            Some(Thing {
                id: item.id,
                owner,
                category: item.category,
                kind: item.kind,
                bites: item.bites,
                pos,
                held,
                chao,
            })
        })
        .collect()
}

/// A player in a garden as its Chao sense them.
#[derive(Clone, Copy)]
struct Seen {
    /// Their player id.
    id: u8,
    pos: V3,
    rot_y: u16,
    character: u8,
    action: u8,
}

/// An item in a garden the server runs, as its Chao sense it.
#[derive(Clone, Copy, Debug)]
struct Thing {
    id: ItemId,
    /// Its owner: a player id, or `SERVER`.
    owner: u8,
    /// ALW category: 2 animal or Chaos Drive, 3 fruit, 7 seed, 9 hat.
    category: u8,
    kind: u8,
    bites: u8,
    pos: V3,
    /// In a player's hands.
    held: bool,
    /// In the hands of the Chao in this world slot.
    chao: Option<usize>,
}

/// The players, ball and items of a garden the server runs.
struct Players {
    here: Vec<Seen>,
    ball: Option<Ball>,
    things: Vec<Thing>,
    /// What each Chao (by world slot) carries that isn't a fruit.
    carried: HashMap<usize, Carried>,
    /// Chao (by world slot) a player's game runs for a touch now: let go of, they were thrown
    /// or dropped (their game has them), not put down.
    thrown: Vec<usize>,
    /// The TV's (0xC9) and radio's (0xCA) signals while they play: kind and where.
    playing: Vec<(u8, V3)>,
    /// The tree spots, their trees' states, and whether they want water.
    trees: Vec<(V3, u8, bool)>,
    /// Chao (by world slot) a player holds: who.
    holders: HashMap<usize, u8>,
    /// Locked Chao (by world slot): their owner's player ids, the only players whose items
    /// they take.
    locks: HashMap<usize, Vec<u8>>,
    /// The player each Chao (by world slot) minded this frame (`senses`): who its doings
    /// with "the player" were with.
    focus: RefCell<HashMap<usize, u8>>,
}

/// GrowTree's planting (0xAC) and watering (0xAE) spheres (re/chao-ai.md §6.1), and the
/// tree record states (byte 1) they're on in: an empty spot, a planted seed.
const PLANT_REACH: f32 = 30.0;
const WATER_REACH: f32 = 20.0;
const TREE_EMPTY: u8 = 0;
const TREE_SEED: u8 = 1;

/// A playing TV's or radio's signal radius (`0x55A850`, re/chao-ai.md §7.5).
const PLAYING_RADIUS: f32 = 25.5;

/// A player's whistle (`PlayerState::action`), and the petting action and how near a hand
/// must be (garden.rs's touches).
const WHISTLE: u8 = 42;
const PET: u8 = 43;
const PET_RANGE: f32 = 12.0;
/// Item colliders (re/chao-ai.md §6.1): radius 4.2, 8 high (a hat 5); a Chao touches with a
/// sphere of 1.5 two up.
const ITEM_RADIUS: f32 = 4.2;
/// Where an item a Chao holds is, in its frame (`ItemPlace::Chao`).
/// An item's position is its bottom (lying ones are at the floor): held at the chest, in
/// front.
pub const IN_CHAO_HANDS: [f32; 3] = [0.0, 0.6, 2.0];

fn distance(a: V3, b: V3) -> f32 {
    ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2) + (a[2] - b[2]).powi(2)).sqrt()
}

/// The item category a touch kind finds (re/chao-ai.md §6.1).
fn touch_category(kind: u8) -> Option<u8> {
    match kind {
        0x83 => Some(3),
        0x85 => Some(7),
        0x87 => Some(2),
        0x93 => Some(9),
        _ => None,
    }
}

/// A brain's fruit id for a shared item, and back.
fn fruit_id(id: ItemId) -> u32 {
    u32::from(id.origin) << 16 | u32::from(id.serial)
}

fn item_id(fruit: u32) -> ItemId {
    ItemId {
        origin: (fruit >> 16) as u8,
        serial: fruit as u16,
    }
}

impl Around for Players {
    /// The game knows one player; online, a Chao's is the one holding it, else the one
    /// whistling to it within 100 (`0x5637B0`), else the nearest. Seen: within sight and in
    /// front. Locked or not, it minds them all alike.
    fn senses(&self, slot: usize, pos: V3, rot_y: u16) -> Box<dyn Senses + '_> {
        let nearest = |a: &&Seen, b: &&Seen| distance(a.pos, pos).total_cmp(&distance(b.pos, pos));
        let holder = self
            .holders
            .get(&slot)
            .and_then(|id| self.here.iter().find(|p| p.id == *id));
        let whistler = || {
            self.here
                .iter()
                .filter(|p| p.action == WHISTLE && distance(p.pos, pos) < 100.0)
                .min_by(nearest)
        };
        let focus = holder
            .or_else(whistler)
            .or_else(|| self.here.iter().min_by(nearest));
        if let Some(p) = focus {
            self.focus.borrow_mut().insert(slot, p.id);
        }
        Box::new(ChaoSenses {
            focus: focus.copied(),
            seen: focus.is_some_and(|p| in_view(pos, rot_y, p.pos)),
            near: focus.is_some_and(|p| distance(p.pos, pos) < PET_RANGE),
            ball: self.ball,
            things: &self.things,
            playing: &self.playing,
            trees: &self.trees,
            carried: self.carried.get(&slot).copied(),
            thrown: self.thrown.contains(&slot),
            owners: self.locks.get(&slot).map(Vec::as_slice),
            slot,
            pos,
            rot_y,
        })
    }
}

/// One Chao's view of its player, the ball and the items.
struct ChaoSenses<'a> {
    focus: Option<Seen>,
    seen: bool,
    near: bool,
    ball: Option<Ball>,
    things: &'a [Thing],
    playing: &'a [(u8, V3)],
    trees: &'a [(V3, u8, bool)],
    carried: Option<Carried>,
    thrown: bool,
    /// Locked: its owner's player ids (only their items, and the server's, it takes).
    owners: Option<&'a [u8]>,
    slot: usize,
    pos: V3,
    rot_y: u16,
}

impl<'a> ChaoSenses<'a> {
    /// Only the items, for the Chao in `slot` at `pos`.
    fn at(things: &'a [Thing], slot: usize, pos: V3) -> Self {
        Self {
            focus: None,
            seen: false,
            near: false,
            ball: None,
            things,
            playing: &[],
            trees: &[],
            carried: None,
            thrown: false,
            owners: None,
            slot,
            pos,
            rot_y: 0,
        }
    }

    /// An item it may take: any, unless it's locked (then its owner's and the server's).
    fn may_take(&self, t: &Thing) -> bool {
        self.owners
            .is_none_or(|o| t.owner == SERVER || o.contains(&t.owner))
    }

    fn as_fruit(&self, t: &Thing) -> Fruit {
        Fruit {
            id: fruit_id(t.id),
            kind: t.kind,
            bites: t.bites,
            pos: t.pos,
            // A player's hand (`D+4 & 0x8000`); a Chao's is `taken`.
            held: t.held,
            taken: t.chao.is_some_and(|s| s != self.slot),
        }
    }

    /// The nearest tree spot in `state` whose sphere of `reach` its touch sphere reaches.
    fn tree_touched(&self, reach: f32, wanted: impl Fn(u8, bool) -> bool) -> Option<V3> {
        let me = [self.pos[0], self.pos[1] + 2.0, self.pos[2]];
        self.trees
            .iter()
            .filter(|&&(at, state, thirsty)| {
                wanted(state, thirsty) && distance(at, me) < reach + 1.5
            })
            .min_by(|a, b| distance(a.0, me).total_cmp(&distance(b.0, me)))
            .map(|&(at, _, _)| at)
    }

    /// Its touch sphere (1.5, two up) reaches item `t`'s collider.
    fn touches(&self, t: &Thing) -> bool {
        let height = if t.category == 9 { 5.0 } else { 8.0 };
        let y = self.pos[1] + 2.0;
        let flat = ((t.pos[0] - self.pos[0]).powi(2) + (t.pos[2] - self.pos[2]).powi(2)).sqrt();
        flat < ITEM_RADIUS + 1.5 && y > t.pos[1] - 1.5 && y < t.pos[1] + height + 1.5
    }
}

impl Senses for ChaoSenses<'_> {
    fn player_seen(&self) -> Option<usize> {
        self.focus.filter(|_| self.seen).map(|p| bond(p.character))
    }
    fn player_pos(&self) -> Option<V3> {
        self.focus.map(|p| p.pos)
    }
    fn player_rot_y(&self) -> Option<u16> {
        self.focus.map(|p| p.rot_y)
    }
    fn player_character(&self) -> Option<u8> {
        self.focus.map(|p| p.character)
    }
    fn whistling(&self) -> bool {
        self.focus.is_some_and(|p| p.action == WHISTLE)
    }
    fn petted(&self) -> bool {
        self.near && self.focus.is_some_and(|p| p.action == PET)
    }
    fn ball(&self) -> Option<Ball> {
        self.ball
    }
    /// The nearest fruit in view (only the nearest counts, re/chao-ai.md §7.5).
    fn nearest_fruit(&self) -> Option<Fruit> {
        self.things
            .iter()
            .filter(|t| t.category == 3 && t.chao != Some(self.slot) && self.may_take(t))
            .filter(|t| in_view(self.pos, self.rot_y, t.pos))
            .min_by(|a, b| distance(a.pos, self.pos).total_cmp(&distance(b.pos, self.pos)))
            .map(|t| self.as_fruit(t))
    }
    fn fruit(&self, id: u32) -> Option<Fruit> {
        self.things
            .iter()
            .find(|t| t.category == 3 && fruit_id(t.id) == id && self.may_take(t))
            .map(|t| self.as_fruit(t))
    }
    fn touching_fruit(&self) -> Option<Fruit> {
        self.things
            .iter()
            .find(|t| {
                t.category == 3 && t.chao != Some(self.slot) && self.may_take(t) && self.touches(t)
            })
            .map(|t| self.as_fruit(t))
    }
    /// Items no Chao holds.
    fn touch(&self, kind: u8) -> Option<V3> {
        let category = touch_category(kind)?;
        self.things
            .iter()
            .find(|t| {
                t.category == category && t.chao.is_none() && self.may_take(t) && self.touches(t)
            })
            .map(|t| t.pos)
    }
    fn carrying(&self) -> Option<Carried> {
        self.carried
    }
    /// Let go of: put down, unless its holder's game took it on (thrown or dropped).
    fn release(&self) -> Release {
        if self.thrown {
            Release::Dropped
        } else {
            Release::PutDown
        }
    }
    /// A knocked-off hat becomes the server's item (`ItemAct::New`), in no one's save: there is
    /// always room for it.
    fn hat_room(&self) -> bool {
        true
    }
    /// An empty tree spot whose planting sphere (0xAC, r 30) its touch sphere reaches.
    fn plant_spot(&self) -> Option<V3> {
        self.tree_touched(PLANT_REACH, |state, _| state == TREE_EMPTY)
    }
    /// A tree wanting water (a planted seed, or a grown one whose life ran low: its keeper
    /// says so) whose watering sphere (0xAE, r 20) its touch sphere reaches.
    fn watering_spot(&self) -> Option<V3> {
        self.tree_touched(WATER_REACH, |state, thirsty| state == TREE_SEED || thirsty)
    }
    /// Any tree spot in view, whatever grows there (re/chao-ai.md §7.5).
    fn tree_seen(&self) -> bool {
        self.trees
            .iter()
            .any(|&(at, _, _)| in_view(self.pos, self.rot_y, at))
    }
    fn nearest_tree(&self) -> Option<Tree> {
        self.trees
            .iter()
            .filter(|&&(at, _, _)| in_view(self.pos, self.rot_y, at))
            .map(|&(pos, state, _)| Tree {
                pos,
                dist: distance(pos, self.pos),
                state,
            })
            .min_by(|a, b| a.dist.total_cmp(&b.dist))
    }
    /// A playing TV's or radio's (no parent).
    fn signal(&self, kind: u8) -> Option<Signal> {
        let me = [self.pos[0], self.pos[1] + 2.0, self.pos[2]];
        self.playing
            .iter()
            .find(|&&(k, at)| k == kind && distance(at, me) < PLAYING_RADIUS + 1.5)
            .map(|&(_, pos)| Signal { pos, parent: None })
    }
}

/// The bond a character's Chao friendship is kept in (`0x53E0C0`: 1..5, else 0).
fn bond(character: u8) -> usize {
    if (1..=5).contains(&character) {
        usize::from(character)
    } else {
        0
    }
}

/// Who a Chao's doings with a player are with, by what it's doing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Party {
    /// The player holding it (held, its held moods, set down).
    Holder,
    /// The player who threw or dropped it (landing upset).
    Thrower,
    /// The player it minds (`Players::focus`): coming to a whistle, petted, knocked, fleeing
    /// them, its bond with them, running to and staying by them, pestering them.
    Focus,
}

/// The behaviours that are a Chao's doings with a player (re/chao-ai.md §2B), and with whom.
const WITH_A_PLAYER: [(u32, Party); 17] = [
    (0x566670, Party::Holder),
    (0x5660A0, Party::Holder),
    (0x5662B0, Party::Holder),
    (0x566300, Party::Holder),
    (0x566580, Party::Holder),
    (0x53D720, Party::Holder),
    (0x564320, Party::Thrower),
    (0x5634D0, Party::Focus),
    (0x563250, Party::Focus),
    (0x5648A0, Party::Focus),
    (0x563EB0, Party::Focus),
    (0x59F5F0, Party::Focus),
    (0x5A0490, Party::Focus),
    (0x5A0960, Party::Focus),
    (0x5A0820, Party::Focus),
    (0x5A18A0, Party::Focus),
    (0x5A1800, Party::Focus),
];

/// Whom Chao `chao` is busy with now, if it's a player: held, its holder; else by what it's
/// doing (`WITH_A_PLAYER`).
fn with_whom(chao: &Chao) -> Option<Party> {
    if chao.held {
        return Some(Party::Holder);
    }
    let doing = chao.current()?.address();
    WITH_A_PLAYER
        .iter()
        .find(|(at, _)| *at == doing)
        .map(|(_, party)| *party)
}

/// After a frame: what Chao `chao`'s owner's save keeps (`Chao::saved`). Busy with a player
/// it's locked against (`theirs`), it's the record from before (`before`, as the frame found
/// it) with only the life tick it had (`ticked`); the brain's record lives through it all.
/// Once they're done, the brain goes on from the kept record. The hat goes as the brain's
/// (one knocked off is an item on the ground now).
fn keep_save(
    chao: &mut Chao,
    theirs: bool,
    before: Option<&Record>,
    ticked: Option<&Vec<(u16, f32)>>,
) {
    let hat = chao.rec.u8(off::HAT);
    match (chao.saved.take(), theirs) {
        (None, true) => {
            let mut saved = before.cloned().unwrap_or_else(|| chao.rec.clone());
            if let Some(ticked) = ticked {
                changes::apply(&mut saved.0, ticked);
            }
            saved.set_u8(off::HAT, hat);
            chao.saved = Some(saved);
        }
        (Some(mut saved), true) => {
            saved.set_u8(off::HAT, hat);
            chao.saved = Some(saved);
        }
        (Some(mut saved), false) => {
            saved.set_u8(off::HAT, hat);
            chao.rec = saved;
        }
        (None, false) => {}
    }
}

/// Like the keeper's `hold_life`: a Chao the server runs never dies here (at the end of its
/// life it waits for its owner's game), nor evolves, unless it's held for its owner
/// (`evolves`): their game does that otherwise (`GardenChao::owner_runs`).
fn hold_life(rec: &mut Record, evolves: bool) {
    if rec.i16(off::LIFE) < 1 {
        rec.set_u16(off::LIFE, 1);
    }
    if !evolves && rec.f32(off::EVOLUTION) > 0.99 {
        rec.set_f32(off::EVOLUTION, 0.99);
    }
}

/// Garden `area`'s thrown items a frame further along their arc, until they land on the
/// floor; their place goes out every other frame so the others see them fly.
fn fly(flying: &mut Vec<Flying>, garden: &Garden, area: u8, acts: &mut Vec<ItemAct>) {
    flying.retain_mut(|(id, a, pos, vel, frames)| {
        if *a != area {
            return true;
        }
        for k in 0..3 {
            pos[k] += vel[k];
        }
        vel[1] -= ITEM_GRAVITY;
        *frames += 1;
        let floor = on_floor(garden, *pos);
        if (vel[1] < 0.0 && pos[1] <= floor[1]) || *frames > MAX_FLIGHT {
            acts.push(ItemAct::Drop { id: *id, at: floor });
            return false;
        }
        if frames.is_multiple_of(2) {
            acts.push(ItemAct::Drop { id: *id, at: *pos });
        }
        true
    });
}

/// A touched Chao's velocity (per frame) from report `from` to `to` `dt` apart: none for a
/// gap (a new touch, lost reports) or a jump (faster than a throw).
fn flight_velocity(from: V3, to: V3, dt: Duration) -> V3 {
    let frames = dt.as_secs_f32() / FRAME.as_secs_f32();
    if !(0.5..=FLIGHT_GAP).contains(&frames) {
        return [0.0; 3];
    }
    let v = [
        (to[0] - from[0]) / frames,
        (to[1] - from[1]) / frames,
        (to[2] - from[2]) / frames,
    ];
    if (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt() > FASTEST_FLIGHT {
        return [0.0; 3];
    }
    v
}

/// Where a Chao reported at `pos` moving at `vel` (per frame) is `frames` later, and its
/// velocity then: falling as the game's gravity has it (`physics::GRAVITY`), stopped on the
/// floor.
fn flight(garden: &Garden, pos: V3, vel: V3, frames: f32) -> (V3, V3) {
    if vel == [0.0; 3] {
        return (pos, vel);
    }
    let t = frames.max(0.0);
    let g = crate::chao_sim::physics::GRAVITY;
    let at = [
        pos[0] + vel[0] * t,
        pos[1] + vel[1] * t + 0.5 * g * t * t,
        pos[2] + vel[2] * t,
    ];
    let floor = on_floor(garden, at);
    let falling = vel[1] + g * t;
    if at[1] <= floor[1] && falling <= 0.0 {
        return (floor, [vel[0], 0.0, vel[2]]);
    }
    (at, [vel[0], falling, vel[2]])
}

/// Reports further apart than this (frames) give no velocity; faster than this (per frame) is a
/// jump, not a throw (the game's throw is about (1, 1) forward and up, `0x475600`).
const FLIGHT_GAP: f32 = 15.0;
const FASTEST_FLIGHT: f32 = 4.0;

/// An item in the air: id, garden, where, velocity (per frame), frames flown.
type Flying = (ItemId, u8, V3, V3, u32);

/// A thrown item falls this much faster each frame, and lands after this many frames at most.
const ITEM_GRAVITY: f32 = 0.055;
const MAX_FLIGHT: u32 = 120;

/// Running Chao are checked for having fallen under the ground this often (frames).
const RESCUE_EVERY: u64 = 10;

/// The server's lying items are checked against the floor this often.
const SETTLE_EVERY: u64 = 60;

/// The floor under `at` (from a little above it), or `at` itself over none.
fn on_floor(garden: &Garden, at: V3) -> V3 {
    let floor = crate::chao_sim::land::probe(&garden.tris, [at[0], at[1] + 10.0, at[2]])[2];
    floor.map_or(at, |f| [at[0], f.at, at[2]])
}

/// How near a Chao may come to a player (their collision radii together).
const PLAYER_ROOM: f32 = 5.0;

/// Moves a Chao at `pos` out of the room of a player at `player`, along the ground.
fn push_out(pos: &mut V3, player: [f32; 3]) {
    if (pos[1] - player[1]).abs() > 10.0 {
        return;
    }
    let (dx, dz) = (pos[0] - player[0], pos[2] - player[2]);
    let d = (dx * dx + dz * dz).sqrt();
    if d >= PLAYER_ROOM {
        return;
    }
    // Right on top of them: out along +x.
    let (nx, nz) = if d > 1e-3 {
        (dx / d, dz / d)
    } else {
        (1.0, 0.0)
    };
    pos[0] = player[0] + nx * PLAYER_ROOM;
    pos[2] = player[2] + nz * PLAYER_ROOM;
}

/// Where a Chao held at `offset` (in the holder's frame) is.
fn in_hands(pos: [f32; 3], rotation_y: i32, offset: [f32; 3]) -> [f32; 3] {
    let a = rotation_y as f32 * std::f32::consts::TAU / 65536.0;
    let (s, c) = (a.sin(), a.cos());
    [
        pos[0] + offset[0] * c + offset[2] * s,
        pos[1] + offset[1],
        pos[2] - offset[0] * s + offset[2] * c,
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chao_sim::brain::Behaviour;
    use crate::chao_sim::land::{GRID_CELLS, Tri};

    /// A floor at y = 0 (corners within the probe's reach), with rows of water.
    fn flat() -> Garden {
        let mut grid = vec![0; GRID_CELLS];
        for (i, c) in grid.iter_mut().enumerate() {
            if (i / 32) % 2 == 0 {
                *c = 1;
            }
        }
        Garden {
            tris: vec![
                Tri {
                    v: [
                        [-100.0, 0.0, -100.0],
                        [100.0, 0.0, -100.0],
                        [-100.0, 0.0, 100.0],
                    ],
                    flags: 1,
                },
                Tri {
                    v: [
                        [100.0, 0.0, -100.0],
                        [100.0, 0.0, 100.0],
                        [-100.0, 0.0, 100.0],
                    ],
                    flags: 1,
                },
            ],
            grid,
            ledges: Vec::new(),
            blockers: Vec::new(),
            toys: Vec::new(),
            tree_spots: Vec::new(),
        }
    }

    fn state(area: i8, position: [f32; 3]) -> Body {
        Body::PlayerState(PlayerState {
            time: 0,
            level: crate::game::LEVEL_CHAO_WORLD,
            mission: 0,
            chao_area: area,
            character: 0,
            position,
            rotation: [0; 3],
            anim: 0,
            frame: 0.0,
            flags: 0,
            room: (0xFF, 0),
            upgrades: 0,
            action: 0,
            status: 0,
        })
    }

    fn upload(host: &mut ChaoHost, from: u8, garden: &Garden, now: Instant) {
        let bytes = garden.to_bytes();
        let parts: Vec<&[u8]> = bytes.chunks(super::super::protocol::LAND_PART).collect();
        for (i, part) in parts.iter().enumerate() {
            host.observe(
                from,
                &Body::GardenLand {
                    garden: 0,
                    fingerprint: garden.fingerprint(),
                    part: i as u8,
                    parts: parts.len() as u8,
                    bytes: part.to_vec(),
                },
                now,
            );
        }
    }

    fn child() -> Box<ChaoData> {
        let mut rec = Record::default();
        rec.set_u8(off::TYPE, crate::chao_sim::record::TYPE_CHILD);
        rec.set_u8(0x81, 1); // garden: Neutral
        rec.set_u16(off::LIFE, 3000);
        Box::new(rec.0)
    }

    #[test]
    fn two_players_in_a_garden_get_its_chao_from_the_server() {
        let mut host = ChaoHost::new();
        let t0 = Instant::now();
        let garden = flat();
        upload(&mut host, 0, &garden, t0);
        host.observe(0, &state(1, [0.0; 3]), t0);
        host.observe(
            0,
            &Body::Chao {
                index: 0,
                data: child(),
            },
            t0,
        );
        let visiting = GardenChao {
            index: 0,
            position: [10.0, 0.0, 10.0],
            ..GardenChao::default()
        };
        host.observe(
            0,
            &Body::GardenChao {
                owner: 0,
                list: vec![visiting],
            },
            t0,
        );
        // Alone: the player's game runs it.
        let mut out = Vec::new();
        host.tick(t0 + Duration::from_millis(100), Shared::default(), &mut out);
        assert!(out.is_empty());
        // A friend comes in: the server runs it and tells them both.
        host.observe(
            3,
            &state(1, [5.0, 0.0, 0.0]),
            t0 + Duration::from_millis(100),
        );
        let mut out = Vec::new();
        host.tick(t0 + Duration::from_millis(200), Shared::default(), &mut out);
        let to: BTreeSet<u8> = out
            .iter()
            .filter_map(|o| match (&o.to, &o.body) {
                (
                    To::One(id),
                    Body::ServerChao {
                        area: 1,
                        owner: 0,
                        list,
                    },
                ) if list.len() == 1 => Some(*id),
                _ => None,
            })
            .collect();
        assert_eq!(to, BTreeSet::from([0, 3]));
    }

    /// A host running player 0's Chao 0 in Neutral with player 3 there too, at `t0 + 200 ms`.
    fn running(t0: Instant) -> ChaoHost {
        running_on(flat(), t0)
    }

    fn running_on(garden: Garden, t0: Instant) -> ChaoHost {
        let mut host = ChaoHost::new();
        upload(&mut host, 0, &garden, t0);
        host.observe(0, &state(1, [0.0; 3]), t0);
        host.observe(3, &state(1, [40.0, 0.0, 40.0]), t0);
        host.observe(
            0,
            &Body::Chao {
                index: 0,
                data: child(),
            },
            t0,
        );
        host.observe(
            0,
            &Body::GardenChao {
                owner: 0,
                list: vec![GardenChao {
                    index: 0,
                    position: [10.0, 0.0, 10.0],
                    ..GardenChao::default()
                }],
            },
            t0,
        );
        // The first tick starts the clock.
        host.tick(
            t0 + Duration::from_millis(100),
            Shared::default(),
            &mut Vec::new(),
        );
        host.tick(
            t0 + Duration::from_millis(200),
            Shared::default(),
            &mut Vec::new(),
        );
        assert!(host.chao_mut((0, 0)).is_some());
        host
    }

    /// Steps `host` from `from` to `to` 100 ms at a time; what it sent.
    fn run_to(host: &mut ChaoHost, from: Duration, to: Duration, t0: Instant) -> Vec<Out> {
        let mut out = Vec::new();
        let mut at = from;
        while at < to {
            at += Duration::from_millis(100);
            host.tick(t0 + at, Shared::default(), &mut out);
        }
        out
    }

    fn ms(n: u64) -> Duration {
        Duration::from_millis(n)
    }

    fn changes_to_owner(out: &[Out]) -> Vec<Vec<(u16, f32)>> {
        out.iter()
            .filter_map(|o| match (&o.to, o.from, &o.body) {
                (
                    To::One(0),
                    SERVER,
                    Body::ChaoChanges {
                        owner: 0,
                        index: 0,
                        changes,
                    },
                ) => Some(changes.clone()),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn what_the_server_changes_goes_to_the_owner_once() {
        let t0 = Instant::now();
        let mut host = running(t0);
        // Its AI raised a stat's progress (say, a fruit).
        let chao = host.chao_mut((0, 0)).unwrap();
        chao.rec.set_u8(0x20, chao.rec.u8(0x20) + 5);
        let out = run_to(&mut host, ms(200), ms(1400), t0);
        assert_eq!(changes_to_owner(&out), vec![vec![(0x20, 5.0)]]);
        // Sent once; the life tick's changes never go (they keep the players there).
        let mut out = Vec::new();
        for s in 1..=20u64 {
            host.observe(0, &state(1, [0.0; 3]), t0 + ms(1400) * s as u32);
            host.observe(3, &state(1, [40.0, 0.0, 40.0]), t0 + ms(1400) * s as u32);
            let visiting = GardenChao {
                index: 0,
                ..GardenChao::default()
            };
            let list = vec![visiting];
            host.observe(
                0,
                &Body::GardenChao { owner: 0, list },
                t0 + ms(1400) * s as u32,
            );
            out.extend(run_to(
                &mut host,
                ms(1400) * s as u32,
                ms(1400) * (s as u32 + 1),
                t0,
            ));
        }
        assert!(host.chao_mut((0, 0)).is_some());
        assert!(changes_to_owner(&out).is_empty());
    }

    #[test]
    fn the_others_get_its_live_fields_each_second() {
        let t0 = Instant::now();
        let mut host = running(t0);
        host.chao_mut((0, 0))
            .unwrap()
            .rec
            .set_u16(off::HUNGER, 4321);
        let out = run_to(&mut host, ms(200), ms(1400), t0);
        let live: Vec<(u8, Vec<u8>)> = out
            .iter()
            .filter_map(|o| match (&o.to, &o.body) {
                (
                    To::One(id),
                    Body::ChaoLive {
                        owner: 0,
                        index: 0,
                        bytes,
                    },
                ) => Some((*id, bytes.clone())),
                _ => None,
            })
            .collect();
        // To player 3, and to its owner (their save takes its emotions and needs).
        let to: BTreeSet<u8> = live.iter().map(|(id, _)| *id).collect();
        assert_eq!(to, BTreeSet::from([0, 3]));
        // HUNGER (0x138) sits 0x0C into the emotions/needs field, after 2 + 12 bytes.
        let at = 2 + 12 + (0x138 - 0x12C);
        assert_eq!(u16::from_le_bytes([live[0].1[at], live[0].1[at + 1]]), 4321);
    }

    #[test]
    fn a_thrown_chao_goes_on_from_where_it_landed() {
        let t0 = Instant::now();
        let mut host = running(t0);
        // Player 3 threw it; their game reports where it is while it runs it.
        for step in 0..5u32 {
            let at = t0 + ms(300) + ms(100) * step;
            let mut mood = vec![0; CHAO_MOOD.1];
            mood[1] = 180; // anger
            let body = Body::ChaoTouched {
                owner: 0,
                index: 0,
                position: [30.0, 0.0, 25.0],
                rotation_y: 0x4000,
                motion: 0x20B,
                mood,
            };
            host.observe(3, &body, at);
            host.tick(at, Shared::default(), &mut Vec::new());
        }
        let chao = host.chao_mut((0, 0)).unwrap();
        assert_eq!(chao.body.pos, [30.0, 0.0, 25.0]);
        assert_eq!(chao.body.rot_y, 0x4000);
        assert!(
            chao.rec.u8(0x12D) > 150,
            "it keeps the anger the touch left"
        );
        // Done: the server's goes on from there.
        let _ = run_to(&mut host, ms(800), ms(1400), t0);
        let pos = host.chao_mut((0, 0)).unwrap().body.pos;
        assert!(distance(pos, [30.0, 0.0, 25.0]) < 5.0, "{pos:?}");
    }

    #[test]
    fn a_chao_its_owner_throws_flies_on_and_lands() {
        let t0 = Instant::now();
        let mut host = running(t0);
        host.tick(t0 + ms(200), Shared::default(), &mut Vec::new());
        // Its owner holds it up in front of them ...
        let hold = Body::ChaoHeld {
            owner: 0,
            index: 0,
            offset: [0.0, 8.0, 3.0],
        };
        host.observe(0, &hold, t0 + ms(250));
        host.tick(t0 + ms(280), Shared::default(), &mut Vec::new());
        assert_eq!(host.chao_mut((0, 0)).unwrap().body.pos, [0.0, 8.0, 3.0]);
        // ... and throws it (about (1, 1) a frame): their game reports it flying, then goes
        // quiet with it in the air.
        let throw = |position| Body::ChaoTouched {
            owner: 0,
            index: 0,
            position,
            rotation_y: 0x4000,
            motion: 0x10F,
            mood: vec![0; CHAO_MOOD.1],
        };
        host.observe(0, &throw([0.0, 8.0, 5.0]), t0 + ms(300));
        host.tick(t0 + ms(300), Shared::default(), &mut Vec::new());
        // Their hold ended with the first report, not half a second later.
        assert_eq!(host.chao_mut((0, 0)).unwrap().body.pos, [0.0, 8.0, 5.0]);
        host.observe(0, &throw([2.0, 10.0, 5.0]), t0 + ms(333));
        host.tick(t0 + ms(383), Shared::default(), &mut Vec::new());
        // Between reports it flies on.
        let pos = host.chao_mut((0, 0)).unwrap().body.pos;
        assert!(pos[0] > 4.0 && pos[1] > 11.0, "{pos:?}");
        // Reports stopped mid-air: it comes down and lands, and doesn't hang there.
        let _ = run_to(&mut host, ms(400), ms(2500), t0);
        let chao = host.chao_mut((0, 0)).unwrap();
        assert!(chao.body.pos[1].abs() < 2.5, "{:?}", chao.body.pos);
        assert!(chao.body.pos[0] > 6.0, "{:?}", chao.body.pos);
        assert!(!chao.held);
    }

    #[test]
    fn a_chao_planting_or_watering_asks_the_trees_keeper() {
        let t0 = Instant::now();
        let mut garden = flat();
        garden.tree_spots = vec![[60.0, 0.0, 60.0], [-60.0, 0.0, -60.0]];
        let mut host = running_on(garden, t0);
        let chao = host.chao_mut((0, 0)).unwrap();
        chao.tree_events.push(TreeEvent::Plant {
            spot_pos: Some([-60.0, 0.0, -60.0]),
            seed_kind: 3,
        });
        chao.tree_events.push(TreeEvent::Water {
            pos: [60.0, 0.0, 70.0],
            radius: 5.0,
            frames: 10,
        });
        host.tick(t0 + ms(220), Shared::default(), &mut Vec::new());
        let acts = host.take_item_acts();
        assert!(acts.contains(&ItemAct::Tree {
            area: 1,
            slot: 1,
            action: TREE_PLANT,
            kind: 3
        }));
        assert!(acts.contains(&ItemAct::Tree {
            area: 1,
            slot: 0,
            action: TREE_WATER,
            kind: 0
        }));
    }

    #[test]
    fn a_thrown_fruit_flies_and_lands_on_the_floor() {
        let garden = flat();
        let id = ItemId {
            origin: 1,
            serial: 2,
        };
        // `throw_away`'s velocity: 0.3 sideways, 0.7 up, from 2 above the floor.
        let mut flying = vec![(id, 1, [0.0, 2.0, 0.0], [0.3, 0.7, 0.0], 0)];
        let mut acts = Vec::new();
        for _ in 0..MAX_FLIGHT {
            fly(&mut flying, &garden, 1, &mut acts);
        }
        assert!(flying.is_empty());
        let Some(ItemAct::Drop { at, .. }) = acts.last() else {
            panic!("{acts:?}");
        };
        assert!(at[0] > 5.0, "it went somewhere: {at:?}");
        assert!(at[1].abs() < 1e-3, "on the floor: {at:?}");
        assert!(acts.len() > 5, "the others saw it fly");
    }

    #[test]
    fn a_held_chaos_changes_are_its_holders_to_send() {
        let t0 = Instant::now();
        let mut host = running(t0);
        let at = t0 + Duration::from_millis(250);
        host.observe(
            3,
            &Body::ChaoHeld {
                owner: 0,
                index: 0,
                offset: [0.0, 1.0, 2.0],
            },
            at,
        );
        host.tick(at, Shared::default(), &mut Vec::new());
        let chao = host.chao_mut((0, 0)).unwrap();
        chao.rec.set_u8(0x20, chao.rec.u8(0x20) + 5);
        let out = run_to(&mut host, ms(250), ms(1400), t0);
        assert!(changes_to_owner(&out).is_empty());
    }

    #[test]
    fn its_owners_record_does_not_reset_the_servers_needs() {
        let t0 = Instant::now();
        let mut host = running(t0);
        host.chao_mut((0, 0)).unwrap().rec.set_u16(off::HUNGER, 123);
        // The owner's game sends its record again (its look changed), its hunger older.
        let mut data = child();
        data.bytes[0x138..0x13A].copy_from_slice(&9000u16.to_le_bytes());
        host.observe(0, &Body::Chao { index: 0, data }, t0 + ms(300));
        assert_eq!(host.chao_mut((0, 0)).unwrap().rec.u16(off::HUNGER), 123);
    }

    #[test]
    fn its_owners_record_keeps_what_the_server_has_not_sent() {
        let t0 = Instant::now();
        let mut host = running(t0);
        let chao = host.chao_mut((0, 0)).unwrap();
        chao.rec.set_u8(0x20, 7);
        // The owner sends its record again (its look changed) before the server sent that.
        let mut data = child();
        data.bytes[0x21] = 3;
        host.observe(
            0,
            &Body::Chao { index: 0, data },
            t0 + Duration::from_millis(300),
        );
        let rec = &host.chao_mut((0, 0)).unwrap().rec;
        assert_eq!((rec.u8(0x20), rec.u8(0x21)), (7, 3));
        let out = run_to(&mut host, ms(300), ms(1400), t0);
        assert_eq!(changes_to_owner(&out), vec![vec![(0x20, 7.0)]]);
    }

    #[test]
    fn a_hungry_chao_takes_a_players_fruit_and_eats_it_up() {
        let t0 = Instant::now();
        // Ground everywhere (`flat` has rows of water).
        let mut dry = flat();
        dry.grid.fill(1);
        let mut host = running_on(dry, t0);
        let mango = ItemId {
            origin: 3,
            serial: 7,
        };
        let chao = host.chao_mut((0, 0)).unwrap();
        let ahead = in_hands(chao.body.pos, i32::from(chao.body.rot_y), [0.0, 0.0, 15.0]);
        // The referee's view: player 3's fruit on the ground in front of it.
        let mut items = vec![(
            1,
            3,
            GardenItem {
                id: mango,
                category: 3,
                kind: 3,
                bites: 4,
                position: ahead,
                ..GardenItem::default()
            },
        )];
        let mut acts = Vec::new();
        let mut at = ms(200);
        while at < Duration::from_secs(120) && !items.is_empty() {
            at += ms(100);
            let chao = host.chao_mut((0, 0)).unwrap();
            chao.rec.set_u16(off::HUNGER, 10_000);
            if at.as_millis().is_multiple_of(1000) {
                host.observe(0, &state(1, [0.0; 3]), t0 + at);
                host.observe(3, &state(1, [40.0, 0.0, 40.0]), t0 + at);
                let list = vec![GardenChao {
                    index: 0,
                    ..GardenChao::default()
                }];
                host.observe(0, &Body::GardenChao { owner: 0, list }, t0 + at);
            }
            host.tick(
                t0 + at,
                Shared {
                    items: items.clone(),
                    ..Shared::default()
                },
                &mut Vec::new(),
            );
            for act in host.take_item_acts() {
                act.apply(&mut items);
                acts.push(act);
            }
        }
        assert!(matches!(
            acts.first(),
            Some(&ItemAct::Take {
                id,
                area: 1,
                chao: (0, 0),
                ..
            }) if id == mango
        ));
        let bites: Vec<u8> = acts
            .iter()
            .filter_map(|a| match a {
                ItemAct::Bite { id, left } if *id == mango => Some(*left),
                _ => None,
            })
            .collect();
        assert_eq!(bites, [3, 2, 1, 0]);
        assert!(items.is_empty());
    }

    #[test]
    fn the_ground_most_players_agree_on_wins() {
        let mut host = ChaoHost::new();
        let now = Instant::now();
        let a = flat();
        let mut b = flat();
        b.grid[0] = 9;
        upload(&mut host, 0, &b, now);
        upload(&mut host, 1, &a, now);
        upload(&mut host, 2, &a, now);
        assert_eq!(host.land(0).map(|(f, _)| f), Some(a.fingerprint()));
    }

    #[test]
    fn a_held_chao_is_in_its_holders_hands() {
        assert_eq!(
            in_hands([1.0, 2.0, 3.0], 0, [0.0, 1.0, 2.0]),
            [1.0, 3.0, 5.0]
        );
        let p = in_hands([0.0; 3], 0x4000, [0.0, 0.0, 2.0]);
        assert!((p[0] - 2.0).abs() < 1e-5 && p[2].abs() < 1e-5);
    }

    const PLAYER_1: PlayerKey = [7; 16];
    const PLAYER_2: PlayerKey = [9; 16];

    /// A child of the Neutral Garden with DNA `dna`.
    fn kid(dna: u8) -> Box<ChaoData> {
        let mut c = child();
        c.bytes[0x438 + 0x10] = dna;
        c
    }

    fn brings(list: &[(u8, u8, Box<ChaoData>)], awake: bool) -> Body {
        brings_locked(list, awake, ChaoLock::Always)
    }

    fn brings_locked(list: &[(u8, u8, Box<ChaoData>)], awake: bool, lock: ChaoLock) -> Body {
        Body::Brought {
            awake,
            lock,
            list: list
                .iter()
                .map(|(garden, row, data)| BroughtChao {
                    garden: *garden,
                    row: *row,
                    nth: 0,
                    data: data.clone(),
                })
                .collect(),
        }
    }

    /// Steps `host` from `from` to `to` 100 ms at a time, each player in `each` sending these
    /// (their state, their stream) every step; what it sent.
    fn live(
        host: &mut ChaoHost,
        each: &[(u8, Body)],
        from: Duration,
        to: Duration,
        t0: Instant,
    ) -> Vec<Out> {
        let mut out = Vec::new();
        let mut at = from;
        while at < to {
            at += ms(100);
            for (id, body) in each {
                host.observe(*id, body, t0 + at);
            }
            host.tick(t0 + at, Shared::default(), &mut out);
        }
        out
    }

    /// Held Chao as player `to` was told of them: (owner, index) -> latest.
    fn told(out: &[Out], to: u8) -> BTreeMap<Key, Option<HeldChao>> {
        let mut seen = BTreeMap::new();
        for o in out {
            if let (To::One(id), Body::HeldChao { owner, index, chao }) = (&o.to, &o.body)
                && *id == to
            {
                seen.insert((*owner, *index), chao.clone());
            }
        }
        seen
    }

    /// The server's latest stream of held Chao `key` to player `to`.
    fn streamed(out: &[Out], to: u8, key: Key) -> Option<GardenChao> {
        out.iter().rev().find_map(|o| match (&o.to, &o.body) {
            (To::One(id), Body::ServerChao { owner, list, .. }) if *id == to && *owner == key.0 => {
                list.iter().find(|c| c.index == key.1).copied()
            }
            _ => None,
        })
    }

    fn saves_to(out: &[Out], to: u8) -> Vec<(u64, u8, Spans)> {
        out.iter()
            .filter_map(|o| match (&o.to, &o.body) {
                (
                    To::One(id),
                    Body::SaveChao {
                        identity,
                        nth,
                        spans,
                    },
                ) if *id == to => Some((*identity, *nth, spans.clone())),
                _ => None,
            })
            .collect()
    }

    /// Player1 (player 0) and Player2 (player 3) online, Player2's game sent the Neutral Garden.
    fn two_keys(t0: Instant) -> ChaoHost {
        let mut host = ChaoHost::new();
        host.joined(0, PLAYER_1, "Player1");
        host.joined(3, PLAYER_2, "Player2");
        upload(&mut host, 3, &flat(), t0);
        host
    }

    #[test]
    fn a_chao_its_owner_left_sleeps_where_it_was_and_only_moves_when_carried() {
        let t0 = Instant::now();
        let mut host = two_keys(t0);
        let record = kid(1);
        host.observe(0, &brings(&[(1, 0, record.clone())], true), t0);
        host.observe(
            0,
            &Body::Chao {
                index: 0,
                data: record.clone(),
            },
            t0,
        );
        let stream = Body::GardenChao {
            owner: 0,
            list: vec![GardenChao {
                index: 0,
                position: [10.0, 0.0, 10.0],
                ..GardenChao::default()
            }],
        };
        let player_2 = (3, state(1, [40.0, 0.0, 40.0]));
        let player_1 = [(0, state(1, [0.0; 3])), (0, stream), player_2.clone()];
        // Both there: it's Player1's own, run as ever; nobody is told of a held one.
        let out = live(&mut host, &player_1, ms(0), ms(2000), t0);
        assert!(host.chao_mut((0, 0)).is_some());
        assert!(told(&out, 3).is_empty());
        let was = host.chao_mut((0, 0)).unwrap().body.pos;
        // Player1 goes offline: it stays, asleep where it was.
        host.left(0);
        let player_2 = [player_2];
        let out = live(&mut host, &player_2, ms(2000), ms(4000), t0);
        let held = told(&out, 3);
        assert_eq!(held.len(), 1);
        let (&key, chao) = held.iter().next().unwrap();
        assert!(key.0 >= HELD_OWNER);
        let chao = chao.as_ref().unwrap();
        assert!(chao.asleep && chao.name == "Player1" && chao.area == 1);
        let c = streamed(&out, 3, key).unwrap();
        assert_eq!((c.motion, c.eyes), (SLEEP_MOTION, SLEEP_EYES));
        assert!(distance(c.position, was) < 0.01, "{:?} {was:?}", c.position);
        // A minute later nothing about it changed: no life, no items, nothing to anyone's save.
        let fruit = GardenItem {
            id: ItemId {
                origin: 3,
                serial: 1,
            },
            category: 3,
            bites: 4,
            position: was,
            ..GardenItem::default()
        };
        let mut out = Vec::new();
        for step in 0..600u32 {
            let at = t0 + ms(4000) + ms(100) * step;
            host.observe(player_2[0].0, &player_2[0].1, at);
            let shared = Shared {
                items: vec![(1, 3, fruit)],
                ..Shared::default()
            };
            host.tick(at, shared, &mut out);
        }
        assert!(host.take_item_acts().is_empty());
        assert!(out.iter().all(|o| !matches!(
            o.body,
            Body::SaveChao { .. } | Body::ChaoChanges { .. } | Body::ChaoLive { .. }
        )));
        let id = host.shown[&key].id;
        assert_eq!(host.roster.get(&id).unwrap().record, record);
        // Player2 carries it off and puts it down: it sleeps there.
        let carried = [
            player_2[0].clone(),
            (
                3,
                Body::ChaoHeld {
                    owner: key.0,
                    index: key.1,
                    offset: [0.0, 2.0, 3.0],
                },
            ),
        ];
        let out = live(&mut host, &carried, ms(64_000), ms(64_500), t0);
        let c = streamed(&out, 3, key).unwrap();
        assert_eq!(c.motion, HELD_MOTION);
        assert!(
            distance(c.position, [40.0, 2.0, 43.0]) < 0.01,
            "{:?}",
            c.position
        );
        let out = live(&mut host, &player_2, ms(64_500), ms(66_000), t0);
        let c = streamed(&out, 3, key).unwrap();
        assert_eq!(c.motion, SLEEP_MOTION);
        assert!(distance(c.position, [40.0, 2.0, 43.0]) < 0.01);
        // A changed Chao from Player2's touch changes nothing of a sleeping one.
        host.observe(
            3,
            &Body::ChaoChanges {
                owner: key.0,
                index: key.1,
                changes: vec![(0x82, 10.0)],
            },
            t0 + ms(66_000),
        );
        assert_eq!(host.roster.get(&id).unwrap().record, record);
    }

    #[test]
    fn an_away_owners_chao_lives_on_and_only_they_hear_of_it() {
        let t0 = Instant::now();
        let mut host = two_keys(t0);
        // Player1 is in the Hero Garden; their Chao lives in the Neutral one, where Player2 is alone.
        host.observe(0, &brings(&[(1, 0, kid(1))], true), t0);
        let each = [(0, state(2, [0.0; 3])), (3, state(1, [40.0, 0.0, 40.0]))];
        let out = live(&mut host, &each, ms(0), ms(3000), t0);
        let held = told(&out, 3);
        let (&key, chao) = held.iter().next().expect("Player2 sees it");
        assert!(!chao.as_ref().unwrap().asleep);
        assert!(told(&out, 0).is_empty(), "not told of their own");
        assert!(host.chao_mut(key).is_some(), "it runs");
        assert!(streamed(&out, 3, key).is_some());
        assert!(streamed(&out, 0, key).is_none());
        // Its life goes on, and into Player1's save: the life tick's needs too.
        let out = live(&mut host, &each, ms(3000), ms(30_000), t0);
        let saves = saves_to(&out, 0);
        assert!(!saves.is_empty());
        assert!(saves_to(&out, 3).is_empty());
        let id = host.shown[&key].id;
        assert!(saves.iter().all(|(i, n, _)| (*i, *n) == (id.identity, 0)));
        let (at, len) = (0x126, 0x3A);
        assert!(saves.iter().any(|(_, _, spans)| {
            spans
                .iter()
                .any(|(o, b)| usize::from(*o) >= at && usize::from(*o) + b.len() <= at + len)
        }));
        // The save has what the server's had at its last send.
        let mut save = *kid(1);
        for (_, _, spans) in &saves {
            for (o, b) in spans {
                save.bytes[usize::from(*o)..usize::from(*o) + b.len()].copy_from_slice(b);
            }
        }
        assert!(save_spans(&save, &host.sent[&key]).is_empty());
        assert_ne!(save, *kid(1));

        // Player1 comes into the garden: their game has it again, the held copy goes.
        let record = kid(1);
        host.observe(
            0,
            &Body::Chao {
                index: 0,
                data: record,
            },
            t0 + ms(30_000),
        );
        let back = [
            (0, state(1, [0.0; 3])),
            (
                0,
                Body::GardenChao {
                    owner: 0,
                    list: vec![GardenChao {
                        index: 0,
                        ..GardenChao::default()
                    }],
                },
            ),
            each[1].clone(),
        ];
        let out = live(&mut host, &back, ms(30_000), ms(31_500), t0);
        assert_eq!(told(&out, 3).get(&key), Some(&None));
        assert!(host.chao_mut(key).is_none());
        assert!(host.chao_mut((0, 0)).is_some());
        assert!(host.shown.is_empty());
    }

    #[test]
    fn an_online_player_without_their_save_loaded_has_their_chao_asleep() {
        let t0 = Instant::now();
        let mut host = two_keys(t0);
        host.observe(0, &brings(&[(1, 0, kid(1))], false), t0);
        let each = [(0, state(0, [0.0; 3])), (3, state(1, [40.0, 0.0, 40.0]))];
        let out = live(&mut host, &each, ms(0), ms(3000), t0);
        assert!(told(&out, 3).values().all(|c| c.as_ref().unwrap().asleep));
        // Loaded: it wakes up.
        host.observe(0, &brings(&[(1, 0, kid(1))], true), t0 + ms(3000));
        let out = live(&mut host, &each, ms(3000), ms(4000), t0);
        let held = told(&out, 3);
        assert_eq!(held.len(), 1);
        assert!(held.values().all(|c| !c.as_ref().unwrap().asleep));
        // No longer brought (moved home, given away): it goes.
        host.observe(0, &brings(&[], true), t0 + ms(4000));
        let out = live(&mut host, &each, ms(4000), ms(5000), t0);
        assert!(told(&out, 3).values().all(Option::is_none));
        assert!(host.shown.is_empty());
    }

    #[test]
    fn a_held_chao_evolves_here_and_its_owner_gets_the_new_record() {
        let t0 = Instant::now();
        let mut host = two_keys(t0);
        let mut ready = kid(2);
        ready.bytes[0xC0..0xC4].copy_from_slice(&1.0f32.to_le_bytes());
        // Leaning to swim, and hero: a hero swim adult (type 9).
        ready.bytes[0xAC..0xB0].copy_from_slice(&(-0.8f32).to_le_bytes());
        ready.bytes[0xB0..0xB4].copy_from_slice(&0.8f32.to_le_bytes());
        host.observe(0, &brings(&[(1, 0, ready)], true), t0);
        let each = [(0, state(2, [0.0; 3])), (3, state(1, [40.0, 0.0, 40.0]))];
        let mut out = Vec::new();
        let mut at = ms(0);
        while at < Duration::from_secs(200)
            && !saves_to(&out, 0)
                .iter()
                .any(|(_, _, s)| s.iter().any(|(o, _)| *o == 0x80))
        {
            out.extend(live(&mut host, &each, at, at + ms(1000), t0));
            at += ms(1000);
        }
        let saves = saves_to(&out, 0);
        let typed = saves
            .iter()
            .flat_map(|(_, _, s)| s)
            .find(|(o, _)| *o == 0x80)
            .expect("it evolved");
        assert_eq!(typed.1[0], 9);
        // Everyone there sees the adult (its identity is the same Chao's).
        let held = told(&out, 3);
        let (_, adult) = held.iter().next().unwrap();
        let adult = adult.as_ref().unwrap();
        assert_eq!(adult.data.chao_type(), 9);
        assert_eq!(adult.data.identity(), kid(2).identity());
        // While it was in the cocoon, the others were shown it.
        assert!(out.iter().any(|o| matches!(
            &o.body,
            Body::ServerChao { owner, list, .. }
                if *owner >= HELD_OWNER && list.iter().any(|c| c.cocoon == 1)
        )));
    }

    #[test]
    fn a_restarted_server_shows_the_chao_it_kept() {
        let t0 = Instant::now();
        let mut host = two_keys(t0);
        host.observe(0, &brings(&[(1, 0, kid(1)), (1, 1, kid(3))], true), t0);
        let files = host.files();
        assert_eq!(files.len(), 1);
        let mut again = ChaoHost::new();
        for (name, text) in &files {
            again.load_file(name, text.as_deref().unwrap());
        }
        again.joined(3, PLAYER_2, "Player2");
        upload(&mut again, 3, &flat(), t0);
        let each = [(3, state(1, [40.0, 0.0, 40.0]))];
        let out = live(&mut again, &each, ms(0), ms(3000), t0);
        let held = told(&out, 3);
        assert_eq!(held.len(), 2);
        assert!(
            held.values()
                .all(|c| c.as_ref().is_some_and(|c| c.asleep && c.name == "Player1"))
        );
    }

    /// Player `id`'s state in garden `area` at `position`, doing `action`.
    fn doing(area: i8, position: [f32; 3], action: u8) -> Body {
        let Body::PlayerState(mut s) = state(area, position) else {
            unreachable!()
        };
        s.action = action;
        Body::PlayerState(s)
    }

    /// Player1 (0) and Player2 (3) in the Neutral Garden, Player1's Chao 0 run by the server, Player1's
    /// lock `lock`; at `t0 + 2 s`.
    fn mason_brings(lock: ChaoLock, t0: Instant) -> (ChaoHost, [(u8, Body); 3]) {
        let mut host = two_keys(t0);
        host.observe(0, &brings_locked(&[(1, 0, kid(1))], true, lock), t0);
        host.observe(
            0,
            &Body::Chao {
                index: 0,
                data: kid(1),
            },
            t0,
        );
        let each = [
            (0, state(1, [0.0; 3])),
            (
                0,
                Body::GardenChao {
                    owner: 0,
                    list: vec![GardenChao {
                        index: 0,
                        position: [10.0, 0.0, 10.0],
                        ..GardenChao::default()
                    }],
                },
            ),
            (3, state(1, [40.0, 0.0, 40.0])),
        ];
        live(&mut host, &each, ms(0), ms(2000), t0);
        assert!(host.chao_mut((0, 0)).is_some());
        (host, each)
    }

    #[test]
    fn a_locked_chaos_owner_gets_nothing_others_do_to_it() {
        let t0 = Instant::now();
        // Present with them in its garden: only Never locks it.
        for (lock, locked) in [
            (ChaoLock::Always, false),
            (ChaoLock::ChaoWorld, false),
            (ChaoLock::ThatGarden, false),
            (ChaoLock::Never, true),
        ] {
            let (host, _) = mason_brings(lock, t0);
            assert_eq!(host.locked_against(3, (0, 0)), locked, "{lock:?}");
            assert!(!host.locked_against(0, (0, 0)), "never against its owner");
        }
        let (mut host, each) = mason_brings(ChaoLock::Never, t0);
        let before = host.chao_mut((0, 0)).unwrap().rec.clone();
        // Player2's changes and the mood their throw left: not taken.
        host.observe(
            3,
            &Body::ChaoChanges {
                owner: 0,
                index: 0,
                changes: vec![(0x20, 9.0)],
            },
            t0 + ms(2000),
        );
        let mut mood = vec![0; CHAO_MOOD.1];
        mood[1] = 180;
        let throw = Body::ChaoTouched {
            owner: 0,
            index: 0,
            position: [30.0, 0.0, 25.0],
            rotation_y: 0,
            motion: 0x20B,
            mood,
        };
        host.observe(3, &throw, t0 + ms(2000));
        let chao = host.chao_mut((0, 0)).unwrap();
        assert_eq!(chao.rec.u8(0x20), before.u8(0x20));
        assert_eq!(chao.rec.u8(0x12D), before.u8(0x12D));
        // ... but it was thrown all the same.
        host.tick(t0 + ms(2050), Shared::default(), &mut Vec::new());
        assert_eq!(host.chao_mut((0, 0)).unwrap().body.pos, [30.0, 0.0, 25.0]);
        // Player1's own changes are taken.
        host.observe(
            0,
            &Body::ChaoChanges {
                owner: 0,
                index: 0,
                changes: vec![(0x21, 4.0)],
            },
            t0 + ms(2100),
        );
        assert_eq!(host.chao_mut((0, 0)).unwrap().rec.u8(0x21), 4);
        let _ = each;
    }

    /// What the server changed in Player1's Chao while Player2 pets it, as sent to them.
    fn petted_by_erin(lock: ChaoLock) -> Vec<Vec<(u16, f32)>> {
        let t0 = Instant::now();
        let (mut host, each) = mason_brings(lock, t0);
        let chao = host.chao_mut((0, 0)).unwrap();
        chao.rec.set_u8(0x20, chao.rec.u8(0x20) + 5);
        let mut out = Vec::new();
        let mut at = ms(2000);
        while at < ms(3400) {
            at += ms(100);
            let pos = host.chao_mut((0, 0)).unwrap().body.pos;
            host.observe(0, &each[0].1, t0 + at);
            host.observe(0, &each[1].1, t0 + at);
            host.observe(3, &doing(1, [pos[0] + 3.0, 0.0, pos[2]], PET), t0 + at);
            host.tick(t0 + at, Shared::default(), &mut out);
        }
        changes_to_owner(&out)
    }

    #[test]
    fn petting_a_locked_chao_doesnt_keep_the_servers_changes_from_its_owner() {
        // Unlocked, Player2's game sends their petting's changes: the server's wait.
        assert!(petted_by_erin(ChaoLock::Always).is_empty());
        // Locked, theirs are dropped: the server's go on to Player1.
        assert!(
            petted_by_erin(ChaoLock::Never)
                .iter()
                .any(|c| c.contains(&(0x20, 5.0)))
        );
    }

    #[test]
    fn a_locked_chao_takes_only_its_owners_and_the_servers_items() {
        let fruit = |owner: u8, serial: u16| Thing {
            id: ItemId {
                origin: owner,
                serial,
            },
            owner,
            category: 3,
            kind: 3,
            bites: 4,
            pos: [0.0; 3],
            held: false,
            chao: None,
        };
        let erins = [fruit(3, 1)];
        let mixed = [fruit(3, 1), fruit(0, 2)];
        let servers = [fruit(SERVER, 3)];
        let senses = |things: &'static [Thing], owners: Option<&'static [u8]>| {
            let mut s = ChaoSenses::at(things, 0, [0.0; 3]);
            s.owners = owners;
            s.touching_fruit().map(|f| item_id(f.id).origin)
        };
        let erins: &'static [Thing] = Box::leak(Box::new(erins));
        let mixed: &'static [Thing] = Box::leak(Box::new(mixed));
        let servers: &'static [Thing] = Box::leak(Box::new(servers));
        assert_eq!(senses(erins, None), Some(3));
        assert_eq!(senses(erins, Some(&[0])), None);
        assert_eq!(senses(mixed, Some(&[0])), Some(0));
        assert_eq!(senses(servers, Some(&[0])), Some(SERVER));
        // Anyone's petting counts (only its save doesn't keep a stranger's).
        let petting = |id: u8, owners: Option<&'static [u8]>| {
            let mut s = ChaoSenses::at(&[], 0, [0.0; 3]);
            s.near = true;
            s.owners = owners;
            s.focus = Some(Seen {
                id,
                pos: [1.0, 0.0, 0.0],
                rot_y: 0,
                character: 0,
                action: PET,
            });
            s.petted()
        };
        assert!(petting(3, None));
        assert!(petting(3, Some(&[0])));
        assert!(petting(0, Some(&[0])));
    }

    #[test]
    fn a_held_chao_is_locked_by_where_its_owner_is() {
        let t0 = Instant::now();
        let away_at = |lock: ChaoLock, player_1: Body| {
            let mut host = two_keys(t0);
            host.observe(0, &brings_locked(&[(1, 0, kid(1))], true, lock), t0);
            let each = [(0, player_1), (3, state(1, [40.0, 0.0, 40.0]))];
            let out = live(&mut host, &each, ms(0), ms(3000), t0);
            let (key, chao) = told(&out, 3).into_iter().next().expect("Player2 sees it");
            let chao = chao.unwrap();
            (host, key, chao, each)
        };
        let away = |lock: ChaoLock, mason_area: i8| away_at(lock, state(mason_area, [0.0; 3]));
        // Player1 in the Hero Garden, their Chao in the Neutral one.
        for (lock, locked) in [
            (ChaoLock::Always, false),
            (ChaoLock::ChaoWorld, false),
            (ChaoLock::ThatGarden, true),
            (ChaoLock::Never, true),
        ] {
            let (host, key, chao, _) = away(lock, 2);
            assert_eq!(chao.locked, locked, "{lock:?}");
            assert_eq!(host.locked_against(3, key), locked, "{lock:?}");
        }
        // Player1 elsewhere in Chao World (the lobby, the Kindergarten, a race, karate): "Chao
        // World" leaves it open, "that garden" doesn't.
        for area in [0, 4, 6, 10] {
            let (_, _, chao, _) = away(ChaoLock::ChaoWorld, area);
            assert!(!chao.locked, "{area}");
            let (_, _, chao, _) = away(ChaoLock::ThatGarden, area);
            assert!(chao.locked, "{area}");
        }
        // Out of Chao World (a stage): "Chao World" locks it too.
        let Body::PlayerState(mut stage) = state(0, [0.0; 3]) else {
            unreachable!()
        };
        stage.level = 13;
        let (_, _, chao, _) = away_at(ChaoLock::ChaoWorld, Body::PlayerState(stage));
        assert!(chao.locked);
        // Player2's changes reach Player1's save (`SaveChao`) only while it's unlocked.
        let saved_change = |lock: ChaoLock| {
            let (mut host, key, _, each) = away(lock, 2);
            host.observe(
                3,
                &Body::ChaoChanges {
                    owner: key.0,
                    index: key.1,
                    changes: vec![(0x20, 9.0)],
                },
                t0 + ms(3000),
            );
            let out = live(&mut host, &each, ms(3000), ms(4500), t0);
            saves_to(&out, 0).iter().any(|(_, _, spans)| {
                spans.iter().any(|(o, b)| {
                    *o <= 0x20 && b.len() > usize::from(0x20 - *o) && b[usize::from(0x20 - *o)] == 9
                })
            })
        };
        assert!(saved_change(ChaoLock::ChaoWorld));
        assert!(!saved_change(ChaoLock::ThatGarden));
        // Changing the lock: the others are told at the next look.
        let (mut host, key, _, each) = away(ChaoLock::Never, 2);
        host.observe(
            0,
            &brings_locked(&[(1, 0, kid(1))], true, ChaoLock::ChaoWorld),
            t0 + ms(3000),
        );
        let out = live(&mut host, &each, ms(3000), ms(4000), t0);
        assert_eq!(told(&out, 3)[&key].as_ref().map(|c| c.locked), Some(false));
        // Player1 goes offline: asleep, locked again ("Chao World": not there).
        host.left(0);
        let out = live(&mut host, &each[1..], ms(4000), ms(6000), t0);
        let chao = told(&out, 3)[&key].clone().unwrap();
        assert!(chao.asleep && chao.locked);
    }

    #[test]
    fn coming_back_alone_to_ones_own_chao_runs_nothing() {
        let t0 = Instant::now();
        let mut host = two_keys(t0);
        host.observe(0, &brings(&[(1, 0, kid(1))], true), t0);
        // Player1 walks into its garden, alone (Player2 is in a stage); their stream comes late.
        let each = [(0, state(1, [0.0; 3])), (3, state(0, [0.0; 3]))];
        let out = live(&mut host, &each, ms(0), ms(8000), t0);
        assert!(host.runs[0].is_none());
        assert!(host.shown.is_empty());
        assert!(
            !out.iter()
                .any(|o| matches!(o.body, Body::ServerChao { .. } | Body::HeldChao { .. }))
        );
    }

    #[test]
    fn a_paused_game_alone_with_its_own_chao_starts_no_run() {
        let t0 = Instant::now();
        let mut host = two_keys(t0);
        host.observe(0, &brings(&[(1, 0, kid(1))], true), t0);
        let stream = Body::GardenChao {
            owner: 0,
            list: vec![GardenChao {
                index: 0,
                ..GardenChao::default()
            }],
        };
        let each = [
            (0, state(1, [0.0; 3])),
            (0, stream),
            (3, state(0, [0.0; 3])),
        ];
        let mut out = live(&mut host, &each, ms(0), ms(3000), t0);
        // Player1 tabs out for half a minute (their game sends nothing), then comes back.
        out.extend(live(&mut host, &each[2..], ms(3000), ms(33_000), t0));
        out.extend(live(&mut host, &each, ms(33_000), ms(36_000), t0));
        assert!(host.runs[0].is_none());
        assert!(host.shown.is_empty());
        assert!(
            !out.iter()
                .any(|o| matches!(o.body, Body::ServerChao { .. } | Body::HeldChao { .. }))
        );
    }

    #[test]
    fn a_paused_player_keeps_their_chao_and_no_held_copy_of_it_shows() {
        let t0 = Instant::now();
        let mut host = two_keys(t0);
        let record = kid(1);
        host.observe(0, &brings(&[(1, 0, record.clone())], true), t0);
        host.observe(
            0,
            &Body::Chao {
                index: 0,
                data: record,
            },
            t0,
        );
        let stream = Body::GardenChao {
            owner: 0,
            list: vec![GardenChao {
                index: 0,
                position: [10.0, 0.0, 10.0],
                ..GardenChao::default()
            }],
        };
        let each = [
            (0, state(1, [0.0; 3])),
            (0, stream),
            (3, state(1, [40.0, 0.0, 40.0])),
        ];
        live(&mut host, &each, ms(0), ms(2000), t0);
        assert!(host.chao_mut((0, 0)).is_some());
        // Player1's game is paused (a tab-out) for 30 s; Player2 plays on.
        let out = live(&mut host, &each[2..], ms(2000), ms(32_000), t0);
        assert!(
            host.chao_mut((0, 0)).is_some(),
            "it's still theirs, run here"
        );
        assert!(host.shown.is_empty());
        assert!(told(&out, 3).is_empty(), "no held copy of it");
        assert!(streamed(&out, 3, (0, 0)).is_some());
        // Back: as it was, nothing made anew.
        let out = live(&mut host, &each, ms(32_000), ms(34_000), t0);
        assert!(host.chao_mut((0, 0)).is_some());
        assert!(host.shown.is_empty() && told(&out, 3).is_empty());
    }

    #[test]
    fn a_chao_under_the_ground_is_put_back_on_it() {
        let t0 = Instant::now();
        let mut host = running(t0);
        let chao = host.chao_mut((0, 0)).unwrap();
        chao.body.pos = [10.0, -40.0, 10.0];
        run_to(&mut host, ms(200), ms(600), t0);
        let pos = host.chao_mut((0, 0)).unwrap().body.pos;
        assert!(pos[1] > -1.0, "{pos:?}");
    }
    /// What player `to` was last told is locked against them.
    fn locked_to(out: &[Out], to: u8) -> Option<Vec<Key>> {
        out.iter().rev().find_map(|o| match (&o.to, &o.body) {
            (To::One(id), Body::ChaoLocked { chao }) if *id == to => Some(chao.clone()),
            _ => None,
        })
    }

    #[test]
    fn everyone_hears_which_chao_are_locked_against_them() {
        let t0 = Instant::now();
        let (mut host, each) = mason_brings(ChaoLock::Never, t0);
        // Player2 heard Player1's Chao is locked against them; Player1 that nothing is against them.
        assert_eq!(host.told_locks.get(&3), Some(&vec![(0, 0)]));
        assert_eq!(host.told_locks.get(&0), Some(&Vec::new()));
        // Told once, not again while it stays so.
        let out = live(&mut host, &each, ms(3000), ms(4000), t0);
        assert_eq!(locked_to(&out, 3), None);
        // Player1 opens it up: Player2 hears at the next look.
        host.observe(
            0,
            &brings_locked(&[(1, 0, kid(1))], true, ChaoLock::Always),
            t0 + ms(4000),
        );
        let out = live(&mut host, &each, ms(4000), ms(5000), t0);
        assert_eq!(locked_to(&out, 3), Some(Vec::new()));
        // Reconnecting, they're told again.
        host.joined(3, PLAYER_2, "Player2");
        host.observe(
            0,
            &brings_locked(&[(1, 0, kid(1))], true, ChaoLock::Never),
            t0 + ms(5000),
        );
        let out = live(&mut host, &each, ms(5000), ms(6000), t0);
        assert_eq!(locked_to(&out, 3), Some(vec![(0, 0)]));
    }

    #[test]
    fn a_held_chao_locked_against_you_is_on_your_list() {
        let t0 = Instant::now();
        let mut host = two_keys(t0);
        host.observe(
            0,
            &brings_locked(&[(1, 0, kid(1))], true, ChaoLock::ThatGarden),
            t0,
        );
        // Player1 in the Hero Garden, their Chao held in the Neutral one with Player2.
        let each = [(0, state(2, [0.0; 3])), (3, state(1, [40.0, 0.0, 40.0]))];
        let out = live(&mut host, &each, ms(0), ms(3000), t0);
        let (key, _) = told(&out, 3).into_iter().next().expect("Player2 sees it");
        assert_eq!(locked_to(&out, 3), Some(vec![key]));
    }

    /// Player state in garden `area` (0 none), loading on (`POSE_LOADING`) or settled.
    fn moving(area: i8, loading: bool) -> Body {
        let Body::PlayerState(mut s) = state(area, [0.0; 3]) else {
            unreachable!()
        };
        if loading {
            s.flags |= crate::game::POSE_LOADING;
        }
        Body::PlayerState(s)
    }

    #[test]
    fn a_load_keeps_its_owner_where_they_were_a_while() {
        let t0 = Instant::now();
        let mut host = two_keys(t0);
        host.observe(
            0,
            &brings_locked(&[(1, 0, kid(1))], true, ChaoLock::ChaoWorld),
            t0,
        );
        // Player1 in the Hero Garden, their Chao held in the Neutral one with Player2.
        let each = [(0, state(2, [0.0; 3])), (3, state(1, [40.0, 0.0, 40.0]))];
        let out = live(&mut host, &each, ms(0), ms(3000), t0);
        let (key, _) = told(&out, 3).into_iter().next().expect("Player2 sees it");
        let at = |host: &mut ChaoHost, body: &Body, t: Duration| {
            host.observe(0, body, t0 + t);
            host.locked_against(3, key)
        };
        let stage = |loading: bool| {
            let Body::PlayerState(mut s) = moving(0, loading) else {
                unreachable!()
            };
            s.level = 13;
            Body::PlayerState(s)
        };
        assert!(!at(&mut host, &state(2, [0.0; 3]), ms(3000)));
        // Loads and places all over Chao World: never out of it.
        assert!(!at(&mut host, &moving(0, true), ms(3100)));
        assert!(!at(&mut host, &moving(0, true), ms(13_200)));
        assert!(!at(&mut host, &moving(0, false), ms(14_000)));
        assert!(!at(&mut host, &moving(6, false), ms(14_500)));
        assert!(!at(&mut host, &moving(3, false), ms(15_000)));
        // A stage's load: still in the Dark Garden for its locks...
        assert!(!at(&mut host, &stage(true), ms(16_000)));
        assert!(!at(&mut host, &stage(true), ms(25_000)));
        // ... until the grace is over, or the stage settles.
        assert!(at(&mut host, &stage(true), ms(26_100)));
        assert!(!at(&mut host, &moving(0, false), ms(27_000)));
        assert!(!at(&mut host, &stage(true), ms(27_500)));
        assert!(at(&mut host, &stage(false), ms(28_000)));
        // Gone: offline at once.
        assert!(!at(&mut host, &moving(2, false), ms(29_000)));
        assert!(!at(&mut host, &stage(true), ms(29_100)));
        host.left(0);
        assert!(host.locked_against(3, key));
    }

    #[test]
    fn moving_on_from_a_garden_keeps_that_garden_lock_open_meanwhile() {
        let t0 = Instant::now();
        let (mut host, _) = mason_brings(ChaoLock::ThatGarden, t0);
        assert!(!host.locked_against(3, (0, 0)));
        host.observe(0, &moving(0, true), t0 + ms(2100));
        assert!(!host.locked_against(3, (0, 0)));
        host.observe(0, &moving(0, false), t0 + ms(2200));
        assert!(host.locked_against(3, (0, 0)));
    }

    /// Player1's Chao (lock `lock`) while Player2 does `player_2` to it from `t0 + 2 s` to `until`:
    /// the host, its record before, and what went to Player1.
    fn erin_does(
        lock: ChaoLock,
        prepare: impl Fn(&mut Chao),
        player_2: impl Fn(V3) -> Vec<Body>,
        until: Duration,
        t0: Instant,
    ) -> (ChaoHost, Record, Vec<Out>, [(u8, Body); 3]) {
        let (mut host, each) = mason_brings(lock, t0);
        prepare(host.chao_mut((0, 0)).unwrap());
        let before = host.chao_mut((0, 0)).unwrap().rec.clone();
        let mut out = Vec::new();
        let mut at = ms(2000);
        while at < until {
            at += ms(50);
            let pos = host.chao_mut((0, 0)).unwrap().body.pos;
            for (id, body) in &each[..2] {
                host.observe(*id, body, t0 + at);
            }
            for body in player_2(pos) {
                host.observe(3, &body, t0 + at);
            }
            host.tick(t0 + at, Shared::default(), &mut out);
        }
        (host, before, out, each)
    }

    fn hold() -> Body {
        Body::ChaoHeld {
            owner: 0,
            index: 0,
            offset: [0.0, 1.0, 2.0],
        }
    }

    /// Whether a change of `at` to Player1 (`ChaoChanges`) or anyone (`ChaoLive`) shows `pick`.
    fn sent_mood(out: &[Out], at: u16, pick: impl Fn(u8) -> bool) -> bool {
        let live_at = CHAO_LIVE_FIELDS
            .iter()
            .scan(0, |n, &(o, len)| {
                let here = (usize::from(at) >= o && usize::from(at) < o + len)
                    .then(|| *n + usize::from(at) - o);
                *n += len;
                Some(here)
            })
            .flatten()
            .next();
        changes_to_owner(out)
            .iter()
            .flatten()
            .any(|&(o, v)| o == at && pick(v as u8))
            || out.iter().any(|o| match &o.body {
                Body::ChaoLive { bytes, .. } => live_at.is_some_and(|i| pick(bytes[i])),
                _ => false,
            })
    }

    #[test]
    fn held_by_someone_its_locked_against_it_reacts_but_its_save_doesnt_change() {
        let t0 = Instant::now();
        let held = |lock: ChaoLock| {
            erin_does(
                lock,
                |c| c.rec.set_u8(0x132, 0),
                |_| vec![doing(1, [40.0, 0.0, 40.0], 0), hold()],
                ms(5000),
                t0,
            )
        };
        // Unlocked: held, and its mood in the hands (`0x132` full) is its own.
        let (mut host, _, _, _) = held(ChaoLock::Always);
        let chao = host.chao_mut((0, 0)).unwrap();
        assert!(chao.held && chao.saved.is_none());
        assert!(chao.rec.u8(0x132) > 100);
        // Locked: just the same in the hands...
        let (mut host, before, out, each) = held(ChaoLock::Never);
        let chao = host.chao_mut((0, 0)).unwrap();
        assert!(chao.held);
        assert_eq!(with_whom(chao), Some(Party::Holder));
        assert!(chao.rec.u8(0x132) > 100);
        assert_eq!(
            chao.body.pos,
            in_hands([40.0, 0.0, 40.0], 0, [0.0, 1.0, 2.0])
        );
        // ... shown with its own held motion, not a stand-in...
        let shown = out.iter().rev().find_map(|o| match (&o.to, &o.body) {
            (To::One(0), Body::ServerChao { owner: 0, list, .. }) => list.first().map(|c| c.motion),
            _ => None,
        });
        assert_eq!(shown, Some(chao.motion));
        // ... but what its save keeps hasn't that mood; its life ticks go on there.
        let kept = chao.kept().clone();
        assert!(kept.u8(0x132) < 10);
        assert!(kept.u32(off::AGE_CLOCK) > before.u32(off::AGE_CLOCK));
        assert!(!sent_mood(&out, 0x132, |v| v > 100));
        // Let go: the brain goes on from the kept record.
        live(&mut host, &each, ms(5000), ms(7000), t0);
        let chao = host.chao_mut((0, 0)).unwrap();
        assert!(!chao.held && chao.saved.is_none());
        assert!(chao.rec.u8(0x132) < 10);
    }

    #[test]
    fn a_locked_chao_thrown_by_another_lands_upset_but_its_save_doesnt_change() {
        let t0 = Instant::now();
        let thrown = |lock: ChaoLock| {
            let (mut host, before, _, each) = erin_does(
                lock,
                |c| {
                    for at in 0x12D..=0x131 {
                        c.rec.set_u8(at, 0);
                    }
                },
                |_| vec![doing(1, [40.0, 0.0, 40.0], 0), hold()],
                ms(2500),
                t0,
            );
            // Player2 throws it: their game has it for a moment, then it's let go.
            let throw = Body::ChaoTouched {
                owner: 0,
                index: 0,
                position: [30.0, 3.0, 25.0],
                rotation_y: 0,
                motion: 0x10F,
                mood: Vec::new(),
            };
            let mut out = Vec::new();
            let mut at = ms(2500);
            let mut upset = false;
            while at < ms(5500) {
                at += ms(50);
                for (id, body) in &each {
                    host.observe(*id, body, t0 + at);
                }
                if at < ms(2700) {
                    host.observe(3, &throw, t0 + at);
                }
                host.tick(t0 + at, Shared::default(), &mut out);
                let chao = host.chao_mut((0, 0)).unwrap();
                upset |=
                    chao.current() == Some(Behaviour::Game(0x564320)) && chao.rec.u8(0x12D) >= 50;
            }
            (host, before, out, upset)
        };
        // Unlocked or not, it lands upset (`0x564320`).
        let (mut host, _, _, upset) = thrown(ChaoLock::Always);
        assert!(upset);
        assert!(host.chao_mut((0, 0)).unwrap().kept().u8(0x12D) >= 40);
        let (mut host, _, out, upset) = thrown(ChaoLock::Never);
        assert!(upset);
        // Locked: its save never had it, and now neither does it.
        assert!(host.chao_mut((0, 0)).unwrap().kept().u8(0x12D) < 40);
        assert!(!sent_mood(&out, 0x12D, |v| v >= 40));
    }

    #[test]
    fn a_locked_chao_comes_to_anyones_whistle_but_keeps_its_bond() {
        let t0 = Instant::now();
        let whistled = |lock: ChaoLock| {
            erin_does(
                lock,
                // It likes Sonic (Player2's character, bond slot 0).
                |c| c.rec.set_u8(off::BONDS, 60),
                |_| vec![doing(1, [40.0, 0.0, 40.0], WHISTLE)],
                ms(2600),
                t0,
            )
        };
        let (mut host, before, _, _) = whistled(ChaoLock::Never);
        let chao = host.chao_mut((0, 0)).unwrap();
        assert!(
            WITH_A_PLAYER
                .iter()
                .any(|(at, _)| chao.current() == Some(Behaviour::Game(*at))),
            "{:?}",
            chao.current()
        );
        assert!(chao.saved.is_some());
        assert_eq!(chao.kept().u8(off::BONDS), before.u8(off::BONDS));
        // Player1 whistles instead: their doing, kept.
        let (mut host, each) = mason_brings(ChaoLock::Never, t0);
        host.chao_mut((0, 0)).unwrap().rec.set_u8(off::BONDS, 60);
        let player_1 = [
            (0, doing(1, [0.0; 3], WHISTLE)),
            each[1].clone(),
            each[2].clone(),
        ];
        live(&mut host, &player_1, ms(2000), ms(2600), t0);
        let chao = host.chao_mut((0, 0)).unwrap();
        assert!(chao.current().is_some_and(|b| b.address() == 0x5634D0));
        assert!(chao.saved.is_none());
    }

    #[test]
    fn a_locked_chaos_own_life_still_reaches_its_owner_while_others_pet_it() {
        let t0 = Instant::now();
        // What the server changes of its own goes to Player1 as ever, petted by Player2 or not.
        let petted = |lock: ChaoLock| {
            let (mut host, before, out, _) = erin_does(
                lock,
                |c| c.rec.set_u8(0x132, 0),
                |pos| vec![doing(1, [pos[0] + 3.0, 0.0, pos[2]], PET)],
                ms(4000),
                t0,
            );
            let chao = host.chao_mut((0, 0)).unwrap();
            let petted = chao.current() == Some(Behaviour::Game(0x5648A0));
            (
                petted,
                chao.kept().u32(off::AGE_CLOCK) > before.u32(off::AGE_CLOCK),
                out,
            )
        };
        let (reacts, lived, out) = petted(ChaoLock::Never);
        assert!(reacts && lived);
        // The petting's warmth (`0x132` up) stays with the brain.
        assert!(!sent_mood(&out, 0x132, |v| v >= 10));
        let (reacts, _, _) = petted(ChaoLock::Always);
        assert!(reacts);
    }

    #[test]
    fn a_locked_chao_minds_everyone_alike() {
        let seen = |id: u8, action: u8, pos: V3| Seen {
            id,
            pos,
            rot_y: 0,
            character: 2,
            action,
        };
        let around = |here: Vec<Seen>, holders: HashMap<usize, u8>| Players {
            here,
            ball: None,
            things: Vec::new(),
            carried: HashMap::new(),
            thrown: Vec::new(),
            playing: Vec::new(),
            trees: Vec::new(),
            holders,
            locks: HashMap::from([(0, vec![0])]),
            focus: RefCell::new(HashMap::new()),
        };
        // Locked to Player1 (0): Player2's (3) whistle, look and character reach it all the same.
        let player_2 = around(vec![seen(3, WHISTLE, [3.0, 0.0, 0.0])], HashMap::new());
        let senses = player_2.senses(0, [0.0; 3], 0);
        assert!(senses.whistling() && senses.player_character() == Some(2));
        drop(senses);
        assert_eq!(player_2.focus.borrow().get(&0), Some(&3));
        // Player2's whistle over Player1 standing nearer.
        let both = around(
            vec![
                seen(3, WHISTLE, [9.0, 0.0, 0.0]),
                seen(0, 0, [3.0, 0.0, 0.0]),
            ],
            HashMap::new(),
        );
        assert_eq!(
            both.senses(0, [0.0; 3], 0).player_pos(),
            Some([9.0, 0.0, 0.0])
        );
        // Held, its holder is its player.
        let held = around(
            vec![
                seen(3, WHISTLE, [9.0, 0.0, 0.0]),
                seen(0, 0, [3.0, 0.0, 0.0]),
            ],
            HashMap::from([(0, 0)]),
        );
        assert_eq!(
            held.senses(0, [0.0; 3], 0).player_pos(),
            Some([3.0, 0.0, 0.0])
        );
        assert_eq!(held.focus.borrow().get(&0), Some(&0));
    }

    #[test]
    fn before_its_garden_runs_a_locked_chao_is_its_owners_games_alone() {
        // No ground yet: each owner's game runs its own Chao (its life goes straight into
        // their save). The only changes others send are their touches, and those are dropped.
        let t0 = Instant::now();
        let mut host = ChaoHost::new();
        host.joined(0, PLAYER_1, "Player1");
        host.joined(3, PLAYER_2, "Player2");
        host.observe(
            0,
            &brings_locked(&[(1, 0, kid(1))], true, ChaoLock::Never),
            t0,
        );
        host.observe(
            0,
            &Body::Chao {
                index: 0,
                data: kid(1),
            },
            t0,
        );
        let each = [(0, state(1, [0.0; 3])), (3, state(1, [40.0, 0.0, 40.0]))];
        let out = live(&mut host, &each, ms(0), ms(3000), t0);
        assert!(host.runs[0].is_none());
        assert!(host.locked_against(3, (0, 0)) && !host.locked_against(0, (0, 0)));
        assert!(changes_to_owner(&out).is_empty());
        assert_eq!(locked_to(&out, 3), Some(vec![(0, 0)]));
    }
}
