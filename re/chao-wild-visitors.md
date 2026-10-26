# Wild visitors: garden areas, ponds, carrying, and the catalog's habitats

Steam `sonic2app.exe`, image base 0x400000 (objdump of the dumped exe). The creatures come from
the player's local catalog (`src/creature/`). How partners live in a garden as animal objects is
in `chao-animals.md`; items, the player's hands and the item records in `chao-garden-items.md`;
the garden ground and grid in `garden-collision.md`.

---

## 1. Animal objects in a garden

- Create 0x548D30 `(i8 id, pos*, angY, vel*, save*)`; with `save = NULL` the animal has no
  record: it is never written to the Chao save and is lost when carried out (§3).
- Registry category 2 holds animals and Chaos Drives together. The add 0x530750 (CX = category)
  compares against `0x8AB838[category]` (u32 array): category 2 is `0x8AB840` = **10**
  (the mod raises it to 32 at start, `registry::raise_animal_limit`; each category has room for
  64 entries, 0x1200 bytes of 0x48, and only the registry functions 0x530400..0x5307F0 read the
  limits). The
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
`creature_link/scene.rs`, which asks whether the animal is one of the mod's objects and, if so,
applies its effect and goes on at 0x5674CC (motions, sparkles, sounds and the status balloon
stay; the animal effects are skipped). Any other animal object the mod makes goes through the
same site. A Chao only notices an animal on its own while collision entry 2 is enabled
(0x486CD0, EAX = object, ECX = 2); the game enables it only while the player carries it
(mode 1), and the mod leaves that as it is for partners and visitors, so a Chao only takes one
it is handed.

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

## 5. The catalog's habitats and garden themes

### Habitats (`Rom::wild_maps`)
- There is one habitat for each of the 18 type ids: every ordinary form whose first type is
  that id. Its section id is the type id, and its name is the type's name with "habitat".
- A habitat's creatures are split into tables of at most 10 species, each species an equal
  share of 100 (the remainder to the first ones), levels 5..=30. Type id 10's tables are water
  tables (the pond); every other habitat's are land tables.
- For the theme rules below, the habitats of type ids 2, 10, 14 and 17 are sky and sea ones,
  those of 4, 5, 7, 8, 9, 15 and 16 caves, and the rest grassland.

### Garden themes (`rotation::theme_of`, `also_of`)
Each area goes to a garden by the first rule that matches:

1. No land tables, or a sky and sea habitat → **Hero**.
2. A cave habitat, or type ids 4, 5 and 9 making up at least half of its land slots by chance
   → **Dark**.
3. Type ids 2 and 10 making up at least half of its land slots by chance → **Hero**.
4. Otherwise → **Neutral**.

A slot's chance counts half for each type of a two-type species. An area also fits another
garden:

- **Neutral**: a grassland habitat with land tables.
- **Hero**: land tables, with type ids 2 and 10 making up at least a third of its land slots.
- **Dark**: a cave habitat with no land tables.

---

## 6. The mod's wild visitors (`src/creature_link/rotation.rs`, `wild.rs`)

- **Areas** (`rotation::areas`): the habitats above, in the catalog's order, each with its
  land and water tables, its theme by the rules of §5 and the other gardens it fits (`also`).
- **The garden's area** (`rotation::pick`, `area_at`): the clock picks it, in 10-minute windows:
  window `v` = Unix seconds (UTC) / 600 (`rotation::slot_of`). A garden's areas are the ones of
  its theme and the ones that fit it too (`Area::fits`), in the catalog's order. With `n` areas
  for the garden,
  window `v` is position `v mod n` of a Fisher-Yates shuffle of them seeded by splitmix64 of
  `(v / n) << 8 | garden`, so each area comes once per round of `n` windows. When a round's
  first area is the one the round before ended on, its first two swap, so no area comes two
  windows running; with 2 areas they alternate, `(v + garden) mod 2`. Every game with the same
  catalog has the same area for a garden in the same window, alone or shared (the server's side is
  in `shared-visitors.md`, "The area and the clock").
