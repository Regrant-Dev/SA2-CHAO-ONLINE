//! Treasure hunt as a room battle (re/treasure-hunt.md).
//!
//! In a hunt stage started together, the room hunts the same 3 pieces: the lowest-numbered room
//! player there leads, and their game's first pick is shared (`HuntSet`). Every game, the
//! leader's too, keeps its 3 piece slots set to it (deaths re-pick pieces otherwise), and
//! re-asks the loaded pieces whether they're chosen. A piece one of us collects is claimed from
//! the host's referee; the first claim wins and the piece goes from everyone else's game, with
//! no score for them (radar and hints move on by themselves). The last piece clears the stage
//! for everyone; the referee's count is the score.

use std::sync::Mutex;
use std::time::{Duration, Instant};

use crate::game;
use crate::hud;
use crate::log;

/// The hunt manager's data: +0 state (4 = hunting), +3/+4 hint state for the target, +5 pieces
/// left, slots at +0x2C/+0x3C/+0x4C (`{u8 kind (0xFE collected, 0xFF none), u8 id, u16,
/// f32 pos[3]}`).
const MANAGER: usize = 0x01AF_014C;
const M_STATE: usize = 0x00;
const M_HINTS: usize = 0x03;
const M_NEAR_VOICE: usize = 0x04;
const M_LEFT: usize = 0x05;
const M_SLOTS: usize = 0x2C;
const SLOT_SIZE: usize = 0x10;
const STATE_HUNTING: u8 = 4;
const COLLECTED: u8 = 0xFE;
const NONE: u8 = 0xFF;
/// "Someone else" in a collected slot's id (0/1 are players in 2P).
const SOMEONE_ELSE: u8 = 2;

/// SET records (re/stage-objects.md): flags +2 (bit 1 = loaded), object +4.
const RECORDS: usize = 0x01DD_E280;
const RECORD_SIZE: usize = 0x10;
const RECORD_COUNT: usize = 0x01DD_E270;
const MAX_RECORDS: usize = 0x600;
/// A piece object: its main, and Data1 (+1 sub-state: 1 = ask whether chosen, 2 = active,
/// 3 = not chosen; +8 rotX = kind << 8 | id).
const PIECE_MAIN: usize = 0x006D_0AD0;
const OBJ_MAIN: usize = 0x10;
const OBJ_DATA1: usize = 0x34;
const SUB_ASK: u8 = 1;
const SUB_ACTIVE: u8 = 2;
const SUB_HIDDEN: u8 = 3;

/// Send the leader's set this often (it's small; late arrivals get it soon).
const SET_EVERY: Duration = Duration::from_secs(1);

type Piece = (u8, u8, [f32; 3]);

struct Hunt {
    stage: Option<(i16, u8)>,
    /// The room's pieces for this stage entry, once known.
    set: Option<[Piece; 3]>,
    /// Who has each piece (referee's word), and what we already saw collected locally.
    owner: [Option<u8>; 3],
    claimed: [bool; 3],
    sent: Option<Instant>,
    set_out: Option<(i16, u8, [Piece; 3])>,
    claims_out: Vec<(i16, u8, u32, u8)>,
    taken_in: Vec<(u32, u8, u8)>,
    ended: bool,
    seen: Option<(u8, u8, [[u8; 2]; 3])>,
}

static HUNT: Mutex<Option<Hunt>> = Mutex::new(None);

fn with<R>(f: impl FnOnce(&mut Hunt) -> R) -> R {
    let mut h = HUNT.lock().unwrap_or_else(|e| e.into_inner());
    f(h.get_or_insert_with(|| Hunt {
        stage: None,
        set: None,
        owner: [None; 3],
        claimed: [false; 3],
        sent: None,
        set_out: None,
        claims_out: Vec::new(),
        taken_in: Vec::new(),
        ended: false,
        seen: None,
    }))
}

fn read<T: Copy>(address: usize) -> T {
    // SAFETY: the game's static globals and the live hunt manager (callers check it).
    unsafe { (address as *const T).read_volatile() }
}

fn write<T>(address: usize, value: T) {
    // SAFETY: as in `read`, on the game thread.
    unsafe { (address as *mut T).write_volatile(value) }
}

