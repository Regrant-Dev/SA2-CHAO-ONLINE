# Wild visitors: garden areas, ponds, carrying, and the GBA ROM's wild tables

Steam `sonic2app.exe`, image base 0x400000 (objdump of the dumped exe), and the US GBA ROM
(game code BPEE; offsets are file offsets, pointers are `0x08000000 +` offset). How partners
live in a garden as animal objects is in `chao-animals.md`; items, the player's hands and the
item records in `chao-garden-items.md`; the garden ground and grid in `garden-collision.md`.

---

## 1. Animal objects in a garden

- Create 0x548D30 `(i8 id, pos*, angY, vel*, save*)`; with `save = NULL` the animal has no
  record: it is never written to the Chao save and is lost when carried out (§3).
- Registry category 2 holds animals and Chaos Drives together. The add 0x530750 (CX = category)
  compares against `0x8AB838[category]` (u32 array): category 2 is `0x8AB840` = **10**. The
  count is `0x1DC0F80[category]`, category 2 at `0x1DC0F88`. When the category is full the add
  returns 0 and `obj+0x3C` stays NULL.
- The game's own animals and drives (stage animals released in the garden 0x5492B0, drives,
  Black Market deliveries 0x548F40) share those 10 entries with any animals the mod makes.
- `0x8AB838` u32 values by category: 0x20, 0x10, 0x0A, 0x30, 0x0A, 0x10, 0x20, 0x10, 0x10,
  0x20.

### Roaming and water (0x57BD40)
- State 0 enables collision entries 0 (only while `+0xF4` is 0) and 1, disables entry 2, then
  runs the shared garden physics 0x53E020 (ESI = object): gravity and the LandTable ground. Water
  is never floor, so an animal in a pond sinks to the pond floor.
- Before stepping ahead it looks up the grid value 3 units ahead (0x535010, EAX = &point): value
  9 (water), or 8 while `Data1.y` is below the water surface (`Data2+0xDC`), makes it turn
  instead. A walking animal keeps to the land cells.

### Water surface per garden
| Garden (`CurrentChaoArea`) | Surface y | Pond floor y | Water COL centre, radius |
|---|---|---|---|
| Neutral (1) | −2.0 | about −7.0 | (−78.1, −98.6), 74.1 |
| Hero (2) | −1.5 | about −6.5 | (76.9, −67.7), 124.3 |
| Dark (3) | −1.0 | about −6.0 | (92.7, −56.9), 112.6 |

A Chao is in water when `y + 2.0 < surface` (0x561630; Neutral uses −2.2 there).

### Spawn cells
The grid (`GRID_ADDRESS`, 32 × 40 i32, 10-unit cells) has value 1 for land and 9 for water;
cell (col, row) has its middle at `x = (col − 16 + 0.5) × 10`, `z = (row − 20 + 0.5) × 10`. Against
the LandTable (`chao_sim::land` test `the_grids_agree_with_the_ground`):

| | Land cells on solid ground | Water cells over water |
|---|---|---|
| Neutral | 288 / 288 | 45 / 45 |
| Hero | 299 / 316 | 55 / 86 |
| Dark | 184 / 185 | 59 / 59 |

Hero's 31 water cells around (0..25, 5..45) and (−35..−5, 85..115) are dry floor at y = 0, and
its land cells at x −5..15, z −125..−25 (and Dark's (15, −105)) are open water along the shore.
A cell whose middle the LandTable probe (`chao_sim::land::probe` from y 60 and 180, highest hit)
finds solid is a land spot; one whose highest hit is water with the floor at least 3 below it is
a water spot. The game's own 15 random spawn points per garden (16 slots of 12 bytes from
0x1366260, `(garden − 1) × 16 + n`) are all on land.

---

## 2. Absorbing (the partners' hook)

The absorb branch at 0x56708E (EBP = Chao, EDI = animal) jumps to the mod's hook in
`gba_link/scene.rs`, which asks whether the animal is one of the mod's objects and, if so,
applies its effect and goes on at 0x5674CC (motions, sparkles, sounds and the status balloon
stay; the animal effects are skipped). Any other animal object the mod makes goes through the
same site. A Chao only notices an animal on its own while collision entry 2 is enabled
(0x486CD0, EAX = object, ECX = 2); the game enables it only while the player carries it
(mode 1), and the partners' main wrapper also enables it while roaming with the player's hands
empty.

---

## 3. Carrying an animal to another area

- The player's held object is `CharObj2(*0x1DE9600)+0x94`; `+0x98` is the one about to be
  picked up. The held object's `Data1+4 & 0x8000` is set.
- GoToChaoArea 0x52B5B0 (cdecl area), before the area is torn down: if the player holds an
  item, `*0x19F6450 = its category` and `*0x19F6454 = its registry entry +0x08` (the record).
