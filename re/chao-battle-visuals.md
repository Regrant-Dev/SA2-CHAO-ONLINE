# Battle visuals: Chao motions, emotion balls, effects, sounds and the GBA ROM's battle graphics

Addresses are in the Steam `sonic2app.exe` (image base 0x400000). The arena, its Chao and the
camera are in `chao-battle-arena.md`; behaviours and their motions in `chao-ai.md`. GBA ROM
offsets are file offsets into the US ROM (game code BPEE, version 0), as in `src/gba/rom.rs`.

---

## 1. Motions on the arena Chao

- `arena::motion(object, id, blend)` calls `0x793D30` (EAX = `D+0xAC`, EDX = id, push blend)
  after checking the id against the motion table (`motion_ok`).
- The arena Chao have no AI (`D+0xA0 & 0x10` clear), so nothing but the mod changes their
  motion. The MainSub still advances it (`D+0xA0 & 0x4`), plays the emotion ball and the face
  timers (`chao-garden-sync.md` §1).
- What a motion does at its end is its `al_motion_table` entry's play mode (+4): 0, 1, 2 loop;
  3 holds the last frame; 4 holds, then blends into `next` (+8); 5 and 6 switch to `next`;
  7 ping-pongs. Its length in frames is `|end − start| / step` (+0x10, +0x14, +0x18).
- Face: eyes `0x53A4B0` (ECX = Chao, EDX = eye id, EAX = frames, −1 until changed), mouth
  `0x53A5A0` (ECX = Chao, EDX = frames, push mouth id, caller pops 4).

### Motions by battle use

Ids are global `al_motion_table` ids. "Played by" is the game code that sets the motion.
Lengths are one pass (one cycle for loops).

| use | id | end | frames | played by |
|---|---|---|---|---|
| contact hit | 0x243, 0x244, 0x246, 0x247 | hold | 50, 86, 53, 84 | karate attacks (karate ids 4, 5, 7, 8) |
| contact hit | 0x245 | loop | 60 | karate attack (karate id 6, voice 0x606F) |
| wind-up | 0x242 | loop | 29 | karate wind-up (karate id 3) |
| hop | 0x8A → 0x8B | next, hold | 27 + 33 | hop `0x5A0F40` (with vy 0.35) |
| throw | 0xBA | hold | 60 | eat `0x5607C0` modes 9/10: throws a disliked fruit after frame 4 |
| teasing | 0x1B9, 0x1BB, 0x1BD, 0x1C3 | hold | 67, 67, 39, 72 | dark-adult pestering `0x5A1600`/`0x5A1700`/`0x5A1780`/`0x5A1680` |
| sulk | 0x1BF | hold | 71 | dislike `0x5A1800` (voice 0x6069) |
| swim stroke | 0x91 | loop | 54 | swim `0x562040` (swim ≥ 2000) |
| swim stroke | 0x94 | loop | 30 | swim `0x561FB0` (swim 300..1999) |
| swim trick | 0x92 → 0x93 | next, hold | 47 + 40 | swim trick `0x5620D0` |
| flap / glide | 0x82 | loop | 65 | flutter `0x5630C0`, race (`0x5389F2`) |
| fighting stance | 0x255, 0x241, 0x240 | loop | 123, 33, 27 | karate stances (karate ids 0, 1, 2) |
| guard | 0x241 | loop | 33 | karate guard (karate id 0x14) |
| focus / taunt | 0x253, 0x254, 0x26D | loop | 138, 158, 150 | karate guard/taunt variety (action 6) |
| focus / taunt | 0x23D, 0x187, 0x181, 0x137, 0x18D | ping-pong, loop, loop, loop, hold | 10, 44, 100, 100, 33 | the same variety; 0x181 and 0x23D are also the watchers `0x59E710` (watching, then the end of the show) |
| pose | 0x264 | hold | 344 | karate end-of-fight pose (action 11) |
| pose | 0x266 → 0x241, 0x267, 0x269 → 0x26A → 0x240 | | 167, 195, 68 + 20 | CPU fighters' intro poses (karate entries 0x2B..0x34) |
| singing | 0x143 → 0x144 | next, hold | 170 + 70 | sing `0x5AB2C0` start (props on nodes 4 and 11) |
| singing | 0x145 | hold → itself | 70 | sing `0x5AB2C0` after the first end pulse |
| cheer | 0x24D, 0x24E | loop | 33, 50 | karate win (karate ids 0xD, 0xE) |
| cheer | 0x1EC | loop | 41 | joy `0x5A0BA0`, race behaviour `0x538350`, after courting |
| cheer | 0x1D4 → 0x1D5, 0x1CF → 0x1D0 | next | 25 + ping-pong 5, 14 + 32 | joy `0x5A0BA0` |
| sleep, lying | 0x12 → 0x13 | next, loop | 60 + 200 | sleeping `0x54EF10` pose 2 (eyes 7) |
| sleep, lying | 0x1B → 0x1C → 0x1D | next | 60 + 100 | sleeping `0x54EF10` pose 1 |
| sleep, sitting | 0x16 ↔ 0x17 | ping-pong, hold → 0x16 | 71, 39 | sit-and-nod `0x54F7A0` |
| eating | 0xA9 | hold | 27 | eat `0x5607C0` mode 0 |
| eating | 0xAF, 0xB3 | loop | 46, 100 | eat bites (mode 4) |
| eating | 0xAD → 0xAB | hold → next | 30 + 40 | eat chewing (modes 5, 7) |
| stumble | 0x9F, then 0xA0 | hold, hold | 56, 53 | stumble `0x562D50` (icon 5 for 240 frames, sound 0x100A at count 40) |
| hit taken | 0x249, 0x24A, 0x24B | hold | 35, 50, 50 | karate hit (karate ids 9, 0xA, 0xB) |
| hit taken | 0x256, 0x257, 0x258 | hold | 50, 55, 55 | karate hit with morale broken |
| illness | 0xF2/0xF3, 0xF4..0xF7, 0xF8/0xF9 | hold | 210, 117, 30 | illnesses 1, 0, 4 (`chao-ai.md` §2A.2; sounds 0x606B, 0x606A, 0x606E) |
| knocked down | 0x24C | hold | 46 | karate knock-down (karate id 0xC) |
| lost | 0x24F, 0x250 | loop | 330, 190 | karate loss (karate ids 0x10, 0x11), loser voice 0x6067 |
| bawling | 0x20B → 0x20C, 0x20D → 0x20E | next, loop | 40 + 50, 120 + 40 | sorrow `0x59FCA0` (tears, sound 0x6001) |
| thrown | 0x10F | ping-pong | 11 | thrown `0x564320`, hit `0x563EB0` |

