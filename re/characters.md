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
**Correction 2026-10-16:** everything above is Eggman's walker (0x7444F0 / 0x744020); Tails'
walker has its own display 0x747DD0 — see "Drawing other players' walkers" below.
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

## Drawing other players' walkers (2026-10-16, from the dumped exe + the two lvl9 crash dumps)
**The bug: 0x7444F0 is Eggman's walker display only.** Every walker has its own display, set by
its loader on the ObjectMaster (+0x14 display, +0x1C delayed display, +0x18 delete 0x743E90 for all):
| Walker | loader | co2 +1 / +3 | display | bone callback | delayed (+0x1C) | files (tex / mdl / mtn) |
|---|---|---|---|---|---|---|
| Eggman 7 | 0x740C50 | 7 / 7 | **0x7444F0** | 0x744020 | 0x7454B0 | EWALKTEX / EWALKMDL / EWALKMTN |
| Tails 6 | 0x740EB0 | 6 / 6 | **0x747DD0** | 0x747940 | 0x748CA0 | TWALKTEX / TWALKMDL / TWALKMTN |
| Chao Walker (Tails 2P alt) | 0x741110 | 6 / **0xD** | 0x746290 | (own) | 0x746850 | CWALK* |
| Dark Chao Walker (Eggman 2P alt) | 0x7412F0 | 7 / **0xE** | 0x7470D0 | (own) | 0x7476A0 | DWALK* |
The alt loaders run instead when `0x174AFDE` (2P mode) and `0x174B015[player]`; EWALK2/TWALK1
(`0x174B013[player] == 1`, co2+2 = 1) are the other 2P costumes. All four allocate a 0x458-byte
CharObj2 and keep the body texlist at **+0x448** (also the Chao walkers; the slot table above saying
0x444 for 10/11 is wrong), WALKTEX at +0x444 (only loaded/freed, no display reads it), model list
+0x44C, motion list +0x450. Note: co2+3 (`CO2_CHAR_ID`) is 0xD/0xE for the Chao walkers, not 10/11
(0xD collides with the mod's "Chaos 13"); only reachable in 2P costumes.

**Both lvl9 crashes = Tails' walker (char 6) drawn with Eggman's display**, whose part models
are model-table slots only EWALKMDL fills (the table is filled once per stage by the loader's
0x459590 call; nothing per-frame sets them, and a Tails stage never loads EWALKMDL):
1. 0x7448FE..0x7449C3: if `co2+0x24 & 0x400000` use model 260 (0x1DE9E44) else model 258
   (0x1DE9E34); the chosen pointer is stored to the local `[esp+0x10]` **only if the slot is
   non-null**. Slot 258 empty → the local stays uninitialised stack garbage, the null test at
   0x7449F3 passes, slot 259 (0x1DE9E3C, local `[esp+0x1C]`) is also 0 → 0x744ACA
   `fld [garbage+0x10]`. Dump: `[esp+0x10]=0xA56F6168`, `[esp+0x1C]=0`, `[esp+0xC]`=our co2 copy.
2. Same function when the garbage happened to end the loop: 0x744DF3 `co2+0x24 & 0x200000 &&
   co2+3 == 7` → models 263/262 (0x1DE9E5C/54), else 266/265 (0x1DE9E74/6C); Tails' co2+3 is 6 →
   slot 266 (empty) → 0x42EB30(NULL) at 0x744E6A.
Nothing else in the crash is state "only the player's update sets up": the display reads only
obj+0x34 (Data1) and obj+0x40 (Data2) from the object.

### What a walker display reads (0x7444F0 and 0x747DD0 have the same shape)
- Object: +0x34 EntityData1, +0x40 CharObj2. Nothing else.
- EntityData1: +0 action (0x33: also reads `[e+0x2C]->+0xC` collision and co2+0x134; 0x36 with
  co2+0xBC != 0: draws co2+0xBC's motion — puppets use action 0), +6 bit 1 (invisible → skipped
  unless 0x174B009), rotation +8/+0xC/+0x10, position +0x14, **scale +0x20/+0x24/+0x28**
  (0x427750; zero scale = invisible).
- CharObj2 reads: +0 player number (indexes 0x1DE4664[], light 0x486E50, viewport test vs
  0x174B009), +2 (Eggman: ==1 skips the part loop), +3, +0x24 upgrades, +0x174 mode, +0x178 anim,
  +0x184 frame, +0x198 AnimInfo (body = ModelTable[AnimInfo[anim].model], motion =
  MotionTable 0x1DEA704[AnimInfo[anim].motion]), +0x19C only when mode == 2, +0x374/+0x378
  effect fades (0 → skipped), +0x424/+0x42C/+0x430/+0x434 part angles (ints, fed to 0x42AC30 and
  written into the part NJS_OBJECTs' child ang/pos every draw — any value is safe), +0x448 texlist.
- CharObj2 writes (harmless on a private copy): +0x200..+0x2FC (callback bone positions),
  +0x23C, +0x248, +0x2D8..+0x2E4, +0x438..+0x440, Tails also +0x3A0..+0x3F4.
- Model slots dereferenced **without a null check**:
  Eggman 0x7444F0: 258 or 260 (by bit 0x400000), 262+263 (0x200000 && co2+3==7) or 265+266,
  268 (anim 0x80, with AnimInfo[0x81] motion), 274/276 (0x100000), 278/291 (0x800000),
  287 + motion slot 130 (0x1DEAB14) when co2+0x378 > 0. Its delayed display 0x7454B0: 279..286, 462.
  Tails 0x747DD0: 306 (0x1DE9FB4, bit 0x100) or 304 (0x1DE9FA4), 309 (bit 0x80) or 312, 313 (anim
  0xB4, with AnimInfo[0xB5] motion), 319 + 321 (bit 0x40), 325 (co2+0x374 > 0); 303/305 are only
  compared / null-tested. Delayed 0x748CA0: 317 (only anim 0xB3).
  Chao 0x746290: 451, 458; Dark 0x7470D0: 474, 481.
  Bone callbacks only compare bone pointers against slots (null never matches) — safe.
- Globals written (save/restore around a puppet call): 0x1A51D00..**0x1A52150** (bone matrices
  0x1A51D2C..0x1A52120 + drawing co2 0x1A51F0C + view copy 0x1A51F70; Tails' callback writes
  0x1A520F0..0x1A52120 and the Chao walkers 0x1A52120..0x1A52150, past the old 0x1A52100 end),
  0x1A557FC..0x1A5584C (matrix top, 0x1A55808, bone callback 0x1A55834, 0x1A55838),
  0x1DE4660..0x1DE4668, 0x1DEB6A0..0x1DEB6AC (its saved render flags), fog 0x25EFFCC..0x25EFFDC.
  It restores 0x25F02D4/0x25F02D8/0x25F0268 itself. The delayed displays read the bone matrices,
  so they must be restored before the game's own delayed pass (they are only read by the
  display/callback/delayed code: 0x744020..0x7458EF, 0x746290..0x748FFF; walker main 0x741530 doesn't).
- 0x420510 (light from 0x19341C0..C8) is called only for the cockpit model when 0x1A55808 == 0 —
  plain globals, safe.

### Slot ranges per walker (decoded from gd_PC/*.prs, big-endian {id, offset} lists)
| file | model slots | motion slots |
|---|---|---|
| EWALKMDL / EWALKMTN | 248..292 (no 252/255), 462, 463, **267** | 87..137 (33 nodes) |
| TWALKMDL / TWALKMTN | 293..327 (no 297/300/301), 464, **267** | 139..188 (33 nodes) |
| CWALKMDL / CWALKMTN | 442..460, 267, 283 | 88..127, 133..135, 190..197 |
| DWALKMDL / DWALKMTN | 461, 465..482, 267, 283 | 88..127, 133..135, 190..201 |
Bodies: Eggman 248 (293 for anims 139..181 — Tails' body index, presumably a table quirk),
Tails 293, Chao 450, Dark 473. Eggman's and Tails' slots are disjoint except 267 (a common part
both files carry; first loaded wins, neither display uses it). This is how 2P battle has both
walkers: each player's loader fills its own disjoint slots. The Chao walkers share Eggman's
motion slots 88..135 (same 33-node skeleton, so a mismatch only looks wrong, never overruns).

### Recipe (recommended: call the right display with a private CharObj2)
1. Pick the display by **the puppet's** walker, never ours: 6 → 0x747DD0, 7 → 0x7444F0
   (10 → 0x746290, 11 → 0x7470D0). cdecl `void(ObjectMaster*)`.
2. Assets: same type as ours → nothing to load. Other type → load its TEX/MDL/MTN with the
   existing other-character loader (`character_files` 6/7 already name them; texlist goes to the
   private co2+0x448). Release at stage end as for other characters.
3. Preflight (every frame, cheap): body model and motion for the anim non-null (`resolve`), and
   **every unguarded slot listed above for that display non-null** (for Eggman: 258, 259, 260,
   262, 263, 265, 266, 268, 274, 276, 278, 287, 291 + motion 130; for Tails: 304, 306, 309, 312,
   313, 319, 321, 325; also require 259 / 305, which the loops test). Any null → draw body-only with `draw()` (texlist +0x448) instead.
4. Build a zeroed 0x458(+8)-byte CharObj2 (don't copy ours: different layouts/meanings per
   type, and ours may not be a walker): +0 = our player number (0), +1 = 6/7, +2 = 0, +3 = 6/7,
   +0x24 = remote upgrades, +0x174 = 0, +0x176 = +0x178 = anim, +0x184 = frame, +0x198 = that
   walker's AnimInfo (0xA0EAA0 / 0xA0E1F0), +0x448 = its texlist. Leave +0x374/+0x378/+0x424..+0x434
   at 0. (A copy of our own co2 is also fine when the type matches; then still force +2 = 0.)
5. Zeroed 0x40-byte EntityData1: action 0, status 0, rotation, position, scale = 1.0 x3.
6. Fake 0x50-byte object with +0x34 = entity, +0x40 = co2. Save the globals above
   (0x1A51D00 len 0x450 etc.) and the render texlist, call, restore, `refresh_fog`.
7. Optional: the delayed display (+0x1C column) right after, before restoring — it draws the
   transparent parts from the matrices just computed (Eggman's needs slots 279..286, 462).
Confidence: high for the display table, both crash causes and slot lists; medium that nothing
else (e.g. 0x42FD10's per-character effect textures, 0x173B3D8[char]) is needed for a different
walker type — the display itself doesn't read it.
