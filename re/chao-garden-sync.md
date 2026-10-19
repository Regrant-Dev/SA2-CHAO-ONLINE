# Chao Garden: per-frame update, motion, being held (for mirroring a visitor live)

Steam `sonic2app.exe`, image base 0x400000. Everything below was read from the objdump listing
of `re/bin/sonic2app.dumped.exe` (most of this code has no Ghidra functions). Read
`chao.md` and `chao-race-sync.md` first: garden, race and menu Chao are the same object type
and share the Data1 layout.

**Correction to `chao-race-sync.md`:** 0x56CCF0 is not the skeletal motion player. It updates
the model/shape block at Data1+0x510 and builds the model matrix (0x56FC20). The motion
(animation) controller is the struct at **Data1+0xAC**, advanced by **0x7938D0** (see section 2).

Notation: `obj` = ObjectMaster*, `D` = `obj->Data1` (+0x34, `OBJ_DATA1` in the mod),
`W` = `*(obj+0x38)` (physics work: velocity at +0/+4/+8).

---

## 1. Garden Chao object and its per-frame update

### What CreateChao (0x5501D0) installs
| ObjectMaster field | Value | Notes |
|---|---|---|
| +0x10 MainSub | **0x54FE20** | `LoadObject(list 2 or 4, "…", EDI=0x54FE20)` at 0x550263..0x55026D |
| +0x14 DisplaySub | 0x54FF80 | Picks the draw by record byte `+0xDD` (jump table 0x55018C); normal Chao: **0x540B60** |
| +0x18 DeleteSub | 0x54FF30 | Frees `D+0x5C` **only if `D+0x58 != 0`** (copy mode). With `copy=0` (what visitors.rs passes) our buffer is never freed by the game |
| +0x1C or +0x20 | 0x5501A0 | Delayed display (area-dependent table at 0x550608); jumps to 0x53CEB0 (shadow etc.) |
| +0x34 Data1 | 0x1D74-byte block | Allocated at 0x55028E |
| +0x3C | world-entity entry | Set by 0x530750 (see below) |

Flags CreateChao sets in **`D+0xA0`** (0x550515..0x5505A5): `|= 0x200, 0x8, 0x2, 0x4, 0x10,
0x20000, 0x20, 0x40, 0x40000, 0x80000, 0x100000`, `&= ~0x80, ~0x2000`.

### MainSub 0x54FE20 (cdecl, `void(ObjectMaster*)`)
```
if (D+0x30 <= 2)      0x53CAC0(EAX=obj)          // first frames: emotion-ball (D+0x6EC) init
if (*0x1DBE634 != 0) {                            // global "update Chao" gate
    0x53DC40(EAX=obj)                             // AI: interrupts + behaviour stack  [gated by D+0xA0 & 0x10]
    if (D+0xA0 & 0x4)  0x7938D0(ESI=D+0xAC)       // advance the motion (animation)
    0x53CB70(EAX=obj)                             // emotion ball (D+0x6EC)
    0x53A3E0(EAX=obj)                             // face timers: eyes D+0x68C/0x692, mouth D+0x698/0x69E
    0x56CCF0(EBX=obj)                             // model/shape block D+0x510, builds matrix from D+0x14/D+0xC
    if (D+0xA0 & 0x2)  0x796780(EAX=obj)          // unknown (menu Chao clear this bit)
    0x53DF00(EAX=obj)                             // physics, mode u16 D+0xA4: 0 ground 0x798300, 2 race path 0x53E3E0
    D+0x30++                                      // frame counter
    if (D+0xA0 & 0x8)  0x47E750(ESI=obj)          // register collision (others can hit / pick up)
    else               0x47E6C0(EAX=obj)          // clear collision
    if (rec+0xDD == 3) 0x7938D0(ESI=D+0xF8)       // second motion table, special record type only
    if (D+0xA0 & 0x80000) 0x540FD0(...)           // something at D+0x5C0..0x5D4
}
```
- 0x53DF00 returns immediately when `W+0x40 & 0x10`.

### AI tick 0x53DC40 (EAX=obj)
- **Returns at once when `D+0xA0 & 0x10` is clear.** That is the same bit as the race (0x53FAF0),
  and it skips everything below:
  - **interrupt checks 0x53DB10**: water 0x561630, whistle 0x5637B0, being held 0x566720, hit
    0x563D30, fruit/toys/etc.
  - "think" timers 0x562C00 (skipped when `D+0xA0 & 0x2000`)
  - the behaviour stack: `D+0x434` count, `D+0x438` index, `D+0x43C[]` fn, `D+0x47C[]` timers.
    A behaviour returns 1 = pop, 2 = choose next (0x562A80/0x562C50), 3 = reset to 0x565BE0.
- Behaviour struct at D+0x1B4: `+0x1B6` u16 mode (sub-state), `+0x1B8`, `+0x1C0` timer,
  `+0x1C4` timer, `+0x430` previous fn. `AL_SetBehavior` 0x53D890 (EAX=obj, push timer, push fn,
  cdecl pop 8) as in the race notes. 0x53D970 (EAX=obj, EDX=fn) pushes a follow-up behaviour.

### How the game itself makes a "display-only" Chao
Menu code does exactly this right after `CreateChao` (0x5560CD, also 0x556695, 0x556D16,
0x55721D, 0x579504, 0x58745D, 0x590E17, ...):
```
D+0xA0 &= ~0x8;  &= ~0x2;  &= ~0x10;  &= ~0x20;  &= ~0x80000
```
MainSub stays 0x54FE20. With 0x10 clear there is no AI, with 0x8 clear there is no collision,
and with 0x4 still set the motion keeps playing.

### World entity list (finding a Chao object from its record)
- Counts: `0x1DC0F80[cat]` (u32). Limits: `0x8AB838[cat]`: cat 0 (Chao) = **32**.
- Entries: `0x1DC0FC0 + cat*0x1200 + i*0x48`.
  - +0x2 u8 → copied to `D+3`
  - +0x4 u16 kind
  - **+0x8 ChaoData\***
  - +0xC int
  - **+0x3C ObjectMaster\*** (0 = free)
  - +0x40 linked entry (interaction partner)
