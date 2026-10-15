# Player characters (for M3 puppets)

Source: Ghidra 12.1.4 on `re/bin/sonic2app.unpacked.exe` (sha256 c95c0143…), 2026-10-14.
Addresses are VAs (image base 0x400000), valid for the running game.

## LoadSonic 0x716E00 — `void LoadSonic(int playerNum)`
Found via the only reference to "SONICMDL.PRS" (0x900968, used at 0x716F6F). Matches the
community header address.

- `playerNum` is 0 or 1 and is stored as byte 0 of the character work (CharObj2). The game's
  per-player arrays (MainCharObj1/2 etc.) have **2** entries, so the real loader can't be used
  for 7 remote players — puppets need their own object that only draws.
- Creates the object with FUN_0046F680("SonicTheHedgehog") (object allocator taking a name),
  then allocates three data blocks (FUN_00470B40 → obj+0x34, FUN_00470BA0 → obj+0x3C,
  FUN_00470B70 → obj+0x38; any failure frees the object via FUN_0046F720).
- Character work: 0x3A4-byte alloc (`..\..\src\figure\sonic\sonic.c`, line 0x64B), magic
  0x12345678 header then 0x3A0 bytes, stored at obj+0x40.
- Callbacks set on the object: +0x14 = FUN_00720090, +0x18 = 0x71E300, +0x1C = 0x720B50,
  +0x2C = FUN_0071E520 (roles to confirm against the ObjectMaster layout).
- Model/motion setup: FUN_0042FD10(PTR_PTR_0173B3D8[alt]), FUN_00459740, FUN_00447580(...),
  FUN_0075A800(playerNum) (probably registers the player slot).
- Globals read: 0x174AFDE (mode flag; when set and 0x174B015[player] != 0 → FUN_007170E0
  instead, likely the alternate-costume/2P path), 0x174B013[player], 0x174AFC0,
  CurrentLevel 0x1934B70 (== 0x13 or 0x2A with player 1 clears a work flag).

## Plan for puppets
1. Find the draw path: decompile the display callbacks (0x71E300, 0x71E520) to see how the
   model + current animation frame are drawn from CharObj2/EntityData1.
2. Write a puppet object in the mod: allocate with the game's allocator, reuse the model and
   motion data the local character already loaded (same character) or load the other
   character's MDL/MTN once per stage, and draw at the networked position/rotation/anim.
3. Start with one puppet of the *same* character as the local player in one stage.

## Sonic draw callback FUN_0071E520 (obj+0x2C) — how a character is drawn
`void(ObjectMaster* obj)`; `e1 = obj+0x34` (EntityData1), `co2 = obj+0x40` (CharObj2).
(0x71E300 at obj+0x18 is not a Ghidra function yet — only referenced as data.)

1. Shadow/ground probe first (FUN_0077FC30, FUN_00495A90(0x80, &e1->Position, co2+0x32C, …)).
2. Skipped when `e1[6] & 2` (invisible flag) unless 0x174B009 (2P split-screen viewport?) is set.
3. **Matrix push**: matrix stack pointer at 0x1A557FC, limit 0x2670588, base 0x267053C;
   entries are 0x30 bytes (3×4 floats); push = copy 12 dwords to `ptr+0x30`.
4. **Translate** FUN_00427470(x, y, z) with `e1->Position` (+0x14/+0x18/+0x1C).
5. **Rotate** Z (+0x10) FUN_00427680, X (+0x08) FUN_004274E0, Y FUN_004275B0(0x8000 − rot.y (+0x0C)).
6. **Scale** FUN_00427750(e1+0x20, +0x24, +0x28).
7. **Draw** FUN_00780870(model, co2+0x184):
   `model = CharacterModels[AnimInfo[anim].model].model`, where
   `anim = *(short*)(co2+0x178)`, `AnimInfo = *(ptr*)(co2+0x198)` (0x10-byte entries,
   model index at +2), CharacterModels = 0x1DE9624 (8-byte entries, model at +0).
   Swaps in 0x1DE97A4 / 0x1DE99FC when present (alternate body models), and uses
   AnimInfo entry 0x1E2/0x10 in a special state. `co2+0x184` = animation/motion state (TBD).
