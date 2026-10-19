# Stage objects: SET loader, kill flags, enemies and item boxes (for co-op kill sync)

## Progression crate correction (2026-10-19)

An in-game report showed Hidden Base's stepping stone missing after a death. Matched
the live log's position (-3990, 840, -4678) against the unmodified big-endian
`set0023_s.bin` and native object list **0xA2DCC0** (89 entries at **0xA2D730**):

- record 268: SOLIDBOX (-3989.7,820,-4677.6), main **0x6FC330**;
- records 270/271: SOLIDBOX (-3969.7,820/860,-4677.6);
- record **273**: **CONTWOOD**, (-3969.7,840,-4677.6), main **0x6FB200**;
- record **308** is another CONTWOOD step at (-3969.7,840,-4852.6).

This identifies the breakable step between solid boxes, rather than an item/reward box.
REA VM/Ghidra inspected the 0x6FB200 initializer: display 0x6FB8A0, update 0x6FB280,
delete 0x6E7140, collision initialization 0x47E620. The existing Breakables evidence above
establishes its persistent kill flag; our replay loop deliberately re-deleted it every
restart. The user confirmed the missing crate persisted after restarting.

Standard wooden/iron crates (**0x6CD7F0,0x6D63F0,0x6FB200,0x6FBBA0**) are now per
player, excluded from both outgoing detection and incoming destruction history. Incoming
events are classified before insertion, so older peers' snapshots cannot reintroduce
crate kills or relay them onwards. Each player keeps the native break/respawn behavior,
and a faster friend cannot destroy a step they still need. Enemy/item/ring/Chao-reward
box sharing remains active. Regression tests cover those categories and the exact
Hidden Base crate. In-game return after death and mixed-version play still need retesting.

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

## Keys (2026-10-17, REA on a slice + objdump)

- KEY: init 0x702880, main 0x7029F0. Each frame it registers at `0x1AF00A8 + 4*type` (type =
  `|Data1+0x28|` rounded, 0 or 1) and runs a state machine in Data2 (+0x40): +0 state, +4 next
  state (-1 none), +8 frames in state; on-enter table 0xAA91C4, per-frame table 0xAA946C.
- States: 0 on the ground (0x703130/0x7031B0), 1 carried (enter plays sound 8 + a voice),
  2 going into the keyhole (lines up with the keyhole at `0x1AF00B0 + 4*type`), 3 in (sound 9;
  the keyhole and door react), 4 back to its start position (Data2+0x18).
- KEYHOLE init 0x6FDC80 registers at 0x1AF00B0; KEYDOOR 0x6FFE40.
- Co-op: a key reaching state 3 is sent (`KeyUsed`); the others set their own key's next state
  to 2 when it's on the ground or returning, so their door opens the game's way. State 2 reads
  the keyhole slot unchecked (0x7035F0), and an unloaded keyhole clears its slot (0x6FDDD4), so
  only while the keyhole is registered.

## Switches (2026-10-17)

- `SwitchFlags` **0x1DE9460**: one byte per switch id (0..31). Pressing sets `|= 3` (0x6DE040 /
  0x6DE0B0, with sound 0x800E the first time); a switch shows itself pressed when bit 0 is set
  (Data1+1 = 1). Doors and gates read the byte for their id.
- SWITCH init 0x6DCDC0: state 0 sets update 0x6DD7E0 / delete 0x6DDCA0 and Data2 (+4 type 0..4
  from SET rotX, +6 id 0..31 from rotY); GSWITCH2, LIGHT SW, GRAV_SW etc. are other switch kinds.
- The table is 32 bytes (0x1DE9460..0x1DE947F, cleared by 0x47E4B0). That clear runs on both
  restart paths (0x43C822, 0x43C90F), so a death switches everything off locally.
- Co-op shares switches turning **on** only (`Body::Switch`), and sets the room's switches again
  after a restart; sharing "off" would close the others' doors when one player died.

## Stage sync audit (2026-10-18, REA/Ghidra on the dumped exe + objdump scans)

