# Shared wild visitors, link machines in shared gardens, and battling a visitor

How wild visitors and link-machine partners work in a garden the server runs, and how a
player battles a visitor in the creature battle scene. Visitors in a garden the player has
alone, the animal objects, absorbing and carrying are in `chao-wild-visitors.md` and
`chao-animals.md`; the server's garden in `chao-garden-sync.md`; the battle scene in
`chao-battle-arena.md`. Addresses are the Steam `sonic2app.exe` (image base 0x400000).

Code:
- `src/chao_sim/wild.rs`: the pure visitor helpers both sides use (`Rng`, `Spots`, `Swim`,
  `walk_toward`, `land_spot`, `in_pond`, `swim_y`, `to_pond`, `Roam`, `WATER_Y`, `CALL_*`,
  `MAX_VISITORS`, `RESERVE`). `creature_link::wild` uses them from there.
- `src/net/wild_host.rs`: the server's visitor lists (`WildHost`, owned by `ChaoHost`, fed from
  `ChaoHost::observe`, `frame` and `left`).
- `src/creature_link/shared.rs`: the game's copies of shared visitors and what the player does with
  them. `src/net/wild_print.rs`: the visitor fingerprint (`visitor_print`, also used by
  fakepeer); the game works it out once per catalog on a background thread.
- `src/battle/wild.rs`: the trip from the garden into the battle scene and back, and the wild
  foe. `src/creature_link/wild_battle.rs`: the card's battle button and asking the host (the game's
  copies of shared visitors tell it which can be battled, `wild_battle::set_shared`).

The server never has the catalog. Everything it keeps and sends is ids (species, form, ability,
move, habitat) and positions; every game draws, names and checks them from its own catalog.

---

## 1. When a garden's visitors are shared

A garden's visitors are shared exactly while the server runs that garden (`ChaoHost.runs[i]`
is `Some`; on the game's side `garden::server_runs()`). Every other garden is the player's own,
as in `chao-wild-visitors.md`.

| Garden as the player's game sees it | Visitors | Link machine |
|---|---|---|
| Alone (no friends, not run by the server) | local (`creature_link::wild`) | local |
| Friends there, not yet run by the server (its ground still uploading) | none from others; the first player's own stay up to `HANDOVER_SECS` (10 s), then go | hidden |
| Run by the server, SharedVisitors on, print matching | copies of the server's list | shown, used for oneself |
| Run by the server, SharedVisitors off, no catalog, or another print | none | hidden |

A garden the server runs with only one player in it (held Chao of absent players waiting
there, `chao-garden-sync.md`) is shared in this sense too: that player is its picker, asked as
soon as their game is settled in it. A garden with only one player and their own Chao is never
run, so its visitors are that player's own from the start.

### The visitor fingerprint (`net/wild_print.rs`)
`visitor_print(rom) -> Option<u32>` (never 0) is FNV-1a 32 over:
- `battle::online::dex_print` (move table, type chart, rules, sound moves);
- every species record's types, base stats, abilities, exp yield and growth rate;
- every species' level-up learnset;
- every habitat's wild tables (`Rom::wild_maps`) with its section id (`Rom::map_section`, the
  type id), which covers every area of `rotation::areas`.

Names and pictures are left out: each game shows its own. The print is worked out once per
catalog on a background thread and kept beside `Link.rom`. Two games with the same print build
the same visitor, the same card numbers, the same partner and the same battle foe from the same
ids.

### What each player reports
`WildHello { flags, print }` goes to the host on connecting, once the print is known, and
whenever the catalog or a setting changes:
- flag 1 (*see*): SharedVisitors on and the catalog loaded with its print.
- flag 2 (*pick*): also WildVisitors on: the player can choose wild visitors for a garden.
- flag 4 (*machine*): also LinkMachine on.

A player without flag 1 is never asked, never sent a list, and their Chao never learn from a
shared visitor. Their game behaves in shared gardens as it does with the switch off: no
visitors and no machine.

---

## 2. The server's visitor list (`wild_host.rs`)