8. **Matrix pop** (`ptr -= 0x30`, not below base), then FUN_00495D20 draws the shadow.

So a puppet can be drawn with: push matrix → translate/rotate/scale from the network state →
FUN_00780870(model for the remote animation, motion state) → pop. Still needed:
- what `co2+0x184` points to (motion + frame), so we can build one per puppet;
- the object allocator FUN_0046F680 signature (to register our own display callback);
- whether models for *other* characters are loaded in a stage (else load MDL/MTN ourselves).

## Object allocator FUN_0046F680 (`task.c`) — custom calling convention
`ObjectMaster* LoadObject(name /*stack*/)` with **ESI = list index** (0..6; 7 = not linked)
and **EDI = main callback** (NULL → default FUN_006FE430). Needs inline asm from Rust.
- Allocates 0x54 bytes (magic 0x12345678 + 0x50-byte object); returns the object.
- Layout (offsets from the returned pointer): +0x00 next, +0x04 prev (circular list per
  index, heads at 0x1A5A254[index]), +0x10 main, +0x14..+0x30 zeroed (more callbacks —
  LoadSonic sets +0x14, +0x18, +0x1C, +0x2C), +0x34..+0x40 data pointers (zeroed),
  +0x44 name, +0x4C zeroed.
- Free: FUN_0046F720(obj) (used by LoadSonic on failure).

