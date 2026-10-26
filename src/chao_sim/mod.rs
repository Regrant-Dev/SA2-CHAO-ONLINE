//! The server's own garden Chao (re/chao-ai.md): the garden they live in and, built on it, the
//! brain that runs them. Plain Rust, no game: the dedicated server runs it, and the mod only
//! uses `land` to read the gardens out of the game and send them up.

pub mod land;

pub mod acts;
pub mod brain;
pub mod evolve;
pub mod life;
pub mod motion;
pub mod physics;
pub mod picker;
pub mod record;
pub mod wild;
pub mod world;

/// The installed game's file `path` (under `SA2CO_GAME_DIR`, the game's folder) for tests
/// that check against the real game: none, and the test skips, when it isn't set.
#[cfg(test)]
pub(crate) fn installed_game_file(path: &str) -> Option<Vec<u8>> {
    let Some(dir) = std::env::var_os("SA2CO_GAME_DIR") else {
        println!("skipped: SA2CO_GAME_DIR not set");
        return None;
    };
    let path = std::path::Path::new(&dir).join(path);
    Some(std::fs::read(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display())))
}

/// The game's data DLL, as `installed_game_file`.
#[cfg(test)]
pub(crate) fn installed_data_dll() -> Option<Vec<u8>> {
    installed_game_file("resource/gd_PC/DLL/Win32/Data_DLL_orig.dll")
}

/// The installed data DLL and the dumped exe (re/bin), or none (skipped) without either.
#[cfg(test)]
pub(crate) fn installed_dll_and_exe() -> Option<(Vec<u8>, Vec<u8>)> {
    let dll = installed_data_dll()?;
    let Ok(exe) = std::fs::read(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/re/bin/sonic2app.dumped.exe"
    )) else {
        println!("skipped: re/bin/sonic2app.dumped.exe not found");
        return None;
    };
    Some((dll, exe))
}
