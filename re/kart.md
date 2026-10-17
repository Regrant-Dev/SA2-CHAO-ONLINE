# Kart racing: entry, start hold, progress, finish and kart drawing (for room races)

Steam `sonic2app.exe`, image base 0x400000. Everything was read from the objdump listing of
`re/bin/sonic2app.dumped.exe` (no runtime tests yet). The kart code is `..\..\src\Cart\ModeMain\*.c`,
at **0x619000..0x626FFF**. Most of its data (models, kart tables, per-type parameters) comes from
the data DLL: the code looks the names up with `GetProcAddress(*(*(*0x1AF0220)), "name")`
(import 0x8730D4). Examples: `specialInfo`, `exModels`, `exParam`, `cartData`.

Short names used below:
- `car[i] = *(ObjectMaster**)(0x1D96D20 + 4*i)` for i = 0..5.
- `ent = car->+0x34` is the EntityData1. `w = car->+0x38` is the 0x2F8-byte car work.

---

## 1. Entering a kart race

### Levels and courses
| CurrentLevel (0x1934B70) | What | Course global 0x1D970FC (set by the level init 0x61A3B0) |
|---|---|---|
| **70 (0x46)** | story kart: Tails' Route 101 / Rouge's Route 280 | `CurrentCharacter (0x1934B80) == 5 (Rouge)` gives 1, otherwise 0. Also sets 0x1D970F8 = 1 (story) |
| **71 (0x47)** | Kart Race mini-game (1P and 2P) | `0x174B070 < 3` gives `0x174B070 + 2`, otherwise 5 (downloaded course). Sets 0x1D970F8 = 0 |

- Values of 0x1D970FC:
  - 0: `setCartTails`
  - 1: `setCartRouge`
  - **2/3/4: `setCartMini1/2/3`**, which are the Beginner / Standard / Expert courses
  - 5: downloaded course (data at 0x1DEB6C0, via 0x7AB860)
- The level header "CART" is at 0x100ACF0: init 0x61A3B0, free 0x61A910, main 0x61A390. It is
  installed by 0x61AA70. The kart SET object list is at 0x100A230 (13 entries):
  - GATE 0x6245A0 (start/goal gate with the signal lights)
  - RESTART_OBJ 0x624100, which calls 0x61B060 and creates the stage tasks and the cars
  - DASH/JUMP PANEL, CART RING, BALOON and others

**Can a 2P course be raced in 1P? Yes.**
- 2P and 1P Kart Race use the same three courses (`0x174B070` = 0/1/2, all on level 71). The
  only difference is the 2P flags `0x174AFDE`/`0x174AFE0` = 1 and the player count
  `0x1DD946C` = 2.
- The story courses (types 0/1, level 70) can't be selected on level 71, and they are time
  trials without rivals. Use level 71 courses 0..2 for room races.

### Kart menu (CurrentMenu 0x1D7BB10 == **10**)
- Update function **0x672C60**, called from the menu switch 0x666270 (case 10).
- SubMenu is 0x1D7BB14, jump table 0x673570, states 0..0xC.
- Work struct: `k = *(u8**)0xC77F34`.

| off | meaning |
|---|---|
| +0x08 | busy (panel animation). States wait for 0 |
| +0x34 | P1 driver index: 0 Sonic, 1 Tails, 2 Knuckles, 3 Shadow, 4 Eggman, 5 Rouge. It is mapped to a character by table **0xC76630** = {0, 6, 4, 1, 7, 5, -1} |
| +0x39 / +0x3A | P1 "special kart" chosen / available (available = `0x1DEB35C[idx]`) |
| +0x3B / +0x40 / +0x41 | the same for P2 |
| +0x54 | course 0 Beginner, 1 Standard, 2 Expert, 3 download |
| +0x68 / +0x6C | download slot / count |
| +0x88 | 2P mode (u8) |

- **SubMenu 5 (0x673091)** is the course list:
  - Up/down change +0x54 (0..2, or 3).
  - A on course 0..2 runs `call 0x673AE0; [0x1A559E8]=4 (menus return to kart); call 0x672C10;
    SubMenu=0xB; [*0xC77F44]=0; [*0xC77F44+0x14]=0`.