## Draw helper FUN_00780870(model, motionState)
Sets 0x25EFE54 = 0x25EFE60, FUN_007819A0(arg2), 0x1D19C0C = FUN_0042E660,
FUN_00782420(model). Plain stack args.
FUN_007819A0 calls FUN_0042C240(arg2) and then only picks render callbacks from flag bits
at 0x25EFE54+0x20 — so `arg2` (Sonic's `co2+0x184`) is most likely the **texture list**,
not a motion. So FUN_00780870 = "draw chunk model with texlist" in its *current pose*.
Open question: where the animation pose is applied (candidates: the other display
callbacks FUN_00720090 / 0x720B50, or a per-frame motion step in the main callback).

## Main draw callback FUN_00720090 (obj+0x14) — correction to the above
Same matrix/translate/rotate/scale sequence, then FUN_00780870(model, co2+0x184), plus
attachments (shoes/hands/etc. from 0x1DE96EC, 0x1DE96F4, 0x1DE96FC, 0x1DE9694…).
Here `*(float*)(co2+0x184)` is used as a **float animation frame** (`frame - 1.0` divided by
the motion's frame count), so `co2+0x184` = current frame, not a texlist.
- AnimInfo entry (0x10 bytes, table at `*(co2+0x198)`): +0 motion index (short),
  +2 model index (short). Current animation index = `*(short*)(co2+0x178)`.
- Motions: 0x1DEA704 table, 8-byte entries, `entry → NJS_MOTION*` with `+4 = nbFrame`.
- Models: 0x1DE9624 table, 8-byte entries, `entry → NJS_OBJECT*` (0x1DE96CC = character
  root model used for some lookups; 0x1DE98D4 alternate).
- 0x1A55834 is set to a per-character callback while drawing (FUN_0071F040 for type 8,
  FUN_0071FBE0 for 0xC, else 0x71EAA0 / FUN_0071F5E0) — likely per-bone hook (shoes, eyes).
- Still unknown: where the NJS_MOTION for the current anim is bound for FUN_00780870
  (likely FUN_0042C240 inside FUN_007819A0 or a global set by FUN_00486E50).

### What a puppet needs (current best understanding)
Per remote player per frame: position, rotation (x/y/z angles), animation index, frame
(float). Draw = matrix push → translate/rotate → FUN_00780870(model for anim, frame) with that
anim's motion bound → pop. The wire PlayerState already has position/rot_y/anim; it needs
rot x/z and the float frame added (protocol v2).

## Confirmed for puppets (assembly-level, 2026-10-14)
- **Player registration** FUN_004599C0 (`player.c`; EAX = object, ECX = player index):
  0x1DEA6E0[i] = player ObjectMaster, 0x1DEA6C0[i] = EntityData1, 0x1DE95E0[i] = obj+0x38,
  **0x1DE9600[i] = CharObj2** (not 0x1DEA6C8). CharObj2: +0 player index, +1 variant,
  +3 CharID (indexes the 0x84-byte physics table 0x17391E8), +0x178 anim (short),
  +0x184 frame (float), +0x198 AnimInfo table, +0x394 texlist (set from LoadSonic).
- **LoadObject** 0x46F680: name on stack (caller pops, plain RET), ESI = list (Sonic uses 1),
  EDI = main (0 → 0x6FE430, which is a bare RET). Callbacks: +0x10 main, +0x14 display
  (Sonic 0x720090, plain RET → cdecl(obj)), +0x18 delete, +0x1C, +0x2C (Sonic shadow/2nd pass).
- **FreeObject** 0x46F720: calls `obj+0x18(obj)`, unlinks, and **frees obj+0x34/+0x38/+0x3C/+0x40**
  through the game allocator. Never store our own pointers there.
- **DrawMotion** 0x780870: `ECX = NJS_MOTION*`, cdecl(NJS_OBJECT* model, float frame); preserves ECX.
  FUN_0042C240 = set motion (ECX motion, frame arg). NJS_MOTION: +0 mdata, +4 nbFrame, +8 type, +0xA inp_fn.
- **Sonic's motion choice** (0x720656): `motion = MotionTable[AnimInfo[AnimInfo[anim].AnimNum].AnimNum]`
  (MotionTable 0x1DEA704, 8-byte entries, pointer at +0); special state `co2+0x174 == 2` uses
  `*(co2+0x19C)` instead. Model = ModelTable[AnimInfo[anim].ModelNum] (0x1DE9624).
- **Matrix helpers** (hidden register args): translate 0x427470 (EAX = matrix, stack x,y,z, caller
  pops 12); rotZ 0x427680 / rotX 0x4274E0 / rotY 0x4275B0 (EBX = matrix, stack angle, caller pops 4);
  Sonic rotates Z, X, then Y by `0x8000 - rot.y`. Matrix stack: current ptr at 0x1A557FC, limit ptr
  at 0x2670588, base ptr at 0x267053C, 0x30-byte entries.
- Before drawing Sonic sets `*(*(0x2670544) + 0x20) = co2+0x394` (texlist for chunk models) and
  0x1A55834 = per-bone callback (cleared to 0 afterwards).

## Spin ball (confirmed in game: missing until handled, 2026-10-14)
Player display 0x72060E: unless `co2+0x174 == 2`, when `EntityData1+4 (u16) & 0x100` and
`co2+3 != 0x0C` and `co2+0x360 & 0x11`, it draws **AnimInfo[30]'s model** (`AnimInfo + 0x1E2`)
with the current motion and skips attachments. Puppets send this as `flags` bit 0.

## Texture-list slot per character (2026-10-14)
Found by scanning loaders for `MOV ECX,tex; CALL 0x44C510; MOV [reg+slot],EAX; MOV EAX,mdl`
(tools/texslots.py) and confirming the display code reads the same slot into
`*(*(0x2670544)+0x20)`:
| Characters (CharID) | slot | display / loader |
|---|---|---|
| Sonic 0, Shadow 1, Amy 8, Metal Sonic 9 (+ alt costumes) | 0x394 | 0x720090 / 0x716E00 |
| Knuckles 4, Rouge 5, Tikal 12, Chaos 13 (+ alt costumes) | 0x3F0 | 0x72EF20 / 0x728110 |
| Tails on foot 2 | 0x3B0 | draw at 0x74FF81 doesn't use the shared model table → not drawn yet |
| Eggman on foot 3 | 0x360 | draw at 0x73E901, same as Tails → not drawn yet |
| Mechs 6/7 (EWALK/TWALK) | 0x448 (+ WALKTEX 0x444) | several models → not drawn yet |
| Chao walkers 10/11 | 0x444 | not drawn yet |

### Walker extra parts (2026-10-15, from a code dump: `--features dumpexe` + tools/rebuild_exe.py)
Walker display (around 0x7445A1) stores the drawing CharObj2 at 0x1A51F0C, copies the view
matrix to 0x1A51F70 and sets the per-bone callback 0x1A55834 = **0x744020** before drawing the
body. The callback compares each bone's NJS_OBJECT with model-table entries 247, 248, 249, 251
(0x1DE9DEC..0x1DE9E0C) and **writes** those bones' world positions into the CharObj2
(+0x200, +0x218, +0x230, +0x260, +0x29C, +0x2B4, +0x2CC, +0x2FC) and saved matrices
(0x1A52090, 0x1A520C0): gameplay data (aim, effects), so puppets must not run it with the
local player's CharObj2. After the body (0x7448C3..) the display draws the extra parts from
those matrices: models 258/259/260 (0x1DE9E34/3C/44) with textures from CharObj2+0x424/+0x42C/
+0x434 (WALKTEX), gated by upgrade bits (e.g. `[co2+0x24] & 0x400000`). **Puppets (2026-10-15):** a
puppet of the same walker as ours is drawn by calling 0x7444F0 itself with a private copy of our
CharObj2 (0x458 bytes, the loader's allocation at 0x740CD9) carrying the puppet's animation,
frame and upgrades (sent in `PlayerState.upgrades`), and a private EntityData1 (action 0, its
angles and position). The globals the display and callback write (0x1A51D00..0x1A52100,
0x1A557FC..0x1A5584C, 0x1DE4660..0x1DE4668, 0x1DEB6A0..0x1DEB6B0, fog 0x25EFFCC..0x25EFFE0)
are saved and restored around the call. A walker different from ours still draws its body only
(our CharObj2 would carry the wrong models/textures).
Knuckles' display (0x72F449) uses the same motion double lookup as Sonic, model from the
shared table, and `co2+0x19C` as the motion when `co2+0x174 == 2`.
Puppets still reuse the *local* player's loaded assets, so only same-character players show.

## Animation state machine (FUN_004692A0, ESI = co2+0x174) — 2026-10-14
`co2+0x174` is a block driven by FUN_004692A0 each frame:
+0x00 (0x174) state: 0 init, 1 start transition, **2 blending** into the new anim, else the
playback mode from AnimInfo+4; +0x02 (0x176) requested anim; +0x04 (0x178) current anim;
+0x06 (0x17A) previous anim; +0x10 (0x184) frame (during state 2: blend progress 0..1);
+0x24 (0x198) AnimInfo*; +0x28 (0x19C) per-player blend buffer whose first dword is a runtime-built
NJS_MOTION (FUN_00469D20). The display uses that blend motion while state == 2.
Puppets were invisible during every blend (constant in Chao World: walk/run/stand switches).
Fix: draw the target anim at frame 0 while blending (POSE_BLENDING).
AnimInfo entry: +0 motion, +2 model, +4 next state/mode, +6 next anim, +8 blend speed, +0xC speed.

## Loading other characters for puppets (2026-10-14)
Loaders (all take the game's own filename strings):
- texlist: 0x44C510, ECX = name (no extension) → texlist; release 0x77F9F0(texlist) cdecl + free the
  block via `(*0x1D19CAC)[1](block-4, file, line)` with tag 0x89ABCDEF (as Sonic's delete 0x71E300).
- models: 0x459590, EAX = file → list of {index, NJS_OBJECT*}; fills ONLY empty ModelTable slots.
  Release 0x4596D0 (ESI = list) clears only slots still pointing at its own models, then frees.
- motions: 0x459740, EAX = file → list of {u16 index, u16 count, NJS_MOTION*}; fills only empty
  MotionTable slots. Release 0x459890 (ESI = list), same "only my own slots" rule.
So loading another character mid-stage and releasing at stage end never disturbs the local player.
Per-character files/anim tables (tools/charassets.py): see `character_files` in puppet.rs
(Sonic 0, Shadow 1, Amy 8, Metal Sonic 9, Knuckles 4, Rouge 5, Tikal 12, Chaos 13).
