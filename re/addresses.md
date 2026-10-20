# Reverse-engineering notes: index

Addresses are for the Steam build in `docs/game-version.md`. Everything the mod uses is
checked against the game's code.

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
| Kart racing: entry, start hold, laps/rank/finish, drawing karts | `kart.md` | `src/kart.rs` |
| Stage SET objects, kill/break flags, enemies and item boxes | `stage-objects.md` | `src/coop.rs`, `src/enemies.rs` |
| Stage timer, goal/level clear, checkpoints, restart time, race progress | `stage-race.md` | `src/stage_race.rs` |
| Treasure hunt: emerald piece selection, collection, radar/hints, 2P hunt | `treasure-hunt.md` | `src/hunt.rs` |
| Main menu, 2P entry, leaving for Stage Select / Kart Race | `main-menu.md` | `src/game.rs`, `src/room.rs` |
| Boss gauge/HP, level clear, boss following | `bosses.md` | `src/bosses.rs`, `src/boss_follow.rs` |
| Sonic-family effects, mech jet, kart exhaust/tilt, shields | `effects.md` | `src/puppet.rs`, `src/kart.rs` |
| "Reboot to change the control scheme" dialog | `control-scheme-dialog.md` | `src/game.rs` |
| Menu texture PRS fallback | `menu-textures.md` | `src/menu_textures.rs` |
| Controller pads, button bits, garden button use; status balloon open/close | `chao-details.md` ("Controller buttons in the garden", "The status balloon") | `src/chao_peek.rs`, `src/balloon.rs` |
| Chao save checksum (0x52EEE0/0x549C40), saving outside a garden | `chao-details.md` ("The Chao save checksum and saving outside a garden") | `src/game.rs`, `src/saving.rs` |
| Locked Chao on others' games: the reactions' item checks (0x53DB95/A3/B1), unlinking an item (0x530690) | `chao-garden-sync.md` ("Locked Chao") | `src/garden.rs` |

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
| Chao slots | 0x19F6EC4 | 0x800-byte records, filled when Chao World first loads. In the Chao save file (`SONIC2B__ALF`, folder from the loader's GetChaoSavePath) they are at file offset 0x3AA4 + n*0x800 |
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

## Pausing

- **What runs each frame:** WinMain's loop (0x401DFE) calls the game loop **0x433EE0** every
  frame. That function reads input and pads, then:
  1. checks the system dialog (0x40AD30; while it's open, the game update is skipped);
  2. runs the game update 0x434160, which calls the GameState machine 0x43A780 at 0x434235 /
     0x434944 (jump table 0x43C1B8, 51 states);
  3. the dialog's update (0x433FCB);
  4. the end-of-frame 2D call at **0x433FF7**, which the Mod Loader points at its debug text
     and `hud.rs` hooks.
- **Pausing from a stage:**
  1. State 16 (0x43AAF8) sees Start: the native stage update (0x43CE20) returns 1/2 for a
     Start press, held in EDI.
  2. It calls 0x436020, which sets `0x174AFD9`, the *sound* pause flag (read only by the sound
     code: 0x436210, 0x436BED, 0x436D3A, 0x4377D1, 0x43878A), and 0x439060 (pads).
  3. It moves to state 17.
  4. State 17 (0x43AF52) calls 0x43D310, whose `call 0x43D240` at 0x43D311 draws the stage
     without updating it. That's the mod's `pause_draw` hook. State 17 draws/updates its menu
     and handles Resume; it does not dispatch stage transitions.
- **Stage transition requests:** **0x1A558A4** is a u16 stage transition request, separate
  from the Start return value. Objects set it to 2 for death, 5 for a time-limit restart, 7 for
  a full restart, and other values for other native transitions. State 16 checks it before the
  Start return, handling lives/game-over, fades and level-specific rules.
- **Tabbing out** goes through the window procedure (0x40186C: WM_ACTIVATEAPP off calls 0x436020
  and 0x439060).
- While paused, neither the Mod Loader's OnFrame nor `pause_draw` necessarily runs (seen in a
  stage and in the Neutral Garden), so the loader's OnFrame isn't a reliable per-frame tick.
- **What the mod does:**
  - `hud.rs`'s 0x433FF7 hook calls `lib::loop_frame` every game-loop frame. After two loop frames
    with no OnFrame, it runs the mod's OnFrame itself, so networking and sync go on during a
    pause.
  - Live pause (`game::set_live_pause`) is on in a stage started together, a race, live karate,
    and in a Chao World area another player is in too. There the stage keeps updating behind
    the pause menu with controls disabled. A stage of your own pauses as usual.
  - **Deaths during a live pause:** the first transition request raised behind the menu (or
    already pending when pause opens) is kept for that level/mission, and object updates stop
    behind the menu while one is pending (the native draw and pause menu input still run). On
    Resume, one native stage update is skipped and the request is handed back to the native
    state-16 dispatcher, so lives, fades and respawn run the game's own way. Queued requests
    are cleared on a level/mission change or any state other than 16/17, so Restart/Quit never
    carries a stale death into a new attempt.

## Running while unfocused

- **The skip check:** WinMain's loop (0x401DA4) peeks messages, then before each frame calls
  **0x425DA0** (from 0x401DE0):
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
- **The Mod Loader's part:** with its "pause when inactive" off, it doesn't hold the game; only
  the check above does. With it on, the loader handles inactivity and reactivation itself
  (inside its Present hook, not in the game image).
- **Update and drawing are interleaved:** 0x433EE0 calls the render device's +0x3C (0x433F72, a
  clear, flags 3) before the game update and its +0x2C (0x434009, end of scene) after it, with
  the 2D work at 0x433FF7 between them. Inside the update, a stage's frame (0x43CE20) runs the
  objects (0x46F8C0), then their displays (0x4704B0, with 0x470840, 0x458BB0/0x458C30 and the 2D
  at 0x44C200, ...) in the same function. Chao World and the menus have their own. Running the
  update without drawing would mean skipping all of these individually.