- **SubMenu 0xB (0x673414) starts the race.** Once `k+8 == 0` it does:
  ```
  LastLevel 0x1934B84 = CurrentLevel; CurrentLevel = 71
  if (0x1748B94==1) {0x1748B94=0; if (0x1749564 != -2) {CurrentCharacter=0x1749564; 0x1749564=-2} else CurrentCharacter=0xC76630[k+0x34]}
  else CurrentCharacter (0x1934B80) = 0xC76630[(s8)k+0x34]
  if (k+0x88) {0x1934BE4 = 0xC76630[k+0x3B]; 0x174AFE0=1; 0x174AFDE=1} else {0x174AFE0=0; 0x174AFDE=0}
  0x174AFC0=0; MissionNum 0x174AFE3=0; NextGameMode 0x174B044=4
  0x174B070 = k+0x54                                  ; course
  0x174B021 = (u8)0xC76630[k+0x34] | (k+0x39 ? 0x80 : 0)   ; P1 kart = character (+0x80 special)
  0x174B022 = the same for P2 (k+0x3B, k+0x40)
  if course==3: copy download slot to 0x1DEB6C0, 0x173D078=..., call 0x445330
  0x1DEB320=0; 0x1DEB31E=0; 0x1DEB31F=0; 0x1DEB321=1
  return 1 (menus exit and the stage loads)
  ```
- Other menus that start a kart race: 0x68C760 (CurrentMenu 11, the download-course menu,
  0x68CB2C) writes the same set with course 3.

