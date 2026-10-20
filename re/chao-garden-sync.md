# Chao Garden: per-frame update, motion, being held, and how a shared garden is synced

Steam `sonic2app.exe`, image base 0x400000. Addresses come from the objdump listing of
`re/bin/sonic2app.dumped.exe` (most of this code has no Ghidra functions). See `chao.md` and
`chao-race-sync.md`: garden, race and menu Chao are the same object type and share the Data1
layout.

0x56CCF0 is not the skeletal motion player. It updates the model/shape block at Data1+0x510 and
builds the model matrix (0x56FC20). The motion (animation) controller is the struct at
**Data1+0xAC**, advanced by **0x7938D0** (section 2).

Notation: `obj` = ObjectMaster*, `D` = `obj->Data1` (+0x34, `OBJ_DATA1` in the mod),
`W` = `*(obj+0x38)` (physics work: velocity at +0/+4/+8).

---

## 1. Garden Chao object and its per-frame update

### What CreateChao (0x5501D0) installs
| ObjectMaster field | Value | Notes |
|---|---|---|
| +0x10 MainSub | **0x54FE20** | `LoadObject(list 2 or 4, "…", EDI=0x54FE20)` at 0x550263..0x55026D |
| +0x14 DisplaySub | 0x54FF80 | Picks the draw by record byte `+0xDD` (jump table 0x55018C); normal Chao: **0x540B60** |
| +0x18 DeleteSub | 0x54FF30 | Frees `D+0x5C` **only if `D+0x58 != 0`** (copy mode). With `copy=0` (what visitors.rs passes) the mod's buffer is never freed by the game |
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
    0x53A3E0(EAX=obj)                             // face timers: eyes D+0x68C/0x692, mouth timer D+0x698
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
Menu code does this right after `CreateChao` (0x5560CD, also 0x556695, 0x556D16, 0x55721D,
0x579504, 0x58745D, 0x590E17, ...):
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
  `+0x3C != 0 && +0x8 == &ChaoSlots[n]` (see "Save copies" below for the slot address).

---

## 2. Motion (animation) controller

### `MotionTable` at **D+0xAC** (0x4C bytes)
| off | type | meaning |
|---|---|---|
| +0x00 | u16 | flags (see "MotionTable timing" below). **bit 0x2 = blending into the next motion**. Bit 0x1 = end/loop pulse. Bit 0x8 = held at the end (play mode 3 only) |
| +0x02 | u16 | posture, from table entry +6 (`D+0xAE`) |
| **+0x04** | int | **current motion id** |
| **+0x08** | int | **next id**: blend target while bit 0x2 is set, otherwise the motion queued after the current one (table entry +8, -1 = none) |
| +0x0C | f32 | speed multiplier, reset to 1.0 every frame |
| +0x10 | f32 | blend step (1/frames) |
| +0x14 | f32 | blend progress 0..1 |
| +0x18 | u16 | play mode of the current motion (0..7, jump table 0x793C1C) |
| **+0x1C** | f32 | **current frame** |
| +0x20 / +0x24 | f32 | start / end frame |
| +0x28 | f32 | frame step |
| +0x2C | ptr | current NJS_MOTION* |
| +0x30..+0x44 | | the same fields for the next motion (mode, start, end, step, motion*) |
| +0x48 | ptr | motion table: entries of 0x1C bytes `{motion*, u16 mode, u16 posture, int next, int blendFrames, f32 start, f32 end, f32 step}` |

- The table comes from the Chao DLL export **"al_motion_table"** (string 0x1304824). 0x55C370 does
  `GetProcAddress(*(*0x1AF0220), …)` via the import at 0x8730D4, then `0x793880(EAX=D+0xAC, push table)`.
  Every Chao uses the same table, so **motion ids are global**: an id read on the owner's game
  plays the same animation on any other Chao.
- `D+0xF8` is a second MotionTable used only for records with `+0xDD == 3`.

### Functions (all usercall, no return value)
| Addr | Call | Effect |
|---|---|---|
| **0x793C40** | **EAX = D+0xAC, EDX = id**, no stack args; clobbers ECX/EDX | **`SetMotionLink`**: blend into `id`. Behaviours use this (e.g. `0x793C40(D+0xAC, 0x101)` = held) |
| 0x793E40 | EAX = mt, EDX = id | Hard switch to `id` without a blend |
| 0x793D30 | EAX = mt, EDX = id, push u16 frames (caller pops 4) | Blend with an explicit length (used by 0x565BE0) |
| 0x793D90 | EAX = mt, EDX = id | Blend that keeps the phase |
| 0x7938D0 | ESI = mt | Per-frame advance (MainSub calls it when `D+0xA0 & 4`) |
| 0x793880 | EAX = mt, push table | Init, starts motion 0 |
| 0x793F70 | EAX = mt | Returns flags & 1 |
| 0x793F80 | EAX = mt | Returns `(flags >> 3) & 1` |

Motion ids seen in code: 0x101 held (calm); 0x102/0x103/0x105/0x107 held happy; 0x109..0x10E
held struggling (dislikes holder); 0x3E/0xC/6/0x1B race idle; 0, 1, 3, 0xEE, 0xEF after being
put down (0x565BE0).

### Face
| Addr | Call | Effect |
|---|---|---|
| 0x53A4B0 | ECX = obj, EDX = eye id, EAX = duration (-1 = until changed) | eyes. Writes `D+0x68C` (timer) and `D+0x692` (eye id) |
| 0x53A5A0 | ECX = obj, EDX = duration, push mouth id (caller pops 4) | mouth. Writes only `D+0x698` (timer) and passes the id to `0x55EAF0`. `D+0x69E` is the default mouth, not the current one |

The default eye is at `D+0x694`, the default mouth at `D+0x69E`. 0x53A3E0 counts the timers down.

### MotionTable timing (0x7938D0)

The server's copy is `src/chao_sim/motion.rs`.

