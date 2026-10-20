# Stage race: timer, goal, checkpoints and progress

Steam `sonic2app.exe`, image base 0x400000. Sources: the disassembly of
`re/bin/sonic2app.dumped.exe` and the stage SET files in the game folder
(`resource/gd_PC/setNNNN_s.bin` / `_u.bin`). `stage-objects.md` covers the SET record table used
here.

Online stage races have up to 8 players run the same level and mission, each in their own game.
The mod shows each player's live position, detects the frame a player clears the stage, and shows
everyone the finish order and times.

---

## 1. Stage timer

### Globals
| Addr | Type | Meaning |
|---|---|---|
| **0x174AFDB** | u8 | timer **minutes** (0..99) |
| **0x174AFDC** | u8 | timer **seconds** (0..59) |
| **0x174AFDD** | u8 | timer **frames** (0..59, 1/60 s) |
| **0x174AFDA** | u8 | TimerStopped: the tick does nothing while it is non-zero |
| 0x174AFF9 | u8 | second "don't tick" flag (set by 0x457272 and in state 7 when 0x458970 says so; story/event use) |
| 0x174AFF7 | u8 | when non-zero, the tick returns immediately, frame counter included (also gates the goal ring's animation). Set by 0x4D2A2E/0x4D2E37 |
| 0x174B03C | u32 | stage frame counter: +1 per logic step unless 0x174AFF7. Reset to 0 on (re)start (0x43D82D, 0x44CAE8, 0x44C988) |
| 0x1934B74 | u32 | +1 after every in-stage frame update (0x43AB29, game state 16 only). Never reset |
| 0x1A5A230 | ptr | a mirror record: +0 min, +1 sec, +2 frames, +4..+6 level-start time, +8/+0xA lives, +0xC/+0xE rings, +0x10, +0x18 score |
| 0x174B00E/0F/10 | u8 x3 | **level-start time** (min/sec/frames). Normally 0. A stage chained after another one (story) starts from here |
| 0x174B028 / 0x174B02A | i16 | rings of player 1 / player 2 |
| 0x174B050 | u32 | score (0x174B054 = score saved at level start) |
| 0x174B024 / 0x174B026 | i16 | lives P1 / P2 (set to 4 by the new-game reset 0x44C9C6) |

### Tick: 0x44DC80 (no arguments)
Called once per logic step from the frame update **0x43CE20** at 0x43CF90. The order inside one
step is: 0x441BA0, object lists 0..5 (0x46F8C0, which runs every object including the goal
ring), 0x486190, 0x48BE80, **then the timer tick**.
```
if (0x174AFF7) return;
0x174B03C += 1;
if (TimerStopped || 0x174AFF9) return;
if (++frames >= 60) { frames = 0; if (++sec >= 60) { sec = 0; if (++min >= 100) {99:59:59} } }
copy min/sec/frames to *0x1A5A230 +0/+1/+2
```
- The timer **counts up in every mission**. The Mission 4 time limit is a separate task,
  0x451DF0: it compares min:sec against the limit in its work data. When the limit is reached it
  requests 5 (restart), or 2 when there are no lives left.
- The game can run 2 logic steps in one frame update (`0x1DEB50C` steps; the extra step happens
  every 5th frame when 0x174AFCF == 2). The tick runs once per step, so the time is game time,
  not wall-clock time.
- HUD: the centiseconds are `frames * 100 * 0.01666` (constants 0x904E30 and 0x9060C8, at 0x44DE72).
  To show the same digits as the game: `cs = (frames * 100.0 * 0.01666) as u32`.
- `SetTime(AL = min, CL = sec, DL = frames)`: **0x44DD50** (a negative value keeps that field).
- Save the level-start time: 0x44CA10 (current time → 0x174B00E..10). Restore it: 0x44CAC0 (also
  sets 0x174B03C = 0). Clear it: 0x44C9E0.

**Exact clear time** = `min*3600 + sec*60 + frames` in 1/60 s, read once 0x174B002 is set
(section 2). The goal routine sets TimerStopped inside the object pass, which runs *before* that
step's tick. So the goal frame's own tick never happens, and the value read after the frame
update is final. Nothing changes it afterwards: the results task reads it but does not write it.

---

## 2. Goal / level clear

### LevelClear: **0x43E6D0** (usercall, **ESI = player index** 0/1, no stack args, plain ret)
```
if (0x174B002 || 0x174B003) return;         // already cleared / results already saved
ControllerEnabled[0] (0x174AFFF) = 0; in 2P also ControllerEnabled[1] (0x174B000) = 0
0x174AFD6 = 1; TimerStopped (0x174AFDA) = 1
2P: wins[ESI] (0x174AFF5+ESI) += 1, 0x174B00C = winner's character
counter 0x174B034[ESI] += 1 (capped at 999)
**0x174B002 = 1**                            // "level cleared"
0x452A00()                                   // clear jingle
levels 35..38 (Cannon's Core) + Mission 2 + 100 rings: special path; else for 35..38 request 1
otherwise: 0x451450(ESI)  → creates the results task in the same frame
           0x4EBB40 / 0x4EBCD0               // player victory action
```
- There are 25 call sites, one per kind of clear: the goal ring (0x6C69E5), 100 rings in Mission 2
  (frame update 0x43CF4F, rings 0x174B028 >= 100), Lost Chao, bosses, hunting, kart, and so on.
  **Every successful clear goes through it.**
- 0x451450 picks the results task. 1P: **"CalcTotalScore"** task (main **0x44F7C0**) for Missions
  1/4/5, the 0x45165B variant for Missions 2/3, and "CalcTotalTime" (0x450780) for level 70. 2P:
  "DispWinnerAndContinue" (0x451050).
- The results task, in its first state, calls **0x43ECA0** (ESI = player). That function writes the
  save-file record for the clear (per-character tables 0x1748488...) and sets **0x174B003 = 1**.
- Flags reset: 0x43A130 (stage load) and **0x43C8B0** (every restart) set 0x174B002 = 0x174B003 = 0.

### Goal ring: GOALRING, main **0x6C63C0** (exec 0x6C6600, display 0x6C6CD0, delete 0x6C65A0)
- Every object list has it, with "always load" range flags (0x202). Its list index varies:
  14 in most stages, 31 in Green Forest, 28 in White Jungle, 13 in Aquatic Mine, Iron Gate and the
  STG07/58/11/49 lists. **Match on the main pointer**, as in stage-objects.md section 5.
- `rotX % 3` (SET +2, copied to Data1+8) is the ring type. Every stage has exactly one ring of
  each type (table in section 6):
  - **0**: the real goal ring.
  - **1**: the Lost Chao spot. In Mission 3 it is the goal. In other missions the ring spawns an
    ITEMBOX child there instead (0x6C6449).
- On touch (0x6C698F): `0x486520(obj)` collision, then 0x46DCC0 → player index.
  - **Mission 2** (MissionNum 1), or **Mission 3** with a type-0 ring: no clear. 0x43F330 stops
    the timer and the controller and requests 7 (state 15 → 0x43C3A0, restart), probably
    "mission failed".
  - Otherwise: **`call 0x43E6D0`** with ESI = player, then `0x1AEFD74[player] = 2`.

### Game state after the goal
The state request is the u16 at 0x1A558A4, read after 0x43CE20 in the state-16 handler
(0x43AB31):

| Request | Next GameState | Meaning |
|---|---|---|
| 1 | pause | |
| 2 | 9, or 14 when out of lives | death |
| 3 | 11 | |
| 4 | 12 | |
| 5 | 13 → 7 | restart: time limit with lives left, or the 2P winner screen |
| 7 | 15 → 7 | restart from scratch (0x43C3A0) |
| 8 | 14 | |

GameState stays **16** after the goal while the victory pose and the results tally run. The
stage ends from the results task later. So **GameState is not a usable goal signal; 0x174B002 is.**

### Detecting the clear
The mod polls 0x174B002 in its frame hook (`game.rs`, the call at 0x43AB24 → 0x43CE20), after the
original returns. It goes to 1 on the clear frame for every clear type, and back to 0 on a restart
or level load. The game time is then `min*3600 + sec*60 + frames` (0x174AFDB/DC/DD, 1/60 s).
0x43E6D0 is usercall (ESI), so a detour on it needs a naked stub that saves registers. In 2P, ESI
(or the wins counter 0x174AFF5[p]) tells which player cleared.

---

## 3. Checkpoints (SAVEPOINT) and the restart point

### SAVEPOINT, main **0x6D99A0** (list index 8 in most lists, 4 in some; match on main)
State byte Data1[0]. Jump table at 0x6D9B30: 0 init 0x6D99EF, 1 idle 0x6D9ACB, 2 0x6D9AD9,
3 0x6D9AFB (already passed).
- **Init:** in **Mission 4 (MissionNum 3) every checkpoint kills itself** (`jmp 0x488DA0`), so
  that mission has no checkpoints. Otherwise it sets **record flag 0x10** (kept through death
  restarts). If the record already has **0x40** it starts in state 3 (passed).
- **Touch** (0x6DA27F, state 1): the collision gives the player p (0x46DCC0), and only player
  entities count. Then:
  - state = 2;
  - **`0x43E520(p, cpIndex)`**: saves the restart point (below);
  - **`rec->flags |= 0x40`** (0x6DA312), the "passed" mark;
  - sound 0x1001;
  - stores min/sec into its work (+0x34/+0x38 + p*2);
  - `0x1AEFD74[p] = 3`;
  - ring bonus voice when rings >= 20.
- `cpIndex` = Data1[3]. It is 0 in 1P, and rot.y's low byte only in 2P. **In 1P the game does
  not number checkpoints**: any later checkpoint overwrites the restart point.

### Restart point: `RestartPoint[2]` at **0x1934B88**, stride 0x2C (one per player)
| Off | Type | Meaning |
|---|---|---|
| +0 | i16 | level |
| +2 | i16 | checkpoint index, **-1 = none** (set by 0x43E380, the reset at each stage (re)start) |
| +4/+5/+6 | u8 | **time when touched** (min/sec/frames) |
| +8 | float[3] | position |
| +0x14 | int[3] | rotation |
| +0x20/+0x24/+0x28 | | camera/path state (0x1DE949C, 0x1DE94AC, 0x1945E08) |

- Save: **0x43E520** (cdecl `(player, index)`, EBX = &pos, EDI = &rot). It skips when the stored
  index is greater than the new one.
- Restore: 0x43E3F0 (EBX = player), called by the player placement **0x43DA40** (EAX = player) at
  every restart.
- The touch (0x6DA2F7) passes the **player's** position and rotation (player Data1 +0x14 / +0x8
  via 0x1DEA6C0[p]), not the checkpoint's. The object's record pointer is ObjectMaster +0x30.
