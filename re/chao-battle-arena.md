# Chao Karate arena: menu, Chao, motions, camera, input and HUD for a scripted battle

Steam `sonic2app.exe`, image base 0x400000. Read `chao-karate-sync.md` (notation `g`, `M`,
`MD`, `FD`, `W`) and `chao-stadium.md` first. Addresses come from the objdump listing of the
dumped exe.

The karate area is `CurrentLevel` 90 (Chao World) with `CurrentChaoArea` (0x134046C) == 10.
`g` (0x1A5D148) and `M` (0x1A5D14C) are set by the area's creator (0x541570) and cleared by the
main object's delete sub (0x5416A0) when the area is torn down.

---

## 1. The mode select (state 3, 0x5417E0)

### Sub-states
- **sub 0**: when `g+3 == 0`, creates the menu window object (MainSub 0x5722E0, DisplaySub
  0x572450), calls 0x572D00 and 0x573DD0 (EAX = M), sets the cursor `g+0xA = 0`, then sub 1.
- **sub 1**: input, only when `g+3 == 0`. Every other sub-state returns at once (0x5417FB), so
  **writing `MD[1] = 2` holds the mode select**: the window stays on screen and no button is read.
  Writing `MD[1] = 1` again resumes it.

### Buttons read in sub 1
All from the menu buttons `0x1DEFAB0` (player 1) and `0x1DEFAB4` (player 2, only when the dword
0x1AEE5AC is non-zero). See section 6 for how they are built.

| bit | effect |
|---|---|
| 0x10 up | cursor -1 or 1 → 0. Window command 1, sound 0x8000 |
| 0x20 down | cursor 0 or 1 → -1 (0xFF). Window command 4, sound 0x8000 |
| 0x04 A | cursor -1: state 0xF (leave Karate). Otherwise `g+5 = cursor`, state 4 (cursor 0, 1P tournament) or 6 (cursor 1, 2P), `g+2 = 1`, the window is closed (below), sound 0x100E |
| 0x02 B | cursor -1: state 0xF. Otherwise cursor = -1 and window command 4 |

In Chao World the cursor only takes the values 0 (tournament) and -1 (exit): up and down never
produce 1. Cursor 1 is the 2P choice, reached in the 2P battle area (11) where state 2 goes
straight to state 6.

### The window object
- `*(g+0x3C)`; its Data1 (`wd`):
  - `wd[8]` highlight / closing flag,
  - `wd[9]`, `wd[0xA]` (also the queue length, up to 4),
  - `(u16)wd[0xC..0x12]` a queue of 4 animation commands. **0x573EC0** (CX = command) appends
    one: 1 = highlight the first button, 4 = highlight exit.
- Closing (what states 4/5/6 and the A handler do):
  `wd[8]=0xFF; wd[9]=1; wd[0xA]=1; (u16)wd[0xC]=wd[0xE]=wd[0x10]=wd[0x12]=0xFFFF`.
- The display 0x572450 draws sprites with 0x77D0B0 from the tables around 0x1297D20..0x1297D82.
  Pixel positions of the two buttons are not decoded here.

### Other menu objects and how they go away
| object | created in | goes away |
|---|---|---|
| background, `g+0x30` (MainSub 0x5728E0) | state 2 | state 8 sets its Data1[0] = 4 and clears `g+0x30` |
| banner (MainSub 0x571E60) | state 2 | slides in holding `g+3 = 1`; on the event `g+2 == 3` (state 7) waits 20 frames, slides out, deletes itself (0x571F9B) |
| 0x572810 | state 2 | deletes itself (0x5728B8) |
| window (0x5722E0) | state 3 sub 0 | closed by the write above |

### Leaving the menu the game's way
State 7 with `g+5 == 1` does not wait for A: it sets `g+2 = 3` (the banner leaves) and goes to
state 8. State 8:
- **sub 1**: closes the background, then sets a 2-frame timer.
- **sub 2**: when the timer is done and `g+3 == 0`:
  - `g+7 = 0`, `*(u32*)(*0x1A55998 + 4) = 3`, `0x1A5CC24 = 0`,
  - `MD[0] = 9, MD[1] = 0`.

So from the mode select (`MD[0]==3, MD[1]==1, g+3==0`):

`g+5 = 1`, close the window, `MD[0] = 7, MD[1] = 0`

tears the whole menu down and arrives at state 9 sub 0 a few frames later. This is the path
live karate uses.

### State 1 and back to the menu
- **State 1** has no handler (jump table entry 1 = 0x542EC4): KarateMainExec only calls 0x47E750
  and clears `g+3`.
