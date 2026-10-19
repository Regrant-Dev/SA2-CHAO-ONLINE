# Bosses: HP and the boss gauge (for shared boss HP in co-op)

## Party completion for every boss (2026-10-19, REA VM/Ghidra)

User reproduced shared damage without shared completion on King Boom Boo and Egg Golem.
Local log level 63: HP 4 -> 3 -> 2 -> 1 from friends, followed by leaving the stage.
The sender explicitly required `hp > 0`, so a finishing hit was never transmitted. The
receiver also intentionally clamped shared HP to 0.01 and required its own final hit.
Server logs contain joins/leaves only, with no detailed boss events to prove otherwise.

All boss arenas **60..67** and rivals **19,20,29,33,42** now share confirmed completion
in co-op. Use the existing `RaceStatus.finish` message, emitted from native LevelClear
flag **0x174B002**, not zero HP alone: multi-phase fights must not end at an intermediate
HP reset. Send the initial clear reliably, including fights without a start-together clock,
and repeat the completed status during results. Do not overwrite that first reliable
announcement with a periodic status in the same frame. Only accept a co-op party member's
matching stage/mission completion; competitive rooms and normal/hunt stages keep their rules.

REA rechecked **0x43E6D0** (usercall ESI = player): it guards duplicate clears, disables
controls, stops the timer, sets 0x174B002, plays the clear sound, creates native results
through 0x451450 and selects the victory action/camera. A matching confirmed party win
calls this same routine for player 0. Wait through loading, pause, death/restart requests,
disabled controls and the start hold. Pending wins are applied after normal control returns.
Use the engine's results/save/story progression rather than writing save bits or GameState.
The local surviving boss's visual state can differ while entering these results; physical
boss-animation synchronization remains separate from confirmed stage completion.

King Boom Boo additionally supports its full native remote defeat sequence:

- Actual main **0x6129C0**, HP at ObjectMaster +0x40 work +0x28. Its switch maps mode 11
  to vulnerable handler **0x611470**, mode 12 to ending **0x612230**.
- Finishing hit at **0x611FE7..0x6120D0** subtracts 1, tests <= 0, sets physics at
  ObjectMaster +0x38 to velocity `(0,1,0)` and zero next vector, disables all 11 collision
  slots (0x486A60: EAX=index, ECX=object, stack 0; 0x486A80: EAX=index, EDX=object,
  stack 3; both caller-pop 4), clears Data1 flag 4 and calls **0x60CE40(object,12)**.
- That cdecl mode helper clears a phase child, rotations X/Z, flag 0x800 and physics +0x4C,
  then sets mode/sub to 12/0. The ending runs its native animation/sounds/effects through
  substates 0..6 and finally invokes 0x43E6D0, camera and results routines itself.
- Match the live boss's work HP pointer to the current gauge, require its collision,
  physics, motion and phase work, verify the mode-helper prefix, then perform only the
  native defeat setup. Do not call the whole vulnerable hit handler: it also affects the
  local player. Never dereference a cached gauge unless it still belongs to the live lists.

Finishing HP is normalized to zero and sent immediately despite normal rate limiting,
with periodic resends; an initial unobserved zero is not a defeat. Bosses without a traced
remote death animation use the confirmed-clear mechanism above. No packet format or
server protocol change is needed. Headless tests cover terminal HP, intro zero, resends and
party/mode/stage gating. Full Golem, Boo, rival and final-boss gameplay retests remain needed.

Read from the objdump listing of `re/bin/sonic2app.dumped.exe` (2026-10-17). Not run in game yet.

## The boss gauge: one mechanism for all of them

**Create: 0x761BA0** (usercall). EAX = parent object (0: LoadObject in list 3; otherwise a child
of EAX via 0x470C00). **EBX = `float *hp`**. The gauge ObjectMaster's main is **0x761C30**, its
name "BossPowerGuageExec", and its texture "boss_pwguage_tex".
- Gauge Data1 (+0x34): **+8 = the `float *hp` it shows**, +0x28 = *hp at creation (the full bar),
  +0x14 = the bar as drawn (it eases toward *hp × 32).
