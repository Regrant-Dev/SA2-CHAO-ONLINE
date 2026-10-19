# Chao Race: selection, start, progress and finish (for network sync)

Steam `sonic2app.exe`, image base 0x400000. Ghidra has no functions defined in most of the
0x52B000-0x5A0000 Chao code (`procedure_pseudo_code` fails with "no procedure"). Everything
below was read from the objdump listing. Read `chao-stadium.md` first.

**Correction to earlier notes:** 0x558690 (`RaceSpawnRacers`) runs in the **Entrance** at the
line-up screen. Its only caller is 0x533DEF, the entrance confirm. It fills the lane list
0x1A5CC00 and spawns the line-up Chao into 0x1DBE5A0. That explains why those objects are stale
in the race area. The race area spawns its own Chao from the same lane list at **0x52CF10**
(see section 3).

---

## 1. Race selection

### Globals that define the race (bytes)
| Addr | Meaning |
|---|---|
| 0x1A0FBD3 | category: 0 Beginner, 1 Jewel, 2 Challenge, 3 Hero, 4 Dark, 5 Party |
| 0x1A0FBD4 | course: Beginner 0..3, Jewel 4..9 (one per jewel colour), Challenge 10, Hero 11, Dark 12, Party 0..9 |
| 0x1A0FBD5 | level within the course: Beginner 0..2, Jewel 0..4, Challenge 0..11, Hero/Dark 0..3 (in the menu grid this is col*4+row), Party unused |
| 0x1A0FBD6 | Party only: number of human players (copied to 0x19F6438 at race load, which is 1 for the other categories) |
| 0x1A5CC00[8] | lane ChaoData* list. Lane 0 is the player's Chao, set at 0x5535AF by the chao-select menu |
| 0x1A0FBD0/1/2 | race clock: minutes / seconds / frames (set to 0 at race load 0x52D61A) |

`GetCourseDesc()` **0x53A9B0** (no args, returns EAX = 0x14-byte descriptor) shows how the three
bytes are combined:
- cat 0: `0x1354828 + (c*3+l)*0x14`
- cat 1: `0x1354918 + ((c-4)*5+l)*0x14`
- cat 2: `0x1354B70 + l*0x14`, which is 12 entries
- cat 3: `0x1354C60 + l*0x14`, which is 4 entries
- cat 4: `0x1354CB0 + l*0x14`, which is 4 entries
- cat 5: course only. c<4 uses the beginner table, otherwise the jewel table.

Descriptor fields: `+4` is the stage variant (0/1/2, picks the map in the AL_RaceMaster loader
0x52D1C0), `+8` indexes the course function table, `+0xC` is passed to 0x5381E0. The CPU racer
generator 0x596140 uses the same three bytes.

### Entrance menu state machine
- Exec function **0x533100**, created by 0x533FB0.
- Work struct pointer: `*0x1A5CC20`.
  - `work+0` is a busy flag. Animating windows set it to 1 and the master clears it.
  - `work+4` is the menu result: 1 = confirm, 2 = back.
  - `work+0xE8` is the master's ObjectMaster.
- State is the byte `Data1[0]` and the sub-state is `Data1[1]`. The jump table at 0x533EE0 has
  0x17 states.
- 0x0F (0x53348C): builds the course menu for the category and sets the default course:
  cat0→0, cat1→4, cat2→10, cat3→11, cat4→12, party→0. Also sets 0x1A0FBD5=0.
- 0x10 dispatches to 0x11 (party course menu), 0x12 (beginner/jewel course+level grid) or 0x13
  (challenge/hero/dark grid).
- 0x11/0x12/0x13 write 0x1A0FBD4/0x1A0FBD5 live as the cursor moves (0x53388B/0x533895,
  0x533A70/0x533A93, 0x533CED..0x533D16).
- Pressing A (pad `[*0x1DEFB60+0x10] & 4`) on a race sets `work+4=1` and calls
  `0x532FA0(EAX=level, ECX=course)`. That records the highest selected level in save byte
  0x1A0F986[course]. The state then goes to 0x14. Selecting Back sets `work+4=2`.
