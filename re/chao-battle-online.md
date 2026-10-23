# Chao battles: trainers from the GBA ROM and battles between two games

Facts for battles against the GBA game's trainers and live battles between two players' games.
ROM offsets are for the US Emerald ROM (`BPEE`, revision 0) as file offsets; a ROM pointer is
the offset + 0x08000000. Struct names are the pret decompilation's. Everything here is read from
the player's own ROM at runtime; nothing from it is kept in the repo.

---

## 1. The battle engine and two games

- `src/battle/` is the whole battle: no game memory, no floats, no hash maps. `Battle::new`
  takes both teams as `Fighter`s, the type chart, the rules and a `u64` seed; every random roll
  comes from that seed's `Rng` (xorshift64*). The same teams, the same seed and the same choices
  give the same events, in the same order, on any machine.
- The side order matters: side 0 and side 1 draw their rolls in a fixed order (turn order
  tie-breaks are drawn per fighter in field order: side 0's first slot, side 1's, side 0's
  second, side 1's). Two games running one battle must build it with the same side order; a game
  whose player is side 1 shows the battle from the other side rather than swapping the teams.
- What a battle reads from the ROM (`team::Dex`): every move record, the type chart, the move a
  fighter uses with no PP left and the sound-move list. Names and descriptions only show on the
  HUD. Two games with the same mod version run the same engine code (players need the same mod
  version to join a lobby), so the ROM's battle data is the only input that can differ.
- A Chao's fighter comes from its record (stats, grades, happiness) and its battle profile
  (types, ability, four moves: `gba_link::battle_profile`, by `ChaoData::identity`). The name
  only shows on the HUD.
- `Fighter`, `Field` and the engine's other state types derive `Debug` without addresses or
  floats, so the same battle state prints the same text in both games.

## 2. Trainer records (`gTrainers`, 0x310030)

40-byte records, ids 1..=854 (id 0 is empty); the layout is in `src/gba/world.rs`. Byte 3 is the
trainer picture id (section 6). The party entries' `iv` (0..=255) scales to 0..=31 as
`iv * 31 / 255`, the same for all six stats.

### What the game gives a trainer's creature (pret `CreateNPCTrainerParty`)
- No EVs; the fixed IV above; the party level.
- A personality value per party member: a start value (0x80 for a double-battle trainer, else
  0x78 for a female trainer, else 0x88), plus `hash << 8`. `hash` is a running sum that is not
  reset between party members: for each member in party order, the trainer name's raw bytes up
  to the 0xFF terminator are added, then that member's species name's raw bytes (11-byte names
  at 0x3185C8 + species * 11, ending at 0xFF). The sum wraps at 32 bits.
- Ability: the species' second ability when `personality & 1` is set and the species has one
  (`Species::abilities[1]` non-zero), else the first.
- Nature: `personality % 25`. Nature n raises stat `n / 5` and lowers stat `n % 5` by 10%
  (rounded down), in the order attack, defense, speed, sp. attack, sp. defense; the two are the
  same stat (no change) when `n / 5 == n % 5`. HP is never changed.
- Friendship: the species record's base friendship (byte 0x12 of the 28-byte record at
  0x3203CC + species * 28).
- Held items (party flag 2) and the trainer's own items (four `u16` at record +0x10) are healing
  and battle items the game's AI uses; a battle without items leaves both out.
- Moves: the party's own four, or the last four the species learns by its level.

### Trainer classes
Class ids (record byte 1, names at 0x30FCD4):

| class | trainers |
|---|---|
| 0x20 | gym leaders: 40 records. The 8 lowest ids are the first battles, in gym order, with rising party levels; the 32 others are their rematches (section 3) |
| 0x1F | the four league trainers, fought in id order, each with one record |
| 0x26 | the champion, one record |
| 0x3A..=0x40 | the battle facility leaders, one record each |
| 0x41 | two placeholder records (the last ids) |

One gym leader record (the seventh) is a double battle (record byte 0x18 = 1) with a party of 4.
The league trainers and the champion fight single battles. Every other class is a route or story
trainer.

## 3. Rematches (`gRematchTable`, 0x5500A4)

