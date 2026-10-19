//! Co-op parties: one boss for everyone (re/bosses.md "GroboLib bosses", "Choosing the boss
//! leader", "Boss following").
//!
//! A boss's MainSub turns its head, runs the function for its mode (`Data1[0]`: these move it,
//! pick its motion and fire its attacks), plays its motion, takes hits (its hit code is inlined)
//! and keeps its collision. Run with a mode its switch has no case for, it does all of that but
//! the mode function: then it's "frozen", and something else decides where it is.
//!
//! - **Leader:** one room player's game leads (keeper.rs's election among the room players
//!   fighting this boss in this stage and mission, each saying so with `BossClaim` while it
//!   does: a player elsewhere, paused or still loading never counts; a claim with a term,
//!   sticky; after a quiet moment the lowest id claims). Alone, we lead: the game as usual.
//!   With nobody leading, every game runs its own boss.
//! - **Who it's after:** the leader's boss is after the leader (that game's player 1). Running
//!   its mode functions with a stand-in player 1 at a friend wasn't provably safe
//!   (re/stage-objects.md "Stage hook safety audit"), so friends aren't its target.
//! - **Followers:** ours is frozen and takes the leader's state (`BossState`): mode bytes, flags,
//!   timer, position (eased), facing and motion.
//! - **Hits:** hits stay real in each game (HP shared by bosses.rs). A hit on a frozen boss in a
//!   mode where its own code ignores hits is undone. A follower's hit is sent (`BossHit`) and
//!   the leader's boss flinches as its own hit code would make it. A follower's last hit makes
//!   ours die here: from then on it runs its own.
//! - **Handoff:** when the leader stops claiming (left, dropped, restarted, paused), the next one
//!   claims and its boss carries on in the mode it was following, from that mode's first step
//!   (which makes what the mode needs).
//!
//! - **Attacks:** the leader sends the attacks its boss has in flight (`BossAttacks`: missiles,
//!   energy-gun beams, bombs and pillars, smoke, flames), each with the player it was fired
//!   at (the leader). A follower draws them on objects of its own (the attack's own display,
//!   the leader's Data1 bytes, eased between updates): nothing of them can hit.
//!
//! Hot Shot, Big Foot and Flying Dog (whose main also runs its engine flames itself; they're
//! its body's, wherever the leader has it, so they aren't sent). Missing: the boss attacking
//! anyone but the leader, and the head turn (each game's own player 1).

use std::sync::Mutex;
use std::time::{Duration, Instant};

use crate::keeper::{Role, decide};
use crate::net::protocol::{BOSS_ATTACK_DATA, Body, BossAttack, BossState, MAX_BOSS_ATTACKS};

const OBJECT_LIST_2: usize = 0x01A5_A254 + 2 * 4;
const OBJ_NEXT: usize = 0x04;
const OBJ_MAIN: usize = 0x10;
const OBJ_DATA1: usize = 0x34;
const OBJ_DATA2: usize = 0x40;
/// Data1: +0 mode, +1 sub-mode, +3 counter, +4 flags, +6 timer, +8 rotation, +0x14 position.
const D_MODE: usize = 0x00;
const D_SUB: usize = 0x01;
const D_COUNTER: usize = 0x03;
const D_FLAGS: usize = 0x04;
const D_TIMER: usize = 0x06;
const D_ROTATION: usize = 0x08;
const D_POSITION: usize = 0x14;
/// Data2 (+0x40): +0xC the motion controller (+4 motion now, +8 next, +0x2C current motion
/// data, +0x48 table), +0x28 HP.
const W_MOTION: usize = 0x0C;
const W_HP: usize = 0x28;
const MC_NOW: usize = 0x04;
const MC_NEXT: usize = 0x08;
const MC_MOTION: usize = 0x2C;
const MC_TABLE: usize = 0x48;
const MOTION_ENTRY_SIZE: usize = 0x1C;
const MAX_MOTION_ID: u16 = 0x100;
/// Starts a motion: usercall EAX = controller, EDX = id.
const START_MOTION: usize = 0x0079_3C40;
/// Flags: 1 guarding (a hit then takes the guarded hurt mode), 4 still flashing from a hit
/// (hits don't count).
const F_GUARD: u16 = 0x1;
const F_FLASH: u16 = 0x4;
/// The mode the intro hands over to (0x5C7A60 ends with mode 2, sub-mode 0).
const FIGHT_START: u8 = 2;

