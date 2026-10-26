//! Stages start together for a room (room.rs): story mode or Stage Select.
//!
//! On a fresh entry into a stage, while other players are connected, the stage is frozen at
//! its first frame (drawn like under the pause screen, nothing updates, the timer stands).
//! Anyone in that group (the room's players in the same stage and mission) starts it for the
//! whole group with Start, F8 or F9 > Start now: 3-2-1 and all go at once. Start waits until
//! every room member is held at this start (slower PCs load longer); pressing again, or
//! `WAIT_LIMIT`, goes without them.
//! Restarts and Chao World are never held.

use std::sync::Mutex;
use std::time::{Duration, Instant};

use crate::game::{self, LEVEL_CHAO_WORLD, POSE_GO, POSE_LOADING, POSE_PLAYING, POSE_WAITING};
use crate::hud;
use crate::log;

const COUNTDOWN: Duration = Duration::from_secs(3);
/// How long our "start now" flag stays up so others surely see it.
const GO_FLAG_TIME: Duration = Duration::from_secs(2);
const GO_TEXT_TIME: Duration = Duration::from_secs(1);
/// How long Start waits for room members still loading before going without them.
const WAIT_LIMIT: Duration = Duration::from_secs(20);

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
    mission: u8,
    party: Option<u8>,
    /// `game::stage_run` of `level`: a replay of the same stage is a new start.
    run: u32,
    phase: Phase,
    go_until: Option<Instant>,
    /// Flags to send in our next player state.
    flags: u8,
    /// "Start now" was chosen in the menu.
    start_now: bool,
    /// We asked to start at this time; waiting for room members still loading.
    want_go: Option<Instant>,
    /// Someone started while we were still loading: count down as soon as we're held.
    go_seen: bool,
    /// A stage started together with others still in it (pausing doesn't stop it).
    live: bool,
}

static TOGETHER: Mutex<Together> = Mutex::new(Together {
    level: -1,
    mission: 0,
    party: None,
    run: 0,
    phase: Phase::Idle,
    go_until: None,
    flags: 0,
    start_now: false,
    want_go: None,
    go_seen: false,
    live: false,
});

/// In a stage started together with others in it.
pub fn live() -> bool {
    TOGETHER.lock().unwrap_or_else(|e| e.into_inner()).live
}

/// When the stage we're in started together (the GO), if it did.
pub fn started_at() -> Option<Instant> {
    match TOGETHER.lock().unwrap_or_else(|e| e.into_inner()).phase {
        Phase::Started { at } => Some(at),
        _ => None,
    }
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
    t.want_go = None;
    t.go_seen = false;
}

/// "Players: You, Player2, Player3" for the players held at the same start (`ids`, without us).
pub fn players_here(ids: impl Iterator<Item = u8>) -> String {
    let names: Vec<String> = std::iter::once("You".to_string())
        .chain(ids.map(crate::room::name_of))
        .collect();
    format!("Players: {}", names.join(", "))
}

/// "Opponents: Player2, Player3" ("Opponent: Player2"), or "Opponents: none yet" (races and karate).
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

fn loaded_stage(mode: i32, state: i16, has_player: bool) -> bool {
    matches!(mode, 5 | 10) && matches!(state, 16 | 17) && has_player
}

/// Forming or changing parties during loaded play keeps that play running. A new stage,
/// mission or run still starts at its normal barrier, including travel from the menus.
fn scope_phase(
    previous: Phase,
    same_stage: bool,
    party_changed: bool,
    loaded: bool,
    hold_start: bool,
    now: Instant,
) -> Phase {
    if !hold_start {
        Phase::Idle
    } else if same_stage
        && party_changed
        && loaded
        && matches!(previous, Phase::Idle | Phase::Started { .. })
    {
        match previous {
            Phase::Started { .. } => previous,
            _ => Phase::Started { at: now },
        }
    } else {
        Phase::Pending
    }
}

