//! Co-op parties: enemies for everyone (re/stage-objects.md "Enemies and friends", "Enemy
//! owners").
//!
//! An enemy near more than one player is run by the game of the player nearest it (sticky:
//! another takes it over only when nearer by `OWNER_MARGIN`; between two claims, the nearer,
//! then the lower id; an owner not heard for `FRESH` counts as gone). The owner sends where it
//! is and faces (`EnemyStates`); the others' copy keeps running its own update but is put where
//! the owner has it each frame, so it's in the same place on every screen.
//!
//! Each game's enemies aim only at its own player. Turning them toward a friend's puppet (player
//! 1 swapped for a stand-in at the friend while the enemy's update ran) hung party stages
//! (the watchdog stopped inside that update), and loading enemies around friends
//! (the two-player loader pass) only fed it, so both are off until that's understood.
//!
//! Left out: the ghosts (E GHOST, E PATHGHOST). Missing: a copy's own update still picks its
//! motion and fires at us by itself (the enemies' state machines aren't frozen like the
//! bosses').

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use crate::net::protocol::{EnemyState, MAX_ENEMY_STATES};

/// Players' EntityData1 pointers (player 1 first).
const E_ROTATION: usize = 0x08;
const E_POSITION: usize = 0x14;
const OBJ_MAIN: usize = 0x10;
const OBJ_DATA1: usize = 0x34;
const DELETE_OBJECT: usize = 0x0046_F720;
/// E GHOST, E PATHGHOST.
const LEFT_OUT: [usize; 2] = [0x0050_3140, 0x0050_4390];
/// Enemies with another player this near are shared; their owner sends them this often.
const SHARE_NEAR: f32 = 600.0;
const SEND_EVERY: u32 = 6;
/// Nearer by this much takes an enemy over from its owner.
const OWNER_MARGIN: f32 = 50.0;
/// An owner's state counts this long.
const FRESH: Duration = Duration::from_millis(600);
/// How much of the gap to the owner's position closes a frame; farther than `SNAP`, it jumps.
const BLEND: f32 = 0.5;
const SNAP: f32 = 80.0;

/// An enemy whose MainSub ours runs.
struct Wrapped {
    /// Its own MainSub.
    main: usize,
    /// Where its owner has it.
    follow: Option<([f32; 3], [i32; 3])>,
}

struct Enemies {
    wrapped: HashMap<usize, Wrapped>,
    /// Owners heard: record -> (player, their state, when).
    owners: HashMap<u16, (u8, EnemyState, Instant)>,
    /// The stage those are for.
    stage: (i16, u8),
    outgoing: Option<(i16, u8, Vec<EnemyState>)>,
    frame: u32,
}

static ENEMIES: Mutex<Option<Enemies>> = Mutex::new(None);

fn with<R>(f: impl FnOnce(&mut Enemies) -> R) -> R {
    let mut e = ENEMIES.lock().unwrap_or_else(|e| e.into_inner());
    f(e.get_or_insert_with(|| Enemies {
        wrapped: HashMap::new(),
        owners: HashMap::new(),
        stage: (0, 0),
        outgoing: None,
        frame: 0,
    }))
}

fn distance(a: [f32; 3], b: [f32; 3]) -> f32 {
    ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2) + (a[2] - b[2]).powi(2)).sqrt()
}

fn our_main() -> usize {
    enemy_main as *const () as usize
}

/// Network: enemies a party player's game runs, in a stage.
pub fn received(from: u8, level: i16, mission: u8, enemies: Vec<EnemyState>) {
    let now = Instant::now();
    with(|e| {
        if (level, mission) != e.stage {
            return;
        }
        for s in enemies {
            e.owners.insert(s.record, (from, s, now));
        }
    });
}

/// Our owned enemies to send: (level, mission, states).
pub fn take_outgoing() -> Option<(i16, u8, Vec<EnemyState>)> {
    with(|e| e.outgoing.take())
}

