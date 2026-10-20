//! What each player brings to the shared gardens, kept by their key (re/chao-garden-sync.md
//! "Asleep and away Chao"). The host shows these Chao in their gardens while their owner isn't
//! there: awake while the owner is online elsewhere, asleep while they're offline.
//!
//! The owner's save is always the real Chao: every login sends what they bring (`Brought`),
//! and what's kept here follows it (`sync`): records and gardens updated, Chao no longer
//! brought (moved home, given away, gone) dropped, new ones added. A Chao is its owner's key,
//! its `ChaoData::identity` and its place among the save's copies of that identity
//! (`ChaoId`); never its DNA alone.
//!
//! The dedicated server keeps them in `SA2CO_DATA/chao/` (`files`, `load`): one text file per
//! key, its name the key in hex. A key not seen for `EXPIRE` is dropped with its Chao.

use std::collections::{BTreeMap, BTreeSet};

use crate::chao::{ChaoData, TYPE_EGG, file};

use super::protocol::{BroughtChao, ChaoLock, MAX_CARRIED_CHAO, MAX_NAME, PlayerKey};

/// A player not seen this long (seconds) loses what's kept of them: a week.
pub const EXPIRE: u64 = 7 * 24 * 3600;
/// Each player's Chao shown in one garden at most: the ones they bring there (every visitor
/// row, `online::BRING_CHAO`).
pub const PER_PLAYER: usize = MAX_CARRIED_CHAO;
/// The last-seen time is written again after this long (seconds) while they're online.
const SEEN_EVERY: u64 = 60;

/// One Chao, wherever it is: its owner's key, identity and copy number.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ChaoId {
    pub key: PlayerKey,
    pub identity: u64,
    pub nth: u8,
}

/// A Chao kept for its owner.
#[derive(Clone, Debug, PartialEq)]
pub struct Stored {
    pub identity: u64,
    pub nth: u8,
    /// Its garden (1..=3) and visitor row there.
    pub garden: u8,
    pub row: u8,
    /// Its record as the owner's save has it (and what the host's run of it added since).
    pub record: Box<ChaoData>,
    /// Where it was last (position, facing); `None` until it has been in its garden.
    pub place: Option<([f32; 3], i32)>,
}

/// A player, as kept.
#[derive(Clone, Debug, PartialEq)]
pub struct Owner {
    pub name: String,
    /// When they were last online (Unix seconds).
    pub seen: u64,
    /// When others may change their Chao (kept for while they're offline).
    pub lock: ChaoLock,
    pub chao: Vec<Stored>,
}

/// What a `sync` changed.
#[derive(Debug, Default, PartialEq)]
pub struct Synced {
    /// Kept ones whose record came anew (and new ones).
    pub changed: Vec<ChaoId>,
    /// Ones no longer brought.
    pub removed: Vec<ChaoId>,
}

#[derive(Default)]
pub struct Roster {
    pub owners: BTreeMap<PlayerKey, Owner>,
    /// Keys whose file is out of date, and keys whose file goes.
    dirty: BTreeSet<PlayerKey>,
    gone: BTreeSet<PlayerKey>,
}