- Registered by 0x530750 (ECX=cat, EBX=obj, stack: kind, data), called from CreateChao with
  cat 0. Removed by 0x5307F0. `obj+0x3C` points back at the entry.
- To find the owner's Chao for save slot `n`: scan cat 0 entries for
  `+0x3C != 0 && +0x8 == &ChaoSlots[n]` (0x19F6EC4 + n*0x800).

---

## 2. Motion (animation) controller

### `MotionTable` at **D+0xAC** (0x4C bytes)
| off | type | meaning |
|---|---|---|
| +0x00 | u16 | flags. **bit 0x2 = blending into the next motion**. Bit 0x1 = "reached end / loop" pulse (cleared each frame by 0x7938D0; read by 0x793F70) |
| +0x02 | u16 | from table entry +6 |
| **+0x04** | int | **current motion id** |
| **+0x08** | int | **next id**: blend target while bit 0x2 is set, otherwise the motion queued after the current one (table entry +8, -1 = none) |
| +0x0C | f32 | speed multiplier, reset to 1.0 every frame |
| +0x10 | f32 | blend step (1/frames) |
| +0x14 | f32 | blend progress 0..1 |
| +0x18 | u16 | play mode of the current motion (0..7, jump table 0x793C1C: loop, hold, chain, ping-pong ...) |
| **+0x1C** | f32 | **current frame** |
| +0x20 / +0x24 | f32 | start / end frame |
| +0x28 | f32 | frame step |
| +0x2C | ptr | current NJS_MOTION* |
| +0x30..+0x44 | | the same fields for the next motion (mode, start, end, step, motion*) |
| +0x48 | ptr | motion table: entries of 0x1C bytes `{motion*, u16 mode, u16, int next, int blendFrames, f32 start, f32 end, f32 step}` |

- The table comes from the Chao DLL export **"al_motion_table"** (string 0x1304824). 0x55C370 does
  `GetProcAddress(*(*0x1AF0220), …)` via the import at 0x8730D4, then `0x793880(EAX=D+0xAC, push table)`.
  Every Chao uses the same table, so **motion ids are global**. An id read on the owner's game
  plays the same animation on any other Chao.
- `D+0xF8` is a second MotionTable used only for records with `+0xDD == 3`.

### Functions (all usercall, no return value)
| Addr | Call | Effect |
|---|---|---|
| **0x793C40** | **EAX = D+0xAC, EDX = id**, no stack args; clobbers ECX/EDX | **`SetMotionLink`**: blend into `id`: `+8=id`, flags=2. Blend length from the table (default constant if negative). This is what behaviours use (e.g. `0x793C40(D+0xAC, 0x101)` = held) |
| 0x793E40 | EAX = mt, EDX = id | Hard switch to `id` without a blend, frame = start, `+8` = table next |
| 0x793D30 | EAX = mt, EDX = id, push u16 frames (caller pops 4) | Blend with an explicit length (used by 0x565BE0) |
| 0x793D90 | EAX = mt, EDX = id | Blend that keeps the phase |
| 0x7938D0 | ESI = mt | Per-frame advance (MainSub calls it when `D+0xA0 & 4`) |
| 0x793880 | EAX = mt, push table | Init, starts motion 0 |
| 0x793F70 | EAX = mt | Returns flags & 1 |

Motion ids seen in code: 0x101 held (calm); 0x102/0x103/0x105/0x107 held happy; 0x109..0x10E
held struggling (dislikes holder); 0x3E/0xC/6/0x1B race idle; 0, 1, 3, 0xEE, 0xEF after being
put down (0x565BE0).

### Face (optional to sync)
| Addr | Call | Effect |
|---|---|---|
| 0x53A4B0 | ECX = obj, EDX = eye id, EAX = duration (-1 = until changed) | eyes. Writes `D+0x68C` (timer) and `D+0x692` (eye id) |
| 0x53A5A0 | ECX = obj, EDX = duration, push mouth id (caller pops 4) | mouth. Writes `D+0x698` (timer) and `D+0x69E` (mouth id) |

The default eye is at `D+0x694`. 0x53A3E0 counts the timers down.

---

## 3. Being held by the player

### Player side
`CO2 = *(u8**)0x1DE9600` (player 1 CharObj2), `P = *(u8**)0x1DEA6C0` (player 1 EntityData1).
| Field | Meaning |
|---|---|
| **CO2+0x94** | **HeldObject** (ObjectMaster*) |
| CO2+0x98 | object to pick up (throw/catch path) |
| CO2+0x9C | object in front, the pickup candidate. Found by 0x45A160 from the player's collision hits: needs the target's `Data1+4 & 0x40` and not `& 0x8000`, within ±0x3000 of the player's facing |
| P+4 bit 0x800 | player is holding something |

**Pick up** (Sonic-family code at 0x725582; the same pattern is at 0x722921, 0x73FAD7 and in
every character):
```
CO2+0x94 = CO2+0x9C (or +0x98);  held->Data1+4 |= 0x8000 (0x8200 on the 0x725582 path);
held->Data1+2 = 0xA;  P+4 = (P+4 & ~8) | 0x800;  player action/anim set to the carry anim
```
**Release:** 0x46E5E0 (EAX=player index, EDX=P), 0x475600 (EAX=P, push CO2) and 0x4755D1:
`held->Data1+4 &= 0x7FFF; P+4 &= ~0x800; CO2+0x94 = 0`. A throw also writes a velocity into
`held->W+0..+8`.

The type the game uses for a Chao is 0x45A0C0: `Data1+2` 0xA or 0xC → 0x200, "two-handed
carry".

**Position each frame (written by the player, not the Chao):** 0x725C70 (Sonic/Shadow family,
called from 0x720B1B) and 0x740950 (other characters, from 0x73F196 and 0x750C36). When the
held object is type 0x200, these write `held->Data1+0x14/18/1C`:
```
pos = (handA + handB) * 0.5 + 1.2 * normalize(dirA + dirB)
handA = CO2ext+0x1E8, handB = CO2ext+0x1F4, dirA = +0x284, dirB = +0x290
```
A second formula is used for carry anims 0x31..0x34 and 0x49 (+0x26C ×2.5).