/// A short signature of a set (claims name the hunt they're for).
fn signature(set: &[Piece; 3]) -> u32 {
    set.iter().fold(0x811C_9DC5u32, |h, (k, i, _)| {
        (h ^ u32::from(*k)).wrapping_mul(0x0100_0193) ^ u32::from(*i)
    })
}

fn slot(m: usize, s: usize) -> usize {
    m + M_SLOTS + s * SLOT_SIZE
}

/// Asks the loaded piece objects again whether they're chosen (after we changed the slots),
/// and hides `hide` (a piece someone else got).
fn requery(set: &[Piece; 3], hide: &[(u8, u8)]) {
    let count = (read::<i16>(RECORD_COUNT).max(0) as usize).min(MAX_RECORDS);
    for n in 0..count {
        let rec = RECORDS + n * RECORD_SIZE;
        if read::<u16>(rec + 2) & 1 == 0 {
            continue;
        }
        let obj = read::<usize>(rec + 4);
        if obj == 0 || read::<usize>(obj + OBJ_MAIN) != PIECE_MAIN {
            continue;
        }
        let d = read::<usize>(obj + OBJ_DATA1);
        if d == 0 {
            continue;
        }
        let rot = read::<i32>(d + 8);
        let piece = ((((rot >> 8) & 0xFF) % 11) as u8, (rot & 0xFF) as u8);
        let sub = read::<u8>(d + 1);
        if hide.contains(&piece) {
            write(d + 1, SUB_HIDDEN);
        } else if set.iter().any(|(k, i, _)| (*k, *i) == piece) {
            if sub != SUB_ACTIVE {
                write(d + 1, SUB_ASK);
            }
        } else if sub == SUB_ACTIVE {
            write(d + 1, SUB_HIDDEN);
        }
    }
}

/// Network: the room's pieces for a stage.
pub fn received_set(level: i16, mission: u8, pieces: [Piece; 3]) {
    with(|h| {
        if h.stage == Some((level, mission)) && h.set.is_none() {
            log!(
                "hunt: the room's pieces {:?}",
                pieces.map(|(k, i, _)| (k, i))
            );
            h.set = Some(pieces);
        }
    });
}

/// Network (the referee): piece `slot` of hunt `sig` went to `player`.
pub fn received_taken(sig: u32, slot: u8, player: u8) {
    with(|h| h.taken_in.push((sig, slot, player)));
}

/// To send: our set (as leader), and our claims.
#[allow(clippy::type_complexity)]
pub fn take_outgoing() -> (Option<(i16, u8, [Piece; 3])>, Vec<(i16, u8, u32, u8)>) {
    with(|h| (h.set_out.take(), std::mem::take(&mut h.claims_out)))
}

