# Chao Garden: interaction effects and garden items (for network sync)

Steam `sonic2app.exe`, image base 0x400000. Everything below was read from the objdump listing
of `re/bin/sonic2app.dumped.exe` (2026-10-16). Ghidra has no functions in most of this range. No
community headers were used, so names are ours. Read `chao.md` and `chao-race-sync.md` first.
This file covers what interactions do to `ChaoData` and how garden items exist and are saved.
How a Chao is drawn, held and driven from the network is in `chao-garden-sync.md` (separate work).

Conventions used below:
- `obj` is an `ObjectMaster*`. `obj+0x10` is MainSub, `+0x18` DeleteSub, `+0x34` Data1 ("work"),
  `+0x38` Data2/motion, `+0x3C` the **ALW entry** (see §3.1).
- For a Chao, `Data1+0x5C` is its `ChaoData*` (set by CreateChao 0x5501D0, see `chao.md`).
- "usercall" means register arguments; the stack arguments are always caller-cleaned.

---

## 0. Corrections and new facts for `chao.md`

| Item | Finding | Confidence |
|---|---|---|
| Slot count | `SpawnAllChaoInGarden` 0x531B10 walks 0x19F6EC4 .. 0x1A02EC4 in 0x800 steps: **24 slots**. | high |
| Chao save region | `0x19F6460`, size **0xCA6C**. There are two copies; the game picks `0x19F6460 + (*0x173D06C == 1 ? 0xCA6C : 0)`. Helper **0x52E440** (no args) returns the base in EAX. In practice the copy index is 0. The mod's `0x19F6EC4` constant is the copy-0 slot array. | high (selector meaning: medium) |
| Save walks live objects? | **Only items** (§3.4). Chao (category 0) are never walked back; their `ChaoData` is edited in place through `Data1+0x5C`. A visitor whose data is in a mod buffer is never written to the save by that path. Exceptions are in §5. | high |
| `ChaoData+0x20` | `u8 StatProgress[8]`, 0..99 (100 = 1 level). Not in the old table. | high |
| `ChaoData+0x8A / +0x8C` | lifespan / max lifespan (u16, max 9999 = 0x1312C7E). | high |
| `ChaoData+0xA8 / +0xAC` | f32 PowerRun (+power, -run) / FlySwim (+fly, -swim), clamped to ±1. | high |
| `ChaoData+0xB0` | f32 alignment, clamped to ±1 (hero +). | high |
| `ChaoData+0xC0` | f32 evolution progress, clamped to [0, 1.2]. | high |
| `ChaoData+0xD5` | u8 hat worn (item kind; 0 = none). | high |
| `ChaoData+0x118` | u32 bitmask: bit n = animal n has been absorbed at least once. | high |
| `ChaoData+0x11C..0x123` | i8 animal part per body slot (animal id, -1 = none). | high |
| `ChaoData+0x16C + i*6` | character bond i (0 Sonic, 1 Shadow, 2 Tails, 3 Eggman, 4 Knuckles, 5 Rouge; any other character uses 0). `+0 i8 like` ±100, `+1 u8` 0..200 (familiarity), `+2 u16`, `+4 u16` interaction counter. | high (field meaning: medium) |
| `ChaoData+0x124..` emotion block | `+0x132` u8 (pet/hold only raise happiness while it is < 10), `+0x136` u16 (> 500 makes the Chao go to sleep behaviour 0x54FCF0), `+0x138` u16 hunger (-2400 per eat step), `+0x13A` u16 (heart fruit sets it +10000: mating), `+0x140` u16 (-2400 per eat step). All are clamped to 0..10000. | medium |

---

## 1. Interaction effects on the Chao

### 1.1 Core helpers (all take the Chao `ObjectMaster*`)
| Addr | Convention | Effect |
|---|---|---|
| **0x536320** `AddHappiness` | usercall **ESI=obj, EDI=int delta**, push f32 `align`; caller pops 4 | `Happiness(0x82) = clamp(h+delta, -100, 100)`. Then alignment by the **local player's character** `*(i8*)(*0x1DE9600+1)`: Sonic/Tails/Knuckles/(8) → `+align`, Shadow/Eggman/Rouge → `-align`, mech walkers (6,7) → none (0x536260). Then **0x53E120** (bond, below). Ends by showing the emotion ball (0x53D660, dl=4 when delta>0, otherwise 5, 0xB4 frames). |
| 0x53E120 | usercall EAX=delta, ECX=obj | `bond[c].like += delta` (clamped ±100). Only when delta > 0: `bond[c].fam += ~10*(300+p[0x155])/300 - ~10*(300+p[0x14C])/300` (personality bytes), clamped 0..200. Then 0x53E280: `bond[c]+4 ++`, `bond[c]+2 = bond[c]+2 * 2/3`. `c` = 0x53E0C0() = local character → bond index. |
| 0x53E1F0 | usercall EAX=obj, EBX=amount | Changes `bond[c].fam` by a personality-scaled amount. Used by hit/throw. |
| 0x536260 | EAX=obj, push f32 | alignment `+= f`, clamp ±1, sets `Data1+0x674 \|= 2` (appearance dirty) |
| 0x536180 / 0x5361F0 | EAX=obj, push f32 | PowerRun (0xA8) / FlySwim (0xAC) `+= f`, clamp ±1, dirty flag |
| 0x536110 | EAX=obj, push f32 | evolution progress (0xC0) `+= f`, clamp [0,1.2] |
| 0x5362D0 | EAX=obj, push int | max lifespan (0x8C) `+= n` (cap 9999), lifespan clamped to it |
| **0x535F00** `GrowStats` | usercall **EAX=obj, DX=item kind**, push u16 `cat` (2 = animal/drive, 3 = fruit), push int `lastBite`; caller pops 8 | For each of 8 stats: `progress[i] += table[kind][i]` (fruit: `value/4`, plus `value%4` when `lastBite`). Below 0 it becomes 0. When progress > 99 and level < 99: `progress -= 100`, `level++`, and **0x535E90** `points[i] += 3*grade + 11 + rand(0..4)`, capped at 4000. A level at 99 caps progress at 99. Calls 0x536040 first, which copies the old grade/level/points into Data1+0x60.. for the stat-up display. When 5 stats reach level 10 it unlocks a Steam achievement. |