- **State 2** (0x5416E0) creates the background, banner and 0x572810, clears `MD+4 & 0x800`
  (outside area 11), writes `*(u32*)(*0x1A55998 + 4) = 2`, calls 0x442CF0 with EDI = 0x1349B90,
  then goes to state 3 (sub 0 makes a new window with the cursor on 0).
- State 13 returns to state 2 the same way after a fight.

---

## 2. What reads the fighters

State 9 sub 0 spawns the fighters (0x578EB0) into `g+0x10/0x14`. These read them without a null
check:

| reader | when | reads |
|---|---|---|
| camera state 1 (0x57452F) | `MD[0] == 9` → camera state 2 (170-frame pan), then state 3 | – |
| camera state 3 (0x574618) | intro close-ups | **0x579950** reads `g+0x10`/`g+0x14` → `+0x40` (fighter work) and plays their intro motions and voices |
| camera state 7 (0x57498A) | fight camera | `g+0x10/0x14` Data1 positions (0x543180 midpoint, 0x543070 separation) |
| state 13 sub 2 (0x542CDA) | after the fade | `*(g+0x10)+0x10 = 0x46F720` and the same for `g+0x14`, unconditionally |
| state 12 (0x542A1F) | result | 0x5795E0 on both fighters, 0x542760 on both records (below) |

### State 12 and records
- In VS (`g+5 != 0`), 0x542760 adds 1 (capped at 9999) to these fields of **both** `g+0x18` and
  `g+0x1C` records:
  - `+0x10E` fights,
  - `+0x110` wins / `+0x112` losses / `+0x114` draws, by `g+0xB`.
- In 1P it updates `g+0x18` only, plus `+0x10A..+0x10C`.

### Fighter Chao
The fighter's Chao is created as `CreateChao(rec, 1, NULL, &pos, angle)` (mode 1, 0x578FE1). The
`1` makes it a private copy (section 3). The creator then clears `D+0xA0` bits 0x8, 0x2, 0x10
and 0x20000 (0x579504..0x579522).

### Karate's own HUD (state 9 sub 1, at timer 300)
- 0x56FCE0, and the name plates **0x5719B0** (EAX = M, ECX = Chao object, BL = side):
  - an object (MainSub 0x571910, display 0x571940) keeping the **Chao object at Data1+8**,
  - it draws the name from that Chao's `D+0x5C` record (+0x12) every frame with **0x536BE0**,
  - it deletes itself on `g+2 == 7`. With an empty name no plate is made.
- HP bars **0x5703A0** (ESI = &HP float, BL = side): Data1+0x10 = the pointer, Data1+0x28 = the
  value at creation (the full bar), Data1+0x20 = the current value. Deleted on `g+2 == 7`
  (0x570436).
- Morale gauges 0x5707B0 (ESI = &morale), the clock 0x575140 (`g+0x34`, writes `g+0xC = 90`),
  object 0x570810.
- 1P only: 0x56FFA0.
- Music volume through `*0x1A55998` (+8) and 0x439490; 0x442CF0 / 0x442D90 with EDI = 0x1349BA0.

---

## 3. Chao made with CreateChao in the arena

`CreateChao` 0x5501D0, cdecl, 5 arguments: `(ChaoData* rec, int copy, void* cpu_template,
Vector* pos, int angle)`, returns the ObjectMaster.

- `rec` type `+0x80`:
  - 1 (egg) makes an egg object instead (0x57B9C0),
  - 0 is turned into a child **in the passed record** before anything else (0x55021A).
  - Records passed must be type 2 or higher.
- **`copy != 0`** (0x5503C4..0x55041E):
  - the game allocates 0x4E0 bytes, copies the first **0x4DC** bytes of `rec` into it,
  - sets `D+0x5C` = the copy and `D+0x58 = 1`,
  - registers the Chao in the world list (0x530750) **with a null record pointer**.
  - The passed record is never kept or written, so the caller's buffer can be freed after the
    call.
  - The DeleteSub 0x54FF30 frees the copy.
  - With `copy == 0`, `D+0x5C` is `rec` itself (what `game::create_chao` and visitors.rs pass).
- Area 10 puts the Chao in object list 4 (index table 0x550600). The Chao registry holds 32 Chao
  per area.
- Flags as in `chao-garden-sync.md` ("How the game itself makes a display-only Chao"). Karate
  clears `D+0xA0 &= ~(0x8 | 0x2 | 0x10 | 0x20000)` on its fighters' Chao:
  - 0x10 off: no AI tick (no behaviours, reactions, life events),
  - 0x8 off: no collision,
  - 0x4 stays on: the motion keeps playing.