- 0xB0 and 0xB1 have `start == end` (a still pose, step 0); they never send an end pulse.
- The table's posture (+6) doesn't matter here: with the AI off nothing reads `D+0xAE`.
- The arena Chao's MainSub (`arena::chao_main`) writes x, z and facing and zeroes the velocity
  (Data2 +0..+8) every frame; the height is the game's physics (ground mode). A hop motion
  alone stays on the floor.

---

## 2. Emotion balls

- **0x53D660**: ECX = Chao object, DL = state, push frames (caller pops 4). It writes the
  queued state `D+0x6EE`, the length `D+0x6F4` and zeroes the timer `D+0x6F0`. When
  `0x134046C == 0x134062C` (the area is the shown one) it plays sound 0x100E at the Chao.
- The ball's update `0x53CB70` runs in every Chao MainSub, AI or not, so an arena Chao shows it.
- States (`D+0x6EC`, functions at 0x1353834: 0x53C180, 0x53C200, 0x53C380, 0x53C3B0,
  0x53C530, 0x53C790): 0 plain, 1 "!", 3 "?", 4 heart, 5 swirl. State 2's update only sets two of
  the ball's counters (+0x38, +0x7C) to 1.
- The game shows 5 when a Chao stumbles (240 frames), is hit or thrown (150), sleeps deeply
  (360, every 402 frames) and dies (300); 1 when it notices something (180); 4 when petted.

---

## 3. SA2 particles

Each spawner takes a particle from a pool (`0x492660`, EBX = pool) and returns at once when the
particle system is off (`*0x1A5A3B8 == 0`) or the pool is full. Pool +4 points at
`{frames*, count}`; frame entries are 12 bytes `{texture name*, +4, +8}`.

