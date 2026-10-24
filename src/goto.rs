//! "Go to friend" (F9 > Go to): join another player where they are.
//!
//! - Same stage (and mission) or same garden: we're moved next to them.
//! - We're on Stage Select, they're in a stage: their stage and mission load (as their
//!   character, like a room pick), then we're moved next to them.
//! - We're in Chao World, they're in a stage: we leave Chao World the way its exit does, and
//!   once Stage Select is up their stage loads as above.
//! - We're in Chao World, they're in another area of it: we go to that area, then next to them.
//! - Otherwise a message says where to go first.

use std::sync::Mutex;
use std::time::{Duration, Instant};

use crate::game::{self, Vector};
use crate::hud;
use crate::log;
use crate::net::protocol::PlayerState;

/// `GoToChaoArea(area)` 0x52B5B0 (cdecl).
const GO_TO_CHAO_AREA: usize = 0x0052_B5B0;
/// Give up on getting next to them after this long (loading included).
const PENDING_LIMIT: Duration = Duration::from_secs(60);
/// Where we land, from them (not inside them).
const BESIDE: f32 = 15.0;

/// Waiting to arrive where they are, then to be moved next to them.
static PENDING: Mutex<Option<(u8, Instant)>> = Mutex::new(None);
/// Leaving Chao World to join them in their stage: (them, since, frames on Stage Select).
static LEAVING: Mutex<Option<(u8, Instant, u32)>> = Mutex::new(None);
/// `GoToChaoArea`'s area that leaves Chao World for the menus (as its exits and karate's do).
const LEAVE_CHAO_WORLD: i32 = 1;
/// Frames Stage Select shows before their stage is picked (it slides in first).
const SETTLE_FRAMES: u32 = 45;

/// The same place: stage and mission, or Chao World area.
fn same_place(s: &PlayerState) -> bool {
    let level = game::current_level();
    if s.level != level || level <= 0 {
        return false;
    }
    if level == game::LEVEL_CHAO_WORLD {
        i32::from(s.chao_area) == game::current_chao_area()
    } else {
        s.mission == game::mission()
    }
}

fn beside(s: &PlayerState) -> Vector {
    let [x, y, z] = s.position;
    Vector {
        x: x + BESIDE,
        y: y + 2.0,
        z,
    }
}

/// F9: go to player `id`. Returns what happened, for the menu message.
pub fn go_to(id: u8) -> String {
    let Some((_, name, state)) = crate::online::others()
        .into_iter()
        .find(|(i, _, _)| *i == id)
    else {
        return "They left".into();
    };
    let Some(s) = state else {
        return format!("{name} hasn't started yet");
    };
    let place = crate::places::place(s.level, s.chao_area);
    if same_place(&s) && game::game_state() == game::GAME_STATE_INGAME {
        game::set_player_position(beside(&s));
        log!("goto: next to {name} in {place}");
        return format!("Moved next to {name}");
    }
    let stage = s.level > 0 && s.level != game::LEVEL_CHAO_WORLD;
    if stage && crate::room::on_stage_select() && !crate::room::stage_select_ready() {
        return "Stage Select is busy: try again in a moment".into();
    }
    if stage && crate::room::stage_select_ready() {
        crate::room::go(crate::room::Pick {
            level: s.level,
            mission: s.mission,
            character: s.character,
        });
        *PENDING.lock().unwrap_or_else(|e| e.into_inner()) = Some((id, Instant::now()));
        log!(
            "goto: loading {place} (mission {}) to join {name}",
            s.mission + 1
        );
        return format!("Going to {name} in {place}");
    }
    if s.level == game::LEVEL_CHAO_WORLD
        && game::current_level() == game::LEVEL_CHAO_WORLD
        && game::game_state() == game::GAME_STATE_INGAME
        && s.chao_area > 0
    {
        // SAFETY: the game's cdecl GoToChaoArea, as its own exits call it.
        unsafe {
            let go: extern "C" fn(i32) = std::mem::transmute(GO_TO_CHAO_AREA);
            go(i32::from(s.chao_area));
        }
        *PENDING.lock().unwrap_or_else(|e| e.into_inner()) = Some((id, Instant::now()));
        log!("goto: going to {place} to join {name}");
        return format!("Going to {name} in {place}");
    }
    if stage
        && game::current_level() == game::LEVEL_CHAO_WORLD
        && game::game_state() == game::GAME_STATE_INGAME
    {
        // SAFETY: the game's cdecl GoToChaoArea, as Chao World's own exits call it.
        unsafe {
            let go: extern "C" fn(i32) = std::mem::transmute(GO_TO_CHAO_AREA);
            go(LEAVE_CHAO_WORLD);
        }
        *LEAVING.lock().unwrap_or_else(|e| e.into_inner()) = Some((id, Instant::now(), 0));
        log!("goto: leaving Chao World to join {name} in {place}");
        return format!("Leaving Chao World to join {name} in {place}");
    }
    if stage {
        format!("{name} is in {place}: go to Stage Select first")
    } else if s.level == game::LEVEL_CHAO_WORLD {
        format!("{name} is in {place}: go to Chao World first")
    } else {
        format!("{name} is in the menus")
    }
}

/// Call every frame: once we've arrived where they are, moves us next to them.
pub fn on_frame() {
    leaving_frame();
    let mut pending = PENDING.lock().unwrap_or_else(|e| e.into_inner());
    let Some((id, since)) = *pending else { return };
    if since.elapsed() > PENDING_LIMIT {
        *pending = None;
        return;
    }
    if game::game_state() != game::GAME_STATE_INGAME || crate::together::is_holding() {
        return;
    }
    let Some(s) = crate::online::others()
        .into_iter()
        .find(|(i, _, _)| *i == id)
        .and_then(|(_, _, s)| s)
    else {
        *pending = None;
        return;
    };
    if same_place(&s) {
        game::set_player_position(beside(&s));
        hud::toast("Moved next to your friend");
        *pending = None;
    } else if game::current_level() > 0 && game::current_level() != s.level {
        log!(
            "goto: the game loaded level {} instead of their level {}",
            game::current_level(),
            s.level
        );
        hud::toast("Couldn't load their stage");
        *pending = None;
    }
}

/// Leaving Chao World for a friend's stage: once Stage Select has settled, their stage loads
/// (wherever they are by then); nothing happens if we end up elsewhere.
fn leaving_frame() {
    let mut leaving = LEAVING.lock().unwrap_or_else(|e| e.into_inner());
    let Some((id, since, frames)) = *leaving else {
        return;
    };
    if since.elapsed() > PENDING_LIMIT {
        *leaving = None;
        return;
    }
    if !crate::room::stage_select_ready() {
        return;
    }
    if frames < SETTLE_FRAMES {
        *leaving = Some((id, since, frames + 1));
        return;
    }
    *leaving = None;
    let Some((_, name, Some(s))) = crate::online::others()
        .into_iter()
        .find(|(i, _, _)| *i == id)
    else {
        hud::toast("Your friend left");
        return;
    };
    if s.level <= 0 || s.level == game::LEVEL_CHAO_WORLD {
        hud::toast(format!("{name} isn't in a stage any more"));
        return;
    }
    let place = crate::places::place(s.level, s.chao_area);
    crate::room::go(crate::room::Pick {
        level: s.level,
        mission: s.mission,
        character: s.character,
    });
    *PENDING.lock().unwrap_or_else(|e| e.into_inner()) = Some((id, Instant::now()));
    log!(
        "goto: loading {place} (mission {}) to join {name}",
        s.mission + 1
    );
    hud::toast(format!("Going to {name} in {place}"));
}
