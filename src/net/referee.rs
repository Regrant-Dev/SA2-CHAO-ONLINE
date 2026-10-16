//! The host's referee: what has to be decided in one place for everyone.
//!
//! - **Garden items** (shared gardens): every item has one owner, whose game has it for real
//!   and saves it; everyone else shows a copy. A player who picks up a copy (or whose Chao takes
//!   it) claims it here: the first claim wins, unless the owner is using it right now (holding
//!   it, or their Chao is). The answer goes to everyone as `ItemOwner`, so the old owner's game
//!   lets it go and the new owner's game keeps it.
//! - **Co-op kills**: each room's destroyed stage objects per stage, sent to a room player who
//!   enters that stage later (even after everyone else left it). Forgotten when the room closes.
//!
//! The dedicated server and a hosting player run the same referee (`session::Host`).

use std::collections::{BTreeSet, HashMap};

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

/// Where a player is, from their latest state.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Seen {
    level: i16,
    mission: u8,
    /// The room they're in: its creator's id.
    room: Option<u8>,
}

/// (room, level, mission).
type StageKey = (u8, i16, u8);

#[derive(Default)]
pub struct Referee {
    items: HashMap<ItemId, Ledger>,
    players: HashMap<u8, Seen>,
    kills: HashMap<StageKey, BTreeSet<u16>>,
}

impl Referee {
    pub fn new() -> Self {
        Self::default()
    }

    /// Looks at a game message from player `from`. Returns whether to pass it on to the
    /// others; answers (if any) go to `out`.
    pub fn observe(&mut self, from: u8, body: &Body, out: &mut Vec<Out>) -> bool {
        match body {
            Body::PlayerState(s) => {
                let room = (s.room.1 != 0 && s.room.0 < 8).then_some(s.room.0);
                let now = Seen {
                    level: s.level,
                    mission: s.mission,
                    room,
                };
                let before = self.players.insert(from, now);
                // Our room closed (we created it and left it): its kills go too.
                if before.is_some_and(|b| b.room == Some(from)) && room != Some(from) {
                    self.kills.retain(|&(r, _, _), _| r != from);
                }
                // Entered a stage: what the room has destroyed there so far.
                let entered = before.is_none_or(|b| {
                    (b.level, b.mission, b.room) != (now.level, now.mission, now.room)
                });
                if entered
                    && let Some(r) = room
                    && let Some(objects) = self.kills.get(&(r, now.level, now.mission))
                    && !objects.is_empty()
                {
                    out.push(Out {
                        to: To::One(from),
                        from: r,
                        body: Body::StageKills {
                            level: now.level,
                            mission: now.mission,
                            objects: objects.iter().copied().collect(),
                        },
                    });
                }
                true
            }
            Body::StageKills {
                level,
                mission,
                objects,
            } => {
                if let Some(r) = self.players.get(&from).and_then(|p| p.room) {
                    self.kills
                        .entry((r, *level, *mission))
                        .or_default()
                        .extend(objects.iter().copied());
                }
                true
            }
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

    /// Player `id` left the lobby: their items went with them, and their room closed.
    pub fn left(&mut self, id: u8) {
        self.items.retain(|_, l| l.owner != id);
        self.players.remove(&id);
        self.kills.retain(|&(r, _, _), _| r != id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::net::protocol::{GardenItem, ItemPlace, PlayerState};

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

    fn state(level: i16, room: (u8, u8)) -> Body {
        Body::PlayerState(PlayerState {
            time: 0,
            level,
            mission: 0,
            chao_area: -1,
            character: 0,
            position: [0.0; 3],
            rotation: [0; 3],
            anim: 0,
            frame: 0.0,
            flags: 0,
            room,
            upgrades: 0,
        })
    }

    #[test]
    fn a_late_room_player_gets_the_stage_kills() {
        let mut r = Referee::new();
        let mut out = Vec::new();
        // Player 0 created room 0 and destroyed things in level 13, then left the stage.
        r.observe(0, &state(13, (0, 2)), &mut out);
        let kills = Body::StageKills {
            level: 13,
            mission: 0,
            objects: vec![5, 9],
        };
        assert!(r.observe(0, &kills, &mut out));
        r.observe(0, &state(0, (0, 2)), &mut out);
        assert!(out.is_empty());
        // Player 4 of the same room enters level 13 afterwards.
        r.observe(4, &state(0, (0, 2)), &mut out);
        r.observe(4, &state(13, (0, 2)), &mut out);
        assert_eq!(
            out,
            vec![Out {
                to: To::One(4),
                from: 0,
                body: kills
            }]
        );
        // Someone outside the room gets nothing; the room closing forgets them.
        out.clear();
        r.observe(5, &state(13, (0xFF, 0)), &mut out);
        assert!(out.is_empty());
        r.observe(0, &state(0, (0xFF, 0)), &mut out);
        r.observe(6, &state(13, (0, 2)), &mut out);
        assert!(out.is_empty());
    }
}