The goal was to find what one co-op player sees in a stage that the others' games don't, from
the code instead of from playtesting.
- **Method:** every write that marks a SET record (`and [rec+2],0x7FFF` "alive", and `or` of
  0x4/0x10/0x20/0x40) was mapped to its function. Then from the function to the object list
  entry whose MainSub reaches it, through calls and MainSub installs (a scan of all the exe's
  object lists: 955 MainSubs with names).
- **Destroyed for real:** a function that clears "alive" and also calls AddScore 0x44D2D0
  counts as destroyed by a player.

### Ring groups (fixed: `Body::StageRings`, tag 50)
- **Most of a stage's rings weren't shared.** RINGLINE 0x6C5ED0, RINGCIRC 0x6C6180, KDDRNGL
  0x6E4830, KDDRNGC 0x6E4FB0, SWDRNGL 0x6ECAE0 and SWDRNGC 0x6ECE10 are ring lines and circles.
  Their rings aren't SET records: the group spawns them with RING's own MainSub 0x6C0F80 and no
  record (LoadObject "Ring", 0x6C62C4).
- **Group layout:**
  - Data1+3 is the ring count (1..=8, from the SET scale Z, Data1+0x28, 0x6C5FB1).
  - Data2 (+0x38, 0xA4 bytes from 0x6C5F70) holds 8-byte slots: +0 flags, +4 the ring object.
  - +0x40 holds the 12-byte ring positions.
- **Slot flags:** bit 2 means collected. RING sets it as it goes (0x6C1425: `[slot] |= 2`,
  slot+4 = 0, ring+0x38 = 0, then DeleteObject; a record ring calls KillObject instead). Bit 1
  means out of range (0x6C0FCB).
- **While loaded:** the update (0x6C6230, 0x6E4B40, 0x6E5120, 0x6ECF60) respawns every
  uncollected slot that has no ring. When no slot is uncollected it clears "alive" (KillObject's
  writes, inlined).
- **While unloaded:** the record's byte +1 keeps the collected slots. The DeleteSub (0x6C6050,
  0x6E4A20, 0x6ECCF0) folds slot i's bit 2 into bit 7 - i, and the init (0x6C5FD8, 0x6E49B6,
  0x6ECC81) sets slot i to 2 from it. A death restart zeroes byte +1 (0x488AD0), so the rings
  come back.
- **Co-op (`coop.rs`):**
  - **Detection:** a group's collected rings come from its slots while loaded, or from byte +1
    reversed while not. New ones are sent.
  - **Applying a friend's ring:** in a loaded group, its ring is deleted and the slot marked, the
    way RING does it. In an unloaded group, byte +1 gets the bit. The receiver also gets a ring
    (AddRings) unless it already had that one.
  - **Restarts and late joiners:** reapplied after a death restart, and sent in full to a player
    who arrives.

### More breakables (fixed: added to `coop.rs` BREAKABLES)
These clear "alive" in the same function that gives the destroy score:
- MDCONTBOX 0x5C3B50 (0x5C3DA5)
- BOMBE 0x6060C0 (0x6061BC) and 0x69EF30 (0x69F04C)
- TANK 0x6079E0 (0x607C46), 0x637980 (0x637B98), 0x69C980 (0x69CBE6) and 0x78BA60 (0x78BCD4)
- ORI 0x6D29F0 (0x6D2CD5) and 0x77C870 (0x77CB55)
- SHUT 0x76D760 (0x76D946)

### Left as they are, and why
- **Cars and BUFFER: left out.** CARMAN / CARMANC / CARKAZ (0x5E0820, 0x5E0B10, 0x5E0DE0,
  0x5B5D30) and BUFFER 0x5B30A0 clear "alive" with the score (0x5DEA20, 0x5B4BE0, 0x5B30F0).
  - They also clear it in paths without a score (0x5E15B0, 0x5EC7A0, 0x5B61C0), which look like
    traffic leaving its route.
  - CARKAZ/BUFFER sit with the kart objects, so syncing them could delete cars a slower player
    still has ahead. Add them only after checking those paths.
