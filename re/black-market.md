# Black Market stock and restock timer

Addresses are for the Steam `sonic2app.exe` (decrypted), base 0x400000. The jewel egg
additions to the same shop are in [jewel-eggs.md](jewel-eggs.md).

## Live data

| Address | Type | Meaning |
|---|---|---|
| **0x1DBED8C** | u32 | Next restock, as a `FrameCount` value |
| **0x1DBED90** | u32 | Stock count, 0..32 |
| 0x1DBEDA0 | 5 × u16 | Purchased items waiting for delivery (`{category:u8,id:u8}`) |
| 0x1DBEDAC | u32 | Count of purchased items waiting |
| **0x1DBEDC0** | 32 × u16 | Stock, packed `{category:u8,id:u8}` in offer order |
| 0x174B038 | u32 | `FrameCount`: the main loop (0x433FE7) adds the logic-step count 0x1DEB50C every frame. Not saved; it starts again at each launch. |
| 0x1DEC60E | u8 | Emblems, the gate for every offer |
| 0x1A556F4 | u32 | Battle DLC installed (Steam check at 0x40A67B) |
| 0x1DEB364/65/67 | u8 | One-time flags for the three category 16 items (ids 1, 2, 0); copied into the main save by 0x4448B0 |

Item categories have descriptors `{attributes*, count}` at **0x8A8728 + category × 8**:
1 eggs (0x8A7B68, 69), 3 fruit (0x8A8028, 24), 7 seeds (0x8A7FB8, 7), 9 hats (0x8A81A8, 85),
16 special items (0x8A86F8, 3). Each attribute is 16 bytes: u32 price, u32 sale price,
i16 emblem requirement, i16 name message, i16 description message, i16 unknown.

## Save layout

The stock lives in the Chao save region. With `M = 0x19F6E50 + (*0x173D06C == 1 ? 0xCA6C : 0)`
(the region base + 0x9F0):

| Offset | Live copy |
|---|---|
| M+0x1C u32 | next restock 0x1DBED8C |
| M+0x20 u32 | stock count 0x1DBED90 |
| M+0x24 u32 | waiting count 0x1DBEDAC |
| M+0x28 32 × u16 | stock 0x1DBEDC0 |
| M+0x68 5 × u16 | waiting items 0x1DBEDA0 |

- **0x52EE60** copies the save into the live data. Its only caller is the Chao World start
  (0x52B30A), so live stock outside Chao World is whatever was last loaded and is replaced on
  the next Chao World entry.
- **0x52EDE0** copies the live data back. Its only caller is the pre-save pass 0x52F090 (area
  changes, Chao file save, kindergarten exits), which then updates the checksum (0x52EEE0).

## Market task

- **0x1AED238** (u32) is the Kindergarten's room. The Kindergarten manager 0x54E3C0 sets it to 0
  when the Kindergarten starts; 0x54E7C0 (ECX room, EDX entry) requests another room (written
  by the lobby room 0x585B27 and room 3 0x5886E8), and the manager then runs the room switch
  0x54E280, which copies it to 0x1AED230. Outside the Kindergarten (Chao area 6) it keeps its
  last value.
- Kindergarten room 2 (switch 0x54E280 on 0x1AED238) loads task main **0x58BFC0** with
  0x470C00(2, ...). On its first run it allocates a 0xD8-byte work into `task+0x40`
  (display 0x58BFA0, delete 0x58BF60, which frees the work and clears `task+0x40`) and every
  frame runs **BlackMarketExec 0x58B5E0**(work). The mode is `work+0x04` (jump table 0x58BF24).
- Mode 0 loads the shop messages and models, then mode 1 waits up to 20 frames. Mode 1 is the
  only way into mode 2, so mode 2 runs once per visit.
- **Mode 2** (0x58B8D7) is the only restock trigger:
  1. `if (*0x1DBED8C <= FrameCount)` (unsigned) call the stock generator at **0x58B8EC**, and
     set `work+0x9A = 1`.
  2. If the Battle DLC is installed and the same condition holds: drop a trailing
     `{16,2}` offer, then append `{16,2}` when the stock has room, 0x1DEB365 is 0, emblems
     reach item 2's requirement and `rand()/32768 <= 0.1`.
  3. If the timer had expired, or it is more than 25200 (0x6270) frames ahead (a value from an
     earlier session), set `next = FrameCount + 10800 + (int)(rand()/32768 × 14400)`, i.e.
     3 to 7 minutes of 60 Hz steps. A far-ahead value is reset without restocking.
  4. Mode 3 (the shop menu) follows. Purchases and browsing index into 0x1DBEDC0 through the
     work (`work+0x9C` is the selected offer).
- Nothing else calls the generator. Stage clears, Chao World entry and saving don't restock;
  the timer runs on `FrameCount` everywhere (stages, menus, Chao World), so the stock changes on
  the first market visit at least 3 to 7 minutes of play after the last restock.