/// Call every frame (game thread).
pub fn on_frame(connected: bool) {
    let active = crate::coop::active(connected) && crate::coop::stage_sync();
    let stage = (crate::game::current_level(), crate::game::mission());
    let me_id = crate::keeper::me();
    // Party players in this stage: (id, where).
    let players: Vec<(u8, [f32; 3])> = if active {
        crate::online::others()
            .into_iter()
            .filter(|&(id, ..)| crate::room::shares_room(id))
            .filter_map(|(id, _, s)| {
                s.filter(|s| (s.level, s.mission) == stage)
                    .map(|s| (id, s.position))
            })
            .collect()
    } else {
        Vec::new()
    };
    let me = crate::game::player_position().map(|(p, _)| [p.x, p.y, p.z]);
    with(|e| {
        e.frame = e.frame.wrapping_add(1);
        if e.stage != stage {
            e.stage = stage;
            e.owners.clear();
        }
    });
    let enemies = if active {
        crate::coop::live_enemies()
    } else {
        Vec::new()
    };
    with(|e| {
        let now = Instant::now();
        e.owners
            .retain(|_, (_, _, at)| now.duration_since(*at) < FRESH);
        // No longer loaded (deleted, or out of our stage): forget them; their objects are freed.
        // A dying one is still loaded: it gets its own MainSub back below.
        e.wrapped
            .retain(|object, _| enemies.iter().any(|&(o, ..)| o == *object));
        let send = e.frame.is_multiple_of(SEND_EVERY);
        let mut ours = Vec::new();
        for &(object, kind, alive, record) in &enemies {
            // SAFETY: a loaded enemy's object (from its SET record) and its Data1.
            let (main, at, rotation) = unsafe {
                let d1 = ((object + OBJ_DATA1) as *const usize).read();
                if d1 == 0 {
                    continue;
                }
                (
                    ((object + OBJ_MAIN) as *const usize).read(),
                    ((d1 + E_POSITION) as *const [f32; 3]).read_unaligned(),
                    ((d1 + E_ROTATION) as *const [i32; 3]).read_unaligned(),
                )
            };
            if main != our_main() {
                // New here, or the address came back as another object.
                e.wrapped.remove(&object);
            }
            let mine = me.map_or(f32::MAX, |p| distance(at, p));
            let usable = alive && !LEFT_OUT.contains(&kind) && main != DELETE_OBJECT;

            // Who runs it: shared when another player is near.
            let shared = usable && players.iter().any(|&(_, p)| distance(at, p) < SHARE_NEAR);
            let owner = e.owners.get(&record).copied();
            let follow = match (shared, owner, me_id) {
                (true, Some((id, s, _)), Some(me)) if id != me => {
                    let keep = s.distance + OWNER_MARGIN > mine
                        || (s.distance - mine).abs() < OWNER_MARGIN && id < me;
                    keep.then_some((s.position, s.rotation))
                }
                _ => None,
            };
            if shared && follow.is_none() && send {
                ours.push(EnemyState {
                    record,
                    position: at,
                    rotation,
                    distance: mine,
                });
            }

            let wanted = usable && follow.is_some();
            match e.wrapped.get_mut(&object) {
                Some(w) if wanted => w.follow = follow,
                None if wanted => {
                    e.wrapped.insert(object, Wrapped { main, follow });
                    // SAFETY: the live enemy's MainSub; ours calls it.
                    unsafe { ((object + OBJ_MAIN) as *mut usize).write(our_main()) };
                }
                Some(w) => {
                    // SAFETY: as above: its own MainSub back.
                    unsafe { ((object + OBJ_MAIN) as *mut usize).write(w.main) };
                    e.wrapped.remove(&object);
                }
                None => {}
            }
        }
        if send && !ours.is_empty() {
            ours.truncate(MAX_ENEMY_STATES);
            e.outgoing = Some((stage.0, stage.1, ours));
        }
    });
}

/// A followed enemy's MainSub: its own, then put where its owner has it.
extern "C" fn enemy_main(object: usize) {
    crate::watch::mark("enemy update (enemies.rs)");
    let call = with(|e| e.wrapped.get(&object).map(|w| (w.main, w.follow)));
    let Some((main, follow)) = call else {
        // Not ours any more (shouldn't happen): nothing safe to run; the next frame decides.
        return;
    };
    // SAFETY: the enemy's own MainSub, called as the game calls it.
    unsafe { std::mem::transmute::<usize, extern "C" fn(usize)>(main)(object) };
    // It may have moved on to another MainSub of its own (next state, or deleting itself):
    // that's the one ours runs from now on, and a deleted one is left alone.
    // SAFETY: the object is still allocated during its own update's frame (DeleteObject as its
    // MainSub frees it on the next pass).
    let now = unsafe { ((object + OBJ_MAIN) as *const usize).read() };
    if now == DELETE_OBJECT {
        with(|e| e.wrapped.remove(&object));
        return;
    }
    if let Some((position, rotation)) = follow {
        // SAFETY: the live enemy's Data1, on the game thread.
        unsafe {
            let d1 = ((object + OBJ_DATA1) as *const usize).read();
            if d1 != 0 {
                let here = ((d1 + E_POSITION) as *const [f32; 3]).read_unaligned();
                let k = if distance(here, position) > SNAP {
                    1.0
                } else {
                    BLEND
                };
                let at: [f32; 3] = std::array::from_fn(|i| here[i] + (position[i] - here[i]) * k);
                ((d1 + E_POSITION) as *mut [f32; 3]).write_unaligned(at);
                ((d1 + E_ROTATION) as *mut [i32; 3]).write_unaligned(rotation);
            }
        }
    }
    if now != our_main() {
        with(|e| {
            if let Some(w) = e.wrapped.get_mut(&object) {
                w.main = now;
                // SAFETY: as above.
                unsafe { ((object + OBJ_MAIN) as *mut usize).write(our_main()) };
            }
        });
    }
}
