# Battle visuals: Chao motions, emotion balls, effects and sounds

Addresses are in the Steam `sonic2app.exe` (image base 0x400000). The arena, its Chao and the
camera are in `chao-battle-arena.md`; behaviours and their motions in `chao-ai.md`. The
effect sprites are the mod's own (`battle::effects_gfx`), chosen from each move's catalog
record.

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

### The table offline, and more motions for moves

- The table is the export `al_motion_table` of `resource/gd_PC/DLL/Win32/Data_DLL_orig.dll`
  (the 31 MB file; `Data_DLL.dll` beside it is the mod loader's proxy): RVA 0x32B7C8, image
  base 0x10000000, 622 entries of 0x1C bytes (ids 0..0x26D; 0x26E has a null motion).
- Every entry passes `motion_ok`: motions lie in 0x10259430..0x1032B7B8, modes are 0..7.
- The clips are `NJS_MOTION` type 3 (position + angle), 40 nodes; node 1 is the body. Position
  keys are `{u32 frame, f32 x, y, z}`, angle keys `{u16 frame, s16 x, y, z}`. Standing body y is
  1.0, sitting 0.55; a negative x angle leans forward. The "moment" below is read from the body
  keys (its lunge, jump or lean); arms and hands weren't measured.
- Ids to keep off the arena Chao: 0x156..0x15A and 0x25D..0x261 (the body leaves its spot),
  0x264..0x267 (the moment is after frame 100), 0x101..0x10E (held in someone's hands), 0x3A and
  0x3C (lying poses). Prop motions (0x143..0x153, 0x15C..0x170, 0xE5..0xED) play without their
  prop, as a mime.
- Loops play on until the next motion (the stance 50 frames on); chains (modes 4..6) go on by
  themselves with no AI.

| id | end | frames | moment | look | played by |
|---|---|---|---|---|---|
| 0x1BD | hold | 39 | f16, leans −25° | head-butt | dark tease `0x5A1780` |
| 0x1D4 → 0x1D5 | next, ping-pong | 25 + 5 | f19, z +0.5 | pounce hop | joy `0x5A0BA0` |
| 0x238 → 0x239 | next, lying | 32 + 167 | f17..23 | dive, face-plant | fright `0x5647B0` |
| 0x1B9 | hold | 67 | f26..46 | hop and swing | dark tease `0x5A1600` |
| 0x1C1, 0x1C0 | hold | 53, 81 | | lean in and back, stand tall | (body keys only) |
| 0x135 | loop | 48 | two hops to y 1.7 | bunny hops | animal play 3 |
| 0x1BB | hold | 67 | y 3.7 at f26, 3.5 at f46 | double high jump | dark tease `0x5A1700` |
| 0x259, 0x207 | hold | 44, 32 | y 2.1 at f18, 1.8 at f13 | hops | (body keys only) |
| 0x146 → 0x147 | next | 67 + 73 | | blowing bubbles | toy skill 9 |
| 0xEB → 0xEC → 0xED | next | 44 + 39 + 39 | | pouring from a can | watering `0x5680F0` |
| 0x83 → 0x82 | next, loop | 84 + 65 | | take-off, then flap | (body keys only) |
| 0x87 → 0x88 | next, ping-pong | 20 | y 2.2 | airborne | (body keys only) |
| 0x14C → 0x14D | next, ping-pong | 25 + 16 | y 2.8, then 2.2..3.65 | rise and hover | toy skill 7 |
| 0x11D → 0x11E → 0x11F | next | 25 + 33 + 13 | f20, y 1.9 | rears up; the game lights a flame at the hand from f20 | dragon play `0x5A27B0` |
| 0x11C | hold | 67 | leans −30° | lean forward | animal play 8 (sound 0x6087) |
| 0x163, 0x165, 0x16B | loop, loop, ping-pong | 57, 20, 20 | | playing a tune | performances A4, A1, A2 |
| 0x14F | loop | 80 | | seated drum | toy skill 1 |
| 0x11A → 0x11B | next, loop | 29 + 29 | | sway and flex | animal play 8 |
| 0x1BF | hold | 71 | f28..57, z +0.9 | leans in, glaring | dislike `0x5A1800` (voice 0x6069) |
| 0x1C3 | hold | 72 | | wiggle tease | dark tease `0x5A1680` |
| 0x136 → 0x137 | next, loop | 12 + 100 | | seated rocking | karate taunt |
| 0x248 | loop | 33 | ±0.2 sideways | sway (reads as a dodge) | karate set, unplayed |
| 0x1F1 → 0x1F2 | next, loop | 33 + 33 | | cower | fear `0x59F350` (whimper 0x602D) |
| 0xB7 → 0xB8 | next, loop | 20 + 20 | | munching (seated) | eat (sound 0x603D) |
| 0xCB → 0xCC | next | 51 + 105 | 360° spin at f29..54, y 1.8 | soaking it up | absorb (sparkle ring `0x6EFF10`) |
| 0xFA | loop | 150 | | pleased | petted |
| 0x13F | hold | 50 | | lean (after 0x13E, 90) | warthog play 5 |
| 0x0D | hold | 50 | | noticing | watering `0x5680F0` (emotion ball 1) |
| 0x133 | loop | 60 | flip at f19..46, z +1.0 | rolling forward flips | animal play 13 |
| 0x81, 0x77 | loop | 33, 24 | | charging run, fast run | warthog chain, whistle and ball |
| 0x140 → 0x81 | hold | 30 | z +0.5 | head-down charge | warthog `0x13E → 0x13F → 0x140 → 0x81` |
| 0xCA | hold | 9 | | reach and grab | grab |
| 0xBC → 0xBD → 0xBE | hold, next | 40 + 47 + 87 | | reaching for an item | pick-up `0x5613C0` |
| 0xCE | hold | 95 | y 2.4 at f9, z +0.9 at f69 | big hop | newborn `0x5685B0` |
| 0x171 → 0x172 | hold, next | 17 + 17 | z +0.38 | quick step | dance B0 |
| 0x184, 0x183, 0x17E, 0x15D | loop, loop, hold, loop | 89, 112, 40, 31 | 0x17E: a 360° spin with a hop | dances | dances B3, B2, B1, A5 |
| 0x138 → 0x139, 0x16F → 0x170 | next, ping-pong | 13 + 33, 33 + 44 | | dances | dances B4, A7 |
| 0x27 → 0x28 | next, ping-pong | 50 + 20 | | yawn | sleepy (sound 0x6005) |
| 0x18C | hold, then 0 | 79 | | swoon | (sound 0x6091) |
| 0x203 → 0x204 | next, loop | 6 + 24 | | mischief | (sounds 0x6065, 0x6066, 0x6068; eyes 10) |
| 0xE5 → 0xE6 → 0xE7 | next | 47 + 13 + 60 | | digging | seeds `0x567B10` |
| 0x92 → 0x93 | next, hold | 47 + 40 | rolls 200° | dive roll | swim trick `0x5620D0` |
| 0x8C | hold | 47 | from y 1.8 | coming down | hop clip |

### How a move picks its motion

- Its kind comes from its catalog record, the first that fits: the sound list; for status moves
  rest, heal, protect, weather, a status for the foe, raising, lowering, else charge; for
  damaging moves the one-hit, two-turn, multi-hit, recoil and draining effects, then going
  first (priority above 0 or the first-turn flinch), then contact, water, flying, spread
  (physical, on both foes or everyone), physical, special (`visuals::classify`).
- Each kind has a set of motions, each with its moment (frame). A move that is the `k`th of
  its kind (by id, `places`) picks `(k + k / n) mod n` of its kind's `n` (`vary`), so
  neighbours in a kind never share one and the set is used evenly.
- Its sprites leave at the moment. A step toward the target reaches furthest then, and the
  stance waits until the step is back; a quick move's dash lasts at least its moment + 4
  frames. A spread move's dust and shake come at its moment (single jumps only: 0x1BB is
  still in the air until about f66).
- A damaging move's sprites land 34..46 frames into the 50-frame attack beat (`LAND_FROM`,
  `LAND_BY`): they leave at the moment, moved just enough to land in that window. The hit
  that follows plays the hurt motion; its plain-hit burst is left out when the move's own
  sprites landed on that Chao in the last 20 frames, and the target only shudders as they
  land for a status move.
