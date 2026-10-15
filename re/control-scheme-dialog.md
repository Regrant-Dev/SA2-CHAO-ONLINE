# "Reboot to change the control scheme." dialog (SA2 Steam, sonic2app.exe)

Static analysis only (REA/Ghidra + objdump listing). Image base 0x400000.

## Message/text objects

Runtime text objects are built by `0x440730` (and `0x440440`, same layout) from
per-language string tables (switch on language byte `0x174AFD1`):

| Global       | Source (English)            | String |
|--------------|-----------------------------|--------|
| `0x1A55904`  | esi-table +0x1C (`0x174A86C`) | "Reboot to change the control scheme." |
| `0x1A55908`  | edi-table +0x0 (`0x174A870`)  | "An internet connection is required." |
| `0x1A5590C`  | edi-table +0x4 (`0x174A874`)  | "Installer needs to reboot ..." (DLC) |
| `0x1A55910`  | edi-table +0x8 (`0x174A878`)  | "Reboot now" |
| `0x1A55914`  | edi-table +0xC (`0x174A87C`)  | "Reboot later" |

`0x440730` ends with `mov [0x1A55944],1` (0x4408A7): "system-dialog resources loaded".
`0x440950..0x440AC1` frees them and clears `0x1A55944`.

## The system-dialog object

Singleton `*(void**)0x1AF19F0`, size 0x80, ctor `0x40A4C0` (lazily created in
GameLoop and elsewhere). Fields:

| Off   | Meaning |
|-------|---------|
| +0x64 | state: 0 idle, 1 idle-after-dialog, 2 "no save data/internet" msg, 3 DLC-installer reboot dialog, 4 **control-scheme reboot dialog** |
| +0x68 | frame counter (poll timer in states 0/1; 30-frame input lockout in dialogs) |
| +0x6C | cursor: 1 = "Reboot now", 0 = "Reboot later" |
| +0x70 | if nonzero, "later" returns to state 1 instead of 0 |
| +0x74 | request -> state 2 |
| +0x78 | request -> state 3 (DLC) |
| +0x7C | **"controller set changed" request -> state 4** |

Update/draw function: `0x40A8F0` (`__stdcall`, arg = object), called every frame
from GameLoop `0x433EE0` at `0x433FCB`.

## Q1: detection and how it opens / blocks

States 0 and 1 (`0x40A915`, `0x40A9CD`): every 61 frames:

```
0x40A92D call 0x402170            ; app singleton (*0x1AF19E4, ctor 0x4254A0)
0x40A932 mov ecx,[eax+0x28]        ; input-device manager (vtable 0x8FFE60)
0x40A937 call [vtbl+0x30]          ; = 0x865130 -> 0x864A00
0x40A93C test eax,eax
0x40A93E je 0x40A943               ; 74 03
0x40A940 mov [ebp+0x7C],esi        ; 89 75 7C  (esi=1)   <- arms dialog
...
0x40A9F8 je 0x40A9FD               ; 74 03   (same in state 1)
0x40A9FA mov [ebp+0x7C],esi        ; 89 75 7C
```

`0x864A00` enumerates XInput pads 0..3 with `XINPUT1_3` ordinal 4
(XInputGetCapabilities), builds a new device list, destroys the old list
(+0x54..+0x58), swaps in the new one, and returns 1 iff the count of
connected XInput pads changed. It is edge-triggered (the list is refreshed on
every poll), so a single plug/unplug yields one `1`.

If `+0x7C != 0` and state != 4: state = 4, calls `(*0x1A55764)->vtbl[0x16C](*0x1A55788)`,
+0x68 = 0, cursor +0x6C = 1 (default "Reboot now").

