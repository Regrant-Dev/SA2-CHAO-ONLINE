# Following a friend between stages

Evidence: `re/bin/sonic2app.dumped.exe`, Steam build in `docs/game-version.md`.
Implementation: `src/game.rs` (`leave_for_friend`, `open_stage_select_for_friend`),
`src/goto.rs` and `src/room.rs`.

## Ordinary stage exit

The state-16 dispatcher reads the u16 request at **0x1A558A4**. Request **8** is compared at
0x43AB4F: it writes GameState **14** at 0x43AB54, then calls **0x43C340**. That no-argument
function clears the request, stops the timer, and sets the transition flags.

State 14 (0x43BC03) waits for the native fade (0x4786E0), calls **0x43D510** for cleanup,
then selects state **21**. State 21 (0x43BD83) calls 0x43D5D0 and returns -1. The GameMode-5
handler (0x43429E) then calls **0x439C80** for full unloading and returns to the native menu
initialization path. GameMode 10 also unloads after this return (0x43498A).

For both live and paused ordinary stages, the helper executes the request-8 branch directly:
GameState 14, then 0x43C340. This avoids running a live frame to resume a paused stage. Native
cleanup at 0x43D510 explicitly clears the sound-pause byte 0x174AFD9. The mod discards its
queued pause transition and releases its own world freeze before beginning this exit.

Request **4** is different: it selects state 12, which fades into state 5 to reload the
current stage/Chao area. It must not be used as an ordinary stage exit.

## Chao World must finish saving

Native paused Quit checks for level 90 at 0x43B31A and calls the current-area callback at
`*0x1DE4698`. Except for the race (area 4), it enters **GameState 29** at **0x43B334**.
The Chao manager (main 0x52B250, object pointer 0x1A5AF04) notices state 29 from its live
states at 0x52B40D/0x52B435 and enters its exit/save state 11.

Its state 11 (0x52B49D) starts the save with 0x530230 when needed, waits in state 12
(0x52B4E4), and eventually reaches state 13 (0x52B507). Only then does it raise request 1
at **0x52B542**. Meanwhile native state 29 (0x43BD98) keeps the manager running and waits for
that request before cleanup and state 21. `leave_for_friend` uses this path instead of the
ordinary stage fade, preserving the native save and its completion wait.

The race exception matches native Quit at 0x43B33B: call the registered area-change callback
at `*0x1DE46A0` with **5** (race entrance), enter GameState **12**, call 0x43C340. After the
entrance finishes loading, the pending travel retries the Chao quit path above.

`GoToChaoArea(1)` (0x52B5B0) goes to the **Neutral Garden**. It does not leave Chao World.

## Fresh menus without a previous stage selection

Before exit, **MenuReturn 0x1A559E8 is set to 0**. Menu initialization at 0x673640 then
takes its default branch (0x6737EF), initializes the title header through **0x671B40**, and
selects **CurrentMenu 0**. Return value 6 would instead call **0x673840** to restore the saved
panels before opening Stage Select (0x67369F), which depends on a valid earlier menu snapshot.

The title confirmation at 0x671C96 checks the active save selector **0x173D06C**. With no
save selected (-1), it opens the native save picker, CurrentMenu 9. With a save already
selected, its branch at **0x671E12** calls the no-argument **0x664B60** to initialize the main
menu, then selects **CurrentMenu 1**. The helper takes only this save-selected branch, after
title sub-menu 2 and idle, non-null panels. It never skips native save selection/loading.

The panel pointers exist before main-menu setup: the executable's `.data` table at
0xC77F00 already contains header 0x1D1B7C0, main 0x1D1BBC0 (entry 0xC77F14), and stage panel
0x1D7BAE0 (entry 0xC77F44). Native menu initialization at 0x666600 creates the header and map
objects (calls at 0x666615 and 0x66661F), then reaches 0x673640. Thus checking these pointers
does not require a previous main-menu visit; GameMode 14, title sub-menu 2 and idle panels
provide the initialization/animation readiness checks.

Once the main menu is idle (sub-menu 2, its 1P panel 4, or the mod's held list 0x1C), the
existing `leave_main_menu(PANEL_STAGE_SELECT)` performs the native panel slide-out. Travel
waits for Stage Select's browsing step before `room::go` calls **0x673AE0** to save fresh menu
state and picks the friend's level, mission and character. Neither loading the friend nor
returning afterwards relies on a previously played stage.

All stage exits require GameMode 5/10 and GameState 16/17, a live player, and single-player
mode. Menu navigation requires initialized menus in GameMode 14. Unit tests cover route
selection and refusal during loads, deaths, and menu transitions.