### Chao side
- "Held" is **`D+4` (u16 Status) bit 0x8000**. Nothing else is needed.
- Interrupt **0x566720**: if Status & 0x8000 and the current behaviour is not a held one, it
  zeroes W, sets `W+0x1C` from the player angle, and `AL_SetBehavior(obj, 0x566670)`.
  0x566670 then picks, by the bond with the current character (record +0x16C table, via
  0x53E0C0):
  - **0x5660A0**: likes (> 50). Motion 0x101/0x102/0x103/0x105/0x107, happy face
  - **0x5662B0**: neutral. Motion 0x101
  - **0x566300**: dislikes (< -50). Motion 0x109..0x10E, struggle voice
  - **0x566580**: sleepy (emotion +0x136 > 500). Motion 0x101 and sleep icon
- All held behaviours clear `D+0xA0 & 0x8` (no collision while held) and call **0x565DC0**
  every frame. While held, 0x565DC0:
  - eases `W+0x1C` toward `W+0x18` (0x446960) and sets `D+0xC = W+0x1C - P->Rot.y`
    (facing relative to the holder),
  - zeroes the velocity,
  - every 600 frames may move between happy/neutral/dislike. At the extreme it calls
    0x46E5E0 and the Chao **jumps out of the hands**.
- When 0x8000 is cleared, 0x565DC0 sets `D+0xA0 |= 0x8`, then sets one of:
  - 0x565BE0: put down (player action list contains 0x14)
  - 0x564320: dropped
  - 0x53D720: thrown, using the velocity in W
- Drawing: the Chao draw 0x540B60 adds **(0, -1.0, 0)** before the position when Status &
  0x8000 (0x540CBB). The delayed display 0x53CEB0 also lowers something (0x53D01E/0x53D1FF).
  So a held Chao is drawn 1 unit below `D+0x14`.

---

## 4. Save risk for a visitor

- Own Chao objects point straight into the save slots (`D+0x5C` = &ChaoSlots[n], `D+0x58=0`).
  The game updates the record in place, and saving writes the whole region
  `0x19F6460 + file*0xCA6C` (0xCA6C bytes; save/load 0x52E570/0x52E630 → 0x532570/0x532360).
- The pre-save sync 0x52F090 walks only world-entity categories 2, 3, 7 and 9 (0x52E710: copies
  object positions into their records) and calls 0x52E6B0 for categories 4 and 5. **Category 0
  (Chao) is never walked**. This pass found no code that copies a live Chao's record into a
  free slot (only 0x800-byte copies: the stadium select at 0x5536A0/0x553B57 into its own buffers).
- A visitor created with `copy=0` keeps our buffer as `D+0x5C`. The game writes only into our
  buffer: garden byte, stats, emotion. The delete sub does not free it (`D+0x58 == 0`), and
  0x531B10 (spawn from slots) never sees it.
- Residual risks (not fully traced):
  - The 4-entry carry/transfer buffer at `*0x1A5CB54` (4 × 0x1084, a 0x800 record copy at
    +0x9C, source pointer at +0x88). 0x531CAC checks it while spawning garden Chao. It looks
    like the transporter/"take a Chao along" system. A visitor the local player picks up and
    carries through an exit could end up there.
  - Picking up a visitor at all lets the local AI-side code run on our buffer.
  
  **Mitigation:** make the mirror unpickable. Keep `D+0xA0 & 0x8` clear (no collision
  registration, so 0x45A160 never finds it), and/or keep Status 0x8000 set (0x45A260 rejects
  held objects). Both are part of the recipe below. Do not use `copy=1`: the game would then
  own a heap copy, and the delete sub frees it, but the record would still not reach the
  slots. It gains nothing.

---

## Driving a visitor from the network

### Owner (reporting its real Chao)
1. Find the object: scan the cat 0 world entities (section 1) for `+0x8 == &ChaoSlots[n]`.
   Re-scan after area changes.
2. Each tick send, from `D = obj->Data1`:
   - position `D+0x14..0x1C` (3 f32) and rotation Y `D+0xC` (i32; X/Z at +0x8/+0x10 are
     normally 0)
   - motion: flags `u16 D+0xAC`, cur `int D+0xB0`, next `int D+0xB4`, frame `f32 D+0xC8`
   - held = `(u16 D+4) & 0x8000`. To be sure it is *this* player holding:
     `*(u32*)(CO2+0x94) == obj`.
   - while held, also send the offset from the holder:
     `off = rotateY(-P.rotY, D.pos - P.pos)` (P = `*0x1DEA6C0`, pos +0x14, rotY +0xC).
     The offset keeps the mirror glued to the puppet's hands despite puppet interpolation.
   - optional: eye `s16 D+0x692` and mouth `s16 D+0x69E`.

### Mirror (the visitor object on other clients)
1. Spawn with `CreateChao(buffer, 0, 0, &pos, ang)` as `visitors::spawn_remote` does. Then,
   the same way the game's display-only Chao are made:
   `D+0xA0 &= ~0x10` (no AI), `&= ~0x8` (no collision, so the local player can't push it or pick
   it up). Keep `0x4` (motion) set. Optionally `&= ~0x20000`, as the race does.
2. Replace `obj+0x10` with a wrapper:
   ```
   write pos/rot   // before: 0x56CCF0 builds the model matrix from them
   call 0x54FE20(obj)   // original: motion advance, face, emotion ball, physics, (no AI, no collision)
   write pos/rot again; W+0..+8 = 0    // after: undo ground physics 0x798300 / gravity
   ```
   With bit 0x10 clear, the original never changes behaviour. The garden AI cannot overwrite
   anything except through physics, which the post-write undoes.
3. Motion: `target = (owner.flags & 2) ? owner.next : owner.cur`, and
   `mine = (D+0xAC & 2) ? D+0xB4 : D+0xB0`. If `target != mine`, call
   **0x793C40(EAX = D+0xAC, EDX = target)** (one call on change, not every frame). To stop
   drift on long loops, when neither side is blending and the ids match, copy `owner.frame`
   into `D+0xC8` if it differs by more than a few frames.
