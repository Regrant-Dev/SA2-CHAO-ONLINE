# Bosses: HP, the boss gauge, defeat and following

## The boss gauge: one mechanism for all of them

**Create: 0x761BA0** (usercall). EAX = parent object (0: LoadObject in list 3; otherwise a child
of EAX via 0x470C00). **EBX = `float *hp`**. The gauge ObjectMaster's main is **0x761C30**, its
name "BossPowerGuageExec", and its texture "boss_pwguage_tex".
- Gauge Data1 (+0x34): **+8 = the `float *hp` it shows**, +0x28 = *hp at creation (the full bar),
  +0x14 = the bar as drawn (it eases toward *hp × 32).
- It only reads *hp, never writes it. Lowering *hp moves the bar.
- The gauge is created a little after the stage starts (rivals: when +0x48 first passes a
  compare, 0x4C8ED7).

| Level | Boss | HP float | Start | A hit |
|---|---|---|---|---|
| 60 | Big Foot | boss obj Data2 (+0x40) **+0x28** | data | −1.0 (0x5D01D2) |
| 61 | Hot Shot | boss obj Data2 +0x28 (gauge 0x5C7BEB) | data | −1.0 (0x5CB6C3) |
| 62 | Flying Dog | boss obj Data2 +0x28 (0x5D1962) | data | −1.0 (0x5D3B26) |
| 63 | King Boom Boo (main 0x6129C0, code from 0x60CFF0) | Data2 +0x28 (0x60D06C) | ? | −1.0 (0x611FE7) |
| 64 | Egg Golem (golem.c, likely the Sonic one) | **`*(0x1A5A580)` +0x14** (+0x18 copy at start) | 5.0 (0xB5D210, in .data) | −1.0 (0x4BFB3F) |
| 67 | Egg Golem (golem_e.c, likely the Eggman one) | `*(0x1A5A580)` +0x14 | 6.0 (0x8AC158) | −1.501 (0x4AA01F) |
| 65 | Biolizard (BossLast1) | Data2 +0x28 (0x51FD4C) | ? | fractional: −0.06 (0x51E760), other sites |
| 66 | Final Hazard (BossLast2) | Data2 +0x28 (0x499A35) | ? | −1.0 (0x499597), −0.016 (0x4A00E5) |
| 19, 42, 20, 29, 33 | rival fights | **`MainCharObj2[1]` (`*(0x1DE9604)`) +0x48**, the AI rival's life | 5.0 / 3.0 (0xB5D210 / 0x1286DD4) | not traced |

Rivals: Sonic/Shadow and Knuckles/Rouge give the gauge `&CharObj2[1]+0x48` directly (0x4C8EF0,
0x61A22C, 0x64B30A). **The mech fights** (0x627CB7, 0x663987) give it a **copy** at the gauge
creator's Data2 +8 (= +0x48 × 0.5) that's refreshed elsewhere, so the real value is +0x48, not
the pointer the gauge holds. The level → module mapping of those five callers is not checked.

## How a hit works (guard robots, 0x5CB6C3, typical)

