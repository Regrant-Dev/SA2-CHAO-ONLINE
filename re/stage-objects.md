# Stage objects: SET loader, kill flags, enemies and item boxes (for co-op kill sync)

Steam `sonic2app.exe`, image base 0x400000. Everything below was read from the objdump listing
of `re/bin/sonic2app.dumped.exe` (none of it run in game yet). Object names come from the
per-level object lists in the exe (each entry carries its name string).

Goal: when one player destroys an enemy or item box, destroy the same object in every other
player's game in the same level and mission.

---

## 1. SET loader and the per-stage record table

### Globals
| Addr | Type | Meaning |
|---|---|---|
| **0x1DDE280** | `SETObjectData[0x600]` | **one record per SET entry**, stride **0x10** |
| **0x1DDE270** | i16 | number of records in use |
| 0x1DDE268 | `ObjectList*` | current level's object list `{u32 count; ObjectListEntry* entries}` |
| 0x1DDE274 / 0x1DDE278 | ptr | SET file header / first SET entry (header + 0x20) |
| 0x1A5A340 | `ObjectMaster*` | the "SetObject" task that runs the loader |
| 0x1A5A33C | u32 | 0 until the loader's first pass after a (re)start |

### `SETObjectData` (0x10 bytes)
| Off | Type | Meaning |
|---|---|---|
| +0 | u8 | load count (incremented on every load, 0x488511) |
| +1 | u8 | object-specific persistent byte (read by 0x488DC0, zeroed by the resets) |
| +2 | u16 | **flags**, see below |
| +4 | `ObjectMaster*` | the live object (only meaningful while flag 0x0001 is set) |
| +8 | `SETEntry*` | the entry in the SET file |
| +0xC | float / ptr | after load: range-out distance (load range^2 + 100). With flag 0x0002 it is instead a heap pointer to a saved position/rotation that the next load uses (and frees) |

### Flags (+2)
| Bit | Meaning | Evidence |
|---|---|---|
| **0x8000** | **alive**: the loader only loads records with this set. Clearing it is how objects stay dead | loader `movsx ebx,[rec+2]; test ebx,ebx; jge skip` (0x488441 / 0x488687); set for every new record (0x48897D) |
| **0x0001** | **loaded now** (an ObjectMaster exists). Set at load (0x488513), cleared by `DeleteObject` (0x46F86D) | |
| 0x0002 | +0xC holds a saved position/rotation for the next load | 0x4884C2, freed at 0x488592 |
| **0x0004** | **item box already broken** (ITEMBOX, ITEMBOXAIR, BALLOON, SG boxes set it). Also used by some other objects | 0x6C81BF, 0x6C92EE, 0x6DB33B, 0x6EAEA0 |
| 0x0008 | never range-out (0x488C8A) | |
| 0x0010 | keep this record's state on a death restart (checkpoints set it, 0x6D9AB0) | 0x488B03 |
| 0x0020 | object-specific "gone/used" bit, kept through a full reset | 0x488B9B; E AI deletes itself on load if set (0x4FF9D5) |
| 0x4000 | keep the record when the SET file is swapped (unused in practice, see below) | 0x488A33 |

### `SETEntry` (0x20 bytes, byte-swapped in place at load by 0x487FC0)
| Off | Meaning |
|---|---|
| +0 | u16 object ID; `ID & 0x7FFF` indexes the current object list |
| +2/+4/+6 | i16 rotation X/Y/Z (copied to Data1+8/+0xC/+0x10 as ints) |
| +8 | float pos XYZ (copied to Data1+0x14) |
| +0x14 | float XYZ parameters (copied to Data1+0x20, "scale") |

### `ObjectListEntry` (0x10 bytes, `entries[ID & 0x7FFF]`)
| Off | Meaning |
|---|---|
| +0 | u8 LoadObject flags (1 Data2/+0x38, 2 Data1/+0x34, 4 +0x3C, 8 +0x40) |
| +1 | u8 object list (task list index) |
| +2 | u16 range flags: 1 = custom range at +4, 2 = always load, 4 = load regardless of range on the first pass |
| +4 | float range, squared (default 160000 = 400 units, 0x1613D04) |
| +8 | **MainSub** |
| +0xC | name `char*` |