### Per garden (`WildGarden`, one per garden 1..=3)
| Field | Meaning |
|---|---|
| `fingerprint` | the run's ground fingerprint (`Run.fingerprint`); a different one drops the list |
| `spots` | `Spots::read(&run.world.garden)`, rebuilt when the fingerprint changes; land spots within `MACHINE_CLEAR` (8) of the machine spot are left out |
| `machine` | `machine::find_spot(garden, tris, tree_spots, toys)` on the same ground (pure, so it is the spot every game finds) |
| `print` | the print of every visitor in the list; set by the first visitor added, cleared when the list is empty |
| `section` | the garden's wild area in the host's clock window (a habitat's section id, with the print it was picked with), `None` until the window's first pick |
| `cap` | 6..=7, drawn once per run (`MIN_CAP..=MAX_VISITORS`) |
| `came` | wild visitors added this run |
| `next` | the frame the next visitor is due (`NEXT_SECS` 20..50 s apart; `ON_ENTRY` = 4 right at the start) |
| `ask` | the outstanding `WildAsk` (serial u16, the asked player, how many land and pond, sent at), at most one |
| `list` | the visitors |
| `frozen` | the run ended while a battle reservation in it is open: only the battled visitors are kept (section 6) |
| `kept` | frozen with no reservation left since then (kept `KEEP_FROZEN`, 3 min) |

### Per visitor (`WildVisitor`)
| Field | Meaning |
|---|---|
| `id` | u16 serial from the host's one counter, 1..=0xFFFF wrapping, never 0, never one in use |
| `creature` | species u16, form u8, level u8, ability u8 (the ability id, worked out by whoever made it with `creature_link::ability_of`), shiny bool, moves [u16; 4] |
| `origin` | `Wild`, or `Partner { bringer: player id, key: u64 }` |
| `at`, `angle` | world position and facing (BAMS, low 16 bits) |
| `born`, `stay` | when it came; how long it stays (`STAY_SECS` 300..480 s; partners: until taken) |
| `place` | `Roam(Roam)`, `Swim(Swim)`, `Held { by, offset, heard, let_go }` or `Battle { by, since }` |

### Room
- Wild visitors: at most `scaled_cap(cap, players in the garden)`: the run's `cap`
  (`MIN_CAP..=MAX_VISITORS`, 6..=7) plus 3 for each player after the first, at most
  `MAX_SHARED_VISITORS` (20).
- All visitors: wild + partner + the category-2 items the referee knows in that garden
  (`ChaoHost.items`, animals and drives) + `RESERVE` (2) ≤ 32, the category-2 limit
  (`0x8AB840`, which the mod raises from the game's 10). A list has at most 30
  (`MAX_WILD_LIST`). The first ask turned down for room is logged.
- A partner that comes with no room left sends the roaming wild visitor farthest from every
  player away first (as `send_one_away` does alone). With none to send away it is refused.

### Movement (every server frame, 60 a second)
- **Roam** (land): stands 60..240 frames, then walks at 0.15 a frame (the game's animal speed,
  `0x165DFD8`) to a land spot within 30 units, picked by the garden's `Rng`. Its height is the
  highest solid floor under it (`land::probe` from y 60 and 180, as `Spots::read`). A step onto a
  grid cell of value 9 (water) turns it to a new goal instead, as the game's own animal does
  (0x57BD40). Players closer than 3 push it aside (`push_out`).
- **Swim** (pond): `Swim::step` on the pond cells; its height is the surface (the copies bob
  it with `swim_y`).
- **Called**: a player whose `PlayerState.action` is 42 (whistle) calls every roaming or
  swimming visitor within `CALL_REACH` (100) for `CALL_SECS` (6 s, starting again while they
  whistle). The nearest whistler wins. Land visitors walk with `walk_toward` (0.3 a frame, to
  within `CALL_NEAR` 4); swimmers go to the pond cell nearest the whistler.
- **Held**: at `in_hands(holder position, rotation, offset)`, as a held Chao.
- **Let go**: follows the holder's let-go reports (world position) until `WildPut`; lists show
  it on the ground (place 0) at that position.
- **Battle**: stands where it was.

### Coming and going
- When `next` is due, wild visitors < `cap`, there is room, and no ask is out, the server asks
  the picker (section 3) for one visitor, `to_pond` (fewer than 3 swimming, the garden has pond
  cells, 3 in 10) deciding land or pond. At the start of a run it asks for `ON_ENTRY`.
- The server places what it gets: land visitors on a `land_spot` at least `COME_FAR` (25) from
  every present player when a spot allows, pond visitors at a pond entry (`pond_entries`).
- A wild visitor whose `stay` is over goes once every present player is at least `GONE_FAR`
  (60) away and it is roaming or swimming. One that falls below `LOST_Y` (−50) goes at once.
- A partner visitor goes when its bringer leaves the garden (a settled `PlayerState` elsewhere,
  or disconnecting), turns *see* off, or the run ends. It is still waiting in the bringer's
  store, so it comes again on their next visit.
- When the run ends the list is dropped (unless frozen, section 6). The games then go back to
  their own visitors (section 4, "Shared → alone").

### The area and the clock
- A garden's area follows the host's clock: window `slot` = Unix seconds (UTC) / 600
  (`rotation::slot_of`), worked out every frame (`WildHost.slot`). Every `WildAsk` carries the
  host's window; with no section the picker picks from `rotation::area_at(areas, garden, slot)`
  with that window, so the area never depends on the picker's own clock, and it is the same
  area a player alone in that garden has in that window (`chao-wild-visitors.md` §6).
- The first pick answering an ask of the current window sets `section`; later asks name it.
- When the window changes the host forgets every garden's `section` (logged), and nothing else:
  the run, `came`, its visitors and an ask that is out stay. The next ask has no section and the
  new window, and lists say 0xFF meanwhile.
- A pick answering an ask from the window before (asks last at most 5 s) is taken as usual, its
  visitors added, but doesn't set `section`, so an old area is never kept into a new window.
- A new run (the garden going shared again, another ground) starts with no section; its first
  pick is the window's area again, so a run restarting doesn't change the area by itself.

### Hand-offs between players
- **Picker**: of the players present in the garden (`present_in`) and settled there
  (`Seen::settled`: their last state isn't `POSE_LOADING`; a game still loading in drops an ask
  for a garden its `shared.rs` doesn't see it in yet) with *see* and *pick* and a print equal to the garden's (any print while the list is empty), the one
  with the earliest `Player.since` (time in that garden), the lower id on a tie. It is worked
  out every frame, so when the picker leaves the next one takes over at once. The list, the
  section and the cap stay as they are. An ask to a player who stopped being picker is dropped
  and asked again of the new one.
- **Ask timeout**: an ask unanswered for 5 s (`ASK_LIMIT`) is dropped and asked again 1 s
  later (`ASK_AGAIN`): the visitors it was for are still due.
- The keeper of trees and toys (`keeper.rs`) is a separate role; the visitor picker doesn't
  follow it.

---

## 3. Messages

Players with a different `PROTOCOL_VERSION` are turned away by their Hello. The shared-visitor
bodies are game bodies, tags 73..=86. The referee consumes every to-the-host body (never passed on) and drops
the from-the-host ones when a player sends them; a game takes the from-the-host ones only from
`SERVER` (0xFF), as `ItemSteer`.

A **creature** on the wire: species u16 (1..`SPECIES_COUNT`), form u8 (≤ 31, an ordinary form),
level u8 (1..=100), ability u8 (< `ABILITY_COUNT`), shiny u8 (0/1), moves u16×4 (each <
`MOVE_COUNT`; at least one not 0; no id twice). 14 bytes. Positions are f32 (finite,
|v| ≤ 1e6, `MAX_COORD`); angles u16 BAMS.

| tag | body | sent | fields and checks |
|---|---|---|---|
| 73 | WildHello | to the host, reliable | flags u8 (bits 1, 2, 4 only), print u32 (0 with flag 1 clear) |
| 74 | WildList | from the host, every 6 frames, unreliable | area u8 (1..=3), serial u16 (one more for each list the host sends), section u8 (0xFF none yet), print u32, picker u8 (a player id, 0xFF none), learners u16 (bit per owner id 0..=15: their Chao can learn now), count u8 (≤ 30), count × entry. Entry: id u16 (not 0), creature, origin u8 (0 wild, 1 partner), bringer u8 (a player id; 0xFF for wild), place u8 (0 roam, 1 swim, 2 held, 3 battle), by u8 (holder or battler, 0xFF otherwise), v f32×3 (ground and battle: world position; held: the offset in the holder's frame), angle u16. 34 bytes an entry, 1032 at most |
| 75 | WildAsk | from the host to the picker, reliable | area u8, ask u16, section u8 (0xFF: the garden's area in window `slot`), land u8, pond u8 (land + pond 1..=5), fresh u8 (0/1: may hand over its own visitors with their places), slot u32 (the host's clock window) |
| 76 | WildPick | to the host, reliable | area u8, ask u16, section u8 (not 0xFF), count u8 (≤ 5), count × (creature, place u8 (0 the server places it on land, 1 in the pond, 2 on land at v, 3 swimming at v), v f32×3, angle u16) |
| 77 | WildBring | to the host, reliable | area u8, key u64, creature, place u8 (0 the server places it, 2 at v), v f32×3, angle u16 |
| 78 | WildCarry | to the host, reliable | area u8, creature, offset f32×3 |
| 79 | WildTake | to the host, reliable | id u16 |
| 80 | WildHeld | to the host, ~10 Hz while held, unreliable | id u16, mode u8 (0 in hands: v is the offset in the sender's frame, \|v\| ≤ 10; 1 let go: v is the world position), v f32×3 |
| 81 | WildPut | to the host, reliable | id u16, at f32×3, angle u16 |
| 82 | WildAbsorbed | to the host, reliable | id u16, owner u8 (≤ 15), index u8 (< 4) |
| 83 | WildBattle | to the host, reliable | id u16, step u8 (0 reserve, 1 won, 2 lost or gave up, 3 cancelled) |
| 84 | WildAnswer | from the host to one player, reliable | kind u8 (0 take, 1 carry, 2 bring, 3 battle), id u16 (the visitor's; 0 when refused before it had one), key u64 (a bring's key, else 0), ok u8 (0/1) |
| 85 | WildTaught | from the host to every session of a Chao's owner, reliable | identity u64, origin u8 (0 wild, 1 a partner), creature |
| 86 | WildUsed | from the host to a partner's bringer, reliable | garden u8 (1..=3), key u64 |

### What the host accepts
Every check failing means the message is dropped (and, for a request, answered with ok 0).
None of them changes a save or stops the server.
- **WildHello**: kept per session. Losing *see* or changing print takes the player out of the
  picker choice and the list's receivers; their held visitor is put down where they are.
- **WildPick**: only from the asked player, for the outstanding ask, in that garden. `count` ≤
  land + pond; swimmers ≤ pond; `section` equals the asked one when the ask names one. On a
  not-fresh ask places 2 and 3 are refused. On a fresh ask placed entries (up to
  `MAX_VISITORS`) are kept where they are when a land spot (place 2) or pond cell (place 3) is
  within 15 units, else placed by the server. The section becomes the garden's when the ask is
  of the host's current window. Each entry is added only while there is room.
- **WildBring**: from a player present in that garden with *see* and *machine*, print equal to
  the garden's (an empty list's: its section's, else its picker's; any with neither), at most 4
  partner visitors of theirs there, no visitor of theirs with that key already. Place 2 is kept
  when v is within 15 of the machine spot and the floor is within 10 under v; otherwise the
  server places it. Room as in section 2.
- **WildCarry**: from a player present with *see*, print matching (as for WildBring), holding
  none, with no carry accepted since they came into the garden (`Player.since`), fewer than
  `MAX_VISITORS` wild visitors there, and room. The visitor is added as `Held { by: them }`.
- **WildTake**: on a roaming or swimming visitor, from a present player with a matching print,
  within 30 units of it. The first one wins; the rest get ok 0.
- **WildHeld / WildPut**: only from the holder. A let-go or put position more than 40 units from
  the holder's last state is clamped to the holder. A put lands on the floor below `at`; in a
  pond cell under the surface it swims (`in_pond`).
- **WildAbsorbed**: the visitor is held by the sender, or was let go by them less than 2 s ago
  and is still roaming or swimming (nobody took or reserved it since);
  the Chao (owner, index) is in this garden's run, or a held Chao shown there (8..=15) and
  awake; the Chao is within 25 units of the visitor. Then the visitor is gone for everyone, and
  teaching follows section 5.
- **WildBattle 0**: a roaming or swimming wild visitor (never a partner) within 30 units of a
  present sender with a
  matching print, who has no other reservation. **1, 2, 3**: only from the player holding that
  reservation, from any area.

### Timeouts on the host
- Held, no `WildHeld` for 2 s while the holder is settled in the garden: put down at the holder.
- Holder settled in another area: a wild visitor still in their hands is carried out (gone for
  everyone; their game keeps it, section 4); one let go of (falling, in a Chao's hands) is put
  down where their last `WildHeld` had it; a partner visitor is put down at the holder's last
  place in the garden.
- Holder disconnects: put down at their last place.
- Battle reservation: 15 min (`BATTLE_LIMIT`), then gone as given up. Battler disconnects:
  free again (`Roam` where it was).

---

## 4. The game's copies (`shared.rs`)

### Making and moving them
- A copy is made like a visitor (`scene::create`: animal id 3, no save record) with the visitor
  display (`billboard`, the 3D actor or thumbnail from the player's own cache, shiny colours by
  its flag). It is in `scene::is_partner_object`, so `items::live_items` never shares it as an item.
- Copies follow the newest `WildList` for the player's garden whose print equals the game's
  own; a list with another print, or none for 1.5 s, removes every copy (as items' `STALE`). A
  list whose serial isn't newer than the last one taken (under 1.5 s old) arrived late and is
  dropped.
- Only creatures that fit the game's catalog get copies (`shared::fits`): the ability one of its
  species', a wild one's moves in its learnset by its level and its species at that level in
  one of the catalog's wild areas (`in_the_wild`), a partner's moves in the catalog. The same check
  stands before a copy is carried out, kept when the garden is the player's alone again, or
  taught (`WildTaught`).
- Ground copies run the animal's own main for its motions, kept from wandering by
  `items::hold_still`, and walk along the list with the `items::Walk` prediction and
  `items::walk_with`. Swimmers take x and z from the list and y from `swim_y`.
- Copies held by someone else are drawn at that player's puppet hands plus the offset
  (`garden::holder`, `garden::in_hands`); in battle, standing where the list has them.
- A copy is skipped (nothing is sent) while the game has no category-2 room (`scene::room`
  with `RESERVE`).
- The info card (`card.rs`) lists copies with the local creatures. A copy's partner comes from
  its entry (types from the catalog), not from `waiting_in`. Held copies say who holds them;
  battled ones "Battling <name>".
- The title names the list's section, or while it has none (a new run, or the host's window
  just changed) the garden's area in the game's own window (`shared::shown_area`): the area's
  name (the habitat's name in the game's own catalog) and the minutes its window has left
  (`rotation::until_next`) show for 4 s on entering and whenever that section changes.

### What the player does
| Action | The game | Then |
|---|---|---|
| Picks up a copy | sends `WildTake`; keeps it in hands meanwhile | refused (`WildAnswer` ok 0, or the list shows it with someone else): dropped at once (`DROP_HELD`, `items.rs`) |
| Holds it | `WildHeld` mode 0 about 10 a second | others draw it in their puppet's hands |
| Lets go | the game's own physics move the copy; `WildHeld` mode 1 with where it is; `WildPut` once it is on the ground (or after 1 s) | the server roams or swims it from there |
| Whistles | nothing new (`PlayerState.action` 42) | the server calls visitors within reach |
| Leaves the garden holding a wild copy the list has in their hands | the copy becomes `carrying`, then `pending` outside, as alone (`wild.rs`); only when the newest list has it in their hands too | the server removes it for everyone on their settled state elsewhere |
| Leaves holding a partner copy | nothing (the copy goes with the garden) | the server puts it down where they left |
| Enters a shared garden with `pending` | waits for the garden's first list, then `WildCarry` (once a visit; the answer is waited for however late, and the list's entry for it is left to the answer) | ok: a copy with the answer's id is made in front of the player and given to them (0x46E5B0); refused: it stays pending with its banner |
| Hands a copy to a Chao | section 5 | |
| Battles it | section 6 | |

Copies the list has in the player's own hands run the game's own carrying; the list never
moves them. A copy seen battling, or held by someone else, can't be picked up (it is let go at
once).

### Alone → shared
- Every player's own visitors go when the garden turns shared (`wild.rs`, as soon as friends are
  there), except one in hand, which is carried in with `WildCarry` once the garden's list comes:
  on ok that same object becomes the copy (`wild::release`, our main and display put on it).
- Where they were (creature, position, facing, swimming) and the visit's section are kept for
  30 s (`wild::take_handover`). A fresh first ask that comes meanwhile is answered with those
  visitors in place (places 2 and 3), whatever area they came from, and the asked count filled
  up with new picks from the asked section or the ask's window's area, so the first player's
  visitors come back as the garden's where they were.

### Shared → alone
When `garden::server_runs()` turns false with no friends left in the garden, the game adopts
its copies instead of deleting them: wild ones become its own visitors (`Live` with the
creature's four moves, `born` now, a new `stay`), and the visit goes on with the area of the
game's own window (`wild::next_area`): its name shows only when that is another area than the
list's section. Partner copies of its own become its waiting partners' objects again (`scene`);
other players' partner copies go. With nothing to adopt the game's own visitors come at once
(`wild::alone_wait`: the garden was shared this visit, so there's no waiting for friends' states).

### Picking for the server
On a `WildAsk` the picker's game picks with its own `wild::pick` on the asked section's tables
(or, for section 0xFF, the garden's area in the ask's window, `rotation::area_at`), with its
odds and the picker's memory of the garden's recent species (`wild::Recent`,
`chao-wild-visitors.md` §6), the creature's moves with `random_moves` at its level, ability slot
at random turned into the ability id, shiny 1 in `SHINY_ODDS`. It answers with as many as it
could pick (0 when its catalog has no such area).

### Pure local state
A wild `Visitor` carries its four moves (chosen with `random_moves` when it is picked) so a
visitor that moves between the game and the server stays the same creature.

---

## 5. Handing a visitor to a Chao, and the lock rule

### In the giver's game
1. Holding a copy near a Chao copy within `ITEM_NEAR` (25) lets that Chao's own AI run for a
   moment (`garden.rs` `touches`), as any held animal does.
2. The animal check (`garden::may_take_items`) says no to a visitor copy for a Chao whose
   owner's bit in the list's `learners` is clear, locked against the player (`ChaoLocked`), or
   asleep. Such a Chao never starts the absorb.
3. The absorb hook (0x56708E, `scene::absorb_hook`) finds a copy: `garden::chao_of(object)`
   gives the Chao's (owner, index) (mirrors, own Chao the server runs, held Chao). The copy is
   hidden (`MODE_DELETED`, its own main put back) and `WildAbsorbed` goes to the host. The
   giver's game never writes its own store for a copy. The +1 happiness on the Chao copy goes
   to its owner as `ChaoChanges`, as for any touch.
4. If the host refuses, the copy comes back with the next list.

### On the host
After the checks of section 3, the visitor is gone for everyone. The owner's game is taught
only when all of these hold:
- the owner (the player id, or for 8..=15 the held Chao's key) has a connected session with
  *see* and the garden's print;
- `!locked_against(from, key)`: the same rule as `ChaoLocked`; the owner's own sessions always
  pass;
- the Chao's identity is known (`records[(owner, index)].identity()`, or the held Chao's
  roster record).

Then `WildTaught { identity, origin, creature }` goes to every session of the owner's key.
For a partner visitor `WildUsed { garden, key }` goes to its bringer whether or not anyone was
taught.

`learners` in each list has the bit of every owner id that would pass the first rule (players
present with *see* and the print; held ids whose key has such a session and whose Chao is
awake).

### In the owner's game
`WildTaught` from `SERVER` only, for an identity of one of the player's own Chao (their save's
Chao slots). The game rebuilds the partner from its own catalog: types from the species, the
ability must be one of the species' two (wild origin), every move must be in its learnset at or
below its level (wild) or a move the catalog has (partner origin). Then `learned_wild(identity,
partner, name)`: `Store::learn`, `write_store`, `resend_partners`, the toast. Anything that
doesn't pass is dropped and nothing is written.

`WildUsed` from `SERVER`: `Store::drop_waiting(garden, key)` removes that waiting partner
without teaching anything, and the store is written.

---

## 6. Battling a visitor (alone or shared)

### Starting
The info card shows **Y: Battle** for the visitor it is about (not one the player holds) when
the game is in a garden, settled and in game, controls on and the window focused, the F9 menu
closed, no chat being typed, the player's hands empty, and no trip or battle scene already on
(`wild_battle::can_go`); in a shared garden the copy must also be wild, roaming or swimming.
Partners are never battled. No Battle DLC is needed: the battle runs in the owned scene, not in
the karate area.

Y does nothing in the garden while standing (only B/X open the action list; Y counts only
while holding, `chao-details.md`).

Y with no Chao to battle with only says so. In a shared garden the game sends `WildBattle 0`
and waits up to 3 s for `WildAnswer` kind 3 (the card says "Asking to battle..."). Refused:
"Someone else is with that visitor". No answer in time ("No answer from the host: try again"),
or a yes once the player can't go any more: `WildBattle 3`. Granted: the visitor is
`Battle { by }` and others see it busy. Alone, the game takes a **snapshot** of the visit
(`wild::snapshot`: garden, area and its window, `cap`, `came`, each visitor's creature,
partner, position, swimming or not, the time it has left, and which one is battled).

### The trip (`battle/wild.rs`)
The trip record: origin garden, the player's position and angle, the foe, the visitor (server
id, or the snapshot's), when it set off, phase, result. Phases:
1. **Going**: the owned battle scene (`battle::arena`) comes up over the loaded garden; nothing
   changes area. Not there within 30 s of running game (`GOING_LIMIT`), or settled in another
   area: cancelled.
2. **Scene**: the battle menu opens with `Session::with_wild(creature, shiny)` at the team
   screen (singles), titled "Your team against wild <name> (up to <n>)"; not open within 30 s
   (`OPEN_LIMIT`): cancelled.
   Confirming builds `wild_fight`; the fight is marked wild (no ladder progress). A venue or
   model import failure closes the scene as cancelled.
3. **Back**: once the menu ends the scene closes; settled in the origin garden again, the player
   is put back where they stood, facing the same way, and a note says whether the visitor stays
   or ran away. Not back within 60 s (`BACK_LIMIT`), or settled in another garden: the trip ends
   there.

### The foe
Built as trainer opponents are (`battle::wild::build`):
`Fighter::new(name, level, trainers::stats(base, 15, level, 0), types, ability, moves)` with
the visitor's four moves (`dex.move_data`), its ability id (it must be one the battles
support), the species form's base stats and types, the catalog's name; `exp_yield` from the
species form. IV 15 in every stat and a neutral nature are what the card shows (`card.rs`),
so the battle has the card's numbers. In the ring it stands as its 3D actor, in shiny colours
when it is shiny (`figures::show_form`).

### Results
| Session end | XP | The visitor |
|---|---|---|
| Won | `levels::wild_xp`: `exp_yield × level / 7` split among the Chao that took part, at least 1, with no trainer-battle bonus | stays: `Roam` where it was (shared, `WildBattle 1`); restored with the snapshot (alone) |
| Lost, a draw, or gave up | none (a wild foe gives XP only when it faints) | runs away: gone for everyone (`WildBattle 2`); left out of the snapshot |
| Backed out before the fight (`Closed`), or the scene couldn't load | none | stays (`WildBattle 3`) |
| Any other way out (the pause menu's exit, the menu stopping) | none | after the fight began: given up (`WildBattle 2`); before it: stays (`WildBattle 3`) |
| The trip past 14 min on the clock (`TRIP_LIMIT`, inside the host's `BATTLE_LIMIT`, 15 min) | none, even for a win after it | given up |

XP goes into the store as for trainer battles (`earn`, `Command::Levels`) once the battle is
decided; the result screen's last line says whether the visitor stays or ran away. Shared,
`WildBattle 1`/`2`/`3` goes to the host when the battle menu ends.

### The garden meanwhile
- **Alone**: the visit is kept by the snapshot; when the battle ends the visit goes on, without
  the battled one if it ran away. A snapshot is dropped on arriving in another garden or after
  `BATTLE_LIMIT`.
- **Shared**: the server keeps the visitor reserved; the reservation belongs to the battler's
  connection, not to their place. If the run ends meanwhile, the garden's list is **frozen**
  with only its battled visitors: the others go, as the remaining player's game keeps them as
  its own (`shared::ended`, which drops the battled one). Frozen, it isn't moved or sent and has
  no asks. When the reservation ends with no run, a won or backed-out visitor is kept for
  `KEEP_FROZEN` (3 min). A run of that garden with the same ground fingerprint takes the kept
  ones up again with `came` back to 0, so its first ask is fresh and the picker hands over their
  own visitors around the returned one; another fingerprint, or the time running out, drops it.
- The trip's limit runs on the clock from setting off (paused or not), 14 minutes, a minute
  inside the host's `BATTLE_LIMIT`, so the game always gives up first. A battle still on then
  counts as given up when it ends: no XP, and the result line says it ran away.

---

## 7. Link machines in shared gardens

- The machine shows in a garden run by the server when the player has SharedVisitors and
  LinkMachine on and a catalog with a print; `machine.rs` and `scene.rs` use the same `shared()`
  test as `wild.rs` (`server_runs() || friends_in(garden)`), so a garden shared but not yet run
  has neither.
- Every game finds the same spot (`find_spot` is pure and the ground is the same), so the
  machine needs no syncing. Each game has its own object and collision box (`REGISTER_LAND`).
  The server keeps visitor spots `MACHINE_CLEAR` away from it and adds a box `Blocker` of the
  machine's footprint at that spot to its Chao's land, so server Chao walk round it.
- Each player uses it for themselves: their kept creatures and gift, the day's imports
  (`can_import`), and Moves, type & ability for their own Chao, all in their own
  `creature/partners.txt`.
- **Bringing one out**: `can_import`, then `WildBring` (place 2, at the hatch). On ok the game
  records the import (`Store::import`, the waiting entry and the day's import) and writes the
  store; on a refusal nothing is recorded ("The garden is full of animals right now").
- **Waiting partners**: on entering a garden the server runs (or when it starts running it),
  the game sends `WildBring` (place 0) for each of its partners waiting in that garden (with
  LinkMachine on: the host takes brings only with flag 4), and makes no local objects for them
  (`scene` deletes its own in every shared garden, server-run or not, one in the player's hands
  too, dropped first). One refused for lack of
  room is offered again 20 s later. Leaving, or the run ending, takes them out of the list; they
  stay waiting.
- A partner visitor is shared like a wild one (anyone sees, carries and hands it to a Chao;
  teaching follows section 5) but never leaves its garden and never leaves by itself.

---

## 8. The SharedVisitors switch

- config.ini `[CreatureLink] SharedVisitors=On` (default On); Mod Manager "Wild visitors in
  shared gardens" in the "Creature link and battles" group (`mod/configschema.xml`); `menu.rs`
  beside `WildVisitors`.
- Off: `WildHello` flag 1 clear. The server never asks that player, never sends them a list,
  and their Chao never learn from shared visitors. Their game has no visitors and no machine in
  shared gardens; their own gardens are as before.

---

## 9. Tests

### Server (`wild_host.rs`, unit tests with a fake `Body` stream as `chao_host`'s `running_on`/`run_to`)
- Picker: the earliest arrival with *see*, *pick* and the print; lower id on a tie; leaving
  hands over at once with the list, section and cap kept; asks to a former picker dropped.
- Picks: refused from a non-picker, for another ask or area, over the asked count, with too many
  swimmers, a changed section, or placed entries on a not-fresh ask; a fresh ask keeps placed
  ones near their spots.
- Room and caps: never more than `cap` wild ones; wild + partners + category-2 items + 2 ≤ 32; a
  partner with no room sends the farthest roaming one away, or is refused.
- Movement: roaming visitors stay on land spots' floors, never step on water cells; swimmers
  stay on pond cells; whistling within 100 calls them, beyond doesn't; leaving after `stay`
  only with every player 60 away; below −50 gone.
- Hands: first `WildTake` wins; a non-holder's held, put or absorb is dropped; held with no
  report for 2 s is put down; a put in the pond swims; a holder settled elsewhere carries a
  wild one out and puts a partner back; a disconnect drops it.
- Absorb: owner always taught; another player taught only when the lock allows
  (Always/ChaoWorld/ThatGarden/Never against owner places); locked: gone, not taught; owner
  with another print or *see* off: gone, not taught; asleep held Chao refused; `WildUsed` to the
  bringer for partners.
- Battles: a reserved one refuses takes and second reservations; won → roams, lost → gone,
  cancelled → roams, disconnect → roams, `BATTLE_LIMIT` → gone; a result from anyone but the
  reserver is dropped; across a run's end only the battled one kept, also a while after a won
  battle, taken up by a run on the same fingerprint with a fresh ask, dropped on another; a
  partner can't be reserved.
- Lists go only to present players with *see* and the garden's print; `learners` bits as in
  section 5.

### Protocol (`protocol.rs`)
Every new body round-trips, and decoding refuses each out-of-range field (area, count, species,
level, ability, moves 0×4 or repeated, place, step, kind, owner, index, non-finite or huge
floats, a WildList over 30 entries).

### Game side (x86_64 unit tests; catalog tests with `SA2CO_TEST_CACHE`)
- `chao_sim::wild`: the moved helpers' tests, and `Roam` steps.
- `wild_print.rs`: the same catalog gives the same print; one learnset entry changed gives
  another (catalog test).
- A pick answer for a land and a pond ask from each garden's areas (catalog test).
- `WildTaught` checks: a wild creature with a move outside its learnset or above its level, or
  an ability not its species', is refused; a valid one becomes the same `Partner` as
  `Visitor::partner` (catalog test).
- The wild foe's stats equal the card's (catalog test); `levels::wild_xp` has no trainer bonus,
  at least 1, 0 unless won; `Session::with_wild` opens at the team screen, `Closed` and `GaveUp`
  give no XP and no ladder progress.
- The alone snapshot restores the same visitors without the one that ran away.

### fakepeer (`FAKEPEER_WILD`, docs/testing.md)
fakepeer sends `WildHello` with the print of `FAKEPEER_CACHE` (flags 1, 2, 4) and copies the
player's state as usual, so it is in the same garden. Modes:
- `pick`: comes into the garden first (alone it walks in the Neutral Garden), becomes picker
  and answers asks from its catalog; the player's game shows the copies, then takes
  over picking when fakepeer leaves (`FAKEPEER_WILD_LEAVE=<secs>`).
- `take`: picks up the nearest roaming visitor, carries it 10 s next to the player, puts it
  down; with `carry`, leaves for the lobby holding it (gone for the player).
- `whistle`: whistles every 20 s (action 42 in its state).
- `absorb`: hands the visitor it holds to the player's nearest Chao (`WildAbsorbed` with the
  player's id): the player's partners.txt changes only when their F9 "Others can change my
  Chao" allows fakepeer.
- `battle-won`, `battle-lost`: every 40 s reserves the nearest visitor, leaves the garden (to the
  karate area), sends the result 25 s later and comes back.
- `hostile`: sends every new body with bad values, unknown ids, picks without an ask, and
  absorbs of visitors it doesn't hold; nothing crashes and no save changes.