`hp -= 1.0` → effects (0x5C6D80, 0x5D4A50) → **`if hp <= 0`**: Data1 state = 0xA or 0xE (defeat)
→ else state 0xC (hurt; it's skipped while already in 0xA/0xC). **The defeat check is only in
the hit handler.** Lowering hp from outside never defeats the boss and plays no flinch; writing
0 leaves a boss with an empty bar that can't die. Golem also does `hp = min(hp, x)` from
scripted phases (0x4A997D, 0x4C3FAD) and switches phase by comparing hp with +0x1C (0x4C0D8D),
so it follows a lowered hp on its own. Multi-phase bosses can reset hp per phase, so zero HP
alone is not a defeat.

## Level clear

**0x43E6D0** (usercall ESI = player) is the level-clear routine: it guards duplicate clears,
disables controls, stops the timer, sets the LevelClear flag **0x174B002**, plays the clear
sound, creates native results through 0x451450 and selects the victory action/camera. Calling it
for player 0 runs the game's own results, save and story progression.

## King Boom Boo's defeat sequence

- Main **0x6129C0**, HP at ObjectMaster +0x40 work +0x28. Its switch maps mode 11 to the
  vulnerable handler **0x611470**, mode 12 to the ending **0x612230**.
- The finishing hit at **0x611FE7..0x6120D0** subtracts 1, tests <= 0, sets physics at
  ObjectMaster +0x38 to velocity `(0,1,0)` and zero next vector, disables all 11 collision
  slots (0x486A60: EAX=index, ECX=object, stack 0; 0x486A80: EAX=index, EDX=object,
  stack 3; both caller-pop 4), clears Data1 flag 4 and calls **0x60CE40(object,12)**.
- That cdecl mode helper clears a phase child, rotations X/Z, flag 0x800 and physics +0x4C,
  then sets mode/sub to 12/0. The ending runs its native animation/sounds/effects through
  substates 0..6 and finally calls 0x43E6D0, the camera and results routines itself.
- The vulnerable hit handler also changes the local player's physics/camera, so it can't be
  called to defeat the boss from outside; only the defeat setup above can.

## Shared HP and completion in co-op (src/bosses.rs)

- **Finding the HP:** in any co-op stage the mod looks for the gauge every 30 frames by walking
  object lists 0..7 (heads at 0x1A5A254 + 4n, circular via +4) and each object's children
  (+0xC) for main 0x761C30. It keeps the gauge object once found (re-checking its main every
  frame) and uses `*(0x1DE9604) + 0x48` instead of the gauge's pointer when there's a player 2
  (only rival fights have one in 1P).
- **Sharing hits:** a drop in HP that isn't the mod's own write is sent as the absolute value
  with the bar's full value (at most every 100 ms, resent every 2 s while the boss is hurt). A
  lower value from a friend with the same full value lowers local HP to `max(theirs, 0.01)`;
  HP is never raised. A finishing hit is sent immediately; a zero seen at the intro isn't a
  defeat.
- **King Boom Boo:** a friend's finishing hit runs the native defeat setup above on the local
  boss (after checking its collision, physics, motion and phase work and the mode helper's
  code), so every game plays his defeat.
- **Completion:** boss arenas **60..67** and rivals **19, 20, 29, 33, 42** share the clear. A
  player's LevelClear (0x174B002) goes out as a reliable `RaceStatus` finish and repeats during
  results. A matching finish from a co-op party member in the same stage and mission calls
  0x43E6D0 for player 0, after loading, pause, death/restart requests, disabled controls and
  the start hold are over. Competitive rooms and normal/hunt stages keep their own rules.

## GroboLib bosses (Hot Shot, Big Foot, Flying Dog)

**Objects** (all LoadObject list 2):
- Hot Shot: "BossHotShotExec", main **0x5CB2D0**, created at 0x5C712C by 0x5C7120.
- Big Foot: "BossBigFootExec", main **0x5CFE40**, created at 0x5CC35C.
- Flying Dog: "BossFlyingDogExec", main **0x5D35E0**, created at 0x5D0D7C.

**Data layout:**
- **Data1 (+0x34):** +0 mode, +1 sub-mode, +3 a counter, +4 flags (1 guarding: a hit takes the
  guarded hurt mode; 4 still flashing from a hit: hits don't count), +6 timer (u16), +8..+0x10
  rotation, +0x14 position, +0x18 a height.
- **Data2 (+0x40):** +0xC the motion controller, +0x14 the gauge, +0x18 a child object, +0x1C a
  work position, +0x28 HP.
- The intro step (Hot Shot 0x5C7A60) writes player 1 directly (zeroes `PLAYERS[0]`+0x14..0x1C to
  place the player) and toggles 0x174AFFF (player control); it ends in mode 2, sub-mode 0.

**Hot Shot's main (0x5CB2D0):**
1. Turns its head toward player 1, through the motion table entries at 0x1A55874.
2. Runs `switch (Data1[0])` over its mode functions:
   - 1 intro 0x5C7A60, 2 0x5C7CF0, 3 0x5C81A0 (makes a cursor), 4 0x5C8670;
   - 5 0x5C8870 (missiles), 6 0x5C9170, 8 0x5C8F30;
   - 9 0x5CA820 (energy gun + 3D cursor), 10 0x5C9380 (hurt, while guarding);
   - 12 0x5C9570 (dying), 14 0x5CAA90 (hurt), 15 0x5CA1A0 (falls).
   - Unhandled: 7, 11, 13.
3. Calls 0x7938D0, its motion.
4. Handles hits (inlined): 0x486A00 (hit by the player), then Data2+0x28 HP −1.0. The mode
   becomes 14/10, or 12/15 at 0 HP. Hits in mode 9 first tidy the energy gun (0x5D7B50).
5. Runs 0x5C7810 (likely its collision/parts).

The mode functions are called three ways: object on the stack, in EAX, or in EBX (0x5CB53C..,
0x5D0094..), with ESI = motion controller and EDI = Data1.

**Big Foot (0x5CFE40):** mode functions 1 0x5CCB30, 2 0x5CCD40, 3 0x5CD1E0 (cursor), 4
0x5CD650, 5 0x5CD800 (missiles), 6 0x5CE150, 8 0x5CDF60, 9 0x5CE360 and 12 0x5CF7C0 (hurt), 10
0x5CE540 (dying), 13 0x5CF180 (falls). Unhandled: 7 and 11. Hits skip modes 9 and 10. Its intro
writes player 1 at 0x5CCC97.

**Flying Dog (0x5D35E0):** mode functions 1 0x5D1800, 2 0x5D1A30, 3 0x5D1AC0, 4 0x5D1EE0, 5
0x5D2060, 6 0x5D2650, 8 0x5D2440, 9 0x5D33C0, 10 0x5D2780 (hurt), 11 0x5D2E90 (dying).
Unhandled: 7. Mode 6 takes the object in EDI (0x5D3868 `mov edi, ebx`), mode 2 in EAX, mode 4
in EBX, the rest on the stack. The whole hit check skips mode 11, HP is skipped in 10/11, and
every hit takes mode 10 (no guarded variant); a hit also deletes its Data2(+0x10)+0x38/+0x48
children. Its main makes its engine flames itself every frame (0x5D1320 → 0x5D5510).

**Motion:** the motion controller at `Data2+0xC` is the same kind the Chao use. The mode
functions pick motions with **0x793C40** (usercall EAX = controller, EDX = motion id; 38 calls
in 0x5C7000..0x5D4800, e.g. Hot Shot 0x5C7D67 id 0, 0x5C7DEE id 1, 0x5C7EAC id 2). **0x7938D0**
(ESI = controller) plays the motion every frame; it's called from the main after the mode
switch, and alone while paused (0x174AFF9). Controller +4 is the motion now, +8 the next one,
+0x2C the current motion data, +0x48 the table (0x1C-byte entries).

**Head turn:** the twist entries at `*0x1A55874`+0x24 (0x2B of 0x30 bytes, types 4/6) are
computed in each main from player 1. That part of the main also runs the hit code (0x46C340
hurts player index 0 through both the EntityData1 and CharObj2 arrays).

**Attack creators:**
- **Shared (GroboLib):** missile 0x5D6320 (from mode 5 / Big Foot 0x5CD800 / Flying Dog
  0x5D2060); cursor 0x5D4B70 (modes 3); energy gun 0x5D5310 and 3D cursor 0x5D58C0 (mode 9 /
  0x5D33C0); e-bombs 0x5D8DF0 / 0x5D9400 (from the energy gun); burn/flame 0x5D5510 (from the
  flame helpers 0x5C7680 / 0x5CC6C0 / 0x5D1320).
- **Per boss, made once at load:** a gun arm (0x5C7370 / 0x5CC640 / 0x5D10B0), the gun data
  block 0x5D5FD0 (not an object) and the pit 0x5D48D0 (main 0x5D4A80, only math and
  collision).
- The flame helpers 0x5C7680 / 0x5CC6C0 are called only from mode functions, so a boss whose
  mode function doesn't run fires nothing (Flying Dog's own flames excepted).
- **Missile 0x5D6320** (EAX = parent object, stack: &launch point, &a vector of the caller's,
  float speed): makes a child of the boss (0x470C00, main 0x5D6A30, display 0x5D6AA0) and keeps
  both pointers (Data2 +0x34 / +0x38). Launch points are the tables 0x1A27CD4.. / 0x1A27D7C..,
  filled each frame by Hot Shot's node draw callback 0x5CB840 (0x426CC0 on model nodes
  0x1133D20..). The missile flies toward what the cursor (0x5D4B70) locked on to: that game's
  player 1.