impl Roster {
    /// Player `key` (named `name`, with `lock`) brings `list` now, read from their save at
    /// `now`: what's kept of them becomes that. A Chao moved to another garden loses its place
    /// there.
    pub fn sync(
        &mut self,
        key: PlayerKey,
        name: &str,
        lock: ChaoLock,
        list: &[BroughtChao],
        now: u64,
    ) -> Synced {
        let owner = self.owners.entry(key).or_insert_with(|| Owner {
            name: name.to_string(),
            seen: now,
            lock,
            chao: Vec::new(),
        });
        owner.name = name.to_string();
        owner.seen = now;
        owner.lock = lock;
        let mut kept = Vec::new();
        let mut synced = Synced::default();
        for b in list.iter().filter(|b| b.data.chao_type() != TYPE_EGG) {
            let identity = b.data.identity();
            if kept
                .iter()
                .any(|s: &Stored| (s.identity, s.nth) == (identity, b.nth))
            {
                continue;
            }
            let id = ChaoId {
                key,
                identity,
                nth: b.nth,
            };
            let before = owner
                .chao
                .iter()
                .position(|s| (s.identity, s.nth) == (identity, b.nth))
                .map(|i| owner.chao.remove(i));
            let place = before
                .as_ref()
                .filter(|s| s.garden == b.garden)
                .and_then(|s| s.place);
            if before
                .as_ref()
                .is_none_or(|s| s.record != b.data || s.garden != b.garden || s.row != b.row)
            {
                synced.changed.push(id);
            }
            kept.push(Stored {
                identity,
                nth: b.nth,
                garden: b.garden,
                row: b.row,
                record: b.data.clone(),
                place,
            });
        }
        synced.removed = owner
            .chao
            .iter()
            .map(|s| ChaoId {
                key,
                identity: s.identity,
                nth: s.nth,
            })
            .collect();
        owner.chao = kept;
        self.dirty.insert(key);
        self.gone.remove(&key);
        synced
    }

    pub fn get(&self, id: &ChaoId) -> Option<&Stored> {
        self.owners
            .get(&id.key)?
            .chao
            .iter()
            .find(|s| (s.identity, s.nth) == (id.identity, id.nth))
    }

    /// Chao `id`, to change (its file is written again).
    pub fn get_mut(&mut self, id: &ChaoId) -> Option<&mut Stored> {
        let stored = self
            .owners
            .get_mut(&id.key)?
            .chao
            .iter_mut()
            .find(|s| (s.identity, s.nth) == (id.identity, id.nth))?;
        self.dirty.insert(id.key);
        Some(stored)
    }

    /// The Chao player `key` brings to `garden` in visitor row `row`.
    pub fn in_row(&self, key: &PlayerKey, garden: u8, row: u8) -> Option<ChaoId> {
        let s = self
            .owners
            .get(key)?
            .chao
            .iter()
            .find(|s| (s.garden, s.row) == (garden, row))?;
        Some(ChaoId {
            key: *key,
            identity: s.identity,
            nth: s.nth,
        })
    }

    /// Player `key` is online at `now`.
    pub fn seen(&mut self, key: &PlayerKey, now: u64) {
        if let Some(owner) = self.owners.get_mut(key)
            && now >= owner.seen + SEEN_EVERY
        {
            owner.seen = now;
            self.dirty.insert(*key);
        }
    }

    /// Drops players not seen for `EXPIRE` (none of `online`); their keys.
    pub fn expire(&mut self, now: u64, online: &[PlayerKey]) -> Vec<PlayerKey> {
        let old: Vec<PlayerKey> = self
            .owners
            .iter()
            .filter(|(k, o)| !online.contains(k) && now.saturating_sub(o.seen) > EXPIRE)
            .map(|(k, _)| *k)
            .collect();
        for key in &old {
            self.owners.remove(key);
            self.dirty.remove(key);
            self.gone.insert(*key);
        }
        old
    }

    /// Files out of date since last asked: (file name, its text, or `None` to delete it).
    pub fn files(&mut self) -> Vec<(String, Option<String>)> {
        let mut out: Vec<(String, Option<String>)> = std::mem::take(&mut self.gone)
            .into_iter()
            .map(|key| (file_name(&key), None))
            .collect();
        for key in std::mem::take(&mut self.dirty) {
            if let Some(owner) = self.owners.get(&key) {
                out.push((file_name(&key), Some(encode(owner))));
            }
        }
        out
    }

    /// Takes back file `name` as `files` wrote it (a bad one is skipped).
    pub fn load(&mut self, name: &str, text: &str) -> bool {
        let Some(key) = name.strip_suffix(".txt").and_then(key_from_hex) else {
            return false;
        };
        match decode(text) {
            Some(owner) => {
                self.owners.insert(key, owner);
                true
            }
            None => false,
        }
    }
}

/// A candidate for a garden's room (`choose`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Candidate {
    pub id: ChaoId,
    /// Its owner was last online then (now, for one online).
    pub seen: u64,
    /// Its visitor row (its owner's first ones go first).
    pub row: u8,
    /// Its owner is offline (or their save isn't loaded).
    pub asleep: bool,
}