/// Call every frame with our id and the room players in our stage.
pub fn on_frame(me: Option<u8>, here: &[u8]) {
    let level = game::current_level();
    let mission = game::mission();
    with(|h| {
        let stage = (level > 0 && level != game::LEVEL_CHAO_WORLD).then_some((level, mission));
        if h.stage != stage {
            *h = Hunt {
                stage,
                set: None,
                owner: [None; 3],
                claimed: [false; 3],
                sent: None,
                set_out: None,
                claims_out: Vec::new(),
                taken_in: Vec::new(),
                ended: false,
                seen: h.seen,
            };
        }
        let m = read::<usize>(MANAGER);
        // Diagnostics: the manager's state and pieces left as they change.
        let seen = (m != 0).then(|| {
            (
                read::<u8>(m + M_STATE),
                read::<u8>(m + M_LEFT),
                std::array::from_fn::<_, 3, _>(|s| read::<[u8; 2]>(slot(m, s))),
            )
        });
        if seen != h.seen {
            log!("hunt: manager (state, left, slots) {seen:?}");
            h.seen = seen;
        }
        let (Some(me), Some((level, mission))) = (me, stage) else {
            return;
        };
        let room = crate::room::in_room() && !here.is_empty();
        if m == 0
            || !room
            || crate::together::started_at().is_none() && !crate::together::is_holding()
        {
            return;
        }
        let hunting = read::<u8>(m + M_STATE) >= STATE_HUNTING;

        // The leader shares its game's pick (and keeps to it).
        let leader = here.iter().all(|&id| me < id);
        if leader && h.set.is_none() && hunting {
            let pieces: [Piece; 3] = std::array::from_fn(|s| {
                let a = slot(m, s);
                (read::<u8>(a), read::<u8>(a + 1), read::<[f32; 3]>(a + 4))
            });
            if pieces.iter().all(|(k, _, _)| *k != NONE && *k != COLLECTED) {
                log!(
                    "hunt: our pieces {:?} for the room",
                    pieces.map(|(k, i, _)| (k, i))
                );
                h.set = Some(pieces);
            }
        }
        let Some(set) = h.set else { return };
        let sig = signature(&set);
        if leader && h.sent.is_none_or(|at| at.elapsed() >= SET_EVERY) {
            h.sent = Some(Instant::now());
            h.set_out = Some((level, mission, set));
        }

        // The referee's answers.
        for (s, sl, player) in std::mem::take(&mut h.taken_in) {
            if s == sig
                && let Some(o) = h.owner.get_mut(usize::from(sl))
                && o.is_none()
            {
                *o = Some(player);
                let who = if player == me {
                    "You".to_string()
                } else {
                    crate::room::name_of(player)
                };
                hud::toast(format!("{who} found piece {}", sl + 1));
            }
        }
        if !hunting {
            return;
        }

        // Keep our slots to the room's set, the others' pieces taken away.
        let mut changed = false;
        let mut left = 0u8;
        let mut hide = Vec::new();
        for (s, &(kind, id, pos)) in set.iter().enumerate() {
            let a = slot(m, s);
            let ours = h.owner[s] == Some(me);
            let theirs = h.owner[s].is_some_and(|o| o != me);
            let (now_kind, now_id) = (read::<u8>(a), read::<u8>(a + 1));
            if now_kind == COLLECTED {
                // Collected here: claim it (once). The referee says whose it is.
                if !h.claimed[s] && !theirs && now_id != SOMEONE_ELSE {
                    h.claimed[s] = true;
                    h.claims_out.push((level, mission, sig, s as u8));
                    log!("hunt: we collected piece {}", s + 1);
                }
                continue;
            }
            if theirs {
                // Someone else got it: gone here, no score for us. If it was the target, the
                // next one's hints start over (like the game does).
                let target = (0..3).find(|&i| read::<u8>(slot(m, i)) != COLLECTED);
                write(a, COLLECTED);
                write(a + 1, SOMEONE_ELSE);
                hide.push((kind, id));
                if target == Some(s) {
                    write(m + M_HINTS, 0u8);
                    write(m + M_NEAR_VOICE, 0u8);
                }
                changed = true;
                continue;
            }
            if ours {
                continue;
            }
            if (now_kind, now_id) != (kind, id) {
                write(a, kind);
                write(a + 1, id);
                write(a + 4, pos);
                changed = true;
            }
            left += 1;
        }
        if changed {
            write(m + M_LEFT, left);
            requery(&set, &hide);
        }

        // The score.
        let score = |p: u8| h.owner.iter().filter(|o| **o == Some(p)).count();
        let mut players: Vec<u8> = here.to_vec();
        players.push(me);
        let line = players
            .iter()
            .map(|&p| {
                let who = if p == me {
                    "You".to_string()
                } else {
                    crate::room::name_of(p)
                };
                format!("{who} {}", score(p))
            })
            .collect::<Vec<_>>()
            .join("  ");
        hud::corner(&format!("Pieces: {line}"));
        if h.owner.iter().all(Option::is_some) && !h.ended {
            h.ended = true;
            let best = players
                .iter()
                .copied()
                .max_by_key(|&p| score(p))
                .unwrap_or(me);
            // Co-op rooms hunt as a team: no winner, just who found what.
            if crate::room::mode() == crate::room::Mode::Coop {
                hud::toast(format!("All pieces found! ({line})"));
            } else {
                let who = if best == me {
                    "You win".to_string()
                } else {
                    format!("{} wins", crate::room::name_of(best))
                };
                hud::toast(format!("Treasure hunt over: {who} ({line})"));
            }
        }
    });
}
