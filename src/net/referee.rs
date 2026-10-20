//! The host's referee: what has to be decided in one place for everyone.
//!
//! - **Garden items** (shared gardens): every item has one owner, whose game has it for real
//!   and saves it; everyone else shows a copy. A player who picks up a copy (or whose Chao takes
//!   it) claims it here: the first claim wins, unless the owner is using it right now (holding
//!   it, or their Chao is). The answer goes to everyone as `ItemOwner`, so the old owner's game
//!   lets it go and the new owner's game keeps it.
//! - **Treasure hunt pieces**: the first player to collect each piece of a room's hunt gets it
//!   (`HuntClaim` -> `HuntTaken` to everyone; a late claim hears who has it). A new hunt in
//!   that stage (`HuntSet`) starts over.
//!
//! Its answers (`ItemOwner`, `HuntTaken`) come only from here: the same bodies from a player
//! aren't passed on.
//!
//! Co-op kills aren't kept here: players in a stage send theirs to a room player who arrives,
//! and a stage everyone left starts fresh next time.
//!
//! - **Shared gardens** (re/chao-garden-items.md 7): each garden's trees (seeded by the first
//!   keeper's, then kept up to date by the keepers' `GardenTrees`), its toys' last jumps, and
//!   items in no one's save whose owner left the garden. It sends them (`ServerGarden`,
//!   `ServerItems`) to everyone every couple of seconds; a server item is claimed like anyone's.
//!   The dedicated server keeps the trees and items in a file (`saved` / `load`).
//!
//! The dedicated server and a hosting player run the same referee (`session::Host`).

use std::collections::HashMap;
use std::time::{Duration, Instant};

use super::chao_host::{IN_CHAO_HANDS, ItemAct, Shared};
use super::protocol::{
    Body, GardenItem, ItemId, ItemPlace, MAX_GARDEN_ITEMS, PlayerKey, TREE_RECORD, TREE_SLOTS,
};

type Trees = [[u8; TREE_RECORD]; TREE_SLOTS];

/// What the referee keeps of one shared garden.
#[derive(Clone, Copy, Default)]
struct Garden {
    trees: Option<Trees>,
    /// Each toy's last jump and when.
    toys: [Option<(u8, Instant)>; 3],
    /// Which trees want water (`GardenTrees::thirsty`), as their keeper last said.
    thirsty: u8,
}

/// The gardens (Chao World areas 1..=3).
const GARDENS: usize = 3;
/// How often the gardens and the server's items go out.
const GARDEN_EVERY: Duration = Duration::from_secs(2);
const ITEMS_EVERY: Duration = Duration::from_secs(1);
const ITEMS_HELD_EVERY: Duration = Duration::from_millis(400);
/// Server items lying in one garden at most (the game's save has room for only so many fruit).
const MAX_SERVER_ITEMS: usize = 20;
/// The server as an item's owner.
use super::protocol::SERVER;
/// Server items get ids of their own: serials from here up, which players' own counters (from
/// 0 each session) never get to.
const SERVER_SERIALS: u16 = 0xF000;

/// Who a referee message goes to.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum To {
    All,
    One(u8),
}

#[derive(Clone, Debug, PartialEq)]
pub struct Out {
    pub to: To,
    /// The `Game` message's sender (a player id; clients don't act on it for these).
    pub from: u8,
    pub body: Body,
}

#[derive(Clone, Copy, Debug)]
struct Ledger {
    owner: u8,
    /// The owner is holding it or their Chao is (from their last report).
    busy: bool,
    /// Its garden and its owner's last report of it (none yet: claimed, not reported).
    last: Option<(u8, GardenItem)>,
}

#[derive(Default)]
pub struct Referee {
    items: HashMap<ItemId, Ledger>,
    /// Treasure hunt pieces: (level, mission, set signature, slot) -> who got it first.
    pieces: HashMap<(i16, u8, u32, u8), u8>,
    gardens: [Garden; GARDENS],
    /// The server's own items: id -> (garden, item).
    server_items: HashMap<ItemId, (u8, GardenItem)>,
    /// Where a Chao the server runs took each item it holds.
    taken_at: HashMap<ItemId, [f32; 3]>,
    /// Players' items a Chao the server runs has, as the server says they are (their owner's
    /// reports don't move them until they say the same; `steer`).
    steered: HashMap<ItemId, GardenItem>,
    /// When each server item came (a count), and the count: the oldest lying ones go first.
    born: HashMap<ItemId, u64>,
    births: u64,
    next_serial: u16,
    /// The garden each player is in (from their `PlayerState`), 0 for none.
    areas: HashMap<u8, u8>,
    /// What changed since `saved` was last asked for.
    dirty: bool,
    sent_gardens: Option<Instant>,
    sent_items: Option<Instant>,
    /// The server running gardens' Chao itself (chao_host.rs); none in some tests.
    chao: Option<Box<super::chao_host::ChaoHost>>,
}