### ObjectMaster fields used here
`+0x8` parent, `+0xC` child, `+0x10` MainSub, `+0x18` DeleteSub, **`+0x30` SETObjectData***,
`+0x34` Data1 (EntityData1), `+0x38` Data2 (enemy work), `+0x40` Data2 (other), `+0x44` name.
`+0x30` is written by the loader right after the object is created (0x48851B / 0x48875D);
`DeleteObject` clears it.

### Functions
| Addr | What | Call |
|---|---|---|
| 0x487E40 | LoadSetObjects: creates the "SetObject" task, stores the object list, registers the SET file | cdecl (objList*, setFile*). 70 callers, one per level |
| 0x487FC0 | byte-swap the SET file and append its entries to the table (0x488930) | EAX = setFile |
| 0x488F60 | build `set%04d` (CurrentLevel) and load `_s.bin` + `_u.bin` (or `_2p_` in 2P, `_hd_` when MissionNum == 4), merged by 0x488DD0 | |
| 0x487F60 | SetObject task MainSub: each frame calls the loader | |
| **0x488630** | loader pass, 1 player: for every record with 0x8000 set and 0x0001 clear that is in range, `LoadObject` + `rec->obj = o; o->+0x30 = rec; flags |= 1` | |
| 0x4883D0 | the same with two players (used when MainCharObj1[1] exists) | |
| 0x46F610 | LoadObject + allocate data blocks | ECX = list, EAX = name, EDI = main, push flags |
| 0x46F720 | **DeleteObject** (calls DeleteSub, frees, `rec->flags &= ~1`) | cdecl (obj). Used as a MainSub to delete next frame |
| **0x488C80** | CheckRangeOut: if no player is within `rec+0xC`, sets `MainSub = DeleteObject` and returns 1. **Does not touch 0x8000**, so the object reloads when you come back | EDX = obj, push float dist |
| **0x488DA0** | **KillObject**: `rec->flags &= 0x7FFF; &= ~1; obj->MainSub = 0x46F720`. No null check on +0x30 | EAX = obj |
| 0x488AD0 | death restart: every record without 0x0010 gets `flags = (flags & ~4) | 0x8000` and byte +1 = 0 | called at 0x43CBC6 |
| 0x488B30 / 0x488BD0 | full reset (flags = 0x8000, keeping 0x20 in the first) | called at 0x43C7FB / 0x43C7F4 |

**Stable index:** `N = (rec - 0x1DDE280) / 0x10`. The table is filled in SET file order (`_s`
entries then `_u` entries) at stage load and is never reordered during a stage. The only code
that compacts it (0x488A00, part of a SET-file swap in 0x488860) is reachable only when
0x1A5A338 != 0, and nothing in the exe writes a non-zero value there. So two games in the
same level with the same SET files get the same N for the same object. The files differ for
2P mode (`_2p_`) and Mission 5 (`MissionNum == 4`, `_hd_`). That is why the mission must match.
Mod Loader file replacements must match too.

---

## 2. Destroyed vs unloaded

| Event | What happens to the record | Comes back? |
|---|---|---|
| Out of range (0x488C80) | MainSub = DeleteObject, so flag 0x0001 is cleared. 0x8000 stays set | yes, when in range again |
| Enemy killed | `flags &= 0x7FFF` and `&= ~1`, MainSub = DeleteObject (inlined in each enemy, or 0x488DA0) | no |
| Ground item box broken (ITEMBOX 0x6C7C10) | `flags |= 0x4` (0x6C81BF). The object stays as an empty shell | reloads as a broken shell |
| Air / balloon / SG item box broken | `flags |= 0x4`, then the object clears 0x8000 itself (at break end, or on the next load: ITEMBOXAIR init 0x6C8FA3) | no |
| KD item box (Knuckles/Rouge) broken | 0x488DA0 (clears 0x8000) at 0x6E95D3 | no |
| Death restart (0x488AD0) | all records except checkpointed (0x10) ones: 0x8000 set again, 0x4 cleared | **yes, everything respawns** |

