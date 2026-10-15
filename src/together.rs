//! Stages start together for a room (room.rs): story mode or Stage Select.
//!
//! On a fresh entry into a stage, while other players are connected, the stage is frozen at
//! its first frame (drawn like under the pause screen, nothing updates, the timer stands).
//! Anyone in that group (the room's players in the same stage and mission) starts it for the
//! whole group with Start, F8 or F9 > Start now: 3-2-1 and all go at once.
//! Restarts and Chao World are never held.

use std::sync::Mutex;
use std::time::{Duration, Instant};

use crate::game::{self, LEVEL_CHAO_WORLD, POSE_GO, POSE_WAITING};
use crate::hud;
use crate::log;

const COUNTDOWN: Duration = Duration::from_secs(3);
/// How long our "start now" flag stays up so others surely see it.
const GO_FLAG_TIME: Duration = Duration::from_secs(2);
const GO_TEXT_TIME: Duration = Duration::from_secs(1);

/// What we know about another player for this.
#[derive(Clone, Copy, Debug)]
pub struct Peer {
    pub id: u8,
    pub level: i16,
    pub mission: u8,
    pub chao_area: i8,
    pub flags: u8,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Phase {
    /// Nothing to do in this stage (or not a stage).
    Idle,
    /// Entered a stage; waiting for it to finish loading.
    Pending,
    Holding,
    Countdown {
        go_at: Instant,
    },
    Started {
        at: Instant,
    },
}

struct Together {
    level: i16,
    phase: Phase,
    go_until: Option<Instant>,
    /// Flags to send in our next player state.
    flags: u8,
    /// "Start now" was chosen in the menu.
    start_now: bool,
    /// A stage started together with others still in it (pausing doesn't stop it).
    live: bool,
}

static TOGETHER: Mutex<Together> = Mutex::new(Together {
    level: -1,
    phase: Phase::Idle,
    go_until: None,
    flags: 0,
    start_now: false,
    live: false,
});

/// In a stage started together with others in it.
pub fn live() -> bool {
    TOGETHER.lock().unwrap_or_else(|e| e.into_inner()).live
}

/// Menu: start this stage now (for everyone waiting in it).
pub fn start_now() {
    TOGETHER.lock().unwrap_or_else(|e| e.into_inner()).start_now = true;
}

/// Held at a start line right now (the menu shows "Start now").
pub fn is_holding() -> bool {
    matches!(
        TOGETHER.lock().unwrap_or_else(|e| e.into_inner()).phase,
        Phase::Holding
    )
}

fn release(t: &mut Together) {
    if matches!(
        t.phase,
        Phase::Pending | Phase::Holding | Phase::Countdown { .. }
    ) {
        game::hold_player(false);
        t.phase = Phase::Idle;
    }
}

/// "Players: You, Sam, Alex" for the players held at the same start (`ids`, without us).
pub fn players_here(ids: impl Iterator<Item = u8>) -> String {
    let names: Vec<String> = std::iter::once("You".to_string())
        .chain(ids.map(crate::room::name_of))
        .collect();
    format!("Players: {}", names.join(", "))
}

/// "Opponents: Sam, Alex" ("Opponent: Sam"), or "Opponents: none yet" (races and karate).
pub fn opponents_here(ids: impl Iterator<Item = u8>) -> String {
    let names: Vec<String> = ids.map(crate::room::name_of).collect();
    match names.len() {
        0 => "Opponents: none yet".into(),
        1 => format!("Opponent: {}", names[0]),
        _ => format!("Opponents: {}", names.join(", ")),
    }
}

fn is_stage(level: i16) -> bool {
    level > 0 && level != LEVEL_CHAO_WORLD
}

/// `POSE_*` flags for our next player state.
pub fn flags() -> u8 {
    TOGETHER.lock().unwrap_or_else(|e| e.into_inner()).flags
}

/// Call every frame (also when offline) with the other players' latest states.
pub fn on_frame(peers: &[Peer], connected: bool) {
    let mut t = TOGETHER.lock().unwrap_or_else(|e| e.into_inner());
    let now = Instant::now();
    let level = game::current_level();
    let mission = game::mission();
    let pressed = std::mem::take(&mut t.start_now);

    if level != t.level {
        release(&mut t);
        t.level = level;
        t.go_until = None;
        t.phase = if crate::room::in_room() && connected && is_stage(level) && !game::two_player() {
            Phase::Pending
        } else {
            Phase::Idle
        };
    }
    if !(crate::room::in_room() && connected) {
        release(&mut t);
    }

    // Only the room's players in this stage and mission count.
    let members = crate::room::members();
    let same: Vec<&Peer> = peers
        .iter()
        .filter(|p| members.contains(&p.id) && p.level == level && p.mission == mission)
        .collect();
    let someone_said_go = same.iter().any(|p| p.flags & POSE_GO != 0);

    match t.phase {
        Phase::Idle => {}
        Phase::Pending => {
            // Freeze before the stage's first frames run, so nobody gets a head start.
            game::freeze_world_after(2);
            if game::game_state() == game::GAME_STATE_INGAME {
                t.phase = Phase::Holding;
                log!("together: holding at the start of level {level} mission {mission}");
            }
        }
        Phase::Holding => {
            game::hold_player(true);
            // Anyone in this group goes for everyone in it: Start on the pad (the stage is
            // frozen, so it doesn't pause), F8 or F9 > Start now.
            let pressed = pressed || game::start_pressed();
            if pressed {
                t.go_until = Some(now + GO_FLAG_TIME);
            }
            if pressed || someone_said_go {
                t.phase = Phase::Countdown {
                    go_at: now + COUNTDOWN,
                };
                log!(
                    "together: countdown ({})",
                    if pressed {
                        "we started"
                    } else {
                        "someone started"
                    }
                );
            } else {
                hud::center2(
                    &players_here(same.iter().map(|p| p.id)),
                    "Press Start or F8 to go",
                );
            }
        }
        Phase::Countdown { go_at } => {
            game::hold_player(true);
            if now >= go_at {
                game::hold_player(false);
                t.phase = Phase::Started { at: now };
                log!("together: go");
            } else {
                let left = (go_at - now).as_secs_f32().ceil() as u32;
                hud::center(&left.to_string());
            }
        }
        Phase::Started { at } => {
            if now.duration_since(at) < GO_TEXT_TIME {
                hud::center("GO!");
            }
        }
    }

    // A stage started together is a shared race: pausing doesn't stop it while others are in.
    t.live = matches!(t.phase, Phase::Started { .. }) && !same.is_empty();

    let mut flags = 0;
    if matches!(t.phase, Phase::Holding | Phase::Countdown { .. }) {
        flags |= POSE_WAITING;
    }
    if t.go_until.is_some_and(|until| now < until) {
        flags |= POSE_GO;
    }
    t.flags = flags;
}
