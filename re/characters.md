# Player characters: objects, drawing, assets (puppets)

Addresses are VAs (image base 0x400000), valid for the running game.

## Objects

**LoadObject 0x46F680** (`task.c`), custom calling convention: name on the stack (caller pops,
plain RET), **ESI = list index** (0..6; 7 = not linked; Sonic uses 1), **EDI = main callback**
(0 → 0x6FE430, a bare RET).
- Allocates 0x54 bytes (magic 0x12345678 + 0x50-byte object) and returns the object.
- Layout: +0x00 next, +0x04 prev (circular list per index, heads at 0x1A5A254[index]), +0x10
  main, +0x14 display, +0x18 delete, +0x1C delayed display, +0x2C a second display pass
  (Sonic: shadow), +0x34..+0x40 data pointers (+0x34 EntityData1, +0x38, +0x3C, +0x40
  CharObj2/Data2), +0x44 name, +0x4C zeroed.

**FreeObject 0x46F720:** calls `obj+0x18(obj)`, unlinks, and **frees obj+0x34/+0x38/+0x3C/+0x40**
through the game allocator, so these slots must only hold game-allocated blocks.

**Player registration 0x4599C0** (`player.c`; EAX = object, ECX = player index):
0x1DEA6E0[i] = player ObjectMaster, 0x1DEA6C0[i] = EntityData1, 0x1DE95E0[i] = obj+0x38,
**0x1DE9600[i] = CharObj2**. The per-player arrays have **2** entries, so the game's loaders
can't create more players; puppets are objects of the mod's own that only draw.

## LoadSonic 0x716E00 — `void LoadSonic(int playerNum)`

- References "SONICMDL.PRS" (0x900968, used at 0x716F6F).
- `playerNum` is 0 or 1 and is stored as byte 0 of the CharObj2.
- Creates the object with LoadObject("SonicTheHedgehog"), then allocates three data blocks
  (0x470B40 → obj+0x34, 0x470BA0 → obj+0x3C, 0x470B70 → obj+0x38; any failure frees the object
  via 0x46F720).
- CharObj2: 0x3A4-byte alloc (`..\..\src\figure\sonic\sonic.c`, line 0x64B), magic 0x12345678
  header then 0x3A0 bytes, stored at obj+0x40.
- Callbacks: +0x14 display 0x720090, +0x18 delete 0x71E300, +0x1C 0x720B50, +0x2C 0x71E520
  (shadow / second pass).
- Model/motion setup: 0x42FD10(0x173B3D8[alt]), 0x459740, 0x447580(...), 0x75A800(playerNum).
- Globals read: 0x174AFDE (2P mode; with 0x174B015[player] != 0 → 0x7170E0 instead, the
  alternate-costume path), 0x174B013[player], 0x174AFC0, CurrentLevel 0x1934B70 (0x13 or 0x2A
  with player 1 clears a work flag).

## CharObj2 fields used for drawing

+0 player index, +1 variant, +2 costume, +3 CharID (indexes the 0x84-byte physics table
0x17391E8), +0x24 upgrades, +0x174 animation block (below), +0x394 texlist (Sonic family; see
the slot table).

**Animation block** (driven each frame by 0x4692A0, ESI = co2+0x174):
- +0x174 state: 0 init, 1 start transition, **2 blending** into the new anim, else the playback
  mode from AnimInfo+4;
- +0x176 requested anim; +0x178 current anim; +0x17A previous anim;
- +0x184 frame (float; during state 2 the blend progress 0..1);
- +0x198 AnimInfo table; +0x19C per-player blend buffer whose first dword is a runtime-built
  NJS_MOTION (0x469D20). The display uses that blend motion while state == 2.

**AnimInfo entry** (0x10 bytes): +0 motion index, +2 model index, +4 next state/mode, +6 next
anim, +8 blend speed, +0xC speed.

**Tables:** models 0x1DE9624 (8-byte entries, `NJS_OBJECT*` at +0; 0x1DE96CC the character root
model, 0x1DE98D4 an alternate), motions 0x1DEA704 (8-byte entries, `NJS_MOTION*` at +0).
NJS_MOTION: +0 mdata, +4 nbFrame, +8 type, +0xA inp_fn.

## How a character is drawn (Sonic's display 0x720090)

1. Shadow/ground probe (0x77FC30, 0x495A90(0x80, &e1->Position, co2+0x32C, …)) in the second
   pass 0x71E520.
