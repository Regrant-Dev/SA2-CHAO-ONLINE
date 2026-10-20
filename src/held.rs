//! Chao the host holds for players who aren't in their garden (net/roster.rs, re/chao-garden-sync.md
//! "Asleep and away Chao").
//!
//! - **Theirs, here.** The host names each one it shows (`HeldChao`: owner id `HELD_OWNER..`,
//!   index, its record, its owner's name, asleep or not). In that garden we make a copy of it
//!   (visitors.rs) where its `ServerChao` has it, and it follows that stream like a friend's
//!   Chao. A sleeping one (its owner is offline) is only carried and thrown (garden.rs); its
//!   name tag says so, and says "locked" while its owner's `ChaoLock` keeps what we do to it
//!   from their save (the host drops it).
//! - **Ours, elsewhere.** While we're online but not in the garden of a Chao we bring, the host
//!   runs it there, and what its life changes comes to us (`SaveChao`): written into its save
//!   slot (found by its `ChaoData::identity` and copy number, the same way `Brought` counted
//!   them), and the save is marked changed (saving.rs writes it out). Nothing is spawned for it
//!   here: we aren't in its garden.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use crate::chao::{CHAO_SIZE, ChaoData, TYPE_EGG};
use crate::game;
use crate::log;
use crate::net::protocol::{GARDEN_DARK, GARDEN_HERO, GardenChao, HELD_OWNER, HeldChao};

/// A held Chao is made here only while its stream is this fresh.
const FRESH: Duration = Duration::from_secs(2);
/// The save's Chao records (24; in the copy in use, `game::chao_slot_address`).
const SAVE_SLOTS: usize = 24;

struct Entry {
    chao: HeldChao,
    /// Its latest state from the server's stream, and when.
    state: Option<(GardenChao, Instant)>,
}

/// What a `SaveChao` carries: identity, copy number, spans.
type Save = (u64, u8, Vec<(u16, Vec<u8>)>);

#[derive(Default)]
struct Held {
    chao: HashMap<(u8, u8), Entry>,
    saves: Vec<Save>,
}

static HELD: Mutex<Option<Held>> = Mutex::new(None);

fn with<R>(f: impl FnOnce(&mut Held) -> R) -> R {
    f(HELD
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .get_or_insert_with(Held::default))
}

/// Network: the host shows held Chao `owner`/`index` (`Some`), or not any more (`None`).
pub fn received(owner: u8, index: u8, chao: Option<HeldChao>) {
    let key = (owner, index);
    let (other, look) = with(|h| {
        let before = h.chao.remove(&key);
        let other = match (&before, &chao) {
            (Some(b), Some(c)) => b.chao.data.identity() != c.data.identity(),
            (Some(_), None) => true,
            _ => false,
        };
        let look = chao
            .as_ref()
            .filter(|c| before.as_ref().is_some_and(|b| b.chao.data != c.data))
            .map(|c| c.data.clone());
        if let Some(chao) = chao {
            log!(
                "held: {}'s Chao {owner}:{index} in garden {} ({})",
                chao.name,
                chao.area,
                if chao.asleep { "asleep" } else { "awake" }
            );
            let state = before.and_then(|b| b.state).filter(|_| !other);
            h.chao.insert(key, Entry { chao, state });
        }
        (other, look)
    });
    if other {
        // Gone, or another Chao under that id: the old copy goes.
        crate::garden::send_home(owner, index);
    } else if let Some(data) = look {
        // Evolved, or its save came anew: the copy takes the new look.
        crate::garden::received_look(owner, index, &data);
    }
}

/// Network: the server's stream of held owner `owner` in garden `area`: only the ones it told
/// us of (a late packet for one gone doesn't bring it back).
pub fn received_state(area: u8, owner: u8, list: Vec<GardenChao>) -> Vec<GardenChao> {
    let now = Instant::now();
    with(|h| {
        list.into_iter()
            .filter(|c| {
                h.chao
                    .get_mut(&(owner, c.index))
                    .filter(|e| e.chao.area == area)
                    .map(|e| e.state = Some((*c, now)))
                    .is_some()
            })
            .collect()
    })
}

/// Network: what one of our Chao's life changed in a garden we're not in (`SaveChao`).
pub fn received_save(identity: u64, nth: u8, spans: Vec<(u16, Vec<u8>)>) {
    with(|h| h.saves.push((identity, nth, spans)));
}

