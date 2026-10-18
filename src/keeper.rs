//! Who keeps a shared garden: one player whose game runs every Chao there (theirs and the
//! visitors), the trees and the toys, so the garden has one brain (re/chao-garden-sync.md
//! "Keeper").
//!
//! The keeper says so a few times a second (`GardenKeeper`, with a term). Everyone follows the
//! player saying so; a newcomer doesn't take over from a keeper already there. When nobody says
//! so for a moment (the keeper left, dropped or tabbed out, which pauses their game), the lowest
//! player id in the garden takes over with a higher term, and the higher term wins if the old
//! keeper comes back. Two players claiming at once: the higher term, then the lower id.
//!
//! Alone in a garden we keep it without saying so (the game as usual).

use std::sync::Mutex;
use std::time::{Duration, Instant};

use crate::net::protocol::Body;

/// A claim counts this long after it's heard.
const CLAIM_STALE: Duration = Duration::from_millis(1500);
/// With friends and no claim this long, the lowest id claims; after `GIVE_UP` times as long
/// anyone does (the lowest never did).
const QUIET: Duration = Duration::from_secs(1);
const GIVE_UP: u32 = 3;
/// The keeper says so this often (frames).
const CLAIM_EVERY: u32 = 15;

/// What a game does about its garden.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Role {
    /// No friends here: we keep it, quietly.
    Alone,
    /// We keep it and say so.
    Keep,
    /// Player `id` keeps it.
    Follow(u8),
    /// Friends here, nobody keeps it yet.
    Wait,
}

/// Decides our role: us, the friends in our garden now, the claims heard lately from them
/// (id, term), whether we claim now (and our term), and how long nobody has.
pub fn decide(
    me: u8,
    friends: &[u8],
    claims: &[(u8, u32)],
    keeping: Option<u32>,
    quiet: Duration,
) -> Role {
    if friends.is_empty() {
        return Role::Alone;
    }
    let beats = |(id, term): (u8, u32), (other, other_term): (u8, u32)| {
        term > other_term || (term == other_term && id < other)
    };
    let best = claims
        .iter()
        .copied()
        .filter(|(id, _)| friends.contains(id))
        .reduce(|a, b| if beats(b, a) { b } else { a });
    match (keeping, best) {
        (Some(term), Some(claim)) if beats(claim, (me, term)) => Role::Follow(claim.0),
        (Some(_), _) => Role::Keep,
        (None, Some(claim)) => Role::Follow(claim.0),
        (None, None) => {
            let lowest = friends.iter().all(|&id| me < id);
            if quiet >= QUIET && (lowest || quiet >= QUIET * GIVE_UP) {
                Role::Keep
            } else {
                Role::Wait
            }
        }
    }
}

struct Keeper {
    area: u8,
    me: Option<u8>,
    role: Role,
    /// Our term while we claim.
    term: Option<u32>,
    /// The highest term heard in this garden.
    top_term: u32,
    /// Claims heard: (id, term, when).
    claims: Vec<(u8, u32, Instant)>,
    /// Since when friends are here and nobody claims.
    quiet_since: Option<Instant>,
    frame: u32,
}

static KEEPER: Mutex<Keeper> = Mutex::new(Keeper {
    area: 0,
    me: None,
    role: Role::Alone,
    term: None,
    top_term: 0,
    claims: Vec::new(),
    quiet_since: None,
    frame: 0,
});

fn lock() -> std::sync::MutexGuard<'static, Keeper> {
    KEEPER.lock().unwrap_or_else(|e| e.into_inner())
}

/// Network: player `from` keeps garden `area` (term `term`).
pub fn received(from: u8, area: u8, term: u32) {
    let mut k = lock();
    if area == k.area && area != 0 {
        k.claims.retain(|&(id, _, _)| id != from);
        k.claims.push((from, term, Instant::now()));
        k.top_term = k.top_term.max(term);
    }
}