- 0x52F2A0 runs in every Chao World area constructor (calls at 0x54B8E8, 0x54C485, 0x54C9D1,
  0x54D3C8, 0x54E511) and re-creates the carried item:
  - Nothing when the record is NULL (`test esi, esi`) or its kind is negative: **an animal
    without a record is lost** at the area change.
  - Position = player position + 3.0 × (sin, 0, cos) of the player's `Data1+0xC` (0x42AAB0 /
    0x42AC30 with ECX = angle; 3.0 is the double at 0x904DD8), y unchanged.
  - Drops what the player holds (0x46E5E0, EAX = 0, EDX = player Data1) and clears
    `CharObj2+0x94`.
  - Animal kinds below 0x15: `0x548D30(kind, &pos, player angle, &zero, record)`; drives
    0x545600.
  - Writes the garden to the record, then **0x46E5B0** (ECX = player index, push object;
    caller pops 4): when `CharObj2+0x98` is 0 it sets player `Data1+4 |= 0x1000`, action
    `Data1+1 = 0x1A` and `CharObj2+0x98 = object`, and the player picks it up.
- So an animal made without a record is handed to the player in the next area by doing the
  same after the area is up: create it 3 units in front of the player and call 0x46E5B0. In the
  new garden a Chao notices it in the player's hands as with any carried animal.
- The mod's partner objects are deleted when the player leaves the gardens; list-2 objects are
  otherwise kept across areas.

---

## 4. A title on garden entry

`hud::center` / `hud::center2` draw large centred text with the menu panel behind it for the
frame they are called in (the Mod Loader's debug text, printable ASCII only); a title stays up
by calling it every frame for a few seconds. `hud::toast` queues a small top-left line for 5
seconds instead. The garden is up when `game::in_garden()` and not `game::between_areas()`.

---

## 5. The ROM's wild tables and areas

### Layout
- Wild headers (pret `gWildMonHeaders`) at 0x552D48: 20 bytes {map group, map number, pad,
  then u32 pointers to the land, water, rock and fishing tables}, ended by group 0xFF. 124
  headers; map (24, 106) has 9 variant headers, of which the game uses the first unless an event
  changes it.
- Each table is {u8 rate, pad, u32 slots}; a slot is {u8 min level, u8 max level, u16 species}.
  Chances: land 20, 20, 10, 10, 10, 10, 5, 5, 4, 4, 1, 1; water and rock 60, 30, 5, 4, 1;
  fishing 70, 30 | 60, 20, 20 | 40, 40, 15, 4, 1 (three rods).
- Map headers (via the group table at 0x486578): +0x00 layout pointer, +0x14 region section id,
  +0x15 cave flag, +0x16 weather, +0x17 map type (1 town, 2 city, 3 route, 4 underground,
  5 underwater, 6 ocean route, 8 indoor), +0x1B battle scene.
- Layout {s32 width, s32 height, border*, u16 blocks*, primary tileset*, secondary tileset*};
  tileset +0x10 = metatile attributes (u16 each). A block's metatile is `block & 0x3FF`
  (below 512 from the primary tileset, else the secondary at `id − 512`); its behaviour is
  `attribute & 0xFF`. Behaviours: grass 0x02, 0x03, 0x07, 0x09; sand 0x06, 0x21, 0x25;
  ash grass 0x24; cave floor 0x08; water 0x10, 0x11, 0x12, 0x14, 0x15, 0x17, 0x18, 0x19,
  0x1A, 0x22, 0x2A.
- Section names (8-byte entries at 0x5A147C, +4 name pointer) give the shown name.
- Shiny palettes (pret `gMonShinyPaletteTable`) at **0x304438**, right after the normal table
  (0x303678 + 440 × 8): 8 bytes {u32 pointer to LZ77 BGR555 colours, u16 tag = species + 500,
  pad}, same format as the normal palettes. The game's shiny odds are 8 in 65536.
- Species types are at `SPECIES_INFO + id × 28 + 6` (two type ids).

### Areas and garden themes
An area is a region section: the union of the first wild header of each of its maps. 64
sections have wild creatures. Each goes to one garden by the first rule that matches:

1. No land table on any of its maps (water and fishing only) → **Hero**.
2. A map of type 5 (underwater) or 6 (ocean route) → **Hero**.
3. A map of type 4 (underground) or 8 (indoor) → **Dark**.
4. Ash grass (0x24) is at least 5% of its maps' blocks, or type ids 4 (ground), 5 (rock) and
   10 (fire) make up at least half of its land slots by chance → **Dark**.
5. Water and sand behaviours are at least 25% of its maps' blocks, or type ids 2 (flying) and
   11 (water) make up at least half of its land slots by chance → **Hero**.
6. Otherwise → **Neutral**.

A slot's chance counts half for each type of a two-type species. Section 85 is an open-air tower
mapped as type 4: it goes to Hero.

| Theme | Sections | Distinct species sets (land + water + fishing) | Species |
|---|---|---|---|
| Neutral | 13 | 12 | 73 |
| Hero | 32 | 15 | 36 |
| Dark | 19 | 19 | 55 |

Without the fishing tables Hero has 11 distinct sets: most of its water-only routes and towns
share one three-species surf table.

- Neutral: section ids 16, 17, 18, 29, 31, 32, 33, 34, 35, 36, 38, 57, 59.
- Hero: 2, 6, 7, 8, 12, 13, 14, 15, 19, 20, 21, 22, 23, 24, 25, 30, 37, 39..49, 50, 51, 61, 85.
- Dark: 26, 27, 28, 55, 60, 62, 63, 65, 67, 68, 70, 72, 74, 76, 198, 199, 202, 209, 210.

Rule hits: rule 1 takes 27 sections, rule 2 one, rule 3 fifteen (plus section 85), rule 4 four
(a desert, an ash route and two volcano routes), rule 5 three (two shore routes and one route
of flying creatures), rule 6 thirteen.

---

## 6. The mod's wild visitors (`src/gba_link/rotation.rs`, `wild.rs`)

- **Areas** (`rotation::areas`): the sections above, in the order the wild headers first name
  them, each with its maps' land, water, rock and fishing tables (each rod's table on its own),
  and the theme by the rules of §5. Map types and block behaviours come from
  `gba::terrain` (map header +0x17; layout and tileset attributes).
