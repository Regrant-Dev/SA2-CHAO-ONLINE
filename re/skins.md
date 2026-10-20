# Skins: models, textures and how characters and karts are drawn

Sources: the disassembly of `re/bin/sonic2app.dumped.exe`, decompiled functions in REA, the
game's `resource/gd_PC/DLL/Win32/Data_DLL_orig.dll` (the mod loader renames the real one), and
the game's model, motion and texture files.

## The kart type table (KART_TYPES 0x1D96F70, 8 entries x 0x1C)
Filled at kart race setup by **0x61FFE0**: it copies the Data_DLL export `specialInfo` (8 x 0x1C)
into 0x1D96F70. 0x6200A0 then overrides some entries with character karts from the export
`exModels` (far = near = exModel[i], texlist = `texlist_kartExModel_cartSpecial`, wheels = 0).

| Off | Meaning |
|---|---|
| +0 | kind |
| +4 | model A (NJS_OBJECT*); copied to car work +0x34 |
| +8 | model B (NJS_OBJECT*); work +0x38. The display draws +0x38 when 0x61EFE0 returns non-zero and +0x34 otherwise (REA). Only +0x34's wheel nodes are posed. |
| +0xC | NJS_TEXLIST*; work +0x3C |
| +0x18 | "wheels": 1 for kinds 2..7, 0 for character karts; work +0x40 |

Data_DLL exports are looked up by name with **0x77DEF0** (cdecl, push name -> pointer;
GetProcAddress on the handle at 0x1AF0220).

Texture setup 0x6200A0 (REA) picks the texlist by 0x1D970FC:
- 0: `texlist_tailsBig`, the story kart, file name `sBig`/`tailsBig`;
- 1: `texlist_rougeBig`;
- otherwise: `texlist_cartData_cartSpecial` with the archive "cartData".

## Textures
- Kinds 2..7 share `texlist_cartData_cartSpecial`, loaded from **cartData** (`gd_PC/cartData.prs`).
  Character karts use **kartExModel** (`gd_PC/kartExModel.prs`).
- **LoadTextureList 0x44C350**: usercall, ECX = archive name (no extension), EDX = NJS_TEXLIST*.
  It first tries the PC texture archive loader 0x430B10, which builds "./resource/" + gd_PC
  path for `PRS/<name>.pak`. The dumped exe has the mod loader's hook at its first byte, so mod
  texture packs and PAK replacements go through there. If that fails it falls back to 0x44C620
  ("%s.PRS", a GVM). Then it copies texaddr/attr into the texlist's entries, **only up to the
  texlist's own count** (+4). So the texlist must already have the right number of NJS_TEXNAME
  (12 bytes: name*, attr, texaddr). Confirmed in REA: entries are filled **by archive order**
  (attr into +4, texaddr into +8), and names are never compared. A skin's texture order must
  match its model's texture ids.
- **Release: 0x77F9F0**, cdecl (NJS_TEXLIST*).
- The mod copies a skin's texture pack to `<mod>/textures/SKIN_<ID>/`, where the mod loader looks
  for texture packs, and loads it with `LoadTextureList("SKIN_<ID>")` into its own NJS_TEXLIST
  sized from `skin.ini`'s texture count.

## Model format: Ninja chunk models
Kart models are NJS_OBJECT trees whose models are **NJS_CNK_MODEL** (0x18 bytes: vlist*,
plist*, center f32[3], radius). The vlist starts with chunk type 0x29 and the plist with
0x22511 (material/tiny chunks). So a skin model must be a chunk model, an SA Tools `.sa2mdl`, not
a GC/`.sa2bmdl` one. Drawn by 0x42EB30(object, 0x42D650) after 0x42D340.

Node layout (kind 2, model A): root (no model) -> children: 3 small nodes, then the body node
with its own children. With wheels = 1 and kind 2..7, the display (0x61CAA6) writes the wheel
angle (work +0xD4) into ang.x (+0x14) of model A's first three child nodes. It reaches them as
root+0x2C, then +0x30 siblings, and clears eval flag 2 on each. It also draws the exhaust
(0x61ECF0). **With wheels = 0, nothing in the model is touched:** safe for any node layout, but
no exhaust.

## .sa2mdl (SA Tools ModelFile)
- u64 header: "SA2MDL" in the low 6 bytes (0x4C444D324153), version in the top byte (3).
- u32 +8: root NJS_OBJECT offset. u32 +0xC: metadata offset (labels/author/etc).
- Pointers inside are file offsets (written with image base 0); 0 = null.
- Relocate by adding the buffer base to:
  - NJS_OBJECT (0x34): +4 model, +0x2C child, +0x30 sibling. Everything else is inline:
    +0 flags, +8 pos, +0x14 ang, +0x20 scale.
  - NJS_CNK_MODEL (0x18): +0 vlist, +4 plist. The vlist and plist are self-contained chunk
    streams with no pointers. Texture ids in the plist index the current texlist, so the
    skin's archive order must match the model's texture ids.
- Models can be shared between nodes, so each node and model is relocated only once.

## Kart skin drawing
- A kart skin is an entry {kind, far = root, near = root, &texlist, 0, 0, wheels 0}, built at
  kart race start from the skin's relocated model and its texture list.