/// Disconnected: the host's Chao are no more (their copies go with the garden's rules).
pub fn clear() {
    let keys: Vec<(u8, u8)> = with(|h| {
        h.saves.clear();
        h.chao.drain().map(|(k, _)| k).collect()
    });
    for (owner, index) in keys {
        crate::garden::send_home(owner, index);
    }
}

/// Whether held Chao `owner`/`index` sleeps (its owner is offline).
pub fn asleep(owner: u8, index: u8) -> bool {
    owner >= HELD_OWNER && with(|h| h.chao.get(&(owner, index)).is_some_and(|e| e.chao.asleep))
}

/// Its name tag ("Mason's Chao", "Mason's Chao (asleep)"), and whether it's locked (nothing we
/// do to it reaches its owner's save: the tag shows a padlock).
pub fn label(owner: u8, index: u8) -> Option<(String, bool)> {
    with(|h| {
        h.chao
            .get(&(owner, index))
            .map(|e| (tag(&e.chao.name, e.chao.asleep), e.chao.locked))
    })
}

/// "Name's Chao", "(asleep)" after it while its owner is offline.
fn tag(name: &str, asleep: bool) -> String {
    if asleep {
        format!("{name}'s Chao (asleep)")
    } else {
        format!("{name}'s Chao")
    }
}

/// Game thread, every frame: our Chao's changes into the save; in garden `garden` (0 when
/// in none) the held Chao there get their copies.
pub fn on_frame(garden: i32) {
    let (saves, spawns) = with(|h| {
        let now = Instant::now();
        let spawns: Vec<((u8, u8), ChaoData, GardenChao)> = h
            .chao
            .iter()
            .filter(|(_, e)| i32::from(e.chao.area) == garden)
            .filter_map(|(&k, e)| {
                let (state, at) = e.state?;
                (now.duration_since(at) < FRESH).then(|| (k, (*e.chao.data).clone(), state))
            })
            .collect();
        (std::mem::take(&mut h.saves), spawns)
    });
    for (identity, nth, spans) in saves {
        write_save(identity, nth, &spans);
    }
    if garden <= 0 {
        return;
    }
    for ((owner, index), data, state) in spawns {
        let [x, y, z] = state.position;
        crate::visitors::spawn_remote(owner, index, data, game::Vector { x, y, z }, Some(state));
    }
}

/// Our save slot holding the `nth` copy (by slot order) of the Chao whose identity is
/// `identity`, among `slots` (each slot's record, if it has a Chao).
pub fn find_slot(slots: &[Option<ChaoData>], identity: u64, nth: u8) -> Option<usize> {
    slots
        .iter()
        .enumerate()
        .filter(|(_, c)| c.as_ref().is_some_and(|c| c.identity() == identity))
        .nth(usize::from(nth))
        .map(|(slot, _)| slot)
}

/// How many slots before `slot` hold a copy of the same Chao (`find_slot`'s `nth`).
pub fn nth_of(slots: &[Option<ChaoData>], slot: usize) -> u8 {
    let Some(Some(me)) = slots.get(slot) else {
        return 0;
    };
    let identity = me.identity();
    slots[..slot]
        .iter()
        .flatten()
        .filter(|c| c.identity() == identity)
        .count() as u8
}

/// The records of the save in use, by slot (`None` for an empty one).
pub fn save_slots() -> Vec<Option<ChaoData>> {
    (0..SAVE_SLOTS)
        .map(|slot| {
            // SAFETY: a slot of the game's static Chao save, read whole (0x800 bytes).
            let bytes =
                unsafe { std::slice::from_raw_parts(record_at(slot) as *const u8, CHAO_SIZE) };
            ChaoData::from_bytes(bytes).filter(|c| !c.is_empty())
        })
        .collect()
}

/// Save slot `slot`'s record in the save in use (as `game::read_chao_slot` reads it).
fn record_at(slot: usize) -> usize {
    game::chao_slot_address(slot)
}

