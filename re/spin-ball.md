# Spin ball / jump aura (Sonic family) — 2026-10-14

Found with objdump and Ghidra; confirmed in game (puppets draw it).

## Summary
The blue spin ball is **not drawn by Sonic's display**. It is a separate short-lived effect object,
**JumpAura**, which Sonic's main spawns **every frame while `EntityData1.Action == 6` (Jump)**.
Each ball is drawn at the position it was spawned at and shrinks over 3 frames. The
newest ball surrounds the player and the 1–2 older, smaller balls left behind make up the "trail".
There is no separate trail effect for the jump.
The `Status & 0x100` / AnimInfo[30] model swap at 0x72060E is unrelated (see characters.md).

## Spawn (Sonic main 0x717xxx, action switch at 0x717C75)
`switch (EntityData1.Action)` via table 0x7197B4 (`cmp eax,0x57; jmp [eax*4+0x7197B4]`).
- **Action 6 (Jump)** → case 0x718544: after the physics calls, `obj = 0x755EA0(EDI = main 0x756AA0)`,
  `data->+1 = player index`, `0x756AE0(EDI = obj)` (init), `data->+2 (age) = 0`.
- **Action 69 (BounceDown, bounce bracelet)** → case 0x7185C9: same, but main = 0x756A60 (longer trail).
- (Action 7 case 0x718639 does not spawn one.)
- 0x755EA0 = `LoadObject(list 3, "exec", main = EDI)` and allocates a 0x24-byte data block at obj+0x40.

## Effect data (obj+0x40, 0x24 bytes)
| off | meaning |
|---|---|
| +0x01 | player index (s8) |
| +0x02 | age (s16, action-6 variant) |
| +0x04..+0x0C | world position (copy of `EntityData1->Collision(+0x2C)->CollisionArray(+0xC)[0].center (+8)`) |
| +0x10 | NJS_OBJECT* model |
| +0x14 | one-entry NJS_TEXLIST: +0x14 = `&textures[0 or 1]`, +0x18 = count 1 |
| +0x1C | scale (float), starts at 1.2 (0xC73C10) |