- Remote karts (src/kart.rs) and the local kart get the entry's fields written into their car
  work (+0x34/+0x38/+0x3C, +0x40 = 0). KART_TYPES itself is left alone, because the CPU karts
  use it.
- With wheels = 0 the display touches no node, so any node layout works.

## Character skeletons (the game's `gd_PC/*mdl.prs`, tools/prs.py)
Character model files are PRS-compressed, **big-endian** model lists: `(i32 model number,
u32 file offset)` pairs ending at -1; objects and chunk models (NJS_OBJECT 0x34,
NJS_CNK_MODEL 0x18: vlist, plist, centre, radius) with offsets from the file start. The
loader 0x459590 swaps them to little-endian on load.

Biggest node trees per file (nodes, model number):
- 62-node humanoid tree (the same layout, different proportions): Sonic (0/31/48), Shadow
  (65/104/123), Amy 395, Metal Sonic 418, Knuckles (142/169), Rouge (171/206), Tails
  (208/528), Tikal (483/503), Chaos 0 (507/525); 60-node variants beside each.
- Eggman: 46 nodes (230/527). Mechs/walkers: 33 nodes (Tails 293/464, Eggman 248/463,
  Chao walkers 442/450, 465/473).

So one bone mapping onto the humanoid tree serves nine characters. The converters read the
target skeleton from the player's own game files (the repo holds no game data).

## Following Sonic's display and motions

### Sonic's display 0x720090 (ObjectMaster +0x14), the body's matrix chain
e1 = obj+0x34 (EntityData1), co2 = obj+0x40. Before drawing: bone callback 0x1A55834 (0x71EAA0,
0x71F5E0 if co2+1, Amy 0x71F040, Metal 0x71FBE0), texlist `*(*(0x2670544)+0x20) = co2+0x394`.
1. push matrix (copy current 0x30 bytes, 0x1A557FC += 0x30, not past 0x2670588).
2. translate:
   - normally e1 pos (+0x14,+0x18,+0x1C);
   - **if e1 Action (byte +0) == 0x33**: hanging from something. Anchor = `*(*(e1+0x2C)+0xC)` (an
     object/collision record; its position at +8/+0xC/+0x10). Offset = (0, -co2+0x134 * 0.5, 0)
     rotated by Z(e1+0x10), X(e1+8), Y(-e1+0xC) (0x4273B0 = transform vector by the scratch
     matrix), translate(anchor + offset).
3. rotate Z(e1+0x10), X(e1+0x08), Y(0x8000 - e1+0x0C) (each skipped when 0). In 2P split
   screen of the other viewport only Y.
4. **anim 11 and (e1+4 & 3)** (on the ground): extra tilt 0x428A30 (push), translate(0, 5, 0),
   rotZ(0x2000 = 45°), translate(0, -5, 0), 0x429740 (pop/merge): Sonic's top-speed lean, about
   his middle (5 units up).
5. scale(e1+0x20, +0x24, +0x28) (normally 1).
6. Body: if e1 Action == 0x36 and co2+0xBC != 0: model AnimInfo[0] with frame *(0x1DEB070)
   (a held/attached state). Else model = ModelTable[AnimInfo[anim].ModelNum] (low-detail swap
   0x1DE97A4/0x1DE971C when far), **spin ball**: unless co2+0x174 == 2, when (e1+4 & 0x100) and
   co2+3 != 0xC and (co2+0x360 & 0x11): model AnimInfo[30] (ModelNum 6, the ball) and anim = 30.
   DrawMotion 0x780870(model, co2+0x184) with ECX = motion (the double AnimInfo lookup).
7. pop; shoes/upgrades after (co2+0x24 & 0x40008 bits, models 0x1DE96EC / 0x1DE9944).
A skin drawn with translate, Z, X, 0x8000 - Y matches the body, apart from action 0x33, the
anim-11 lean and e1's scale.

### DrawMotion 0x780870 and the motion format
- 0x780870: 0x7819A0(frame) with ECX = motion, then 0x782420(model) walks the tree.
- 0x42C240 (set motion, ECX = NJS_MOTION, 0 = a default): NJS_MOTION = { +0 mdata*, +4 nbFrame u32,
  +8 type u16 (1 POS, 2 ANG, 4 SCL, 0x2000 = quaternion-style rotation via 0x7828E0),
  +0xA inp_fn u16: low 6 bits = key arrays per node (n), bits 6..7 = interpolation }.
- mdata: one entry per node in depth-first order (object, its child tree, then sibling),
  n*8 bytes: n key pointers, then n u32 key counts (POS, ANG, SCL in that order, only those
  present in `type`). The node counter (state +0x1C) advances once per node.
- Keys are 16 bytes: u32 frame + 3 values: POS/SCL f32[3], ANG i32[3] (BAMS).
- Interpolation 0 (linear, tables 0x174B294 / 0x174B2F8): key search 0x42C2C0 truncates the
  frame, k0 = last key with frame <= f (binary search), k1 = next key, or key 0 after the last
  (wrap; then t = f - k0.frame unscaled). t = (f - k0.frame) / (k1.frame - k0.frame).
  POS: k0 + (k1 - k0) * t. ANG: per axis `k0 + (i16)(k1 - k0) * t` (shortest way, 16-bit wrap),
  then truncated to int. (The angle interpolator 0x42DF60 starts with a jmp into a hook at
  0x774896C0 in this dump; the body after it is the original.)
