//! PARTY BATTLES: online, the main menu's 2P entry opens this picker (the game's own 2P handler
//! returns without doing anything, see `game::block_two_player_menu`). It's drawn over the
//! main menu and driven by the game's menu buttons while the main menu waits.

use std::sync::Mutex;

use crate::game;
use crate::hud;
use crate::log;

/// Menu buttons (`MenuButtons_Pressed`): up, down, A/Start, B.
const UP: u32 = 0x10;
const DOWN: u32 = 0x20;
const CONFIRM: u32 = 0x0C;
const BACK: u32 = 0x02;

struct Choice {
    label: &'static str,
    note: &'static str,
}

const CHOICES: [Choice; 3] = [
    Choice {
        label: "KART RACE",
        note: "Pick a course: your party races along, no CPU karts",
    },
    Choice {
        label: "TREASURE HUNT",
        note: "Stage Select: pick a Knuckles or Rouge hunt stage, start together",
    },
    Choice {
        label: "BACK",
        note: "Chao Race and Karate: in Chao World, at the stadium",
    },
];

struct Picker {
    open: bool,
    cursor: usize,
    /// Frames since it opened (the opening press isn't a pick).
    age: u32,
}

static PICKER: Mutex<Picker> = Mutex::new(Picker {
    open: false,
    cursor: 0,
    age: 0,
});

/// Call every frame (game thread).
pub fn on_frame(connected: bool) {
    // F9 owns its confirmation keys; Go to must not also start a kart race underneath it.
    if crate::menu::is_open() {
        return;
    }
    let mut p = PICKER.lock().unwrap_or_else(|e| e.into_inner());
    let buttons = game::menu_buttons_pressed();
    if !p.open {
        if connected
            && game::main_menu_2p_label().is_some_and(|(_, _, selected)| selected)
            && buttons & CONFIRM != 0
            && game::hold_main_menu(true)
        {
            log!("battles: picker open");
            *p = Picker {
                open: true,
                cursor: 0,
                age: 0,
            };
        }
        return;
    }
    if !game::on_main_menu() || !connected {
        p.open = false;
        game::hold_main_menu(false);
        return;
    }
    p.age += 1;
    if p.age > 1 {
        if buttons & UP != 0 {
            p.cursor = p.cursor.saturating_sub(1);
        } else if buttons & DOWN != 0 {
            p.cursor = (p.cursor + 1).min(CHOICES.len() - 1);
        } else if buttons & BACK != 0 {
            p.cursor = CHOICES.len() - 1;
            p.open = false;
        } else if buttons & CONFIRM != 0 {
            p.open = false;
        }
        if !p.open {
            game::hold_main_menu(false);
            if p.cursor < 2 {
                ensure_room();
            }
            match p.cursor {
                0 => {
                    log!("battles: Kart Race");
                    game::leave_main_menu(game::PANEL_KART);
                }
                1 => {
                    log!("battles: Treasure Hunt -> Stage Select");
                    game::leave_main_menu(game::PANEL_STAGE_SELECT);
                    hud::toast(
                        "Treasure hunt stages: Wild Canyon, Pumpkin Hill, Aquatic Mine, \
                         Death Chamber, Meteor Herd (Knuckles); Dry Lagoon, Egg Quarters, \
                         Security Hall, Mad Space (Rouge)",
                    );
                }
                _ => {}
            }
            return;
        }
    }
    // Drawn over the main menu's buttons, in its 640x480 space.
    for (i, c) in CHOICES.iter().enumerate() {
        hud::menu_label(
            320.0,
            150.0 + 60.0 * i as f32,
            318.0,
            52.0,
            c.label,
            i == p.cursor,
        );
    }
    hud::banner(CHOICES[p.cursor].note);
}

/// Battles are played in a room: outside one, join an open room (friends likely made it), or
/// make one.
fn ensure_room() {
    if crate::room::in_room() {
        return;
    }
    let choices = crate::room::choices();
    let pick = choices
        .iter()
        .find(|(c, _)| matches!(c, crate::room::Choice::Join(_)))
        .or_else(|| {
            choices
                .iter()
                .find(|(c, _)| *c == crate::room::Choice::Create)
        });
    if let Some((choice, _)) = pick {
        let said = crate::room::choose(*choice);
        log!("battles: {said}");
        hud::toast(said);
    }
}

/// The picker is showing (other main-menu overlays step aside).
pub fn is_open() -> bool {
    PICKER.lock().unwrap_or_else(|e| e.into_inner()).open
}

/// A Go to request supersedes this menu without confirming its current battle choice.
pub fn close_for_travel() {
    let open = {
        let mut p = PICKER.lock().unwrap_or_else(|e| e.into_inner());
        std::mem::replace(&mut p.open, false)
    };
    if open {
        game::hold_main_menu(false);
    }
}