`table` is **0x8A6240** for cat 2 (animals + drives, 25 rows) and **0x8A6448** for cat 3 (fruit,
24 rows). Rows are 20 bytes: `i16 ?, i16 ?, i16 stat[8]` (swim, fly, run, power, stamina, luck,
intelligence, unused). The two leading words aren't read by 0x535F00.

### 1.2 What each interaction does
| Interaction | Code | Happiness | Alignment (sign by local character) | Bond | Other | Conf. |
|---|---|---|---|---|---|---|
| **Pet** (B near Chao) | trigger 0x564C10 (Chao `Data1+4 & 1` and player check 0x54B1F0) → behaviour **0x5648A0**, effect at 0x564931 | **+1** (only if `ChaoData+0x132 < 10`) | ±0.05 (0x1312D40) | like +1, familiarity up | heart emotion ball | high |
| **Pick up / hold** | 0x566720: Chao `Data1+4 & 0x8000` → behaviour **0x566670**, effect at 0x5666F7 | **+1** (same 0x132 < 10 rule) | ±0.0333 (0x1312D38) | like +1 | reaction picked by `bond.like`: > 50 → 0x5660A0, < -50 → 0x566300, else 0x5662B0 | high |
| **Thrown** | release path 0x565EDE → behaviour **0x564320**, effect at 0x56451A | **-5** | ∓0.0667 (0x1312D44, negated) | like -5; 0x53E1F0(10) | a worn hat falls off (§5) | medium-high |
| **Hit by player attack** | 0x563D30: Chao `Data1+4 & 4` → behaviour **0x563EB0**, effect at 0x563F88 | **-30** | ∓0.1 (0x1312D3C, negated) | like -30; 0x53E1F0(50) | a worn hat falls off (§5) | high |
| Dislikes the fruit it is eating | eat behaviour 0x5607C0 (0x560946/993/9DD, 0x5611D6) | -1 | ∓0.1 | like -1 | depends on `ChaoData+0x157` and hero/dark checks 0x535360/0x535390 | medium |
| **Given a fruit from the hand** | fruit mode 3 (0x545AF0, at 0x545BDB) when a Chao takes a whole fruit (4 bites left) | **+1** (0x1312D08) | child ±0.025, adult ±0.01 | like +1 | then the fruit goes to mode 4 (eaten) | high |
| **Fruit bite** (every 180 frames while eating) | fruit mode 4 **0x545C20** → **0x545790**(ESI=chao; push kind, push lastBite) | - | - | - | evolution +0.005 child / +0.00125 adult (0x536110); lifespan +3 (0x1312D00, capped at max); max lifespan +5 (0x5362D0); `GrowStats(kind, cat 3)` with value/4 per bite | high |
| Fruit finished (4th bite) | 0x545D32.. | - | kind 5 **+0.15**, kind 6 **-0.15** (fixed sign, 0x536260) | - | kind 10: `ChaoData+0x13A += 10000` (heart fruit). The save record is freed (0x52F680) and the object deleted. | high |
| Eating, the Chao's own side | eat behaviour 0x5607C0 | - | - | - | `+0x138` and `+0x140` -= 2400 (0x1312DA6) per step | medium |
| **Animal or Chaos Drive absorbed** | behaviour **0x566DF0** (set at 0x566CC5; trigger 0x566BC0) | **+1** (0x1312D34) | child ±0.025, adult ±0.01 | like +1 | see 1.3 | high |

The "sign by local character" column matters for networking: the game uses **the local player's
current character** (`*0x1DE9600 + 1`) for both the alignment sign and the bond slot. The local
player is whoever's game runs the code.

### 1.3 Animals and drives in detail (0x566DF0, from 0x567011)
- Item id = `item Data1+3`. Animals are 0..0x14 (21). Drives are 0x15..0x18: yellow, green,
  red, purple.