/// The GroboLib attacks a follower draws when they're aimed at someone else: (display slot,
/// display). Each display reads only its object's Data1 below its collision pointer
/// (re/bosses.md "Attacks drawn on followers"), so a copy of ours with the leader's Data1 bytes
/// draws it, and having no update or collision of the game's, it never hits anything.
const ATTACKS: [(usize, usize); 6] = [
    (0x14, 0x005D_6AA0), // missile (main 0x5D6A30)
    (0x1C, 0x005D_9890), // energy-gun beam (0x5D9A70)
    (0x14, 0x005D_9CA0), // energy-gun bomb (0x5D9BA0)
    (0x14, 0x005D_A2D0), // energy-gun pillar (0x5DA280)
    (0x1C, 0x005D_6BA0), // missile smoke (0x525A70)
    (0x1C, 0x005D_5730), // flame (0x5D56B0)
];
/// Flames: Flying Dog's main makes its own every frame (they're its body's), so not those.
const FLAME: u8 = 5;
const FLYING_DOG: usize = 0x005D_35E0;
const OBJECT_LIST_3: usize = 0x01A5_A254 + 3 * 4;
const OBJ_CHILD: usize = 0x0C;
const OBJ_SIBLING: usize = 0x00;
const OBJ_DELETE: usize = 0x18;
const DELETE_OBJECT: usize = 0x0046_F720;
const DATA1_SIZE: usize = 0x30;
const ATTACK_LIST: u32 = 3;
const ATTACK_NAME: &[u8] = b"SA2ChaoOnlineBossAttack\0";
/// The leader sends its attacks this often (frames); a copy moves on by itself in between.
const ATTACKS_EVERY: u32 = 6;

/// How each followed boss's main (and its inlined hit code) uses its modes.
struct Kind {
    main: usize,
    /// A mode its switch has no case for.
    frozen: u8,
    /// The intro: its first step makes the gauge and parts and places player 1.
    intro: u8,
    /// Hits take no HP in these; it dies in these.
    no_hit: [u8; 2],
    dying: [u8; 2],
    /// A hit's mode: guarded, else the hurt one.
    guarded: u8,
    hurt: u8,
    /// A hit in this mode first tidies up (0x5D7B50, Hot Shot's energy gun): not mirrored, so a
    /// friend's hit then doesn't flinch it.
    tidy: Option<u8>,
}

/// Hot Shot (main 0x5CB2D0), Big Foot (0x5CFE40) and Flying Dog (0x5D35E0), from their mains.
const KINDS: [Kind; 3] = [
    Kind {
        main: 0x005C_B2D0,
        frozen: 7,
        intro: 1,
        no_hit: [10, 12],
        dying: [12, 15],
        guarded: 10,
        hurt: 14,
        tidy: Some(9),
    },
    Kind {
        main: 0x005C_FE40,
        frozen: 7,
        intro: 1,
        no_hit: [9, 10],
        dying: [10, 13],
        guarded: 9,
        hurt: 12,
        tidy: None,
    },
    // Its main also runs its engine flames itself (0x5D1320, every frame but while dying), so
    // a frozen Flying Dog keeps them: they're part of its body, wherever the leader has it.
    // A hit always takes mode 10 (it has no guarded one).
    Kind {
        main: 0x005D_35E0,
        frozen: 7,
        intro: 1,
        no_hit: [10, 11],
        dying: [11, 11],
        guarded: 10,
        hurt: 10,
        tidy: None,
    },
];

impl Kind {
    /// Its own hit code ignores hits in this mode (or it's dying, or not fighting yet).
    fn ignores_hits(&self, mode: u8) -> bool {
        mode == 0 || mode == self.intro || self.no_hit.contains(&mode) || self.dying.contains(&mode)
    }
}

/// A claim counts this long after it's heard; with fighters here and no claim this long, the
/// lowest id claims (anyone after 3 times as long, keeper.rs).
const FRESH: Duration = Duration::from_millis(1500);
const CLAIM_EVERY: u32 = 15;
const STATE_EVERY: u32 = 3;
/// How much of the gap to the leader's position closes a frame; farther than `SNAP`, it jumps.
const BLEND: f32 = 0.3;
const SNAP: f32 = 60.0;

