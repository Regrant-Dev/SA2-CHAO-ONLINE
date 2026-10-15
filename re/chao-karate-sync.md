# Chao Karate: fight flow, start hold, fighter state, result (for network sync)

Steam `sonic2app.exe`, image base 0x400000. Read `chao-stadium.md` and `chao-race-sync.md` first.
Most karate functions *are* defined in Ghidra (0x5416E0..0x5430C0 and 0x5779C0..0x578EB0
decompile). The two exec entry points 0x542DA0 and 0x578D30 are not, so they were read from the
objdump listing.

Notation:
- `g` = `*(u8**)0x1A5D148`. This is the karate global block (heap, 0x40 bytes, it is the main
  object's Data2/+0x40).
- `M` = `*(ObjectMaster**)0x1A5D14C`. This is the karate main object.
- `MD` = `M->Data1` (+0x34).
- For a fighter object F: `FD` = `F->Data1` (+0x34), `W` = `F->Data2` (+0x40, 0x4C-byte work).
  The task brief used offsets from the allocation base (base = W-4). This file uses offsets
  from W, which is what the game code uses.

---

## 0. Globals

### Karate main object
- **`M` = 0x1A5D14C**, `g` = 0x1A5D148.
- Both are set in the creator at 0x5415ED/0x541641 and cleared in the delete sub 0x5416A0.
- MainSub = **0x542DA0** (`KarateMainExec`).
- The creator sets `MD+4 |= 0x400` when CurrentChaoArea (0x134046C) == 0xB. That is the
  battle-mode 2P karate area. In Chao World (area 10) the bit is clear.

### `g` layout
| off | type | meaning |
|---|---|---|
| +2 | u8 | **one-frame event** for other objects. KarateMainExec zeroes it at the start of every run (0x542DD7). State handlers set it on a transition: 1 = mode/chao chosen, 3 = confirm, **4 = "FIGHT!" (fight begins)**, 5 = fight ended, 7 = fade, 8 = result, 9 = prize |
| +3 | u8 | **busy / fight-end code**. Menus and announcers set 1 while busy. **2 = KO or ring-out**, **3 = time up**. KarateMainExec clears it at the end of every run unless the state is 0xF (0x542EC9). |
| +4 | u8 | fight phase: 2 = fighting (set at 0x5425E4), 3 = ended by KO/ring-out, 4 = ended by time-out |
| +5 | u8 | 0 = 1P (tournament vs CPU), 1 = 2P / VS (both fighters are real records) |
| +6 | s8 | tournament (cursor 0..3: Beginner / Standard / Expert / Super) |
| +7 | s8 | round within the tournament, 0..4 (5 = all won). Set to 0 in state 8. Incremented after a 1P win (0x542560) |
| +9 | u8 | copy of the chosen Chao record +0x10B |
| +0xA | s8 | menu cursor |
| +0xB | s8 | **result: 0 = side 0 won, 1 = side 1 won, 2 = draw** (0x577DE0) |
| +0xC | u16 | **fight clock, seconds left**. Set to 90 (0x5A) by the timer creator 0x575140 |
| +0x10 / +0x14 | ObjectMaster* | fighter side 0 / side 1 (KarateChaoExec objects) |
| +0x18 / +0x1C | ChaoData* | records for side 0 / side 1. In 2P both are used; in 1P only +0x18 |
| +0x20 / +0x24 | float* | &W0+0x3C / &W1+0x3C (HP, read by the HUD) |
| +0x28 / +0x2C | float* | &W0+0x38 / &W1+0x38 (morale gauge, read by the HUD) |
| +0x30 | ObjectMaster* | menu background object |
| +0x34 | ObjectMaster* | timer object |
| +0x38 | ObjectMaster* | object 0x574FE0 (camera?) |
| +0x3C | ObjectMaster* | menu window object |

---

## 1. Fight flow (KarateMainExec 0x542DA0)

The state is `MD[0]` and the sub-state is `MD[1]`.
- Jump table **0x542EDC**, 17 entries.
- If the pause byte 0x174AFF7 is set, nothing runs.
- If `MD[0]==0x11`, it only calls 0x47E750 (suspended).

| state | handler | meaning |
|---|---|---|
| 0 | inline | sets state 2 |
| 1 | – | idle |
| 2 | 0x5416E0 | creates menu objects. Goes to 3, or to 6 with g+5=1 when `MD+4 & 0x400` |
| 3 | 0x5417E0 | **mode select** 1P/2P (cursor g+0xA). A: `g+5=cursor`, then state 4 (1P) or 6 (2P). B: state 0xF |
| 4 | 0x541C60 | 1P Chao select (0x554490). On select (0x1AED254==4): `g+0x18=*0x1A5CC00`, `g+9=rec[0x10B]`, then state 5. Back: state 3 |
| 5 | 0x541A50 | **tournament select**. A: `g+6=cursor`, `g+2=1`, `0x1A5CC24=1`, then state 7. B: state 4 |
| 6 | 0x541E60 | 2P Chao select. On select: `g+0x18=*0x1A5CC00`, `g+0x1C=*0x1A5CC04`, then state 7 |
| 7 | 0x542070 | **confirm** screen. Waits for A, **but goes on immediately when g+5==1**. Then g+2=3 and state 8 |
| 8 | 0x5421C0 | closes the menu (g+0x30 object state 4). After 2 frames with g+3==0: **g+7=0**, `0x1A5CC24=0`, state 9 |
| 9 | 0x542250 | **fight setup / intro**: see below |
| 10 | 0x542510 | round announcer 0x571470(g+7). It holds g+3=1 while showing. When done: g+2=4, state 0xB |
| 11 | 0x542560 | **fight**. sub0: 0x5716B0 (the "FIGHT" sprite), `g+4=2`. sub1: waits for g+3==2/3, then g+4=3/4, `g+0xB=0x577DE0()`, and in 1P on a win g+7++. sub2: then state 0xC |
| 12 | 0x5428C0 | result. Fighters are placed at 0x1349B68/0x1349B74 by 0x5795E0, plus win/lose motions. Next: 1P win with round==5: state 0xE (prize). 1P win otherwise: state 0xD, then the next round. 1P loss, or 2P: `MD+4 |= 0x800`, then state 0xD |
| 13 | 0x542C90 | fades out and deletes both fighters (MainSub=0x46F720) and clears g+0x10..0x2C. If `MD+4 & 0x400` or `& 0x800`: state 2 (back to the menu). Otherwise state 9 (**next round**) |
| 14 | 0x542B60 | tournament prize/emblem (sets 0x1DEF831[g+6]). Then state 0xF |
| 15 | inline | exit: `0x52B5B0(1)`, or in the battle area `0x1A558A4=1`. Then state 0x10 |
| 16 | inline | g+3=1 (dead) |

### State 9 detail (0x542250)
- **sub0**: timer `MD+6 = 470`, then spawns the fighters (0x578EB0).
  - Side 0: `EAX=g+0x18`, pos 0x1349B50, angle 0x4000, mode 1.
  - Side 1:
    - 1P: CPU entry from 0x1296954 + (g+6*5+g+7)*10, mode 0. The call is at 0x542308. With
      table byte +8 >= 0 it does one `rand()` that may switch to the alternate entry
      0x1296940.
    - 2P: `EAX=g+0x1C`, mode 1 (call at 0x54232B), pos 0x1349B5C, angle 0xC000.
  - Fills g+0x10..0x2C, then sub1.
- **sub1**: counts down. At 300 it creates the HUD, the timer (0x575140, which writes
  **g+0xC=90**) and so on, then sub2.
- **sub2**: `old=MD+6; MD+6=old-1; if old==0 → sub3`. That is about 300 frames.
- **sub3**: when g+3==0, state 10.

### Recipe: make a follower start a fight (recommended: VS mode, g+5=1)
VS mode spawns **both** sides as real records with mode 1:
- no CPU table and no `rand()` in the setup,
- no tournament progression and no prize,
- after the result it goes back to the menu (state 2, then 3), because state 12 sets 0x800
  when g+5 != 0.

Steps:
1. Preconditions: `M != 0`, and `MD[0]` is in the menu (3, 4 or 5; best 3 = mode select)
   with g+3 == 0.
2. Write:
   - `g+5 = 1`
   - `*(ChaoData**)(g+0x18) = own record` (pointer into the save Chao array)
   - `*(ChaoData**)(g+0x1C) = friend record`. This must be a stable 0x800-byte buffer owned by
     the mod. Keep +0x80 type >= 2 and +0x438 != 0, as in the race notes.
3. Set `MD[0] = 7; MD[1] = 0`. State 7 sees g+5==1 and goes straight to state 8, then state 9.
   - Alternative: `MD[0]=8, MD[1]=0` skips the confirm window update.
   - The window object at g+0x3C may stay visible for a moment. States 4/5/6 normally close it
     by writing `win->Data1[8]=0xFF; [9]=1; [0xA]=1; (u16)[0xC]=(u16)[0xE]=(u16)[0x10]=(u16)[0x12]=0xFFFF`.
     Do the same.

With 1P instead (g+5=0):
- set g+0x18 = own, g+6 = tournament, `MD[0]=7` (this waits for A), or `MD[0]=8`.
- Side 1 comes from the CPU table unless the existing 0x542308 redirect is used.
- After a win, 1P **advances to the next round** (state 13 then 9). To force a return to the
  menu, set `MD+4 |= 0x800` while `MD[0]==0xC/0xD`.

Confidence: state table and handlers high (decompiled). Skipping the menu via `MD[0]=7/8`
medium (not run).

---

## 2. Holding the start

The fighters stay in their intro action 1 until they see **g+2==4** (0x578010). The timer
object 0x5750C0 also waits for g+2==4 (in its state 1). g+2==4 is only produced at the end of
state 10, so holding anywhere before that holds everything.

**Recommended clamp:** each frame, if `MD[0]==9 && MD[1]==2 && *(u16*)(MD+6) < 2`, write
`*(u16*)(MD+6) = 2`.
- This holds after the fighters, HUD and timer exist (the "fighters in the ring" intro) and
  before the round announcer.
- Release by no longer clamping. That gives about 2 frames, then state 10 (announcer, fixed
  length), then state 11.
- Release both games on the same network tick, as for the race.

Alternative: clamp in sub1 before HUD creation (`MD+6 > 301`). This is less useful because the
HUD isn't there yet.

Confidence: high (the sub2 decrement-and-compare is read directly).

---

## 3. Fighter state

### Fighter object (`KarateChaoExec`)
- Created by **0x578EB0**. MainSub is **0x578D30** (`mov edi,0x578d30` at 0x578ECA).
- The visible Chao is a separate object, `W+0x08` (created by CreateChao 0x5501D0). It is a
  puppet.
- **Each frame 0x578D30 copies `FD+0x14..0x1C` (pos) and `FD+8/+0xC/+0x10` (rotation ints)
  into the Chao's Data1** (0x578DB0..0x578DE3). So **the fighter's FD pos/rot is the
  authoritative transform.**

### `FD` (fighter Data1)
| off | meaning |
|---|---|
| +0 | action (AI state, see below) |
| +1 | sub-state |
| +3 | 0..60 frame counter |
| +4 | u16 status bits (below) |
| +6 | u16 timer |
| +8, +0xC, +0x10 | rotation X/Y/Z (int BAMS) |
| +0x14 | position vec3 |

Status bits at `FD+4`:
- 0x100 hit received (pending)
- 0x200 guarding
- 0x400 my attack connected
- 0x800 morale broken / "gives up" (forces action 7)
- **0x1000 lost (KO or ring-out)**
- 0x2000 cheer
- 0x4000 counter / guard-break
- 0x8000 motion finished

### `W` (fighter work, `F+0x40`)
| off | meaning |
|---|---|
| +0x02 | u16 side (0/1) |
| +0x04 | u16 flags: 1 = weak child, 2 = in-air/knocked, **4 = CPU** (stats randomised by `rand`), 8 = from rec+0xD6 |
| +0x06 | u16 CPU id |
| +0x08 | Chao ObjectMaster* |
| +0x0C | motion controller ptr: `[0]` motion id, `[1]`,`[2]` sub, `[5]` event bits 0x08/0x10 |
| +0x10 | source record ptr |
| +0x18 / +0x1C | ptrs to Chao Data1+0x5C0 / +0x5CC |
| +0x20 | swim+1000 (defence) |
| +0x24 | fly+1000 |
| +0x28 | (run*1.5+1000)/1000 (speed) |
| +0x2C | power+1200 (attack) |
| +0x30 | (run*0.5+1000) - power/3 |
| +0x34 | intelligence |
| +0x38 | **morale gauge**, starts at 1000.0 (0x1281EA4) and is capped at 1000 |
| **+0x3C** | **HP current** = stamina+1000 |
| +0x40 | luck |
| **+0x44** | **HP max** |
| +0x48 | AI approach/attack accumulator |

Stats are the raw value in areas 6/10/11, and x3 elsewhere.

### Actions (jump table 0x578E2C)
- 0 → 1
- **1** 0x578010: intro pose, waits for g+2==4
- **2** 0x5780F0: approach (the rand accumulator in W+0x48)
- **3** 0x578230: attack wind-up (the rand picks the move)
- **4** 0x578370: attack resolve. The hit test is 0x5778D0; on a hit it sets the opponent's
  `FD+4 |= 0x4000`/0x100
- **5** 0x578510: **take damage** (`rand`-scaled)
- **6** 0x578890: guard/taunt
- **7, 8, 9: no handler** (7 is the "gave up / dazed" pose, driven only by motion)
- 10 0x578B30: knocked down
- 11 0x578C20: result pose (waits for g+2==8)

Per frame after the switch, in order:
- **0x5779C0**: hit intake. Only in actions 2/3/4/7. Pending hit → action 5, or action 6 when
  guarding. In other actions it clears the hit bits.
- **0x577A40**: morale drain, cheering (pad A in 1P; per-side pads 0x1DBED44 in 2P),
  `rand`-based give-up → action 7. Skipped in actions 1, 10 and 11.
- 0x5777A0: motion player (switch on motion id).
- 0x796780 / 0x798300: physics/collision.
- **0x577E30**: ring-out check.
- 0x578CD0: collision copy.

### End conditions (all need g+4 == 2)
- **KO** (inside 0x578510, at 0x578675):
  - `HP -= dmg`. If HP < 0: `HP = 0`, `g+3 = 2`, `FD+4 |= 0x1000`.
  - Damage is `attacker.W+0x2C * rand factor - defender.W+0x20`, with a minimum of
    `5% of HPmax`.
  - It also drains morale `W+0x38 -= dmg/3`.
- **Ring-out** (0x577E30, at 0x577F9A): if `FD.y < 7.0` (or the in-air flag with
  `|vel| > 16`), then `g+3 = 2`, `FD+4 |= 0x1000`, `W+4 |= 0x100`.
- **Time-up** (timer object 0x5750C0, at 0x575110):
  - g+0xC is decremented every 60 frames.
  - At 0 with g+4==2: `g+3 = 3`.
- **Judge** (0x577DE0, which writes g+0xB):
  - g+4==3 (KO/ring-out): **result = (side0 FD+4 & 0x1000) ? 1 : 0**. Only side 0's flag is
    read.
  - g+4==4 (time-up): **0x5430C0** compares `round(W.HP/W.HPmax*100)` for both sides.
    Equal → 2 (draw), otherwise the higher one wins (0/1).

### What to overwrite on the follower
**Recommended ("puppet" fighters):**
1. When the fight starts (follower sees `MD[0]==0xB`), put both follower fighters in **action 8**
   (`FD[0]=8, FD[1]=0`) and keep them there.
   - Action 8 has no handler.
   - 0x5779C0 discards hits in action 8.
   - 0x577A40 only drains morale (no action change outside action 2).
   - So no AI, no attacks, no local damage, no `rand` decisions.
2. Wrap the fighter MainSub (`F+0x10`, original 0x578D30), like the race wrapper. Call the
   original, **then** write the network values:
   - `FD+0x14..0x1C` (pos), `FD+8/+0xC/+0x10` (rot).
   - **Also the same into the Chao's Data1** (`(*(W+8))->Data1 +0x14 / +8..`), because the
     original copied the old values to it earlier in this frame.
   - `W+0x3C` HP, `W+0x38` morale.
3. Animation: stream the motion id `*(u8*)(*(W+0xC))`. When it changes, apply it the way the
   game does:
   `m=*(u8**)(W+0xC); FD+4 &= 0x7FFF; m[5] &= 0xE7; m[0]=id; m[1]=0; m[2]=0;`
   (the inline pattern at 0x579689, for example). 0x575A00 is the game's helper. Its regcall
   was not verified.
   - Optional hit effects: on a network "hit" event, momentarily set the target's action to 5.
     It plays the effect/sound and damage, and the HP is overwritten afterwards. Then put it back
     to 8.
4. Write `g+0xC` (clock) from the leader every frame.
5. Stop all overrides once `MD[0] >= 0xC`. State 12 (0x5795E0) re-positions the fighters and
   sets the result motions itself.

**Do not overwrite** (or don't rely on):
- `FD+4` status bits other than as described,
- `W+0x08/+0x0C/+0x18/+0x1C` pointers,
- the Chao object's MainSub,
- the camera object (0x574480 / g+0x38). It reads `g+4` and calls 0x577DE0 itself at 0x57574C.

Simpler fallback: let the follower AI run and only overwrite pos/rot/HP/morale after the
original MainSub. It works, but the animations won't match and local hits can still fire the
end triggers, so the patches below are required.

### Making the native end triggers inert on the follower
Byte patches, only while a network fight is active:
- **0x578679** `75 11` → `EB 11`: KO never sets g+3/0x1000.
- **0x577F9D** `74 0B` → `90 90`: ring-out never fires. It falls into the existing
  `fstp st0; return 0` path.
- **0x575114** `75 04` → `EB 04`: time-up never sets g+3=3. The timer still stops at 0.

With puppets, only the time-up patch is strictly needed. The other two are a safety net.

### Ending the fight on the leader's signal
The leader sends `{code: 2|3, result: 0|1|2}` after its g+0xB is known (at `MD[0]==0xC`).

On the follower:
- **Wrap the main MainSub** (`M+0x10`, original 0x542DA0). If a pending end exists and
  `MD[0]==0xB && MD[1]==1` and the pause byte 0x174AFF7==0:
  - code 2: set `(follower side-0 FD)+4 |= 0x1000` **iff follower side 1 won**, then `g+3=2`.
  - code 3: write the final HP of both sides first (so 0x5430C0 gives the same answer,
    including a draw), then `g+3=3`.
- Then call the original.
- It must be set **inside** the wrapper, because KarateMainExec clears g+3 at the end of every
  run.

Confidence:
- End triggers, judge and HP/max offsets: high (decompiled).
- Puppet via action 8: medium-high (read, not run).
- Motion id write: medium.

---

## 4. Sides and mirroring
- Spawn points:
  - side 0: **(-5, 8, 19)**, yaw 0x4000
  - side 1: **(+5, 8, 19)**, yaw 0xC000
- Result points: side 0 (-7, 8, 19), side 1 (+7, 8, 19).
- Camera default: 0x1297C50 = (0, 13, 37).
- The ring is symmetric about the plane **x = 0** (z is about 19). Ring-out depends only on Y
  (`y < 7.0`), not X.
- The follower has its own Chao on side 0 and the leader's Chao on side 1. Map:
  - follower side0 ← leader side1, follower side1 ← leader side0
  - pos `(x,y,z) → (-x, y, z)`
  - rot Y `→ (0x10000 - rotY) & 0xFFFF` (0x4000 ↔ 0xC000 checks out). Rot Z → −Z if non-zero.
    Rot X unchanged.
  - HP/morale swap with the sides.
  - result 0 ↔ 1 (2 stays 2).
- The HUD shows g+0x18's record name on the left (side 0). Swapping the records on the follower
  is the reason the mirroring is needed. If you don't mind the follower's own Chao on the
  right, use the leader's side order and skip the mirroring.
- Confidence: positions high (read from data). Arena mesh symmetry medium (inferred from the
  symmetric spawn/result points).

---

## 5. RNG
- `rand` = **0x7A89D8**. It is the MS CRT LCG `seed = seed*0x343FD + 0x269EC3; return
  (seed>>16)&0x7FFF`. The seed is **per-thread** (`_getptd()+0x14`, 0x7AC4D3), not a fixed
  global.
- `srand` = **0x7A89C6** (cdecl, 1 arg). 2794 call sites use `rand`.
- Karate uses `rand` for:
  - the CPU stat jitter (0x578EB0, only with W+4&4),
  - the 1P CPU-alternate pick (0x5422CA),
  - every AI decision (0x5780F0, 0x578230, 0x578890, 0x577A40),
  - damage (0x578510).
- Seeding both games with `srand(x)` at fight start (call 0x7A89C6 on the game thread) only
  gives lockstep if every `rand` consumer runs identically on both machines. Effects, other
  objects and frame drops make that fragile. So it is **only a fallback**. The streamed
  pos/HP plus forced end above is the reliable path.
