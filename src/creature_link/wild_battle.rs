//! The battle button on a wild visitor's info card (`battle::wild`, re/shared-visitors.md
//! section 6): with the card showing a wild visitor and the player's hands empty, Y takes the
//! player into the independent creature battle scene to battle it.
//!
//! In a garden the player has alone, the visit is kept for when they come back
//! (`wild::snapshot`), and the battle's end says whether that visitor is still in it
//! (`wild::battle_outcome`). In a shared garden the host is asked first (`Body::WildBattle`
//! with `WildStep::Reserve`, answered by `WildAnswer`): it keeps the visitor for the player
//! while they're away (others see it busy) and is told how the battle ended. The game's copies
//! of shared visitors (`set_shared`) say which can be battled.
//!
//! Partners from the link machine aren't battled: only wild visitors.

// Off the game (Linux builds) only the tests use the text side.
#![cfg_attr(not(target_arch = "x86"), allow(dead_code))]

use std::sync::Mutex;

use crate::battle::wild::{Battled, Ending};
use crate::net::protocol::{Body, WildStep};

/// The host's answer to asking for a battle is waited for this long.
const ASK_SECS: u64 = 3;

/// One of the game's copies of a shared visitor: its object, its host id, whether it's free
/// (roaming or swimming: nobody holds or battles it) and whether it's a wild one (not a
/// player's partner).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SharedCopy {
    pub object: usize,
    pub id: u16,
    pub free: bool,
    pub wild: bool,
}

static SHARED: Mutex<Vec<SharedCopy>> = Mutex::new(Vec::new());
/// The host's answers to our asking (visitor id, granted), for the next frame.
static ANSWERS: Mutex<Vec<(u16, bool)>> = Mutex::new(Vec::new());

/// The game's copies of the shared visitors in the player's garden now (none outside one).
pub fn set_shared(copies: Vec<SharedCopy>) {
    *SHARED.lock().unwrap_or_else(|e| e.into_inner()) = copies;
}

fn shared_copy(object: usize) -> Option<SharedCopy> {
    SHARED
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .iter()
        .find(|c| c.object == object)
        .copied()
}

/// From the host (`WildAnswer` for a battle): whether we may battle visitor `id`.
pub fn answered(id: u16, ok: bool) {
    ANSWERS
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .push((id, ok));
}

/// What the card's creature is for a battle.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    /// One of the player's own wild visitors (a garden they have alone).
    Own,
    /// A shared wild visitor nobody holds or battles.
    Shared(u16),
    /// Not one to battle now (a partner, a copy someone holds or battles).
    No,
}

/// The card's line about battling: the button when a battle can start, the wait while the host
/// is asked.
fn line(can: bool, asking: bool) -> Option<&'static str> {
    if asking {
        Some("Asking to battle...")
    } else if can {
        Some("Y: Battle")
    } else {
        None
    }
}

/// The battle with a visitor ended (`battle::wild`): alone, the kept visit learns whether that
/// one stays; shared, the host is told.
pub fn over(battled: Battled, ending: Ending) {
    match battled {
        Battled::Own => {
            #[cfg(target_arch = "x86")]
            super::wild::battle_outcome(ending.stays());
        }
        Battled::Shared(id) => crate::battle::online::send(vec![step(id, ending.step())]),
    }
}

fn step(id: u16, step: WildStep) -> Body {
    Body::WildBattle { id, step }
}

#[cfg(target_arch = "x86")]
pub use game::{card, on_frame};

#[cfg(not(target_arch = "x86"))]
pub fn on_frame() {}

#[cfg(target_arch = "x86")]
mod game {
    use std::sync::Mutex;
    use std::time::{Duration, Instant};

    use super::super::store::Partner;
    use super::{ANSWERS, ASK_SECS, Kind, line, shared_copy, step};
    use crate::battle::wild::{self, Battled, Trip, WildFoe};
    use crate::game as sa2;
    use crate::log;
    use crate::net::protocol::WildStep;

    /// Player 1's raw pad (`*0x1DEFB60`): `+0x08` held buttons; Y is 0x200 (re/chao-details.md
    /// "Controller buttons in the garden": Y does nothing while the player stands).
    const PAD1: usize = 0x01DE_FB60;
    const PAD_HELD: usize = 0x08;
    const BUTTON_Y: u32 = 0x200;

    /// Asking the host to battle shared visitor `id`.
    struct Asking {
        id: u16,
        foe: WildFoe,
        garden: u8,
        since: Instant,
    }

    struct State {
        /// Y held last frame, and pressed this frame.
        y_held: bool,
        y_pressed: bool,
        asking: Option<Asking>,
    }

    static STATE: Mutex<State> = Mutex::new(State {
        y_held: false,
        y_pressed: false,
        asking: None,
    });

    fn lock() -> std::sync::MutexGuard<'static, State> {
        STATE.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn y_held() -> bool {
        // SAFETY: the game's pad pointer (null-checked) and its held buttons.
        unsafe {
            let pad = (PAD1 as *const usize).read_volatile();
            pad != 0 && ((pad + PAD_HELD) as *const u32).read_volatile() & BUTTON_Y != 0
        }
    }