struct Follow {
    stage: (i16, u8),
    /// The boss object and its own MainSub (ours runs it), and whether it passed its intro here
    /// (only then is it led or followed).
    boss: Option<(usize, usize)>,
    ready: bool,
    /// Died here from a last hit of ours: it runs its own from then on.
    released: bool,
    /// Fighters heard: id -> (term, when).
    claims: Vec<(u8, u32, Instant)>,
    term: Option<u32>,
    top_term: u32,
    quiet_since: Option<Instant>,
    role: Role,
    /// Leading: who the boss is after, where they are, the mode it was in, and turns given.
    target: u8,
    /// Following: the leader's state, when it came, and whether ours has taken it yet.
    state: Option<(u8, BossState, Instant)>,
    following: bool,
    hits: u32,
    outgoing: Vec<(Body, bool)>,
    frame: u32,
    /// Leading: our attacks in flight (object, our number, who they were fired at).
    sent_attacks: Vec<(usize, u16, u8)>,
    serial: u16,
    /// Following: the leader's latest attacks, and our drawn copies of them.
    attacks_in: Option<Vec<BossAttack>>,
    copies: Vec<Copy>,
}

/// A drawn copy of one of the leader's attacks: its number, our object, and how far it moves
/// a frame until the next update.
struct Copy {
    serial: u16,
    object: usize,
    step: [f32; 3],
}

impl Follow {
    const fn new() -> Self {
        Follow {
            stage: (0, 0),
            boss: None,
            ready: false,
            released: false,
            claims: Vec::new(),
            term: None,
            top_term: 0,
            quiet_since: None,
            role: Role::Alone,
            target: 0,
            state: None,
            following: false,
            hits: 0,
            outgoing: Vec::new(),
            frame: 0,
            sent_attacks: Vec::new(),
            serial: 0,
            attacks_in: None,
            copies: Vec::new(),
        }
    }
}

static FOLLOW: Mutex<Follow> = Mutex::new(Follow::new());

fn lock() -> std::sync::MutexGuard<'static, Follow> {
    FOLLOW.lock().unwrap_or_else(|e| e.into_inner())
}

/// Network: a room player's boss message.
pub fn received(from: u8, body: Body) {
    let mut f = lock();
    let here = |level: i16, mission: u8| (level, mission) == f.stage;
    match body {
        Body::BossClaim {
            level,
            mission,
            term,
        } if here(level, mission) => {
            f.claims.retain(|&(id, ..)| id != from);
            f.claims.push((from, term, Instant::now()));
            f.top_term = f.top_term.max(term);
        }
        Body::BossState(s) if here(s.level, s.mission) => {
            f.state = Some((from, s, Instant::now()));
        }
        Body::BossHit { level, mission } if here(level, mission) => f.hits += 1,
        Body::BossAttacks {
            level,
            mission,
            attacks,
        } if here(level, mission) => f.attacks_in = Some(attacks),
        _ => {}
    }
}

/// Our boss messages to send: (body, reliable).
pub fn take_outgoing() -> Vec<(Body, bool)> {
    std::mem::take(&mut lock().outgoing)
}

fn kind(main: usize) -> Option<&'static Kind> {
    KINDS.iter().find(|k| k.main == main)
}

fn our_main() -> usize {
    boss_main as *const () as usize
}

/// This stage's followed boss in object list 2: (object, its MainSub now).
fn find_boss() -> Option<(usize, usize)> {
    let ours = our_main();
    // SAFETY: the game's object list 2 (circular through +4), read on the game thread.
    unsafe {
        let head = (OBJECT_LIST_2 as *const usize).read();
        let mut object = head;
        for _ in 0..4096 {
            if object == 0 {
                return None;
            }
            let main = ((object + OBJ_MAIN) as *const usize).read();
            if main == ours || kind(main).is_some() {
                return Some((object, main));
            }
            object = ((object + OBJ_NEXT) as *const usize).read();
            if object == head {
                return None;
            }
        }
    }
    None
}

