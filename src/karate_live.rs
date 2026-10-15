//! Live karate (rooms, Battle DLC): both players' games show the same fight.
//!
//! Both games start the game's own VS fight (`g+5 = 1`) with the same two Chao records: side 0
//! the leader's Chao, side 1 the follower's (so the follower sees its own Chao on the right).
//! The start is held with both fighters in the ring until both games are there. The leader's
//! game runs the fight normally and sends both fighters (position, rotation, HP, morale,
//! motion) and the clock ~30 times a second, then the result. In the follower's game both
//! fighters are puppets (action 8: no AI, no damage) that copy the leader's, the native
//! end-of-fight checks are switched off, and the fight ends with the leader's result.
//! See re/chao-karate-sync.md.

use std::sync::Mutex;
use std::time::{Duration, Instant};

use crate::chao::ChaoData;
use crate::game::{self, POSE_GO, POSE_KARATE_BUSY, POSE_WAITING};
use crate::hud;
use crate::log;
use crate::net::protocol::{KarateFighter, KarateState};
use crate::together::Peer;

/// Karate globals `g` (pointer) and the karate main object `M`.
const KARATE_GLOBALS: usize = 0x01A5_D148;
const KARATE_MAIN: usize = 0x01A5_D14C;
/// MainSubs we wrap: the karate main (0x542DA0) and each fighter (0x578D30).
const MAIN_EXEC: usize = 0x0054_2DA0;
const FIGHTER_MAIN: usize = 0x0057_8D30;
/// `ChaoArea_Karate`.
const AREA_KARATE: i32 = 10;
/// Game paused (the end of the fight waits for it to be clear).
const PAUSED: usize = 0x0174_AFF7;

// Offsets in `g`.
const G_END: usize = 0x03;
const G_PHASE: usize = 0x04;
const G_VS: usize = 0x05;
const G_RESULT: usize = 0x0B;
const G_CLOCK: usize = 0x0C;
const G_FIGHTERS: usize = 0x10;
const G_RECORDS: usize = 0x18;

// Karate main states (`MD[0]`).
const MENU_STATES: std::ops::RangeInclusive<u8> = 3..=5;
/// The mode select, the only menu screen a live fight starts from (no sub-windows open).
const STATE_MODE_SELECT: u8 = 3;
const STATE_CONFIRM: u8 = 7;
const STATE_SPAWN: u8 = 9;
const STATE_ANNOUNCE: u8 = 10;
const STATE_FIGHT: u8 = 11;
const STATE_RESULT: u8 = 12;

// Objects: Data1 at +0x34, work (`W`) at +0x40, MainSub at +0x10.
const OBJ_MAIN: usize = 0x10;
const OBJ_DATA1: usize = 0x34;
const OBJ_WORK: usize = 0x40;
// Fighter Data1 (`FD`): action, sub, status bits, rotation, position.
const FD_STATUS: usize = 0x04;
const FD_ROTATION: usize = 0x08;
const FD_POSITION: usize = 0x14;
const STATUS_LOST: u16 = 0x1000;
/// A fighter action without a handler: no AI, hits are dropped.
const ACTION_PUPPET: u8 = 8;
// Fighter work (`W`).
const W_SIDE: usize = 0x02;
const W_CHAO: usize = 0x08;
const W_MOTION: usize = 0x0C;
const W_MORALE: usize = 0x38;
const W_HP: usize = 0x3C;

/// Per-side buttons the fight reads (filled from both pads each frame by 0x537480): 12 bytes
/// per side, `+4` = pressed this frame; bit 4 (A) is a cheer (0x577C6D).
const SIDE_BUTTONS: usize = 0x01DB_ED40;
const SIDE_BUTTONS_SIZE: usize = 12;
const BUTTONS_PRESSED: usize = 4;
const BUTTON_CHEER: u32 = 4;
/// Player 1's pad (`*0x1DEFB60`): `+0x10` = pressed this frame.
const PAD1: usize = 0x01DE_FB60;
const PAD_PRESSED: usize = 0x10;

