# Character effects and kart effects for puppets (2026-10-17, objdump of the dumped exe)

Puppets already draw: the remote motion and frame, mech upgrade parts (walkers) and the
Sonic-family jump/bounce ball trail (`POSE_BALL` = Action 6/69, re/spin-ball.md). Everything
below is what they still lack. None of it has been tried in game.

## Sonic family effect objects (all `0x755EA0(EDI = main)`: list 3 "exec", data 0x24 bytes at +0x40)
Sonic's main switches on Data1 Action (byte +0) via table 0x7197B4 (0x58 cases):

| Action | case | spawns | effect (display at obj+0x1C) |
|---|---|---|---|
| 3 spin charge | 0x717FBE | main 0x756260, init 0x7562A0 every frame | ball, display 0x756040 |
| 4 spin dash (rolling) | 0x718030 | 0x756930(idx) every frame (main 0x756260, init 0x7566C0: random rotZ/X/Y); when fast (co2+0xE8 vs Data1... +0x64) also 0x7577E0(idx) and co2+0x35C \|= 1 | ball, display 0x756520; streak main 0x757190 / display 0x757040 |
| 5 (rolling, likely) | 0x71830B | 0x756930(idx) every frame | same ball |
| 6 jump / 69 bounce | 0x718544 / 0x7185C9 | done (POSE_BALL) | |
| 13 homing / air dash | set at 0x71AF32 after 0x720E00 | once at the start: main 0x756DE0, init 0x756E20 | one ball at the start point, scale *0x1286DD4, shrinks by the double at 0x9050C8 per frame, display 0x756C90 |
| 60 (unknown, uncertain) | 0x7182A2 | 0x721920, then 0x7577E0(idx) | streak |

- All the balls use the same model and texlist as the jump ball (slot 19 with S_EFFTEX 0xA08B94,
  Shadow slot 84 with 0xA08D94, Amy 0xA08F94, co2+3 == 0xC → slot 436 with 0xA0917C). So
  `puppet.rs::spin_ball` can draw them all as is.
- The streak (0x757190) re-reads `0x1DEA6C0[data+1]` every frame (0x7571F2) and follows that
  player, so it can't be spawned for a puppet. Draw it by hand or skip it.
- Knuckles, Rouge, Tails and Eggman spawn none of these: 0x755EA0 has no other callers (the 7th,
  0x7592A6, is an object called from 0x4C7FA3/0x4C8E2F). Their glide, climb, dig and light dash
  are motion only, which puppets already copy. Light dash and somersault have no separate effect
  object that I found (uncertain for action 60).

**Puppet recipe:** send Data1 Action (u8) instead of the 1-bit `POSE_BALL` (a protocol change;
or use `flags` bit 0x80 as "spin": Actions 3/4/5). Draw the 3-ball trail for 3/4/5 like 6, and
for 13 draw one fading ball at the point where the action became 13 (scale from *0x1286DD4,
minus *(f64*)0x9050C8 per frame).

## Mech booster flame (Tails' walker only: both callers check co2+3 == 6)
- While **co2+0x36E (s16 frame counter) > 0** (capped at 0x5F, -1 per frame), the walker main
  at 0x741BF1 and 0x7421ED spawns a jet particle every frame:
  `0x75EE70(cdecl: idx, kind 3, &velocity, float scale)`, with **EBX = &position**. The caller
  pops 0x10.
  - position = nozzle co2+0x218..0x220 + dir co2+0x2B4..0x2BC × (counter/k + c);
  - velocity = -dir × const.
  - The second site also checks co2+0x176 (anim) == 0x1B.
- 0x75EE70 makes a child of the player object 0x1DEA6E0[idx] (or a list-3
  "WalkerJetExecutor" when there's none). Data1 holds: +0 kind, +1 idx, +0x14 position,
  +0x8 velocity, +0x20 scale (shrinks by +0x24), +0x28 the scale argument. Its main 0x75EC80
  only reads 0x174AFF7 (the pause bit for idx); it never reads the player. **So with idx 0 it
  works as a free particle for a puppet**, freed with the stage.
- **Puppet recipe:** the sender adds a jet bit plus the nozzle and dir (co2+0x218 / +0x2B4,
  only written for the local walker). Or the receiver estimates the nozzle from the puppet's
  position and rotation. While the bit is on, call 0x75EE70 every frame (asm stub for EBX).
  The scale arg is uncertain: copy what 0x741CB6 computes (≈ counter/[0x906580] + [0x905998]).

## Kart races
- **There are no items in SA2 kart racing.** The kart SET list (0x100A230 → 0x100A160, 13
  entries) has DMYOBJ, JUMP PANEL 0x625A00, **DASH PANEL 0x625320**, GATE 0x6245A0, GATE2,
  KANBAN, RESTART_OBJ, LIGHT SW, **CART RING 0x625770** (rings, 0x44CE10), **BALOON 0x624E10**,
  HIKOUSEN, KAKUSI and G RING.
- **Dash panel:** for a human car, 0x61F710 writes the velocity w+0x7C..0x84 and clears w+0x30
  and w+0x68. Then 0x61FDD0 plays a voice. There's no boost flag or timer, only speed, so
  remote karts already show it as speed.
- **Exhaust:** the kart display 0x61C6C0 draws the exhaust sprites 0x61ECF0 when course ≥ 2
  and w+0x40 is set; kart.rs already sets w+0x40. **w+0x114 == 1 makes the exhaust bright.**
  It is "accelerator held": 0x61D8A0 sets it from buttons & 0x402 and clears it when
  w+0x28C > 10. w+0x110 = brake (button bit 2). Cheap win: send the accel bit and write
  w+0x114 for puppets.
- **Jumps and bumps:** the body tilt w+0xF0..+0x104 isn't sent, so a remote kart stays flat
  over jump panels. Sending those 6 ints (or 3 summed pairs) makes jumps and bumps show.
- Balloon hit: 0x61F7B0 sets w+0x108 = 1 and w+0x288 = 0x4EC and calls 0x443480 / 0x6260F0
  (meaning uncertain, likely a bonus timer with no visible effect on the car).

## Not done: upgrade models on foot
Sonic's display swaps model slots depending on co2 state (0x7205A3..0x7205D1: slot 0 ↔ 48/31,
66 ↔ 0x1DE99FC/0x1DE9964 slots). The shoe and bracelet parts likely depend on these swaps
(uncertain). Puppets on foot always draw the base model.

## Ranking (value / effort)
1. Kart accel exhaust (w+0x114) plus tilt: tiny protocol addition, visible all race.
2. Spin dash and charge balls (Actions 3/4/5): reuses `spin_ball`, needs only the action
   (or bit 0x80).
3. Homing start ball (Action 13): small, same draw code.
4. Tails' mech jet: medium (asm stub, nozzle data in the protocol), nice in co-op mech stages.
5. Spin-dash streak (0x757040) and on-foot upgrade models: high effort, low value. Skip.
