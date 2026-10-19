//! SA2 Chao Online: mod DLL loaded by SA2 Mod Loader.

mod audio;
mod battles;
mod boot_art;
mod boss_follow;
mod bosses;
mod carried;
pub mod chao;
mod chao_details;
mod chat;
mod coop;
mod eggs;
mod enemies;
pub mod game;
mod garden;
mod garden_toys;
mod goto;
mod hide;
mod hud;
mod hunt;
mod input;
mod items;
mod karate;
mod karate_live;
mod kart;
mod keeper;
mod lockon;
mod log;
mod menu;
mod modloader;
pub mod net;
mod online;
mod patch;
pub mod places;
mod puppet;
mod race;
mod registry;
mod room;
mod saving;
mod shots;
mod skin_sync;
mod skins;
mod stage_race;
mod timeline;
mod together;
mod toys;
mod trees;
mod unlocks;
mod visitors;
mod voice;
mod watch;
mod whistle;

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
        log!(
            "build: {} (protocol {})",
            option_env!("GITHUB_SHA").unwrap_or("local"),
            net::protocol::PROTOCOL_VERSION
        );
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
        game::set_chao_save_path(chao_save);
    }
    game::install_freeze();
    game::install_frame_skip();
    karate::install();
    voice::install();
    hide::install();
    race::install();
    garden::install();
    game::prepare_loader_profile(&path);
    guarded("online init", || online::init(&path));
    guarded("skins init", || skins::init(&path));
    guarded("audio init", || audio::init(&path));
    guarded("coop init", || coop::init(&path));
    guarded("boot art", || boot_art::install(&path));
    watch::start();
}

/// Game-loop frames (0x433EE0) since the Mod Loader's OnFrame last came.
static LOOP_FRAMES: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