4. Held:
   - While the owner reports held: `D+4 |= 0x8000` and
     `pos = puppet.pos + rotateY(puppet.rotY, off)`. Use the puppet's rendered (interpolated)
     pose, and send rotY as is.
   - Setting 0x8000 gives the same -1.0 Y draw offset and shadow as a real held Chao, and
     makes the mirror unpickable.
   - The motion (0x101/0x10x) arrives through step 3. The puppet's carry animation arrives
     through the normal pose sync (the owner's player plays its carry anim).
   - On release: `D+4 &= 0x7FFF`. With AI off this triggers no put-down or throw behaviour.
     The owner's stream shows the fall and landing (motion and position).
5. Never re-enable bit 0x10 while 0x8000 is set: the interrupt 0x566720 would put the mirror
   into the held behaviour with no holder.

## Confidence
- MainSub layout, the 0x10 AI gate and the display-only flag pattern: **high** (read directly;
  the game's own menus use the same flags).
- MotionTable layout, 0x793C40/0x793E40 call convention, shared "al_motion_table": **high**.
  Play mode semantics of +0x18 and the meaning of every flag bit: medium.
- Held = Status 0x8000, CO2+0x94 HeldObject, pickup/release writers, held behaviours, motion
  ids: **high**. Held position formula (0x725C70/0x740950) and which characters use which copy:
  medium-high. Status 0x40 as "holdable": medium (checked at 0x45A2AD, writer not found).
- Draw offset under 0x8000: medium-high (0x540CBB read; 0x53CEB0 effect not decoded).
- 0x796780 (bit 0x2) and 0x540FD0 (bit 0x80000): unknown. Leaving them on is harmless for a
  mirror because the position is re-written after MainSub.
- Save safety: high that the Chao slots are only written in place and that the pre-save walker
  skips Chao. Medium for the carry buffer `*0x1A5CB54` (not decoded), which is why the mirror
  must stay unpickable.
- None of this has been run in game yet.

## Toys and instruments in a Chao's hands (2026-10-17, static, not yet seen in game)

- Attach points: `Data1+0x514 + 4*n` points at a body node; behaviors use n = 4, 11 (hands)
  and 35. Node +0x98 model, +0x9C texture list (`AL_TOY` 0x1366AFC), +0xA0 a timer reset on a
  new model, +0xA4 shown, +0xA8, +0xAC.. parameters.
- 0x5669B0 (EAX Chao object, ECX node, EDX model): attach with the toy textures; 15 callers
  (garden 0x55Dxxx/0x55Exxx, stadium 0x59Dxxx). 0x566A20 (EDX = block {model, texlist, 5
  floats}): 7 callers; blocks 0x12F5954/0x12F5974 have a null model filled at run time.
- 0x566AF0 (cdecl node) hides a node and its children (+0xA4 = 0; child +0x2C, sibling +0x30).
- The drum is separate: `Data1+0x660` model 0x12FD9DC, +0x664 texlist, +0x668 size, +0x66C
  1.0, set by the behavior at 0x55E4F9 (motion 0x14F), drawn by the Chao display at 0x540E68
  when +0x660 is set.
- Not instruments: 0x55CB90 & co. build the garden's fixed toys (ball, horse, TV...).

The mod mirrors these onto copies (`garden::PROPS`, `GardenChao::props`/`drum`).

## Garden toys and fruit trees (2026-10-17, objdump of the dumped exe, not run in game)

Object = LoadObject 0x46F680 (push name, EDI init sub, ESI list). Init subs then set +0x10 main.
All toys register in ALW category 6 (0x530750, ECX=6, push save 0, push kind).

| Toy | Create | Name | List | Init / main / display / delete | ALW kind | Global |
|---|---|---|---|---|---|---|
| Ball | 0x55D6B0 cdecl (arg1 ?, arg2 &pos) | ALO_Ball | 2 | 0x55D640 / **0x55D310** / 0x55D3B0 / 0x57B9B0 | 4 | **0x1DBE570** = ball object |
| TV | 0x55CB90 (arg &pos) | ALO_TVExecutor | 4 | 0x55C540 is main; display 0x55C9E0; delete 0x55CB70 | 2 | 0x1AED288 = TV object |
| Radio | ~0x57CCA0 | ALO_RadicaseExecutor | 4 | main 0x57C840 | 3 | |
| Box | ~0x580890 | ALO_BoxExecutor | 4 | main 0x5801C0 | 8 | |
| Horse | 0x580F30 | ALO_Horse | ? | 0x580E80 / 0x580BC0 / 0x580CA0 / 0x57B9B0 | 7 | |
| Race toy | 0x5604F0 | ALO_RaceKariToy | ? | 0x560470 / 0x5601C0 / 0x560220 | 0 | |

Garden loader 0x54CA20.. calls the ball (0x55D6B0), TV (0x55CB90) and box (0x580890).

Ball: Data1[0] mode 0..3 (table 0x55D39C: 0 0x55CC40, **1 free/rolling 0x55CCC0**, 2 0x55D150
release (unlinks 0x530690, back to 1), 3 0x55D230 held by a Chao (0x56D170), back to 1).
Position Data1+0x14, **velocity Data2+0x00..0x08** (Data2 = obj+0x38; gravity/bounce in 0x55CCC0,
the player kick goes through 0x54B1F0). 0x1DBE574 = 1 while mode 1 and |vel| > 0x904EB0;
0x1DBE56C cleared every frame. Held by the player: CharObj2+0x94 == ball (as items).
TV mode Data1[0] 0..11 (table 0x55C9AC), radio 0..5 (0x57CA64): not decoded; they're turned on by
Chao AI, which runs separately in each game.

### Trees
GrowTree object (0x548210, cdecl (save record*), "ALO_GrowTreeExecutor", list 2, main **0x546810**,
display 0x547E70, delete 0x5481E0). Save record (8 bytes, `0x19F6480 + garden*0x38 + i*8`, 7 per
garden): +0 kind, +1 state, +2 growth, +3 life, +4..+6 fruit growth per slot.
Data1 (0x100 bytes): +0 mode (0..10, table 0x547414), +6 frame counter, +0x30 kind, +0x31 state (5
= the garden's own tree), +0x34 scale, +0x38 growth, +0x3C life (a 0x54B1F0(ECX=4, EDX=obj,
push 0xAD) hit sets it to 10000: probably watering, medium), **fruit slots +0x44, 3 × 0x20**
(+0 ripeness f32, +8 drop timer), +0xCC/+0xD0 shake latch / shaking, +0xD4 shake strength,
+0xE8 garden (1..3). Data2+0xC..0x14 = push from the player, cleared every frame.
Shaking (0x546CEB): |push| > 0x905030 → sound 0x1008 and +0xD0 = 1; each ripe slot's timer
counts down while shaking. A ripe slot also drops on its own (frame % 10 == 1 and a random roll,
0x546F42). Fruit only ripens while fewer than 40 fruit records are used (0x546F04).
Drop (0x5470C0): position 0x546670, record 0x52F9E0(ECX=3), then **0x546180(kind, &pos, ang,
NULL, record)**; kind = garden-1 (0/1/2) for state-5 trees, else tree kind+2. So a dropped fruit
is an ordinary saved fruit and already syncs as an item.

**Shared trees (2026-10-18, REA; src/trees.rs).**
- Garden load 0x52EB70 makes all 7 spots' GrowTrees, empty ones too: `0x548210(position, record)`
  with positions `0x1366B30 + (garden-1)*0x78 + slot*0xC` and records at
  `0x52E440() + 0x20 + (garden-1)*0x38 + slot*8`. Nothing else makes trees: planting changes an
  empty spot's tree.
- The constructor takes the record: +0 kind (Data1+0x30), +1 state (+0x31; 0 empty, 1 seed,
  2 growing, 3 grown, 5 the garden's own; 4 is read as 0; kind 8 means state 5), +2 growth,
  +3 life x100 (+0x3C), +4..+6 fruit ripeness (x100, or x50 for the garden's own), +7 facing
  (high nibble). It keeps the record pointer at Data1+0xEC.
- The main 0x546810 writes it back every frame (0x548660), as does the garden's save on exit
  (0x52E6B0 over registry categories 4 and 5). The delete sub 0x5481E0 only frees the model and
  the registry entry.
- Modes (Data1 byte 0): 0 empty (planting: 0x530710 reads and clears the entry's +0x34; when it's
  5, kind = entry +0x36 + 1, mode 1), 1 seed (watering: 0x54B1F0(0xAD); +100 a tick to 10000),
  2/3 sprouting, 4 growing (life +0x3C, scale +0x34), 5/6 grown up, 7 grown with fruit, 8..10
  dying, withering, back to 0.
- Fruit slots (Data1+0x44, 3 x 0x20): ripeness f32, its target f32, drop timer i32 (+8). In mode
  7, a frame with Data1+0xD0 set (shaken) counts each ripe (> 0.9) fruit's timer down; at < 1 it
  drops: position 0x546670(out, data1, slot), record 0x52F9E0(ECX=3), then
  `0x546180(kind, &pos, angle, &vel, record)`; ripeness reset to -rand*0.3, timer 11 + rand*13.
  Over 1.9 it can also drop on its own (frame % 10 == 1, 0.2%).
- The mod: the garden's lowest player id keeps the trees and sends its 7 records each second.
  Everyone else frees their own trees (save untouched) and makes copies with GrowTree from records
  it owns, MainSub wrapped: fruit kept under 1.85 with timers at 100 (never drops), an entry +0x34
  of 5 (a seed planting) and Data1+0xD0 (shaken) sent to the keeper. The keeper's game marks its
  empty spot's entry the same way, or runs that shake frame and drops the fruit as above.

## Whistling (2026-10-18, objdump; src/whistle.rs)
- Each frame a garden Chao's reaction check 0x53DB10 asks 0x5637B0 whether player 1
  (`*0x1DEA6C0`, EntityData1) has action 42 (whistling) within 100 units (squared distance
  < 10000); if so it may start behaviour 0x5634D0, which walks to player index 0 (0x796AB0).
- Only player 1 counts, so a friend's whistle never reaches our Chao. The mod: our Chao (MainSub
  0x54FE20) within 100 of a whistling friend runs a wrapper for up to 600 frames, which points
  `*0x1DEA6C0` at a stand-in copy of player 1's first 0x40 bytes, placed where the friend is
  (whistling for the first 20 frames), only while that Chao's own MainSub runs.

## Eggs (2026-10-18, objdump; src/eggs.rs)
- Eggs are made by CreateChao 0x5501D0 for type 1 (spawner 0x57B9C0), MainSub 0x57B520, registry
  category 1; Data1 +0x5C its record, +0x14 position. Mode table 0x57B618; modes 4, 6 and 8
  hatch (0x57ADD0, 0x57AFD0, 0x57B1D0 call CreateChao and the hat 0x54A540).
- The mod: eggs visit like Chao (protocol `GardenEggs`); a copy runs the egg MainSub only while
  held or settling and is put back from the hatching modes, otherwise it sits where its owner's
  egg is. Picking a copy up (CharObj2 +0x94) sends `EggHeld`; the owner's game holds the real egg
  there, so only the owner's egg can hatch.

## Toys: TV, radio, jack-in-the-box (2026-10-18, objdump; src/garden_toys.rs)
- One object each in object list 4, main at +0x10: TV 0x55C540 (made by 0x55CB90,
  "ALO_TVExecutor", also kept at 0x1AED288), radio 0x57C840 (0x1AED2E0), box 0x5801C0
  (0x1AED32C).
- Data1: +0 mode, +1 and +3 counters, +6 u16 timer, +8 picture frame (TV) or spin (radio).
  Every jump to a mode other than the next clears +1, +3 and +6.
- Touches: 0x486760(object, kind) (cdecl, read-only) returns the hit from collision kind `kind`
  (+4 who hit, 0 if none). Kind 0xAF switches an off toy on (TV/radio mode 1, box mode 1 with
  +6 = 30 less a random bit), 0xB1 switches one that's on over or off.
- TV modes 1..12 (table 0x55C9AC): odd modes start a channel (frame index +8 = 0, screen texture
  0x55EA00), the next even one plays it; after switching on it picks a channel at random
  (rand 0x7A89D8); +6 counts up to 3600 frames, then mode 3 (switching off), then 0.
- Radio modes 1..6 (table 0x57CA64): songs 4..6 picked at random, 3 off.
- Box modes 0..4 (table 0x5804F4): 1 counts +6 down, 2..4 pop open (floats +0x20/+0x24/+0x28),
  back to 0.
- The mod: the trees' keeper (lowest id) wraps the toys' mains and sends every jump (`ToyMode`);
  the others' toys jump the same unless already there or one step past. A touch in someone
  else's game goes to the keeper, which makes the jump and sends it to everyone.

## Keeper: one brain per shared garden (2026-10-18; src/keeper.rs, src/garden.rs)

### Area transitions and saving (2026-10-18)

- Carrying a Chao into another garden caused shared crashes at `0x8355C2` and `0x835022`
  (invalid native audio-object pointers). A concrete unsafe path: `carried::received`, called
  on the network thread, deleted a cached object after an area unload could free/reuse it.
  Carry reports now coalesce in a queue and are applied on the game thread. Before touching
  a cached object, the live Chao registry, record identity and copy callback must match.
  Egg hold reports use the same game-thread queue rule. Garden mirrors are pruned when their
  registry/record identity disappears, and post-update position writes check the object again.
  These guards address the invalid-memory paths; the shared-crash reproduction still needs
  an in-game check on the updated build.
- Native Chao save wrapper `0x52E630` is cdecl `(file_index) -> int`: REA confirms it calls
  item/checksum write-back `0x52F090`, then `0x532360(index, save_base)`. The latter builds the
  file buffer and completes the write; nonzero means success. Index is 1 only when selector
  `0x173D06C` is 1, else 0. `saving.rs` uses this on the game thread from F9 and after five
  minutes in gardens. Saving requires a loaded garden, game state 16/17 and a player; autosave
  waits two safe seconds after loading and retries failures no faster than every 30 seconds.

- With friends in a garden, one player's game keeps it: it runs every Chao there with its own AI
  (its own and the visitors' copies), the trees and the toys. The keeper says so with
  `GardenKeeper` (area, term) 4 times a second. Everyone follows the claim with the higher term,
  then the lower id. A newcomer follows a keeper already there. With friends and no claim for
  1 s, the lowest id claims with term = highest seen + 1 (anyone after 3 s). A paused game sends
  nothing (tabbing out pauses SA2), so its claim lapses after 1.5 s and someone else takes over;
  coming back it sees the higher term and follows.
- Keeper: visitor copies run `CHAO_MAIN` with AI on. Every 30 frames their record is diffed
  (`chao::changes`) and the changes are sent to the owner. All Chao are sent per owner
  (`GardenChao` owner field). A friend's hold of any Chao it runs wraps it in the hold MainSub.
- Everyone else: friends' copies follow the keeper's stream; their own visiting Chao do too
  (wrapped like a copy, AI off, never deleted), and their records stay their save's. The keeper's
  changes go into the save and the follower's snapshot. A touch still runs the AI locally for 5 s,
  and its changes go to the owner and the keeper. Holds go to the keeper. Petting requires
  player action 43 within reach: proximity alone used to enable a second AI indefinitely,
  causing different positions and activities (reported with drumming, 2026-10-18). Only an
  item this Chao actually holds/eats (registry partner and held/eaten state) extends a touch;
  fruit lying nearby no longer prevents it from following the keeper again.
- Handoffs keep an existing follower frozen through the election's `Wait`, instead of
  restarting its own AI. A newly responsible game adopts the last received position/facing
  and clears old velocity before running the Chao. A gap over 30 units snaps to the shared
  position; ordinary movement is still smoothed.
- Held-item crash, 2026-10-18 21:45:21: `items::place` called `following_copy` for a Chao
  whose Data1 was already null during a transition (GameState 5). Dump: DLL RVA `0x7A77E`,
  null read at `0xC`; inline symbols lead through `in_hands` to `items::place`/`on_frame`.
  `following_pose` now verifies that the registry still names the object, that it isn't queued
  for deletion and that Data1 exists before returning position/facing. Outgoing held-item
  reports use the same `live_pose` check. Regression test covers cleared Data1 and deletion.
- Nobody keeping yet (friends just met): each owner's game runs its own Chao, as before.
- Breeding (checked 2026-10-18): the mating behaviour makes the egg from a free slot in the local
  save (0x531AA0), genes mixed from both parents (0x551D20), then the egg spawner 0x57B9C0 (call
  at 0x59C2D7). So a baby made in a shared garden is the keeper's.
- End of life: when a Chao's lifespan (record +0x8A) is 0, 0x562A20 starts 0x568F60, leading to
  death or reincarnation: 0x568570 makes an egg from the Chao's own record (+0x8E reincarnations).
  The keeper keeps its visitor copies' lifespan at 1 or more without sending that, so this only
  happens in the owner's game.
- Hats (section 5 of re/chao-garden-items.md, items 1 and 2): closed for visitors, see the next
  section.

## What a copy shows vs. its brain's Chao (2026-10-18, REA/Ghidra + objdump; src/garden.rs)

An audit of everything the Chao's update and draw read, against what `GardenChao` and the look
sync carry. A copy runs the full MainSub 0x54FE20 with the AI bit (Data1+0xA0 & 0x10) off, so
everything except 0x53DC40 (reactions 0x53DB10, behaviours, think timers 0x562C00) still runs.

Shown the same on a copy (nothing to send):
- Emotion ball and the icon over it (0x53CB70 → 0x53CA40, handler table 0x1353834: 6 handlers
  0x53C180..0x53C790 that read only the mood block Data1+0x6EC..0x76C; icon id at +0x726, drawn by
  the delayed display 0x53CEB0 from table 0x1366ACC, 10 = none). The mood state, timer and
  length are sent, so the rest follows.
- Face: eyes/mouth are sent; 0x53A3E0 counts their timers and blinks (0x53A1E0, +0x6C0..0x6E8)
  locally.
- Shape and colour (0x56CCF0 → 0x5A49C0 when +0x674 bit 2): from the record (look sync).
- Draw 0x540B60: body from +0x510, +0x650 model (written only in the stadium, 0x59E196), drum
  +0x660 (sent). Held: -1.0 Y from Status 0x8000 (set by `put`).
- Physics 0x53DF00 / 0x796780: overwritten by `put`. Shadow list 0x540FD0: local.
- Param window ("ParamFukidasi", 0x5659E0): the local player's UI, not the Chao's.

Life tick, runs on every Chao including copies: 0x5363D0 (garden master) takes one registry
Chao every few frames (`0x1312DE2`) and calls 0x5353C0 (ESI = object, only with Data1+0xA0 &
0x20000): emotions (+0x12D..+0x156), lifespan, and for a child evolution progress (0x536110) and
in the Hero/Dark garden (0x134046C = 2/3) alignment ± (0x536260). It touches only the record.
Since it also runs on the owner's real Chao, what it does to a copy must not be forwarded, or
the owner's Chao drifts twice (alignment was: the keeper's 30-frame diff sent it). The mod
wraps the call (0x53640F) and adds a copy's tick changes to its snapshot.

Evolution: only the behaviour chooser starts it: 0x562A80 → 0x562800: child (record +0x80 = 2)
with evolution progress (+0xC0) > 1.0 (0x1312C90) → cocoon behaviour 0x568D30. A copy whose AI
runs (the keeper's, or a touched one) is kept at 0.99 (also lifespan ≥ 1), so a Chao evolves,
dies or is reborn only in its owner's game. When it's due, its owner's game runs it (below).

Hats, fixed for visitors (records outside our save's 24 Chao slots in either save copy):
- Dropping (hit 0x563E84, thrown 0x56440A): 0x54A540 (cdecl kind, &pos = Data1+0x14, angle,
  &velocity, record in our save's hat list B+0x4D8) makes the hat object (ALO_ObakeHeadExecutor)
  and the record; the caller then clears +0xD5. Hooked: a visitor's isn't made and it keeps it
  on (the clear is skipped, below).
- Putting one on: the reactions check calls 0x5691B0 (ESI = object) at 0x53DB87: touch kind
  0x93 → behaviour 0x569240 (frees our hat record). Hooked: visitors don't.

Behaviour effects that aren't the Chao object (each behaviour runs only in the brain's game):
- Book (BookExecutor, 0x55E180, EBX = Chao): reading behaviour 0x55E210 (motions 0x151..0x153).
  Pointer at Data1+0x38 (the cocoon is at +0x34); MainSub 0x55DF90, display 0x55E050, Data1[0]
  mode: 0 opening (scale +0x24 to 1), 1 open, 2/3 page turn (+0x20), 4 closing, then it deletes
  itself without clearing the Chao's pointer. Sent (`GardenChao::book`); the copy makes its own,
  turns its page and closes it.
- Bubbles (ALO_ShabonExecutor, 0x55DA10: EBX = &pos, push &velocity): the bubble behaviour
  0x55DB10 attaches the wand (0x12FE33C node 4, 0x12FE574 node 11; nothing else uses them), plays
  0x146 and from frame 60, every 10 frames with chance 0.4, blows one at the hand node
  (Data1+0x5E4, direction +0x614). A copy showing the wand does the same.
- Items in a Chao's hands (0x56CFF0 keeps the partner item at the hand nodes +0x5D8/+0x5E4 by
  Data1+0xA0 bits 0x400/0x800): the item's owner reports it as held by that Chao. A keeper's item
  held by a visitor it runs was reported as "busy" at a fixed spot; `ItemPlace::Chao` now names
  the Chao's owner too.
- Sounds: synced, see the next section.

## Chao sounds and life events during a visit (2026-10-18; src/garden.rs)

Sounds. Every Chao behaviour and reaction runs inside the AI tick 0x53DC40, which the MainSub
calls once, at 0x54FE49 (EAX = object); the mod wraps that call and notes the Chao while it runs.
Two calls play a sound at a place: 0x4372E0 and 0x437590, both EDI = sound id, ESI = &position
(the 3 floats are copied), 3 cdecl arguments the caller pops (0x4372E0: a value kept with the
sound, volume byte, then a third; 0x437590 also takes EBX = the sound's holder, keeping one
sound per (holder, id), and its third argument is a length). The Chao code has 82 and 19 calls
with ESI at an object's position (Data1+0x14), redirected through the mod (`SOUND_AT_CALLS`,
`SOUND_HELD_CALLS`). The character voice hook (voice.rs) patches 0x4372E0's start, so sites are
redirected rather than the function. Sounds made while a Chao our game runs thinks are sent
(`ChaoSounds`) with the offset from the Chao; copies following the sender play them with the same
call at their own place (EBX = the copy). 0x437260 (no position) is only called from menus,
karate and the garden masters, not from Chao.

Jingles: 0x443480 (EBX = .adx name) plays music ("JingleTask"). The Chao behaviours call it for
hatching/evolving (0x56880C, 0x56887E, 0x568E6D, 0x568E89, 0x568EA5: chao_g_born_h2/_d2/_c/_h/_d),
dying (0x5690C6, chao_g_dead), dancing (0x59C10F, chao_g_dance) and 0x565066 (chao_k_net_fine).
These go the same way, as an index into `garden::JINGLES`. The TV and radio play theirs from
their own mains (0x55C5F3.., 0x57C9BE), kept in step by the toy sync.

Hats: the clears after a drop (0x563E92 `mov byte [edx+0xD5],0`, 0x564418 the same with EAX)
are now a call to the mod, which skips a visitor's; no frame without the hat.

Life events of our own Chao while a friend keeps the garden. A copy's AI never starts them
(`hold_life`), and our Chao follows the keeper with its AI off, so nothing would. Instead, our
game watches its own: a child past 1.0 evolution, a life at 0, or one in the evolving (0x568D30)
or end-of-life (0x568F60) behaviour or with a cocoon (Data1+0x34). While that holds, and 10 s
after, it stops following the keeper: its AI runs here, so the game's own chooser starts the
event and plays it out (cocoon, evolution or death/rebirth, all in our save, as at home). It's
sent with `GardenChao::owner_runs`; everyone, the keeper too, then follows our game for that
Chao (the keeper stops running and sending its copy, its holds come to us), and our game sends
its sounds and jingles. After an evolution its look changed, so it's sent again
(`Body::Chao`) and the copies are made anew. A death ends its visit (its row goes); a rebirth
leaves an egg, which visits as eggs do.

## Patch sites checked, and what's left (2026-10-18, third pass)

All 116 patched spots were checked in `re/bin/sonic2app.dumped.exe` (sha256 d1ac8ed9…, the file
REA analyses; spot reads through REA at 0x54FE49, 0x563E92, 0x5A2D78 match): every redirected
call is `E8 rel32` to the function named (82 × 0x4372E0, 19 × 0x437590, 8 × 0x443480, 2 ×
0x54A540, 0x5691B0 at 0x53DB87, 0x5353C0 at 0x53640F, 0x53DC40 at 0x54FE49), and the hat clears
are `C6 82 D5 00 00 00 00` (0x563E92) and `C6 80 D5 00 00 00 00` (0x564418). The mod checks the
same before patching each one and leaves out (and logs) any that differs.

Sound call arguments, read in the callees (both `push ecx; push ebx/ebp` first):
- 0x4372E0: `bl = [esp+0x14]` = 3rd argument, the volume byte (slot +5/+6); `[esp+0xC]` = 1st,
  kept at slot +0x10 (a holder key, where 0x437590 keeps EBX); the 2nd goes to the slot search
  0x4370D0. Callers pass 0 or a register for the 1st; the mod sends only whether there was one,
  and the copy is the holder.
- 0x437590: `dl = [esp+0x10]` = 2nd, volume (+5/+6); `bp = [esp+0x14]` = 3rd, length (+0xC);
  EBX at +0x10, EDI at +0x14, so one sound per (holder, id). Callers pass 1, volume, length.

A copy made anew (its look changed: evolved, took an animal) used to appear where new visitors
do; it now takes the old copy's place (where its brain has it), facing and motion.

Items a Chao here takes from a friend: the claim goes to the referee; if it's refused while
the Chao still has it (eating a fruit, putting a hat on: 0x569240 links it with 0x56CFB0, and
0x569430 frees its record only when it has one), the copy is deleted so nothing that isn't ours
is used up; the owner's next report makes it again.

Checked and needing nothing new:
- Carrying a visitor out of the garden (kindergarten, lobby, any exit): visitors.rs clears the
  carried Chao (0x1A5CA5C) for visitor buffers, eggs too. Our own Chao, following the keeper or
  not, is our save's and goes as usual.
- Race and karate entrances: no garden Chao there; races use the sent Chao data.
- Name tags: friends' copies (not our own following ones).
- Hats: visitors neither drop nor put on hats here; our own Chao taking a friend's hat item goes
  through the item claim (above).

## The other Chao World areas (2026-10-18)

ChaoArea (0x134046C): 0 lobby, 4 race, 5 entrance (race/karate), 6 kindergarten, 9 stadium,
10 karate; the gardens are 1..=3. Players in the same area see each other's puppets (online.rs
draws everyone in the same level and, in Chao World, the same area).

- Kindergarten (classroom, doctor, black market, name machine, fortune teller), the lobby's
  machines and the stadium/entrance menus are each player's own menus on their own save: the
  classroom takes your Chao for a lesson in your save, the doctor reads your Chao, the black
  market spends your rings. Nothing there is world state another player sees, so nothing to
  share beyond the puppets.
- Visible gap, fixed: a Chao carried out of the garden (the game makes it again in the next
  area from 0x1A5CA5C) showed as empty hands on the carrier's puppet. `carried.rs` sends it
  (`CarriedChao`) and the others show a display-only copy in the puppet's hands (CreateChao, as
  the game does for a carried Chao in these areas; menu flags: no AI, no collision).