/// Call every frame (game thread).
pub fn on_frame(connected: bool) {
    let mut f = lock();
    let stage = (crate::game::current_level(), crate::game::mission());
    if stage != f.stage {
        let outgoing = std::mem::take(&mut f.outgoing);
        *f = Follow {
            stage,
            outgoing,
            ..Follow::new()
        };
    }
    f.frame = f.frame.wrapping_add(1);
    let me = crate::keeper::me();
    let active = crate::coop::active(connected) && crate::coop::stage_sync() && me.is_some();

    // The boss, looked up in its list every frame (it goes with the stage, and a restart makes
    // a new one), so a gone one is never touched.
    let ours = our_main();
    let found = find_boss().and_then(|(object, main)| match f.boss {
        Some((o, own)) if o == object && main == ours => Some((object, own)),
        _ if main != ours => Some((object, main)),
        _ => None,
    });
    if found.map(|(o, _)| o) != f.boss.map(|(o, _)| o) {
        f.ready = false;
        f.released = false;
        f.state = None;
    }
    f.boss = found;
    let Some((object, main)) = f.boss else {
        f.term = None;
        f.role = Role::Alone;
        return;
    };
    let Some(kind) = kind(main) else {
        return;
    };
    let mode = boss_mode(object).unwrap_or(0);
    if mode != kind.intro && mode != 0 && mode != kind.frozen {
        f.ready = true;
    }
    let fighting = active && f.ready && !f.released;

    // Who leads: keeper.rs's rules among the fighters heard lately.
    let now = Instant::now();
    f.claims
        .retain(|&(_, _, at)| now.duration_since(at) < FRESH);
    let role = match me.filter(|_| fighting) {
        Some(me) => {
            let friends: Vec<u8> = f
                .claims
                .iter()
                .map(|&(id, ..)| id)
                .filter(|&id| id != me)
                .collect();
            let claims: Vec<(u8, u32)> = f
                .claims
                .iter()
                .filter(|&&(id, term, _)| id != me && term > 0)
                .map(|&(id, term, _)| (id, term))
                .collect();
            if friends.is_empty() || !claims.is_empty() || f.term.is_some() {
                f.quiet_since = None;
            } else if f.quiet_since.is_none() {
                f.quiet_since = Some(now);
            }
            let quiet = f
                .quiet_since
                .map_or(Duration::ZERO, |at| now.duration_since(at));
            decide(me, &friends, &claims, f.term, quiet)
        }
        None => Role::Alone,
    };
    match role {
        Role::Keep if f.term.is_none() => {
            let term = f.top_term + 1;
            f.term = Some(term);
            f.top_term = term;
            crate::log!("boss: we lead the boss (term {term})");
        }
        Role::Keep => {}
        _ => {
            if f.term.take().is_some() {
                crate::log!("boss: we no longer lead the boss ({role:?})");
            }
        }
    }
    if role != f.role {
        crate::log!("boss: {:?} -> {role:?}", f.role);
        f.role = role;
    }

    // The leader's boss is after the leader (its game's player 1): running its mode functions
    // with a stand-in player 1 at a friend wasn't provably safe (re/stage-objects.md "Stage hook
    // safety audit"), so friends see its attacks drawn but aren't its target.
    let me_id = me.unwrap_or(0);
    f.target = me_id;

    // Our MainSub wraps the boss's while it leads after a friend, or follows a fresh leader.
    let follow = match role {
        Role::Follow(leader) => f.state.is_some_and(|(from, s, at)| {
            from == leader && now.duration_since(at) < FRESH && !kind.dying.contains(&s.mode)
        }),
        _ => false,
    };
    let wrap = follow;
    // SAFETY: the live boss's MainSub; ours calls its own.
    unsafe {
        let at = (object + OBJ_MAIN) as *mut usize;
        if wrap && at.read() == main {
            at.write(ours);
        } else if !wrap && at.read() == ours {
            at.write(main);
        }
    }
    // Done following (we lead now, or nobody does): ours carries on from what it followed.
    if std::mem::replace(&mut f.following, follow) && !follow {
        resume(object, kind);
    }

    // What we say.
    if fighting && f.frame.is_multiple_of(CLAIM_EVERY) {
        let (level, mission) = stage;
        let term = f.term.unwrap_or(0);
        f.outgoing.push((
            Body::BossClaim {
                level,
                mission,
                term,
            },
            false,
        ));
    }
    // Attacks: ours are sent while we lead; a follower draws the leader's aimed at others.
    if f.term.is_some() && f.frame.is_multiple_of(ATTACKS_EVERY) {
        let stage = f.stage;
        if let Some(body) = leader_attacks(&mut f, object, main, stage) {
            f.outgoing.push((body, false));
        }
    }
    if follow {
        if let Some(attacks) = f.attacks_in.take() {
            draw_attacks(&mut f, attacks, me_id);
        }
    } else {
        f.attacks_in = None;
        remove_copies(&mut f);
    }

    if f.term.is_some() {
        // Friends' hits: ours flinches for them.
        for _ in 0..std::mem::take(&mut f.hits) {
            flinch(object, kind);
        }
        if f.frame.is_multiple_of(STATE_EVERY)
            && let Some(state) = read_state(object, stage, f.target)
        {
            f.outgoing.push((Body::BossState(state), false));
        }
    } else {
        f.hits = 0;
    }
}