- **Attack objects (list 3) and their displays.** Each display reads only its object's Data1 at
  offsets 0..0x2B (never its collision pointer +0x2C), plus models and textures that load with
  the boss stage:

| Attack | Main | Display (slot) | Data1 read |
|---|---|---|---|
| missile | 0x5D6A30 | 0x5D6AA0 (+0x14) | +0, rotation +8..+0x10, position +0x14.. |
| energy-gun beam | 0x5D9A70 | 0x5D9890 (+0x1C) | +6, +8..+0x20 |
| energy-gun bomb | 0x5D9BA0 | 0x5D9CA0 (+0x14) | +0x14..+0x28 |
| energy-gun pillar | 0x5DA280 | 0x5DA2D0 (+0x14) | +0x14..+0x28 |
| missile smoke | 0x525A70 | 0x5D6BA0 (+0x1C) | |
| flame | 0x5D56B0 | 0x5D5730 (+0x1C) | +0x14..+0x28 |
| gun fire | 0x5D8B20 | none found | |
| e-torus | 0x5D9F40 | none found | |

## Boss following (src/boss_follow.rs)

In co-op, the GroboLib bosses run as one boss for the party:
- **Freezing:** a boss's main run with Data1[0] set to a mode its switch skips (7) still turns
  its head, plays its motion (0x7938D0), takes hits and keeps its collision and parts, but runs
  no mode function, so it fires nothing.
