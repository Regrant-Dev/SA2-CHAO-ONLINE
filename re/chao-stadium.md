# Chao Stadium: Karate gate and Chao Race opponents

Steam `sonic2app.exe`, image base 0x400000. Found with Ghidra and the objdump listing.

## 1. Why the Stadium only offers Race (Karate gate)

**Karate is gated on Steam ownership of the "Battle" DLC. It is not a save flag and has nothing
to do with game progress.**

- `HaveBattleDLC` is the dword at **0x1A556F4** (named in SA2Variables.h). A copy is also stored
  at 0x1A556F8.
- It is written once at startup, at **0x40A67B**: `ISteamApps` vtable+0x18 (BIsSubscribedApp/DLC
  query) is called with AppID **0x3532C (217900)**. The result goes through `movzx eax,al` and is
  stored to 0x1A556F4 and 0x1A556F8. Nothing in the save data is involved.
- Stadium hub menu (`AL_CaveMenuMaster`; create 0x581750, exec **0x581870**, free 0x581A30;
  work struct at `*0x1AED6B0`, cursor at `+0x1C` with values 0..2):
  - 0x581891 `mov ebp,[0x1A556F4]`. When it is 0, pressing left or right skips cursor 1
    (0x5818A4..0x5818AE and 0x5818D6..0x5818E0). Karate can then never be selected.
  - Confirm (0x5818F7): cursor 0 = Race (`NextChaoArea`=5 Entrance, path at 0x58194E). Cursor 1 =
    `call 0x52B5B0(10)` = ChaoArea_Karate (0x581935). Cursor 2 = `0x52B5B0(1)` exit.
  - Drawing: 0x5A86B0 (the arrow/icon display of the objects created by 0x5A89C0) checks
    0x1A556F4 at 0x5A8828 and 0x5A894D. These hide the Karate icon and arrow (sprite 0x1B) when
    there is no DLC.
- The Karate module itself only checks the flag once, at 0x542963, to gate a Steam achievement
  (id 0xE). Karate assets (`al_stg_karate_*.prs` and others) are in the base install.

**What this means for the mod:** Karate (and therefore online Karate fights) works only for
players who own the Battle DLC. Treat 0x1A556F4 as read-only. The mod can read it to detect
ownership and hide or disable Karate features for players who don't own the DLC. These notes
deliberately leave out a plan to force the flag or patch the menu skip, because that would
unlock paid DLC content without a purchase. Players need to own the Battle DLC to see Karate
in the Stadium.

## 2. Chao Race: how racers are created

All 8 racers, including the CPU ones, are **ordinary 0x800-byte ChaoData records**, each
spawned with `CreateChao`. No special CPU mode exists, so adding a remote player's Chao only
means putting a real record into a slot.

### Key data
| Address | Meaning |
|---|---|
| 0x1A5CC00 `ChaoData* racers[8]` | Lane list. `[0]` is the player's Chao (a pointer into the save chao array, set at 0x5535AF from the entrance selection 0x1DBEE80). `[1..7]` are the CPU racers. |
| 0x1DB9540 `ChaoData cpu[7]` (0x800 each, ends at 0x1DBCD40) | Scratch buffer that holds the generated CPU racers. Not in save data. |
| 0x11CE2A0 | CPU racer templates, **0x54 bytes each**, indexed by opponent id. |
| 0x1A0FBD3 / 0x1A0FBD4 / 0x1A0FBD5 (bytes) | Race category (0 beginner, 1 jewel, 2 challenge, 3 hero, 4 dark, 5 party/multiplayer) / course / level. |
| Course descriptors, 0x24 bytes each | `+0` shuffle flag (0 = shuffle lane order), `+4` featured "rival" index, `+8..+0x20` = 7 template ids (dwords). Tables are 0x11D0888 and 0x11D0A38 (beginner/jewel, picked by 0x1A0FBD4/5), 0x11D0E70 (via 0x1354B70), 0x11D1020 and 0x11D10B0. |
| 0x1DBE5A0 `ObjectMaster* racerObj[8]` | Spawned Chao objects. |
| 0x1312F00 | Lane start positions, read by the spawn loop. |