/// Writes what the host's run of one of our Chao changed into its save slot, and has the save
/// written out. Only bytes inside `SAVE_CHAO_FIELDS`; its identity (DNA) never changes here.
fn write_save(identity: u64, nth: u8, spans: &[(u16, Vec<u8>)]) {
    if !game::chao_in_memory() {
        log!("held: a change for our Chao came before the save was loaded; dropped");
        return;
    }
    let slots = save_slots();
    let Some(slot) = find_slot(&slots, identity, nth) else {
        log!("held: a change for a Chao no longer in our save; dropped");
        return;
    };
    let Some(before) = slots[slot].clone() else {
        return;
    };
    let mut after = before.clone();
    if !apply(&mut after, spans) {
        return;
    }
    // SAFETY: our save slot's record in the game's static Chao save, written on the game
    // thread (a live object of it reads it in place, as the game's own updates do).
    unsafe {
        std::ptr::copy_nonoverlapping(after.bytes.as_ptr(), record_at(slot) as *mut u8, CHAO_SIZE);
    }
    crate::saving::changed();
    let kind = after.chao_type();
    if kind != before.chao_type() {
        log!(
            "held: our Chao in slot {} evolved in its garden (type {kind:#04x})",
            slot + 1
        );
        crate::hud::toast("One of your Chao evolved in its garden");
        if let Some(garden) = opened_by(kind) {
            crate::unlocks::open(garden);
        }
    }
}

/// `spans` into `chao`, those inside `SAVE_CHAO_FIELDS` (the decoder checked them); false
/// when nothing was left to write (or it would no longer be a valid Chao).
fn apply(chao: &mut ChaoData, spans: &[(u16, Vec<u8>)]) -> bool {
    let mut after = chao.clone();
    for (at, bytes) in spans {
        let at = usize::from(*at);
        if crate::net::protocol::save_field(at, bytes.len()) {
            after.bytes[at..at + bytes.len()].copy_from_slice(bytes);
        }
    }
    if after == *chao || after.validate().is_err() || after.chao_type() == TYPE_EGG {
        return false;
    }
    *chao = after;
    true
}

/// The garden an evolution into `kind` opens (the game's 0x568630: Hero types set 0x10, Dark
/// ones 0x40).
fn opened_by(kind: u8) -> Option<u8> {
    match kind {
        5..=22 => match (kind - 5) % 3 {
            1 => Some(GARDEN_HERO),
            2 => Some(GARDEN_DARK),
            _ => None,
        },
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn chao(dna: u8) -> ChaoData {
        let mut c = ChaoData {
            bytes: [0; CHAO_SIZE],
        };
        c.bytes[0x80] = 2;
        c.bytes[0x81] = 1;
        c.bytes[0x440] = dna;
        c
    }

    #[test]
    fn our_chao_is_found_by_identity_and_copy_number() {
        let slots = vec![
            Some(chao(1)),
            None,
            Some(chao(2)),
            Some(chao(1)),
            Some(chao(1)),
        ];
        let one = chao(1).identity();
        assert_eq!(find_slot(&slots, one, 0), Some(0));
        assert_eq!(find_slot(&slots, one, 2), Some(4));
        assert_eq!(find_slot(&slots, one, 3), None);
        assert_eq!(find_slot(&slots, chao(3).identity(), 0), None);
        assert_eq!(
            (0..5).map(|s| nth_of(&slots, s)).collect::<Vec<_>>(),
            [0, 0, 0, 1, 2]
        );
    }

    #[test]
    fn name_tags_say_asleep() {
        assert_eq!(tag("Mason", false), "Mason's Chao");
        assert_eq!(tag("Mason", true), "Mason's Chao (asleep)");
    }

    #[test]
    fn only_a_chaos_life_is_written() {
        let mut c = chao(1);
        // Evolved into a hero swim adult, happier; a span outside the fields is skipped.
        assert!(apply(
            &mut c,
            &[(0x80, vec![9]), (0x82, vec![30, 0]), (0x12, vec![1, 2, 3])]
        ));
        assert_eq!((c.bytes[0x80], c.bytes[0x82], c.bytes[0x12]), (9, 30, 0));
        assert_eq!(opened_by(9), Some(GARDEN_HERO));
        assert_eq!(opened_by(0x16), Some(GARDEN_DARK));
        assert_eq!(opened_by(5), None);
        // Nothing new, or a record that isn't a Chao any more: left alone.
        assert!(!apply(&mut c, &[(0x80, vec![9])]));
        assert!(!apply(&mut c, &[(0x80, vec![0])]));
        assert_eq!(c.bytes[0x80], 9);
    }
}
