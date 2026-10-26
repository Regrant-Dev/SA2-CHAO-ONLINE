//! Friends' whistles in a shared garden (re/chao-garden-sync.md "Whistling").
//!
//! A garden Chao hears a whistle only from player 1: each frame its reactions check (0x53DB10)
//! asks 0x5637B0 whether player 1's action is 42 (whistling) within 100 units, and if so it may
//! start coming (behaviour 0x5634D0), walking to player 1 (0x796AB0). A friend's whistle is
//! their own game's, so our Chao (whose real AI runs here and whom they see as copies) never
//! hear it.
//!
//! So when a friend whistles near a Chao our game runs (ours, or a visitor while we keep the
//! garden: keeper.rs), that Chao is "called": for a while, while its
//! own MainSub runs (and only then), player 1 is a stand-in standing where the friend is, whistling
//! at first. It hears them, walks to them and goes back to its own business; the friends see it
//! come (they see our Chao as we move them).

use std::collections::HashMap;
use std::sync::Mutex;

use crate::game::Vector;

/// Players' EntityData1 pointers (player 1 first).
const PLAYERS: usize = 0x01DE_A6C0;
/// EntityData1: action (byte), position; how much of it the stand-in copies.
const E_ACTION: usize = 0x00;
const E_POSITION: usize = 0x14;
const ENTITY_SIZE: usize = 0x40;
const ACTION_WHISTLE: u8 = 42;
/// The whistle's reach (0x5637B0: squared distance under 10000).
const REACH: f32 = 100.0;

const OBJ_MAIN: usize = 0x10;
const OBJ_DATA1: usize = 0x34;
const D_POSITION: usize = 0x14;
/// Our own garden Chao's MainSub (copies of friends' Chao run another).
const CHAO_MAIN: usize = 0x0054_FE20;

/// How long a call lasts (frames), and how long the stand-in whistles at its start.
const CALL_FRAMES: u32 = 600;
const WHISTLE_FRAMES: u32 = 20;
/// Close enough: the call ends.
const ARRIVED: f32 = 12.0;

struct Call {
    /// The friend who whistled, and how long ago (frames).
    friend: u8,
    age: u32,
}

struct Whistles {
    /// Friends in our garden right now: id -> (position, action).
    friends: HashMap<u8, (Vector, u8)>,
    /// Our Chao objects answering a friend.
    calls: HashMap<usize, Call>,
    /// The stand-in player 1 (EntityData1-sized).
    stand_in: [u8; ENTITY_SIZE],
}

static WHISTLES: Mutex<Option<Whistles>> = Mutex::new(None);

fn with<R>(f: impl FnOnce(&mut Whistles) -> R) -> R {
    let mut g = WHISTLES.lock().unwrap_or_else(|e| e.into_inner());
    f(g.get_or_insert_with(|| Whistles {
        friends: HashMap::new(),
        calls: HashMap::new(),
        stand_in: [0; ENTITY_SIZE],
    }))
}

fn distance(a: Vector, b: Vector) -> f32 {
    ((a.x - b.x).powi(2) + (a.y - b.y).powi(2) + (a.z - b.z).powi(2)).sqrt()
}