/// Most hunt pieces remembered (a few per hunt); past it the oldest hunts are forgotten.
const MAX_PIECES: usize = 256;

impl Referee {
    /// A referee that runs shared gardens' Chao (chao_host.rs).
    pub fn new() -> Self {
        Self {
            chao: Some(Box::new(super::chao_host::ChaoHost::new())),
            ..Self::default()
        }
    }

    /// Looks at a game message from player `from`. Returns whether to pass it on to the
    /// others; answers (if any) go to `out`.
    pub fn observe(&mut self, from: u8, body: &Body, out: &mut Vec<Out>) -> bool {
        if let Some(chao) = self.chao.as_mut() {
            chao.observe(from, body, Instant::now());
        }
        match body {
            // The server's own garden Chao: only it sends those; the ground and what players
            // bring are for it alone. A touch (`ChaoTouched`) goes on: it ends its sender's
            // hold of that Chao in every game at once.
            Body::ServerChao { .. }
            | Body::ChaoLive { .. }
            | Body::GardenLand { .. }
            | Body::Brought { .. }
            | Body::HeldChao { .. }
            | Body::SaveChao { .. }
            | Body::ChaoLocked { .. } => false,
            // A Chao locked against its sender (its owner's `ChaoLock`): never to its owner.
            Body::ChaoChanges { owner, index, .. }
                if self
                    .chao
                    .as_ref()
                    .is_some_and(|c| c.locked_against(from, (*owner, *index))) =>
            {
                false
            }
            Body::HuntClaim {
                level,
                mission,
                sig,
                slot,
            } => {
                let key = (*level, *mission, *sig, *slot);
                if self.pieces.len() >= MAX_PIECES && !self.pieces.contains_key(&key) {
                    self.pieces.clear();
                }
                let first = !self.pieces.contains_key(&key);
                let owner = *self.pieces.entry(key).or_insert(from);
                out.push(Out {
                    to: if first { To::All } else { To::One(from) },
                    from,
                    body: Body::HuntTaken {
                        level: *level,
                        mission: *mission,
                        sig: *sig,
                        slot: *slot,
                        player: owner,
                    },
                });
                false
            }
            // A different hunt in this stage: who got what in the last one no longer counts.
            Body::HuntSet {
                level,
                mission,
                pieces,
            } => {
                let sig = super::protocol::hunt_signature(pieces);
                self.pieces
                    .retain(|&(l, m, s, _), _| (l, m) != (*level, *mission) || s == sig);
                true
            }
            // Only the referee decides (or sends) these.
            Body::ItemOwner { .. }
            | Body::ItemSteer { .. }
            | Body::HuntTaken { .. }
            | Body::ServerGarden { .. }
            | Body::ServerItems { .. } => false,
            Body::GardenItems { area, items } => {
                // A late report of one the server's Chao took from them doesn't count.
                for item in items
                    .iter()
                    .filter(|i| !self.server_items.contains_key(&i.id))
                {
                    let entry = self.items.entry(item.id).or_insert(Ledger {
                        owner: from,
                        busy: false,
                        last: None,
                    });
                    // Reports from someone who no longer owns it (crossed a hand-off) don't count.
                    if entry.owner != from {
                        continue;
                    }
                    // One a server Chao has: as the server says, until its owner's game shows it
                    // the same (on the ground again, it's theirs to report).
                    if let Some(&steered) = self.steered.get(&item.id) {
                        if item.place != steered.place || steered.place != ItemPlace::Ground {
                            continue;
                        }
                        self.steered.remove(&item.id);
                    }
                    entry.busy = item.busy();
                    entry.last = Some((*area, *item));
                }
                true
            }
            Body::PlayerState(state) => {
                let area = if state.level == crate::game::LEVEL_CHAO_WORLD
                    && (1..=GARDENS as i8).contains(&state.chao_area)
                {
                    state.chao_area as u8
                } else {
                    0
                };
                let was = self.areas.insert(from, area).unwrap_or(0);
                if was != 0 && was != area {
                    self.keep_unsaved(from, Some(was));
                }
                true
            }
            Body::GardenTrees {
                area,
                trees,
                thirsty,
            } => {
                if let Some(g) = self.garden(*area) {
                    g.thirsty = *thirsty;
                    if g.trees != Some(*trees) {
                        g.trees = Some(*trees);
                        self.dirty = true;
                    }
                }
                true
            }
            Body::ToyMode { area, toy, mode } => {
                if let Some(slot) = self
                    .garden(*area)
                    .and_then(|g| g.toys.get_mut(usize::from(*toy)))
                {
                    *slot = Some((*mode, Instant::now()));
                }
                true
            }
            // One a Chao the server runs has in its hands stays the server's.
            Body::ItemClaim { id } if self.server_items.get(id).is_some_and(|(_, i)| i.busy()) => {
                out.push(Out {
                    to: To::One(from),
                    from: SERVER,
                    body: Body::ItemOwner {
                        id: *id,
                        owner: SERVER,
                    },
                });
                false
            }
            Body::ItemClaim { id } if self.server_items.contains_key(id) => {
                // The server's: the first claim gets it, and it's theirs (saved) from now on.
                let id = *id;
                self.server_items.remove(&id);
                self.dirty = true;
                self.items.insert(
                    id,
                    Ledger {
                        owner: from,
                        busy: true,
                        last: None,
                    },
                );
                out.push(Out {
                    to: To::All,
                    from,
                    body: Body::ItemOwner { id, owner: from },
                });
                false
            }
            Body::ItemClaim { id } => {
                let id = *id;
                match self.items.get_mut(&id) {
                    Some(l) if l.owner == from => {}
                    Some(l) if !l.busy => {
                        l.owner = from;
                        // The new owner has it in hand until they report otherwise.
                        l.busy = true;
                        out.push(Out {
                            to: To::All,
                            from,
                            body: Body::ItemOwner { id, owner: from },
                        });
                    }
                    // Taken, or in use by its owner: tell the claimer who has it.
                    Some(l) => out.push(Out {
                        to: To::One(from),
                        from: l.owner,
                        body: Body::ItemOwner { id, owner: l.owner },
                    }),
                    // Never reported: its first owner keeps it.
                    None => out.push(Out {
                        to: To::One(from),
                        from: id.origin,
                        body: Body::ItemOwner {
                            id,
                            owner: id.origin,
                        },
                    }),
                }
                false
            }
            _ => true,
        }
    }