- Missing keys leave the node's own NJS_OBJECT pos/ang.

### What Sonic's motions do (sonicmtn.prs / plcommtn.prs, PRS-decompressed; big-endian files)
File: entries {i16 id, u16 nodes (62 for Sonic), u32 offset} until id -1; motion data
big-endian, pointers file-relative (the loader swaps and relocates). AnimInfo 0xA17C18 maps
anim n -> motion n for 0..202 (except 100 -> 66, 131/132 and 155/156 swapped); 203.. use
model 65 (the alternate body).
- **Node 0 (root) has no keys in any motion**; node 2 (the body) carries a small position bob
  (pos only); limb nodes carry the rotations. So there is no whole-body root rotation to sample
  for runs, grinds or board tricks: they're limb poses.
- **Anim 30 (spin ball, model 6)**: 8 frames, node 1 rotates a full turn about **Z** (span
  57343 BAMS): the ball rolls about the model's Z axis, so character models face along X (Z is
  their side axis). A car facing +Z needs the 90° turn, and a flip about its own X (its side
  axis) matches the ball's roll.
- Full turns elsewhere: motion 117 (64 frames) node 4 ±X full turn; 102/103 node 4; plcom 51/52
  (10 frames, node 4, large spans), probably tricks. Anims 104-111 (8 x 17 frames, mode 13),
  121-124 / 145-148 (mode 13) and 157-168 (mode 9, return to 129) are Sonic-only sets, probably
  the board/grind lean directions and board tricks.

## Big the Cat cameo (REA slices `re/bin/big_4f5ef0.bin`, `bigdisp_4f6040.bin`)

**The object.** SET object "BIG THE CAT", main **0x4F61E0** (state table 0x4F62C4). It is in
about 50 stage object lists, so most stages have it.
- Init **0x4F5EF0** (REA): Data2 +0x1F8 = motion table **0x1521D70**, +0x1FC = model
  **0x14E0EAC**, +0x1F3 = pose kind (from the SET's rot), display 0x4F6040, delete 0x7A11C0.
- The first Big in a stage loads the textures with `LoadTextureList("E_BIGTEX", 0x1521D38)`
  (0x44C350; static texlist **0x1521D38**). It then makes a helper object (0x46F680, main
  0x6FE430) whose delete 0x4F5ED0 frees them. The object pointer is kept at 0x1A5A79C.

**Display 0x4F6040 (REA):**
1. Set texlist 0x1521D38, push the matrix.
2. Translate to the position, then rotate Z, X, and **Y(rot.y + 0x4000)**, not
   0x8000 − y as the characters do.
3. `DrawMotion(model 0x14E0EAC, Data2+0x1E0 frame)` with ECX = `*(0x1521D70 + 16 * Data2[0x1F2])`.
   In mode 2 (Data2+0x1F0) it uses the shape-motion draw 0x782320 instead.
- Pose kind 0x17 draws nothing.

**The model is static exe data, little-endian chunk models.**
- At runtime the root is simply 0x14E0EAC. It is always there (the exe's .data), whatever the
  stage.
- Offline, read it from the decrypted exe with `tools/bigmodel.py <exe>`.
- 73 nodes, meshes on 36 of them, 566 vertices.
- All vertex blocks are plain unweighted `CnkV_VN` (0x29): each node's mesh sits rigidly in
  that node's frame.
- Polygon chunks are materials **0x13 and 0x17** (diffuse + ambient (+ specular): a size of 4
  or 6 u16), tiny texture ids (0x08) and strips 0x41.
- The mod's model checker (skins.rs) accepts only material 0x11, so these must be rewritten as 0x11.

**Motions.** The table at 0x1521D70 is 15 entries × 16 bytes; +0 is an NJS_MOTION* with type 3
(position + angle), 17 to 120 frames. These are the cameo poses: normal, fishing, dancing and
so on.

**Node tree in bind pose** (world coordinates: Y up, legs at ∓Z, tail along +X; arms hang
straight down):

| Nodes | Part |
|---|---|
| 0 / 1 | root / hips (no mesh) |
| 3–10, 11–18 | legs at z −2.98 / +2.98: thigh 4/12 (y −1.84), knee 5/13 (y −4.39), ankle/foot mesh 7/16 (y −6.34), toes 8/17, shin mesh 9/15, thigh mesh 10/18 |
| 19–22 | tail (towards +X) |
| 23 | pelvis mesh |
| 24–26 | chest group (25 is turned by 0x8000) |
| 27–37, 38–51 | arms at z −3.93 / +3.93: shoulder 28/39 (y 5.15), elbow 29/40 (y 1.45), wrist 30/41 (y −2.30); hand meshes 33–35 / 43–45; fishing rod 46–48 on the +z hand |
| 52–57 | meshless chains below the hands (y −10 to −14): IK or line points |
| 58–69 | head: neck 59 (y 6.14), head mesh 62, eyes 63–66, ears 67/68 |
| 70–72 | torso mesh, belt |

So it is a humanoid that maps cleanly onto the 62-node tree: hips, two 3-segment legs, two
3-segment arms, chest, head.

**Textures: E_BIGTEX.** `gd_PC/e_bigtex.prs` is a PRS-compressed **GVM** (GameCube) holding 6
GVRTs, in texlist order:
- b_beruto3 32²;
- b_ring3 32²;
- big 128²;
- btx_hada0 64²;
- btx_sims 64²;
- b_sao00 32².

`tools/gvm.py <prs> <dir>` exports them as PNGs (CMPR/DXT1, RGB5A3, RGB565, I4/I8/IA8,
ARGB8888). Or load the archive in game by name with 0x44C510("E_BIGTEX").

**Big on Knuckles' tree:** Big's mesh nodes map onto Knuckles' nodes (`knuckmdl.prs`, model 142
or 169) by the labels above. Each mesh's vertices move from Big's bind frame (world) into the
mapped Knuckles node's bind frame; Big's arms hang down, so each limb segment's direction has to
be aligned first. The textures can come from the PNG export or straight from the game with
`0x44C510("E_BIGTEX")`.

## Rigged skins: the humanoid tree, body slots, and what a replacement must keep
From the game files (tools/charskel.py, tools/prs.py) and the disassembly.

**Node evaluation** (object draw 0x42EBDD; rotation dispatch 0x4277B0): child = parent x T x R x S.
Unless flag 1, translate by pos; unless flag 2, rotate (flag 0x20 clear: Z(ang.z), then
Y(ang.y), then X(ang.x); set: Y, X, Z); unless flag 4, scale; flag 8 hides the node's mesh.
Computed this way, Sonic's bind pose is upright (Y up), **facing -X** (shoe tips and the nose
marker at -X, the tail at +X), with his **right side at -Z**.