/// Native end checks, switched off in the follower's game during a live fight:
/// knockout (0x578679), ring-out (0x577F9D), time up (0x575114).
const END_CHECK_PATCHES: [(usize, &[u8], &[u8]); 3] = [
    (0x0057_8679, &[0x75], &[0xEB]),
    (0x0057_7F9D, &[0x74, 0x0B], &[0x90, 0x90]),
    (0x0057_5114, &[0x75], &[0xEB]),
];

const HOLD_LIMIT: Duration = Duration::from_secs(30);
const GO_FLAG_TIME: Duration = Duration::from_secs(2);
/// A follower keeps a leader's pick this long while they get to the karate menu.
const PICK_KEEP: Duration = Duration::from_secs(20);

/// How a fight ended: (code 2 knockout / 3 time up, winning side or 2 = draw, final HP).
type FightEnd = (u8, u8, [f32; 2]);

#[derive(Clone, Copy, PartialEq)]
enum Role {
    Leader { peer: u8 },
    Follower { leader: u8 },
}

impl Role {
    fn other(self) -> u8 {
        match self {
            Role::Leader { peer } => peer,
            Role::Follower { leader } => leader,
        }
    }
}

#[derive(Clone, Copy, PartialEq)]
enum Start {
    Waiting,
    Holding { since: Instant, had_company: bool },
    Released { go_until: Instant },
}

struct Live {
    role: Option<Role>,
    start: Start,
    start_now: bool,
    /// A leader picked us: (leader, when).
    incoming_pick: Option<(u8, Instant)>,
    /// Our pick to send (the opponent's id), after our Chao for it.
    outgoing_pick: Option<u8>,
    outgoing_chao: Option<ChaoData>,
    /// Leader: the fight to send; the end to send (once).
    state_out: Option<KarateState>,
    end_out: Option<FightEnd>,
    end_sent: bool,
    /// Follower: cheers to send. Leader: the follower's cheers to apply.
    cheers_out: u8,
    cheers_in: u8,
    /// Follower: the leader's fight; its end, to apply.
    remote: Option<KarateState>,
    pending_end: Option<FightEnd>,
    last_motion: [Option<u8>; 2],
    patched: bool,
    /// F9 choice of opponent (`None` = the first free room player at Karate).
    chosen_opponent: Option<u8>,
    /// Diagnostics already logged this fight.
    logged_main: bool,
    logged_wait: bool,
    last_state: Option<u8>,
    /// The two records the fight uses (stable buffers handed to the game).
    records: Option<[&'static mut ChaoData; 2]>,
}

static LIVE: Mutex<Live> = Mutex::new(Live {
    role: None,
    start: Start::Waiting,
    start_now: false,
    incoming_pick: None,
    outgoing_pick: None,
    outgoing_chao: None,
    state_out: None,
    end_out: None,
    end_sent: false,
    cheers_out: 0,
    cheers_in: 0,
    remote: None,
    pending_end: None,
    last_motion: [None; 2],
    patched: false,
    chosen_opponent: None,
    logged_main: false,
    logged_wait: false,
    last_state: None,
    records: None,
});

fn lock() -> std::sync::MutexGuard<'static, Live> {
    LIVE.lock().unwrap_or_else(|e| e.into_inner())
}

fn read<T: Copy>(address: usize) -> T {
    // SAFETY: callers pass addresses of live game data (checked non-null bases).
    unsafe { (address as *const T).read_unaligned() }
}

fn write<T>(address: usize, value: T) {
    // SAFETY: as in `read`.
    unsafe { (address as *mut T).write_unaligned(value) }
}

fn in_karate() -> bool {
    game::current_level() == game::LEVEL_CHAO_WORLD && game::current_chao_area() == AREA_KARATE
}

