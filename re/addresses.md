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
| Chao slots | 0x19F6EC4 | 0x800-byte records |
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