State 4 handler (`0x40ABD9`): requires `0x1A55944 != 0`; each frame calls
`0x436020` (sets `0x174AFD9 = 1` = game paused flag + pauses sound/video),
0x439060, 0x441BA0, 0x41B0C0(0x80), then draws via `0x43FFB0` with
esi = `*0x1A55904` (text) and edi = cursor. After 30 frames reads buttons
`0x1DEFAB0` / `0x1DEFAB4` (pressed masks; 0x441990 picks which):
0x10/0x20 move cursor (sound 0x8000), 0x04 confirm, 0x02 cancel.

**Blocking**: `0x40AD30` returns 1 when `0x1A55944 != 0` and state in {2,3,4}.
GameLoop: `0x433F74 call 0x40AD30; test eax,eax; jne 0x433F9D` -> skips
`GameModeHandler 0x434160` entirely (which runs `GameStateHandler 0x43A780`
and thus objects/players). So per static code the whole game mode logic is
skipped while the dialog is up, not just input. (The report that the stage
timer keeps running is not explained by this path; verify at runtime.)
`0x411AE0` (returns state != 0) is also used by `0x43B1D6`/`0x43B1EE`/`0x665119...`
to divert to `0x411B30` (forces state 2) in some menu transitions.

## Q2: button paths (state 4)

- Confirm (0x04) with cursor=1, **"Reboot now"** (`0x40ACA8`): play sound 0x8001,
  `ShellExecuteW(0,"open","Launcher.exe","-r",0,SW_SHOWNORMAL)` (`0x40ACC0` pushes
  L"-r" @0x89DD38, joins 0x40AB57), then `PostQuitMessage(0)` -> game exits,
  launcher restarts it. (State 3 / DLC uses "-q" @0x89DD68 instead.)
- Confirm with cursor=0 or Cancel (0x02), **"Reboot later"** (`0x40ACDF`):
  `call 0x435FD0` (if GameState `0x1934BE0 != 0x11` -> resume: `0x436040`
  clears `0x174AFD9`, resumes sound), sound 0x8009, `+0x7C = 0`,
  state = (+0x70 ? 1 : 0). Nothing input-related is changed: no device rebind.

## Q3: recommended mod fix

**Patch (runtime-toggleable, 1 byte each):** turn the two `je` that skip arming into
unconditional jumps, so the poll still runs (device list stays refreshed and the
change edge is consumed) but `+0x7C` is never set:

| VA         | file off | original | patched |
|------------|----------|----------|---------|
| `0x40A93E` | 0x9D3E   | `74 03`  | `EB 03` |
| `0x40A9F8` | 0x9DF8   | `74 03`  | `EB 03` |

(Equivalent: NOP `89 75 7C` -> `90 90 90` at 0x40A940 / 0x40A9FA.)
Write with VirtualProtect while online, restore `74` when offline. Toggling is
safe at any time because no persistent state is left armed.

If enabling while the dialog is already open (state 4), also close it like "later":
```
obj = *(u32*)0x1AF19F0
if obj && *(u32*)(obj+0x64) == 4 {
    *(u32*)(obj+0x7C) = 0;
    if *(u8*)0x174AFD9 != 0 { call 0x435FD0 (cdecl, no args) }  // resume
    *(u32*)(obj+0x64) = (*(u32*)(obj+0x70) != 0) ? 1 : 0;
}
```
This also works as a no-patch per-frame alternative (run before GameLoop; the
dialog sets state 4 inside 0x40A8F0 and only blocks from the next GameLoop's
0x40AD30 check). It does exactly what "Reboot later" does minus the sound.
(The vtbl[0x16C] call made on entering state 4 has no counterpart in "later"
either.)

### Side effects
- Same as choosing "Reboot later": the game does nothing else on a device
  change. `0x864A00` itself rebuilds Magic's XInput device list on each poll, so
  the low-level list is current; whether the per-player pad mapping follows a
  newly connected pad without restart is not confirmed statically (test).
- Only XInput connected-count changes trigger this; keyboard/DInput don't.
- Patch also suppresses the dialog in menus (not only in stages).

Confidence: high for detection/state machine/patch sites; medium for
"input rebinding after change" behaviour.
