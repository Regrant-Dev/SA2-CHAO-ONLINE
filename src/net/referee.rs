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

use super::protocol::{
    Body, GardenItem, ItemId, ItemPlace, MAX_GARDEN_ITEMS, TREE_RECORD, TREE_SLOTS,
};

type Trees = [[u8; TREE_RECORD]; TREE_SLOTS];

/// What the referee keeps of one shared garden.
#[derive(Clone, Copy, Default)]
struct Garden {
    trees: Option<Trees>,
    /// Each toy's last jump and when.
    toys: [Option<(u8, Instant)>; 3],
}

/// The gardens (Chao World areas 1..=3).
const GARDENS: usize = 3;
/// How often the gardens and the server's items go out.
const GARDEN_EVERY: Duration = Duration::from_secs(2);
const ITEMS_EVERY: Duration = Duration::from_secs(1);
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
    next_serial: u16,
    /// The garden each player is in (from their `PlayerState`), 0 for none.
    areas: HashMap<u8, u8>,
    /// What changed since `saved` was last asked for.
    dirty: bool,
    sent_gardens: Option<Instant>,
    sent_items: Option<Instant>,
}

/// Most hunt pieces remembered (a few per hunt); past it the oldest hunts are forgotten.
const MAX_PIECES: usize = 256;

impl Referee {
    pub fn new() -> Self {
        Self::default()
    }

    /// Looks at a game message from player `from`. Returns whether to pass it on to the
    /// others; answers (if any) go to `out`.
    pub fn observe(&mut self, from: u8, body: &Body, out: &mut Vec<Out>) -> bool {
        match body {
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
            | Body::HuntTaken { .. }
            | Body::ServerGarden { .. }
            | Body::ServerItems { .. } => false,
            Body::GardenItems { area, items } => {
                for item in items {
                    let entry = self.items.entry(item.id).or_insert(Ledger {
                        owner: from,
                        busy: false,
                        last: None,
                    });
                    // Reports from someone who no longer owns it (crossed a hand-off) don't count.
                    if entry.owner == from {
                        entry.busy = item.busy();
                        entry.last = Some((*area, *item));
                    }
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
            Body::GardenTrees { area, trees } => {
                if let Some(g) = self.garden(*area)
                    && g.trees != Some(*trees)
                {
                    g.trees = Some(*trees);
                    self.dirty = true;
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

    /// Player `id` left the lobby: their items went with them, but the server keeps those that
    /// were in no one's save.
    pub fn left(&mut self, id: u8) {
        self.keep_unsaved(id, None);
        self.items.retain(|_, l| l.owner != id);
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
            self.server_items.insert(
                new,
                (
                    a,
                    GardenItem {
                        id: new,
                        place: ItemPlace::Ground,
                        unsaved: true,
                        ..item
                    },
                ),
            );
            self.dirty = true;
        }
    }

    /// Call often (the host's poll): the gardens and the server's items to send now.
    pub fn tick(&mut self, now: Instant, out: &mut Vec<Out>) {
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
        if due(self.sent_items, ITEMS_EVERY) && !self.server_items.is_empty() {
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
            let [x, y, z] = i.position;
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
                        self.server_items.insert(item.id, (area, item));
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

    fn claim(r: &mut Referee, from: u8) -> Vec<Out> {
        let mut out = Vec::new();
        assert!(!r.observe(from, &Body::ItemClaim { id: MANGO }, &mut out));
        out
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
        r.observe(1, &Body::GardenTrees { area: 1, trees }, &mut out);
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
