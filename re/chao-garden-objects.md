# Garden objects: drawing a model, placing it and using it

Addresses are in the Steam `sonic2app.exe` (image base 0x400000, dump `re/bin/sonic2app.dumped.exe`).
How the gardens' ground is read is in garden-collision.md; skins' models in skins.md; the
garden buttons in chao-details.md "Controller buttons in the garden".

## 1. Chunk models as the game draws them

### Draw path
- `draw_object` **0x42E730** (cdecl `NJS_OBJECT*`) draws a whole tree with the current matrix
  and the texture list at render state `*0x2670544 + 0x20`. Each node's model goes through
  **0x42D500** (cdecl `NJS_CNK_MODEL*`): the vertex list into the chunk vertex buffer
  (0x42D190), then the polygon list (0x42CAD0).
- `puppet::draw_solid` wraps it: push a matrix, translate, rotate Z, X, then Y by
  `0x8000 - y`, scale, draw, pop, with its own texture list swapped in. A caller that wants the
  model's +Z turned by `yaw` passes `0x8000 - yaw` (billboard.rs does).
- In a display callback the matrix stack's top (`*0x1A557FC`) is the view matrix.

### NJS_OBJECT (0x34 bytes)
| Off | Field |
|---|---|
| +0x00 | eval flags: 1 no translate, 2 no rotate, 4 no scale, 8 hide this node's mesh, 0x10 no children; rotation Z, Y, X, or Y, X, Z with 0x20 (skins.md) |
| +0x04 | `NJS_CNK_MODEL*` (null: a node without a mesh) |
| +0x08 | position f32[3] |
| +0x14 | angles s32[3] (BAMS) |
| +0x20 | scale f32[3] |
| +0x2C | child |
| +0x30 | sibling |

A single-mesh object is flags 0x17 with unit scale (billboard.rs `model`); a converted tree is
a hidden root (0x0F) whose child chain holds the pieces (tools/skinconv.py).

### NJS_CNK_MODEL (0x18 bytes)
`+0 vlist*`, `+4 plist*`, `+8 centre f32[3]`, `+0x14 radius`. The node draw 0x42E660 tests the
sphere against the view unless the radius is 0.0; the mod's models use 0 (skins.md).

### Vertex list (u32 stream, 0x42D190)
Each block: `h1 = type | flags << 8 | words << 16`, `h2 = offset | count << 16`, then the
vertices; `words` counts the u32s after `h1`. The list ends with a block whose low byte is
0xFF (`0x000000FF`). A block fills slots `offset .. offset + count` of the vertex buffer
(`*0x25EFE48`, 8192 slots of 32 bytes); strips index those slots, so a later node may use an
earlier node's vertices.

| Type | Handler | Per vertex | Notes |
|---|---|---|---|
| 0x29 VN | 0x42E440 | position f32[3], normal f32[3] | `words = 1 + 6n`. The only type billboard.rs and skinconv.py write. |
| 0x2C VN_NF | 0x42ED60 | position, normal, u32 slot \| weight << 16 | weighted (skins.md "Weighted vertices") |
| 0x2A VN_D8 | 0x42F4F0 | position, normal, u32 ARGB colour | sets `*0x19346DC \|= 2` |
| 0x23 V_D8 | 0x42ECD0 | position, u32 ARGB colour | sets `*0x19346DC \|= 3` |

Any other type is skipped without drawing. skins.rs's checker accepts 0x29 and 0x2C only.

### Polygon list (u16 stream, 0x42CAD0)
| Head low byte | Size | Meaning |
|---|---|---|
| 0x01 | 1 word | blend: `head & 0x3F00` becomes the blend mode (`*0x25F0264`) |
| 0x02, 0x03 | 1 word | skipped |
| 0x04, 0x05 | 1 word | cache / draw a cached polygon list (`*0x2670340[head >> 8]`) |
| 0x08 .. 0x0F | 2 words | tiny: texture. Word 2: id `& 0x1FFF` (index in the current texture list), 0x4000 bilinear. Head flags: bits 8..11 mipmap adjust, 0x1000 clamp V, 0x2000 clamp U |
| 0x10 .. 0x3F | 2 + size words | material (0x42C3F0); also sets the blend from `head & 0x3F00` |
| 0x40 | 2 + size words | strips, index only |
| 0x41 | 2 + size words | strips with UVs, 256 = 1.0 |
| 0x42 | 2 + size words | strips with UVs, a separate precision flag (0x42CAD0 keeps `type == 0x42` per strip set) |
| 0xFF | | end |