/// Our boss's attacks in flight now: (object, `ATTACKS` index). They're in object list 3 or
/// made under the boss (two levels down).
fn attack_objects(boss: usize, main: usize) -> Vec<(usize, u8)> {
    let kind_of = |object: usize| -> Option<u8> {
        ATTACKS
            .iter()
            .position(|&(slot, display)| {
                // SAFETY: a live object's display slot.
                unsafe { ((object + slot) as *const usize).read() == display }
            })
            .map(|k| k as u8)
            .filter(|&k| !(k == FLAME && main == FLYING_DOG))
    };
    // SAFETY: a circular object list from `first` through `next`, read on the game thread.
    let walk = |first: usize, next: usize, out: &mut Vec<usize>| unsafe {
        let mut object = first;
        for _ in 0..512 {
            if object == 0 {
                break;
            }
            out.push(object);
            object = ((object + next) as *const usize).read();
            if object == first {
                break;
            }
        }
    };
    let mut found = Vec::new();
    // SAFETY: the game's object list 3 head and the boss's (and its children's) first child.
    unsafe {
        walk((OBJECT_LIST_3 as *const usize).read(), OBJ_NEXT, &mut found);
        let mut children = Vec::new();
        walk(
            ((boss + OBJ_CHILD) as *const usize).read(),
            OBJ_SIBLING,
            &mut children,
        );
        for child in children.clone() {
            walk(
                ((child + OBJ_CHILD) as *const usize).read(),
                OBJ_SIBLING,
                &mut children,
            );
        }
        found.extend(children);
    }
    found
        .into_iter()
        .filter_map(|object| kind_of(object).map(|k| (object, k)))
        .collect()
}

/// Leading: our attacks in flight to send (with whom each was fired at), or None.
fn leader_attacks(f: &mut Follow, boss: usize, main: usize, stage: (i16, u8)) -> Option<Body> {
    let target = f.target;
    let mut kept = Vec::new();
    let mut attacks = Vec::new();
    for (object, kind) in attack_objects(boss, main)
        .into_iter()
        .take(MAX_BOSS_ATTACKS)
    {
        let (serial, at) = match f.sent_attacks.iter().find(|&&(o, ..)| o == object) {
            Some(&(_, serial, at)) => (serial, at),
            None => {
                f.serial = f.serial.wrapping_add(1);
                (f.serial, target)
            }
        };
        kept.push((object, serial, at));
        let d = data1(object);
        if d == 0 {
            continue;
        }
        let mut data = [0u8; BOSS_ATTACK_DATA];
        // SAFETY: the live attack's Data1, at least DATA1_SIZE bytes.
        unsafe { std::ptr::copy_nonoverlapping(d as *const u8, data.as_mut_ptr(), data.len()) };
        attacks.push(BossAttack {
            serial,
            kind,
            target: at,
            data,
        });
    }
    let had = !f.sent_attacks.is_empty();
    f.sent_attacks = kept;
    (had || !attacks.is_empty()).then_some(Body::BossAttacks {
        level: stage.0,
        mission: stage.1,
        attacks,
    })
}

fn attack_position(data: &[u8; BOSS_ATTACK_DATA]) -> [f32; 3] {
    std::array::from_fn(|i| {
        let at = D_POSITION + i * 4;
        f32::from_le_bytes(data[at..at + 4].try_into().unwrap())
    })
}

/// Following: our copies of the leader's attacks aimed at someone else, as the leader has them
/// now; gone ones go. A copy keeps its own position and eases to the leader's by the next
/// update.
fn draw_attacks(f: &mut Follow, attacks: Vec<BossAttack>, me: u8) {
    let wanted: Vec<&BossAttack> = attacks
        .iter()
        .filter(|a| a.target != me && usize::from(a.kind) < ATTACKS.len())
        .collect();
    let gone: Vec<usize> = f
        .copies
        .iter()
        .filter(|c| !wanted.iter().any(|a| a.serial == c.serial))
        .map(|c| c.object)
        .collect();
    for object in gone {
        remove_copy(f, object);
    }
    for a in wanted {
        let to = attack_position(&a.data);
        let (object, from) = match f.copies.iter_mut().find(|c| c.serial == a.serial) {
            Some(c) => {
                let d = data1(c.object);
                // SAFETY: our copy's Data1 (ours, DATA1_SIZE bytes).
                let here = (d != 0)
                    .then(|| unsafe { ((d + D_POSITION) as *const [f32; 3]).read_unaligned() });
                if let Some(here) = here {
                    c.step = std::array::from_fn(|i| (to[i] - here[i]) / ATTACKS_EVERY as f32);
                }
                (c.object, here)
            }
            None => {
                let Some(object) = new_copy(a.kind) else {
                    continue;
                };
                f.copies.push(Copy {
                    serial: a.serial,
                    object,
                    step: [0.0; 3],
                });
                (object, None)
            }
        };
        let d = data1(object);
        if d != 0 {
            // SAFETY: our copy's Data1; its collision pointer (+0x2C) stays 0.
            unsafe {
                std::ptr::copy_nonoverlapping(a.data.as_ptr(), d as *mut u8, a.data.len());
                if let Some(here) = from {
                    ((d + D_POSITION) as *mut [f32; 3]).write_unaligned(here);
                }
            }
        }
    }
}