2. Skipped when `EntityData1+6 & 2` (invisible) unless 0x174B009 (2P split-screen viewport).
3. **Matrix push:** current pointer at 0x1A557FC, limit pointer 0x2670588, base pointer
   0x267053C; 0x30-byte entries (3×4 floats); push = copy 12 dwords to `ptr+0x30`.
4. **Translate** 0x427470 (EAX = matrix, stack x,y,z, caller pops 12) with EntityData1 position
   (+0x14/+0x18/+0x1C).
5. **Rotate** Z (+0x10) 0x427680, X (+0x08) 0x4274E0, Y 0x4275B0 by `0x8000 − rot.y` (+0x0C)
   (EBX = matrix, stack angle, caller pops 4).
6. **Scale** 0x427750(+0x20, +0x24, +0x28).
7. Sets the texlist `*(*(0x2670544) + 0x20) = co2+0x394` and the per-bone callback 0x1A55834
   (0x71F040 for type 8, 0x71FBE0 for 0xC, else 0x71EAA0 / 0x71F5E0; cleared afterwards).
8. **Motion choice** (0x720656): `motion = MotionTable[AnimInfo[AnimInfo[anim].motion].motion]`;
   when `co2+0x174 == 2` it uses `*(co2+0x19C)` instead. Model = ModelTable[AnimInfo[anim].model];
   0x1DE97A4 / 0x1DE99FC swap in alternate body models when present.
9. **DrawMotion 0x780870:** `ECX = NJS_MOTION*`, cdecl(NJS_OBJECT* model, float frame); preserves
   ECX. The frame is used as `(frame − 1.0)` over the motion's frame count. 0x42C240 sets the
   motion (ECX motion, frame arg).
10. Attachments (shoes/hands, from 0x1DE96EC, 0x1DE96F4, 0x1DE96FC, 0x1DE9694…).
11. **Matrix pop** (`ptr -= 0x30`, not below base); 0x495D20 draws the shadow.

Knuckles' display (0x72F449) uses the same motion double lookup, model from the shared table,
and `co2+0x19C` as the motion when `co2+0x174 == 2`.

A puppet is drawn the same way from network state: position, rotation x/y/z, animation index
and float frame. While the game blends between animations (state 2) a puppet draws the target
animation at frame 0 (`POSE_BLENDING`), since the blend motion only exists in the sender's game.

## Spin ball

Player display 0x72060E: unless `co2+0x174 == 2`, when `EntityData1+4 (u16) & 0x100` and
`co2+3 != 0x0C` and `co2+0x360 & 0x11`, it draws **AnimInfo[30]'s model** (`AnimInfo + 0x1E2`)
with the current motion and skips attachments. Puppets send this as `flags` bit 0. See
`spin-ball.md`.

## Texture-list slot per character

Found by matching loaders' `MOV ECX,tex; CALL 0x44C510; MOV [reg+slot],EAX; MOV EAX,mdl`
(tools/texslots.py) with the display reading the same slot into `*(*(0x2670544)+0x20)`:

| Characters (CharID) | slot | display / loader |
|---|---|---|
| Sonic 0, Shadow 1, Amy 8, Metal Sonic 9 (+ alt costumes) | 0x394 | 0x720090 / 0x716E00 |
| Knuckles 4, Rouge 5, Tikal 12, Chaos 13 (+ alt costumes) | 0x3F0 | 0x72EF20 / 0x728110 |
| Tails on foot 2 | 0x3B0 | draw at 0x74FF81 doesn't use the shared model table |
| Eggman on foot 3 | 0x360 | draw at 0x73E901, as Tails |
| Walkers 6/7 and Chao walkers | 0x448 (+ WALKTEX 0x444) | see below |

## Loading other characters' assets

