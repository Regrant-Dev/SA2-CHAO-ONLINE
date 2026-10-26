//! "Go to friend": follow their current stage through the game's native exits and freshly
//! initialized Stage Select, join their party, then move next to them after loading. No
//! destination resources or saved Stage Select panels need to have been loaded beforehand.

use std::sync::Mutex;
use std::time::{Duration, Instant};

use crate::game::{self, Vector};
use crate::hud;
use crate::log;
use crate::net::protocol::PlayerState;
use crate::room::{self, Pick, RoomId};

const GO_TO_CHAO_AREA: usize = 0x0052_B5B0;
/// Includes leaving Chao World (and its save), opening the menus and loading the stage.
const PENDING_LIMIT: Duration = Duration::from_secs(120);
const BESIDE: f32 = 15.0;
/// How long a raised Chao area change may take to start before travel gives up.
const AREA_CHANGE_LIMIT: Duration = Duration::from_secs(15);

struct Journey {
    player: u8,
    since: Instant,
    /// The Chao area change already raised, and when. It's raised once, not every frame.
    area_change: Option<(i8, Instant)>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum AreaStep {
    Raise,
    Wait,
    TimedOut,
}

fn area_step(raised: Option<(i8, Instant)>, area: i8, now: Instant) -> AreaStep {
    match raised {
        Some((pending, at)) if pending == area => {
            if now.duration_since(at) > AREA_CHANGE_LIMIT {
                AreaStep::TimedOut
            } else {
                AreaStep::Wait
            }
        }
        // Nothing raised yet, or the friend moved on to another area meanwhile.
        _ => AreaStep::Raise,
    }
}

static PENDING: Mutex<Option<Journey>> = Mutex::new(None);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Destination {
    Stage(Pick),
    Chao(i8),
}

impl Destination {
    fn from_state(state: &PlayerState) -> Option<Self> {
        if state.level == game::LEVEL_CHAO_WORLD {
            // Races and karate need their own selected Chao and match setup; teleporting
            // directly into those arenas would bypass it. Their entrances remain reachable.
            return matches!(state.chao_area, 0..=3 | 5..=9).then_some(Self::Chao(state.chao_area));
        }
        let pick = Pick {
            level: state.level,
            mission: state.mission,
            character: state.character,
        };
        room::valid_pick(pick).then_some(Self::Stage(pick))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Place {
    level: i16,
    mission: u8,
    area: i32,
}

#[derive(Clone, Copy, Debug, Default)]
struct Here {
    /// Only live/paused gameplay with a valid local player counts as a loaded place.
    place: Option<Place>,
    stage_select: bool,
    menus: bool,
    holding: bool,
}

impl Here {
    fn now() -> Self {
        let mode = game::game_mode();
        Self {
            place: (matches!(mode, 5 | 10)
                && matches!(game::game_state(), 16 | 17)
                && !game::local_entity().is_null())
            .then(|| Place {
                level: game::current_level(),
                mission: game::mission(),
                area: game::current_chao_area(),
            }),
            stage_select: room::stage_select_ready(),
            menus: mode == 0xE,
            holding: crate::together::is_holding(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Action {
    Wait,
    Arrive,
    LeaveStage,
    OpenStageSelect,
    Load(Pick),
    ChangeArea(i8),
    NeedChaoWorld,
}

/// Decide from loaded gameplay, never from CurrentLevel alone: that global can already
/// name the destination while the previous stage is still fading or the next one is loading.
fn route(here: Here, target: Destination, loading: bool, same_party: bool) -> Action {
    if loading {
        return Action::Wait;
    }
    match target {
        Destination::Stage(pick) => {
            if let Some(place) = here.place {
                if place.level == pick.level && place.mission == pick.mission && same_party {
                    return if here.holding {
                        Action::Wait
                    } else {
                        Action::Arrive
                    };
                }
                // Changing parties reloads even the same stage: its objects must come from
                // this party's history, rather than our previous solo/party run's leftovers.
                return Action::LeaveStage;
            }
            if here.stage_select {
                Action::Load(pick)
            } else if here.menus {
                Action::OpenStageSelect
            } else {
                Action::Wait
            }
        }
        Destination::Chao(area) => match here.place {
            Some(place) if place.level == game::LEVEL_CHAO_WORLD => {
                if place.area == i32::from(area) {
                    Action::Arrive
                } else {
                    Action::ChangeArea(area)
                }
            }
            _ => Action::NeedChaoWorld,
        },
    }
}

fn beside(state: &PlayerState) -> Vector {
    let [x, y, z] = state.position;
    Vector {
        x: x + BESIDE,
        y: y + 2.0,
        z,
    }
}

fn same_party(state: &PlayerState) -> bool {
    room::party_owner() == RoomId::from_wire(state.room.0, state.room.1).map(|room| room.owner)
}

fn friend(id: u8) -> Option<(String, Option<PlayerState>)> {
    crate::online::others()
        .into_iter()
        .find(|(player, _, _)| *player == id)
        .map(|(_, name, state)| (name, state))
}

/// F9: queue the entire trip. Re-selecting a friend replaces any earlier trip.
pub fn go_to(id: u8) -> String {
    // A new choice replaces the previous destination even when that friend just left.
    PENDING.lock().unwrap_or_else(|e| e.into_inner()).take();
    let Some((name, Some(state))) = friend(id) else {
        return "Your friend isn't available yet".into();
    };
    if game::two_player() {
        return "Go to is available in single-player online play".into();
    }
    let Some(target) = Destination::from_state(&state) else {
        return if state.level == game::LEVEL_CHAO_WORLD {
            format!("{name} is in a race or battle: meet them at its entrance")
        } else {
            format!("{name} isn't in a stage yet")
        };
    };
    let action = route(
        Here::now(),
        target,
        state.flags & game::POSE_LOADING != 0,
        same_party(&state),
    );
    if action == Action::NeedChaoWorld {
        return format!("{name} is in Chao World: enter Chao World first");
    }
    *PENDING.lock().unwrap_or_else(|e| e.into_inner()) = Some(Journey {
        player: id,
        since: Instant::now(),
        area_change: None,
    });
    log!("goto: travelling to {name}: {target:?}");
    crate::battles::close_for_travel();
    // Start on the same frame as the menu choice, including when the game is paused.
    advance().unwrap_or_else(|| format!("Joining {name}'s game"))
}

pub fn on_frame() {
    if let Some(message) = advance() {
        hud::toast(message);
    }
}

fn advance() -> Option<String> {
    // Native exits can invoke other callbacks. Never keep our lock while calling the engine
    // or other systems, and never keep an object pointer across an unloading/loading frame.
    let mut journey = PENDING.lock().unwrap_or_else(|e| e.into_inner()).take()?;
    if journey.since.elapsed() > PENDING_LIMIT {
        return Some("Couldn't reach your friend: travel timed out".into());
    }
    let Some((name, Some(state))) = friend(journey.player) else {
        return Some("Your friend left".into());
    };
    if game::two_player() {
        return Some("Go to cancelled: local two-player play started".into());
    }
    let Some(target) = Destination::from_state(&state) else {
        return Some(format!("{name} isn't in a joinable stage any more"));
    };
    let action = route(
        Here::now(),
        target,
        state.flags & game::POSE_LOADING != 0,
        same_party(&state),
    );
    match action {
        Action::Arrive => {
            if let Err(message) = room::join_friend(journey.player) {
                return Some(message.into());
            }
            game::set_player_position(beside(&state));
            log!("goto: arrived next to {name}");
            return Some(format!("Moved next to {name}"));
        }
        Action::LeaveStage => {
            game::leave_for_friend();
        }
        Action::OpenStageSelect => {
            game::open_stage_select_for_friend();
        }
        Action::Load(pick) => {
            if let Err(message) = room::join_friend(journey.player) {
                return Some(message.into());
            }
            if room::go_to_friend(pick) {
                hud::toast(format!(
                    "Loading {} to join {name}",
                    crate::places::place(pick.level, 0)
                ));
                log!("goto: loading {pick:?} for {name}");
            }
        }
        Action::ChangeArea(area) => match area_step(journey.area_change, area, Instant::now()) {
            AreaStep::TimedOut => {
                return Some("Couldn't reach your friend: the area change didn't start".into());
            }
            AreaStep::Wait => {}
            // GoToChaoArea raises a native area-change request, which the pause state does
            // not dispatch. Wait for Resume rather than queueing it again each paused frame.
            AreaStep::Raise if game::game_state() == game::GAME_STATE_INGAME => {
                if let Err(message) = room::join_friend(journey.player) {
                    return Some(message.into());
                }
                // SAFETY: the native area transition, only from live Chao World into an
                // ordinary area. Its own load path owns teardown and the new objects.
                unsafe {
                    std::mem::transmute::<usize, extern "C" fn(i32)>(GO_TO_CHAO_AREA)(i32::from(
                        area,
                    ));
                }
                journey.area_change = Some((area, Instant::now()));
            }
            AreaStep::Raise => {}
        },
        // The raised area change is loading: Chao World isn't live gameplay meanwhile.
        Action::NeedChaoWorld
            if journey
                .area_change
                .is_some_and(|(_, at)| at.elapsed() <= AREA_CHANGE_LIMIT) => {}
        Action::NeedChaoWorld => {
            return Some(format!(
                "{name} moved to Chao World: enter Chao World to follow"
            ));
        }
        Action::Wait => {}
    }
    *PENDING.lock().unwrap_or_else(|e| e.into_inner()) = Some(journey);
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    const PICK: Pick = Pick {
        level: 3,
        mission: 2,
        character: 4,
    };

    fn in_stage(level: i16, mission: u8) -> Here {
        Here {
            place: Some(Place {
                level,
                mission,
                area: 0,
            }),
            ..Here::default()
        }
    }

    #[test]
    fn a_first_visit_exits_the_old_stage_and_loads_through_fresh_menus() {
        let destination = Destination::Stage(PICK);
        assert_eq!(
            route(in_stage(10, 0), destination, false, true),
            Action::LeaveStage
        );
        // No cached Stage Select or previously loaded destination is part of this route.
        assert_eq!(
            route(Here::default(), destination, false, true),
            Action::Wait
        );
        assert_eq!(
            route(
                Here {
                    menus: true,
                    ..Here::default()
                },
                destination,
                false,
                true
            ),
            Action::OpenStageSelect
        );
        assert_eq!(
            route(
                Here {
                    menus: true,
                    stage_select: true,
                    ..Here::default()
                },
                destination,
                false,
                true
            ),
            Action::Load(PICK)
        );
        assert_eq!(
            route(in_stage(PICK.level, PICK.mission), destination, false, true),
            Action::Arrive
        );
    }

    #[test]
    fn stale_level_numbers_and_loading_targets_cannot_complete_travel() {
        let target = Destination::Stage(PICK);
        assert_eq!(route(Here::default(), target, false, true), Action::Wait);
        assert_eq!(
            route(in_stage(PICK.level, PICK.mission), target, true, true),
            Action::Wait
        );
        assert_eq!(
            route(in_stage(PICK.level, 0), target, false, true),
            Action::LeaveStage
        );
        let mut here = in_stage(PICK.level, PICK.mission);
        here.holding = true;
        assert_eq!(route(here, target, false, true), Action::Wait);
    }

    #[test]
    fn switching_parties_reloads_even_when_the_friend_is_in_the_same_stage() {
        assert_eq!(
            route(
                in_stage(PICK.level, PICK.mission),
                Destination::Stage(PICK),
                false,
                false
            ),
            Action::LeaveStage
        );
    }

    #[test]
    fn a_changing_destination_is_resolved_again_before_loading() {
        let here = Here {
            stage_select: true,
            menus: true,
            ..Here::default()
        };
        let changed = Pick {
            level: 14,
            mission: 1,
            ..PICK
        };
        assert_eq!(
            route(here, Destination::Stage(changed), false, true),
            Action::Load(changed)
        );
    }

    #[test]
    fn chao_world_leaves_through_native_save_flow_for_stage_travel() {
        let here = Here {
            place: Some(Place {
                level: game::LEVEL_CHAO_WORLD,
                mission: 0,
                area: 1,
            }),
            ..Here::default()
        };
        assert_eq!(
            route(here, Destination::Stage(PICK), false, true),
            Action::LeaveStage
        );
        assert_eq!(
            route(here, Destination::Chao(0), false, true),
            Action::ChangeArea(0)
        );
        assert_eq!(
            route(here, Destination::Chao(1), false, true),
            Action::Arrive
        );
    }

    #[test]
    fn an_area_change_is_raised_once_then_waits_until_it_times_out() {
        let at = Instant::now();
        assert_eq!(area_step(None, 2, at), AreaStep::Raise);
        let raised = Some((2, at));
        assert_eq!(
            area_step(raised, 2, at + Duration::from_secs(1)),
            AreaStep::Wait
        );
        assert_eq!(
            area_step(raised, 2, at + AREA_CHANGE_LIMIT + Duration::from_secs(1)),
            AreaStep::TimedOut
        );
        // The friend moved again before ours started: follow the new area instead.
        assert_eq!(
            area_step(raised, 3, at + Duration::from_secs(1)),
            AreaStep::Raise
        );
    }
}
