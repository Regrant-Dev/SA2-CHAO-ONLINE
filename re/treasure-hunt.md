# Treasure hunt: emerald pieces, selection, collection, radar/hints

Steam `sonic2app.exe`, image base 0x400000. Sources: the disassembly of
`re/bin/sonic2app.dumped.exe` and the game's own SET files in `resource/gd_PC`.

In a room hunt, up to 8 players each run their own 1P game of the same Knuckles/Rouge hunting
stage and mission. Everyone hunts the same 3 pieces, a host-side referee decides who was first
to each piece, a collected piece disappears for everyone, and everyone sees the scores.

Short version:
- The candidate spots are the stage's **EMERALD SET objects**. The exe has no table of spots.
- At load each EMERALD object registers itself in a per-kind candidate list.
- The manager **"SearchEmeraldsGameManager"** then picks 3 of them with the CRT `rand()`.
- The 3 chosen pieces live in a small struct at `*(0x1AF014C)`. Writing that struct (or hooking
  the one selection call) forces a set.

---

## 1. The manager (SearchEmeraldsGameManager)

| Addr | What |
|---|---|
| **0x1AF014C** | `u8*` **manager data** (0x74 bytes, also `ObjectMaster+0x40`). 0 when there is no hunt |
| 0x73AA70 | create the manager (LoadObject list 1, main 0x739570, name 0x900CEC). Called from 4 sites in the Knuckles/Rouge character load (0x728141, 0x728491, 0x7288B7, 0x728B64). Skipped if CurrentLevel > 0x3C or is 0x21/0x25/0x26 (KvR boss, Cannon's Core R/K), **MissionNum 1 or 2 (missions 2 and 3)**, or the manager already exists |
| 0x739570 | manager MainSub: the state machine below |
| 0x73A6F0 | manager +0x28 sub: **radar/HUD draw and beeps** |
| 0x73A970 | DeleteSub: frees the lists and sets 0x1AF014C = 0 |

### Manager data (`m = *(u8**)0x1AF014C`)
| Off | Type | Meaning |
|---|---|---|
| +0x00 | u8 | **state**, see below |
| +0x01 | u8 | full-reset request (set by the restart 0x43C7C0, and by a death in Mission 4) |
| +0x02 | u8 | partial-reset request (set by the death restart 0x43C8B0, except in Mission 4) |
| +0x03 | i8 | hints read for the current target (0..3). Feeds the hint text choice and the bonus |
| +0x04 | u8 | "it's close" voice already played (bit0 P1, bit1 P2) |
| **+0x05** | u8 | **pieces still to find**. The hunt ends when it reaches 0 |
| +0x06..+0x09 | i8 | candidate counts: C list, B list, A list, enemy list |
| +0x0A/+0x0B | u8 | per-player radar bytes (P1/P2). They stay 0 during a 1P hunt; 0x73A6F0 works the radar colour out from the distance each frame |
| +0x0C..+0x2B | | debug: duplicate candidates found during selection |
| **+0x2C, +0x3C, +0x4C** | `Slot[3]` | **the 3 chosen pieces** (piece 1, 2, 3) |
| +0x5C / +0x60 / +0x64 / +0x68 | `Slot*` | candidate lists **C** (48 slots), **B** (48), **A** (48), **E** in-enemy (128) |
| +0x6C | u32 | frames since the hunt started or the last piece was collected (time bonus) |
| +0x70 | ptr | HUD texlist (KGAMETEX / KGAMETEXDZ / KGAMETEXSH) |

`Slot` (0x10 bytes): `+0 u8 kind` (0xFF = none, **0xFE = collected**), `+1 u8 id` (after
collection: the collecting player 0/1), `+2 u16` (unused), `+4 float pos[3]`.

A piece is identified by **(kind, id)**. Both come from the SET entry's rotation X:
`kind = ((rotX >> 8) & 0xFF) % 11`, `id = rotX & 0xFF`. They are unique per stage, except
(3,6) in Mad Space 1P and (8,4) in Wild Canyon 2P.

### States (+0, jump table 0x739FA4)
| State | Code | What |
|---|---|---|
| 0 | 0x7395D0 | allocate the lists, set all slots to 0xFF, then state 1 |
| 1 | 0x739668 | wait for the SET loader's first pass (`*(u32*)0x1A5A33C == 1`), then state 2 |
| 2/3 | 0x73967D | **select** (see 2). Mission 5 (MissionNum 4): `+5 = 3` with no selection. Then state 4 |
| 4 | 0x739A65 | hunting: distance to the first slot that isn't 0xFE, beeps (sound 0x1009), near voice 0x506/0x507. **When `+5 == 0`, state 5** |
| 5 | 0x739E6C | `call 0x43E6D0` (stage clear, ESI = winner, 0 in 1P), then state 6 |
| 6 | 0x739ECD | full reset (all slots 0xFF, counts 0), back to state 1. **Everything is chosen again** |
| 7 | 0x739F1B | partial reset (slots that aren't 0xFE go back to 0xFF), back to state 1. Uncollected pieces are **chosen again** |

The manager runs on object list 1 and EMERALD objects on list 2. So in the selection frame the
manager runs first. Vanilla relies on this order (see 3).

---

## 2. Candidate spots and the selection

### Candidates = EMERALD SET objects
- EMERALD (and EMERALD_F) list entry: main **0x6D00C0**, list 2, range flags 4 ("load on the
  first pass regardless of range"). So **every** candidate is loaded during the first loader pass.
- 0x6D00C0 dispatches by kind through the table **0xB43428** (11 entries of 0x24 bytes):
  `{kind, name, init, ?, ?, main, display, delete, ?}`.

| Kind | Name | Init / Main | List |
|---|---|---|---|
| 0 | B NORMAL | 0x6D09B0 / 0x6D0AD0 | B |
| 1 | C NORMAL | 0x6D09B0 / 0x6D0AD0 | C |
| 2 | B HIDDEN | same | B |
| 3 | C HIDDEN | same | C |
| 4 | A UNDERGND | same (must dig: player action 0x3E/0x42, checked by 0x6CFF40) | A |
| 5 | B UNDERGND | same (dig) | B |
| 6 | A 2P UNDGND | 0x6D11B0 / 0x6D12C0 | A, **2P only** |
| 7 | A PATHMOVE | 0x6D1FD0 / 0x6D2310 (moving piece; its slot position is updated) | A |
| 8 | A 1P TeCH | same as 0 | A, **1P only** |
| 9 | ? FINAL | display 0x6D2950 | **Mission 5 only**: it writes slot[id] directly |
| 10 | ? INENEMY | 0x6D27F0 / 0x6D2880 | E. Also registered by enemies (0x478A05, enemy work +0x45 = id) |

- Kinds 0..5 and 8 share MainSub 0x6D0AD0; the moving kind 7 has its own MainSub 0x6D2310 with
  the same selection sub-states (section 3). A scan for chosen pieces must match both.
- Enemy registration **0x4789D0** takes the id from the enemy's Data1 rotation Y low byte (into
  work +0x45) and appends its position to the E list while the manager is in states 1/2/3. Ids
  are fixed by the SET data, not per-machine counters, but enemies (ghosts, moving enemies) are
  not in the same physical state on every machine.
- The candidate lists C/B/A (+0x5C/+0x60/+0x64, counts +6/+7/+8, 16-byte entries, capacity 48
  each) stay intact after selection; the picker only removes entries from the E list.

- **Register**: init 0x6D09B0 calls **0x73AFE0** (cdecl `(kind, id)`, EDI = `&pos`), which
  appends to a list (jump table 0x73B2D8).
  - In **Pumpkin Hill (5) and Egg Quarters (26) 1P**, kinds 7/8 are only added if
    `MainCharObj2[0]+0x24 & 0x2000400` (upgrade bits; probably Sunglasses / Treasure Scope).
    So **candidate lists can differ between players' saves**.
- The EMERALD object-list index (SET entry `ID & 0x7FFF`) is 0x0F in every hunt stage, except
  0x0E for Aquatic Mine (7) and Pool Quest (49). The lists are found through each stage's
  LoadSetObjects call.

Candidate counts parsed from the SET files (1P files, `_s` + `_u`):

| Level | C | B | A (1P-valid) | FINAL (M5) | Notes |
|---|---|---|---|---|---|
| 5 Pumpkin Hill | 31 | 32 | 16 + 1 + 8 (kinds 7/8 need the upgrade) | 2 | 8 kind-6 (2P only) |
| 7 Aquatic Mine | 27 | 26 | 22 | 2 | |
| 8 Security Hall | 32 | 32 | 32 | 3 | |
| 16 Wild Canyon | 32 | 31 | 25 | 2 | |
| 18 Dry Lagoon | 32 | 31 | 27 | 3 | |
| 25 Death Chamber | 32 | 32 | 32 | 3 | |
| 26 Egg Quarters | 32 | 32 | 31 (15 of them kinds 7/8, need the upgrade) | 3 | |
| 32 Meteor Herd | 32 | 29 | 30 | 3 | 8 kind-6 (2P only) |
| 44 Mad Space | 32 | 29 | 21 | 4 | |
| 59 Death Chamber 2P | 31 | 23 | 14 | 0 | same files in 1P and 2P |

### RNG
- `rand()` = MSVC CRT **0x7A89D8**: per-thread `holdrand = holdrand*0x343FD + 0x269EC3`,
  returns `(holdrand >> 16) & 0x7FFF`, with state at `_getptd()+0x14`.
- `srand` = 0x7A89C6. It is only seeded with 0xDEAD0CAB for demos (0x434EEE, 0x4395E7,
  0x439A97). In normal play it is never reseeded per stage.
- 2794 call sites share the stream. On top of that the 1P picker first throws away
  `FrameCount(0x174B038) & 0x3FF` values (unless demo, 0x174AFC0 != 0).
- So the pick is **not reproducible across machines**, and seeding is not a usable way to force it.

### 1P picker 0x7380A0 (cdecl, arg = manager data). Called only at **0x739A55**
Skips any slot that is already 0xFE.
1. **Piece 1** (+0x2C): uniform over C ∪ E: `r = trunc(rand()/32768 * (nC+nE))`.
   An E pick is removed from the E list.
2. **Piece 2** (+0x3C): over B ∪ E.
   - Each candidate is ranked by distance from piece 1 (rank = number of candidates closer).
   - `r = trunc(N - rand()/32768*N/2)`, clamped to N-1: one of the **farther half**.
3. **Piece 3** (+0x4C): over A, ranked the same way with metric 0x77FC90 (uses both earlier
   pieces), and picked from the far half.
4. `+5 += 1` for each slot filled.

2P pickers: 0x7387D0 (3 pieces) and 0x739340 (one piece from all lists, when 0x174AFDF == 1).
Mission 5 has **fixed** pieces: the FINAL objects in the `_hd_` SET (ids 0..2), with `+5 = 3`.
So Mission 5 is already identical for everyone.

### Forcing a set

**Hook:** the only call to the picker is at **0x739A55**, in this sequence in the manager's
MainSub 0x739570:

```
0x739A54  push edi
0x739A55  call 0x7380A0
0x739A5A  add esp,4
0x739A5D  mov byte [edi],4
```

Redirecting it to an `extern "C" fn(m: *mut u8)` that writes the 3 slots (`{kind, id, 0, x, y, z}`)
and adds 1 to `m[5]` per slot written replaces the selection:
- It runs at the vanilla moment, before the EMERALD objects query in the same frame.
- It runs again after death and full restarts (states 6/7), so restarts keep the same set. A slot
  already 0xFE (collected before a death restart) is left as is.
- A spot that isn't in the local candidate list (upgrade gating) still works: the object exists
  in the SET file and turns active because only (kind, id) is compared (0x73B660).

**Data only:** in the frame hook (before 0x43CE20), when `m && m[0] == 2`, writing the 3 slots,
setting `m[5]` to the number written and setting `m[0] = 4` skips the vanilla selection. Writing
`m[0] = 1` every frame while it is 2 holds the selection (case 1 puts it back to 2 because
0x1A5A33C is still 1), but the objects then query too early and go to sub-state 3, so they need a
re-query (section 3) once the slots are written.

**SET file format** (`resource/gd_PC/set00NN_s.bin` + `_u.bin`, subject to Mod Loader
replacements): big-endian, header 0x20, `u32 count` at +0; entries of 0x20 bytes:
`u16 ID, i16 rotX, rotY, rotZ, f32 pos[3], f32 scale[3]`. EMERALDs have `ID & 0x7FFF == 0x0F`
(0x0E in level 7). Piece 1 comes from C (kinds 1, 3), piece 2 from B (0, 2, 5), piece 3 from A
(4, 7, 8); kind 6 is 2P only, kind 9 is Mission 5 only, and kinds 7/8 in levels 5/26 depend on the
save's upgrades.

---

## 3. Piece objects: active or not, collection, removal

### Active check
EMERALD main **0x6D0AD0**. The object's work is Data1 = `obj+0x34`:
- `+0` action,
- **`+1` sub-state**: 0 → 1 → query → **2 active** or **3 not chosen** (invisible, no collision,
  never asks again while loaded),
- `+8` rotX as int (`kind<<8 | id`),
- `+0x14` position.

The query is **0x73B660** (usercall BL = kind, DL = id, EDI = new position or 0). It returns
slot+1 when a slot that isn't 0xFE holds (kind, id), else 0. Display 0x6D0DA0 only draws
sub-state 2.

**Re-query** (after changing the slots while objects are loaded): for each SET record with
flag 1 whose `main_of(rec) == 0x6D00C0` (see `stage-objects.md`), with `d = rec->obj->+0x34`:
- if `(d[8] >> 8 % 11, d[8] & 0xFF)` is in the set: `d[1] = 1` (asks again next frame, becomes 2),
- else if `d[1] == 2`: `d[1] = 3`.

Objects that range out and reload ask again by themselves.

### Collection: **0x73B3E0**
- Call: usercall **EAX = kind, ECX = &pos** (or 0), stack **(id, player)**, caller pops 8.
- 5 call sites:

| Site | Who calls |
|---|---|
| 0x6D0C56 | normal kinds |
| 0x6D140E | kind 6 |
| 0x6D2527 | kind 7 |
| 0x6D28E2 | kind 10 object |
| 0x47AC64 | enemy that holds a piece (EAX = 10) |

If no slot that isn't 0xFE matches (kind, id), it does nothing. With `player >= 2` it instead
spawns the piece through `*(fn*)0x1A5A298` = 0x6CFFC0 `(pos*, kind, id)` (enemy drop).

With player 0/1 it:
1. `m[5]--`, `slot.kind = 0xFE`, `slot.id = player`,
2. EmeraldGetSign effect via `*(fn*)0x1A5A29C` = 0x6D0530,
3. HUD icon "ExecDisplayGottenEmeraldIcon" (0x73AEC0, main 0x73AE40),
4. sound 0x1008, Knuckles/Rouge voice 0x4EE..0x4F5 (0x443130),
5. **score +500** (+1000 with a CharObj2 flag) into 0x174B050, plus the hint/time bonus
   **0x73B310** (EAX = m):

   | Hints used (m[3]) | Bonus |
   |---|---|
   | 0 | 2000 within 1 min, 1000 within 5 min, else 200 |
   | 1 | 1500 within 1 min, 800 within 5 min, else 100 |
   | 2 | 600 within 3 min, 500 within 5 min, else 0 |
   | 3 | 400 within 3 min, 300 within 5 min, else 0 |

6. `m[0x6C] = 0`, `m[3] = 0` if the piece was the current target, `m[4] = 0`,
7. outside Mission 4, probably moves the player's restart point (0x43D8E0 / 0x43E520).

The object then calls `KillObject 0x488DA0`, so the record loses 0x8000 and never reloads.

### Detecting a local collection
While `m != 0 && m[0] == 4`, a slot whose kind changes to 0xFE is a collection. After collection
slot +1 holds the player, not the id, so the previous (kind, id) has to be remembered. Slots going
back to 0xFF are the state 6/7 resets. Alternatively 0x73B3E0 can be wrapped: at entry, EAX/stack
give (kind, id, player), and a matching slot that isn't 0xFE means a real collection.

### Removing a piece without collecting it
On the game thread, when `m[0] >= 4`:
```
slot = m + 0x2C + s*0x10
if slot.kind in (0xFE, 0xFF): return
kind, id = slot.kind, slot.id
was_target = (s is the first slot whose kind != 0xFE)
slot.kind = 0xFE; slot.id = 2                // 2 = "someone else" (0/1 are counted in 2P)
m[5] -= 1                                    // see section 5 for what 0 means
if was_target: m[3] = 0; m[4] = 0            // like vanilla: next target's hints start over
// the live object:
for each SET record with flag 1 and main_of == 0x6D00C0 whose Data1 rotX == kind<<8|id:
    KillObject 0x488DA0 (EAX = obj)          // or Data1[1] = 3 to just hide it
```
- No score, sound, voice, HUD icon or restart-point change (those only happen in 0x73B3E0).
- If the object is not loaded, the slot change alone is enough. When it loads, the query fails,
  so it becomes sub-state 3.
- If it stays loaded and active, it is **still collectible**. 0x73B3E0 would then do nothing,
  but the object would still vanish locally, so it has to be hidden or killed.
- **Enemy-held piece (kind 10):** the enemy still carries the id, but on death 0x73B3E0 finds no
  slot, so nothing spawns.
- **Radar:** 0x73A6F0 skips 0xFE slots and puts the full radar on the first remaining slot, so
  the radar retargets by itself.
- **Hint monitors** (HINTBOX, main 0x6E2700): they read hints through **0x73B880**, which looks
  up the first slot that isn't 0xFE in the per-stage hint table (`*(ptr*)0x1AF016C`: 8-byte
  `{kind, id, i16 hint[3]...}` entries ending at 0xFF; texts `*(ptr*)0x1AF0170`). They follow
  the new target automatically.
- **Death restart:** state 7 keeps 0xFE slots, and the picker hook puts the forced set back for the others.

---

## 4. 2P treasure hunt vs 1P

- **2P mode:** `TwoPlayerMode 0x174AFDE != 0`.
  - `0x174AFDF` is a 2P rule byte. 1 means the single-piece game: picker 0x739340, first piece wins.
    Otherwise there are 3 pieces and the **first to 2** wins.
  - State 4's 2P branch (0x739A65) counts 0xFE slots per `+1` player and goes to state 5.
    State 5 calls `0x43E6D0` with ESI = winner, which adds a win to `0x174AFF5[p]`.
  - Time-up: **0x451A0E** compares the counts.
  - Each player has their own radar (`m[0xA]` / `m[0xB]`).
  - SET files: `set%04d_2p_s/u.bin`, falling back to the 1P names when missing (0x488F60).
    Mission 5 uses `_hd_`, also with a fallback.
- **Battle stage table 0xC70F98** (0x10 bytes: `i16 level, i16 0, i16 stage no., i16 0, u32
  text ids`; groups end at -1). Hunt group: **46 Dry Lagoon 2P, 26 Egg Quarters, 8 Security Hall,
  41 Wild Canyon 2P, 5 Pumpkin Hill, 32 Meteor Herd, 59 Death Chamber 2P, 44 Mad Space**. A
  separate group has **49 Pool Quest, 50 Planet Quest**.
- **Playing a 2P stage in 1P:**
  - **59 works for pieces** (it only has `set0059_s/u.bin`, used in both modes: 68 candidates).
  - **41, 46, 49, 50 have no pieces in 1P**:
    - their EMERALDs exist only in the `_2p_` files,
    - 41 has no 1P SET file at all,
    - 46/49/50 have 1P files with 0 EMERALDs.
  - Even with the `_2p_` files forced:
    - 50 has only kinds 4/5, so piece 1's list C is empty in 1P,
    - 49 has only 4 C pieces,
    - kind 6 never registers in 1P.
- Usable 1P hunt stages: **5, 7, 8, 16, 18, 25, 26, 32, 44** and 59. Missions 1, 4 and 5 have a
  hunt; 2 and 3 don't create the manager. Mission 5 uses other SET files and fixed pieces.

---

## 5. Completion and time

- Completion = `m[5]` reaching 0 in state 4. The next manager frame goes to state 5 and calls
  **`0x43E6D0`** (ESI = 0 in 1P), which:
  - returns at once if `0x174B002` or `0x174B003` is set,
  - sets **TimerStopped 0x174AFDA = 1**, 0x174AFD6 = 1, **ControllerEnabled 0x174AFFF = 0**,
  - sets **0x174B002 = 1** (stage cleared),
  - continues the clear sequence (0x452A00 ...).
- **Time:** the stage timer bytes **0x174AFDB minutes, 0x174AFDC seconds, 0x174AFDD frames
  (1/60)**. They are frozen from that frame on. They are reset from the checkpoint copies
  0x174B00E/0F/10 (0x44C975).
  - `m+0x6C` counts frames since the last piece (bonus only).
- Removing a piece (section 3) also decrements `m[5]`, so when the last piece is taken by anyone,
  every game clears at the same moment with its own time.
- The vanilla score includes per-machine time/hint bonuses.

---

## How the mod runs a room hunt

- Applies to stages 5, 7, 8, 16, 18, 25, 26, 32, 44 and 59 in 1P, Missions 1, 4 and 5, in a room
  (a co-op room, or a stage started together).
- The lowest player number present in the stage is the authority. It reads its own picks once
  hunting starts (after waiting 2 s for a set already going round) and broadcasts the set every
  second. Only the authority's set is accepted, and it can replace an earlier conflicting one.
- Sets keep vanilla's fixed choices (C kinds 1/3, B kinds 0/2/5, A kind 4; Mission 5 FINAL ids
  0/1/2). Enemy-held, moving and upgrade-only picks are replaced with the nearest fixed
  candidate in the same list. Duplicate (kind, id) candidates (Mad Space 3/6) are excluded so one
  id never points at two spots.