| spawner | call | pool | texture set |
|---|---|---|---|
| 0x6EFF10 | cdecl (&pos, &vel, f32 scale) | 0xB0964C | `kiran01..04` (sparkles; the absorb ring, `0x566D6A`) |
| 0x6ECA30 | cdecl (&pos, &vel, f32 scale) | 0xB0ADA0 | `flame01..` (animal play `0x5A283B`) |
| 0x6ED1C0 | cdecl (&pos, &vel, f32 scale) | 0xB0AC58 | `splash_a01..08` |
| 0x6ED270 | EDI = &vel, cdecl (&pos, f32 scale) | 0xB0ACC8 | `splash_a01..08` (tears, colour 0xFFA090FF) |
| 0x6ED340 | | 0xB0AC90 | `splash_a01..08` |
| 0x6EDD90 | EDI = &vel, cdecl (&pos, f32 scale) | 0xB0A4F0 | `water01..08` |
| 0x6EBE50, 0x6EC050, 0x6EC1D0, 0x6EC2A0 | first: cdecl (&pos, &vel, f32 scale) | 0xB0AE58.. | `snowp01..08` |
| 0x6C9DD0, 0x6CACF0, ... (8) | 0x6C9DD0: EDI = &vel, (&pos, scale); 0x6CACF0: (&pos, &vel, scale) | 0xB486D0.. | `dust01..08` |
| 0x6B80B0, 0x6B81C0, 0x6B82B0 | 0x6B80B0: cdecl (&pos, &vel, f32 scale) | 0xB5D598.. | `bomb03..10` (enemy explosions) |
| 0x6DEB80 | EDI = &vel, cdecl (&pos, f32 scale) | 0xB15FB0 | `exp01..08` |
| 0x6D75A0, 0x6D7670 | 0x6D75A0: EDI = &vel, cdecl (&pos, f32 scale) | 0xB35254, 0xB352C4 | `eff_melody00..05`, `eff_onpu0..1` (music notes) |
| 0x6C4200, 0x6C43B0 | 0x6C4200: cdecl (&pos, &vel, f32 scale) | 0xB562F0, 0xB56328 | `ring01..08` |
| 0x6EA480, 0x6EA570, 0x6EA860 | | 0xB0B060.. | `roketthrust01..08` |

### Which textures are bound

- `0x44C7B0` (EAX = null-ended texture file pairs `{name*, NJS_TEXLIST*}`, push a null-ended
  list of null-ended lists of `{frames*, count}` sets, caller pops) loads the files
  (`0x44C350`) and binds every set. `0x44C810` (EAX = the lists, ECX = the pairs) zeroes every
  frame's +4 and +8 and releases the textures (`0x77F9F0`).
- **0x44C880** (EAX = one `{frames*, count}` set, push NJS_TEXLIST*, caller pops) binds one set:
  for each frame it finds the texture of the same name in the list (`0x44C770`) and copies
  that NJS_TEXNAME's +4 (attributes) and +8 (texture) to the frame's +4 and +8. Names not in
  the list are left as they were.
- A frame whose +8 is 0 has no texture bound.
- The sets (`{frames*, count}`, at pool +4): sparkles 0xB09644, splashes 0xB0AC50, flames
  0xB0AD98, snow 0xB0AE50, dust 0xB48068, explosions 0xB15FA8.
- `efftex_common` (NJS_TEXLIST 0x104D80C, 121 textures) holds the `barria_n`, `barria_t`,
  `bomb01..16`, `bubble`, `dust`, `exp`, `flame01..10`, `kiran`, `lens`, `muteki`, `p_exp`,
  `pan_jetb`, `panlight`, `ring`, `roketthrust`, `snowp`, `splash_a`, `tsuchi_yama` and
  `watercolumn01` textures; not `eff_melody`/`eff_onpu` or `water01..08`.
- The karate area binds `efftex_common` with `kiran` only: its constructor (the one that loads
  `objLandTableChaoKarate`) at `0x52DA02` (pairs 0x1366E60, list 0x1366E48), its destructor
  releases them at `0x52D924`. With the game's own bindings only the `kiran` pool (0x6EFF10)
  has textures in the arena; `efftex_common` itself is loaded there, so `0x44C880` with
  0x104D80C binds any of the sets whose textures it holds.
- The garden constructors bind `efftex_common`/`objtex_common` with `watercolumn`, `kiran`,
  `barria_n`, `tsuchi_yama`, `dust`, `flame` and `splash_a` (pairs 0x125C104, list 0x125C0DC;
  `0x54B79A`, `0x54BD33`, `0x54C340`, `0x54C848`, `0x54D27A`); their destructors release them
  (`0x54B6BC`, ...).
- The race area binds `watercolumn` and `kiran` (`0x52D3E5`, list 0x1367020).
- Karate itself spawns no particle for a hit (`chao-battle-arena.md` §4).

---

## 4. Sounds

`arena::sound` is `0x437260` (ESI = id, push 0, 0, 0). `0x4372E0` (EDI = id, ESI = &position,
3 cdecl) plays at a point.

