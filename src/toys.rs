//! Shared garden: the ball and the fruit trees (re/chao-garden-sync.md "Garden toys and fruit
//! trees").
//!
//! The ball: whoever last kicked or held it owns it and sends where it is and how it's moving;
//! everyone else's ball follows. Ownership is a claim number that only goes up, so the newest
//! kick wins without asking anyone.
//!
//! Fruit trees: each game's trees drop ripe fruit on their own, and every dropped fruit is shared
//! as an item, so two players in a garden got twice the fruit. With friends there, only one
//! player's trees drop on their own (the lowest id); shaking still works for everyone.

use crate::game;
use crate::log;
use crate::net::protocol::{BALL_HELD, BALL_IN_WATER};
use std::sync::Mutex;

/// The garden's ball object (null outside a garden).
const BALL: usize = 0x01DB_E570;
/// Set while the ball is in the water.
const BALL_IN_WATER_FLAG: usize = 0x01DB_E56C;
const OBJ_MAIN: usize = 0x10;
const OBJ_NEXT: usize = 0x04;
const OBJ_DATA1: usize = 0x34;
const OBJ_DATA2: usize = 0x38;
const D_POSITION: usize = 0x14;
/// Data1 byte 0: 1 free or rolling, 2 being let go, 3 held by a Chao.
const BALL_FREE: u8 = 1;
/// The local player's CharObj2 pointer; +0x94 is the object they hold.
const PLAYER_CHAR_OBJ2: usize = 0x01DE_9600;
const PLAYER_HELD: usize = 0x94;

/// Sideways speed gained in one frame that the ball's own rolling never gives it: a kick.
/// (Bounces off walls and the ground keep or lose speed.)
const KICK: f32 = 0.5;
/// How close we must be for a sudden change to be our kick.
const KICK_REACH: f32 = 25.0;
/// Farther than this from where the owner has it: jump there.
const SNAP: f32 = 20.0;
/// How much of the gap to the owner's position closes per update.
const BLEND: f32 = 0.3;
const SEND_EVERY: u32 = 6;

const OBJECT_LIST_2: usize = 0x01A5_A254 + 2 * 4;
const TREE_MAIN: usize = 0x0054_6810;
/// Data1 +0x44: 3 fruit slots of 0x20, +0 ripeness. Above 0.9 a shake drops it; above 1.9 it
/// can drop by itself (0x546F42).
const TREE_SLOTS: usize = 0x44;
const TREE_SLOT_SIZE: usize = 0x20;
const SELF_DROP: f32 = 1.85;

/// The ball as its owner sends it: (area, claim, position, velocity).
pub type BallState = (u8, u16, [f32; 3], [f32; 3]);

struct Toys {
    area: u8,
    claim: u16,
    owner: bool,
    last_velocity: Option<[f32; 3]>,
    incoming: Option<(u8, BallState)>,
    frame: u32,
}

static TOYS: Mutex<Toys> = Mutex::new(Toys {
    area: 0,
    claim: 0,
    owner: false,
    last_velocity: None,
    incoming: None,
    frame: 0,
});

/// Network: a friend's ball.
pub fn received(from: u8, ball: BallState) {
    TOYS.lock().unwrap_or_else(|e| e.into_inner()).incoming = Some((from, ball));
}

fn ball() -> Option<(usize, usize, usize)> {
    // SAFETY: the game's ball pointer and its Data1/Data2, each checked for null.
    unsafe {
        let ball = (BALL as *const usize).read_volatile();
        if ball == 0 {
            return None;
        }
        let d1 = ((ball + OBJ_DATA1) as *const usize).read();
        let d2 = ((ball + OBJ_DATA2) as *const usize).read();
        (d1 != 0 && d2 != 0).then_some((ball, d1, d2))
    }
}

fn held_by_player(ball: usize) -> bool {
    // SAFETY: the player's CharObj2 pointer (null without a player).
    unsafe {
        let c = (PLAYER_CHAR_OBJ2 as *const usize).read_volatile();
        c != 0 && ((c + PLAYER_HELD) as *const usize).read() == ball
    }
}