- It only reads *hp, never writes it. Lowering *hp moves the bar.

| Level | Boss | HP float | Start | A hit |
|---|---|---|---|---|
| 60 | Big Foot | boss obj Data2 (+0x40) **+0x28** | data | −1.0 (0x5D01D2) |
| 61 | Hot Shot | boss obj Data2 +0x28 (gauge 0x5C7BEB) | data | −1.0 (0x5CB6C3) |
| 62 | Flying Dog | boss obj Data2 +0x28 (0x5D1962) | data | −1.0 (0x5D3B26) |
| 63 | King Boom Boo (likely: unlabeled code at 0x60CFF0) | Data2 +0x28 (0x60D06C) | ? | −1.0 (0x611FE7) |
| 64 | Egg Golem (golem.c, the Sonic one I think) | **`*(0x1A5A580)` +0x14** (+0x18 copy at start) | 5.0 (0xB5D210, in .data) | −1.0 (0x4BFB3F) |
| 67 | Egg Golem (golem_e.c, the Eggman one I think) | `*(0x1A5A580)` +0x14 | 6.0 (0x8AC158) | −1.501 (0x4AA01F) |
| 65 | Biolizard (BossLast1) | Data2 +0x28 (0x51FD4C) | ? | fractional: −0.06 (0x51E760), other sites |
| 66 | Final Hazard (BossLast2) | Data2 +0x28 (0x499A35) | ? | −1.0 (0x499597), −0.016 (0x4A00E5) |
| 19, 42, 20, 29, 33 | rival fights | **`MainCharObj2[1]` (`*(0x1DE9604)`) +0x48**, the AI rival's life | 5.0 / 3.0 (0xB5D210 / 0x1286DD4) | not traced |

Rivals: Sonic/Shadow and Knuckles/Rouge give the gauge `&CharObj2[1]+0x48` directly (0x4C8EF0,
0x61A22C, 0x64B30A). **The mech fights** (0x627CB7, 0x663987) give it a **copy** at the gauge
creator's Data2 +8 (= +0x48 × 0.5) that's refreshed elsewhere. Write the real +0x48, not the
pointer the gauge holds. The level → module mapping of those five callers is not checked.

## How a hit works (guard robots, 0x5CB6C3, typical)
`hp -= 1.0` → effects (0x5C6D80, 0x5D4A50) → **`if hp <= 0`**: Data1 state = 0xA or 0xE (defeat)
→ else state 0xC (hurt; it's skipped while already in 0xA/0xC). **The defeat check is only in
the hit handler.** Lowering hp from outside never defeats the boss and plays no flinch. Golem
also does `hp = min(hp, x)` from scripted phases (0x4A997D, 0x4C3FAD) and switches phase by
comparing hp with +0x1C (0x4C0D8D), so it follows a lowered hp on its own.

## Recipe for the mod
1. Each frame in a co-op stage at levels 60–67, find the gauge. Walk object lists 0..7 (heads at
   0x1A5A254 + 4n, circular via +4) and each object's children (+0xC), looking for main ==
   0x761C30. Then `hp = *(float**)(Data1 + 8)` and `full = *(float*)(Data1 + 0x28)`. For the
   rival levels use `*(float**)0x1DE9604 + 0x48` instead (null-check the CharObj2).
   - Instead of the walk: Golem `*(0x1A5A580)+0x14`, the others the boss object's Data2+0x28.
     The walk is simpler and covers both.
2. **Detect our hits:** hp lower than last frame, and not by our own write. Send the absolute value
   `(level, hp)`. It's idempotent, so sending it reliably or resending after loss is fine.
3. **Apply:** `if their_hp < our_hp { *hp = max(their_hp, SLIVER) }` with SLIVER ≈ 0.01. Never
   raise hp, and never write ≤ 0. The bar drops to the shared value. The killing blow is still
   each player's own: the next real hit takes it below 0 and runs the game's defeat. Writing 0
   leaves a boss with an empty bar that can't die (no defeat path outside the hit handler).
4. Reset per stage entry. The gauge object is created a little after the stage starts (rivals:
   when +0x48 first passes a compare, 0x4C8ED7), so keep polling until it exists.

Unsure: King Boom Boo's identity (it sits between the 0x60xxxx boss modules and its gauge
call, with no name strings); which Golem file is level 64 vs 67; whether Biolizard and Final
Hazard have extra hp writes besides the hit handler (multi-phase bosses may reset hp per phase,
which a stale "their hp" would then cut short, so compare only within the same phase: send `full`
too and apply only when `full` matches); and the rival decrement and defeat path.