- The MainSub 0x54FE20 still runs physics (0x53DF00, ground mode 0x798300), so the Chao stands
  on the arena floor.
- Position `D+0x14` (vec3), rotation `D+8/+0xC/+0x10` (BAMS ints).
- Kept in place like a fighter's Chao: wrap the MainSub and write position and rotation before
  and after calling 0x54FE20, and zero the velocity at Data2 (+0x38) +0/+4/+8. This is
  `garden.rs` `update_at`.
- Removal: `obj+0x10 = 0x46F720` (DeleteObject); the object goes on the next object pass.
- Arena geometry:
  - fighter spawns (-5, 8, 19) facing 0x4000 and (+5, 8, 19) facing 0xC000;
  - result points (±7, 8, 19);
  - a fighter is out of the ring when its y < 7.0;
  - the ring is symmetric about x = 0.
- Face: eyes 0x53A4B0 and mouth 0x53A5A0, as in garden.rs.
- Emotion-ball icon **0x53D660**: ECX = Chao object, DL = icon, push frames, caller pops 4.
  Icons seen in the AI code: 1 "!", 3 "?", 4 heart, 5 swirl. It plays its sound when the area
  is the active one (`0x134046C == 0x134062C`).

---

## 4. Motions

### Karate motion controller
- `W+0xC` is a 0x28-byte block made by 0x77DF40:
  - `[0]` karate motion id, `[1]` sub, `[3]` eyes, `[4]` mouth,
  - `[5]` event bits (0x08 / 0x10, the hit frames),
  - `+0xC` table index, `+0x14` frame,
  - `+0x24` = the Chao's `D+0xAC`, set by **0x575A30** (EAX = fighter).
- **0x575A00** (EAX = fighter object, push id, caller pops 4): `FD+4 &= 0x7FFF`,
  `[5] &= 0xE7`, `[0] = id`, `[1] = [2] = 0`.
- The motion player 0x5777A0 (ids 0..0x2D, index table 0x5778A0, jump table 0x577854). On sub 0
  it reads the entry at **0x12971C0 + id·0x2C** and calls
  `0x793D30(EAX = Chao D+0xAC, EDX = entry+4, push (u16)entry+0)`, then sets eyes/mouth from
  `entry+0xC`/`+0x10`. -1 there means the Chao's own (`D+0x694` / `D+0x69E`).
- Entry layout (0x35 entries, 0..0x34):

| off | meaning |
|---|---|
| +0 | u16 blend frames |
| +4 | Chao motion id (global `al_motion_table` id) |
| +0xC / +0x10 | eyes / mouth (-1 = keep) |
| +0x14 | 1.0 |
| +0x18..+0x28 | up to 5 event frames (-1 ends the list); the attack handlers raise the `[5]` event bits there |

- Motion id 0x2D plays the entry whose index is already in `+0xC`, without changing the face
  (0x575D80). That is how entries above 0x2D are used.

### Ids and where karate uses them
The same Chao motion ids play on any Chao in the area with
`0x793D30(EAX = D+0xAC, EDX = id, push blend)`, cdecl pop 4 (as 0x575B3F does), or 0x793C40
(no blend argument). `garden.rs` `motion_ok` checks an id against the table first.

