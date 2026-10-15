//! SA2 Chao Online: mod DLL loaded by SA2 Mod Loader.

pub mod chao;
pub mod game;
mod hud;
mod input;
mod karate;
mod karate_live;
mod log;
mod menu;
mod modloader;
pub mod net;
mod online;
mod patch;
mod puppet;
mod race;
mod room;
mod timeline;
mod together;
mod visitors;

use std::ffi::{CStr, c_char};
use std::panic;
use std::sync::Mutex;

use input::Hotkey;
use modloader::{HelperFunctions, MOD_LOADER_VER, ModInfo};

const MOD_VERSION: &str = env!("CARGO_PKG_VERSION");

struct FrameState {
    frame: u32,
    level: i16,
    chao_area: i32,
    menu_key: Hotkey,
    start_key: Hotkey,
}

static STATE: Mutex<FrameState> = Mutex::new(FrameState {
    frame: 0,
    level: -1,
    chao_area: -1,
    menu_key: Hotkey::new(input::VK_F9),
    start_key: Hotkey::new(input::VK_F8),
});

#[unsafe(no_mangle)]
pub static SA2ModInfo: ModInfo = ModInfo::new(MOD_LOADER_VER);

/// Runs `f` and logs a panic instead of unwinding into the game.
fn guarded(name: &str, f: impl FnOnce() + panic::UnwindSafe) {
    if panic::catch_unwind(f).is_err() {
        log!("panic in {name}");
    }
}

#[unsafe(no_mangle)]
/// # Safety
/// Called by the mod loader with a valid NUL-terminated path.
pub unsafe extern "C" fn Init(path: *const c_char, helpers: &HelperFunctions) {
    // SAFETY: the loader passes a valid NUL-terminated path.
    let path = unsafe { CStr::from_ptr(path) }
        .to_string_lossy()
        .into_owned();
    log::open(&path);
    panic::set_hook(Box::new(|info| log!("{info}")));

    guarded("Init", || {
        log!("sa2-chao-online {MOD_VERSION} loaded");
        log!("mod path: {path}");
        log!("mod loader helper version: {}", helpers.version);
        log!("F9 opens the menu (multiplayer, start together, Chao tools)");
    });
    hud::init(helpers);
    game::install_freeze();
    karate::install();
    race::install();
    guarded("online init", || online::init(&path));
}

#[unsafe(no_mangle)]
pub extern "C" fn OnFrame() {
    guarded("OnFrame", || {
        let mut state = STATE.lock().unwrap_or_else(|e| e.into_inner());
        state.frame = state.frame.wrapping_add(1);

        // Log level/area changes instead of a per-second heartbeat.
        let (level, chao_area) = (game::current_level(), game::current_chao_area());
        if level != state.level || (level == game::LEVEL_CHAO_WORLD && chao_area != state.chao_area)
        {
            log!(
                "frame {}: level {level}, chao area {chao_area}",
                state.frame
            );
            state.level = level;
            state.chao_area = chao_area;
        }

        visitors::on_frame();
        let (peers, connected) = online::peers();
        together::on_frame(&peers, connected);
        room::on_frame(connected);
        race::start_frame(&peers, connected);
        race::on_frame();
        race::drive_frame();
        // Pausing doesn't stop play that others are in.
        game::set_live_pause(together::live() || race::live() || karate_live::live());
        game::restore_after_background();
        // Online: the main menu's 1P entry reads MULTIPLAYER (online play goes through the 1P
        // modes), 2P can't be picked, and the status shows at the bottom.
        game::block_two_player_menu(connected);
        if connected && let Some((x, y, selected)) = game::main_menu_1p_label() {
            // (x, y) is the label's centre; cover the whole 1P button (about 314 x 50).
            hud::menu_label(x, y, 318.0, 52.0, "MULTIPLAYER", selected);
        }
        if game::on_main_menu()
            && let Some(status) = online::status()
        {
            hud::banner(&format!("Multiplayer on: {status}  (F9)"));
        }
        // Online, controller changes don't stop the game with the "Reboot" dialog.
        game::block_reboot_dialog(connected);
        if connected {
            game::close_reboot_dialog();
        }
        online::on_frame();
        puppet::on_frame();
        menu::on_frame();
        hud::on_frame();

        if state.menu_key.pressed() {
            menu::toggle();
        }
        // F8: start what you're holding for the others (stage, race or live karate).
        if state.start_key.pressed() {
            together::start_now();
            race::start_now();
            karate_live::start_now();
        }
    });
}

#[unsafe(no_mangle)]
pub extern "C" fn OnExit() {
    guarded("OnExit", online::shutdown);
}