/// A newcomer joins a stage already running in this party immediately. Advertising GO or
/// starting another countdown would delay them and could start other loading members again.
fn join_running(t: &mut Together, same: &[&Peer], now: Instant) -> bool {
    if matches!(t.phase, Phase::Started { .. })
        || !same
            .iter()
            .any(|p| p.flags & POSE_PLAYING != 0 && p.flags & (POSE_LOADING | POSE_WAITING) == 0)
    {
        return false;
    }
    t.phase = Phase::Started { at: now };
    t.go_until = None;
    t.want_go = None;
    t.go_seen = false;
    true
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
    let party = crate::room::party_owner().filter(|_| connected);
    let state = game::game_state();
    let loaded = loaded_stage(game::game_mode(), state, !game::local_entity().is_null());
    let ready = loaded && state == game::GAME_STATE_INGAME;
    let hold_start = party.is_some() && is_stage(level) && !game::two_player();
    let pressed = std::mem::take(&mut t.start_now);

    let run = game::stage_run();
    let same_stage = level == t.level && mission == t.mission && run == t.run;
    let party_changed = party != t.party;
    if !same_stage || party_changed {
        let phase = scope_phase(t.phase, same_stage, party_changed, loaded, hold_start, now);
        release(&mut t);
        t.level = level;
        t.mission = mission;
        t.party = party;
        t.run = run;
        t.go_until = None;
        t.phase = phase;
    }
    if party.is_none() {
        release(&mut t);
    }
    // A native exit can begin while we're held. Do not reapply that hold to the fade or
    // menus; their GameState/CurrentLevel may still name the stage that just unloaded.
    if !ready && matches!(t.phase, Phase::Holding | Phase::Countdown { .. }) {
        release(&mut t);
        t.go_until = None;
        t.phase = if hold_start {
            Phase::Pending
        } else {
            Phase::Idle
        };
    }

    // Only the room's players in this stage and mission count.
    let members = crate::room::members();
    let same: Vec<&Peer> = peers
        .iter()
        .filter(|p| members.contains(&p.id) && p.level == level && p.mission == mission)
        .collect();
    let someone_said_go = same.iter().any(|p| p.flags & POSE_GO != 0);

    if party.is_some() && is_stage(level) && ready && join_running(&mut t, &same, now) {
        game::hold_player(false);
        log!("together: joining the party's running level {level} mission {mission}");
    }

    match t.phase {
        Phase::Idle => {}
        Phase::Pending => {
            t.go_seen |= someone_said_go;
            if ready {
                // Freeze before the stage's first frames run, so nobody gets a head start.
                game::freeze_world_after(2);
                t.phase = Phase::Holding;
                log!("together: holding at the start of level {level} mission {mission}");
            }
        }
        Phase::Holding => {
            game::hold_player(true);
            // Anyone in this group goes for everyone in it: Start on the pad (the stage is
            // frozen, so it doesn't pause), F8 or F9 > Start now. The first press waits for room
            // members still loading; a second press goes without them.
            let pressed = pressed || game::start_pressed();
            let force = pressed && t.want_go.is_some();
            if pressed && t.want_go.is_none() {
                t.want_go = Some(now);
                log!("together: start asked, waiting for everyone to load");
            }
            // Room members coming here: in this stage but not held yet (still loading), or
            // between screens (level 0 or below, e.g. taken along from Stage Select). Members in
            // another stage or Chao World play their own game and aren't waited for.
            let loading: Vec<u8> = peers
                .iter()
                .filter(|p| members.contains(&p.id))
                .filter(|p| {
                    let here = p.level == level && p.mission == mission;
                    (here && p.flags & (POSE_WAITING | POSE_GO | POSE_PLAYING) == 0) || p.level <= 0
                })
                .map(|p| p.id)
                .collect();
            let we_go = t.want_go.is_some_and(|asked| {
                loading.is_empty() || force || now.duration_since(asked) >= WAIT_LIMIT
            });
            if we_go {
                t.go_until = Some(now + GO_FLAG_TIME);
            }
            if we_go || someone_said_go || t.go_seen {
                // Karts: the game's own fly-by and lights follow the release, in step.
                let countdown = if level == crate::kart::KART_LEVEL {
                    Duration::ZERO
                } else {
                    COUNTDOWN
                };
                t.phase = Phase::Countdown {
                    go_at: now + countdown,
                };
                log!(
                    "together: countdown ({})",
                    if !we_go {
                        "someone started"
                    } else if loading.is_empty() {
                        "we started"
                    } else {
                        "we started without everyone"
                    }
                );
                t.want_go = None;
                t.go_seen = false;
            } else if t.want_go.is_some() {
                let names: Vec<String> =
                    loading.iter().map(|id| crate::room::name_of(*id)).collect();
                hud::center2(
                    &format!("Waiting for {} to load", names.join(", ")),
                    "Press Start or F8 again to go now",
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
            if loaded && now.duration_since(at) < GO_TEXT_TIME {
                hud::center("GO!");
            }
        }
    }

    // A stage started together is a shared race: pausing doesn't stop it while others are in.
    t.live = loaded && matches!(t.phase, Phase::Started { .. }) && !same.is_empty();

    let mut flags = 0;
    if matches!(t.phase, Phase::Holding | Phase::Countdown { .. }) {
        flags |= POSE_WAITING;
    }
    if loaded && t.go_until.is_some_and(|until| now < until) {
        flags |= POSE_GO;
    }
    if loaded && matches!(t.phase, Phase::Started { .. }) {
        flags |= POSE_PLAYING;
    }
    t.flags = flags;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn held(phase: Phase) -> Together {
        Together {
            level: 13,
            mission: 0,
            party: Some(0),
            run: 1,
            phase,
            go_until: None,
            flags: 0,
            start_now: false,
            want_go: None,
            go_seen: false,
            live: false,
        }
    }

    fn peer(flags: u8) -> Peer {
        Peer {
            id: 1,
            level: 13,
            mission: 0,
            chao_area: 0,
            flags,
        }
    }

    #[test]
    fn late_join_releases_when_one_member_is_playing_and_others_are_still_loading() {
        let now = Instant::now();
        let playing = peer(POSE_PLAYING);
        let loading = peer(POSE_LOADING);
        for phase in [
            Phase::Idle,
            Phase::Pending,
            Phase::Holding,
            Phase::Countdown {
                go_at: now + COUNTDOWN,
            },
        ] {
            let mut t = held(phase);
            t.go_until = Some(now + GO_FLAG_TIME);
            t.want_go = Some(now);
            t.go_seen = true;
            assert!(join_running(&mut t, &[&playing, &loading], now));
            assert_eq!(t.phase, Phase::Started { at: now });
            assert!(t.go_until.is_none());
            assert!(t.want_go.is_none());
            assert!(!t.go_seen);
        }
    }

    #[test]
    fn joining_member_does_not_restart_an_existing_run() {
        let now = Instant::now();
        let started = now - Duration::from_secs(30);
        let mut t = held(Phase::Started { at: started });
        assert!(!join_running(&mut t, &[&peer(POSE_WAITING)], now));
        assert_eq!(t.phase, Phase::Started { at: started });
    }

    #[test]
    fn fresh_start_keeps_its_hold_until_a_settled_member_is_playing() {
        let now = Instant::now();
        let mut t = held(Phase::Holding);
        for flags in [
            POSE_WAITING,
            POSE_GO | POSE_WAITING,
            POSE_LOADING | POSE_PLAYING,
            0,
        ] {
            assert!(!join_running(&mut t, &[&peer(flags)], now));
            assert_eq!(t.phase, Phase::Holding);
        }
        assert!(!join_running(&mut t, &[], now));
    }

    #[test]
    fn stale_menu_state_and_loading_objects_are_not_loaded_gameplay() {
        assert!(!loaded_stage(0xE, 16, true));
        assert!(!loaded_stage(4, 16, true));
        assert!(!loaded_stage(5, 16, false));
        assert!(!loaded_stage(5, 14, true));
        assert!(loaded_stage(5, 16, true));
        assert!(loaded_stage(10, 17, true));
    }

    #[test]
    fn forming_a_party_during_play_does_not_refreeze_its_leader() {
        let now = Instant::now();
        let started = now - Duration::from_secs(30);
        assert_eq!(
            scope_phase(Phase::Idle, true, true, true, true, now),
            Phase::Started { at: now }
        );
        assert_eq!(
            scope_phase(Phase::Started { at: started }, true, true, true, true, now),
            Phase::Started { at: started }
        );
        // A newcomer joining from menus still enters the new stage's normal start path.
        assert_eq!(
            scope_phase(Phase::Idle, true, true, false, true, now),
            Phase::Pending
        );
        // Changing mission or replaying a stage does not inherit the previous start.
        assert_eq!(
            scope_phase(
                Phase::Started { at: started },
                false,
                false,
                true,
                true,
                now
            ),
            Phase::Pending
        );
    }
}