- **The visit's area** (`rotation::pick`): with `n` areas for the garden, visit `v` is
  position `v mod n` of a Fisher-Yates shuffle of them seeded by splitmix64 of
  `(v / n) << 8 | garden`, so each area comes once per round of `n` visits. When a round's
  first area is the one the round before ended on, its first two swap, so no area comes two
  visits running; with 2 areas they alternate, `(v + garden) mod 2`. `v` goes up by one each
  time the player walks into that garden, from a random start each time the game starts.
- **Spots** (`wild::Spots`): land cells whose middle's highest probe hit is solid, and water
  cells whose highest hit is water with solid floor at least 3 below it (the surface height is
  kept per cell). Visitors come into the pond at water cells with at least three water
  neighbours.
- **Picks** (`wild::pick`): a table at random among the area's tables of the kind, then a slot by
  its chance, level uniform in the slot's range, ability slot 0 or 1 at random, shiny 1 in 1024.
  Land: the land tables, or a rock table one time in four when there is one. Pond: the water
  tables, or the fishing tables one time in four. In an area without land tables, type id 2
  creatures of the water and fishing tables go to the grass and the others to the pond.
- **Objects**: each visitor is an animal of id 3 with no record (as partners), made only while
  the category-2 count plus 2 plus the garden's waiting partners that have no object yet is
  below the limit at 0x8AB840 (2 entries stay free for the game's own animals and drives, and
  the partners have theirs). A partner picked at the link machine when that leaves no room has
  the roaming visitor farthest from the player go first (it is deleted); only with no visitor to
  go does it take one of the 2 entries. A carried visitor handed over in the next garden does
  the same. The main wrapper runs the animal's own main, silenced: the animal code's five calls
  to the positional sound call 0x4372E0 (0x57B43D, 0x57BF3B, 0x57C4B3, 0x57C5FB, 0x57C6E9:
  ids 0x1011, 0x1010 twice, 0x1031, 0x1020) go through a check that skips them while one of our
  creatures' mains runs (partners too); in mode 0 a visitor in a
  pond cell below that cell's surface swims: its position is set each frame to the surface
  − 1.5 (bobbing 0.2), moving 0.06 a frame between neighbouring pond cells, its `Data1+0xC`
  turned along its way and its Data2 velocity zeroed. The absorb hook (§2) treats a visitor as a
  partner, and what it teaches goes to `partners.txt` only then.
- **Carrying**: while the player holds a visitor its data is kept; leaving the garden with it
  in hand keeps it through Chao World's other areas, and in the next garden it is made 3 units in
  front of the player and handed over with 0x46E5B0. Leaving Chao World drops it.
- **Shared gardens**: a garden counts as shared while another player's state puts them in it or
  the server runs its Chao (`garden::server_runs`). There no visitor comes and the ones about
  are deleted, except one in the player's hands (`CharObj2+0x94`, or Data1 mode 1): the game
  keeps writing through that pointer, so it is deleted only once it is let go of, and a carried
  one waits for the next garden that isn't shared. Nothing comes, and the area's name doesn't
  show, until a garden has been up and not shared for 2.5 s, as friends' states and the
  server's Chao arrive a little after the area loads.
- **Without the absorb hook** (§2: the bytes at 0x56708E aren't as expected) no visitor comes:
  a Chao absorbing one would take the game's own animal path into the Chao save.