    /// The player can set off now: playing in a garden, controls on, no menu or chat, hands
    /// empty, and no independent creature battle already open. No Battle DLC is required.
    fn can_go() -> bool {
        sa2::in_garden()
            && !sa2::between_areas()
            && sa2::game_state() == sa2::GAME_STATE_INGAME
            // SAFETY: a static byte in the game's data section.
            && unsafe { (sa2::CONTROLLER_ENABLED as *const u8).read_volatile() } != 0
            && crate::input::game_focused()
            && !crate::menu::is_open()
            && !crate::chat::typing()
            && crate::garden::player_held() == 0
            && sa2::local_player().is_some_and(|p| !p.holding)
            && !wild::underway()
            && !crate::battle::game::is_open()
    }

    fn kind(object: usize, garden: u8) -> Kind {
        if let Some(c) = shared_copy(object) {
            return if c.wild && c.free {
                Kind::Shared(c.id)
            } else {
                Kind::No
            };
        }
        let alone = !crate::garden::server_runs() && !crate::bring::friends_in(i32::from(garden));
        if alone && super::super::wild::is_visitor(object) {
            Kind::Own
        } else {
            Kind::No
        }
    }

    /// Every frame (before the card): the Y button's edge, the host's answers and the wait.
    pub fn on_frame() {
        let held = y_held();
        let answers = std::mem::take(&mut *ANSWERS.lock().unwrap_or_else(|e| e.into_inner()));
        let mut s = lock();
        s.y_pressed = held && !s.y_held;
        s.y_held = held;
        for (id, ok) in answers {
            let asked = s.asking.take_if(|a| a.id == id);
            match (asked, ok) {
                (Some(a), true) => set_off_shared(a),
                (Some(_), false) => crate::hud::toast("Someone else is with that visitor"),
                // A late yes we no longer wait for.
                (None, true) => crate::battle::online::send(vec![step(id, WildStep::Cancelled)]),
                (None, false) => {}
            }
        }
        if s.asking
            .as_ref()
            .is_some_and(|a| a.since.elapsed() > Duration::from_secs(ASK_SECS))
            && let Some(a) = s.asking.take()
        {
            crate::battle::online::send(vec![step(a.id, WildStep::Cancelled)]);
            crate::hud::toast("No answer from the host: try again");
        }
    }

    /// The host keeps the visitor for us: off to the arena, if the player still can go.
    fn set_off_shared(a: Asking) {
        let here = sa2::current_chao_area() == i32::from(a.garden);
        let started = here && can_go() && set_off(a.garden, a.foe, Battled::Shared(a.id));
        if !started {
            crate::battle::online::send(vec![step(a.id, WildStep::Cancelled)]);
        }
    }

    fn set_off(garden: u8, foe: WildFoe, battled: Battled) -> bool {
        let Some((at, angle)) = sa2::player_position() else {
            return false;
        };
        wild::start(Trip::new(garden, [at.x, at.y, at.z], angle, foe, battled))
    }

    /// The card shows `object`, which teaches `partner`: its battle line, and Y battles it.
    pub fn card(object: usize, partner: &Partner, shiny: bool) -> Option<&'static str> {
        let mut s = lock();
        let garden = sa2::current_chao_area() as u8;
        if let Some(a) = &s.asking {
            return line(false, a.garden == garden);
        }
        let kind = kind(object, garden);
        let can = kind != Kind::No && can_go();
        if can && std::mem::take(&mut s.y_pressed) {
            if !crate::battle::game::has_chao() {
                crate::hud::toast("You have no Chao to battle with");
                return line(can, false);
            }
            let foe = WildFoe::of(partner, shiny);
            match kind {
                Kind::Own => {
                    if super::super::wild::snapshot(object) && !set_off(garden, foe, Battled::Own) {
                        super::super::wild::battle_outcome(true);
                    }
                }
                Kind::Shared(id) => {
                    log!("creature link: asking the host to battle shared visitor {id}");
                    crate::battle::online::send(vec![step(id, WildStep::Reserve)]);
                    s.asking = Some(Asking {
                        id,
                        foe,
                        garden,
                        since: Instant::now(),
                    });
                    return line(false, true);
                }
                Kind::No => {}
            }
        }
        line(can, false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_card_line() {
        assert_eq!(line(true, false), Some("Y: Battle"));
        assert_eq!(line(false, false), None);
        assert_eq!(line(false, true), Some("Asking to battle..."));
        assert_eq!(line(true, true), Some("Asking to battle..."));
    }

    #[test]
    fn a_shared_battle_tells_the_host_how_it_ended() {
        assert_eq!(
            step(7, Ending::RanAway.step()),
            Body::WildBattle {
                id: 7,
                step: WildStep::Lost
            }
        );
    }

    #[test]
    fn copies_say_which_can_be_battled() {
        set_shared(vec![SharedCopy {
            object: 0x1000,
            id: 3,
            free: true,
            wild: true,
        }]);
        assert_eq!(shared_copy(0x1000).map(|c| c.id), Some(3));
        assert!(shared_copy(0x2000).is_none());
        set_shared(Vec::new());
        assert!(shared_copy(0x1000).is_none());
    }
}
