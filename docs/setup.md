# Building from source

Players don't need this: download the release zip and see [how-to-play.md](how-to-play.md).

The mod is a 32-bit Windows DLL written in Rust, loaded by the SA2 Mod Loader. It runs the
same on Windows and under Proton.

## Windows

1. Install Rust from <https://rustup.rs> and the Visual Studio Build Tools with the
   "Desktop development with C++" workload (for the linker).
2. Install SA Mod Manager (<https://github.com/X-Hax/SA-Mod-Manager>) with the SA2 Mod Loader.
3. From the repository folder:

```powershell
powershell -ExecutionPolicy Bypass -File scripts\windows.ps1             # build + install
powershell -ExecutionPolicy Bypass -File scripts\windows.ps1 -Package    # release zip in dist\
powershell -ExecutionPolicy Bypass -File scripts\windows.ps1 -GameDir "D:\Games\Sonic Adventure 2"
```

Installing backs up `resource\gd_PC\SAVEDATA` to `%USERPROFILE%\sa2-savedata-backups` first
whenever it changed. Then enable "SA2 Chao Online" in SA Mod Manager.

## Linux (game under Proton)

Needs `rustup` and the MinGW cross compiler (Arch: `mingw-w64-gcc`; Debian/Ubuntu:
`gcc-mingw-w64-i686`). `.cargo/config.toml` builds for `i686-pc-windows-gnu` by default.

```sh
rustup target add i686-pc-windows-gnu x86_64-unknown-linux-gnu x86_64-unknown-linux-musl
export SA2_DIR="$HOME/.local/share/Steam/steamapps/common/Sonic Adventure 2"  # your game folder
scripts/install.sh     # back up saves, build, install into $SA2_DIR/mods/sa2-chao-online
scripts/package.sh     # release zip in dist/
```

SA Mod Manager runs in the game's Proton prefix:

```sh
protontricks 213610 vcrun2022 dotnetdesktop8
cd "$SA2_DIR" && protontricks-launch --appid 213610 SAModManager.exe
```

Start the game from Steam. The mod writes `mods/sa2-chao-online/sa2-chao-online.log`.

### Automatic updates when launching from Steam (Linux)

Install the current mod once using SA Mod Manager, and enable it. In Steam, open Sonic
Adventure 2 > Properties > General > Launch Options and enter:

```sh
bash "/absolute/path/to/sa2-chao-online/scripts/steam-launch.sh" %command%
```

Replace the script path with your checkout's absolute path. Each launch checks GitHub and
installs a newer mod automatically before starting the game with Steam's original Proton
command and environment. There is no update confirmation or **Save & Play** step. `curl`
must be installed. Keep this script at the configured path; clear the launch option to
restore the normal Steam launch. An older installation without `sa2co-launcher` opens Mod
Manager once so you can accept the update that installs it.

On Linux, the update subprocess uses the host's libraries, avoiding Steam runtime
`libcurl` conflicts. Proton and the game keep Steam's original environment for Steam
Input and the overlay. The script does the same when it bootstraps an older updater.

### Automatic updates when launching from Steam (Windows)

Install the current mod once through SA Mod Manager and enable it. In Steam's Launch Options:

```text
"C:\Program Files (x86)\Steam\steamapps\common\Sonic Adventure 2\mods\sa2-chao-online\sa2co-launcher.exe" %command%
```

Adjust the path to your Steam library. The launcher installs new mod releases automatically,
then starts the game in Steam's session. No Bash or PowerShell command is needed. It uses
the system `curl.exe` included with current Windows 10/11. Continue opening Mod Manager
manually when changing mods or game settings.

The launcher updates only this mod's program files and itself. Settings, skins, exports and
game saves are preserved. An installation error restores the previous mod and prevents
starting a partially updated game. A connection/update error is shown on Windows and logged
to `mods/sa2-chao-online/auto-update.log`; retry the launch after resolving it. Close the game
before updating. The in-game log's `build:` line identifies the DLL actually loaded.

## Tests and tools

```sh
cargo test --target x86_64-unknown-linux-gnu      # unit + localhost network tests
cargo clippy --release                            # the DLL
cargo clippy --release --target x86_64-unknown-linux-gnu --all-targets --features fakepeer
```

- `src/bin/server.rs`: the dedicated server `sa2co-server` (a host that isn't a player).
  `cargo run --release --target x86_64-unknown-linux-gnu --bin sa2co-server -- 43902`, or the
  image: `docker build -t sa2-chao-online-server .` (a static musl binary on `scratch`, under
  1 MB). CI publishes it as `ghcr.io/regrant-dev/sa2-chao-online-server:latest` on every push
  to `main`.
- `src/bin/fakepeer.rs`: a fake second player for testing on one PC, built only with
  `--features fakepeer` (see [testing.md](testing.md)).
- `tools/mdmp.py <dump>`: summarise a game crash dump (`CrashDump/` in the game folder).
- `tools/charassets.py`, `tools/texslots.py <unpacked exe>`: find character asset files and
  texture-list slots (used for `re/characters.md`).
- `cargo build --release --features dumpexe` + `tools/rebuild_exe.py`: an analysable copy of
  the game exe for reverse engineering (see [game-version.md](game-version.md)). Never ship or
  share a `dumpexe` build or its output.

Reverse-engineering notes are in `re/`; addresses are for the Steam build in
[game-version.md](game-version.md).

## Releasing

GitHub Actions does it (`.github/workflows/ci.yml`). Every push and pull request is formatted,
linted, tested and packaged. Every push to `main` then replaces the repository's one release
with the new `sa2-chao-online.zip`: all older releases and tags are deleted, so the Releases
page only ever has the latest build (older code stays in the commit history). There are no
version tags to push.

Bump `version` in `Cargo.toml` (and `Version` in `mod/mod.ini`) when players need to update
together: everyone playing together needs the same mod version.
