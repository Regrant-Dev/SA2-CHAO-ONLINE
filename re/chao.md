# Chao data

`ChaoData` is 0x800 bytes (`ChaoDataBase` 0x4DC + 0x324 unknown). The offsets below match
the community `SA2Structs.h` layout and decode a real `SONIC2B__ALF` save correctly.

| Field | Offset | Type | Notes |
|---|---|---|---|
| Name | 0x12 | u8[7] | game's own character table |
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
- `ChaoSlots` 0x19F6EC4: `ChaoData[37]` per the header; the mod uses the first 24 (8 per garden).
- `CurrentChaoArea` 0x134046C: 0 lobby, 1 neutral, 2 hero, 3 dark (`ChaoArea` enum).

## Save file

`SONIC2B__ALF` (64 KiB, header "SONIC ADVENTURE 2 BATTLE CHAO GARDEN DATA") holds the
24 slots at file offset 0x3AA4, 0x800 each. A fresh save has 4 eggs (type 1) in gardens
1, 1, 2, 3, with grades 0..4 and levels/points 0.

## CreateChao 0x5501D0

`ObjectMaster* CreateChao(ChaoData* data, int copy, int flag, NJS_VECTOR* pos, Angle angle)`,
cdecl, 1062 bytes. The mod calls it with `copy = 0, flag = 0`.

- `data == NULL`: the game allocates a fresh 0x4DC-byte data block and initialises a new Chao.
- `copy == 0`: the object keeps the caller's pointer (`work+0x5C = data`, `work+0x58 = 0`),
  so the buffer must outlive the Chao object.
- `copy != 0`: the game allocates its own 0x4DC bytes and copies 0x137 dwords from `data`
  (and passes NULL to FUN_00530750 instead of `data`). Only the first 0x4DC bytes are copied.
- `data[0x80]` (Type) == 1 (egg) → returns FUN_0057B9C0(0, data, copy, pos, 0), the egg spawner.
- `data[0x80] == 0`: if `data[0] == 0xFF` it logs "CreateChao Recieved Illegal Data" and returns 0;
  otherwise it sets Type = 2 (child) and initialises it.
- **Writes into `data`**: if DNA byte `0x438 == 0` it sets Garden (0x81) and generates DNA
  (FUN_00551DA0); and whenever `CurrentChaoArea (0x134046C) < 4` it sets Garden (0x81) to the
  current area. A spawned copy of another player's Chao therefore takes the host garden's
  number; the owner's save is unaffected.
- `flag != 0` → FUN_00536550() extra init (purpose unknown).
- `data[0xDD] == 3` → extra setup (FUN_00548CF0, FUN_00793880(work+0x144)); field unknown.

Unknown: the meaning of slots 24..36 of `ChaoSlots`, and the name character table.

## Garden unlocks (src/unlocks.rs)

- **Where they live:** the Chao save's flags dword at **0x19F646C** (+0xCA6C for the second Chao
  save; `0x173D06C` says which is in use; each reader picks it with `cmp eax,1`).
  - Bit 0x2 is set on first entering the Chao World and 0x4 the kindergarten (both at
    0x54C9EA/0x54CA22, in the lobby setup).
  - **0x10 = Hero Garden** and **0x40 = Dark Garden** are set in 0x568630, a Chao's
    evolution: a Hero or Dark evolution opens that garden (0x5687BC / 0x56882D). With both
    open, it also unlocks Steam achievement 7 (0x40E850, ECX = 7).
- **Who reads them:** the lobby makes the Hero gate only with 0x10 (0x54C162 → 0x57FCE0,
  "ALO_LobbyGateHeroExecutor", main 0x57F940) and the Dark gate only with 0x40 (0x54C1B2 →
  0x580140). 0x531A75/0x531A90 and 0x591536/0x59158E test them too.
- **It isn't a stage key.** The Chao Key in stages ("ObjectChaoKeyExec", main 0x6E9DE0, let out
  of Chao boxes, 0x6E9D10) only sets "got the key this stage" (`0x174B004`, `0x1AEFF64` =
  "ChaoKeyTask"). The stage's end then sends the player into Chao World (0x43C6BB: state 0xB,
  level 0x5A) and marks `0x1DEF829[character]` (0x444014). The Hero/Dark gardens come from a
  Chao's evolution.
- **Co-op:** party players share them (`Body::GardenUnlocks`, tag 58). A player's unlocks are
  sent when they change and every 10 s while in a party. A party player's open gardens are ORed
  into the local save's flags (kept the next time the game saves the Chao World), with a toast.