/// Call every frame in Chao World with the ids of the others in our garden. Returns the ball
/// to send while we own it, with its `Ball::flags` (held by us, in the water).
pub fn on_frame(me: Option<u8>, area: u8, friends: &[u8]) -> Option<(BallState, u8)> {
    let mut t = TOYS.lock().unwrap_or_else(|e| e.into_inner());
    let incoming = t.incoming.take();
    let Some(me) = me.filter(|_| area != 0 && !friends.is_empty() && game::in_garden()) else {
        t.area = 0;
        t.owner = false;
        t.last_velocity = None;
        return None;
    };
    if t.area != area {
        t.area = area;
        t.claim = 0;
        t.owner = false;
        t.last_velocity = None;
    }
    // Only the lowest id's trees drop fruit by themselves.
    if friends.iter().any(|&f| f < me) {
        hold_fruit();
    }
    let (ball, d1, d2) = ball()?;
    // SAFETY: the live ball's Data1/Data2 (from `ball`), on the game thread.
    let (mode, position, mut velocity) = unsafe {
        (
            (d1 as *const u8).read(),
            ((d1 + D_POSITION) as *const [f32; 3]).read(),
            (d2 as *const [f32; 3]).read(),
        )
    };
    let held = held_by_player(ball);

    // Our kick: a sudden sideways speed-up near us that the last frame didn't explain.
    let kicked = t.last_velocity.is_some_and(|was| {
        let speed = |v: [f32; 3]| (v[0] * v[0] + v[2] * v[2]).sqrt();
        speed(velocity) - speed(was) > KICK
            && game::player_position().is_some_and(|(p, _)| {
                let (x, y, z) = (p.x - position[0], p.y - position[1], p.z - position[2]);
                x * x + y * y + z * z < KICK_REACH * KICK_REACH
            })
    });
    if (held || kicked) && !t.owner {
        t.claim = t.claim.wrapping_add(1);
        t.owner = true;
        log!("toys: we have the ball (claim {})", t.claim);
    }

    if let Some((from, (a, claim, their_position, their_velocity))) = incoming
        && a == area
    {
        let newer = claim > t.claim || (claim == t.claim && from < me);
        if newer && !held {
            if t.owner {
                log!("toys: friend {from} has the ball");
            }
            t.claim = claim;
            t.owner = false;
            if mode == BALL_FREE {
                let gap = [
                    their_position[0] - position[0],
                    their_position[1] - position[1],
                    their_position[2] - position[2],
                ];
                let far = gap.iter().map(|g| g * g).sum::<f32>() > SNAP * SNAP;
                let k = if far { 1.0 } else { BLEND };
                let at = [
                    position[0] + gap[0] * k,
                    position[1] + gap[1] * k,
                    position[2] + gap[2] * k,
                ];
                velocity = their_velocity;
                // SAFETY: as above.
                unsafe {
                    ((d1 + D_POSITION) as *mut [f32; 3]).write(at);
                    (d2 as *mut [f32; 3]).write(velocity);
                }
            }
        }
    }
    t.last_velocity = Some(velocity);

    t.frame = t.frame.wrapping_add(1);
    // SAFETY: the game's "ball in the water" word (re/chao-ai.md §2B), on the game thread.
    let in_water = unsafe { (BALL_IN_WATER_FLAG as *const u32).read() } != 0;
    let flags = if held { BALL_HELD } else { 0 } | if in_water { BALL_IN_WATER } else { 0 };
    (t.owner && t.frame.is_multiple_of(SEND_EVERY))
        .then_some(((area, t.claim, position, velocity), flags))
}

/// Keeps every tree's fruit just below dropping by itself; a shake still drops ripe fruit.
fn hold_fruit() {
    // SAFETY: the game's object list 2 (circular through +4) and each tree's Data1, on the
    // game thread.
    unsafe {
        let head = (OBJECT_LIST_2 as *const usize).read();
        let mut object = head;
        for _ in 0..4096 {
            if object == 0 {
                break;
            }
            if ((object + OBJ_MAIN) as *const usize).read() == TREE_MAIN {
                let d1 = ((object + OBJ_DATA1) as *const usize).read();
                if d1 != 0 {
                    for slot in 0..3 {
                        let ripe = (d1 + TREE_SLOTS + slot * TREE_SLOT_SIZE) as *mut f32;
                        if ripe.read() > SELF_DROP {
                            ripe.write(SELF_DROP);
                        }
                    }
                }
            }
            object = ((object + OBJ_NEXT) as *const usize).read();
            if object == head {
                break;
            }
        }
    }
}