/// The karate globals and the main object's Data1, while in the karate area.
fn karate() -> Option<(usize, usize, usize)> {
    if !in_karate() {
        return None;
    }
    let g: usize = read(KARATE_GLOBALS);
    let main: usize = read(KARATE_MAIN);
    if g == 0 || main == 0 {
        return None;
    }
    let md: usize = read(main + OBJ_DATA1);
    (md != 0).then_some((g, main, md))
}

/// In Chao World's karate area.
pub fn in_karate_area() -> bool {
    in_karate()
}

/// At the karate mode select, where a VS fight can be started.
pub fn at_menu() -> bool {
    karate()
        .is_some_and(|(g, _, md)| read::<u8>(md) == STATE_MODE_SELECT && read::<u8>(g + G_END) == 0)
}

/// In a live fight (pausing doesn't stop it).
pub fn live() -> bool {
    lock().role.is_some()
}

/// In a live fight (for the F9 menu's "Start now").
pub fn is_holding() -> bool {
    matches!(lock().start, Start::Holding { .. })
}

pub fn start_now() {
    // Only while held, so a stray F8 doesn't start the next fight on its own.
    let mut l = lock();
    l.start_now = matches!(l.start, Start::Holding { .. });
}

/// `POSE_*` flags for our next player state.
pub fn flags() -> u8 {
    let l = lock();
    let busy = if l.role.is_some() {
        POSE_KARATE_BUSY
    } else {
        0
    };
    busy | match l.start {
        Start::Holding { .. } => POSE_WAITING,
        Start::Released { go_until } if Instant::now() < go_until => POSE_GO,
        _ => 0,
    }
}

/// F9: who to challenge (a player id), or `None` = the first free one.
pub fn set_opponent(id: Option<u8>) {
    lock().chosen_opponent = id;
}

pub fn opponent() -> Option<u8> {
    lock().chosen_opponent
}

/// Room players at Karate, with whether they're free to fight: for the F9 opponent row.
pub fn candidates(peers: &[(u8, Peer)]) -> Vec<(u8, bool)> {
    let members = crate::room::members();
    peers
        .iter()
        .filter(|(id, p)| {
            members.contains(id)
                && p.level == game::LEVEL_CHAO_WORLD
                && i32::from(p.chao_area) == AREA_KARATE
        })
        .map(|(id, p)| (*id, p.flags & POSE_KARATE_BUSY == 0))
        .collect()
}

/// Starts the VS fight with `side0` (the leader's Chao) and `side1` (the follower's).
fn start_fight(l: &mut Live, side0: &ChaoData, side1: &ChaoData) -> bool {
    let Some((g, _, md)) = karate() else {
        return false;
    };
    let records = l.records.get_or_insert_with(|| {
        [
            Box::leak(Box::new(side0.clone())),
            Box::leak(Box::new(side1.clone())),
        ]
    });
    *records[0] = side0.clone();
    *records[1] = side1.clone();
    write(g + G_VS, 1u8);
    write(g + G_RECORDS, &*records[0] as *const ChaoData as usize);
    write(g + G_RECORDS + 4, &*records[1] as *const ChaoData as usize);
    write(md, STATE_CONFIRM);
    write(md + 1, 0u8);
    l.start = Start::Waiting;
    l.end_sent = false;
    l.end_out = None;
    l.remote = None;
    l.pending_end = None;
    l.last_motion = [None; 2];
    l.logged_main = false;
    l.logged_wait = false;
    true
}

/// The Chao the creator picked for the fight, to send before the pick (so the opponent's game
/// uses the same one).
pub fn take_outgoing_chao() -> Option<ChaoData> {
    lock().outgoing_chao.take()
}

pub fn take_outgoing_pick() -> Option<u8> {
    lock().outgoing_pick.take()
}

/// Network: `leader` picked `opponent` for a live fight.
pub fn received_pick(leader: u8, opponent: u8, me: Option<u8>) {
    // Only players in the challenger's room are taken into their fight.
    if me == Some(opponent) && crate::room::shares_room(leader) {
        lock().incoming_pick = Some((leader, Instant::now()));
    }
}