## What the mod does (src/bosses.rs)
No level list: in any co-op stage it looks for the gauge every 30 frames, keeps the gauge
object once found (re-checking its main every frame), and uses `*(0x1DE9604) + 0x48` instead of
the gauge's pointer when there's a player 2 (only rival fights have one in 1P).

## Following one player's boss (2026-10-18 assessment; not built)
- **How a boss runs:** a boss is one object made by the level's own code (not a SET record), with
  a Data1 state machine and helpers. Hot Shot's intro step 0x5C7A60 shows the shape:
  - **Data1 (+0x34):** +0 mode, +1 sub-mode, +6 timer (u16), +0x14 position, +8..+0x10
    rotation.
  - **Data2 (+0x40):** +0x1C a work position, +0x14 the gauge, +0x18 a child object, +0x28 HP.
  - **Attacks:** spawned by the mode functions (in the intro, a child from 0x5D86B0 and an effect from 0x4ECEA0; what each is wasn't traced).
  - **Scripted writes:** the boss code writes player 1 directly (the intro step 0x5C7A60
    zeroes `PLAYERS[0]`+0x14..0x1C to place the player) and toggles 0x174AFFF (player control).
- **Why following the leader's state doesn't work generically:**
  - **Attacks fire twice or never.** Copying the leader's mode, timer and position into the
    others' boss doesn't stop their mode functions. Whichever frame the copy lands on decides
    which of its own attacks fire.
  - **Copying everything isn't enough either.** The children (missiles, the gauge, parts) are
    separate objects with their own state, so copying all of Data1/Data2 still leaves them
    behind.
  - **The scripted player writes break.** They would have to run only in each player's own game.
- **What each boss would need:** pause the boss's own mode functions on the followers. Then each
  attack spawn becomes a message (kind, position, direction), fired with the boss's own spawner
  on the others, like `shots.rs`. The Data1 pose follows the leader every frame. Per boss:
  - **Big Foot / Hot Shot / Flying Dog (levels 60-62, GroboLib):** they share
    `b_grobo_missile.c` and the gauge code, so one spawner table (missiles, machine gun, jump
    stomps) could cover all three. They're the best first candidate.
  - **King Boom Boo (63, 0x60CFF0):** the ghost fight works through the sun/shadow light
    switches. Those are SET switches the existing switch sync covers. Its pose plus "vulnerable"
    state would also need following.
  - **Egg Golem (64 Sonic, 67 Eggman):** HP is at `*(0x1A5A580)`+0x14. The boss is a scripted
    sequence of falling platforms and head attacks, so following mode and timer is close to
    enough. Platforms broken in one game would need the kill-style sync.
  - **Biolizard / Final Hazard (65, 66):** fractional HP hits, with many child parts (the
    life-support pod, the spikes). They're the most work and the least sound to share; keep HP
    only.
  - **Rival fights (19, 20, 29, 33, 42):** the rival is an AI player 2 (`MainCharObj1[1]`) whose
    life is `CharObj2[1]`+0x48. Following it means driving player 2 from the leader's rival pose
    (as a puppet drives a drawing), with its AI off. That's a separate project.
- **What stays:** the shared HP (bosses.md "What the mod does") gives one result for everyone.
  Each player dodges their own copy of the boss.

## GroboLib bosses: following one leader (2026-10-18; RE done, stopped before building)
**Objects:**
- **The three boss objects:**
  - Hot Shot: "BossHotShotExec", main **0x5CB2D0**, created at 0x5C712C by 0x5C7120.
  - Big Foot: "BossBigFootExec", main **0x5CFE40**, created at 0x5CC35C.
  - Flying Dog: "BossFlyingDogExec", main **0x5D35E0**, created at 0x5D0D7C.

  All three are LoadObject list 2.
- **Hot Shot's main (0x5CB2D0):**
  1. Turns its head toward player 1, through the motion table entries at 0x1A55874.
  2. Runs `switch (Data1[0])` over its mode functions:
     - 1 intro 0x5C7A60, 2 0x5C7CF0, 3 0x5C81A0 (makes a cursor), 4 0x5C8670;
     - 5 0x5C8870 (missiles), 6 0x5C9170, 8 0x5C8F30;
     - 9 0x5CA820 (energy gun + 3D cursor), 10 0x5C9380 (hurt, while guarding);
     - 12 0x5C9570 (dying), 14 0x5CAA90 (hurt), 15 0x5CA1A0 (falls).
  3. Calls 0x7938D0, its motion.
  4. Handles hits: 0x486A00 (hit by the player), then Data2(+0x40)+0x28 HP −1.0. The mode
     becomes 14/10, or 12/15 at 0 HP.
  5. Runs 0x5C7810, likely its collision/parts.
- **Data1 fields:** +4 flags (bits 1 and 4 choose the hit reaction; their meaning isn't confirmed), +3 a
  counter, +6 timer, +0x18 a height.
- **Attack creators:**
  - **Shared (GroboLib):** missile 0x5D6320 (from mode 5 / Big Foot 0x5CD800 / Flying Dog
    0x5D2060); cursor 0x5D4B70 (modes 3); energy gun 0x5D5310 and 3D cursor 0x5D58C0 (mode 9 /
    0x5D33C0); e-bombs 0x5D8DF0 / 0x5D9400 (from the energy gun); burn 0x5D5510 (from the gun
    arms 0x5C7680 / 0x5CC6C0 / 0x5D1320).
  - **Per boss:** each boss's gun arm (0x5C7370 / 0x5CC640 / 0x5D10B0, plus 0x5D5FD0) and pit
    (0x5D48D0) are made once at load, as children that run with the boss.
  - **Attack object mains:** gun fire 0x5D8B20, e-beam 0x5D9A70, e-bomb 0x5D9BA0, e-torus
    0x5D9F40, e-pillar 0x5DA280, missile smoke 0x525A70 (all list 3).

**Why it stopped short of building:**
- **Freezing the mode functions also freezes the look.** A wrapper can run each follower's
  main with Data1[0] set to a mode the switch doesn't handle (7, 11 or 13). Then the head turn,
  motion, hits and parts still run, but no mode function does, so no attacks spawn locally, and
  the leader's mode is put back after. The catch: the mode functions are what choose the motion
  (and probably the arm/gun parts' states). A frozen follower would slide around in its last
  pose unless the motion id is followed too. That's a separate RE of the motion controller
  (0x7938D0's data), not done.
- **Replaying attacks needs more RE than a message.** The spawns take boss-relative arguments.
  - **The cursor** (0x5D4B70) locks onto *its game's* player 1 and the missiles fly to the
    cursor. Each follower must aim at itself, so a replay has to rebuild the cursor/missile
    pair locally rather than copy the leader's target.
  - **The energy gun and burn** are children of the per-boss gun arm. A replay needs the
    follower's own arm object as the parent, plus each creator's usercall convention (none
    traced yet).
- **Hits and the end of the fight would fight the leader's state.** A follower's own hit
  handler sets the hurt and dying modes (14/10/12/15). The wrapper would overwrite those with
  the leader's mode, unless following stops once local HP reaches 0. Then the last-hit rule
  bosses.rs depends on still has to hold.

**What's safe today:** shared HP (bosses.rs). Each player fights their own copy of the boss.

**Next RE steps, if this goes on:**
1. Hot Shot's motion controller: what 0x7938D0 reads, so the motion id can be followed.
2. The usercall arguments of 0x5D6320, 0x5D4B70, 0x5D5310 and 0x5D5510.
3. Whether the gun arm children read Data1[0].

### Further RE (2026-10-18, round 3)
**Motion.**
- **Same controller as the Chao.** A GroboLib boss's motion controller is `Data2(+0x40)+0xC`, the
  same kind the Chao use.
  - The mode functions pick motions with **0x793C40** (usercall EAX = controller, EDX = motion
    id; 38 calls in 0x5C7000..0x5D4800, e.g. Hot Shot 0x5C7D67 id 0, 0x5C7DEE id 1, 0x5C7EAC
    id 2).
  - **0x7938D0** (ESI = controller) plays the motion every frame. It's called from the main
    after the mode switch, and alone while paused (0x174AFF9).
  - Controller +4 is the motion now, +8 the next one, +0x48 the table (`garden.rs` reads the
    same fields for Chao).
- **What follows from that:** a follower could play the leader's motion by calling 0x793C40 when
  the leader's +4 differs (after checking the id against the table, as `garden.rs`
  `motion_ok` does).

**Modes.**
- **Big Foot (main 0x5CFE40):**
  - Mode functions: 1 0x5CCB30, 2 0x5CCD40, 3 0x5CD1E0 (cursor), 4 0x5CD650, 5 0x5CD800
    (missiles), 6 0x5CE150, 8 0x5CDF60, 9 0x5CE360 and 12 0x5CF7C0 (hurt), 10 0x5CE540
    (dying), 13 0x5CF180 (falls).
  - Unhandled: 7 and 11. Hits skip modes 9 and 10.
- **Hot Shot:** unhandled 7, 11 and 13 (above).
- **Flying Dog (0x5D35E0):** not decompiled yet.

**Missiles.**
- **0x5D6320** (EAX = parent object, stack: &launch point, &a vector of the caller's, float speed):
  - makes a child of the boss (0x470C00, main 0x5D6A30, display 0x5D6AA0);
  - keeps both pointers (Data2 +0x34 / +0x38).
- **Launch points:** tables 0x1A27CD4.. / 0x1A27D7C.., filled each frame by Hot Shot's node
  draw callback 0x5CB840 (0x426CC0 on model nodes 0x1133D20..).
- **Target:** the missile flies from that node toward what the mode's cursor (0x5D4B70) locked
  on to, which is that game's player 1.

**Why it still isn't built:**
1. **A replayed attack is real, and real attacks hit whoever stands in them.** The leader's boss
   only ever targets the leader (its player 1). Replaying the leader's missiles on a follower
   means real missiles flying at the leader's puppet; they hurt the follower if they're in the
   way. Never hurting us from a friend-targeted attack takes a draw-only copy of each attack
   (missile, energy-gun beam/bomb/torus/pillar, flame), as `lockon.rs` does for lasers: per
   attack, find its display and the fields it reads.
2. **Frozen modes leave the children unchecked.** With the modes frozen, it isn't checked yet
   whether the children made at load (gun arms 0x5C7370/0x5CC640/0x5D10B0, pits 0x5D48D0) read
   the boss's mode and act on it locally. Each needs reading before a follower can run with a
   copied mode.
3. **Only the leader reacts to hits.** A follower's hits lower the shared HP, but the leader's
   boss only changes mode for its own hits (the hit handler is inlined in each main). So nobody
   would see a follower's hit land. Mirroring the hurt reaction means setting the leader's
   mode 14 / Big Foot 12 (or the guarded 10/9) with sub-mode, flash and timer reset, as each
   main does. That's per boss and needs testing.

**Other bosses:**
- **King Boom Boo (63):** its mode code (0x60CFF0 area) isn't decompiled. The fight hinges on the
  light switches (synced as SET switches) and the boss's vulnerable state. Same pattern
  needed: main, motion controller, attacks.
- **Egg Golem (64 / 67):** HP at `*(0x1A5A580)`+0x14. Not decompiled beyond the hit sites
  (0x4BFB3F / 0x4AA01F). Its falling platforms are separate objects; check whether they're SET
  records (then kill sync covers them).
- **Biolizard / Final Hazard (65 / 66):** multi-part bosses (the life-support pod, the spikes)
  with fractional HP hits (0x51E760, 0x4A00E5). Each part is its own object and attack.
  Following needs every part's pose plus a draw-only copy of each attack. Keep shared HP only.
- **Rival fights (19, 20, 29, 33, 42):** the rival is a real AI player 2 (`MainCharObj1[1]`,
  life `CharObj2[1]`+0x48). Following it means turning its AI off and driving player 2's
  pose/motion from the leader's rival (like a puppet, but a real player object, so it keeps its
  collision). Its attacks are player-2 moves, so they hurt player 1, which is us on the
  followers. Needs its own RE of the player-2 AI switch.

### Choosing the boss leader (requirements for when following is built)
- **Co-op rooms only**, like the shared HP.
- **Who can lead:** only room players in the same stage *and* mission who have the boss loaded
  (fighting it) and were heard recently (fresh within about 1.5 s, like keeper.rs's
  `KEEPER_FRESH`). A room player on a menu, in another stage, or still loading never leads.
- **How a leader is chosen:** keeper.rs's election (`keeper::decide`):
  - **An explicit claim with a term, and sticky:** a newcomer doesn't take over.
  - **After a quiet moment, the lowest id claims;** later, anyone.
  - **Between two claims,** the higher term wins, then the lower id.
- **Handoff:** the next player claims after a short timeout when the leader:
  - disconnects;
  - leaves the stage;
  - dies or restarts (which resets their boss);
  - pauses or tabs out (they stop sending).

  The new leader's boss resumes from the state it was following (mode, sub-mode, timer,
  position, motion), not from the start. The old leader follows when it comes back.
- **One player fighting:** they lead, which is the normal game.
- **No fresh leader:** every game runs its own boss, as now, instead of freezing.

## Boss following (2026-10-18, src/boss_follow.rs; Hot Shot and Big Foot; not tested in game)
- **Freezing:**
  - A boss runs "frozen" when its main runs with Data1[0] set to a mode its switch skips (7
    for both). The head turn, motion (0x7938D0), hit code, collision and parts still run; only
    the mode function doesn't.
  - All of these bosses' attacks come from their mode functions:
    - the flame helpers 0x5C7680 / 0x5CC6C0 are called only from mode functions;
    - the gun (0x5D5FD0) is a data block, not an object;
    - the pit (0x5D48D0, main 0x5D4A80) only does math and collision.

    So a frozen boss fires nothing.
- **The leader:**
  - Its boss runs as usual while it's after the leader.
  - On each new hunting mode (not the intro, a hurt mode or dying), it picks the next fighter
    in turn.
  - **While it's after a friend:** its main runs frozen. Then that mode's function is called by
    us with player 1 (`PLAYERS[0]`) pointing at a static copy of our EntityData1 moved to the
    friend.
    - **How the function is called:** the main calls the mode functions three ways: object on
      the stack, in EAX, or in EBX (0x5CB53C.., 0x5D0094..). We pass all three, plus ESI =
      motion controller and EDI = Data1 as the main has them.
    - **Why the order changes nothing that matters:** the mode function then runs after the
      motion and hit code instead of before (one frame later for the motion). The intro, the
      only mode function that writes player 1 (0x5CCC97), never runs this way.
- **Followers:**
  - **When it's after someone else:** frozen, taking the leader's state (mode bytes, flags,
    timer, position eased, rotation, motion via 0x793C40 after a table check).
  - **When it's after us:** its own main runs. Each mode it takes from the leader starts from
    sub-mode 0, because a mode's first step makes what the rest of it uses (cursor objects,
    the gun), so it never starts mid-mode. Its attacks are real and aimed at us.
- **Hits:**
  - **Undone when its own code would ignore them:** with the boss frozen, the hit code sees mode
    7 and takes every hit. A hit in a mode where the boss's own code ignores hits (Hot Shot
    10/12, Big Foot 9/10, dying, the intro) is undone (HP and mode put back).
  - **A follower's hit:** sent as BossHit. The leader sets the hurt mode (Hot Shot 14 / guarded
    10, Big Foot 12 / guarded 9) with sub-mode, counter and timer cleared, as the inlined hit
    code does. That's skipped in Hot Shot's mode 9, whose hit first tidies its energy gun
    (0x5D7B50), and while it's flashing (flags & 4).
  - **A follower's last hit** (its hit code picks a dying mode) releases its boss to die there.