    /// What a Chao the server runs did to an item (chao_host.rs).
    fn item_act(&mut self, act: ItemAct, out: &mut Vec<Out>) {
        self.dirty = true;
        match act {
            ItemAct::Take {
                id,
                area,
                chao: (owner, index),
                at,
            } => {
                self.taken_at.insert(id, at);
                let held = |item: GardenItem| GardenItem {
                    place: ItemPlace::Chao { owner, index },
                    position: IN_CHAO_HANDS,
                    ..item
                };
                // A player's stays in their save: the Chao borrows it, their game shows it so.
                if self.steer(id, out, |item| (held(item), false)) {
                    return;
                }
                let Some((_, item)) = self.server_items.get(&id).copied() else {
                    return;
                };
                self.add_server_item(area, held(item));
            }
            ItemAct::Bite { id, left } => {
                let bitten = |item: GardenItem| {
                    (
                        GardenItem {
                            bites: left,
                            ..item
                        },
                        left == 0,
                    )
                };
                if self.steer(id, out, bitten) {
                    return;
                }
                if left == 0 {
                    self.server_items.remove(&id);
                } else if let Some((_, item)) = self.server_items.get_mut(&id) {
                    item.bites = left;
                }
            }
            ItemAct::Drop { id, at } => {
                self.taken_at.remove(&id);
                let dropped = |item: GardenItem| {
                    let item = GardenItem {
                        place: ItemPlace::Ground,
                        position: at,
                        ..item
                    };
                    (item, false)
                };
                if self.steer(id, out, dropped) {
                    return;
                }
                if let Some((_, item)) = self.server_items.get_mut(&id) {
                    item.place = ItemPlace::Ground;
                    item.position = at;
                }
            }
            ItemAct::Gone { id } => {
                if self.steer(id, out, |item| (item, true)) {
                    return;
                }
                self.server_items.remove(&id);
            }
            ItemAct::ToyOn { area, toy } => {
                let Some(slot) = self
                    .garden(area)
                    .and_then(|g| g.toys.get_mut(usize::from(toy)))
                else {
                    return;
                };
                if slot.is_some_and(|(mode, _)| mode != 0) {
                    return;
                }
                // Its keeper's toy jumps for real and tells everyone (garden_toys.rs).
                *slot = Some((1, Instant::now()));
                out.push(Out {
                    to: To::All,
                    from: SERVER,
                    body: Body::ToyMode { area, toy, mode: 1 },
                });
            }
            ItemAct::Tree {
                area,
                slot,
                action,
                kind,
            } => {
                // As a player's action on their copy of that tree: the trees' keeper does it.
                out.push(Out {
                    to: To::All,
                    from: SERVER,
                    body: Body::TreeAction {
                        area,
                        slot,
                        action,
                        kind,
                    },
                });
            }
            ItemAct::New {
                area,
                category,
                kind,
                at,
            } => {
                let id = ItemId {
                    origin: 0,
                    serial: SERVER_SERIALS | (self.next_serial & 0x0FFF),
                };
                self.next_serial = self.next_serial.wrapping_add(1);
                self.add_server_item(
                    area,
                    GardenItem {
                        id,
                        category,
                        kind,
                        bites: 0,
                        place: ItemPlace::Ground,
                        position: at,
                        unsaved: true,
                    },
                );
            }
        }
    }