- Co-op checkpoint sharing: a record gaining 0x40 is sent with RestartPoint[0] +8/+0x14; the
  others set 0x40, move a live idle one (state 1) to state 3, and call 0x43E520(0, 0) with the
  sender's position/rotation when it's further along the stage route.

---

## 4. Death and restart: what happens to the timer

A death sets request 2. GameState goes **9** (fade, 0x43BA75) → 5 → 7 → 16. The restart runs
0x43C370/0x43D7A0 → **0x43DA40** for each player, then **0x43CB50**: 0x43C8B0 clears the clear
flags, and 0x488AD0 respawns the SET objects except checkpointed ones.

In **0x43DA40**:
- **If a checkpoint was touched** (RestartPoint.index != -1), 0x43E3F0 puts the player at its
  position and, in 1P, calls **`SetTime(RestartPoint time)`** (0x43E455). **The timer rewinds to
  the time the checkpoint was touched.** Rings are set to 0 (`0x44CE10(-rings)`).
- **Without a checkpoint**, the player goes to the start position and 0x44CAC0 **restores the
  level-start time (0:00:00)**. Score and rings return to the level-start values (0x174B054,
  0x44CA50).
- While the player is dead and the screen fades, the timer keeps ticking until the restart rewinds
  it.

**So the in-game clear time does not include time lost to deaths.** It is the time of the
successful attempt, measured from the last checkpoint's saved time. In a stage started together,
the mod ranks finishers by its own race clock (time since the GO, deaths included) and shows the
game's time next to it.

