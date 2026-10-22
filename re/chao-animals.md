# Garden animals and absorbing them

Addresses are in the Steam `sonic2app.exe` (image base 0x400000). Item registry ("ALW") and
the stat helpers are in chao-garden-items.md; the behaviour machinery in chao-ai.md.

## The animal object

- **Create 0x548D30**, cdecl `(i8 id 0..20, NJS_VECTOR* pos, int angY, NJS_VECTOR* vel (not
  NULL), ItemSave* save)` → ObjectMaster. List 2, name "AL_MinimalExecutor", main (+0x10)
  0x548C50, display (+0x14) 0x5489D0, delete (+0x18) 0x548C30. The other display subs (+0x1C,
  +0x20, +0x24) and the shadow-map sub (+0x2C) stay NULL; the display loop 0x470010 calls +0x14,
  +0x20, +0x1C, +0x24.
- Data1 (0xF8 bytes, zeroed): +0 mode, +1 state, +2 = 10, +3 id, +4 u16 status (0x240 at
  create, 0x8000 = held by the player), +6 u16 timer, +0xC angle Y, +0x10 frame counter (cries),
  +0x14 position, +0x2C collision, +0x30 f32 speed, +0x34 off-screen countdown (200), +0x38
  motion controller, +0x84 motion table (0x70 bytes from 0x1316B60, patched per id from
  0x1296870 / 0x12967F0 / 0x1296770 / 0x12968D8; id 0x11 copies entry 1 over entry 0), +0xF4
  set once a Chao has let go of it.
- Data2 (obj+0x38, 0x26C bytes, 0x7966D0): +0 velocity (from `vel`), +0x1C angle while held,
  +0x60 position copy.
- Per id: model 0x12966F0[id], roam routine 0x1316B00[id] (0x57BD40 for all 21), cry case
  table 0x57C784 (ids 3, 12, 14, 16 and 19 are silent; the others play 0x102C..0x1031 every 120
  frames).
- Collision 0x8A7488, 3 entries of 0x30: 0 the player's pickup, 1 (flags 6), 2 id **0x87**, the
  touch Chao react to (radius 4.2, height 8). Flag 0x10 at `(*(Data1+0x2C)+0xC)[i*0x30+4]`
  disables entry i; 0x486CD0 / 0x486D00 (EAX = object, ECX = i) enable / disable it.
- Registry: category 2, kind = id, `save` as given (NULL: the animal is never saved and has no
  record to free). Entry +0x28 = 1.3 and +0x2C = −0.5 (how far out and how low a Chao holds
  it). The category holds 10 (`0x8AB838[2]`; count at 0x1DC0F88) for animals and Chaos Drives
  together; when full, 0x530750 returns 0 and `obj+0x3C` stays NULL, which the display and
  0x56CFB0 do not expect.

### Main 0x548C50
- Mode `Data1[0]`: 0 roam 0x57BD40, 1 held by the player 0x548730, 2 held by a Chao 0x548870
  (EAX = object), 3 deleted (`MainSub = 0x46F720`).
- Every frame: position copied to Data2+0x60, motion 0x7938D0 (ESI = Data1+0x38), collision
  (0x47E750 in modes 0/1, 0x47E6C0 in mode 2), and the blob shadow 0x540FD0 (EAX = &pos, push
  0.24f) into the garden shadow list `*0x1A5D144` (64 at most). The shadow comes from the main,
  not the display.
- Roam, state 0: enables entries 0 (only while +0xF4 is 0) and 1, disables entry 2. Status
  0x8000 switches to mode 1.
- Mode 1 (the player carries it): entries 0 and 1 off, entry 2 on, motion 2, the angle follows
  the player; the position is the player's carry code's. On release: a Chao has it (entry +6 &
  2) → mode 2, otherwise mode 0.
- Mode 2 (a Chao holds it): all entries off, motion 2, entry +0x44 = the Chao's entry. Each
  frame 0x56D170 (EDX = object) moves the position 0.3 of the way to entry +0x10 (the Chao
  writes its hands there, 0x56CFF0). On release: +0xF4 = 1, the save record (if any) is freed,
  status &= ~0x240, back to mode 0.

So in the game a Chao only notices an animal while the player carries it: entry 2 is on in
mode 1 only.

### Display 0x5489D0
- Visibility 0x54AC70 (EAX = object; 3.0, 3.0, 2.0). Off screen with +0xF4 set it counts +0x34
  down and sets mode 3 at 0: the only way an animal goes away by itself.
- On screen: +0x34 = 200, texlist 0x1366AEC, translate to the position and y −1.0 (−1.2 while
  carried), rotate Y by +0xC, 0x793F90 (EAX = Data1+0x38, ECX = model).
