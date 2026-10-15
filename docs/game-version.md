# Game version

All addresses in this project are for the current Steam build of Sonic Adventure 2 (app 213610).
The mod refuses to go online with a different build (it compares the exe's PE timestamp and
image size; `game::game_build`).

| File | Size | SHA-256 |
|---|---|---|
| `sonic2app.exe` | 21858816 | `4b1e09d782a2d919cbed2324ba2c0c78ef5fbe1677d400edab39608191f12c16` |
| `steam_api.dll` | 120616 | `b0fd893eaa8a225510389e04c0802312539a01fd2eb3a1a8a38005cee7f1d10f` |

Mod loader: X-Hax/sa2-mod-loader, `ModLoaderVer` 16 (helper functions up to `DisplayDebugString`).

## Static analysis

The exe on disk is wrapped with SteamStub (variant 2.1), so its code is encrypted. The
section layout and addresses are the same once decrypted (image base 0x400000). Two ways to
get an analysable **local copy** (keep it in `re/bin/`, which is git-ignored, and never share it):

- **Code dump (no extra tools):** build the mod with `cargo build --release --features dumpexe`,
  install that DLL, start the game once (it writes `sonic2app.text.bin` to the mod folder),
  then `tools/rebuild_exe.py <game>/sonic2app.exe <mod folder>/sonic2app.text.bin
  re/bin/sonic2app.dumped.exe`. Reinstall the normal build and delete the dump afterwards.
  The dump includes the Mod Loader's code patches, which doesn't matter for analysis.
- **Steamless** (`--keepbind`): the unwrapped copy used for the early notes has SHA-256
  `c95c01434f1e8be2ef9472ff2cffb605354aeea1c95e36d5cfa1aa3b7a5c0386`.

The game itself keeps running the original exe.
