# Main menu (CurrentMenu == 1) — RE notes

Binary: unpacked sonic2app.exe, sha256 c95c0143…0386, image base 0x400000. All static
analysis via REA/Ghidra; nothing here was tested at runtime yet.

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

`0x1DEFDB4`: compared against -1 in several places; inferred to be "player-2 pad index,
-1 = no second controller" (that is why 2P is refused with no 2nd pad). Not verified.

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

## Recommended block (Q1/Q2) — high confidence

Turn the conditional jump into an unconditional one, so A on "2P" always takes the
game's own locked path (error sound 0x800A, nothing else happens):

| addr | original | patched |
|---|---|---|
| 0x6650D9 | `0F 84` | `90 E9` |

The rel32 that follows (`BB 03 00 00`) stays valid: `nop; jmp rel32` at 0x6650DA ends at
0x6650DF, same as the original `jz`, so the target is still 0x66549A. Only 2 bytes change
(16-bit write within one cache line), and the stack is identical on both paths
(3 pushes already done, cleaned at 0x6654A4). Toggle by writing back `0F 84`.
Needs VirtualProtect(PAGE_EXECUTE_READWRITE) on .text; check the original bytes before
writing (another mod may have patched them). Safest to toggle from the game's main thread
(e.g. the mod's per-frame hook), but it is benign at any time.

Optional, cursor skips the 2P entry (reuses the "skip entry 5" logic):

| addr | original | patched | note |
|---|---|---|---|
| 0x664FC9 | `74 0E` | `66 90` | up: drop the `[0x1A556F8]` gate |
| 0x664FCE | `05` | `01` | up: skip index 1 instead of 5 |
| 0x665005 | `74 0E` | `66 90` | down: drop gate |
| 0x66500A | `05` | `01` | down: skip index 1 |

Side effect while patched: entry 5 is no longer skipped when `[0x1A556F8] != 0` (harmless,
its A-handler returns immediately then). When enabling, also write cursor 0x1D1BBE4 = 0 if
it is 1 (cursor survives menu re-entry). The 0x6650D9 patch alone is sufficient; the
cursor skip is cosmetic.

Other ways into Battle exist outside this list (SubMenu 7 "old 2P" panel with
[+0x5C]==1 → 2P kart; CurrentMenu 0x10 itself). As a belt-and-braces guard the mod can
also bounce `CurrentMenu == 0x10` back to 1 while online.

## Greying (Q3) — the native flag does nothing visible

Drawer FUN_006695C0 (main panel loop at 0x669600..0x6696CE) calls
FUN_006687A0(alpha, scale, highlight, disabled) with ECX = entry id (0..6),
EAX = &pos[i] (0x1A3D678 + 12*i), disabled = byte 0x1D1BBE5+i. In FUN_006687A0 the
disabled flag only selects a third sprite from table 0xC77820
(`{sel, unsel, disabled}` int triples, row = lang + id*2), and **every main-menu row has
disabled = -1**, so the flag falls back to the normal sprite. Forcing
`0x664ED2: 0F 94 C0 → B0 01 90` (mov al,1) would therefore change nothing on screen.
The only native "dim" is the alpha*0.5 path hard-coded for entry 5 (0x669646..0x66968C,
uses entry 5's pos/flag directly), not reusable without a code cave. To show 2P as
locked, draw an overlay from the mod.

## Entry label drawing (Q4)

Per entry, FUN_006687A0 (TextLanguage 0x174AFD1 < 2, i.e. Japanese/English):
1. label sprite: `FUN_00668730` with EDI = sprite index from 0xC77820 table
   (+0 when highlighted, +4 otherwise), ESI = &pos copy. FUN_00668730 builds an
   NJS_SPRITE {p = pos, sx = sy = scale, ang, tlist = 0xC6A240, tanim = 0xC6A248} and
   queues it via FUN_0066EF40(z, 0x22).
2. button plate: FUN_00668730 again with EDI = 0x1A/0x1B (0x1C/0x1D for ids 8..18),
   same x/y (0x668FC8..0x66900A).

For TextLanguage >= 2 (FR/ES/DE/IT?) the label is instead drawn from a text atlas by
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

**Hide only the 1P label (toggleable, data-only):** write `sx = sy = 0` (dword 0 at each
of 0xC6A248, 0xC6A2C0, 0xC6A338, 0xC6A3B0; original dword `FF 00 28 00` = 255|40<<16)
to get a zero-size sprite; restore `0x002800FF` to undo. The plate (sprites 0x1A/0x1B)
and the A-handler are untouched, so 1P still works. Medium confidence that a 0-size
sprite draws nothing (Ninja scales the quad by sx/sy); alternative is setting u2=u1/v2=v1.

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
label is 255x40 * 0.9 = **229.5 x 36** at anchor (320, 86). The texanim has cx = cy = 0, so
under Ninja rules the anchor is the sprite's top-left (label spans x 320..549.5,
y 86..122); the language>=2 text path centres on x instead. Verify the anchor with one
screenshot before relying on it.

## Addresses quick list

- FUN_00664EC0 main-menu update; FUN_006695C0 main-menu draw; FUN_006687A0 entry drawer;
  FUN_00668730 sprite draw; FUN_00669200 panel anim; FUN_00666790 init; FUN_00664B60 entry/setup.
- Cursor 0x1D1BBE4; P2 pad 0x1DEFDB4; sound fn 0x437260 (ESI = id).

## 2P → Kart Race menu (ROOM BATTLES)

The 1P panel's Kart Race handler is 0x66541F: unless `[0x1DEF428]` or `[0x1DEF4EC]` is set
(kart unlocked) it buzzes; otherwise 3 pushes, sound 0x8001 (0x665434), 0x664DA0 (main panel
out), 0x671B80 (header out), `[*0xC77F34+0x88] = 0` (1P), SubMenu = 5. SubMenu 5 (0x6654AE)
waits for both panels to be idle, then switches on the 1P panel cursor `[0x1D1BC00]`; case 2
(0x665516) opens the kart menu (CurrentMenu 10).

The mod now patches 0x6650D9 to `90 E9 C5 03 00 00` (jmp 0x6654A4: clean the 3 pushes, return
0, no sound), holds the main list with SubMenu 0x1C (past the table, the update does nothing)
while its picker shows, then does the handler's steps itself (+0x40 = entry, 0x664DA0,
0x671B80, kart +0x88 = 0 or stage panel +0x14 = 0, SubMenu 5). Stage Select is panel entry 1
(case 0x6654EB: 0x672640, CurrentMenu 3).

Patching 0x6650D9 to `90 E9 55 03 00 00` (jmp 0x665434, past the unlock check) sends the 2P entry there, with the
same stack (the 2P handler already pushed 3 zeros, EBX = 0); the 1P panel cursor must be 2.
Calling 0x672B10 / CurrentMenu = 10 directly from SubMenu 2 crashes at 0x77D0F1 (sprite draw
with a null animation, from the kart menu's display 0x670830): the panels never slid out.