- It reads registry entry +0x30 with no NULL check (0x548BAF).

## The Chao's side

- **0x566BC0** (ESI = Chao): not while the behaviour is 0x566DF0 or 0x566C40. Touch 0x87
  (0x486760) with an object not held by a Chao (entry +6 & 2) → W+0x54 = its position,
  `AL_SetBehavior(0x566C40, -1)`.
- **0x566C40**: motion 0xCA, turns toward it (0x796910, 0x600 a frame) until within 0x71C,
  touch 0x87 again, then 0x46E5E0 (EAX = 0, EDX = `*0x1DEA6C0`: player 1 drops what it holds),
  0x56CFB0 (ESI = Chao, ECX = item: links both registry entries through 0x530640, item entry
  +6 |= 2, Chao `D+0xA0 |= 0xC00`), `AL_SetBehavior(0x566DF0)`.
- **0x566DF0** (mode at D+0x1B6):
  - Mode 0: motion 0xCB, a count of 90..290 at D+0x1C4, sound 0x1004, sparkle task
    0x470C00(2, 0x566CF0, Chao), status balloon 0x5659E0 (EAX = Chao).
  - Mode 1, once the motion flags bit 1: the animal is the partner of the Chao's registry entry
    (+0x40, both ways). Motion 0xCC, mode 2, sound 0x1005, particle ring 0x6EFF10 (×40). With
    no partner it goes to 0x5674CE. Otherwise (0x567011) EDI = animal, EBX = id, animal
    Data1+0x10 = 1 and its velocity flings it away (XZ away from the Chao × 0.4, y 0.85).
  - Id ≥ 0x15 (Chaos Drives) jumps over the parts (`0x56708E: jge 0x567241`). Animals:
    `rec+0x118 |= 1 << id`, the 8 body parts (0x5668B0), the slot fix-ups at
    0x5671FD..0x56723C.
  - 0x567241, both: the 0xD3/0xD6/0x8000/0x10000/0x20000 flags, AddHappiness 0x536320 (ESI =
    Chao, EDI = 1, push alignment), PowerRun/FlySwim drift (0x5361F0 / 0x536180), GrowStats
    0x535F00 (EAX = Chao, DX = id, push 2, push 0).
  - 0x5674CC `xor ebx, ebx`; 0x5674CE: the status balloon `*0x1AED28C` showing this Chao goes
    to state 6 (counts its shown stats up to the record's, closes after 180 frames). Both
    registry entries are unlinked (+6 = 0, +0x40 = 0, +0xC = −1), and it returns.
  - Mode 2: ends when the motion flags bit 8.
  - The absorbed animal is not deleted: it is let go (mode 2 release) and despawns off screen.
- At 0x56708E (`0F 8D AD 01 00 00`): EBP = Chao, EDI = animal, EBX = id, EAX = animal Data2;
  the FPU stack is empty and the flags are from `cmp ebx, 0x15`.
- What the animal effects change: `rec+0x118` (Chaos Chao evolution reads bits 0..20 at
  0x5686B9; animal play 0x5A2F10 and tricks read it too), parts `rec+0x11C..0x123`, the flags
  above, happiness and bonds, the PowerRun/FlySwim drift, and stats (GrowStats is also where
  the "5 stats at level 10" achievement is unlocked, 0x536000..0x536026).

## Stats

- Record (Chao Data1+0x5C): progress u8[8] at +0x20 (0..99), grade u8[8] at +0x28, level u8[8]
  at +0x30 (99 at most), points u16[8] at +0x38 (4000 at most from growing). Order: swim, fly,
  run, power, stamina, luck, intelligence, unused.
- GrowStats reads `0x8A6240 + kind*20` (category 2) or `0x8A6448 + kind*20` (fruit), both in
  read-only data. A level up adds `3*grade + 11 + rand(0..4)` points (0x535E90, EDI = record,
  ESI = stat).

## GBA link partners (`src/gba_link/`)

- A partner is an animal of id 3 (silent) created without a save record, after checking the
  category-2 count. Its display is replaced by one drawing the partner's picture, and its main
  is wrapped: after the animal's own main, while it roams and the player holds nothing, collision
  entry 2 is enabled (0x486CD0) so Chao notice it in the open.
- `0x56708E` jumps to the mod: for a partner it applies the partner's effect, has the animal's
  own main delete it (mode 3) and goes on at 0x5674CC, so the motions, sparkles, sounds and
  balloon stay and the animal effects are skipped; any other animal or drive goes on at 0x567094
  / 0x567241 as before.
- Partners are left out of shared garden items (items.rs) and are deleted when the player
  leaves the garden.