- **Per-player rewards: not shared, on purpose.** Each player should get their own:
  - CONTCHAO 0x6DE3B0 (Chao box: sets 0x4|0x20 and clears "alive", 0x6DE735);
  - SCHBOX / SG_SEARCHBOX (hint boxes);
  - GOALRING 0x6C63C0 (each player clears the stage);
  - item box contents (the breaker gets the item, the others get the broken shell);
  - the animals from kills (1P only, the killer's game);
  - the upgrades.
- **Enemy flags other than "alive":** E NAMIE 0x4 (0x4F7C5F, 0x4F8CAB), E AKAHIGE 0x4
  (0x7A0A85) and E AI 0x20 (0x4FEF87) are per-enemy persistent marks set on their kill paths.
  The "alive" sync already covers those enemies.
- **Springs:** SPRA / SPRB / KDSPRING clear "alive" (0x6C4698, 0x6C50AC) without a score. They
  are probably one-shot or level-scripted springs, so they are left alone.
- **Not attributed:**
  - 0x62DEB0 clears "alive" with a score, but sits inside a larger function near the mech
    boss objects (LOCKON 0x629DB0, LAZER 0x62CED0). Not attributed.
  - 0x5B11B0 / 0x5B1580 / 0x5B18C0 (kart) and 0x7A2930 / 0x7A2C50 are breakable-part callbacks
    in data tables. Their owner's record is the parent's (E BEETON is already synced).
- **Enemies' live state (not synced; fine for co-op):**
  - Enemies are local: each game's enemies react only to its own player (the 1P loader
    0x488630 checks player 1). On your screen, a friend's puppet is ignored by enemies.
  - Only "destroyed" needs one owner, and it's shared.
  - Boss positions and attacks are per game too. HP is shared (bosses.md).
- **Mech homing shots:** `shots.rs` fires a friend's bazooka shell from the same place in the
  same direction, but without their lock-on target. A friend's lock-on volley flies straight on
  your screen. What it destroys still goes away through the kill sync.
- **Puppet effects:** see effects.md. The spin and homing balls are drawn (from `action`). Still
  missing: Tails' mech jet and the kart exhaust/tilt.
- **Moving platforms and timed objects:** they run from the stage start, which rooms start
  together. They drift only for a player who restarts.

## Mech lock-on (2026-10-18): what a volley is, and why it isn't replayed
- **The volley isn't a homing projectile.** `WalkerBulletExecutor` (main 0x75D520) and
  `WalkerBazookaExecutor` (0x75D9F0) fly straight along Data1+0x20 and have no target field.
  They're the tap shots (0x741692 bazooka with the upgrade bit co2+0x24 & 0x200000, 0x741727
  bullet), and `shots.rs` already mirrors them.
- **Lock-on (holding B, co2+0x35C = 1):** each frame, 0x74CCF0 scans for a target while
  co2+0x368 (the locked count) < 0x20.
  - **Where it looks:** the candidate is the nearer of two collision lists, scanned by 0x74CB30
    (list 0x1DE4EE0 with count 0x1DE4EC8, list 0x1DE6DA0 with count 0x1DE944A). In 2P it also
    looks at the other player (0x74CC40).
  - **What it skips:** targets whose Data1+2 is 0x16/0x17.
- **Per target, a marker:** a child of our player object (0x470C00, list 2, main 0x74C260),
  with 0x20 bytes of data at +0x40:
  - +8 the player index;
  - +0x10 the target's collision centre (0x47EEC0);
  - +0x1C **the target ObjectMaster**.

  The target links back to the marker: an enemy at its work +0x40 (`obj+0x38` +0x40), a player
  at CharObj2+0x38C (0x486CB0 tells them apart). co2+0x368 counts up, and co2+0x390 is the last
  target's position.
- **Fire (B released):** each marker's main sets its Data1 +0x14 to our gun (co2+0x23C..0x244),
  +0x20 to the aim direction (co2+0x2D8..0x2E0), and switches to the flight main 0x74B920
  (display 0x74B620).
  - **Flight:** it homes on +0x1C every frame and on arrival hits the target with the player
    index (0x7617C0 / 0x75D310).
  - **Laser line:** the line is the marker's own display.