    /// A player's item a server Chao did something to (`change` gives it as it is now, and
    /// whether it's gone): the referee's view of it changes, and its owner's game is told
    /// (`ItemSteer`) so their own item, in their save, does the same. False when it's no
    /// player's item.
    fn steer(
        &mut self,
        id: ItemId,
        out: &mut Vec<Out>,
        change: impl FnOnce(GardenItem) -> (GardenItem, bool),
    ) -> bool {
        let Some(ledger) = self.items.get_mut(&id) else {
            return false;
        };
        let Some((area, item)) = ledger.last else {
            return false;
        };
        let item = self.steered.get(&id).copied().unwrap_or(item);
        let (item, gone) = change(item);
        let owner = ledger.owner;
        if gone {
            self.items.remove(&id);
            self.steered.remove(&id);
        } else {
            ledger.last = Some((area, item));
            ledger.busy = item.busy();
            self.steered.insert(id, item);
        }
        out.push(Out {
            to: To::One(owner),
            from: SERVER,
            body: Body::ItemSteer { area, item, gone },
        });
        true
    }

    /// Adds (or replaces) one of the server's items. A garden keeps at most
    /// `MAX_SERVER_ITEMS` of them lying around (fruit from trees, dropped by Chao, left by
    /// players): past that the oldest lying one goes. Ones in a Chao's hands stay.
    fn add_server_item(&mut self, area: u8, item: GardenItem) {
        self.births += 1;
        self.born.insert(item.id, self.births);
        self.server_items.insert(item.id, (area, item));
        let items = &self.server_items;
        self.born.retain(|id, _| items.contains_key(id));
        loop {
            let lying = self
                .server_items
                .iter()
                .filter(|(_, (a, i))| *a == area && !i.busy());
            if lying.clone().count() <= MAX_SERVER_ITEMS {
                break;
            }
            let Some(oldest) = lying
                .min_by_key(|(id, _)| self.born.get(*id).copied().unwrap_or(0))
                .map(|(&id, _)| id)
            else {
                break;
            };
            self.server_items.remove(&oldest);
            self.born.remove(&oldest);
            self.dirty = true;
        }
    }

    /// Player `id` joined with key `key` (their Chao are known by it) and name `name`.
    pub fn joined(&mut self, id: u8, key: PlayerKey, name: &str) {
        if let Some(chao) = self.chao.as_mut() {
            chao.joined(id, key, name);
        }
    }

    /// The Chao held for players that changed (`session::Host::chao_files`).
    pub fn chao_files(&mut self) -> Vec<(String, Option<String>)> {
        self.chao.as_mut().map_or_else(Vec::new, |c| c.files())
    }

    pub fn load_chao_file(&mut self, name: &str, text: &str) {
        if let Some(chao) = self.chao.as_mut() {
            chao.load_file(name, text);
        }
    }

    /// Player `id` left the lobby: their items went with them, but the server keeps those that
    /// were in no one's save.
    pub fn left(&mut self, id: u8) {
        if let Some(chao) = self.chao.as_mut() {
            chao.left(id);
        }
        self.keep_unsaved(id, None);
        self.items.retain(|_, l| l.owner != id);
        // Theirs a server Chao had go with them (their save has them).
        let items = &self.items;
        self.steered.retain(|item, _| items.contains_key(item));
        self.areas.remove(&id);
    }

    fn garden(&mut self, area: u8) -> Option<&mut Garden> {
        usize::from(area)
            .checked_sub(1)
            .and_then(|i| self.gardens.get_mut(i))
    }

    /// Player `owner`'s unsaved items lying in a garden (`area`, or any) become the server's,
    /// under new ids. Ones they hold or their Chao has go with them.
    fn keep_unsaved(&mut self, owner: u8, area: Option<u8>) {
        let kept: Vec<(ItemId, u8, GardenItem)> = self
            .items
            .iter()
            .filter(|(_, l)| l.owner == owner && !l.busy)
            .filter_map(|(&id, l)| {
                let (a, item) = l.last?;
                (item.unsaved && area.is_none_or(|x| x == a)).then_some((id, a, item))
            })
            .collect();
        for (id, a, item) in kept {
            self.items.remove(&id);
            let new = ItemId {
                origin: id.origin,
                serial: SERVER_SERIALS | (self.next_serial & 0x0FFF),
            };
            self.next_serial = self.next_serial.wrapping_add(1);
            self.add_server_item(
                a,
                GardenItem {
                    id: new,
                    place: ItemPlace::Ground,
                    unsaved: true,
                    ..item
                },
            );
            self.dirty = true;
        }
    }

