//! Treasure hunt as a room battle (re/treasure-hunt.md).
//!
//! In a hunt stage started together, the room hunts the same 3 pieces: the lowest-numbered room
//! player there leads, and a fixed native set is shared (`HuntSet`). Every game, the
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
use crate::net::protocol::hunt_signature as signature;

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
const CANDIDATE_POINTERS: usize = 0x5C;
const CANDIDATE_COUNTS: usize = 0x06;
const CANDIDATE_CAPACITY: usize = 48;
const PICK_CALL: usize = 0x0073_9A55;
const NATIVE_PICK: usize = 0x0073_80A0;
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
const MOVING_MAIN: usize = 0x006D_2310;
const OBJ_MAIN: usize = 0x10;
const OBJ_DATA1: usize = 0x34;
const SUB_ASK: u8 = 1;
const SUB_ACTIVE: u8 = 2;
const SUB_HIDDEN: u8 = 3;

/// Send the leader's set this often (it's small; late arrivals get it soon).
const SET_EVERY: Duration = Duration::from_secs(1);
/// The leader waits this long for a set already going round (someone started before it
/// loaded) before giving the room its own pieces.
const SET_WAIT: Duration = Duration::from_secs(2);

type Piece = (u8, u8, [f32; 3]);

#[derive(Default)]
struct Hunt {
    stage: Option<(i16, u8)>,
    /// `game::stage_run` of `stage`: a replay starts a new hunt.
    run: u32,
    /// The room's pieces for this stage entry, once known.
    set: Option<[Piece; 3]>,
    /// Only this player supplies the canonical set. Relaying competing sets can split a hunt.
    authority: Option<u8>,
    incoming: Vec<(u8, i16, u8, [Piece; 3])>,
    /// Rebind even if the vanilla slot already has the same ID, and discard old local picks.
    rebind: bool,
    reset_collected: bool,
    active: bool,
    /// When we started hunting without a set.
    waiting: Option<Instant>,
    /// Who has each piece (referee's word), and what we already saw collected locally.
    owner: [Option<u8>; 3],
    claimed: [bool; 3],
    sent: Option<Instant>,
    set_out: Option<(i16, u8, [Piece; 3])>,
    claims_out: Vec<(i16, u8, u32, u8)>,
    taken_in: Vec<(u32, u8, u8)>,
    ended: bool,
}

static HUNT: Mutex<Option<Hunt>> = Mutex::new(None);

fn with<R>(f: impl FnOnce(&mut Hunt) -> R) -> R {
    let mut h = HUNT.lock().unwrap_or_else(|e| e.into_inner());
    f(h.get_or_insert_with(Hunt::default))
}

fn fixed_kind(slot: usize, kind: u8, mission: u8) -> bool {
    if mission == 4 {
        return kind == 9;
    }
    match slot {
        0 => matches!(kind, 1 | 3),
        1 => matches!(kind, 0 | 2 | 5),
        2 => kind == 4,
        _ => false,
    }
}

fn valid_set(pieces: &[Piece; 3], mission: u8) -> bool {
    pieces
        .iter()
        .enumerate()
        .all(|(slot, &(kind, id, position))| {
            fixed_kind(slot, kind, mission)
                && (mission != 4 || usize::from(id) == slot)
                && position
                    .iter()
                    .all(|v| v.is_finite() && v.abs() <= 1_000_000.0)
        })
}

/// Keep vanilla's fixed picks. Replace moving, enemy-held or upgrade-only choices with a
/// native fixed candidate in that slot's own list, nearest to the original pick.
fn fixed_pick(original: Piece, candidates: &[Piece], slot: usize, mission: u8) -> Option<Piece> {
    let valid = |p: &Piece| {
        fixed_kind(slot, p.0, mission)
            && p.2.iter().all(|v| v.is_finite() && v.abs() <= 1_000_000.0)
            && candidates
                .iter()
                .filter(|c| (c.0, c.1) == (p.0, p.1))
                .count()
                == 1
    };
    if valid(&original) {
        return Some(original);
    }
    if !original.2.iter().all(|v| v.is_finite()) {
        return None;
    }
    candidates.iter().copied().filter(valid).min_by(|a, b| {
        let distance = |p: &Piece| {
            p.2.iter()
                .zip(original.2)
                .map(|(a, b)| (a - b).powi(2))
                .sum::<f32>()
        };
        distance(a).total_cmp(&distance(b))
    })
}

