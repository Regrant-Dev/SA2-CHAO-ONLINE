//! SA2 Chao Online: mod DLL loaded by SA2 Mod Loader.

mod battles;
pub mod chao;
mod chat;
mod coop;
pub mod game;
mod garden;
mod goto;
mod hud;
mod hunt;
mod input;
mod items;
mod karate;
mod karate_live;
mod kart;
mod log;
mod menu;
mod modloader;
pub mod net;
mod online;
mod patch;
pub mod places;
mod puppet;
mod race;
mod room;
mod stage_race;
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
        #[cfg(feature = "dumpexe")]
        dump_text(&path);
    });
    hud::init(helpers);
    // SAFETY: the loader's helper returns a NUL-terminated path (or null).
    let chao_save = unsafe { (helpers.get_chao_save_path)() };
    if !chao_save.is_null() {
        // SAFETY: as above, checked for null.
        let chao_save = unsafe { CStr::from_ptr(chao_save) }
            .to_string_lossy()
            .into_owned();
        log!("chao save: {chao_save}");
        game::set_chao_save_path(chao_save);
    }
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
        garden::on_frame();
        let (peers, connected) = online::peers();
        together::on_frame(&peers, connected);
        coop::on_frame(&peers, connected);
        goto::on_frame();
        stage_race::on_frame(&peers, connected);
        kart::on_frame(stage_race::others_racing());
        {
            // Room players in our stage, for the treasure hunt battle.
            let members = room::members();
            let (level, mission) = (game::current_level(), game::mission());
            let here: Vec<u8> = peers
                .iter()
                .filter(|p| members.contains(&p.id) && p.level == level && p.mission == mission)
                .map(|p| p.id)
                .collect();
            hunt::on_frame(online::my_id(), &here);
        }
        room::on_frame(connected);
        race::start_frame(&peers, connected);
        race::on_frame();
        race::drive_frame();
        // Pausing doesn't stop play that others are in.
        game::set_live_pause(together::live() || race::live() || karate_live::live());
        game::restore_after_background();
        // Online: the main menu's 1P entry reads MULTIPLAYER (online play goes through the 1P
        // modes), 2P opens the battles picker, and the status shows at the bottom.
        game::block_two_player_menu(connected);
        if connected && let Some((x, y, selected)) = game::main_menu_1p_label() {
            // (x, y) is the label's centre; cover the whole 1P button (about 314 x 50).
            hud::menu_label(x, y, 318.0, 52.0, "MULTIPLAYER", selected);
        }
        // Online, local two-player (split screen) can't be picked: its button reads ROOM BATTLES
        // and opens the battles picker.
        if connected && let Some((x, y, selected)) = game::main_menu_2p_label() {
            hud::menu_label(x, y, 318.0, 52.0, "ROOM BATTLES", selected);
        }
        battles::on_frame(connected);
        if game::on_main_menu()
            && !battles::is_open()
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
        chat::on_frame(connected, menu::is_open());
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

/// Dev only (`--features dumpexe`): the game's code section as it is in memory, after Steam's
/// wrapper decrypted it (0x401000, raw size 0x46F600), for reverse engineering.
#[cfg(feature = "dumpexe")]
fn dump_text(path: &str) {
    const TEXT: usize = 0x0040_1000;
    const TEXT_SIZE: usize = 0x0046_F600;
    // SAFETY: the exe's code section is mapped and readable for the whole run.
    let bytes = unsafe { std::slice::from_raw_parts(TEXT as *const u8, TEXT_SIZE) };
    let out = format!("{path}/sonic2app.text.bin");
    match std::fs::write(&out, bytes) {
        Ok(()) => log!("dumpexe: wrote {out}"),
        Err(e) => log!("dumpexe: {out}: {e}"),
    }
}
