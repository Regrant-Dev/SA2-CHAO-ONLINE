# Detailed Chao inspection

`src/chao_details.rs` formats record snapshots for the F9 **Chao details** view.
Snapshots come from `game::read_chao_slot` (memory after Chao World loads, otherwise the save
file). The menu rereads them each frame and keeps a save-slot identity, never an
ObjectMaster/Data1 pointer. Holding a Chao preselects its slot when it is one of the local
save's records. Inspection makes no AI or mutation calls.

## Field names

Field names for personality axes, emotions, symptoms and appearance follow
[Justin113D/SA2ChaoStatMenu](https://github.com/Justin113D/SA2ChaoStatMenu)'s `ChaoData.h`
(naming only; the mod's formatting, navigation and HUD drawing are its own, with no
third-party assets). A name does not prove that a hidden trait affects vanilla gameplay.

The view opens on a summary page (abilities, alignment and leanings, evolution or time left,
happiness, battle level and partner), followed by detail pages and a raw page with the
uninterpreted data. Detail pages show exact values next to their readings: personality words
bucket the game's −80/−40/0/40/80 trait values, an illness shows as present while its counter
is non-zero, and time left counts age ticks by the rules in re/chao-ai.md ("Age tick") and
converts them to garden time. A child's predicted form runs the evolution rules on a copy of
its record.

## Native life/emotion tick

`0x5353C0`, the native life/emotion tick, reads
the record via `ObjectMaster+0x34 -> Data1+0x5C` and uses emotion base `record+0x124`.
It increments clock `+0xA4`, age `+0x86`, adult age `+0x88`, adjusts maximum lifespan `+0x8C`,
and clamps remaining life `+0x8A` to the maximum. These are counters, not real-world minutes.

Field widths from this function:

- Emotion magnitudes at `+0x12C..+0x133` are unsigned bytes clamped to 0..200. Reading them
  as signed chars would incorrectly turn values above 127 negative.
- Need/condition fields at `+0x134..+0x148` are unsigned 16-bit counters clamped to 0..10000.
- Personality modifiers at `+0x14A..+0x156` use signed-byte arithmetic. Examples include
  `+0x14C`, `+0x14F`, `+0x150`, `+0x155` and `+0x156` influencing different rates.
- Six condition counters `+0x15A..+0x15F` move toward zero using signed-byte arithmetic.

The decompiler reports bad instruction/control-flow boundaries in part of this function, so
its pseudocode is incomplete; the operand widths and clamp blocks above are reliable.

Native debug-label data in the dumped image includes `ANGER`, `SORROW`, `FEAR`, `PAIN`,
`RELAX`, `TOTAL`, `SLEEPY`, `SLPDPTH`, `HUNGER`, `BREED`, `TEDIOUS`, `LONELY`, `TIRE`,
`STRESS`, `NOURISH`, `CONDITN` and `THIRSTY` around `0x12E5BC4..0x12E5CCC`. These corroborate
the emotion/need layout. The `+0x148` field is called energy in the old viewer, but THIRSTY
in native debug data; the details view labels it thirst.

Happiness `+0x82`, alignment `+0xB0`, tendencies `+0xA8/+0xAC`, evolution `+0xC0` and the
six-character bond table `+0x16C` are covered in [chao-garden-items.md](chao-garden-items.md).
The two legacy form IDs 3/4 precede the normal forms: 5 is Neutral Normal, 6 Hero Normal,
20 Neutral Chaos and 23..25 Tails/Knuckles/Amy. Don't shift these by omitting the legacy IDs.

Some appearance and personality fields are unused in vanilla; the view shows them as stored
values/IDs. Eggs and invalid hidden-stat filler show as uninitialized, and nonfinite shape
floats are not drawn as gauges.

## The status balloon (ParamFukidasi)

Addresses from the dumped exe (d1ac8ed9…). Texture contents are from the PC
`resource/gd_PC/PRS/al_tex_common.pak` (DDS).

### Lifecycle

- Create **0x5659E0** (usercall EAX = Chao obj). Closes an old balloon (`[[*0x1AED28C]+0x40] = 7`,
  global cleared), then `0x46F680("ParamFukidasiExecutor")` with ESI = list 3, EDI = main
  **0x564C80**; Data1 from 0x470B40. Data2 (`obj+0x40`) = 0xA0-byte block (+4 past a 0x12345678
  tag). `obj+0x1C` (delayed/2D display) = **0x565200** (written by `C7 46 1C imm32` at 0x565A80,
  imm at **0x565A83**); `obj+0x18` delete = 0x5659C0 (clears 0x1AED28C if it is this obj).
  Stores the global last.
- Side: projects the Chao (0x77FB00); screen x ≤ 320 → balloon X = 520 (`+0x30` = 1, right side),
  else X = 120 (`+0x30` = 0). Y = 216 fixed. The balloon never sits on the Chao; it sits on the
  other half of the screen.
- Callers: 0x560771 (take offered fruit 0x5606D0), **0x565B50** (cdecl obj; held, from 0x566720;
  only when CurrentChaoArea 0x134046C is 1..3; forces X = 120, side 0), 0x566ED2 (absorb
  animal/drive 0x566DF0 mode 0). Close: 0x565B90 (state 7), 0x565BC0 (state 6), 0x565DC0, and
  the mod's `garden::delete_chao`.

Data block (`d = obj+0x40`):

| off | type | use |
|---|---|---|
| +0x00 | u8 | state |
| +0x04 | int | state timer |
| +0x0C | i8 | frame rows shown (0..5); frame height = rows·40 |
| +0x10 / +0x18 | f32 | X / target X (X eases 4 %/frame to the target) |
| +0x14 / +0x1C | f32 | Y = 216 / copy; only the display reads +0x14 |
| +0x20 | f32 | content alpha 0..1 (content drawn when > 0.01, 0x109E000) |
| +0x28 / +0x2C | int | += 0x260 / 0x180 each frame (not used by the display) |
| +0x30 | int | side, 1 = right (X 520), 0 = left (X 120) |
| +0x34 | int | frames since the last side check |
| +0x38 / +0x3C | ptr | Chao obj / record (`Data1+0x5C`) |
| +0x40 / +0x44 / +0x48 | int | count-up active / step delay / level-up sound cooldown |
| +0x4C + 0x10·i | row i (0..4) | +0 u8 shown level, +1 u8 shown bar segments 0..10, +2 u16 shown points, +4 int level-up anim timer (1..120), +8 int side at level up, +0xC i16 points gained |

Main 0x564C80 (jump table 0x5651D0):

| state | meaning |
|---|---|
| 0 → 1 | grow: `+0xC`++ per frame until 5 (`cmp byte [edi+0xC],5` imm at **0x564CBD**, `mov byte [edi+0xC],5` imm at **0x564CC7**) → 3 |
| 3 | fade in: alpha += 0.1 → 1.0 → 4 |
| 4 | shown until closed |
| 5, 6 | shown, closes after 180 frames (6 set by 0x565BC0 after eating) |
| 7 | fade out: alpha −= 0.1 → 9 (the global is cleared by whoever sets 7) |
| 9 | shrink: `+0xC`-- → 10 |
| 10 | main = 0x46F720 (delete) |

In 4/6: after 100 frames it re-projects the Chao; Chao x in 450..640 while on the right → move
left; x in 0..190 while on the left → move right; off-screen (x < 0 or > 640) → state 7. It
counts shown points toward `rec+0x38+2i` (one per 8 frames while that row's anim runs) and,
when a row's shown level/segments differ from the record, steps the segments every 20 frames
(after a 60-frame delay); segment 11 → level+1, segments 0, level-up anim, sound 0x443480.
The grade (`rec+0x28`) is never shown.

### Record reads

Rows i = 0..4 (swim, fly, run, power, stamina): level `rec+0x30+i` (u8), progress `rec+0x20+i`
(0..99) → segments `(p+5)/10` (10 when the shown level is below the record's), points
`rec+0x38+2i` (u16). Name `rec+0x12` (7 bytes, the game's alphabet).

### Display 0x565200 (cdecl obj, from obj+0x1C)

Returns if state = 0. Sets `*0x1AED260 = 0` (blend mode read by the sprite drawers) and colour
(0.4, 1, 1, 1). Frame, all on sprite sheet `level00` (see below), x = X, alignX = 0, z −1.5:

- n = `+0xC`; top = Y − (n/2)·40 − (n odd)·20 (Y − 100 at n = 5).
- 0x1312B38 cap (165×24) at top−8, alignY 1 (bottom); 0x1312B5C top edge (165×8) at top,
  alignY 1; 0x1312BA4 bottom (165×12) at top + n·40, alignY −1; n × 0x1312B80 middle
  (165×40) at top + 40k, alignY −1. Each middle piece is one row's dark inset box.

Content (when rec ≠ 0 and alpha > 0.01), x0 = X − 72, row y = Y − 96 + 40i, alignX/Y −1:

| what | call | where |
|---|---|---|
| stat word | 0x558BE0, sprite 0x1312A84 + 0x24·i, colour (a,1,1,1) | (x0, y) |
| "Lv." | sprite 0x1312A3C, colour (a,1,1,0) | (x0, y+20) |
| level digits | sprite 0x13128B0 + 0x24·d (tens, units) | (x0+24, y+20), (x0+36, y+20) |
| points | 0x5369F0 (4 digits) | (x0+86, y+5) |
| points gauge | 0x536770 | (x0+49, y+20), 92×8 |
| progress | 0x536E80(shown seg, target seg, …) | (x0+49, y+26), 92×8 |
| level-up | "LEVEL UP!" 0x1312A60 for t 1..60, then "+" 0x1312A18 and 2 digits of the gain for t 61..120, rising t/2 px and fading | x = x0+231 (balloon left) or x0−66 (balloon right) |
| name | 0x536BE0(rec+0x12, x0, Y−124, 21, 21, a<<24 \| 0x00FFFF, 0), ECX = −1 | in the cap |

Ends by setting the colour global `0x12D27A4` = 0xFFFFFFFF.

### Draw calls

- **0x558BE0** sprite: EAX = sprite desc; stack (f32 x, f32 y, f32 z, f32 scaleX, f32 scaleY,
  int alignX, int alignY), caller pops 0x1C. Align 0 = centre, 1 = right/bottom edge at the point,
  other (−1) = left/top. 640×480 virtual screen (0x558CA0 maps from 320/240). Colour = global
  `0x12D27A4`. Desc (0x24 bytes): u32 1 (float UVs; 0 = texel ints / 256), f32 w, f32 h,
  f32 u0, v0, u1, v1, NJS_TEXLIST*, u32 texture index.
- **0x559FA0**(f32 a, r, g, b) cdecl: sets `0x12D27A4` (byte0 r, 1 g, 2 b, 3 a).
- **0x559720**(u32 colour, x0, y0, x1, y1, z) cdecl: untextured rectangle. Colour 0xAABBGGRR
  (it swaps to D3D order). Alpha 0xFF picks the opaque path.
- **0x536E80**(int shown, int target, x, y, w, h, alpha) cdecl: black box w×h, then 10 segments
  7×4 every 9 px at (x+2, y+2): both → 0xFF5A5A (blue), shown only → 0xFF (red), target only →
  0xFFFF (yellow), neither → 0x7F7F7F.
- **0x536770**(x, y, w, h, alpha) cdecl, EAX = value: black box, inner box and a fill of
  `(w−4)·(v%1000)/1000`; inner/fill colours = table[v/1000], table[v/1000+1] (grey, yellow,
  orange, red, pink…; from stack constants, order approximate).
- **0x5369F0**(x, y, alpha) cdecl, EAX = value: white 4 digits of `v%10000` (leading zeros) at
  x, +14, +28, +42, z −1.2.
- **0x536BE0** text: (char* s, x, y, f32 size_w, f32 size_h, u32 colour 0xAABBGGRR, int mode)
  cdecl, ECX = align (−1 left, 0 centre, 1 right). mode 0 = at most 7 chars, else NUL-ended.
  Glyph advance = size_w − 1, scale size/22, z −1.2. Codes: 1..0x5E = ASCII − 0x20 (glyph c−1),
  0x60..0xFE glyph c−2 (accented), 0x5F and 0xFF are skipped but advance (use 0x5F for a space;
  0 ends the string). Glyphs: AL_TEX_COMMON index 1, 22-px cells, 23 per row.

### Textures

Texlist **0x1366ABC = AL_TEX_COMMON** (7 textures; loaded by the Chao texture list at
0x52AC5A, along with AL_COMMON_TEX 0x1366AB4, AL_EYE 0x1366AC4, AL_ICON 0x1366ACC (58, emotion
ball icons, used at 0x53D0A6), AL_jewel, AL_MOUTH, AL_MINIMAL_TEX 0x1366AEC):

| idx | name | contents |
|---|---|---|
| 0 | ad_common (128²) | buttons (x 0..40), icon tiles 32×32 at x 40..72: ear y 0..32, **heart** 32..64, **"?"** 64..96, **"!"** 96..128 (UV u 0.3125..0.5625, v by quarters); orange item tile (u 0.5625..0.8125, v 0..0.25), small gauge, ring panel |
| 1 | ad_22ascii | font (above) |
| 2 | level00 | balloon frame, "LEVEL UP!", digits 0..9, "+", "Lv.", Swim/Fly/Run/Power/Stamina, three ovals |
| 3..6 | panel_seed / egg / fruit / item_sp | 32² item panels |

level00 sprite descs (all index 2): digits 0x13128B0.. (10 × 12×14), "+" 0x1312A18, "Lv."
0x1312A3C (24×14), "LEVEL UP!" 0x1312A60 (88×16), words 0x1312A84.. (48/32/40/56/72 × 20),
frame 0x1312B38/B5C/B80/BA4, ovals 0x1312BC8/BEC/C10 (36×26, 28×22, 18×16; nothing in the
exe references them; they look like a speech-bubble tail). The PC DDS files are 2× the DC size;
UVs are normalized so this does not matter.

### Extra rows (src/balloon.rs)

The mod shows 11 rows: the game's five, Luck and Intelligence drawn like them, and N = 4 of its
own (happiness/alignment, evolution or form/time left, hunger/mood, partner types/battle level),
plus a grade letter on each ability row:

1. Display: the imm32 at 0x565A83 points to the mod's cdecl fn, so new balloons call it. It
   draws the frame, calls 0x565200, then draws the rest.
2. Frame: the imm8s at 0x564CBD and 0x564CC7 are 11, so `+0xC` grows to 11 and shrinks
   back from it. The game's frame is skipped by a 5-byte `JMP 0x5653C4` (`E9 56 01 00 00`) at
   **0x565269** (over `AND EAX,0x80000001`; ESP is back at the frame base there, EBX = data,
   EBP = 0, X/Y locals already set). The mod draws the frame with the four frame sprites above
   (cap 24 px tall, top edge 8, middle 40, bottom 12) and n = `+0xC`, with its top at Y − 100
   so the vanilla rows (Y − 96 + 40i) and name (Y − 124) line up: middle k at top + 40k, the
   last row (10) scaled to 36 px tall, and the bottom under the last middle. Each piece is
   drawn with scaleX (165 + 24)/165 centred at X − 12: the frame reaches 24 px further left,
   for the grade column. `*0x1AED260 = 0` and colour (0.4,1,1,1) are set first.
3. To keep it on screen, `+0x14` = 132 (only the display reads it): the cap starts at y = 0
   and the bottom edge ends at 480. With the side X of 120 or 520 the widened frame spans
   X − 106.5 .. X + 82.5.
4. Everything below is drawn only when rec ≠ 0, alpha `+0x20` > 0.01 and the frame has grown
   to the row (i < n). x0 = X − 72, row y = Y − 96 + 40i.
   - Grade letters, rows 0..6: grade `rec+0x28+i` (0..6 = E..X) with 0x536BE0, size 18, at
     (x0 − 22, y + 7), coloured by grade. Luck and Intelligence can hold a grade byte past X
     (their grade genes can come from leftover stack bytes in 0x5506B0); 0x535E90 levels them
     all the same (3·grade + 11 + 0..4 points, capped at 4000), and they show as "?".
   - Rows 5 and 6 (luck, intelligence) copy 0x565200's row loop (objdump 0x565456..0x565694),
     reading the record directly (the count-up block `+0x4C + 0x10i` covers rows 0..4 only):
     the word with 0x536BE0 (size 18; level00 has no word sprite for them); colour
     (a,1,1,0), "Lv." 0x1312A3C at (x0, y+20) and the level's two digit sprites at x0+24 and
     x0+36; 0x5369F0 (EAX = points; x0+86, y+5, alpha); 0x536770 (EAX = points; x0+49, y+20,
     92 (f32 0x906C88), 8, alpha); 0x536E80(s, s, x0+49, y+26, 92, 8, alpha) with
     s = `(progress + 5) / 10`.
   - Rows 7..9: label and value text with 0x536BE0 (size 16), two lines each (y+2 and y+22).
   - Row 10: the partner's type names (text) over "Lv." and the battle level's digits, with
     0x536E80 at (x0+49, y+26) for the XP toward the next level when it is known.
   Sources: happiness `rec+0x82` (s16, −100..100), alignment `rec+0xB0` (f32 ±1), evolution
   `rec+0xC0`, leanings `rec+0xA8/0xAC`, life `rec+0x8A/0x8C`, adult age `rec+0x88`, emotions
   `+0x12C..0x133` (u8 0..200), needs `+0x134..0x148` (u16 0..10000).
5. After the rows, `0x12D27A4` is reset to 0xFFFFFFFF.

Constraints: the hook is cdecl(obj), called from the delayed display list; it must keep
EBX/ESI/EDI/EBP and leave the x87 stack empty. 0x558BE0, 0x5369F0 and 0x536770 take EAX and
0x536BE0 takes ECX, so they need small asm thunks. Draw order is call order; the frame is 40 %
alpha, so frame pieces must not overlap. AL_TEX_COMMON is loaded in every Chao area that makes
the balloon. Sprite descs must stay in static memory. There is one balloon at a time, and
`+0x3C` points into the live record, so it is read each frame.

### Opening it without a pickup (src/chao_peek.rs)

- Open 0x5659E0: usercall EAX = Chao obj, saves EBX/ESI/EDI; reads `[[EAX+0x34]+0x5C]` (record)
  unchecked, so only call with Data1 and record set. Global 0x1AED28C = the new balloon after.
- Close 0x565B90: ECX = Chao obj, no stack args, uses only EAX. If `*0x1AED28C` ≠ 0 and its
  data `+0x38` == ECX (or ECX == 0: any) → state 7, global cleared. Never derefs ECX.
- "In front" (player action list 0x475AF0 → 0x486BB0(0x4000 − rotY)): target at
  `atan2(dx, dz)` (0x859B6A, ST1 = dx, ST0 = dz; ×−10430.38 at 0x905658) within ±0x2000 BAMS
  of 0x4000 − rotY, i.e. forward = (cos rotY, sin rotY) in (x, z), 45° half-cone.

## Controller buttons in the garden

Addresses from the dumped exe d1ac8ed9….

Pads (`PDS_PERIPHERAL`-like, 0x34 bytes): +0x04 support mask, **+0x08 held**, +0x0C ~held,
**+0x10 pressed this frame**, +0x14 released, +0x1C/+0x1E stick x/y (i16, ×128/72; menus use
±0x27 as the dead zone, 0x441AB0).

- Raw: 0x1A52918 + i·0x34 (0x77E8A0 picks by port), written by 0x77E780 from 0x425700.
  `*0x1DEFB60` (+4·i) = player i's raw pad (dummy 0x1A55954 when none). Not gated.
- Game copy: 0x1DEFC00 + i·0x34 (i = 0, 1), copied in 0x441BA0 only while 0x174AFFE and
  ControllerEnabled 0x174AFFF are 1, else zeroed. Characters read this copy.
- 0x442390: from the copy, per player: A (4) → jump bytes 0x1DEFAA4/0x1DEFB80/…;
  B|X (0x402) → action bytes 0x1DEFBF0 (held) / **0x1DEFB00 (pressed)** / 0x1DEFB20 / 0x1DEFB42.
- 0x441AB0: menu buttons 0x1DEFAB0 (pressed, with key repeat) / 0x1DEFB10 (held) = pad bits
  | stick as 0x10 up, 0x20 down, 0x40 left, 0x80 right.

PC mapping, two tables. Device button index → intermediate (0x8AE160, 11 pairs per port):
0 Start→0x1000, 1 up→8, 2 down→4, 3 left→1, 4 right→2, 5 A→0x100, 6 B→0x200, 7 X→0x400,
8 Y→0x800, 0xB→0x40, 0xC→0x20 (XInput also sets 0x40/0x20 from trigger axis 0xF). Intermediate
→ DC bits (0x9A2088, used by 0x77E910): 1→0x40, 2→0x80, 4→0x20, 8→0x10, 0x40→0x20000 (R),
0x20→0x10000 (L), 0x10→0x100 (no device button maps to it), 0x100→4 (A), 0x200→2 (B),
0x400→0x400 (X), 0x800→0x200 (Y), 0x1000→8 (Start). Support mask written 0x1F07FE.

So DC bits: 2 B, 4 A, 8 Start, 0x10/0x20/0x40/0x80 D-pad up/down/left/right, 0x200 Y,
0x400 X, 0x10000 L, 0x20000 R. With no controller, the keyboard path (0x425650) sets both the
D-pad bits and the stick (±0x80) from its movement keys.

Garden use (Sonic's action code 0x719920, the action switch 0x7230E0, cameras):
- A jump; B/X (0x402) the action list (pick up, pet 43, tree, put down/throw…); standing
  (action 0) the list always ends with **whistle 42** (0x719920, action 0 case).
- Holding (actions 0x16/0x17): `pressed & 0x602` (B, X, **Y**) → 0x4754F0; 0xC00 (X, Y) adds
  action 0x18. So Y is taken while holding.
- Camera 0x4F3DB0: held L/R (0x10000/0x20000) turn it. 0x4F09A0 (a free camera) moves on the
  D-pad while held, not the normal garden camera.
- Idle (action 0x26) breaks on any of 0x306FE (includes the D-pad bits; harmless).
- Chao World menus/machines (0x541A50, 0x553AC0, 0x52FC60…) read the D-pad via 0x1DEFAB0, but
  only while their own screen runs (controls are off for the player then).
- 0x5381E0 (Chao World camera manager create): Y held at that moment picks another camera.

The status-balloon peek (src/chao_peek.rs) uses **D-pad up** (`held & 0x10` on the raw pad,
edge-detected by the mod), only while the stick is inside ±0x27, so a keyboard's "forward"
(which also sets 0x10, with the stick at full tilt) doesn't fire it.

## The Chao save checksum and saving outside a garden

The Chao save lives in memory as two 0xCA6C-byte copies at **0x19F6460** and **+0xCA6C**;
`0x173D06C == 1` selects the second and GetSaveData **0x52E440** returns the active base. On
disk (`SONIC2B__ALF`, Steam layout) the payload is at file offset 0x3040.

Pre-save pass **0x52F090**: `0x52E6B0()` (walks the live Chao counts at 0x1DC0F90/0x1DC0F94,
0x530410 → 0x548660 writes each live Chao back into its record; its area test
`0 < area || area < 4` is always true, so it trusts whatever the counts hold), `0x52E710(cat)`
for item categories 2/3/7/9 (live item positions into their records), `0x52EDE0()` (copies
live garden globals at 0x1DBED8C.. into save +0xA0C..+0xA62 of the active copy), the race copy
when `*0x134046C == 4`, then the checksum. The first two need a loaded garden; none of these
run when the mod saves outside a garden.

Checksum **0x52EEE0**: usercall **ESI = save base**, no stack arguments, plain `ret`. Clobbers
EAX/ECX/EDX, calls rand 0x7A89D8, saves/restores the x87 control word. It zeroes bytes
0xCA69/64/6B/66/67, writes rand salt to 0xCA65/68/6A, then calls **0x549C40** (ECX = base):
a reflected CRC-32 loop over all 0xCA6C bytes, `crc = (crc >> 8) ^ T[(crc ^ b) & 0xFF]`, start
0x6368616F ("chao"), final XOR 0x686F6765 ("hoge"). **T at 0x8A6FF8 is not the standard
table**: the low 24 bits match poly 0xEDB88320 but the top bytes differ (T[1] = 0xC9073096),
so it isn't any polynomial's table (`saving.rs` embeds it). Result bytes 0..3 go to
0xCA69/0xCA64/0xCA6B/0xCA66, then 0xCA67 gets more salt (not covered). The Rust
reimplementation matches the stored checksum of a real Steam save.

Mod (`game::chao_save_snapshot`, `saving.rs`): in a garden (state 16/17 with a player) the full
0x52F090 runs. Elsewhere, only with Chao data in memory and either a running/paused
stage (GameMode 5/10, GameState 16/17, a player; Chao World's lobby and other areas included)
or the menus (GameMode 0xE), the thunk calls 0x52EEE0 with ESI = the validated active base
and copies the buffer as is. Every snapshot is re-verified with the Rust checksum before the
atomic write. Autosaves wait 2 s after any GameMode/level/area change.