- **Device loss in borderless/windowed:**
  - The present step (0x4247A0) calls the render device's +0x38, which is 0x867AE0, a `jmp`
    into the Mod Loader's Present hook.
  - It sets `0x1A5570B` only when that returns D3DERR_DEVICELOST (0x88760868), which windowed
    and borderless never do.
  - The reset branch's +0x104 (0x868E60) calls TestCooperativeLevel and resets (0x868820)
    only on DEVICENOTRESET (0x88760869); on D3D_OK it returns 1 without resetting.
  - So setting the lost flag from outside does nothing in borderless.
- **Gotchas:**
  - Forcing `WindowActive` back to 1 hides the deactivation from the game and the Mod Loader,
    so their activate/reset handling runs out of step with the real window and the game hangs.
  - Under Proton the window is minimized on every focus loss (client area about 160x31), so no
    frame can be drawn then. Presenting to a minimized borderless window's zero-size swapchain
    can block in DXVK.
  - With the loader's "pause when inactive" off, returning to the game can hang in the first
    frame's drawing; with it on it doesn't.
- **What the mod does:**
  - `prepare_loader_profile` reads the Mod Loader profile's `ScreenMode` and
    `EnablePauseOnInactive` at start and switches the pause back on (from the next launch) if it
    is off.
  - `frame_skip` replaces the call at 0x401DE0. Online, in a windowed/borderless launch (`ScreenMode`
    0/1/2) whose profile has the loader's pause off, a frame the game would skip runs anyway
    when it was skipped only for focus (`WindowActive` 0), the device isn't lost, the game isn't
    paused (GameState 17), and the window can take a frame (not `IsIconic`, a non-empty
    `GetClientRect`). It logs one line on each focus change with every condition.
  - `WindowActive`, the sound pause and the loader keep seeing the real focus. While unfocused,
    the player's controller flag (0x174AFFF) is off and is restored on return.
  - Otherwise the game idles while unfocused as usual; networking goes on in the net thread.
