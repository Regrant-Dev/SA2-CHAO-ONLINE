# Detailed Chao inspection

`src/chao_details.rs` formats owned record snapshots for the F9 **Chao details** view.
Snapshots come from the existing `game::read_chao_slot` path (memory after Chao World loads,
otherwise the save file). The menu rereads them each frame and retains a save-slot identity,
never an ObjectMaster/Data1 pointer. Holding a Chao initially selects its slot only when the
current registry identifies it as one of our own save records. No AI or mutation calls are
needed for inspection.

## Existing viewer reference

[Justin113D/SA2ChaoStatMenu](https://github.com/Justin113D/SA2ChaoStatMenu) provides an existing
in-garden held-Chao viewer. Its `ChaoData.h` and stat display identify fields useful for this
feature, including personality axes, emotions, symptoms and appearance. The repository had
no license listed when checked on 2026-10-19. Our Rust formatting, navigation and HUD drawing
are implemented independently with our existing menu and font; no third-party assets are used.

The reference supplies naming/interpretation, not proof that every hidden trait affects
vanilla gameplay. Display exact signed/unsigned values without assigning invented personality
categories or health diagnoses. The raw page preserves access to uninterpreted data.

## Native checks (REA / offline Ghidra VM)

`0x5353C0`, the native life/emotion tick, was decompiled from a staged code slice. It reads
the record via `ObjectMaster+0x34 -> Data1+0x5C` and uses emotion base `record+0x124`.
It increments clock `+0xA4`, age `+0x86`, adult age `+0x88`, adjusts maximum lifespan `+0x8C`,
and clamps remaining life `+0x8A` to the maximum. These are counters, not real-world minutes.

The native pass confirms:

- Emotion magnitudes at `+0x12C..+0x133` are unsigned bytes clamped to 0..200. Reading them
  as signed chars would incorrectly turn values above 127 negative.
- Need/condition fields at `+0x134..+0x148` are unsigned 16-bit counters clamped to 0..10000.
- Personality modifiers at `+0x14A..+0x156` use signed-byte arithmetic. Examples include
  `+0x14C`, `+0x14F`, `+0x150`, `+0x155` and `+0x156` influencing different rates.
- Six condition counters `+0x15A..+0x15F` move toward zero using signed-byte arithmetic.

The decompiler reports bad instruction/control-flow boundaries in part of this function;
use the observed operand widths and clamp blocks, not that incomplete pseudocode as an
exhaustive behavioral model. The inspection code does not execute or hook this function.

Native debug-label data in the dumped image includes `ANGER`, `SORROW`, `FEAR`, `PAIN`,
`RELAX`, `TOTAL`, `SLEEPY`, `SLPDPTH`, `HUNGER`, `BREED`, `TEDIOUS`, `LONELY`, `TIRE`,
`STRESS`, `NOURISH`, `CONDITN` and `THIRSTY` around `0x12E5BC4..0x12E5CCC`. These corroborate
the emotion/need layout. The `+0x148` field is called energy in the old viewer, but THIRSTY
in native debug data; our view labels it thirst/raw rather than assuming an energy mechanic.

Happiness `+0x82`, alignment `+0xB0`, tendencies `+0xA8/+0xAC`, evolution `+0xC0` and the
six-character bond table `+0x16C` were already verified in [chao-garden-items.md](chao-garden-items.md).
The two legacy form IDs 3/4 precede the normal forms: 5 is Neutral Normal, 6 Hero Normal,
20 Neutral Chaos and 23..25 Tails/Knuckles/Amy. Don't shift these by omitting the legacy IDs.

Appearance and personality field names also use the existing viewer as a reference. Some
fields are unused in vanilla; these are displayed as stored values/IDs, without speculative
effects. Eggs and invalid hidden-stat filler are shown as uninitialized, and nonfinite shape
floats are not drawn as valid gauges.

## Checks

Tests cover unsigned emotion 200, signed personality -80 and affection -100, 16-bit needs,
unchanged record bytes after inspecting every page, egg filler, NaN values, form IDs, stable
save-slot navigation, disappearing selections, and fitting 320x240 through 2560x1440.
Native HUD appearance and navigation still need an in-game check after updating.

## The status balloon (ParamFukidasi)

REA/Ghidra on the dumped exe (d1ac8ed9…), 2026-10-19. Texture pictures checked from the PC
`resource/gd_PC/PRS/al_tex_common.pak` (DDS, extracted locally, not committed).

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
  animal/drive 0x566DF0 mode 0). Close: 0x565B90 (state 7), 0x565BC0 (state 6), 0x565DC0, our
  `garden::delete_chao`.

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
  orange, red, pink…; read from stack constants, order approximate).
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