/// The held Chao a garden shows, given `room` places left after its present players' own:
/// the most recently played first (online owners' before offline ones'), at most
/// `PER_PLAYER` of each player; the rest wait.
pub fn choose(mut candidates: Vec<Candidate>, room: usize) -> Vec<Candidate> {
    candidates.sort_by(|a, b| {
        b.seen
            .cmp(&a.seen)
            .then(a.id.key.cmp(&b.id.key))
            .then(a.row.cmp(&b.row))
            .then(a.id.cmp(&b.id))
    });
    let mut per: BTreeMap<PlayerKey, usize> = BTreeMap::new();
    candidates
        .into_iter()
        .filter(|c| {
            let n = per.entry(c.id.key).or_default();
            *n += 1;
            *n <= PER_PLAYER
        })
        .take(room)
        .collect()
}

/// A key's file name: its 32 hex digits.
pub fn file_name(key: &PlayerKey) -> String {
    format!("{}.txt", key_hex(key))
}

pub fn key_hex(key: &PlayerKey) -> String {
    key.iter().map(|b| format!("{b:02x}")).collect()
}

pub fn key_from_hex(hex: &str) -> Option<PlayerKey> {
    if hex.len() != 32 || !hex.is_ascii() {
        return None;
    }
    let mut key = [0; 16];
    for (i, b) in key.iter_mut().enumerate() {
        *b = u8::from_str_radix(&hex[i * 2..i * 2 + 2], 16).ok()?;
    }
    Some(key)
}

/// `name <name>`, `seen <unix seconds>`, `lock <ChaoLock::ini>`, then per Chao `chao <garden> <row> <nth> <placed 0/1>
/// <x> <y> <z> <facing> <the .sa2chao file (chao::file) in hex>`.
fn encode(owner: &Owner) -> String {
    let mut text = format!(
        "name {}\nseen {}\nlock {}\n",
        owner.name,
        owner.seen,
        owner.lock.ini()
    );
    for c in &owner.chao {
        let ([x, y, z], facing) = c.place.unwrap_or(([0.0; 3], 0));
        let hex: String = file::encode(&c.record)
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect();
        text.push_str(&format!(
            "chao {} {} {} {} {x} {y} {z} {facing} {hex}\n",
            c.garden,
            c.row,
            c.nth,
            u8::from(c.place.is_some()),
        ));
    }
    text
}

fn decode(text: &str) -> Option<Owner> {
    let mut owner = Owner {
        name: String::new(),
        seen: 0,
        lock: ChaoLock::Always,
        chao: Vec::new(),
    };
    for line in text.lines() {
        let (word, rest) = line.split_once(' ').unwrap_or((line, ""));
        match word {
            "name" => {
                // As a `Hello` has it: at most `MAX_NAME` bytes.
                owner.name.clear();
                for c in rest.chars().filter(|c| !c.is_control()) {
                    if owner.name.len() + c.len_utf8() > MAX_NAME {
                        break;
                    }
                    owner.name.push(c);
                }
            }
            "seen" => owner.seen = rest.trim().parse().ok()?,
            "lock" => owner.lock = ChaoLock::parse(rest),
            "chao" => {
                // A Chao that doesn't read back is left out, not the whole player.
                if let Some(c) = decode_chao(rest) {
                    owner.chao.push(c);
                }
            }
            _ => {}
        }
    }
    (!owner.name.is_empty()).then_some(owner)
}