- **Material 0x11** (diffuse only): `0x11 | blend << 8`, size 2, then the diffuse colour as a
  u32 ARGB. Blend 0x25 is source alpha / inverse source alpha (source `(b >> 3) & 7` = 4,
  destination `b & 7` = 5). The game's own models also use 0x13 and 0x17 (with ambient and
  specular).
- **Strips 0x40/0x41/0x42**: `type | flags << 8`, `size` (u16 words that follow it), then
  `count | user << 14` (user words per vertex, 0 here), then per strip an i16 length (negative:
  the other winding) and that many vertices: a u16 slot, plus i16 u, v for 0x41/0x42. Strip
  flags as the mod's models use them: 0x01 no lighting, 0x02 no specular, 0x04 no ambient,
  0x08 alpha, 0x10 both sides.
- While render flag 0x800 of `*0x25F02D8` is set, strip flags become
  `(flags & *0x25F0268) | *0x25F02D4` (the overrides puppet.rs saves around a character draw).
- The strip setup 0x42CA20 draws alpha strips of a texture whose info format (+4) is 0xE with
  alpha test and depth writes; other alpha textures blend without depth writes (billboard.rs).
- The u16 stream keeps 4-byte alignment between chunks: a strip chunk with an odd word count
  gets a padding word (skinconv.py).

### What the mod's builders write
- billboard.rs `quad`: one 0x29 block of 4 vertices, then `0x2511` (material 0x11, blend 0x25,
  white), `0x3008` (texture 0, clamp U and V), one 0x41 strip of 4 with flags 0x19 (no
  lighting, alpha, both sides), end.
- tools/skinconv.py: per node one 0x29 block (offset 0, at most 500 vertices), then per
  texture `0x2511`-style material (`0x11 | 0x25 << 8`, white), tiny `0x0408` with
  `0x4000 | id`, and 0x41 strips of one triangle each with flags 0x16 (no specular, no ambient,
  both sides; 0x1E with alpha). The garden draws these lit by its own lights.
- Colour without vertex colours: flat-coloured faces take their UVs from swatches of one
  texture. A hard edge needs its own vertices per face (one normal per vertex slot).

### Textures
- **Made at runtime** (billboard.rs): RGBA8 frames → a GVM archive (`gvm`, RGB5A3) → the
  game's GVM parser **0x42FC30** (EAX = archive, EBX = an `NJS_TEXLIST` with one entry per
  texture) fills the list's entries (name, attr, texaddr). Global indices `0x7E5A0000 +` are
  far from the game's. Width and height are multiples of 4.
- **From a texture pack** (skins.rs): `<mod>/textures/<name>/index.txt` + PNGs, loaded by
  **0x44C350** (ECX = name, EDX = a list with exactly the pack's texture count).
- Either way the records belong to the game: they go with the scene (the gardens' areas
  included), so a list is drawn and released (0x77F9F0) only while its records still match how
  they were made in that scene (billboard.rs `ours`, skins.md "Texture records"). The pools
  need free records (`*0x2670590`, count `*0x267058C`, 0x18 bytes, free when +0xC is 0) and
  infos (`*0x25F0278`, count `*0x25EFFE0`, 0x28 bytes, free when +0 is -1).
- Texture ids in the polygon list index the list handed to the draw, so one list can hold a
  fixed texture at id 0 and a swapped picture at id 1 (`NJS_TEXNAME` entries are 12 bytes:
  name, attr, texaddr).

## 2. A fixed object in a garden

### The transporters (`ALO_OdekakeMachine`)
Made by **0x57E4F0** (EBX = position, angle pushed; LoadObject list 2, main 0x57E460; position
into Data1 +0x14, angle into Data1 +0xC) from each garden's constructor:

| Garden | Call | Position | Angle |
|---|---|---|---|
| Neutral | 0x54CA38 | (72, 10, 28) | 0x6000 |
| Hero | 0x54D3F4 | (135, 0, 133) | 0x616C |
| Dark | 0x54B914 | (-116, 3.5, -113) | 0xE000 |

Each stands on a flat plaza of grid value 0 (outside the cells Chao are sent to): Neutral's at
y 10 above the garden's slope (x 25..105, z 5..65), Hero's at y 0 in the garden's east corner,
Dark's at y 3.5 on the raised west side (x below -85, z below -85). The Chao land (grid 1) is
west and north of Neutral's plaza, west of Hero's and east of Dark's.

### Free ground near them
Spots 20 to 25 units from the transporter, flat (solid floor, normal y above 0.97) across a
17 x 17 footprint, no water within 14, no roof, at least 18 from a tree spot (LandTable
probes as in garden-collision.md §6b, `chao_sim::land::probe`). The angle turns +Z toward the
middle of the garden's Chao land.

| Garden | Spot | Floor | +Z toward | Angle | Nearest SET objects |
|---|---|---|---|---|---|
| Neutral | (50, 22) | 10.0 (9.7..10.0 under the footprint) | (-5, -65) | 0x96F8 | none within 40 |
| Hero | (117, 117) | 0.0 | (-75, -20) | 0xA6C0 | flowers (types 2, 3) 23 and 28 away, pillar (type 0) at (137.6, 141.8) 32 away |
| Dark | (-98, -98) | 3.5 | (-55, 0) | 0x10D9 | small fences (type 2) at (-104.6, -76.5) and (-73.9, -107.5), 23 and 26 away |

The garden's toys and placed items are only known at runtime (`garden_toys::placed`, the item
registry in chao-garden-items.md).

