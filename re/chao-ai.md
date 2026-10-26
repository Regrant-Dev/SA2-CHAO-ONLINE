# Chao AI (garden)

What a garden Chao thinks and does; the server's Chao simulation (`src/chao_sim/`) follows it.
Addresses are for the dumped exe (sha256 d1ac8ed9…). Builds on `chao.md`, `chao-garden-sync.md`,
`chao-garden-items.md` and `chao-details.md` (cited, not repeated). Scope: garden Chao only (not
races, karate, kindergarten classes, evolution).

Most behaviour routines have no Ghidra function object (`batch_decompile` reports "No procedure
exists"), so they are decoded from raw bytes (REA `read_bytes`); constants are read the same way.

Notation: `obj` ObjectMaster, `D` = Data1 (`obj+0x34`, 0x1D74 bytes), `W` = MOVE_WORK (`obj+0x38`),
`rec` = save record (`D+0x5C`). BAMS: 0x10000 = 360°. Garden id `*0x134046C` (1 Neutral, 2 Hero, 3 Dark).

Contents: 0 groundwork · 1 update and picker · 2A/2B behaviours · 3 emotions and needs ·
4 stats, aging, personality · 5 movement · 6 interactions · behaviour table · unknowns ·
7 perception.

## 0. Groundwork

An index of the fields and helpers. The later sections give the exact behaviour.

### Fields the AI uses

| Offset | Type | Meaning | Evidence |
|---|---|---|---|
| D+0x4 | u16 | Status: 0x8000 held; 0x4 hit pending (cleared by hit check); 0x1 on ground (§5.10); `|= 0x240` on every behaviour change | `0x566720`, `0x563D30`, `0x53DF90`, `0x53D760` |
| D+0xC | int | rotation Y (BAMS), used by the view cone | `0x590590` |
| D+0x14..0x1C | f32×3 | position | `0x5637B0`, `0x53DA80` |
| D+0x38 | ptr | book object (deleted on behaviour change) | `0x53D760` |
| D+0x40 | ptr | signal-collision task made by `0x55A920` (deleted on behaviour change) | `0x55A920`, `0x53D760` |
| D+0xA0 | u32 | flags: 0x10 AI on; 0x2000 frozen-think path; 0x8 collision (set by `0x53D760`); 0x2; 0x400/0x800 hand items | `0x53DC4E`, `0x53DC56`, `0x53D760`, `0x567710` |
| D+0xAC | struct | MotionTable; `+0` bit 1 = end-of-motion pulse polled by behaviours (`D+0xAC & 1`) | `0x54EF10`, `chao-garden-sync.md` §2 |
| D+0x1B4 | u16 | water flags: bit 1 entered water (latch), bit 4 in water this frame | `0x561630` |
| D+0x1B6 | u16 | behaviour mode (sub-state), 0 on set/pop | `0x53D890`, `0x53D9B0` |
| D+0x1B8 | u16 | behaviour scratch, 0 on set/pop | `0x53D890` |
| D+0x1C0 | int | behaviour counter/timer, 0 on set/pop | `0x53D890`, `0x565BE0` |
| D+0x1C4 | int | behaviour counter, 0 on set/pop | `0x53D890`, `0x561AF0` |
| D+0x1C8 | int | 3600 on set/pop; also used as a timer by `0x567830` | `0x53D890`, `0x567830` |
| D+0x1CC | u16 | forced-behaviour id; picker only runs while 0 | `0x53DD04`, `0x562C50` |
| D+0x1CE | u16 | cleared with 0x1CC | `0x562A80` |
| D+0x1D4..0x210 | int[16] | think timers, −1 per frame while ≠ 0 | `0x562C00` |
| D+0x1D4 | int | lonely branch B gate | `0x5A04A0` |
| D+0x1DC | int | lonely branch A gate | `0x5A04A0` |
| D+0x1F4 | int | toy cooldown | `0x599B60`, `0x598F40` |
| D+0x20C | int | hunger branch gate | `0x59EB00` |
| D+0x210 | int | drowsy cooldown | `0x54E970` |
| D+0x214, 0x218 | int | zeroed at init, not decremented | `0x53DA80` |
| D+0x41C | int | behaviour parameter (song id etc.), 0 on set | `0x53D890`, `0x5AB5D0` |
| D+0x420 | f32 | 80.0 at init | `0x53DA80` |
| D+0x424..0x42C | f32×3 | spawn position at init | `0x53DA80` |
| D+0x430 | fn | previous behaviour | `0x53D890`, `0x53D9B0` |
| D+0x434 | int | stack count | `0x53D890`, `0x53D970` |
| D+0x438 | int | current index | `0x53DC64` |
| D+0x43C | fn[16] | behaviour stack | `0x53D890` |
| D+0x47C | int[16] | per-entry frame limit (−1 none) | `0x53DCD2` |
| D+0x6EE/0x6F0/0x6F4 | | emotion-ball overrides cleared on behaviour change | `0x53D760` |
| D+0x7CC | f32 | perception range | `0x590590` |
| D+0x7D4 | int | perception half-angle (BAMS) | `0x590590` |
| D+0x7E0 + k*0x398 | list | perception list k: `+0` u16 count, `+4` int found, `+0x10` f32 nearest dist (init 100000.0), `+0x14` s16 nearest idx (−1), `+0x18 + i*0x1C` (32 entries): `+8` f32 dist, `+0xC` 1, `+0x18` ALW entry | `0x590520`, `0x590680` |
| list 0 @0x7E0 | | player 1 (`*0x1DEA6E0`) | `0x590590`, `0x590CE0` |
| list 1 @0xB78 | | Chao (ALW cat 0, excluding self) | `0x590680` |
| list 2 @0xF10 | | fruit (cat 3) | `0x5907C0`, `0x59EB00` |
| list 3 @0x12A8 | | trees (cats 4 and 5) | `0x590900` |
| list 4 @0x1640 | | toys (cat 6) | `0x590B30`, `0x590D20` |
| list 5 @0x19D8 | | only cleared | `0x590C70` |
| W+0x0..0x8 | f32×3 | velocity | `0x563D30`, `0x566720` |
| W+0x40 | u16 | physics flags: 0x400 on ground, 0x4000 wall hit, 0x4800 | `0x54AC20`, `0x561AF0`, `0x562EB0` |
| W+0x54..0x5C | f32×3 | behaviour target position | `0x5691B0`, `0x562D20` |
| W+0xDC | f32 | water surface Y for this area | `0x561630` |
| rec+0x38..0x44 | u16 | stat points; AI reads 0x38 swim, 0x3A, 0x3C, 0x44; ×3 outside areas 6/10/11 | `0x562330`, `0x5A3000`, `0x5A3650`, `0x567830` |
| rec+0x80 | u8 | type: 2 child; adults 5 normal, 8 swim, 11 fly, 14 run, 17 power, 20 Chaos (+1 hero, +2 dark; `0x568630`); 7,10,13,16,19,22 dark adults; 20..25 excluded from mating | `0x562800`, `0x5A1240`, `0x562950` |
| rec+0x8A | u16 | lifespan (0 → death) | `0x562A20` |
| rec+0xB0 | f32 | alignment | `0x59B900` |
| rec+0xC0 | f32 | evolution progress (> 1.0 → cocoon) | `0x562800` |
| rec+0x118 | u32 | animals learned → animal play `0x11C4BE0` | `0x5A2F10` |
| rec+0x12A | u16 | need-period counter of the life tick; behaviours also use it as a once-per-period latch (§3.1) | `0x535957`, `0x536420`, `0x5A3000` |
| rec+0x12C..0x133 | u8[8] | emotion bytes, index 0..7 of `0x54E930` | `0x54E930`, #16–#21 |
| rec+0x134..0x148 | u16[11] | needs SLEEPY..THIRSTY, index 8..18 of `0x54E930` | `0x54E930` |
| rec+0x14A.. | i8 | personality, index 19+ of `0x54E930`; AI reads 0x14A, 0x14B, 0x14C, 0x155 | `0x5626D0`, `0x5A1240`, `0x59F860`, `0x5A0B10` |
| rec+0x15A..0x15F | i8[6] | condition slots; < 0 makes illness behaviour `0x11C9BF4[i]` possible | `0x599CB0` |
| rec+0x160 | u8 | song bits 0..4 | `0x5AB5D0` |
| rec+0x161 | u8 | skill set B bits 0..4 → `0x11C3F44` | `0x59C6D0`, `0x59C3D0` |
| rec+0x162 | u8 | skill set C levels → `0x11C9BD4` | `0x59B900` |
| rec+0x163 | u8 | skill set A bits 0..7 → `0x11C9BB4` | `0x59D410`, `0x59D0D0` |
| rec+0x164 | u16 | toy skills bits 0..11 → `0x12FE978`; bit 0x100 song | `0x55E7A0`, `0x5AB5D0` |
| rec+0x16C + c*6 | bond | `+0` i8 like, `+1` u8 fear (0..200, `0x53E120`/`0x53E1F0`), `+2` u16, `+4` u16; c = `0x53E0C0` | `0x5637B0`, `0x5A04A0` |

### Helper functions

- `0x53DC40`: AI tick (interrupts, behaviour, timers, picker)
- `0x53DB10`: interrupt checks in fixed order (§1.4)
- `0x53D890`: AL_SetBehavior(EAX=obj; push timer, fn); clears stack, resets mode/timers, mailbox 3 to partner, calls `0x53D760`
- `0x53D970`: push follow-up (EAX=obj, EDX=fn), max 15 entries
- `0x53D9B0`: pop; returns non-zero if another entry exists
- `0x53D760`: per-change cleanup (face reset, attached models hidden, `D+0x38`/`D+0x40` objects deleted, collision re-enabled)
- `0x53DA80`: AI init from CreateChao (`0x5504DB`)
- `0x562C00`: think timers `D+0x1D4..0x210` −1 per frame
- `0x562A80`: picker (candidate list §1.5)
- `0x562C50`: forced behaviour by `D+0x1CC` (debug table)
- `0x590C70`: perception refresh (only from the picker); `0x590520` clears a list; `0x590590` player; `0x590680` Chao; `0x5907C0` fruit; `0x590900` trees; `0x590B30` toys
- `0x590CE0`: returns player obj (`*0x1DEA6E0`) if perceived; `0x590D20`: nearest perceived toy object
- `0x562440`: f16(v,t) = (v−t)/(10000−t)
- `0x562490`: f8(v,t) = (v−t)/(200−t)
- `0x5624D0`: score jitter s*(0.4+0.6r), ESI=&s
- `0x54E930`: generic emotion getter (EAX=obj, ECX=index: 0..7 u8 @0x12C+i, 8..18 u16 @0x124+2i, ≥19 i8 @0x137+i)
- `0x7A89D8`: rand() 0..32767; doubles `0x904E90`=1/32768, `0x904E98`=0.5, `0x9050A8`=30000.0, `0x905A20`=−121.0
- `0x8287C0`: float→int conversion used after rand
- `0x77FBD0`: distance (EAX=&a, ECX=&b) → ST0; `0x77FC30`: vector length
- `0x859B6A`: atan2-style angle (perception bearing); `0x446960`: angle approach
- `0x42AAB0` / `0x42AC30`: sin/cos of BAMS angle in ECX (velocity from facing)
- `0x796910(rate)`: turn/move toward `W+0x54`; `0x796AB0(rate)`: same toward player (used by whistle/lonely)
- `0x486760(obj, kind)`: cdecl, touch/collision hit of `kind` (+4 = other obj)
- `0x486880(obj, 1, kind)`: cdecl, nearby signal of `kind` (globals `0x19468A0..AC` reset before each call); `0x54B1F0(kind)` wraps it and returns the object
- `0x486800`: attacker lookup for the hit check
- `0x55A920(obj, kind, f32 radius; EDI=&pos, EBX=frames)`: emits a signal-collision task stored at `D+0x40`
- `0x54AC20` (EAX=obj): standing still on ground (W+0x40&0x400 and |v_xz| < 0.04 `0x16048F4`)
- `0x5319F0` (ESI=garden): number of non-empty save slots in that garden
- `0x53E0C0`: player character → bond index (1..5, else 0)
- `0x535010`, `0x535390`, `0x535360`: garden/alignment predicates (seen in choosers)
- `0x534F80`: picks a wander destination into EDI from `*0x1A1693C` list
- `0x5A3650`: installs walk variant by run points; `0x5A3000`: walk step (arrival < 6 units, wall/jump/climb)
- `0x561630`: water check; `0x561920`/`0x561AF0`: swim movement helpers
- `0x565DC0`: held update/release; `0x565B90`: close ParamFukidasi balloon over a Chao
- `0x530640` link, `0x530690` unlink, `0x5304F0` set target, `0x5304D0` target check (ALW, see chao-garden-items.md §3.1)
- `0x793C40` / `0x793D30` / `0x793E40`: motion set (see chao-garden-sync.md §2); `0x53A4B0` eyes; `0x53A5A0` mouth; `0x53A600` face variant; `0x53D660`: emotion-ball icon (dl=icon, push frames)
- `0x4372E0` / `0x437590`: positional sound (EDI id, ESI &pos); played only when `*0x134046C == *0x134062C`
- `0x5353C0` / `0x5363D0`: life/emotion tick (chao-details.md, chao-garden-sync.md), not part of `0x53DC40`


## 1. Per-frame update and behaviour picker

Notation as in `chao-garden-sync.md`: `obj` ObjectMaster, `D = obj+0x34` (Data1, 0x1D74 bytes), `W = obj+0x38` (physics work), `rec = D+0x5C` (ChaoData). `r` = `rand()/32768` (`rand` = `0x7A89D8`, 0..32767; the scale `1/32768` is the double at `0x904E90`). The MainSub order (motion, face, emotion ball, physics, collision) is already in `chao-garden-sync.md` §1. The AI is the single call to `0x53DC40` at `0x54FE49`.

All score comparisons in the picker are strict (`score > best`, FCOMPP + `TEST AH,5; JP`, for example `0x54EADB`), so a tie keeps the earlier candidate. Every "best < 1" gate is `FLD1; FCOMP best; TEST AH,0x41; JNP skip` (for example `0x562530`): the candidate runs only while best < 1.0.

### 1.1 AI tick `0x53DC40` (EAX = obj)
```
if !(D+0xA0 & 0x10) return                                   // 0x53DC4E
if (D+0xA0 & 0x2000) {                                       // "frozen think" path, 0x53DC56
    interrupts 0x53DB10                                      // 0x53DC5F
    cur = D+0x43C[D+0x438]; if cur==NULL -> pop              // 0x53DC64..0x53DC79
    ret = cur(obj)                                           // 0x53DC7E
    ret==1 -> pop; ret==3 -> AL_SetBehavior(0x565BE0,-1); anything else (incl. 2) -> nothing
    // no think timers, no D+0x47C countdown, no picker
} else {
    think timers 0x562C00                                    // 0x53DC9A
    interrupts 0x53DB10                                      // 0x53DCA1
    ret = cur ? cur(obj) : 1                                 // 0x53DCAC..0x53DCC7
    i = D+0x438 (re-read after the call)                     // 0x53DCCC
    if (D+0x47C[i] > 0) {                                    // 0x53DCD2 (signed; -1 = no limit)
        D+0x47C[i]--                                         // 0x53DCDE
        if (D+0x47C[i] == 0) -> pop, ret IGNORED             // 0x53DCFA
        // otherwise FALL THROUGH to the ret handling below
    }
    if ret==2 { if (u16 D+0x1CC == 0) picker 0x562A80;       // 0x53DCFC..0x53DD10
                0x562C50(EAX = u16 D+0x1CC, ESI = obj); return }   // 0x53DD1F
    if ret==1 -> pop                                         // 0x53DC83
    if ret==3 -> AL_SetBehavior(0x565BE0, -1)                // 0x53DD28..0x53DD36
}
pop = 0x53D9B0; if it returns 0 (no next entry) -> AL_SetBehavior(0x565BE0, -1)   // 0x53DC8E..0x53DD36
```
Return codes: 0 = keep running, 1 = done (pop), 2 = ask the picker, 3 = reset. A frame limit that is still running does not block the return code. When `D+0x47C[i] > 0` the counter is decremented and the return value is still handled. Only the frame on which the counter reaches 0 forces a pop and ignores `ret` (`0x53DCDE..0x53DCFF`).

### 1.2 Behaviour stack
| Fn | Convention | Effect |
|---|---|---|
| `AL_SetBehavior 0x53D890` | EAX=obj, push timer, push fn (cdecl, caller pops 8) | `D+0x1B6=0, +0x1B8=0, +0x1C0=0, +0x1C4=0, +0x1C8=3600` (`0x53D89B..0x53D8A9`). `D+0x430 = D+0x43C[D+0x438]`, the old current fn (`0x53D8B1..0x53D8BE`). Clears `D+0x440..0x478` (stack slots 1..15, `0x53D8C8..0x53D91C`); `D+0x47C[1..15]` are NOT cleared. Sets `D+0x43C[0]=fn`, `D+0x434=1`, `D+0x438=0`, `D+0x47C[0]=timer`, `D+0x41C=0` (`0x53D922..0x53D942`). If the ALW entry `obj+0x3C` has a partner `entry+0x40` whose own `+0x40 == entry`, it writes u16 3 to the partner's `+0x34` (`0x53D948..0x53D960`). Then it tail-jumps to `0x53D760` (`0x53D965`). |
| `0x53D760` (cleanup on every set, not on pop) | EAX=obj | `D+0x50=0`; `0x566AF0(D+0x510)`; `D+0x65C = D+0x66C = 0.0f` (`0x8AF004`). If the new `D+0x43C[0]` != `D+0x430`, resets the eyes `0x53A4B0(EAX=-1, ECX=obj, EDX=s16 D+0x694)` and the mouth `0x53A5A0(ECX=obj, EDX=-1, push s16 D+0x69E)` (`0x53D791..0x53D7C3`). Clears u16 `D+0x6EE`, `D+0x6F0`, `D+0x6F4`. Deletes `D+0x40` and `D+0x38` by setting their MainSub to `0x46F720` and zeroing the pointer. `D+0xA0 |= 8`; collision-info byte `+2 |= 7` then `&= 0x8F`; Status `D+4 |= 0x240`; collision-info `+4` and `+0x64 &= ~0x10`; `D+0xA0 |= 2` (`0x53D7DF..0x53D884`). |
| push `0x53D970` | EAX=obj, EDX=fn | if `D+0x434 < 15`: `D+0x43C[count]=fn`, `D+0x47C[count]=-1`, `count++` (`0x53D97E..0x53D99B`) |
| pop `0x53D9B0` | EAX=obj | `D+0x1B6=0, +0x1C0=0, +0x1C4=0, +0x1B8=0, +0x1C8=3600`; `D+0x430 = D+0x43C[index]`; `count--`, `index++`; returns `D+0x43C[index] != 0` (`0x53D9B8..0x53D9FA`). It does not call `0x53D760` and does not reset `D+0x41C`. |
| init `0x53DA80` | EAX=obj, called from CreateChao at `0x5504DB` | `D+0x1B6=0, +0x1C0=0, +0x1C4=0`; `D+0x420 = 80.0f` (`0x8AE6F4` = 0x42A00000); `D+0x424..0x42C = D+0x14..0x1C`; `AL_SetBehavior(0x565BE0,-1)`; zeroes `D+0x1D4..0x218` (18 dwords, `0x53DAD1..0x53DB07`) |

Quirk: push writes at `index = count`, but pop decrements count and increments index (`0x53D970` vs `0x53D9B0`). A push made after a pop therefore lands on a slot that is already in use. Many choosers inline the same push code (`0x5626D0` at `0x5627BF`, `0x59D0D0` at `0x59D268`, `0x5A3650` at `0x5A36AA`).

Current behaviour = `D+0x43C[D+0x438]` (read the same way at `0x53DB25`).

### 1.3 Think timers `0x562C00`
16 dwords `D+0x1D4..D+0x210`. Each frame: `if (t != 0) t--`, a 4×4 unrolled loop from `EAX = D+0x1D8` with `[EAX-4]` first (`0x562C03..0x562C40`). The test is `TEST; JBE`, so negative values also keep decrementing. The timers run only on the normal path. Known users:
| Timer | Use | Evidence |
|---|---|---|
| `D+0x1D4` | lonely chooser branch B allowed only when 0 | `0x5A04DD` load, test at `0x5A067B` (assembly-checked) |
| `D+0x1DC` | lonely chooser branch A allowed only when 0 | `0x5A04F2` (assembly-checked; `0x53E0C0` touches only EAX) |
| `D+0x1F4` | toy cooldown: the toy chooser needs 0. The horse ride (`0x598F40` mode 3, when `--D+0x1C0 ≤ 0`) sets it to `1800 − trunc(r·−1801)` = `1800 + trunc(r·1801)` (1800..3600, `0x906268` = −1801.0, FPU truncate), then `D+0xA0 \|= 2` and `AL_SetBehavior(0x598E90,-1)`, then returns 1, so the tick pops the fresh queue and settles (`0x565BE0`); the dismount never runs | `0x599B6A`; `0x599166..0x5991E5` |
| `D+0x20C` | when non-zero, forces the hunger chooser into its "look for fruit" branch | `0x59EB09` |
| `D+0x210` | drowsy cooldown. Rewritten on EVERY picker run, whatever the gate: `trunc(600 + 2400*0.5*((10000-SLEEPY)/10000 + r))`, giving 600..3000 frames | `0x54EA3C..0x54EACA` (`0x1312E5C`=600, `0x1312E60`=2400, `0x904E98`=0.5, `0x904F50`=10000.0, FPU control `OR 0xC00` = truncate) |

### 1.4 Interrupts `0x53DB10` (EAX=obj), every frame, before the behaviour
`cur = D+0x43C[D+0x438]` (`0x53DB25`).
1. `cur ∈ {0x568D30 cocoon, 0x568F60 death}` → no interrupts at all (`0x53DB32`, `0x53DB3E`).
2. Water `0x561630(obj)` (cdecl). If true, skip 3–4 with flag=1 (`0x53DB4B..0x53DB55`, `0x53DBC8`). Held/pet (5), hit (6) and the tail (7) still run on a water frame.
3. `cur ∈ {0x54EF10, 0x54F7A0}` (sleeping) → skip 4, flag stays 0 (`0x53DB57..0x53DB65`).
4. All of these are evaluated in order with no short circuit. Each may call `AL_SetBehavior`, so the last one that fires wins: whistle `0x5637B0` (EDX=obj), ball `0x563B70` (EDI=obj), hat `0x5691B0`, `0x567560`, fruit `0x560640`, animal `0x566BC0`, `0x564760` (ESI=obj, and EDI=obj from `0x53DB77`) (`0x53DB69..0x53DBC6`).
5. Held `0x566720`; only if it is false, pet `0x564C10` (`0x53DBCD..0x53DBE7`).
6. Hit `0x563D30` (EBX=obj) (`0x53DBF0`).
7. Each check returns 1 when it installs (always `AL_SetBehavior(fn, timer)`), else 0. If anything fired: ALW entry `+6 = 0` (u16), `+0x40 = 0`, `+0xC = -1`, then `0x530690(EAX=obj)` (`0x53DC05..0x53DC1E`). Then, unless held fired and hit did not (hit forces EDI=0 at `0x53DBF9`), it tail-jumps to `0x565B90(ECX=obj)` (`0x53DC23..0x53DC30`).

| Check | Condition | Installs | Evidence |
|---|---|---|---|
| water `0x561630` | If held (`D+4 & 0x8000`), clears `D+0x1B4` bit 0x1 and returns 0. Water surface `W+0xDC` by area `*0x134046C`: 1 → -2.2 (`0x905C50`), 2 → -1.5 (`0x9055D0`), 3 → -1.0 (`0xB087CC`), else -10000.0 (`0x905B6C`). In water when `D.y + 2.0 (0x904F78) < surface`. | First frame in water (`D+0x1B4 & 1` clear): sets bit 0x1, plays sound id `0x1020` via `0x4372E0` (ESI=&pos, push 0,0,0) if `*0x134046C == *0x134062C`, then `AL_SetBehavior(0x562330,-1)`. Every frame in water: if `W+4 < 0`, `W+4 *= 0.1` (`0x905030`); `D+0x1B4 |= 4`; returns 1. Out of water: `D+0x1B4 &= ~5`, returns 0. | `0x561643..0x56173C` |
| whistle `0x5637B0` | `*(u8*)*0x1DEA6C0 == 0x2A` and squared 3D distance (player `+0x14..0x1C` vs `D+0x14..0x1C`) < 10000.0 (`0x905664`) | `0x5634D0` only if the i8 `bond[c].like` (`rec+0x16C+c*6`, c=`0x53E0C0`) > 50; returns 1 either way | `0x5637B1..0x56381C` |
| ball `0x563B70` | `*0x1DBE574 != 0` (ball in mode 1 with speed > 0.4, `0x55D373`); TEDIOUS `rec+0x13C > 4000`; `0x77FBD0(D+0x14, (*0x1DBE570)->D+0x14) < 50.0` (`0x905048`) | TEDIOUS = clamp(TEDIOUS − 2000, 0, 10000), then `0x563830` | `0x563B70..0x563BE7` |
| hat `0x5691B0` | cur ∉ {0x5607C0, 0x54EF10, 0x569240}; `0x486760(obj,0x93)` hit with other obj (`+4`) ≠ NULL; if that obj has an ALW entry (`+0x3C`), entry `+6 & 2` (Chao-held) must be clear | `W+0x54..0x5C = other D+0x14..0x1C`; `AL_SetBehavior(0x569240,-1)`; returns 1 | `0x5691B0..0x569236` |
| `0x567560` | cur ∉ {0x54EF10, 0x5607C0, 0x567640}; `rec+0x3C` ×1 if area `*0x134046C` is 6 or 10..11, else ×3, must be ≥ u16 `*0x1312D6E` (=150, unsigned `JB`); no mutual partner (ALW `+0x40`, partner `+0x40 == entry`); `0x486760(obj,0x85)` other obj ≠ NULL; `0x5304D0(EAX=other obj) == 0`; no Chao-held test | `W+0x54..0x5C = other pos`; `AL_SetBehavior(0x567640,-1)`; returns 1 | `0x567560..0x567630`, `0x1312D6E` |
| fruit `0x560640` | cur ∉ {0x5607C0, 0x54EF10, 0x5606D0}; touch kind 0x83, other obj ≠ NULL, not Chao-held (skipped if no ALW entry) | `W+0x54..0x5C = fruit pos`; `AL_SetBehavior(0x5606D0,-1)`; returns 1 | `0x560640..0x5606C6` |
| animal `0x566BC0` | cur ∉ {0x566DF0, 0x566C40}; touch kind 0x87, other obj ≠ NULL, not Chao-held (skipped if no ALW entry) | `W+0x54..0x5C = animal pos`; `AL_SetBehavior(0x566C40,-1)`; returns 1 | `0x566BC0..0x566C3F` |
| `0x564760` | cur != 0x5647B0; `0x486760(obj,0xA2)` with other obj (`+4`) ≠ NULL | `AL_SetBehavior(0x5647B0, 500)` (no target pos); returns 1 | `0x564760..0x5647AF` |
| held `0x566720` | cur ∉ {0x566670, 0x5660A0, 0x5662B0, 0x566300, 0x566580}; Status `D+4 & 0x8000` | `W+0x1C = (*0x1DEA6C0)+0xC + D+0xC − 0xFA0` (player rotY + own rotY − 4000); `a = 0x7969B0(EAX=obj, ECX=0)`; `W+0x18 = 0xC000` if \|s16(D+0xC − a)\| < 0x4000, else `0x4000`; `W+0x0..0x14` (6 floats) = 0.0; `AL_SetBehavior(0x566670,-1)`; then `0x565B50(obj)` (cdecl); returns 1 | `0x566720..0x566806` |
| pet `0x564C10` | cur ∉ {0x5648A0, 0x54FCF0, 0x59FCA0, 0x59F970, 0x59FA50}; `D+4 & 1`; `0x54B1F0(2)` (ECX=1, EDX=obj) ≠ 0 | `AL_SetBehavior(0x5648A0,-1)`; returns 1 | `0x564C10..0x564C7C` |
| hit `0x563D30` | cur ∉ {0x563EB0, 0x568D30, 0x568F60}; `D+4 & 4`; `atk = 0x486800(obj,1)` then `+4`. `D+4 &= ~4` once the cur and flag tests pass, even when atk is NULL; nothing is cleared while cur is excluded | `n` = normalise (`0x77F3A0`) of (atk.x − x, ?, atk.z − z) (y slot never written). `W+0 = −n.x·0.8` (`0x905230`), `W+4 = 1.5` (`0x1283DDC`), `W+8 = −n.z·0.8`. `AL_SetBehavior(0x563EB0,-1)`. Hat drop if `rec+0xD5 ≠ 0`: first free slot (u16 == 0xFFFF) among 24×0x14 at `0x19F6460 + (*0x173D06C==1)·0xCA6C + 0x4D8`; none free → keeps the hat; else `0x54A540(hat, &D+0x14, random BAMS trunc(r·360·182.044), W, slot)`, `rec+0xD5 = 0`. Returns 1 | `0x563D30..0x563EAB` |

### 1.5 Picker `0x562A80` (EAX=obj); runs only when the behaviour returned 2 and `D+0x1CC == 0`
1. `best = 0.2` (`0x1312DF4`, `0x562A81`). `0x590C70` refreshes perception (EBX=obj; this is its only caller, `0x562A99`). Then `D+0x1CC = D+0x1CE = 0` (`0x562AAA`).
2. The candidates run in the fixed order of the table, each with `&best` (`0x562AB7..0x562BB4`). Each one compares `score > best` and, on success, calls `AL_SetBehavior` at once and sets `best = score`. A later winner replaces an earlier one, and a tie keeps the earlier one. Most also clear `D+0x1CC/0x1CE` again.
3. Candidates 21–26 are skipped when `rec+0x80 == 2` (child) (`0x562B6A`).
4. At the end: `if !(0.2 < best)` (that is, best ≤ 0.2) → `AL_SetBehavior(0x562D20, -1)`, then clear `D+0x1CC/0x1CE` (`0x562BB9..0x562BEE`).

Shared scoring helpers (assembly-checked):
- `f16(v,t)` = `v > t` (signed) ? `(v−t)/(10000−t)` : `0.0`, returned as a float in ST0. `0x562440` (cdecl push t, push v; 10000 from u16 `0x1312C82`).
- `f8(v,t)` = same with 200 (u16 `0x1312C80`). `0x562490`.
- jitter: `s = s*0.4 + r*(1−0.4)*s`, so `s*(0.4+0.6r)`. `0x5624D0` (ESI = &s; 0.4 at `0x1312DF0`).

Needs (u16, 0..10000) are named after the debug labels in record order `0x134..0x148`: SLEEPY, SLPDPTH, HUNGER, BREED, TEDIOUS, LONELY, TIRE, STRESS, NOURISH, CONDITN, THIRSTY (mapping by order only; the label table has no xrefs).

| # | Candidate | Gate | Score | Installs | Side effects |
|---|---|---|---|---|---|
| 1 | `0x599CB0` illness | best < 1; for each i in 0..5 with `(i8)rec[0x15A+i] < 0` | `30000*r > CONDITN + 20000`; CONDITN read by `0x54E930(ECX=0x11)` = u16 `rec+0x146`; 30000.0 = `0x9050A8` (`0x599CDC..0x599D15`) | `AL_SetBehavior(0x11C9BF4[i], -1)`, best = 0.99 (`0x109E008`). There is no compare with best inside the loop, so the last success wins. | none |
| 2 | `0x54E970` drowsy | `D+0x210 == 0` and SLEEPY > 6000 (`0x1312E64`) | `(v−6000)/2000` for v < 8000 (`0x1312E68`); `1−(v−8000)/1000` for v < 9000 (`0x1312E6C`); 0 for v ≥ 9000. Then ×0.99 (`0x1312E70`), jitter. | `0x54F980` | always rewrites `D+0x210` (§1.3) |
| 3 | `0x54EB20` deep sleep | SLPDPTH `rec+0x136` > 500 | **`1.5*f16(v,500)`**, jittered: `FLD 0.5; FMUL ST1; FADDP` = f + 0.5f (`0x54EB4A..0x54EB59`), so 0..1.5 before jitter | `0x54FCF0` | |
| 4 | `0x54EBA0` go to bed | `!(D.y < W+0xDC)` (not under the water surface) and SLEEPY > 8000 (`0x1312E84`) | `0.99*f16(v,8000)` (`0x1312E88`), jitter | `0x54EE10` | |
| 5 | `0x59EB00` hunger | none | Path choice: path A (fruit) when `D+0x20C != 0` or `r ≥ 0.6` (`0x905268`). Path B otherwise (`0x59EB09..0x59EB3E`). **A:** nearest fruit in list 2 (`D+0xF14` found, `D+0xF24` index, entry `D+0xF28 + idx*0x1C`, `+0x18` ALW, `+0x3C` obj). If the fruit's ALW `+0x40` is set and is not my entry, return. Otherwise score `0.99*f16(HUNGER,5000)` (`0x1312E54/58`), jitter. **B:** `0.85*f16(HUNGER,5500)` (`0x1312E48/44`), jitter. Then, separately, if HUNGER > 9900 (`0x26AC`): score = **jitter(1.0) = 0.4..1.0** (`FLD1` + `0x5624D0`, `0x59EBAD..0x59EBB7`), compared again with best. | A: fruit held (`fruit D+4 & 0x8000`) → `0x59E8C0` + push `0x59EA00` (`0x59EC89..0x59EC9F`); else `0x5614D0` (`0x59ECD2`). B: `0x59ED60` (`0x59EB78`); HUNGER > 9900 → `0x59EF40` (`0x59EBD1`) | A with no valid fruit but a tree seen (list 3, `D+0x12AC`/`0x12BC`) computes a jittered score and installs nothing (`0x59ED01..0x59ED4A`) |
| 6 | `0x562520` thirst | best < 1; THIRSTY `rec+0x148` > 9000 (`0x1312E94`) | `0.99*f16(v,9000)` (`0x1312E98`), jitter | `0x5A1020` | |
| 7 | `0x59D0D0` join skill A | best < 1; signal 0xA5 found by `0x486880(obj,1,0xA5)` (globals `0x19468A0..AC` reset first, `0x59D0F5..0x59D11E`) | TEDIOUS v; if v > 1000 (`0x1312E8C>>1`): `v += 3000` when v < 7000; `0.99*f16(v,1000)` (`0x1312E90`), jitter | Let S = signal object (`hit+4`) and P = `S+8` (its parent). **Own skill-A bits** (`rec+0x163`): `W+0x54 = S.pos`, `0x56B500` + push `0x11C9BB4[random known bit]`, TEDIOUS −4000 and LONELY (`0x13E`) −4000 on self (clamped 0..10000), and if P exists P's LONELY −4000 (`0x59D1FB..0x59D35C`). **No bits:** `W+0x54 = P.pos` if P exists, `0x56B500` + push `0x55C430` + push `0x59E710`, no need changes (`0x59D361..0x59D3F9`). best = 0.99 either way. | see Installs |
| 8 | `0x59C3D0` join skill B | best < 1; signal 0xA6 (`0x59C3F4`) | TEDIOUS (+3000 if < 7000) `0.99*f16(v,2000)`, jitter | If P exists and P's current fn is `0x59C980/0x59CA90/0x59CB20/0x59CBF0` with own `rec+0x161` bit 2/4/8/0x10 respectively (`0x59C4A7..0x59C533`): `W+0x54 = S.pos`, `0x56B500` + push `0x11C3F44[1..4]`, TEDIOUS −4000, LONELY −4000 self, P LONELY −3000. Otherwise (also when P runs `0x59C870`): `W+0x54 = P.pos` (if P), `0x56B500` + push `0x55C430` + push (`r < 0.5` → `0x59CDA0`, else `0x59CF50`) (`0x59C66B..0x59C6B4`). best = 0.99. | |
| 9 | `0x59B710` watch skill C | best < 1; signal 0xA7 (`0x59B730`) | as #8 | `W+0x54 = P.pos` (if P), `0x56B500` + push `0x55C430` + push `0x59B3D0` (`0x59B7E3..0x59B807`); best = 0.99 | |
| 10 | `0x59B820` | best < 1; signal 0xA8 (`0x59B841`) | TEDIOUS > 2000, then `r < 0.7` (`0x9050C0`; the rand is drawn only when TEDIOUS > 2000) → 0.99, else 0 (no jitter) | `0x59B580` | |
| 11 | `0x599A10` TV | best < 1; **signal 0xC9 found** (`0x599A31..0x599A82`) | TEDIOUS (+3000 if < 7000) `0.99*f16(v,2000)`, jitter | If `*0x1AED288 != 0` and `0x55C4A0(EBX=&pos)` succeeds: `W+0x54 = pos`, `0x5304F0(EAX=obj, ECX=TV)`, `0x55C3C0` + push `0x56B480`, `0x56B560`, `0x56B6C0`, `0x598890` (`0x599AE4..0x599B47`). **Quirk:** `best = 0.99` even when the TV object is missing or `0x55C4A0` fails, with nothing installed (`0x599ACE..0x599B56`). | |
| 12 | `0x5998C0` radio | best < 1; **signal 0xCA found** (`0x5998E1`) | as #11 | `*0x1AED2E0` and `0x57C7A0`: same chain ending in `0x598B90` (`0x5999BB..0x5999F7`); same best = 0.99 quirk (`0x5999FC`) | |
| 13 | `0x599B60` toy | `D+0x1F4 == 0`; nearest toy `0x590D20` (no best < 1 gate) | `0.99*f16(TEDIOUS,2000)`, jitter (no +3000) | by the toy ALW kind `+4` (jump table `0x599C90`): 2 → `0x599210`, 3 → `0x599400`, 4/5/6 → `0x40AD80`, 7 → `0x5995F0` unless `0x535010(EAX=&toy pos) == 3` (then it returns with no change at all), 8 → `0x599770` | TEDIOUS = clamp(−5000), best = score, also for kinds outside 2..8 where nothing is installed (`0x599BD6 → 0x599C40`) |
| 14 | `0x5A10B0` learned play | best < 1; TEDIOUS > 2000 | `0.99*f16(TEDIOUS,2000)`, jitter | `r1 ≥ 0.4` (`0x904EB0`): `r2 < 0.5` → `0x55E7A0` (toy, EBX=obj), else `0x5A2F10` (animal). `r1 < 0.4`: `r2 < 0.25` (`0x8AC6C8`) → `0x59D410`; `< 0.5` → `0x59C6D0`; `< 0.75` (`0x8A77E8`) → `0x59B900`; else `0x5AB5D0` (`0x5A111C..0x5A11D7`) | only if the sub-chooser returns non-zero: TEDIOUS −5000 (clamped), best = 0.99, and `D+0x1CC/0x1CE` cleared |
| 15 | `0x5626D0` tired | best < 1; TIRE `rec+0x140` > `2000 + trunc(((p14A − p155 + 200)/400.0) * 4000)` (`0x1312EAC`=2000, `0x1312EB0`=6000, `0x905278`=400.0, truncating FPU mode) | `0.99*f16(TIRE, threshold)` (`0x1312EB4`), jitter | `0x55C430` + push `0x5A1BA0` | |
| 16 | `0x59F210` | u8 `rec+0x12F > 100` (`0x1312E08`) | Player seen (`D+0x7E4 != 0`, `D+0x7F4 ≥ 0`, `*0x1DEA6E0 != 0`): only if the fear byte u8 `bond[c]+1` (`rec+0x16D+c*6`) > 75 (`0x1312E0C`), score `0.99*f8(rec+0x12F, 75)` (`0x1312E14`), jitter; otherwise 0. Player not seen: `1 − (p14C+100)*0.005*(1−0.7)` (`0x9055A8`=0.005, `0x1312E10`=0.7), no jitter | `0x59F350` (+ push `0x59F5F0` in the player-seen branch, `0x59F2D8`) | |
| 17 | `0x59F860` | u8 `rec+0x12E > 90` (`0x1312E18`) | `1 − (p14C+100)*0.005*0.3` (0.7 at `0x1312E1C`), no jitter | `0x59F8E0` | |
| 18 | `0x5A0010` | u8 `rec+0x131 > 150` (`0x1312E24`) | `1 − (100−p14C)*0.005*0.5` (0.5 at `0x1312E28`), jitter | `0x5A00A0` | |
| 19 | `0x5A0B10` | u8 `rec+0x12C > 100` (`0x1312E30`) | `1 − (100−p155)*0.005*0.4` (0.6 at `0x1312E34`), jitter | `0x5A0BA0` | |
| 20 | `0x562800` evolve | best < 1; `rec+0x80 == 2`; **`rec+0xC0 ≥ 1.0`** (`0x1312C90`, FCOM + `TEST AH,0x41; JP`, so equality passes); it then writes `rec+0xC0 = 1.0` (before the standing check); standing still `0x54AC20` | 1.0 | `0x568D30` | clamps `rec+0xC0` to 1.0 even when not standing (`0x562834`) |
| — | candidates 21–26 run only when `rec+0x80 != 2` (`0x562B6A`) | | | | |
| 21 | `0x5A1240` dark adults | `(i8)p14B ≤ 30`; `(type−7)` unsigned ≤ 15 and divisible by 3 (types 7,10,13,16,19,22); u8 `rec+0x12D > 80` (`0x1312DF8`); no best < 1 gate | (a) player perceived (`0x590CE0`) and i8 `bond.like < 0` and i8 `p14C > 30` and `r < 0.7` (`0x9050C0`): `0.99*f8(rec+0x12D,80)`, jitter. (b) Same player/like condition, (a) not taken, and `p14C > −50` (`0xCE`): same score. (c) Otherwise (no player, like ≥ 0, or p14C ≤ −50): `1 − (100−p14C)*0.005*0.3`, no jitter. | (a) `0x5A1460` + push `0x5A18A0` (`0x5A1335..0x5A1344`); (b) `0x5A1800` (`0x5A13B3`); (c) `0x5A1460` with no follow-up (`0x5A142A`) | |
| 22 | `0x5625B0` | best < 1; signal 0xAE (`0x5625D5`) with non-null `hit+4` | THIRSTY < 5000 (`0x1312EA4`): `0.891*(5000−v)/5000` (`0x1312EA8`), jitter. THIRSTY ≥ 5000: the FPU value is the 0.0 loaded at `0x5625B3`, so the score is **0** (`0x562638 → 0x562648`) | `0x567FA0`; my ALW `+0x44` = target's ALW entry (`0x56268A..0x56269B`) | |
| 23 | `0x5A04A0` lonely | player seen (`D+0x7E4`, `D+0x7F4 ≥ 0`, `*0x1DEA6E0`) | A: `0.99*f16(LONELY,1000)` (`0x1312E74/78`); B: `0.99*f16(LONELY,5000)` (`0x1312E7C/80`); jitter | **A** (`D+0x1DC == 0`): like ≥ 0 and (u16 `bond+4` < 2 or u16 `bond+2` > 300) → `0x5A03B0` + push `0x5A0490`. Then, independently, like < −20 and `bond+2` > 300 → `0x5A1800` + push `0x5A0490`. **B** (`D+0x1D4 == 0`): like > 60 and `bond+2` > 100 → `0x5A0960`. Then, independently, like < −60 and `0x535390()` and `bond+2` > 100 and `p14C > 30`: if score > best, `rec+0x12D = min(rec+0x12D+200, 200)` (always 200) and push `0x5A18A0` with no AL_SetBehavior; best = score (`0x5A07BC..0x5A07F7`) | |
| 24 | `0x562950` | type not 2 and not 20..25; signal 0xAB with non-null `hit+4` (`0x56298A..0x5629BC`); no best < 1 gate | BREED `rec+0x13A` > 5000 (`0x1312E9C`) → `0.99*0.88` (`0x1312EA0`, `0x9071B8`), jitter | `0x59BF30` | |
| 25 | `0x562880` breed | best < 1; type not 2 and not 20..25; `0x54B1F0(EDX=obj, ECX=1, push 0xAB)` == 0; BREED ≥ 10000 | 1.0 (no jitter) | `0x5319F0(ESI=*0x134046C)` < 8 and `0x54AC20` → `0x55C430` + push `0x59BB50`; ≥ 8 → BREED = 0 (`0x56293A`) | |
| 26 | `0x562A20` death | best < 1; u16 `rec+0x8A == 0`; `0x54AC20` | 1.0 | `0x568F60` | |

Default `0x562D20` (bytes at `0x562D20..0x562D4F`, not a Ghidra function): `if ((int)*0x1A1693C > 0) 0x534F80(EAX=0x1A1693C, EDI=&W+0x54, push *0x1A1693C)`; then `0x5A3650(ESI=obj)`; returns 0. `0x5A3650` takes the run points u16 `rec+0x3C`, ×3 unless the area is 6 or 10..11 (`0x5A3650..0x5A366E`), and compares them **as 16 bits** (`CMP AX`): < 150 (`0x1312D6E`) → `0x55C430` + push `0x5A3280`; < 2000 (`0x1312D70`) → `0x55C3C0` + `0x5A3380`; < 4000 (`0x1312D72`) → `0x55C3C0` + `0x5A3460`; else `0x55C3C0` + `0x5A3580`.

`0x562C50(EAX = D+0x1CC, ESI = obj)`: 0 → nothing. Not 0x11 → `AL_SetBehavior(0x565BE0,-1)`. 0x11 → calls `rand()` (result unused, `0x562C83`), then reads the table by u8 `*0x1A5A3EF` (> 4 → nothing): 0 → `0x565BE0`, 1 → `0x11C9BF4`, 2 → `0x11C9BB4`, 3 → `0x11C4BE0`, 4 → `0x12FE978`, indexed by u8 `*0x1A5A3F2`. Then it clears `D+0x1CC/0x1CE`.

### 1.6 Think cadence: fallback `0x565BE0` (not a Ghidra function; decoded from bytes `0x565BE0..0x565DBC`)
- Mode 0 (`D+0x1B6 == 0`):
  - If the current motion id u16 `D+0xAE` is in 1..3: `0x793D30(EAX=D+0xAC, EDX=0x3E, push 0x23)`.
  - Otherwise roll `r` and pick a motion:
    - `r < 0.15` (`0x165DFD8`): motion 3 (`0x793C40`, EDX=3), emotion-ball icon `0x53D660(dl=3, push 180)`, and sound `0x601C` via `0x4372E0` if `*0x134046C == *0x134062C`.
    - `r < 0.5` (`0x165DFD4`): motion 0.
    - `r < 0.6` (`0x8AF048`): motion 0xEE.
    - `r < 0.7` (`0x109DFF0`): motion 0xEF.
    - otherwise: motion 1.
  - Then eyes `0x53A4B0(-1,0)` and mouth `0x53A5A0(-1,0)`.
  - `D+0x1C0 = 60 + trunc(121*r)` (−121.0 at `0x905A20`, truncating FPU mode), `D+0x1B6++`.
- Every frame, mode 0 included: `t = D+0x1C0--`. If `t > 0`: when on ground (`W+0x40 & 0x400`), `W+0..W+8 *= 0.8` (`0x905230`), then return 0. Else return 2 (`0x565DB3`).
- So an idle Chao re-decides after 61..181 calls.


## 2A. Behaviour routines (part A)

### 2A.0 Conventions and helpers these routines rely on

- **Behaviour signature:** `int fn(ObjectMaster* obj)`, cdecl. The AI tick `0x53DC40` runs only when `D+0xA0 & 0x10` (`0x53DC4E`). It has **two paths**:
  - **Normal path (`D+0xA0 & 0x2000` clear, the garden case; CreateChao clears 0x2000, see chao-garden-sync.md):**
    1. Think timers `0x562C00`, then interrupts `0x53DB10`, then the current slot's fn. An empty slot counts as return 1 (`0x53DCC7`).
    2. **Frame limit:** if `D+0x47C[idx] > 0`, it is decremented. When it reaches 0 the entry is popped, whatever the routine returned (`0x53DCD2`–`0x53DCFA`).
    3. Return **2** runs the picker `0x562A80`, but only when `word D+0x1CC == 0`. It then always calls `0x562C50(EAX = D+0x1CC)` (`0x53DCFC`–`0x53DD1F`).
    4. Return **1** pops the entry (`0x53DC83`). Return **3** resets to `0x565BE0` (`0x53DD28`). Any other value (0, a pointer, …) does nothing.
  - **`D+0xA0 & 0x2000` set:** no think timers and no frame-limit countdown. Only return 1 (pop) and return 3 (reset) are handled. **Return 2 is ignored** (`0x53DC5F`–`0x53DC95`).
- **The "stack" is a FIFO queue.** `D+0x434` is the count, `D+0x438` the read index, `D+0x43C[16]` the fns and `D+0x47C[16]` the frame limits.
  - **Pop `0x53D9B0`:** clears mode `D+0x1B6`, `D+0x1B8`, `D+0x1C0` and `D+0x1C4`. Sets `D+0x1C8 = 3600`, saves the popped fn in `D+0x430`, decrements the count and increments the index. Returns 0 when the next slot is null. On that return the tick calls `SetBehavior(0x565BE0)` (`0x53DD2D`).
  - **SetBehavior `0x53D890(EAX=obj, push fn, push limit)`:** clears the same fields and sets `D+0x1C8 = 3600`. Saves the current fn in `D+0x430`, clears all 16 slots and puts fn in slot 0 (count 1, index 0, limit = arg). Then zeroes `D+0x41C`, sends mailbox 3 to a mutual ALW partner, and tail-calls cleanup `0x53D760`.
  - **Push `0x53D970(EAX=obj, EDX=fn)`:** appends at `slot[count]` with limit −1, but only when count < 15.
- **`0x53DA00` "resume previous":** swaps the current slot with `D+0x430`. Zeroes mode, `D+0x1B8`, `D+0x1C0`, `D+0x1C4`, **`D+0x41C`** and `D+0x1BC`, and sets `D+0x1C8 = 3600`. Sends mailbox 3 to a mutual partner and runs `0x53D760`. `0x562D50` uses it to hand control back to the walk it interrupted. The resumed walk starts again at mode 0 with `W+0x54` unchanged.
- **Motion table fields used:**
  - `D+0xAE` = posture (MotionTable+2) = entry +6 copied to `D+0xAE` when a motion starts (not when a link begins): 0 standing, 1 sitting, 2 lying, 3 on the belly (crawl/trip), 4/5 other (chao-garden-sync.md, MotionTable timing).
  - `D+0xB0` = current motion id.
  - `D+0xB8` = speed multiplier.
  - `D+0xC8` = current frame.
  - `D+0xAC` bit 0x1 = end pulse. Bit **0x8** is set only by play mode 3 (hold at the end), together with 0x1, on every frame from the end on; for any other motion it never fires. `0x793F80` returns it.
  - `0x793C40(EAX=D+0xAC, EDX=id)` sets a motion. `0x793D30(…, push blend)` sets one with a blend; the "blend N" values below are that push and apply only through `0x793D30`. A plain `0x793C40` link blends 15 frames (1/15 at `0x904DCC`), apart from two entries with blend 0.
- **Physics work W (`obj+0x38`):**
  - +0x0..8 velocity.
  - +0xC/+0x14 horizontal drive, set from facing (`sin/cos(D+0xC)·speed`).
  - +0x10 vertical force, set to `−W+0x48·k`.
  - +0x1C bearing to target (BAMS); `0x530510` writes it.
  - +0x13C reference Y.
  - +0x140..0x148 a vector; the walk step computes `0·x + y + 0·z` of it and compares that with 0.7 (`0x5A30E8`–`0x5A311C`).
  - +0x40 flags: 0x400 on ground, 0x4000 wall.
- **`0x796910(EAX=obj, push maxTurn)`:** turns `D+0xC` toward `W+0x54`. Returns |remaining difference|. `0x56B500` treats a value < 0xB6 as facing (`0x56B54A`).
- **`0x530510(EAX=obj, push rate)`:** target = `entry(+0x3C)→+0x44→+0x3C` object. Writes `W+0x1C = 0x54B0A0(...)`, turns `D+0xC` via `0x446960` and returns the |difference|.
- **`0x530580(ECX=obj)`:** distance (`0x77FBD0`) to that target. With no link it returns **0.0** (distance from the Chao to itself; the −1.0 `0xB087CC` path at `0x5305B6` needs a null saved ECX, see §2B `0x567FA0`).
- **`0x54B0E0(EAX=obj, push f)`:** moves the position directly by `facing·f` (no physics).
- **ALW links and mailbox:** ALW entry = `obj+0x3C`; the partner pointer is entry+0x40.
  - `0x530730(EAX=obj, EDX=msg)` writes msg into the partner entry's +0x34, only when the link is mutual.
  - `0x530710` reads and clears your own +0x34.
  - `0x5306D0` returns the mutual partner entry. `0x5306F0(EAX, push cat)` does the same but also requires partner word +0 == cat.
  - `0x530640` links both ways. Both +0xC get the same link id from the counter `0x1A5B5EC` (mod 256).
  - `0x530690` unlinks: clears own +0x6 and +0x40 and sets +0xC = −1. When the link is mutual it also clears the partner's +0x6 and +0x40. The partner's +0xC is set to −1 either way.
  - `0x5304F0` sets the target (+0x44).
- **`rec+0x12A` once-latch.** `rec = D+0x5C`. `0x536420(EAX=obj)` returns 1 and sets the word to 1 only when it was 0, otherwise it returns 0. The eat bite calls it (`0x560E3F`). The walk step (`0x5A3151`–`0x5A3161`) and the sleep tick (`0x54EC7C`–`0x54EC8C`) **inline the same check-and-set** instead of calling it. The life tick resets the word to 0 at each need period (§3.1).
- **`0x56CFB0` (ESI=chao, ECX=item):** links to the item, sets partner flag +6 |= 2, sets `D+0xA0 |= 0xC00`, holds the item.
- **`0x56CFF0(ECX=chao)`:** returns 0 unless `entry+0x40` is set and links back (`0x56D008`–`0x56D01C`, `0x56D163`). Otherwise it positions the item at the hand nodes `D+0x5D8/0x5E4` and returns 1 (`0x56D152`).
- `0x565BC0` (cdecl obj): if the balloon `*0x1AED28C` belongs to this Chao, sets its mode byte to 6 (meaning unknown). The eat tail at `0x561350` sets it to 7 and clears the global.
- **`AddHappiness 0x536320(ESI=obj, EDI=delta, push f)`:**
  1. `rec+0x82 += delta`, clamped −100..100 (`0x536326`–`0x536345`).
  2. If `*0x1DE9600` is non-null and its byte +1 ≤ 8, calls `0x536260(±f)`. The sign comes from the table at `0x5363B8`/`0x5363AC`, indexed by that byte (`0x53634C`–`0x53637E`).
  3. Calls `0x53E120`.
- Wall-hit kinds: `0x486760(obj,0xCB)` = climbable hit (C_CLIMB boxes). `0x54B1F0(ECX=1, EDX=obj, push 0xCC)` = obstacle signal (Hero HASIRA/HASIRAKAKE, Dark fences and graves). Both: garden-collision.md §8.
- Sound `0x4372E0` / `0x437590` (EDI id, ESI &pos, 3 pushes) plays only when `*0x134046C == *0x134062C`. Ids are raw.
- **Random ranges:** the code uses `a − trunc(r·k)` with k negative, r = rand()/32768 (`0x904E90` = 2^-15), and FPU control word |0xC00 (truncate). With k = −(n+1) that gives an integer uniform in **a..a+n**. Ranges below use that form; the k constants are given where they matter.

### 2A.1 Walking and movement

| Routine | Start (mode 0) | Per frame | Ends |
|---|---|---|---|
| `0x562D20` wander | If `*0x1A1693C` > 0, `0x534F80(EAX=0x1A1693C, EDI=&W+0x54, push count)` picks a destination. Then `0x5A3650` (`0x562D44`) | – | returns 0. `0x5A3650` has already replaced it |
| `0x5A3650` install walk | Run points `p = rec+0x3C`, ×3 unless `*0x134046C` is 6, 10 or 11 (`0x5A3650`–`0x5A3671`). p<150 (`0x1312D6E`): `Set(0x55C430)` + push `0x5A3280`. p<2000 (`0x1312D70`): `Set(0x55C3C0)` + push `0x5A3380`. p<4000 (`0x1312D72`): `Set(0x55C3C0)` + push `0x5A3460`. Otherwise `Set(0x55C3C0)` + push `0x5A3580`. All frame limits −1. Pushes are skipped when count ≥ 15 | – | – |
| `0x55C3C0` stand-up prelude A | posture `D+0xAE`: 1 → motion 0x30 blend 40; 2 → 0x26 blend 40; 3 → 0x26 blend 35; other → return 1 at once (`0x55C3E9`) | – | end pulse → 1 |
| `0x55C430` stand-up prelude B | posture 0 → 0x2F blend 40; 2 → 0x2A blend 40; 3 → 0x2A blend 35; other → 1 (`0x55C459`) | – | end pulse → 1 |
| `0x5A3280` slow walk | motion 0x73 (a crawl, posture 3; inferred from the posture data) | When `D+0x30 % 120 == 0` and r<**0.3** (`0x9050C8`): sound 0x6014 if r<0.5, else 0x6015. On ground: drive = facing·**0.038** (`0x1312C6C`). `D+0xB8` is not set | tail-calls `0x5A3000` |
| `0x5A3380` walk | motion 0x64 | When `D+0x1C0 % 85 == 1` (value before the increment) and r<**0.03** (`0x905000`) → `SetBehavior(0x562D50)`. Drive **0.055** (`0x1312C68`). `D+0xB8 = |v|·16` (`0x9052D0`) | `0x5A3000` |
| `0x5A3460` jog | motion 0x65 | When `D+0x1C0 % 100 == 0` and r<**0.02** (`0x9054C0`) → `0x562D50`. Drive `0.028 + 7e-6·p` (`0x1312D74`, `0x1312D78`; p scaled as above). `D+0xB8 = |v|·14` (`0x9052E0`) | `0x5A3000` |
| `0x5A3580` run | motion 0x66 | Drive `0.028 + 7e-6·p`. `D+0xB8 = |v|·14` | `0x5A3000` |

**Walk step `0x5A3000` (ESI=obj)**, run every frame by the four walks:
1. If `dx²+dz²` to `W+0x54` < **36.0** (`0x8AE5DC`) → return 1 (`0x5A3029`). The walks have **no timeout**.
2. Wall bit `W+0x40&0x4000`: if `0x486760(obj,0xCB)` returns a hit with +4≠0 → `SetBehavior(0x562EB0)` and `D.y(D+0x18) += 0.1` (`0x905030`). Otherwise → `SetBehavior(0x56B740)` (`0x5A3040`–`0x5A308D`).
3. `D+0x6` (u16) is incremented. When its old value is > 30, `0x54B1F0(0xCC)` is checked; a hit → `0x56B740` (`0x5A3090`–`0x5A30C7`). Nothing here resets `D+0x6`.
4. `0x796910(obj, 0x120)`.
5. On ground: if `W+0x144` < **0.7** (`0x109DFF0`) → `vy(W+4) += 0.55` (`0x9067E8`) and `SetBehavior(0x5A0F40)` (`0x5A311C`–`0x5A313E`).
6. In the air, when `D.y − W+0x13C > 2.0` (`0x904F78`): `vy += 0.55`, then the first match wins:
   - `0x535010(EAX=&W+0x54) == 9` → `0x5A0F40`.
   - fly points (`rec+0x3A`, ×3 unless the game mode is 6, 10 or 11) < 400 → `0x5A0F40`.
   - r<**0.6** (`0x905268`) → `0x5A0F40`.
   - otherwise → `0x5630C0` (`0x5A3196`–`0x5A322B`).
7. Once per `rec+0x12A` latch (inline): **rec+0x13C −100** (`0x1312DB0`) and **rec+0x140 +300** (`0x1312DC2`), each clamped 0..10000 (`0x5A3146`–`0x5A3273`). Returns 0.

The walk keeps running after it calls SetBehavior in steps 2, 3, 5 and 6, so the latch update still applies on that frame.

| Routine | Start | Per frame | Ends |
|---|---|---|---|
| `0x5A0F40` hop | eyes 1, mouth 2, motion 0x8A. v = facing·**0.4** (`0x904EB0`), vy = **0.35** (`0xA3D28C`). Mode 1 | `W+0x10 = −g(W+0x48)·0.45` (`0x905398`) | on ground → `SetBehavior(0x562D20)` (re-wander) (`0x5A0F68`) |
| `0x5630C0` glide | eyes 5, motion 0x82 blend 15, v = facing·0.4, vy = 0.35. `D+0x1C0` = 300..600 | `0x796910(0x100)`. Drive = facing·0.05 − v·0.1. `W+0x10 = −g·0.95` while `D+0x1C0` > 0, then `−g·0.92`. If `D.y > 100` → clamp to 100.0 (`0xC5D234`) | on ground → `0x562D20` |
| `0x56B740` wall bump | motion 0x64 blend 15. r<0.5 → mode 1, turning `+0x16C`/frame; otherwise mode 2, turning `−0x16C`/frame (±2°). Count `D+0x1C0 = 45 + trunc(46r)` = 45..90 (`0x905A68` = −46) | rotate `D+0xC`. `D+0xB8 = 1.6` (`0x1708AA4`) | count done → 1. The queue now holds nothing, so reset follows |
| `0x56B860` turn around | motion 0x64 blend 15. 50/50 direction ±0x16C/frame. Count fixed **90** (`0x56B937`), i.e. 180° | rotate. `D+0xB8 = 1.6` | count done → 1. Caller unknown |
| `0x56B960` random turn | motion 0x64 blend 15. ±2°/frame, 50/50. Count = trunc(r·91) = 0..90 (`0x905A60` = 91.0), stored as u16 | same | 1 |
| `0x56B500` face target | motion 0x64 blend 15. `D+0x1C0 = 60` (unused) | `0x796910(0x200)` on every frame, including the first | returns 1 once the angle error < 0xB6 (≈1°) (`0x56B54A`) |
| `0x56B6C0` face ALW target | motion 0x64 blend 15. Count 90, mode 1 | while count > 0: `0x530510(0x200)`. `D+0xB8 = 1.6` | count exhausted → 1 |
| `0x56B560` walk to `W+0x54` | mode 0, every frame: eyes 4 (`0x53A4B0` EDX=4) and `D+0x1C8 = 1800` (the timeout restarts). The first 32 frames it stands still. When the `D+0x1C0` old value > 30: motion 0x73 if run pts (scaled) < 150, else 0x64 (no blend), `D+0xB8 = 1.5` (`0x1283DDC`) | `0x796910(0x180)`. Drive 0.055·0.8 (`0x905230`) = **0.044**, on ground or not. `D+0x1C8−−` | 3-D distance (`0x7968A0`) < **4.0** (`0x104DD30`) → 1. Distance < 0.0 → reset (cannot fire). `D+0x1C8` ≤ 0 (1800 frames) → `SetBehavior(0x565BE0)` |
| `0x56BA80` walk to ALW target | as `0x56B560`, but always motion 0x64 | `0x530510(0x180)`, drive 0.044. Distance from `0x530580` | < 4.0 → 1. With no link `0x530580` returns 0.0, so the walk ends (→1) on its first walking frame (frame 33); the < 0.0 reset (`0x56BB0F`) cannot fire. Timeout 1800 → reset |
| `0x56B480` "notice" (**not** movement) | emotion ball `0x53D660(DL=1, push 180)`. `D+0x1C8 = 1800`. Mode 1 | count `D+0x1C0` up; `D+0x1C8−−` | returns 1 on the 122nd mode-1 frame (old count > 120, `0x56B4A3`). `D+0x1C8` ≤ 0 → reset |
| `0x56BBA0` press toy switch | motion 0xBE | on ground: v·=0.9 | after 61 frames: `0x55A920` signal **0xAF**, radius 3.5 (`0x16AFE94`), 10 frames, at facing·3 / y+1 → 1 (`0x56BC77`). 0xAF switches an off toy on (chao-garden-sync.md, Toys) |
| `0x562D50` interrupting idle | motion 0x9F blend 15, `D+0x1C0 = 0`, mode 1 | One counter `D+0x1C0` runs through modes 1–2 and is never reset. Mode 1: at count 40 → sound 0x100A. When the old count > 60 → icon 5 for 240 frames, eyes 1, mode 2. In modes 0/1 on ground: drive = facing·0.055. Mode 2: when the old count > 300 (about frame 302 of the behaviour) → motion 0xA0 blend 15, mode 3 | mode 3, end pulse → **`0x53DA00`**, which resumes the interrupted behaviour (`0x562E88`). Always returns 0 |
| `0x565BE0` reset/settle | posture 1..3 → motion 0x3E blend 35. Otherwise (no blend): r<**0.15** (`0x165DFD8`) → motion 3 + icon 3 for 180 frames + sound 0x601C; r<0.5 (`0x165DFD4`) → motion 0; r<0.6 (`0x8AF048`) → 0xEE; r<0.7 (`0x109DFF0`) → 0xEF; else motion 1. Then eyes 0, mouth 0. `D+0x1C0 = 60 + trunc(121r)` = 60..180 (`0x905A20` = −121) | on ground: v·=0.8 (`0x905230`) | → 2 after N+1 frames (61..181) (`0x565DB3`) |

### 2A.2 Illness (`rec+0x15A+i < 0` → `0x11C9BF4[i]`, table read via REA)
Common pattern: mode 0 picks the motion (blend 40), eyes and mouth. Mode 1 loops. A "cycle roll" either ends the behaviour (→1) or bumps the cycle count `D+0x1C4` and goes back to mode 0. None of these routines change needs or stats. Illnesses 0, 1, 2 and 4 test the motion bit `&8`, not `&1`.

| i | Routine | Motion (posture 1..3 / other) | Face | Exit and cadence |
|---|---|---|---|---|
| 0 | `0x599D40` | 0xF4/0xF5 (50%) or 0xF6/0xF7 (50%) | eyes 1 (r<0.7) else 3; mouth 2 (r<0.7) else 4 | On pulse 0x8: r<0.1 → 1, or cycles>10 → 1, else restart. Every 60 frames, r<0.4 → sound 0x606A |
| 1 | `0x599F30` | 0xF2 / 0xF3 | eyes 1/3, mouth 8 | Pulse 0x8 and a 60-frame tick and r<0.3: r<0.4 → 1, cycles>10 → 1, else restart. Frame 180: sound 0x606B, mouth 4 |
| 2 | `0x59A0B0` | r<0.2: motion 0 (no timer), else 0x31 + sound 0x1015 with timer 60..120 | eyes 3, mouth 8; r<0.1 → sound 0x602C | When the timer runs out or on pulse 0x8: r<0.2 → 1, cycles>13 → 1, else restart. The r<0.2 motion-0 branch doesn't set the timer, so the cycle roll happens on the same frame (or right after a restart) |
| 3 | `0x59A260` | 0x51 / motion 0 | eyes 9, mouth 8 (r<0.4) or 0 | Each 60-frame tick, r<0.3: r<0.4 → 1, else sound 0x1010 (arg 0x46), 61-frame pause (mode 2), restart. Mode 2 also zeroes `D+0x1C4`, so the ">2 cycles" exit can never trigger |
| 4 | `0x59A400` | 0xF8 / 0xF9 | eyes 1/3, mouth 4 (r<0.5) or 8 | 60-frame tick and pulse 0x8 and r<0.5: r<0.3 → 1, cycles>10 → 1, else restart. Frame 20: sound 0x606E |
| 5 | `0x59A5A0` | 0x5E / 0xA3 | eyes 1/3, mouth 8; sound 0x6022 (r<0.3) or 0x6023 at the start of every cycle | 60-frame tick with r<0.3: r<0.4 → 1, cycles>10 → 1, else restart |

### 2A.3 Sleep
- **`0x54EE10` go to bed:**
  1. `SetBehavior(0x54EF10)`.
  2. Requires the tree list (`D+0x12AC` ≠ 0) and the nearest index `D+0x12BC` ≥ 0. The entry is at `D+0x12C0 + idx·0x1C`.
  3. Requires **r > 0.5 + 0.003·(s8)rec+0x14C** (`0x904E98`, `0x905838`), entry float +8 < **30.0** (`0x905090`), and tree `obj(+0x18)→+0x3C→+0x34` byte +0x31 equal to **3 or 5**.
  4. If all hold: `0x5304F0`, `SetBehavior(0x56BA80)`, push `0x56B960`, push `0x54EF10` (`0x54EE10`–`0x54EF04`). The Chao walks to the tree, turns at random, then sleeps.
- **Sleep tick `0x54EC70` (ESI=obj)**, gated by the inline `rec+0x12A` latch. The phase is held in `D+0x1B8`:
  - Phase 0: SLPDPTH(`rec+0x136`) **+1500** (`0x1312D9E`); phase++ when ≥ 10000.
  - Phase 1: SLEEPY(`rec+0x134`) **−1500** (`0x1312D9C`) and TIRE(`rec+0x140`) **−trunc((300 + (s8)rec+0x151)·300/300)** = −(300 + rec+0x151) (`0x1312DBE`, magic `0x1B4E81B5` >>5). Phase++ when SLEEPY = 0.
  - Phase 2 (stays): SLEEPY −1500 and SLPDPTH **−3500** (`0x1312DA0`).
  - All values clamped 0..10000.
- **Wake test `0x54EC40`:** SLEEPY < 1000 **and** SLPDPTH < **2000** (`0x1312DA2`).
- **`0x54EF10` sleeping** (17 modes, jump table `0x54F750`). Every mode except 16 runs the sleep tick. Sounds play only in the active garden; the rand rolls happen regardless.
  - Mode 0: posture 0 → motion 0x25, mode 1. Posture 1 with TIRE > **7000** and r<0.5 → `SetBehavior(0x54F7A0)`. Otherwise → mode 2. Every branch zeroes `D+0x41C`.
  - Mode 1: end pulse → mode 2.
  - Mode 2: choose a pose. `D+0x41C` = 1 (motion 0x1B blend 40) when (f32)p14C > r·100, else 2 (0x12 blend 40). Later visits play 0x1C or 0x13 with no blend. Eyes 7, duration −1; timer 1000, mode 3, and it falls into mode 3 the same frame.
  - Mode 3: snore every 120 frames.
    - Pose 1: r<0.5 → 0x6008 (r<0.8) or 0x6009.
    - Pose 2: r<0.4 → 0x600B (r<0.8) or 0x600A.
    - When the timer expires: r<**0.3** (f32 `0x145D484`) → motion 0x18, mode 4; r<0.6 (f32 `0x8AF048`) → motion 0x34 (mode 6) or 0x37 (mode 8), 50/50; r<0.9 (double `0x904E28` = 0.8999999761581421) → count 2..5 = 2 − trunc(r·−4.0) (`0x905D68`) pulses, then motion 0x22 (mode 10) or 0x1F (mode 12), 50/50; else stay. Then 25% → sound 0x600F/0x6011.
    - Wake test → mode 15.
  - Modes 4, 6, 8: 1000 frames or wake → motion 0x1A / 0x36 / 0x39 → modes 5 / 7 / 9.
  - Modes 10, 12: the pulse count runs out or wake → 0x24 / 0x21 → modes 11 / 13. Mode 12 checked (`0x54F673`).
  - Modes 5, 7, 9, 13 (shared handler `0x54F52B`) and 11 (`0x54F649`): end pulse → mode 2.
  - Mode 14 (`0x54F023`): only runs the sleep tick. No path that sets mode 14 was seen.
  - Mode 15: motion 0x1E blend 40, eyes 1, mouth 3, mode 16.
  - Mode 16: pulse 0x8 → clears `D+0x1CC/0x1CE` and returns **1** (`0x54F726`) without the sleep tick.
- **`0x54F7A0` sit-and-nod.** Mode 0: r < TIRE·0.0001 (`0x905FB0`) → motion 0x14 (mode 1); else 0x16 (mode 2) and count 3..12 = trunc(r·10+3) (`0x904F48`, `0x904DD8`). Eyes 7. Mode 2: every end pulse counts down; at 0 → motion 0x17 and a new count. Either mode: wake test → motion 0x3E and return 1 (mode 2 also clears `D+0x1CC/0x1CE`). Sleep tick every frame (`0x54F96E`).
- **`0x54FCF0` deep sleep.**
  - Mode 0: posture 1 → motion 0x3F, 2 → 0x12, else 0 (no blend). Eyes 9; r<0.3 (`0x9050C8`) → mouth 8; icon 5 for 360 frames; mode 1.
  - Icon cadence: `D+0x1C0` counts up and the icon is re-issued when the old value > 400, so **every 402 frames** (`0x54FD9F`).
  - **SLPDPTH −2 every frame, no latch** (`0x54FDCA`–`0x54FDF4`). At 0 → 1 (`0x54FE06`).
- **`0x54F980` drowsy** (jump table `0x54FCC4`).
  - Posture 0, 50/50:
    - motion 0x27 (eyes 3, mouth 2) + sound 0x6005 + count 3..8; mode 1 = 0x27 cycles → 0x29 (mode 2) → end; or
    - motion 0x54 (eyes 1, mouth 2) + sound 0x600E → end.
  - Posture ≠0:
    - r<0.3 → 0xF (eyes 3, mouth 2) + sound 0x6005 + count 2..7; mode 4 = 0xF cycles → 0x11 (mode 5);
    - r<0.6 → 0x1E (eyes 1, mouth 2) + sound 0x600E;
    - else 0x15 (eyes 3, mouth 8).
  - Modes 2, 3, 5, 6, 7 → mode 8 on the end pulse. Mode 8 clears `D+0x1CC/0x1CE` and returns **1**. No need changes.

### 2A.4 Food and drink
- **`0x59E8C0` beg (player holds fruit).** Motion 0x76 blend 10, eyes 4, mouth 3/2 (unless the mouth is 1). Each frame: player-1 (`*0x1DEA6C0`) 3-D distance² < **100** → 1. More than 1000 frames → 1. Otherwise `0x796AB0(ECX=0, 0x120)` and drive 0.055.
- **`0x59EA00`:** motion 0x18E blend 25, eyes 4, mouth 3/2, waits 180..300 → 1.
- **`0x5614D0` go eat.**
  - Takes the nearest entry of list 2 (`D+0xF10`; index +0x14, entries 0x1C bytes at +0x18), then its object `+0x18→+0x3C→+0x3C` (the fruit's ALW entry).
  - If that entry's +0x40 partner is non-null and is not its own entry → returns the **fruit entry pointer** (a no-op value), so it retries every frame (`0x561533`→`0x56162C`).
  - Otherwise: its own entry +0x44 = fruit, `SetBehavior(0x55C3C0)`, push `0x56B480`, `0x56BA80`, `0x5613C0`, `0x5607C0`; returns 0.
  - No fruit → 1 (`0x561627`).
- **`0x5613C0` pick up.** Motion 0xBC, sound 0x1001, timeout `D+0x1C8 = 1800`. On the end pulse, if the fruit is free or ours: `0x56CFB0` grabs it. It returns 1 on that same end-pulse frame whether or not it took the fruit; its mode 2 is unreachable. Timeout → **3** (`0x5614B5`).
- **`0x5607C0` eat** (14 modes, jump table `0x561374`).
  - **Tail `0x561197`, every frame:** if `0x56CFF0` returns 1 (still holding) → return 0.
  - Otherwise, if its mailbox (read and cleared) == **6**:
    - `AddHappiness(−1, 0.1 [0x1312D18], sign via 0x536260)`;
    - `rec+0x12D = min(200, rec+0x12D+200)` and `rec+0x12E` likewise (both effectively 200);
    - if the balloon `*0x1AED28C` belongs to this Chao, its mode byte is set to 7 and the global is cleared.
  - Then → **1** (`0x5611A6`–`0x561360`).
  - Mode 0: motion 0xA9, `D+0x1C4` = 100..399.
  - Mode 1 (end pulse): if rec+0x150<0 and rec+0x14A<0 and HUNGER ≤ 5000 → mode 4. Otherwise r≤0.5 → mode 4, r>0.5 → motion 0xB4 (mode 2).
  - **Dislike test** on the fruit kind `fruitD1+8`, using jump table `0x5613AC` for kinds 5..9:
    - kind 5 if `0x535390` (dark adult);
    - kind 6 if `0x535360` (hero adult);
    - kind 7 if rec+0x157 ∈ {2,4};
    - kind 8 if ∈ {0,5};
    - kind 9 if ∈ {1,3}.
    - A dislike → mode 9, `AddHappiness(−1, 0.1 [0x1312D14])` (`0x560905`–`0x560A34`), sound 0x603A.
  - Mode 2: waits 120 frames, then the end pulse → motion 0xB6, eyes 4, sound 0x6039, mode 3. Mode 3: waits 60 frames, then the end pulse → mode 4.
  - Mode 4: p150 ≤ 50 and HUNGER ≤ 8000 → motion 0xAF + sound 0x603B. Otherwise 0xB3 + sound 0x603C. Then mode 6. The style `D+0x41C` is chosen on the first visit only and reused on later visits.
  - **Mode 6, bite loop `0x560BC0`:**
    1. Every frame, `0x530730(EDX=2)` messages the fruit.
    2. If its mailbox (read and cleared) == **4** (fruit finished). Quirk: any other value, including **6**, is read and discarded here, so a fruit taken while chewing ends the meal with no penalty (the tail's mailbox-6 branch sees an empty mailbox): motion 0xA5 (`0x793D30`, blend 60), `D+0x1C4 = 180 + trunc(200r)` = 180..379 (`0x9051D8` = 200), mode 11, unlink `0x530690`, `0x565BC0`, eyes 4, mouth 2, sound 0x6042; then the tail runs.
    3. Otherwise `D+0x1C4−−`. While it is > 0 → the need change (step 5).
    4. When it reaches 0:
       - p150 > 50 and r<**0.4** (`0x904EB0`) → motion 0xB7, mode 5, count `2 + trunc(5r)` = 2..6 (`0x905010`), sound 0x603D, eyes 1.
       - Otherwise: motion 0xB1 (r<0.5) or 0xB0; count **10** if r<**0.3** (`0x9050C8`), else `40 + trunc(200r)` = 40..239. Eyes 0, mode 7.
    5. **Need change at `0x560E3D`**, reached on every mode-6 frame except the mailbox-4 frame and gated by `0x536420`: **HUNGER(rec+0x138) −2400 and rec+0x140 −2400** (`0x1312DA6`, clamped 0..10000). If HUNGER is then 0 → mode 9, which overrides the mode 5/7 just set (`0x560E4C`–`0x560EC6`).
    6. Fruit bite: on the **182nd** mode-6 frame (counter fruit `D+6`), not every 180 frames.
  - Mode 5: counts 2..6 motion ends, then motion 0xAD (mode 8).
  - Mode 7: when the pause runs out, r<0.65 → motion 0xAD (mode 8); otherwise it swaps 0xB0↔0xB1 (blend 25) with a new pause.
  - Mode 8: back to mode 4 with a new chew budget 180..579.
  - Modes 9/10 (full or disliked): motion 0xBA. Past frame 4 the fruit is thrown with v = (sin, 0.7, cos)(rot+0x71C7)·0.3, then `0x565B90` and unlink (`0x561119`–`0x561120`).
  - **Modes 11–13 (post-meal 0xA6/0xA5/0x52) are unreachable:** both mode-11 entry points unlink just before jumping to the tail (`0x560C52`, `0x561120`). `0x56CFF0` then returns 0, the mailbox was already cleared, and the tail returns 1 on the same frame.
- **`0x59ED60` hungry** (jump table `0x59EF2C`). Posture 1 → motion 0x5E, else 0xA3. Eyes 3 on both paths. Posture 1: count 180 down, then wait for the motion end (`&1`). Other postures: wait for the motion end. Then mode 3 returns 1 when its old count is > 60 (`0x59EE6A`, i.e. 61 frames); on that frame it clears `D+0x1CC/0x1CE` and sets **`D+0x20C = trunc(1200 + 0.5·10800·(r + (10000−HUNGER)/10000))`** (V: `0x59EE8C`–`0x59EF18`; ints `0x1312E3C` = 1200, `0x1312E40` = 10800; `0x904F50` = 10000.0; `0x904E98` = 0.5) → 1.
- **`0x59EF40` starving.** Eyes 1, mouth 6, motion 0x1DC blend 20, 240..440 frames (240+trunc(201r), `0x905A00`). Effect `0x6ED270` at two body nodes on 4 frames out of every 8. When `D+0x30 % 60 == 0`, 50% → sound 0x6003. At the end, the same `D+0x20C` formula → 1.
- **`0x5A1020` thirsty.** Picks a destination from the list at `*0x1A1E95C` (`0x534F80`). Plays sound 0x6043, or 0x6044 only when r == 0.0 exactly (`0x904E00`). Then `0x5A3650`. Returns 0.

### 2A.5 Performances and watchers (signals from `0x55A920`)
These loop until `0x486880(obj,1,kind)` stops returning a hit with +4≠0, then return 1. The chooser creates the signal after `SetBehavior`, at `pos + facing·8` (`0x905008`) with radius **20.0** (`0x8AE6C4`):
- **0xA5** (skill A): `1800 + trunc(2401r)` = 1800..4200 frames (`0x907E78` = −2401; `0x59D57F`).
- **0xA6** (skill B): also **1800..4200**, radius 20 (`0x59C801`–`0x59C84E`).
- **0xA7** (skill C): `1500 + trunc(1801r)` = 1500..3300 (`0x906268` = −1801; `0x59BA15`). Non-dark animal-10 trick: `1200 + trunc(1201r)` = 1200..2400 (`0x907E80`; `0x59BB1B`–`0x59BB3D`).
- **0xA8** (dark animal-10 trick): 1800..3600 (`0x59BAB4`–`0x59BAE2`). Chosen when `rec+0x118 & 0x400` and **alignment `rec+0xB0` < −0.5** (`0xE9923C`, `0x59BA70`–`0x59BA83`). Otherwise the non-dark `0x59B0E0` is used.

| Routine | Motions | Attachments (node, model) | Sounds and cadence |
|---|---|---|---|
| A0 `0x59D8E0` | 0x15F/0x160 (50%) | 11, block `0x11C9B54` | every 30 frames: 0x4009 |
| A1 `0x59DE90` | 0x165 ↔ 0x166/0x167 (re-chosen every 60..240 frames, r<0.6) | 4, `0x11C9B94` | 0x400D |
| A2 `0x59E2D0` | 0x16B ↔ 0x16C | 4 `0x11C8964`, 11 `0x11C8FFC` | 0x4007 |
| A3 `0x59DA40` | 0x161 ↔ 0x162 (every 60..240 frames) | 4 `0x11C6674`, 11 `0x11C6A14`, 35 `0x11C62C4` | 0x4008 (every 16 or 12 frames); in 0x162 the sound is a 40% roll every 12 frames |
| A4 `0x59DD10` | 0x163 (restarts every 300..600 frames) | 11, block `0x11C9B74` | every 180 frames, 50% → 0x400A |
| A5 `0x59D5A0` | 0x15D/0x15E/0x15C | 4 `0x11C505C`, 11 `0x11C54F4` | 0x400F, 40% → 0x6033 |
| A6 `0x59E120` | 0x169 (the 50% roll is unused) | special model `D+0x650 = 0x11C830C`, `+0x654` AL_TOY texlist, `+0x65C = 1.0` | every 180 frames, 50% → 0x4010 |
| A7 `0x59E500` | 0x170 ↔ 0x16F, alternating every 60..120 frames | 4, `0x11C9AE4` | 0x4011 at frame 30 of 0x16F |

All skill A routines start with eyes 4 and mouth 3 (A5: eyes 1, mouth 1).

- **Skill B** (eyes 4; new motion on pulse 0x8):
  - B0 `0x59C870`: 0x171/0x173/0x175/0x177 at r<0.25/0.5/0.75/else.
  - B1 `0x59C980`: 0x179/0x17B/0x17D/0x17E at r<0.35/0.7/0.85/else.
  - B2 `0x59CA90`: 0x183.
  - B3 `0x59CB20`: 0x184, then 10% per end pulse → 0x185.
  - B4 `0x59CBF0`: 0x138 → (r<0.3) 0x13A → 0x13B×1 → 0x13C → 0x13D → 0x138, counted by end pulses.
- **Skill C** (`0x11C9BD4`): eyes 4, motion 0x121, on pulse 0x8 → 0x121/0x125. Mouth 3/2 every 30 frames.
  - Level (`0x59B900`) = (bit 0 of `rec+0x162` ? 0 : −1) + count of bits 1..4 set, so −1..4, and it indexes the table at `0x11C9BD4` = {`0x59A740`, `0x59A8A0`, `0x59AA30`, `0x59ABC0`, `0x59AD50`, `0x59AD50`, …} directly. Level 4 (bits 0..4 all set) **does** reach `0x59AD50`.
  - Every 241 frames a voice from `0x11C414C` = {0x607E, 0x6079, 0x607A, 0x607B} via `0x437590`. The index is 0 / trunc(r·1.99) / r·2.99 / r·3.99 for levels 1–4.
  - `0x59AD50` (level 4) alternates modes 1/2 (every 241/451 frames) and plays the special voice `0x11C415C` = 0x6080 (30% / 30%). It also ends if `0x54B1F0(0xA7)` finds nothing (`0x59AEA3`).
- **Animal-10 trick** (only when `rec+0x162` bits 0..4 are all clear and `rec+0x118 & 0x400`):
  - `0x59B0E0`: as skill C, every 181 frames a voice from {0x6076, 0x6077, 0x6078} (`0x11C40A4`), signal 0xA7.
  - `0x59B270` (dark): every 221 frames 0x607C, signal 0xA8.
- **Watchers** (`0x59E710` 0xA5, `0x59CDA0` 0xA6, `0x59B3D0` 0xA7): motion 0x181 (r<0.5) or 0x3F, eyes 4, mouth 3, re-chosen every 300..600 frames. When the signal disappears: motion **0x23D**, wait 180..300 → 1. `0x59CF50` (0xA6 variant 2) uses 0x17F and ends with **0x23E**.
- **`0x59B580` react to 0xA8:** posture 1..3 → 0x232 blend 35, else 0x3F. Eyes 1/3, mouth 8/4. Waits 300..600, then 50% → **rec+0x12D += 150** (clamped 200) → 1.
- **`0x5AB5D0` song chooser:** returns 0 when there are ≥ 100 song objects (`*0x1AEDC00`). 50%: a known `rec+0x160` bit → song id: bit 0 → 9; bit 1 → by type (hero 0xB, dark 0xC, else 0xA); bit 2 → 0xD; bit 3 → 0xF; bit 4 → 0xE. Otherwise, if `rec+0x164 & 0x100`: `0x5AB4B0`, which rewrites every bond like ≥ 50 to 50 (quirk); then 80% the song is one of those bond indices (0..5), else the type song (7 hero, 8 dark, 6 other).
- **`0x5AB2C0` "sing" (song id `D+0x41C`):**
  - Start: `0x54A690` (collision bits |= 0x70), eyes 4, motion 0x143 blend 20, block `0x11BB93C` on node 4.
  - End pulse → block `0x11BB95C` on node 11.
  - On pulse 0x8: signal **0xA9**, r 2.0, 1800 frames, at facing·3; `0x5AB200` spawns an object (main `0x5AAFB0`) carrying the song id; motion 0x145 blend 20.
  - Then 1800 frames, with sound 0x1026 30% of the time every 120 frames → 1.
  - Note: SetBehavior zeroes `D+0x41C`, so the song id must be written after SetBehavior.

### 2A.6 Animal plays (`0x11C4BE0[n]` when `rec+0x118` bit n is set; table read via REA)
| n | Routine | Behaviour |
|---|---|---|
| 0 | `0x5A2BF0` | motion 0x71. Position (`D+0x14/+0x1C`) += facing·**0.15** (`0x9058A8`) every frame. Count `120 + trunc(61r)` = 120..180 (`0x905F10`) end pulses → 1 |
| 1 | `0x5A2B10` | motion 0x141. Position += (facing−90°)·0.05/frame (sideways), 500..800 frames |
| 3 | `0x5A2860` | motion 0x135, 4..7 end pulses |
| 4 | `0x5A1F10` | motion 0x118, 2..4 end pulses |
| 5 | `0x5A2310` | 0x13E → 0x13F → 0x140 → 0x81 for 120..180 frames. Slides facing·0.3/frame in the last two modes. `D+0xB8 = 1.5` |
| 6 | `0x5A2230` | motion 0x12D. Position += facing·0.05/frame, 120..240 frames |
| 7 | `0x5A2E40` | 70%, with the nearest tree < **40** and state 3 or 5: target the tree, `Set(0x56BA80)`, push `0x56B6C0`, push `0x5A2DA0` (motion 0x120 for 300..500 frames). Otherwise → 1 |
| 8 | `0x5A2010` | 0x11A for 120..220 frames. Then r<0.4 → 1. Else r<0.4 → 0x11C + sound 0x6087, which waits for the hold pulse (`&8`) and then plays 0x11A for 160..280; else 0x11A again for 160..280 |
| 9 | `0x5A2460` | 0x3A/0x3C (50%), 600..900 frames |
| 11 | `0x5A29C0` | `Set(0x55C430)` + push **`0x5A2910`**, returns 0. `0x5A2910`: motion 0x136 (no blend), count `4 + trunc(3.99999r)` = 4..7 (`0x905A10`) end pulses → 1 |
| 12 | `0x5A25D0` | motion 1 for 120..240 frames. Then sound 0x1013, effect `0x6CC4E0` 3 units behind and 0.2 up, signal **0x97** (radius 5.0 `0xB5D210`, 60 frames) at that point; 120..240 more frames → 1 |
| 13 | `0x5A2A10` | 0x132, end pulse, then 4..7 end pulses moving facing·0.1/frame |

## 2B. Behaviour routines (part B)

Where Ghidra has the function (`0x565DC0`, `0x53A4B0`, `0x53A5A0`, `0x54A690`, `0x54A730`, `0x53D660`), the notes come from its decompiler; everything else is decoded from bytes.

**Conventions.**
- `r` = `rand()/32768` (`0x7A89D8` × `0x904E90` = 1/32768).
- "`N+trunc(K·r)`" is the game's `N - trunc(r·(-K-1))` idiom (FPU control word `|0xC00` = truncate), so the range is **N..N+K** (e.g. `0x905A00` = −201, `0x905A08` = −301, `0x905A20` = −121, `0x905A28` = −501). A few routines (whistle `0x5634D0`) use the plain `N + trunc(K·r)` form, range N..N+K−1; this is noted where it applies.
- Probabilities are written "r<p". Common cut constants: `0x904E60`=0.2, `0x9050C8`=0.3, `0x145D484`=0.3f, `0x904EB0`=0.4, `0x904E98`=0.5, `0x165DFD4`=0.5f, `0x8AF048`=0.6f, `0x109DFF0`=0.7f, `0x9050C0`=0.7, `0x905230`=0.8.
- `Eyes(id[,dur])` is `0x53A4B0`: **EAX = duration** (stored `D+0x68C`), **DX = id** (stored `D+0x692`). The common idiom `or eax,-1; lea edx,[eax+N]` therefore means **id = N−1**, duration −1 (until changed). `Mouth(id)` is `0x53A5A0`: id is pushed, EDX = duration. It writes only the duration `D+0x698` and passes the id to `0x55EAF0`; `D+0x69E` is the **default** mouth, not the current one (the displayed mouth lives in the face/brain state; the mischief "current mouth == 1" test reads that). `Icon(k,f)` is `0x53D660` (ECX=obj, DL=icon k, push f frames; also plays a sound when `*0x134046C==*0x134062C`).
- `Motion(id[,blend])` is `0x793C40` (EAX=`D+0xAC`, EDX=id), or `0x793D30` when a blend length is pushed. `MotionSet` is `0x793E40`.
- `Snd(id)` is `0x4372E0` (EDI=id, ESI=`D+0x14`). Volume is the 3rd push, 0x6E unless noted. It plays only when `*0x134046C==*0x134062C`.
- "mode" = `D+0x1B6`, "timer" = `D+0x1C0`, "cnt" = `D+0x1C4`.
- **End** = return 1 (pop). Return 0 = continue.
- "Motion end" means the bit tested in `D+0xAC`; the code uses either `&1` or `&8` (both noted). `&8` fires only for play-mode-3 (hold) motions, see §2A.0.
- Need names follow the debug labels: `rec+0x134` SLEEPY, `0x136` SLPDPTH, `0x138` HUNGER, `0x13A` BREED, `0x13C` TEDIOUS, `0x13E` LONELY, `0x140` TIRE, `0x142` STRESS, `0x146` CONDITN, `0x148` THIRSTY. All are clamped 0..10000 (`cmp eax,0x2710`). Emotion bytes `0x12C..0x133` are clamped 0..200 (`cmp eax,0xC8`).
- **Latch** = `if (u16 rec+0x12A==0) { rec+0x12A=1; <effects> }` (e.g. `0x5A1C52..0x5A1C62`, `0x59F6C9..0x59F6D5`). The latch means "apply once per need period"; the life tick resets rec+0x12A (§3.1).
- **Posture** = `D+0xAE` (MotionTable +2, see chao-garden-sync §2) = entry +6 copied to `D+0xAE` when a motion starts (not when a link begins): 0 standing, 1 sitting, 2 lying, 3 on the belly (crawl/trip), 4/5 other. Many behaviours pick their animation by it.
- **Stat ×3 rule**: stat points `rec+0x38+2i` are multiplied by 3 unless `*0x134046C` is 6, 10 or 11 (`0x562340..0x56236A`, `0x561B04..0x561B26`, `0x563407..0x563432`, `0x563A30..0x563A59`).
- **Walk drive**: on ground (`W+0x40&0x400`) the code writes `W+0xC = sin(rotY)·s`, `W+0x14 = cos(rotY)·s` (sin/cos `0x42AAB0`/`0x42AC30`, e.g. `0x5A0AD2..0x5A0AFF`).
- **Grab template** (fruit, animal, seed, hat): motion 0xCA (blend 15), timer 60, turn to `W+0x54` with `0x796910(rate 0x600)`. When the returned angle error is < 0xB6 (0x71C for animals), `0x486760(obj,kind)`. If the hit object's ALW entry `+6&2` is clear (not held by a Chao): `0x46E5E0` makes player 1 drop it, `0x56CFB0(ECX=item)` links it into the hands, then the next behaviour (hat: `0x5692C0..0x5692D7`).
  - Fruit, hat, seed: on a frame with angle ≥ 0xB6, or a touched item that another Chao holds, it returns 0 without counting down. The 60-frame countdown runs only while facing the item and touching nothing; when it runs out, End.
  - Animal: no window. Facing with no touch, or a Chao-held item, ends at once.
- `rec+0x118` is the "animals absorbed" bitmask (`|= 1<<animal id`, chao-garden-items §1.3). Tests below on `rec+0x118` bits therefore mean "has absorbed animal N".

### Hatching, release and holding
- **Newborn is `0x5685B0`.** The hatch code does `push -1` (`0x57AE60`), `push 0x5685B0` (`0x57AE62`), `call 0x53D890` (`0x57AE67`).
- **`0x53D720` and `0x564320` are picked by `*0x174AFFE`, not by flight/landing** (`0x565DC0`, decompiled + bytes). On release (Status `0x8000` clear):
  - `*0x174AFFE==0` (`0x565E3E`): build `(W+0, 0, W+8)`, normalise it if its length > 0 (`0x77FC30`/`0x77F3A0`), `D+0x18 += 2.0` (`0x904F78`), velocity `W = (nx·0.2, 0.87, nz·0.2)` (`0x904E60`, `0x128C454`=0.87f), `AL_SetBehavior(0x53D720)` (`0x565E8A`). This path does **not** set `D+0xA0|=8`.
  - flag set: if the player action list (`*0x1DE9600+4`, count `+0xC`) contains 0x14 → zero W, `W+0x18=0`, `AL_SetBehavior(0x565BE0)` (put down), `D+0xA0|=8`. Otherwise `W+0x18=0`, `AL_SetBehavior(0x564320)` (`0x565EFD`), `D+0xA0|=8` (`0x565F16`).
  - Only `0x564320` applies the throw penalties (`0x56451A`). chao-garden-sync §3 labels these "0x564320 dropped / 0x53D720 thrown". `*0x174AFFE` is written by game-mode code (`0x43A80B..`, `0x519A52..`); it looks like "player control on".
- `0x5A03B0` is not an approach: it is a 201-frame motion (below). `0x5A0490` is a bond tick, not movement.
- `0x5634D0` (whistle) does not walk: no walk drive is written; it turns with `0x796AB0(0, 0x300)` (`0x563660`) and then installs `0x563250` (`0x563779`), which walks to the player.
- `0x599210`, `0x599400`, `0x5995F0` and `0x599770` are installed by the toy picker with `AL_SetBehavior` (`0x599BDF..0x599C38`). Each acts on its first tick by replacing itself with a chain, or Ends when there is no toy.
- `0x5647B0` never returns 1 by itself (both exits `xor eax,eax`, `0x564878`). It ends through the 500-frame stack limit pushed by its interrupt `0x564760` (`push 0x1F4` at `0x564793`).
- While held **and on the release frame**, `0x565DC0` writes `rec+0x132 = min(old+200, 200)` unconditionally at entry (`0x565DE1..0x565E07`). Since the byte is ≥ 0, this always sets it to 200.
- **Held mood drift is driven by HUNGER, not bond** (`0x565DC0`): while held, `cnt` (`D+0x1C4`) counts up; when the old value > 600 (every 601 frames) it is reset to 0 and, checked in this order on the current behaviour:
  - HUNGER (`rec+0x138`) > 9999, current = `0x566300` (dislike) and `1 ≤ *0x134046C ≤ 3` → `0x46E5E0` (player drops it, if `*0x1DEA6C0`), `0x565B90`, `AL_SetBehavior(0x565BE0)` (the Chao jumps out).
  - HUNGER > 8000 and current = `0x5662B0` (neutral) → `AL_SetBehavior(0x566300)`.
  - HUNGER > 5000 and current = `0x5660A0` (like) → `AL_SetBehavior(0x5662B0)`.
  - Every held frame also zeroes `W+0..0x14`, eases `W+0x1C` (`0x446960`) and sets `D+0xC = W+0x1C − P.rotY`. Every call does `W+0x10 −= W+0x48`.

### Animal play (`0x5A2F10` table)
| Fn | Start | Run / end |
|---|---|---|
| `0x5A2CD0` (14) | Motion 0x23F; timer 400+trunc(200r) = 400..600 (`0x5A2D36`, const `0x905A00`=−201); Snd 0x6084 (`0x5A2D73`) | timer−− → End at ≤0 (`0x5A2D81`) |
| `0x5A27B0` (18) | Motion 0x11D (`0x5A27DC`) | after 20 frames, every frame `0x6ECA30(D+0x608, ½·node vec, 0.25)` (effect at hand node, `0x5A2801`); End on motion `&8` (`0x5A2846`) |
| `0x5A1FC0` (19) | Motion 0x12F | End on motion `&8` (`0x5A1FF9`) |
| `0x5A2530` (20) | Motion 0x187; timer 300+trunc(200r) (`0x5A256C`) | End at timer 0 |
| `0x5A2DA0` | Motion 0x120; timer 300+trunc(200r) (`0x5A2DC5`) | End at timer 0. Installed by `0x5A2E40` (animal play 7): nearest perceived tree (list 3 @`D+0x12A8`, entry stride 0x1C, dist at +8, object at +0x18) with dist < 40 (`0x8AE63C`=40.0f), r > 0.3 (70%, `0x5A2E90`, `test ah,0x41`) and tree `Data1+0x31` ∈ {3,5} (`0x5A2EBE..0x5A2ECA`). Then `0x5304F0` target, `AL_SetBehavior(0x56BA80,−1)`, push `0x56B6C0`, push `0x5A2DA0` (`0x5A2ED3..0x5A2EF7`). Any failed check → `0x5A2E40` returns 1 (`0x5A2F01`). |

### Toy skills (picked by `0x55E7A0`)
Skills 0, 4, 5, 6, 7 and 9 set Eyes 4 at start (`0x55DE1D`, `0x55E637`, `0x55DD4D`, `0x55DEED`, `0x55E70D`, `0x55DB4D`); drum and book set Eyes 4 only with r<0.5.
| Fn | Start | Run / end |
|---|---|---|
| `0x55DDE0` skill 0 | Motion 0x14A; attach model `0x12FA174` to node 11 (`0x5669B0`) | 600+trunc(200r) frames → End (`0x55DE40`) |
| `0x55E3E0` skill 1 drum | Motion 0x14F; r<0.5 Eyes 4. Drum: `D+0x660=0x12FD9DC`, `+0x664=0x1366AFC`, `+0x668=0.0` (`0x8AF004`=0.0f), `+0x66C=1.0` (`0x55E4F9..0x55E519`); timer 300+trunc(100r) (`0x905F20`=−101); mode 1 | mode 1: timer → r<0.2 End (`0x55E59B`); else r<0.5 Eyes 4, Motion 0x150, mode 2. mode 2: on motion `&8` → Motion 0x14F, r<0.5 Eyes 4, timer 380+trunc(180r) (`0x905F18`=−181), mode 1 (`0x55E40C..0x55E4AC`) |
| `0x55E210` skill 2 book | `[[D+0x2C]+0xC]+0x32 |= 0x70` (`0x55E2CD`); Motion 0x151; r<0.5 Eyes 4 (`0x55E2F6..0x55E30D`); timer 300+trunc(100r); create book `0x55E180` (EBX=obj) | mode 1: timer → r<0.3: book mode byte = 4 (closing), End (`0x55E3A8`); else book mode = 2, Motion 0x153, mode 2. mode 2: on motion `&8` → Motion 0x152, timer 120+trunc(180r), mode 1 |
| `0x55E600` skill 4 | Motion 0x148; attach `0x12F8364` to node 4 | on motion `&8`: r<0.2 End; else Motion 0x148 (r<0.6) or 0x149 (`0x55E688..0x55E6BA`) |
| `0x55DD10` skill 5 | Motion 0x14E; attach `0x12FBC5C` to node 11 | 600+trunc(200r) → End |
| `0x55DEB0` skill 6 | Motion 0x14B; attach `0x12F6D54` to node 11 | 600+trunc(200r) → End |
| `0x55E6D0` skill 7 | Motion 0x14C; attach `0x12FB41C` to node 11 | 600+trunc(300r) → End (`0x905A08`=−301) |
| `0x55DB10` skill 9 bubbles | Motion 0x146; attach `0x12FE33C` to node 4 and `0x12FE574` to node 11 (`0x55DB52..0x55DB6F`) | timer++. When old timer > 60 and new timer %10==0 and r<0.4 (`0x904EB0`): blow a bubble `0x55DA10` at hand node `D+0x510+0xD4..0xDC` + 0.5·dir(`+0x104..0x10C`), y +0.05, speed s = 0.2+0.1r (`0x904E60`, `0x905030`), each axis +0.1r (`0x55DB7B..0x55DCAA`). On motion `&8`: r<0.25 (`0x904EB8`) End, else Motion 0x146 and timer 0 (`0x55DCB4..0x55DCFD`) |

### TV, radio, horse, box
- **`0x599210` go to TV** (toy kind 2). Uses the nearest perceived toy (list 4 @`D+0x1640`). `0x55C4A0(EBX=&out)` gives the viewing spot; End if 0. Own ALW `+0x44` = TV entry; `W+0x54..5C` = spot. `AL_SetBehavior(0x55C3C0)`, then push `0x56B480`, `0x56BA80`, `0x56BBA0`, `0x56B560`, `0x56B6C0`, `0x598890` (inline pushes `0x5992CD..0x5993C2`).
- **`0x599400` go to radio** (kind 3). Same, with spot from `0x57C7A0` and last entry `0x598B90`.
- **`0x40AD80`** (kinds 4/5/6) is `mov eax,1` (REA `inspect_native_instruction`). It Ends at once.
- **`0x598890` watch TV**, `0x598B90` radio (identical code; the signal kind differs).
  - mode 0 (`0x598925..0x598ABF`): Motion r<0.3 → 0x3E, r<0.7 → 0x3F, else 0x1CB (`0x145D484`=0.3f, `0x109DFF0`=0.7f). If `rec+0x118&0x200` (animal 9 absorbed) and r<0.3 and current motion (`D+0xB0`) is not 0x3A/0x3C → Motion 0x3A (r<0.5) or 0x3C. Eyes 4 (r<0.5) or default `D+0x694`. Mouth 3 (r<0.5) or default `D+0x69E`. Timer 300+trunc(300r) (`0x905A08`); mode 1, and it falls into mode 1 the same frame.
  - mode 1: timer−− → mode 0 (re-pick next frame). The signal check comes after the pose timer. Every frame clears `0x19468A0/A4`, sets `0x19468A8/AC = −1`, then `0x486880(obj,1,0xC9)` (radio 0xCA). With no hit or hit `+4==0` (toy off) → Motion 0x23D, timer 120+trunc(90r) (`0x906260`=−91), mode 2 (`0x598AD5..0x598B78`).
  - mode 2: timer → toy cooldown `D+0x1F4 = 1800+trunc(1800r)` (`0x906268`=−1801), End (`0x5988BE..0x598918`).
- **`0x5995F0` go to horse** (kind 7). Ends if the horse's ALW partner is someone else. `W+0x54` = horse pos; `0x530640` links. `AL_SetBehavior(0x55C3C0)`, then push `0x56B480`, `0x56B560`, `0x56B500`, `0x598F40` (`0x599680..0x59973A`).
- **`0x598F40` ride horse** (jump table `0x5991FC`: `0x598F68` / `0x599041` / `0x599112` / `0x599166`).
  - mode 0: Motion 0x84; Eyes 4 or default (`0x598FB2`); Mouth 3 (`0x598FDE`) or `0x53A600`. `0x54A690` sets collision bits `|=0x70`. Hop: `W = (sin·0.08, 0.65, cos·0.08)` (`0x904F90`=0.08, `0xC71F38`=0.65; `0x598FF4..0x599037`); mode 1.
  - mode 1: once `vy<0` (`0x599041`: 0.0 vs `W+4`): blend 30 Motion 0x158; `D+0xA0&=~2`; timer 29; mode 2.
  - mode 2: timer → MotionSet 0x159; emit signal 0xAF (radius 3.5 `0x16AFE94`, 10 frames) via `0x55A920`; timer 1800; mode 3.
  - mode 3: timer → `D+0x1F4 = 1800+trunc(1800r)`; `D+0xA0|=2`; `AL_SetBehavior(0x598E90)` (`0x599179..0x5991E5`), then **returns 1**: the tick pops the fresh one-entry queue and resets to `0x565BE0`, so the dismount `0x598E90` never runs from the ride.
  - Every frame while the link is mutual: in modes 1–3, rotY approaches the horse rotation by 0x400 (`0x446960`, `0x5990B2..0x5990BD`); in modes 2–3, pos += 0.1·(horse − pos). A broken link → `AL_SetBehavior(0x565BE0)` and End (same pop as mode 3).
  - Horse main `0x580BC0`: off → mode 1, motion 3 for 1800 frames, then motion 4 and mode 0. A non-mutual link while rocking → stop (motion 4), unlink, mode 0.
- **`0x598E90` after horse** (`0x598E90..0x598F33`). While mode==0 (never incremented): MotionSet 0x84; `W = (sin(rotY+0x4000)·0.3, [0xDF21B4]=0.8, cos(rotY+0x4000)·0.3)` (`0x9050C8`=0.3); collision `|=0x70`. Returns `(W+0x40>>10)&1`, i.e. Ends when the on-ground bit 0x400 is set (`0x598F23..0x598F2C`). The velocity is rewritten every frame, so if it starts in the air it rises forever (vy 0.8). Unreached from the ride (above).
- **`0x599770` go to box** (kind 8). ALW `+0x44` = box. `AL_SetBehavior(0x55C3C0)`, then push `0x56B480`, `0x56BA80`, `0x56BBA0`, `0x565BE0` (`0x5997D7..0x599890`). No box behaviour of its own.

### Moods and emotions
- **`0x5A1BA0` rest (tired).**
  - mode 0 (`0x5A1D1D..0x5A1EFF`): timer 120+trunc(500r) (`0x905A28`=−501). Motion (blend 40) by r: <0.2 → 0x3E, <0.3 → 0x41, <0.4 → 0x3F, <0.5 → 0x40, <0.6 → 0x41, <0.7 → 0x42, <0.8 → 0x43/0x44 (r<0.5 → 0x43) with timer 120+trunc(121r) = 120..240 (`0x905A20` = −121), else 0x47 with timer 60+trunc(121r) = 60..180. mode 1.
  - mode 1, each frame: timer−−. Latch {TIRE −= 100·(300+p)/300; CONDITN += 30·(300+p)/300}, p = `(i8)rec+0x151`, constants `0x1312DC0`=100, `0x1312DDC`=30, division by 300 via magic `0x1B4E81B5`/`sar 5` (`0x5A1C47..0x5A1D10`).
  - When the old timer ≤ 0: End if HUNGER>8000, SLEEPY>8000, TEDIOUS>8000, LONELY>8000 (`0x1312EB8`=8000) or TIRE<100; else End with r<0.3; else mode 0 and the latch block still runs that frame (`0x5A1BD6..0x5A1C43`).
- **`0x59F350` (emotion 0x12F behaviour)** (`0x59F37B..0x59F486`). Motion 0x1F1 (r<0.5) or 0x1F2 (no blend); **Eyes 1** (`0x59F3B6..0x59F3BE`, `lea edx,[eax+2]`); Mouth 8; timer 180+trunc(300r). Every frame where timer%150==0 (including the start frame) r<0.5 → Snd 0x602D. timer−− (`add [timer],-1; jne`): End exactly when it reaches 0.
- **`0x59F5F0` follow-up when player seen (flee)** (`0x59F5F0..0x59F859`).
  - mode 0: blend 10 Motion 0x76 (r<0.5) or 0x6A; **Eyes 1** (`0x59F651..0x59F65C`); Mouth 8; `0x437590` sound 0x602E (pushes 1, 0x6E, 0x78); `0x59F490` picks the flee target; mode 1, timer=cnt=0.
  - Each frame: Latch {TIRE += 900 (`0x1312DC4` u16=900)}; timer++ → End if old > 1000. When the new timer % 100 == 0 (`0x59F719`), r<0.03 (`0x905000`) → `AL_SetBehavior(0x562D50)` (stumble); the frame continues (`0x59F768`).
  - Arrived (xz dist² to `W+0x54` < 64, `0x905078`=64.0f): End if player dist² ≥ 2500 (`0x16B643C`=2500.0f); else mode 0 (flee again). Wall (`W+0x40&0x4000`) → mode 0.
  - `0x796910(0x120)`; walk drive s=0.08 (`0x1312C70`); motion speed `D+0xB8 = |W|·8` (`0x77FC30` × `0x905008`=8.0; `0x59F835..0x59F850`).
  - Flee target `0x59F490` (checked from bytes): zone = `0x535010(pos)`. When the zone ≠ 0: n = xz-normalised away vector pos − player (`0x77F3A0`); target = pos + 30·n (`0x905090`), keeping its own y. If that lands in another zone, turn 90°: r<0.5 → (x+30·nz, z−30·nx), else (x−30·nz, z+30·nx). If it still crosses zones, take a point from zone list `0x1A15938+zone·0x1004` (`0x534F80`; the list holds the cells whose value is that zone). When the zone = 0, use the global list `0x1A1693C`. Written to `W+0x54..5C`.
- **`0x59F8E0` (emotion 0x12E dispatcher).** If `rec+0x12E` > 120: 0x59FA50 (r<0.5) or 0x59FCA0. Otherwise 0x59FA50 (r<0.5) or 0x59F970 (`0x59F8EB..0x59F963`). Its picker `0x59F860` runs only when `rec+0x12E` > `[0x1312E18]` and a personality-scaled score (`(i8)rec+0x14C`+100, `0x9055A8`, `0x1312E1C`) beats the current best (`0x59F860..0x59F8DA`).
- **`0x59F970`** (0x12E variant, low). Posture 1..3 → blend 35 Motion 0x1F9, else blend 30 Motion 0x1F6. Eyes 3 (`0x59F9C8`); Mouth 4; 180+trunc(120r) frames → End.
- **`0x59FA50`** (0x12E variant).
  - Start: posture 1..3 → r<0.3 → 0x225, <0.6 → 0x227, <0.8 → 0x228, else 0x229; other postures → 0x220 / 0x222 / 0x223 / 0x224 with the same cuts (blend 35). Eyes 3 or 1 (`0x59FB9D`); Mouth 4 (r<0.5) or 6. Timer 100+trunc(100r).
  - On old timer ≤ 0: `rec+0x12E −= 10` (clamp 0..200); End if < 10, else mode 0 (`0x59FC31..0x59FC8B`).
- **`0x59FCA0`** (0x12E variant, ≥121).
  - Start: posture 1..3 → r<0.4 → 0x210, <0.8 → 0x213, else 0x215; other postures → 0x20B (r<0.5) or 0x20D (blend 35). Eyes 3/1 (`0x59FD9A`); Mouth 4 (r<0.4) or 6. Timer 100+trunc(100r).
  - On motion `&8` or old timer ≤ 0: `rec+0x12E −= 10`; End if < 10, else mode 0 (`0x59FE31..0x59FE94`). The timer is not decremented on a frame whose motion ended (`&8`).
  - While timer%8 < 4: two `0x6ED270` effects at body nodes (`D+0x510+0x110..`, y−0.2, scale `[0xB5D144]`=0.1f; `0x59FEB9..0x59FF9B`). On every non-ending frame with `D+0x30 % 180 == 0`, r<0.3 → Snd 0x6001.
- **`0x5A00A0` (emotion 0x131)** (jump table `0x5A0398`).
  - mode 0: if `rec+0x131` > 100 → blend 35 Motion 0x230; Eyes 6 (50%, `0x5A0110`); Mouth 8 (r<0.5) or 4; timer 240+trunc(180r); mode 1. When `rec+0x131` ≤ 100: mode = 2 and it runs on into the mode-1 countdown with timer 0, so `rec+0x131 −= 10` happens at once (`0x5A01A6..0x5A01B2`).
  - mode 1: old timer ≤ 0 → `rec+0x131 −= 10`, mode 2 (`0x5A01C9..0x5A01F4`).
  - mode 2: Motion r<0.33 → 0x232, <0.66 → 0x233, else 0x234 (blend 35); Eyes 3/1; Mouth 4; timer 120+trunc(200r); mode 3.
  - mode 3: old timer ≤ 0 → cnt++ (End when the old cnt > 4); `rec+0x131 −= 20`; End if < 10, else mode 2 (`0x5A0319..0x5A038A`).
- **`0x5A0BA0` (emotion 0x12C)** (jump table `0x5A0F28`). Picker `0x5A0B10` requires `rec+0x12C` > `[0x1312E30]`=100 and a score with `(i8)rec+0x155` (`0x5A0B17..0x5A0B6F`).
  - mode 0: r<0.2 → Motion 0x1D4, mode 1; r<0.6 → 0x1CF, mode 3; else 0x1EC, mode 4. Eyes 1 (r<0.5) or 4 (`0x5A0C8D`). Mouth 3/5/2 at cuts 0.4/0.8. With r<0.7, a second rand picks Snd 0x602F/0x6030/0x6031/0x6032/0x6033 at cuts 0.2/0.4/0.6/0.8 (no second rand otherwise). Timer 120+trunc(60r) = 120..180 (`0x905F10` = −61). Mode 0 runs on into the mode-1 code the same frame (`0x5A0E29`).
  - Timers are decremented before the test. mode 1: timer → Motion 0x1D6, mode 2. mode 2: motion `&1` → mode 5. mode 3: timer → mode 5. mode 4: waits for the timer, then for the motion end (`&1`) → mode 5.
  - mode 5: cnt++ (End when the old cnt > 8); `rec+0x12C −= 20`; End if < 10; else mode 0 (`0x5A0EBF..0x5A0F1A`).

### Life events
- **`0x568D30` cocoon / evolve** (jump table `0x568F0C`; counter = `D+0x1C0`; a step ends when the old counter > limit).
  - mode 0: `0x54A690` (collision `[[D+0x2C]+0xC]+0x32 |= 0x70`) + `0x54A730` (Status `&= 0xFDBF` i.e. `~0x240`, and collision `+4 |= 0x10`, `+0x64 |= 0x10`). Motion by posture: 0 → 0x2F; 2 → 0x2A; 3 → 0x2A blend 35; else 0x3E (blend 15).
  - mode 1 (200, `0x1312CCC`=200) → `0x568CD0(obj,0)` creates the cocoon task (`0x470C00(2, main 0x568890)`, stored at `D+0x34`).
  - mode 2 (1800, `0x1312CD0`=1800) → evolution `0x568630`, then `0x56CF40`.
  - mode 3 (900, `0x1312CD4`=900) → `0x568D10` (cocoon byte = 4) and a jingle: type 20..22 → `0x12BEEBC` (`0x568E6D`); else `0x535360` → `0x12BF2D4` (`0x568E89`); else `0x535390` → `0x12BF6EC` (`0x568EA5`).
  - mode 4 (900, `0x1312CD8`) → mode 5. mode 5 (100, `0x1312CDC`) → `0x54A6C0` + `0x54A6F0` (restore), End.
  - All waits compare unsigned (`jbe`).
- **`0x568F60` death / rebirth** (jump table `0x569180`).
  - mode 0: untouchable (`0x54A690`, `0x54A730`); Eyes 3 (`0x568FA0`); Mouth 4; Icon(5,300). Posture 0 → 0x2F; 1 → 0x3E; 2 → 0x2A; 3 → 0x3E blend 45.
  - mode 1 (>200): cocoon `0x568CD0(obj, 0x568F30()?1:2)`. `0x568F30` = happiness `(i16)rec+0x82` > 50 (`0x1312CEC`=50, signed) && STRESS `rec+0x142` < 700 (`0x1312CEE`=700, unsigned) (`0x568F30..0x568F5E`).
  - mode 2 (>1800): `D+0xA0 &= ~0x200, ~0x100000, ~0x80000` (`0x569080..0x56909A`). If `0x568F30` (`0x5690A4`) → `0x568570` (`rec+0x8E++`, `0x551560(&DNA)`, `0x5307F0` unregister, egg `0x57B9C0(0,rec,0,&pos,2)`) and mode 5. Otherwise jingle `0x12F5944` (`0x443480`) and mode 3. Counters reset; `D+0xA0 &= ~0x200` again.
  - mode 3 (>900, `0x1312CD4`) → `0x568D10`, mode 4. mode 4 (> `[0x1312CD8]`+30 = 930) → `0x550620` (`0x569136`; apparently clears the 0x4DC-byte record and sets MainSub = `0x46F720`).
  - mode 5 (>900) → `0x568D10`, mode 6. mode 6 (>930) → MainSub = `0x46F720` (`0x569170`; rec kept for the egg).
- **`0x5685B0` newborn**. Blend 15 Motion 0xCE; Ends on motion `&1` (`0x5685B0..0x5685EE`).
- **`0x59BB50` breed (caller)** (jump table `0x59BF08`). Every frame `D+0x3C=1`.
  - mode 0: `0x54A690`; `0x5AAF60` (child effect task, main `0x5AAE20`); `D+0x40` = `0x55A920(obj, 0xAB, r=30.0 (0x89E4DC), frames 9000−120)` (`0x59BB85..0x59BBB5`).
  - mode 1: blend 15 Motion 0x3E; Eyes 4; Icon(4, 8700).
  - mode 2: timer++. At 4500 → BREED=0; at > 9000 → End. Mailbox `0x530710` == 1 and mutual Chao partner → target = partner pos, Motion 0x30, mode 3, signal task deleted (`D+0x40` MainSub=`0x46F720`).
  - mode 3: after 30 frames `0x796910(0x400)`. On motion `&1`: partner still mutual → Motion 0xFC, mode 4; otherwise End.
  - mode 4: mailbox == 1 → Motion 0x1D4, BREED=0, mode 5; motion speed `D+0xB8=1.5` (`0x1283DDC`=1.5f).
  - mode 5: at timer 20 hop `W += (−sin·0.25, +0.8, −cos·0.25)`. On motion `&1`: unlink, Motion 0x1EC, `cnt=3+trunc(3r)`, mode 6.
  - mode 6: each motion `&1` timer++ → End when ≥ cnt.
- **`0x59BF30` court nearby 0xAB (partner side; makes the egg)** (jump table `0x59C3B4`).
  - mode 0: `0x54B1F0(1, 0xAB)` must find the caller, else End. Posture 1 → 0x30, 2 → 0x26, else 0xEE.
  - mode 1: on motion `&1`: Motion 0x78; Eyes 4 (`0x59BFFF`); Icon(4,400); target xz = caller pos; `0x5305D0` one-way partner.
  - mode 2: mailbox 3 → End. `0x796910(0x200)`; walk drive `+= 0.038` (`0x1312C6C`=0.038f). When signal 0x81 is the partner and the partner is free: `0x530640` link, mail 1, jingle `0x11C3FF4` (`0x59C10F`), Motion 0xFC, mode 3. Speed `D+0xB8=1.4` (`0xB18F54`). Lost 0xAB, or `D+0x1C8` (3600) runs out → End.
  - mode 3: mutual Chao partner required; after 300 frames: mail 1, Motion 0x1D4, BREED=0, mode 4 (sets `D+0xB8 = 1.5`, `0x59C1F5`).
  - mode 4 (`0x59C206..0x59C373`): when the old timer == 20 (frame 20): hop `W += (−sin·0.25, +0.8, −cos·0.25)` (`0x904EB8`=0.25, `0x905230`=0.8); then if `0x5306F0` returns a partner with an object → genes `0x551D20` (`0x59C2B7`) → free slot `0x531AA0` → egg `0x57B9C0(&genes, slot, 0, pos + 2·(sin,·,cos), 1)` (`0x59C2D7`; the ×2 is `fadd st,st`). On motion `&1`: unlink `0x530690`, blend 15 Motion 0x1EC, `cnt=4+trunc(3r)` (`0x905A10`), mode 5.
  - mode 5: each motion `&1` timer++ → End when ≥ cnt (`0x59C375..0x59C396`).

### Dark-adult mischief and lonely
- **`0x5A1460` mischief A.** Blend 20 Motion 0x203; Eyes 10 (`0x5A14AB`); Mouth 9 if current mouth == 1, else 4; timer 180+trunc(300r). Every 150 frames r<0.5 → Snd 0x6065 (r<0.33), 0x6066 (<0.66) or 0x6068. timer−− (`add [timer],-1; jne`): End exactly when it reaches 0.
- **`0x5A18A0` mischief follow-up (pester player).** Eyes 10 at `0x5A18E8`; cuts and the far test checked from bytes.
  - mode 0: blend 10 Motion 0x7D; Eyes 10; Mouth 9/4. End if `rec+0x12D` < 10.
  - Each frame: Latch (no effect); timer++ > 1000 → End.
  - Player dist² < 25: `rec+0x12D −= 10`. `AL_SetBehavior` of 0x5A1600 (r<0.35f, `0xA3D28C`), 0x5A1780 (<0.7f, `0x109DFF0`), 0x5A1680 (<0.9, `0x904E28`) or 0x5A1700, then push 0x5A18A0. With r<0.6, a second rand picks Snd 0x606F/0x6070/0x6071/0x6072 at cuts 0.25/0.5/0.75.
  - After the near branch: player dist² > 100.0 (`0x904E30`) → `AL_SetBehavior(0x5A1460)` with no follow-up.
  - `0x796AB0(player 0, 0x120)`; walk drive s=0.055 (`0x1312C68`) (`0x5A1939..0x5A1B8C`).
- **`0x5A1700`, `0x5A1600` / `0x5A1680` / `0x5A1780`.** Blend 20 Motion 0x1BB / 0x1B9 / 0x1C3 / 0x1BD; Eyes 10 (`0x5A1743`, `0x5A1643`, `0x5A16C3`, `0x5A17C3`); Mouth 9/4; End on motion `&1`.
- **`0x5A1800` dislike.** Blend 20 Motion 0x1BF; Eyes 1 (`0x5A1842`; the id comes from EDX left over from `0x5A1831`); Mouth 4; Snd 0x6069; End on motion `&1`.
- **`0x5A03B0`.** Alignment `rec+0xB0` > 0.5f (strictly) → blend 20 Motion 0x18C + Snd 0x6091 (vol 0); else 0x18D. Eyes 4 (`0x5A0440`); Mouth 3; 201 frames → End (`0x5A03D3..0x5A047D`).
- **`0x5A0490`.** `0x53E280(ECX=obj)` (bond counter for the current character c = `0x53E0C0`: 1..5 by character, 0 with no player), then End. `0x53E280`: `bond+4 += 1` capped 0xFFFF; `bond+2 = 2·⌊v/3⌋` (magic `0x55555556`, then `add eax,eax`), not ×2/3.
- **`0x5A0960` run to player** (`0x5A0960..0x5A0B09`).
  - mode 0: blend 10 Motion 0x75; Eyes 4 (`0x5A09A4`); Mouth 3; mode 1, timer=cnt=0. Then, in this same mode-0 frame only, if `rec+0x12C` > 180 (`0xB4`, `0x5A09D9`) → LONELY −= 6000 (`0x1770`) and End.
  - Each frame: timer++ → End if old > 1000. Player-1 (`*0x1DEA6C0`) 3D dist² < 25 (`0x904F70`=25.0) → `rec+0x12C += 10` (`0x5A0A89`), `AL_SetBehavior(0x5A0820)`, push 0x5A0960 (`0x53D970`).
  - `0x796AB0(ECX=0, 0x120)`; drive 0.055 (`0x1312C68`).
- **`0x5A0820` near player** (`0x5A0820..0x5A095F`). Blend 20 Motion 0x190; Eyes 3 (`0x5A0859`); Mouth 3. On motion `&1`:
  - player dist² < 25 → `rec+0x12C += 10` (`0x5A08C7`) and `AL_SetBehavior(0x5A0820)` (restart). If `rec+0x12C` > 140 (`0x8C`, `0x5A0905`) → LONELY −= 6000, End. That return 1 comes right after the restart's `AL_SetBehavior`, so it pops the fresh stack and the Chao settles (`0x565BE0`).
  - otherwise `AL_SetBehavior(0x5A0960)`.

### 0xAE target, seeds and watering
- **`0x567FA0` go to 0xAE target.**
  - Start: blend 15 Motion 0x64; Eyes 0 (`0x567FD6`); Icon(1,300); `D+0x1C8 = 1800+trunc(120r)`.
  - Each frame: `D+0x1C8` → End. Turn to the ALW target `0x530510(0x280)`. Distance `0x530580` < 4.0 (`0x104DD30`) → `AL_SetBehavior(0x5680F0)`. Drive 0.055·0.8; speed `|W|·18`.
  - **No-target case (corrected):** `0x530580` (`0x530580..0x5305C0`) starts with `push ecx`, so `[esp]` holds the caller's ECX (the Chao object). When the ALW target chain (`[ecx+0x3C]→+0x44→+0x3C`) is missing it loads `eax=[esp]` (the Chao itself, non-null) and returns `0x77FBD0(D+0x14, D+0x14)`, i.e. the distance from the Chao to itself, 0.0. The −1.0 branch (`0x5305B6`, `0xB087CC`=−1.0f) is reached only if that saved ECX is null. Either value is < 4.0, so the switch to `0x5680F0` happens at once.
- **`0x5680F0` 0xAE activity / watering** (jump table `0x568440`).
  - mode 0: blend 15 Motion 1; timer 60+trunc(60r); collision `|=0x70`.
  - mode 1: timer → Motion 0xD; Icon(1,120); Eyes 4 (`0x5681D0`); Snd 0x601A; timer 60+trunc(40r) (`0x907EB0`=−41). `0x53E360(ECX=11)` (toy skill bit 11, `0x568243`) set → mode 2 (`0x568258` → `0x56817E`). Not set: THIRSTY += 2000 (`0x56825E..0x568271`) and End.
  - mode 2: timer → Motion 0x64, timer 150.
  - mode 3: `0x54B0E0(obj, −0.015)`; turn `0x530510(0x200)`. Timer → Motion 0xEB; block `0x12F5974` with model `0x12FCA1C` on node 11 (`0x566A20`); timer 240+trunc(100r).
  - mode 4: timer → Motion 0xED.
  - mode 5: motion `&1` → THIRSTY += 2000, `AL_SetBehavior(0x568460)`.
- **`0x568460` after it.** Motion 0xDB blend 15 (again on motion `&8`, `0x5684F2..0x56850D`); collision `|=0x70`. On the second motion `&8`: `0x55A850(0xAD, pos + 4·(sin,0,cos), r=5.0, 10 frames)` (`0x904EA8`=4.0, `[0xB5D210]`=5.0f, push 0xA, push 0xAD at `0x5684DA`), a watering signal (trees read 0xAD, chao-garden-sync), then End.
- **`0x567640` pick up 0x85 (a seed).** Grab template with kind 0x85, next `0x567710`. Interrupt `0x567560`: not while sleeping (0x54EF10) or eating (0x5607C0); run points (×3 rule) ≥ 150 (`0x1312D6E`=150); no mutual partner; item not targeted by another Chao (`0x5304D0`).
- **`0x567710` hold.** Motion 3; Eyes 0 (`0x56774C`); Icon(3,300); Snd 0x601C; `D+0xA0 = (&~0x800)|0x400`; timer 300+trunc(120r) → `AL_SetBehavior(0x567830)`. Ends if the item is lost (`0x56CFF0`==0).
- **`0x567830` carry.**
  - mode 0: Motion 0xBF; Eyes 0 (`0x567A18`); `D+0xA0|=0xC00`; `D+0x1C8 = 1800+trunc(400r)`; target from list `0x1A1693C` (`0x534F80`).
  - mode 1: if 0xAC is found (`0x54B1F0`) with dist < 20 + intel·0.001 (`rec+0x44`, ×3 rule) → set target, Snd 0x601A, mode 2. Otherwise `0x796910(0x100)`; on arrival (xz dist² < 64) r<0.7 → `AL_SetBehavior(0x567710)`.
  - mode 2: turn `0x530510(0x280)`; dist < 2.5 → `AL_SetBehavior(0x567B10)`.
  - `D+0x1C8` → End. Drive 0.044; speed `|W|·18`.
- **`0x567B10` plant** (jump table `0x567F84`).
  - mode 0: Motion 0; Eyes 4 (`0x567B4F`); Icon(1,180); hand 0x400; collision `|=0x70`; timer 120+trunc(120r).
  - mode 1: turn `0x530510(0x100)`. On old timer ≤ 0 → `0x53E360(ECX=10)` (toy skill bit 10, `0x567C12`) clear: Icon(5,180), End (`0x567C24..0x567C35`). Set: Motion 0xE5 and block `0x12F5954` (model `0x12FDE44`) on node 11; timer 180+trunc(120r).
  - mode 2: timer → Motion 0xE7.
  - mode 3: Snd 0x1023 every 35 frames. On motion `&1`, with a partner from `0x5306F0` (the held seed): read seed kind = `(u16)seed Data1+8`; find 0xAC via `0x54B1F0(1, 0xAC)`; the 0xAC object gets the seed's rotation (`Data1+0xC`), its ALW mailbox `+0x34=5`, `+0x36=seed kind` (`0x567DDA..0x567DE8`; = the GrowTree planting command); `0x530690` unlink; `0x5304F0` target the spot. Free the seed record (`0x52F680`) and clear its pointer; seed MainSub = `0x46F720` (delete) (`0x567DFC..0x567E34`). Motion 0xE8 (blend 15); timer 180+trunc(180r).
  - mode 4: Snd 0x1024 every 35 frames; Snd 0x6085 (vol 100) every 60 frames with r<0.5. Timer → Motion 0xEA.
  - mode 5: motion `&8` → `AL_SetBehavior(0x5680F0)`.

### Swimming (`0x562330` and helpers)
- **`0x562330` dispatcher** (`0x562330..0x56243D`). swim = `rec+0x38` (×3 rule, `0x562340..0x56236A`); `0x561740` picks a swim target; collision `[[D+0x2C]+0xC]+0x64 |= 0x10`.
  - swim < 300 (`0x1312D7E`=300, `0x56238C`) → `AL_SetBehavior(0x561E00)` only.
  - Otherwise < 2000 (`0x1312D80`=2000, `0x5623A1`) → `0x561FB0`, else `0x562040`. Then if `rec+0x118&4` (animal 2 absorbed, `0x5623C4`) and r<0.5 → `0x5620D0`. Then r<0.2 (`0x562411`) → `0x562160` (overrides).
  - Then calls `0x561AF0` once (`0x562431`), returns 0.
- **`0x561740` swim target.** THIRSTY < 100 → point from list `0x1A1693C`; with an empty land list the target is left as it was. Otherwise pos + (0.3+0.7r)·60 at angle rot − (0.5−r)·200° until `0x535010(p)==9`, 100 tries in all (1+99); on failure x=z=0, y = own y. `W+0x1C` = bearing.
- **`0x561AF0` swim step** (EAX=obj; used by `0x561E00`, `0x561FB0`, `0x562040`, `0x5620D0`; `0x561AF0..0x561DFA`):
  - Not in water (`D+0x1B4&4` clear) and on ground (`W+0x40&0x400`) → `AL_SetBehavior(0x562D20)`.
  - cnt (`D+0x1C4`) is incremented **twice per frame** (`0x561B63` and `0x561C33`); on frames where the first old value %10==0 it calls `rand()` and discards the result (`0x561B6E`, consumes RNG).
  - Wall (`W+0x40&0x4000`): THIRSTY < 1000 → if touch 0xCB (`0x486760`) → `AL_SetBehavior(0x562EB0)`, else nothing. THIRSTY ≥ 1000 → r<0.002 (`0x9055B0`) → re-dispatch `0x562330` (`0x561B73..0x561BE1`).
  - xz dist² to `W+0x54` < 36 (`0x8AE5DC`=36.0f) → re-dispatch. cnt old value > 600 (`0x561C2E`) → re-dispatch; with the double increment that is after ~300 frames.
  - `0x796910(0x120)`.
  - When below the surface (`D+0x18` vs `W+0xDC` − 2.1, `0x906C10`): thrust = swim·1e-6 + 0.0025 (`0x1312D88`, `0x1312D84`); y += 0.1 (`0x905030`) clamped to surface−2.1; `W+0x10 = −W+0x48·…`; `W+0xC = sin·thrust − W+0·0.05`, `W+0x14 = cos·thrust − W+8·0.05` (`0x9050D8`=0.05) (`0x561C5B..0x561D11`).
  - Latch {THIRSTY −600 (`0x1312DE0`), TEDIOUS −100 (`0x1312DB2`), TIRE +100 (`0x1312DC6`)}; `0x437590` sound 0x101F (pushes 1, −25, 0x5A) every frame, outside the latch (`0x561D15..0x561DEB`).
- **`0x561920` float step** (used by `0x562160`). Leaving water → `0x562D20`. Rise 0.02/frame; `a.xz = sin·0.0 − v·0.05` (`0x904E00` = 0.0). Latch {THIRSTY −600, TEDIOUS −100, TIRE −100}; sound 0x1021 via `0x437590` every frame.
- **`0x561E00`.** Eyes 1 (`0x561E28`, EDX=EBP). If the default mouth `D+0x69E` is 1: r<0.25 → Mouth 9. Otherwise Mouth 6/8/0xB/4 at cuts 0.25/0.5/0.75. Motion 0x8E. Every 120 frames r<0.2 → Snd 0x6024. Latch {THIRSTY −6000}. Then step `0x561AF0`. EBP = 1 at the eyes call (eyes 1, duration −1).
- **`0x561FB0` / `0x562040` / `0x5620D0`.** r<0.2 → Eyes 1; r<0.4 → Eyes 4 (`0x562010`, `0x5620A0`, `0x562130` call with EDX=4). Motion 0x94 / 0x91 / 0x92. Then step `0x561AF0`.
- **`0x562160` float** (jump table `0x562314`).
  - mode 0: blend 20 Motion 0x9A.
  - mode 1: after 30 frames: Eyes 1 for 120 frames (`0x5621BF`, EAX=0x78, EDX=1); Mouth 2 (120); Snd 0x6025 (vol 0). Mode 1 jumps to the mode-2 check when the count's old value ≤ 30 (`0x5621B3` jle `0x562203`), so the motion-end test runs on mode-1 frames too. Its mode +1 then takes mode 1 to 2, not 3. The motion-end path calls `0x561920` and returns without running mode 3.
  - mode 2: on motion `&1`: Eyes 3 (r<0.5) or 4 (r<0.8) for 120 frames (`0x562261`); Motion 0x96; timer 600+trunc(600r).
  - mode 3: timer → `AL_SetBehavior(0x562330)`.
  - Each frame `0x561920`.
- **`0x562EB0` climb out.** Triggered by a 0xCB touch, i.e. a C_CLIMB box (garden-collision.md §8). Modes 1–2 checked from bytes.
  - mode 0: Motion 0x9B; `W=0`.
  - mode 1: while touching a wall (`W+0x40&0x4800`): rotY approaches the wall heading by 0x800 and the 0x4000 bit is cleared. `W+0x9C`/`W+0xA4` are x,z of the faced wall's normal (`0x796CA0` stores the wall at `W+0x90..0xA4` with bit 0x4000); heading = 0x8000 − trunc(atan2(nx,nz)·−10430.38) = 0x8000 + angle(n), into the wall. Otherwise after 3 frames `D+0x54 = y` and mode 2.
  - mode 2: timer > 3 on ground with ground-normal check `0x77FBA0` > 0.7 → `0x562D20`. After that wander the code goes on: the >30 / rise >2.5 check and the frame's move still run (count `D+0x1C4` restarts from 0). Wall again → mode 1. Timer > 30 or rose > 2.5 → `0x562D20`.
  - Every frame: pos += (sin·0.1, 0.08, cos·0.1); `W+0x10 = −W+0x48`; cnt > 240 → End.

### Player interactions
- **`0x5634D0` come to whistle** (jump table `0x563790`, 6 entries). Interrupt `0x5637B0`: player-1 (`*0x1DEA6C0`) action byte `P+0` == 0x2A (42), 3D dist² < 10000 (`0x905664`=10000.0f), and bond like `(i8)rec+0x16C+6·c` > 50 (c = `0x53E0C0()`, `0x5637F3..0x563801`) → `AL_SetBehavior(0x5634D0)`. **The interrupt returns 1 whenever the whistle is heard in range, even when like ≤ 50 and nothing is set** (`0x563809` → `0x56381C`).
  - mode 0: timer = 20 + trunc(40r) (plain form, 20..59, `0x9053B8`=40); blend 10 Motion 0; mode 1.
  - mode 1: timer → blend 10 Motion 0xEE (r<0.5) or 3; Icon(3,180); timer 20 + trunc(60r) (`0x905100`=60); mode 2.
  - mode 2: timer → blend 10 Motion 0x64; mode 3.
  - mode 3: speed `D+0xB8=1.5` (`0x1283DDC`); `0x796AB0(ECX=0, 0x300)` (`0x563660`). When it returns < 0x100: Icon(1,60); timer 20 + trunc(20r) (`0x9050B0`=20). With r<0.7 (`0x9050C0`): blend 10 Motion 0x1EC, Eyes 4, mode 4, Snd 0x6019; else mode 5.
  - mode 4: motion `&1` → mode 5.
  - mode 5: timer → `AL_SetBehavior(0x563250)` (`0x563779`).
- **`0x563250` walk to player** (`0x563250..0x5634CB`).
  - mode 0: blend 10 Motion 0x76 (r<0.2), 0x77 (<0.6, `0x8AF048`) or 0x6A; Eyes 4; Mouth 1 (r<0.4) or 3; Icon(4,120); mode 1, timer 0.
  - Modes 0–1, each frame: target `W+0x54..5C` = player-1 pos. All modes: timer++; every 100 frames r<0.1 (`0x905030`) → `AL_SetBehavior(0x562D50)`. 3D dist² to the target < 64 (`0x905078`) → End (`0x5633D2`).
  - `0x796910(0x100)`; drive s = 0.05·(run·0.0001 + 1.0) (`0x9050D8`, `0x905FB0`, `0x904DC0`; run = `rec+0x3C`, ×3 rule).
- **`0x563830` chase ball.** Interrupt `0x563B70`: `*0x1DBE574` ≠ 0 (ball in mode 1 with speed > 0.4, set at `0x55D373`), TEDIOUS > 4000 (`0xFA0`), 3D dist (`0x77FBD0`) to ball `*0x1DBE570` < 50 (`0x905048`) → TEDIOUS −= 2000 and `AL_SetBehavior(0x563830)` (`0x563B70..0x563BED`).
  - Start: Motion 0x76/0x77/0x6A at cuts 0.15/0.6; Eyes 4 on both sides of an r<0.6 roll (the roll still uses a rand); Mouth 1/3; Icon(1,120).
  - Each frame: target = ball pos with x and z ×1.02 (`0x906448`), y unchanged (`0x563946..0x56397F`). Ball `Data1+4&0x8000` (held) → End. Every 100 frames r<0.15 (`0x9058A8`) → `0x562D50`. `*0x1DBE56C≠0` (ball in water: set at `0x55CF49`, cleared each ball frame at `0x55D32B`) → Icon(5,240) and End (`0x5639D9..0x5639EA`).
  - `0x796910(0x180)`; drive s = run·7e-6 + 0.028 (`0x1312D78`, `0x1312D74`; ×3 rule). Latch {TIRE +300 (`0x1312DC2`), TEDIOUS −100 (`0x1312DB0`)}.
  - End if timer > 3000 (`0xBB8`), TEDIOUS < 1000 or TIRE > 7000 (`0x1B58`) (`0x563B1A..0x563B60`). On every End: own ALW entry `+6=0`, `+0xC=−1`, `+0x40=0` (`0x5639F2..0x563A06`).
- **`0x569240` put on hat.** Grab template with kind 0x93; on success `0x46E5E0`, `0x56CFB0`, `AL_SetBehavior(0x569340)`, then push `0x569430` if `rec+0x118&0x10000` (animal 16 absorbed) and no hat (`rec+0xD5==0`); otherwise push `0x569550` (`0x5692C0..0x569315`).
- **`0x569340`.** Blend 35 Motion 0xBE; Icon(3,120); Snd 0x601C; 120+trunc(60r) = 120..180 frames → End. Ends early if the item is lost.
- **New `0x569430` wear hat.** Motion 0xC7; Icon(1,120). Mode 1, on motion `&8`: `rec+0xD5` = hat `Data1+8`; free the hat record; delete the hat; Motion 0xC8. It never advances the mode, so mode 2 is unreachable; it ends through `0x56CFF0` once the hat is gone.
- **`0x569550` toss hat.** Motion 0xC9. On motion `&1`: hat `W = (sin(rot+0x71C7)·0.3, 0.7, cos·0.3)`; unlink; End.
- **`0x5606D0` approach fruit.** Grab template with kind 0x83, next `0x5607C0`, plus `0x5659E0` balloon. On timeout Snd 0x6037. Interrupt `0x560640` (not while eating, sleeping or already approaching).
- **`0x566C40` approach animal/drive.** Grab template with kind 0x87 and tolerance 0x71C, no timer: no hit when facing → End. Next `0x566DF0`. Interrupt `0x566BC0`.
- **`0x566DF0` absorb.**
  - mode 0: Motion 0xCB; cnt = 90+trunc(200r); Snd 0x1004; effect task `0x470C00(2, 0x566CF0)`; balloon.
  - mode 1: item lost → End. On motion `&1`: Motion 0xCC, Snd 0x1005, particles, then the effects in chao-garden-items §1.3.
  - mode 2: End on motion `&8`.
- **`0x5647B0` 0xA2 reaction** (`0x5647B0..0x56489E`). Interrupt `0x564760`: current ≠ `0x5647B0` and `0x486760(obj, 0xA2)` hit with `+4≠0` → `AL_SetBehavior(0x5647B0, 0x1F4)`. Start: Motion 0x238 (no blend); **Eyes 1** (`0x5647D9..0x5647E1`); Mouth 8; Icon(5,300); Snd 0x6029 (r<0.5) or 0x606D. Every frame on ground (`W+0x40&0x400`) all three velocity components `W+0/4/8` × 0.9 (`0x904E28`). Always returns 0; ends at the 500-frame limit.
- **`0x566670` held dispatcher.**
  - SLPDPTH > 500 → `0x566580`.
  - Otherwise by bond like: > 50 → `0x5660A0`; < −50 → `0x566300`; else `0x5662B0`. If `rec+0x132` < 10 → `0x536320(+1, ±0.0333)` (`0x1312D38`).
  - Then `D+0xA0&=~8` and `0x565DC0`.
- **`0x5660A0` likes holder.** Eyes 4 (`0x5660FC`); Mouth 5/3 (`0x56612C`); timer 140+trunc(180r). Motion by r: <0.25 → 0x101 with default face (`D+0x694`/`D+0x69E`, `0x5661D1..0x5661E6`) and timer 200+trunc(180r); <0.4 → 0x102; <0.55 → 0x103; <0.7 → 0x105; else 0x107. Timer → re-pick.
- **`0x5662B0` neutral.** Motion 0x101.
- **`0x566300` dislikes.** **Eyes 1** (`0x56635B`, `lea edx,[eax+2]`); Mouth 4 (`0x566367`). Motion by r: <0.3 → 0x101 with default face (`0x5663BE..0x5663D3`); <0.5 → 0x109; <0.6 → 0x10A; <0.7 → 0x10B; <0.8 → 0x10C; <0.9 → 0x10D; else 0x10E. Timer 100+trunc(140r). With r<0.4: Snd 0x6047/0x6048/0x6049 at cuts 0.33/0.66. Timer → re-pick.
- **`0x566580` sleepy.** Motion 0x101; Eyes 9 (`0x5665C0`); Icon(5,360), repeated when the old counter > 400, i.e. every 402 frames. SLPDPTH −= 2 per frame; End when it reaches 0.
- All held behaviours call `0x565DC0` every frame and never End themselves; release and the HUNGER-driven mood drift (above) are handled inside `0x565DC0`.
- **`0x564320` dropped/thrown** (modes 0 → 2 → 3; `0x564320..0x56475F`).
  - mode 0 (`0x5644AE`): rotY = atan2(`W+0`, `W+8`)·10430.38 (`0x859B6A`, `0x904DB8`, i.e. radians→BAMS) = flight bearing; Eyes 1 (`0x5644D6`); Icon(5,150); blend 15 Motion 0x10F; mode = 2; `0x536320(EDI=−5, align −0.0667)` (`0x1312D44` negated, `0x56451A`); `0x53E1F0(EBX=10)`; emotion bytes `rec+0x12D..0x131` each += 50+trunc(100r) (`0x904E30`=100, `0x905048`=50), clamped 0..200.
  - mode 2 (`0x564371`): every frame with `W+0x40&0x800` and a hat (`rec+0xD5≠0`) → the hat goes into the first free save hat record (save = `0x19F6460 + (*0x173D06C==1)·0xCA6C`, list at `+0x4D8`, 24 × 0x14, free = u16 0xFFFF) via `0x54A540(kind, &pos, W, random angle r·360·182.044)` and `rec+0xD5=0`. If Status `D+4&1` → blend 15 Motion 0x9F, **timer 180+trunc(120r)** (`EDX=0xB4`, `0x905A20`; 180..300), Icon(5,150), mode 3.
  - mode 3: timer−− → End when ≤ 0.
- **`0x53D720`** (`0x53D720..0x53D757`). In mode 0: Motion 0 (no blend), mode 1, `D+0xA0&=~8`. Returns 0 forever.
- **`0x5648A0` petted** (jump table `0x564BF0`: `0x5648C7`/`0x564943`/`0x564A98`/`0x564B7D`/`0x564B98`). Interrupt `0x564C10`: current behaviour not `0x5648A0`, `0x54FCF0` (sleep), `0x59FCA0`, `0x59F970` or `0x59FA50`; Status `D+4&1`; `0x54B1F0(ECX=1, EDX=obj, push 2)` finds signal kind 2 → `AL_SetBehavior(0x5648A0)`, return 1.
  - mode 0: `0x54A690` (collision `|=0x70`). Posture 1 → 0x3E, 2 → 0x2A, 3 → 0x3E, else 0 (blend 20). If `rec+0x132` < 10 → `0x536320(EDI=+1, ±0.05)` (`0x1312D40`, `0x564931`). mode 1.
  - mode 1: on motion `&1`: SLPDPTH > 500 → `AL_SetBehavior(0x54FCF0)` (`0x564958..0x56496A`). Otherwise posture 1..2 → 0x41 (r<0.5)/0x42, else 0xFA (r<0.5)/0xFC (blend 20); mode 2; timer 10; with r<0.3 (`0x9050C8`): Snd 0x6074 (r<0.5) or 0x6075.
  - mode 2: on each motion `&1` timer−−; when it reaches ≤ 0: `0x54A730` and mode 3 (so ~10 motion pulses). Every frame: if `rec+0x132` > 100 → **Eyes 3** (`0x564AC4..0x564ACC`, `lea edx,[eax+4]`), Mouth 3; cnt++, once the old cnt ≥ 3: `rec+0x132 += 1`/frame, clamp 200 (`0x564AE0..0x564B16`). Then `0x486760(obj,2)`: if touch kind 2 is gone (no hit or `+4==0`) → posture 0 → blend 20 Motion 1, posture 1 → Motion 0xC, other postures no motion; mode 4, timer=cnt=0 (`0x564B19..0x564B72`).
  - mode 3: timer++ → End when old > 10.
  - mode 4: cnt++; once the old cnt ≥ 15: `rec+0x132 −= 1`/frame; End when < 10 (`0x564B98..0x564BDE`).
- **`0x563EB0` hit** (jump table `0x564308`; interrupt `0x563D30`).
  - mode 0 (`0x563ED9`): rotY = atan2(`W+0`,`W+8`)·10430.38 (flight direction); Eyes 1 (`0x563F08`, EDX=1); Icon(5,150). r<0.5: blend 15 Motion 0x3E, mode 1; else blend 15 Motion 0x10F, mode 2. `0x536320(EDI=−30, align −0.1)` (`0x1312D3C` negated, `0x563F88`); `0x53E1F0(EBX=50)`; bytes `rec+0x12D..0x131` each += 50+trunc(100r).
  - mode 1 (`0x5641CB`): until Status&1: `D+0x50 += 0x400` per frame (spin). Then blend 15 Motion 0x9F, timer 180+trunc(120r), `D+0x50=0`, Icon(5,150), mode 2.
  - mode 2 (`0x564254`): on Status&1 → blend 15 Motion 0x9F, timer 180+trunc(120r), Icon(5,150), mode 3.
  - mode 3: timer−− → End when ≤ 0 (`0x5642E6`).



## 3. Emotions and needs

Everything here lives in the Chao save record (`rec` = `Data1+0x5C`), in the block that starts at `rec+0x124`. The field layout is the one in `chao-details.md`: bytes `0x12C..0x133` run 0..200, words `0x134..0x148` run 0..10000, and personality bytes `0x14A..0x156` are signed. This section adds what changes each field and how fast.

### 3.1 Clocks: the life tick and its sub-periods

| What | Rule | Evidence |
|---|---|---|
| Scheduler | `0x5363D0` is called once per master frame. Its only caller is `0x5308D1`. Counter `0x1DBED74` is incremented, and when the **new** value reaches `u16[0x1312DE2]` = **3** it is reset to 0. Then the ALW Chao slot `(*0x1DBED70 & 0x1F)` is used: the ObjectMaster pointer at `0x1DC0FFC + i*0x48`, if it is non-null, gets `0x5353C0` (ESI = obj). After that `0x1DBED70++`. | `0x5363D0..0x53641B` |
| Life tick rate | One slot every 3 frames over 32 slots, so **each Chao gets one life tick every 96 frames**, however many Chao there are. An empty slot still uses its turn. | `0x5363E6..0x536415` |
| Gate | Runs only when `Data1+0xA0 & 0x20000` (`0x5353C6`). If the gate fails, the tick returns without the bond tick (`0x535E82`). CreateChao sets this bit with `or [eax+0xA0],0x20000` at `0x550546`, right after `or ...,0x10` at `0x550540`. The tick does not test AI bit 0x10 and changes only the record. | `0x5353C6`, `0x550546` |
| Sub-counters | A sub-period fires when the counter's **old** value is ≥ the constant (unsigned `JC` skip). The counter is then reset to 0, so the period is constant + 1 ticks. | `0x535401`, `0x53559E`, `0x53595E`, `0x535DF5` |
| `rec+0xA4` (**u32**) age clock | `u16[0x1312DE4]` = 120, so every **121** ticks (11616 frames). Age, lifespan and evolution belong to §4. The illness roll (§3.4) runs only inside this period. | `0x5353E5..0x535587` |
| `rec+0x126` (u16) emotion-decay counter | `u16[0x1312DE8]` = 7, so every **8** ticks (768 frames) | `0x53558C..0x5355A6` |
| `rec+0x12A` (u16) need counter | `u16[0x1312DE6]` = 10, so every **11** ticks (1056 frames) | `0x53594C..0x53596D` |
| `rec+0x128` (u16) condition counter | `u16[0x1312DEC]` = 80, so every **81** ticks (7776 frames) | `0x535DE2..0x535DF9` |
| `rec+0x168` (**i32**, signed `JL`) bond counter | `0x53E2D0` is tail-called at the end of every gated life tick (`0x535E6A` / `0x535E7D`). Constant `u16[0x1312DEA]` = 10, so every **11** ticks. | `0x53E2D0..0x53E2FC` |

**`rec+0x12A` as a latch.** This field is the need-period counter above. Behaviours use it as a once-per-need-period latch: if the word is 0, the behaviour sets it to 1 and applies its own need changes. The helper is `0x536420` (EAX = obj), which returns 1 once; eat calls it at `0x560E3F`. Inline copies of the same check are at `0x54EC7C`, `0x561A13`, `0x561D1B`, `0x561F42`, `0x563AA3`, `0x59F6C4`, `0x5A193F`, `0x5A1C52` and `0x5A3151`. A reimplementation must reproduce three side effects:
- The word is 0 only between a need tick and the next life tick, a window of at most 96 frames. A behaviour that is not running in that window gets no change for that period.
- Within one period, only the first latch user that sees 0 applies its change.
- Setting 0→1 moves the next need tick one life tick earlier (10 ticks instead of 11).

The **personality scale** used everywhere is `trunc(C*(300±p)/300)`, where `p` is a signed personality byte. The divide is the magic multiply `0x1B4E81B5`, `sar 5`, plus the sign fix-up, which truncates toward 0. Each result is clamped separately: 0..200 for bytes, 0..10000 for words. All constants `C` are u16 values from `0x1312D8C..0x1312DDE`, read in-tick at `0x5355B7`, `0x5355DA`, etc. The table values were read and checked: decay 4×7, then 90, 1500, 1500, 3500, 2000, 70, 2400, 12, 60, 30, 30, 100, 100, 60, 1000, 300, 400, 10, 300, 100, 300, 900, 100, 100, 36, 80, 9000, 120, 1000, 60, 8000, 300, 100, 30, 150, 600.

### 3.2 Need words (`rec+0x134..0x148`), per need tick (every 11 life ticks)

The names are the native debug labels, in order at `0x12E5BF0..0x12E5CA8` (see `chao-details.md`). The thresholds are the picker-function constants.

| Field | Label | Life-tick change | Main other raisers (+) / lowerers (−) | Behaviour thresholds |
|---|---|---|---|---|
| `0x134` | SLEEPY | `+= trunc( trunc(90*(300+p[0x14D])/300) * (TIRE*6e-5 + 1.0) )`. The doubles are `0x905A88` = 5.9999998e-5 and `0x904DC0` = 1.0, and the float→int is `0x8287C0` (`0x535964..0x5359E6`). | − sleep helper phases 1/2: −1500 per need tick (`0x54ED34`, `0x54ECC6`) | Drowsy `0x54F980` while 6000 < S < 9000 and `D+0x210 == 0` (`0x54E970`, consts `0x1312E64/68/6C`). Bed `0x54EE10` when S > 8000 and not under the water surface (`0x54EBA0`, `0x1312E84`). Wake check: S < 1000 **and** SLPDPTH < 2000 (`0x54EC40`, `0x1312DA2`). |
| `0x136` | SLPDPTH | none | + sleep phase 0: +1500 per need tick until ≥ 10000, then phase 1 (`0x54EDBD..0x54EE05`). − phase 2: −3500 per need tick (`0x54ECFA`). − deep sleep `0x54FCF0` and held-sleepy `0x566580`: **−2 per frame**; deep sleep returns 1 at 0 (`0x54FDDD`, `0x54FDFE`, `0x56662B`). | > 500: deep sleep `0x54FCF0` (picker `0x54EB20`, score 0.5+0.5·f16). Petting goes to sleep when > 500 (`0x564958`). Holding goes to held-sleepy when > 500 (`0x566681`). Spawn goes straight to sleeping when > 0 (`0x531DA3`). |
| `0x138` | HUNGER | `+= trunc(70*(300+p[0x150])/300)` (`0x5359DF..0x535A31`) | − eating: −2400 (`u16[0x1312DA6]`) per need tick (`0x560E52..0x560E7C`). At HUNGER == 0, eat sets its mode to 9 (`0x560EB3..0x560EC6`). | Picker `0x59EB00`. Fruit path: 0.99·f16(H,5000). No-fruit path (only when `D+0x20C==0` and r<0.6): 0.85·f16(H,5500) → `0x59ED60`; H ≥ 9901 → `0x59EF40` at score 1.0. Held for more than 600 frames (`0x565FA5`): H ≥ 10000 → wriggles free; > 8000 → dislike pose; > 5000 → neutral pose (`0x565FC5..0x56606F`). |
| `0x13A` | BREED | Adults only, type byte `rec+0x80` in 3..19: `+= trunc(12*(300+p[0x14F])/300)` (`0x535A38..0x535A91`) | + heart fruit +10000 (`add eax,0x2710` at `0x545D67`). Set to 0 at `0x56293C` when the garden has ≥ 8 Chao. Set to 0 after mating (`0x59BC3B`, `0x59BD9B`, `0x59C1D8`). | > 9999 → breed `0x59BB50` via `0x562880` (score 1.0) |
| `0x13C` | TEDIOUS | `+= trunc(60*(300+p[0x14A])/300)`, then `+= trunc(30*(300+p[0x14C])/300)`, then `+= trunc(30*(300−p[0x155])/300)`: three separately clamped adds (`0x535A98..0x535B88`). Then, if > 9999 and r < 0.1 (double `0x905030`), it is **set to 6000** (`0x535B95..0x535BD3`). | − toy chosen −5000 (`0x599C53`). − play/skill/song chosen −5000 (`0x5A11F3`). − skill choosers −4000 (`0x59C587`, `0x59D2A0`). − ball interrupt −2000 (`0x563BB6`). − ball chase −100 (`u16[0x1312DB0]`) per need tick (`0x563AF3`). − walk step −100 per need tick (`0x5A3172`). − swim helper `0x561920` −100 per need tick (`0x561A68`). | Toy `0x599B60` and play `0x5A10B0`: 0.99·f16(T,2000). Ball interrupt `0x563B70` needs a ball (`*0x1DBE574 != 0`), T > 4000 and distance < 50.0 (`0x563B85`, `0x563BA2`). Ball chase ends when T < 1000 (`0x563B47`). |
| `0x13E` | LONELY | `+= trunc(60*(300−p[0x14E])/300)` (`0x535BD7..0x535C23`) | − skill choosers −4000 / −3000 (`0x59C5B7`, `0x59C607`, `0x59D2D0`, `0x59D320`). − near player −6000 when byte `0x12C` > 140 (`0x5A0905`) or > 180 (`0x5A09D9`). | Picker `0x5A04A0`: 0.99·f16(L,1000) → `0x5A03B0`/`0x5A1800`; 0.99·f16(L,5000) → `0x5A0960`. The bond gates are in §1/§6. |
| `0x140` | TIRE | `+= trunc(10*(300−p[0x14F])/300)` (`0x535C2A..0x535C76`) | + walk step +300 (`0x5A3240`). + ball chase +300 (`u16[0x1312DC2]`, `0x563ABF`). + `0x59F5F0` +900 (`0x59F6E6`). + swim helper `0x561AF0` +100 (`0x561D98`). All are per need tick. − eat −2400 (`0x560E86`). − sleep phase 1 `−trunc(300*(300+p[0x151])/300)` (`0x54ED4A..0x54ED9C`). − rest `0x5A1BA0` `−trunc(100*(300+p[0x151])/300)` (`0x5A1C6C`). − swim helper `0x561920` −100 (`0x561A9C`). | Rest: TIRE > 2000 + round((6000−2000)·(p[0x14A]−p[0x155]+200)/400), score 0.99·f16(TIRE, threshold) (`0x5626D0`, `0x1312EAC`=2000, `0x1312EB0`=6000, `0x1312EB4`=0.99). Sleeping runs sub-behaviour `0x54F7A0` (50%) when T > 7000 (`0x54EF52`). Ball chase ends when T > 7000 (`0x563B59`). Rest ends when T < 100 (`0x5A1C17`). |
| `0x142` | STRESS | none | **No writer found** | > 8000 (`u16[0x1312DD6]`) lowers CONDITN (below). Also read at `0x568F46`. |
| `0x144` | NOURISH | `−= 36` (`u16[0x1312DCA]`, `0x535C7D..0x535CAD`) | **No raiser found.** Fruit effect `0x545790` → `0x535F00` / `0x536040` touch only `rec+0x20..0x47` and do not write it. Initial value is `trunc(r*5000)` (`0x5513EA`). | > 9000 or < 1000 changes CONDITN |
| `0x146` | CONDITN | Every need tick, after NOURISH, in this order: +80 if NOURISH > 9000; −120 if NOURISH < 1000; −60 if STRESS > 8000; −300 if byte `0x131` > 100. Each is clamped separately (`0x535CB1..0x535DA7`, consts `0x1312DCC..0x1312DD8`). | + rest `+trunc(30*(300+p[0x151])/300)` (`u16[0x1312DDC]`) per need tick (`0x5A1CBA..0x5A1D10`). Initial value 0 (`0x5513D3`). | No behaviour reader found outside the tick |
| `0x148` | THIRSTY | `+= 150` (`u16[0x1312DDE]`, `0x535DAE..0x535DDE`) | − swimming: −600 (`u16[0x1312DE0]`) per need tick (`0x561A34`, `0x561D41`). − weakest swimmer `0x561E00`: −6000 (= 600·5·2, `0x561F55..0x561F7C`). + `0x5680F0` activity +2000 (`add eax,0x7D0`, twice, `0x568264`, `0x5683F8`). Initial value `3000 + trunc(r*6000)` (`0x551427..0x551442`). | > 9000 → `0x5A1020` (0.99·f16, jittered, `0x562520`). `0x5625B0` (runs only while the best score is < 1.0): asks `0x486880(obj, 1, 0xAE)` for an 0xAE entry. If one is found, score = 0.891·(5000−T)/5000 when T < 5000, otherwise exactly 0 (the FPU keeps 0.0 at `0x562638`). Jittered; installs `0x567FA0` and copies the found entry's `+0x3C` into the Chao's ALW `+0x44`. < 100 → the swim target becomes a land wander point `0x534F80` (`0x561751`, in `0x561740`). < 1000 at a wall → climb out (`0x561B89`). |

`f16(v,t) = (v−t)/(10000−t)` is `0x562440`. The score jitter `s*(0.4+0.6r)` is `0x5624D0`. Both come from the groundwork.

**Drowsy picker `0x54E970`.** Score = 0.99·(S−6000)/2000 while S < 8000, 0.99·(1−(S−8000)/1000) while 8000 ≤ S < 9000, and 0 otherwise (`0x1312E70` = 0.99), then jittered. The cooldown `D+0x210 = round(600 + 2400·0.5·((10000−S)/10000 + r))` is rewritten on every call, whether or not the behaviour is chosen (`0x1312E5C`=600, `0x1312E60`=2400).

**Sleeping cycle** (`0x54EC70`, ESI=obj). It runs at most once per need period through the inline latch at `0x54EC7C` and is called from the sleeping code `0x54EF9B..0x54F96E`. The phase is the u16 `D+0x1B8`:
- Phase 0: SLPDPTH +1500 (`0x1312D9E`). Moves to phase 1 when SLPDPTH ≥ 10000 (`0x54EDFC`).
- Phase 1: SLEEPY −1500 (`0x1312D9C`), TIRE −trunc(300·(300+p151)/300) (`0x1312DBE`). Moves to phase 2 when SLEEPY == 0 (`0x54EDA9`).
- Phase 2: SLEEPY −1500, SLPDPTH −3500 (`0x1312DA0`). This phase has no exit of its own.
- The Chao wakes when `0x54EC40` holds: SLEEPY < 1000 (immediate `0x3E8`) and SLPDPTH < `u16[0x1312DA2]` = 2000.

**Rest.** The picker `0x5626D0` installs `0x55C430` with AL_SetBehavior, then pushes `0x5A1BA0` onto the behaviour stack (count `< 0xF`, timer −1). In mode 1 of `0x5A1BA0`, the countdown `D+0x1C0` is decremented every frame. When its old value is ≤ 0:
- The rest **ends** (returns 1) if HUNGER, SLEEPY, TEDIOUS or LONELY is > 8000 (`0x1312EB8`), or if TIRE < 100.
- Otherwise it ends **with probability 0.3** (r < 0.3, double `0x9050C8`).
- Otherwise (70%) the mode goes back to 0 and the rest repeats (`0x5A1BC9..0x5A1C43`).

The latch block (TIRE −, CONDITN +) runs on every mode-1 frame, not only at timer expiry (`0x5A1C47..0x5A1D10`).

### 3.3 Emotion bytes (`rec+0x12C..0x133`)

**Decay.** Every emotion-decay period (8 life ticks = 768 frames), each term below is subtracted separately with its own 0..200 clamp. All constants are 4 (`u16[0x1312D8C..0x1312D98]`). `0x12C` has no personality term.

| Byte | Decay terms per period | p=0 total | Evidence |
|---|---|---|---|
| `0x12C` | −4 | −4 | `0x5355AA..0x5355E1` |
| `0x12D` | −4(300−p14C)/300, −4(300+p14B)/300, −4(300+p155)/300 | −12 | `0x5355E4..0x5356CA` |
| `0x12E` | −4(300+p14C)/300, −4(300+p155)/300 | −8 | `0x5356D0..0x535769` |
| `0x12F` | −4(300+p14C)/300, −4(300+p155)/300 | −8 | `0x53576F..0x535801` |
| `0x130` | −4(300+p155)/300 | −4 | `0x535807..0x535852` |
| `0x131` | −4(300+p151)/300 | −4 | `0x535858..0x5358AA` |
| `0x132` | −4(300+p14A)/300, −4(300+p156)/300 | −8 | `0x5358B0..0x535946` |
| `0x133` | not touched by the tick; no garden write found | – | scan of `0x530000..0x5B0000` |

**Event changes.** Each change is clamped to 0..200.

| Byte | Raised by | Lowered by | Picker threshold → behaviour |
|---|---|---|---|
| `0x12C` | +10 while within 5 units of the player (squared 3D distance < 25.0, `0x904F70`), in `0x5A0820` (`0x5A08C7`). After the +10 it re-installs `0x5A0820` (`0x5A08F7`). Also +10 on arrival in `0x5A0960` (`0x5A0A7C`). | `0x5A0BA0`: −20 per cycle (up to 9 cycles, ends below 10) (`0x5A0EE2`, `0x5A0F0F`) | > 100 (`0x1312E30`) → `0x5A0BA0`, score `1−(100−p155)·0.005·(1−0.6)` (`0x1312E34`), jittered (`0x5A0B10`) |
| `0x12D` | Hit: +trunc(r·100+50) (`0x563F9A..0x564001`). Thrown: same (`0x56452C..0x564596`). Eating with message 6: +200 (`0x5611E1`). `0xA8` reaction: +150 with a random gate (`0x59B6CE`). Lonely dislike branch: +200, then pushes `0x5A18A0` (`0x5A07C2..0x5A07F2`). | `0x5A18A0`: −10 when within 5 units of the target (`0x5A19AD`), ends below 10 (`0x5A1927`) | `0x5A1240` requires: p14B < 31, type byte `rec+0x80` ∈ {7,10,13,16,19,22}, and `0x12D` > 80 (`0x1312DF8`). (a) If `0x590CE0()` ≠ 0 and the bond `like` byte `rec+0x16C+idx*6` (idx from `0x53E0C0`) is < 0: when p14C > 30 and r < 0.7 → score 0.99·f8(v,80), jittered, install `0x5A1460` and a `0x53D970` follow-up; otherwise when p14C > −50 → same score → `0x5A1800`. (b) In every other case: score `1−(100−p14C)·0.005·(1−0.7)` (`0x1312E00`), not jittered → `0x5A1460`. |
| `0x12E` | Hit/thrown +trunc(r·100+50) (`0x564004..0x56406E`, `0x564599..`). Eating with message 6: +200 (`0x561316..0x56133C`). | `0x59FA50` and `0x59FCA0`: −10 each time the timer runs out, end below 10 (`0x59FC4A`, `0x59FE53`) | > 90 (`0x1312E18`) → `0x59F8E0`, score `1−(p14C+100)·0.005·(1−0.7)` (`0x1312E1C`), **not jittered** (`0x59F860`). Dispatcher: byte < 121 → 50% `0x59F970` / 50% `0x59FA50`; ≥ 121 → 50% `0x59FCA0` / 50% `0x59FA50` (`0x59F8E0`). |
| `0x12F` | Hit/thrown +trunc(r·100+50) (`0x564071..0x5640DB`, `0x564631..`) | decay only (no behaviour writer found) | > 100 (`0x1312E08`) → `0x59F350` (`0x59F210`). **If the player is perceived** (`D+0x7E4 != 0`, `D+0x7F4 >= 0` and `*0x1DEA6E0 != 0`): score is 0.99·f8(v,75) (`0x562490`, `0x1312E0C`=75, `0x1312E14`=0.99), jittered, plus follow-up `0x59F5F0` (`0x59F2D1`). This applies only when that bond's fear byte `rec+0x16D+idx*6` > 75. If fear ≤ 75 the score stays 0 and nothing is installed. **If the player is not perceived:** score `1−(p14C+100)·0.005·(1−0.7)` (`0x1312E10`), not jittered (`0x59F2DF..0x59F325`). |
| `0x130` | Hit/thrown +trunc(r·100+50) (`0x564109`, `0x56469E`) | decay only | none found |
| `0x131` | Hit/thrown +trunc(r·100+50) (`0x564176`, `0x56470B`) | `0x5A00A0`: −10 per timer (`0x5A01CF`); its other mode −20 per timer, up to 5 times, ends below 10 (`0x5A034F`, `0x5A037C`) | > 150 (`0x1312E24`) → `0x5A00A0`, score `1−(100−p14C)·0.005·(1−0.5)` (`0x1312E28`), jittered (`0x5A0010`). > 100 also costs CONDITN −300 per need tick (`0x535D7A`). |
| `0x132` | Petting `0x5648A0` mode 2: +1 **per frame** once the frame counter `D+0x1C4` has an old value ≥ 3, i.e. from the 4th frame (`0x564AE0..0x564B16`). Held update `0x565DC0`: +200 per call, so it is pinned at 200 while held (`0x565DE1..0x565E07`). | Petting mode 4 (after the hand leaves): −1 per frame once `D+0x1C4` old ≥ 15; returns 1 when < 10 (`0x564B98..0x564BDE`) | none. Gates: happiness from pet/hold is only given when < 10 (`0x564919`, `0x5666DF`). Petting shows eyes 3 / mouth (EDX −1, push 3) while > 100 (`0x564ABB..0x564AD8`). |

The hit (`0x563EB0`) and thrown (`0x564320`) increments are `(int)(rand/32768·100.0 + 50.0)`, computed with the truncating FPU control word (`or 0xC00`). The doubles are `0x904E90` = 1/32768, `0x904E30` = 100, `0x905048` = 50. That gives 50..149, drawn separately for each of `0x12D..0x131` (5 rand calls). Both handlers first call `0x53E1F0` with EAX = obj (`0x563F95`, `0x564527`): it raises the current character's bond fear byte `bond+1` by `trunc(n(300−p155)/300) + trunc(n(300−p14C)/300)`, clamped 0..200, with EBX = n = 50 for a hit and 10 for a throw.

**Bond fear `bond+1`** (`rec+0x16D + c·6`). `0x53E120` (from AddHappiness): for delta > 0 it goes **down** by `trunc(10(300+p155)/300) + trunc(10(300+p14C)/300)`, clamped 0..200 (magic /−300 at `0x53E1A4`). `0x53E1F0` raises it (above). The scared candidate `0x59F210` tests it > 75.

Eat "message 6" means the ALW entry (`obj+0x3C`) word `+0x34` == 6, which is cleared on read (`0x5611B1..0x5611BF`). The same branch calls AddHappiness(−1, align ∓0.1) (`0x5611D6`).

### 3.4 Happiness, bonds and illness

- **Happiness `rec+0x82`** (−100..100) is changed only by the AddHappiness helper `0x536320`, at the interaction sites listed in `chao-garden-items.md` §1.2. The life tick `0x5353C0` never writes `+0x82` (full listing read).
- **Bond tick `0x53E2D0`** (every 11 life ticks: counter `rec+0x168`, body runs when the old value ≥ u16 `0x1312DEA` = 10, then resets to 0): for each of the 6 character bonds, the u16 at `rec+0x16E + k*6` (bond+2) gets `+= u16[0x1312D6C]` = **16**, capped at 50000 (`0xC350`) (`0x53E2F5..0x53E32B`). The same +16 / cap 50000 is applied to 20 u16 entries at `rec+0x1A6 + j*0x1C` (`0x53E32D..0x53E34F`). (Re-checked; 150 is the neighbouring word `0x1312D6E`, not this add.) `0x53E280` (called by the `0x5A0490` follow-up; AddHappiness calls `0x53E120`) does `bond+4 += 1` capped 0xFFFF and `bond+2 = 2·⌊v/3⌋` (magic `0x55555556`, `add eax,eax`), not ×2/3 (§2B). The lonely chooser tests `bond+2` > 300 / > 100 (`0x5A04A0`).
- **Condition slots `rec+0x15A..0x15F`** (i8): each moves 1 toward 0 every condition period, 81 life ticks (`0x535DFD..0x535E73`).
  - New illness: once per age period, if r < 1/6000 (double `0x905A90` = 1.6667e-4), pick `idx = trunc(r·5.9999)` from a second rand (double `0x9054F8` = 5.99990). If `idx != 3` and slot `rec+0x15A+idx` is 0, set it to −100 (`0x9C`) (`0x535510..0x535587`).
  - A negative slot makes illness behaviour `idx` choosable (picker `0x599CB0`, groundwork).

### 3.5 How emotion state is shown

- **Emotion ball.** `0x53D660(ECX=obj, DL=state, push frames)` writes the queued state `D+0x6EE` (sign-extended DL) and its length `D+0x6F4`, and **zeroes the current timer `D+0x6F0`**. When `0x134046C == 0x134062C`, it also plays sound `0x100E` (`mov edi,0x100E`) via `0x4372E0` at `Data1+0x14` (`0x53D680..0x53D699`).
  - `0x53CA40` calls handler `0x1353834[D+0x6EC]`. When the handler returns 1, the queued state becomes current with `D+0x6F0` = its frames, and the queue is set to −1. If nothing is queued, it goes to state 0 with timer 0. It also clears `D+0x724/0x728/0x768/0x76C`. `D+0x6F0` counts down by 1 per frame while > 0.
  - Handler table (read): `0x53C180` (state 0), `0x53C200` (1), `0x53C380` (2), `0x53C3B0` (3), `0x53C530` (4), `0x53C790` (5); each takes obj on the stack with E = D+0x6EC. The icon word `D+0x726` is set to 10 (state 0, first frame only), 1, – (none), 2, 3, 4 (`0x53C1AD`, `0x53C33E`, `0x53C4E8`, `0x53C669`, `0x53C917`). State 0 returns 1 only when a state is queued (`D+0x6EE != −1`, `0x53C1E3`/`0x53C1F2`). State 2 always returns 0 (`0x53C3AB`), so it never ends by itself. (Re-checked from bytes.)
  - Confirmed requests: deep sleep, state 5 for 360 frames (`0x54FD8A`), refreshed when the frame counter `D+0x1C0` passes 400, i.e. every **402** frames (`0x54FD9F..0x54FDC0`). Hit: state 5, 150 frames (`0x563F0D..0x563F16`). Thrown: state 5, 150 frames (`0x5644DC..0x5644E5`). AddHappiness uses state 4 (delta > 0) or 5, for 0xB4 frames (`chao-garden-items.md`).
- **Face.** Eyes `0x53A4B0` and mouth `0x53A5A0` are set by each behaviour. No per-frame function that maps emotion values to the face was found. The only emotion-driven face found is petting with `0x132` > 100 (`0x564ABB..0x564AD8`).
- **Clock or day effects.** The life tick reads no real-time or day variable. The only clock is `rec+0xA4` (§3.1, §4).

### 3.6 Initial values of a new Chao (`0x551560` → `0x5513C0`, ESI = rec+0x124)

- CONDITN = 0 (`0x5513D3`).
- NOURISH = trunc(r·5000) (double `0x9054F0` = 5000, `0x5513EA..0x5513FD`).
- THIRSTY = 3000 − trunc(r·(−6000)) = 3000 + trunc(r·6000) (double `0x9054E8` = −6000, `0x551427..0x551442`).
- The low byte of SLEEPY (`rec+0x134`) is set to 0 (`0x551643`, `mov byte [esi+0x10],0` with ESI = rec+0x124); the high byte is left as is. `0x5513C0` does not write `+0x10`.

`0x5513C0` also writes words at `rec+0x14A..0x158`. `0x55164C..0x55168F` then overwrites bytes `0x14A..0x156` with personality values `(gene−2)*40`. Callers of `0x5513C0`: `0x551647`, `0x551FCC`.


## 4. Stat growth, aging and personality

Scope: what changes the save record (`rec` = `Data1+0x5C`) over a Chao's life, outside evolution. Fruit, animal and drive effects are already in `chao-garden-items.md` §1.1–1.4 and are only summarized here. All constants were read from the dumped exe with REA. `rand` means `0x7A89D8() * 1/32768` (double `0x904E90`), a value in [0, 1). `trunc` is x87 truncation, either through `_ftol 0x8287C0` or through `FISTP` with the control word set to `|0xC00`.

### 4.1 Stat growth

**`GrowStats` 0x535F00 has only two resolved callers:** the fruit bite `0x545790` and the animal/drive absorb `0x566DF0` (REA `procedure_callers`). The level-up helper `0x535E90` is called only from `0x535F00`. Walking, swimming, flying and toys don't raise stat progress, levels or points. Caveat: Ghidra leaves some behaviour bodies undisassembled (for example `0x5607C0` and `0x568F60`). Calls from those bodies don't appear in caller lists.

| Mechanic | Formula / constant | Evidence |
|---|---|---|
| Progress per item | `progress[i] += table[kind][i]`. Fruit gives `value/4` per bite, plus the remainder on the last bite. Tables: `0x8A6240` (animals and drives) and `0x8A6448` (fruit). | `chao-garden-items.md` §1.1/§1.4, `0x535F00` |
| Level up | When progress > 99 and level < 99: `progress -= 100`, `level++`, `points[i] += 3*grade[i] + 11 + rand0..4`, capped at 4000. At level 99, progress is capped at 99. | `0x535F00`, `0x535E90` (`chao-garden-items.md`) |
| Stat-up display copy | Copies progress `rec+0x20+i` → `Data1+0x60+i`, level `rec+0x30+i` → `Data1+0x70+i` and points `rec+0x38+2i` → `Data1+0x78+2i`, for i = 0..7. These are the old values, kept for the stat-up display. | `0x536040..0x5360FB` |
| Fruit bite side effects | Life `+3` (`0x1312D00`=3). Max life `+5` (cap 9999) for children only: adults skip `0x5362D0`. Evolution `+0.005` (child, f32 `0x1312CF4`) or `+0.00125` (adult, f32 `0x1312CF8`). | `0x545790`, `0x5362D0` (`chao-garden-items.md`) |
| Exercise (needs only) | Walk step `0x5A3000`: TEDIOUS `0x13C -= 100` (`u16 0x1312DB0`), TIRE `0x140 += 300` (`0x1312DC2`). Swim `0x561920`: THIRSTY `0x148 -= 600` (`0x1312DE0`), TEDIOUS `-= 100` (`0x1312DB2`), TIRE `-= 100` (`0x1312DC8`). All results are clamped to 0..10000. Each runs only when `rec+0x12A == 0` and then sets it to 1. | `0x5A3146..0x5A3273`, `0x561A08..0x561ABF` |
| Fly points as an AI input | Reads fly points `u16 rec+0x3A`. They are multiplied by 3 unless `CurrentChaoArea` is 6 or 10..11. If the result is < 400 (`0x190`), climb `0x5A0F40` runs. Otherwise climb runs when rand < 0.6 (double `0x905268`) and fly `0x5630C0` runs when rand ≥ 0.6. When `0x535010()==9`, climb always runs. This path runs only when the ground flag `0x400` is clear. | `0x5A31C8..0x5A322B` |

`rec+0x12A` is the needs-tick counter of the life tick (`rec+0x124+6`). It is incremented every life tick, compared with `u16 0x1312DE6`=10 at `0x535957`, and reset to 0 at `0x53596D`. Walk and swim therefore apply their need changes at most once in the 96-frame window after a needs tick, while the counter is still 0. They then set it to 1, so the next needs tick comes one life tick early.

**Grades** (`rec+0x28+i`) are set from DNA in `0x551DA0` (§4.3). Rebirth keeps them (`0x551560` never writes `rec+0x28..0x2F`). Evolution raises one by 1 (`0x551780`, see "Evolution (0x568630)").

### 4.2 Aging and lifespan

**Life-tick scheduler.** The garden manager main `0x530850` works as a state machine:
- state 0 → 1;
- state 1 → 2 when `CurrentChaoArea` is 1..3, else 3 (`0x530A97..0x530AAB`);
- in state 2 it calls `0x5363D0` on every run (`0x5308D1`).

`0x5363D0` works like this:
- It increments counter `0x1DBED74`. When the new value is ≥ `u16 0x1312DE2`=3, it resets the counter to 0.
- It takes the object at ALW cat-0 slot `0x1DC0FFC + (idx & 0x1F)*0x48`, with idx = `0x1DBED70`. If that slot is non-null, it calls `0x5353C0` with ESI = obj.
- It always increments `0x1DBED70`, even when the slot is empty.

Each slot is therefore ticked once every 3 × 32 = 96 runs of `0x530850`. `0x5353C0` returns immediately unless `Data1+0xA0 & 0x20000` is set. CreateChao sets that bit (`OR [EAX+0xA0],0x20000` at `0x55054D`). The egg spawner `0x57B9C0` only sets or clears bit 0x2.

Counters inside `rec`, all in `0x5353C0`. "Old" is the value before the increment. Each counter is reset to 0 when it fires.

| Counter | Fires when | Period | Action |
|---|---|---|---|
| `rec+0xA4` u32 | old ≥ `u16 0x1312DE4`=120 (`0x5353EB..0x535403`) | 121 life ticks = 11616 frames | age tick (below) |
| `rec+0x126` u16 | old ≥ `0x1312DE8`=7 (`0x53558C..0x5355A6`) | 8 life ticks = 768 frames | emotion decay (§4.4) |
| `rec+0x12A` u16 | old ≥ `0x1312DE6`=10 (`0x53594C..0x53596D`) | 11 life ticks = 1056 frames | needs tick (§4.4) |
| `rec+0x128` u16 | old ≥ `0x1312DEC`=80 (`0x535DE2..0x535DF9`) | 81 life ticks = 7776 frames | each condition byte `rec+0x15A..0x15F` (signed) moves 1 toward 0 (`0x535DFD..0x535E73`) |
| `rec+0x168` u32 | old ≥ `u16 0x1312DEA`=10 (in `0x53E2D0`) | 11 life ticks | `+16` (`s16 0x1312D6C`) to six u16 at `rec+0x16E+6k` (k = 0..5, the `bond[k]+2` word) and to 20 u16 at `rec+0x1A6+0x1C*k` (k = 0..19), each capped at 50000 |

`0x5353C0` tail-jumps to `0x53E2D0` (`0x535E6A`/`0x535E7D`) on every life tick that passed the `0x20000` check.

**Age tick** (`0x535409..0x535587`):
- `age rec+0x86 += 1` and `rec+0xA4 = 0`.
- **Child** (`rec+0x80 == 2`):
  - evolution helper `0x536110(+1/60)` (f32 `0x1312CA0`);
  - in area 2: alignment helper `0x536260(+1/120)` (f32 `0x1312CA8`); in area 3: `0x536260(-1/120)` (FCHS). In other areas, alignment doesn't change.
- **Not child:**
  - `adultAge rec+0x88 += 1`;
  - if `type < 0x14`: `maxLife rec+0x8C = max(0, maxLife - (adultAge + 100) / 15)`, using the new adultAge and integer division by magic `0x88888889` (`0x53548A..0x5354B2`). Types ≥ 20 (Chaos and character Chao) skip this step;
  - then `life rec+0x8A = min(life, maxLife)` (`0x5354B9..0x5354C9`);
  - then `0x536110(+1/240)` (`0x1312CA4`), plus another `0x536110(±1/240)` (`0x1312CAC`): + in area 2, − in area 3 (`0x5354D0..0x535508`). Both calls go to the evolution helper, not to alignment.
- **Helpers:**
  - `0x536110` adds to evolution `rec+0xC0` and clamps it to [0.0, 1.2] (`0x1312C8C`=1.2, `0x8AF004`=0.0).
  - `0x536260` adds to alignment `rec+0xB0` and clamps it to [−1, 1].
  - Both set `Data1+0x674 |= 2`.
- **Illness onset, every age tick:** if `rand < 1.6667e-4` (double `0x905A90`), let `slot = trunc(rand * 5.9999)` (`0x9054F8`). If `slot != 3` and `rec[0x15A+slot] == 0`, it is set to `0x9C` (−100) (`0x535510..0x535587`).

**Lifespan values:**
- **Initial:** `life = maxLife = max(DNA+0x6C, DNA+0x6D) * 100 + 3000`, using `u16 0x1312C60`=100 and `0x1312C5E`=3000 (`0x551E80..0x551EA8`). New random alleles are `trunc(rand * 9.9999)` (`0x905500`), so 0..9 (`0x5508DA..0x55096C`), giving 3000..3900.
- **Other writers:** fruit raises life by 3, and maxLife by 5 for children only (adults skip `0x5362D0`). No other writer of `rec+0x8A` was found, so life normally falls only through the clamp to the decaying maxLife.
- **Worked example** (60 fps, garden time only, no fruit, adultAge counted from 0): maxLife 3000 reaches 0 after 221 adult age ticks (≈11.9 h). maxLife 3900 takes 262 ticks (≈14.1 h).

**Death or rebirth trigger:** picker candidate `0x562A20`.
- Condition: best score < 1.0 (`FLD1; FCOMP [EDI]`), `life rec+0x8A == 0`, and `0x54AC20` returns true. `0x54AC20` is true when ground flag `0x400` is set in `Data2+0x40` and the length of `(vx, 0, vz)` (via `0x77FC30`) is < 0.04 (`0x16048F4`).
- Action: `AL_SetBehavior(0x568F60, -1)`; best = 1.0; `D+0x1CC` and `D+0x1CE` are cleared.
- While it runs, the interrupt checks skip (`cmp edi,0x568F60` at `0x53DB3E`; `cmp eax,0x568F60` at `0x563D5B`).

**Behaviour `0x568F60`.** Ghidra has no function here; this was hand-decoded from the bytes at `0x568F60..0x56917D`. The mode is the u16 at `D+0x1B6`, dispatched through jump table `0x569180`. The timer is `D+0x1C0`. Each wait compares the old timer value, then increments it.

| Mode | Action | Leaves when |
|---|---|---|
| 0 (`0x568F8A`) | Calls `0x54A690` and `0x54A730`. Eyes: `0x53A4B0` (EAX=−1, EDX=3). Mouth: `0x53A5A0` (EDX=−1, push 4). Emotion ball: `0x53D660` (DL=5, 300 frames). Motion `0x793D30` with id chosen by `u16 D+0xAE` through table `0x56919C`: 0→0x2F, 1→0x3E, 2→0x2A (all 0xF frames), 3→0x3E with 0x2D frames. Values > 3 skip the motion call (`ja 0x569006`). Then mode=1 and timer=`D+0x1C4`=0. | Falls straight through into the mode-1 code in the same frame |
| 1 (`0x569015`) | wait | Timer > `0x1312CCC`=200: mode=2, timers cleared, then `0x568CD0(1)` if decision `0x568F30` is true, else `0x568CD0(2)` |
| 2 (`0x569068`) | wait | Timer > `0x1312CD0`=1800: clears `D+0xA0` bits 0x200, 0x100000 and 0x80000, then evaluates `0x568F30`. True: rebirth `0x568570`, mode=5. False: `0x443480` with EBX = `"chao_g_dead.adx"` (`0x12F5944`), mode=3. Timers cleared, 0x200 cleared again. |
| 3 / 5 (`0x5690F0` / `0x569142`) | wait | Timer > `0x1312CD4`=900: `0x568D10`, mode 4 / 6, timers cleared |
| 4 (`0x569120`) | death | Timer > `[0x1312CD8]`(900)+30: `0x550620` does `memset(rec, 0, 0x4DC)` and sets `obj+0x10` (MainSub) = `0x46F720` |
| 6 (`0x56915A`) | rebirth end | Timer > 930: `obj+0x10 = 0x46F720` (the record now belongs to the new egg) |

**Decision `0x568F30`:** rebirth iff `(i16) happiness rec+0x82 > 50` (`0x1312CEC`, signed `jle`) **and** `(u16) STRESS rec+0x142 < 700` (`0x1312CEE`, `jae`).

**Rebirth `0x568570`:**
- `rec+0x8E` (reincarnations) `+= 1`;
- `0x551560(push DNA = rec+0x438)` with EDI = rec;
- `0x5307F0` (ALW unregister);
- `0x57B9C0(0, rec, 0, &Data1+0x14, 2)`.

The egg spawner keeps the record pointer:
- It runs `0x551DA0` only if `rec+0x438 == 0` (`0x57BCB3`). `0x551DA0` set that byte to 1 at birth (`0x551DB0`), so rebirth doesn't re-run it.
- It sets type `rec+0x80 = 1` (egg) and garden `rec+0x81 = CurrentChaoArea` when the area is 1..3 (`0x57BCEE..0x57BCFF`).
- Case 2 of jump table `0x57BD2C` stores hatch param `[0x1312CD8]+[0x1312CD4]+0x23` = 1835 (`0x57BBCA`).

**`0x551560` (record reset at rebirth):**
- **Stats:** for each of 8 stats, progress `rec+0x20+i = 0`, level `rec+0x30+i = 1`, points `rec+0x38+2i = min(points, 5000) / 10` (`0x551570..0x55159A`). Grades are kept.
- **DNA write-back:** for each personality byte p at `rec+0x14A+k` (k = 0..12), one allele of pair `0x74+2k` becomes `trunc(p/40) + 2`. The allele index is `0x74+2k - trunc(rand * -1.99)` (`0x9054C8`), so the first allele is picked with p ≈ 0.5025 (`0x5515A7..0x5515FD`).
- **Other resets:**
  - `u32 rec+0x118 = 0`;
  - each of the 8 bytes `rec+0x11C+i` is set to 0xFF when rand < 0.3 (`0x9050C8`);
  - byte `rec+0x134 = 0`.
- **`0x5513C0`** with ESI = `rec+0x124` (below).
- **Personality re-roll:** `rec+0x14A+k = (DNA[pair k, 50/50 pick] − 2) * 40`, as at birth. Taste is **not** re-rolled.
- **Lifespan and age:** `life = maxLife = max(DNA+0x6C, DNA+0x6D)*100 + 3000 + oldMaxLife/10`; `age = adultAge = 0`.
- **Alignment `rec+0xB0`:** 1.0 if type ≥ 6 and (type−6)%3 == 0; −1.0 (`0xB087CC`) if type ≥ 7 and (type−7)%3 == 0; else 0.0. The type here is still the adult form.
- **Other floats:** `0xA8 = 0xAC = 0xB4 = 0xB8 = 0xC0 = 0.0`, `0xBC = 0xB0` (`0x551751..0x551777`).

**`0x5513C0` (needs init; called at birth `0x551FCC` and at rebirth `0x551647`, ESI = `rec+0x124`).** It writes u16 values:

| Field | Value |
|---|---|
| NOURISH `rec+0x144` | `trunc(rand*5000)` |
| CONDITN `rec+0x146` | 0 |
| THIRSTY `rec+0x148` | `3000 − trunc(rand * −6000)` = 3000..8999 |
| `rec+0x14A` | `trunc(rand*999.9999)` |
| `rec+0x14C` | `trunc(rand*5000)` |
| `rec+0x14E` | `trunc(rand * −0.0001)` = 0 |
| `rec+0x150` | `trunc(rand*8000)` |
| `rec+0x152` | 0 |
| `rec+0x154`, `rec+0x156` | 10000 (`0x1312C82`) |
| `rec+0x158` | 0 |

The `0x14A..0x156` writes land on the personality bytes and are overwritten by the personality loop that follows in both callers. At birth, `0x551DA0` also rewrites taste `0x157..0x159` afterwards. At rebirth it doesn't, so after rebirth taste `rec+0x157` = 0x27 (39, the high byte of 10000) and `rec+0x158 = rec+0x159 = 0`. No later writer was found. With 0x157 = 39, the fruit-kind 7/8/9 dislike checks in §4.4 never match.

### Evolution (0x568630)

Only caller: cocoon `0x568D30` mode 2 (`0x568E04`: `EAX=obj; CALL 0x568630; MOVZX EAX,AL; ECX=obj; CALL 0x56CF40`), then mode 3. No rand anywhere. Gate: picker #20 `0x562800` (child, `rec+0xC0 >= 1.0`, clamped to 1.0). Server: `src/chao_sim/evolve.rs` `evolve(&mut Record) -> u8`.

- **Kind** (`0x56865F..0x568737`; f32 `0x1312C94` = 0.5). a8 = `rec+0xA8` (run −/power +), ac = `rec+0xAC` (swim −/fly +).
  - Directional unless `-0.5 < a8 < 0.5` and `-0.5 < ac < 0.5` (FCOM `TEST AH,0x41` / `TEST AH,5; JP`, so ±0.5 is directional).
  - Directional (`0x56870B`): `ac <= a8` → power 0x11, but swim 0x08 if `ac <= -a8`; else fly 0x0B, but run 0x0E if `ac <= -a8`. Ties: a8 == ac → power/swim side; ac == −a8 → swim (a8 > 0) or run (a8 < 0).
  - Normal 0x05, or **Chaos 0x14** when all bits 0..20 of `rec+0x118` (animals) are set **and** `(i16) rec+0x82 > 80` (`JLE`) **and** `(u16) rec+0x8E >= 2` (reincarnations, `JC` vs BX=2) (`0x5686B9..0x5686F4`).
- **Alignment** (`0x56873C..`; f32 `0x1312C98` = 0.5, `0x8AF004` = 0.0): if `rec+0xB0 <= -0.5` or `>= 0.5`: `> 0` → type+1 (hero; save `0x19F646C[slot] |= 0x10`, jingle `chao_g_born_h2.adx` `0x12BE8BC` via `0x443480`), else type+2 (dark; `|= 0x40`, `chao_g_born_d2.adx` `0x12BEBBC`). If both 0x10 and 0x40 are set (and `0x1DEFAA0 == 0x1A5570C`) → `0x40E850(ECX=7)` (achievement-like). Slot = `0x173D06C == 1`, stride 0xCA6C. `rec+0xB0` itself is kept.
- **Writes** (`0x56876A..0x568784`): `rec+0xC0 = rec+0xA8 = rec+0xAC = 0.0`; `rec+0x80 = type`. Returns type in AL.
- **Grade** `0x5517B0` (EDI=rec, ECX=DNA=`rec+0x438`): index by type via byte table `0x551828` / jumps `0x551814`: 8..10 → 0 swim, 11..13 → 1 fly, 14..16 → 2 run, 17..19 → 3 power, 5..7 and 20..22 → 4 stamina. `0x551780`: g = `rec+0x28+i`; if g < 5: if DNA `+0x5C+2i` == g it is incremented, else if `+0x5D+2i` == g it is; then `rec+0x28+i = g+1`. (DNA pairs `+0x5C+2i` are the grade alleles `0x551DA0` picks from at birth.)
- **`0x56CF40`** (EAX=type, ECX=obj; rejects 0,1,3,4,>25): `0x56CE70` frees shape/palette, `rec+0x80 = type` again, `0x56C9D0` reloads the model `AL_RootObject[type-2]`, `0x539F90` eyes/mouth, `0x53C010`, `0x566B80` re-attaches parts, `0x5904C0` resets perception. Record side effects:
  - `0x539F90` (`0x53A072..0x53A089`): `(type-7) <= 15 && (type-7)%3 == 0` (dark: 7,10,13,16,19,22) → **`rec+0xD1` (eyes) = 10**. Chaos types use fixed eyes/mouth 0xB/0xC/0xD on Data1 only.
  - `0x566B80` → `0x5668B0` ×8: part slot `(i8) rec+0x11C+i` outside 0..20 → 0xFF (else rewritten unchanged).
  - `0x55F0F0`/`0x55E8D0` rewrite `rec+0xD8` with its own value.
  - Everything else is Data1/model state: a client copy rebuilds it from the new type.
- Not touched: age, adult age, life/max life, reincarnations, personality, stats, alignment.

### 4.3 Personality and likes: how they are set

**DNA block** at `rec+0x438`, 0xA4 bytes, stored as allele pairs. Random DNA comes from `0x5506B0` (ESI = DNA). Its resolved callers are CreateChao `0x5501D0`, the egg spawner `0x57B9C0` (for a NULL record, `0x57BC8C`) and the new-save starter eggs `0x530EF0`. Bred eggs mix both parents' genes in `0x551D20` (`chao-garden-sync.md`, "Breeding"). `0x551D20` has no resolved caller in Ghidra; its algorithm is unmapped.

**`0x5506B0`:**
- memset(DNA, 0, 0xA4); `DNA[0] = 0`.
- **Grade alleles** `DNA+0x5C+2i` (both alleles equal). Let `r = rand`, compared as f32:
  - **`r < 0.33`** (`0xB51274`):
    - A local array {0,1,2,3,4} gets 10 swaps of indices `trunc(rand*4.9999)` (`0x905520`). Stats 0..4 take the shuffled values.
    - Stats 5..7 take the low bytes of `[ESP+0x2C]`, `[ESP+0x30]` and `[ESP+0x34]` (`0x5507E7..0x5507FF`): an uninitialized local, the return address and the caller's stack slot above it.
  - **`0.33 ≤ r < 0.66`** (`0x905518`): each stat gets `1 − trunc(rand * −2.9999)` (`0x905510`) = 1..3.
  - **`r ≥ 0.66`:** all grades start at 0. Then 14 increments go to stat `trunc(rand * 7.9999)` (`0x905508`); a stat already at 5 is re-rolled without counting (`0x550884..0x5508D8`).
- **Lifespan alleles** `0x6C` and `0x6D`: `trunc(rand*9.9999)` = 0..9.
- `0x6E..0x73` = 0.
- **Personality alleles** `0x74..0x8D` (13 pairs): each byte is `_ftol(rand*4.9999)` = 0..4 (`0x550970..0x5509B7`).
- **Taste alleles** `0x8E..0x93`: `trunc(rand*5.9999)` = 0..5.
- `0x94..0x9B` = 0.

**Record from DNA, `0x551DA0`** (EDI = DNA, push rec). Resolved callers: CreateChao (`0x5504A1`) and the egg spawner (`0x57BCDB`), each only when `DNA[0] == 0`.
- **Set first:**
  - `DNA[0] = 1`, `rec+0x80 = 2`, `rec+0x84 = 0xFF`;
  - `rec+0x90/0x94` are copied from the save header at `0x19F6464`;
  - `rec+0x98 = rec+0x9C = 0`;
  - `rec+0xA0` takes the save counter `0x19F647C`, which is then incremented;
  - `u16 rec+0x40 = 0`.
- **Grades:** grade = the higher allele with probability 0.7 (`0x9050C0`), else the lower (`0x551E20..0x551E7E`).
- **Lifespan:** as in §4.2.
- **Floats:** PowerRun `0xA8`, FlySwim `0xAC` and alignment `0xB0` = allele × `u16 0x1312C62`/`0x1312C64`. Both multipliers are 0, so the result is always 0.0.
- **`0x5513C0`:** called here (§4.2).
- **Personality** `rec+0x14A+k` (k = 0..12): `(allele − 2) * 40`. The allele is `DNA[0x74+2k − trunc(rand * −1.99)]`, the first one with p ≈ 0.5025. Values are −80, −40, 0, 40 and 80 (`0x551FD8..0x552017`).
- **Taste:** `rec+0x157`, `0x158` and `0x159` come from pairs `0x8E/0x8F`, `0x90/0x91` and `0x92/0x93`. The pick is `trunc(rand * −1.9999)` (`0x906298`), about 50/50. Values are 0..5 (`0x552019..0x5520FF`).
- **`rec+0xD1` / `rec+0xD2`:** `0x12D6534[k]` and `0x12D68B4[k]`, where `k = b(0x14A) + 3*b(0x14B) + 9*b(0x14C)` and `b(v)` = 0 if v < −45, 2 if v ≥ 45, else 1 (`0x5520F3..0x552157`).
- **`DNA[1]` (special-type byte):** when non-zero, a switch (`0x552171`, tables `0x552370`/`0x552388`) fills `DNA+0x94..0x9B` and `rec+0xDC`, then clears `DNA[1]`. `rec+0xD8..0xDB` are then picked from those alleles (`0x55221C..0x55236E`). This is appearance, not mapped here.

**Changes during life:** apart from `0x551DA0`, `0x551560` and the `0x5513C0` side effect, no writer of `rec+0x14A..0x159` was found.

### 4.4 How personality and taste bias behaviour

`p[x]` is the signed personality byte at `rec+x`. Integer divisions truncate toward zero (magic `0x1B4E81B5`, `sar 5` = /300). Results are clamped after each step: emotions (u8) to 0..200 and needs (u16) to 0..10000.

| Where | Formula | Evidence |
|---|---|---|
| Emotion decay (every 8 life ticks) | `0x12C -= 4`<br>`0x12D -= (300−p14C)*4/300`, then `(300+p14B)*4/300`, then `(300+p155)*4/300`<br>`0x12E -= (300+p14C)*4/300`, then `(300+p155)*4/300`<br>`0x12F -= (300+p14C)*4/300`, then `(300+p155)*4/300`<br>`0x130 -= (300+p155)*4/300`<br>`0x131 -= (300+p151)*4/300`<br>`0x132 -= (300+p14A)*4/300`, then `(300+p156)*4/300` | `0x5355A4..0x535946`; `u16 0x1312D8C..0x1312D98` are all 4 |
| Needs tick (every 11 life ticks), in this order | SLEEPY `0x134 += trunc(((300+p14D)*90/300) * (1 + TIRE*6e-5))` (`0x1312D9A`=90, double `0x905A88`=6e-5, `0x904DC0`=1.0)<br>HUNGER `0x138 += (300+p150)*70/300` (`0x1312DA4`)<br>BREED `0x13A += (300+p14F)*12/300`, types 3..19 only (`0x1312DA8`)<br>TEDIOUS `0x13C += (300+p14A)*60/300`, then `+(300+p14C)*30/300`, then `+(300−p155)*30/300` (`0x1312DAA/DAC/DAE`); then if TEDIOUS > 9999 and rand < 0.1 (`0x905030`), TEDIOUS = 6000<br>LONELY `0x13E += (300−p14E)*60/300` (`0x1312DB4`)<br>TIRE `0x140 += (300−p14F)*10/300` (`0x1312DBC`)<br>NOURISH `0x144 −= 36` (`0x1312DCA`)<br>CONDITN `0x146 += 80` if NOURISH > 9000 (`0x1312DCC/DCE`)<br>CONDITN `−= 120` if NOURISH < 1000 (`0x1312DD0/DD2`)<br>CONDITN `−= 60` if STRESS `0x142` > 8000 (`0x1312DD4/DD6`)<br>CONDITN `−= 300` if emotion `0x131` > 100 (`0x1312DD8`)<br>THIRSTY `0x148 += 150` (`0x1312DDE`) | `0x535964..0x535DDE` |
| Bond fear (`bond+1`) | Kind act (AddHappiness delta > 0, `0x53E120`): `−= trunc(10(300+p155)/300) + trunc(10(300+p14C)/300)`. Hit/throw (`0x53E1F0`, n = 50 / 10): `+= trunc(n(300−p155)/300) + trunc(n(300−p14C)/300)`. Clamped 0..200. | `0x53E120`, `0x53E1F0` (§3.3) |
| Tired → rest (`0x55C430`, then follow-up `0x5A1BA0` pushed inline onto `D+0x43C[D+0x434]` when count < 15) | Runs only if best < 1.0.<br>`f = (float)((p14A − p155 + 200) / 400.0)` (double `0x905278`)<br>`thr = trunc(f * (6000−2000)) + 2000` (`0x1312EAC`/`0x1312EB0`)<br>`score = 0x562440(TIRE, thr) * 0.99` (`0x1312EB4`), then jitter `0x5624D0`<br>Taken if score > best. `0x562440(v,t)` = `(v−t)/(10000−t)` if v > t, else 0 (`0x1312C82`=10000). | `0x5626D0..0x5627F7` |
| Emotion `0x12E` > 90 → `0x59F8E0` | `score = 1 − (p14C+100)*0.005*(1−0.7)`, no jitter; taken if score > best | `0x59F860`; `0x1312E18`=90, `0x1312E1C`=0.7, `0x9055A8`=0.005 |
| Emotion `0x12C` > 100 → `0x5A0BA0` | `score = 1 − (100−p155)*0.005*(1−0.6)`, then jitter; taken if score > best | `0x5A0B10`; `0x1312E30`=100, `0x1312E34`=0.6 |
| Dark-adult mischief | Gates: `p14B ≤ 30`, type 7..22 with (type−7)%3 == 0, emotion `0x12D > 80` (`0x1312DF8`).<br>If `0x590CE0()` is true and the bond like with the local character is < 0 (`rec+0x16C+6*0x53E0C0()`, signed):<br>• `p14C > 30` and rand < 0.7 (`0x9050C0`): `score = 0x562490(0x12D, 80) * 0.99` (`0x1312E04`), jittered. If > best: `0x5A1460` plus follow-up `0x53D970(EDX=0x5A18A0)`.<br>• Otherwise, if `p14C > −50`: the same score; if > best: `0x5A1800`, no follow-up.<br>Otherwise (or when `p14C ≤ −50`): `0x5A1460` with `score = 1 − (100−p14C)*0.005*(1−0.7)` (`0x1312E00`), no jitter, if > best.<br>`0x562490(v,t)` = `(v−t)/(200−t)` if v > t, else 0 (double `0x9051D8`=200). | `0x5A1240..0x5A1451` |
| Score jitter `0x5624D0` | `s = s*0.4 + rand*(1−0.4)*s`, i.e. `s*(0.4 + 0.6*rand)` (`0x1312DF0`=0.4) | `0x5624D0..0x56250F` |
| Reincarnation | happiness > 50 and STRESS < 700 (§4.2). Personality persists into the next life through the DNA write-back in `0x551560`. | `0x568F30`, `0x551560` |

**Taste (fruit likes)** in eat behaviour `0x5607C0`. The switch on fruit kind (`fruitData1+8`, minus 5, at most 4) uses jump table `0x5613AC` (`0x5608F3..0x560905`):

| Fruit kind | Disliked when | Evidence |
|---|---|---|
| 5 (hero fruit) | `0x535390` is true (dark adult, type 7, 10, … 22) | `0x56090C..0x56091B` |
| 6 (dark fruit) | `0x535360` is true (hero adult, type 6, 9, … 21) | `0x56091D..0x560926` |
| 7 | `rec+0x157` ∈ {2, 4} | `0x56095E..0x560973` |
| 8 | `rec+0x157` ∈ {0, 5} | `0x5609A9` |
| 9 | `rec+0x157` ∈ {1, 3} | `0x560A15..0x560A2B` |

A disliked fruit sets mode 9 and calls `AddHappiness 0x536320` with EDI = −1 and pushed f32 `−0.1` (`0x1312D14`=0.1, negated with FCHS) (`0x56092C..0x560946`, `0x560979..`). `AddHappiness` applies the float to alignment with a sign that depends on the local player's character (`chao-garden-items.md` §1.1), so the alignment change is ∓0.1, not a fixed −0.1. No reader of `0x158` or `0x159` was found.


## 5. Movement and pathing

Notation follows `chao-garden-sync.md`: `D` = Data1, `W` = `obj+0x38` (MOVE_WORK), `rec` = save record (`D+0x5C`). "BAMS" means 0x10000 = 360°. Garden id = `*0x134046C` (1 Neutral, 2 Hero, 3 Dark). Stat scaling "×3" means `u16 rec+X * 3` everywhere except areas 6, 10, 11 (raw).

### 5.1 Summary
- **No pathfinding.** A Chao picks a target point and steers at it in a straight line, turning a fixed number of BAMS per frame. It reacts to obstacles only when it hits them: a wall makes it spin in place, a climbable edge (collision kind 0xCB) makes it climb, a steep slope or a drop makes it hop.
- **Where it can go comes from a 10×10-unit cell grid.** Each garden has a grid baked into the exe (`0x1362130` N, `0x1363530` H, `0x1364930` D; 32 cols × 40 rows of `int`, 0x1400 bytes each). At garden load the cells are put into lists by value (`0x534EF0`, called only from `0x52B030`). Wandering only targets cells with value **1** (land). Water tests use value **9**. This is pure data (3×5120 bytes); the server reads the same grids (`src/chao_sim/land.rs`).
- **Physical movement uses the real level collision.** The engine ray-casts the garden LandTable's triangles (`objLandTableNeut/Hero/Dark`, from the Chao data DLL) on ±X/±Y/±Z every frame (`0x797E10`). That gives floor height, wall push-out and wall-hit flags. To reproduce walking exactly the server needs those meshes; to reproduce only *where Chao choose to go*, the grid is enough.
- **Water is a fixed height per garden, not geometry.** Neutral −2.2 (AI check `0x561630`) or −2.0 (physics `0x53DF00`), Hero −1.5, Dark −1.0.

### 5.2 Per-frame physics pipeline (ground mode)
Order inside MainSub `0x54FE20` (chao-garden-sync §1): AI `0x53DC40` → … → `0x796780` (only when `D+0xA0 & 2`; CreateChao sets this bit at `0x55052F`) → `0x53DF00`.

| Step | Address | What it does |
|---|---|---|
| Integrate | `0x796780` | `a.y(W+0x10) += W+0x48` (gravity), then `v.x += W+0xC`, `v.y += W+0x10`, `v.z += W+0x14`, then clears `W+0xC..0x14`. **Rest** (only if `W+0x40&8`): if not resting, on ground (`0x400`), not wall-pushed (`0x800`) and `|v| < 0.01` (`*0x109E000`): `v = 0`, `W+0x40 |= 0x10`, return without moving. If already resting: stay put (return) while `|v| <= 0.01` and the byte `*(*(D+0x2C)+4) & 0x10` is clear; otherwise clear `0x10`. Finally `pos += v`. |
| Gravity | `0x5501D0` (`0x55034E`) | `W+0x48 = *0x908548 = −0.055` |
| Physics dispatch | `0x53DF00` | Returns at once if `W+0x40&0x10`. `W+0x40 &= 0xA0FF`. Mode `u16 D+0xA4`: 0 → `0x798300`; 1 → heightmap `0x53DD50` (on hit: `D+4|=1`, `|=0x400`, `v.xz *= W+0xD0`, snap y, **return 1 before the water write**); 2 → `0x798300` only if `D+0xA0&0x80`, then `0x53E3E0`. Then writes `W+0xDC` per garden: 1 → `*0x104FF64` = **−2.0**, 2 → **−1.5**, 3 → `*0xB087CC` = **−1.0** (other areas: unchanged). |
| Ground collide | `0x798300` (ESI=obj) | Skipped if `W+0x40&0x10`. `D+4 &= ~1`, `W+0x42 = 0`, then `0x797E10` (query) → `0x796B20` (floor) → `0x796CA0` (walls). |
| Floor | `0x796B20` | Clears `0x400` first. `feet = y + W+0xAC + W+0xBC` (3.0 − 3.0 = y). If `feet < floorY (W+0x13C)`: `y += floorY − feet`. If `!(W+0x40&4) or normal.y > 0.78`: `v.y -= 2·v.y·W+0xC8` (0.7 → `v.y ×= −0.4`); else reflect v along the floor normal with the same factor. If `!(W+0x40&4)`: `v.x,v.z ×= W+0xD0` (**0.6**). Then clears `W+0x40` bit 4, `D+4 |= 1`, `W+0x40 |= 0x400`. |
| Walls | `0x796CA0` | Starts with `W+0x40 &= 0xB7FF` (clears 0x800, 0x4000). Uses the four horizontal slots (−X `W+0xEC`, +X `W+0x10C`, −Z `W+0x16C`, +Z `W+0x18C`; normal at slot+0x14) with radius `W+0xB8` = **2.2**. A slot counts only if the normal's component along that axis is outside ±0.2. Distance `d` from `0x780310`: for component < −0.2 push if `r − d > 0`, for > 0.2 push if `d + r < 8.0` (`*0xC5F0A4`); push-out is that overlap ·0.95 along the normal, sets `0x800`. If `normal·(sin D+0xC, 0, cos D+0xC) < −0.07071136`: wall stored at `W+0x90..0xA4`, `W+0x40 |= 0x4000`. If `v·n < 0`: `v += −2·(v·n̂)·W+0xC4·n̂` (W+0xC4 = 0.8, so the normal component becomes −0.6×). |

MOVE_WORK init: `0x7966D0` allocates 0x270 bytes (4-byte 0x12345678 header), `W` = block+4, zeroes 0x26C bytes, sets defaults; `0x5501D0` then overrides some.

| W+ | Value | Set at | Use |
|---|---|---|---|
| 0x00..0x08 | v | | velocity |
| 0x0C..0x14 | a | | per-frame acceleration set by behaviours (cleared by `0x796780`) |
| 0x1C | BAMS | `0x796910` | last computed target angle |
| 0x28 | 0x100 | `0x7966D0` | use not traced |
| 0x30 | ftol(`0x57A7A0`(3.0)) | `0x5503BA` | use not traced |
| 0x40 | u16 | | flags: 0x4 (special floor mode, cleared by `0x796B20`), 0x8 may rest (`0x5503B5`), 0x10 resting, 0x400 on ground, 0x800 pushed by wall, 0x2000 hit a water polygon, 0x4000 facing into a wall |
| 0x42 | u16 | `0x550377` = `D+3`; zeroed every frame by `0x798300` | |
| 0x48 | −0.055 | `0x55035B` | gravity |
| 0x54..0x5C | pos | | behaviour target |
| 0x90..0xA4 | | `0x796CA0` | last wall hit (coord + normal) |
| 0xAC | 3.0 | `0x7966D0`, `0x55036B` (`*0x1286DD4`) | query-centre height |
| 0xB4 | 1.5 | default 3.0 (`0x7966D0`); `0x55037B` (`*0x1283DDC`) | use not traced |
| 0xB8 | 2.2 | `0x7966D0` (`*0x1681DD4`) | wall radius |
| 0xBC | −3.0 | `0x7966D0`, `0x550387` (`*0xC64B44`) | feet offset |
| 0xC0 | `*0x8AE63C` | `0x7966D0` | use not traced |
| 0xC4 | 0.8 | `0x7966D0` (`*0xDF21B4`) | wall restitution |
| 0xC8 | 0.7 | default 0.9 (`*0x8AE548`); `0x550393` (`*0x1312C78`) | floor restitution |
| 0xCC | `*0x145D4D0` | `0x7966D0` | use not traced |
| 0xD0 | 0.6 | default 0.8; `0x55039F` (`*0x1312C74`) | ground friction |
| 0xDC | f32 | reset to −2,000,000 at the start of `0x797E10`, then water polygon / `0x53DF00` / `0x561630` | water surface Y |
| 0xE0..0xE8 | | `0x797E10` | query point = pos + (0, W+0xAC, 0) |
| 0xEC + k·0x20 | 6 slots | `0x797E10` | ray hits −X, +X, −Y (floor), +Y, −Z, +Z: `+0` hit, `+0x10` hit coordinate (init −1e6/+1e6/−1e6/+1e6/−1e6/+1e6), `+0x14..0x1C` normal; slots are refilled each frame from templates at `W+0x1AC..` |
| 0x13C / 0x140..0x148 | | | floor Y / floor normal |

### 5.3 Collision and terrain data the Chao queries
| Data | Where it comes from | Read by | Notes |
|---|---|---|---|
| **LandTable triangles** | Garden constructor: `PUSH "objLandTableNeut"` (`0x12DD80C`) at `0x54C7F4`, `GetProcAddress` via `[0x8730D4]` at `0x54C7FA`, `[0x1A5A2D8] = 1`, then call `0x47BD30` at `0x54C80F` (Neutral ctor `0x54C7B0`); Hero `PUSH "objLandTableHero"` (`0x12DBB0C`) at `0x54D22D` → `0x47BD30` at `0x54D247`; Dark `PUSH "objLandTableDark"` (`0x12E0A94`) at `0x54B74D` → `0x47BD30` at `0x54B767` | `0x797E10` → `0x47CD60` → `0x48C5E0` | `0x47CD60(x, y+3, z, 1.0)` builds a candidate list (max 1024, count `0x1DE9484`, buffer `*0x1A5A2DC`, 12-byte entries `{flags, COL*, int}`). **Dynamic list first** (`0x1A5A2D0`, count `0x1A5A2C8`): kept if distance < model radius × **min** of the 3 scales (or radius alone if `*COL & 4`) + 1 + 30; entries with `0x10000000` use their object matrix; water entries (`&0x2002`) ignore Y. **Landtable list second**, only if `*0x171CAAC == 1 && *0x19459DC != 0` (`0x1A5A2D4`, count `0x1A5A2CC`, stride 0x20): kept if flags `+0x1C & 0x2003` and distance − 1 < entry radius `+0xC`. `0x797E10` re-checks entries whose `+8 != 0` against radius × **max** scale, then calls `0x48C5E0(EDX=entry, EDI=local buffer)` per entry (`0x798095`). `0x48C5E0` clears the 6 slot flags and sets each slot's `+0x10` to pos ± 220. It finds the node in `*0x1A5A400` whose `+8 == EDX` and tests only triangles whose first vertex is within **220** (cumulative dx², +dz², +dy² ≤ 48400; `0x905B90` f32 48400, `0x905B98` 220.0). How `0x797E10` merges the per-entry buffers after `0x79809A` is not traced. |
| Water polygons | COL flags `& 0x2002` | `0x797E10` | Excluded from the +X, floor and ceiling slots (not from −X, −Z, +Z). A water hit on the floor ray sets `W+0xDC = hitY`, `W+0x40 |= 0x2000`; a water hit on the ceiling ray also sets `W+0xDC` to the **floor** ray's hit Y (`local_13c`). The value is then overwritten by `0x53DF00`. |
| **Water level** | Hard-coded | `0x561630`, `0x53DF00` | `0x561630`: N `0xC00CCCCD` = **−2.2**, H `0xBFC00000` = **−1.5**, D `*0xB087CC` = **−1.0**, other areas `0xC61C4000` = −10000. `0x53DF00`: N −2.0, H −1.5, D −1.0. Interrupts run before behaviours in the AI tick, so Neutral behaviours see −2.2. |
| **Cell grid** | Static: `0x1362130` (N), `0x1363530` (H), `0x1364930` (D), 32 cols × 40 rows `int` | `0x535010(EAX=&pos)` | Valid only for `x∈[−150,150]` and `z∈[−200,150]` (inclusive). `col = ftol(x·0.1 + 100) − 84` (`0x53508F`), `row = ftol(z·0.1 + 100) − 80` (`0x53509F`); index `row·32 + col`. Constants: −150 `0x905640` (f32), 150 `0x9050F8`, −200 `0x9051F0`, 0.1 `0x905030`, 100 `0x904E30` (doubles). Out of range it returns EAX holding the FNSTSW status in AX (C0/C2 or TOP bits set) and the input pointer's high half, so never 1 or 9. Cell (c,r) covers `x ∈ [(c−16)·10, +10)`, `z ∈ [(r−20)·10, +10)`. All non-zero cells lie in rows 2..31, cols 1..29. |
| Cell lists | `0x534EF0`: memsets `0x1A15938` (0x10040 bytes); for each cell with v ∈ 1..15 appends `{col,row}` to list v at `0x1A15938 + v·0x1004` (`+0` count, `+4+i·8` col, `+8+i·8` row) | `0x534F80` | List 1 = `0x1A1693C` (land), list 8 = `0x1A1D958`, list 9 = `0x1A1E95C` (water). |
| Random point in a list | `0x534F80(EAX=list, push count, EDI=&out)` | `0x562D20`, `0x561740`, `0x5A1020` | `i = ftol(rand/32768 · (count − 1e-5))`; `x = (rand/32768 + col − 16)·10`, `z = (rand/32768 + row − 20)·10`. **Y is not written.** Returns 1. |
| Heightmap (mode 1 only) | `0x1DBE66C` u16 grid | `0x53DD50` (only caller `0x53DF75`, in `0x53DF00`: u16 `D+0xA4` 0 → `0x53DFC3`, 1 → heightmap, 2 → `0x798300`/`0x53E3E0`) | x,z clamped to ±200. Index = `ftol(x+200) + ftol(z+200)` with **no row stride** (`0x53DDE3`). Heights = (u16 & 0x3FFF)·0.04 − 100 from words i, i+1, i+2, interpolated with `0x8294A0` (modf) and written to `[out+4]`. If `D.y` ≤ height: `D+4 |= 1`, `W+0x40 |= 0x400` (`0x53DF8D..0x53DFA2`). CreateChao zeroes Data1 (memset 0x1D74 at `0x5502A8`) and neither `0x5501D0` nor `0x53DA80` writes `D+0xA4`, so garden Chao start in mode 0 (other callees not checked). |
| Object signals | collision kinds | `0x486760(obj, 0xCB)` at `0x5A3051`, `0x561B98`; `0x54B1F0(ECX=1, EDX=obj, push 0xCC)` at `0x5A30AD` | 0xCB touch with `ret != 0 && *(ret+4) != 0` → climb. 0xCC nearby → turn away. Sources and touch test: garden-collision.md §8 (0xCB = C_CLIMB boxes, a pure trigger; 0xCC = Hero pillars and Dark fences/graves, §8.4: solid to the Chao and a turn-away trigger, but object colliders, **not in the LandTable**). |

Grid contents (value counts, decoded from REA bytes; 0 = not walkable / outside):

| Garden | 1 (land, wander list) | 9 (water) | 8 | others |
|---|---|---|---|---|
| Neutral `0x1362130` | 288 | 45 | 21 | — |
| Hero `0x1363530` | 316 | 86 | 14 | 2: 139, 4: 15, 6: 12 |
| Dark `0x1364930` | 185 | 59 | 54 | — |

Only the meaning of 1 (wander targets) and 9 (water tests) is established from code. Checked against the LandTable in garden-collision.md §6b: some Hero "water" cells sit over solid floor, and a few Hero/Dark land cells at the shore are open water (§7.2), so a Chao sent there swims. Lists 2/4/6 (`0x1A17940` …) have no xrefs, so the Hero value-2 region (rows 18..31, cols 17..29) is never a wander target. List 8 is read only inside `0x57BD40` (`0x57C407`, `0x57C68E`; small-animal main). List 1 is also read by `0x567830` (`0x567A7E`), `0x59F490` (`0x59F5C7`) and `0x57BD40`; `0x535010` is also called by `0x599B60`, `0x59F490`, `0x57BD40` (not traced).

### 5.4 Choosing a target
| Routine | Address | Target |
|---|---|---|
| Wander | `0x562D20` (`[ESP+8]` = obj) | If list-1 count > 0: `0x534F80(list1, count, EDI=&W+0x54)`; then always `0x5A3650` (ESI=obj). Returns 0. |
| Go to water | `0x5A1020` | If list-9 count > 0: random point in a water cell → `W+0x54`. If this area is active: sound 0x6043, or 0x6044 only when r == 0.0 exactly (`0x904E00` = 0.0, `0x5A1057..0x5A1086`). Then installs the walk via `0x5A3650` (ESI=obj, `0x5A109F`). Returns 0. |
| Swim target | `0x561740` (EAX=obj) | If `rec+0x148 < 100`: random land cell (list 1) if any, else the target is left as it was; Y and `W+0x1C` not written. Else: random heading, distance `(0.3 + 0.7r)·60` (18..60), repeated until `0x535010 == 9`, 100 tries in all (1+99), else x=z=0, y = own y. Writes `W+0x54..0x5C` (y = current y) and `W+0x1C`. |
| Fixed objects | `0x56BA80`, `0x56B6C0` | Steer at the ALW-linked object via `0x530510` (angle `0x54B0A0`, approach `0x446960`); distance from `0x530580` (calls `0x77FBD0`). |

**Grid lookups** (grid vs LandTable in garden-collision.md §6b/§7). `0x535010` (EAX = &pos): only for x in −150..=150 and z in −200..=150 (outside, it returns the FPU status word, a junk non-zero value); col = `ftol(…) − 0x54`, row = `ftol(…) − 0x50`, i.e. col = x/10 + 16, row = z/10 + 20; value = `grid[row·32 + col]` of the garden `*0x134046C` (1 → `0x1362130`, 2 → `0x1363530`, 3 → `0x1364930`; other areas 0). `0x534F80` (EAX = list, EDI = out, push count): random entry (`+4 + i·8` col, `+8 + i·8` row), then `x = (col − 16 + r)·10`, `z = (row − 20 + r)·10` (r = rand·3.0517578e−05); y not written.

### 5.5 Walking
**Installer `0x5A3650` (ESI=obj).** `run = rec+0x3C` (×3 rule; compared as u16 `AX`). The prelude is set with `AL_SetBehavior` (clears the stack) and the walk is pushed with frame limit −1 (manual push for the first two tiers, `0x53D970` for the last two):

| run (scaled) | Prelude | Walk | Motion | Accel per frame (on ground only) | Motion speed `D+0xB8` | Random event |
|---|---|---|---|---|---|---|
| < 150 (`*0x1312D6E`) | `0x55C430` | `0x5A3280` | 0x73 | 0.038 (`*0x1312C6C`) | — | sound every 120 frames of `D+0x30` (`% 120 == 0`), 30%, active area only |
| 150..1999 (`*0x1312D70`) | `0x55C3C0` | `0x5A3380` | 0x64 | 0.055 (`*0x1312C68`) | `|v|·16` (`0x77FC30`) | `D+0x1C0++`; when old value `% 85 == 1`, 3% → `0x562D50` |
| 2000..3999 (`*0x1312D72` = 4000) | `0x55C3C0` | `0x5A3460` | 0x65 | 0.028 + run·7e-6 (`*0x1312D74`, `*0x1312D78`) | `|v|·14` (`0x9052E0`) | `D+0x1C0++`; when old value `% 100 == 0`, 2% (`0x9054C0`) → `0x562D50` |
| ≥ 4000 | `0x55C3C0` | `0x5A3580` | 0x66 | 0.028 + run·7e-6 | `|v|·14` | — |

- Acceleration is along the facing: `a.x = sin(D+0xC)·k`, `a.z = cos(D+0xC)·k` (`0x42AAB0`/`0x42AC30`), only while `W+0x40&0x400`.
- With friction 0.6 on every floor contact, steady ground speed ≈ **1.5·k** units/frame (derived, not measured): 0.057 / 0.0825 / up to ≈0.145.
- **Walk step `0x5A3000`** (ESI=obj; every walk variant calls it last). Steps 2–6 only call `AL_SetBehavior` and **do not return**; several can fire in one frame and the last one wins (order: wall, 0xCC, hop/flutter). Turn and latch still run.
  1. Arrive if XZ distance² to `W+0x54` < 36.0 (`*0x8AE5DC`) → return 1 (stack empties, AI settles in `0x565BE0` and runs the picker).
  2. If `W+0x40 & 0x4000`: if `0x486760(obj, 0xCB)` returns an entry with `+4 != 0` → `0x562EB0` (climb) and `y += 0.1`. Otherwise → `0x56B740` (spin away).
  3. `u16 D+6++`; if the old value > 30 and `0x54B1F0(0xCC)` finds an object → `0x56B740`.
  4. `0x796910(0x120)` (≈1.58°/frame).
  5. On ground and floor `normal.y < 0.7` (`*0x109DFF0`) → `v.y += 0.55` (`0x9067E8`), set `0x5A0F40` (hop; never flutter here).
  6. Airborne and `y − floorY(W+0x13C) > 2.0` (`0x904F78`) → `v.y += 0.55`. Then if `0x535010(EAX=&W+0x54) == 9` → hop; else if fly (`rec+0x3A`, ×3) < 400 or rand < 0.6 (`0x905268`) → hop; else `0x5630C0` (flutter, so 40% when fly ≥ 400).
  7. Once-latch `rec+0x12A`: `rec+0x13C −= *0x1312DB0` (100) clamped ≥ 0, `rec+0x140 += *0x1312DC2` (300) clamped ≤ 10000.
  - No timeout in the walk itself (limit −1).
- **Preludes (stand-up transitions).** Read posture `u16 D+0xAE` (MotionTable +2). Mode 0 starts a blend with `0x793D30`, sets mode 1; mode 1 returns 1 (pop to the walk) once `D+0xAC & 1`.
  - `0x55C3C0`: posture 1 → motion **0x30**, posture 2 → **0x26** (both blend 0x28), posture 3 → 0x26 (blend 0x23); posture 0 or ≥ 4 → returns 1 at once.
  - `0x55C430`: posture 0 → **0x2F**, posture 2 → **0x2A** (both blend 0x28), posture 3 → 0x2A (blend 0x23); posture 1 or ≥ 4 → returns 1 at once.
- **Turn helper `0x796910(rate)`** (EAX=obj): `W+0x1C = atan2(target − pos)` (`0x859B6A`); `D+0xC` moves toward it by at most `rate` BAMS (snaps when |diff| ≤ rate; stored as u16); returns the remaining |diff| (s16). `0x796AB0(rate)` is the same toward player `[0x1DEA6C0 + ECX*4]` (angle from `0x7969B0`; ECX = player index, the whistle passes 0).

### 5.6 Obstacles, slopes, ledges and falling
| Behaviour | Address | Trigger | Effect | Ends |
|---|---|---|---|---|
| Spin away | `0x56B740` | wall hit without 0xCB, or 0xCC object (`0x5A3000`) | Mode 0: motion 0x64 blend 15 (`0x793D30`); rand < 0.5 (`0x904E98`) → mode 1 (+0x16C BAMS/frame) else mode 2 (−0x16C); `D+0x1C0 = 45 + trunc(46r)` (45..90; −46 `0x905A68`); `D+0xB8 = 1.6` (`*0x1708AA4`). Modes 1/2: count down, rotate. | Return 1 when the counter reaches 0 |
| Hop | `0x5A0F40` | steep floor, or airborne > 2 above floor (`0x5A3000`) | Mode 0: eyes (`0x53A4B0`, 1), mouth (`0x53A5A0`, 2), motion 0x8A, **`v = (sin·0.4, 0.35, cos·0.4)`** (0.4 `0x904EB0`, 0.35 `*0xA3D28C`; overwrites the +0.55 from `0x5A3000`), mode 1, `D+0x1C0 = 0`. Every frame, all modes: `a.y = −g·0.45` (`0x905398`; net gravity −0.03025). | Mode 1 on ground → `0x562D20` |
| Flutter / glide | `0x5630C0` | as hop, fly ≥ 400 and 40%, target cell not water | Mode 0: eyes (4), motion 0x82 blend 15, launch velocity as the hop, `D+0x1C0 = 300 + trunc(301r)` (−301 `0x905A08`). Every frame (all modes): turn 0x100 (`0x796910`), `a.xz = dir·0.05 − v·0.1` (`0x9050D8`, `0x905030`); while `D+0x1C0 > 0`: decrement, `a.y = −g·0.95` (`0x905810`), else `−g·0.92` (`0x905BC8`); if y > 100 (`0x904E30`) then y = 100 (`*0xC5D234`). | Mode 1 on ground → `0x562D20` |
| Climb ledge | `0x562EB0` | wall + 0xCB (`0x5A3000`); in water: wall, `rec+0x148 < 1000` and 0xCB (`0x561AF0`) | Mode 0: motion 0x9B, `v = 0`, mode 1, and falls into mode 1 the same frame. 0xCB comes from the garden's C_CLIMB boxes (garden-collision.md §8). Mode 1: while `W+0x40 & 0x4800`, face the wall (`0x446960`), clear 0x4000, `D+0x1C0 = 0`; when clear, `D+0x1C0++` and once the old value > 2: `D+0x54 = y`, mode 2. Mode 2: `D+0x1C0++`; if < 4 or airborne: back to mode 1 when `&0x4800`; else (≥ 4, on ground) if `0x77FBA0` > 0.7 → wander; also wander if `D+0x1C0 > 30` or `y − D+0x54 > 2.5`. Every frame: `x += sin·0.1`, `y += 0.08`, `z += cos·0.1`, `a.y = −gravity`. | `D+0x1C4` old > 240 → return 1 |
| Stumble / fall over | `0x562D50` | `0x5A3380` and `0x5A3460` (see table above) | Jump table `0x562E94`. Mode 0: motion 0x9F blend 15, `D+0x1C0 = 0`, falls into mode 1. Mode 1: when counter == 40 and area active, sound 0x100A at pos (`0x4372E0`); counter++; when old > 60: icon 5 for 240 frames (`0x53D660`), eyes (1), mode 2; while in mode 1 and on ground, forward accel 0.055 (`*0x1312C68`). Mode 2: counter++; when old > 300: motion 0xA0 blend 15, mode 3. Mode 3: on `D+0xAC & 1` → `0x53DA00`. | `0x53DA00` resets mode/timers, `D+0x1C8 = 3600`, swaps `D+0x430` (previous fn) into the current stack slot, i.e. resumes the walk |

There is no separate falling state: a Chao that walks off an edge is caught by step 6 of `0x5A3000`. A thrown or dropped Chao is section 6.

### 5.7 Swimming
- **Entering (interrupt `0x561630`, first check in `0x53DB10`).** Flags are on `u16 D+0x1B4`.
  - Held (`D+4 & 0x8000`): `D+0x1B4 &= ~0x1`, return 0.
  - Writes `W+0xDC` (water level, §5.3).
  - If `W+0xDC ≤ y + 2.0`: `D+0x1B4 &= 0xFFFA` (clears 0x1 and 0x4), return 0.
  - Otherwise, on first entry (0x1 clear): set 0x1, play sound 0x1020 if this area is active (`*0x134046C == *0x134062C`), `AL_SetBehavior(0x562330)`.
  - Each frame in water: if `v.y < 0` then `v.y ×= 0.1`; set 0x4; return 1. "In water" = more than 2.0 below the surface.
- **Dispatcher `0x562330`.** `swim = rec+0x38` (×3). Calls `0x561740` (new target), then `*(u32*)(*(u32*)(*(D+0x2C)+0xC) + 0x64) |= 0x10` (meaning not traced). Each `AL_SetBehavior` clears the stack, so the last wins:
  - swim < 300 (`*0x1312D7E`) → `0x561E00`;
  - otherwise `0x561FB0` (< 2000, `*0x1312D80`) or `0x562040` (≥ 2000); then if `rec+0x118 & 4`, 50% → `0x5620D0`; then 20% → `0x562160` (float).
  - Then calls `0x561AF0` once; returns 0.
- **Swim movement `0x561AF0`** (EAX=obj; every swim variant calls it each frame). It does not return early; several `AL_SetBehavior` calls can happen in one frame (last wins):
  - `D+0x1B4 & 4` clear and on ground → `0x562D20` (walked out).
  - **`D+0x1C4` is incremented twice per call** (`0x561B64`, `0x561C36`).
  - Wall (`0x4000`): if `rec+0x148 < 1000`: `0x486760(obj,0xCB)` hit → `0x562EB0` (climb out), no hit → nothing; if `rec+0x148 ≥ 1000`: 0.2% (`0x9055B0`) → `0x562330`.
  - XZ distance² to target < 36 → `0x562330`. Second-increment old value of `D+0x1C4` > 600 → `0x562330`, i.e. about **300 frames** after the behaviour started.
  - Turn 0x120 BAMS/frame.
  - Only while `y ≤ surface − 2.1` (`0x906C10`): `y = min(y + 0.1, surface − 2.1)`; `a.y = −g − v.y·0.1`; `a.xz = dir·(swim·1e-6 + 0.0025) − v·0.05` (`*0x1312D88`, `*0x1312D84`, `0x9050D8`). Terminal speed ≈ 20·k (derived). Above that depth there is no propulsion and gravity pulls the Chao down.
  - Once-latch `rec+0x12A`: `rec+0x148 −= *0x1312DE0` (600), `rec+0x13C −= *0x1312DB2` (100), `rec+0x140 += *0x1312DC6` (100), clamped 0..10000.
  - Every frame, if area active: `0x437590(EDI=0x101F, ESI=&pos, 1, −25, 0x5A)` (sound).
- **Variants.**
  - `0x561E00` (swim < 300): mode 0: eyes 1 (EBP = 1, duration −1); mouth: if `D+0x69E == 1`, 25% → 9 (else none); otherwise r < 0.25 → 6, r < `*0x165DFD4` → 8, r < 0.75 → 0xB, else 4; motion 0x8E; mode 1. Every 120 frames of `D+0x30`, 20%, active area: sound. Once-latch `rec+0x148 −= 10·600 = 6000`. Then `0x561AF0`.
  - `0x561FB0`: motion 0x94 (`0x56201D`). `0x562040`: motion 0x91 (`0x5620AD`). `0x5620D0`: motion 0x92 (`0x56213D`). Set once in mode 0 via `0x793C40` (no blend); then step `0x561AF0` every frame, return 0. (Labels, not functions.)
  - `0x562160` (float): cases fall through (mode 1 → 2 detail in §2B). 0: motion 0x9A blend 20 (`0x793D30(0x14)`), mode 1. 1: `D+0x1C0++`, old > 30 → eyes, mouth 2, sound if active, mode 2. 2: on `D+0xAC & 1`: eyes with chance, motion 0x96, `D+0x1C0 = 600 − trunc(r·−601)` (600..1200), mode 3. 3: count down; old < 1 → `0x562330`. Every frame `0x561920`.
  - `0x561920` (float movement): walked out uses `D+0x1B4 & **1**` clear and on ground → `0x562D20`. `D+0x1C4++`. While `y ≤ surface − 2.1`: rise 0.02/frame clamped, `a.y = −g − v.y·0.05`, `a.xz = sin·0.0 − v·0.05` (`0x904E00` = 0.0; no propulsion). Once-latch: `0x148 −= 600`, `0x13C −= 100`, `0x140 −= *0x1312DC8` (100). Sound 0x1021 `0x437590(1, 0, 0x78)` every frame if active.
- **Leaving.** (a) swim target on land when `rec+0x148 < 100` (`0x561740`); the Chao walks out once the water interrupt clears bit 0x4 and it is on ground (`0x561AF0` → `0x562D20`); (b) climbs out at a 0xCB edge (`0x561AF0` → `0x562EB0`, only while `rec+0x148 < 1000`); (c) any interrupt.

### 5.8 Generic movement helpers (pushed by choosers)
| Address | Name | Behaviour |
|---|---|---|
| `0x56B480` | notice pause | Mode 0: icon 1 for 180 frames (`0x53D660`), `D+0x1C8 = 1800`, `D+0x1C0 = D+0x1C4 = 0`. Mode 1: `D+0x1C0++`, old > 120 → return 1. Every frame `D+0x1C8--`; ≤ 0 → `0x565BE0`. |
| `0x56B500` | face target | Mode 0: motion 0x64 blend 15, `D+0x1C0 = 60`. Each frame `0x796910(0x200)`; returns 1 when the remaining diff < 0xB6 (≈1°). |
| `0x56B560` | walk to `W+0x54` | Mode 0: eyes (4), `D+0x1C8 = 1800` (every mode-0 frame); after `D+0x1C0` old > 30: motion 0x73 (run×3 < 150) or 0x64, `D+0xB8 = 1.5` (`*0x1283DDC`), mode 1. Mode 1: `0x796910(0x180)`, `a.xz = dir·0.055·0.8 = 0.044` (`0x905230`; no on-ground check). 3D distance `0x7968A0` < 0.0 (`*0x8AF004`) → `0x565BE0` (cannot fire); < 4.0 (`*0x104DD30`) → return 1. Every frame `D+0x1C8--`; ≤ 0 → `0x565BE0`. |
| `0x56B6C0` | face linked object | Mode 0: motion 0x64 blend 15 (`0x793D30`), `D+0x1C0 = 90`, mode 1. Mode 1: `D+0x1C0--`; while the old value > 0, `0x530510(0x200)`; at 0 → return 1 (pop). `D+0xB8 = 1.6` (`*0x1708AA4`) on every non-returning frame (`0x56B6C0..0x56B73D`). |
| `0x56B960` | random spin | Like `0x56B740`, but `ftol(91r)` frames stored as u16 (0..90; 91.0 at `0x905A60`). |
| `0x56BA80` | walk to linked object | Same layout as `0x56B560`: steers with `0x530510(0x180)`, accel 0.044, distance from `0x530580`; < 0.0 → `0x565BE0` (cannot fire: with no link `0x530580` returns 0.0, so the walk returns 1 on frame 33); < 4.0 → return 1; motion always 0x64; `D+0xB8 = 1.5`; 1800-frame timeout. |
| `0x56BBA0` | press switch | Mode 0: motion 0xBE (`0x793C40`), `D+0x1C0 = 0`. Mode 1: `D+0x1C0++`; when old > 60: emits signal 0xAF via `0x55A920` (EBX = 10 frames, radius `*0x16AFE94`) at pos + (sin·`0x904DD8`, `0x904DC0`, cos·`0x904DD8`) and returns 1. Otherwise, on ground, `v ×= 0.9` on all three axes (`0x904E28`). Signal 0xAF switches TV/radio/box on (chao-garden-sync, Toys). |

### 5.9 Per-garden differences (movement only)
| | Neutral | Hero | Dark |
|---|---|---|---|
| LandTable export | `objLandTableNeut` (`0x54C7F4`) | `objLandTableHero` (`0x54D22D`) | `objLandTableDark` (`0x54B74D`) |
| Water Y (AI / physics) | −2.2 / −2.0 | −1.5 / −1.5 | −1.0 / −1.0 |
| Grid | `0x1362130` | `0x1363530` (extra values 2/4/6) | `0x1364930` |
| Land / water cells | 288 / 45 | 316 / 86 | 185 / 59 |

Stat scaling (×3 in gardens, raw in areas 6/10/11) is identical in all three gardens (`0x5A3650`, `0x5A3000`, `0x562330`, `0x561AF0`).

### 5.10 Notes
- `D+4 & 1` is the **on-ground bit**: cleared at `0x798300` and `0x53E048`, set at `0x796B20` and in `0x53DF00` together with `W+0x40 |= 0x400` (`0x53DF90`). It has no "pettable/touched" meaning; the pet check `0x564C10` just requires the Chao to be on the ground.
- `0x562D50` is the stumble/fall-over behaviour, not an idle.
- `0x56B960` is a random spin in place, not "toward bed".
- `0x5A0F40` is a hop and `0x5630C0` a flutter/glide; neither climbs.
- `0x55C3C0`/`0x55C430` are posture (stand-up) transitions.
- `0x5A1020` walks to a random **water cell** (list 9).
- Neutral water level differs between the AI check (−2.2, `0x561630`) and physics (−2.0, `0x53DF00`).
- Pond floors vary. Where the pond-bed COLs carry the water bits, `0x797E10` keeps them out of the floor slot, so there is no floor for the physics and a Chao is held up only by its strokes. But some ponds do have solid floors: garden-collision.md §7.2 finds a solid pool floor (COL #69 `0x20000001`, −6.5 Hero / −6.0 Dark) under some shore cells. Which ponds have a solid floor is not mapped. Pond edges are walls, and climbing out needs a 0xCB (C_CLIMB) touch (garden-collision.md §8).
- The preludes map postures to motions as in §2A.1; the swim timeout is ≈300 frames (the counter is incremented twice per frame); the swim wall 0.2% branch runs only when `rec+0x148 ≥ 1000`; `0x5A3000`/`0x561AF0` keep running after `AL_SetBehavior`; stumble checks use `D+0x1C0` with `%85 == 1` / `%100 == 0`; `0x47CD60` uses the min scale for the dynamic list and the entry radius for the landtable list.


## 6. Interactions

Scope: what a garden Chao decides and changes when it deals with fruit, toys, trees/seeds, other Chao and the player. Item-side effects (fruit bite stats, GrowStats, AddHappiness, bonds, animal parts, item objects/registry) are already in `chao-garden-items.md` §1–§3 and are cited, not repeated. Held-Chao mechanics are in `chao-garden-sync.md` §3.

Notation as in the groundwork: `D`=Data1, `W`=`obj+0x38`, `rec`=`D+0x5C`. Need fields: SLEEPY `0x134`, SLPDPTH `0x136`, HUNGER `0x138`, BREED `0x13A`, TEDIOUS `0x13C`, LONELY `0x13E`, TIRE `0x140`, THIRSTY `0x148` (labels per `chao-details.md`). `r` = `rand()/32768` (`0x904E90` = 1/32768). "jitter" = `0x5624D0` (s·(0.4+0.6r)). `f16(v,t)=(v−t)/(10000−t)` (`0x562440`), `f8(v,t)=(v−t)/(200−t)` (`0x562490`). All need/emotion writes clamp to 0..10000 (u16 needs) or 0..200 (emotion bytes).

**"best < 1.0" gate.** Choosers that test the running best do `FLD1; FCOMP [best]; TEST AH,0x41; JNP skip`, i.e. they run only while **best < 1.0 (strict)** (seen in `0x5625B0`, `0x562A20`, `0x562880`, `0x59D0D0`, `0x59C3D0`, `0x5A10B0`). Where this section says "best < 1.0" it means this test. `0x562950` (courter) and `0x5A04A0`/`0x5A1240` have no such gate.

### 6.1 How an interaction reaches the Chao

**Reaction (interrupt) order, `0x53DB10`** (runs every AI tick before the behaviour; current behaviour = `D+0x43C[D+0x438]`, `0x53DB25`):
1. Skip everything if the current behaviour is `0x568D30` (cocoon) or `0x568F60` (death) (`0x53DB32`, `0x53DB3E`).
2. Water `0x561630`. If it fires, jump straight to the held check (step 11) (`0x53DB55`).
3. If current is `0x54EF10`/`0x54F7A0` (sleeping) skip steps 4–10 (`0x53DB57`, `0x53DB5F`).
4. Whistle `0x5637B0` → 5. Ball `0x563B70` → 6. Hat `0x5691B0` → 7. Seed (kind 0x85) `0x567560` → 8. Fruit (0x83) `0x560640` → 9. Animal/drive (0x87) `0x566BC0` → 10. 0xA2 `0x564760`.
11. Held `0x566720`; only if it did **not** fire: pet `0x564C10` (`0x53DBCD`..`0x53DBE0`).
12. Hit `0x563D30` (always checked, `0x53DBF0`).
13. If anything fired: clear the Chao's ALW entry flags `+6`, partner `+0x40`, link id `+0xC = −1`, unlink `0x530690` (`0x53DC05`..`0x53DC1E`); then close the ParamFukidasi balloon `0x565B90` unless the held check fired and hit did not (`0x53DC23`..`0x53DC30`).

Every reaction calls `AL_SetBehavior`, so a later one in the list overrides an earlier one in the same frame. The whistle check returns 1 (counts as "fired") for any whistle in range even when it does not set a behaviour (`0x5637B0`). Reactions that do not exclude their own behaviour (ball `0x563B70`) re-fire and restart it every tick while their condition holds (see §6.4 Ball).

**Touch kinds** (`0x486760(obj, kind)` = the Chao touches a collider of this kind). Collision tables are 0x30-byte entries, kind in byte 0, centre `+8`, radius `+0x14`, height `+0x18`:
| Kind | Owner (table, installed at) | Shape | Reaction |
|---|---|---|---|
| 0x83 | fruit (`0x8A7728`, `0x8A76C8`+0x60, `0x5461DA`) | r 4.2, h 8 | `0x560640` |
| 0x85 | seed (`0x8A7458`, `0x549BCD`) | r 4.2, h 8 | `0x567560` |
| 0x87 | animal (`0x8A74E8`, `0x548E44`), Chaos Drive (`0x8A77B8`, `0x5456F3`) | r 4.2, h 8 | `0x566BC0` |
| 0x93 | hat (`0x8A6FC8`, `0x54A619`) | r 4.2, h 5 | `0x5691B0` |
| 0xA0 | ball (`0x8A5BE0`, `0x55D64D`) | r 5.2, h 6 | (not a Chao reaction; ball's own) |
| 0x96 / 0xAC / 0xAE | GrowTree (`0x8A7518`, 9 entries, pushed at `0x548298`): 0x96 r 12 centred y 15 (entry `0x8A7578`); 0xAC r 30 (`0x8A75A8`); 0xAE r 20 (`0x8A7608`) | | 0xAC = plant spot (§6.5), 0xAE = watering (§6.5) |
| 0xA2 | see signal table | | `0x564760` (touch query `0x486760(obj,0xA2)`, `0x564779`) |
| 2 | (player hand; source not traced) | | pet trigger `0x564C10` via `0x54B1F0(2)`; pet continue is a touch query `0x486760(obj,2)` at `0x564B1C` |

**Signals** ("nearby signal" queries `0x486880(obj,1,kind)` / `0x54B1F0`). Chao behaviours emit them with `0x55A920(obj, kind, f32, EDI=&pos, EBX=frames)` (stored in `D+0x40`, deleted on behaviour change):
| Kind | Emitted by | Params | Listened by |
|---|---|---|---|
| 0xA5 | skill A performance `0x59D57F` | 20.0 (`0x8AE6C4`) | `0x59D0D0` (watch/join) |
| 0xA6 | skill B performance `0x59C84E` | 20.0 | `0x59C3D0` |
| 0xA7 | skill C performance `0x59BA15` | 20.0 | `0x59B710` |
| 0xA8 | pushes at `0x59B386`, `0x59BAE2` (not decoded) | | `0x59B820` |
| 0xA9 | sing `0x5AB3BC` | 2.0 (`0x12848D4`), 1800 frames | heart object main `0x5AAFB0` (`0x5AAFD9`) |
| 0xAB | breed caller `0x59BBAA` | 30.0 (`0x89E4DC`), `*(u16*)0x1312D58` (9000) − 120 = 8880 frames (`0x59BB8F`..`0x59BB9B`) | `0x562950`, `0x562880`, `0x59BF30` |
| 0xAD | watering `0x568460` (via `0x55A850`) | 5.0 (`0xB5D210`), 10 frames, 4 units ahead | GrowTree (seed watering, `chao-garden-sync.md` Trees) |
| 0xAF | horse ride `0x59914F`; toy press `0x56BC77` (both 3.5 `0x16AFE94`, 10 frames); `0x55F6BB`, `0x55F8B8` (1.5, 2 frames) | | toys: switches an off toy on (`chao-garden-sync.md` Toys) |
| 0xC9 / 0xCA | TV / radio while playing (TV push at `0x55C992`) | | `0x599A10`, `0x598890` / `0x5998C0`, `0x598B90` |
| 0x97 | animal play `0x5A2793` | 0x3C frames | no listener found |
| 0xA2 | object with main `0x56A110` (init `0x56A700`, collision kinds 0/0xA3) via `0x55A850` at `0x56A23A`; also `0x53F963` | | `0x564760` |

Taking an item from the player: the fruit/seed/animal/hat approach behaviours (`0x5606D0`, `0x567640`, `0x566C40`, `0x569240`) skip items whose ALW entry has flag `+6 & 2` (already held by a Chao), then, if player 1 exists, call `0x46E5E0(EAX=0, EDX=*0x1DEA6C0)` before linking the item (`0x560752`, `0x5676C2`, `0x566CB7`, `0x5692C2`). `0x46E5E0`: if the player's Status `+4 & 0x800` (holding) and it has a held object (`playerwk+0x94`), it clears the player's 0x800 bit, clears the held object's Status 0x8000 and nulls `+0x94` — i.e. **player 1 drops whatever it holds**. The item is then linked by `0x56CFB0`.

### 6.2 The player

**Whistle.** Trigger `0x5637B0`: player 1 (`*0x1DEA6C0`) action byte == 42 (`'*'`) and squared distance < 10000.0 (100 units); then only if `bond[c].like` (`rec+0x16C + c·6`, signed byte, `c = 0x53E0C0()`) > 50 → `AL_SetBehavior(0x5634D0)`. `0x5634D0` does not walk; `0x796AB0` only turns `D+0xC` toward the player by ≤ rate BAMS and returns the remaining |angle| (pseudocode of `0x796AB0`). Walking happens in `0x563250`.
- `0x5634D0` (jump table `0x563790`): m0 motion 0 (blend 10), wait 20+r·40 → m1 motion 50% 0xEE else 3, icon 3 for 180 frames, wait 20+r·60 → m2 motion 0x64 → m3 (`0x563648`): motion speed `D+0xB8 = 1.5` (`0x1283DDC`), turn 0x300/frame until remaining angle < 0x100 BAMS (`0x563668`); then icon 1 for 60 frames, wait 20 + r·20 (`0x9050B0` = 20.0); if r < 0.7 (`0x9050C0`): motion 0x1EC (blend 10), eyes 4 (`0x563705`), sound 0x6019 only when `*0x134046C == *0x134062C` (`0x563719`) → m4; else m5. m4 waits for motion end (`D+0xAC & 1`) → m5; m5 timer → `AL_SetBehavior(0x563250)` (`0x563779`).
- `0x563250`: m0 motion r<0.2 → 0x76, r<0.6 → 0x77, else 0x6A; mouth 40% 1 else 3; icon 4 for 120 frames. Then every frame target = player pos (`0x563335`); every 100 frames, r < 0.1 (`0x905030`) → `0x562D50` (`0x56336D`); ends (return 1) when squared distance < 64.0 (`0x905078`, 8 units, `0x5633D2`); turn 0x100/frame (`0x5633E9`); on ground velocity = facing × 0.05 × (1 + run·0.0001) (`0x9050D8`, `0x905FB0`; run = `rec+0x3C`, ×3 outside areas 6/10/11, area = `*0x134046C`).

**Pet.** Trigger `0x564C10`: current behaviour not `0x5648A0`, `0x54FCF0`, `0x59FCA0`, `0x59F970`, `0x59FA50`; `D+4 & 1`; `0x54B1F0(2)` non-null. Behaviour `0x5648A0` (table `0x564BF0`):
- m0: `0x54A690`; motion by posture `D+0xAE`: 1/3 → 0x3E, 2 → 0x2A, else 0 (blend 20). If `rec+0x132 < 10` (`0x564919`): AddHappiness `0x536320`(EDI=+1, f=0.05 `0x1312D40`) (effects in items §1.2).
- m1 (motion end): if SLPDPTH > 500 → `0x54FCF0` sleep (`0x564958`). Else motion (posture 1–2: 50% 0x41/0x42; else 50% 0xFA/0xFC), counter `+0xC = 10`; 30% (`0x9050C8`) a sound (50% 0x6074 / 0x6075), only when `*0x134046C == *0x134062C` (`0x564A45`, `0x564A71`).
- m2: each motion end counter−1, at 0 → `0x54A730`, m3. If `rec+0x132 > 100`: eyes 3 + mouth 3 (`0x564ABB`). From the 4th frame of m2 on (counter `+0x10` ≥ 3): `rec+0x132 += 1` (cap 200) per frame (`0x564AEE`). While touched by kind 2 (`0x486760(obj,2)`, `0x564B1C`) stay; when not touched: motion 1 (posture 0) / 0xD (posture 1), → m4 (`0x564B61`).
- m3: end after 11 frames (`0x564B80`). m4: from the 16th frame `rec+0x132 −= 1`/frame (`0x564B9B`); ends (return 1) whenever `rec+0x132 < 10` (`0x564BD7`).

**Hit by the player.** Trigger `0x563D30`: Status bit 4 (cleared here), attacker found by `0x486800(obj,1)`; not if current is `0x563EB0`/`0x568D30`/`0x568F60`. Knock-back `W.v = (−0.8·dx̂, 1.5, −0.8·dẑ)` away from the attacker (`0x905230` = 0.8, `0x1283DDC` = 1.5). A worn hat drops (items §5). Behaviour `0x563EB0` (table `0x564308`):
- m0: face along velocity; eyes 1; icon 5 for 150 frames (`0x563F0D`); r < 0.5: motion 0x3E → m1, else 0x10F → m2 (`0x563F33`..`0x563F6D`); AddHappiness(EDI=−30, f=−0.1 `0x1312D3C`) (`0x563F74`..`0x563F88`); `0x53E1F0(EBX=50)` (`0x563F95`); emotion bytes `rec+0x12D..0x131` each `+= trunc(50 + r·100)` (cap 200) (`0x563F9A`..`0x5641BF`; 50.0 `0x905048`, 100.0 `0x904E30`).
- m1: until `D+4 & 1`, `D+0x50 += 0x400` per frame (spin); then motion 0x9F, timer 180..301 (`180 − r·−121`), icon 5 150 → m3. m2: same without spin. m3: timer → end.

**Thrown / dropped (landing) `0x564320`.** m0 (`0x5644AE`): face along velocity, eyes 1, icon 5 150, motion 0x10F (blend 15), AddHappiness(EDI=−5, f=−0.0667 `0x1312D44`) (`0x564500`..`0x56451A`), `0x53E1F0(10)` (`0x564527`), emotion bytes `0x12D..0x131 += trunc(50 + r·100)` → m2. m2: on `W+0x40 & 0x800` drop the hat; when `D+4 & 1`: motion 0x9F, timer 180..301, icon 5 → m3. m3: timer → end. Which release path sets `0x564320`, `0x565BE0` (put down) or `0x53D720` is in `chao-garden-sync.md` §3.

**Held.** `0x566720` → `0x566670` → `0x5660A0` (like > 50) / `0x5662B0` / `0x566300` (like < −50) / `0x566580` (SLPDPTH > 500), release via `0x565DC0`: see `chao-garden-sync.md` §3; effects in items §1.2.

**Lonely / attachment chooser `0x5A04A0`** (adults only, §6.6 note; no best gate). Requires player 1 perceived (list 0 `D+0x7E4`, idx `D+0x7F4`, then `*0x1DEA6E0` non-null, `0x5A04A8`..`0x5A04D2`). `c` = `0x53E0C0()`, `like = bond[c]+0` (signed byte), `B2 = bond[c]+2` (u16), `B4 = bond[c]+4` (u16), bond = `rec+0x16C + c·6`. Scores are 0 (`0x8AF004` = 0.0) when LONELY ≤ the threshold.
- Gate A (`D+0x1DC == 0`, `0x5A04F2`):
  - `like ≥ 0` and (`B4 < 2` or `B2 > 300`): s = jitter(f16(LONELY, 1000 `0x1312E74`)·0.99 `0x1312E78`) → `0x5A03B0` + follow-up `0x5A0490` (`0x5A058C`..`0x5A05A4`).
  - `like < −20` and `B2 > 300`: same score → `0x5A1800` + follow-up `0x5A0490` (`0x5A0640`..`0x5A0658`).
- Gate B (`D+0x1D4 == 0`, `0x5A067B`):
  - `like > 60` and `B2 > 100`: s = jitter(f16(LONELY, 5000 `0x1312E7C`)·0.99 `0x1312E80`) → `0x5A0960` (no follow-up) (`0x5A06FC`).
  - `like < −60`, dark adult (`0x535390`), `B2 > 100`, `rec+0x14C > 30` (signed): same score; if s > best: anger byte `rec+0x12D += 200` (cap 200) and **only a follow-up** `0x5A18A0` is pushed onto the currently installed stack (`0x53D970`, no `AL_SetBehavior`), best = s (`0x5A07BC`..`0x5A07F7`).
- `0x5A03B0` (not an approach; it stands still): motion 0x18C + sound 0x6091 if alignment `rec+0xB0 > 0.5f` (strictly), else 0x18D (blend 20); eyes, mouth 3; ends after 201 frames. Follow-up `0x5A0490` = `0x53E280` (bond `+4 ++` capped 0xFFFF, `+2 = 2·⌊v/3⌋`, items §1.1) then end.
- `0x5A0960` run to liked player: m0 motion 0x75 (blend 10); if `rec+0x12C > 180` → LONELY −6000 and end (checked once, at start, `0x5A09D9`..`0x5A0A23`). Then each frame: after 1000 frames → end (`0x5A0A2A`); when squared distance to player 1 < 25.0 (double `0x904F70`, 5 units, `0x5A0A6C`): `rec+0x12C += 10` (cap 200) → `AL_SetBehavior(0x5A0820)` + follow-up `0x5A0960` (`0x5A0A79`..`0x5A0AB9`).
- `0x5A0820` near player: motion 0x190, eyes 3, mouth 3; at each motion end, if still within 5 units: `rec+0x12C += 10`, restart itself; if then `rec+0x12C > 140`: LONELY −6000 and end (the end pops the freshly set stack, so it settles via `0x565BE0`); if farther than 5 → `0x5A0960`.
- `0x5A1800` dislike: motion 0x1BF (blend 20), eyes 1, mouth 4, sound 0x6069, ends at motion end.

**Dark-adult mischief `0x5A1240`** (adults only; no best gate). Gates (`0x5A1253`..`0x5A129C`): `rec+0x14B ≤ 30` (signed), type `rec+0x80` ∈ {7,10,13,16,19,22}, anger `rec+0x12D > 80` (`0x1312DF8`).
- Player perceived (`0x590CE0`) and `like < 0`:
  - if `rec+0x14C > 30` and r < 0.7 (`0x9050C0`): s = jitter(f8(anger, 80)·0.99 `0x1312E04`) → `0x5A1460` + follow-up `0x5A18A0` (`0x5A132C`..`0x5A1344`);
  - else if `rec+0x14C > −50`: same score → `0x5A1800` (`0x5A13B1`).
  - In both branches, if s ≤ best the chooser returns without trying anything else (`0x5A1326`, `0x5A13AB`).
- Otherwise (not perceived, like ≥ 0, or `rec+0x14C ≤ −50`): s = 1 − (100 − p14C)·0.005 (`0x9055A8`)·(1 − 0.7 `0x1312E00`) (no jitter, `0x5A13E6`..`0x5A1414`) → `0x5A1460` (no follow-up). This gives 0.7..1.0, so such a Chao almost always wins the picker.
- `0x5A1460`: motion 0x203, laugh voices 0x6065/0x6066/0x6068, timer. `0x5A18A0`: motion 0x7D; ends if anger < 10 or after 1000 frames; approaches the player; within 5 units: anger −10, then r<0.35 → `0x5A1600` (motion 0x1B9), <0.7 → `0x5A1780` (0x1BD), <0.9 → `0x5A1680` (0x1C3), else `0x5A1700` (0x1BB), each with follow-up `0x5A18A0` (loop); 60% a voice 0x606F..0x6072 (`0x5A19D4`..`0x5A1B1F`).

### 6.3 Fruit

**Hunger chooser `0x59EB00`** (called from picker `0x562ADE`; EBX = &best; no best gate):
- Branch B (only when `D+0x20C == 0` **and** r < 0.6 `0x905268`, `0x59EB09`..`0x59EB3E`): s = jitter(f16(HUNGER, 5500 `0x1312E44`)·0.85 `0x1312E48`); if s > best → `0x59ED60` hungry. Then, independently, if HUNGER > 9900 (`0x59EBA1`): s = jitter(1.0); if s > best → `0x59EF40` starving. Branch B never evaluates fruit.
- Branch A (otherwise): nearest perceived fruit (list 2, `D+0xF14`, idx `D+0xF24`, entries `D+0xF28 + i·0x1C`). If the fruit's ALW partner is set and is not this Chao → nothing (`0x59EC3D`). If the fruit is held by the player (`fruit.D+4 & 0x8000`): s = jitter(f16(HUNGER, 5000 `0x1312E54`)·0.99 `0x1312E58`) → `0x59E8C0` + follow-up `0x59EA00`. Else same score → `0x5614D0`.
- If no fruit is perceived but a tree is (list 3, `D+0x12AC`), a score is computed and discarded (`0x59ED01`..`0x59ED4A`): Chao never go to trees for fruit, nor shake them, in this exe.
- Hungry/starving end by setting the gate `D+0x20C = trunc(1200 + 10800·0.5·((10000−HUNGER)/10000 + r))` (rand `0x59EE99`, store `0x59EF18`; constants `0x1312E3C` = 1200, `0x1312E40` = 10800, `0x904E98` = 0.5, `0x904F50` = 10000.0; FPU control word `| 0xC00` = truncation, not rounding).

| Behaviour | Does |
|---|---|
| `0x59ED60` hungry | eyes 3; posture 1: motion 0x5E, 180 frames, then motion end (`&1`); else motion 0xA3, motion end; then 61 frames → set gate, end |
| `0x59EF40` starving | mouth 6, motion 0x1DC (blend 20), 240..440 frames (240+trunc(201r), `0x905A00`); tear particles every frame with `D+0x1C0 % 8 < 4`; when `D+0x30 % 60 == 0`, 50% a sound; then set gate, end |
| `0x59E8C0` beg | motion 0x76, eyes, mouth 50% 3/2; turn to player 0x120/frame, walk 0.055 (`0x1312C68`) when on ground; ends within 10 units (squared < 100.0) or after 1000 frames |
| `0x59EA00` | motion 0x18E (blend 25), wait 180..301, end |
| `0x5614D0` go eat | sets ALW target to the fruit; `0x55C3C0`, then `0x56B480`, `0x56BA80`, `0x5613C0`, `0x5607C0` |
| `0x5613C0` reach | motion 0xBC + sound 0x1001; at motion end, if the fruit isn't partnered elsewhere, link it into the hands (`0x56CFB0`, `0x56CFF0`); ends on that same end-pulse frame whether or not it took the fruit (mode 2 unreachable); gives up with return 3 (reset) after 1800 frames |
| `0x5606D0` take offered fruit | from the 0x83 touch: motion 0xCA (blend 15); turn 0x600/frame; when within 0xB6 BAMS and still touching a fruit without ALW flag 2: player drops (`0x560752`), link (`0x560759` → `0x56CFB0`), `0x5607C0`, open balloon `0x5659E0`. The 60-frame countdown runs only on frames where it faces the fruit and touches nothing; at 0, sound 0x6037, end |

**Eat `0x5607C0`** (jump table `0x561374`, 14 modes; reads fruit kind `fruit.D+8`, fruit-kind table `0x5613AC`):
- m0 motion 0xA9; chew budget `D+0x1C4 = 100 + r·300` (`0x905360`).
- m1 (motion end): partner fruit = `0x5306F0(obj,3)`. If `rec+0x150 < 0` and `rec+0x14A < 0` and HUNGER ≤ 5000 → m4; else 50% motion 0xB4, wait 120, then motion 0xB6 + sound 0x6039, wait 60 (m2, m3) → m4; else m4.
- Taste (same frame): kind 5 (hero) disliked by dark adults (`0x535390`: type ∈ {7,…,22 step 3}); kind 6 (dark) by hero adults (`0x535360`: type ∈ {6,…,21 step 3}); kind 7 if `rec+0x157` ∈ {2,4}; kind 8 if ∈ {0,5}; kind 9 if ∈ {1,3}. Disliked → mode 9, AddHappiness(−1, ∓0.1), sound 0x603A (`0x560946`, `0x560993`, `0x5609DD`).
- m4 picks style `D+0x41C` once (later visits reuse it): `rec+0x150 > 50` or HUNGER > 8000 → 1 (motion 0xB3, sound 0x603C) else 2 (0xAF, 0x603B); → m6.
- m6 every frame: mailbox 2 ("bite") to the fruit (`0x560BC7`). If own mailbox == 4 (fruit finished): motion 0xA5 (blend 60), 180..380 frames, unlink, sound 0x6042 → m11. Else chew: while `D+0x1C4 > 0`, if the once-latch `rec+0x12A` is 0 (`0x536420` tests it and sets it to 1, `0x560E3F`): **HUNGER −2400 and TIRE −2400** (u16 `0x1312DA6`, clamped 0..10000; `0x560E52`..`0x560EA9`); then HUNGER == 0 → m9 (stop) (`0x560EB3`). When the budget runs out: gluttons (`rec+0x150 > 50`) 40% motion 0xB7 + sound 0x603D (m5: 2..6 motion ends, then 0xAD), else 0xB0/0xB1 pauses (m7: r<0.65 → 0xAD, else swap 0xB0↔0xB1 blend 25 with a new pause); m8 then goes back to m4 with budget 180..579. The fruit bites on the 182nd m6 frame (fruit `D+6`).
- m9/m10: motion 0xBA, then drop/throw the fruit away (`0x5306D0`, velocity from facing), close balloon `0x565B90`, unlink.
- Fruit taken away (own mailbox 6): AddHappiness(−1, ∓0.1), anger `rec+0x12D += 200` (cap 200), end (`0x5611BB`..`0x56126A`). Quirk: in m6 the bite loop reads and discards mailbox 6, so a fruit taken while chewing ends the meal with no penalty.
- Per-bite stats/lifespan/evolution, given-from-hand happiness, heart fruit BREED +10000, alignment of hero/dark fruit: fruit side, items §1.2.

### 6.4 Toys

**Toy chooser `0x599B60`** (requires toy cooldown `D+0x1F4 == 0`, `0x599B6A`; nearest perceived toy `0x590D20`; no best gate): s = jitter(f16(TEDIOUS, 2000 `0x1312E8C`)·0.99 `0x1312E90`). If s > best, by toy ALW kind (`entry+4`, jump table `0x599C90` for kinds 2..8): 2 TV → `0x599210`; 3 radio → `0x599400`; 4/5/6 → `0x40AD80` (`mov eax,1; ret`: returns 1 immediately); 7 horse → `0x5995F0` unless `0x535010(&horse.pos) == 3` (then the chooser returns with no change, `0x599C24`); 8 box → `0x599770`. **Any** other kind (outside 2..8) skips `AL_SetBehavior` but still does TEDIOUS −5000 and sets best = s (`0x599BD6` → `0x599C40`..`0x599C77`). Kind 4 is the ball (`chao-garden-sync.md` toy table), so choosing the ball costs 5000 boredom for a no-op.

**Toy already playing `0x599A10` (TV, signal 0xC9) / `0x5998C0` (radio, 0xCA)**: only if best < 1.0 and the signal is present; T = TEDIOUS, `T += 3000` if T < 7000; if TEDIOUS > 2000: s = jitter(f16(T, 2000)·0.99). If s > best: if the TV/radio exists (`*0x1AED288` / `*0x1AED2E0`) and `0x55C4A0` / `0x57C7A0` gives a viewing spot: target = spot, ALW target, `0x55C3C0` + `0x56B480`, `0x56B560`, `0x56B6C0`, `0x598890`/`0x598B90`. Best becomes 0.99 (`0x109E008`) even if no spot was found.

**Viewing spot `0x55C4A0` (TV) / `0x57C7A0` (radio):** distance 10 + 3r from the toy, at angle `trunc((80r − 40)·−182.044)` off the toy's rotY + 0x8000 (`0x904F48`, `0x904DD8`, `0x905088`, `0x9053B8`, `0x9058A0`), so within ±40° of that direction.

| Behaviour | Does |
|---|---|
| `0x599210` / `0x599400` go to TV / radio (off) | spot from `0x55C4A0` / `0x57C7A0` (fails → end); `0x55C3C0`, `0x56B480`, `0x56BA80`, `0x56BBA0` (presses: 0xAF at `0x56BC77`), `0x56B560`, `0x56B6C0`, then `0x598890` / `0x598B90` |
| `0x598890` watch TV / `0x598B90` radio | pose r<0.3 → 0x3E, <0.7 → 0x3F, else 0x1CB; if animal bit 9 (`rec+0x118 & 0x200`) 30%: 0x3A/0x3C; new pose every 300..600 frames; while the 0xC9/0xCA signal lasts. When it stops: motion 0x23D, 120..210 frames, toy cooldown `D+0x1F4 = 1800..3600`, end |
| `0x5995F0` go to horse | horse not partnered elsewhere; target = horse; link (`0x530640`); `0x55C3C0`, `0x56B480`, `0x56B560`, `0x56B500`, `0x598F40` |
| `0x598F40` ride | m0 jump (forward 0.08, up 0.65 `0xC71F38`); m1 when falling: blend 30, `D+0xA0 &= ~2`, 29 frames; m2: emit 0xAF (3.5, 10 frames) → horse starts; 1800 frames; m3: cooldown 1800..3600, `D+0xA0 |= 2`, `AL_SetBehavior(0x598E90)` then return 1, which pops it, so the Chao settles in `0x565BE0` and the dismount never runs. While linked: facing approaches the horse's in m1–m3, position lerps 10%/frame to the horse in m2–m3. Unlinked → `0x565BE0` |
| `0x598E90` dismount | jump sideways 0.3 / up 0.8 `0xDF21B4`, rewritten every frame (started in the air it rises forever); ends on landing (`W+0x40 & 0x400`). Not reached from the ride |
| `0x599770` go to box | `0x55C3C0`, `0x56B480`, `0x56BA80`, `0x56BBA0` (press 0xAF), `0x565BE0`; no cooldown |

**Ball.** Reaction `0x563B70`: ball moving (`*0x1DBE574 != 0`: ball mode 1, speed > 0.4), TEDIOUS > 4000, distance to the ball (`0x77FBD0`) < 50.0 (`0x905048`): TEDIOUS −2000 → `AL_SetBehavior(0x563830)`, return 1. It does **not** exclude an ongoing chase, so it re-fires (−2000 each tick, chase restarted at m0) every tick until TEDIOUS ≤ 4000 or the ball stops/leaves 50 units (pseudocode `0x563B70`).
Chase `0x563830`: m0 motion r<0.2 0x76, <0.6 0x77, else 0x6A (blend 10); mouth r<0.4 → 1 else 3; icon for 120 frames (`0x53D660(0x78)`). Each frame (m0/m1) target = ball pos with x,z ×1.02; if the ball is held (`ball.D+4 & 0x8000`) → end. Then: frame counter `D+0x1C0`; every 100 frames r < 0.15 → `0x562D50`; while `*0x1DBE56C == 0`: turn 0x180, on ground speed = facing × (0.028 `0x1312D74` + 7e−6 `0x1312D78`·run) (run = `rec+0x3C`, ×3 outside areas 6/10/11); if latch `rec+0x12A == 0` (set to 1 here): TIRE +300 (`0x1312DC2`, cap 10000), TEDIOUS −100 (`0x1312DB0`); ends after 3000 frames, or when TEDIOUS ≤ 999 or TIRE ≥ 7001 (continues only while TEDIOUS > 999 and TIRE < 7001). When `*0x1DBE56C != 0` (ball in water): `0x53D660(0xF0)` (icon 240 frames) and end. On every end it clears its ALW flags `+6`, link id `+0xC = −1`, partner `+0x40` (pseudocode `0x563830`).

**Self-play chooser `0x5A10B0`** (best < 1.0; score 0 unless TEDIOUS > 2000): s = jitter(f16(TEDIOUS, 2000 `0x1312E8C`)·0.99 `0x1312E90`). If s > best: r < 0.4 → second r: < 0.25 skill A `0x59D410`, < 0.5 (`0x165DFD4`) skill B `0x59C6D0`, < 0.75 skill C `0x59B900`, else sing `0x5AB5D0`; r ≥ 0.4 → second r ≥ 0.5 animal play `0x5A2F10` else toy skill `0x55E7A0`. If the sub-chooser returned non-zero: TEDIOUS −5000, best = 0.99 (`0x109E008`), `D+0x1CC/0x1CE` cleared; otherwise best is unchanged.
- `0x55E7A0`: uniformly picks a set bit i of `rec+0x164` whose table entry `0x12FE978[i]` is non-null (index `trunc(r·(n − 1e-4))`, `0x904FF8`): 0 `0x55DDE0`, 1 `0x55E3E0` (drum), 2 `0x55E210` (book), 4 `0x55E600`, 5 `0x55DD10`, 6 `0x55DEB0`, 7 `0x55E6D0`, 9 `0x55DB10` (bubbles); 3, 8, 10, 11 are null. Bits 10 (shovel) and 11 (watering can) are used by §6.5.
- Sing `0x5AB5D0`: returns 0 with ≥ 100 song objects (`*0x1AEDC00`); song id from `rec+0x160` bits or, with `rec+0x164 & 0x100`, from the bonds (§2A.5; `0x5AB4B0` caps every bond like at 50 as a side effect); `0x5AB2C0` attaches props to nodes 4/11, motions 0x143/0x145, emits 0xA9, 1800 frames, sound 0x1026.

### 6.5 Trees and seeds

No Chao behaviour shakes or climbs trees (`0x59EB00` tree branch is dead, §6.3; no reaction uses 0x96).

**Seed pick-up** (no skill bit needed):
- Reaction `0x567560`: current not `0x54EF10`/`0x5607C0`/`0x567640` (`0x567570`, `0x567578`, `0x5675CE`); run points `rec+0x3C` ≥ 150 (u16 `0x1312D6E`) in areas 6/10/11 (`*0x134046C`), else `rec+0x3C·3 ≥ 150` (`0x56758E`..`0x5675B9`); the Chao holds nothing (skip if its ALW partner's partner is itself, `0x5675BB`..`0x5675CC`); touch 0x85 (`0x5675DC`) whose object is not targeted (`0x5304D0` returns 0, `0x5675F1`) → W target = seed pos, `AL_SetBehavior(0x567640)`.
- `0x567640`: motion 0xCA (blend 15), 60-frame window; turn 0x600; within 0xB6 BAMS and touching a seed without ALW flag 2: player drops (`0x5676C2`), link → `0x567710`; else end after 60 frames.
- `0x567710`: motion 0xF, hand flag `D+0xA0 = (|0x400) & ~0x800`, 300..421 frames → `0x567830`.
- `0x567830`: both-hands flag `|= 0xC00`; wander timer `D+0x1C8 = 1800..2201`; m0 pick a destination (`0x534F80`); m1 look for an empty tree spot (signal 0xAC) within `intelligence·0.001 + 20` units (`rec+0x44`, ×3 outside 6/10/11) → ALW target, sound, m2; otherwise walk (turn 0x100), at the destination (squared < 64) 70% → back to `0x567710`; m2 turn 0x280 to the target, within 2.5 (`0x165E030`) → `0x567B10`. Speed 0.055·0.8 on ground; ends (return 1, still holding) when `D+0x1C8` runs out.

**Planting `0x567B10`** (needs `rec+0x164` bit 10, tested with `0x53E360(ECX=10)` at `0x567C13`; `0x53E360` = `(1<<cl) & rec+0x164`):
- m0 motion 0, icon 1 180, 120..241 frames facing the spot; m1 without bit 10: icon 5, end. With it: motion 0xE5, shovel model `0x12FDE44` on node 11 (`0x566A20`), 180..301 frames; m2 motion 0xE7; m3 digging, sound 0x1023 every 35 frames; at motion end (`D+0xAC & 1`, `0x567D72`): held seed = `0x5306F0(obj,7)`; tree spot = `0x54B1F0(0xAC)`; the spot's facing `D+0xC` := the seed's; spot ALW mailbox `+0x34 = 5`, `+0x36 = seed kind` (seed `D+8`) (`0x567DDA`..`0x567DE8`; the tree plants it, `chao-garden-sync.md` Shared trees); unlink `0x530690`, `0x5304F0(spot)`; the seed's save record freed (`0x52F680`, `0x567E1B`) and the seed deleted (task MainSub `+0x10 = 0x46F720`, `0x567E34`); motion 0xE8; later modes play sound 0x1024 every 35 frames.

**Watering** (needs `rec+0x164` bit 11 only for the can animation):
- Chooser `0x5625B0` (adults only; best < 1.0): signal 0xAE (tree, r 20) present (`0x486880(obj,1,0xAE)`, `0x5625D5`..`0x562600`); v = `rec+0x148`; if v < 5000 (`0x1312EA4`): s = jitter(0.891 `0x1312EA8`·(5000 − v)/5000); else s = jitter(0.891·0.0 `0x8AF004`) = 0. If s > best → `0x567FA0`, Chao ALW `+0x44` = the tree's ALW entry (`0x56269B`), best = s.
- `0x567FA0`: motion 0x64, icon 1 300 frames, timeout 1800..1921 frames; turn 0x280 to the tree, within 4.0 (`0x104DD30`) → `0x5680F0`.
- `0x5680F0`: m0 60..121 frames; m1: icon, sound 0x601A (`0x5681E9`); bit 11 test `0x53E360(ECX=11)` (`0x56823E`): without it → `rec+0x148 += 2000` (cap 10000, `0x568243`), end. With it: m2 150 frames; m3 turn to target, watering-can model `0x12FCA1C` (`0x566A20`), 240..341 frames; m5 at motion end `rec+0x148 += 2000` → `0x568460`, which emits 0xAD (radius 5.0, 10 frames, 4 units ahead) at motion flag 8 and ends. The tree side of 0xAD is in `chao-garden-sync.md`.

### 6.6 Other Chao

Picker order (`0x562A80`): `0x590C70`, `0x599CB0`, `0x54E970`, `0x54EB20`, `0x54EBA0`, `0x59EB00`, `0x562520`, `0x59D0D0`, `0x59C3D0`, `0x59B710`, `0x59B820`, `0x599A10`, `0x5998C0`, `0x599B60`, `0x5A10B0`, `0x5626D0`, `0x59F210`, `0x59F860`, `0x5A0010`, `0x5A0B10`, `0x562800`; **children (`rec+0x80 == 2`) stop there** (`0x562B6A`); others then run `0x5A1240` (mischief), `0x5625B0` (watering), `0x5A04A0` (lonely), `0x562950` (courter), `0x562880` (breed caller), `0x562A20` (death: best < 1.0, `rec+0x8A == 0`, standing still `0x54AC20` → `0x568F60`, best = 1.0). Best starts at 0.2 (`0x1312DF4`); if still ≤ 0.2 → wander `0x562D20` (`0x562BB9`..`0x562BD5`).

**Watching or joining a performer.** All need best < 1.0, the signal nearby and TEDIOUS > T (else score 0); T' = TEDIOUS + 3000 if < 7000; s = jitter(f16(T', T)·0.99 `0x1312E90`); on success best = 0.99 (`0x109E008`).
- `0x59D0D0` (0xA5, T = `0x1312E8C`>>1 = 1000): if the Chao knows skill A bits (`rec+0x163`, bits 0..7): uniformly random known bit → W target = signal pos (S Data1+0x14), `AL_SetBehavior(0x56B500)` (face target, §2A.1) then push `0x11C9BB4[bit]`; own TEDIOUS −4000, LONELY −4000, performer's (signal owner `+8`) LONELY −4000. Else `0x56B500` + `0x55C430` + `0x59E710` (watch), target = performer pos (pseudocode `0x59D0D0`).
- `0x59C3D0` (0xA6, T = 2000): if the performer's current behaviour is `0x59C980`/`0x59CA90`/`0x59CB20`/`0x59CBF0` and the Chao has the matching bit in `rec+0x161` (2/4/8/0x10): target = signal pos, `0x56B500` + join the same behaviour; TEDIOUS −4000, LONELY −4000, performer's LONELY −3000. Performer in `0x59C870` or any other behaviour → target = performer pos, `0x56B500` + `0x55C430` + 50% `0x59CDA0` / `0x59CF50` (pseudocode `0x59C3D0`).
- `0x59B710` (0xA7, T = 2000): target = performer pos, `0x56B500` + `0x55C430` + `0x59B3D0`.
- Joiners aim at the signal position; watchers at the performer's position.
- `0x59B820` (0xA8): TEDIOUS > 2000 and r < 0.7 → s = 0.99 → `0x59B580`.
- Watchers `0x59E710`/`0x59CDA0`/`0x59B3D0`: pose 50% 0x181 / 0x3F (`0x59CF50`: 0x17F), mouth 3, new pose every 300..600 frames while the signal lasts; then motion 0x23D (`0x59CF50`: 0x23E), 180..300 frames, end.
- `0x59B580`: motion 0x232 (sitting postures) else 0x3F, mouth 50% 8/4, 300..600 frames, then 50%: anger `rec+0x12D += 150` (cap 200).

**Mating** (adults, not types 20–25):
- Caller chooser `0x562880` (best < 1.0; type ≠ 2 and not 0x14..0x19): no 0xAB signal nearby (`0x54B1F0(0xAB)`) and BREED ≥ 10000 (> 9999): if fewer than 8 Chao in the garden (`0x5319F0`) and standing still (`0x54AC20`) → `0x55C430` + follow-up `0x59BB50`, best = 1.0; with 8 or more, BREED = 0 (pseudocode `0x562880`).
- `0x59BB50` caller: m0 `0x5AAF60` (heart object), emit 0xAB (30.0, 8880 frames) into `D+0x40` (`0x59BBAA`); m1 motion 0x3E, icon 4 for 8700 frames; m2 wait up to 9000 frames (BREED = 0 at 4500); when its mailbox reads 1 (partner arrived): face partner, motion 0x30, delete the signal; m3 after 30 frames turn 0x400 to the partner; then motion 0xFC (dance), later 0x1D4 / 0x1EC with the partner (`0x59BD84`, `0x59BE4E`).
- Courter chooser `0x562950` (no best gate; type ≠ 2, not 0x14..0x19): 0xAB nearby (`0x486880(obj,1,0xAB)`, `0x562996`) and BREED > 5000 (`0x1312E9C`) → s = jitter(0.99 `0x1312EA0`·0.88); if s > best → `0x59BF30`, best = s.
- `0x59BF30` courter: finds the caller (`0x54B1F0(0xAB)`), motion 0x78, icon 400 frames, walks to it; on touch 0x81: links both (`0x530620`/`0x530640`), mailbox 1 to the caller, dance jingle (`0x443480`, `0x59C10F`), motion 0xFC; after 300 frames mailbox 1 again, motion 0x1D4, own BREED = 0; 20 frames later the egg: partner = `0x5306F0(obj,0)`, position = own pos + 2·(facing vector) (`0x59C291`..`0x59C2B3`), genes `0x551D20(partner, self)` (`0x59C2B7`), free slot `0x531AA0` (`0x59C2BF`), egg spawn `0x57B9C0` (`0x59C2D7`); motion 0x1EC, wait, end.

**0xA2 reaction `0x564760`** (touch kind 0xA2, not while already in `0x5647B0`) → `AL_SetBehavior(0x5647B0, 500)`. `0x5647B0`: motion 0x238, eyes 1, mouth 8, icon 5 300 frames, sound 50% 0x6029 / 0x606D; velocity ×0.9 per frame on ground; never returns non-zero, so it ends only by the 500-frame limit.

### 6.7 Animals, drives and hats

- Animal/drive reaction `0x566BC0` (touch 0x87, item not held by a Chao) → `0x566C40`: motion 0xCA (blend 15), turn 0x600 until within 0x71C BAMS, then if touching: player drops (`0x566CB7`), link → `0x566DF0` (absorb, items §1.3); not touching, or the item is Chao-held → end at once (no 60-frame window).
- Hat reaction `0x5691B0` (touch 0x93; not while eating/sleeping/already `0x569240`) → `0x569240`: same take pattern (motion 0xCA, 60-frame window, 0xB6 BAMS; skip if hat ALW flag 2, `0x5692B0`; player drops `0x5692C2`; link `0x56CFB0`) → `AL_SetBehavior(0x569340)`: motion 0xBE (blend 35), icon 3 120 frames, sound 0x601C, 120..180 frames; follow-up chosen at take time (`0x5692E5`..`0x569315`): if `rec+0x118 & 0x10000` and no hat worn (`rec+0xD5 == 0`) → `0x569430` (motions 0xC7/0xC8, wears it, frees the record `0x5694C5`; ends once the hat is gone); else `0x569550` (motion 0xC9, throws it: forward 0.3, up 0.7, unlink).

### Notes
- `0x5A03B0` is a stationary call to the player (motion 0x18C/0x18D, 201 frames), not an approach; follow-up `0x5A0490` updates bond counters (`0x53E280`).
- `0x5634D0` only turns toward the player; walking to the player is `0x563250` (stops within 8 units).
- `0x567640`..`0x567B10` (the "0x85 chain") is seed pick-up → wander → planting; 0x85 is the seed collision kind (`0x8A7458`). Pick-up needs no skill bit; only planting needs `rec+0x164` bit 10.
- `0x5680F0` is tree watering (`rec+0x148 +2000`, emits 0xAD), reached from `0x5625B0` via `0x567FA0`.
- Further mischief behaviours: `0x5A1600`, `0x5A1680`, `0x5A1780` (picked by `0x5A18A0`). Extra hat behaviours: `0x569340`, `0x569430`, `0x569550`. Extra choosers: `0x599A10`, `0x5998C0`, `0x5A10B0`.
- The `0xA2` push at `0x53530E` is an allocator line number, not a signal.

- Chooser gate is `best < 1.0` (strict) (`0x5625CB` pattern); courter `0x562950` has no gate.
- Hunger gate `D+0x20C` is truncated, not rounded (`0x59EEF8`..`0x59EF11`); in branch B the starving check runs even if hungry won, and fruit is never considered in that tick.
- Seed pick-up (`0x567560`) does not test skill bit 10.
- Ball reaction re-fires every tick (TEDIOUS −2000 each) and restarts the chase; ball chase ends while TEDIOUS ≤ 999 or TIRE ≥ 7001.
- Whistle approach adds eyes 4; whistle (0x6019) and pet (0x6074/0x6075) voices only play when `*0x134046C == *0x134062C`.
- Mischief: losing branch scores do not fall through to the default; default branch has no follow-up.


## Behaviour table

Every behaviour function the picker, interrupts and follow-ups install (the section column says where it is described).

| Address | Name | Trigger | Changes | § |
|---|---|---|---|---|
| `0x40AD80` | no-op toy | 0x599B60 toy kinds 4 (ball), 5, 6 | returns 1 at once; chooser still took TEDIOUS -5000 (kinds outside 2..8 also cost -5000 with no behaviour) | 6 |
| `0x53D720` | released (flag 0) | 0x565DC0 release when *0x174AFFE==0 (after y+=2, vel=(dir*0.2, 0.87)) | motion 0, collision bit 8 cleared; never ends by itself | 2B |
| `0x54EE10` | go to bed / sleeping (0x54EF10) | SLEEPY>8000 and Chao Y >= water surface W+0xDC (0x54EBA0) | 0x54EC70 once per need period (latch): phase0 SLPDPTH+1500 until >=10000; phase1 SLEEPY-1500, TIRE-trunc(300*(300+p151)/300) until SLEEPY==0; phase2 SLEEPY-1500, SLPDPTH-3500; wake when SLEEPY<1000 and SLPDPTH<2000 (0x54EC40) | 3 |
| `0x54EF10` | sleeping | 0x54EE10, spawn 0x531DB4 | sleep poses 0x12/0x13/0x1B/0x1C, snores 0x6008-0x600B, 0x600F/0x6011; sleep tick per latch (SLPDPTH+1500 to 10000; SLEEPY-1500 and TIRE-(300+rec+0x151) to SLEEPY 0; SLEEPY-1500 and SLPDPTH-3500); wakes when SLEEPY<1000 and SLPDPTH<2000; returns 1 from mode 16 | 2A |
| `0x54F7A0` | sit-and-nod sleep | 0x54EF10 mode 0, posture 1, TIRE>7000, 50% | motion 0x14/0x16/0x17, eyes 7, sleep tick; wake -> 0x3E, 1 | 2A |
| `0x54F980` | drowsy | picker 0x54E970: 6000<SLEEPY<9000 and D+0x210==0; score 0.99*(S-6000)/2000 below 8000, 0.99*(1-(S-8000)/1000) from 8000 to 9000, jittered | no need writes found; D+0x210 = round(600+2400*0.5*((10000-S)/10000+r)) rewritten on every picker call | 3 |
| `0x54FCF0` | deep sleep | SLPDPTH>500 (0x54EB20, score 0.5+0.5*f16); from petting 0x56496A | SLPDPTH -2/frame, returns 1 at 0 (0x54FDDD/0x54FDFE); ball state 5 for 360 frames (0x54FD8A), re-queued every 402 frames (D+0x1C0>400) | 3 |
| `0x55C3C0` | stand-up prelude A | SetBehavior by 0x5A3650 (run>=150) and toy/food choosers | posture D+0xAE 1->motion 0x30, 2->0x26 (blend 0x28), 3->0x26 (blend 0x23), 0/other -> pop at once; pop on D+0xAC&1 | 5 |
| `0x55C430` | stand-up prelude B | SetBehavior by 0x5A3650 (run<150), tired/breed choosers | posture 0->motion 0x2F, 2->0x2A (blend 0x28), 3->0x2A (blend 0x23), 1/other -> pop at once; pop on D+0xAC&1 | 5 |
| `0x55DB10` | toy skill 9 bubbles | 0x55E7A0 | motion 0x146, wand models nodes 4/11; bubble 0x55DA10 every 10f (40%) after 60f, speed 0.2..0.3; 25% end per motion&8 | 2B |
| `0x55DD10` | toy skill 5 | 0x55E7A0 | motion 0x14E, model 0x12FBC5C node 11, 600..800f | 2B |
| `0x55DDE0` | toy skill 0 | 0x55E7A0 | motion 0x14A, eyes 4, model 0x12FA174 node 11, 600..800f | 2B |
| `0x55DEB0` | toy skill 6 | 0x55E7A0 | motion 0x14B, model 0x12F6D54 node 11, 600..800f | 2B |
| `0x55E210` | toy skill 2 book | 0x55E7A0 | book 0x55E180, motions 0x151/0x153/0x152, eyes 4 at 50%, 30% close (book mode 4) per cycle | 2B |
| `0x55E3E0` | toy skill 1 drum | 0x55E7A0 | motions 0x14F/0x150 loop, eyes 4 at 50%, drum D+0x660=0x12FD9DC; first cycle 300..400f, later 380..560f; 20% end per cycle | 2B |
| `0x55E600` | toy skill 4 | 0x55E7A0 | motion 0x148/0x149, model 0x12F8364 node 4, 20% end per motion end | 2B |
| `0x55E6D0` | toy skill 7 | 0x55E7A0 | motion 0x14C, model 0x12FB41C node 11, 600..900f | 2B |
| `0x55E7A0` | learned play sub-choosers | picker 0x5A10B0: r1>=0.4: r2<0.5 toy else animal; r1<0.4: r2 <0.25/<0.5/<0.75/else | TEDIOUS -5000 and best 0.99 if the sub-chooser installs | 1 |
| `0x5606D0` | take offered fruit | reaction 0x560640 (touch 0x83, fruit not held by a Chao) | motion 0xCA; turn 0x600; player 1 drops held item (0x46E5E0); links fruit; -> 0x5607C0; balloon 0x5659E0; else 60 frames + sound 0x6037 | 6 |
| `0x5607C0` | eat | go-eat chain, 0x5606D0 | bite loop mailbox 2/4 (mailbox 6 read and discarded in mode 6: fruit taken while chewing = no penalty); every mode-6 frame per rec+0x12A latch HUNGER-2400 and rec+0x140-2400, HUNGER 0 -> mode 9; dislike rules by fruit kind/rec+0x157 -> happiness -1 (+0x536260 0.1), sound 0x603A, throw fruit; fruit lost (msg 6) -> happiness -1, rec+0x12D/0x12E=200; returns 1 once not holding (post-meal modes 11-13 unreachable) | 2A |
| `0x5613C0` | pick up fruit | follow-up of 0x5614D0 | motion 0xBC, sound 0x1001; links fruit into hands at motion end and returns 1 on that frame either way (mode 2 unreachable); returns 3 after 1800 frames | 6 |
| `0x5614D0` | go eat | picker 0x59EB00 branch A | targets nearest fruit (entry+0x44); Set(0x55C3C0)+0x56B480+0x56BA80+0x5613C0+0x5607C0; busy-waits (returns fruit entry ptr) while another Chao is linked; no fruit -> 1 | 2A |
| `0x561E00` | swim (swim<300, struggling) | 0x562330 | random eyes/mouth (6/8/0xB/4, or 9 if D+0x69E==1 at 25%); sound every 120f at 20%; once-latch rec+0x148 -6000; movement 0x561AF0 | 5 |
| `0x561FB0` | swim (300..1999) | 0x562330 | motion 0x94; movement 0x561AF0 (accel swim*1e-6+0.0025, damping 0.05, depth surface-2.1) | 5 |
| `0x562040` | swim (>=2000) | 0x562330 | motion 0x91; movement 0x561AF0 | 5 |
| `0x5620D0` | swim trick | 0x562330, rec+0x118&4 and 50% | motion 0x92; movement 0x561AF0 | 5 |
| `0x562160` | float in water | 0x562330, 20% | blend 20; after >30f eyes, mouth 2, sound; at end of motion rest 600..1200f then 0x562330; movement 0x561920 (rise 0.02/f to surface-2.1, no propulsion, damping 0.05; once-latch rec+0x148 -600, 0x13C -100, 0x140 -100) | 5 |
| `0x562330` | swim dispatcher | water interrupt 0x561630 (first entry); 0x561AF0: target reached, D+0x1C4>600 (~300 frames, double increment), wall with rec+0x148>=1000 at 0.2%; end of float 0x562160 | new swim target 0x561740; sets 0x10 at [[D+0x2C]+0xC]+0x64; picks 0x561E00 (<300) or 0x561FB0/0x562040, maybe 0x5620D0 (rec+0x118&4, 50%), maybe 0x562160 (20%); calls 0x561AF0 | 5 |
| `0x562D20` | default wander | picker best <= 0.2 (0x562BCC) | W+0x54 = destination from 0x534F80; installs a walk via 0x5A3650 (run points x3 outside areas 6/10/11: <150, <2000, <4000, else) | 1 |
| `0x562D50` | interrupting idle | walks (3%/2%), 0x563396, 0x563830, 0x59F763 | motion 0x9F, sound 0x100A at count 40, icon 5 (240f) + eyes 1 after 60, forward drift 0.055 in modes 0/1, motion 0xA0 at count>300 (about frame 302), end pulse -> resumes previous via 0x53DA00; always returns 0 | 2A |
| `0x562EB0` | climb ledge / out of water | wall + 0xCB touch in 0x5A3000; in water wall + rec+0x148<1000 + 0xCB (0x561AF0) | v=0; pos += (sin*0.1, 0.08, cos*0.1) per frame, a.y=-gravity, faces wall while touching; -> wander when on flat ground (>0.7) after >=4f, or >30f, or risen >2.5; return 1 after D+0x1C4>240 | 5 |
| `0x5630C0` | flutter/glide | 0x5A3000 airborne >2.0 above floor, target cell not water, fly x3>=400 and rand>=0.6 | eyes 4, motion 0x82 blend 15; launch like hop; turn 0x100/f, a.xz=dir*0.05-v*0.1; a.y=-g*0.95 for 300..600f then -g*0.92; y capped 100; landing -> wander | 5 |
| `0x563250` | walk to whistling player | end of 0x5634D0 | motion 0x76/0x77/0x6A, mouth 1/3, icon 4 120f; target = player pos; turn 0x100; ground speed 0.05*(1+run*0.0001); ends when sq dist < 64; every 100 frames 10% -> 0x562D50 | 6 |
| `0x5634D0` | come to whistle (turn + alert) | reaction 0x5637B0: player 1 action 42 within 100 units (sq < 10000), bond like > 50 | motions 0, 0xEE/3, 0x64, 70% 0x1EC (+eyes 4, sound 0x6019 if *0x134046C==*0x134062C); icons 3 (180f) and 1 (60f); D+0xB8=1.5; turns to player 0x300/frame until <0x100 BAMS; then 0x563250 | 6 |
| `0x563830` | chase ball | reaction 0x563B70: ball moving, TEDIOUS>4000, dist<50 (TEDIOUS -2000 every tick it fires; restarts chase) | motion 0x76/0x77/0x6A, mouth 1/3; target ball pos x,z*1.02; turn 0x180, speed 0.028+7e-6*run; per free latch TIRE +300, TEDIOUS -100; ends after 3000 frames, TEDIOUS<=999, TIRE>=7001, ball held, or ball in water *0x1DBE56C (icon 240f); 15%/100 frames -> 0x562D50; clears ALW flags/link | 6 |
| `0x563EB0` | hit by player | reaction 0x563D30: Status bit 4 + attacker (0x486800) | knockback 0.8 horiz/1.5 up; AddHappiness -30 (f -0.1); 0x53E1F0(50); emotion bytes 0x12D..0x131 += trunc(50+r*100) cap 200; eyes 1, icon 5 150f; hat dropped; motions 0x3E (spin 0x400/frame) or 0x10F, then 0x9F; 180..301 frames | 6 |
| `0x564320` | thrown/dropped landing | release path in 0x565DC0 | AddHappiness -5 (f -0.0667); 0x53E1F0(10); emotion bytes 0x12D..0x131 += trunc(50+r*100); eyes 1, icon 5; hat drops on W+0x40&0x800; motions 0x10F/0x9F; 180..301 frames | 6 |
| `0x5647B0` | 0xA2 reaction | interrupt 0x564760 (touch 0xA2), 500f limit | motion 0x238, eyes 1, mouth 8, icon 5 (300f), snd 0x6029/0x606D, ground velocity x0.9 per frame; never ends itself | 2B |
| `0x5648A0` | petted | reaction 0x564C10: D+4&1, 0x54B1F0(2) non-null, not already petted/sleeping/0x59FCA0/0x59F970/0x59FA50 | AddHappiness +1 (f 0.05) if rec+0x132<10; rec+0x132 +1/frame while touched by kind 2 (cap 200), -1/frame from 16th frame after; eyes/mouth 3 when >100; SLPDPTH>500 -> sleep 0x54FCF0; motions 0x3E/0x2A/0/0x41/0x42/0xFA/0xFC/1/0xD, 30% sound 0x6074/0x6075; ends when rec+0x132<10 | 6 |
| `0x565BE0` | idle/settle fallback | init 0x53DA80; tick return 3 or empty stack (0x53DD2D); 0x562C50 when D+0x1CC is non-zero and not 0x11 | motion (0x3E blend, or 3/0/0xEE/0xEF/1 by r with thresholds 0.15/0.5/0.6/0.7), icon 3 for 180 frames plus sound 0x601C on the 15% roll, eyes/mouth reset, D+0x1C0=60+trunc(121r), velocity x0.8 on ground; returns 2 after 61..181 frames | 1 |
| `0x5660A0` | held, likes | 0x566670 | motions 0x101..0x107, eyes 4, re-picked every 140..380f; HUNGER>5000 every 601f -> 0x5662B0 (via 0x565DC0) | 2B |
| `0x5662B0` | held, neutral | 0x566670 | motion 0x101; HUNGER>8000 every 601f -> 0x566300 (via 0x565DC0) | 2B |
| `0x566300` | held, dislikes | 0x566670 | motions 0x101/0x109..0x10E, eyes 1, mouth 4, snd 0x6047..0x6049 (40%), re-pick 100..240f; HUNGER>9999 every 601f in areas 1..3 -> jumps out of hands (0x565BE0) | 2B |
| `0x566580` | held, sleepy | 0x566670 | motion 0x101, eyes 9, icon 5 every 402 frames; SLPDPTH -2/frame, end at 0 | 2B |
| `0x566670` | held dispatcher | held interrupt 0x566720 | SLPDPTH>500 -> 0x566580; like>50/<-50/else -> 0x5660A0/0x566300/0x5662B0; happiness +1 if rec+0x132<10 | 2B |
| `0x566C40` | take animal/drive | reaction 0x566BC0 (touch 0x87, not held by a Chao) | motion 0xCA; turn 0x600 to within 0x71C; player drops; links -> 0x566DF0; else ends | 6 |
| `0x566DF0` | absorb animal/drive | 0x566C40 | motions 0xCB/0xCC, snds 0x1004/0x1005, absorb effects (items doc 1.3) | 2B |
| `0x567640` | take seed | reaction 0x567560: touch 0x85, run points (x3 outside areas 6/10/11) >= 150, hands empty, not eating/sleeping | motion 0xCA; player 1 drops held item; links seed -> 0x567710; else ends after 60 frames | 6 |
| `0x567710` | hold seed | 0x567640; 0x567830 70% on arrival | motion 3, eyes 0, icon 3, snd 0x601C, hand flag 0x400, 300..420f -> 0x567830 | 2B |
| `0x567830` | wander with seed, find plant spot | 0x567710 | hand flags 0xC00; 1800..2201 frames; finds 0xAC tree spot within intel*0.001+20; within 2.5 -> 0x567B10 | 6 |
| `0x567B10` | plant seed | 0x567830 at spot | needs rec+0x164 bit 10 (0x53E360(10), else icon 5, end); shovel model, motions 0xE5/0xE7/0xE8, sounds 0x1023/0x1024; spot facing = seed's; spot ALW mailbox +0x34=5,+0x36=seed kind; frees seed record (0x52F680) and deletes seed (MainSub 0x46F720) | 6 |
| `0x567FA0` | go to 0xAE target | picker 0x5625B0 | motion 0x64, icon 1; turns to ALW target; <4 units (or no target: distance to self 0.0) -> 0x5680F0; 1800..1920f limit | 2B |
| `0x5680F0` | water tree | 0x567FA0 | sound 0x601A; without rec+0x164 bit 11: rec+0x148 +2000 and end; with it: watering-can model, rec+0x148 +2000 -> 0x568460 | 6 |
| `0x568460` | pour water | 0x5680F0 mode 5 | emits 0xAD 4 units ahead (r 5.0, 10 frames) at motion flag 8; ends | 6 |
| `0x5685B0` | newborn | egg hatch push at 0x57AE62, call 0x57AE67 (also 0x57B067, 0x57B2FB) | motion 0xCE, end on motion&1 | 2B |
| `0x568D30` | cocoon/evolve | picker 0x562800 | cocoon task at 200f, evolution 0x568630 at ~2000f, jingle, ~3900f total, collision/holdable off then restored | 2B |
| `0x568F60` | death / rebirth | Picker candidate 0x562A20 when best < 1.0, life rec+0x8A == 0 and 0x54AC20 is true (ground flag 0x400 and horizontal speed < 0.04). Sets best = 1.0 and clears D+0x1CC/0x1CE. | Mode 0: eyes 0x53A4B0 (EDX=3), mouth 0x53A5A0, emotion ball 0x53D660 (dl=5, 300 frames), motion 0x793D30 by D+0xAE (0→0x2F, 1→0x3E, 2→0x2A with 15 frames; 3→0x3E with 45 frames; >3 no motion). After 200 frames: cocoon 0x568CD0(1 = rebirth / 2 = death). After 1800 more frames: decision 0x568F30 ((i16)happiness > 50 && (u16)STRESS < 700). Rebirth 0x568570: rec+0x8E++, record reset 0x551560, ALW unregister, new egg 0x57B9C0 case 2 (type 1, same record). Death: jingle chao_g_dead.adx, then after 900+930 frames 0x550620 memsets the 0x4DC record and sets MainSub = 0x46F720. On rebirth, MainSub = 0x46F720 after 900+930 frames. Interrupts (0x53DB3E) and the hit check (0x563D5B) skip while it runs. | 4 |
| `0x569240` | grab hat | hat interrupt 0x5691B0 (touch 0x93) | grab template; -> 0x569340 then 0x569430 (animal 16 absorbed and no hat) or 0x569550 | 2B |
| `0x569340` | look at hat | 0x569240 | motion 0xBE, icon 3, sound 0x601C, 120..180 frames | 6 |
| `0x569430` | wear hat | follow-up set by 0x569240 when rec+0x118&0x10000 and rec+0xD5==0 | motions 0xC7/0xC8; wears hat (rec+0xD5), frees hat record (0x5694C5); mode never advances (mode 2 unreachable), ends via 0x56CFF0 once the hat is gone | 6 |
| `0x569550` | toss hat | pushed by 0x569240 otherwise | motion 0xC9, hat thrown (0.3, 0.7) at rot+0x71C7, unlink | 2B |
| `0x56B480` | notice | go-eat chain, toy choosers | emotion icon 1 (180 frames), returns 1 on 122nd frame; D+0x1C8 timeout -> reset | 2A |
| `0x56B500` | face target (skill join/watch head) | pickers 0x59D0D0, 0x59C3D0, 0x59B710 (signals 0xA5/0xA6/0xA7) | motion 0x64, 0x796910(0x200) until the angle error < 0xB6; target = signal pos (join) or performer pos (watch); follow-ups as listed in the 1.5 table; skill joins change TEDIOUS -4000, LONELY -4000 self, performer LONELY -4000 (A) / -3000 (B) | 1 |
| `0x56B560` | walk to W+0x54 | follow-up of TV/radio/horse/toy choosers | eyes 4; stands still (timeout restarted) for 32f, then motion 0x73/0x64, D+0xB8=1.5; turn 0x180/f; accel 0.044 (no ground check); return 1 at 3D dist<4.0; 1800f timeout -> 0x565BE0 | 5 |
| `0x56B6C0` | face linked object | follow-up of TV/radio/toy, 0x5A2EEB | motion 0x64; 0x530510(0x200) toward ALW link for 90f; D+0xB8=1.6; pop | 5 |
| `0x56B740` | spin away from wall | 0x5A3000: W+0x40&0x4000 without 0xCB hit, or 0xCC object after D+6>30 | motion 0x64 blend 15; D+0xC +/-0x16C per frame (50/50) for 45..90 frames; D+0xB8=1.6; return 1 | 5 |
| `0x56B860` | turn around 180 deg | unknown | motion 0x64 blend 15, rotate +-0x16C/frame for 90 frames (180 deg), D+0xB8=1.6, then 1 | 2A |
| `0x56B960` | random spin | follow-up of 0x54EE10 | motion 0x64; +/-0x16C BAMS/f for ftol(91r) frames (0..90); pop | 5 |
| `0x56BA80` | walk to linked object | 0x54EE10, 0x5614D0, 0x599210, 0x599400, 0x599770, 0x5A2EDC | eyes 4; after 31f motion 0x64, D+0xB8=1.5; 0x530510(0x180); accel 0.044; dist (0x530580) <4.0 -> return 1 (no link: 0x530580 = 0.0, ends on frame 33; the <0 reset cannot fire); 1800f timeout | 5 |
| `0x56BBA0` | press toy switch | follow-up of 0x599210/0x599400/0x599770 | motion 0xBE; v*=0.9 on ground while waiting; after >60f emits signal 0xAF ahead of the Chao (0x55A920, 10 frames) and returns 1 | 5 |
| `0x598890` | watch TV | last entry of 0x599210 chain | motions 0x3E/0x3F/0x1CB(/0x3A/0x3C if animal 9), re-pick every 300..600f; TV signal 0xC9 gone -> motion 0x23D 120..210f, D+0x1F4=1800..3600, end | 2B |
| `0x598B90` | listen to radio | 0x599400, 0x5998C0 (signal 0xCA) | same as watch TV with signal 0xCA; cooldown 1800..3600 | 6 |
| `0x598E90` | jump off horse | 0x598F40 mode 3 sets it, but the ride returns 1 at once, so it never runs from there | MotionSet 0x84, side hop vel 0.3 at rot+0x4000, vy 0.8 [0xDF21B4], rewritten every frame (rises forever if started in the air); ends when on-ground bit set | 2B |
| `0x598F40` | ride horse | follow-up of 0x5995F0 | jumps on (vy 0.65), emits 0xAF (3.5, 10 frames; horse rocks), 1800 frames, faces the horse (m1-m3), follows horse pos (10% lerp, m2-m3); cooldown 1800..3600; sets 0x598E90 then returns 1 -> settles in 0x565BE0 | 6 |
| `0x599210` | go to TV | picker 0x599B60 toy kind 2 | sets ALW target, W+0x54 spot (0x55C4A0), installs 0x55C3C0,0x56B480,0x56BA80,0x56BBA0,0x56B560,0x56B6C0,0x598890 | 2B |
| `0x599400` | go switch on radio | 0x599B60 toy kind 3 (the chooser takes TEDIOUS -5000) | walk chain, press 0xAF, then 0x598B90 | 6 |
| `0x5995F0` | go to horse | picker 0x599B60 toy kind 7 | links horse, installs 0x55C3C0,0x56B480,0x56B560,0x56B500,0x598F40 | 2B |
| `0x599770` | go to box | 0x599B60 toy kind 8 (the chooser takes TEDIOUS -5000) | walk chain, press 0xAF (0x56BBA0), then 0x565BE0 | 6 |
| `0x599B60` | toy chooser | D+0x1F4==0, toy perceived, 0.99*f16(TEDIOUS,2000) | TEDIOUS -5000 when it wins (0x599C53) | 3 |
| `0x599D40` | illness 0 | picker 0x599CB0, rec+0x15A<0 | motions 0xF4..0xF7, sound 0x606A; ends randomly (10%/cycle, max 11 cycles) | 2A |
| `0x599F30` | illness 1 | rec+0x15B<0 | motion 0xF2/0xF3, sound 0x606B at frame 180; ends 12% per 60-frame roll | 2A |
| `0x59A0B0` | illness 2 | rec+0x15C<0 | motion 0/0x31, sounds 0x1015/0x602C; ends 20%/cycle, max 14 cycles | 2A |
| `0x59A260` | illness 3 | rec+0x15D<0 | motion 0x51/0, eyes 9, sound 0x1010; ends 12% per 60-frame tick | 2A |
| `0x59A400` | illness 4 | rec+0x15E<0 | motion 0xF8/0xF9, sound 0x606E; ends 15% per tick, max 11 cycles | 2A |
| `0x59A5A0` | illness 5 | rec+0x15F<0 | motion 0x5E/0xA3, sound 0x6022/0x6023; ends 12% per tick, max 11 cycles | 2A |
| `0x59A740` | skill C level 1 | 0x59B900 | motion 0x121/0x125, voice 0x607E every 241 frames; while 0xA7 (1500..3300, r20) | 2A |
| `0x59A8A0` | skill C level 2 | 0x59B900 | as level 1, voice from 0x11C414C[0..1] | 2A |
| `0x59AA30` | skill C level 3 | 0x59B900 | voice from 0x11C414C[0..2] | 2A |
| `0x59ABC0` | skill C level 4 | 0x59B900 | voice from 0x11C414C[0..3] | 2A |
| `0x59AD50` | skill C level 4 | 0x59B900 table entry 4 (bits 0..4 of `rec+0x162` all set) | voices 0x11C414C[0..3] and 0x6080 (30%), two cadences 241/451 frames; ends when no 0xA7 | 2A |
| `0x59B0E0` | animal-10 trick | 0x59B900 alt (rec+0x162 bits 0..4 clear, rec+0x118&0x400, alignment rec+0xB0 >= -0.5) | motion 0x121/0x125, voices 0x6076-0x6078 every 181 frames; signal 0xA7 1200..2400 r20 | 2A |
| `0x59B270` | animal-10 trick, dark | 0x59B900 alt (rec+0x162 bits 0..4 clear, rec+0x118&0x400, alignment rec+0xB0 < -0.5 [0xE9923C]) | motion 0x121/0x125, voice 0x607C every 221 frames; signal 0xA8 1800..3600 r20 | 2A |
| `0x59B3D0` | watch skill-C performer | 0x59B710 (0xA7) | poses 0x181/0x3F while 0xA7; then 0x23D | 6 |
| `0x59B580` | react to 0xA8 | picker 0x59B820 | motion 0x232/0x3F, 300..600 frames; 50% rec+0x12D +150 (max 200) | 2A |
| `0x59BB50` | breed caller | 0x562880 (best<1, not child, not types 20-25): BREED>=10000, no 0xAB near, <8 Chao, standing still; best 1.0 | emits 0xAB (30.0, 8880 frames), heart object 0x5AAF60, icon 4; BREED=0 at 4500 frames; waits for partner (mailbox 1), dance motions 0xFC/0x1D4/0x1EC | 6 |
| `0x59BF30` | court partner (egg maker) | picker 0x562950 | walks to 0xAB caller, links, jingle 0x11C3FF4, dance 0x1D4, BREED=0, hop + egg 0x57B9C0 at 2 units ahead on frame 20 of mode 4, ends after 4..6 motion ends | 2B |
| `0x59C870` | skill B0 | 0x59C6D0 | motions 0x171/0x173/0x175/0x177; while 0xA6 (1800..4200, r20) | 2A |
| `0x59C980` | skill B1 | 0x59C6D0/0x59C3D0 | motions 0x179/0x17B/0x17D/0x17E; while 0xA6 | 2A |
| `0x59CA90` | skill B2 | 0x59C6D0/0x59C3D0 | motion 0x183; while 0xA6 | 2A |
| `0x59CB20` | skill B3 | 0x59C6D0/0x59C3D0 | motion 0x184, 10% per end pulse 0x185; while 0xA6 | 2A |
| `0x59CBF0` | skill B4 | 0x59C6D0/0x59C3D0 | motion cycle 0x138-0x13D; while 0xA6 | 2A |
| `0x59CDA0` | watch 0xA6 (v1) | picker 0x59C3D0 r<0.5 | motion 0x181/0x3F; end motion 0x23D 180..300 frames | 2A |
| `0x59CF50` | watch 0xA6 (v2) | picker 0x59C3D0 r>=0.5 | motion 0x17F; end motion 0x23E 180..300 frames | 2A |
| `0x59D5A0` | skill A5 | 0x59D410/0x59D0D0 | motion 0x15C-0x15E, models 0x11C505C/0x11C54F4, sounds 0x400F/0x6033; while 0xA5 | 2A |
| `0x59D8E0` | skill A0 | 0x59D410/0x59D0D0/0x562C50 | motion 0x15F/0x160, block 0x11C9B54 node 11, sound 0x4009/30 frames; while 0xA5 signal (1800..4200, r20) | 2A |
| `0x59DA40` | skill A3 | 0x59D410/0x59D0D0 | motion 0x161/0x162, 3 attached models, sound 0x4008; while 0xA5 | 2A |
| `0x59DD10` | skill A4 | 0x59D410/0x59D0D0 | motion 0x163, block 0x11C9B74, sound 0x400A; while 0xA5 | 2A |
| `0x59DE90` | skill A1 | 0x59D410/0x59D0D0 | motion 0x165-0x167, model 0x11C9B94 node 4, sound 0x400D; while 0xA5 | 2A |
| `0x59E120` | skill A6 | 0x59D410/0x59D0D0 | motion 0x169, special model D+0x650=0x11C830C, sound 0x4010; while 0xA5 | 2A |
| `0x59E2D0` | skill A2 | 0x59D410/0x59D0D0 | motion 0x16B/0x16C, models 0x11C8964/0x11C8FFC, sound 0x4007; while 0xA5 | 2A |
| `0x59E500` | skill A7 | 0x59D410/0x59D0D0 | motion 0x170/0x16F, model 0x11C9AE4, sound 0x4011; while 0xA5 | 2A |
| `0x59E710` | watch 0xA5 performer | picker 0x59D0D0 | motion 0x181/0x3F, eyes 4, mouth 3; when signal gone motion 0x23D for 180..300 frames | 2A |
| `0x59E8C0` | beg for fruit held by player | 0x59EB00 branch A, nearest fruit has Status 0x8000: jitter(f16(HUNGER,5000)*0.99) | motion 0x76; turns 0x120/frame and walks 0.055 to player; ends within 10 units or 1000 frames; follow-up 0x59EA00 | 6 |
| `0x59EA00` | beg follow-up | push 0x59EC9F | motion 0x18E, eyes 4, 180..300 frames | 2A |
| `0x59ED60` | hungry | 0x59EB00 branch B (D+0x20C==0 and r<0.6): jitter(f16(HUNGER,5500)*0.85) | motion 0x5E (posture 1, 180 frames) or 0xA3; sets D+0x20C=trunc(1200+5400*((10000-HUNGER)/10000+r)) | 6 |
| `0x59EF40` | starving | 0x59EB00 branch B and HUNGER>9900: jitter(1.0) | mouth 6, motion 0x1DC, 240..440 frames (`0x905A00`), tears, sound 50% when D+0x30 % 60 == 0; sets D+0x20C as hungry | 6 |
| `0x59F350` | emotion 0x12F behaviour | picker 0x59F210 | motion 0x1F1/0x1F2, eyes 1, mouth 8, snd 0x602D (50% every 150f), 180..480f | 2B |
| `0x59F5F0` | flee from player | pushed at 0x59F2D8 when player seen | motion 0x76/0x6A, eyes 1, mouth 8, flee target 0x59F490 30 units, latch TIRE+900, drive 0.08; ends >1000f or arrived with player dist >=50 | 2B |
| `0x59F8E0` | emotion 0x12E dispatcher | picker 0x59F860 | sets 0x59FA50/0x59FCA0 (rec+0x12E>120) or 0x59FA50/0x59F970 | 2B |
| `0x59F970` | emotion 0x12E low | 0x59F8E0 | motion 0x1F9/0x1F6, eyes 3, mouth 4, 180..300f | 2B |
| `0x59FA50` | emotion 0x12E variant | 0x59F8E0 | motions 0x220..0x229, rec+0x12E -=10 per cycle until <10 | 2B |
| `0x59FCA0` | emotion 0x12E strong | 0x59F8E0 | motions 0x20B..0x215, tear effects 0x6ED270 (scale 0.1), snd 0x6001, rec+0x12E -=10 per cycle | 2B |
| `0x5A00A0` | emotion 0x131 behaviour | byte 0x131>150; score 1-(100-p14C)*0.005*0.5, jittered (0x5A0010) | 0x131 -10 per timer (one mode) or -20 per timer up to 5 times (other mode), ends below 10 | 3 |
| `0x5A03B0` | call to player (lonely) | 0x5A04A0 gate A (D+0x1DC==0): like>=0 and (bond+4<2 or bond+2>300), jitter(f16(LONELY,1000)*0.99) | motion 0x18C+sound 0x6091 if alignment>0.5 else 0x18D; eyes, mouth 3; 201 frames; follow-up 0x5A0490 | 6 |
| `0x5A0490` | bond counter update | follow-up of 0x5A03B0 / 0x5A1800 (from 0x5A04A0) | 0x53E280: bond[c]+4 ++ (cap 0xFFFF), bond[c]+2 = 2*floor(v/3); ends | 6 |
| `0x5A0820` | stay near player | 0x5A0960 within 5 units | motion 0x190, eyes 3, mouth 3; rec+0x12C +10 per motion loop while within 5; >140 -> LONELY -6000 and end; farther than 5 -> 0x5A0960 | 6 |
| `0x5A0960` | run to liked player | 0x5A04A0 gate B (D+0x1D4==0): like>60, bond+2>100, jitter(f16(LONELY,5000)*0.99); also follow-up of itself via 0x5A0820 | motion 0x75; at start if rec+0x12C>180: LONELY -6000 and end; sq dist<25 to player 1: rec+0x12C +10 -> 0x5A0820 (+follow-up 0x5A0960); max 1000 frames | 6 |
| `0x5A0BA0` | emotion 0x12C behaviour | picker 0x5A0B10 (rec+0x12C>100) | motions 0x1D4/0x1D6/0x1CF/0x1EC, snds 0x602F..0x6033; rec+0x12C -=20 per cycle (max 9) | 2B |
| `0x5A0F40` | hop | 0x5A3000: on ground with floor n.y<0.7; or >2.0 above floor and (target cell water, or fly x3<400, or rand<0.6) | eyes 1, mouth 2, motion 0x8A; v=(sin*0.4,0.35,cos*0.4); a.y=+0.45*0.055 every frame; mode 1 on ground -> wander | 5 |
| `0x5A1020` | go to water | picker 0x562520 (groundwork: THIRSTY>9000) | W+0x54 = random point in a water cell (list 9 at 0x1A1E95C); sound 0x6043 (0x6044 only if r == 0.0; active area); installs walk 0x5A3650 | 5 |
| `0x5A10B0` | play/skill/song chooser | TEDIOUS>2000, 0.99*f16(TEDIOUS,2000); 40% skills A/B/C or song, 60% toy skill or animal play | TEDIOUS -5000 when something is installed | 3 |
| `0x5A1460` | dark mischief laugh | 0x5A1240: p14B<=30, type in {7,10,13,16,19,22}, anger>80; either jitter(f8(anger,80)*0.99) (perceived, like<0, p14C>30, r<0.7) or 1-(100-p14C)*0.005*0.3 | motion 0x203, voices 0x6065/0x6066/0x6068, timed; follow-up 0x5A18A0 only in the perceived branch | 6 |
| `0x5A1600` | mischief act 1 | 0x5A18A0 (r<0.35) | motion 0x1B9, eyes 10, end on motion&1 | 2B |
| `0x5A1680` | mischief act 3 | 0x5A18A0 (r<0.9) | motion 0x1C3, eyes 10, end on motion&1 | 2B |
| `0x5A1700` | mischief act 4 | 0x5A18A0 (r>=0.9) | motion 0x1BB, eyes 10, end on motion&1 | 2B |
| `0x5A1780` | mischief act 2 | 0x5A18A0 (r<0.7) | motion 0x1BD, eyes 10, end on motion&1 | 2B |
| `0x5A1800` | dislike player | picker 0x5A1240; 0x5A04A0 | motion 0x1BF, eyes 1, mouth 4, snd 0x6069, end on motion&1 | 2B |
| `0x5A18A0` | mischief loop (approach player, taunt) | follow-up of 0x5A1460 (perceived branch), 0x5A04A0 gate-B angry branch (follow-up only), itself | motion 0x7D; within 5 units anger -10 and picks 0x5A1600/0x5A1780/0x5A1680/0x5A1700 (0.35/0.35/0.2/0.1); 60% voice 0x606F..0x6072; ends if anger<10 or 1000 frames | 6 |
| `0x5A1BA0` | rest (tired) | picker 0x5626D0: TIRE > 2000+round(4000*(p14A-p155+200)/400), score 0.99*f16; installs 0x55C430 and pushes 0x5A1BA0 as follow-up | per need period: TIRE -trunc(100*(300+p151)/300), CONDITN +trunc(30*(300+p151)/300); when timer D+0x1C0 expires it ends if HUNGER/SLEEPY/TEDIOUS/LONELY>8000 or TIRE<100, otherwise ends with p=0.3 (r<0.3) and repeats with p=0.7 | 3 |
| `0x5A1F10` | animal play 4 | 0x5A2F10 | motion 0x118, 2..4 end pulses | 2A |
| `0x5A1FC0` | animal play 19 | 0x5A2F10 table | motion 0x12F, end on motion&8 | 2B |
| `0x5A2010` | animal play 8 | 0x5A2F10 | motion 0x11A/0x11C, sound 0x6087, random-length loop | 2A |
| `0x5A2230` | animal play 6 | 0x5A2F10 | motion 0x12D, forward 0.05/frame, 120..240 frames | 2A |
| `0x5A2310` | animal play 5 | 0x5A2F10 | 0x13E/0x13F/0x140/0x81, slide 0.3/frame, 120..180 frames | 2A |
| `0x5A2460` | animal play 9 | 0x5A2F10 | motion 0x3A/0x3C, 600..900 frames | 2A |
| `0x5A2530` | animal play 20 | 0x5A2F10 table | motion 0x187, 300..500 frames | 2B |
| `0x5A25D0` | animal play 12 | 0x5A2F10 | motion 1, sound 0x1013, effect 0x6CC4E0 behind, signal 0x97 r5 60 frames | 2A |
| `0x5A27B0` | animal play 18 | 0x5A2F10 table | motion 0x11D, hand-node effect 0x6ECA30 after 20f, end on motion&8 | 2B |
| `0x5A2860` | animal play 3 | 0x5A2F10 | motion 0x135, 4..7 end pulses | 2A |
| `0x5A2910` | animal play 11 body | push by 0x5A29C0 | motion 0x136, 4..7 end pulses, then 1 | 2A |
| `0x5A29C0` | animal play 11 | 0x5A2F10 | Set(0x55C430)+push 0x5A2910; returns 0 | 2A |
| `0x5A2A10` | animal play 13 | 0x5A2F10 | motion 0x132, forward 0.1/frame, 4..7 end pulses | 2A |
| `0x5A2B10` | animal play 1 | 0x5A2F10 | motion 0x141, sideways 0.05/frame, 500..800 frames | 2A |
| `0x5A2BF0` | animal play 0 | 0x5A2F10 | motion 0x71, forward 0.15/frame, 120..180 end pulses | 2A |
| `0x5A2CD0` | animal play 14 | 0x5A2F10 table | motion 0x23F, snd 0x6084, 400..600 frames then end | 2B |
| `0x5A2DA0` | tree activity (animal 7 follow-up) | push 0x5A2EF7 | motion 0x120 for 300..500 frames | 2A |
| `0x5A2E40` | animal play 7 | 0x5A2F10 | 70% with tree <40 (state 3/5): walk to tree (0x56BA80), face it (0x56B6C0), 0x5A2DA0 (motion 0x120 300..500) | 2A |
| `0x5A3000` | walk step (helper, not a stack entry) | called by every walk each frame | arrive XZ^2<36 -> return 1; wall -> 0x562EB0 (+y 0.1) or 0x56B740; D+6>30 and 0xCC -> 0x56B740; turn 0x120/f; steep (n.y<0.7) -> v.y+=0.55, hop; >2 above floor -> v.y+=0.55, hop or flutter; no early return (last SetBehavior wins); once-latch rec+0x13C -100, rec+0x140 +300 | 5 |
| `0x5A3280` | walk (run<150) | pushed by 0x5A3650 after prelude 0x55C430 | motion 0x73; accel 0.038 along facing on ground; sound when D+0x30%120==0, 30%, active area; then 0x5A3000 | 5 |
| `0x5A3380` | walk (run 150..1999) | pushed by 0x5A3650 after 0x55C3C0 | motion 0x64; accel 0.055 on ground; D+0xB8=\|v\|*16; D+0x1C0++, old%85==1 -> 3% stumble 0x562D50; then 0x5A3000 | 5 |
| `0x5A3460` | jog (run 2000..3999) | pushed by 0x5A3650 (0x53D970) after 0x55C3C0 | motion 0x65; accel 0.028+run*7e-6 on ground; D+0xB8=\|v\|*14; D+0x1C0++, old%100==0 -> 2% stumble 0x562D50; then 0x5A3000 | 5 |
| `0x5A3580` | run (run>=4000) | pushed by 0x5A3650 (0x53D970) after 0x55C3C0 | motion 0x66; accel 0.028+run*7e-6 on ground; D+0xB8=\|v\|*14; then 0x5A3000 | 5 |
| `0x5A3650` | install walk (helper) | 0x562D20, 0x5A1020 and others | run pts (x3 unless game mode 6/10/11): <150 Set(0x55C430)+0x5A3280; <2000 Set(0x55C3C0)+0x5A3380; <4000 Set(0x55C3C0)+0x5A3460; else Set(0x55C3C0)+0x5A3580 | 2A |
| `0x5AB2C0` | sing | 0x5AB5D0 | motions 0x143/0x145, blocks on nodes 4/11, signal 0xA9 r2 1800 frames, spawns 0x5AB200 song object (id D+0x41C), sound 0x1026; 1800 frames | 2A |
| `0x11C9BF4` | illness behaviour slot i | picker 0x599CB0: (i8)rec+0x15A+i < 0 and 30000r > CONDITN+20000 | best=0.99 (behaviour body not mapped here) | 1 |

## Unknowns

- Names of the emotion bytes `rec+0x12C..0x133`, and the need-word labels, which follow record order only: the debug label table at `0x12E5BC4` has no xrefs. The behaviour evidence fits `0x12E..0x133` = ANGER..TOTAL; `0x12C`/`0x12D` have no label.
- Who writes `D+0x1CC` (forced-behaviour id) and `*0x1A5A3EF` / `*0x1A5A3F2`; only reads were seen (it looks like a debug leftover).
- Which garden situations set `D+0xA0 & 0x2000` (CreateChao clears it).
- Where `D+0x6` (the obstacle counter in `0x5A3000`) is reset.
- What `*0x174AFFE` is exactly (it selects `0x53D720` vs `0x564320` on release).
- Meaning of the icon ids passed to `0x53D660`, the eye/mouth ids, and many motion ids.
- The per-character sign table of `AddHappiness` (`0x5363B8`/`0x5363AC`) has not been read.
- Who sends mailbox message 6 (fruit lost), and what `0x565BC0` (balloon mode 6) means.
- The caller of `0x56B860` (fixed 180° turn) and any setter of sleeping mode 14.
- Collision kinds: the object that emits 0xA2 (main `0x56A110`), 0xAC (planting spot), 0xAE, the 0x81 partner check in courting, and what kind 2 (pet trigger) is attached to.
- The second fruit-category object (`0x55A560`, main `0x55A290`), toy kind 0 (`0x5604C3`), and which objects register toy kinds 5 and 6.
- Which animals `rec+0x118` bits 2 (swim trick), 9 (TV motions) and 16 (wear hat) stand for.
- NOURISH (`0x144`) is never raised and STRESS (`0x142`) never written in the scanned code; CONDITN (`0x146`) has no reader outside the tick and rest.
- Meaning of `rec+0xD1`/`rec+0xD2` (looked up from personality buckets in `0x12D6534`/`0x12D68B4`); personality bytes `0x152..0x154` and taste bytes `0x158`/`0x159` have no readers.
- The 20 u16 entries at `rec+0x1A6 + 0x1C·k` that the bond tick grows (+16, cap 50000), probably Chao-to-Chao memory.
- The gene mix for bred eggs (`0x551D20`).
- Who sets physics mode `D+0xA4` to 1 (heightmap `0x53DD50`) or 2.
- Uses of `W+0x28`, `W+0x30`, `W+0xB4`, `W+0xC0`, `W+0xCC`, `W+0x40` bit 0x4, and the bit 0x10 that swim sets at `[[D+0x2C]+0xC]+0x64`.
- Whether Chao push each other or the player through the object collision system (`0x47E750`).
- Signals: no listener found for 0x97; the behaviours containing the 0xAF emitters at `0x55F6BB`/`0x55F8B8` and the 0xA8 emitter at `0x59B386`; when the TV/radio emit 0xC9/0xCA (`0x55C960`, `0x57CA10`).
- The preludes `0x55C3C0`/`0x55C430` return whatever EAX `0x793D30` leaves in mode 0.
- `0x530510` with no ALW target reads an uninitialised stack slot as the target.
- The mating dance modes of `0x59BB50` after mode 3.


## 7. Perception (0x590C70) and senses

Sources: REA pseudocode and assembly of `0x5904C0`..`0x590D20`, `0x530410`, `0x53E0C0`,
`0x55A920`, `0x55A850`, `0x486760`, `0x486880`, `0x47EC50`, `0x485620`, `0x485850`, `0x483170`;
constants read from the dumped exe.

### 7.1 Record layout

Init `0x5904C0` (EAX=obj; called from CreateChao `0x5504D4` and `0x56CF8B`) clears the six lists and
sets constants that **nothing else writes**:

| Field | Value | Use |
|---|---|---|
| `D+0x7CC` f32 | **50.0** (`0x8AF024`) | range |
| `D+0x7D0` int | 0x4E38 (20024 BAMS, ≈110°) | not read |
| `D+0x7D4` int | **0x271C** (10012 BAMS ≈ **±55°**) | half-angle of the view cone |
| `D+0x7D8`, `D+0x7DC` f32 | 40.0 (`0x8AE63C`) | not read |

Six lists of 0x398 bytes from `D+0x7E0` (header 0x18 + 32 entries × 0x1C). Clear `0x590520` (ECX=list):

| List off | Header field | Meaning (cleared value) |
|---|---|---|
| +0x00 u16 | count | entries written (0) |
| +0x04 int | found | 1 if anything is in view (0) |
| +0x10 f32 | nearest distance | (100000.0, `0x905060`) |
| +0x14 s16 | nearest index | index into the entries (−1) |
| +0x18 + i·0x1C | entry i | `+0..+6` 4×u16 (0xFFFF, unused), `+8` f32 distance (100000.0), `+0xC` int 1 when written, `+0x10/+0x14` 0 unused, `+0x18` **ALW entry** of the object (`obj+0x3C`) |

| # | Base (found / nearest idx / entry 0) | Objects | Filled by | Read by |
|---|---|---|---|---|
| 0 | `0x7E0` (`0x7E4` / `0x7F4` / `0x7F8`) | player 1 only (`*0x1DEA6E0`); at most one entry, its `+0x18` (`D+0x810`) = 0 | `0x590590` | `0x59F210` (#16), `0x5A04A0` (#23 lonely), `0x590CE0` → `0x5A1240` (#21) |
| 1 | `0xB78` (`0xB7C` / `0xB8C` / `0xB90`) | ALW cat 0 (Chao), self excluded | `0x590680` | **nobody** |
| 2 | `0xF10` (`0xF14` / `0xF24` / `0xF28`) | ALW cat 3 (fruit) | `0x5907C0` | `0x59EB00` (#5 hunger) |
| 3 | `0x12A8` (`0x12AC` / `0x12BC` / `0x12C0`) | ALW cat 4 then cat 5 (trees) | `0x590900` | `0x59EB00` (dead branch), go to bed `0x54EE10` |
| 4 | `0x1640` (`0x1644` / `0x1654` / `0x1658`) | ALW cat 6 (toys) | `0x590B30` | `0x590D20` → toy chooser `0x599B60` (#13) |
| 5 | `0x19D8` | nothing | only cleared | nobody |

(Readers found by scanning all of `0x53xxxx..0x5Bxxxx` for these displacements; lists 1 and 5 have no reader.)

### 7.2 How `0x590C70` fills them

`0x590C70` (EBX=obj): clear all six lists (`0x590C80..0x590C9F`), then `0x590590`, `0x590680`, `0x5907C0`,
`0x590900`, `0x590B30` in that order (each clears its own list again first). Its only caller is the
picker `0x562A80` (`0x562A99`), so **perception is refreshed only on a picker run** (behaviour returned 2
and `D+0x1CC == 0`; an idle Chao every 61..181 frames, §1.6). Every reader runs inside that same picker
pass, except `0x54EE10`, which the picker installs and which runs on the next tick (one tick stale).

Per object (same code in all five):
1. Object source: for the ALW lists, `n` runs from `count[cat]−1` (u32 `0x1DC0F80[cat]`) **down to 0**,
   `obj = 0x530410(EAX=cat, EDI=n)` = the n-th live entry's `+0x3C` among the first `count` slots. Skip if
   NULL or `obj+0x34` (Data1) is NULL; list 1 also skips `obj == self` (`0x5906EE`). Player: `*0x1DEA6E0`
   and its Data1 non-NULL.
2. Distance `d = 0x77FBD0(target Data1+0x14, own D+0x14)` = **3D Euclidean** (x, y, z).
   In range iff **`d < 50.0`** (strict; `FCOMPP; TEST AH,0x41; JNZ skip`).
3. Bearing `b = ftol(atan2(Δx, Δz) · 10430.3804)` (`0x859B6A` = `_CIatan2` on ST1=Δx, ST0=Δz;
   double `0x904DB8`; Δ = target − self). In view iff **`|s16(D+0xC − b)| < 0x271C`** (16-bit subtract,
   negate if negative, compare as u16 against the int; `0x59061D..0x590634`). Same convention as
   `physics.rs` (`atan2(dx,dz)·10430.378`). No pitch test.
4. **No line-of-sight, no water/area, no state test.** Nothing about the target (held, eaten, being
   ridden) is checked here.
5. On pass: `found = 1`; entry[k] = {`+8` d, `+0xC` 1, `+0x18` `obj+0x3C`}; if `d < nearest` (strict)
   then nearest = d, nearest idx = k; `count++`, `k++`. Ties keep the first one visited (highest ALW n).
6. **No capacity check.** Cap 32 entries; ALW caps (`0x8AB838`) are Chao 32, fruit **48**, tree 10 + 16,
   toys 32. More than 32 fruit in the cone would overrun into list 3, which `0x590900` then re-clears.

Player list `0x590590` writes entry 0 only (`D+0x800` d, `D+0x804` 1, `D+0x810` 0) and sets nearest
idx `D+0x7F4 = 0` (d < 100000 always).

ALW categories as registered (`0x530750`, ECX = cat, kind = 2nd push):

| Cat | Registered at | Notes |
|---|---|---|
| 0 Chao | `0x5502E1` | |
| 3 fruit | `0x5462A0` (fruit `0x546180`), `0x55A5B3` (object at `0x55A560`, main `0x55A290`, same delete `0x57B9B0`; not identified) | |
| 4 tree | **no call site** | list 3's cat-4 pass is always empty |
| 5 tree | `0x548628`, unconditionally at the end of the GrowTree constructor `0x548210` | **all 7 tree spots of the garden, empty ones (state 0) included** |
| 6 toys | kind 2 `0x55CBF3` (TV), 3 `0x57CD00` (radio), 4 `0x55D693` (ball), 7 `0x580F19`, 8 `0x5808F4`, 0 `0x5604C3` | kinds 7/8/0 objects not identified here |

### 7.3 Player seen, `*0x1DEA6E0`, `0x590CE0`, `0x590D20`, `0x53E0C0`

- The player-registration routine `0x4599C0` (ESI = player obj, ECX = index) stores, for player i:
  `0x1DEA6E0[i]` = the **ObjectMaster**, `0x1DEA6C0[i]` = its Data1 (`+0x34`), `0x1DE95E0[i]` = `+0x38`,
  `0x1DE9600[i]` = `+0x40` (CharObj2) (`0x459A1C..0x459A31`). So `*0x1DEA6E0` is player 1's task.
  Player 2 is never perceived.
- **`D+0x7E4 != 0 && D+0x7F4 >= 0`** holds iff at the last picker run player 1 existed and
  `|P1.pos − pos| < 50` (3D) and `|s16(rotY − atan2(Δx,Δz)·65536/2π)| < 10012`. Nothing else: no
  line of sight, no check of what the player is doing. The readers then also test `*0x1DEA6E0 != 0`.
- `0x590CE0` (EAX=obj): returns `*0x1DEA6E0` if `D+0x7E4 != 0` and `D+0x7F4 >= 0` (the entry-address
  test is always true), else 0. It reads the current global, not the list entry.
- `0x590D20` (EAX=obj): if toys found and nearest idx ≥ 0, returns `entry.+0x18` (ALW entry) `→ +0x3C`
  (the toy **object**); 0 if any link is NULL. The chooser then reads the toy kind from `obj+0x3C → +4`.
- `0x53E0C0`: `p = *0x1DE9600` (player 1's CharObj2); returns u8 `p+1` if it is 1..5, else 0 (also 0
  with no player). It is **independent of perception**. `p+1` is the main character slot (the one
  `src/game.rs` reads at `co2+1`): 0 Sonic, 1 Shadow, 2 Tails, 3 Eggman, 4 Knuckles, 5 Rouge. Per
  `game.rs`, Tails and Eggman on foot in the garden have 6/7 there, so **they map to bond 0
  (Sonic's)**.

### 7.4 Signals and touches (`0x55A920`, `0x55A850`, `0x486880`, `0x486760`)

- Emit `0x55A920(parent, kind, f32 R; EDI=&pos, EBX=frames)`: child task of `parent` (`0x470C00`,
  so the signal's `+8` is the emitter), main `0x55A6A0`, collision `0x47E520(table 0x8A5C40, 1 entry,
  3)`; entry kind byte := kind, entry radius `+0x14` := R; Data1 pos := *EDI (**fixed**, it does not
  follow the emitter), `+0x20` := R; frames > 0 → `+1 = 1`, `+6 = frames`. Stored at emitter `D+0x40`
  (deleted on the emitter's next behaviour change, `0x53D760`). `0x55A850(kind, &pos, R, frames)` is
  the same as a top-level task ("AL_FieldExecutor", no parent): used by TV 0xC9 and radio 0xCA
  (`0x55C997`, `0x57CA3E`: R 25.5 `0x90747C`, 3600 frames), 0xA2 (`0x58035B`, R 5.0), 0xC8 (`0x53B3D2`).
- Signal main `0x55A6A0`: if `+1`: when `+6 == 0` delete, else `+6 −= 1`; then register collision
  (`0x47E750`). So it lives frames+1 frames. With frames ≤ 0, `+1` is never set and the signal never
  expires by itself. Overwriting `D+0x40` with a new signal does not delete the old one (only `0x53D760`
  deletes the one it points at). The emitter senses its own signal (no parent filter).
- Signal collider (table `0x8A5C40`): sphere (shape byte `+1` = 0), centre (0,0,0), flags `0x000C8000`.
- Chao colliders (table `0x13134D0`, 5 entries, installed `0x550334`):

| Idx | Kind | Shape | Centre | Radius / h | Flags (`+4`) |
|---|---|---|---|---|---|
| 0 | 0x91 | 1 (cylinder) | (0,2,0) | 15 / 4 | 0x00782008 |
| **1** | **0x81** | **0 sphere** | **(0,2,0)** | **1.5** | 0x00002400 |
| 2 | 0x92 | 1 | (0,2,0) | 5 / 4 | 0x00782008 |
| 3 | 0x95 | 0 | (0,2,3.7) | 2.8 | 0x00002410 |
| 4 | 0 | 0 | (0,2,0) | 1.4 | 0x00002410 |

- Hit records (`0x47EC50`): each object's CollisionInfo (`Data1+0x2C`) keeps **16** records at
  `+0x10 + 8i`: `+0` u8 **own** entry index, `+1` s8 other's entry index (−1 = end), `+4` other object.
  A record is not written into A when A's entry has flag 0x8 or B's entry has flag 0x4.
- Pair filter `0x485850`: broad sphere (`0x47EA90`), then for each entry pair `mask[A.id] & B.flags == 0`,
  `mask[B.id] & A.flags == 0`, neither flag 0x10 (`mask` = `0x171CA7C`; [3] = 0x80000, [9] = 0x400000:
  Chao entry 1 and the signal pass).
- Sphere/sphere `0x483170` (`0x485620` case 0/0): hit iff **`|cA − cB|² < (rA + rB)²`** (centres from
  `0x47F020`, i.e. object pos + rotated offset).
- **`0x486880(obj, 1, kind)`** returns the first record whose own index is **1** and whose other
  object's entry has that kind; `+4` = the signal task. `0x54B1F0` resets the iterator globals
  `0x19468A0..AC` first and returns `+4`. So a Chao **senses a signal iff its entry 1 sphere
  (pos + (0,2,0), r 1.5) overlaps the signal sphere: `|pos + (0,2,0) − S.pos| < R + 1.5`**. Hits come
  from the collision pass, which runs between object updates (a frame of lag).
- **Only entry 1 ever records hits in the Chao**: entries 0 and 2 have flag 0x8 (they push others
  but record nothing in the Chao) and entries 3 and 4 have flag 0x10 (skipped by the pair filter).
- `0x486760(obj, kind)` (touch): same walk but any own entry, and only if CollisionInfo `+4 & 1`
  (set when a record was written). Given the above, a touch is also **entry 1 (pos + (0,2,0), r 1.5)
  overlapping a collider of that kind** (e.g. fruit r 4.2 h 8, §6.1).

### 7.5 Rules for `Senses` (`src/chao_sim/picker.rs`)

Computed when the picker runs, from positions at that moment. "In view" = `d < 50` (3D) and
`|s16(rotY − bearing)| < 0x271C`, bearing as in §7.2.

| Method | Game rule | Server |
|---|---|---|
| `signal(kind)` | The live signal of that kind (any emitter, any Chao but see below) with `|chao.pos + (0,2,0) − S.pos| < S.R + 1.5`. Returns S.pos and parent = emitter (pos, current behaviour) for `0x55A920` signals, none for `0x55A850` ones (TV/radio). Order among several: the collision record order. | Signals from the Chao the server runs (kind, R, pos = pos + facing·8, frames, removed when the emitter changes behaviour), checked with the same sphere test; a Chao ignores its own. TV/radio (0xC9/0xCA, R 25.5, 3600 frames from the TV/radio position) depend on the toys' on/off state. |
| `player_seen()` | Player 1 in view. Value: bond index `0x53E0C0` = P1's `co2+1` if 1..5 else 0. | The game has one P1 per machine; the server has several players. A Chao's player is the nearest one whistling within 100, else the nearest; "seen" is the in-view test against that player. Characters 1..5 map to themselves, everything else (Sonic, walkers 10/11 = Tails/Eggman on foot, others) to 0. |
| `player_pos()` | `*0x1DEA6C0 + 0x14`, perceived or not. | The same player's position. |
| bond for bond_tick (`0x5A0490`) | `0x53E0C0()`, **regardless of perception** | `bond_tick` uses that player's character, seen or not (0 only with no player). |
| `nearest_fruit()` | Minimum-d fruit (ALW cat 3) in view. **Only the nearest is tested**: if its ALW `+0x40` is set and is not this Chao's entry, hunger returns (no second choice). `held` = fruit Data1 `+4 & 0x8000`. | Fruit position: on the ground, the item position; held by a player, the hand (≈ player position); held by a Chao, its hold point. |
| `tree_seen()` | List 3 found: **any of the 7 tree spots (cat 5, any state, including empty) in view**. | The only `tree_seen` reader that matters is go-to-bed `0x54EE10`, which needs the **nearest** tree in view: `d < 30.0` and Data1 `+0x31` (state) ∈ {3, 5}. If the nearest is an empty/young spot it fails even when a grown tree is farther, so `Senses::nearest_tree()` returns position, distance and state. Tree spots are fixed per garden (`0x1366B30 + (garden−1)·0x78 + slot·0xC`); states come from the referee. |
| `nearest_toy()` | Minimum-d ALW cat-6 object in view; kind = ALW `+4` (2 TV, 3 radio, 4 ball, 7, 8, 0). | Toys are fixed per garden except the ball (position from whoever owns it). |
| `touch(kind)` | `0x486760`: the Chao's entry-1 sphere (pos + (0,2,0), r 1.5) overlaps a collider of that kind (§7.4). | Uses the item colliders (§6.1 touch table). Ledge 0xCB is handled by `Near`. |
| `chao_count()` | `0x5319F0(ESI = garden)`: non-empty save slots of the garden (not perception). | Chao in the garden's records. |
| other Chao | List 1 is built and **never read**. Chao react to each other only through signals and the ALW partner/target links. | No "Chao seen" sense needed. |