- **Handoff:** when following ends (we lead, or nobody does), ours carries on in the mode it was
  following, from sub-mode 0 (the intro or a dying mode become mode 2, where the intro hands
  over).
- **Missing:**
  - **Attacks aimed at a friend aren't shown.** Nothing is fired at a friend's puppet either.
    Drawn-only copies of the missile (main 0x5D6A30, display 0x5D6AA0), the energy-gun pieces
    (0x5D9A70 beam, 0x5D9BA0 bomb, 0x5D9F40 torus, 0x5DA280 pillar), the gun fire 0x5D8B20 and
    the flame are next: per attack, its display and the fields it reads.
  - **The head turn** follows each game's own player 1.
  - **Flying Dog** isn't followed: its main calls the flame helper itself (0x5D1320) and isn't
    decompiled.
  - **King Boom Boo, the Egg Golems, Biolizard / Final Hazard and the rivals:** as above.

### Attacks drawn on followers, and Flying Dog (2026-10-18)
- **The displays and what they read:** each display reads only its object's Data1, at offsets
  0..0x2B (never its collision pointer +0x2C), plus static models and textures that load with
  the boss stage:
  - missile: display 0x5D6AA0 in slot +0x14 (+0, rotation +8..+0x10, position +0x14..);
  - energy-gun beam: 0x5D9890 in +0x1C (+6, +8..+0x20);
  - energy-gun bomb: 0x5D9CA0 in +0x14 (+0x14..+0x28);
  - energy-gun pillar: 0x5DA2D0 in +0x14 (same);
  - missile smoke: 0x5D6BA0 in +0x1C;
  - flame (0x5D5510's child, main 0x5D56B0): 0x5D5730 in +0x1C (+0x14..+0x28).
- **How a follower draws them:** the leader finds its attacks by those display pointers, in
  object list 3 and two levels under the boss (children circular through +0). It sends their
  Data1 bytes with a serial and the target at first sighting, at 10 Hz. A follower keeps one
  object of its own per attack fired at someone else:
  - Data1 from the game's allocator, the display set, our update moving it on between updates,
    and our delete callback;
  - no update or collision of the game's, so it never hits.
- **Not mirrored:**
  - Gun fire 0x5D8B20 and the e-torus 0x5D9F40: no display found next to their creation.
  - Flying Dog's flames: its main makes them itself every frame (0x5D1320 → 0x5D5510) as part
    of its body. A frozen follower keeps making its own where the leader has it, so the
    leader's aren't sent.
- **Flying Dog (main 0x5D35E0):**
  - Mode functions: 1 0x5D1800, 2 0x5D1A30, 3 0x5D1AC0, 4 0x5D1EE0, 5 0x5D2060, 6 0x5D2650,
    8 0x5D2440, 9 0x5D33C0, 10 0x5D2780 (hurt), 11 0x5D2E90 (dying). Unhandled: 7.
  - Calling conventions: mode 6 takes the object in EDI (0x5D3868 `mov edi, ebx`), mode 2 in
    EAX, mode 4 in EBX, the rest on the stack.
  - Hits: the whole check skips mode 11, HP is skipped in 10/11, and every hit takes mode 10
    (no guarded variant). On a hit it also deletes its Data2(+0x10)+0x38/+0x48 children.
- **Not done:** the head turn. The twist entries at `*0x1A55874`+0x24 (0x2B of 0x30 bytes,
  types 4/6) are computed in each main from player 1, and that part of the main also runs the
  hit code (0x46C340 hurts player idx 0 through both the EntityData1 and CharObj2 arrays). So
  it can't run with a stand-in player 1, and the body's facing (copied) is what turns to the
  target.