| id | where the game plays it |
|---|---|
| 0, 2 | karate hits (`0x5787C8`) |
| 0x606F, 0x6070, 0x6071 | karate attack voices |
| 0x6067 | karate loser |
| 0x6008, 0x6009, 0x600A, 0x600B | snores (sleeping `0x54EF10`; 0x6008 is also a karate taunt voice) |
| 0x6001 | bawling (`0x59FCA0`) |
| 0x6069 | dislike (`0x5A1800`) |
| 0x602F..0x6033 | joy (`0x5A0BA0`) |
| 0x606A, 0x606B, 0x606E | illnesses 0, 1, 4 |
| 0x100A | stumble thud (`0x562D50`) |
| 0x100E | emotion ball; karate menu confirm |
| 0x1004, 0x1005 | absorbing an animal |
| 0x1020 | entering water (`0x561630`); 0x101F swimming, 0x1021 floating |
| 0x1026 | singing (`0x5AB2C0`) |
| 0x4007..0x4012 | karate crowd cheers (`0x543360`); in a garden the same ids are the instruments |
| 0x8000 | menu cursor |

The 0x40xx ids mean different sounds in the karate area and in a garden: that bank is the area's.

---

## 5. Camera shake and screen flash

- The karate camera's callback copies its work `C+8` (position) and `C+0x20` (look-at) to the
  view every frame (`chao-battle-arena.md` §5). The battle writes both every frame after the
  karate main object (`battle::game::after` → `Karate::set_camera`), so an offset added there
  moves the view on that frame and is gone on the next.
- `hud.rs` rectangles are D3D9 `Clear` fills: opaque, no blending.
- **0x559720** (u32 colour 0xAABBGGRR, f32 x0, y0, x1, y1, z), cdecl: an untextured rectangle
  in the 640×480 virtual screen, drawn in a display pass; alpha 0xFF takes the opaque path,
  anything less is blended (`chao-details.md` "Draw calls").

---

## 6. The GBA ROM's battle animation graphics

### Tables (BPEE version 0)

| what | offset | entries |
|---|---|---|
| move animation scripts (pret `gBattleAnims_Moves`) | 0x2C8D6C | u32 pointer per move id 0..354, then one unused |
| status condition scripts (`gBattleAnims_StatusConditions`) | 0x2C92FC | 9 |
| general scripts (`gBattleAnims_General`) | 0x2C9320 | 23 |
| sprite sheets (`gBattleAnimPicTable`) | 0x524B44 | 289 × `{u32 ptr to LZ77 4bpp tiles, u16 size, u16 tag}` |
| sprite palettes (`gBattleAnimPaletteTable`) | 0x52544C | 289 × `{u32 ptr to LZ77 BGR555, u16 tag, u16 0}` |

- Tags run 10000 + index in both sheet tables (the sheet at index i has tag 10000 + i).
- The 8 bytes before the move script table are the LE u16 list 47, 195, 320, 0xFFFF (pret
  `gMovesWithQuietBGM`).
- The sheet's `size` is what the game copies to VRAM; the LZ77 header's length is the data's
  real length, and they differ for 11 sheets (indices 19, 25, 38, 83, 84, 90, 97, 102, 118,
  218, 271). Every palette is 32 bytes except index 206 (160 bytes: five 16-colour palettes).
  The palette entry at index 47 carries tag 10046; every other entry's tag is 10000 + index.
- Status condition scripts by index: 0 poison, 1 confusion, 2 burn, 3 infatuation, 4 sleep,
  5 paralysis, 6 freeze, 7 and 8 two move-made conditions. Their sheets: 1 → 10073, 2 → 10029,
  3 → 10210, 4 → 10228, 5 → 10011, 7 → 10200, 8 → 10221; 0 and 6 draw with palette and
  background tasks (6 loads 10010, 144 tiles, which its task draws as a 64×64 sprite with
  subsprites; no move script makes a sprite of it). General script 15 (a plain hit) uses
  10135.

### Script bytecode

One opcode byte, then its operands (LE; pointers are ROM addresses, 0x08000000 + offset):