/// Called every game-loop frame (hud.rs, at the end of the frame's 2D work, after the game's
/// update and drawing). While the game is paused the Mod Loader's OnFrame stops (re/addresses.md
/// "Pausing"); then ours runs from here instead, so networking and the shared world go on.
pub fn loop_frame() {
    use std::sync::atomic::Ordering;
    // One loop frame without an OnFrame can be ordering (the loader's call may come later in
    // the frame); two in a row means it isn't coming.
    if LOOP_FRAMES.fetch_add(1, Ordering::Relaxed) >= 1 {
        watch::mark("loop frame");
        OnFrame();
        LOOP_FRAMES.store(2, Ordering::Relaxed);
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn OnFrame() {
    LOOP_FRAMES.store(0, std::sync::atomic::Ordering::Relaxed);
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

        watch::mark("visitors::on_frame");
        visitors::on_frame();
        watch::mark("garden::on_frame");
        garden::on_frame();
        watch::mark("skins::on_frame");
        skins::on_frame();
        watch::mark("let ");
        let (peers, connected) = online::peers();
        game::track_run();
        watch::mark("together::on_frame");
        together::on_frame(&peers, connected);
        watch::mark("coop::on_frame");
        coop::on_frame(&peers, connected);
        watch::mark("bosses::on_frame");
        bosses::on_frame(connected);
        watch::mark("boss_follow::on_frame");
        boss_follow::on_frame(connected);
        watch::mark("shots::on_frame");
        shots::on_frame(connected);
        watch::mark("enemies::on_frame");
        enemies::on_frame(connected);
        watch::mark("lockon::on_frame");
        lockon::on_frame(connected);
        watch::mark("goto::on_frame");
        goto::on_frame();
        watch::mark("stage_race::on_frame");
        stage_race::on_frame(&peers, connected);
        watch::mark("kart::on_frame");
        kart::on_frame(stage_race::others_racing());
        {
            // Room players in our stage, for the treasure hunt battle.
            let members = room::members();
            let (level, mission) = (game::current_level(), game::mission());
            let here: Vec<u8> = peers
                .iter()
                .filter(|p| members.contains(&p.id) && p.level == level && p.mission == mission)
                // Only those who started it with us (a late joiner hunts on their own).
                .filter(|p| p.flags & (game::POSE_WAITING | game::POSE_PLAYING) != 0)
                .map(|p| p.id)
                .collect();
            watch::mark("hunt::on_frame");
            hunt::on_frame(online::my_id(), &here);
        }
        watch::mark("room::on_frame");
        room::on_frame(connected);
        watch::mark("race::start_frame");
        race::start_frame(&peers, connected);
        watch::mark("race::on_frame");
        race::on_frame();
        watch::mark("race::drive_frame");
        race::drive_frame();
        // Pausing doesn't stop play that others are in.
        // Pausing doesn't stop play that others are in (like online games: the menu opens over
        // a world that goes on): a stage started together, a race, live karate, or a Chao World
        // area (garden, lobby, kindergarten) another player is in too. A stage of our own
        // pauses as usual.
        let area = game::current_chao_area();
        let shared_area = connected
            && game::current_level() == game::LEVEL_CHAO_WORLD
            && online::others().iter().any(|(_, _, s)| {
                s.is_some_and(|s| {
                    s.level == game::LEVEL_CHAO_WORLD && i32::from(s.chao_area) == area
                })
            });
        game::set_live_pause(
            shared_area || together::live() || race::live() || karate_live::live(),
        );
        // Online: the main menu's 1P entry reads MULTIPLAYER (online play goes through the 1P
        // modes), 2P opens the battles picker, and the status shows at the bottom.
        game::block_two_player_menu(connected);
        // Online, tabbing out of a windowed or borderless game doesn't stop it.
        game::run_unfocused(connected);
        if connected && let Some((x, y, selected)) = game::main_menu_1p_label() {
            // (x, y) is the label's centre; cover the whole 1P button (about 314 x 50).
            hud::menu_label(x, y, 318.0, 52.0, "MULTIPLAYER", selected);
        }
        // Online, local two-player (split screen) can't be picked: its button reads PARTY BATTLES
        // and opens the battles picker.
        if connected && let Some((x, y, selected)) = game::main_menu_2p_label() {
            hud::menu_label(x, y, 318.0, 52.0, "PARTY BATTLES", selected);
        }
        watch::mark("battles::on_frame");
        battles::on_frame(connected);
        if game::on_main_menu()
            && !battles::is_open()
            && let Some(status) = online::status()
        {
            hud::banner(&format!("Multiplayer on: {status}  (F9)"));
        }
        // Online, controller changes don't stop the game with the "Reboot" dialog.
        watch::mark("game::block_reboot_dialog");
        game::block_reboot_dialog(connected);
        if connected {
            game::close_reboot_dialog();
        }
        watch::mark("online::on_frame");
        online::on_frame();
        watch::mark("puppet::on_frame");
        puppet::on_frame();
        watch::mark("saving::on_frame");
        saving::on_frame();
        watch::mark("menu::on_frame");
        menu::on_frame();
        watch::mark("chat::on_frame");
        chat::on_frame(connected, menu::is_open());
        watch::mark("hud::on_frame");
        hud::on_frame();
        // Shared skins coming down: a bar on the menus (never over play).
        if game::local_entity().is_null() {
            if let Some((done, total)) = skin_sync::progress() {
                hud::progress(
                    &format!(
                        "Downloading skins... {:.1} / {:.1} MB",
                        done as f64 / 1e6,
                        total as f64 / 1e6
                    ),
                    done as f32 / total as f32,
                );
            } else if skins::restart_hint() {
                hud::progress("New skins downloaded: restart the game to use them", 1.0);
            } else if let Some(n) = skin_sync::updated_notice() {
                hud::progress(
                    &format!("Skins updated ({n} file{})", if n == 1 { "" } else { "s" }),
                    1.0,
                );
            }
        }

        if state.menu_key.pressed() {
            menu::toggle();
        }
        // F8: start what you're holding for the others (stage, race or live karate).
        if state.start_key.pressed() {
            together::start_now();
            race::start_now();
            karate_live::start_now();
        }
        watch::mark("OnFrame done");
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