### Functions
- **0x595520** `BuildRaceChao`, custom regcall: **ECX = dest 0x800 record, EAX = src 0x54
  template**. It memsets dest to zero, sets `+0x438 = 1` (gene block marked initialized), then
  fills in the type, stats, name and so on.
- **0x596060** `GenerateCpuRacers`, **EBX = course descriptor**. It shuffles 0..6 when `+0` == 0,
  then `for k in 0..7: BuildRaceChao(0x1DB9540 + k*0x800, 0x11CE2A0 + desc[2+perm[k]]*0x54)`.
- **0x596140** `GenerateCpuRacersForCurrentCourse()`, no args. It switches on 0x1A0FBD3 and
  calls 0x596060. It is also called at 0x557105 (entrance rival preview, single record 0x1A26D88).
- **0x558690** `RaceSpawnRacers` (race start):
  ```
  if (raceCategory != 5) {
      call 0x596140                         ; @0x5586A0  <-- hook point
      for k=1..7: racers[k] = (cpu[k-1].type(+0x80) >= 2) ? &cpu[k-1] : NULL   ; 0x5586A5..0x558738
  }
  clear racerObj[0..7]
  for i=0..7: if racers[i]:
      racerObj[i] = CreateChao(racers[i], 0, 0, &pos, randAngle)   ; call @0x558830
      ; cdecl, 5 stack args, caller pops 0x14
      ; pos from lane table 0x1312F00, then sets ObjectMaster flags (+0xA0 |= 0x2000) etc.
  ```
- Party race (category 5, menu code around 0x553A2C..0x553A94) skips CPU generation. It sets
  `racers[0,1,6,7] = NULL` and `racers[2..5]` = the up to 4 players' real records. This is the
  game's own way of putting real records into races.

### Recommended hook: replace CPU racers with remote players' Chao
1. Hook the **5-byte `call 0x596140` at 0x5586A0** (`E8 9B DA 03 00`). Redirect it to a stub
   that first calls the original 0x596140 (no args, no return value, callee preserves EBX, ESI
   and EDI). The stub then copies each remote player's 0x800-byte ChaoData over
   `0x1DB9540 + k*0x800` (k = 0..6, which is lane k+1). Leave slots without a remote player as
   CPU racers.
2. Make sure the copied record has `+0x80` (type) >= 2 (a child or adult, not an egg). Otherwise
   0x5586A5.. sets that lane to NULL. Keep `+0x438` non-zero. A real record from another save
   already has it set, which stops CreateChao (0x550478) from treating it as a new Chao and
   re-initializing its genes and garden.
3. Nothing else is needed. The existing loop at 0x558830 spawns every lane with
   `CreateChao(rec,0,0,&pos,angle)`, the same way as the player's own Chao.
   - Alternative: hook after 0x558738 (before the loop at 0x55876B) and write your own
     `ChaoData*` pointers into 0x1A5CC04.. directly. Copying into the static buffer is simpler
     and the record stays valid for the whole race.
4. For determinism across clients, every client must put the same records in the same lanes.
   The lane shuffle in 0x596060 uses `rand` (0x7A89D8), and so does the spawn angle. Write the
   remote records **after** the original call so the shuffle doesn't matter, and assign lanes
   by player id.

Caveats (not verified): race-end code may write results or medals only to `racers[0]`
(the player's record). The CPU buffer is scratch space and is regenerated by 0x596140, so
anything written into a copied remote record is discarded and never saved. Race AI and speed
come from the record's stats, so a remote Chao races with its own stats.

Confidence: Karate gate, high. Race slot and buffer layout, high (read straight from the
disassembly). Descriptor field meanings beyond `+0` and `+8..`, medium.