So the persistent "destroyed" marks are **0x8000 cleared** (enemies, most boxes) and
**0x0004 set** (item boxes).

### Enemy kill path (example: E KUMI, main 0x501530)
- Hit handler 0x5004B0: `0x47AA70` (was the enemy hit by a player attack?). If yes:
  - `Data1[0] = 8`, collision off (`colli+4 |= 0x10`),
  - 0x47A5B0 (split the model into flying pieces),
  - `0x44DAC0(EAX = 100, or 200 when 0x46F590 says it was a special attack)`,
  - `0x47ABB0(EAX = Data1, push enemyWork /*obj+0x38*/)`.
- Next frame, state 8/9 (0x5015A3) clears 0x8000 and 0x1 and sets MainSub = DeleteObject.

Shared helpers used by the enemies:
| Addr | What | Callers |
|---|---|---|
| **0x47ABB0** | **EnemyDestroyed**: finds the attacking player from the collision, combo/score popup (0x73B3E0), `Data1->Status |= 0x100`, explosion sprite (0x6B80B0) / "dmyEnemy" debris, animal drops in 1P (0x48F8E0 / 0x48ADA0), sound 0x8015. Usercall EAX = Data1, one stack arg = enemy work (obj+0x38: +0x45 player index, +0x4C effect flags), caller pops | 18 call sites: every E-enemy plus a few others |
| **0x44DAC0** | AddEnemyScore: `0x44D2D0(EAX)` (adds to score 0x174B050, doubled by a CharObj2 flag) + "CounterOfDestructedEnemy" combo counter | 18 call sites, all enemies |
| 0x44D2D0 | AddScore(EAX = points) | |
| 0x47AA70 | "was hit by a player attack" test (EAX = Data1, push enemyWork) | 22 |
| 0x6B80B0 | spawn an explosion/smoke sprite, cdecl (pos*, vel*, float scale) | 46 (medium confidence on the args) |

### Item box break path (ITEMBOX, main 0x6C7C10)
- State 2 → 0x6C8090:
  - finds the touching player (0x486520 / 0x4865B0 → 0x46DCC0),
  - gives the item: `itemTable 0xB4D120[obj+0x3C].func(obj, player)` (8-byte entries, +4 = fn),
  - `Data1[0] = 3`, break effect 0x6EBE50, sound 0x800D,
  - **`rec->flags |= 4`** (or the parent's record if `Data1[1] != 0`, a box spawned by another object).
- Init 0x6C7DB0: if `rec->flags & 4`, it goes straight to the broken-shell state 4 (no item).

---

## 3. Detecting "the local player destroyed object #N"

There is no single function. The record clear is inlined about 100 times, and the shared
EnemyDestroyed (0x47ABB0) is reached from 18 call sites and only gets Data1. **Recommended:
poll the table once per frame** in the existing frame hook (0x43AB24 → 0x43CE20, `game.rs`):

```
count = *(i16*)0x1DDE270; recs = 0x1DDE280
for n in 0..count:
    f = *(u16*)(recs + n*0x10 + 2)
    was = last[n]; last[n] = f
    if !is_synced_type(n): continue
    killed = (was & 0x8000) && !(f & 0x8000)
    broken = !(was & 0x4) && (f & 0x4)
    if (killed || broken) && !applied_by_us[n]: send Kill(level, mission, n)
```
- Cost: at most 0x600 u16 reads per frame.
- Snapshot `last` after the stage's first loader pass (`*0x1A5A33C != 0`). Reset it when
  CurrentLevel changes or `0x1DDE270` drops.
- A death restart (0x488AD0) sets 0x8000 again and clears 0x4. Treat those 0→1 changes as
  "respawned locally" (see recipe step 4), not as events.
- Latency: an enemy clears 0x8000 one frame after the hit (state 8 → 9).
- It catches every way an enemy dies (attacks, bombs, Chao, falling objects). That's what we
  want for "the object is gone in my game".

Hook alternative (if attribution to a player matters): redirect the 18 `call 0x47ABB0` sites.
At entry EAX = Data1. Find the record with `rec->flags & 1 && rec->obj->+0x34 == EAX`. Item
boxes would still need the poll (or the 0x6C81BF-type sites).

---

## 4. Destroying object #N from outside

Run this on the game thread (frame hook), only when CurrentLevel and MissionNum match:

```
rec = 0x1DDE280 + N*0x10
if N >= *(i16*)0x1DDE270: ignore
type = main_of(rec)                                   // section 5
if type is an item box:  rec->flags |= 0x0004
if type is not the ground ITEMBOX (0x6C7C10/0x6C7CE0): rec->flags &= 0x7FFF
if (rec->flags & 1) && rec->obj:                      // currently loaded
    rec->obj->MainSub(+0x10) = 0x46F720               // deleted next object pass, DeleteSub runs
applied_by_us[N] = true
```
- **Enemies:** with 0x8000 cleared and the object deleted, it is gone for good. No score,
  animals, rings or sound for us.
  - This is exactly what 0x488DA0 does. You can call 0x488DA0 with EAX = obj instead of the two
    writes (usercall, needs `obj+0x30 != 0`).
- **Ground ITEMBOX:** keep 0x8000 and set 0x4. Deleting the live box makes the loader recreate it
  next pass as the empty broken shell (init 0x6C7DB0), which looks like it was broken.
  No item is given. If the box is not loaded, 0x4 alone is enough.
- **Other boxes:** 0x4 plus clear 0x8000. They just disappear (the vanilla end state anyway).
- **Not loaded:** the same flag writes. The loader skips records without 0x8000, and box inits
  read 0x4.
- Effect (optional): `0x6B80B0(&Data1->pos, &zero3, 16.0f)` before deleting a loaded enemy
  should give a puff. This is not verified. Don't call 0x47ABB0 or 0x44DAC0: they award
  score/combo and use the local player's collision.

---

## 5. Which records to sync

Classify by the list entry's MainSub (stable across levels), not by list index (it differs
per level):
```
main_of(rec) = (*(ObjectList**)0x1DDE268)->entries[(rec->entry->ID & 0x7FFF)].main  // +8
```
The name at entry +0xC can be logged for debugging.

