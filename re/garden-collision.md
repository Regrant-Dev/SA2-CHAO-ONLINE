# Chao garden collision meshes (LandTable → triangles)

How the engine turns `objLandTableNeut/Hero/Dark` into the triangles that `0x48C5E0` ray-casts for Chao movement (re/chao-ai.md §5.2/§5.3), and what the three gardens contain. Layouts come from the dumped exe (sha256 `d1ac8ed9…`) via REA; data was read from `Data_DLL_orig.dll`.

## 1. Pipeline

| Step | Address | What it does |
|---|---|---|
| Load | `0x47BD30(lt)` | `*0x19459DC = lt`. If `lt == 0`: `*0x171CAA8 = *0x171CAAC = 0`. If `lt+0x18 != 0`: `0x44C350` (texture load) when `s16 lt+0xA == 0`, then `lt+0xA += 1`. Sets `*0x171CAA4 = *0x16D4F38` (= **400.0**). Garden ctor sets `*0x1A5A2D8 = 1` first. |
| Gather | `0x47D160` | Builds the dynamic list (`0x1A5A2D0`) and the **landtable list** (`0x1A5A2D4`, count `0x1A5A2CC`, max 0x80) + the combined 12-byte candidate list `*0x1A5A2DC` (`{flags, key, 0}`, count `s16 0x1DE9484`, max 0x400). Centred on the **players**, not on the Chao (see §5). |
| Cache | `0x48BE80` | Calls `0x47D160`; for each candidate (max 0xC0) not already cached (or flagged `0x8000000` = rebuild every frame) takes a cache node from free list `*0x1A5A3FC`, links it into `*0x1A5A400`, sets the matrix to **identity**, then `0x48B9F0(key=COL+0x10 object, COL flags, node)`. Callers: `0x43CC20`, `0x43CE20`, `0x43D510` (frame loops, not traced). Nodes no longer in the candidate list are freed. |
| Hierarchy | `0x48B9F0(obj, flags, node)` | push matrix; `T(obj+8)` (`0x427470`), `Rz(obj+0x1C)` (`0x427680`), `Ry(obj+0x18)` (`0x4275B0`), `Rx(obj+0x14)` (`0x4274E0`), `S(obj+0x20)` (`0x427750`); for `c = obj+0x2C` while `c`: `c = 0x48B9F0(c)` (returns `c+0x30`, i.e. walks child's siblings); then `0x48B610(obj)`; pop. **Eval flags at obj+0 are ignored** (no ZXY order, no skip bits). The root object's own siblings are not visited. |
| Triangulate | `0x48B610(obj)` + `0x48B510` | Walks the basic model's mesh sets, transforms each vertex with `0x426CC0`, writes one cache triangle per strip triangle via `0x4974F0`. |
| Ray-cast | `0x48C5E0` (EDX = key, EDI = query) | Finds the cache node with `node+8 == key`, then tests every triangle (see §4). |

## 2. LandTable (`0x47BD30`, `0x47D160` at `0x47D4F5..0x47D53A`)

| Off | Type | Use |
|---|---|---|
| +0x00 | s16 | COL count |
| +0x02 | s16 | if ≥ 0: first N COLs are skipped by collision (visual-only block); collision walks `COL[N .. count)`. If < 0: all. |
| +0x08 | u16 | (=1 in all gardens; not read in traced code) |
| +0x0A | s16 | texture load refcount (`0x47BD30`) |
| +0x0C | f32 | 3000.0 in all gardens (draw distance? not traced) |
| +0x10 | COL* | COL array, stride 0x20 |
| +0x18 | ptr | texture name/pack; non-zero triggers `0x44C350` |

## 3. COL entry (0x20 bytes)

| Off | Type | Read at | Use |
|---|---|---|---|
| +0x00 | vec3 | `0x47D587` | bounding-sphere centre (world) |
| +0x0C | f32 | `0x47D61D`, `0x47CD60` | radius |
| +0x10 | NJS_OBJECT* | `0x47D670` | object root; also the cache key |
| +0x14, +0x18 | u32 | — | 0 in every garden entry |
| +0x1C | u32 | `0x47D550` | flags |

Flag bits used by code: `& 0x2003` → collidable (enters the landtable list, `0x47D553`); `& 0x2002` → water (`0x47D574`: Y distance ignored when gathering; `0x797E10` excludes water from +X/floor/ceiling slots and sets `W+0xDC`); `0x8000000` → rebuild cache every frame (`0x48BF0x`); `0x10000000` → dynamic entries use an object matrix (`0x47D280`). Every garden water COL has flags `…0003` (bit 0 *and* bit 1), so "solid" must be tested as `& 1 && !(& 0x2002)`. Bits seen but not traced: `0x20`, `0x80`, `0x100`, `0x1000`, `0x400000`, `0x800000`, `0x8000`, `0x20000000`, `0x40000000`, `0x80000000` (all skipped visual entries carry `0x80000000`).

Gather rule for the landtable list (`0x47D587..0x47D627`): for each player position P (P1 `*0x1DEA6C0 +0x14`, P2 `*0x1DEA6C4 +0x14`, fallback `*0x1DD92B0`), keep the COL if `|centre − P| − range < radius`, range = `*0x171CAA4` (400.0) for the first player, `*0x16D4F38` (400.0) after; water: `dy² := 0` (`*0x8AF004` = 0.0).

## 4. Geometry format and triangle cache

**NJS_OBJECT** (offsets from `0x48B9F0`/`0x48BE80`): +0 eval flags, +4 model*, +8 pos vec3, +0x14/+0x18/+0x1C s32 BAMS angles X/Y/Z, +0x20 scale vec3, +0x2C child*, +0x30 sibling*.

**Matrix convention.** 3×4 row-major, `out = M·v + M[:,3]` (`0x426CC0`); every op post-multiplies (`M = M·R`), with the standard right-handed `Rx/Ry/Rz` (e.g. `0x4274E0`: `M01' = c·M01 + s·M02`, `M02' = −s·M01 + c·M02`). Zero angles skip the multiply. sin/cos come from `0x42AAB0`/`0x42AC30`: a 4096-step quarter-wave table (`0x8ACF88`) with linear interpolation on the low 4 BAMS bits, so world vertices differ from `math.sin` by ≲1e-6 relative. World vertex = `T·Rz·Ry·Rx·S` of every ancestor, root first.

**Basic model** (from `0x48B610`): +0 points* (vec3, 12 B), +0xC meshsets*, +0x10 materials* (stride 0x14), +0x14 u16 meshset count, +0x18 centre vec3, +0x24 radius. Normals (+4) and per-poly attrs are **not** read: normals are recomputed per triangle.

**Meshset** (stride 0x18): +0 u16 `type<<14 | matId` (`type = & 0xC000`, `matId = & 0x3FFF`), +2 u16 poly count, +4 s16* indices. `0x48B510` decoding:

| type | Poly | Triangles |
|---|---|---|
| 0x0000 | 3 indices | 1 |
| 0x4000 | 4 indices | 2: (0,1,2), (1,3,2) |
| 0x8000, 0xC000 | header s16: len = `& 0x7FFF` (clamped 0x400), flip = `& 0x8000`; then len indices | len−2: t-th = (t,t+1,t+2) if `(t+flip)&1 == 0`, else (t,t+2,t+1) |

All three gardens use only type 0xC000 (strips).

**Cache triangle node** (`0x4974F0`, filled at `0x48B6xx`): +0 next, +8 centroid (×0.3333), +0x14 max squared vertex–centroid distance, +0x18/+0x24/+0x30 v0/v1/v2 (world), +0x3C material `+0x10` attr flags (not used by `0x48C5E0`). Cache node: +0 next, +4 prev-link, +8 key (object*), +0xC COL flags, +0x10 triangle list, +0x28 max |ΔY| edge, +0x30 model centre (transformed).

**Ray-cast `0x48C5E0`** (query point Q = `W+0xE0`, 6 slots of 0x20 from Q+0xC: −X, +X, −Y, +Y, −Z, +Z). Per triangle: skip unless v0 within 220 of Q (`dx² ≤ 48400`, `dx²+dz² ≤ …`, `+dy² ≤ …`; tested on **v0 only**). Then three axis-aligned lines through Q: Z-line (2D edge-sign test in XY), X-line (YZ), Y-line (XZ); inside if all three edge products ≥ 0 (two-sided). Normal = unnormalized cross product (`0x48AF70`); skip if the component along the ray axis squared < 0.0001; hit coordinate from the plane equation. Hit below Q goes to the − slot (keeps the nearest, i.e. largest), above to the + slot (keeps smallest). Slot: +0 = 1, +4 = COL flags, +8/+0xC = slope angles (atan2 `0x859B6A`), +0x10 = hit coordinate, +0x14..+0x1C = unit normal (`0x77F3A0`; (0,1,0) if length 0).

## 5. Extraction results (`Data_DLL_orig.dll`, image base `0x10000000`, 488 exports, 181 472 relocations; pointers used as-is against the preferred base)

| | Neutral | Hero | Dark |
|---|---|---|---|
| LandTable VA | `0x11C86170` | `0x11C4BAA0` | `0x11C4A5C0` |
| COL count / skipped (LT+2) | 40 / 25 | 89 / 57 | 73 / 42 |
| Collision COLs: solid / water | 14 / 1 | 31 / 1 | 30 / 1 |
| Triangles: solid / water / total | 620 / 51 / 671 | 454 / 32 / 486 | 452 / 68 / 520 |
| Rotated nodes / scaled / children | 0 / 0 / 0 | 5 / 0 / 0 | 1 / 0 / 0 |
| BBox min (x,y,z) | −200.3, −7.0, −208.1 | −198.5, −6.5, −177.0 | −215.4, −7.0, −162.2 |
| BBox max (x,y,z) | 144.7, 190.0, 72.0 | 177.6, 145.0, 198.5 | 185.5, 145.0, 146.8 |
| Water polygon Y | **−2.000** (COL #39, flags `0x40000003`, centre (−78.1, −2.0, −98.6) r 74.1) | **−1.500** (COL #88, `0x20000003`, (76.9, −1.5, −67.7) r 124.3) | **−1.000** (COL #48, `0x20000003`, (92.7, −1.0, −56.9) r 112.6) |
| Max COL radius / longest edge | 166.7 / 192.6 | 139.9 / 170.3 | 112.6 / 161.8 |
| Material attr flags on tris | `0x94002400`, `0x94082400`, `0x96002400` | `0x94002400`, `0x96002400` | same as Hero |

Water polygons match the physics constants of `0x53DF00` exactly (N −2.0, H −1.5, D −1.0); the AI check `0x561630` uses −2.2 for Neutral, i.e. 0.2 below the real water mesh. Neutral water COL centre lies over the grid's value-9 block (cols ~1–8, rows ~8–16 → x ≈ −150..−80, z ≈ −120..−40).

Highest solid floor (XZ-projected, highest hit) at sample points:

| (x, z) | Neutral | Hero | Dark |
|---|---|---|---|
| (0, 0) | 4.68 (`0x20000021`) | 0.28 (`0x40000001`) | 0.00 (`0x40000021`); water −1.0 below it |
| (20, −40) | −0.01 | −6.50 under water −1.5 | −6.00 under water −1.0 |
| (−50, 50) | 7.33 | 0.00 | 0.00 |
| (60, 30) | 10.00 | −6.50 under water −1.5 | 0.26 |
| water COL centre | −5.00 under water −2.0 | −6.50 | −6.00 |

Pool floors sit 4.5–5.5 below the surface, consistent with the swim depth test (`y ≤ surface − 2.1`).

## 6. Rebuilding the ground (server, `chao_sim::land`)

1. Parse PE, find the export, read LandTable `+0`, `+2`, `+0x10`.
2. For `i` in `[max(+2,0), count)`: take COL if `flags & 0x2003`; walk `COL+0x10` with identity, `T·Rz·Ry·Rx·S`, children + child siblings (not root siblings), eval flags ignored.
3. Triangulate strips with the parity rule above; tag each triangle with its COL flags (water = `& 0x2002`).
4. Ray-cast as in §4 (v0 within 220 cull, two-sided, unnormalized-normal threshold 1e-4 on the ray axis).

## 6b. Grid against the LandTable (`chao_sim::land` tests)

Cell middles probed from y = 60 and 180, highest floor hit:

| | Values (count) | Land cells on solid | Water cells on water |
|---|---|---|---|
| Neutral | 0: 926, 1: 288, 8: 21, 9: 45 | 288 / 288 | 45 / 45 |
| Hero | 0: 698, 1: 316, 2: 139, 4: 15, 6: 12, 8: 14, 9: 86 | 299 / 316 | 55 / 86 |
| Dark | 0: 982, 1: 185, 8: 54, 9: 59 | 184 / 185 | 59 / 59 |

- Hero's 31 "water" cells around (0..25, 5..45) and (−35..−5, 85..115) sit over solid floor at y = 0 (flags `0x40000021`), above the −1.5 water: a Chao sent there walks.
- Hero's land cells at x −5..15, z −125..−25 and Dark's cell (15, −105) have only the water polygon under them: open water along the shore, not missing ground (§7: the gardens register no dynamic collision). A Chao sent there swims. Reversing the Y rotation changed nothing, so it isn't a transform error.

## 7. Dynamic collision and normals

### 7.1 Dynamic list mechanics

| Step | Address | What it does |
|---|---|---|
| Storage | `0x47BB50` (alloc, `land.c`), `0x47B9F0` (free), `0x47BCC0` (reset counts) | Source table `*0x1A5A2E0` (0xC00 B = 0x100 × 12), count **s16** `0x19459D4`. `*0x1A5A2D0` (0x600 B, max 0x80) is only the per-frame *gathered* copy, count `0x1A5A2C8`. |
| Register | `0x47D6B0` (EAX = flags, EDX = task, ESI = NJS_OBJECT) | If count < 0x100: appends `{flags, obj, task}`; sets `*(u16*)(task+0x34 → Data1 +4) |= 0x100`. Adds **`0x10000000`** itself when `|model+0x18 centre|² ≥ 225` **and** `(obj+0x14 | +0x18 | +0x1C) & 0xFFF0` (rotated object with an off-centre model). Only writer: xrefs to `0x1A5A2E0`/`0x19459D4` are all in `0x47B9F0`, `0x47BB50`, `0x47BCC0`, `0x47D160`, `0x47D6B0`, `0x47D760`. |
| Unregister | `0x47D760` (EDI = task, EBX = obj) | Removes the entry whose task matches, compacts, clears the cache node's key (`node+8 = 0`) for `obj`. |
| Gather | `0x47D160` (`0x47D230..0x47D4A5`) | Walks the table backwards. Skips the entry if `task+0x10 == 0x46F720` (task being deleted), the parent's `+0x10 == 0x46F720`, or Data1+4 lacks `0x100`. Sphere centre: no `0x10000000` → `model+0x18 + obj+8`; with it (`0x47D280`) → `M = I·Rz·Ry·Rx` (or `Ry·Rx·Rz` if obj eval flag `0x20`), `0x426CC0(model centre) + obj+8`. Keeps it if within `model+0x24 + 400` of a player (same players/ranges as the landtable gather). Copies `{flags, obj, task}` to `0x1A5A2D0` and to the candidate list `*0x1A5A2DC` (dynamic entries **first**). |
| Cache | `0x48BE80` | Key = `obj` for both kinds (landtable candidates carry `COL+0x10`). Same path as §1: **identity**, then `0x48B9F0(obj)` → world = `T(obj+8)·Rz·Ry·Rx·S(obj+0x20)` of the object's own fields, children/siblings as §1. So the owner places its collision by writing the NJS_OBJECT's pos/angles. A node is rebuilt only if flags `& 0x8000000` (every frame) or its key equals `*0x194A208`; otherwise it is built once while it stays in the candidate list. |
| Query | `0x47CD60`, `0x797E10` | As chao-ai §5.3 (dynamic entries first; `0x797E10` re-checks entries with `task != 0` against `model+0x24 × max(obj scale)`). |

Example caller: the Chao **Entrance** elevator `0x558510` (`ALO_EntranceElevatorExecutor`, `src\Chao\stg_entrance_alo.c`): NJS_OBJECT copied from exe template `0x11CD5EC` (0x38 B), pos from its Data1, registered at `0x55862A` with flags `0x28000001` (solid, rebuild every frame, `0x20000000`).

### 7.2 Gardens: nothing on the dynamic list

- Direct callers of `0x47D6B0`: 99 sites, `0x4A6CAB..0x78D7ED`. All 20 whose function names a source file or model (16 in `0x4A6CAB..0x4E455F`, 4 in `0x5AE517..0x5BB061`) are stage/boss objects (`stg05_pumpkin`, `stg06_mountain`, `stg07_coalmine`, `stg14_highway`, `stg17_Mission`, `stg30_FinalRush`, `stg34_CannonsCore`, `stg42_sonicvsshadow2`, `BossGolem`); the others sit in the same stage-object ranges or at `0x5C349A` and above, outside the Chao/garden code. The only one in the Chao code is `0x55862A` (Entrance elevator, not a garden). It takes register arguments, so there are no indirect callers.
- Garden constructors (Neutral `0x54C7B0`; Hero `0x54D210`, no Ghidra function; Dark contains `0x54B74D`) load the LandTable (`0x47BD30`), then `0x54B190` (SET file `set_chao_neut/hero/dark.bin`, + `_s`/`_u`) with `0x487E40(objlist, set)`: Neutral list `0x121C8B0`, Hero `0x121AE90` (14 types at `0x121ADB0`, 0x10 B each). Then fixed creators `0x57D4C0`, `0x57D2E0`, `0x549490`, `0x530B80`, `0x531B10`, `0x52EB70`, `0x531E10`, `0x52F2A0`, `0x57E4F0`, `0x55D6B0` and `0x55CB90`/`0x580890` (N) or `0x580F30` (H), and finally `0x56B040` (`WaterManagerExecutor`, display only: table `0x1313D40` N / `0x1313A48` H of Data_DLL exports such as `object_gneut_nc_water_nc_water`).
- Hero SET types: HASIRA `0x5A7A60`, HASIRAKAKE `0x5A7AA0` (decor + CCL via `0x47E520`), HANAAKA/HANAMURA/KUSA A–C/TOWER `0x5A7AE0..0x5A7BD0` (decor: model ptr at task+0x40, main `0x5B4250`/`0x6089D0`), C_SPHERE/C_CYL/C_CUBE/WALL_PLANE `0x6E5400/0x6E5470/0x6E54E0/0x6E5550` (invisible CCL shapes via `0x47E520`, data `0xB14570..0xB14600`), LOBBY `0x55AB90`, C_CLIMB `0x55AB20`. None registers land collision; CCL shapes are not read by `0x797E10`.
- So in all three gardens the Chao ground is the LandTable alone, and nothing has to be collected at runtime. (Elsewhere, a runtime collector would walk `*0x1A5A2E0`, `s16 *0x19459D4` entries `{flags, obj, task}`, skip dead tasks as in `0x47D160`, and triangulate `obj` exactly like a COL root.)

**The §6b "crossing" is open water.** All COLs (also the visual block, by sphere) at those cells: Hero (5, −105), (5, −125), (15, −75): water −1.5 over pool floor −6.5 (COL #69 `0x20000001`), nothing else; Dark (15, −105): water −1.0 over floor −6.0, with land at y 0.0 (COL #72 `0x40000021`) 10 units west, so that cell straddles the shore. The test's "highest hit" counts water, while `0x797E10` keeps water out of the floor slot, so the game's floor there is the pool floor, `W+0xDC` = surface, and a Chao sent there swims. The grid (10-unit cells) is coarse at these shores; the swim logic handles them.

### 7.3 Triangle normal sign

- **Cache triangle order** (`0x48B610`, triangle loop `0x48B6xx`): vertex buffer from `0x48B510` (in index order); +0x18 = vert[t]; if `(t + flip) & 1 == 0`: +0x24 = vert[t+1], +0x30 = vert[t+2], else +0x24 = vert[t+2], +0x30 = vert[t+1]; `flip` = `0x48B510`'s EAX (`header & 0x8000` for strips, 0 for tris/quads). Quads: t = 0, 1 → (0,1,2), (1,3,2).
- **Call sites** in `0x48C5E0`: `0x48C7A8` (Z-line), `0x48CA22` (X-line), `0x48CC7C` (Y-line). Each: `EBX = ESI = tri+0x18` (v0, `0x48C689`, `MOV ESI,EBX` at `0x48C7A6`/`0x48CA20`/`0x48CC7A`), `EDX = tri+0x24` (v1, `0x48C6FD`/`0x48C96E`/`0x48CBD4`), `ECX = tri+0x30` (v2, `0x48C702`/`0x48C974`/`0x48CBDA`), `EAX` = out.
- **`0x48AF70`** (ESI = P, EDX = A, ECX = B, EAX = out): `a = A − P`, `b = P − B`, `out = −(a × b)` component-wise (x87, `DE E8+i` = FSUBP ST(i)−ST0, `DE E0+i` = FSUBRP ST0−ST(i); checked by emulating the instruction sequence numerically) ⇒ **`out = (A − P) × (B − P)` = (v1 − v0) × (v2 − v0)**.
- **No flip anywhere:** `0x48C5E0` normalises it (`0x77F3A0`, (0,1,0) if length 0) and stores it at slot+0x14 unchanged (FCHS at `0x48C7E4`/`0x48C7FF` etc. only touch the plane constant and hit coordinate). `0x797E10` copies whole slots (8 dwords). `0x796B20` reads `W+0x144` (normal.y) raw for `> 0.78`; `0x796CA0` takes the branch by the sign of the normal along the slot axis (−X slot: `−n.x < −0.2` = facing the Chao → push if `r − d > 0`; `> 0.2` = facing away → the `d + r < 8.0` branch), so a reversed winding would change wall behaviour.
- land.rs `unit(cross(v1 − v0, v2 − v0))` with the `(t + flip) & 1` strip rule and quad `(b, d, c)` **matches the game**. Cross-check: every Hero floor/water sampled above has n.y = +1 and the underside of the platform at (−5..5, −75) has n.y = −1.

## Unknowns
- Which candidates the Chao actually sees: the cache is built around the **players** (`0x47D160`, range 400, list max 0x80/0x400/0xC0). Checked over the grid's valid area (x −150..150, z −200..150, y 0 player): Neutral and Dark keep every collision COL wherever the player stands (margin −90 / −14); **Hero can drop COL #76** when the player is near (150, −200) (margin +34), so a Chao near #76 would then lose that geometry. Not verified at runtime.
- `0x4974F0` allocator: Y-bucketing (`0x19EDC00` table, 0x100 dwords, cleared per node) and pool exhaustion behaviour (returns 0 → `0x48B610` aborts the node) not traced.
- COL flag bits other than 1/2/0x2000/0x8000000/0x10000000 (e.g. `0x20`, `0x80`, `0x1000`, `0x400000`, `0x20000000`, `0x40000000`) — likely surface/sound/camera types; their consumers were not traced. `0xCB`/`0xCC` "climbable" signals in chao-ai §5.6 come from objects, not these flags (unverified).
- `0x48C5E0` slot fields +8/+0xC (atan2 angles) and the slot "+4 = COL flags" interpretation feeding `0x797E10`'s water test were read from the decompile only.
- The dumped exe's matrix stack base when `0x48BE80` runs is reset to identity per node; confirm no global garden offset is applied elsewhere (none seen).
- Grid cross-check (value-1 cells all over solid floor, value-9 cells over water) was done only by eye for Neutral; a full per-cell check needs the three grids exported from the exe.
- `0x47D7F0` (hands out an NJS_OBJECT for dynamic collision, used by stage objects e.g. `0x5B3470`) and the `0x194A208` one-shot rebuild key in `0x48BE80` were not traced; neither matters in the gardens (§7.2).
- `0x780310` (distance used by `0x796CA0`) not decoded, so the exact push distance for back-facing walls (the `d + r < 8.0` branch) is unverified.

## 8. Climb ledges (C_CLIMB) and touch tests

### 8.1 C_CLIMB object (`0x55AB20`, one function for all three gardens)

| Step | Address | What it does |
|---|---|---|
| Range check | `0x55AB30..0x55AB48` → `0x488C80` | `dist = *(task+0x30)+0xC` (SET status record, see 8.3); if out of range the task is dropped (spawn/keep radius² = `*0x1613D04` = 160000 → **400** units from the player, +100 on the squared keep distance, `0x4885E2`, `0x904E30`). Record flag `+2 & 8` skips the check. |
| Action 0 (init) | `0x55AB65..0x55AB7F` | `0x47E520(EAX=task, data 0x1306110, count 1, list 4)` copies the 0x30-B entry into a new CollisionInfo (`Data1+0x2C`, `+0` = list id, `+6` = count, `+0xC` = entries, `+0x9C` = task). Then `0x6E53C0(EAX=Data1)`: entry `+0x14/+0x18/+0x1C` = `Data1+0x20/+0x24/+0x28` (SET scale) **+ 10.0** (`0x904F48`, double), then `0x486240` (bounding radius `ci+8`). Action = 1. |
| Action 1 (every frame) | `0x55AB5C` → `0x47E750` | Clears the hit list (`0x47E6C0`) and appends the task to list 4 (`0x19468B8[]`, count `0x1946AC0`, max 0x80). No other per-frame work. |
| Other actions | `0x55AB57` | Reset to 0. |

**Collision entry `0x1306110`** (0x30 B): `CB 03 70 0C`, `+4 = 0`, centre `+8..+0x10 = (0,0,0)`, a/b/c `+0x14..+0x1C` = 40/30/20 (overwritten by scale+10), `+0x28` (angle add) = 0.
- byte0 **kind 0xCB**; byte1 **shape 3 = box** (bound radius `√(a²+b²+c²)` in `0x486240`, jump table `0x4863AC`: 0 sphere, 1/2 cylinder, 3/4/5 box, 6 capsule); byte2 `0x70` = push mask; `+4` flags 0 (not disabled, no 0x10).
- Box half-extents = `(scale.x+10, scale.y+10, scale.z+10)` **signed** (negative scale shrinks it; Neutral uses −5 → 5). Centre = Data1 pos (entry centre 0). Orientation = **Y only**: `rotY = Data1+0xC + entry+0x28`; X/Z rotation is ignored by the box test (`0x483FDD..0x483FF2`).
- **Not solid:** the push step `0x47EB30` (EAX = sphere ci, ECX = box ci, EDX = sphere entry, EDI = box entry) needs `box.byte2 & T1[sphere list]` ≠ 0 (`T1 = 0x171B2C4`: `01 00 02 02 04 04 08 08 FF 01`, list 9 → 1); `0x70 & 1 = 0` → no push. C_CLIMB is a pure trigger. (For comparison the `0x77` objects below are solid to the Chao.)

### 8.2 How the touch is produced and read

- **Chao collision** (`0x550334`: `0x47E520(task, 0x13134D0, 5, list 9)`; list 9 = `0x1946618[]`, count `0x1946ABC`, max 16):

| # | kind | shape | centre | a (r) | b | c | `+4` flags | vs list 4 |
|---|---|---|---|---|---|---|---|---|
| 0 | 0x91 | 1 cyl | (0,2,0) | 15 | 4 | 0 | `0x782008` | masked (`& 0x100000`) |
| 1 | **0x81** | **0 sphere** | **(0,2,0)** | **1.5** | 2.5 | 0.7 | `0x2400` | **tested** |
| 2 | 0x92 | 1 cyl | (0,2,0) | 5 | 4 | 0 | `0x782008` | masked |
| 3 | 0x95 | 0 sphere | (0,2,3.7) | 2.8 | | | `0x2410` | disabled (`0x10`) |
| 4 | 0 | 0 sphere | (0,2,0) | 1.4 | | | `0x2410` | disabled |

- **Pair filter** `0x485850(A, B)`: broad phase `0x47EA90` (|posA−posB|² < (ciA+8 + ciB+8)²); per entry pair skip if `M[listA] & entryB+4`, `M[listB] & entryA+4` (`M = 0x171CA7C`: list 4 → `0x100000`, list 9 → `0x400000`), or either `+4 & 0x10`; then `0x485620` dispatches by shapes. List 9 × list 4 runs in `0x485C70` (driver `0x4861D0` → `0x485920, 0x485B20, 0x485C70, 0x485E10, 0x485EF0, 0x485F50`). Only Chao entry 1 can meet a C_CLIMB.
- **Sphere × box `0x483E90`** (args: sphere owner, sphere entry, box owner; EAX = box entry; called from `0x48571F` and `0x4857A2`): `S` = sphere world centre (`0x47F020`: entry centre through the Chao's Data1 matrix → `pos + R·(0,2,0)`), `C` = box centre (`0x47F320`), `d = S − C`, rotated by `RotY(−rotY)` (`0x4275B0`, sin `0x42AAB0` / cos `0x42AC30`, applied by `0x4273B0`; base matrix `0x25F02A0` taken as identity):
  `lx = cos(ry)·dx − sin(ry)·dz`, `lz = sin(ry)·dx + cos(ry)·dz` (ry in BAMS, ×2π/65536).
  Three branches (top `0x48401F`, bottom, side); each needs `0x47FD60`: **`|lx| < r + hx` and `|lz| < r + hz`** (strict, square corners, no rounding), and together they reduce to **`S.y − r < C.y + hy` and `S.y + r > C.y − hy`**. Every branch calls `0x47EC50` (hit registration) **before** the push check, so touching does not depend on pushing.
- **Hit registration `0x47EC50(objA, entryA, objB, entryB)`**: unless `entryA+4 & 8` or `entryB+4 & 4`, sets `ciA+4 |= 1` and writes the first free 8-B slot of `ciA+0x10..+0x90` (16 slots; free = byte1 `0xFF`): `{u8 own entry index, u8 other entry index, ptr other task}`; same for B. Full list → dropped. Lists are cleared each frame by `0x47E6C0` (via `0x47E750` when the owner re-registers).
- **`0x486760(obj, kind)`**: needs `ci+4 & 1`; walks the slots from the cursor (`0x4860C0`, reset by a new kind / `0x54B1F0`), stops at byte1 `0xFF`, returns the first slot whose other task's entry[byte1] byte0 == `kind` (and other task/Data1/ci non-null). Callers then require `slot+4 != 0`.
- **`0x54B1F0(ECX = own index, EDX = obj, push kind)`** → `0x486880`: same, but also needs slot byte0 == own index (the walk step passes 1 → Chao entry 1), returns the other task or 0.

**Touch test** (per frame, per Chao): `S = P + (0, 2, 0)`, `r = 1.5`; touched if any box satisfies the XZ and Y conditions above. Then the existing rules apply (chao-ai §5.5/§5.7: wall bit `W+0x40 & 0x4000` from the LandTable walls **and** a 0xCB touch → climb `0x562EB0`).

### 8.3 SET files and where they live in memory

- **Loaded files** (`0x488F60`, name from `0x54B190` with the extension cut): `set_chao_X_s.bin` then `set_chao_X_u.bin` (`_hd_`/`_2p_` variants tried only for 2P or `*0x174AFE3 == 4`, none exist). The plain `set_chao_X.bin` is **not** loaded. `0x488DD0` allocates `max(count·0x20, size_s+size_u)`, copies `_s` (header + entries) then `_u`'s entries, header count = `s.count + u.count` (so merged index `i` = `_s#i`, then `_u` from `s.count`).
- **Format** (big-endian on disk): header 0x20 B (`u32 count` + 0x1C padding), entries 0x20 B: `+0 u16 id` (type index = `id & 0x7FFF`; `_u` entries use `0x8000`), `+2/+4/+6 s16 rotX/rotY/rotZ` (BAMS), `+8..+0x10 f32 pos`, `+0x14..+0x1C f32 scale/params`.
- **In memory** (`0x487FC0` byte-swaps every field in place; `0x429840` = big-endian-host check, false on PC): LE after load. Pointers: `*0x1DDE274` = header (current garden), `*0x1DDE278` = first entry, `*0x1DDE268` = object list; per garden also `*0x1AED224` (Neutral, `0x54C827`), `*0x1AED228` (Hero, `0x54D25C`), `*0x1AED220` (Dark, `0x54B77C`).
- **Object lists** (`{u32 count, ptr}` → 0x10-B types `{u8, u8 list, u16 flags, f32 dist, ptr main, ptr name}`): Neutral `0x121C8B0` (12, C_CLIMB = **6** "C CLIMB"), Hero `0x121AE90` (14, C_CLIMB = **13**), Dark `0x125B4F0` (18, C_CLIMB = **13**). All flags 0 except Hero KUSA C (1, dist 250000).
- **Spawn** (`0x4883D0`, status records `0x1DDE280 + 0x10·i`, filled by `0x488930`: `+0` count, `+2` flags (`0x8000` live, bit 0 spawned, bit 1 = record carries its own copy at `+0xC`), `+4` task, `+8` SET entry, `+0xC` keep distance): `Data1+8/+0xC/+0x10 = (s32) rotX/Y/Z`, `+0x14..+0x1C = pos`, `+0x20..+0x28 = scale` (`0x48859E..0x4885D8`), `task+0x30` = record.

**Reading the C_CLIMB boxes from live memory (in a garden):** read `hdr = *0x1DDE274`, `n = *(u32*)hdr`, `e = *0x1DDE278`, `list = *0x1DDE268`; climb index = 6 if `list == 0x121C8B0`, else 13 (`0x121AE90`, `0x125B4F0`); for `k < n` with `(*(u16*)(e+0x20k) & 0x7FFF) == idx` send `{pos = f32[3] @+8, rotY = s16 @+4, half = f32[3] @+0x14 + 10}`. The live tasks are only spawned within 400 units of the player, so they don't cover every box (from a task: record `+4` task → `+0x34` Data1 → `+0x2C` ci → `+0xC` entry with the final half-extents at `+0x14..+0x1C`).

### 8.4 Other 0xCB / 0xCC sources in the gardens

From a scan of the exe for 0x30-B entries with byte0 0xCB/0xCC and every SET type's `0x47E520` table:

| Garden | Object (main) | Table | Kind / shape | Geometry (local) | Solid to Chao? |
|---|---|---|---|---|---|
| all | C_CLIMB `0x55AB20` | `0x1306110` | 0xCB box | half = scale+10 | no (`0x70`) |
| Hero | HASIRA `0x5A7A60` | `0x8A1770` ×1 | 0xCC capsule (6) | centre (0,20,0), radius 5, half-length 20 | yes (`0x77`) |
| Hero | HASIRAKAKE `0x5A7AA0` | `0x8A17A0` ×5 | 0xCC capsule (0,14,0) r5 hl10 + spheres r5 at y 3/10/15/20 | follows the object's X/Y/Z rotation (SET has tilted ones) | yes |
| Dark | FENCE L `0x5A61E0` | `0x8A18F0` | 0xCC box | (0,10,0), half 14.2/10/0.8 | yes |
| Dark | FENCE M `0x5A6220` | `0x8A18C0` | 0xCC box | (0,5,0), 5.3/15/0.8 | yes |
| Dark | FENCE S `0x5A6260` | `0x8A1890` | 0xCC box | (0,7,0), 8/7/0.3 | yes |
| Dark | HAKA A `0x5A62A0` | `0x8A1920` | 0xCC box | (0,5.8,0), 4.5/5.8/1.5 | yes |
| Dark | HAKA B `0x5A62E0` | `0x8A1950` | 0xCC box | (0,7,0), 4/7/1.5 | yes |

- All on list 4, flags `+4 = 0` (HASIRAKAKE `0x200`). Capsule test `0x484460`: segment `centre ± b·axis(local Y)`, touch if distance < a + r. Box half-extents here are fixed (no scale+10; their mains don't call `0x6E53C0`).
- No 0xCB/0xCC in Neutral's SET types. C_SPHERE/C_CYL/C_CUBE/WALL_PLANE (`0xB14570..`), LOBBY/ENTRANCE (`0x1305F00`), C_CUBE2 (`0x13060E0`), KIRIKABU/KUWA/KUSA/SKULL (`0x8A1980`) all have kind 0.
- Instances (merged index): Hero `_u#2` HASIRA, `_u#6`/`_u#8` HASIRAKAKE; Dark FENCE S `_u#32..34, 36..38`, FENCE L `_u#35`, HAKA A `_u#10, 23`, HAKA B `_u#22, 24, 26, 27`. They are object colliders, not part of the LandTable: the Chao is pushed out by them and also turns away on the 0xCC signal (chao-ai §5.3). The server gets them as blockers in `GardenLand` (`chao_sim::land`).

### 8.5 C_CLIMB entries (from `resource/gd_PC/set_chao_*_s.bin`; none in `_u`)

Half = scale + 10. ry = SET rotY.

Neutral (type 6; all `rx = 0x0056` ≈ 0.5°, ignored):

| # | ry (BAMS / deg) | pos (x, y, z) | scale | half (hx, hy, hz) | y range |
|---|---|---|---|---|---|
| _s#3 | `8A41` / −165.6 | −88.89, 1.10, −29.63 | 3, −5, −5 | 13, 5, 5 | −3.9..6.1 |
| _s#4 | `9C41` / −140.3 | −55.52, 1.10, −46.37 | 17, −5, −5 | 27, 5, 5 | −3.9..6.1 |
| _s#5 | `BA41` / −98.1 | −33.37, 1.10, −93.19 | 17, −5, −5 | 27, 5, 5 | −3.9..6.1 |
| _s#6 | `DC41` / −50.3 | −53.50, 1.10, −143.63 | 24, −5, −5 | 34, 5, 5 | −3.9..6.1 |

Hero (type 13):

| # | ry (BAMS / deg) | pos (x, y, z) | scale | half (hx, hy, hz) | y range |
|---|---|---|---|---|---|
| _s#5 | `A2AA` / −131.3 | 109.50, −2.00, −106.33 | 22, −8, 32 | 32, 2, 42 | −4.0..0.0 |
| _s#6 | `B8A3` / −100.4 | −11.50, −9.00, −83.59 | 20, 0, 0 | 30, 10, 10 | −19.0..1.0 |
| _s#7 | `CAC3` / −74.9 | −8.38, −11.00, −20.84 | 20, 0, 0 | 30, 10, 10 | −21.0..−1.0 |
| _s#8 | `E5C3` / −36.9 | 18.06, −11.00, 18.63 | 20, 0, 0 | 30, 10, 10 | −21.0..−1.0 |
| _s#9 | `FAC3` / −7.4 | 66.73, −11.00, 37.59 | 20, 0, 0 | 30, 10, 10 | −21.0..−1.0 |
| _s#10 | `0CC3` / 17.9 | 107.99, −11.00, 27.67 | 20, 0, 0 | 30, 10, 10 | −21.0..−1.0 |
| _s#11 | `1CC3` / 40.4 | 152.29, −11.00, 5.74 | 10, 0, 0 | 20, 10, 10 | −21.0..−1.0 |

Dark (type 13):

| # | ry (BAMS / deg) | pos (x, y, z) | scale | half (hx, hy, hz) | y range |
|---|---|---|---|---|---|
| _s#0 | `AC63` / −117.6 | 28.12, −10.00, −118.47 | 20, 0, −6 | 30, 10, 4 | −20.0..0.0 |
| _s#1 | `BA73` / −97.8 | 5.97, −11.00, −71.40 | 19, 0, −6 | 29, 10, 4 | −21.0..−1.0 |
| _s#2 | `C173` / −88.0 | 3.90, −11.00, −21.08 | 19, 0, −6 | 29, 10, 4 | −21.0..−1.0 |
| _s#3 | `E573` / −37.3 | 22.83, −11.00, 14.81 | 19, 0, −3 | 29, 10, 7 | −21.0..−1.0 |
| _s#4 | `0673` / 9.1 | 71.62, −11.00, 23.98 | 19, 0, −3 | 29, 10, 7 | −21.0..−1.0 |
| _s#5 | `0973` / 13.3 | 123.05, −11.00, 8.29 | 19, 0, −3 | 29, 10, 7 | −21.0..−1.0 |
| _s#6 | `0973` / 13.3 | 169.27, −11.00, −1.84 | 19, 0, −3 | 29, 10, 7 | −21.0..−1.0 |
| _s#7 | `4273` / 93.4 | 86.38, −11.00, −126.50 | 6, 0, −2 | 16, 10, 8 | −21.0..−1.0 |
| _s#8 | `5C73` / 130.0 | 94.68, −11.00, −103.43 | 6, 0, −2 | 16, 10, 8 | −21.0..−1.0 |
| _s#9 | `6F73` / 156.7 | 113.92, −11.00, −87.27 | 6, 0, −2 | 16, 10, 8 | −21.0..−1.0 |
| _s#10 | `7F73` / 179.2 | 140.54, −11.00, −83.25 | 6, 0, −2 | 16, 10, 8 | −21.0..−1.0 |

**Pond-edge check** (LandTable floors at local `(0, ±(hz+4))` with the rotation above): in all 22 boxes one side is the pool and the other land, with the long axis (local X) along the shore — Hero/Dark: local −z = water (−1.5 / −1.0 over floor −6.5 / −6.0), +z = land ≈ 0; Neutral: +z = water (−2.0 over −7.0), −z = land ≈ 0.1. The consistent pattern over 22 different angles also confirms the rotation sign. Exceptions: Hero `_s#5` (flat 64×4×84 slab, top y 0, water on both ±z ends, land 13.0 at its centre: a shallow ramp/island edge) and Hero `_s#6` (land side is a 33.8 cliff top, so this one only lets a Chao start a climb that `0x562EB0` ends after 30 frames / 2.5 rise).
- Hero/Dark boxes top out at −1.0 (0.5 above Hero's water, level with Dark's), so a Chao on land (sphere bottom = y + 0.5 ≥ 0.5) never touches them: they only fire for a Chao in the water. Neutral boxes reach 6.1, so they also fire for a Chao walking on the shore.

### 8.6 Unknowns

- `0x25F02A0` (pushed as the base matrix in `0x483E90`/`0x47F320`) assumed identity (consistent with 8.5's check; not read at runtime).
- The Chao's sphere centre uses its full Data1 matrix (`0x47F020`); `(0,2,0)` assumes rotX/rotZ ≈ 0.
- Swimming Chao height vs the box tops (needs Chao `y < top − 0.5`: −1.5 Hero/Dark, 5.6 Neutral) not checked against the swim code's actual Y.
- Hit-list overflow (16 slots) and the `0x4860C0` cursor reuse across calls of the same kind are ignored above.