/// A copy of attack `kind`: an object of ours with the attack's display and our update.
fn new_copy(kind: u8) -> Option<usize> {
    let (slot, display) = *ATTACKS.get(usize::from(kind))?;
    // SAFETY: the game's object loader and allocator on the game thread; the Data1 block is the
    // game's own (with its header), so freeing the object frees it the game's way.
    unsafe {
        let data1 = crate::lockon::game_alloc(DATA1_SIZE);
        if data1 == 0 {
            return None;
        }
        let object = crate::lockon::load_object(
            ATTACK_LIST,
            ATTACK_NAME.as_ptr(),
            copy_main as *const () as usize,
        );
        if object == 0 {
            return None;
        }
        ((object + OBJ_DATA1) as *mut usize).write(data1);
        ((object + OBJ_DELETE) as *mut usize).write(copy_deleted as *const () as usize);
        ((object + slot) as *mut usize).write(display);
        Some(object)
    }
}

fn remove_copy(f: &mut Follow, object: usize) {
    if let Some(i) = f.copies.iter().position(|c| c.object == object) {
        f.copies.swap_remove(i);
        // SAFETY: our live copy (listed until its delete callback runs).
        unsafe { ((object + OBJ_MAIN) as *mut usize).write(DELETE_OBJECT) };
    }
}

fn remove_copies(f: &mut Follow) {
    let all: Vec<usize> = f.copies.iter().map(|c| c.object).collect();
    for object in all {
        remove_copy(f, object);
    }
}

/// A copy's update: it moves on as it did between the leader's last two updates.
extern "C" fn copy_main(object: usize) {
    crate::watch::mark("boss attack copy (boss_follow.rs)");
    let f = lock();
    let Some(c) = f.copies.iter().find(|c| c.object == object) else {
        return;
    };
    let d = data1(object);
    if d != 0 {
        // SAFETY: our copy's Data1.
        unsafe {
            let p = ((d + D_POSITION) as *const [f32; 3]).read_unaligned();
            let next: [f32; 3] = std::array::from_fn(|i| p[i] + c.step[i]);
            ((d + D_POSITION) as *mut [f32; 3]).write_unaligned(next);
        }
    }
}

/// Delete callback: the game frees one of our copies (we removed it, or the stage ended).
extern "C" fn copy_deleted(object: usize) {
    crate::watch::mark("boss attack copy deleted (boss_follow.rs)");
    lock().copies.retain(|c| c.object != object);
}

/// Our boss runs its own again after following (we lead now, or nobody does): it carries on
/// in the mode it was following, from that mode's first step (which makes what the mode needs);
/// from the fight's first mode if it was following the intro.
fn resume(object: usize, kind: &Kind) {
    let mode = boss_mode(object).unwrap_or(0);
    let mode =
        if mode == 0 || mode == kind.intro || mode == kind.frozen || kind.dying.contains(&mode) {
            FIGHT_START
        } else {
            mode
        };
    set_mode(object, mode, 0, 0);
    crate::log!("boss: ours runs its own again (mode {mode})");
}

fn data1(object: usize) -> usize {
    // SAFETY: the live boss object's Data1 pointer.
    unsafe { ((object + OBJ_DATA1) as *const usize).read() }
}

fn data2(object: usize) -> usize {
    // SAFETY: the live boss object's Data2 (+0x40) pointer.
    unsafe { ((object + OBJ_DATA2) as *const usize).read() }
}

fn boss_mode(object: usize) -> Option<u8> {
    let d = data1(object);
    // SAFETY: the boss's Data1 (checked for null).
    (d != 0).then(|| unsafe { ((d + D_MODE) as *const u8).read() })
}

fn set_mode(object: usize, mode: u8, sub: u8, timer: u16) {
    let d = data1(object);
    if d == 0 {
        return;
    }
    // SAFETY: the boss's Data1, on the game thread.
    unsafe {
        ((d + D_MODE) as *mut u8).write(mode);
        ((d + D_SUB) as *mut u8).write(sub);
        ((d + D_COUNTER) as *mut u8).write(0);
        ((d + D_TIMER) as *mut u16).write_unaligned(timer);
    }
}

