# Main menu (CurrentMenu == 1)

Image base 0x400000.

## Data

`MainMenuData` pointer at 0xC77F14 holds **0x1D1BBC0** (struct size 0xBC, memset in
init FUN_00666790). Relevant fields (absolute address in brackets):

| off | abs | meaning |
|---|---|---|
| +0x08 | 0x1D1BBC8 | "transition/animation busy" (OR of all panels' busy flags, FUN_00669200) |
| +0x10 | 0x1D1BBD0 | label sprite set: 0 = Japanese, 1 = other (`TextLanguage 0x174AFD1 > 0`) |
| +0x14 | 0x1D1BBD4 | main panel visible (slide in/out target) |
| +0x18 | 0x1D1BBD8 | highlight enabled (multiplied with "cursor == i") |
| +0x24 | **0x1D1BBE4** | **main-menu cursor (signed byte, 0..6)** |
| +0x25+i | 0x1D1BBE5+i | per-entry "disabled" byte passed to the drawer (see below) |
| +0x26 | 0x1D1BBE6 | entry 1 disabled byte = `(dword[0x1DEFDB4] == -1)`, rewritten every frame |
| +0x40 | 0x1D1BC00 | cursor of the 1P sub-panel (4 entries) |

Entry indices (from the A-press dispatch, jump table 0x666154):

| i | action on A | meaning |
|---|---|---|
| 0 | → sub 3/4 (1P panel: story / stage select / kart / …) or straight to StorySelect if <2 modes unlocked | **1P / single-player** |
| 1 | if `[0x1DEFDB4] != -1`: SubMenu=0xE → next frame CurrentMenu=0x10 (Battle); else "error" sound only | **2P / Battle** |
| 2 | FUN_00411ae0()==0 → SubMenu 0x17 | (unidentified) |
| 3 | FUN_00411ae0 / FUN_0040d300 | (unidentified) |
| 4 | SubMenu 0xF → 0x10 (6-entry options panel) | Options |
| 5 | SteamFriends()->vtbl[0x60](0x3532C) (overlay/store); skipped by cursor when `[0x1A556F8] != 0` | Steam/DLC |
| 6 | SubMenu 0x11 (quit confirm) → 0x12 PostQuitMessage | Exit |

`0x1DEFDB4`: compared against -1 in several places; most likely the player-2 pad index, -1 =
no second controller (2P is refused without a second pad).

## Update function: FUN_00664EC0 (switch on CurrentSubMenu 0x1D7BB14)

- 0x664EC0..0x664ED5: `MainMenuData[0x26] = ([0x1DEFDB4] == -1)` every frame (bytes `0F 94 C0` = SETZ AL at 0x664ED2).
- SubMenu 2 = main list. Buttons from MenuButtons_Pressed 0x1DEFAB0: 0x10 up, 0x20 down, 0x0C A/Start, 0x02 B.
  - Up (0x664FB4): `dec cursor; if ([0x1A556F8] && cursor==5) dec again; clamp >=0`.
  - Down (0x664FF0): `inc; same skip of 5; clamp <=6`.
  - A (0x665032): `jmp [cursor*4 + 0x666154]`.
- Entry 1 (2P) handler at 0x6650CE:
  ```
  0x6650CE 33 DB              xor ebx,ebx
  0x6650D0 53 53 53           push ebx x3            ; args for PlaySound(0,0,0)
  0x6650D3 39 2D B4 FD DE 01  cmp [0x1DEFDB4],ebp    ; ebp = -1
  0x6650D9 0F 84 BB 03 00 00  jz  0x66549A           ; no P2 pad -> "can't select"
  0x6650DF BE 01 80 00 00     mov esi,0x8001         ; OK sound
  0x6650E4 E8 ...             call 0x437260          ; sound, id in ESI
  ...                         call 0x664DA0; call 0x671B80; SubMenu = 0xE
  0x66549A BE 0A 80 00 00     mov esi,0x800A         ; "can't select" sound
  0x66549F E8 ...             call 0x437260 ; add esp,0xC ; return 0
  ```
  FUN_00437260 = play menu sound, sound id in ESI (0x8001 confirm, 0x800A error/locked,
  0x8000 / 0x8009 also used). SubMenu 0xE (next frame, when +0x08 == 0) calls
  FUN_00669F40 and sets CurrentMenu = 0x10.

## Taking over the 2P entry (`game::block_two_player_menu`)

While online, the `jz` at 0x6650D9 becomes an unconditional jump, so A on "2P" never opens the
game's Battle menu:

| addr | original | patched |
|---|---|---|
| 0x6650D9 | `0F 84 BB 03 00 00` | `90 E9 C5 03 00 00` |

`nop; jmp rel32` ends at 0x6650DF like the original `jz`; the target 0x6654A4 cleans the 3
pushed arguments and returns 0 with no sound. The stack is the same on both paths. Offline the
original bytes are restored. The mod's own picker (`battles.rs`) opens instead.

Other facts about this code:
- Patching only the opcode (`0F 84` → `90 E9`, keeping rel32 `BB 03 00 00`) sends 2P to the
  game's own "can't select" path at 0x66549A (error sound 0x800A).
- Patching to `90 E9 55 03 00 00` (jmp 0x665434, past the kart unlock check) sends the 2P entry
  into the 1P panel's Kart Race handler with the same stack (the 2P handler already pushed 3
  zeros, EBX = 0); the 1P panel cursor must be 2.
