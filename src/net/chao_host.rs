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

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::chao::{ChaoData, changes};
use crate::chao_sim::brain::{Chao, FruitEvent, ItemEvent, TreeEvent};
use crate::chao_sim::land::Garden;
use crate::chao_sim::land::V3;
use crate::chao_sim::motion::MotionTable;
use crate::chao_sim::picker::{Ball, Carried, Fruit, Release, Senses, Signal, ToyEvent, Tree};
use crate::chao_sim::record::{Record, off};
use crate::chao_sim::world::World;
use crate::chao_sim::world::{Around, in_view};

use super::protocol::{
    BALL_HELD, BALL_IN_WATER, Body, CHAO_LIVE_FIELDS, CHAO_MOOD, ChaoSound, GardenChao, GardenItem,
    ItemId, ItemPlace, MAX_CARRIED_CHAO, MAX_CHAO_SOUNDS, PlayerState, SOUND_AT, TOY_BOX,
    TOY_RADIO, TOY_TV, TREE_PLANT, TREE_SLOTS, TREE_WATER,
};
use super::referee::{Out, To};

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
/// Each Chao's state goes to the log every 5 seconds.
const STATUS_EVERY: u64 = 300;
/// A Chao's record changes go to its owner every second.
const CHANGES_EVERY: u64 = 60;
/// After a player holds, pets or changes a Chao, its changes are theirs to send this long.
const TOUCH_QUIET: Duration = Duration::from_secs(3);
/// The Chao's voice call (`0x4372E0`) as the behaviours make it: no holder, volume 0x6E.
const VOICE_ARGS: [i16; 3] = [0, 0, 0x6E];
/// The sender id the server's own Chao messages carry.
use super::protocol::SERVER;

type Key = (u8, u8);

/// A garden's ball: position, velocity, `Ball::flags`, when heard.
type BallSeen = ([f32; 3], [f32; 3], u8, Instant);

struct Player {
    area: u8,
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
    /// motion.
    touched: HashMap<Key, ([f32; 3], i32, u16, Instant)>,
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
}

impl ChaoHost {
    pub fn new() -> Self {
        Self::default()
    }