78 rows of 16 bytes: 5 `u16` trainer ids (the first battle, then the rematches in order), map
group `u16`, map number `u16`, 2 bytes of padding. The map is where the trainer stands
(`Rom::map_name` gives its name). Rows for trainers without rematches repeat the same id five
times. The ids in columns 2..5 that differ from column 1 (291 ids) are rematch records: same
trainer, stronger party.

Route trainers without the classes in section 2 and without rematch records are 541 records;
17 of them repeat another record's class, name, species and levels exactly (the same trainer met
in two places), leaving 524. Their highest party level runs from 3 to 78. Ranked against the
first-battle gym leaders' highest party levels, 93 are at or below the first leader's, 67 more
at or below the second's, and so on; two are above the last leader's (the higher of them is
above the champion's).

## 4. Gym leader TM gifts (event scripts)

The TM a gym leader gives is in their gym's map script: after the battle the script gives the
item with `giveitem`. Event script bytes (pret `data/script_cmd_table.inc`, `asm/macros/event.inc`):

| bytes | command |
|---|---|
| `5C type id:u16 local:u16 ptr...` | `trainerbattle`; the pointer count depends on `type`: 0 → 2, 1 → 3, 2 → 3, 3 → 1, 4 → 3, 5 → 2, 6 → 4, 7 → 3, 8 → 4 |
| `1A 00 80 item:u16 1A 01 80 n:u16 09 00` | `giveitem item, n` (`setorcopyvar 0x8000, item; setorcopyvar 0x8001, n; callstd 0`) |

Types 5 and 7 are rematch battles. For each of the 8 first-battle leader ids, the ROM holds
exactly one `trainerbattle` with that id, a non-rematch type, `local` 0 and every pointer
argument a ROM pointer (0x08000000..0x09000000); the first `giveitem` of a TM item within 0x200
bytes after its arguments is the leader's gift (most gyms' scripts give it in two places: after
the battle, and again when the leader is spoken to and the bag was full the first time). TM items are item ids 289..=338 (TM01..TM50); item
id − 289 is the index into the TM/HM move table (`Rom::tmhm_move`). The 8 gifts are 8 different
TMs. The league trainers and the champion give no TM.

## 5. Badges

The trainer card's badge sheet: LZ77 at 0x56F5CC (unpacks to 0x400 bytes, 4bpp, 16 tiles wide
and 2 high: 8 badges of 16×16, badge k in tile columns 2k and 2k+1), its 16-colour palette
uncompressed at 0x56F4EC (32 bytes, colour 0 transparent). Badge k is the one the k-th gym
leader (in gym order) gives.

## 6. Trainer pictures

- Pictures (`gTrainerFrontPicTable`, 0x305654): 93 entries of 8 bytes {pointer to LZ77 4bpp
  tiles, `u16` size (0x800 for one 64×64 frame, 0x1000 for two), `u16` tag = index}. The tiles
  are 8×8 tiles row by row, as the creature pictures.
- Palettes (`gTrainerFrontPicPaletteTable`, 0x30593C, right after): 93 entries of 8 bytes
  {pointer to an LZ77 16-colour palette, `u16` tag = index, 2 bytes}.
- A trainer's picture id is record byte 3.

## 7. The host and messages between two players

- Clients only talk to the host. The host passes each `Game` message through its referee
  (`net/referee.rs`, `Referee::observe`) before relaying it to everyone else; `observe`
  returning false keeps it from the others. The referee's own messages (`Out`) go to everyone
  (`To::All`) or one player (`To::One(id)`), reliably, with a stand-in `from` when the receiver
  would otherwise see its own id. A hosting player's own messages go through the same referee
  (as id 0), and referee messages to id 0 become its own events.
- The dedicated server has no ROM and no game: the referee can order, hold back, compare and
  time messages, and make random numbers (`RandomState`-seeded, as the link epochs are), but
  cannot run a battle.
- `Referee::left(id)` runs when a player drops (Bye, or 6 s without a heartbeat); every other
  player also gets `PeerLeft`. `Referee::tick` runs on every poll of the host.
- A reliable message is at most 32 fragments of about 1180 bytes each (about 37 KB). A Chao record is
  0x800 bytes, so six records with their battle profiles (11 bytes each: types ×2, ability,
  moves ×4) are about 12.4 KB.
- `PlayerState.flags` has no free bit; 32 (`POSE_KARATE_BUSY`) marks a player as busy at
  karate, and karate's opponent list leaves busy players out.