- The cursor's "skip entry 5" logic can skip index 1 instead: 0x664FC9 / 0x665005 `74 0E` →
  `66 90` (drop the `[0x1A556F8]` gate) and 0x664FCE / 0x66500A `05` → `01`. The cursor survives
  menu re-entry.
- Other ways into Battle exist outside this list (SubMenu 7 "old 2P" panel with [+0x5C]==1 →
  2P kart; CurrentMenu 0x10 itself).

## Greying: the native flag does nothing visible

Drawer FUN_006695C0 (main panel loop at 0x669600..0x6696CE) calls
FUN_006687A0(alpha, scale, highlight, disabled) with ECX = entry id (0..6),
EAX = &pos[i] (0x1A3D678 + 12*i), disabled = byte 0x1D1BBE5+i. In FUN_006687A0 the
disabled flag only selects a third sprite from table 0xC77820
(`{sel, unsel, disabled}` int triples, row = lang + id*2), and **every main-menu row has
disabled = -1**, so the flag falls back to the normal sprite. Forcing
`0x664ED2: 0F 94 C0 → B0 01 90` (mov al,1) would therefore change nothing on screen.
The only native "dim" is the alpha*0.5 path hard-coded for entry 5 (0x669646..0x66968C,
uses entry 5's pos/flag directly), not reusable without a code cave. The mod draws its own
overlay on the entry instead.

## Entry label drawing

Per entry, FUN_006687A0 (TextLanguage 0x174AFD1 < 2, i.e. Japanese/English):
1. label sprite: `FUN_00668730` with EDI = sprite index from 0xC77820 table
   (+0 when highlighted, +4 otherwise), ESI = &pos copy. FUN_00668730 builds an
   NJS_SPRITE {p = pos, sx = sy = scale, ang, tlist = 0xC6A240, tanim = 0xC6A248} and
   queues it via FUN_0066EF40(z, 0x22).
2. button plate: FUN_00668730 again with EDI = 0x1A/0x1B (0x1C/0x1D for ids 8..18),
   same x/y (0x668FC8..0x66900A).

For TextLanguage >= 2 (the other languages) the label is instead drawn from a text atlas by
FUN_0066F160 at (x-159.56, y-12.75, w 325.125, h 25.5) with v = table 0xC77728[id]; the
texanim trick below does not apply there.

Sprite rows for entry 0 (1P): Japanese {sel 0x0C, unsel 0x12}, other {sel 0x00, unsel 0x06}.
These four sprite indices are referenced by no other row; the texanim array 0xC6A248 is
only referenced by FUN_00668730. NJS_TEXANIM entries (20 bytes: sx, sy, cx, cy, u1, v1,
u2, v2, texid, attr):

| sprite | addr | value |
|---|---|---|
| 0x00 | 0xC6A248 | 255,40, 0,0, 0,0,255,40, tex0 |
| 0x06 | 0xC6A2C0 | 255,40, 0,0, 0,0,255,40, tex1 |
| 0x0C | 0xC6A338 | 255,40, 0,0, 0,0,255,40, tex2 |
| 0x12 | 0xC6A3B0 | 255,40, 0,0, 0,0,255,40, tex3 |

The original first dword of each is `FF 00 28 00` (255 | 40<<16); Ninja scales the quad by
sx/sy, so `sx = sy = 0` gives a zero-size label while the plate (sprites 0x1A/0x1B) and the
A-handler are untouched.

**Position/size (640x480 menu space):** final position table at 0xC77B18
(*0xC77C98, count 7, 16 bytes {x, y, z, angle}):

| i | x | y |
|---|---|---|
| 0 (1P) | 320.0 | 86.0 |
| 1 (2P) | 320.0 | 131.0 |
| 2 | 320.0 | 176.0 |
| 3 | 320.0 | 221.0 |
| 4 | 320.0 | 266.0 |
| 5 | 320.0 | 311.0 |
| 6 | 320.0 | 356.0 |

During slide-in the live position is lerped each frame by FUN_0068D190 and stored at
**0x1A3D678 + 12*i** (float x, y, z): read entry 0 at 0x1A3D678/0x1A3D67C each frame to follow
the animation. Anim counter dword 0x1A3D670 (0..180); t = counter/180. Label alpha = t
(t*0.5 while quit-confirm `[0x1AEE2F0]` is up), label scale = 0.9*t, so fully shown the 1P
label is 255x40 * 0.9 = **229.5 x 36** at anchor (320, 86). The texanim has cx = cy = 0; the
language>=2 text path centres on x. `game::main_menu_1p_label` / `main_menu_2p_label` read the
live positions for the mod's overlays.

## Addresses quick list

- FUN_00664EC0 main-menu update; FUN_006695C0 main-menu draw; FUN_006687A0 entry drawer;
  FUN_00668730 sprite draw; FUN_00669200 panel anim; FUN_00666790 init; FUN_00664B60 entry/setup.
- Cursor 0x1D1BBE4; P2 pad 0x1DEFDB4; sound fn 0x437260 (ESI = id).

## Leaving the main menu for a 1P panel entry (`game::leave_main_menu`)

The 1P panel's Kart Race handler is 0x66541F: unless `[0x1DEF428]` or `[0x1DEF4EC]` is set
(kart unlocked) it buzzes; otherwise 3 pushes, sound 0x8001 (0x665434), 0x664DA0 (main panel
out), 0x671B80 (header out), `[*0xC77F34+0x88] = 0` (1P), SubMenu = 5. SubMenu 5 (0x6654AE)
waits for both panels to be idle, then switches on the 1P panel cursor `[0x1D1BC00]`; case 2
(0x665516) opens the kart menu (CurrentMenu 10).

Stage Select is panel entry 1 (case 0x6654EB: 0x672640, CurrentMenu 3).

The mod holds the main list with SubMenu 0x1C (past the jump table, so the update does nothing)
while its picker shows, then does the handler's steps itself: +0x40 = entry, 0x664DA0,
0x671B80, kart +0x88 = 0 or stage panel (`*0xC77F44`) +0x14 = 0, SubMenu 5. No unlock checks.

Calling 0x672B10 / CurrentMenu = 10 directly from SubMenu 2 crashes at 0x77D0F1 (a sprite draw
with a null animation, from the kart menu's display 0x670830), because the panels never slid
out.

## Picking a stage for the player (`room::go`)

- Stage Select's update (0x671FC0) switches on CurrentSubMenu through the table at 0x6725FC
  (0..10). Sub-menu 2 (0x672065) is the map being browsed; its confirm (0x6721C0) calls
  0x686E00, which sets CurrentLevel, CurrentCharacter and MissionNum from the map entry under
  the cursor (page 0x1D1BF54, index 0x1D1BF8C, entries in the table at 0xC711F8 built while the
  game runs) and returns the entry's kind (jump table 0x672628).
- For a stage the confirm plays sound 0x8001, calls **0x673AE0**, then writes MenuReturn
  0x1A559E8 = 6, `[*0xC77F48]` = `[*0xC77F48+4]` = 0, `[*0xC77F44]` = 0, 0x1AEE2FC = 0 and
  CurrentSubMenu = 5.
- **0x673AE0** saves the menus' working state (the panels behind 0xC77F00..0xC77F1C) into
  0x1A559EC onwards; returning to the menus after the stage restores Stage Select from that
  copy.
- Sub-menu 5 (0x672438) waits for the panel (`[*0xC77F48+8]` == 0), calls 0x674980, which
  sets the menu resources for CurrentMenu 12 and releases the rest (the resource table at
  0xC77D10, 12 bytes an entry: wanted flag, load, release), and moves to CurrentMenu 12.
  Sub-menus 9 and 10 (0x67250E, 0x67259C) load Chao World (CurrentLevel 0x5A).
- The map's display object (created by 0x6779A0 from the menu setup 0x66661F; display
  0x6777C0, delete 0x677990) draws through 0x675DF0 with the model at `*0x1AEE750`, which only
  the resource entry at 0xC77D28 (load 0x677AE0, release 0x677B60) sets. Returning to Stage
  Select without the saved state from 0x673AE0 brings the map up with that model released,
  and 0x41DC80 writes through the null pointer (0x41DD16).
- So the mod picks only in sub-menu 2, calls 0x673AE0 first, then writes what the confirm
  writes with the chosen level, mission and character.
