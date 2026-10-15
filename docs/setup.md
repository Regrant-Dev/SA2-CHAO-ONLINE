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
rustup target add i686-pc-windows-gnu x86_64-unknown-linux-gnu
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
