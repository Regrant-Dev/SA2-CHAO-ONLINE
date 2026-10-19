# Reverse-engineering notes: index

Addresses are for the Steam build in `docs/game-version.md`. Community headers
(X-Hax/sa2-mod-loader `SA2ModLoader/include`) were the starting point; everything the mod
uses was checked against the game's code, and most of it in game.

| Topic | Notes | Code |
|---|---|---|
| Player state, levels, missions, game state, freeze/pause hooks | below | `src/game.rs` |
| Character assets, texture lists, drawing other characters | `characters.md` | `src/puppet.rs` |
| Spin ball (JumpAura) effect | `spin-ball.md` | `src/puppet.rs` |
| Chao record layout, garden slots, CreateChao | `chao.md` | `src/chao.rs`, `src/visitors.rs` |
| Chao Stadium: karate unlock, race opponents | `chao-stadium.md` | `src/race.rs`, `src/karate.rs` |
| Race selection, start hold, racer progress | `chao-race-sync.md` | `src/race.rs`, `src/room.rs` |
| Karate fight flow, start hold, fighters, live sync | `chao-karate-sync.md` | `src/karate_live.rs` |
| Garden Chao update, motion, being held, mirroring a visitor | `chao-garden-sync.md` | `src/visitors.rs` |
| Kart racing: entry, start hold, laps/rank/finish, drawing karts | `kart.md` | (planned room kart races) |
| Stage SET objects, kill/break flags, enemies and item boxes | `stage-objects.md` | (planned co-op kill sync) |
| Stage timer, goal/level clear, checkpoints, restart time, race progress | `stage-race.md` | (planned stage race) |
| Treasure hunt: emerald piece selection, collection, radar/hints, 2P hunt | `treasure-hunt.md` | (planned hunt battle) |
| Stage Select confirm (Stage Rooms) | `characters.md` (end) and `src/room.rs` | `src/room.rs` |

## Core addresses

| Name | Address | Notes |
|---|---|---|
| CurrentLevel | 0x1934B70 | i16 |
| MissionNum | 0x174AFE3 | u8 |
| GameState | 0x1934BE0 | i16; 16 = in game, 17 = paused |
| GameMode / NextGameMode | 0x174B040 / 0x174B044 | 0xE = menus, 4 = load stage |
| CurrentMenu / CurrentSubMenu | 0x1D7BB10 / 0x1D7BB14 | 3 = Stage Select; sub 5 = leaving after a pick |
| CurrentChaoArea | 0x134046C | 1-3 gardens, 4 race, 5 race entrance, 9 stadium, 10 karate |
| MainCharObj1 / MainCharObj2 | 0x1DEA6C0 / 0x1DE9600 | player 1's EntityData1 / CharObj2 |
| ControllerEnabled / TimerStopped | 0x174AFFF / 0x174AFDA | u8 |
| HaveBattleDLC | 0x1A556F4 | set from Steam at startup; only read |
| Chao slots | 0x19F6EC4 | 0x800-byte records, filled when Chao World first loads. In the Chao save file (`SONIC2B__ALF`, folder from the loader's GetChaoSavePath) they are at file offset 0x3AA4 + n*0x800 (matched byte for byte, 2026-10-16) |
| CreateChao | 0x5501D0 | cdecl (record, 0, 0, &pos, angle) |
| LoadObject | 0x46F680 | ESI = list, EDI = main, name on the stack |

## Hooks the mod installs

| Call site | Original | Purpose |
|---|---|---|
| 0x433FF7 | Mod Loader debug text | draw the F9 menu's dark panel first (`hud.rs`) |
| 0x43AB24 | 0x43CE20 frame update | freeze a stage while waiting at the start (`game.rs`) |
| 0x43D311 | 0x43D240 draw-only | keep a synced stage running behind the pause menu (`game.rs`) |
| 0x542308 | 0x578EB0 fighter spawn | karate opponent from a friend's Chao (`karate.rs`) |
| 0x5586A0 | 0x596140 CPU racers | friends' Chao in race lanes (`race.rs`) |

## Pausing (2026-10-18, REA/objdump)
- **What runs each frame:** WinMain's loop (0x401DFE) calls the game loop **0x433EE0** every
  frame. That function reads input and pads, then:
  1. checks the system dialog (0x40AD30; while it's open, the game update is skipped);
  2. runs the game update 0x434160, which calls the GameState machine 0x43A780 at 0x434235 /
     0x434944 (jump table 0x43C1B8, 51 states);
  3. the dialog's update (0x433FCB);
  4. the end-of-frame 2D call at **0x433FF7**, which the Mod Loader points at its debug text
     and our hud.rs hooks.
- **Pausing from a stage:**
  1. State 16 (0x43AAF8) sees Start.
  2. It calls 0x436020, which sets `0x174AFD9`, the *sound* pause flag (read only by the sound
     code: 0x436210, 0x436BED, 0x436D3A, 0x4377D1, 0x43878A), and 0x439060 (pads).
  3. It moves to state 17.
  4. State 17 (0x43AF52) calls 0x43D310, whose `call 0x43D240` at 0x43D311 draws the stage
     without updating it. That's our `pause_draw` hook.
- **Tabbing out** goes through the window procedure (0x40186C: WM_ACTIVATEAPP off calls 0x436020
  and 0x439060).
- **Seen in game (2026-10-18):** while paused, neither the Mod Loader's OnFrame nor `pause_draw`
  marked the watchdog, in a stage (level 11) or in the Neutral Garden. So the loader's OnFrame
  comes from a place that doesn't run while paused, and the state-17 path wasn't the one taken
  then. Not resolved statically.
- **What the mod does now:**
  - `hud.rs`'s 0x433FF7 hook calls `lib::loop_frame` every game-loop frame. After two loop frames
    with no OnFrame, it runs our OnFrame itself (watchdog mark "loop frame"), so networking and
    sync go on during a pause, and the next test's log shows whether the game loop runs while
    paused.
  - Live pause (`game::set_live_pause`) is on in a stage started together, a race, live karate,
    and in a Chao World area another player is in too. A stage of our own pauses as usual.
    Either way our per-frame work keeps running while paused, unless the game itself stops
    (tabbing out).

## Running while unfocused (2026-10-18)
- **The skip check:** WinMain's loop (0x401DA4) peeks messages, then before each frame calls
  **0x425DA0**:
  - `if WindowActive == 0 → return 1`, the cmp at 0x425DA0 and `jne 0x425DAC` at 0x425DA7
    (bytes `75 03`);
  - otherwise it handles a lost D3D device (0x1A5570B, a vtable call on the render device at
    +0x104) and returns that flag.

  A 1 skips the frame: 0x401E53 runs only 0x438750 and loops (a busy idle).
- **Who sets `WindowActive` (0x174F5AC):** only the game's window procedure (0x401810, message
  table at 0x4018D0/0x4018B8). WM_ACTIVATEAPP (0x1C) → case 0x40186C:
  - **Focus on:** 1, then restore sounds 0x435FD0.
  - **Focus off:** 0, then pause sounds 0x436020 (sets the sound flag 0x174AFD9) and 0x439060
    (pads).
- **The Mod Loader's part:** with its "pause when inactive" off, it doesn't hold the game. What
  was left is this check.
- **What the mod does (`game::run_unfocused` + `frame_skip`, the call at 0x401DE0 redirected):**
  online, and only when the launch isn't fullscreen (Mod Loader `ScreenMode` 0/2/3, read at
  start), a frame the game would skip runs anyway when all of these hold:
  - it was skipped only for focus (`WindowActive` 0);
  - the device isn't lost (0x1A5570B 0);
  - the game isn't paused (GameState 17);
  - the game thread's window can take a frame (not `IsIconic`, a non-empty `GetClientRect`).

  `WindowActive`, the sound pause and the loader keep seeing the real focus. While unfocused,
  our player's controller flag (0x174AFFF) is off and is restored on return.
