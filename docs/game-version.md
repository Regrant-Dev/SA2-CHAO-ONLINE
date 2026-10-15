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

The exe on disk is wrapped with SteamStub (variant 2.1), so its code is encrypted. For reverse
engineering, unwrap a **local copy** with Steamless (`--keepbind`); the section layout and
addresses are unchanged (image base 0x400000; the unwrapped copy we used has SHA-256
`c95c01434f1e8be2ef9472ff2cffb605354aeea1c95e36d5cfa1aa3b7a5c0386`). Keep it in `re/bin/`
(git-ignored) and never share it. The game itself keeps running the original exe.