fn decode_chao(text: &str) -> Option<Stored> {
    let words: Vec<&str> = text.split_whitespace().collect();
    let [garden, row, nth, placed, x, y, z, facing, hex] = words.as_slice() else {
        return None;
    };
    let garden: u8 = garden.parse().ok().filter(|g| (1..=3).contains(g))?;
    let row: u8 = row
        .parse()
        .ok()
        .filter(|&r| usize::from(r) < super::protocol::MAX_CARRIED_CHAO)?;
    let pos: [f32; 3] = [x.parse().ok()?, y.parse().ok()?, z.parse().ok()?];
    if pos.iter().any(|v| !v.is_finite()) || hex.len() % 2 != 0 || !hex.is_ascii() {
        return None;
    }
    let bytes: Vec<u8> = (0..hex.len() / 2)
        .map(|i| u8::from_str_radix(&hex[i * 2..i * 2 + 2], 16).ok())
        .collect::<Option<_>>()?;
    let record = file::decode(&bytes).ok()?;
    Some(Stored {
        identity: record.identity(),
        nth: nth.parse().ok()?,
        garden,
        row,
        record: Box::new(record),
        place: (*placed == "1").then_some((pos, facing.parse().ok()?)),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chao::CHAO_SIZE;

    const PLAYER_1: PlayerKey = [7; 16];
    const PLAYER_2: PlayerKey = [9; 16];

    /// A child of garden `garden` whose DNA is `dna`.
    fn chao(dna: u8, garden: u8) -> Box<ChaoData> {
        let mut c = ChaoData {
            bytes: [0; CHAO_SIZE],
        };
        c.bytes[0x80] = 2;
        c.bytes[0x81] = garden;
        c.bytes[0x438 + 0x10] = dna;
        Box::new(c)
    }

    fn brought(dna: u8, garden: u8, row: u8, nth: u8) -> BroughtChao {
        BroughtChao {
            garden,
            row,
            nth,
            data: chao(dna, garden),
        }
    }

    fn id(dna: u8, nth: u8) -> ChaoId {
        ChaoId {
            key: PLAYER_1,
            identity: chao(dna, 1).identity(),
            nth,
        }
    }

    #[test]
    fn the_save_wins_at_every_login() {
        let mut r = Roster::default();
        let first = r.sync(
            PLAYER_1,
            "Player1",
            ChaoLock::Always,
            &[brought(1, 1, 0, 0), brought(2, 1, 1, 0)],
            100,
        );
        assert_eq!(first.changed, [id(1, 0), id(2, 0)]);
        r.get_mut(&id(1, 0)).unwrap().place = Some(([1.0, 2.0, 3.0], 0x4000));
        r.get_mut(&id(2, 0)).unwrap().place = Some(([4.0, 0.0, 4.0], 0));
        // Next login: Chao 1 grew (its record is new, its place kept), Chao 2 moved to the
        // Dark Garden (its place there is unknown), an egg isn't kept, a new Chao came.
        let mut grown = brought(1, 1, 0, 0);
        grown.data.bytes[0x82] = 40;
        let mut egg = brought(5, 1, 1, 0);
        egg.data.bytes[0x80] = TYPE_EGG;
        let next = r.sync(
            PLAYER_1,
            "Player1",
            ChaoLock::Always,
            &[grown, brought(2, 3, 0, 0), egg, brought(3, 2, 0, 0)],
            200,
        );
        assert_eq!(next.changed, [id(1, 0), id(2, 0), id(3, 0)]);
        assert!(next.removed.is_empty());
        let one = r.get(&id(1, 0)).unwrap();
        assert_eq!(
            (one.record.bytes[0x82], one.place),
            (40, Some(([1.0, 2.0, 3.0], 0x4000)))
        );
        assert_eq!(
            (
                r.get(&id(2, 0)).unwrap().garden,
                r.get(&id(2, 0)).unwrap().place
            ),
            (3, None)
        );
        assert!(r.get(&id(5, 0)).is_none());
        // Then Chao 2 is given away (or died) and 1 isn't brought: they go.
        let last = r.sync(
            PLAYER_1,
            "Player1",
            ChaoLock::Always,
            &[brought(3, 2, 0, 0)],
            300,
        );
        assert!(last.changed.is_empty());
        assert_eq!(last.removed, [id(1, 0), id(2, 0)]);
        assert_eq!(r.owners[&PLAYER_1].chao.len(), 1);
    }

    #[test]
    fn twins_are_told_apart_by_slot_order_and_owners_by_key() {
        let mut r = Roster::default();
        r.sync(
            PLAYER_1,
            "Player1",
            ChaoLock::Always,
            &[brought(1, 1, 0, 0), brought(1, 1, 1, 1)],
            0,
        );
        assert_eq!(r.owners[&PLAYER_1].chao.len(), 2);
        assert_ne!(id(1, 0), id(1, 1));
        // Player2's copy of the same DNA is Player2's Chao, not Player1's.
        r.sync(
            PLAYER_2,
            "Player2",
            ChaoLock::Always,
            &[brought(1, 1, 0, 0)],
            0,
        );
        let erins = ChaoId {
            key: PLAYER_2,
            ..id(1, 0)
        };
        assert!(r.get(&erins).is_some() && r.get(&id(1, 0)).is_some());
        assert_eq!(r.in_row(&PLAYER_1, 1, 1), Some(id(1, 1)));
        assert_eq!(r.in_row(&PLAYER_2, 1, 1), None);
    }

    #[test]
    fn a_garden_shows_the_most_recent_four_of_each_player_first() {
        let c = |key: PlayerKey, dna: u8, seen: u64, row: u8| Candidate {
            id: ChaoId {
                key,
                identity: u64::from(dna),
                nth: 0,
            },
            seen,
            row,
            asleep: seen != u64::MAX,
        };
        // Player1 brings five rows' worth (an old file can't, but four is the most shown).
        let all = vec![
            c(PLAYER_1, 1, 10, 0),
            c(PLAYER_1, 2, 10, 1),
            c(PLAYER_1, 3, 10, 2),
            c(PLAYER_1, 7, 10, 3),
            c(PLAYER_1, 8, 10, 3),
            c(PLAYER_2, 4, 50, 1),
            c(PLAYER_2, 5, 50, 0),
            c([1; 16], 6, u64::MAX, 0),
        ];
        let ids = |v: Vec<Candidate>| v.into_iter().map(|c| c.id.identity).collect::<Vec<_>>();
        // Online first, then Player2 (seen later), then Player1; four each at most.
        assert_eq!(ids(choose(all.clone(), 32)), [6, 5, 4, 1, 2, 3, 7]);
        assert_eq!(ids(choose(all.clone(), 3)), [6, 5, 4]);
        assert!(choose(all, 0).is_empty());
    }

    #[test]
    fn kept_chao_survive_a_restart_and_expire_after_a_week() {
        let mut r = Roster::default();
        r.sync(
            PLAYER_1,
            "Player1 K",
            ChaoLock::ThatGarden,
            &[brought(1, 2, 1, 0)],
            1000,
        );
        r.get_mut(&id(1, 0)).unwrap().place = Some(([1.5, -2.0, 30.25], -0x4000));
        let files = r.files();
        assert_eq!(files.len(), 1);
        let (name, Some(text)) = &files[0] else {
            panic!("{files:?}")
        };
        assert_eq!(name, &format!("{}.txt", "07".repeat(16)));
        assert!(r.files().is_empty());
        let mut back = Roster::default();
        assert!(back.load(name, text));
        assert_eq!(back.owners, r.owners);
        assert_eq!(back.owners[&PLAYER_1].lock, ChaoLock::ThatGarden);
        // A file from before locks: as Always.
        let old: String = text
            .lines()
            .filter(|l| !l.starts_with("lock "))
            .map(|l| format!("{l}\n"))
            .collect();
        let mut older = Roster::default();
        assert!(older.load(name, &old));
        assert_eq!(older.owners[&PLAYER_1].lock, ChaoLock::Always);
        // A bad file or name is left out.
        assert!(!back.load("nothex.txt", text));
        assert!(!Roster::default().load(name, "seen x"));
        // Seen lately (or online) it stays; a week later it goes, and its file with it.
        assert!(back.expire(1000 + EXPIRE, &[]).is_empty());
        assert!(back.expire(1001 + EXPIRE * 2, &[PLAYER_1]).is_empty());
        assert_eq!(back.expire(1001 + EXPIRE, &[]), [PLAYER_1]);
        assert!(back.owners.is_empty());
        assert_eq!(back.files(), [(name.clone(), None)]);
    }
}