/// Network: the leader's fight.
pub fn received_state(from: u8, state: KarateState) {
    let mut l = lock();
    if l.role == Some(Role::Follower { leader: from }) {
        l.remote = Some(state);
    }
}

/// Network: the follower cheered.
pub fn received_cheer(from: u8, count: u8) {
    let mut l = lock();
    if l.role == Some(Role::Leader { peer: from }) {
        l.cheers_in = l.cheers_in.saturating_add(count);
    }
}

/// Follower: cheers to send (A presses during the fight).
pub fn take_cheers() -> u8 {
    std::mem::take(&mut lock().cheers_out)
}

/// Network: how the leader's fight ended.
pub fn received_end(from: u8, code: u8, result: u8, hp: [f32; 2]) {
    let mut l = lock();
    if l.role == Some(Role::Follower { leader: from }) {
        log!("karate: leader's fight ended (code {code}, result {result})");
        l.pending_end = Some((code, result, hp));
    }
}

/// Leader: the fight to send now (at the state rate), and the end once.
pub fn take_outgoing() -> (Option<KarateState>, Option<FightEnd>) {
    let mut l = lock();
    let end = if !l.end_sent && l.end_out.is_some() {
        l.end_sent = true;
        l.end_out
    } else {
        None
    };
    (l.state_out.take(), end)
}

fn set_patches(l: &mut Live, on: bool) {
    if l.patched == on {
        return;
    }
    let mut ok = true;
    for (address, original, patched) in END_CHECK_PATCHES {
        let (from, to) = if on {
            (original, patched)
        } else {
            (patched, original)
        };
        ok &= crate::patch::replace_code(address, from, to);
    }
    l.patched = on;
    if !ok {
        log!("karate: end-check patch didn't apply cleanly (on = {on})");
    }
}

fn reset(l: &mut Live) {
    set_patches(l, false);
    if let Some((_, main, _)) = karate() {
        let sub = main + OBJ_MAIN;
        if read::<usize>(sub) == main_exec as extern "C" fn(usize) as usize {
            write(sub, MAIN_EXEC);
        }
    }
    l.role = None;
    l.start = Start::Waiting;
    l.state_out = None;
    l.remote = None;
    l.pending_end = None;
}

/// The two fighter objects (side 0, side 1), once spawned.
fn fighters(g: usize) -> [usize; 2] {
    [read(g + G_FIGHTERS), read(g + G_FIGHTERS + 4)]
}