- Animals only:
  - `ChaoData+0x118 |= 1<<id`.
  - For each of the 8 body slots, if the animal has that part (tables 0x12E59A0 child /
    0x12E5938 adult, slot map 0x12F59E8), with probability 0.5 (0x1312CB0) set the part through
    **0x5668B0**(EAX=obj, EDX=slot, ECX=id). Otherwise, with probability 0.2 (0x1312CB4), clear
    the part (-1).
  - Animals 0x0F/0x10/0x11 can set extra flags `0x118 |= 0x8000/0x10000/0x20000`, `+0xD3`,
    `+0xD6`. This is **random**, so the owner can't recompute it. Send the resulting bytes.
- Appearance (magnitude m = 0.1 child / 0.05 adult; table 0x567530):
  - animals 0-2: FlySwim -m (swim)
  - animals 3-5: PowerRun -m (run)
  - animals 6-8: PowerRun +m (power)
  - animals 9-11: FlySwim +m (fly)
  - animals 12-20: a random axis and sign
  - drives (m/2): yellow FlySwim -, green PowerRun -, red PowerRun +, purple FlySwim +
- Stats: `GrowStats(id, cat 2, 0)` with the full row from 0x8A6240.

### 1.4 Effect tables (stat progress units, 100 = one level)
Animals and drives (0x8A6240 + id*20 + 4), order swim/fly/run/power/stamina/luck/intel:
Every animal row also has stamina 0, luck +20, intel +20. Each drive has luck +40, intel +20.
```
id  swim fly run pow      id  swim fly run pow      id  swim fly run pow
 0   36  12   8 -24        7   -8 -16  20  36       14   20  16   4   8
 1   40 -16   0   8        8   -4  -8   4  40       15   32   0   8  24
 2   44  -4   8 -16        9   12  48 -16 -12       16    8   8  32  16
 3    0   8  40 -16       10    0  48 -20   4       17    8  40   8   8
 4   -8  -8  40   8       11   20  60 -24  16       18   20   4   8  32
 5   -4 -12  32  16       12    8  12  16  12       19   16  12  36   0
 6    8 -16   4  36       13    8   8  16  16       20   12  32   4  16
21 yellow drive: swim +24   22 green: run +24   23 red: power +24   24 purple: fly +24
```
Fruit (0x8A6448 + kind*20 + 4). Kind names are **tentative**. Kinds 0-2 come from the garden
trees and 13-19 from the GBA transfer, both read from code. Kinds 5/6/10 come from the end-of-fruit
code. The rest is guessed from the values.
| kind | stats (swim,fly,run,power,stamina,luck,intel) | notes |
|---|---|---|
| 0,1,2 | stamina +40 / +99 / +40 | garden-tree fruit for neutral/hero/dark gardens (tree type 5 at 0x5470E7) |
| 3,4 | stamina +50 / +30 | planted trees give kind `tree+0x30 + 2` (0x5471CA) |
| 5,6 | stamina +40 | hero / dark fruit (alignment ±0.15 when finished) |
| 7,8,9 | stamina +40 | round / triangle / square? |
| 10 | stamina +40 | heart fruit (mating flag) |
| 11 | all five +50 | Chao fruit? |
| 12 | intel +50 | smart fruit? |
| 13..19 | mixed (e.g. 13: +30,-20,-20,+30,+10) | GBA fruits (0x57DE50: kind = 13 + GBA id) |
| 20,21 | stamina+luck 50/50, 80/100 | mushrooms? |
| 22,23 | stamina+luck 40/40 | ? |

---

## 2. Applying a friend's interaction to YOUR real Chao (goal a)

Problem: the friend pets their **visitor copy** of your Chao. Their game changes the visitor
buffer (a mod pool buffer, `Data1+0x5C`), using **their** character for the alignment sign and
bond slot. Your save never sees the change.