- **Why it isn't replayed on the friends' screens:**
  - **It hits as us:** a marker is our player's child and attacks as player +8. A friend's
    volley rebuilt here would fire from our mech's gun and hit for us (our score, combo and
    animals).
  - **Crash risk:** it holds a raw target pointer, while the kill sync deletes the same targets
    on its own schedule. The game avoids the dangling pointer only by checking the target's main
    for DeleteObject in the same frame (0x74C2A3), and our deletes don't go through it.
  - **Doing it right:** a draw-only copy of the laser (the 0x74B620 display driven from our own
    data, as puppet.rs draws characters), fed with the friend's gun position and the SET
    records of their targets.
  - **Until then:** the targets still disappear through the kill sync, so only the laser lines
    are missing.

## Enemies and friends (2026-10-18; src/enemies.rs. The stand-in and the loading near friends below were removed the same day: see "Stage hook safety audit")
- **What an enemy looks at:** player 1's EntityData1 through `PLAYERS[0]` 0x1DEA6C0 (62 reads in
  the E-enemy code 0x4F5000..0x512000 and 0x7A0000..0x7A4000). It also reads player 1's CharObj2
  0x1DE9600 (15 reads) and `PLAYERS[1]` only for 2P (0x4FDD8A, 0x50856B). Range-out (0x488C80)
  checks `PLAYERS[0]` and `[1]`.
- **Writes through those pointers in enemy code:** only E GHOST does any, a grab that sets the
  player's NextAction to 0x16/0x21 (0x501F2D, 0x502599) and CharObj2+0x28 (0x501F31, 0x50259D).
- **The fix (`src/enemies.rs`):** an enemy whose nearest player is a friend's puppet (within 600,
  at least 20 nearer than us to switch) runs its own MainSub with `PLAYERS[0]` pointing at a copy
  of our EntityData1 (0x30 bytes) moved to the friend's position. Then it turns, chases and fires
  at the friend.
  - **Nothing can reach us through it:** writes during its update land in the copy, and hits on
    us come only from real collisions.
  - **MainSub changes:** an enemy changing its own MainSub (next state, or DeleteObject) is
    followed.
  - **Left out:** E GHOST / E PATHGHOST (the grab writes our CharObj2).
  - **Not checked:** shared helpers the enemies call with a player index (outside the enemy
    ranges).
- **Loading:** the 1P loader 0x488630 loads SET objects only around player 1. So enemies near a
  friend but out of our range (default 400, `ObjectListEntry+4`) don't exist in our game until
  we get close.
  - **What the stand-in changes:** a turned enemy's range-out check sees the friend's position,
    so once loaded it stays loaded while the friend is near it.
  - **Loading around friends:** the 2P loader 0x4883D0 (used when `MainCharObj1[1]` exists)
    loads around both players. Running a pass of it with `PLAYERS[1]` set to a stand-in at a
    friend could load enemies around them.
  - **Not done:** 2P code paths read `MainCharObj1[1]`/`CharObj2[1]` too (camera, HUD), so it
    needs its own audit.

### Lock-on drawn for friends (2026-10-18, src/lockon.rs, `Body::LockOn` tag 51)
- **Sending ours:** our markers are found among our player object's children. The list starts at
  ObjectMaster+0xC and runs circularly through +0 (0x470C00 links them).
  - Main 0x74C260 means locked; 0x74B920 means flying.
  - Each marker's target (+0x40, +0x1C) is mapped to its SET record (`coop::record_of`).
  - What's sent: the gun (CharObj2+0x23C), the locked records with their aim points, and the
    ones that just started flying (fired).
- **Drawing a friend's:**
  - **The object:** one of our own per cursor or laser (LoadObject, list 3). Its Data1 (0x30)
    and marker block (0x20) come from the game's allocator with the 0x12345678 header, so the
    game's DeleteObject frees them.
  - **Slots:** display +0x1C is the game's cursor (0x74BEC0) or laser (0x74B620); the delete
    callback +0x18 is ours.
  - **Marker block:** +8 = 0, because the displays take the mech textures from player 0's
    CharObj2+0x444, so they draw only while we're a mech. +0xE = 7 (fully shown), +0x10 the
    aim.
  - **Target:** +0x1C points at a stand-in object of ours. 0x74C4E0 reads its Data1+2,
    Data1+0x2C, collision +0xC and +4, so the stand-in is built with those fields: the cursor
    draws and nothing real is touched.
  - **Movement:** the updates are ours. A cursor stays on the target, at its position here if
    it's loaded, otherwise where the friend aimed. A laser flies 12 units a frame from the
    friend's gun to the target and then goes. Nothing is hit or scored; the kills arrive as
    StageKills.