**The 62-node humanoid tree** (depth-first order = motion node order; the same parents and the
same mesh nodes for Sonic model 0 and Knuckles 142, except Knuckles' 7/17 carry no mesh).
Sonic's world bind positions (x, y, z), feet at y = 0:
| Nodes | Role |
|---|---|
| 0 | root (hidden, no mesh) |
| 1 (0, 5.15, 0) mesh, 2 (0, 5.24, 0) mesh | hips / lower torso, waist |
| 3, 4 (0.03, 5.09, 0) mesh | pelvis |
| 5 (0.27, 4.62, 0) | leg and tail branch |
| 6-15 (z -0.28) | **right leg**: 7 thigh (4.69, mesh), 8 knee (3.93, mesh), 9 shin (3.28, mesh), 10 ankle (1.92), 11 foot (mesh), 12-13 shoe (mesh), 14 sole/toe (0.27, mesh), 15 toe end |
| 16-25 (z +0.32) | **left leg**, the same |
| 26-30 (+X, y 4.58 to 3.96) | tail: 27-29 meshes, 30 end |
| 31, 32 (0, 5.24) mesh | chest |
| 33 (5.85) mesh | upper chest |
| 34 (6.63) | neck / shoulder branch |
| 35-42 (z +0.82 to +1.74) | **left arm**: 35 shoulder (6.22, mesh), 37 upper arm (mesh), 38 elbow / forearm (5.53, mesh), 39 wrist (4.71, mesh), 40-41 hand joints, 42 hand (4.22, mesh) |
| 43, 44 (6.48) | head joint (44 hidden) |
| 45 | head end marker (-2.7 along X: the facing) |
| 46/47, 48/49 (8.19) mesh | head meshes (two variants; 49 is swapped for mouths, below) |
| 50/51, 52/53 (8.21, z +/-0.15) mesh | eyes (Knuckles: dreadlock/ear parts at z +/-0.7) |
| 54-61 (z negative) | **right arm**, as 35-42 |
Knuckles' proportions differ (head joint at 6.78, hands at y ~4.1, z +/-2.5), with the same tree.

**Body slots.**
- **Sonic** (AnimInfo 0xA17C18): every anim draws **model 0**, except anim 30 (the ball, model 6).
  The display 0x720090 swaps model 0 by camera distance (0x77FBD0): above 150 it draws
  **model 48**, below 100 (or with 0x174AFDE set) **model 31**. It only swaps when slots 48 and 31
  are loaded and the body pointer equals ModelTable[0] (0x7205A3). Shadow uses 65, 104 and 123
  the same way. Textures: co2+0x394.
- **Knuckles** (AnimInfo 0xA13320): all 141 anims draw **model 142**; there's no distance swap in
  0x72EF20. Textures: co2+0x3F0. Rouge, Tikal and Chaos share the display and slot.
- **Slots pointing inside the body.** Many other model-table slots point at nodes *inside* the
  body tree. Bone callbacks (Sonic 0x71EAA0 and others, Knuckles 0x72EAA0) find hands, feet and
  head by comparing each drawn node's pointer with those slots. They then write world positions
  into co2 (+0x230, +0x2CC, ...: gameplay data such as held objects and effects).
  - Sonic body 0: slot 1 is node 2, then 2:31, 3:43, 4:50, 5:52, 8:44, 9:14, 10:24, 11:13, 12:23,
    15:60, 16:41, 17:55, 18:36, 21:48, 22:46, 23:60, 24:41 (bodies 31 and 48 have their own: 32-47
    and 49-64).
  - Knuckles 142: 143:2, 144:31, 145:43, 146:50, 147:52, 149:44, 150:14, 151:24, 152:61, 153:42,
    154:55, 155:36, 156:48, 157:46, 158:60, 159:41, 160:48, 167:33.
- **Replacing a body.** Every slot that pointed at node *i* of the old tree must point at node *i*
  of the new one. These slots can be found at run time by matching pointers, so no per-character
  list is needed.
- **Mouths.** Sonic's display temporarily replaces the mesh of slot 21's child (node 49) for mouth
  shapes (0x7204C8: slot 21, or slot 86 for Shadow). Knuckles' does the same through slot 156,
  node 48 (0x72F3D0, co2+0x3E4). So a skin must leave nodes 47 and 49 without a mesh and put the
  head mesh on node 44 (clearing its hide flag in the skin's copy).

**Meshes and weights.**
- Every node's mesh is drawn in that node's space after its transform; the chunk vertices go into
  one shared buffer at each block's index offset.
- **Knuckles uses only rigid `0x29` (VN) blocks, with no weights**, and stitches joints by giving
  each node's vertices their own offset in the buffer (e.g. 334 x4, 86 x51, 324 x9). A later
  node's strips then use vertices an earlier node transformed. So unweighted, rigid meshes are
  fully supported.
- Sonic also uses `0x2C` (VN_NF) blocks with weight status in flag bits 0-1 (0 start, 1 middle,
  2 end) for smooth skinning. Flag 0x80 appears on most non-first blocks and is masked off by the
  vertex code (see "Weighted vertices"); flag 0 with an offset also occurs.
- Buffer offsets seen go up to 421 (Knuckles: 370 + 51).
- Polygon chunks: 0x11/0x13/0x17 materials, tiny texture ids, 0x40/0x41 strips.
- A rigid converted model uses one `0x29` block per node, flags 0, each node's vertices at a
  distinct offset (total under ~500). Each node's strips may use its own vertices and those of
  earlier nodes in depth-first order, so a joint's bridging triangles go on the child.

### Hiding the character

**Everything the game draws ends in two renderer entries.** The renderer is a C++ object at
`*0x1A557C0` (set by 0x423DBB; its first word is its function table). Of the table's entries,
two put geometry on screen, both thiscall with the callee popping its arguments:
- +0x40, 7 arguments: cached buffers. Called only by 0x41BE30 (the GC display-list cache,
  also via 0x4240A0).
- +0x54, 6 arguments: plain vertices. Called by 0x41C070, the flush every immediate draw ends
  in (32 callers: chunk models 0x42D500, display lists 0x41C9B0, sprites and polygons
  0x77xxxx, ...).

All 306 uses of the renderer pointer were sorted by entry. The others set state: +0x5C/+0x58
transforms (118 uses), +0xF0..+0xF8 states and textures, +0x3C clear, +0x34 begin/end scene,
+0x30 viewport.

So the mod hides a character by swapping the renderer's table for a copy whose +0x40 and +0x54
return 0 (src/hide.rs) while the character's displays run. Nothing of it reaches the screen
(body, wings, gloves, upgrades, mouths, effects drawn there, the shadow-map pass), yet all its
node matrices and bone callbacks still run, so hands and held things follow. The copy is
sized with VirtualQuery (up to 256 entries; the game uses up to +0x114).

**Model draws** (for reference):
- `0x42D500` chunk model draw, cdecl (model), returns 0. It runs the vertex lists through
  0x42D190, then the strips.
- DrawMotion `0x780870` (cdecl (model, frame), ECX = motion; starts `51 D9 44 24 0C 51`)
  walks the nodes with callback 0x42E660, which draws each through 0x42D500.
- `0x42D790`/`0x42D8E0` queue and flush up to 10 nodes for the GC models' drawer `0x42BED0`
  (opaque and translucent mesh sets by `0x25EFEE0` & 1 / & 2; display lists 0x41C9B0).

**Rigged skins** go in through the character's own display. A hook on DrawMotion lets only
the skin's root through with the real table, so none of the character's other models show.
Rouge's display, for example, draws fixed slots 174 (a separate 30-node model), 186, 190,
192, 195, 196, 199, 201, 203 and 205. When the display draws another body instead (Sonic's
family's ball, model AnimInfo[30], a 31-node tree), the skin's own model is drawn turning over
in its place.

**Every display of the object.** The object list's display loop (0x470010) calls each object's
+0x14, then +0x20 (sorted translucent queue: 0x492F60 / 0x493A90), +0x1C and +0x24, with the
pass in `0x25EFEE0`. Sonic's +0x1C (0x720B50) draws a model at the player from a distance
(0x7983F0 culls under 50 units). All of them run hidden while a skin is on.

**The shadow pass** (0x46FD10). Shadows are a shadow map. The pass sets the
shader mode to 4 through 0x41B1F0 (mode -> program: 4 = SonicVS/PS_ShadowGen, program 11 of
the 13 that 0x4248B0 loads into the shader object `*0x1A557BC` + 0x648; 0x424050 selects
one). It locks that mode (`0x1A55709` = 1), then for each of the 6 object lists 0x46FB00
calls every object's **+0x2C** with the object. A character's +0x2C (Sonic's family 0x71E520,
REA; Knuckles 0x72DCE0) starts its own shadow map (0x495A90(0x80, position, CharObj2 +0x32C,
...)), sets its texture list, pushes a matrix, applies its place and turn as the display does,
draws the body with DrawMotion, sets renderer +0x34(1), and lays the shadow on the ground
(0x495D20(1, 50.0, CharObj2 +0x308, ...)). Under a skin it runs as it is, but its DrawMotion is
replaced (src/hide.rs `replacing`): the skin is drawn there in the pose it had in the display,
from the matrix under the character's. 0x495A90 pushes a matrix and builds the light's view
in it (identity, then 0x427330/0x427180/0x4270E0), and the +0x2C pushes another for its place
and turn, so the light's view is one below the top at DrawMotion. The stack top as +0x2C
begins is the camera's: drawn from there, nothing lands in the shadow map.
- **Culling.** DrawMotion's node draw 0x42E660, when `0x25F02D8` & 0x100, tests the model's
  sphere (0x42F640, against the current projection: in this pass the light's) unless its radius
  is 0.0 (compared with 0x8AF004), and skips the draw if it's out. A large radius does not
  prevent this, so the mod sets every skin model's radius to 0 when it loads one. A rigged skin's
  own root goes as it is.
- Skipping the whole +0x2C call would also drop the shadow laid on the ground, which is why only
  its DrawMotion is replaced.

**Separate objects.** The jump ball is a JumpAura object per frame (re/spin-ball.md), drawn by
its own display 0x756960 (+0x1C). The hook there skips the local player's balls (data +1 == 0)
while a skin is on.

**Character displays** (the init writes ObjectMaster +0x14). Body slots come from AnimInfo;
the alternates have the same tree.

| Character | Init | Display | Texture list (CharObj2) | Bodies (nodes) |
|---|---|---|---|---|
| Sonic, Shadow, Amy, Metal Sonic | 0x71706C, 0x7175CC, 0x7172E5, 0x7177C7 | 0x720090 | +0x394 | 0/31/48, 65/104/123, 395, 418 (62) |
| Knuckles | 0x7283DA | 0x72EF20 | +0x3F0 | 142, 169 (62) |
| Rouge | 0x7287F7 | 0x730970 | +0x3F0 | 171, 206 (62) |
| Tails on foot | 0x74D152 | 0x7507D0 | +0x3B0 | 208, 528 (62) |
| Eggman on foot | 0x73C36B | 0x73EF20 | +0x360 | 230, 527 (46) |

Eggman's 46 nodes: hips 1, torso 25/26, neck 27, head joint 36 (mesh 38), legs 7-14
(right, z -) and 16-23, shoulder joints 28/39, arms 29-33 (left, z +) and 40-45.

### Weighted vertices (0x42D190, 0x42ED60, 0x42E440)
0x42D190 runs a model's vertex list: per block, h1 = type | flags << 8 | words << 16 and
h2 = offset | count << 16; the slot base is buffer + offset * 32 (`*0x25EFE48`, 0x40000 bytes
allocated at 0x43502E: 8192 slots). The 0x1000 stored at 0x25EFE4C is never read.
- **0x29 (VN)**, 0x42E440: per vertex position, normal; the slot gets them through the current
  matrix, and its word 6 = 0xFFFFFFFF.
- **0x2C (VN_NF)**, 0x42ED60: per vertex position, normal, u32 flags. The slot is
  base + (flags & 0xFFFF), the weight (flags >> 16) / 255. The status is h1's flags with 0x80
  masked off: 0 sets the slot to the weighted vertex, 1 adds to it, 2 adds and marks it
  complete (word 6 = 0xFFFFFFFF).
- Others: 0x2A (VN_D8, 0x42F4F0), 0x23 (0x42ECD0).

tools/skinanim.py writes a vertex moved by several bones (up to 3, each at least 10%) as
0x2C shares. Every share but the last goes in at a share node (no transform: the bone's space)
right before the child subtree that completes the vertex, so its slot isn't held across other
subtrees. Triangles are drawn once all their corners are complete.

### Garden actions (for animated skins' extra animations)
Player actions are `EntityData1+0` (Sonic's switch: 0x719B19, table 0x71E1A0, 0x58 entries; a
per-frame switch at 0x717C81, table 0x7197B4, same numbering). Animation = CharObj2 +0x176 (next)
/ +0x178 (current). The action button: CharObj2 +4..+0xB is this frame's list of possible
actions (count +0xC), +0xD the one the object in front offers; on B the switch at 0x723182
(context code = action number) starts it through small shared setters (0x475F00..0x4762A0).
The garden list is built by **0x475AF0** from the flags **0x45A0C0** gives the object in front,
by its `Data1+2` kind: 0xA/0xC holdable (0x200), 0xB pushable (0x400), 0xD tree (0x800), 0xE
(0x1000).

| Action | Animation | What |
|---|---|---|
| 19 (0x13) | 31 | pick up (object at hand height) |
| 41 (0x29) | 69 | pick up from the ground (bend down) |
| 49 (0x31) | 69 → 79 | pick up a kind-0xE object (voice 0x3004 Sonic / 0x3016 Shadow) |
| 43 (0x2B) | 78 | **pet a Chao**: spawns `ChaonadeExec` (0x477100) at the hand (CharObj2 +0x1E8) while the action lasts (max 60 frames, facing the Chao); pet voice at its frame 30 (0x5EC, 0x5ED, ... by character, 0x476FD0) |
| 20 (0x14) | 53 | put down (holding, slow) |
| 26 (0x1A) | 49 / 50 | throw (holding, moving; 0x46E5B0 also sets it when made to pick something up) |
| 33 (0x21) | 51 / 52 | throw in the air |
| 23 (0x17) | 43 | reach for a tree → 34 |
| 34 (0x22) | 41 | hold the tree (voice 0x6C0 / 0x6C3 by character, 0x4754F0) |
| 35 (0x23) | 42 | **shake the tree**: pushes the tree's Data2 +0xC..+0x14 by the animation frame (0x718BA1) |
| 42 (0x2A) | 72 | **whistle** (Chao check 0x5637B0; sound 0x2005/0x2006 by character) |
| 22 (0x16) → 32 (0x20) | 43 → 40 | push a box / wall |
| 45, 46/47, 48 | 82, 83, 84 | Mystic Melody (by each character's upgrade bit, 0x475A40) |
Holding anything: EntityData1 status +4 & 0x800, CharObj2 +0x94 the object.
Voices: 0x443130 (EAX = voice id, EDX = 2) and 0x4372E0 (EDI = id); sounds 0x437260 (ESI = id).
The game's own voice banks are in `resource/gd_PC/MLT`.

### Voices
- **0x4372E0**: play a sound at a position (EDI = sound id, ESI = position, cdecl stack args;
  returns AL = 0xFF when it couldn't). 322 callers. It starts `51 53 8A 5C 24 14` (push ecx,
  push ebx, mov bl,[esp+0x14]), so a 6-byte jump there is safe to put back exactly.
- Sound ids: 0x20xx the player's sound effects; **0x30xx the character voice bank** (Sonic
  0x3000..0x3011, Shadow +0x12, Amy/Metal 0x3021..0x3028). Known: 0x3004 / 0x3016 picking up
  (0x725582), 0x3005 / 0x3017 putting down or throwing (0x725640, 0x725970), 0x3008 / 0x301A
  light dash, 0x3009 / 0x301B homing attack (action 13). 0x300B, 0x300C (several sites), 0x3000,
  0x3006, 0x3007, 0x300A not pinned down.
- **0x443130** (EAX = voice number, EDX = 2): numbered voice lines, e.g. the pet line (0x5EC,
  0x5ED...) and grabbing a tree (0x6C0 / 0x6C3); not hooked.
- Rings: i16 at **0x174B028** (+2 per player); lives: i16 at **0x174B024** (AddRings 0x44CE10,
  extra life 0x44CB10).
- The mod (src/voice.rs) drops 0x30xx while a skin with sounds is on and plays the skin's
  clips with winmm `PlaySoundA` (memory, async).
- **0x443130 takes the voice id as its stack argument** (`push id; mov edx, 2; call`), not EAX;
  it returns -1 when it doesn't play, and callers then use EDX (`lea eax, [edx-1]`). Its first
  bytes are already a `jmp` at run time (the mod loader hooks it), so it can't be hooked there.
- Garden voices through it (the mod redirects exactly these sites while a voiced skin is on):
  - pet (ChaonadeExec 0x476FD0, frame 30, `jmp` table 0x4770DC by CharObj2 +3): Sonic 0x5ED
    (0x47705B), Shadow 0x5EC (0x477071), Tails 0x6C1 (0x477087), Eggman 0x6BE (0x47709D),
    Knuckles 0x4DA (0x4770B3), Rouge 0x4DB (0x4770C9);
  - tree: grabbing it (0x4754F0, by CharObj2 +0 = 3 / 2) 0x6C0 (call 0x47553D) / 0x6C3
    (0x475557); starting to shake it (action 35 in two other characters' code) 0x6C0
    (0x73DAB1) / 0x6C3 (0x74EEB3).
- Not garden, left alone: action 11 (hurt, 0x47530C) plays 0x3002 / 0x3014 / 0x3013 / 0x300C /
  0x3022 through 0x4372E0 and 0x65E / 0x65F / 0xA72 through 0x443130 by character; per-character
  sets at 0x473FFB.. (0x61C, 0x515, 0x68C, 0x68D, 0xA6B, 0xA9D) and 0x474794.. (0x6B7, 0x6B4,
  0xA69) not identified.
- The numbered voice call 0x443130 is used by every character's own code with many ids
  (Sonic/Shadow 0x604..0x609, 0x5F0.., 0xA6B..; Eggman/Tails 0x510..0x513, 0x4F6..0x4F8, ...),
  so a list of ids can't cover them. The mod chains a hook onto its first instruction (the mod
  loader's `jmp`, followed on) and drops calls whose return address is in the player code:
  0x450000..0x478000 (shared player helpers: pick up, pet, tree) and 0x710000..0x760000 (the
  characters' mains and actions). Cutscene, Omochao and story voices come from elsewhere.

### Voices in cutscenes

`GameModeHandler` 0x434160: mode 5 runs 0x43A780 (stage states); mode 7 loads `EVENT` (string
0x8B7674) and mode 8 runs it; modes 15/16 load/run `ENDING` (0x8B76D8). The mod's voice hooks
only replace voices in mode 5, state 16, with `ControllerEnabled[0]` (0x174AFFF) non-zero, checked
live on each call. So cutscenes, events and input-locked scripted intros keep their own voices.
The per-frame voice player uses the same conditions; on entering a scene or pause it clears
pending events and movement history and stops any skin WAV that is playing.

## Hurt blink

After a hit a character can't be hurt again for a while: EntityData1 +6 (u16) counts it down.
The character's display (Sonic's family 0x720090) skips the body while +6 has bit 2 (`(+6 & 2)
&& !*0x174B009`), so it blinks every other two frames; 0x174B009 is set while the split screen
draws one player's view, which turns the blink off. Its shadow display (0x71E520) has the same
check around the body it draws into the shadow map, so the shadow blinks too.
- Solid and animated skins aren't drawn on those frames (skins.rs `blinked_out`); a rigged skin
  is drawn by the character's display, which already skips it. The skin's shadow is drawn inside
  the shadow display's own body draw (hide.rs `replacing`), so it blinks with it already.
- Puppets: the sender's `PlayerState` status has 4 while +6 is nonzero; puppets (skinned or not)
  aren't drawn on the local game's blink frames (puppet.rs), with the same 0x174B009 rule. Puppets
  have no shadows.

## Animated skins' movement

The character's position also moves while it stands (pushed back by the ground on a slope or a
wall, carried by a platform), so position change is not a reliable walk signal. The game's own
movement goes by CharObj2 +0x64 (3 f32: forward, up, sideways speed in the character's frame;
0x45B610 compares +0x64 with the physics speeds at +0xE8.., a hit sets it to the knock-back,
0x46C340). Animated skins choose idle/walk/run from its forward and sideways part. For puppets,
the sender's status has 8 when that speed is under 0.05 on the ground, and their skin stands;
moving, it goes by their position. The animator steps once per game frame
(`Animator::step_once`), however many times it's drawn.

## Boards and rails

Sonic's family (Sonic, Shadow, Amy, Metal Sonic) share one action table: Sonic's main 0x717840
switches on the action (0..0x57) through 0x7197B4, its action step through 0x71E1A0.
- **Board: actions 0x4C..=0x52.** Their handlers work with the board object at CharObj2 +0xA0
  (0x71D450..: City Escape's board, o_ce_board.c, checks `co2+0xA0 == itself`, 0x5EB104), and a
  City Escape object checks the player's action against 0x4C..0x52 (0x5EA1C6). The board draws
  itself (its own display 0x5EBCE0 and shadow 0x5EBFC0, set at 0x5EBCAE), so hiding the
  character's body never hides it.
- **Rail: actions 0x47 and 0x48.** Their handlers call 0x46D040 (speed kept only along the way)
  and 0x46D140, which moves the player along a path (CharObj2 +0xB8, distance +0x30, 0x4905A0).
- An animated skin rides there (`ride_role`): its `ride=` / `grind=` animation if it has one,
  else it stands (idle), never runs. Puppets the same, from the `PlayerState` character and
  action. Other "carried along" moves (pulleys, rockets, swing poles, light dash) play jump/fall
  in the air.

## Texture records

- A texture list's entries point (+8) at the game's texture records (`NJS_TEXMEMLIST`), shared
  by global index and counted: 0x44C510 (the loader, through the mod loader's texture pack hook)
  fills a list; 0x77F9F0 releases one: per entry it lowers the record's count (+0x10) and its
  texture info's (+0xC, count +0x24); at 0 it releases the D3D texture and clears the info (+0
  = -1) and the record (+0 = 0, +8 = -1, +0xC = 0), which the next load can then take.
- The game frees its textures between scenes (Chao World's areas too), and the next scene's
  textures take the records. Releasing a list after that would count down and free the new
  scene's textures, which then get taken by the next load while the scene's own lists still
  point there (the garden drawn with a skin's textures).
- So the mod remembers a list's records as loaded (`texlist_signature`: record, +0, +8, +0xC)
  together with the level, stage run and Chao World area, and only draws with or releases the
  list while all of these still match (`texlist_ours`). A list from another scene is dropped
  without releasing anything. Switching skins mid-scene keeps the old list until it's dropped
  the same way.
- In the same scene, textures that look gone aren't loaded again for 300 frames
  (`RELOAD_AFTER`).

### Skin texture lists
- Skin texture lists and entries live in memory owned by the mod. 0x44C350 takes ECX = archive and
  EDX = a caller-owned NJS_TEXLIST and fills exactly the entries the list has, so the pack's
  texture count is checked against `skin.ini` before the call. 0x77F9F0 releases a list's records
  but does not free the list itself.
- Replacing a list clears `LAST_DRAWN` (the last shadow pose) before freeing its storage.