| karate id | Chao motion | blend | used for (setter) |
|---|---|---|---|
| 0, 1, 2 | 0x255, 0x241, 0x240 | 25 | fighting stance, picked by luck `W+0x40` (< 300 → 0; ≥ 500 → 2, else 1) (0x578089, 0x57955B) |
| 0x24 | 0x25E | 25 | stance of a weak child (`W+4 & 1`) |
| 3 / 0x25 | 0x242 / 0x25D | 20 / 10 | attack wind-up / weak child (0x578293) |
| 4, 5 | 0x243, 0x244 | 15 | attack, first kind (power `W+0x2C` < 1600 → 4, else 5); hit frames 4, 6 / 0, 6.3, 9 (0x578331) |
| 7, 8 | 0x246, 0x247 | 15 | attack, second kind (< 1600 → 7, else 8); hit frames 2.5, 5.5 / 0, 10.5, 14 |
| 6 | 0x245 | 15 | attack (handler 0x575E30, voice 0x606F) |
| 9, 0xA, 0xB | 0x249, 0x24A, 0x24B | 1 | hit taken: damage < 300 / 300..700 / ≥ 700 (0x5786D1..0x578757) |
| 0x1D, 0x1E, 0x1F | 0x256, 0x257, 0x258 | 1 | the same while morale is broken (`FD+4 & 0x800`) or airborne (`W+4 & 2`) |
| 0xC / 0x26 | 0x24C / 0x25F | 1 / 10 | knocked down, 120 frames (action 10, 0x578B61) / airborne |
| 0x15..0x1C | 0x253, 0x23D, 0x187, 0x181, 0x137, 0x26D, 0x18D, 0x254 | 30..40 | guard/taunt variety, picked by `rand` (action 6, 0x5789F4..0x578AAC), each with a voice |
| 0x14 | 0x241 | 10 | guard stance (0x578ABF) |
| 0xD, 0xE / 0xF | 0x24D, 0x24E / 0x26C | 1 | result: won (random of two) / won, weak child (0x5797C4..0x5797FA) |
| 0x10, 0x11 | 0x24F, 0x250 | 20 | result: lost (random) (0x5798CE) |
| 0x2C | 0x1E0 | 10 | result: draw (0x579696) |
| 0x29 / 0x2A | 0x264 / 0x265 | 5 | end-of-fight pose / weak child (action 11) |
| 0x27, 0x28 | 0x260, 0x261 | 10 | clear / set the airborne flag `W+4 & 2` at the motion end (0x575C60) |
| entries 0x2B..0x34 | 0x204, 0x263, 0x190, 0x269, 0x26B, 0x204, 0x266, 0x267 | | intro and result poses of the CPU fighters (played through id 0x2D) |

### Sounds the fighters play
Sounds are played with these functions:
- **0x437260**: ESI = id, push 0, 0, 0; caller pops 0xC.
- **0x4372E0**: EDI = id, ESI = &position, 3 cdecl arguments (garden.rs).
- **0x437C70**: ESI = id, 2 cdecl arguments, the last a length in frames.

| id | where |
|---|---|
| 0 / 2 | the hit (0x5787C8): 0 when the attacker's motion is 4 or 5, else 2 |
| 0x6070 / 0x6071 / 0x606F | attack voices of motions 4,7 / 5,8 / 6 |
| 0x6008, 0x602F, 0x603C, ... | taunt voices (0x437C70, 0xA0 frames, action 6) |
| 0x6082, 0x6015, 0x6005, 0x6031/0x6069, 0x6091 | other motion handlers (0x576AA7, 0x576CA7, 0x5769A8, 0x5772A2, 0x577741) |
| 0x6067 | loser (0x579841) |
| 0x608D, 0x6036, 0x6038, 0x6079, 0x6035, 0x6089 | CPU fighters' result and intro voices |
| 0x4007..0x4012 | crowd cheers, through 0x543360 (an object that plays them after a delay) |
| 0x8000 / 0x100E | menu cursor / menu confirm |

The fighter code creates no hit-spark object. A hit is:
- the reaction motion,
- the hit sound,
- a knock-back in the reaction handler (0x576400),
- in 1P/2P, a morale drain.

---

## 5. Camera

The camera object is `g+0x38`:
- created by 0x574FE0 in the area creator, after M, in the same object list (2),
- MainSub **0x574480**, 12 states, jump table 0x574FB0, state in Data1[0],
- work `C` = obj+0x40 (0x50 bytes).

| `C` off | meaning |
|---|---|
| +0 | u16 camera slot (copy of 0x1DD0194) |
| **+8** | camera position, vec3 |
| +0x14 | position the intro/fight states move toward |
| **+0x20** | look-at point, vec3 |
| +0x2C | look-at the states move toward |

- State 0 registers the callback **0x5743A0** with 0x574420. Every frame the callback copies
  `C+8` to the camera position (0x1DCFF0C) and `C+0x20` to the look-at (0x1DCFF30). So
  **writing `C+8` and `C+0x20` moves the view** in any state that leaves them alone.
- State 0 also sets position (0, 20, 127) and look-at (0, 11.5, 0) (0x1297BF0/0x1297BFC, the
  menu view).

States:
- **1**: does nothing until `MD[0] == 9`; then state 2 (170-frame move).
- **3**: intro close-ups through the event camera 0x4EBCD0 and 0x579950 (fighters).
- **4**: waits for `MD[0] == 0xB`, then moves to position (0, 13, 36) / look-at (0, 11, 19)
  (0x1297C38 / 0x1297C44).
- **7**: follows the two fighters.
- On `g+2 == 7`: 0x574460 releases the callback and the camera goes back to state 0.
- 0x1297C50 = (0, 13, 37) is the default fight position.

M is loaded before the camera object, so a change to `MD[0]` made inside M's MainSub is what the
camera reads in the same frame. A change made after the object pass is read one frame later.

---

## 6. Buttons