/// Call every frame. `chao_of(id)` gives a player's first Chao brought online, `mine` ours.
pub fn on_frame(
    peers: &[(u8, Peer)],
    connected: bool,
    chao_of: impl Fn(u8) -> Option<ChaoData>,
    mine: Option<&ChaoData>,
) {
    let mut l = lock();
    let now = Instant::now();
    let Some((g, main, md)) = karate() else {
        if l.role.is_some() {
            reset(&mut l);
        }
        l.incoming_pick = None;
        return;
    };

    let state_now: u8 = read(md);
    let previous = l.last_state.replace(state_now);

    // In a room: our own pick in the karate menu (tournament screens 4-6, then the confirm,
    // 7) becomes a live fight against the chosen room player at Karate (or the first free
    // one), with the Chao we picked.
    if l.role.is_none()
        && state_now == STATE_CONFIRM
        && previous.is_some_and(|p| (4..=6).contains(&p))
        && crate::room::in_room()
    {
        let free: Vec<u8> = candidates(peers)
            .into_iter()
            .filter_map(|(id, free)| free.then_some(id))
            .collect();
        let opponent = l
            .chosen_opponent
            .filter(|id| free.contains(id))
            .or(free.first().copied());
        let picked: usize = read(g + G_RECORDS);
        let mine = if picked != 0 {
            // SAFETY: the game's record for the Chao just picked (a save slot).
            Some(unsafe { (*(picked as *const ChaoData)).clone() })
        } else {
            mine.cloned()
        };
        match (opponent, mine) {
            (Some(peer), Some(mine)) => match chao_of(peer) {
                Some(theirs) if start_fight(&mut l, &mine, &theirs) => {
                    l.role = Some(Role::Leader { peer });
                    l.outgoing_chao = Some(mine);
                    l.outgoing_pick = Some(peer);
                    log!("karate: live fight against player {peer}, leading");
                    hud::toast("Live karate: starting");
                }
                _ => hud::toast("Live karate: waiting for your opponent's Chao (normal fight)"),
            },
            _ => hud::toast("Room: nobody free in your room is at Karate (normal fight)"),
        }
    }

    // Follower: start the leader's fight once we're at the karate menu.
    if let Some((leader, at)) = l.incoming_pick {
        if at.elapsed() > PICK_KEEP {
            l.incoming_pick = None;
            hud::toast("Live karate: go to Karate in the Chao Stadium to join");
        } else if at_menu() {
            l.incoming_pick = None;
            match (chao_of(leader), mine) {
                (Some(theirs), Some(mine)) => {
                    if start_fight(&mut l, &theirs, mine) {
                        l.role = Some(Role::Follower { leader });
                        log!("karate: live fight against player {leader}, following");
                        hud::toast("Live karate: starting");
                    }
                }
                _ => hud::toast("Live karate: missing a Chao (enter a garden first)"),
            }
        }
    }

    let Some(role) = l.role else { return };
    let state: u8 = read(md);
    let sub: u8 = read(md + 1);
    // Back at the menu after the fight: done.
    if MENU_STATES.contains(&state) && l.start != Start::Waiting {
        reset(&mut l);
        return;
    }
    let other = role.other();
    let other_here = peers.iter().find(|(id, p)| {
        *id == other && p.level == game::LEVEL_CHAO_WORLD && i32::from(p.chao_area) == AREA_KARATE
    });

    // Hold with both fighters in the ring until both games are there.
    if state == STATE_SPAWN && sub == 2 {
        match l.start {
            Start::Waiting => {
                l.start = Start::Holding {
                    since: now,
                    had_company: false,
                };
                log!("karate: holding the start");
            }
            Start::Holding { since, had_company } => {
                // The challenger starts the fight (F9 > Start now) once they're happy; the
                // opponent waits for their go. If the other player leaves, go anyway.
                let ready =
                    other_here.is_some_and(|(_, p)| p.flags & (POSE_WAITING | POSE_GO) != 0);
                let go = other_here.is_some_and(|(_, p)| p.flags & POSE_GO != 0);
                let left = had_company && other_here.is_none();
                // Either fighter starts it for both (F8 or F9 > Start now).
                let pressed = std::mem::take(&mut l.start_now);
                let stuck = other_here.is_none() && now.duration_since(since) > HOLD_LIMIT;
                if go || left || pressed || !connected || stuck {
                    log!("karate: start");
                    l.start = Start::Released {
                        go_until: now + GO_FLAG_TIME,
                    };
                } else {
                    l.start = Start::Holding {
                        since,
                        had_company: had_company || other_here.is_some(),
                    };
                    if read::<u16>(md + 6) < 2 {
                        write(md + 6, 2u16);
                    }
                    let opponent = crate::room::name_of(other);
                    if ready {
                        hud::center2(&format!("Opponent: {opponent}"), "Press F8 to fight");
                    } else {
                        hud::center(&format!("Waiting for {opponent}..."));
                    }
                }
            }
            Start::Released { .. } => {}
        }
    }

    match role {
        Role::Leader { .. } => {
            if matches!(state, STATE_ANNOUNCE | STATE_FIGHT) {
                l.state_out = Some(fight_state(g));
            }
            // The follower's fighter (side 1) reads its cheers from the second side's buttons.
            if state == STATE_FIGHT {
                let fighter = fighters(g)[1];
                if fighter != 0 && read::<usize>(fighter + OBJ_MAIN) == FIGHTER_MAIN {
                    write(
                        fighter + OBJ_MAIN,
                        cheer_main as extern "C" fn(usize) as usize,
                    );
                }
            }
            if state == STATE_RESULT && l.end_out.is_none() {
                let code = if read::<u8>(g + G_PHASE) == 4 { 3 } else { 2 };
                let result: u8 = read(g + G_RESULT);
                let f = fight_state(g);
                l.end_out = Some((code, result.min(2), [f.fighters[0].hp, f.fighters[1].hp]));
                log!("karate: fight over (code {code}, result {result})");
            }
        }
        Role::Follower { .. } => {
            if matches!(state, STATE_ANNOUNCE | STATE_FIGHT) {
                set_patches(&mut l, true);
                // The leader's game ends the fight; ours follows (see main_exec).
                let sub_main = main + OBJ_MAIN;
                let current: usize = read(sub_main);
                if current == MAIN_EXEC {
                    write(sub_main, main_exec as extern "C" fn(usize) as usize);
                    log!("karate: following the leader's fight");
                } else if current != main_exec as extern "C" fn(usize) as usize && !l.logged_main {
                    l.logged_main = true;
                    log!("karate: main object {main:#x} has MainSub {current:#x}, not wrapping");
                }
                for fighter in fighters(g) {
                    if fighter != 0 && read::<usize>(fighter + OBJ_MAIN) == FIGHTER_MAIN {
                        write(
                            fighter + OBJ_MAIN,
                            fighter_main as extern "C" fn(usize) as usize,
                        );
                    }
                }
                if let Some(remote) = l.remote {
                    write(g + G_CLOCK, remote.clock);
                }
                // Our A presses cheer our Chao in the leader's game.
                let pad: usize = read(PAD1);
                if state == STATE_FIGHT
                    && pad != 0
                    && read::<u32>(pad + PAD_PRESSED) & BUTTON_CHEER != 0
                {
                    l.cheers_out = l.cheers_out.saturating_add(1);
                }
            } else if state >= STATE_RESULT {
                set_patches(&mut l, false);
            }
        }
    }
}