Enemies (all lists in the exe, names starting "E "):
| Main | Names |
|---|---|
| 0x4F8F90 | E NAMIE (_F/_N) |
| 0x4F9F00 | E SHOUKO (shares NAMIE's kill/score code; probably an enemy) |
| 0x4FC700 | E KYOKO (_F/_N) |
| 0x4FDD00 | E EMI (_F/_N) |
| 0x4FF990 | E AI (_F/_N) |
| 0x501530 | E KUMI (_F/_N) |
| 0x503140 | E GHOST |
| 0x504390 | E PATHGHOST |
| 0x505C20 | E PATH |
| 0x506510 | E GOLD |
| 0x50A850 | E CHAOS |
| 0x50C3F0 | E PATHCHAOS |
| 0x50D790 | E 1000 |
| 0x50E4D0 | E SARU |
| 0x50F150 | E UNI |
| 0x7A0D10 | E AKAHIGE |
| 0x7A1600 | E BATABATA |
| 0x7A21C0 | E BEETON |

Leave out: `E SW SHOUKO`/`E SW SHOUKOC` (0x4FA070, probably the trigger for E SHOUKO). Also
leave out "E PATH"/"E CHAOS" entries whose main is 0x5B4250 (an empty stub) or 0x6C0F80 (RING)
in some lists. Matching by main handles both automatically.

Item boxes:
| Main | Names | Break marks |
|---|---|---|
| 0x6C7C10 | ITEMBOX | 0x4 (shell stays) |
| 0x6C7CE0 | ITEMBOX2_B | same layout as ITEMBOX (assumed same behaviour) |
| 0x6C8EF0 | ITEMBOXAIR | 0x4, then 0x8000 cleared |
| 0x6DB030 | ITEMBOXBALLOON | 0x4, then 0x8000 cleared |
| 0x6E9350 | KDITEMBOX | 0x8000 cleared (0x488DA0) |
| 0x6EABC0 | SGITEMBOX / SG_ITEMBOX | 0x4 and 0x8000 cleared |

Breakables (synced since 2026-10-17): every object list entry whose update calls KillObject
0x488DA0 (clears 0x8000, never reloads). Found by mapping the 30 KillObject call sites to the
function containing them and the init that installs it, then to the entry names: DRMCN 0x4B6A40,
0x4D9BD0, 0x5BBAB0; PPSHED 0x4B6D60; MITRUCK 0x4D7AB0; SLANE 0x4DF5E0, 0x5AE3E0; SLANE2 0x5AF4A0;
E EMI 0x4FDD00; BIGSHUTTER 0x5C2B90; BALOON 0x624E10; BOMBE 0x638A60; DLCONT 0x645B30; DYNAMITE
0x65ADA0, 0x6DA720; CONTWOOD 0x6CD7F0, 0x6FB200; CONTIRON 0x6D63F0, 0x6FBBA0; KDDOOR 0x6E3F80
(not DOORT 0x6E3F60); WSMMLS 0x6EA1E0; BCOL 0x6F6EA0; POISON 0x777250; SGOLID 0x779FB0; CGWOOD
0x77A950. Three call sites had no named entry (0x6356F0, 0x6E9DE0, 0x77BA60).

Rings: RING 0x6C0F80 and SGRING 0x6C1470 also end in KillObject. In co-op a ring collected by
anyone is gone for everyone and gives each player a ring: AddRings **0x44CE10** (EAX = player,
DX = signed count; rings at 0x174B028 + 2*player, clamped to 999, extra life per 100; 27 callers).

Not synced (examples): RING 0x6C0F80 (clears 0x8000 when collected), SPRA/SPRB, SAVEPOINT
0x6D99A0 (sets 0x10), SWITCH, SCHBOX/SG_SEARCHBOX (hint/search boxes), SOLIDBOX (no KillObject in
it), DOOR 0x613550/0x6B06D0/0x7049E0 (switch doors), CONTCHAO, ITEMBOXBAL GEN 0x6DBA70
(a balloon generator, see caveats).

---

## Syncing kills (recipe)

1. **On stage start** (CurrentLevel set, GameState 16, `*0x1A5A33C != 0`): reset `last[]`
   and `applied_by_us[]`. Key the session by (CurrentLevel, MissionNum, 2P flag 0x174AFDE).
2. **Each frame (before the original 0x43CE20):**
   1. Apply queued remote kills (section 4).
   2. Then diff the table and send new local kills/breaks of synced types (section 3).
   Doing the apply first means our own writes are already in `last[]` and marked
   `applied_by_us`, so they are never echoed.
3. **Receive** `Kill(level, mission, N)`: ignore if level/mission differ. Otherwise queue it,
   and keep a per-stage set of killed N.
4. **Death restart** (seen as many records getting 0x8000 back at once, or hook the
   `call 0x488AD0` at 0x43CBC6): reapply the stored kill set right after it, before the next
   loader pass. Then objects other players destroyed stay destroyed. Skip this to match vanilla
   (everything respawns for the player who died).
5. Late joiner: send the stored set when a player enters the same stage.

Caveats:
- A box created by another object (`Data1[1] != 0`) writes its parent's record (obj+0x8 →
  +0x30). The parent's main is then e.g. the balloon generator. Leave these out unless needed.
- Mod Loader SET-file mods must match on all clients (index = file order).
- In 2P (`_2p_` files) both local players share one table. Syncing is per-machine anyway.
- 0x0004 means something else for some non-box objects. Only test it for item-box mains.

Confidence:
- High: table address/stride/count, record layout (+0..+0xC), flag 0x8000 (alive) / 0x0001
  (loaded), loader and DeleteObject behaviour, CheckRangeOut not clearing 0x8000, KillObject
  0x488DA0, ITEMBOX break setting 0x4 and init reading it, E KUMI kill sequence.
- Medium: the other flag bits (0x2/0x8/0x10/0x20/0x4000), EnemyDestroyed 0x47ABB0 and
  AddEnemyScore 0x44DAC0 (the call pattern and "CounterOfDestructedEnemy" string are clear, but
  the internals were only skimmed), the enemy whitelist (built from names; only KUMI, AI and
  SHOUKO were read), and the claim that the SET-file swap never runs.
- Low: 0x6B80B0 argument meaning. No part of this has been tested in game yet.