### Pads (`chao-details.md` "Controller buttons in the garden")
- Raw pad per player: `*(0x1DEFB60 + 4·i)`, 0x34 bytes. +0x08 held, +0x10 pressed this frame,
  +0x1C/+0x1E stick x/y (i16).
- The keyboard path (0x425650) writes the same pad: the keyboard's movement keys set the D-pad
  bits and the stick.
- **0x441AB0** (ECX = player) builds the menu buttons from the raw pad, **not gated by
  ControllerEnabled**:
  - `0x1DEFAB0 + 4·i` pressed this frame: raw pressed | stick directions newly past ±0x27 as
    0x10 up / 0x20 down / 0x40 left / 0x80 right;
  - direction bits repeat after 30 frames held, then every 6 frames (counter 0x1934BF0 + 4·i);
  - `0x1DEFB10 + 4·i` held.
  - Gated copies for gameplay: `0x1DEFB50` (pressed) and `0x1DEFBE0` (held), zeroed while
    0x174AFFE is 0.
- Bits: 2 B, 4 A, 8 Start, 0x10/0x20/0x40/0x80 up/down/left/right, 0x200 Y, 0x400 X,
  0x10000 L, 0x20000 R. `game::menu_buttons_pressed` reads `0x1DEFAB0`.

### Readers in the karate area
- The mode select (state 3 sub 1): up, down, A, B from 0x1DEFAB0/0x1DEFAB4.
- Fighters' cheering (0x577A40):
  - 1P: player 1's raw pad A;
  - 2P: the per-side buttons `0x1DBED40 + 12·side` (+4 pressed, bit 4 = A), filled from both
    pads by 0x537480.
  - Skipped in fighter actions 1, 10 and 11.
- Start opens the pause menu.

With the karate main object in state 1 and no fighters, nothing in the area reads 0x1DEFAB0.

---

## 7. HUD drawing (src/hud.rs)

hud.rs draws through the Mod Loader's debug text and a rectangle queue:
- **Text**: `display_debug_string`, ASCII only, on a grid of font-size cells. A position snaps to
  a multiple of the font size. One call per line; drawn at the end of the frame over everything.
- **Rectangles**: up to 256 per frame, filled with D3D9 `Clear` just before the debug text
  (hook at 0x433FF7). They are opaque (no alpha) and drawn under all text.
- Helpers:
  - `text(x, y, size, colour, line)`;
  - `panel(x, y, size, cols, rows)` (dark panel with border);
  - `bar(x, y, w, h, fraction, colour)` (track 0xFF303A48) and `rect(x1, y1, x2, y2, colour)`;
  - `menu_label(cx, cy, w, h, text, selected)` (gold/dark button in 640x480 space);
  - `virtual_screen`, `from_virtual` (640x480 → back buffer);
  - `menu_font`, `center`, `center2`, `banner`, `corner`, `progress`, `toast`, `note`.
- Screen size is the floats 0x174BA64 / 0x174BA68.
- Points in the world reach the screen through `puppet::TagProjector` during a display pass
  (`calc_point` with the view matrix at the top of the stack, `project_screen` 0x77FB00).
- The game's Chao-name text (game font) is **0x536BE0**, called from the name plate display:
  - ECX = -1 or 1;
  - push name bytes (record +0x12), x, y, scale x, scale y (floats), colour 0xFFFFFFFF, 0;
  - caller pops 0x1C.

---

## 8. Pause, focus and area changes

- **Pause** byte **0x174AFF7**: the karate main object, fighters, HUD objects and camera return
  at once from their MainSubs while it is set. The Mod Loader's OnFrame does not run while the
  game is paused (hud.rs).
- **Focus**: windowed, a game that runs unfocused (`game::run_unfocused`) turns
  ControllerEnabled off, but 0x441AB0 still fills 0x1DEFAB0 from the pad. `input::game_focused`
  tells whether the window has focus. Fullscreen loses its device and the game waits.
- **Leaving the karate area**:
  - state 0xF (`0x52B5B0(1)`), the pause menu, or any area change deletes every object in the
    area: the karate main object, camera, HUD and Chao, including Chao made with CreateChao,
    whose DeleteSub frees their record copies;
  - 0x5416A0 clears `g` and `M`.
  - `karate_live::karate()` returns None once the area is no longer 10 or `g`/`M` are null.
- **Leftovers**: anything the area does not own survives an area change:
  - MainSub wrappers point at code in our DLL, but the wrapped objects are deleted with the area;
  - byte patches (`chao-karate-sync.md` end checks) stay until removed;
  - leaked record buffers stay allocated.