### Adding N rows (plan)

1. Display: write our cdecl fn into the imm32 at 0x565A83 (new balloons call it). It
   calls 0x565200 and then draws our rows.
2. Frame: patch the imm8s at 0x564CBD and 0x564CC7 from 5 to 5 + N, so `+0xC` grows to 5 + N and
   shrinks back from it. Skip the game's frame: 5-byte `JMP 0x5653C4` (`E9 56 01 00 00`) at
   **0x565269** (over `AND EAX,0x80000001`; ESP is back at the frame base there, EBX = data,
   EBP = 0, X/Y locals already set). Before calling 0x565200, our fn draws the frame itself with
   the four frame sprites above and n = `+0xC`. Its top must stay at Y − 100 so the vanilla rows
   (Y − 96 + 40i) and name (Y − 124) line up: top = Y − 100, middle k at top + 40k, bottom at
   top + 40n. Set `*0x1AED260 = 0` and colour (0.4,1,1,1) first.
3. To keep it on screen, write `+0x14` = 216 − 20N (only the display reads it). N ≤ 4 with
   40-px rows (the cap then starts at y ≈ 4 and the bottom ends at ≈ 408).
4. Extra row k (y = Y − 96 + 40·(5+k), x0 = X − 72) only when rec ≠ 0 and alpha > 0.01, using
   alpha `+0x20`: label at (x0, y) with 0x536BE0 (white, size ~16, NUL mode) or an ad_common
   icon (heart for happiness, "!"/"?" for mood) through our own desc → 0x558BE0; bar at
   (x0+49, y+26) via 0x536E80 (shown = target = segments) or a 0x559720 bar (alignment: centred,
   blue for hero, red for dark); number via 0x536BE0 text (handles signs) or 0x5369F0.
   Values: happiness `rec+0x82` (s8, −100..100), alignment `rec+0xB0` (f32 ±1), emotions
   `+0x12C..0x133` (u8 0..200), needs `+0x134..0x148` (u16 0..10000), personality
   `+0x14A..0x156` (s8). Each 40-px row has two lines (y and y+20), like the vanilla rows.
5. After our rows, reset `0x12D27A4` = 0xFFFFFFFF.

Constraints: the hook is cdecl(obj), called from the delayed display list; keep EBX/ESI/EDI/EBP
and leave the x87 stack empty. 0x558BE0, 0x5369F0 and 0x536770 take EAX and 0x536BE0 takes ECX,
so they need small asm thunks. Draw order is call order; the frame is 40 % alpha, so do not
draw frame pieces over each other. AL_TEX_COMMON must be loaded (it is in every Chao area that
makes the balloon). Our sprite descs must stay in static memory. One balloon at a time;
`+0x3C` points into the live record, so read it each frame.

Open: the colour byte order is inferred (0x559720 swaps byte0 into red; then yellow "Lv."/name
and blue segments fit), not tested. The 0x536770 colour table order and the AL_ICON picture
mapping have not been checked. 0x565269 / 0x564CBD / 0x564CC7 patches are untested.

### Opening it without a pickup (src/chao_peek.rs)

- Open 0x5659E0: usercall EAX = Chao obj, saves EBX/ESI/EDI; reads `[[EAX+0x34]+0x5C]` (record)
  unchecked, so only call with Data1 and record set. Global 0x1AED28C = the new balloon after.
- Close 0x565B90: ECX = Chao obj, no stack args, uses only EAX. If `*0x1AED28C` ≠ 0 and its
  data `+0x38` == ECX (or ECX == 0: any) → state 7, global cleared. Never derefs ECX.
- "In front" (player action list 0x475AF0 → 0x486BB0(0x4000 − rotY)): target at
  `atan2(dx, dz)` (0x859B6A, ST1 = dx, ST0 = dz; ×−10430.38 at 0x905658) within ±0x2000 BAMS
  of 0x4000 − rotY, i.e. forward = (cos rotY, sin rotY) in (x, z), 45° half-cone.

## Controller buttons in the garden

REA, dumped exe d1ac8ed9…, 2026-10-19.

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

Chosen for the status-balloon peek: **D-pad up** (`held & 0x10` on the raw pad, our own edge),
only while the stick is inside ±0x27 so a keyboard's "forward" (which also sets 0x10, with the
stick at full tilt) doesn't fire it.