- **Live test 2026-10-18 (the first, byte-patch version):** a tab-out in the garden was fine, but
  a tab-out with the pause menu open hung for good after "puppet display" (the frame's draw). A
  borderless window that gets minimized on focus loss has a zero-size swapchain, and presenting
  to it can block in DXVK. Paused frames while unfocused were the case that hung. Both are now
  left to the game's own skip.
- **Why it avoids the hang on refocus:** the old approach forced `WindowActive` back to 1. That
  hid the deactivation from the game and the Mod Loader, so their own activate/reset handling
  ran out of step with the real window state. The patch doesn't touch focus state at all; it
  only lets the frame run. A windowed/borderless D3D9 device (DXVK) isn't lost on focus
  change, and if it were, the game's own reset in 0x425DAC still runs before each frame.
- **Next test's log:** `frame_skip` logs one line on each focus change with every condition:
  online, not fullscreen, device lost, GameState, minimized, client size, the game's own skip.
- **Running the update without the draw (assessed 2026-10-18, not built):** in a frame the update
  and the drawing are interleaved, not two calls.
  - 0x433EE0 calls the render device's +0x3C (0x433F72, a clear, with flags 3) before the game
    update, and its +0x2C (0x434009, the end of the scene) after it. The 2D work at 0x433FF7
    sits between the two.
  - Inside the update, a stage's frame (0x43CE20) runs the objects (0x46F8C0), then their
    displays (0x4704B0, with 0x470840, 0x458BB0/0x458C30 and the 2D at 0x44C200, ...) in the
    same function. Chao World and the menus have their own.
  - Skipping only the drawing would mean skipping the clear and end-of-scene wrapper calls (or
    leaving a scene open) plus every display call inside each state's update. Each needs its own
    RE and test against DXVK, so it isn't done. While the window is minimized, the game idles
    as it always has (networking goes on in the net thread).
- **Coming back from a tab-out (2026-10-18 logs and RE):**
  - **The window is minimized every time.** Every focus loss under Proton logged `minimized
    true, client 160x31`, so no frame can be drawn then and `frame_skip` lets the game idle.
  - **Three returns hung** in the first frame's drawing (last mark "puppet display"), each with
    the loader's pause-when-inactive off. With it on (the fullscreen days) returning never hung.
  - **The game's device-lost path is no help in borderless:**
    - The present step (0x4247A0) calls the render device's +0x38, which is 0x867AE0, a `jmp`
      into the Mod Loader's Present hook.
    - It sets `0x1A5570B` only when that returns D3DERR_DEVICELOST (0x88760868), which windowed
      and borderless never do.
    - The reset branch's +0x104 (0x868E60) calls TestCooperativeLevel and resets (0x868820)
      only on DEVICENOTRESET (0x88760869); on D3D_OK it returns 1 without resetting.
    - So setting the lost flag ourselves would do nothing in borderless.
  - **What reactivation depends on:** the loader's own reactivation handling (inside its Present
    hook, not in the game image), which "pause when inactive" switches.
- **Now:**
  - `prepare_loader_profile` keeps the loader's pause on, and switches it back on if an earlier
    build turned it off (from the next launch).
  - `run_unfocused` only acts when this launch's profile has that pause off, so with it on
    nothing of ours runs frames without focus.
  - Networking still goes on in the net thread, and the keeper hands off while you're away.
- **Not tried:** keeping the window from being minimized. Under X11 that's the window
  manager's and Wine's decision, not the game's.