fn controller(object: usize) -> usize {
    let w = data2(object);
    if w == 0 {
        return 0;
    }
    // SAFETY: the boss's Data2 and its motion controller pointer.
    unsafe { ((w + W_MOTION) as *const usize).read() }
}

fn hp_slot(object: usize) -> Option<*mut f32> {
    let w = data2(object);
    (w != 0).then_some((w + W_HP) as *mut f32)
}

fn read_state(object: usize, (level, mission): (i16, u8), target: u8) -> Option<BossState> {
    let (d, mc) = (data1(object), controller(object));
    if d == 0 || mc == 0 {
        return None;
    }
    // SAFETY: the live boss's Data1 and motion controller.
    unsafe {
        Some(BossState {
            level,
            mission,
            mode: ((d + D_MODE) as *const u8).read(),
            sub: ((d + D_SUB) as *const u8).read(),
            counter: ((d + D_COUNTER) as *const u8).read(),
            flags: ((d + D_FLAGS) as *const u16).read_unaligned(),
            timer: ((d + D_TIMER) as *const u16).read_unaligned(),
            position: ((d + D_POSITION) as *const [f32; 3]).read_unaligned(),
            rotation: ((d + D_ROTATION) as *const [i32; 3]).read_unaligned(),
            motion: ((mc + MC_NOW) as *const u32).read() as u16,
            target,
        })
    }
}

/// A friend hit our (leading) boss: it flinches as its own hit code would make it (the HP
/// drop comes through bosses.rs). Not while a hit wouldn't count here.
fn flinch(object: usize, kind: &Kind) {
    let d = data1(object);
    // SAFETY: the boss's Data2 HP (from `hp_slot`).
    let hp = hp_slot(object).map_or(0.0, |at| unsafe { at.read() });
    if d == 0 || hp <= 0.0 {
        return;
    }
    // SAFETY: the boss's Data1.
    let (mode, flags) = unsafe {
        (
            ((d + D_MODE) as *const u8).read(),
            ((d + D_FLAGS) as *const u16).read_unaligned(),
        )
    };
    if kind.ignores_hits(mode) || kind.tidy == Some(mode) || flags & F_FLASH != 0 {
        return;
    }
    let to = if flags & F_GUARD != 0 {
        kind.guarded
    } else {
        kind.hurt
    };
    set_mode(object, to, 0, 0);
    crate::log!("boss: a friend's hit; ours flinches (mode {mode} -> {to})");
}

/// The boss's MainSub while we follow a leader.
extern "C" fn boss_main(object: usize) {
    crate::watch::mark("boss update (boss_follow.rs)");
    let step = {
        let f = lock();
        let Some((_, main)) = f.boss.filter(|&(o, _)| o == object) else {
            // Not ours any more: nothing safe to run until the next frame decides.
            return;
        };
        let leader = f.state.filter(|_| f.term.is_none()).map(|(_, s, _)| s);
        Some((main, leader))
    };
    let Some((main, leader)) = step else {
        return;
    };
    let Some(leader) = leader else {
        // Nobody to follow this frame (the next frame unwraps it): it runs as it would alone.
        // SAFETY: the boss's own MainSub, called as the game calls it.
        unsafe { std::mem::transmute::<usize, extern "C" fn(usize)>(main)(object) };
        return;
    };
    let (Some(kind), d) = (kind(main), data1(object)) else {
        return;
    };
    if d == 0 {
        return;
    }
    let own_main = || {
        // SAFETY: the boss's own MainSub, called as the game calls it.
        unsafe { std::mem::transmute::<usize, extern "C" fn(usize)>(main)(object) }
    };

    // SAFETY: the boss's Data1 on the game thread.
    let mode = unsafe { ((d + D_MODE) as *const u8).read() };
    match frozen_main(object, d, kind, mode, own_main) {
        Hit::Last => {
            hit_here(object, main, true);
            return;
        }
        Hit::Counted => hit_here(object, main, false),
        Hit::None => {}
    }
    // SAFETY: as above.
    unsafe { take_state(object, d, &leader, false) };
}

#[derive(PartialEq)]
enum Hit {
    None,
    Counted,
    Last,
}