## Stock generator 0x589A00

No arguments, ignores its return value, no use of the work. It clears the count and rebuilds
the whole stock with the C runtime `rand()` **0x7A89D8** (the per-thread MSVC LCG, 0..32767;
`r` below is `rand()/32768`). Every append stops at 32 entries. Picks of "n distinct" use a
256-bit set on the stack and append through 0x5899A0(cat), which writes the set bits in
ascending id order (0x589960 pops the lowest bit).

1. **Eggs (category 1).** `set = (dword[S+4] + dword[S+8]) & 0x7FFFFFFF) % 3`, where S is the
   Chao save region base; S+8 is a random id written when the Chao save is created
   (0x532EA0 at 0x52F155). Ten rows of `{i16 emblems, i16 shiny emblems, i8 egg[3]}` at
   **0x8A3378**: 5/40 → 2,2,2; 10/50 → 6,9,1; 15/60 → 12,4,5; 20/70 → 8,3,11; 30/80 → 7,7,7;
   90/140 → 10,10,10; 100/150 → 9,1,6; 110/160 → 4,5,12; 120/170 → 3,11,8; 130/180 → 13,13,13.
   Each row the player has the emblems for gives `egg[set]`, plus 27 (the shiny egg) when the
   shiny threshold is met and `r <= 0.3`. Then `k = 1 + (int)(r × 5)` (1..5): all candidates
   when there are at most `k`, otherwise `k` random distinct ones.
2. **Fruit (category 3).** Pairs `{id, percent}` at 0x8A3548: 5/70, 6/70, 10/50, 11/50, each
   appended when `(int)(r × 100) < percent`. Then 2 distinct of {7, 8, 9} (0x8A2C7C).
3. **Seeds (category 7).** Seed 0 always; with `r <= 0.7` one of {1, 2, 3} (0x8A2EFC); then
   2 distinct of {4, 5, 6} (0x8A2FA4).
4. **Hats (category 9).** Candidates are ids 0..84 except 13 and 14 with a price above 0 and
   the emblem requirement met. All of them when there are at most 5, otherwise 5 distinct.
   Drawing hat 12 rolls `(int)(r × 31)`: below 5 gives 14 and below 15 gives 13, when their
   emblem requirements are met.
5. **Fruit (category 3).** Id 20 (0x8A3550), or 21 with `r <= 0.3`, when its emblem
   requirement is met and `(int)(r × 100) < 70`.
6. **Special (category 16), Battle DLC only.** Id 0 (0x1DEB367 clear, 100 emblems, 10000
   rings) and id 1 (0x1DEB364 clear, 140 emblems, 20000 rings), each with `r <= 0.1`. Id 2
   (180 emblems, 30000 rings) is added by mode 2 above, not by the generator.

The jewel egg feature replaces the CALL at 0x58B8EC; after the original generator it appends
at most one jewel egg (jewel-eggs.md).

## Rerolling outside mode 2

- The generator touches only the live count and stock and the game's `rand()` state. Running
  it from another thread uses that thread's own `rand()` state.
- Live stock written outside Chao World is replaced by 0x52EE60 on the next Chao World entry,
  and 0x52EDE0 doesn't run there, so a stock change outside Chao World doesn't reach the save.
- While the market task is in mode 3 or later, its work holds an index into the stock; the
  stock must not change under it.
- Setting `*0x1DBED8C` to a value at or below `FrameCount` makes the next visit restock through
  the normal mode 2 path (generator, jewel egg, special item 2, new timer). A value at most
  25200 frames ahead keeps mode 2 from restocking or resetting the timer.

## The mod's restock line and 10-minute restock

While Kindergarten room 2 is open and the game isn't paused, `src/market_refresh.rs` shows the
time to the next restock top right: `*0x1DBED8C - FrameCount` steps at 60 a second (or the
10-minute count's rest when that is on), and "New stock on your next visit" once the timer is
at or below `FrameCount` (mode 2 only restocks as the market opens).

With `BlackMarketRefresh` on (off by default), `src/market_refresh.rs` writes only `*0x1DBED8C`, in Chao World (level 90) while the Kindergarten
room isn't 2. Every 10 minutes of unpaused play it sets the timer to `FrameCount`, so
the next visit restocks through mode 2; otherwise it keeps the timer 3600..25200 steps ahead
(`FrameCount + 25200` when it isn't), so mode 2 neither restocks nor resets it. A set timer that
comes back ahead before a market visit (loaded from the save at the Chao World start) is set
again.
`FrameCount` starts again at each launch, so the timer can't carry a waiting restock across
sessions; the play toward the next mark and an earned restock not yet had are kept in the
mod's `market-refresh.txt` instead, and a kept restock is set again on the first Chao World
frame.