/// Both fighters as our game has them now.
fn fight_state(g: usize) -> KarateState {
    let mut state = KarateState {
        clock: read(g + G_CLOCK),
        ..KarateState::default()
    };
    for (side, fighter) in fighters(g).into_iter().enumerate() {
        if fighter == 0 {
            continue;
        }
        let fd: usize = read(fighter + OBJ_DATA1);
        let w: usize = read(fighter + OBJ_WORK);
        if fd == 0 || w == 0 {
            continue;
        }
        let motion: usize = read(w + W_MOTION);
        state.fighters[side] = KarateFighter {
            position: read(fd + FD_POSITION),
            rotation: read(fd + FD_ROTATION),
            hp: read(w + W_HP),
            morale: read(w + W_MORALE),
            motion: if motion != 0 { read(motion) } else { 0 },
        };
    }
    state
}

/// MainSub of the follower's fighter in the leader's game: presses the second side's A
/// button for each cheer the follower sent, then runs the fighter as usual.
extern "C" fn cheer_main(object: usize) {
    {
        let mut l = lock();
        if l.cheers_in > 0 {
            l.cheers_in -= 1;
            let pressed = SIDE_BUTTONS + SIDE_BUTTONS_SIZE + BUTTONS_PRESSED;
            write(pressed, read::<u32>(pressed) | BUTTON_CHEER);
        }
    }
    // SAFETY: the fighter's own MainSub, which this object had.
    unsafe { std::mem::transmute::<usize, extern "C" fn(usize)>(FIGHTER_MAIN)(object) };
}

