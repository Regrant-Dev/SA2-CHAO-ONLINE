# Chao data

`ChaoData` is 0x800 bytes (`ChaoDataBase` 0x4DC + 0x324 unknown). The offsets below were
measured by compiling `SA2Structs.h` with MinGW and reading `offsetof` values,
so they match the community header exactly. They are **not yet verified** against our exe.

| Field | Offset | Type | Notes |
|---|---|---|---|
| Name | 0x12 | u8[7] | game's own character table, not decoded yet |
| StatGrades | 0x28 | u8[8] | E..S = 0..5 (X = 6 with mods) |
| StatLevels | 0x30 | u8[8] | 0..99 |
| StatPoints | 0x38 | i16[8] | |
| Type | 0x80 | u8 | `ChaoType`: 0 empty, 1 egg, 2 child, 3..0x19 adult forms |
| Garden | 0x81 | u8 | `SA2BChaoGarden`: 1 neutral, 2 hero, 3 dark |
| Happiness | 0x82 | i16 | |
| Lifespan | 0x8A | i16 | |
| Reincarnations | 0x8E | i16 | |
| Alignment | 0xB0 | f32 | |
| EvolutionProgress | 0xC0 | f32 | |
| Color | 0xD8 | u8 | |
| Shiny | 0xDB | u8 | |
| Emotion | 0x124 | struct | |
| SA2BCharacterBonds | 0x16C | struct[6] | |
| DNA | 0x438 | struct | |

## In memory

- `ChaoSaveStart` 0x19F6460: start of the Chao save region (the `SONIC2B__ALF` file is a copy of it).
- `ChaoSlots` 0x19F6EC4: `ChaoData[37]` per the header; we read only the first 24 (8 per garden).
- `CreateChao(ChaoData*, int, void*, NJS_VECTOR*, Angle)` 0x5501D0, cdecl. The game keeps the
  `ChaoData*`, so the buffer must outlive the Chao object.
- `CurrentChaoArea` 0x134046C: 0 lobby, 1 neutral, 2 hero, 3 dark (`ChaoArea` enum).

## Open questions (need RE)

- What do `CreateChao`'s 2nd/3rd args mean? We pass `0, null` (common in mods); check `SpawnAllChaoInGarden` (0x531B10).
- Does the save routine only copy the save region, or does it walk live Chao objects back
  into slots? If it walks objects, visitors could end up in the host's save.
- Is the slot count 24 or 37 (37 may include transporter/temporary slots)?
- The name character table.

## Save file check (2026-10-14)
`SONIC2B__ALF` (64 KiB, header "SONIC ADVENTURE 2 BATTLE CHAO GARDEN DATA") holds the
24 slots at file offset 0x3AA4, 0x800 each. A fresh save decoded with the offsets above:
4 eggs (type 1), gardens 1,1,2,3, grades 0..4, levels/points 0. Consistent.

## CreateChao 0x5501D0 — decompiled (Ghidra, unpacked exe, 2026-10-14)
`ObjectMaster* CreateChao(ChaoData* data, int copy, int flag, NJS_VECTOR* pos, Angle angle)`,
1062 bytes. Confirms the header signature and what our F6 path relies on:

- `data == NULL`: game allocates a fresh 0x4DC-byte data block and initialises a new Chao.
- `copy == 0` (what visitors.rs passes): the object keeps **our pointer** (`work+0x5C = data`,
  `work+0x58 = 0`). So the buffer must outlive the object — the leaked pool is required.
- `copy != 0`: game allocates its own 0x4DC bytes and copies 0x137 dwords from `data`
  (and passes NULL to FUN_00530750 instead of `data`). Possible alternative to the pool;
  untested, and only the first 0x4DC bytes travel.
- `data[0x80]` (Type) == 1 (egg) → returns FUN_0057B9C0(0, data, copy, pos, 0), the egg
  spawner. So eggs *could* visit through the same call; we currently skip them.
- `data[0x80] == 0`: if `data[0] == 0xFF` logs "CreateChao Recieved Illegal Data" and returns 0;
  otherwise sets Type = 2 (child) and initialises it.
- **Writes into `data`**: if DNA byte `0x438 == 0` it sets Garden (0x81) and generates DNA
  (FUN_00551DA0); and whenever `CurrentChaoArea (0x134046C) < 4` it sets Garden (0x81) to the
  current area. A visitor's garden byte therefore changes to the host garden — only in our
  copy, never in the owner's file or ChaoSlots.
- `flag != 0` → FUN_00536550() extra init (unknown); we pass 0.
- `data[0xDD] == 3` → extra setup (FUN_00548CF0, FUN_00793880(work+0x144)) — unknown field.