fn read_fixed_set(manager: usize, mission: u8) -> Option<[Piece; 3]> {
    let mut pieces = [(NONE, 0, [0.0; 3]); 3];
    for (s, piece) in pieces.iter_mut().enumerate() {
        let a = slot(manager, s);
        let original = (read::<u8>(a), read::<u8>(a + 1), read::<[f32; 3]>(a + 4));
        if matches!(original.0, NONE | COLLECTED) {
            return None;
        }
        let mut candidates = Vec::new();
        if mission == 4 {
            candidates.push(original);
        } else {
            let count = usize::from(read::<u8>(manager + CANDIDATE_COUNTS + s));
            let pointer = read::<usize>(manager + CANDIDATE_POINTERS + s * 4);
            if count > CANDIDATE_CAPACITY || pointer == 0 {
                return None;
            }
            for index in 0..count {
                let a = pointer + index * SLOT_SIZE;
                candidates.push((read(a), read(a + 1), read(a + 4)));
            }
        }
        *piece = fixed_pick(original, &candidates, s, mission)?;
    }
    valid_set(&pieces, mission).then_some(pieces)
}

impl Hunt {
    fn adopt(&mut self, from: u8, pieces: [Piece; 3], mission: u8) -> bool {
        if self.authority != Some(from) || !valid_set(&pieces, mission) {
            return false;
        }
        if self.set == Some(pieces) {
            return false;
        }
        if self
            .set
            .is_none_or(|old| signature(&old) != signature(&pieces))
        {
            self.owner = [None; 3];
            self.claimed = [false; 3];
            self.claims_out.clear();
            let new_sig = signature(&pieces);
            // Reliable pickup acknowledgements can beat the unreliable set announcement.
            self.taken_in.retain(|(sig, _, _)| *sig == new_sig);
            self.ended = false;
            self.reset_collected = true;
        }
        self.set = Some(pieces);
        self.sent = None;
        self.rebind = true;
        true
    }
}

fn picker_call(target: usize) -> [u8; 5] {
    let mut bytes = [0xE8, 0, 0, 0, 0];
    bytes[1..].copy_from_slice(&(target.wrapping_sub(PICK_CALL + 5) as u32).to_le_bytes());
    bytes
}

/// Keep the canonical selection on death/retry before piece objects can query or collide.
pub fn install() {
    if crate::patch::replace_code(
        PICK_CALL,
        &picker_call(NATIVE_PICK),
        &picker_call(pick as extern "C" fn(usize) as usize),
    ) {
        log!("hunt: native selection hook installed");
    } else {
        log!("hunt: native selection hook unavailable; check conflicting hunt mods");
    }
}

fn projected_set(
    set: [Piece; 3],
    owner: [Option<u8>; 3],
    claimed: [bool; 3],
    me: Option<u8>,
) -> [Piece; 3] {
    std::array::from_fn(|s| {
        if owner[s].is_some() || claimed[s] {
            (
                COLLECTED,
                if owner[s].is_some() && owner[s] != me {
                    SOMEONE_ELSE
                } else {
                    0
                },
                set[s].2,
            )
        } else {
            set[s]
        }
    })
}

extern "C" fn pick(manager: usize) {
    // SAFETY: the original native cdecl picker, with the manager argument from 0x739A55.
    #[cfg(target_arch = "x86")]
    unsafe {
        std::mem::transmute::<usize, extern "C" fn(usize)>(NATIVE_PICK)(manager)
    };
    let _ = std::panic::catch_unwind(|| {
        // Ask outside the hunt lock: the network thread holds ONLINE before HUNT.
        let me = crate::online::my_id();
        with(|h| {
            if !h.active
                || game::two_player()
                || h.stage != Some((game::current_level(), game::mission()))
            {
                return;
            }
            let Some(set) = h.set else { return };
            let selected = projected_set(set, h.owner, h.claimed, me);
            for (s, &(kind, id, pos)) in selected.iter().enumerate() {
                let a = slot(manager, s);
                write(a, kind);
                write(a + 1, id);
                write(a + 4, pos);
            }
            write(
                manager + M_LEFT,
                selected.iter().filter(|p| p.0 != COLLECTED).count() as u8,
            );
            h.rebind = true;
        });
    });
}

fn read<T: Copy>(address: usize) -> T {
    // SAFETY: the game's static globals and the live hunt manager (callers check it).
    unsafe { (address as *const T).read_volatile() }
}