- Sprites that stay on the user (orbits, auras, rises, gathers, bursts on the user) move with
  it through its step and lift; shots leave from where it is at that frame.
- Status moves giving a status pick by the status: sleep 0x27, confusion 0x17E, infatuation
  0x18C (with the heart ball, state 4), poison 0x203, paralysis 0x1BF, burn 0x11D; fire and
  dragon special moves rear up (0x11D) with flame particles at the moment.
- Two-turn moves: the narrator marks the turn that only gets ready (`Cue::Attack { charging }`,
  a `Readying` start after the use). That turn plays 0x14C with a lift (into the air), 0xE5
  (underground), 0x92 (underwater) or a focus pose, and shows no effect sprites. While it is
  away the stance doesn't come back, and in the air it hovers 2 units up until its next
  attack, a hurt, a sign or a faint. The next turn strikes (0x8C from the air, coming down by
  the moment; 0x1BB from the ground, with the dust at its take-off, f13) with them.
- Multi-hit moves: each later hit on the target keeps the user's loop going and, for a contact
  move, steps in again. Only a move's hits count (a hurt with the hit's burst just before it),
  not poison, weather and other damage.
- A new attack drops the last move's sprites that haven't started yet, with the shake, flash
  and landing that were to come with them; what's already in flight still lands.
- With `BattleEffects` off none of this runs: a damaging move plays one of 0x243, 0x244, 0x246,
  0x247 with the step, a status move 0x242.

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
- The effect sprites are camera-facing quads in a display object of object list 1 with texture
  format 0xE (alpha test, depth writes); the arena Chao (list 4) and the trainers' figures draw
  after them, so a quad through the middle of a Chao is hidden by the Chao's front half. Drawn
  1.8 units toward the camera from where they are, they show in front of the body.
- The object display loop `0x470010` runs every list's +0x14, then the +0x20 displays, then
  list 0's and list 1's +0x1C, then lists 2..5's +0x1C. A list-1 object's +0x1C therefore
  draws after the arena Chao. See-through effect sprites draw there, their opaque pixels at
  alpha 0x92 (4 of RGB5A3's 7 steps), keeping the parser's texture format (blended, no depth
  writes), so the Chao shows through them.