- 0x14 (0x533DA1): sets `work[0]=1` and `0x1A0F980[cat]=3` (clears the category's "new" mark),
  then goes to state 0x15.
- **0x15 (0x533DC1) is the real confirm.** It runs once `work[0]==0`. With `work+4==1`
  (0x533DE5):
  `*0x1A5CC24=1; call 0x558690 (CPU racers + lane list + line-up Chao); call 0x5584D0;`
  then state 0x16 shows the "press A" prompt (message 0x35). With `work+4==2` it goes back to
  state 5.
- **0x16 (0x533E4B) starts the race.** After more than 30 frames, pressing A runs:
  `call 0x437E90; [*0x1AF0ECC+0xA00648]=0; call 0x4431B0; 0x1A5CA5C=0; call 0x52F090;`
  `if CurrentChaoArea(0x134046C)!=4 { LastChaoArea(0x13402AC)=cur; NextChaoArea(0x134062C)=4 }`
  `WORD 0x1A558A4=4; call 0x4925B0`.
  This matches the generic `GoToChaoArea(int)` 0x52B5B0 with area 4.

### Unlock checks (bypassed when the bytes are written directly)
- Category unlock is in save bytes `0x1A0F980[cat]`. 0 = locked and 2 = newly unlocked. The
  loader at 0x534043 unlocks 1 and 2 once beginner courses 0x1A0F993..996 are all >= 2.
- Progress per course is in `0x1A0F993[course]`, 13 entries (0x1A0F99D/E/F = challenge/hero/
  dark). The menus clamp the level to `progress+1` (0x533A7A) and limit the course count
  (work+0x10/+0x1C/+0x20/+0x24, built by the menu builders 0x5937A0 / 0x594550 / ...).
- Nothing at race load re-checks unlocks. The race data exists for every course and level.

### Recipe: force a race from the entrance
Preconditions: the master exists (`*0x1A5CC20 != 0`), lane 0 is chosen (`*0x1A5CC00 != 0`),
and the master state is 0x11, 0x12 or 0x13 (the course menu is open).
1. Write 0x1A0FBD3 = cat, 0x1A0FBD4 = course and 0x1A0FBD5 = level. For party, also write
   0x1A0FBD6.
2. Set `w=*0x1A5CC20; *(int*)(w+4)=1;` and
   `m=*(ObjectMaster**)(w+0xE8); m->Data1[0]=0x14; m->Data1[1]=0`.
   The state machine then runs 0x14, then 0x15 (lane list, CPU racers and line-up), then 0x16.
   Optionally skip calling 0x532FA0 so no save byte changes.
3. At state 0x16 the game waits for A. To skip the wait, once `Data1[0]==0x16`, call
   `0x52B5B0(4)` (cdecl, 1 arg) or copy the 0x533E7C..0x533ED4 sequence.
   - Alternative: call 0x558690 yourself and then 0x52B5B0(4). This needs fewer state
     assumptions, but leaves the menu windows to be freed by the area change.

Confidence: globals and descriptor maths high. Entrance states 0x14/0x15/0x16 high. Menu
geometry and save-byte semantics medium.

---

## 2. Race start (countdown)

### Race manager (`AL_RaceMaster`)
- ObjectMaster pointer at **`*0x1A5AF18`**, MainSub **0x52BFA0**, created in 0x52D1C0.
- **Main state** is `*(int*)(mgr->Data2 /*+0x40*/)`, a heap int. Jump table 0x52CD84, states
  0..0xB, and 0xC = exiting.
  - 0: init. 1: unused.
  - 2: intro camera finished. Written at **0x55C0CC** by the intro camera, which also creates
    the HUD (0x597230/0x597FF0).
  - **3: running.** Written at **0x53A72B** by the starter. Each frame it calls 0x52BC40 to rank
    the racers, checks the end condition and advances the clock 0x533090 while
    `0x133AE14 != 0`.
  - 4: race over. 200-frame wait, then 5, 6 ... results and exit.
  - Other code tests `state>=6` and `state>=0xA`.
- **Sub-sequencer** in `mgr->Data1`. It runs every frame after the main switch, at 0x52CA77.
  - Action 0: plays the category BGM and sets the beat length `Data1+3` to 0x3E, 0x3D, 0x3B,
    0x3C, 0x39 or 0x41 frames for categories 0..5. Then Action=1.
  - Action 1 (0x52CA8C): increments the u16 `Data1+6` every frame.
    - At counts `6b-6+k*b` (k=0..7) it spawns the per-lane intro object 0x536FF0.
    - At **count == T** it calls **0x53A950** (creates the starter) and sets Action=2.
      T = `16b - b/4` for categories 0-1, `15b - b/4` for categories 2-5 (0x52CC17/0x52CC49),
      and `15b` otherwise.
- **Starter / starting-gun object:** exec **0x53A670**, display 0x53A7D0, state byte `Data1[0]`.
  - 0: timer `Data1+6 = beat` (same per-category table at 0x53A7B4), then state 4.
  - **4: counts down. At 0 (0x53A704) it writes race state=3 and calls 0x52BF50 (GO), then
    state 5.**
  - 5: second timer, then 6.
  - 6: fades out and deletes itself.
- **GO = 0x52BF50**: for each lane object in 0x1DCFAF0 (+0x14 stride) it calls
  `AL_SetBehavior(obj, 0x538EE0 /*race run*/, -1)` and sets `Data1+0xA0 |= 0x40` and `&= ~0x80`.
  Until then the Chao stay in the pre-race idle behaviour 0x538BE0. That behaviour is set by
  the race Chao main 0x53FC10 when `Data1[0]==0`.

### How to hold the start
Recommended:
- While holding, each frame, if `mgr=*0x1A5AF18` and `D=mgr->Data1` and `D[0]==1`, set
  `*(u16*)(D+6) = min(cur, T-2)`. Use T-2 because the game increments before it compares
  (`==T`). This stops the starter from being created, so state stays below 3, the Chao stay
  idle at the line and the clock doesn't run.
- Release by no longer clamping. The game creates the starter and fires GO about one beat
  (~1 s) later. All clients release on the same network tick.

Alternatives:
- Hook 0x53A670 and return early while `Data1[0]==4` on the starter.
- Don't stub 0x53A950 itself. Its call sites still do `inc Action`, so the race would never
  start.

The BGM keeps playing, so the GO can drift off the music beat. That is cosmetic.

Confidence: state values and writers high. Exact T per category medium (two jump tables
inferred from code order).

---

## 3. Racers, progress and finish

### Race-area spawn
- **0x52CF10** runs for each lane with a non-null `0x1A5CC00[i]`: `CreateChao(rec,0,0,&pos,ang)`
  (0x5501D0).
- It then replaces MainSub, DisplaySub and DeleteSub with **MainSub 0x53FC10**, Display
  0x53FCC0 and Delete 0x53FCE0. It also sets `Data1+0xA0 |= 0x8000`, `&=~0x20` and
  `&=~0x20000`, and `+0xA4=2`.
- Racer count is kept at **0x19F6408**, and the lane list at 0x1DBEC60[].

### Lane table
**0x1DCFAE0 + lane*0x14**, 8 lanes. Reset/filled by 0x52CF10.
| off | meaning |
|---|---|
| +0x00 | int rank, 1-based. Set at finish, or by 0x52BC40 for unfinished lanes |
| +0x04 | int finished flag. 1 = finished. Empty lanes are preset to 1 |
| +0x08 | int, set to 8 (unknown) |
| +0x0C | unknown |
| +0x10 | ObjectMaster* of the race Chao (the live object; use this instead of scanning object lists) |

Other globals:
| Addr | Meaning |
|---|---|
| 0x19F640C | finish counter |
| 0x19F6414[rank] | ObjectMaster* by rank (rank 1..8 at 0x19F6418..0x19F6434). Filled when the race ends |
| 0x1DCFAC0 | player (lane 0) finish time: u16 min/sec, then u8 frames |
| 0x1DCFABC | first finisher's time, same format |
| 0x133AE14 | "accepting finishes / clock running" flag. Set to 1 at load (0x52D6BD) |

### Progress
- `ChaoData1+0xA8` is a float, called `waypointID` in SA2Structs: waypoint index plus the
  fraction to the next waypoint. It is written by the path-follow code, for example at
  0x53EB45 inside 0x53E390.
- 0x52BC40 ranks the unfinished lanes after all finished ones by comparing +0xA8. Larger is
  further ahead.
- `+0x7B0` is `waypointLerpFactor` (randomised lane offset, 0x53FB90).

### Finish
- **`RaceRegisterFinish` 0x52BB40**, usercall **EDX = Chao ObjectMaster***, no stack args.
  - It does nothing unless `race state < 4` and `0x133AE14 != 0`.
  - Otherwise it finds the lane whose +0x10 == EDX, sets the finished flag (+4) to 1 and sets
    `rank = ++0x19F640C`.
  - For lane 0 it stores the clock in 0x1DCFAC0. Outside party races it also **clears
    0x133AE14**, which stops the clock and every later finish.
  - It does **not** guard against being called twice. A second call would assign a second rank.
- **Goal detection:** inside the race trigger handler **0x53F1A0**. That handler is called
  from 0x53FAF0, which runs only if `Data1+0xA0 & 0x10`. It fires when the current behaviour
  is 0x538EE0/0x538D00 (or 0x538E30) and the area query `0x486880(obj,1,0xB8)` hits (0xB8 =
  goal area). On a hit (0x53F66A and 0x53F6D7):
  `AL_SetBehavior(obj, 0x538420 /*goal*/, -1)` (0x53D890: EAX=obj, push -1, push fn, cdecl
  pop 8), then `call 0x52BB40` with EDX=obj, then `+0xA0 &= ~0x40; |= 0x80`.
- **Race end:** checked in race state 3 (0x52BFFB).
  - Categories 0-4: the race ends as soon as **lane 0 (the local player) has finished**.
  - Party: the race ends when `finishCount >= racerCount`.
  - At the end the game fills 0x19F6414[rank], calls 0x5396C0 (results/rewards), and sets
    state=4 with a 200-frame timer.

### Race Chao per frame (MainSub 0x53FC10)
- If `Data1[0]==0`, it sets the pre-race behaviour 0x538BE0.
- If `*0x1DBE634 != 0`:
  - 0x53FAF0. Only when `+0xA0 & 0x10`: the triggers and goal (0x53F1A0), then 0x53EF70, then
    the behaviour stack dispatch (`+0x434` count, `+0x438` index, `+0x43C` fn[], `+0x47C`
    timers). When the stack empties it falls back to 0x538EE0.
  - Then 0x53CB70, 0x53A3E0, 0x56CCF0 (motion), 0x796780, 0x53DF00, and collision
    0x47E750/0x47E6C0.
- 0x1DBE634 is a global "update race Chao" gate.

### Driving a lane from the network
(a) Stop the AI:
- On the remote lane's object (`*(ObjectMaster**)(0x1DCFAF0 + i*0x14)`), replace `MainSub`
  (+0x10) with a wrapper. The wrapper calls the original 0x53FC10 and then writes the network
  position (Data1+0x14), rotation and progress (`Data1+0xA8`). Writing after the original
  means physics can't move the Chao and the display uses the network position.
- For a fully passive Chao, also clear `Data1+0xA0` bit **0x10** after spawn (after 0x52CF10).
  0x53FAF0 then skips the behaviour, the trigger/goal checks and the progress update. The Chao
  stays in whatever animation it had.
- Keeping bit 0x10 set keeps the running animation, but the AI's own triggers (pit, water,
  goal) can then switch behaviours and fire the goal by itself.