/// MainSub of a fighter in the follower's game: a puppet of the leader's fighter.
extern "C" fn fighter_main(object: usize) {
    let fd: usize = read(object + OBJ_DATA1);
    let apply = {
        let l = lock();
        let fighting = karate().is_some_and(|(_, _, md)| read::<u8>(md) == STATE_FIGHT);
        (fighting && fd != 0).then_some(l.remote).flatten()
    };
    if apply.is_some() {
        // No AI, no damage: the leader's game decides.
        write(fd, ACTION_PUPPET);
        write(fd + 1, 0u8);
        // No local cheering either: our A presses go to the leader's game (take_cheers),
        // which cheers our Chao there.
        for side in 0..2 {
            let pressed = SIDE_BUTTONS + side * SIDE_BUTTONS_SIZE + BUTTONS_PRESSED;
            write(pressed, read::<u32>(pressed) & !BUTTON_CHEER);
        }
    }
    // SAFETY: the fighter's own MainSub, which this object had.
    unsafe { std::mem::transmute::<usize, extern "C" fn(usize)>(FIGHTER_MAIN)(object) };
    let Some(remote) = apply else { return };
    let _ = std::panic::catch_unwind(|| {
        let w: usize = read(object + OBJ_WORK);
        if w == 0 {
            return;
        }
        let side = usize::from(read::<u16>(w + W_SIDE)).min(1);
        let f = remote.fighters[side];
        write(fd + FD_POSITION, f.position);
        write(fd + FD_ROTATION, f.rotation);
        // The visible Chao already got last frame's transform from the original MainSub.
        let chao: usize = read(w + W_CHAO);
        if chao != 0 {
            let data1: usize = read(chao + OBJ_DATA1);
            if data1 != 0 {
                write(data1 + FD_POSITION, f.position);
                write(data1 + FD_ROTATION, f.rotation);
            }
        }
        write(w + W_HP, f.hp);
        write(w + W_MORALE, f.morale);
        let mut l = lock();
        if l.last_motion[side] != Some(f.motion) {
            l.last_motion[side] = Some(f.motion);
            let motion: usize = read(w + W_MOTION);
            if motion != 0 {
                // As the game switches motions (e.g. 0x579689).
                write(fd + FD_STATUS, read::<u16>(fd + FD_STATUS) & 0x7FFF);
                write(motion + 5, read::<u8>(motion + 5) & 0xE7);
                write(motion, f.motion);
                write(motion + 1, 0u8);
                write(motion + 2, 0u8);
            }
        }
    });
}

/// MainSub of the karate main object in the follower's game: ends the fight the way the
/// leader's ended (it has to be set here, because the main clears `g+3` every run).
extern "C" fn main_exec(object: usize) {
    let _ = std::panic::catch_unwind(|| {
        let mut l = lock();
        let Some((code, result, hp)) = l.pending_end else {
            return;
        };
        let Some((g, _, md)) = karate() else { return };
        let (state, sub, paused) = (read::<u8>(md), read::<u8>(md + 1), read::<u8>(PAUSED));
        if state != STATE_FIGHT || sub != 1 || paused != 0 {
            if !l.logged_wait {
                l.logged_wait = true;
                log!("karate: end waiting (state {state}, sub {sub}, paused {paused})");
            }
            return;
        }
        let [side0, side1] = fighters(g);
        for (fighter, hp) in [(side0, hp[0]), (side1, hp[1])] {
            let w: usize = if fighter != 0 {
                read(fighter + OBJ_WORK)
            } else {
                0
            };
            if w != 0 {
                write(w + W_HP, hp);
            }
        }
        if code == 2 && result == 1 && side0 != 0 {
            // A knockout is judged from side 0's "lost" bit only.
            let fd: usize = read(side0 + OBJ_DATA1);
            if fd != 0 {
                write(fd + FD_STATUS, read::<u16>(fd + FD_STATUS) | STATUS_LOST);
            }
        }
        write(g + G_END, code);
        l.pending_end = None;
        log!("karate: ending the fight like the leader's (code {code}, result {result})");
    });
    // SAFETY: the karate main object's own MainSub.
    unsafe { std::mem::transmute::<usize, extern "C" fn(usize)>(MAIN_EXEC)(object) };
}