    /// Call often (the host's poll): the gardens, the server's items and its Chao to send now.
    pub fn tick(&mut self, now: Instant, out: &mut Vec<Out>) {
        if let Some(chao) = self.chao.as_mut() {
            let items = self
                .items
                .values()
                .filter_map(|l| l.last.map(|(area, item)| (area, l.owner, item)))
                .chain(
                    self.server_items
                        .values()
                        .map(|&(area, item)| (area, SERVER, item)),
                )
                .collect();
            let toys = self
                .gardens
                .map(|g| g.toys.map(|t| t.map_or(0, |(mode, _)| mode)));
            // Tree records: byte 1 is the state.
            let trees = self
                .gardens
                .map(|g| g.trees.map_or([0; TREE_SLOTS], |t| t.map(|r| r[1])));
            let thirsty = self.gardens.map(|g| g.thirsty);
            chao.tick(
                now,
                Shared {
                    items,
                    toys,
                    trees,
                    thirsty,
                },
                out,
            );
            let acts = chao.take_item_acts();
            if !acts.is_empty() {
                for act in acts {
                    self.item_act(act, out);
                }
                // The others see it now, not at the next round.
                self.sent_items = None;
            }
        }
        let due = |at: Option<Instant>, every| at.is_none_or(|t| now.duration_since(t) >= every);
        if due(self.sent_gardens, GARDEN_EVERY) {
            self.sent_gardens = Some(now);
            for (i, g) in self.gardens.iter().enumerate() {
                let Some(trees) = g.trees else {
                    continue;
                };
                let toys = g.toys.map(|t| {
                    t.map_or((0, u16::MAX), |(mode, at)| {
                        let age = now.duration_since(at).as_secs();
                        (mode, u16::try_from(age).unwrap_or(u16::MAX))
                    })
                });
                out.push(Out {
                    to: To::All,
                    from: 0,
                    body: Body::ServerGarden {
                        area: i as u8 + 1,
                        trees,
                        toys,
                    },
                });
            }
        }
        // Items in Chao's hands go more often: a copy not heard of for 1.5 s goes.
        let every = if self.server_items.values().any(|(_, i)| i.busy()) {
            ITEMS_HELD_EVERY
        } else {
            ITEMS_EVERY
        };
        if due(self.sent_items, every) && !self.server_items.is_empty() {
            self.sent_items = Some(now);
            for area in 1..=GARDENS as u8 {
                let mut items: Vec<GardenItem> = self
                    .server_items
                    .values()
                    .filter(|(a, _)| *a == area)
                    .map(|(_, item)| *item)
                    .collect();
                items.sort_by_key(|i| (i.id.origin, i.id.serial));
                for chunk in items.chunks(MAX_GARDEN_ITEMS) {
                    out.push(Out {
                        to: To::All,
                        from: 0,
                        body: Body::ServerItems {
                            area,
                            items: chunk.to_vec(),
                        },
                    });
                }
            }
        }
    }

    /// Whether the trees or the server's items changed since the last `saved`.
    pub fn changed(&self) -> bool {
        self.dirty
    }

    /// The trees and the server's items, as text for a file: `tree <area> <56 hex digits>`,
    /// `item <area> <origin> <serial> <category> <kind> <bites> <x> <y> <z>`, `serial <n>`.
    pub fn saved(&mut self) -> String {
        self.dirty = false;
        let mut text = String::new();
        for (i, g) in self.gardens.iter().enumerate() {
            if let Some(trees) = g.trees {
                let hex: String = trees.iter().flatten().map(|b| format!("{b:02x}")).collect();
                text.push_str(&format!("tree {} {hex}\n", i + 1));
            }
        }
        let mut items: Vec<&(u8, GardenItem)> = self.server_items.values().collect();
        items.sort_by_key(|(_, i)| (i.id.origin, i.id.serial));
        for (area, i) in items {
            // One in a Chao's hands is saved where it was taken (its position is an offset).
            let [x, y, z] = if i.busy() {
                self.taken_at.get(&i.id).copied().unwrap_or(i.position)
            } else {
                i.position
            };
            text.push_str(&format!(
                "item {area} {} {} {} {} {} {x} {y} {z}\n",
                i.id.origin, i.id.serial, i.category, i.kind, i.bites
            ));
        }
        text.push_str(&format!("serial {}\n", self.next_serial));
        text
    }