Restart from the pause menu (request 7 path / 0x43C3A0 with 0x1A558A0 = 1) is a full stage
reset. It cancels the restart point and clears 0x40 on all records (0x488B30).

---

## 5. Live progress (positions 1st..8th)

### What the game offers
- **No whole-stage path.** Paths exist only locally: loops, rails and enemy paths ("E PATH...",
  "A PATHMOVE"). The camera uses `camset%04d*.bin` SET-style camera volumes, not a spline.
- **Checkpoints: 0 to 10 per stage**, and none in Mission 4 or in many hunting/boss stages.
  **Their SET order is not stage order.** City Escape's records 45/218/343 are in reverse.
  Cosmic Wall and Final Rush are shuffled (section 6).
- **One real goal ring** (type 0) per goal-ring stage. Its position is static, but the straight-line
  distance to it is **not monotonic** on winding stages:
  - Final Rush checkpoints are 13 000 to 24 000 units from the goal, out of order.
  - Cosmic Wall's goal is about 30 000 units from every checkpoint.
  - The Crazy Gadget, Final Chase and Eternal Engine checkpoints get closer and then farther.

### How the mod measures progress
- The first clear of a stage records its route: the player's position every ~50 units, with loops
  caused by deaths cut out. It is saved under `routes/<level>_<mission>.route` and shared with the
  room.