| op | name | bytes | operands |
|---|---|---|---|
| 0x00 | load sheet | 3 | u16 tag (loads the sheet and the palette of that tag) |
| 0x01 | unload sheet | 3 | u16 tag |
| 0x02 | create sprite | 7 + 2n | u32 SpriteTemplate*, u8 (0x80 = placed at the target, low 7 bits subpriority), u8 n, n × s16 |
| 0x03 | create task | 7 + 2n | u32 function, u8 priority, u8 n, n × s16 |
| 0x04 | delay | 2 | u8 |
| 0x05, 0x06, 0x07 | wait / nop | 1 | |
| 0x08 | end | 1 | |
| 0x09 | sound | 3 | u16 |
| 0x0A, 0x0B | | 2 | u8 |
| 0x0C | | 3 | u16 |
| 0x0D | | 1 | |
| 0x0E | call | 5 | u32 script |
| 0x0F | return | 1 | |
| 0x10 | set argument | 4 | u8, u16 |
| 0x11 | pick by turn | 9 | u32 script, u32 script (no fall-through) |
| 0x12 | jump if turn | 6 | u8, u32 script |
| 0x13 | goto | 5 | u32 script |
| 0x14, 0x18 | | 2 | u8 |
| 0x15, 0x16, 0x17 | | 1 | |
| 0x19 | sound with pan | 4 | u16, u8 |
| 0x1A | | 2 | u8 |
| 0x1B | | 7 | u16, 4 × u8 |
| 0x1C | | 6 | u16, 3 × u8 |
| 0x1D | | 5 | u16, 2 × u8 |
| 0x1E | | 3 | u16 |
| 0x1F | sound task | 6 + 2n | u32 function, u8 n, n × s16 |
| 0x20 | | 1 | |
| 0x21 | jump if argument | 8 | u8, u16, u32 script |
| 0x22, 0x23 | | 2 | u8 |
| 0x24 | jump if contest | 5 | u32 script |
| 0x25 | | 4 | 3 × u8 |
| 0x26, 0x27 | | 7 | u16, 4 × u8 |
| 0x28, 0x2A..0x2E | | 2 | u8 |
| 0x29, 0x2F | | 1 | |

Walking a script means following `call`, `goto` and both targets of 0x11, 0x12, 0x21 and 0x24
with a visited set, stopping at `end` and `return`. With those rules every move script
1..354 parses to the end without an unknown opcode.

### Sprite templates

- `SpriteTemplate` (0x18 bytes): u16 sheet tag, u16 palette tag, u32 OamData*, u32 anims
  (table of AnimCmd lists), u32 images, u32 affine anims, u32 callback.
- OamData (8 bytes): shape = byte 1 bits 6..7, size = byte 3 bits 6..7, 8bpp = byte 1 bit 5.

| shape \ size | 0 | 1 | 2 | 3 |
|---|---|---|---|---|
| 0 square | 8×8 | 16×16 | 32×32 | 64×64 |
| 1 wide | 16×8 | 32×8 | 32×16 | 64×32 |
| 2 tall | 8×16 | 8×32 | 16×32 | 32×64 |

- AnimCmd (4 bytes): s16 at +0: −1 end, −2 jump, −3 loop; otherwise a frame: u16 tile offset
  (8×8 tiles of 32 bytes into the sheet), bits 16..21 duration, bit 22 h-flip, bit 23 v-flip.
  A template whose `anims[0][0]` is the end command shows tile 0.
- A frame's tiles are row-major, `w/8` per row (1D mapping), 4bpp, low nibble the left pixel;
  colour 0 is transparent.
- Templates whose sheet tag is below 10000 are palette-blend and shake helpers with no sheet.
- Across the 354 move scripts: 2115 sprite creations with a sheet, all 4bpp, palette tag
  always equal to the sheet tag; 9 sizes (32×32, 16×16, 8×8, 8×16, 32×16, 64×64, 16×32,
  32×64, 64×32). 1191 of them are placed at the attacker.
- Tag 10135 (the plain hit) appears in 106 move scripts; 25 scripts use only it and 47 create
  no sprite with a sheet (tasks and palette effects only).
- A move's type is byte 2 of its record (`rom.rs` `MOVES`), so the commonest sheet among the
  moves of each type can be counted from the ROM; every type id 0..17 has one.

---

## 7. Shiny palettes

- pret `gMonPaletteTable` at 0x303678: 440 × `{u32 ptr to LZ77 BGR555, u16 tag = index, u16 0}`.
- pret `gMonShinyPaletteTable` follows it at **0x304438**: 440 entries of the same layout with
  tag = 500 + index. Each palette is 32 bytes after LZ77 (16 colours), for the same front
  sprite as the normal palette.
- A party member is shiny when `(id_hi ^ id_lo ^ pid_hi ^ pid_lo) < 8`, with the 32-bit
  original trainer id and personality split into their high and low 16 bits (1 in 8192 for a
  random personality).