- **A new window** (`wild::next_area`): every frame the garden is the player's alone, the
  window is worked out again; when it isn't the one the visit's area is for, the area becomes
  the new window's. New visitors then come from it; the ones about stay until they go as usual.
  The area's name (its habitat name) shows at the top for 4 s with the minutes its window
  has left (`rotation::until_next`) on entering the garden and whenever the area changes
  (`hud::top_title`).
- **Spots** (`wild::Spots`): land cells whose middle's highest probe hit is solid, and water
  cells whose highest hit is water with solid floor at least 3 below it (the surface height is
  kept per cell). Visitors come into the pond at water cells with at least three water
  neighbours.
- **Odds** (`wild::ways`): every slot a visitor to the grass, or to the pond, can come from,
  with its odds (a place's add up to 1):
  - Grass: the land tables. Pond: the water tables. In an area without land tables, type id 2
    creatures of the water tables go to the grass and the others to the pond.
  - Among a kind's tables: each table with a slot for the place counts the same.
  - In a table, species `s` (chance `c_s`, the sum of its slots', of the table's total `C`, among
    `k` species) has `0.6 × c_s / C + 0.4 / k`, shared among its slots by their chances (for its
    levels). On a table of 12 slots with chances 25, 15, 15, 10, 10, 8, 5, 4, 3, 2, 2, 1 and a
    species per slot, the 1% slot becomes 3.9% and the 25% one 18.3%.
- **Picks** (`wild::pick`): a slot by its odds times the garden's recent factor, level uniform
  in the slot's range, ability slot 0 or 1 at random, shiny 1 in 1024.
- **Recent species** (`wild::Recent`): each garden keeps the species of its last 8 visitors
  (`wild::with_recent`, the same memory for the player's own visitors and for the ones they pick
  as a shared garden's picker; not saved). For each time a species is among them its odds are
  multiplied by `0.1 + 0.9 × age / 8` (age 0 the newest). With four species of the same odds,
  back-to-back repeats go from 1 in 4 to about 1 in 17.
- **Who can come** (`wild::who_comes`, the link machine's list, `Page::who_comes`): the odds of
  `ways` added up by species and where from (grass or pond) with the level range of its slots,
  grass first, the likeliest first; recent species aren't counted there. A percentage is of the
  visitors to the grass or of those to the pond.
- **Objects**: each visitor is an animal of id 3 with no record (as partners), made only while
  the category-2 count plus 2 plus the garden's waiting partners that have no object yet is
  below the limit at 0x8AB840 (2 entries stay free for the game's own animals and drives, and
  the partners have theirs). A partner picked at the link machine when that leaves no room has
  the roaming visitor farthest from the player go first (it is deleted); only with no visitor to
  go does it take one of the 2 entries. A carried visitor handed over in the next garden does
  the same. The main wrapper runs the animal's own main, silenced: the animal code's five calls
  to the positional sound call 0x4372E0 (0x57B43D, 0x57BF3B, 0x57C4B3, 0x57C5FB, 0x57C6E9:
  ids 0x1011, 0x1010 twice, 0x1031, 0x1020) and its held sound call 0x437590 at 0x57C2F8 (id
  0x101F, the swimming sound, 0x5A frames; its result unused) are among the Chao code's sound
  calls garden.rs already redirects, and both hooks play nothing while one of our creatures'
  mains runs (`garden::SOUNDS_QUIET`, partners too). While player 1's action is 42 (whistling;
  `*0x1DEA6C0` +0), visitors within 100 units are called for 6 s: on land they walk to within 4
  units of the player (0.3 a frame, position and facing written after the animal's main, Data2
  velocity zeroed); in the pond they swim cell by cell to the pond cell nearest the player; in mode 0 a visitor in a
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
  server's Chao arrive a little after the area loads (`alone_wait`). A garden that was shared
  earlier in the same visit (the server handed it back, or the friends left) is known to be the
  player's, so its visitors come at once.
- **Without the absorb hook** (§2: the bytes at 0x56708E aren't as expected) no visitor comes:
  a Chao absorbing one would take the game's own animal path into the Chao save.