    /// Sees player `from`'s game message (the referee passes everything on).
    pub fn observe(&mut self, from: u8, body: &Body, now: Instant) {
        match body {
            Body::PlayerState(state) => self.player(from, state, now),
            Body::Chao { index, data } if usize::from(*index) < MAX_CARRIED_CHAO => {
                let key = (from, *index);
                self.records.insert(key, data.clone());
                // The owner's record, with what the server changed and hasn't sent yet, and
                // the server's own emotions and needs (its life here; the owner's game takes
                // them from `ChaoLive`, not the other way).
                let pending = self.sent.get(&key).cloned();
                if let Some(chao) = self.chao_mut(key) {
                    let mut rec = (**data).clone();
                    if let Some(sent) = pending {
                        changes::apply(&mut rec, &changes::diff(&sent, &chao.rec.0));
                    }
                    let (at, len) = CHAO_MOOD;
                    rec.bytes[at..at + len].copy_from_slice(&chao.rec.0.bytes[at..at + len]);
                    chao.rec = Record(rec);
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
                // How the touch left it (hit: angry, scared; petted: glad): it goes on so.
                let (at, len) = CHAO_MOOD;
                if mood.len() == len
                    && let Some(chao) = self.chao_mut(key)
                {
                    chao.rec.0.bytes[at..at + len].copy_from_slice(mood);
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
                    changes::apply(&mut chao.rec.0, list);
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
        self.players.insert(
            from,
            Player {
                area,
                position: state.position,
                rotation_y: state.rotation[1],
                character: state.character,
                action: state.action,
                at: now,
            },
        );
    }

    /// Player `id` left: their Chao go.
    pub fn left(&mut self, id: u8) {
        self.players.remove(&id);
        self.records.retain(|&(o, _), _| o != id);
        self.sent.retain(|&(o, _), _| o != id);
        self.quiet.retain(|&(o, _), _| o != id);
        self.own.retain(|&(o, _), _| o != id);
        self.held.retain(|_, (holder, _, _)| *holder != id);
        self.touched.retain(|&(o, _), _| o != id);
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
        self.items = shared.items;
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
        for index in 0..3 {
            self.membership(index, now, out);
            let Some(run) = self.runs[index].as_mut() else {
                continue;
            };
            for (key, &slot) in &run.slots {
                if let Some(chao) = run.world.chao.get_mut(slot).and_then(Option::as_mut) {
                    hold_life(&mut chao.rec);
                    if let Some(sent) = self.sent.get_mut(key) {
                        let mut rec = Record((**sent).clone());
                        hold_life(&mut rec);
                        **sent = rec.0;
                    }
                    // A player holds it (their `ChaoHeld`); letting go clears it.
                    chao.held = self
                        .held
                        .get(key)
                        .is_some_and(|(_, _, at)| now.duration_since(*at) < HELD_STALE);
                    let petted = self.players.values().any(|p| {
                        p.action == PET
                            && now.duration_since(p.at) < PLAYER_STALE
                            && distance(p.position, chao.body.pos) < PET_RANGE
                    });
                    if chao.held || petted {
                        self.quiet.insert(*key, now + TOUCH_QUIET);
                    }
                }
            }
            let around = Players {
                here: self
                    .players
                    .values()
                    .filter(|p| {
                        p.area == index as u8 + 1 && now.duration_since(p.at) < PLAYER_STALE
                    })
                    .map(|p| Seen {
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
            };
            run.world.frame(&around);
            let things = around.things;
            // The life tick happens in the owner's game too: not theirs to be sent.
            if let Some((slot, ticked)) = run.world.ticked.take()
                && let Some(key) = run.slots.iter().find(|(_, s)| **s == slot).map(|(k, _)| k)
                && let Some(sent) = self.sent.get_mut(key)
            {
                changes::apply(sent, &ticked);
            }
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
                // Evolving and dying are the owner's game's (`hold_life`).
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
                            self.acts.push(ItemAct::Released { id });
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
                            let senses = ChaoSenses::at(&things, slot, chao.body.pos);
                            if let Some(t) = touch_category(touch).and_then(|c| {
                                things.iter().find(|t| {
                                    t.category == c && t.chao.is_none() && senses.touches(t)
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
                                self.acts.push(ItemAct::Released { id });
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
                } else if let Some(&(pos, rot, _, at)) = self.touched.get(&key)
                    && now.duration_since(at) < HELD_STALE
                {
                    // A player's game runs it for a touch (thrown, petted, fed): it's where
                    // theirs is, and goes on from there when they're done.
                    chao.body.pos = pos;
                    chao.body.rot_y = rot as u16;
                    chao.body.vel = [0.0; 3];
                    self.quiet.insert(key, now + TOUCH_QUIET);
                } else {
                    // The players' bodies push it aside, as their collision does in their
                    // games: otherwise their copies walk into them and shove them.
                    for p in self.players.values().filter(|p| {
                        usize::from(p.area) == index + 1 && now.duration_since(p.at) < PLAYER_STALE
                    }) {
                        push_out(&mut chao.body.pos, p.position);
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
            if self.frames.is_multiple_of(STATUS_EVERY) {
                // Where players' games hold items in their own Chao's hands (`IN_CHAO_HANDS`).
                for (_, owner, item) in self
                    .items
                    .iter()
                    .filter(|(a, o, _)| usize::from(*a) == index + 1 && *o != SERVER)
                {
                    if let ItemPlace::Chao { .. } = item.place {
                        let [x, y, z] = item.position;
                        crate::log!(
                            "chao host: player {owner}'s Chao holds item {}:{} at ({x:.2}, {y:.2}, {z:.2})",
                            item.id.origin,
                            item.id.serial
                        );
                    }
                }
                for (key, &slot) in &run.slots {
                    if let Some(c) = run.world.chao.get(slot).and_then(Option::as_ref) {
                        let [x, y, z] = c.body.pos;
                        crate::log!(
                            "chao host: garden {} chao {key:?}: {:#x} at ({x:.1}, {y:.1}, {z:.1}), motion {}",
                            index + 1,
                            c.current().map_or(0, |b| b.address()),
                            c.motion
                        );
                    }
                }
            }
            let keys: Vec<Key> = run.slots.keys().copied().collect();
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
                .flat_map(|&(at, len)| chao.rec.0.bytes[at..at + len].iter().copied())
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
        let Some(rec) = self.chao_mut(key).map(|c| c.rec.0.clone()) else {
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
        let here: Vec<u8> = self
            .players
            .iter()
            .filter(|(_, p)| p.area == area && now.duration_since(p.at) < PLAYER_STALE)
            .map(|(&id, _)| id)
            .collect();
        let land = self.land(index).map(|(f, g)| (f, g.clone()));
        let enough = here.len() >= MIN_PLAYERS;
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
            if let Some(run) = self.runs[index].as_ref() {
                for key in run.slots.keys().copied().collect::<Vec<_>>() {
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
            crate::log!("chao host: running garden {area} for {here:?}");
            self.runs[index] = Some(Run {
                world: World::new(garden, index, 0x5A2C_0000 | area as u32),
                slots: BTreeMap::new(),
                fingerprint,
                short: None,
            });
        }
        let table = self.table();
        // Whose Chao are out: a player heard lately (a hitch or a tab-out doesn't take theirs
        // out of the garden, and its brain with it).
        let present: Vec<u8> = self
            .players
            .iter()
            .filter(|(_, p)| p.area == area && now.duration_since(p.at) < VISIT_STALE)
            .map(|(&id, _)| id)
            .collect();
        let Some(run) = self.runs[index].as_ref() else {
            return;
        };
        let gone: Vec<Key> = run
            .slots
            .keys()
            .filter(|k| !self.wanted(k, &present, now))
            .copied()
            .collect();
        for &key in &gone {
            crate::log!(
                "chao host: player {}'s Chao {} leaves garden {area} (owner here {}, streamed {}, owner runs it {})",
                key.0,
                key.1,
                present.contains(&key.0),
                self.own
                    .get(&key)
                    .is_some_and(|(_, at)| now.duration_since(*at) < VISIT_STALE),
                self.own.get(&key).is_some_and(|(c, _)| c.owner_runs),
            );
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
            .filter(|(key, _)| Self::is_wanted(&self.own, &self.records, key, &present, now))
            .map(|(&key, (c, _))| (key, *c))
            .collect();
        for key in gone {
            if let Some(slot) = run.slots.remove(&key) {
                run.world.remove(slot);
            }
        }
        for (key, c) in wanted {
            if run.slots.contains_key(&key) {
                continue;
            }
            let Some(record) = self.records.get(&key) else {
                continue;
            };
            let chao = Chao::new(c.position, c.rotation_y as u16, Record((**record).clone()));
            if let Some(slot) = run.world.add(chao) {
                run.slots.insert(key, slot);
                self.sent.insert(key, record.clone());
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
            self.acts.push(ItemAct::Released { id });
        }
    }

    fn wanted(&self, key: &Key, here: &[u8], now: Instant) -> bool {
        Self::is_wanted(&self.own, &self.records, key, here, now)
    }

    /// Out now: an owner here streams it as visiting (their game has it in this garden, its
    /// home or not) and isn't running it themselves (evolving, life ending); its record came.
    fn is_wanted(
        own: &HashMap<Key, (GardenChao, Instant)>,
        records: &HashMap<Key, Box<ChaoData>>,
        key: &Key,
        here: &[u8],
        now: Instant,
    ) -> bool {
        here.contains(&key.0)
            && own
                .get(key)
                .is_some_and(|(c, at)| now.duration_since(*at) < VISIT_STALE && !c.owner_runs)
            && records.contains_key(key)
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
                    from: owner,
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
    /// Let go of and at rest (landed, put down): one that came from a player's save goes back
    /// to them while they're in the garden.
    Released { id: ItemId },
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
                if let Some(at) = find(items, id) {
                    let (a, o, item) = &mut items[at];
                    (*a, *o) = (area, SERVER);
                    item.place = ItemPlace::Chao { owner, index };
                    item.position = IN_CHAO_HANDS;
                    item.unsaved = true;
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
            Self::New { .. } | Self::ToyOn { .. } | Self::Tree { .. } | Self::Released { .. } => {}
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
                ItemPlace::Chao { owner: o, index } if owner == SERVER => {
                    let slot = *run.slots.get(&(o, index))?;
                    let c = run.world.chao.get(slot)?.as_ref()?;
                    let pos = in_hands(c.body.pos, i32::from(c.body.rot_y), item.position);
                    (pos, false, Some(slot))
                }
                ItemPlace::Chao { .. } => return None,
            };
            Some(Thing {
                id: item.id,
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
    pos: V3,
    rot_y: u16,
    character: u8,
    action: u8,
}

/// An item in a garden the server runs, as its Chao sense it.
#[derive(Clone, Copy, Debug)]
struct Thing {
    id: ItemId,
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
    /// The game knows one player; online, a Chao's is the one whistling to it within 100
    /// (`0x5637B0`), else the nearest. Seen: within sight and in front.
    fn senses(&self, slot: usize, pos: V3, rot_y: u16) -> Box<dyn Senses + '_> {
        let whistler = self
            .here
            .iter()
            .filter(|p| p.action == WHISTLE && distance(p.pos, pos) < 100.0)
            .min_by(|a, b| distance(a.pos, pos).total_cmp(&distance(b.pos, pos)));
        let focus = whistler.or_else(|| {
            self.here
                .iter()
                .min_by(|a, b| distance(a.pos, pos).total_cmp(&distance(b.pos, pos)))
        });
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
            slot,
            pos,
            rot_y: 0,
        }
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
            .filter(|t| t.category == 3 && t.chao != Some(self.slot))
            .filter(|t| in_view(self.pos, self.rot_y, t.pos))
            .min_by(|a, b| distance(a.pos, self.pos).total_cmp(&distance(b.pos, self.pos)))
            .map(|t| self.as_fruit(t))
    }
    fn fruit(&self, id: u32) -> Option<Fruit> {
        self.things
            .iter()
            .find(|t| t.category == 3 && fruit_id(t.id) == id)
            .map(|t| self.as_fruit(t))
    }
    fn touching_fruit(&self) -> Option<Fruit> {
        self.things
            .iter()
            .find(|t| t.category == 3 && t.chao != Some(self.slot) && self.touches(t))
            .map(|t| self.as_fruit(t))
    }
    /// Items no Chao holds.
    fn touch(&self, kind: u8) -> Option<V3> {
        let category = touch_category(kind)?;
        self.things
            .iter()
            .find(|t| t.category == category && t.chao.is_none() && self.touches(t))
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

/// Like the keeper's `hold_life`: a Chao the server runs never evolves or dies here; its
/// owner's game does that (`GardenChao::owner_runs`).
fn hold_life(rec: &mut Record) {
    if rec.i16(off::LIFE) < 1 {
        rec.set_u16(off::LIFE, 1);
    }
    if rec.f32(off::EVOLUTION) > 0.99 {
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
            acts.push(ItemAct::Released { id: *id });
            return false;
        }
        if frames.is_multiple_of(2) {
            acts.push(ItemAct::Drop { id: *id, at: *pos });
        }
        true
    });
}

/// An item in the air: id, garden, where, velocity (per frame), frames flown.
type Flying = (ItemId, u8, V3, V3, u32);

/// A thrown item falls this much faster each frame, and lands after this many frames at most.
const ITEM_GRAVITY: f32 = 0.055;
const MAX_FLIGHT: u32 = 120;

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
        // It lands (and, let go of, may go back to whose it was).
        assert_eq!(acts.last(), Some(&ItemAct::Released { id }));
        let Some(ItemAct::Drop { at, .. }) = acts.iter().rev().nth(1) else {
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
}