/// Call every frame (game thread): the friends in our garden (id, position, action), or none
/// when we're not in one.
pub fn on_frame(friends: &[(u8, Vector, u8)]) {
    let chao = our_chao();
    with(|w| {
        w.friends = friends.iter().map(|&(id, p, a)| (id, (p, a))).collect();
        // Calls end: their Chao went, the friend went, the time's up, or it got there.
        let ended: Vec<usize> = w
            .calls
            .iter()
            .filter(|&(object, call)| {
                let here = chao.iter().find(|&&(o, _)| o == *object);
                let friend = w.friends.get(&call.friend);
                match (here, friend) {
                    (Some(&(_, at)), Some(&(p, _))) => {
                        call.age >= CALL_FRAMES
                            || (call.age > WHISTLE_FRAMES && distance(at, p) < ARRIVED)
                    }
                    _ => true,
                }
            })
            .map(|(&o, _)| o)
            .collect();
        for object in ended {
            w.calls.remove(&object);
            // SAFETY: a live Chao (checked by `our_chao` while it's listed) back to its own
            // MainSub, unless something else runs it by now (it follows a keeper, say); a gone
            // one isn't touched.
            if chao.iter().any(|&(o, _)| o == object) {
                unsafe {
                    let main = (object + OBJ_MAIN) as *mut usize;
                    if main.read() == called_main as *const () as usize {
                        main.write(CHAO_MAIN);
                    }
                }
            }
        }
        for call in w.calls.values_mut() {
            call.age += 1;
        }
        // New whistles: our Chao in reach of a whistling friend are called.
        let whistling: Vec<(u8, Vector)> = w
            .friends
            .iter()
            .filter(|&(_, &(_, action))| action == ACTION_WHISTLE)
            .map(|(&id, &(p, _))| (id, p))
            .collect();
        for &(object, at) in &chao {
            if w.calls.contains_key(&object) {
                continue;
            }
            if let Some(&(friend, _)) = whistling.iter().find(|&&(_, p)| distance(at, p) < REACH) {
                w.calls.insert(object, Call { friend, age: 0 });
                // SAFETY: a live Chao of ours running its own MainSub.
                unsafe {
                    ((object + OBJ_MAIN) as *mut usize).write(called_main as *const () as usize)
                };
            }
        }
    });
}

/// Our own Chao in the garden (live, running their own MainSub or ours): (object, position).
fn our_chao() -> Vec<(usize, Vector)> {
    let mut out = Vec::new();
    // SAFETY: registry entries' live objects; Data1 is checked for null.
    unsafe {
        for (_, object) in crate::registry::entries(crate::registry::CHAO) {
            let main = ((object + OBJ_MAIN) as *const usize).read();
            let data1 = ((object + OBJ_DATA1) as *const usize).read();
            if data1 != 0 && (main == CHAO_MAIN || main == called_main as *const () as usize) {
                out.push((
                    object,
                    ((data1 + D_POSITION) as *const Vector).read_unaligned(),
                ));
            }
        }
    }
    out
}

/// A called Chao's MainSub: its own, with player 1 standing where the friend is.
extern "C" fn called_main(object: usize) {
    let target = with(|w| {
        let call = w.calls.get(&object)?;
        let &(at, _) = w.friends.get(&call.friend)?;
        // SAFETY: player 1's live EntityData1 (when there is one), copied into the stand-in.
        let player = unsafe { (PLAYERS as *const usize).read() };
        if player == 0 {
            return None;
        }
        unsafe {
            std::ptr::copy_nonoverlapping(
                player as *const u8,
                w.stand_in.as_mut_ptr(),
                ENTITY_SIZE,
            );
        }
        w.stand_in[E_ACTION] = if call.age < WHISTLE_FRAMES {
            ACTION_WHISTLE
        } else {
            0
        };
        w.stand_in[E_POSITION..E_POSITION + 12]
            .copy_from_slice(&[at.x, at.y, at.z].map(f32::to_le_bytes).concat());
        Some((player, w.stand_in.as_ptr() as usize))
    });
    let main = || {
        // SAFETY: the Chao's own MainSub, called as the game calls it.
        unsafe { std::mem::transmute::<usize, extern "C" fn(usize)>(CHAO_MAIN)(object) }
    };
    match target {
        Some((player, stand_in)) => {
            // SAFETY: player 1's pointer, put back right after the Chao's own update; the stand-in
            // lives in a static.
            unsafe {
                (PLAYERS as *mut usize).write(stand_in);
                main();
                (PLAYERS as *mut usize).write(player);
            }
        }
        None => main(),
    }
}