### Recipe A: start a 1P kart race from the kart menu (recommended)
Preconditions: CurrentMenu == 10, `k = *0xC77F34 != 0`, the panel is idle (`k+8 == 0`), and
the SubMenu is the course list (5). Any idle state probably works.
1. Write `k+0x34` = driver index (0..5, menu order above), `k+0x39 = 0` (normal kart; special
   karts swap that character's models, see 4.3), `k+0x54` = course 0..2, `k+0x88 = 0` (1P).
   For safety write `0x1748B94 = 0` so the character override isn't used.
2. Copy the A-press: `call 0x673AE0` and `call 0x672C10` (both no-arg; optional, they close the
   panels), then `[0x1A559E8]=4`, `[*0xC77F44]=0`, `[*0xC77F44+0x14]=0` and `SubMenu=0xB`.
3. The game does the rest next frame (0x673414).
Confidence: high for the globals and SubMenu flow. Medium for skipping 0x673AE0/0x672C10.

### Recipe B: from Stage Select (same as `room::go`)
Do what `go()` does with `level = 71`, `mission = 0`, `character = the kart driver's
character`, plus these kart bytes before NextGameMode:
- `0x174B070 = course (0..2)`
- `0x174B021 = character (u8, no 0x80)`
- `0x174AFDE = 0x174AFE0 = 0`
- `0x1DEB320/31E/31F = 0` and `0x1DEB321 = 1` (meaning unknown; copied from 0x673532)
- Optionally `MENU_RETURN 0x1A559E8 = 4`. Afterwards the menus then return to the kart menu
  instead of Stage Select.

The level init reads only CurrentLevel, 0x174B070, 0x174AFDE, CurrentCharacter (0x1934B80, via
0x43D880) and 0x174B021. Confidence: medium-high (not run).

### What the level spawns (mini race)
- **0x61AE20(story)** creates the cars. It is called from 0x61B060 with `0x1D970F8`.
  - Story: 1 car. 0x1D96F40 = 1 (story start pending) and **0xEF5AA0 = 0**.
  - Mini race: **6 cars**. 0x1D96F40 = 0 and **0xEF5AA0 = 1** (start hold, see 2).
- Car i is a human when `i < PlayerCount (0x1DD946C)`. Its param block is
  `{isPlayer, type, i, x, y, z, 0, 0, 0}`.
  - Grid position: `x = (i&1 ? -80 : 0) + 30`, `y = 6`, `z = -100 - 40*i`.
  - Kart type:
    - P1 gets `0x100A238[CurrentCharacter]`, P2 gets `0x100A238[0x1934BE4]`.
    - CPU cars get the first type in 2..7 not used yet.
    - Table 0x100A238 = {2, 6, 4, 5, 3, 7, 4, 5} for characters 0..7.
    - **Types: 2 Sonic, 3 Knuckles, 4 Tails, 5 Eggman, 6 Shadow, 7 Rouge.** 0/1 are the story
      karts and 8 is the download/special kart.
- **Car create 0x61C520(param*)** (cdecl, returns the object). It calls LoadObject(list 2,
  "cartExecuter", main 0x61CD60), then sets:
  - Display (+0x14) 0x61C6C0
  - Delete (+0x18) 0x61C660
  - +0x2C 0x61CB80 (player-only shadow/sign)
  - +0x34 a 0x30-byte entity
  - +0x38 the 0x2F8-byte work. The 9 param dwords are copied to `w+0x44`.
  - Then 0x61D280 initialises the work from the type, and 0x47E520 adds collision.
  - The object is stored in `car[i]` (0x1D96D20[i]). Humans are also stored at
    `0x1D9443C + 0x10*k`.

---

## 2. The start (countdown) and how to hold it

### Start flag
- **`*(u32*)0xEF5AA0` (mini race) = "waiting for GO".**
  - While it is set, the car main (0x61CD60, at 0x61CE03) only runs 0x61D5C0/0x61D680/
    0x61F150. These are pose and collision only, so there is no driving or AI for the player
    or the CPUs.
  - The race clock doesn't run (0x622A21), and rank voices/finish are skipped (0x622FF9).
- It is cleared only by **0x623D50**. That is the DeleteSub of the "startExec" task created
  by **0x623CC0** (main 0x623D40 = `jmp DeleteObject`, so the task dies on its first frame and
  clears the flag). **0x623CC0 is the GO.**

### Countdown driver: the kart camera **0x620DF0**
- It is registered per player by 0x620D10 (via 0x4EBC50).
- Called as `fn(0x1DCFF40 + p*0x24D8, entry)`, and the dispatcher at 0x4EE546 then does
  `entry->+8 += 1`.
- Locate the camera entry: `c = *(i32*)(0x1DD0194 + p*0x24D8)`,
  `entry = 0x1DD0198 + p*0x24D8 + c*0x114`. Check `*(u32*)(entry+0xC) == 0x620DF0`.
  - `entry+8` is the frame timer (u32).
  - `entry+0x14` is the camera state.
  - `entry+0x18` is the state to enter after the intro.
  - For player 0 this is normally slot 1, giving entry 0x1DD02AC, timer 0x1DD02B4 and
    state 0x1DD02C0.
- States:
  - **0** (0x620E46): set up. Next state is 2 for mini races and 3 for story. Countdown
    digit `0x1D96D00 = 3`. State becomes 1.
  - **1** (0x620E95): intro fly-by. At timer **== 0x12C** (300) it switches to `entry+0x18`.
    The timer is not reset.
  - **2** (0x620F82): camera behind the kart, countdown.
    - Timer **== 0x168**: `0x1D96D00 = 2`.
    - **== 0x1A4**: `0x1D96D00 = 1`.
    - **== 0x1E0** (480 = 8 s after load): `0x1D96D00 = 0`, state 3, **call 0x623CC0 (GO)**.
  - **3**: race camera. 5/6 are goal and replay cameras.
- The GATE SET object (0x6245A0) shows the lights from `3 - 0x1D96D00` and plays the beeps:
  sound 0x18, and 0x19 for GO.

### Recipe: hold the kart start until the room says go
Every frame while holding (game thread, player 0's entry as above):
```
if entry_fn == 0x620DF0 && *(u32*)0xEF5AA0 != 0:
    st = *(i32*)(entry+0x14); t = *(u32*)(entry+8)
    if st == 2 && t > 0x166: *(u32*)(entry+8) = 0x166   ; kart on the grid, camera behind it, no light yet
    (alternative: if st == 1 && t > 0x12A: write 0x12A   ; parks the end of the fly-by)
```
- The checks use `==`, and the dispatcher increments the timer after the call. Clamping to 2
  below the trigger means the trigger value is never seen.
- Release by no longer clamping, on the same network tick for everyone. Lights follow at
  +1 s, +2 s and GO at +3 s (0x1E0), the same on every client.
- 0xEF5AA0 stays 1 the whole time, so nobody can move and the clock stays 0.
- Alternative (cruder): leave the camera alone and keep writing `*(u32*)0xEF5AA0 = 1` after
  GO. The lights would already say GO.
- Don't patch the `call 0x623CC0` at 0x621003. Camera state 3 would run with the race still
  frozen.

Confidence: high for the flag, GO and camera thresholds. Medium-high for the camera-entry
address maths (read from 0x4EBC50/0x4EE528, not run).

---

## 3. Progress, rank, time, finish

### Car work fields (`w = car->+0x38`)
| off | meaning |
|---|---|
| +0x00..+0x2F | copy of the entity, made each frame at the start of main. +0x08/+0x0C/+0x10 rot X/Y/Z (BAMS ints), +0x14/18/1C position |
| +0x30 | state 0..9 (jump table 0x61D258; 7 = AI drive, 9 = story start, 10 = time over) |
| +0x34 / +0x38 | far / near model (NJS_OBJECT*). Near is used when the camera is within 200 units (0x61EFE0) |
| +0x3C | texlist. +0x40: driver/wheel-anim flag (from the type table) |
| +0x44 | isPlayer (1 = human). **Set to 0 after the player finishes** (0x61D4B0: AI takes over) |
| +0x48 | kart type (see 1). +0x4C: car index. +0x70: player number or -1 |
| +0x68 | extra yaw added in display (spin/drift). +0x7C..+0x84: velocity |
| **+0xBC** | **gate index** (0..N-1) of the last gate passed |
| **+0xC0** | **rank 1..6** (written every frame by the race manager) |
| **+0xC4** | **laps completed** (0 at start; **3 = finished**). +0xC8 = max laps so far |
| +0xD4 | wheel spin angle (BAMS, += speed every frame at 0x61D887) |
| +0xF0..+0x104 | body tilt/bounce angles added in display (x: +0xF0/+0xFC, y: +0xF4/+0x100, z: +0xF8/+0x104) |
| +0x138.. | physics result position, copied to entity+0x14 by 0x61D680 |
| +0x254.. | CPU AI path target (+0x254 next gate, +0x274 yaw) |

### Track gates
- `N = *(i32*)0x1D7C3C8`. Gate `i` is at `0x1D7C3E0 + i*0x20`:
  - +0x00/+0x04: one end (x, z)
  - **+0x08/+0x0C: centre (x, z)**
  - +0x18/+0x1C: other end (x, z)
- A per-gate attribute pointer is at `0x1D8C400[i]`.
- **0x623340** (ECX=gate, EDX=car) tests whether the car's last move (`w+0x14` to `ent+0x14`)
  crossed gate i.

### Race manager "startGoalExec" (created by 0x622600, main **0x6227A0**)
- **Reset at load:**
  - `0x1D94428 = 0` (clock frames)
  - `0x1D94420/21/22 = 0` (min / sec / hundredths, bytes)
  - Lap times `0x1D94430 + 3*lap` (P1) and `0x1D94440 + 3*lap` (P2) = 0xFF
  - `0x1D9442C = 3600` (story time limit)
  - `0x1AEDEE8 = 0` (goal; reset in 0x620D10)
  - `0x1A36C30 = 0` (post-goal counter)
- **Every frame:**
  1. If `0xEF5AA0 == 0 && 0x1AEDEE8 == 0`: `0x1D94428++` (60 per second, capped at 0x10000000)
     and the min/sec/hundredths bytes are rebuilt. The current lap's split goes to
     0x1D94430[3*lap].
  2. Lap counting: for each non-null car[i], test gates bc+1, +2, +3 (humans up to +9; humans
     can also go back 2-3).
     - Passing makes `bc++`.
     - Wrapping past N-1 gives `bc = 0` and `+0xC4++`.
     - Going backwards over the line `+0xC4--` (courses 2..5).
  3. Rank:
     - `score = ((lap + 10)*1000 + bc)*1000 + f`.
     - `f = 100 - 100*|car - centre(bc+1)| / |centre(bc) - centre(bc+1)|`, clamped to >= 0
       (xz plane).
     - The scores are shell-sorted (0x6234C0) and **`w+0xC0` = rank** (1 = leader).
     - The loop stops at the first null `car[i]`.
  4. Finish: for each car with `+0xC4 >= 3`:
     - **1P: `0x1AEDEE8 = 1` for ANY car, CPU included.** The first car to finish ends the
       race and stops the clock.
     - 2P: only humans, and it also creates "cartResultExec" (0x61C220).
- **After goal** (`0x1AEDEE8 != 0`), `0x1A36C30++`:
  - At 60: if `car[0]` rank == 1, the record time is saved (0x445350, course record table
    0x1DEF5BC + course*0xD).
  - At 200: held at 199 while `0x1D96F54` (2P result screen).
  - **At 300 (0x12C): exits.** `0x1D96F50 = 1`, `WORD 0x1A558A4 = 8`, `0x174AFD6 = 1`.
- Cars after goal: `+0xC4/+0xC8` are clamped to 3. Humans switch to AI (0x61D4B0) and keep
  driving.

### Recipes
- **Live position** for the network: send `(lap = w+0xC4, gate = w+0xBC, f, N)` or just the
  game's score above. Also send `ent+0x14` position and `ent+0x08..0x10` rotation. The
  room-wide ranking = sort by score (laps first).
- **Finish:**
  - The local player finished on the first frame where `car[0]` work `+0xC4 >= 3`
    (equivalently `0x1AEDEE8` went 0→1 with no other cars racing).
  - **Finish time = `*(u32*)0x1D94428` frames** (frozen from then on). Display copy is in
    0x1D94420..22.
  - Lap splits are in 0x1D94430.. (3 bytes each).
- **Holding the goal screen** until the room's results are in: clamp `*(u32*)0x1A36C30` to
  <= 0x12A each frame (the exit test is `== 0x12C`, and 60/200 still happen). Release to let
  the game leave.
- **Showing the room rank on the HUD** (optional): after the manager's main runs and before
  display, write the room rank into `car[0]` `w+0xC0`.

### CPU rivals (hide them; don't delete them)
- The cars are `car[1..5]`, with `w+0x44 == 0`.
- **Don't null them or spawn fewer cars.** For example, patching `mov ebp,6` at 0x61AE7A
  (`BD 06 00 00 00`) to 1 would crash: the GATE object calls **0x6232B0**, which reads
  `car[0..5]->+0x38->+0xC4` with **no null checks**. 0x61C333 (2P results) does too.
- Recommended, once after spawn (when `car[5] != 0`), for each CPU car:
  - Set `MainSub (+0x10)` to a no-op. This means no AI and no collision, because collision is
    re-added each frame by 0x47E750 at the end of main.
  - Set `DisplaySub (+0x14)` to a no-op. +0x2C already returns for CPUs.
  - Move `ent+0x14` to e.g. y = -100000.
  - Set `w+0xC4 = -10` so they always rank last and never reach 3 laps. The manager then only
    ends the race when the local player finishes, so each player ends their own race.
- Confidence: high for the crash risk and the 1P "any car finishes" rule. Medium that stubbing
  main fully removes collision.

---

## 4. Drawing other players' karts (puppets)

### 4.1 Kart type table
- **0x1D96F70 + type*0x1C**, 9 entries. It is copied from the DLL export `specialInfo` by
  0x61FFE0 at load.
  - `+4` (0x1D96F74) far model
  - `+8` (0x1D96F78) near model
  - `+0xC` (0x1D96F7C) texlist
  - `+0x18` (0x1D96F88) flag → `w+0x40`
- All six character karts (types 2..7) are loaded in a level-71 race, because the CPUs use the
  five characters the player doesn't. Textures are loaded by 0x6200A0: `cartData` with
  texlist export `texlist_cartData_cartSpecial`, and `kartExModel`.
- Level 70 loads only its own story kart (`tailsBig` / `rougeBig`).
- A **special kart** (`0x174B021 & 0x80`, 0x620160) overwrites that character's type entry
  with `exModels` + `texlist_kartExModel_cartSpecial`. A puppet of the same character would
  then also show the special kart. Prefer normal karts in rooms.

### 4.2 Display 0x61C6C0 (`cdecl void(ObjectMaster*)`)
1. Skips the car when farther than 5000 units from camera `0x1DD92B0[*0x1DD92A0]`.
2. Matrix push. `translate(w+0x14, w+0x18 - 5, w+0x1C)`.
3. Player-only "1P/2P sign" (only if `w+0x44`).
4. `if (w+0x3C) [*0x2670544 + 0x20] = w+0x3C` (sets the texlist).
5. Rotations:
   - `rotZ(w+0x10)`, `rotX(w+0x08)`, `rotY(w+0x68 + w+0x0C - 0x8000)`
   - `rotZ(w+0x104+w+0xF8)`, `rotX(w+0xFC+w+0xF0)`, `rotY(w+0x100+w+0xF4)`
6. **Wheels:** if type 2..7 and `w+0x40`, three nodes `m->child`, `->sibling` and
   `->sibling->sibling` get `ang.x (+0x14) = w+0xD4` and evalflag bit 2 cleared.
7. Draw: `translate(0,-1,0)`, `m = 0x61EFE0(obj) ? w+0x38 : w+0x34`, `call 0x42D340`,
   `0x42EB30(m, 0x42D650)`. Then matrix pop.
8. If course >= 2 and `w+0x40`: exhaust/effect sprites 0x61ECF0. Then 0x61E810 (an empty
   matrix push/pop).

If `w+0x44 == 0` (a CPU car), the display first copies `ent+0x14..0x1C` into `w+0x14..0x1C`.

### 4.3 Puppet recipe (reuse the game's display)
Per remote player:
- A zeroed 0x30-byte entity `e`.
- A zeroed 0x2F8-byte work `w`.
- A fake 0x50-byte ObjectMaster with `+0x34 = e` and `+0x38 = w`.

Fill once:
- `w+0x48 = type` (2..7 from the remote character via 0x100A238)
- `w+0x34/+0x38/+0x3C/+0x40` from the type table above
- `w+0x44 = 0`
- `w+0x70 = -1`

Fill every frame from the network:
- `e+0x14..0x1C` position (the display copies it)
- `w+0x08/+0x0C/+0x10` rotation
- `w+0x68` yaw extra
- `w+0xD4` wheel angle (or advance locally by speed)
- optionally `w+0xF0..+0x104` tilt

Then, from your own display task, call `0x61C6C0(&fake)`.
- The near/far switch, culling, wheel animation and texlist all come for free.
- The shared model's wheel nodes are rewritten on every draw, so many puppets of one type are
  fine.
- If 0x61ECF0's effects misbehave on a zeroed work, set `w+0x40 = 0`. You lose the wheel spin
  and the effects.

Hand-drawn alternative: the transform sequence above, then
`[*0x2670544+0x20] = texlist; call 0x42D340; 0x42EB30(model, 0x42D650)`.

### 4.4 State to send per player (~30 bytes at 10-20 Hz)
- Position `ent+0x14` (3 floats).
- Rotation `ent+0x08/0x0C/0x10` (BAMS; Y is the heading).
- `w+0x68`.
- `w+0xD4` (or speed `|w+0x7C..0x84|`).
- Kart type.
- Progress: lap `+0xC4`, gate `+0xBC`, score.
- Finished flag + time.

Confidence: high for the display transform, model/texlist fields and wheel nodes. Medium that
calling 0x61C6C0 on a fake object is side-effect free (it reads only `w`, `ent`, the camera and
globals, and writes 0xEF789C only for players).

---

## 5. Address list
| Addr | What |
|---|---|
| 0x61A3B0 / 0x61A390 / 0x61A910 | CART level init / main / free (header 0x100ACF0) |
| 0x61AC20 | (re)spawn stage pieces. 0x61AE20(story) creates the cars. 0x61B060 is the stage task setup (called by RESTART_OBJ 0x624100) |
| 0x61C520 | create car. Main 0x61CD60, display 0x61C6C0, delete 0x61C660 |
| 0x1D96D20[6] | car ObjectMaster* list. 0x1D9443C (+0x10 stride) is the human cars |
| 0x1D970F8 / 0x1D970FC | story flag / course index (0..5) |
| 0x174B070 / 0x174B074 | menu course 0..3 / download sub-flag |
| 0x174B021 / 0x174B022 | P1/P2 kart = character (+0x80 special) |
| 0x1DD946C | player count (1/2). 0x174AFDE / 0x174AFE0 = 2P flags |
| 0xEF5AA0 | mini-race start hold (cleared by 0x623D50 after GO 0x623CC0) |
| 0x1D96F40 | story start pending (1 car mode) |
| 0x620DF0 | kart camera + countdown (timer entry+8; 0x168/0x1A4/0x1E0) |
| 0x1D96D00 | countdown digit 3..0 |
| 0x6227A0 | race manager main (clock, laps, rank, finish) |
| 0x1D94428 / 0x1D94420..22 | race clock frames / min-sec-hundredths |
| 0x1D94430 / 0x1D94440 | P1/P2 lap split times (3 x 3 bytes) |
| 0x1AEDEE8 | race over (goal) flag. 0x1A36C30 = post-goal frames (exit at 300) |
| 0x1D7C3C8 / 0x1D7C3E0 | gate count / gates (0x20 stride) |
| 0x623340 | gate-crossed test (ECX gate, EDX car) |
| 0x6234C0 | shell sort used for the ranking |
| 0x6232B0 | max lap over car[0..5] (no null checks) |
| 0x1D96F70 | kart type table (9 x 0x1C) |
| 0x100A238 | character → kart type |
| 0xC76630 | kart menu driver index → character |
| 0x672C60 | kart menu (CurrentMenu 10). Work *0xC77F34. SubMenu 5 = course list, 0xB = start |