- Each frame, progress is the nearest route point (searched within ±40 points of the last match,
  or along the whole route when the player is more than 200 units away), plus the fraction
  towards the next point, as a 0..1 value.
- Players are ranked: finished first (by race time), then by progress, then those still loading.

A rough measure without a route is `passedCheckpoints * 1e6 - dist3(p, goalRingPos)`:
- `passedCheckpoints` = count of SAVEPOINT records (main 0x6D99A0) with flag **0x40** in the SET
  table (0x1DDE280, `count` at 0x1DDE270). It survives death restarts.
- `goalRingPos` = the SETEntry position (+8) of the GOALRING record whose `rotX % 3 == 0`
  (`== 1` in Mission 3), using the C-style remainder of the sign-extended i16 at SETEntry+2.
  The ring is always loaded, so its SET entry can be read at stage start.

This works on straight stages (City Escape's end, Metal Harbor, Radical Highway, Pyramid Cave)
and is wrong on winding ones.

Hunting (Knuckles/Rouge) and boss stages have no meaningful route.

---

## 6. Per-stage SET data (normal `_s` + `_u` files, Missions 1-4)

Record N = index in the SET table (`_s` entries, then `_u`). Positions are rounded. "to-goal" is
the straight-line distance to the type-0 ring. Level IDs come from the exe's level headers (`STGnn`
name next to each level's init, e.g. "STG13" → init 0x5DCD50 → object list 0x109EE90).

| Level | Object list | Goal ring (type 0) rec / pos | Checkpoint recs (to-goal) |
|---|---|---|---|
| 3 Green Forest | 0x10861C0 | 198 (10973,-1823,11058) | 58 (5333), 69 (3227), 92 (12265), 261 (8455), 377 (4579) |
| 4 White Jungle | 0xE99B00 | 21 (13178,-3569,-5499) | 38, 79, 111, 160, 217, 218, 235, 242, 247, 420 (6472..17093) |
| 6 Sky Rail | 0x1178588 | 228 (-1197,-4856,10065) | 47 (6305), 125 (8113), 333 (8717) |
| 9 Prison Lane | 0x104C408 | 76 (-4440,967,-1133) | 0 (1043), 197 (3490), 360 (2059) |
| 10 Metal Harbor | 0xAECB20 | 2 (8797,-139,-10266) | 164 (10326), 282 (7616), 312 (4160) |
| 11 Iron Gate | 0xBEDEB0 | 2 (6231,-1488,-1229) | 87 (5799), 265 (2663), 305 (4080) |
| 12 Weapons Bed | 0x9CC758 | 72 (4320,-192,-10139) | 24 (4723), 145 (9171), 239 (3087) |
| 13 City Escape | 0x109EE90 | 21 (3955,-22139,16130) | 45 (9705), 218 (16056), 343 (17345) |
| 14 Radical Highway | 0x158DD10 | 105 (-9970,-6973,-18440) | 252 (16970), 290 (7158), 292 (13065) |
| 17 Mission Street | 0x11B8A78 | 441 (7170,630,-6912) | 76 (4723), 93 (9261) |
| 21 Sand Ocean | 0xD01210 | 92 (2895,185,-7030) | 87 (3245), 88 (3096), 242 (4917) |
| 22 Crazy Gadget | 0x9BB5C0 | 98 (-8470,-1068,-2905) | 3 (7178), 132 (5286), 156 (2608), 239 (2210), 386 (4073) |
| 23 Hidden Base | 0xA2DCC0 | 206 (-3035,1310,-5280) | 32 (3802), 51 (2561), 255 (1443) |
| 24 Eternal Engine | 0x984490 | 186 (220,-1798,-9150) | 208 (7535), 279 (3442), 532 (5096), 547 (4588), 575 (5325) |
| 27 Lost Colony | 0x9FF378 | 281 (4710,1600,-2400) | 191 (6533), 228 (5527), 263 (3577) |
| 28 Pyramid Cave | 0xAB6670 | 5 (960,-4030,-22190) | 89 (13624), 98 (5980), 409 (16587) |
| 30 Final Rush | 0x16A42A0 | 207 (2530,-26465,7445) | 157, 181, 241, 286, 290, 363 (13339..24471) |
| 31 Green Hill | 0x10EE3E8 | 162 (-1970,-1360,-6950) | 3 (4612), 169 (2347) |
| 40 Final Chase | 0x1536FD8 | 315 (122,-9930,7627) | 265, 305, 336, 353, 370, 413 (4002..11097) |
| 43 Cosmic Wall | 0xEAEE98 | 40 (3550,-1570,29690) | 34, 63, 166, 203, 215, 239, 353 (20999..31071) |

- Hunting and boss stages also have a type-0 and a type-1 ring but no checkpoints. These are 5,
  7, 8, 16, 18, 25, 26, 32, 34 and 44. Mad Space (44) has only the type-1 ring.
- The other Cannon's Core parts (35..38) have no goal ring in the SET file.
- Mission 5 loads `_hd_` files and 2P loads `_2p_` files. Their record numbers differ (not dumped).

---

## Confidence

**High:**
- timer bytes and the tick (0x44DC80);
- TimerStopped;
- LevelClear 0x43E6D0 (ESI = player) setting 0x174B002 and the timer stop;
- GOALRING main/touch → 0x43E6D0;
- SAVEPOINT touch → 0x43E520 and flag 0x40;
- the Mission 4 checkpoint kill;
- the RestartPoint layout (0x1934B88, 0x2C);
- the restart path rewinding the time (0x43E3F0 → SetTime 0x44DD50, or 0x44CAC0 without a
  checkpoint);
- the per-stage SET data (parsed from the game's files with the exe's object lists).

**Medium:**
- the meaning of request codes 3/4/5/7/8 and states 9/11..15;
- 0x43F330 being "mission failed";
- 0x43ECA0 = save the clear record;
- results task names;
- the 0x1A5A230 record fields;
- the level ID ↔ name mapping.

**Low:**
- 0x174AFF7 / 0x174AFF9 roles;
- 0x1AEFD74 meaning.