### Loading enemies near friends (2026-10-18, `coop::load_enemies_near`, src/enemies.rs)
- **What the 2P loader reads:** 0x4883D0 reads only `PLAYERS[0]` (or the camera 0x1A5A784 when
  there's no player 1) and `PLAYERS[1]`+0x14 (their positions). It loads unloaded alive records
  in range of either player with the same LoadObject/Data1 setup as 0x488630. Nothing in it
  reads `MainCharObj1[1]` or `CharObj2[1]`.
- **How we use it:**
  1. Every 10 frames, for each friend at least 200 from us, run one pass of it.
  2. During the pass, `PLAYERS[1]` points at a 0x30-byte stand-in at the friend's position.
  3. Every record except unloaded enemies within 300 of the friend (ghosts left out) has its
     "alive" bit cleared for the pass, so nothing else loads.
  4. The flags and the slot are put back right after.
- **Why player 2 is never seen:** what loads starts running on the next object pass, by when
  player 2 is gone again. `enemies.rs` turns it toward the friend that same frame, so its
  range-out check (0x488C80) sees the friend and it stays loaded while they're near.
- **Not before the first pass:** never called before the stage's first loader pass (0x1A5A33C),
  because that pass loads some objects regardless of range and our pass would mark it done.
- **Known churn:** an enemy loaded near a friend but nearer to us (not turned) can range out
  and load again on the next pass, every 10 frames at most.

### Lock-on when we aren't a mech
- **Where the textures come from:** the cursor and laser displays take the effect textures
  from a player's CharObj2+0x444, the "WALKTEX" archive (0x900E00, loaded by the walker loaders
  0x740DDE / 0x74103E / 0x741220 / 0x741400).
- **What we do instead:**
  - We load WALKTEX ourselves (0x44C510, as puppets load character textures) the first time a
    friend's lock-on arrives, and release it when the stage changes.
  - Only while one of our marker displays runs, player 2's slots (`PLAYERS`+4 0x1DEA6E4 and
    the CharObj2 array+4 0x1DE9604, empty except in rival fights, and put back right after) point at a stand-in player whose
    CharObj2 holds that texlist. The marker's +8 is set to 1.
  - The display reads only +0x444, +0x24 and +1 of it (the model variant).
- **Model and texture variant:** taken from our zeroed stand-in, not from the friend's mech.

### Enemy owners (2026-10-18, src/enemies.rs, `Body::EnemyStates` tag 56)
- **Who runs an enemy:** an enemy (alive, not a ghost) with another party player within 600 is
  run by the game of the player nearest it.
  - **The owner** sends its position, rotation and distance at 10 Hz.
  - **The others** keep its MainSub wrapped. After its own update runs, they ease it toward the
    owner's position (half the gap a frame, jumping past 80) and copy the rotation, so it stands
    in the same place on every screen.
  - **Taking it over:** another player takes it over only when nearer by 50. Between two
    senders, the nearer one keeps it, then the lower id. A sender stops when its state isn't
    heard for 0.6 s.
- **Turning toward friends:** only where we run the enemy (owner, or nobody else near). There,
  an enemy after a friend runs with the stand-in.
  - **Its shots are real.** Deleting them at once (EnemyBulletExecutor 0x511AB0,
    EnemyLaserExecutor 0x510560, ...) hung a party stage with fakepeer on 2026-10-18 (the
    watchdog stopped at "stage update"), so it was removed.
    - **The likely cause:** an enemy keeps pointers to the shots it makes (E AI steers its
      moving laser every frame). Deleting one under it is a use after free in the game's heap.
    - **Now:** the shots fly at the friend's puppet and hit us only if we're in their way. In
      the friend's game, their copy of the enemy fires at them for real.