- Every game, the authority's too, keeps its slots set to the room's set through the picker hook
  at 0x739A55 and a per-frame check, then re-queries the loaded piece objects (MainSubs 0x6D0AD0
  and 0x6D2310). Rebinding clears local collected marks from a different set and resets hints.
- A local collection is claimed from the host's referee; the first claim wins. Pieces owned by
  others are removed from every other game with no score (radar and hints move on by
  themselves). Claims that arrive before the set are kept.
- The last piece clears the stage for everyone. The referee's piece count is the score.

Confidence:
- High:
  - manager pointer, data layout of slots, `+5` and the state machine,
  - EMERALD dispatch/kind table, candidate registration (0x73AFE0) and list mapping,
  - query 0x73B660, collection 0x73B3E0 and its effects order,
  - completion via `+5 == 0` → 0x43E6D0,
  - CRT rand and the FrameCount burn,
  - SET-file naming/fallback, battle stage table contents.
- Medium:
  - selection details (ranking direction, piece 3 metric 0x77FC90),
  - manager-before-objects ordering,
  - sub-state 3 → 1 re-query being harmless,
  - meaning of 0x174AFDF,
  - upgrade bits for kinds 7/8,
  - 0x43D8E0/0x43E520 being the restart point,
  - timer byte meanings (standard SA2 layout, matched to the reset code only).
- Low: `slot.id = 2` for "someone else" being harmless in 1P (only 2P code reads it).

## Radar draw gates (0x73A6F0)

The radar (and its beep, 0x73A450 for the first piece still out) is skipped entirely unless
`[0x174AFCB] != 0`, `[0x174B002] != 1` (stage not cleared), `[0x174B009] == 0` and
`[0x174AFF9] != 1`. Per slot it skips 0xFE (collected) and in 2P (`[0x174AFDE]`) takes the other
branch. The colour sprite comes from the distance to the player (0x77FBD0) against the
thresholds at 0x904F30 / 0x109E030 / 0x904E30.