**Recommended: hook, snapshot, diff, send.** On the friend's side, wrap these functions. Inside
each wrapper, if `obj` is one of our visitor objects (check `Data1+0x5C` against the visitor pool),
copy the 0x800 record before calling the original, call it, then diff a whitelist and send
`{owner, slot, field deltas}`.
| Hook | Covers |
|---|---|
| 0x536320 (ESI/EDI/stack) | pet, hold, throw, hit, given fruit, animal happiness, alignment, bonds |
| 0x545790 (ESI; 2 stack args) | fruit bite: lifespan, evolution, stats (it calls 0x535F00 itself) |
| 0x535F00 (EAX/DX; 2 stack args) | stats from animals and drives (and fruit, if 0x545790 isn't hooked) |
| 0x566DF0 (cdecl obj) | animal or drive absorb: parts, 0x118 mask, appearance, flags. The random results arrive as plain bytes |
| 0x545D32 path, or diff after 0x545C20 | heart/hero/dark fruit finish |

Whitelist (deltas for numbers, values for parts):
- 0x20..0x27 progress, 0x30..0x37 levels, 0x38..0x47 points
- 0x82 happiness, 0x8A/0x8C lifespan, 0xA8/0xAC/0xB0/0xC0 floats
- 0x118..0x123 animals, 0x16C..0x18F bonds
- optionally 0x124..0x16B emotions

The owner applies the deltas to `ChaoSlots[slot]` (clamping as the game does). If the Chao is
spawned right now, also set `Data1+0x674 |= 2` so the appearance refreshes. Apply on the game
thread.

Alternative (owner replays): send `{event, kind, friendCharacter}` and have the owner call 0x536320
or 0x535F00 on their live Chao object. This only works while the Chao is spawned. The
character-dependent parts would use the owner's character, not the friend's, and animal parts
are random. Only use this for stat growth from a known item kind.

Policy note: hit (-30) and throw (-5) also go through 0x536320. Decide whether friends may
harm your Chao, and filter by delta sign or by caller.

Confidence: hook points and field effects high. Using the diff approach for everything is a
design choice; it is untested.

---

## 3. Garden items

### 3.1 ALW object registry (every Chao-world object)
- **Counts**: `u32 0x1DC0F80[cat]`.
- **Entries**: `0x1DC0FC0 + cat*0x1200 + i*0x48`, 64 slots per category. Capacity per category at
  `0x8AB838[cat]`.
- Categories (with capacity):
  - 0 Chao (32)
  - 1 egg (16)
  - 2 animal + Chaos Drive (10)
  - 3 fruit (48)
  - 4 tree (10)
  - 5 growing tree (16)
  - 6 toys: TV, radio, ball, horse, box etc., fixed per garden, not saved (32)
  - 7 seed (16)
  - 8 sound (16)
  - 9 hat (32)

Entry layout (0x48):
| off | meaning |
|---|---|
| +0x00 u16 | category |
| +0x02 u16 | slot index |
| +0x04 u16 | kind passed at register time. Fruit: fruit kind. Animal: animal id. Drive: 1. Seed/hat: 0. Use Data1 for the real kind. |
| +0x06 u16 | flags. Bit 0x2 = **held by a Chao** (its position follows +0x10) |
| +0x08 ptr | **save record** (`ItemSave*`, §3.3), or `ChaoData*` for a Chao. NULL means not persisted. |
| +0x0C int | link id (0x1A5B5EC counter) |
| +0x10 vec3 | hold position written by the holding Chao (0x56CFF0) |
| +0x20 int | holder's Y angle |
| +0x28, +0x2C f32 | size / height offset (set by each create function) |
| +0x34 u16 | command mailbox. 0x530730 (EAX=self, DX=cmd) writes the partner's. Chao→fruit 2 = "eat a bite". Fruit→Chao 4 = finished, 6 = taken away |
| +0x3C ptr | the `ObjectMaster*` (`obj+0x3C` points back to the entry) |
| +0x40 ptr | partner entry. Mutual links: 0x530640 (EAX, ECX objs), cleared by 0x530690 (EAX=obj) |
| +0x44 ptr | target entry (0x5304F0) |

Helpers:
| Addr | Convention | Purpose |
|---|---|---|
| 0x530750 | usercall ECX=cat, EBX=obj; push save, push kind | register |
| 0x5307F0 | usercall ESI=obj | unregister, called by the delete subs |
| 0x530410 | usercall EAX=cat, EDI=n | n-th live **object** of the category |
| 0x530470 | usercall EDX=cat, EBX=n | n-th live **entry** |
| 0x530370 | | clears the whole registry (garden load) |

### 3.2 Item objects
All are cdecl, return `ObjectMaster*`, and **must run on the game thread**. `pos`/`vel` are
`NJS_VECTOR*`, `ang` is a BAMS Y angle, `save` is an `ItemSave*` or NULL.
| Item | Create | Args | Object | Data1 fields |
|---|---|---|---|---|
| Fruit | **0x546180** | `(int kind, pos, int ang, vel/NULL, save)` | list 4, main 0x545E40, display 0x545EE0, delete 0x57B9B0, cat 3 | `+8` kind, `+0xC` ang, `+0x10` **bites left** (4; from save+4), `+0x14` pos. Mode `Data1[0]`: 0 init/fall 0x545A40, 1 0x545860, 2 on ground 0x5458F0, 3 held by player 0x545AF0, 4 being eaten 0x545C20 |
| Animal | **0x548D30** | `(i8 id 0..20, pos, int ang, vel (NOT NULL), save)` | list 2, main 0x548C50, display 0x5489D0, delete 0x548C30, cat 2 | `+3` id |
| Chaos Drive | **0x545600** | `(u8 color 0..3, pos, vel (NOT NULL), save)` | list 2, main 0x5450C0, delete 0x5455B0, cat 2 | `+3` = color+0x15, `+0x30` color; the save record kind is color+0x15 |
| Seed | **0x549B00** | `(int kind, pos, vel/NULL, save)` | list 2, main 0x549860, delete 0x57B9B0, cat 7 | `+8` kind |
| Hat | **0x54A540** | `(int kind, pos, int ang, vel/NULL, save)` | list 4, main 0x54A050, delete 0x57B9B0, cat 9 | `+8` kind |

Each create function writes `save->kind = kind` (fruit also sets `save+4 = 4` when the record was
empty). That is how a fresh record from 0x52F9E0 gets claimed.

Delete an item: `DeleteObject` **0x46F720** (cdecl obj). It runs the delete sub, which
unregisters from ALW. The game itself usually does `obj->MainSub = 0x46F720` to delete on the
next tick. Deleting does **not** free the save record. Free it first with **0x52F680** (EAX =
record; sets kind/garden = -1, zeroes the rest), as the eaten-fruit path does at 0x545DD9.

Held by the player:
- `CharObj2(*0x1DE9600)+0x94` = held `ObjectMaster*`. `+0x98` = object about to be picked up.
- Player `Data1+4 & 0x800` = holding.
- The item's `Data1+4 & 0x8000` = held by a player.
- **0x46E5B0** (ECX = player index, push obj): make the player pick up `obj` (sets action 0x1A).
- **0x46E5E0** (EAX = player index, EDX = player Data1): drop the held object.

Held by a Chao: entry +0x06 bit 2, linked through +0x40. The Chao writes the hold position every
frame (0x56CFF0). The item copies it in 0x56D170 (EDX = item obj; returns 1 while a Chao holds
it). When the player lets go of a fruit that a Chao is linked to, the fruit's mode 3 code
(0x545B90) hands it to the Chao. If the fruit is whole: +1 happiness and mode 4.

Chao triggers come from collision ids, checked with 0x486760(obj, id):
- 0x83: sees fruit → 0x5606D0 approach, then 0x5607C0 eat
- 0x93: hat → 0x569240 put on. This sets `ChaoData+0xD5 = hat kind`, **frees the hat's save record**
  and deletes the hat.
- animals/drives: 0x566BC0 → 0x566DF0 absorb

### 3.3 Saved item lists (inside the Chao save region)
Base `B = 0x19F6460` (copy 0). Records are `ItemSave`, 0x14 bytes:
`i16 kind (-1 = empty), i16 garden (1..3), i16 param, i16 age, f32 pos[3]`.
| Range | Address (copy 0) | Count | Category | Spawned by |
|---|---|---|---|---|
| B+0x020 + g*0x38 | 0x19F6480 | 3 gardens × 7 × 8 bytes | trees | 0x548210 (positions table 0x1366B30) |
| **B+0x0C8** | 0x19F6528 | **40** | fruit (`param` = bites left; `age`) | 0x546180 |
| **B+0x3E8** | 0x19F6848 | **12** | seeds | 0x549B00 |
| **B+0x4D8** | 0x19F6938 | **24** | hats | 0x54A540 |
| **B+0x6B8** | 0x19F6B18 | **10** | animals (kind < 0x15) and drives (kind 0x15..0x18) | 0x548D30 / 0x545600 |
| B+0x780 | 0x19F6BE0 | 0x250 bytes | race progress, copied from 0x1A0F980 | |
| B+0x9F0 | 0x19F6E50 | | Black Market inventory: +0x24 count, +0x28 u16 list. Mirrors 0x1DBEDA0/0x1DBEDAC | |
| B+0xA64 | 0x19F6EC4 | 24 × 0x800 | ChaoData slots | |
| B+0xCA64 | | 8 bytes | checksum/random bytes (0x52EEE0 / 0x52F030) | |

Record helpers:
| Addr | Convention | Purpose |
|---|---|---|
| **0x52F9E0** | ECX = cat (2/3/7/9) | returns a free record (kind == -1) or NULL. It does not mark the record; the create call does. |
| 0x52F4F0 | ECX = cat | count of used records |
| 0x52F650 | EAX = cat | capacity (40/12/24) |
| 0x52F680 | EAX = record | free |

### 3.4 Load, save and carry
- **Load** (garden entry), 0x52EB70: trees, then 0x52E920(2) and 0x52E920(3), then seeds and
  hats inline. Each record with `garden == CurrentChaoArea` is spawned with `save = &record`. A
  fruit with `param == 0` is freed instead. Records with an invalid garden are cleared. The
  carried record (`*0x19F6454`) is skipped.
- **Write back**, **0x52F090**:
  - 0x52E6B0 (trees), then **0x52E710(cat)** for 2, 3, 7, 9, then 0x52EDE0 (inventory), then
    0x52EEE0 (checksum bytes).
  - 0x52E710 walks the ALW entries of the category. For every entry with a non-NULL `+0x08`
    record it writes `garden = current area` and `pos = Data1+0x14`. For fruit it also writes
    `param = Data1+0x10`, and increments `age` each time while the fruit lies on the ground. At
    age 9 the record is **freed** (the fruit rots).
  - An item held by the player (`0x8000`), below y = -100, or touching collision 0xCD is moved
    to one of 15 random spawn points per garden (0x1366260).
  - 0x52F090 runs from GoToChaoArea (0x52B6AB), from the Chao file save **0x52E630** (before
    0x532360 writes the file), and from the other menu and kindergarten exits.
- **Carry between areas**, GoToChaoArea **0x52B5B0** (cdecl area):
  - If the player holds an item, `*0x19F6450 = its category` and `*0x19F6454 = entry+0x08`
    (record).
  - If the player holds a Chao or egg, `*0x1A5CA5C = its ChaoData*` (0x531F40).
  - In the next garden, 0x52F2A0 re-creates the carried item from the record in front of the
    player and puts it in their hands (0x46E5B0). The carried Chao is spawned by 0x531E10 with
    `CreateChao(*0x1A5CA5C, ...)`.
  - A held item with a NULL record carries nothing; it is simply lost.
- New items that get records:
  - tree drops 0x5470D3 (record may be NULL when all 40 are used)
  - Black Market deliveries 0x548F40
  - stage animals released in the garden 0x5492B0 (cat 2)
  - hats a Chao drops (§5)
  - GBA fruit 0x57DE36

Confidence: layout, counts and helpers high. Fruit kind names and the copy-selector meaning
medium.

---

## 4. Minimal operations for sharing items (goal b)

| Op | How | Notes |
|---|---|---|
| Spawn item T at P | fruit `0x546180(kind,&P,ang,NULL,NULL)`; animal `0x548D30(id,&P,ang,&zeroVel,NULL)`; drive `0x545600(color,&P,&zeroVel,NULL)`; seed `0x549B00(kind,&P,NULL,NULL)`; hat `0x54A540(kind,&P,ang,NULL,NULL)` | Use `save = NULL` for **mirrored** items so they never reach the local save. Fruit bites left: write `Data1+0x10` after the call. Check that `*0x1DC0F80[cat] < 0x8AB838[cat]` first. |
| Spawn a real (persisted) item | `rec = 0x52F9E0(ECX=cat)`; if `rec` is not NULL, create with `save = rec` | Only for items that should **move into** this player's save (ownership transfer). |
| Remove item | if `entry+8`: `0x52F680(EAX=entry+8)`, `entry+8 = 0`; then `0x46F720(obj)` (or `obj+0x10 = 0x46F720`) | Free the record only when the item really leaves this save. |
| Enumerate items | for cat in {2,3,7,9}: walk the 64 entries at `0x1DC0FC0+cat*0x1200`, keeping those with `+0x3C != 0` | Kind: `Data1+8` (fruit/seed/hat) or `Data1+3` (animal/drive). Pos: `Data1+0x14`. Persisted: `entry+8 != 0`. |
| Local player picks up / drops | poll `*(obj*)(*0x1DE9600 + 0x94)` each frame | null→obj = pickup, obj→null = drop, throw or give. On drop, `entry(+0x40)` → a category-0 entry means it was given to that Chao. |
| Feeding detected | fruit `Data1[0] == 4` and partner is a Chao; `Data1+0x10` decrements per bite; the object disappears at 0 | Or hook 0x545790 (the bite) / 0x545C20. |
| Item removed by the game | its ALW slot `+0x3C` changes or becomes 0 | eaten, absorbed, worn as a hat, or fruit rotted on save |
| Make the local player hold an item | 0x46E5B0(ECX=0, push obj) | only when `CharObj2+0x98 == 0` |

Suggested model: the **garden owner's** items are real (with records) in the owner's game and
mirrored with `save = NULL` everywhere else. Each item gets a network id mapped to the local
`ObjectMaster*` on each side. Each game runs its own physics; send position on drop and at rest
rather than every frame. Hide a mirrored item while a remote player holds it (delete it) and
re-create it on drop. When a mirror is eaten, absorbed, worn or picked up for good, tell the
owner, who removes the real item and frees its record. Moving an item into the other player's
save means: the giver frees and deletes, the receiver allocates with 0x52F9E0 and creates.

Feasible:
- spawn, delete and enumerate (all plain cdecl)
- pickup/drop detection by polling
- forwarding effects with the hook-and-diff approach (§2)

Hard:
- keeping item physics in step; held and thrown items look different on each side
- exact replay of random animal effects (send the results instead)
- the Chao-held link: entry `+0x40` and flag 2 point at local objects only, so a remote Chao
  can't "hold" a local item without driving its entry ourselves
- concurrent pickups need an owner arbitration step

---

## 5. Save-file risks

1. **Visitor Chao drops its hat into YOUR save.** When a hatted Chao is hit (0x563D30 → 0x563E84)
   or thrown (0x564320 → 0x56440A), the game allocates a hat record in the **local** list at
   B+0x4D8 and spawns the hat. It then clears `+0xD5` only in that Chao's data. For a visitor,
   that duplicates the friend's hat into your save, while their real Chao keeps it. Fix: hook
   these paths, or clear `+0xD5` in visitor buffers and draw the hat some other way, or create
   the hat with `save = NULL`. **Done 2026-10-18** (garden.rs `drop_hat`): a visitor keeps its hat.
2. **Visitor Chao puts on YOUR hat.** (**Done 2026-10-18**: visitors skip the hat check
   0x5691B0, garden.rs `may_wear_hat`.) 0x569240 frees the local hat record and deletes the hat.
   The hat ends up only in the visitor buffer, so it is lost from your save. Your own Chao
   wearing a *mirrored* hat duplicates it unless the owner is told to remove theirs.
3. **Visitor Chao eats or absorbs your items.** The local record is freed (fruit 0x545DD9;
   animals are deleted). This may be intended. Note it.
4. **Carrying a visitor Chao out of the garden.**
   - GoToChaoArea stores its `ChaoData*` (the mod's pool buffer) in **0x1A5CA5C**.
   - The next area re-creates it with `CreateChao(ptr)` (0x531E10).
   - `visitors::on_frame` releases the pool on leaving the garden, so the pointer dangles.
   - It also means a visitor walking into the kindergarten machines (name, doctor, etc.) with
     their data.
   - Fix: before area changes, if the held object is a visitor, drop it first (0x46E5E0) or clear
     `*0x1A5CA5C`. Hook 0x52B5B0, or check `CharObj2+0x94` each frame against the visitor
     objects.
5. **Mirrored items must have `save = NULL`.** Otherwise 0x52F090 writes them into this player's
   save on the next area change or Chao save. A mirrored item the player carries out is lost,
   which is safe.
6. **Copy selector.** Item lists and ChaoSlots live in the copy chosen by `*0x173D06C`. Use
   0x52E440 for the base instead of a constant if this ever matters.
7. **Character-dependent effects.** On the friend's side, alignment and bond changes use the
   friend's character. That is correct for "friend X petted my Chao", but apply deltas, not
   absolute values, or the owner's own changes get overwritten.
8. **Breeding.** Not checked: a visitor mating with a local Chao probably creates an egg (cat 1)
   in the local garden, and possibly a local record. Check before enabling visitor AI near
   local Chao.

---

## 6. Address summary
| Name | Addr |
|---|---|
| AddHappiness (+alignment, +bond) | 0x536320 |
| Bond like/familiarity | 0x53E120, 0x53E1F0, 0x53E280; char→bond 0x53E0C0 |
| GrowStats / level-up points | 0x535F00 / 0x535E90 |
| Stat tables (animal+drive / fruit) | 0x8A6240 / 0x8A6448 |
| Chao parameter constants | 0x1312C60..0x1312E00 |
| Fruit bite effect / fruit eaten mode | 0x545790 / 0x545C20 |
| Animal/drive absorb behaviour | 0x566DF0 (part setter 0x5668B0) |
| Pet behaviour / trigger | 0x5648A0 / 0x564C10 |
| Held behaviour / trigger | 0x566670 / 0x566720 |
| Thrown / hit behaviours | 0x564320 / 0x563EB0 (trigger 0x563D30) |
| Chao interaction dispatcher | ~0x53DB80 (calls 0x5691B0, 0x567560, 0x560640, 0x566BC0, 0x564760, 0x566720, 0x564C10, 0x563D30) |
| CreateFruit / Animal / Drive / Seed / Hat | 0x546180 / 0x548D30 / 0x545600 / 0x549B00 / 0x54A540 |
| DeleteObject | 0x46F720 |
| ALW counts / entries / caps | 0x1DC0F80 / 0x1DC0FC0 / 0x8AB838 |
| ALW register / unregister / nth obj / nth entry | 0x530750 / 0x5307F0 / 0x530410 / 0x530470 |
| Save base getter / record alloc / free | 0x52E440 / 0x52F9E0 / 0x52F680 |
| Item load / save-back | 0x52EB70 / 0x52F090 (per-cat 0x52E710) |
| Carried item cat / record / Chao | 0x19F6450 / 0x19F6454 / 0x1A5CA5C |
| Player held object | `*0x1DE9600 + 0x94` (`+0x98` pending) |
| Player grab / drop | 0x46E5B0 / 0x46E5E0 |

## 7. The server's share of a garden (design, 2026-10-18)

Goal: nothing in a shared garden belongs to a player who can leave, unless it really is in
their save. The host's referee (`net/referee.rs`; the dedicated server and a hosting player run
the same one) keeps it; the dedicated server also keeps it in a file across restarts.

**Trees.** The referee keeps 7 records (`TREE_RECORD` bytes each) per garden.
- Seeded by the first `GardenTrees` it hears for a garden (the first keeper's own trees, as
  today), then updated by every later one.
- It sends its set (`ServerGarden`, every 2 s and to newcomers' first tick) to everyone.
- The keeper's game, once it has the server's set, runs trees made from it (records the mod
  owns, like the copies' records) with the tree's own MainSub: they grow and ripen as usual. Its
  wrapper keeps the game from dropping fruit itself (that would make a save record), and drops
  ripe fruit (by itself at 1.9, or shaken) the mod's way, *without* a save record. It sends
  those records as its `GardenTrees`.
- Everyone else shows copies of the keeper's (or, before the keeper has sent any, the
  server's). A new keeper starts from the server's set, so a keeper change no longer swaps the
  trees.
- Players' own trees stay untouched in their saves; the real ones come back when nobody else is
  in the garden.

**Toys.** The referee keeps each garden's last toy jump (`ToyMode`) and when. `ServerGarden`
carries them with their age; a player entering a garden makes its toys jump to modes younger
than a minute (a TV programme runs 3600 frames), so the newcomer sees what everyone sees.

**Items.** Ownership stays as it is (the owner's game has the real object and reports it; the
others show copies and claim what they use). New: an item can be *unsaved*, in no one's save:
fruit from the server's trees, or a copy we hold for the server. Its owner reports it with
`place | 0x80`.
- When its owner leaves the garden (their `PlayerState` shows another area) or the lobby, the
  referee keeps their unsaved items itself: from then on *it* reports them (`ServerItems`, every
  second, with a fresh id from its own counter so they can't clash with a player's) and they
  stay where they were.
- Every game shows a server item as a copy. A claim (picked up, or a Chao took it) is answered as
  now: the first claimer gets it (`ItemOwner`), adopts it into their save (it's saved now), and
  reports it; the referee forgets it.
- Items in a leaver's save (what they brought or picked up and kept) still leave with them. If
  the server kept them, they'd be in their save *and* in the garden: one item twice. That's why
  only unsaved items stay.
- The dedicated server keeps its items in its file too; a player-hosted session keeps them while
  it runs.

What the server keeps is plain data (records, modes, item kinds and spots), no game code: the
server needs no game files.

**Fix after the first live test (2026-10-18).** The host saw extra mangoes and egg shells kept by
the server. Two causes:
- The item registry isn't packed. Removing an item (0x5307F0) lowers the category count and
  leaves a hole; registering one (0x530750) takes the first hole. So the first `count` slots
  (what the game's own walk 0x530470 looks at, and what the server trees' fruit check looked at)
  can miss the last item. When a tree drop filled a hole, the count went up and a saved fruit
  further on came into view, looked new, and lost its save record. Those were the host's saved
  mangoes; their spots are the game's fixed item spots (0x1366260, where the pre-save pass
  0x52E710 puts items), hence several at one spot. Fix: all 64 slots are compared, by (entry,
  object), and only a fruit within reach of the tree that dropped it counts (test
  `only_fruit_made_now_is_new`).
- An egg's shell is a hat made by the hatch (0x57AFA2, 0x57B1A2, 0x57B416) with a record from the
  hat list, or none when the list is full (24): the game itself drops those on reload. "No record"
  isn't "the server's", so only items the mod made unsaved (`items::mark_unsaved`, the server
  trees' fruit) are reported unsaved now.

## 8. Where a held fruit goes (2026-10-19, static)

- Hands: `Data1+0x5D8` (node 4) and `+0x5E4` (node 11), world positions. Written every frame by
  the MainSub's model pass `0x56CCF0` → `0x56FC20` (identity, translate `D+0x14`, rotate `D+0xC`)
  → node walk `0x56F5C0` → `0x56F180` (switch on node counter `*0x1AED2D4`; also `+0x5C0` node 1,
  `+0x5F0` 6, `+0x5FC` 13, `+0x5CC` 15, `+0x620` 17, `+0x638` 20, `+0x608` 28, `+0x718` 33).
  Not behind the AI flag (`0x53DC40`) nor the on-screen flag `0x1000`; needs only the MainSub
  `0x54FE20` and `*0x1DBE634`. So a copy with its AI off still has hands that follow its motion.
- Link `0x56CFB0` (ESI = Chao): partner entry `+6 |= 2`, calls `0x56CFF0`, `D+0xA0 |= 0xC00`.
- Each frame `0x56CFF0` (ECX = Chao; eat tail `0x561197` etc.) writes the **item's ALW entry**:
  `half = 0.5 * |hand4 − hand11|` (`0x77FBD0`: 0 when dist² < 0.025), `r = sqrt(half² + E+0x28²)`;
  `0xC00` (both hands): `E+0x10 = mid.x + r·sin(D+0xC)`, `E+0x14 = mid.y`, `E+0x18 = mid.z +
  r·cos(D+0xC)` (`0x42AAB0` sin, `0x42AC30` cos, Ninja angles). Only `0x400`: hand 4; only
  `0x800`: hand 11. Then `E+0x14 −= E+0x2C`, `E+0x20 = D+0xC`. Returns 0 when not mutually
  linked (`E+0x40`).
- Fruit side: being-eaten mode `0x545C20` (and others) call `0x56D170` (EDX = fruit): while
  `E+6 & 2` and linked, fruit pos `Data1+0x14 += (E+0x10 − pos) * 0.3` per frame, then clears
  `E+6 & 2`. Return 0 → mode 2 (ground).
- Fruit entry sizes: `E+0x2C = 1.0`, `E+0x28 = 2.0` (`*0x12848D4`) at create `0x546180`; after each
  bite `0x545C20` sets `E+0x28 = bites * 0.5`.
- Mod: `garden::hold_point` (both hands, as `0x56CFB0` sets) for a fruit in a Chao copy's hands
  (`ItemPlace::Chao`, `items::place`); falls back to the fixed offset. No 0.3 easing: the copy is
  put on the target each frame.