## Init 0x756AE0 (EDI = object)
Copies the collision center, sets count = 1, scale = 1.2, then chooses the model and texlist by `co2 = 0x1DE9600[idx]`:
| condition | model (ModelTable slot) | texlist (static NJS_TEXLIST) |
|---|---|---|
| `co2+3 == 8` (Amy) | `*0x1DE96BC` = model 19 (SONIC_JUMPEFFECT) | 0xA08F94 (AMY_EFFTEX) |
| `co2+3 == 0xC` | `*0x1DEA3C4` = model 436 (Metal's jump effect) | 0xA0917C (S_EFFTEX loaded for Metal) |
| else `co2+1 == 0` (Sonic) | `*0x1DE96BC` = model 19 | **0xA08B94 (S_EFFTEX)** |
| else (Shadow) | `*0x1DE98C4` = model 84 (SHADOW_JUMPEFFECT) | 0xA08D94 (SH_EFFTEX) |
(ModelTable entry = 0x1DE9620 + 8*i, model pointer at +4, so slot i is at 0x1DE9624 + 8*i.)
The texture is random: `rand() (0x7A89D8) / 32768 > 0.5` → `textures + 0xC` (entry 1, "s_ball_b"), else entry 0 ("s_ball_a").
S_EFFTEX textures array 0xA089C0 (39 entries: s_ball_a, s_ball_b, s_dash01, ...).
Finally sets **obj+0x1C (delayed/translucent display) = 0x756960**.
(In the header, `co2+3` values 8 and 0xC select Amy and Metal Sonic. characters.md lists Metal Sonic as 9, so the 0xC case is unresolved.)

## Lifetime
- main 0x756AA0 (Jump): `if age > 0: scale -= 0.3`; `age++`; once the old age is above 2, sets main = FreeObject 0x46F720 and +0x1C = 0.
  The ball is drawn at scales **1.2, 0.9, 0.6** on three consecutive frames. At any moment there are 3 balls:
  1.2 at the current position, 0.9 at last frame's position, 0.6 at the position from 2 frames ago.
- main 0x756A60 (Bounce): `scale -= 0.1` each frame and the ball is killed at ≤ 0 (about 11 visible balls).

## Display 0x756960 (JumpAura_Display, cdecl(obj), placed at obj+0x1C)
```
if (0x1DEA6C0[data+1] == 0) return;               // player gone
*(u8*)0x25EFFCC = 0; 0x42A870();                   // fog colour -> 0 (black) while drawing
*(*(u32*)0x2670544 + 0x20) = &data->texlist (+0x14) // one-texture texlist
push matrix (copy 0x30 bytes, bump *0x1A557FC)
0x427470 translate(EAX = cur matrix; stack x,y,z = data+4..+0xC)   // world pos, no rotation
multiply by diag(s,s,s) (inline 0x426E40; same as njScale 0x427750(s,s,s) cdecl on current matrix)
0x42D340(); 0x42EB30(model, 0x42D650)              // == DrawObject 0x42E730(model), cdecl
pop matrix
*(u8*)0x25EFFCC = 1; 0x42A870();                   // restore fog
```
No rotation is applied. The translucency comes from the model's own material (JUMPEFFECT chunk model) and the
delayed display pass. 0x42A870: `if (!*0x1A55808) 0x420510(flag ? *0x193411C : 0, *0x19341C4, *0x19341C8, *0x19341C0)`.

## Ball position = collision center
Sonic's 0x71E3D0 (Knuckles 0x72D9B0 likewise) sets `CollisionArray[0].center = pos + Rz(rot.z)·Rx(rot.x)·Ry(-rot.y)·(0, co2+0x140, 0)`
(via 0x468E70: push unit matrix, rotZ, rotX, rotY(-y), apply to vector), then adds the position.
In action 0x48 the height is multiplied by the double at 0x904DD0.
`co2+0x140` = physics table `0x17391E8 + 0x84*CharID + 0x80`: **Sonic/Shadow/Amy/Metal 5.4**, Tails 4.5, Knux/Rouge 5.7, Eggman 10, mechs 15.
Y rotation does not affect (0,h,0), so: center = pos + RotZ·RotX·(0, 5.4, 0).

## Texture loading
S_EFFTEX is loaded only by the character loaders: Sonic 0x717081 → `LoadSonEffTex` 0x755DA0, Amy 0x7172FA → 0x755E20,
Shadow 0x7175E1 → 0x755DE0, Metal 0x7177DC → 0x755E60. 0x755DA0 (cdecl, no args) is idempotent
(guarded by `*0x1AF018C`). It calls 0x44C350 (ECX = "S_EFFTEX", EDX = 0xA08B94) and creates a "ManTex" object (list 0)
whose delete 0x755D20 releases the texlist (0x77F9F0) and clears 0x1AF018C, so it is freed with the stage's objects.
So: **with a local Sonic, S_EFFTEX and model 19 are already loaded**. With any other local character, call 0x755DA0
yourself (once per stage, after the stage objects exist) and make sure ModelTable slot 19 is filled. Sonic's model file
from 0x459590 supplies it. Check `*0x1DE96BC != 0` and texture entry 0's texaddr (`*(0xA089C0+8)`) != 0 before drawing.

## 0x725C70 is not the trail
It runs at the end of Sonic's display and **positions a held object**. `co2+0x94` = held object, and 0x45A2E0 & 0x200 = "holding".
It writes the held object's Data1 position from the hand bone positions co2+0x1E8/+0x1F4 and the hand vectors
co2+0x284/+0x290 (0x77FC30 = vector length, 0x77F3A0 = normalize). Anim 0x31–0x34/0x49 use a different formula. It does not draw anything.

## Related effects (same family, not needed for the jump)
SpinDashAura 0x756040 (init 0x7562A0, spawned in the spin-charge action at 0x718022), HomingAura 0x756520,
JumpDashAura 0x756C90 / 0x757040 (spawned from 0x720E1E). All use the same JUMPEFFECT model and EFFTEX.

## Recommended puppet implementation
The puppet needs the remote Action (or a "jump aura" bit = `Action == 6`, optionally a bounce bit = `Action == 69`).
Anim 65/66/100 + Status 0x400 is only an approximation.
Per puppet, keep the last 3 ball centers (world) with a random texture index (0/1) per frame, and draw the ball scales 1.2/0.9/0.6
(newest first) while the aura bit was set on that frame. Draw from the puppet's delayed display (obj+0x1C) if possible,
otherwise at the end of the normal display, **after popping the puppet's own matrix** (the ball uses world coordinates and no rotation):
```
static TL: [u32;2];                       // NJS_TEXLIST {textures, count}
TL = [*(0xA08B94 as *u32) + 12*tex, 1];   // Sonic S_EFFTEX; Shadow: 0xA08D94 + model *0x1DE98C4
*(*(0x2670544 as *u32) + 0x20) = &TL;
*(0x25EFFCC as *u8) = 0; call 0x42A870;   // optional (fog)
push matrix; 0x427470(EAX=*0x1A557FC, cx,cy,cz) [caller pops 12]
0x427750(s,s,s) cdecl [caller pops 12]
0x42E730(*(0x1DE96BC as *NJS_OBJECT)) cdecl [caller pops 4]
pop matrix
*(0x25EFFCC as *u8) = 1; call 0x42A870;
```
center = remote pos + RotZ(rot.z)·RotX(rot.x)·(0, 5.4, 0). Alternatively, inside the puppet's rotated matrix: translate(0, 5.4, 0) and then scale(1.2).
An alternative that may be simpler: spawn the game's own effect with `0x755EA0(EDI=0x756AA0)` + init. This **does not work** directly for puppets,
because the init reads player index → `0x1DEA6C0[i]` collision and `0x1DE9600[i]` co2, and the display requires `0x1DEA6C0[i] != 0`. You can instead
fill the 0x24-byte data yourself after 0x755EA0 (set +1 = local player index 0 so the "player exists" check passes, +2 = 0, +4..+C = center,
+0x10 = model, +0x14/+0x18 = texlist, +0x1C = 1.2) and set obj+0x1C = 0x756960. The game's main then makes the trail by itself.
EDI is a register argument, so this needs a small asm stub.