fn write<T>(address: usize, value: T) {
    // SAFETY: as in `read`, on the game thread.
    unsafe { (address as *mut T).write_volatile(value) }
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
        if obj == 0 || !matches!(read::<usize>(obj + OBJ_MAIN), PIECE_MAIN | MOVING_MAIN) {
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
pub fn received_set(from: u8, level: i16, mission: u8, pieces: [Piece; 3]) {
    with(|h| {
        // Keep one pending update per sender. Choose the authority on the game thread,
        // after this frame's peer list is available, rather than letting packet order elect it.
        if h.stage == Some((level, mission)) && valid_set(&pieces, mission) {
            h.incoming.retain(|(id, _, _, _)| *id != from);
            if h.incoming.len() < 8 {
                h.incoming.push((from, level, mission, pieces));
            }
        }
    });
}

/// Network (the referee): piece `slot` of hunt `sig` went to `player`.
pub fn received_taken(level: i16, mission: u8, sig: u32, slot: u8, player: u8) {
    with(|h| {
        if h.stage == Some((level, mission)) {
            h.taken_in.push((sig, slot, player));
        }
    });
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
        let run = game::stage_run();
        if h.stage != stage || h.run != run {
            *h = Hunt {
                stage,
                run,
                ..Hunt::default()
            };
        }
        h.active = false;
        let (Some(me), Some((level, mission))) = (me, stage) else {
            return;
        };
        if !matches!(game::game_state(), game::GAME_STATE_INGAME | 17)
            || game::two_player()
            || !matches!(level, 5 | 7 | 8 | 16 | 18 | 25 | 26 | 32 | 44 | 59)
            || !matches!(mission, 0 | 3 | 4)
        {
            return;
        }
        let m = read::<usize>(MANAGER);
        let room = crate::room::in_room() && !here.is_empty();
        if m == 0
            || !room
            || (crate::room::mode() != crate::room::Mode::Coop
                && crate::together::started_at().is_none()
                && !crate::together::is_holding())
        {
            return;
        }
        h.active = true;
        let hunting = read::<u8>(m + M_STATE) >= STATE_HUNTING;

        let authority = here.iter().copied().chain([me]).min().unwrap_or(me);
        if h.authority != Some(authority) {
            h.authority = Some(authority);
            h.sent = None;
            h.set_out = None;
        }
        for (from, l, m, pieces) in std::mem::take(&mut h.incoming) {
            if (l, m) == (level, mission) && h.adopt(from, pieces, mission) {
                log!("hunt: adopted player {from}'s fixed spots {:?}", pieces);
            }
        }
        // One authority chooses and broadcasts. Followers cannot lock themselves to a
        // temporary local pick or keep rebroadcasting a conflicting old selection.
        let leader = me == authority;
        if h.set.is_none() && hunting {
            h.waiting.get_or_insert_with(Instant::now);
        }
        let waited = h.waiting.is_some_and(|at| at.elapsed() >= SET_WAIT);
        if leader
            && waited
            && h.set.is_none()
            && hunting
            && let Some(pieces) = read_fixed_set(m, mission)
        {
            h.adopt(me, pieces, mission);
            log!("hunt: our fixed spots {:?} for the room", pieces);
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
                && (player == me || here.contains(&player))
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
        let rebind = std::mem::take(&mut h.rebind);
        let reset_collected = std::mem::take(&mut h.reset_collected);
        let mut changed = rebind;
        let mut left = 0u8;
        let mut hide = Vec::new();
        for (s, &(kind, id, pos)) in set.iter().enumerate() {
            let a = slot(m, s);
            let ours = h.owner[s] == Some(me);
            let theirs = h.owner[s].is_some_and(|o| o != me);
            let (now_kind, now_id) = (read::<u8>(a), read::<u8>(a + 1));
            if now_kind == COLLECTED && !reset_collected {
                // Collected here: claim it (once). The referee says whose it is.
                if !h.claimed[s] && !theirs && now_id != SOMEONE_ELSE {
                    h.claimed[s] = true;
                    h.claims_out.push((level, mission, sig, s as u8));
                    log!("hunt: we collected piece {}", s + 1);
                }
                continue;
            }
            if theirs || ours {
                // An already-owned piece stays collected after resets. Other players' pieces
                // give no local score. Reset hints when the current target is removed.
                let target = (0..3).find(|&i| read::<u8>(slot(m, i)) != COLLECTED);
                write(a, COLLECTED);
                write(a + 1, if theirs { SOMEONE_ELSE } else { 0 });
                hide.push((kind, id));
                if target == Some(s) {
                    write(m + M_HINTS, 0u8);
                    write(m + M_NEAR_VOICE, 0u8);
                }
                changed = true;
                continue;
            }
            if rebind || (now_kind, now_id) != (kind, id) {
                write(a, kind);
                write(a + 1, id);
                write(a + 4, pos);
                changed = true;
            }
            left += 1;
        }
        if changed {
            write(m + M_LEFT, left);
            if rebind {
                write(m + M_HINTS, 0u8);
                write(m + M_NEAR_VOICE, 0u8);
            }
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

#[cfg(test)]
mod tests {
    use super::*;

    fn fixed() -> [Piece; 3] {
        [
            (1, 2, [10.0, 0.0, 0.0]),
            (5, 2, [780.0, 145.0, 895.0]),
            (4, 5, [600.0, -15.0, 1740.0]),
        ]
    }

    #[test]
    fn unsafe_enemy_and_moving_picks_use_fixed_native_candidates() {
        let original = (10, 12, [5.0, 0.0, 0.0]);
        let candidates = [
            (3, 4, [50.0, 0.0, 0.0]),
            (1, 8, [6.0, 0.0, 0.0]),
            (10, 12, [5.0, 0.0, 0.0]),
        ];
        assert_eq!(fixed_pick(original, &candidates, 0, 0), Some(candidates[1]));
        assert_eq!(
            fixed_pick(fixed()[0], &[fixed()[0]], 0, 0),
            Some(fixed()[0])
        );
        assert_eq!(
            fixed_pick((7, 2, [0.0; 3]), &[(8, 3, [0.0; 3]), fixed()[2]], 2, 0),
            Some(fixed()[2])
        );
        assert!(fixed_pick(original, &[], 0, 0).is_none());
    }

    #[test]
    fn authority_can_replace_a_conflicting_pick_but_followers_cannot() {
        let mut h = Hunt {
            authority: Some(1),
            ..Hunt::default()
        };
        assert!(!h.adopt(2, fixed(), 0));
        assert!(h.adopt(1, fixed(), 0));
        h.owner[0] = Some(2);
        h.claimed[0] = true;
        assert!(!h.adopt(1, fixed(), 0));
        assert_eq!(h.owner[0], Some(2)); // repeated announcements preserve shared credit
        let mut other = fixed();
        other[0].1 = 7;
        assert!(!h.adopt(2, other, 0));
        assert!(h.adopt(1, other, 0));
        assert_eq!(h.owner, [None; 3]);
        assert_eq!(h.claimed, [false; 3]);
        assert!(h.reset_collected && h.rebind);
    }

    #[test]
    fn retry_reuses_shared_spots_and_preserves_both_players_collections() {
        let original = fixed();
        let projected = projected_set(original, [Some(1), Some(2), None], [false; 3], Some(1));
        assert_eq!(projected[0].0, COLLECTED);
        assert_eq!(projected[0].1, 0);
        assert_eq!(projected[1].0, COLLECTED);
        assert_eq!(projected[1].1, SOMEONE_ELSE);
        assert_eq!(projected[2], original[2]);
        assert_eq!(
            projected_set(original, [None; 3], [true, false, false], Some(1))[0].0,
            COLLECTED
        );
    }

    #[test]
    fn invalid_or_moving_sets_cannot_become_the_shared_hunt() {
        assert!(valid_set(&fixed(), 0));
        let mut set = fixed();
        set[2].0 = 7;
        assert!(!valid_set(&set, 0));
        set = fixed();
        set[0].2[1] = f32::NAN;
        assert!(!valid_set(&set, 0));
        let hard = [(9, 0, [0.0; 3]), (9, 1, [1.0; 3]), (9, 2, [2.0; 3])];
        assert!(valid_set(&hard, 4));
        assert!(!valid_set(&hard, 0));
    }

    #[test]
    fn duplicate_native_ids_cannot_point_the_radar_at_two_different_spots() {
        let duplicate = (3, 6, [0.0; 3]); // duplicated in Mad Space's native SET
        let pool = [duplicate, (3, 6, [1000.0; 3]), (1, 7, [20.0; 3])];
        assert_eq!(fixed_pick(duplicate, &pool, 0, 0), Some(pool[2]));
    }

    #[test]
    fn pickup_credit_arriving_before_the_set_is_preserved() {
        let set = fixed();
        let sig = signature(&set);
        let mut h = Hunt {
            authority: Some(1),
            taken_in: vec![(sig, 0, 2), (sig ^ 1, 1, 3)],
            ..Hunt::default()
        };
        assert!(h.adopt(1, set, 0));
        assert_eq!(h.taken_in, vec![(sig, 0, 2)]);
    }
}