    /// Takes back what `saved` wrote (lines it can't read are skipped).
    pub fn load(&mut self, text: &str) {
        for line in text.lines() {
            let words: Vec<&str> = line.split_whitespace().collect();
            match words.as_slice() {
                ["tree", area, hex] if hex.len() == TREE_SLOTS * TREE_RECORD * 2 => {
                    let mut trees = [[0u8; TREE_RECORD]; TREE_SLOTS];
                    let ok = trees.iter_mut().flatten().enumerate().all(|(i, b)| {
                        u8::from_str_radix(&hex[i * 2..i * 2 + 2], 16).is_ok_and(|v| {
                            *b = v;
                            true
                        })
                    });
                    if let (true, Ok(area)) = (ok, area.parse::<u8>())
                        && let Some(g) = self.garden(area)
                    {
                        g.trees = Some(trees);
                    }
                }
                ["item", area, origin, serial, category, kind, bites, x, y, z] => {
                    let item = (|| {
                        let area: u8 = area.parse().ok().filter(|a| (1..=3).contains(a))?;
                        let id = ItemId {
                            origin: origin.parse().ok().filter(|&o| o < 8)?,
                            serial: serial.parse().ok()?,
                        };
                        let category: u8 = category
                            .parse()
                            .ok()
                            .filter(|c| matches!(c, 2 | 3 | 7 | 9))?;
                        let position = [x.parse().ok()?, y.parse().ok()?, z.parse().ok()?];
                        Some((
                            area,
                            GardenItem {
                                id,
                                category,
                                kind: kind.parse().ok()?,
                                bites: bites.parse::<u8>().ok()?.min(4),
                                place: ItemPlace::Ground,
                                position,
                                unsaved: true,
                            },
                        ))
                    })();
                    if let Some((area, item)) = item {
                        self.add_server_item(area, item);
                    }
                }
                ["serial", n] => self.next_serial = n.parse().unwrap_or(0),
                _ => {}
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::net::protocol::{GardenItem, ItemPlace};

    const MANGO: ItemId = ItemId {
        origin: 1,
        serial: 12,
    };

    fn report(place: ItemPlace) -> Body {
        Body::GardenItems {
            area: 1,
            items: vec![GardenItem {
                id: MANGO,
                category: 3,
                kind: 0,
                bites: 4,
                place,
                position: [0.0; 3],
                unsaved: false,
            }],
        }
    }

    #[test]
    fn changes_to_a_locked_chao_go_only_from_its_owner() {
        use crate::net::protocol::{ChaoLock, PlayerState};
        let changes = Body::ChaoChanges {
            owner: 0,
            index: 1,
            changes: vec![(0x20, 3.0)],
        };
        let passes = |lock: ChaoLock, from: u8| {
            let mut r = Referee::new();
            r.joined(0, [7; 16], "Player1");
            r.joined(3, [9; 16], "Player2");
            let mut out = Vec::new();
            let brought = Body::Brought {
                awake: true,
                lock,
                list: Vec::new(),
            };
            assert!(!r.observe(0, &brought, &mut out));
            let here = Body::PlayerState(PlayerState {
                time: 0,
                level: crate::game::LEVEL_CHAO_WORLD,
                mission: 0,
                chao_area: 1,
                character: 0,
                position: [0.0; 3],
                rotation: [0; 3],
                anim: 0,
                frame: 0.0,
                flags: 0,
                room: (0xFF, 0),
                upgrades: 0,
                action: 0,
                status: 0,
            });
            for id in [0, 3] {
                r.observe(id, &here, &mut out);
            }
            r.observe(from, &changes, &mut out)
        };
        assert!(passes(ChaoLock::Always, 3));
        assert!(passes(ChaoLock::ThatGarden, 3));
        assert!(!passes(ChaoLock::Never, 3));
        assert!(passes(ChaoLock::Never, 0));
    }

    fn claim(r: &mut Referee, from: u8) -> Vec<Out> {
        let mut out = Vec::new();
        assert!(!r.observe(from, &Body::ItemClaim { id: MANGO }, &mut out));
        out
    }

    #[test]
    fn a_server_chao_only_borrows_a_players_item() {
        let mut r = Referee::new();
        let mut out = Vec::new();
        r.observe(1, &report(ItemPlace::Ground), &mut out);
        out.clear();
        let take = ItemAct::Take {
            id: MANGO,
            area: 1,
            chao: (2, 0),
            at: [0.0; 3],
        };
        r.item_act(take, &mut out);
        // Still player 1's (in their save); their game is told it's in that Chao's hands.
        assert_eq!(r.items[&MANGO].owner, 1);
        assert!(!r.server_items.contains_key(&MANGO));
        let Body::ItemSteer { item, gone, .. } = &out[0].body else {
            panic!("{out:?}");
        };
        assert_eq!(out[0].to, To::One(1));
        assert_eq!(item.place, ItemPlace::Chao { owner: 2, index: 0 });
        assert!(!gone);
        // Their game's late report (on the ground still) doesn't move it; nobody can claim it.
        r.observe(1, &report(ItemPlace::Ground), &mut out);
        assert_eq!(
            r.items[&MANGO].last.unwrap().1.place,
            ItemPlace::Chao { owner: 2, index: 0 }
        );
        let out = claim(&mut r, 3);
        assert_eq!(
            out[0].body,
            Body::ItemOwner {
                id: MANGO,
                owner: 1
            }
        );
        // Put down: theirs to report again once their game shows it there.
        let mut out = Vec::new();
        r.item_act(
            ItemAct::Drop {
                id: MANGO,
                at: [5.0, 0.0, 5.0],
            },
            &mut out,
        );
        r.observe(1, &report(ItemPlace::Ground), &mut out);
        assert!(r.steered.is_empty());
        // They leave: it goes with them.
        r.item_act(take, &mut out);
        r.left(1);
        assert!(r.steered.is_empty() && !r.items.contains_key(&MANGO));
    }

    #[test]
    fn a_garden_keeps_only_so_many_lying_server_items() {
        let mut r = Referee::new();
        let mut out = Vec::new();
        r.item_act(
            ItemAct::Take {
                id: MANGO,
                area: 1,
                chao: (2, 0),
                at: [0.0; 3],
            },
            &mut out,
        );
        r.observe(1, &report(ItemPlace::Ground), &mut out);
        for _ in 0..MAX_SERVER_ITEMS + 5 {
            r.item_act(
                ItemAct::New {
                    area: 1,
                    category: 9,
                    kind: 1,
                    at: [0.0; 3],
                },
                &mut out,
            );
        }
        let lying: Vec<ItemId> = r
            .server_items
            .iter()
            .filter(|(_, (a, i))| *a == 1 && !i.busy())
            .map(|(id, _)| *id)
            .collect();
        assert_eq!(lying.len(), MAX_SERVER_ITEMS);
        // The oldest went: the first five.
        let first = SERVER_SERIALS;
        assert!(lying.iter().all(|id| id.serial >= first + 5));
        assert_eq!(r.born.len(), r.server_items.len());
    }

    #[test]
    fn a_borrowed_fruit_eaten_up_is_gone_from_its_owner() {
        let mut r = Referee::new();
        let mut out = Vec::new();
        r.observe(1, &report(ItemPlace::Ground), &mut out);
        let take = ItemAct::Take {
            id: MANGO,
            area: 1,
            chao: (2, 0),
            at: [0.0; 3],
        };
        r.item_act(take, &mut out);
        out.clear();
        r.item_act(ItemAct::Bite { id: MANGO, left: 2 }, &mut out);
        let Body::ItemSteer { item, gone, .. } = &out[0].body else {
            panic!("{out:?}");
        };
        assert_eq!((item.bites, *gone), (2, false));
        out.clear();
        r.item_act(ItemAct::Bite { id: MANGO, left: 0 }, &mut out);
        assert!(matches!(out[0].body, Body::ItemSteer { gone: true, .. }));
        assert!(!r.items.contains_key(&MANGO));
    }

    #[test]
    fn first_claim_wins_and_everyone_hears() {
        let mut r = Referee::new();
        let mut out = Vec::new();
        assert!(r.observe(1, &report(ItemPlace::Ground), &mut out));
        assert_eq!(
            claim(&mut r, 2),
            vec![Out {
                to: To::All,
                from: 2,
                body: Body::ItemOwner {
                    id: MANGO,
                    owner: 2
                }
            }]
        );
        // Player 3 was a moment late.
        assert_eq!(
            claim(&mut r, 3),
            vec![Out {
                to: To::One(3),
                from: 2,
                body: Body::ItemOwner {
                    id: MANGO,
                    owner: 2
                }
            }]
        );
        // Player 1's stale report from before the hand-off changes nothing.
        r.observe(1, &report(ItemPlace::Ground), &mut out);
        assert_eq!(claim(&mut r, 3)[0].to, To::One(3));
    }

    #[test]
    fn an_item_its_owner_uses_cant_be_taken() {
        let mut r = Referee::new();
        let mut out = Vec::new();
        r.observe(1, &report(ItemPlace::Chao { owner: 1, index: 0 }), &mut out);
        assert_eq!(claim(&mut r, 2)[0].to, To::One(2));
        // Put down: now it can.
        r.observe(1, &report(ItemPlace::Ground), &mut out);
        assert_eq!(claim(&mut r, 2)[0].to, To::All);
    }

    #[test]
    fn items_and_rooms_go_with_a_player_who_leaves() {
        let mut r = Referee::new();
        let mut out = Vec::new();
        r.observe(1, &report(ItemPlace::Ground), &mut out);
        r.left(1);
        // Unknown now: its first owner (gone) keeps it; nobody else gets it.
        assert_eq!(
            claim(&mut r, 2)[0].body,
            Body::ItemOwner {
                id: MANGO,
                owner: 1
            }
        );
    }

    fn report_unsaved(id: ItemId) -> Body {
        Body::GardenItems {
            area: 2,
            items: vec![GardenItem {
                id,
                category: 3,
                kind: 0,
                bites: 4,
                place: ItemPlace::Ground,
                position: [1.0, 2.0, 3.0],
                unsaved: true,
            }],
        }
    }

    #[test]
    fn unsaved_items_stay_when_their_owner_leaves() {
        let mut r = Referee::new();
        let mut out = Vec::new();
        let saved = ItemId {
            origin: 1,
            serial: 5,
        };
        r.observe(1, &report_unsaved(MANGO), &mut out);
        let mut kept = report_unsaved(saved);
        if let Body::GardenItems { items, .. } = &mut kept {
            items[0].unsaved = false;
        }
        r.observe(1, &kept, &mut out);
        r.left(1);
        let mut out = Vec::new();
        r.tick(Instant::now(), &mut out);
        let Some(Body::ServerItems { area: 2, items }) = out.last().map(|o| o.body.clone()) else {
            panic!("no server items: {out:?}");
        };
        // The unsaved mango (reported last in garden 2) is the server's now, under a server id;
        // the saved one left with its owner.
        assert_eq!(items.len(), 1);
        assert!(items[0].id.serial >= SERVER_SERIALS && items[0].unsaved);
        // The first claim gets it, and only once.
        let id = items[0].id;
        let mut out = Vec::new();
        assert!(!r.observe(3, &Body::ItemClaim { id }, &mut out));
        assert_eq!(out[0].body, Body::ItemOwner { id, owner: 3 });
        let mut out = Vec::new();
        r.observe(2, &Body::ItemClaim { id }, &mut out);
        assert_eq!(out[0].to, To::One(2));
    }

    #[test]
    fn gardens_are_kept_and_saved() {
        let mut r = Referee::new();
        let mut out = Vec::new();
        let trees = [[2, 3, 100, 90, 150, 40, 0, 0x70]; TREE_SLOTS];
        let body = Body::GardenTrees {
            area: 1,
            trees,
            thirsty: 0,
        };
        r.observe(1, &body, &mut out);
        r.observe(1, &report_unsaved(MANGO), &mut out);
        r.left(1);
        assert!(r.changed());
        let text = r.saved();
        assert!(!r.changed());
        let mut again = Referee::new();
        again.load(&text);
        assert_eq!(again.saved(), text);
        let mut out = Vec::new();
        again.tick(Instant::now(), &mut out);
        assert!(out.iter().any(|o| matches!(
            o.body,
            Body::ServerGarden { area: 1, trees: t, .. } if t == trees
        )));
    }

    #[test]
    fn the_first_to_collect_a_piece_gets_it() {
        let mut r = Referee::new();
        let mut out = Vec::new();
        let claim = Body::HuntClaim {
            level: 16,
            mission: 0,
            sig: 7,
            slot: 2,
        };
        assert!(!r.observe(3, &claim, &mut out));
        assert!(!r.observe(1, &claim, &mut out));
        let taken = |to| Out {
            to,
            from: 0,
            body: Body::HuntTaken {
                level: 16,
                mission: 0,
                sig: 7,
                slot: 2,
                player: 3,
            },
        };
        assert_eq!(out[0].to, taken(To::All).to);
        assert_eq!(out[0].body, taken(To::All).body);
        assert_eq!(out[1].to, To::One(1));
        assert_eq!(out[1].body, taken(To::One(1)).body);
    }

    #[test]
    fn a_new_hunt_starts_over_and_players_cant_decide() {
        let mut r = Referee::new();
        let mut out = Vec::new();
        let claim = Body::HuntClaim {
            level: 16,
            mission: 0,
            sig: 7,
            slot: 2,
        };
        r.observe(3, &claim, &mut out);
        let set = Body::HuntSet {
            level: 16,
            mission: 0,
            pieces: [(0, 0, [0.0; 3]); 3],
        };
        assert!(r.observe(1, &set, &mut out));
        out.clear();
        r.observe(1, &claim, &mut out);
        assert_eq!(out[0].to, To::All);
        // A player can't hand out pieces or items themselves.
        let forged = Body::HuntTaken {
            level: 16,
            mission: 0,
            sig: 7,
            slot: 2,
            player: 5,
        };
        assert!(!r.observe(5, &forged, &mut out));
        let forged = Body::ItemOwner {
            id: MANGO,
            owner: 5,
        };
        assert!(!r.observe(5, &forged, &mut out));
    }
}