- **Per-type coverage:** all 18 E-enemy MainSubs in `coop.rs` ENEMIES, except E GHOST /
  E PATHGHOST (their grab writes CharObj2[0]): shared position and facing, turning, and shot
  suppression.
  - **Not frozen:** none of them is frozen like the bosses. Their state machines are inlined in
    each MainSub (no separate mode functions), so a copy still animates and attacks from its
    own update. Its attacks then aim at its own game's player 1, which is real and correct for
    that player.
  - **Not drawn:** shots at a friend aren't drawn as copies. The projectile displays weren't
    checked.

## Shared rewards (2026-10-18, src/coop.rs)
Rule: in a co-op party stage, what anyone collects or opens is gone for everyone (the kill
sync above), and each player gets its reward, once per item, through the game's own
function.

| Object | How it rewards (RE) | Shared? |
|---|---|---|
| RING / SGRING | AddRings 0x44CE10 | yes (since 2026-10-17): a ring each |
| Ring lines/circles (RINGLINE etc.) | the ring's own pickup | yes: a ring each (tag 50) |
| ITEMBOX 0x6C7C10, ITEMBOX2_B 0x6C7CE0 | item = SET scale X truncated to 0..10 (inits 0x6C7DB0 / 0x6C7F20); breaking calls icon 0x6DF440 cdecl(player, icon) then table 0xB4D120[item]+4 cdecl(box, player) (0x6C8101..0x6C811E) | yes: a friend's box, already broken here, gives us its item with those two calls for player 0 and a stand-in box of ours (Data1 at the box, no record). Not item 6 (blows up the enemies around the box: those kills come as kills) or 9 (nothing); the 1-up (2) not when the record says it was taken (flag 0x20). Score, rings, shields, speed shoes, invincibility, 1-ups all come with the item's own function |
| Upgrades (LEVUPDAI 0x6D8540) | sets its flag byte `0x1DEB300[i]` (i < 0x1E, the save's table, copied in at 0x68DBA1) and ORs its bits into the player's CharObj2+0x24 when CharObj2+1 is its character (0x6D87F9..0x6D881B; descriptor at Data2+8: +0 character, +8 bits, +0xC flag) | yes (tag 57): everyone sets the flag (kept with their save), and gets the bits now when playing that character |
| ITEMBOXAIR 0x6C8EF0, ITEMBOXBALLOON 0x6DB030, SGITEMBOX 0x6EABC0 | the same rule with their own tables: air 0xB493A0 (item in Data1+3, from the SET scale X, init 0x6C9022; break 0x6C9266), balloon 0xB19928 (+0x3C, 0x6DB136; 0x6DB28B), SG 0xB0AFA0 (+0x3C, 0x6EAC93; 0x6EADF4) | yes, as the ground boxes (`ITEM_TABLES`). The stand-in box now has a record of its own: the air box's 1-up (0x6C97F0) writes +0x30's flags without checking for none |
| KDITEMBOX 0x6E9350 | shares the air box's break (0x6C91F0) but its Data1+3 isn't the SET scale (0x6E955C sets it) | broken for everyone; its item isn't given yet |
| CONTCHAO 0x6DE3B0 (Chao box) | opened (0x6DE5E8): while the stage's Chao key isn't got (0x6E9C20: `0x174B004` != 1 and `0x1AEFF64` == 0) it lets out the **Chao key** (0x6E9D10 makes "ObjectChaoKeyExec", main 0x6E9DE0; EBX = &position, cdecl (height, 2.5)); otherwise animals (0x48ACD0, ESI = kind, cdecl (x, y, z)): four (kinds 2, 1, 2, 1, 10 around, 10 up) for the stage's first opened box (bit 0x100 of `0x1A5A32C`), one (kind 0) for the second (0x200), none after. Then its record gets 0x4|0x20 and loses alive | yes: opening is shared (BREAKABLES), and a friend's opening lets the same out here (`chao_box_reward`): the key, or the animals by the same first/second rule, for us to pick up |
| Animals (MINIMAL 0x48ADE0) | spawned by kills and boxes, collected by touching | no: they're not SET records; each game's are its own |
| HINTBOX / OMOCHAO / SCHBOX | hint text for the one who opens it | no: nothing to share (and opening isn't consumed) |
| Lost Chao, treasure pieces, emeralds | mission goals | no: see "Mission rules" (treasure: hunt.rs) |
| Chao keys | made by the Chao box (above) | yes, through the Chao box: each player picks up their own |
| GOALRING | stage clear | no: each player finishes |

## Mission rules in co-op (2026-10-18, decisions)
- **Mission 1 (clear):** each player finishes on their own goal ring.
- **Mission 2 (100 rings):** shared rings count for everyone, so the party collects together.
  Each player's own counter reaches 100.
- **Mission 3 (lost Chao):** finding the Chao clears the mission for the finder. Sharing it would
  clear it for players who never got there, so it stays per player. The Chao box and its
  animals aren't shared yet either (above).
- **Mission 4 (time limit):** the timer is per player (each game's own). A death restart keeps
  the party's kills and switches.
- **Mission 5 (hard mode):** different SET files (`_hd_`), the same for everyone in the mission.
  Everything above applies.
- **Treasure hunts:** shared through hunt.rs (one set of pieces, first claim wins).
- **Chao keys:** the Chao box makes them (0x6E9D10). When a friend opens one, ours lets out its key too, for each player to pick up.

## Stage objects with state outside the SET flags (2026-10-18 audit)
| Object | Where its state lives | Co-op |
|---|---|---|
| Switch doors and gates (SWITCH, GSWITCH2, LIGHT SW, CC SWITCH, CLEAR SW) | `SwitchFlags` 0x1DE9460 (32 bytes; doors and gates read their id's byte) | shared since 2026-10-17 (on only; set again after a restart) |
| Key doors (KEY, KEYHOLE, KEYDOOR) | key state machine in the key's Data2 | shared (`KeyUsed`) |
| Checkpoints | SET flag 0x40 and the restart point | shared |
| Breakable doors, walls and shutters (KDDOOR, BIGSHUTTER, SHUT, CONT*) | SET "alive" | shared (kills) |
| Moving platforms, elevators, rotating things (UGOKIDAI, MLIFT, ELV, MILIFT, KAITEN...) | their own timers from the stage start | not synced. Party stages start together (together.rs), so they run in step. A player who restarts gets their own timing back, like the game |
| Falling platforms (FALLASHIBA) | each falls under its own game's player and then clears "alive" (0x5DC8F9, no score) | not shared on purpose: dropping one because a friend stood on it would drop whoever stands on it here |
| Gravity switches (GRAV_SW) | the player's own gravity | not shared on purpose: they flip one player's gravity |
| Story cutscenes and scripted camera events | each game's player | not shared |

## Stage hook safety audit (2026-10-18)
**The hang.** Twice in a party Co-op stage with fakepeer, the game thread stopped for good
inside the stage update. The second time the watchdog's last mark was "enemy update
(enemies.rs)", which is at the very start of the wrapper that ran an enemy's own MainSub with
`PLAYERS[0]` swapped for a stand-in at the friend.
- **What was already ruled out:** the first fix (no more deleting the shots an enemy just fired)
  didn't stop it, and our locks can't deadlock there (the wrapper takes `ENEMIES` only around its
  bookkeeping, never across the enemy's code, and the network thread's lock order ONLINE →
  ENEMIES has no reverse).
- **Most likely cause:** the stand-in itself. It was a 0x30-byte copy of our real EntityData1
  holding the same collision pointer (+0x2C, our player's real collision info), at another
  position and under another address than any player in `PLAYERS`/`MainCharObj1`. During that
  update the enemy's code (and the shared helpers it calls with "player 1": collision checks,
  0x486520/0x4865B0/0x46DCC0-style lookups that tie a collision back to a player, the 62
  `PLAYERS[0]` reads) sees a player whose collision belongs to another entity. The game's own
  code that walks those links can then go round forever. Which helper spun wasn't pinned down
  statically.
- **Fix:** the stand-in is gone from enemies.rs. Enemies aim only at their own game's player.
  The two-player loader pass that loaded enemies around friends (which only fed the stand-in)
  is gone too.

| Hook (runs in) | What the game code we call reads and writes through ours | Pointers that outlive the call | Complete enough? Loops? | Verdict |
|---|---|---|---|---|
| enemies.rs `enemy_main` (stage update, enemy MainSub) | now only the enemy's own update, untouched; then we write its Data1 position and rotation (where its owner has it) | none | yes; no loop of ours | **kept**: position following only, and only for enemies whose owner sends `EnemyStates` |
| enemies.rs stand-in `PLAYERS[0]` while an enemy runs | everything an enemy reads of player 1 (62 reads; collision through +0x2C, which pointed at our real player's collision) | the stand-in's address (a shot or collision link can keep it) | no: a copy can't be complete (collision, CharObj2 and the player tables disagree); the hang | **removed** |
| enemies.rs / coop.rs two-player loader pass (0x4883D0, `PLAYERS[1]` stand-in at a friend) | the loader reads only the two positions | none | yes, but what it loads near a friend only stays with the stand-in above | **removed** |
| enemies.rs deleting fresh shots | — | the enemy's own pointer to its shot (E AI steers its laser) | no: use after free | **removed** (first fix) |
| boss_follow.rs `boss_main` frozen (stage update, boss MainSub) | the boss's own main with Data1[0] = a mode its switch skips: head turn, motion, inlined hit code (0x46C340 hurts *our* real player, as in the game), parts; then we write Data1 mode bytes, flags, timer, position, rotation and start a motion (0x793C40, id checked against the table) | none | yes (all three bosses' attack code is in mode functions, which don't run; the pit child is passive) | **kept** |
| boss_follow.rs leader running mode functions with a stand-in player 1 at a friend | the mode functions' player-1 reads and writes (and the cursor objects they make keep the stand-in pointer) | the stand-in (made static so it lived, but it's still a player that isn't in the player tables) | no, for the same reason as the enemies | **removed**: the boss is after the leader only |
| boss_follow.rs attack copies (list 3, our main, the game's display) | the display reads Data1 0..0x2B only (missile 0x5D6AA0, beam 0x5D9890, bomb 0x5D9CA0, pillar 0x5DA2D0, smoke 0x5D6BA0, flame 0x5D5730) plus static models and textures; Data1 from the game's allocator | none (freed by the game's DeleteObject; our delete callback forgets it) | yes; no loops | **kept** |
| lockon.rs cursors and lasers (list 3, our main, the game's marker displays 0x74BEC0 / 0x74B620) | display-only reads: marker +8/+0xE/+0x10/+0x1C, the stand-in target's Data1+2, +0x2C → +0xC → +4, `MainCharObj1[n]`+0x40 → +0x444, `CharObj2s[n]`+0x24/+1; the marker chain via +0/+4 (0 here) | player-2 slots swapped only during our call and put back; the target stand-in is static | yes (every field the displays read is set); no writes; the chain is empty | **kept** |
| coop.rs item-box rewards (OnFrame, not the stage update) | the reward function with a stand-in box (Data1 0x30, +1 = 0, a record of its own at +0x30 for the 1-up) and player 0 (us) | the box isn't kept (rewards take the player index; the blast item that uses the box's position is skipped) | yes | **kept** |
| coop.rs Chao box `let_out_key` / `let_out_animal` (OnFrame) | the key maker copies the position (0x6E9D4D..); the animal maker takes floats | none | yes | **kept** |
| shots.rs friends' walker shots (OnFrame) | the creators copy position and direction | none | yes | kept (unchanged) |
| puppet.rs display, skins.rs / hide.rs draw replacements (display pass) | draw-only, unchanged this round | — | — | unchanged (earlier tests in stages) |

- **Safety switch:** config.ini `StageSync=Off` (shown as "Shared enemies and bosses" in the
  mod settings) turns off enemies.rs, boss_follow.rs and lockon.rs entirely. Kills, rings,
  boxes and switches stay shared.