Loaders (all take the game's own filename strings):
- **Texlist:** 0x44C510, ECX = name (no extension) → texlist. Release with 0x77F9F0(texlist)
  cdecl, then free the block via `(*0x1D19CAC)[1](block-4, file, line)` with tag 0x89ABCDEF (as
  Sonic's delete 0x71E300 does).
- **Models:** 0x459590, EAX = file → list of {index, NJS_OBJECT*}; fills **only empty**
  ModelTable slots. Release 0x4596D0 (ESI = list) clears only slots still pointing at its own
  models, then frees.
- **Motions:** 0x459740, EAX = file → list of {u16 index, u16 count, NJS_MOTION*}; fills only
  empty MotionTable slots. Release 0x459890 (ESI = list), same rule.
- The shared player motions (PLCOMMTN: walking, standing...) are loaded once for every character
  (list at 0x1A5A240).

So loading another character mid-stage and releasing it at stage end never disturbs the local
player. Per-character files and anim tables are in `character_files` in `src/puppet.rs`
(tools/charassets.py).

## Walkers (mechs)

Every walker has its own display, set by its loader on the ObjectMaster (+0x14 display, +0x1C
delayed display, +0x18 delete 0x743E90 for all):

| Walker | loader | co2 +1 / +3 | display | bone callback | delayed (+0x1C) | files (tex / mdl / mtn) |
|---|---|---|---|---|---|---|
| Eggman 7 | 0x740C50 | 7 / 7 | **0x7444F0** | 0x744020 | 0x7454B0 | EWALKTEX / EWALKMDL / EWALKMTN |
| Tails 6 | 0x740EB0 | 6 / 6 | **0x747DD0** | 0x747940 | 0x748CA0 | TWALKTEX / TWALKMDL / TWALKMTN |
| Chao Walker (Tails 2P alt) | 0x741110 | 6 / **0xD** | **0x746290** | 0x745CC0 | 0x746850 | CWALKTEX / CWALKMDL / CWALKMTN |
| Dark Chao Walker (Eggman 2P alt) | 0x7412F0 | 7 / **0xE** | **0x7470D0** | 0x746B00 | 0x7476A0 | DWALKTEX / DWALKMDL / DWALKMTN |

- AnimInfo (co2+0x198): Eggman 0xA0E1F0, Tails 0xA0EAA0, Chao 0xA0F680 (0xC6 entries), Dark
  0xA102E0.
- The alt loaders run instead when `0x174AFDE` (2P mode) and `0x174B015[player]`; EWALK2/TWALK1
  (`0x174B013[player] == 1`, co2+2 = 1) are the other 2P costumes.
- All four allocate a 0x458-byte CharObj2 (0x740CD9) and keep the body texlist at **+0x448**,
  WALKTEX at +0x444 (only loaded/freed, no display reads it), model list +0x44C, motion list
  +0x450. co2+3 is 0xD/0xE for the Chao walkers (0xD is also Chaos's CharID); they are only
  reachable in 2P costumes. The mod numbers them 10/11.
- The model table is filled once per stage by the loader's 0x459590 call; nothing per frame
  sets the part slots. A display drawn without its own walker's models loaded crashes: Tails'
  walker drawn with Eggman's display reads uninitialised stack at 0x744ACA (slot 258 empty
  leaves its local unset) or calls 0x42EB30(NULL) at 0x744E6A (slot 266 empty).

### What a walker display reads (0x7444F0 and 0x747DD0 have the same shape)

- **Object:** +0x34 EntityData1, +0x40 CharObj2. Nothing else.
- **EntityData1:** +0 action (0x33: also reads `[e+0x2C]->+0xC` collision and co2+0x134; 0x36
  with co2+0xBC != 0: draws co2+0xBC's motion), +6 bit 1 (invisible → skipped unless
  0x174B009), rotation +8/+0xC/+0x10, position +0x14, **scale +0x20/+0x24/+0x28** (0x427750;
  zero scale = invisible).
- **CharObj2 reads:** +0 player number (indexes 0x1DE4664[], light 0x486E50, viewport test vs
  0x174B009), +2 (Eggman: ==1 skips the part loop), +3, +0x24 upgrades, +0x174 mode, +0x178
  anim, +0x184 frame, +0x198 AnimInfo (body = ModelTable[AnimInfo[anim].model], motion =
  MotionTable[AnimInfo[anim].motion]), +0x19C only when mode == 2, +0x374/+0x378 effect fades
  (0 → skipped), +0x424/+0x42C/+0x430/+0x434 part angles (ints, fed to 0x42AC30 and written
  into the part NJS_OBJECTs' child ang/pos every draw; any value is safe), +0x448 texlist.
- **CharObj2 writes:** +0x200..+0x2FC (callback bone positions), +0x23C, +0x248,
  +0x2D8..+0x2E4, +0x438..+0x440, Tails also +0x3A0..+0x3F4. These are gameplay data (aim,
  effects), so a puppet must not draw with the local player's CharObj2.
- **Model slots dereferenced without a null check:**
  - Eggman 0x7444F0: 258 or 260 (by upgrade bit 0x400000), 262+263 (0x200000 && co2+3==7) or
    265+266, 268 (anim 0x80, with AnimInfo[0x81] motion), 274/276 (0x100000), 278/291
    (0x800000), 287 + motion slot 130 (0x1DEAB14) when co2+0x378 > 0. Its delayed display
    0x7454B0: 279..286, 462.
  - Tails 0x747DD0: 306 (0x1DE9FB4, bit 0x100) or 304 (0x1DE9FA4), 309 (bit 0x80) or 312, 313
    (anim 0xB4, with AnimInfo[0xB5] motion), 319 + 321 (bit 0x40), 325 (co2+0x374 > 0); 303/305
    are only compared / null-tested. Delayed 0x748CA0: 317 (only anim 0xB3).
  - Bone callbacks only compare bone pointers against slots (null never matches).
- **Eggman's extra parts:** the bone callback 0x744020 compares each bone with model-table
  entries 247, 248, 249, 251 (0x1DE9DEC..0x1DE9E0C) and writes those bones' world positions into
  the CharObj2 and saved matrices (0x1A52090, 0x1A520C0). After the body (0x7448C3..) the display
  draws models 258/259/260 (0x1DE9E34/3C/44) from those matrices, gated by upgrade bits (e.g.
  `[co2+0x24] & 0x400000`).
- **Globals written:** 0x1A51D00..**0x1A52150** (bone matrices 0x1A51D2C..0x1A52120, the drawing
  co2 at 0x1A51F0C, the view copy at 0x1A51F70), 0x1A557FC..0x1A5584C (matrix top, 0x1A55808,
  bone callback 0x1A55834, 0x1A55838), 0x1DE4660..0x1DE4668, 0x1DEB6A0..0x1DEB6AC (its saved
  render flags), fog 0x25EFFCC..0x25EFFDC. It restores 0x25F02D4/0x25F02D8/0x25F0268 itself. The
  delayed displays read the bone matrices, which only the display/callback/delayed code touches
  (0x744020..0x7458EF, 0x746290..0x748FFF; the walker main 0x741530 doesn't).
- 0x420510 (light from 0x19341C0..C8) is called only for the cockpit model when 0x1A55808 == 0.

### Chao walkers (0x746290 / 0x7470D0)

Both are the same code with different slots; same shape as Tails' display but **no upgrade
parts**: they never read co2+1, +2, +3 or +0x24.

| | Chao Walker (10) | Dark Chao Walker (11) | condition |
|---|---|---|---|
| display / bone callback / delayed | 0x746290 / 0x745CC0 / 0x746850 | 0x7470D0 / 0x746B00 / 0x7476A0 | |
| body (via AnimInfo) | 450 (451 for 0x81/0xB5, 287 for 0x82) | 473 (**451** for 0x81, 474 for 0xB5, 287 for 0x82) | 287 is an EWALKMDL slot; 451 a CWALKMDL slot |
| callback root, **always** | **442** (same NJS_OBJECT as 450) | **465** (same object as 473) | callback does `bone == slot ? … : bone == [slot+0x2C]`: a null slot reads 0x2C |
| held object | **451** + motion **193** (AnimInfo[0xC1].motion) | **474** + motion **198** | anim == 0xB4 && co2+0x174 != 2 |
| effect | 458 | 481 | co2+0x374 > 0 |
| delayed, null-checked | 460; 283 (anim 0xB3) | 461; 283 (anim 0x7F) | every file's 283 has a child with a model |
| bone matrix outside 0x1A51D00.. | **0x1D19D00** (0x30) | **0x1D19CC0** (0x30) | written by the callback, read only by its delayed display |

- Only compared (never dereferenced) in the callbacks: Chao 443..449, 451..454; Dark 466..472,
  475..477.
- CharObj2 reads: +0 (player: 0x1DE4664[], light 0x486E50, and the callback reads
  `*(u8*)MainCharObj1[co2+0]` for bone 443/466 with **no null check** on 0x1DEA6C0[co2+0], so
  +0 must be a real player number), +0x174, +0x178, +0x184, +0x198, +0x19C (mode 2 only),
  +0x374, +0x448 texlist; +0x2C (only if that action == 1), +0x428 (callback angle, 0 →
  skipped); +0x134 / +0xBC only for entity actions 0x33 / 0x36. Writes: +0x1E8..+0x2FC,
  +0x3A0..+0x3E8 (only when +0x374 > 0), +0x3F4..+0x424, +0x438..+0x440.
- Globals: matrices 0x1A51D5C/D8C/E1C/EAC/F70/FA0/0x1A52120 (Chao), 0x1A51DBC/DEC/E7C/EAC/F40/
  F70/FD0 (Dark), all inside 0x1A51D00..0x1A52150; plus 0x1D19D00 / 0x1D19CC0 (nothing else
  references 0x1D19CC0..0x1D19D30); 0x1DEB6A0..AC, 0x1DE4664, fog 0x25EFFCC..DC, 0x1A55834,
  0x1A55844 (delayed). The tail helpers 0x748AF0 (Chao, shared with Tails: body 293 only) /
  0x745310 (Dark, Eggman's: body 248 only) just push/pop the matrix.

### Slot ranges per walker (from gd_PC/*.prs, big-endian {id, offset} lists)

| file | model slots | motion slots |
|---|---|---|
| EWALKMDL / EWALKMTN | 248..292 (no 252/255), 462, 463, **267** | 87..137 (33 nodes) |
| TWALKMDL / TWALKMTN | 293..327 (no 297/300/301), 464, **267** | 139..188 (33 nodes) |
| CWALKMDL / CWALKMTN | 442..460, 267, 283 | 88..127, 133..135, 190..197 |
| DWALKMDL / DWALKMTN | 461, 465..482, 267, 283 | 88..127, 133..135, 190..201 |

Bodies: Eggman 248 (293 for anims 139..181), Tails 293, Chao 450, Dark 473. Eggman's and
Tails' slots are disjoint except 267 (first loaded wins; neither display uses it), which is how
2P battle has both walkers. The Chao walkers share Eggman's motion slots 88..135 and each
other's 190..197 (same 33-node skeleton: a mismatch looks wrong but never overruns).

### How the mod draws a walker puppet (src/puppet.rs)

1. The display is picked by **the puppet's** walker: 6 → 0x747DD0, 7 → 0x7444F0, 10 →
   0x746290, 11 → 0x7470D0 (cdecl `void(ObjectMaster*)`).
2. Assets of a different walker are loaded with the other-character loaders (texlist into the
   private co2+0x448) and released at stage end.
3. Each frame it checks the body model and motion for the anim and every unguarded slot listed
   above for that display (Eggman: 258, 259, 260, 262, 263, 265, 266, 268, 274, 276, 278, 287,
   291 + motion 130; Tails: 304, 306, 309, 312, 313, 319, 321, 325, plus 259 / 305; Chao: 442,
   +451 & motion 193 for anim 0xB4; Dark: 465, +474 & motion 198 for anim 0xB4). If any is
   null, it draws the body only.
4. A zeroed 0x458(+8)-byte CharObj2: +0 = local player number (0), +1 and +3 = the walker type,
   +2 = 0, +0x24 = remote upgrades (`PlayerState.upgrades`), +0x174 = 0, +0x176 = +0x178 =
   anim, +0x184 = frame, +0x198 = that walker's AnimInfo, +0x448 = its texlist; +0x374/+0x378/
   +0x424..+0x434 left 0.
5. A zeroed 0x40-byte EntityData1: action 0, rotation, position, scale 1.0 ×3.
6. A 0x50-byte object with +0x34 = entity, +0x40 = co2. The globals above (0x1A51D00 len 0x450,
   0x1A557FC len 0x50, 0x1DE4660, 0x1DEB6A0, fog, 0x1D19CC0 len 0x70) and the render texlist are
   saved, the display called, and everything restored.

## Walker shots

- Bullet: `CreateWalkerBullet` 0x75D7D0 (EAX parent or 0, ECX &velocity, stack &position, caller
  pops 4); object `WalkerBulletExecutor` (name 0x901660) in list 3, main 0x75D520 (until it
  dies: main = DeleteObject), Data1 +0x14 position, +0x20 velocity; then 0x75D630 and the muzzle
  effect 0x755690(&position). One caller: the walker fire code at 0x741734 (position =
  CharObj2+0x248).
- Bazooka: 0x75DCA0 (EAX parent or 0, EBX &position, stack &velocity), `WalkerBazookaExecutor`
  (0x901678), main 0x75D9F0, same Data1 layout; callers 0x7416A0, 0x7416EE.
- `WalkerJetExecutor` (0x901690, creator near 0x75EE9B) is the booster flame;
  `EWalker2PLaserMan` is 2P only.
- Co-op mirrors bullets and shells (`src/shots.rs`, `Body::Shot`).
