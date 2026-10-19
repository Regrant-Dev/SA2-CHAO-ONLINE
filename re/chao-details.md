# Detailed Chao inspection

`src/chao_details.rs` formats owned record snapshots for the F9 **Chao details** view.
Snapshots come from the existing `game::read_chao_slot` path (memory after Chao World loads,
otherwise the save file). The menu rereads them each frame and retains a save-slot identity,
never an ObjectMaster/Data1 pointer. Holding a Chao initially selects its slot only when the
current registry identifies it as one of our own save records. No AI or mutation calls are
needed for inspection.

## Existing viewer reference

[Justin113D/SA2ChaoStatMenu](https://github.com/Justin113D/SA2ChaoStatMenu) provides an existing
in-garden held-Chao viewer. Its `ChaoData.h` and stat display identify fields useful for this
feature, including personality axes, emotions, symptoms and appearance. The repository had
no license listed when checked on 2026-10-19. Our Rust formatting, navigation and HUD drawing
are implemented independently with our existing menu and font; no third-party assets are used.

The reference supplies naming/interpretation, not proof that every hidden trait affects
vanilla gameplay. Display exact signed/unsigned values without assigning invented personality
categories or health diagnoses. The raw page preserves access to uninterpreted data.

## Native checks (REA / offline Ghidra VM)

`0x5353C0`, the native life/emotion tick, was decompiled from a staged code slice. It reads
the record via `ObjectMaster+0x34 -> Data1+0x5C` and uses emotion base `record+0x124`.
It increments clock `+0xA4`, age `+0x86`, adult age `+0x88`, adjusts maximum lifespan `+0x8C`,
and clamps remaining life `+0x8A` to the maximum. These are counters, not real-world minutes.

The native pass confirms:

- Emotion magnitudes at `+0x12C..+0x133` are unsigned bytes clamped to 0..200. Reading them
  as signed chars would incorrectly turn values above 127 negative.
- Need/condition fields at `+0x134..+0x148` are unsigned 16-bit counters clamped to 0..10000.
- Personality modifiers at `+0x14A..+0x156` use signed-byte arithmetic. Examples include
  `+0x14C`, `+0x14F`, `+0x150`, `+0x155` and `+0x156` influencing different rates.
- Six condition counters `+0x15A..+0x15F` move toward zero using signed-byte arithmetic.

The decompiler reports bad instruction/control-flow boundaries in part of this function;
use the observed operand widths and clamp blocks, not that incomplete pseudocode as an
exhaustive behavioral model. The inspection code does not execute or hook this function.

Native debug-label data in the dumped image includes `ANGER`, `SORROW`, `FEAR`, `PAIN`,
`RELAX`, `TOTAL`, `SLEEPY`, `SLPDPTH`, `HUNGER`, `BREED`, `TEDIOUS`, `LONELY`, `TIRE`,
`STRESS`, `NOURISH`, `CONDITN` and `THIRSTY` around `0x12E5BC4..0x12E5CCC`. These corroborate
the emotion/need layout. The `+0x148` field is called energy in the old viewer, but THIRSTY
in native debug data; our view labels it thirst/raw rather than assuming an energy mechanic.

Happiness `+0x82`, alignment `+0xB0`, tendencies `+0xA8/+0xAC`, evolution `+0xC0` and the
six-character bond table `+0x16C` were already verified in [chao-garden-items.md](chao-garden-items.md).
The two legacy form IDs 3/4 precede the normal forms: 5 is Neutral Normal, 6 Hero Normal,
20 Neutral Chaos and 23..25 Tails/Knuckles/Amy. Don't shift these by omitting the legacy IDs.

Appearance and personality field names also use the existing viewer as a reference. Some
fields are unused in vanilla; these are displayed as stored values/IDs, without speculative
effects. Eggs and invalid hidden-stat filler are shown as uninitialized, and nonfinite shape
floats are not drawn as valid gauges.

## Checks

Tests cover unsigned emotion 200, signed personality -80 and affection -100, 16-bit needs,
unchanged record bytes after inspecting every page, egg filler, NaN values, form IDs, stable
save-slot navigation, disappearing selections, and fitting 320x240 through 2560x1440.
Native HUD appearance and navigation still need an in-game check after updating.