/// Who runs our garden's Chao, trees and toys: us (alone or keeping), a friend, or nobody yet.
pub fn current() -> Option<u8> {
    let k = lock();
    match k.role {
        Role::Alone | Role::Keep => k.me,
        Role::Follow(id) => Some(id),
        Role::Wait => None,
    }
}

/// Our player id (None offline).
pub fn me() -> Option<u8> {
    lock().me
}

/// Whether our game runs the garden (also when offline or alone).
pub fn keeping() -> bool {
    let k = lock();
    k.me.is_none() || matches!(k.role, Role::Alone | Role::Keep)
}

/// Call every frame (game thread): us, our garden (1..=3, or 0) and the friends in it whose
/// updates are coming. Returns our claim to send, now and then while we keep it.
pub fn on_frame(me: Option<u8>, area: u8, friends: &[u8]) -> Option<Body> {
    let mut k = lock();
    k.me = me;
    k.frame = k.frame.wrapping_add(1);
    if area != k.area {
        k.area = area;
        k.term = None;
        k.top_term = 0;
        k.claims.clear();
        k.quiet_since = None;
    }
    let Some(me) = me.filter(|_| area != 0) else {
        k.role = Role::Alone;
        k.term = None;
        return None;
    };
    let now = Instant::now();
    k.claims
        .retain(|&(id, _, at)| id != me && now.duration_since(at) < CLAIM_STALE);
    let claims: Vec<(u8, u32)> = k.claims.iter().map(|&(id, term, _)| (id, term)).collect();
    let nobody = k.term.is_none() && !claims.iter().any(|(id, _)| friends.contains(id));
    if friends.is_empty() || !nobody {
        k.quiet_since = None;
    } else if k.quiet_since.is_none() {
        k.quiet_since = Some(now);
    }
    let quiet = k
        .quiet_since
        .map_or(Duration::ZERO, |at| now.duration_since(at));
    let role = decide(me, friends, &claims, k.term, quiet);
    match role {
        Role::Keep if k.term.is_none() => {
            let term = k.top_term + 1;
            k.term = Some(term);
            k.top_term = term;
            crate::log!("keeper: keeping garden {area} (term {term})");
        }
        Role::Keep => {}
        _ => {
            if k.term.take().is_some() {
                crate::log!("keeper: no longer keeping garden {area} ({role:?})");
            }
        }
    }
    if role != k.role {
        crate::log!("keeper: garden {area}: {:?} -> {role:?}", k.role);
        k.role = role;
    }
    let term = k.term?;
    k.frame
        .is_multiple_of(CLAIM_EVERY)
        .then_some(Body::GardenKeeper { area, term })
}

#[cfg(test)]
mod tests {
    use super::*;

    const LONG: Duration = Duration::from_secs(10);

    #[test]
    fn alone_keeps_quietly() {
        assert_eq!(decide(5, &[], &[], None, LONG), Role::Alone);
    }

    #[test]
    fn newcomer_follows_the_keeper_there() {
        assert_eq!(
            decide(1, &[4], &[(4, 3)], None, Duration::ZERO),
            Role::Follow(4)
        );
    }

    #[test]
    fn lowest_takes_over_after_a_quiet_moment() {
        assert_eq!(decide(1, &[4], &[], None, Duration::ZERO), Role::Wait);
        assert_eq!(decide(1, &[4], &[], None, QUIET), Role::Keep);
        assert_eq!(decide(4, &[1], &[], None, QUIET), Role::Wait);
        assert_eq!(decide(4, &[1], &[], None, QUIET * GIVE_UP), Role::Keep);
    }

    #[test]
    fn higher_term_then_lower_id_wins() {
        assert_eq!(decide(1, &[4], &[(4, 2)], Some(1), LONG), Role::Follow(4));
        assert_eq!(decide(1, &[4], &[(4, 1)], Some(1), LONG), Role::Keep);
        assert_eq!(decide(4, &[1], &[(1, 1)], Some(1), LONG), Role::Follow(1));
    }

    #[test]
    fn claims_from_players_elsewhere_dont_count() {
        assert_eq!(decide(1, &[4], &[(7, 9)], Some(1), LONG), Role::Keep);
    }
}