- **Table**: Data_DLL export `al_motion_table` (in the install it is `Data_DLL_orig.dll`, renamed by
  the mod loader; `*(*0x1AF0220)` is that module's handle, loaded by 0x77DD80). **622 entries**
  (0..0x26D) of 0x1C bytes, no count in the game; entry 0x26E onward is other data (mode > 7).
  Entry: `+0 NJS_MOTION*`, `+4 u16 play mode`, **`+6 u16 posture`** (copied to MT+2 = `D+0xAE`),
  `+8 int next`, `+0xC int blend`, `+0x10/+0x14 f32 start/end`, `+0x18 f32 step`. The player
  never reads the NJS_MOTION frame count (`motion+4`, nbFrame); every entry has start/end set.
  Blend is ≤ 0 in all but two entries (-40 in 570).
- **Posture** (`D+0xAE`): 0 standing, 1 sitting (0x2F sit down → 1, 0x30 stand up → 0), 2 lying
  (0x25 → 2, 0x26 → 0), 3 on the belly (0x71..0x74 crawl, 0x9F trip), 4/5 others (0x8A, 0xA0,
  0xEE/0xEF; four entries 5). It changes only when a motion actually starts (0x793E40, 0x793F10),
  **not** when a link blend begins.
- **Flags `+0`**: 1 end pulse, 2 blending, 4 both motions advance during the blend (only
  0x793D90 between two mode-0 motions), **8 = held at the end (only play mode 3 sets it, with
  1)**, 0x10 = mode-2 wrap blend. 0x7938D0 clears 1 and 8 first (`and 0xFFF6`), so pulses last
  until the next advance (the AI runs before the advance in MainSub, so it sees the previous
  frame's pulses). `+0xC` speed is reset to 1.0 at the end of every advance; init 0x793880 leaves
  it 0.0, then `0x793E40(0)`.
- **Set functions** (all write flags, clearing pulses): 0x793E40 set (frame = start, flags 0,
  next = entry.next, posture); 0x793EA0 same keeping the overshoot (`frame + start − old end`);
  0x793C40 link: next fields from the entry, `+8 = id`, blend step = 1/blend, or **1/15
  (`0x904DCC`) when blend < 0**, flags 2; 0x793D30 the same with step = 1/(pushed u16);
  0x793D90 link at the same phase `(frame−start)/(end−start)` of the new range, flags 6 if both
  modes are 0; 0x793CB0 (mode-4 chain) step = 1/(−cur.blend) if cur.blend < 0, else
  1/next.blend; 0x793F10 ends a blend (next fields become current, flags 0).
  0x793F70 = `flags & 1`, 0x793F80 = `(flags >> 3) & 1`.
- **Blend** (flags & 2): progress += |step|·speed. Without flag 4 the current frame is frozen;
  at progress ≥ 1: with 0x10, frame = start ± (p−1) and flags &= ~0x12; else
  `0x793E40(+8)`. With flag 4, both frames advance and wrap by their range; at ≥ 1, 0x793F10.
  A finished blend makes no end pulse.
- **Play** (no blend): frame += step·speed. Reached when frame ≥ end (step ≥ 0) or ≤ end
  (step < 0); mode 1 uses end ± 1. Then by mode (jump table 0x793C1C):
  0 loop `frame −= end−start`, pulse 1; 1 loop `frame −= end+1−start` (end−1 backwards), pulse 1;
  2 pulse 1 and a wrap blend (flags 0x13, step |step|, progress = overshoot, frame frozen at the
  end) back to start; 3 hold frame = end, pulse 1|8 **every frame from then on**; 4 frame = end,
  chain to `next` (0x793CB0) if ≠ −1, pulse 1 (no next: pulses every frame); 5 `0x793E40(next)`
  if ≠ −1, pulse 1 (no next: frame runs on, pulse every frame); 6 the same with 0x793EA0;
  7 ping-pong: frame = end, swap start/end, step = −step, pulse 1.
- Examples: walk 0x64 (mode 1, 0..11, 0.23) pulses every 53 frames; a 0x793C40 link takes 15
  frames; sit 0x2F (mode 3, 0..12, 0.17) holds and pulses 1|8 from about frame 71.

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
- Status 0x40 marks a holdable object (checked at 0x45A2AD; its writer is not identified).

---

## 4. Saving and visitor records

- Own Chao objects point straight into the save slots (`D+0x5C` = &ChaoSlots[n], `D+0x58=0`).
  The game updates the record in place, and saving writes the whole region
  `0x19F6460 + file*0xCA6C` (0xCA6C bytes; save/load 0x52E570/0x52E630 → 0x532570/0x532360).
- The pre-save sync 0x52F090 walks only world-entity categories 2, 3, 7 and 9 (0x52E710: copies
  object positions into their records) and calls 0x52E6B0 for categories 4 and 5. **Category 0
  (Chao) is never walked.** No code copies a live Chao's record into a free slot (the only
  0x800-byte copies are the stadium select at 0x5536A0/0x553B57 into its own buffers).
- A visitor created with `copy=0` keeps the mod's buffer as `D+0x5C`. The game writes only into
  that buffer: garden byte, stats, emotion. The delete sub does not free it (`D+0x58 == 0`), and
  0x531B10 (spawn from slots) never sees it. `copy=1` gains nothing: the game would own a heap
  copy that the delete sub frees, and the record still would not reach the slots.
- The 4-entry carry/transfer buffer at `*0x1A5CB54` (4 × 0x1084, a 0x800 record copy at
  +0x9C, source pointer at +0x88) is checked by 0x531CAC while spawning garden Chao; it appears
  to be the transporter / "take a Chao along" system and is not decoded. A visitor carried
  through an exit could end up there, so visitors.rs clears the carried Chao (0x1A5CA5C) for
  visitor buffers and eggs.

### Saving from the mod (saving.rs)
- `0x52E630(file_index)` (pre-save `0x52F090`, then `0x532360(index, save_base)`) depends on
  per-slot file state and fails when called outside an ordinary garden exit. The mod's save
  (F9 and autosave) instead runs `0x52F090` on the game thread for native item write-back and
  checksums, then reads `0xCA6C` bytes from the active buffer returned by `0x52E440`. The getter
  normalizes selector `0x173D06C` to copy 1 only when it equals 1; base is
  `0x19F6460 + copy*0xCA6C`.
- `0x532360`'s Steam file layout is a `0x40` header + `0x3000` icon + payload, padded to
  `0x10000`. `saving.rs` keeps the existing header and padding, replaces only the payload, syncs
  a temporary file in the same directory and atomically replaces the Chao save. Unexpected file or
  snapshot sizes are rejected without changing the save.
- Saving requires a loaded garden, game state 16/17 and a player; autosave waits two safe seconds
  after loading and retries failures no faster than every 30 seconds.

### Save copies
Every read and write of the save's Chao records goes through `game::chao_slot_address`:
`0x19F6EC4 + (0x173D06C == 1) * 0xCA6C + slot * 0x800`, the copy the garden load 0x531B10
spawns from, GetSaveData 0x52E440 saves and 0x568630 writes. `bring.rs`'s `slot_of` and
`garden::in_save` accept either copy (they only test whether a record pointer is a save slot).

---

## 5. Mirroring a Chao from the network

### Owner (reporting its real Chao)
1. Find the object: scan the cat 0 world entities (section 1) for `+0x8 == &ChaoSlots[n]`.
   Re-scan after area changes.
2. Each tick send, from `D = obj->Data1`:
   - position `D+0x14..0x1C` (3 f32) and rotation Y `D+0xC` (i32; X/Z at +0x8/+0x10 are
     normally 0)
   - motion: flags `u16 D+0xAC`, cur `int D+0xB0`, next `int D+0xB4`, frame `f32 D+0xC8`
   - held = `(u16 D+4) & 0x8000`. This player is the holder when `*(u32*)(CO2+0x94) == obj`.
   - while held, the offset from the holder:
     `off = rotateY(-P.rotY, D.pos - P.pos)` (P = `*0x1DEA6C0`, pos +0x14, rotY +0xC).
     The offset keeps the mirror in the puppet's hands despite puppet interpolation.
   - eye `s16 D+0x692` (the displayed mouth goes through `0x55EAF0`; `D+0x69E` is the default).

### Mirror (the visitor object on other clients)
1. Spawn with `CreateChao(buffer, 0, 0, &pos, ang)` (`visitors::spawn_remote`), then, as the
   game does for display-only Chao: `D+0xA0 &= ~0x10` (no AI), `&= ~0x8` (no collision, so the
   local player can't push it or pick it up). `0x4` (motion) stays set.
2. `obj+0x10` is replaced with a wrapper:
   ```
   write pos/rot   // before: 0x56CCF0 builds the model matrix from them
   call 0x54FE20(obj)   // original: motion advance, face, emotion ball, physics, (no AI, no collision)
   write pos/rot again; W+0..+8 = 0    // after: undo ground physics 0x798300 / gravity
   ```
   With bit 0x10 clear, the original never changes behaviour; physics is undone by the post-write.
   0x796780 (bit 0x2) and 0x540FD0 (bit 0x80000) are unknown and harmless here for the same reason.
3. Motion: `target = (owner.flags & 2) ? owner.next : owner.cur`, and
   `mine = (D+0xAC & 2) ? D+0xB4 : D+0xB0`. If `target != mine`, call
   **0x793C40(EAX = D+0xAC, EDX = target)** (once on change, not every frame). When neither side
   is blending and the ids match, `owner.frame` is copied into `D+0xC8` if it differs by more
   than a few frames.
4. Held:
   - While the owner reports held: `D+4 |= 0x8000` and
     `pos = puppet.pos + rotateY(puppet.rotY, off)`, using the puppet's rendered (interpolated) pose.
   - 0x8000 gives the same -1.0 Y draw offset and shadow as a real held Chao, and makes the
     mirror unpickable (0x45A260 rejects held objects).
   - The motion (0x101/0x10x) arrives through step 3; the puppet's carry animation arrives
     through the normal pose sync.
   - On release: `D+4 &= 0x7FFF`. With AI off this triggers no put-down or throw behaviour.
     The owner's stream shows the fall and landing.
5. Bit 0x10 must never be set while 0x8000 is set: the interrupt 0x566720 would put the mirror
   into the held behaviour with no holder.

### What a copy shows vs. its brain's Chao
A copy runs the full MainSub 0x54FE20 with the AI bit (Data1+0xA0 & 0x10) off, so everything
except 0x53DC40 (reactions 0x53DB10, behaviours, think timers 0x562C00) still runs.

Shown the same on a copy without being sent:
- Emotion ball and the icon over it (0x53CB70 → 0x53CA40, handler table 0x1353834: 6 handlers
  0x53C180..0x53C790 that read only the mood block Data1+0x6EC..0x76C; icon id at +0x726, drawn by
  the delayed display 0x53CEB0 from table 0x1366ACC, 10 = none). The mood state, elapsed time and
  length are sent, so the rest follows.
- Face: eyes/mouth are sent; 0x53A3E0 counts their timers and blinks (0x53A1E0, +0x6C0..0x6E8)
  locally.
- Shape and colour (0x56CCF0 → 0x5A49C0 when +0x674 bit 2): from the record (look sync).
- Draw 0x540B60: body from +0x510, +0x650 model (written only in the stadium, 0x59E196), drum
  +0x660 (sent). Held: -1.0 Y from Status 0x8000.
- Physics 0x53DF00 / 0x796780: overwritten by the mirror. Shadow list 0x540FD0: local.
- Param window ("ParamFukidasi", 0x5659E0): the local player's UI, not the Chao's.

Life tick, runs on every Chao including copies: 0x5363D0 (garden master) takes one registry
Chao every few frames (`0x1312DE2`) and calls 0x5353C0 (ESI = object, only with Data1+0xA0 &
0x20000): emotions (+0x12D..+0x156), lifespan, and for a child evolution progress (0x536110) and
in the Hero/Dark garden (0x134046C = 2/3) alignment ± (0x536260). It touches only the record.
It also runs on the owner's real Chao, so a copy's tick changes must not be forwarded (the
owner's Chao would drift twice). The mod wraps the call (0x53640F) and adds a copy's tick changes
to its snapshot instead.

Evolution: only the behaviour chooser starts it: 0x562A80 → 0x562800: child (record +0x80 = 2)
with evolution progress (+0xC0) >= 1.0 (0x1312C90) → cocoon behaviour 0x568D30. A copy whose AI
runs (a touched one) is kept at 0.99 (also lifespan ≥ 1), so a Chao evolves, dies or is reborn
only in its owner's game.

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
  Data1+0xA0 bits 0x400/0x800): the item's owner reports it as held by that Chao
  (`ItemPlace::Chao`, which names the Chao's owner).
- Tears: a copy's AI is off, so effects its behaviours spawn never appear. `garden::shed_tears`
  makes them client-side: while a copy that follows its brain plays a bawling motion (0x210,
  0x213, 0x215, 0x20B, 0x20D) or starving (0x1DC), on 4 frames of every 8 by its own frame count,
  after its update. Both behaviours make the same call twice (bawl `0x59FEB9..0x59FF9B`, starving
  `0x59F0B4..0x59F196`): `0x6ED270` (push scale `[0xB5D144]` = 0.1f, push &position, EDI =
  &velocity, caller pops; it takes a particle from pool `0xB0ACC8`, colour 0xFFA090FF, nothing
  of the Chao's) with position = point − (0, 0.2, 0) and velocity = 0.1·direction + (0, 0.1, 0),
  for the two body points {position, direction} at Data1+0x620 and +0x638 (`0x510+0x110`,
  `+0x128`). The game gates on the behaviour's timer (bawl `D+0x1C0`; starving its own counter).

A copy made anew (its look changed: evolved, took an animal) takes the old copy's place, facing
and motion.

### Hats on visitors
Visitor records are outside the save's 24 Chao slots in either save copy, so hat handling is
hooked:
- Dropping (hit 0x563E84, thrown 0x56440A): 0x54A540 (cdecl kind, &pos = Data1+0x14, angle,
  &velocity, record in the save's hat list B+0x4D8) makes the hat object (ALO_ObakeHeadExecutor)
  and the record; the caller then clears +0xD5 (0x563E92 `mov byte [edx+0xD5],0`, 0x564418 the
  same with EAX). Both the drop call and the clear go through the mod: a visitor's hat isn't
  made and the visitor keeps it on.
- Putting one on: the reactions check calls 0x5691B0 (ESI = object) at 0x53DB87: touch kind
  0x93 → behaviour 0x569240 (frees the hat record). Hooked: visitors don't.

### Sounds and jingles
Every Chao behaviour and reaction runs inside the AI tick 0x53DC40, which the MainSub calls once,
at 0x54FE49 (EAX = object); the mod wraps that call and notes the Chao while it runs. Two calls
play a sound at a place: 0x4372E0 and 0x437590, both EDI = sound id, ESI = &position (the 3
floats are copied), 3 cdecl arguments the caller pops:
- 0x4372E0: `bl = [esp+0x14]` = 3rd argument, the volume byte (slot +5/+6); `[esp+0xC]` = 1st,
  kept at slot +0x10 (a holder key, where 0x437590 keeps EBX); the 2nd goes to the slot search
  0x4370D0. Callers pass 0 or a register for the 1st; the mod sends only whether there was one,
  and the copy is the holder.
- 0x437590: also EBX = the sound's holder; `dl = [esp+0x10]` = 2nd, volume (+5/+6);
  `bp = [esp+0x14]` = 3rd, length (+0xC); EBX at +0x10, EDI at +0x14, so one sound per
  (holder, id). Callers pass 1, volume, length.

The Chao code has 82 and 19 calls with ESI at an object's position (Data1+0x14), redirected
through the mod (`SOUND_AT_CALLS`, `SOUND_HELD_CALLS`). The character voice hook (voice.rs)
patches 0x4372E0's start, so call sites are redirected rather than the function. Sounds made
while a Chao this game runs thinks are sent (`ChaoSounds`) with the offset from the Chao; copies
following the sender play them with the same call at their own place (EBX = the copy). 0x437260
(no position) is only called from menus, karate and the garden masters, not from Chao.

Jingles: 0x443480 (EBX = .adx name) plays music ("JingleTask"). The Chao behaviours call it for
hatching/evolving (0x56880C, 0x56887E, 0x568E6D, 0x568E89, 0x568EA5: chao_g_born_h2/_d2/_c/_h/_d),
dying (0x5690C6, chao_g_dead), dancing (0x59C10F, chao_g_dance) and 0x565066 (chao_k_net_fine).
These are sent the same way, as an index into `garden::JINGLES`. The TV and radio play theirs
from their own mains (0x55C5F3.., 0x57C9BE), kept in step by the toy sync.

### Patch sites
All 116 patched spots match `re/bin/sonic2app.dumped.exe` (sha256 d1ac8ed9…): every redirected
call is `E8 rel32` to the function named (82 × 0x4372E0, 19 × 0x437590, 8 × 0x443480, 2 ×
0x54A540, 0x5691B0 at 0x53DB87, 0x5353C0 at 0x53640F, 0x53DC40 at 0x54FE49), and the hat clears
are `C6 82 D5 00 00 00 00` (0x563E92) and `C6 80 D5 00 00 00 00` (0x564418). The mod checks the
same bytes before patching each one and leaves out (and logs) any that differs.

### Deleting a copy safely
- The status balloon over a Chao (ParamFukidasi, made by 0x5659E0, one at a time in 0x1AED28C)
  keeps the Chao object at data (object +0x40) +0x38, and in its states 4 and 6 projects that
  Chao's position (`[[data+0x38]+0x34]+0x14`, 0x564DA5, through 0x77FB00). Behaviours make it
  when the player deals with a Chao (0x560771, 0x565B62, 0x566ED2). Deleting a Chao under a
  balloon crashes at 0x77FB53 (`fld [esi+8]`, ESI = 0x14). `garden::delete_chao` first closes
  a balloon over it the way its creator closes an old one (state 7, the global cleared), then
  deletes the object normally (its delete sub unlinks it from the registry and its partner,
  0x5307F0). Every copy the garden or carried.rs deletes goes through it.
- Network reports that touch game objects (carry reports, egg holds) are queued and applied on
  the game thread; before touching a cached object, the live Chao registry, record identity and
  copy callback must still match, since an area unload can free or reuse it. Garden mirrors are
  pruned when their registry/record identity disappears, and position writes after the update
  check the object again. `following_pose`/`live_pose` check that the registry still names the
  object, that it isn't queued for deletion and that Data1 exists (Data1 is null during
  transitions, GameState 5).

---

## 6. Garden objects

### Toys and instruments in a Chao's hands
- Attach points: `Data1+0x514 + 4*n` points at a body node; behaviours use n = 4, 11 (hands)
  and 35. Node +0x98 model, +0x9C texture list (`AL_TOY` 0x1366AFC), +0xA0 a timer reset on a
  new model, +0xA4 shown, +0xA8, +0xAC.. parameters.
- 0x5669B0 (EAX Chao object, ECX node, EDX model): attach with the toy textures; 15 callers
  (garden 0x55Dxxx/0x55Exxx, stadium 0x59Dxxx). 0x566A20 (EDX = block {model, texlist, 5
  floats}): 7 callers; blocks 0x12F5954/0x12F5974 have a null model filled at run time.
- 0x566AF0 (cdecl node) hides a node and its children (+0xA4 = 0; child +0x2C, sibling +0x30).
- The drum is separate: `Data1+0x660` model 0x12FD9DC, +0x664 texlist, +0x668 size, +0x66C
  1.0, set by the behaviour at 0x55E4F9 (motion 0x14F), drawn by the Chao display at 0x540E68
  when +0x660 is set.
- 0x55CB90 & co. build the garden's fixed toys (ball, horse, TV...), not instruments.

The mod mirrors these onto copies (`garden::PROPS`, `GardenChao::props`/`drum`).

### Garden toys
Object = LoadObject 0x46F680 (push name, EDI init sub, ESI list). Init subs then set +0x10 main.
All toys register in ALW category 6 (0x530750, ECX=6, push save 0, push kind).

| Toy | Create | Name | List | Init / main / display / delete | ALW kind | Global |
|---|---|---|---|---|---|---|
| Ball | 0x55D6B0 cdecl (arg1 ?, arg2 &pos) | ALO_Ball | 2 | 0x55D640 / **0x55D310** / 0x55D3B0 / 0x57B9B0 | 4 | **0x1DBE570** = ball object |
| TV | 0x55CB90 (arg &pos) | ALO_TVExecutor | 4 | 0x55C540 is main; display 0x55C9E0; delete 0x55CB70 | 2 | 0x1AED288 = TV object |
| Radio | ~0x57CCA0 | ALO_RadicaseExecutor | 4 | main 0x57C840 | 3 | 0x1AED2E0 |
| Box | ~0x580890 | ALO_BoxExecutor | 4 | main 0x5801C0 | 8 | 0x1AED32C |
| Horse | 0x580F30 | ALO_Horse | ? | 0x580E80 / 0x580BC0 / 0x580CA0 / 0x57B9B0 | 7 | |
| Race toy | 0x5604F0 | ALO_RaceKariToy | ? | 0x560470 / 0x5601C0 / 0x560220 | 0 | |

Garden loader 0x54CA20.. calls the ball (0x55D6B0), TV (0x55CB90) and box (0x580890). The mod
finds the TV, radio, box and horse by their MainSub (`garden_toys::placed`); ALW category 6 does
not list them. A garden without bought toys has none.

Ball: Data1[0] mode 0..3 (table 0x55D39C: 0 0x55CC40, **1 free/rolling 0x55CCC0**, 2 0x55D150
release (unlinks 0x530690, back to 1), 3 0x55D230 held by a Chao (0x56D170), back to 1).
Position Data1+0x14, **velocity Data2+0x00..0x08** (Data2 = obj+0x38; gravity/bounce in 0x55CCC0,
the player kick goes through 0x54B1F0). 0x1DBE574 = 1 while mode 1 and |vel| > 0x904EB0;
0x1DBE56C cleared every frame. Held by the player: CharObj2+0x94 == ball (as items).

TV, radio, box (src/garden_toys.rs):
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
- The mod: the garden's keeper (section 7) wraps the toys' mains and sends every jump
  (`ToyMode`); the others' toys jump the same unless already there or one step past. A touch in
  another game goes to the keeper, which makes the jump and sends it to everyone.

### Trees (src/trees.rs)
GrowTree object (0x548210, cdecl (position, save record*), "ALO_GrowTreeExecutor", list 2, main
**0x546810**, display 0x547E70, delete 0x5481E0).
- Garden load 0x52EB70 makes all 7 spots' GrowTrees, empty ones too: `0x548210(position, record)`
  with positions `0x1366B30 + (garden-1)*0x78 + slot*0xC` and records at
  `0x52E440() + 0x20 + (garden-1)*0x38 + slot*8` (8 bytes, 7 per garden). Nothing else makes
  trees: planting changes an empty spot's tree.
- Record: +0 kind (Data1+0x30), +1 state (+0x31; 0 empty, 1 seed, 2 growing, 3 grown, 5 the
  garden's own; 4 is read as 0; kind 8 means state 5), +2 growth, +3 life x100 (+0x3C), +4..+6
  fruit ripeness (x100, or x50 for the garden's own), +7 facing (high nibble). The constructor
  keeps the record pointer at Data1+0xEC.
- The main 0x546810 writes it back every frame (0x548660), as does the garden's save on exit
  (0x52E6B0 over registry categories 4 and 5). The delete sub 0x5481E0 only frees the model and
  the registry entry.
- Data1 (0x100 bytes): +0 mode, +6 frame counter, +0x30 kind, +0x31 state, +0x34 scale, +0x38
  growth, +0x3C life, **fruit slots +0x44, 3 × 0x20**, +0xCC/+0xD0 shake latch / shaking, +0xD4
  shake strength, +0xE8 garden (1..3). Data2+0xC..0x14 = push from the player, cleared every frame.
- Modes (Data1 byte 0, table 0x547414): 0 empty (planting: 0x530710 reads and clears the entry's
  +0x34; when it's 5, kind = entry +0x36 + 1, mode 1), 1 seed (watering: 0x54B1F0(ECX=4, EDX=obj,
  push 0xAD); +100 a tick to 10000), 2/3 sprouting, 4 growing (life +0x3C, scale +0x34), 5/6
  grown up, 7 grown with fruit, 8..10 dying, withering, back to 0.
- Fruit slots: ripeness f32, its target f32, drop timer i32 (+8). Shaking (0x546CEB): |push| >
  0x905030 → sound 0x1008 and +0xD0 = 1. In mode 7, a frame with +0xD0 set counts each ripe
  (> 0.9) fruit's timer down; at < 1 it drops (0x5470C0): position 0x546670(out, data1, slot),
  record 0x52F9E0(ECX=3), then `0x546180(kind, &pos, angle, &vel, record)` with kind = garden-1
  (0/1/2) for state-5 trees, else tree kind+2; ripeness reset to -rand*0.3, timer 11 + rand*13.
  Over 1.9 a fruit can also drop on its own (frame % 10 == 1, 0.2%, 0x546F42). Fruit only ripens
  while fewer than 40 fruit records are used (0x546F04). A dropped fruit is an ordinary saved
  fruit and syncs as an item.
- The mod: the garden's keeper keeps the trees and sends its 7 records each second. Everyone else
  frees their own trees (save untouched) and makes copies with GrowTree from records it owns,
  MainSub wrapped: fruit kept under 1.85 with timers at 100 (never drops), an entry +0x34 of 5
  (a seed planting) and Data1+0xD0 (shaken) sent to the keeper. The keeper's game marks its
  empty spot's entry the same way, or runs that shake frame and drops the fruit as above.

### Whistling (src/whistle.rs)
- Each frame a garden Chao's reaction check 0x53DB10 asks 0x5637B0 whether player 1
  (`*0x1DEA6C0`, EntityData1) has action 42 (whistling) within 100 units (squared distance
  < 10000); if so (and bond like > 50) it may start behaviour 0x5634D0, which turns toward the player
  (0x796AB0) and then installs 0x563250, which walks to the player.
- Only player 1 counts, so another player's whistle never reaches the local Chao. The mod: a
  local Chao (MainSub 0x54FE20) within 100 of a whistling player runs a wrapper for up to 600
  frames, which points `*0x1DEA6C0` at a stand-in copy of player 1's first 0x40 bytes, placed
  where that player is (whistling for the first 20 frames), only while that Chao's own MainSub runs.

### Eggs (src/eggs.rs)
- Eggs are made by CreateChao 0x5501D0 for type 1 (spawner 0x57B9C0), MainSub 0x57B520, registry
  category 1; Data1 +0x5C its record, +0x14 position. Mode table 0x57B618; modes 4, 6 and 8
  hatch (0x57ADD0, 0x57AFD0, 0x57B1D0 call CreateChao and the hat 0x54A540).
- The mod: eggs visit like Chao (protocol `GardenEggs`); a copy runs the egg MainSub only while
  held or settling and is put back from the hatching modes, otherwise it sits where its owner's
  egg is. Picking a copy up (CharObj2 +0x94) sends `EggHeld`; the owner's game holds the real egg
  there, so only the owner's egg can hatch.

### Breeding and end of life
- The mating behaviour makes the egg from a free slot in the local save (0x531AA0), genes mixed
  from both parents (0x551D20), then the egg spawner 0x57B9C0 (call at 0x59C2D7).
- When a Chao's lifespan (record +0x8A) is 0, 0x562A20 starts 0x568F60, leading to death or
  reincarnation: 0x568570 makes an egg from the Chao's own record (+0x8E reincarnations).

---

## 7. Who runs what in a shared garden

### Keeper (src/keeper.rs)
One player's game keeps a shared garden's trees and toys (and picks the co-op boss leader). The
keeper says so with `GardenKeeper` (area, term) 4 times a second. Everyone follows the claim with
the higher term, then the lower id. A newcomer follows a keeper already there. With other players
and no claim for 1 s, the lowest id claims with term = highest seen + 1 (anyone after 3 s). A
paused game sends nothing (tabbing out pauses SA2), so its claim lapses after 1.5 s and someone
else takes over; coming back it sees the higher term and follows. The keeper does not run Chao.

### The server runs the Chao (protocol 21; src/net/chao_host.rs)
The host (dedicated server or a player hosting) runs a shared garden's visiting Chao itself
(`chao_sim`, re/chao-ai.md) once two players are in it and their games have sent its ground
(`GardenLand` garden 0..2: LandTable, cell grid, C_CLIMB ledges, 0xCC blockers, toys from ALW
category 6, the 7 tree spots `0x1366B30`; garden 3: the motion table). The ground is sent again
on every (re)connect. Everyone follows `ServerChao` (sender `garden::SERVER`, 0xFF).

- **Sounds.** The brain's voice ids go out as `ChaoSounds` from 0xFF, unreliably, as the
  game's `0x4372E0` call with arguments (0, 0, 0x6E) (the behaviours' usual push, e.g.
  `0x569340`: `push 0x6E; push 0; push 0; mov edi, id`). Clients play them on copies that follow
  the server.
- **Record changes.** Once a second, what the AI changed in a Chao's record
  (`chao::changes::diff`, including the worn hat `rec+0xD5`) goes to its owner as
  `ChaoChanges` from 0xFF. Not the life tick's changes (the owner's game ticks too;
  `World::ticked`), and not while a player holds or pets it or sends changes for it (3 s):
  their game runs it then and sends its own. A newer `Body::Chao` from the owner keeps what the
  server changed and hasn't sent yet. Evolving and dying stay the owner's game's (`hold_life`).
- **Items.** The Chao sense the referee's items: on the ground, in a player's hands (their
  offset), or in a server Chao's hands. A fruit, seed, animal or hat one takes becomes the
  server's (`ItemOwner` to 0xFF; its old owner's game drops it and keeps a copy), shown in that
  Chao's hands (`ItemPlace::Chao`, offset (0, 2, 1.5)) through `ServerItems`, sent every 0.4 s
  while one is held. Bites change its bites; the last removes it. Let go or thrown, it lands on
  the floor under the Chao's hands (items lie **on** the floor in the games: y = floor), and the
  server settles its own lying items every second; one saved while in a Chao's hands is saved
  where it was taken. A hat put on goes; one knocked off is a new server item. Claims for an
  item a server Chao holds are refused.
- **Toys.** A press on the TV, radio or box (`ToyEvent::Press`) switches it on when the
  referee's last mode for it is 0: `ToyMode` mode 1 from 0xFF, which only the toys' keeper
  carries out (and then tells everyone). A playing TV or radio gives the Chao its 0xC9 / 0xCA
  signal (radius 25.5) from the toy's position. The horse is each game's own.
- **Trees.** Spots from the garden bytes, states from the referee's tree records (byte 1).
  Planting and watering stay the trees' keeper's.
- **Holds.** `ServerChao` never says `held`: every game hears a hold from its holder
  (`ChaoHeld`). A Chao in the local player's own hands never follows the stream
  (`garden::place`). The emotion ball goes as (state, elapsed, length).
- **Timing.** Time the local game stood still (tab-out with "pause when inactive") doesn't count
  as the server going quiet (`PAUSE_GAP`). The server keeps a garden running 15 s with too few
  players (`RUN_GRACE`), sends to players heard within 8 s, and pushes its Chao 5 units out of
  players' way (their collision would shove the player).

Before the server runs a garden (no ground yet, or one player there), each owner's game runs its
own Chao and every other game's copy follows the owner's stream.

### Life events of a player's own Chao
A copy's AI never starts a life event (`hold_life`), and a player's own Chao following the server
has its AI off. So the owner's game watches its own Chao: a child past 1.0 evolution, a life at
0, or one in the evolving (0x568D30) or end-of-life (0x568F60) behaviour or with a cocoon
(Data1+0x34). While that holds, and 10 s after, it stops following and its AI runs locally, so
the game's own chooser starts the event and plays it out (cocoon, evolution or death/rebirth, in
the owner's save). This is sent with `GardenChao::owner_runs`; everyone then follows the owner's
game for that Chao, and the owner's game sends its sounds and jingles. After an evolution its
look changed, so it is sent again (`Body::Chao`) and the copies are made anew. A death ends its
visit; a rebirth leaves an egg, which visits as eggs do.

### Visitor presence
- Owner `GardenChao` reports are cached per remote player even while the local player is in
  another area, so entering a garden seeds visitors at their reported position without waiting
  for a paused owner. Active snapshots expire after 8 s; paused snapshots after 300 s. An area
  change or an empty owner roster invalidates cached presence. Empty owner lists are sent even
  with all visiting rows disabled.
- Visitors don't go home while their owner is still in the garden by their last state but their
  updates stopped (their game idles on a tab-out): `garden::set_alive` (players whose state came
  in the last 2 s) and `PAUSED_LEFT` (5 minutes). A disconnect sends them home at once. Egg
  copies stay put the same way.
- An item a local Chao takes from another player is claimed from the referee; if the claim is
  refused while the Chao still has it (eating a fruit, putting a hat on: 0x569240 links it with
  0x56CFB0, and 0x569430 frees its record only when it has one), the copy is deleted so nothing
  that isn't local is used up; the owner's next report makes it again.

### Bring 4, hide the rest (src/bring.rs)
In a shared garden (another player's state says they're in it, or the server runs it) each game
shows only the Chao its player brings (F9 Bring Chao 1..4, `online::BRING_CHAO` = 4 =
`MAX_CARRIED_CHAO`; Auto rows take the first ones of that garden, My Chao first). Online, a
player always brings (a row on None brings nobody); an old config's `VisitWith` is ignored. The
player's other Chao and eggs there have no object while it's shared; their records are never
written.
- **Load.** The garden constructor's spawn loop 0x531B10 walks the active copy's 24 slots
  (`0x19F6EC4 + (0x173D06C == 1) * 0xCA6C`, step 0x800): record +0x81 == area, not the Chao
  `*0x1A5CA5C` nor 0x532D00()'s, not in the transfer buffer `*0x1A5CB54`; type +0x80 1 (egg) or
  hatched with +0x84 == 0xFF. It calls CreateChao 0x5501D0(record, 0, 0, spawn point, angle)
  at **0x531D91** (Chao; result tested at 0x531D99) and **0x531DFF** (eggs; result ignored).
  Both calls are redirected: a left-home record gets 0. The loop's own overcrowding move
  (`+0x81 = other garden` for one random slot) is untouched. Spawn points: `0x1366260 +
  ((garden - 1) * 16 + n) * 12`, n in 0..16.
- **Mid-visit.** Deleted like every Chao on leaving: main = DeleteObject 0x46F720 (after closing
  a status balloon over it, `garden::delete_chao`). Chao delete sub 0x54FF30: 0x56CE70 (frees
  Data1 work +0x5B4/+0x5B8/+0x5BC/+0x670 and the model) and 0x5307F0 (registry); frees the
  record only when `Data1+0x58 != 0` (a heap copy), never for a save slot (`+0x58 == 0`, set
  by CreateChao with copy=0). Egg delete sub 0x57B9B0: 0x5307F0 only. Skipped while held
  (player's CharObj2+0x94, Chao status 0x8000), run by another MainSub (whistle), or an egg in
  mode >= 4 (hatching).
- **Coming back.** CreateChao(record, 0, 0, spawn point, angle) as the load does, only for
  records left out this visit that still have type != 0, +0x81 == area and no registry
  object (Chao or egg category).
- **Saving.** The pre-save 0x52F090 calls 0x52E6B0 (trees: categories 4/5 via 0x548660),
  0x52E710 for item categories 2, 3, 7, 9, 0x52EDE0 (garden globals into `0x19F6E50 + 0x1C..0x72`,
  below the first slot at 0x19F6EC4), a 0x250-byte copy into `0x19F6BE0` when area == 4, and
  0x52EEE0 (checksum bytes at +0xCA64..). None touches the Chao slots, and Chao records are
  updated in place by their own object only, so a Chao with no object is saved exactly as it is.

### Asleep and away Chao (protocol 22; net/roster.rs, net/chao_host.rs, held.rs)
The Chao a player brings (each garden's visitor rows, `Brought`) stay in their garden when the
player isn't there. The host keeps them by the player's key (config.ini `PlayerKey`, sent in
`Hello`); a Chao is (key, `ChaoData::identity`, copy number by slot order). The identity hashes
the DNA block `rec+0x438` (0xA4 bytes) without what a life changes in it: `DNA[0..2]` (hatching,
`0x551DA0`), the grade alleles `+0x5C..0x6C` (evolution, `0x551780`) and the personality alleles
`+0x74..0x8E` (rebirth, `0x551560`).
- **Login.** Every connect, garden change and change of what's brought sends the whole list from
  the save; the host's copies follow it (records, gardens, dropped, added). Eggs aren't held.
- **Owner online elsewhere (awake).** Run in the server's world as any Chao, but its life tick's
  changes and an evolution are sent too: absolute bytes inside `SAVE_CHAO_FIELDS` (`SaveChao`),
  once a second. The owner's game writes them into the slot of the save in use
  (`game::chao_slot_address`) wherever it is (the slots are loaded from the first Chao World
  visit on; before that the client says `awake = 0` and its Chao sleep), then `saving::changed`.
  The cocoon (`0x568D30` mode 2 → 3) runs `0x568630` on the server (`chao_sim::evolve`);
  `GardenChao::cocoon` = 1 while it's closed (modes 2..=3), so the copies show it (`0x568CD0`).
  An evolution into a hero or dark adult opens that garden in the owner's save (flags
  `0x19F646C` |= 0x10 / 0x40, as 0x568630 does). Life is held at 1: a Chao whose life ends waits
  for its owner's game.
- **Owner offline (asleep).** Not in the server's world: shown lying in the sleeping behaviour's
  second pose (`0x54EF10` mode 2: motion 0x13, eyes 7) where it was left; carried (ChaoHeld,
  motion 0x101) and thrown (ChaoTouched) by others, and kept where it lands. On the copies only
  a hold or a hit (`D+4 & 4`) runs its AI; nothing they do to it is sent.
- **Owner comes back.** When their stream for that row arrives (or 5 s after they came in), the
  held copy goes and their game's runs it as before. Their game is never told of its own held
  Chao, so it never makes a copy of a Chao it has in its save.
- **Room.** A garden's ALW Chao category holds 32 (`0x8AB838`): present players' own first, then
  held ones, online owners first, then by last seen, 4 per player (their brought rows). The
  server's world can hold 64 (slot n ticks with n % 32, still every 96 frames). Held ids are
  owner 8..=15 × index 0..=3 (index = a player's row, so 4 per id), 32 in all over the three
  gardens; past that, more wait. One garden shows at most 32 Chao anyway, and the owner nibble
  of `ItemPlace::Chao` has no room for more ids. Each game makes at most 28 visitor copies
  (`visitors::MAX_VISITORS` = 32 − `BRING_CHAO`; its own 4 besides).
- **Start position.** A held Chao starts where it was last seen (its owner's stream, or the
  server's world), or at a random land cell (`0x534F80` value 1) when it never was in that garden.

## Locked Chao (protocol 22; net/chao_host.rs, online.rs, garden.rs)

F9 "Others can change my Chao" (`config.ini` `OthersChangeChao`: Always, ChaoWorld (also read
from `AnyGarden`), ThatGarden, Never; `ChaoLock`) goes to the host in `Brought` (again whenever
it changes) and is kept with the player's held Chao (their roster file's `lock` line), so it
holds while they're offline. Locked = `ChaoLock::locks(owner's place, Chao's garden)`.

**Where the owner is** (`lock_place`, `lock_area`): level 90 (`LEVEL_CHAO_WORLD`) is all of Chao
World; ChaoArea 0x134046C: 0 lobby, 1..3 gardens, 4 race, 5 race/karate entrance,
6 Kindergarten (the Black Market is inside it), 9 stadium, 10 karate. A garden gives 1..3, any
other Chao World area `PLACE_CHAO_WORLD` (0x10), anything else 0. A state flagged
`POSE_LOADING` (`game::between_areas`: GameMode not 5/10/0xE, or GameState not 16/17, or no
player object) keeps the last place for 10 s (`AREA_GRACE`) or until a settled state.
Disconnecting is 0 at once.

**Same behaviour, separate save.** A locked Chao's senses (`Players::senses`) pick a player as
for any Chao (holder, nearest whistler within 100, nearest); it is held (`Chao::held`, held
behaviours 0x566670..), thrown (`0x564320` upset) and petted as usual. `Chao` has `saved:
Option<Record>`:
- `with_whom` (table `WITH_A_PLAYER`) says whose the current activity is: the holder for held,
  held moods and set down; the thrower for the landing; the frame's focus (`Players::focus`)
  for whistle, walk to player, petted, knocked, flee, bond tick, stay near, pester, dislike.
- When that player is locked against, `keep_save` sets `saved` to the record before the frame
  plus that frame's life tick. While it lasts the brain's `rec` takes every reaction delta;
  `saved` gets only `life_tick` (same random state) and the owner's own `ChaoChanges`,
  `Body::Chao` and roster updates (`change` applies to both).
- When the activity is no longer with such a player, `rec` = `saved` and `saved` is cleared.
- Everything written back reads `kept()` (`saved` else `rec`): `ChaoChanges` from 0xFF,
  `ChaoLive`, `SaveChao`, `HeldChao`, the tick bookkeeping. The worn hat follows `rec` (a hat
  knocked off is an item on the ground). A held Chao evolving while `saved` is set evolves
  from `saved`.
- Resetting fields every frame instead doesn't work: behaviours read their own emotion changes
  (petting's wind-down waits for `0x132` < 10).

Also for a locked Chao the host drops other players' `ChaoChanges` (its world doesn't apply them
and the referee doesn't relay them) and the mood of their `ChaoTouched` (the position still
counts), doesn't pause its own changes while they touch it (`quiet`), and lets it take only its
owner's or the server's items (`ChaoSenses::may_take`). The owner's game refuses `ChaoChanges`
for its Chao from another player while its own lock says no. Each player is told the Chao
locked against them (`ChaoLocked`); `HeldChao::locked` is re-checked every `HELD_EVERY` frames
(`relock`). Name tags draw a padlock after the name (`hud::name_tag`, 9 rects).

On the other player's game (garden.rs, `Keep`): a copy locked against the local player, or a
held one asleep:
- **Items, at the source.** The reactions dispatcher `0x53DB10` (ESI = Chao, every reacting
  frame of its AI) calls the item checks after the hat one: `0x567560` at 0x53DB95 (a seed it
  touches → carry it, 0x567640), `0x560640` at 0x53DBA3 (fruit seen or held out, collision 0x83:
  → walk to it 0x5606D0, then eat 0x5607C0), `0x566BC0` at 0x53DBB1 (animal or drive → 0x566C40,
  absorb 0x566DF0). Each takes only ESI and returns EAX 1 when it reacts. The calls go through
  thunks (`item_check_thunk!`) that return 0 for a copy in `KEEP_OFF`. Hunger's own "go and eat"
  (`0x5614D0`, picked from the needs at 0x59ECD2, not the dispatcher) can still link a fruit:
- **Let go.** `0x530690` (EAX = object; its entry `+0x3C`) unlinks an object from its partner
  both ways (`+6` flags, `+0x40`, `+0xC` = -1), as the dispatcher does at 0x53DC1E when a
  reaction takes over. Every frame a kept copy whose entry's partner is an item (category
  2/3/7/9) is unlinked. A fruit bites only on its Chao's command 2 in its mailbox `+0x34` after
  0xB5 frames of its eaten mode `0x545C20`, and that mode drops back to mode 2 (lying) as soon
  as `0x56D170` finds no Chao holding it, so nothing is eaten. Fruit held out to it is only
  handed over (mode 3, 0x545B90) to a linked Chao. The eat behaviour 0x5607C0 ends at once
  with nothing in its hands. items.rs doesn't claim an item such a copy touched.
- **Touches.** Petting, holding and hitting still run its AI (the reaction shows); an item held
  near it doesn't (`touches`), nor does its being busy with one keep it running.
- **Record.** Nothing goes out (no `ChaoChanges`), and each frame `undo_drift` puts back
  `SAVE_CHAO_FIELDS` from the copy's snapshot (its brain's saved record, which `ChaoLive` keeps
  up to date), so its balloon (and the game's five stats) show the saved values.

**Before the server runs a garden** (no ground yet, or one player there) each owner's game runs
its own Chao, and a copy only sends `ChaoChanges` while it's touched (`mirrors_frame`). A locked
Chao's natural life goes into its owner's save directly; only touches are refused. Held Chao
exist only in a run.

Other paths: eggs are only carried (a copy never hatches; the owner's game moves the real one),
visitors keep their hats (drop and wear hooks), a visitor can't be carried out of the garden (no
naming, races or doctor), races and karate use read-only copies, and the server has no player
attacks (`hit_by`).

---

## 8. The other Chao World areas

ChaoArea (0x134046C): 0 lobby, 4 race, 5 entrance (race/karate), 6 kindergarten, 9 stadium,
10 karate; the gardens are 1..=3. Players in the same area see each other's puppets (online.rs
draws everyone in the same level and, in Chao World, the same area).

- Kindergarten (classroom, doctor, black market, name machine, fortune teller), the lobby's
  machines and the stadium/entrance menus are each player's own menus on their own save: the
  classroom takes the player's Chao for a lesson in their save, the doctor reads their Chao, the
  black market spends their rings. None of it is world state another player sees.
- A Chao carried out of the garden (the game makes it again in the next area from 0x1A5CA5C) is
  sent by `carried.rs` (`CarriedChao`); the others show a display-only copy in the puppet's hands
  (CreateChao, as the game does for a carried Chao in these areas; menu flags: no AI, no
  collision). A Chao the carrier puts down outside the garden is not shown.
- Visitors can't be carried out (visitors.rs clears 0x1A5CA5C for visitor buffers and eggs).
  Race and karate entrances have no garden Chao; races use the sent Chao data. Name tags show on
  other players' copies only.
- Races and karate: re/chao-race-sync.md section 4 and re/chao-karate-sync.md section 6.