- Not shown: a Chao the carrier puts down outside the garden (rare; it goes back to their
  garden when they leave the area).
- Races and karate: re/chao-race-sync.md 4 and re/chao-karate-sync.md 6.


## Crash at 0x77FB53 when a kept visitor went home (2026-10-18, minidump + REA)

The live test crashed in the same second the keeper removed a visitor it was running with its AI
on. Minidump: EIP 0x77FB53 in 0x77FB00 (world point to screen: ESI = &point, EBX = out),
`fld [esi+8]` with ESI = 0x14, called from 0x564DB2. That caller is the status balloon over a
Chao (ParamFukidasi, made by 0x5659E0, one at a time in 0x1AED28C): its data (object +0x40) keeps
the Chao object at +0x38, and in its states 4 and 6 it projects that Chao's position
(`[[data+0x38]+0x34]+0x14`, 0x564DA5). Behaviours make it when the player deals with a Chao
(0x560771, 0x565B62, 0x566ED2), so a copy whose AI runs (the keeper's) can have one; the copy
was deleted under it and its Data1 read as 0. The game's own Chao never vanish under a balloon.
Fix (`garden::delete_chao`): before deleting a Chao object, a balloon over it is closed the way
its creator closes an old one (state 7, the global cleared); then the object goes through the
normal delete (its delete sub unlinks it from the registry and its partner, 0x5307F0). Used for
every copy the garden deletes (gone home, made anew) and carried.rs's copies.

Visitors also no longer go home just because their owner's updates stopped while the owner is
still in our garden by their last state (their game idles on a tab-out): `garden::set_alive`
(players whose state came in the last 2 s) and `PAUSED_LEFT` (5 minutes); a disconnect still
sends them home at once. Egg copies stay put the same way.