- **Leader:** one room player's game leads, chosen with keeper.rs's election among room players
  fighting the boss in the same stage and mission and heard within 1.5 s (`BossClaim`: a claim
  with a term, sticky; after a quiet moment the lowest id claims). Alone, you lead; with nobody
  leading, every game runs its own boss. The leader's boss is after the leader only.
- **Followers:** the local boss is frozen and takes the leader's state (`BossState`): mode bytes,
  flags, timer, position (eased), facing and motion (via 0x793C40 after a table check).
- **Hits:** hits stay real in each game (HP shared as above). A hit on a frozen boss in a mode
  where its own code ignores hits (Hot Shot 10/12, Big Foot 9/10, dying, the intro) is undone.
  A follower's hit is sent (`BossHit`) and the leader's boss takes the hurt mode (Hot Shot 14 /
  guarded 10, Big Foot 12 / guarded 9) with sub-mode, counter and timer cleared, as its inlined
  hit code does; not in Hot Shot's mode 9 or while it's flashing. A follower's last hit lets its
  own boss die there.
- **Handoff:** when the leader stops claiming (left, dropped, restarted, paused), the next one
  claims and its boss carries on in the mode it was following, from sub-mode 0 (a mode's first
  step makes what the rest of it uses); the intro or a dying mode become mode 2.
- **Attacks:** the leader finds its boss's attacks by their display pointers in object list 3
  and two levels under the boss (children circular through +0), and sends their Data1 bytes
  with a serial and target (`BossAttacks`, 10 Hz). A follower draws each on an object of its
  own (the attack's display, the leader's Data1 bytes, eased between updates) with no update or
  collision of the game's, so it never hits. Flying Dog's flames aren't sent: a frozen follower
  makes its own where the leader's boss is.
- **Not followed:** the head turn (each game's own player 1).

## Other bosses

- **King Boom Boo (63):** the fight works through the sun/shadow light switches, which are SET
  switches covered by the switch sync.
- **Egg Golem (64 / 67):** a scripted sequence of falling platforms and head attacks.
- **Biolizard / Final Hazard (65 / 66):** multi-part bosses (the life-support pod, the spikes),
  each part its own object, with fractional HP hits.
- **Rival fights (19, 20, 29, 33, 42):** the rival is a real AI player 2 (`MainCharObj1[1]`,
  life `CharObj2[1]`+0x48); its attacks are player-2 moves that hurt player 1.

These share HP and completion only.