(b) Finish:
- When the network reports that player finished, run once (in host-authoritative order on
  every client):
  `AL_SetBehavior(obj,0x538420,-1)`, then `0x52BB40(EDX=obj)`, then `Data1+0xA0 &= ~0x40;
  |= 0x80`.
- Keep `+0xA8` updated so unfinished remote lanes rank correctly when the race ends.
- With bit 0x10 cleared the native goal never fires, so there's no risk of a double call.

Caveats:
- In non-party categories the local player's finish ends the race and blocks later finishes.
  To let everyone finish:
  - set 0x133AE14 back to 1 after the local finish,
  - patch the category test `cmp eax,4 / ja 0x52C0E6` at 0x52C007 so every category uses the
    party end rule (`finishCount >= racerCount`, 0x52C0E6).
  CPU lanes that never finish would then hang the race.
- Medals and rewards come from 0x5396C0 using lane 0's rank.

Confidence: lane table, finish function, goal trigger and end rule high. Flag 0x10 skipping
all AI medium-high (read from 0x53FAF5, not run). Effects of 0x53DF00 and the other movement
callees not verified, which is why the post-main position overwrite is recommended.

## 4. Gaps (audit 2026-10-18)

What a friend's lane shows in our party race, against their real Chao in theirs:
- Position: theirs (shifted by the lane gap), set after the lane's own update (race.rs).
- Animation: was the lane's own behaviour's (running), so a friend's Chao that stopped to rest,
  tripped, swam or cheered in their race ran on the spot in ours. Fixed: the racer's state now
  carries its motion id in `PlayerState::anim` (current, or the one it blends into), and the
  lane starts it (SetMotionLink 0x793C40, checked against the shared table) when theirs changes.
- Facing and progress (`Data1+0xA8`) stay the lane's: the lane follows the course, so it faces
  the right way, and the ranking (0x52BC40) keeps working.
- Finish: the lane's own goal check fires when it reaches the goal where their Chao is, so the
  finish order follows the real one to within the update delay.
- Not synced: the lane's emotion ball and face (the race shows them small), sounds. Results and
  rewards are each player's own (0x5396C0 in each game).

