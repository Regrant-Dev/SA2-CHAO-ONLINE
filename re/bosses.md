# Bosses: HP and the boss gauge (for shared boss HP in co-op)

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