### Frame order
- In-stage update **0x43CE20**, per update step: **0x441BA0** (pads into the gameplay copy
  0x1DEFC00, then 0x442390 the jump and action bytes, then 0x441AB0 the menu buttons), the
  object lists **0, 1, 2, 3, 4, 5** in that order (0x46F8C0(i)), then 0x486190 and the land
  cache **0x48BE80**. The players are list 1, the garden's own objects list 2 and 4. A slow
  frame runs several update steps.
- Display **0x470010**: list 0 (with `*0x25EFEE0` = 3), list 1 (1), then the others. A list 0
  object's display callback draws in the same pass as the garden.
- **LoadObject 0x46F680**: ESI = list (0..7), EDI = main (called every update step), name
  pushed (caller pops); +0x14 display, +0x18 delete. **0x470B40** (no arguments) gives a
  zeroed 0x30-byte Data1 from the game heap for +0x34. Deleting an object (0x46F720 →
  0x46F7A8..0x46F89D) frees its collision (0x486430) and +0x34, +0x3C, +0x38 and +0x40 through
  the game allocator, so those hold only game-heap blocks or null. Every object in an area is
  deleted when the area changes (the transporter's jump 0x52B030 included).

### Collision the player stands against: dynamic land entries
The ground query (garden-collision.md §7) takes the dynamic entries first, then the LandTable;
the game's own Chao use it, and the Chao Entrance elevator the player rides is one.
- **Register 0x47D6B0**: EAX = COL flags, EDX = object (ObjectMaster), ESI = `NJS_OBJECT*`
  whose model is a **basic model**. Up to 0x100 entries; sets Data1 +4 `|= 0x100` (the object
  needs a Data1). Flag 1 is solid; 0x8000000 rebuilds its triangles every update (for a
  moving object); 0x10000000 is added by the call itself for a turned object with an
  off-centre model (`|centre|² ≥ 225`).
- **Unregister 0x47D760**: EDI = object, EBX = `NJS_OBJECT*`.
- **Reset**: loading a LandTable (0x47BD30, the garden constructors) clears the list
  (0x47BCC0), so an entry is registered after the garden is up.
- **Gather 0x47D160**: skips an entry whose object's main (+0x10) or parent's main is the
  delete stub 0x46F720 or whose Data1 +4 lacks 0x100; keeps it within `model radius + 400` of
  a player. The world triangles are the object's own pos, angles (Z, Y, X) and scale applied
  to the model (§7.1), so the `NJS_OBJECT` holds the garden position and angle.
- **Basic model** (`NJS_MODEL`, 0x28 bytes): `+0 points* (f32[3])`, `+4 normals*` (not read),
  `+8 u32 point count`, `+0xC meshsets*`, `+0x10 materials*` (0x14 bytes each), `+0x14 u16
  meshset count`, `+0x16 u16 material count`, `+0x18 centre`, `+0x24 radius`. Meshset (0x18
  bytes): `+0 u16 type << 14 | material`, `+2 u16 polygon count`, `+4 s16* indices`; type 0
  triangles, 0x4000 quads (0,1,2)+(1,3,2), 0xC000 strips (garden-collision.md §4). Each
  triangle's normal is `(v1 - v0) × (v2 - v0)`, and walls push by that side (§7.3), so faces
  wind with the normal pointing out.
- Example: the Chao Entrance elevator (0x558510): template `NJS_OBJECT` 0x11CD5EC (flags 0x16)
  → basic model 0x11CD5C4 (17 points, one 0xC000 meshset of 4 strips at 0x11CD3A0, one
  material, centre (0, 1.25, 0), radius 12.02), copied to the heap, eval flags `& ~3`,
  position from its Data1, registered with 0x8000001 (or 0x28000001/0x48000001/0x68000001 by
  height).

### Server-run Chao
A server running the garden's Chao (chao_sim) knows only what `GardenLand` carries: the
LandTable, grid, ledges, `blockers` (0xCC shapes: `Shape::Box { half }` turned by `rot_y`
about y, pushed out by `Blocker::push`) and toys. The fingerprint covers all of them.

### Light
Chunk strips without flag 0x01 are lit by the garden's lights like the game's own models
(skins and karts drawn through `draw_solid` in the gardens); flag 0x01 shows the texture's
colours as they are (a lit screen).

## 3. The action button near an object

- `0x442390` (inside 0x441BA0) writes per player `p` (byte arrays): **0x1DEFB00 + p** = 1
  when B or X (0x402) is newly pressed, 0x1DEFBF0 + p held, 0x1DEFB20 + p and 0x1DEFB42 + p
  (from the pad's +0xC and +0x14 words); and 0x1DEFAA4 + p / 0x1DEFB80 + p for A. From the
  gameplay pad copy **0x1DEFC00 + 0x34·p** (+8 held, +0x10 pressed), which 0x441BA0 zeroes
  while ControllerEnabled (0x174AFFF) is 0.
- The garden actions read these in the player's own update (list 1): 0x4756F6, Sonic's action
  code 0x7230EF and others test `0x1DEFB00 + p`; standing still the list ends with the whistle
  (chao-details.md). While holding something, `pressed & 0x602` (B, X, Y) in 0x4754F0 puts it
  down or throws it.
- So a **list 0** object runs after the pads are read and before the player: clearing
  `0x1DEFB00 + p` and the 0x402 bits of the copy's pressed word there leaves the press unseen
  by every action this step.
- Menus of the mod's own read the ungated menu buttons **0x1DEFAB0** (`game::menu_buttons_pressed`,
  `battle::arena::buttons`: 2 B, 4 A, 0x10/0x20 up/down, direction repeat after 30 frames),
  with ControllerEnabled at 0 so the player stands still (`game::set_controls`, as chat.rs).
  The battle picker (src/battle/game.rs `draw_menu`) and hud.rs (`panel`, `text`,
  `menu_label`) draw such a list; `battle::arena::sound` plays the menu sounds.
- The player: `game::player_position` (Data1 position and Y angle), CharObj2 `*0x1DE9600`
  +0x94 the object they hold (0 when empty).

## 4. The mod's link machine (src/gba_link/machine.rs)

- **Model**: tools/link_machine.py builds the three variants in Blender (Y up and front +Z on
  export, game units, a 32x32 swatch texture of 8x8 colour cells); src/bin/machineconv.rs
  (src/gba_link/machine_model.rs) writes `assets/link_machine/<variant>.lmc`, which the DLL
  embeds. Each file holds the chunk pieces (one 0x29 block of at most 500 vertices at slot 0,
  `0x2511` material, `0x3008` texture 0, 0x41 one-triangle strips with flags 0x16, padded as in
  §1), the swatch, the screen rectangle, the exit point and the collision box. The draw is a
  hidden root (0x0F) whose child chain holds the pieces (0x17), through `puppet::draw_solid`
  with `0x8000 - angle`.
- **Object**: LoadObject list 0, its main taking the action press (§3), display drawing the
  model and the screen picture (billboard.rs `draw_panel`, a quad at the screen's bottom middle
  0.06 in front, 0.9 of the screen's height, lit by nothing), delete unregistering the box.
  Data1 from 0x470B40 (angle +0xC, position +0x14). The object is checked against list 0's
  links each frame; one that is gone without its delete is forgotten.
- **Collision**: one box per garden as a basic model (8 points already turned by the angle, one
  material, one type-0 meshset of 12 triangles wound outward), its `NJS_OBJECT` at the spot with
  eval flags 0x14 and no angles, registered with flag 1.
- **Spots**: the §2 ones (Neutral (50, 10, 22) 0x96F8, Hero (117, 0, 117) 0xA6C0, Dark
  (-98, 3.5, -98) 0x10D9). In reach from in front of the box's front face (up to 4 units out,
  0.5 inside), no more than 1 unit past its sides and 5 up of the spot, facing it within 60
  degrees (the player's `Data1+0xC` against the machine's angle + 0x8000), and with nothing the
  player can pick up (registry categories 0, 1, 2, 3, 7, 9) closer than the face plus 1 unit;
  otherwise the press stays the game's.
- **Pick**: the list opens with the controls off (§3). A pick is brought in with
  `scene::arrive_at` at the model's exit point: the animal hops out along the machine's angle.

