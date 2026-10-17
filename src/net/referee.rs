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
//! The dedicated server and a hosting player run the same referee (`session::Host`).

use std::collections::HashMap;

use super::protocol::{Body, ItemId};

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
}

#[derive(Default)]
pub struct Referee {
    items: HashMap<ItemId, Ledger>,
    /// Treasure hunt pieces: (level, mission, set signature, slot) -> who got it first.
    pieces: HashMap<(i16, u8, u32, u8), u8>,
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
            // Only the referee decides these.
            Body::ItemOwner { .. } | Body::HuntTaken { .. } => false,
            Body::GardenItems { items, .. } => {
                for item in items {
                    let entry = self.items.entry(item.id).or_insert(Ledger {
                        owner: from,
                        busy: false,
                    });
                    // Reports from someone who no longer owns it (crossed a hand-off) don't count.
                    if entry.owner == from {
                        entry.busy = item.busy();
                    }
                }
                true
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

    /// Player `id` left the lobby: their items went with them.
    pub fn left(&mut self, id: u8) {
        self.items.retain(|_, l| l.owner != id);
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
        r.observe(1, &report(ItemPlace::Chao(0)), &mut out);
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