/// Runs the boss's main frozen (no mode function), then puts its mode back. A hit its own code
/// would have ignored in `mode` is undone (HP and mode).
fn frozen_main(object: usize, d: usize, kind: &Kind, mode: u8, run: impl Fn()) -> Hit {
    let hp = hp_slot(object);
    // SAFETY: the boss's Data1 and HP on the game thread; its main called as the game does.
    unsafe {
        let before = hp.map(|at| at.read());
        ((d + D_MODE) as *mut u8).write(kind.frozen);
        run();
        let after = ((d + D_MODE) as *const u8).read();
        if after == kind.frozen {
            ((d + D_MODE) as *mut u8).write(mode);
            return Hit::None;
        }
        if kind.ignores_hits(mode) {
            if let (Some(at), Some(b)) = (hp, before) {
                at.write(b);
            }
            ((d + D_MODE) as *mut u8).write(mode);
            return Hit::None;
        }
        if kind.dying.contains(&after) {
            Hit::Last
        } else {
            Hit::Counted
        }
    }
}

/// Our hit on a followed boss: sent to the leader; our last one makes it die here.
fn hit_here(object: usize, main: usize, last: bool) {
    let mut f = lock();
    if last {
        f.released = true;
        // SAFETY: the live boss's MainSub back to its own.
        unsafe {
            if ((object + OBJ_MAIN) as *const usize).read() == our_main() {
                ((object + OBJ_MAIN) as *mut usize).write(main);
            }
        }
        crate::log!("boss: our last hit; ours dies here");
    }
    let (level, mission) = f.stage;
    f.outgoing.push((Body::BossHit { level, mission }, true));
}

/// The leader's state onto ours: mode bytes, flags, timer, facing and motion, and its position
/// (eased, unless `jump` or far off).
///
/// # Safety
/// `d` is the live boss's Data1, on the game thread.
unsafe fn take_state(object: usize, d: usize, s: &BossState, jump: bool) {
    unsafe {
        ((d + D_MODE) as *mut u8).write(s.mode);
        ((d + D_SUB) as *mut u8).write(s.sub);
        ((d + D_COUNTER) as *mut u8).write(s.counter);
        ((d + D_FLAGS) as *mut u16).write_unaligned(s.flags);
        ((d + D_TIMER) as *mut u16).write_unaligned(s.timer);
        ((d + D_ROTATION) as *mut [i32; 3]).write_unaligned(s.rotation);
        let here = ((d + D_POSITION) as *const [f32; 3]).read_unaligned();
        let gap = [
            s.position[0] - here[0],
            s.position[1] - here[1],
            s.position[2] - here[2],
        ];
        let far = gap.iter().map(|g| g * g).sum::<f32>() > SNAP * SNAP;
        let k = if jump || far { 1.0 } else { BLEND };
        let at = [
            here[0] + gap[0] * k,
            here[1] + gap[1] * k,
            here[2] + gap[2] * k,
        ];
        ((d + D_POSITION) as *mut [f32; 3]).write_unaligned(at);
        let mc = controller(object);
        if mc != 0 {
            let (now, next) = (
                ((mc + MC_NOW) as *const u32).read() as u16,
                ((mc + MC_NEXT) as *const u32).read() as u16,
            );
            if now != s.motion && next != s.motion && motion_ok(mc, s.motion) {
                start_motion(mc, s.motion);
            }
        }
    }
}

/// `id` names a real entry of the controller's motion table (0x793C40 doesn't check).
///
/// # Safety
/// `mc` is a live motion controller.
unsafe fn motion_ok(mc: usize, id: u16) -> bool {
    if id >= MAX_MOTION_ID {
        return false;
    }
    unsafe {
        let (table, current) = (
            ((mc + MC_TABLE) as *const usize).read(),
            ((mc + MC_MOTION) as *const usize).read(),
        );
        if table == 0 || current == 0 {
            return false;
        }
        let entry = table + usize::from(id) * MOTION_ENTRY_SIZE;
        let motion = (entry as *const usize).read();
        let mode = ((entry + 4) as *const u16).read();
        // The boss's motions live with the one it plays now.
        motion != 0 && mode <= 7 && motion.abs_diff(current) < 0x0100_0000
    }
}

#[cfg(target_arch = "x86")]
unsafe fn start_motion(mc: usize, id: u16) {
    // SAFETY: usercall EAX = controller, EDX = id, as the boss's mode functions call it.
    unsafe {
        std::arch::asm!(
            "call {func}",
            func = in(reg) START_MOTION,
            inout("eax") mc => _,
            inout("edx") u32::from(id) => _,
            clobber_abi("C"),
        );
    }
}

#[cfg(not(target_arch = "x86"))]
unsafe fn start_motion(_: usize, _: u16) {
    let _ = START_MOTION;
}
