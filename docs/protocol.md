# Network protocol (v31)

Creatures travel as ids from each player's local catalog (species, form, types, ability and
moves); no game data is sent. Shared partners, live battle members and wild creatures carry a
form byte after the species id. Peers compare a fingerprint of their catalog and battle rules
before they battle or share visitors.

Code: `src/net/`. All multi-byte values are little-endian. Every received byte goes through
the bounds-checked `wire::Reader`; bad data is dropped, never trusted. Players need the same
`PROTOCOL_VERSION`, mod version and game build to join.

## Transport (`udp.rs`)

Packet header (14 bytes): `magic "SA2O" u32 | version u8 = 1 | kind u8 | epoch u32 | seq u32`.

| kind | meaning |
|---|---|
| 0 unreliable | payload delivered as-is (events, chunked item reports, heartbeats) |
| 1 reliable | payload = `fragment index u8 | fragment count u8 | data`; acked, resent every 150 ms (40 tries), delivered in order |
| 2 ack | acks `seq` |
| 3 sequenced | payload = `stream u32 | data`; unreliable, only newer `seq` values for that stream are delivered |

- Max packet 1200 bytes; reliable messages up to 32 fragments (~37 KB). A Chao (2 KB) is 2 fragments.
- `epoch` is random per process. A new epoch from a known address means the peer restarted:
  both directions reset. The last eight retired epochs are ignored so delayed packets from
  a previous process cannot reset the new link or acknowledge its reliable messages.
- At most 16 links (connected players included); the session drops anything that isn't a valid
  Hello. A link that has heard nothing for 20 s and has nothing left to deliver is dropped.
- A reliable packet too far ahead of a missing one (256+) isn't kept or acked; it's sent again.
- Sequenced snapshots have a separate wrapping send counter and at most 512 receive streams
  per link, reset on a new epoch; an inactive stream is evicted when this bound is reached.
  Streams distinguish sender, body and its owner/garden/Chao
  where needed: a newer update of one Chao owner does not discard another owner's update.
  Player poses, GardenChao, ServerChao, ChaoLive, ChaoHeld/ChaoTouched, GardenTrees, Ball, WildList,
  KartState and KarateState use this when sent unreliably. Reliable events retain their own
  ordered channel; chunked item reports and sounds stay independent, so one doesn't replace
  another. Holds and touches of the same Chao share a stream, so a delayed hold cannot undo
  its holder's newer release. Every peer supports this transport kind.

## Session (`session.rs`)

Star: the host is player 0 and assigns 1..=7 (a dedicated host, `sa2co-server`, isn't a
player: it's left out of `Welcome` and assigns 0..=7). Clients talk only to the host; the host relays
`Game` messages to everyone else and **overwrites `from`** with the sender's real id.

- Join: the client sends `Hello` (reliable). The host checks protocol version, mod version and
  game build, then answers `Welcome` (your id + all players) and broadcasts `PeerJoined`;
  otherwise `Reject` with a reason (e.g. "lobby full").
- Heartbeat every 1 s each way; 6 s of silence = gone (`PeerLeft`). `Bye` on a clean exit.
  Clients rejoin 3 s after losing the host.
- Player frame counters are checked before the host updates the world/referee or a client
  updates its latest state, including frames sent reliably; old and duplicate frames are
  ignored, with wrapping handled. A leaving player's frame history is cleared for their next
  connection. Player playback buffers six frames (100 ms at 60 fps) for normal network jitter.

## Messages (`protocol.rs`)

| tag | message | fields |
|---|---|---|
| 1 | Hello | protocol u16, mod_version str≤16, game_build [u8;8], name str 1..10, key [u8;16] (the player's own random key, config.ini `PlayerKey`, made once by the mod; the host knows their Chao by it from one session to the next. An older Hello without it is still read, and turned away by its protocol number) |
| 2 | Welcome | your_id u8, count u8 ≤ 8, (id u8, name str)* |
| 3 | Reject | reason str≤64 |
| 4 | PeerJoined | id u8, name str |
| 5 | PeerLeft | id u8 |
| 6 | Heartbeat | |
| 7 | Bye | |
| 8 | Game | from u8, body |

Game bodies:

| tag | body | sent | fields |
|---|---|---|---|
| 1 | PlayerState | 30 Hz, unreliable | time u32 (sender frame), level i16, mission u8, chao_area i8, character u8, position f32×3 (finite, \|v\| ≤ 1e6), rotation i32×3 (BAMS), anim u16, frame f32 (finite, \|v\| ≤ 1e5), flags u8 (128: between places, a load or an area being set up, so `level`/`chao_area` aren't settled yet), room (owner u8, kind u8), upgrades u32 (`CharObj2Base::Upgrades`, for mech upgrade parts), action u8 (EntityData1 action), status u8 (1 on the ground, 2 holding something, 4 just hurt and can't be hurt again yet: its puppet blinks like the character, every other two frames, 8 standing still by the game's own speed: a skin on its puppet stands): action and status drive skins' animations |
| 2 | Chao | reliable | index u8 (0..=1 garden Chao, 2 = My Chao), 0x800 bytes `ChaoData` (must pass `ChaoData::validate`) |
| 4 | RoomPick | reliable | level i16 (1..=100), mission u8 (0..=4), character u8 (0..=15): a room player's Stage Select pick |
| 5 | RacePick | reliable | category u8 (0..=4), course u8 (0..=12), level u8 (0..=11): a room player's race |
| 6 | KaratePick | reliable | opponent u8: a room player starts a live VS fight against this player |
| 7 | KarateState | ~30 Hz, unreliable | clock u16, 2 × (position f32×3, rotation i32×3, hp f32, morale f32, motion u8) |
| 8 | KarateEnd | reliable | code u8 (2 knockout/ring-out, 3 time up), result u8 (0/1 winning side, 2 draw), hp f32×2 |
| 9 | KarateCheer | reliable | count u8: the member cheered (A) this many times |
| 10 | StageKills | reliable | level i16 (0..=100), mission u8 (0..=4), count u16 (≤ 0x600), then count × u16 object index (< 0x600): co-op room, these stage objects (`SETObjectData` record indices, re/stage-objects.md) were destroyed. New kills as they happen; the whole list when a room player enters the stage (sent by the room players already in it) |
| 11 | GardenChao | 30 Hz, unreliable | owner u8, count u8 (≤ 4), then per Chao: index u8 (< 4), position f32×3, rotation_y i32, motion u16, held u8 (0/1), offset f32×3, mood (current state i16, timer i32, length i32), eyes i16, mouth i16, props u8×3 (per attach point 4/11/35: 0 none, else 1 + index into `garden::PROPS`, ≤ 22), drum u8 (size 0..=255, 0 none), cocoon u8 (0 none, 1 + kind 0..=2), book u8 (the picture book in front of it: 0 none, 1 + its mode 0..=3), owner_runs u8 (0/1: from its owner, their game runs it now for a life event, evolving or the end of its life, and everyone follows the owner's for it, the server too): player `owner`'s visiting Chao as they are in the sender's game (re/chao-garden-sync.md). Each player sends their own (that's how the others and the server know they visit); while the server runs the garden everyone's copies follow `ServerChao` instead, else the owner's; offset = where it is relative to the sender while held, in the sender's frame |
| 12 | ChaoChanges | reliable | owner u8, index u8 (< 4), count u8 (≤ 128), then count × (offset u16 (< 0x800), value f32): the sender's interaction changed `owner`'s visiting Chao; `chao::changes` deltas (or values for animal parts) for the owner to apply to the real one, and for the server to apply to the one it runs. From the server (sender 0xFF): what its AI changed, about once a second, to the owner only. For a Chao locked against the sender (its owner's lock, see Brought) the host neither applies nor passes it on, and the owner's game refuses one from another player while its own lock says no |
| 13 | ChaoHeld | 30 Hz, unreliable | owner u8, index u8 (< 4), offset f32×3: the sender is holding `owner`'s Chao `index`; the game running it (the server, or while it doesn't run the garden the owner's) shows it in their hands |
| 14 | GardenItems | ~10 Hz, unreliable | area u8 (1..=3), count u8 (≤ 24), then per item: id (origin u8, serial u16), category u8 (2/3/7/9), kind u8, bites u8 (≤ 4), place u8 (bit 0x80: unsaved, in no one's save; then 0 ground, 1 held by the sender, 2 held/eaten by a visiting Chao the sender's game runs (its own, or any while it keeps the garden), 3 in use unseen), chao u8 (place 2: that Chao's owner × 16 + its index (< 4)), position f32×3 (ground/unseen: where; held: offset in the holder's frame): items the sender owns in that garden (src/items.rs) |
| 15 | ItemClaim | reliable, to the host only | id: the sender picked up (or a Chao in their game took) item `id` and asks to own it. The host's referee answers; it is never passed on |
| 16 | ItemOwner | reliable, from the host | id, owner u8: item `id` belongs to `owner` (to everyone when a claim is granted, to the claimer alone when refused) |
| 17 | Chat | reliable | text str (1..=80 printable ASCII) |
| 18 | RaceStatus | ~4 Hz unreliable, reliable at the finish | level i16, mission u8, progress u16 (0..=10000, 0xFFFF unknown), finished u8 (0/1) [race u32, game u32 centiseconds]: room race status |
| 19 | Route | reliable | level i16, mission u8, count u16 (≤ 2000), count × f32×3: a stage route (first clear), for race positions |
| 20 | KartPick | reliable | course u8 (0..=2): a room player started a Kart Race |
| 21 | KartState | 30 Hz unreliable | position f32×3, rotation i32×3, yaw i32, wheel i32, kind u8 (2..=7), accel u8 (0/1, bright exhaust), tilt i32×3 (body tilt sums), frame u16 (the sender's game frame, wrapping; repeats and late ones are dropped, speeds come from it): the sender's kart |
| 22 | HuntSet | ~1 Hz unreliable | level i16, mission u8, 3 × (kind u8, id u8, position f32×3): the room's treasure hunt pieces |
| 23 | HuntClaim | reliable, to the host only | level, mission, sig u32, slot u8 (0..=2): the sender collected this piece. Not passed on |
| 24 | HuntTaken | reliable, from the host | level, mission, sig, slot, player u8: who got the piece first (everyone; a late claimer alone) |
| 25 | KeyUsed | reliable | level i16, mission u8, key u8 (0/1): co-op room, the sender's stage key went into its keyhole; the others' keys go in too (from the ground), so their doors open |
| 26 | Shot | ~unreliable, per shot | level i16, mission u8, kind u8 (0 bullet, 1 bazooka), position f32×3, direction f32×3: co-op room, the sender's mech fired; the others fire the same shot |
| 27 | Switch | reliable | level i16, mission u8, id u8 (< 32), on u8 (always 1; off isn't shared, since a death restart clears every switch): co-op room, a stage switch turned on; the others turn it on too (their doors and gates follow) |
| 28 | Checkpoint | reliable | level i16, mission u8, record u16 (< 0x600), position f32×3, rotation i32×3: co-op room, the sender passed this checkpoint standing there; the others light it and restart there if they have no checkpoint yet or it's further along the stage's route |
| 29 | Ball | 10 Hz unreliable | area u8 (1..=3), claim u16, position f32×3, velocity f32×3, flags u8 (1 its owner holds it, 2 it's in the water): shared garden, the ball as whoever kicked or held it last has it (a higher claim takes over; equal claims go to the lower id) |
| 30 | BossHp | reliable | level i16, mission u8, hp f32, full f32: co-op room, the sender's boss is down to `hp`; the others' boss with the same `full` drops to it (never below a sliver: the last hit is each player's own) |
| 31 | SkinWorn | reliable, on change and every 10 s | skin str ≤ 24 (a skin id: `[a-z0-9_]`, or "" for none): what the sender wears; nothing shows on screen |
| 32 | SkinListGet | reliable, to the host | page u16 |
| 33 | SkinList | reliable, from the host | page u16, pages u16, count u8 (≤ 48), then count × (path str ≤ 96, size u32 (≤ 16 MB), hash [u8;8] FNV-1a 64): the host's shared skin files (file index = page × 48 + position) |
| 34 | SkinGet | unreliable, to the host | file u16 (< 1024), offset u32 |
| 35 | SkinChunk | unreliable, from the host | file u16, offset u32, len u16 (1..=1024), data |
| 36 | GardenTrees | 1 Hz unreliable | area u8 (1..=3), 7 × 8 bytes, thirsty u8 (a bit per spot: its tree wants water, a planted seed or GrowTree `Data1+0xAC` set; the server's Chao water those): shared garden (friends there), the sender keeps this garden's trees (the garden's keeper) and these are its 7 tree records (kind, state, growth, life, fruit ×3, facing): the server's set it runs, or before the server has one, their save's (which seeds it); the host's referee keeps them, the others show copies of them (`trees.rs`) |
| 37 | TreeAction | reliable | area u8 (1..=3), slot u8 (< 7), action u8 (0 plant, 1 shake, 2 water), kind u8 (the seed's): the sender planted in, shook or watered their copy of that tree (or, from the host (0xFF), a Chao the server runs did); its keeper does it for real (a planted seed, one shake of ripe fruit, or its life to 10000 as the game's 0xAD watering sphere does) |
| 38 | GardenEggs | ~10 Hz unreliable | area u8 (1..=3), count u8, count × (index u8 (< 4), position f32×3, held u8 (0/1)): the sender's visiting eggs (their Chao index) where they lie, or held by the sender (`eggs.rs`) |
| 39 | EggHeld | reliable | owner u8, index u8 (< 4), held u8 (0/1), position f32×3: the sender picked up or put down a copy of player `owner`'s egg `index`; the owner's game holds the real one there |
| 40 | ToyMode | reliable | area u8 (1..=3), toy u8 (0 TV, 1 radio, 2 jack-in-the-box), mode u8 (<= 12): the sender's toy jumped to that mode. From the toys' keeper (the garden's lowest player id), the others' toy jumps too; from anyone else (a touch), the keeper makes the jump and sends it on (`garden_toys.rs`) |
| 41 | GardenKeeper | 4 Hz unreliable | area u8 (1..=3), term u32: the sender keeps that garden's trees and toys (`keeper.rs`; its Chao are the server's). The others follow the claim with the higher term (then the lower id); with nobody claiming for a moment the lowest id in the garden claims with a higher term |
| 42 | ChaoSounds | unreliable, as they happen | count u8 (≤ 16), then per sound: owner u8, index u8 (< 4), call u8 (0 sound 0x4372E0, 1 sound 0x437590, 2 jingle), id u16 (sound id, or an index into `garden::JINGLES`), offset f32×3 (from the Chao, world axes), args i16×3 (the call's own arguments; for call 0 the first is only 0/1, whether it had a holder): sounds and jingles the sender's game played while that Chao's AI ran (one it runs: its own, or any while it keeps the garden). The others' copies that follow the sender play them at their own place; copies being touched (running their own AI) don't |
| 43 | ServerGarden | from the host's referee, on garden entry and every 2 s, reliable | area u8 (1..=3), trees 7 × 8 bytes, then per toy (TV, radio, box) mode u8 (≤ 12) and age u16 (seconds since that jump; 65535 none): the shared garden as the server keeps it (re/chao-garden-items.md 7). The keeper runs these trees (with friends there), the others show copies; a newcomer's toys make jumps younger than a minute |
| 44 | ServerItems | from the host's referee, on garden entry, pickup/bite/drop and every second (400 ms while held), reliable | area u8, then items as in GardenItems: items in no one's save whose owner left the garden (ids with serial ≥ 0xF000), in a server Chao's hands too (`ItemPlace::Chao`). A player's item a server Chao takes stays theirs: see `ItemSteer`. Shown as copies by everyone, claimed with ItemClaim like anyone's |
| 45 | CarriedChao | reliable, on a change and every second | offset f32×3 (from the sender, their frame), has u8 (0/1), then a Chao record (0x800 bytes) if 1: the Chao the sender carries in Chao World outside the gardens (lobby, kindergarten, entrances), or none any more. The others show it in their puppet's hands (`carried.rs`) |
| 46 | ServerChao | from the host, 30 Hz, unreliable | area u8 (1..=3), owner u8 (a player, or 8..=15 a Chao it holds, see HeldChao), then the list as in GardenChao (`held` always 0; `cocoon` 1 while a held Chao's evolution cocoon is closed): the host runs shared garden `area`'s Chao (chao_sim, net/chao_host.rs, once two players are there and its ground is known) and these are player `owner`'s as it has them; everyone in the garden follows these (re/chao-garden-sync.md "The server runs the Chao") |
| 47 | GardenLand | to the host, reliable, on entering a garden and on every (re)connect | garden u8 (0 Neutral, 1 Hero, 2 Dark; 3 the Chao motion table), fingerprint u32 (crc32 of the whole), part u8, parts u8 (≤ 64), bytes (≤ 16 KiB): part of `chao_sim::land::Garden::to_bytes` (`SA2LAND4`: ground triangles, cell grid, ledges, blockers, toys, tree spots) or `MotionTable::to_bytes`, read from the sender's game; the host uses what most players agree on |
| 48 | ChaoLive | from the host (sender 0xFF), 1 Hz, unreliable | owner u8, index u8 (< 4), then 93 bytes: player `owner`'s Chao `index` as the server runs it, the record fields that change as it lives (`CHAO_LIVE_FIELDS`: happiness 0x82 ×2, abilities and alignment 0xA8 ×12, emotions, needs and personality 0x12C ×0x2B, bonds 0x16C ×36), to everyone in the garden but its owner; their copy of it takes them (record and snapshot), so its status balloon shows them |
| 49 | ChaoTouched | to the host, ~10 Hz while it lasts, unreliable | owner u8, index u8 (< 4), position f32×3, rotation_y i32, motion u16 (what it plays there; the others are shown it), then 0x2B bytes (`CHAO_MOOD`: its emotions, needs and personality, record 0x12C..0x157): the sender's game runs that Chao for a while because they touched it (threw, petted, fed it; holding is `ChaoHeld`), and this is where it is; the server's Chao is kept there, takes that mood (so a beaten Chao stays angry or scared; not from a player it's locked against), and goes on from it |
| 50 | StageRings | reliable | level i16, mission u8, count u16 (≤ 0x600), then count × (record u16 (< 0x600), ring u8 (< 8)): co-op room, rings of ring lines and circles (RINGLINE, RINGCIRC and the Knuckles/Rouge ones; `record` is the group's `SETObjectData` index) were collected; they go for everyone, and each player gets a ring. New ones as they happen; all of them when a room player enters the stage |
| 51 | LockOn | reliable | level i16, mission u8, gun f32×3, then twice (locked, then fired): count u8 (≤ 32), count × (record u16 (< 0x600), aim f32×3): co-op room, the sender's mech lock-on: the stage objects (SET records) it's locked on to now, and those it just fired its lasers at. Sent when the locked set changes (every 6 frames at most) and on firing. The others draw the cursors and lasers with the game's own marker displays on objects of their own; nothing is hit (the kills come as StageKills) (`lockon.rs`) |
| 52 | BossClaim | 4 Hz unreliable | level i16, mission u8, term u32: co-op party, the sender is fighting this stage's boss (it has passed its intro there). With term > 0 the sender's game leads the boss. The leader is chosen keeper-style among the fresh fighters (`boss_follow.rs`) |
| 53 | BossState | 20 Hz unreliable | level i16, mission u8, mode u8, sub u8, counter u8, flags u16, timer u16, position f32×3, rotation i32×3, motion u16, target u8 (player id): co-op party, the leading boss's Data1 state, its motion, and the player it's after. The others' bosses follow it, and run their own mode function while the target is them |
| 54 | BossHit | reliable | level i16, mission u8: co-op party, the sender hit its following boss. The leader's boss flinches as its own hit code would (HP is shared by BossHp) |
| 55 | BossAttacks | 10 Hz unreliable | level i16, mission u8, count u8 (≤ 24), then count × (serial u16, kind u8 (< 6: missile, energy-gun beam, bomb, pillar, smoke, flame), target u8 (player id), Data1 44 bytes): co-op party, the leading boss's attacks in flight and whom each was fired at. Followers draw the ones fired at someone else on objects of their own, with the attack's own display; nothing of them can hit (`boss_follow.rs`) |
| 56 | EnemyStates | 10 Hz unreliable | level i16, mission u8, count u8 (≤ 32), then count × (record u16 (< 0x600), position f32×3, rotation i32×3, distance f32): co-op party, the stage enemies the sender's game runs (its player is nearest) that another party player is near too. Each other game puts its copy there every frame, unless its own player is nearer by 50 (it takes the enemy over); between two senders the nearer, then the lower id, keeps it (`enemies.rs`) |
| 57 | UpgradeGot | reliable | level i16, mission u8, index u8 (< 0x1E), character u8, mask u32: co-op party, the sender picked up an upgrade (its flag in the save's upgrade table 0x1DEB300, for that character, giving its CharObj2+0x24 those bits). Everyone sets the flag (kept with their save) and, if they're playing that character, gets the bits now (`coop.rs`) |
| 58 | GardenUnlocks | reliable | gardens u8 (bits 0x10 Hero Garden, 0x40 Dark Garden): party, the sender's Chao World has these gardens open. Sent when they change and every 10 s while in a party; party players open the same gardens in their own Chao save (`unlocks.rs`) |
| 59 | ItemSteer | from the host (0xFF) to each player in that garden (by the area of their last PlayerState), reliable | area u8, gone u8 (0/1), then one item as in GardenItems: a Chao the server runs borrowed an owner's item (it stays in their save). The real item and every copy follow its position, place and bites; gone removes it from their save and the copies. The server repeats active steering and briefly repeats finished items to the same players, protecting against delayed owner reports and players arriving during a meal |
| 60 | Brought | to the host, reliable, on connecting, on entering or leaving a garden and when it changes | awake u8 (0/1: the save is loaded, so SaveChao reach it), lock u8 (when others may change the sender's Chao, F9 "Others can change my Chao": 0 Always, 1 while they're anywhere in Chao World, 2 while they're in the garden that Chao is in, 3 Never; offline counts as in no garden; kept with their Chao), count u8 (≤ 12), count × (garden u8 (1..=3), row u8 (< 4: its visitor row there, its Chao index while they're in it), nth u8 (earlier save slots holding a Chao of the same identity), 0x800 bytes `ChaoData`): every Chao the sender brings to the shared gardens (each garden's visitor rows; eggs left out), from their save, whichever garden they're in. The host keeps them by the sender's key (net/roster.rs): the save wins, so kept ones are updated, moved, dropped or added to match. Not passed on |
| 61 | HeldChao | from the host (0xFF), reliable, when it changes | owner u8 (8..=15), index u8 (< 4), has u8, then if 1: area u8 (1..=3), asleep u8 (0/1), locked u8 (0/1: its owner's lock keeps what others do to it from their save; sent again when that changes), name str (its owner's), 0x800 bytes `ChaoData`: a Chao the host holds for a player who isn't in that garden is shown there under this owner id (asleep: its owner is offline; awake: online elsewhere), or (has 0) not any more. To everyone but its owner. Copies follow its ServerChao (`held.rs`) |
| 62 | SaveChao | from the host (0xFF) to a Chao's owner only, reliable, about once a second while it changes | identity u64 (`ChaoData::identity`), nth u8, count u8 (≤ 64), count × (offset u16, len u8, bytes): what that Chao's life changed in a garden its owner isn't in (it lives there while they're online elsewhere), absolute record bytes, each span inside `SAVE_CHAO_FIELDS` (stats, type, happiness, age and life, abilities and evolution, eyes, hat, animals, emotions, needs, personality, illnesses, bonds, DNA grade alleles). The owner's game writes them into the save slot holding the `nth` copy of that identity and has the save written out |
| 63 | ChaoLocked | from the host (0xFF), reliable, when it changes for that player and on (re)connecting | count u8 (≤ 64), count × (owner u8 (≤ 15), index u8 (< 4)): the Chao locked against the receiver now (see Locked Chao), present players' and held ones; never their own. Their game keeps its copies of these from taking items and undoes what touches change on them |
| 64 | Partners | reliable, on connecting and when someone joins, a Chao absorbs a partner, is taught a milestone move, has its four moves, its types or its ability chosen or gains battle XP | count u8 (≤ 32), count × (identity u64 (`ChaoData::identity`), species u16, form u8, types u8×2, ability u8 (0: none), moves u16×4, level u8 (1..=100)): every one of the sender's Chao with a creature link partner, a learned move or a battle level: its last partner's species and form (0 and 0 without one), the types (the same twice for one; Normal without a partner) and ability it battles with, the four moves it battles with (not its whole lists) and its battle level, in as many lists as needed. Ids are checked against the catalog's ranges: species < `SPECIES_COUNT`, form ≤ 31 and an ordinary form (not a battle-only or held-item one), types < `TYPE_COUNT`, ability < `ABILITY_COUNT` and one the battles support, moves < `MOVE_COUNT`; a bad entry drops the message. Battles against copies of their Chao use it, and the status balloon shows their battle levels |
| 65 | BattleChallenge | to the host only, reliable | to u8, format u8 (0 singles, 1 doubles), actual levels u8 (1: each Chao battles at its own battle level; 0: every Chao at level 50, both games for both teams), dex u32, team: the sender challenges player `to` to a live battle (see Live battles). `dex` is a fingerprint of the catalog data the battle runs on and the battle rules version (`battle::online::dex_print`). A team is count u8 (1..=6), count × (0x800 bytes `ChaoData` (must pass `ChaoData::validate`), species u16, form u8, types u8×2, ability u8, moves u16×4, level u8 (1..=100): the Chao's battle profile, 0 for none, with the same checks as Partners) |
| 66 | BattleOffer | from the host, reliable | from u8, format u8, actual levels u8 (as the challenge's): player `from` challenges the receiver |
| 67 | BattleAnswer | to the host only, reliable | to u8 (the challenger, or the starter of a multi battle the sender is invited to), answer u8 (0 decline, 1 accept, 2 busy: their game can't take a challenge now), dex u32, team (with count 1..=6 when accepting, at most 3 for a multi battle; 0 otherwise) |
| 68 | BattleStart | from the host, to both players, reliable | battle u16, format u8, side u8 (the receiver's: 0 the challenger, 1 the accepter), opponent u8, seed u64, the opponent's team |
| 69 | BattleChoice | to the host only, reliable | battle u16, step u16, check u32 (the sender's battle checksum before this step), count u8 (≤ 2), count × pick: the sender's choices for this step (none when they have nothing to choose; in a multi battle only the picks for the sender's own place count). A pick is pos u8 (0..=1), kind u8 (0 move, 1 the out-of-PP move, 2 switch, 3 send in for a fainted fighter), value u8 (a move index 0..=3, 0 for kind 1, a team member 0..=5), target u8 (side × 2 + pos, 0xFF none), pass u8 (the team member a passing move hands on to, 0xFF none) |
| 70 | BattleStep | from the host, to the battle's players, reliable | battle u16, step u16, seed u64, count u8 (≤ 4), count × (side u8 (0..=1), pick): every player's choices for the step (in a multi battle those of the players still in it; none for the places the computer plays) and its rolls' seed |
| 71 | BattleLeave | to the host only, reliable | battle u16 (0: before a battle starts, taking back the sender's challenge, calling off their multi battle, or turning down (or leaving after accepting) a challenge or invite to them), reason u8 (0 the battle reached its end, 1 gave up, 2 left the battle) |
| 72 | BattleEnd | from the host, reliable | battle u16 (0: a challenge or multi battle that never started), reason u8 (0 busy, 1 declined, 2 the catalog data differs, 3 forfeit: the opponent gave up, 4 disconnect: the opponent (a multi battle's starter) left the battle or the lobby, 5 timeout, 6 desync, 7 withdrawn, 8 replaced: the receiver took too long to choose in a multi battle and the computer plays their place), winner u8 (the winning side, 2 nobody) |
| 73 | WildHello | to the host only, reliable, on connecting and when it changes | flags u8 (only bits 1 see: SharedVisitors on and the catalog read; 2 pick: WildVisitors on too; 4 machine: LinkMachine on too), print u32 (the catalog's visitor print, `net::wild_print`; non-zero exactly with bit 1). See Shared wild visitors |
| 74 | WildList | from the host (0xFF), every 6 frames, unreliable | area u8 (1..=3), serial u16 (one more for each list the host sends; a game drops a list not newer than the last one it took, while that one is under 1.5 s old), section u8 (the garden's wild area in the host's clock window: a type habitat, by the type id its creatures share; 0xFF none yet: no pick this window), print u32 (its visitors' print; for an empty list with no area its picker's, 0 with no picker either), picker u8 (a player id, 0xFF none), learners u16 (a bit per owner id 0..=15 whose Chao can learn from these visitors now), count u8 (≤ 30), count × entry: id u16 (not 0, each once), creature, origin u8 (0 wild, 1 a partner), bringer u8 (the partner's bringer, a player id; 0xFF for a wild one), place u8 (0 on the ground, 1 swimming, 2 in a player's hands, 3 battled by a player), by u8 (that player's id, 0xFF for 0 and 1), v f32×3 (places 0, 1, 3: its world position; 2: its offset in the holder's frame, each axis ≤ 10), angle u16 (BAMS). A **creature** is species u16 (1..`SPECIES_COUNT`), form u8 (≤ 31, an ordinary form), level u8 (1..=100), ability u8 (an ability id, < `ABILITY_COUNT`), shiny u8 (0/1), moves u16×4 (each < `MOVE_COUNT`, at least one not 0, none twice): 14 bytes, and an entry 34. Ids in the list are unique |
| 75 | WildAsk | from the host to a garden's picker, reliable | area u8, ask u16, section u8 (0xFF: the garden's area in window `slot`), land u8, pond u8 (land + pond 1..=5), fresh u8 (0/1: the garden just became shared; the picker may hand over its own visitors where they are), slot u32 (the host's clock window: Unix seconds, UTC, / 600) |
| 76 | WildPick | to the host only, reliable | area u8, ask u16, section u8 (not 0xFF), count u8 (≤ 5), count × (creature, place u8 (0 the host puts it on the grass, 1 in the pond, 2 on the grass at v, 3 swimming at v), v f32×3, angle u16) |
| 77 | WildBring | to the host only, reliable | area u8, key u64 (the partner's key in the sender's store), creature, place u8 (0 the host puts it, 2 at v), v f32×3, angle u16: a partner of the sender's comes into the garden (from the link machine, or one waiting there) |
| 78 | WildCarry | to the host only, reliable | area u8, creature, offset f32×3 (each axis ≤ 10): the sender comes into the garden holding a visitor from elsewhere |
| 79 | WildTake | to the host only, reliable | id u16: the sender picks up that visitor |
| 80 | WildHeld | to the host only, about 10 a second while holding, unreliable | id u16, mode u8 (0 in hands: v is the offset in the sender's frame, each axis ≤ 10; 1 let go of: v is its world position), v f32×3 |
| 81 | WildPut | to the host only, reliable | id u16, at f32×3, angle u16: the visitor the sender let go of is on the ground |
| 82 | WildAbsorbed | to the host only, reliable | id u16, owner u8 (a player id or 8..=15), index u8 (< 4): that Chao took the visitor from the sender |
| 83 | WildBattle | to the host only, reliable | id u16, step u8 (0 reserve it for a battle, 1 won, 2 lost or gave up, 3 backed out before the fight) |
| 84 | WildAnswer | from the host (0xFF) to one player, reliable | kind u8 (0 take, 1 carry, 2 bring, 3 battle), id u16 (the visitor's; 0 when refused before it had one), key u64 (a bring's key, else 0), ok u8 (0/1) |
| 85 | WildTaught | from the host (0xFF) to every session of a Chao's owner, reliable | identity u64 (`ChaoData::identity`), origin u8 (0 wild, 1 a partner), creature: that Chao absorbed this visitor |
| 86 | WildUsed | from the host (0xFF) to a partner's bringer, reliable | garden u8 (1..=3), key u64: their partner waiting in that garden was absorbed and waits no more |
| 87 | MultiInvite | to the host only, reliable | actual levels u8 (as a challenge's), dex u32, team (count 1..=3), then 3 × seat for places 1..=3 (the sender's ally, foe 1, foe 2): kind u8 (0 a player: id u8 (< 8); 1 the computer: team, count 1..=3). The sender starts a multi battle as place 0 (see Multi battles) |
| 88 | MultiOffer | from the host, reliable | from u8 (the starter), place u8 (1..=3, the receiver's), actual levels u8, players u8×4 (each place's player, 0xFF the computer): the receiver is invited; they answer with `BattleAnswer` to `from` |
| 89 | MultiStatus | from the host to a multi battle's starter, reliable | place u8 (1..=3), status u8 (0 accepted, 1 declined, 2 busy (away, in another battle, or a bad answer), 3 didn't answer in time, 4 left after accepting, 5 has a different catalog): what the player invited to that place did. A place not taken is open |
| 90 | MultiFill | to the host only, reliable | place u8 (1..=3), team (count 1..=3): the starter gives an open place to the computer |
| 91 | MultiStart | from the host, to each player of a multi battle, reliable | battle u16, seed u64, place u8 (0..=3, the receiver's), actual levels u8, then 4 × (player u8 (0xFF the computer), team (count 1..=3)): every place's player and team |
| 92 | MultiTakeover | from the host, to the other players of a multi battle, reliable | battle u16, place u8 (0..=3), reason u8 (3 gave up, 4 left the battle or the lobby, 8 too slow to choose): that place's player is gone and the computer plays it from the next step on |

## Skins (`net/files.rs`, `skin_sync.rs`)

A host shares a folder of skins: the dedicated server its `SA2CO_SKINS` folder (`/skins` in
Docker, looked at again every minute), a player hosting their own `skins/`. Skin requests are
answered straight to the asker, never relayed or refereed, and a client gets at most 2048
chunks a second. On joining, a client asks for the list page by page, keeps the files it
already has the same of (size and hash), and fetches the rest a file at a time with up to 64
chunk requests out (lost ones asked again after 400 ms). A finished file is checked against
its hash and written whole (side file, then renamed) under `skins/`; the skins are then read
again, and every model, motion and sound file is still checked when it's loaded. A path is
`<skin id>/<parts>` with plain names only (no `..`, no leading `.`, no backslashes). While it
downloads, the menus show a progress bar. A new skin's textures are found by the mod loader
only at start, so new skins (not changed ones) need a restart; the menus say so.

Players announce their skin with SkinWorn. A player wearing an animated or solid skin the receiver has
is drawn with it, animated from their PlayerState (position, action, status, spin ball);
skins built on a character's skeleton (rigged) show the character to others.

`PlayerState.flags` (`game::POSE_*`): 1 spin ball (jump), 2 blending into `anim`, 4 waiting at
a synced start, 8 "start now", 16 in a Chao Race the state is the player's racing Chao (position
and rotation y are the Chao's, `frame` its race progress), 32 busy: in a live karate fight, a
Chao battle, a live battle's challenge or a multi battle's invites.

`PlayerState.room`: (0xFF, 0) = none, (creator id, kind) = in that player's room; kind 1 =
Race, 2 = Co-op (the creator's setting; any non-zero kind is a room). A room exists
while its creator is in it (`room.rs`).

Strings: u8 length + UTF-8, no control characters. Player ids must be < 8; a Chao's owner (in
ChaoSounds, ServerChao, ChaoLive, ChaoTouched, ChaoChanges, ChaoHeld and `ItemPlace::Chao`) may
also be 8..=15, a Chao the host holds (HeldChao). Trailing bytes are an error.

`game_build` = PE `TimeDateStamp` + `SizeOfImage` read from the exe header in memory
(`game::game_build`); the supported build is `E8 1A 60 50 00 C0 3F 02`.

## The host's referee (net/referee.rs)

The host (the dedicated server, or a hosting player) decides what must be the same for
everyone. It reads every `Game` message before passing it on:
- `GardenItems`: who owns each item, and whether its owner is using it (held, or a Chao has it).
- `ItemClaim`: the first claim for an item that isn't in use wins; the answer is `ItemOwner`.
  Claims aren't passed on.
- `HuntClaim`: the first claim for each piece of a room's hunt wins; answered with `HuntTaken`.
  A `HuntSet` with different pieces for that stage starts over.
- `ItemOwner` and `HuntTaken` from a player are dropped: only the referee sends them.

Co-op messages (`StageKills`, `KeyUsed`, `Shot`, `Switch`, `Checkpoint`, `BossHp`) are applied only
from players in the receiver's room. BossHp values are at most 1000.

Referee answers are sent with a stand-in `from` (a client ignores messages "from" itself).

### Live battles (net/battles.rs, battle/online.rs)

The host has no catalog and runs no battle: both players' games run the same battle, and the host
pairs them up, holds each step's choices and decides its rolls. All its battle messages come
from 0xFF; the same bodies from a player are dropped, and players' battle messages are never
passed on.
- `BattleChallenge` from a player who is in no challenge or battle, to a player in the lobby who
  is in none either, goes to the target as `BattleOffer`; otherwise the challenger gets
  `BattleEnd` busy. So does a challenge to the same player within 5 seconds of the last one.
  An offer waits 2 minutes for its answer (then `BattleEnd` timeout to both).
- `BattleAnswer`: declined or busy ends the challenge for the challenger. An accept with a
  different `dex` ends it for both (the catalog data differs). Otherwise the battle starts:
  `BattleStart` to both, with one battle id and seed, the challenger as side 0 and each with
  the other's team.
- Both games build the battle the same way: side 0's team, then side 1's, each Chao's fighter
  from its record and profile at the challenge's level rule (its own level, or 50), the seed
  from `BattleStart`, then the battle's start. The accepter's game shows it with the sides
  swapped.
- Each later decision point (every turn's choices, every round of sending in for fainted
  fighters) is a step, numbered from 1. Both games send `BattleChoice` for it. Once both are in,
  different checksums end the battle for both (desync, no winner); otherwise both get
  `BattleStep` with a new random seed. Each game then reseeds the battle with it and plays both
  sides' picks; a side whose picks the battle refuses gets the battle AI's choices, from that
  seed, in both games.
- A player who gives up (`BattleLeave` 1) loses: the other gets `BattleEnd` forfeit with them
  the winner. Leaving the battle (`BattleLeave` 2) or the lobby is a disconnect, also a loss,
  except before anyone chose for step 1: then the other gets `BattleEnd` withdrawn (the
  challenger left) or declined (the accepter left), with no winner. Once one player has chosen
  for a step, the other has 2 minutes however long the step was open (then `BattleEnd` timeout
  to both, the one who chose winning); a step nobody chooses for in 10 minutes ends with no
  winner.
- `BattleLeave` 0 with a battle id says the sender's game reached the battle's end; nothing is
  sent for it, as each game knows the result. That player is free for other challenges at once,
  and the battle ends on the host once both games have said so. If the other game has chosen
  for a step, or chooses after, it gets `BattleEnd` desync with no winner.
- The battle never touches either save.

### Multi battles (net/battles.rs, battle/online.rs)

Doubles with four places, each a player's or the computer's: place = side × 2 + pos, the
starter is place 0 (side 0), their ally place 1, the foes places 2 and 3. Each place has its own
team of 1..=3 Chao and plays only its own.
- `MultiInvite` from a player in no challenge or battle, inviting each player at most once and
  not themselves, sets one up; otherwise the starter gets `BattleEnd` busy. Each invited player
  in the lobby and in nothing else gets `MultiOffer`; any other invited place is open at once
  (`MultiStatus` busy). Nobody in a setup can be challenged or invited elsewhere.
- An invited player's `BattleAnswer` to the starter: an accept with the same `dex` and at most 3
  Chao takes the place (`MultiStatus` accepted); anything else opens it (`MultiStatus` with why;
  another catalog or too big a team also sends that player `BattleEnd`). `BattleLeave` 0 from an
  invited player opens their place (declined, or left once accepted); from the starter it calls
  the battle off (`BattleEnd` withdrawn to the others; disconnect when they leave the lobby).
  The starter fills an open place with `MultiFill`. An invite waits 2 minutes for its answer
  (then `BattleEnd` timeout to that player, `MultiStatus` timeout to the starter); a setup not
  started in 5 minutes ends with `BattleEnd` timeout for everyone in it.
- Once every place is a player's who accepted or the computer's, `MultiStart` goes to each
  player with one battle id and seed. Every game builds the battle the same way: side 0's team
  is place 0's Chao then place 1's, side 1's place 2's then place 3's (fighters as for a live
  battle, under the level rule); each place sends in, switches to and passes on to only its own,
  and a place whose own Chao are all down stays empty. Each game shows its own side on the left.
- Each step: every player still in the battle sends `BattleChoice`; the host keeps the picks
  for the sender's own place. Once all are in, different checksums end it for everyone (desync);
  otherwise `BattleStep` goes to them all with a new seed. Each game plays every place with
  picks from them; a place with none (the computer's), or whose picks the battle refuses, gets
  the battle AI's choices from that seed and place, the same in every game.
- A player who gives up (`BattleLeave` 1), leaves the battle (2) or the lobby, or takes more
  than 2 minutes to choose once someone has (they get `BattleEnd` replaced) is out of it: the
  others get `MultiTakeover`, and the computer plays that place from then on. The battle ends in
  the games, when a side has no Chao left; each game's `BattleLeave` 0 says so, as for a live
  battle (a game that chooses after another said so gets `BattleEnd` desync). A step nobody
  chooses for in 10 minutes ends it for everyone (`BattleEnd` timeout, no winner).

## Shared wild visitors (net/wild_host.rs, net/wild_print.rs)

While the host runs a garden, its wild visitors are the same for everyone in it: the host
keeps the garden's list and moves the visitors, on the ground players sent (`GardenLand`). It
never has the catalog: it keeps and sends ids and positions, and every game names, draws and checks
them from its own catalog. All the visitor messages a player sends are for the host only (never
passed on); the ones the host sends, from a player, are dropped.

- **Print**: a catalog's visitor print (`visitor_print`) is FNV-1a over the battle data
  (`dex_print`), every species' record and level-up learnset, and every type habitat's wild
  tables with its type id. Never 0. A player whose `WildHello` has no *see*, or whose print
  differs from a garden's visitors' (while it has none, its picker's), is never sent its list,
  never asked and never taught from it.
- **Picker**: of the players settled in the garden (their state not flagged 128, so a game still
  loading in isn't asked for a garden it isn't in yet) with *see* and *pick* and the garden's
  print (any print while the list is empty), the one there longest, the lower id on a tie, worked
  out every frame. When they leave, the next one is picker at once; the list, the area and the
  cap stay. An ask to a player who isn't picker any more is dropped and asked again of the new
  one; one unanswered for 5 s is dropped and asked again 1 s later.
- **Area**: a garden's area follows the host's clock in 10-minute windows (Unix seconds, UTC,
  / 600). Every ask carries the host's window, and for section 0xFF the picker picks from the
  garden's area in that window (`rotation::area_at`), so the picker's own clock doesn't matter.
  The first pick of a window sets the garden's area (the list's section); later asks in that
  window name it. When the host's window changes it forgets the area (the list says 0xFF until
  the next pick) without ending the run or dropping visitors, and the next ask is for the new
  window's. A pick answering an ask from the window before is still taken but doesn't set the
  area. A new run has no area until its first pick, which is the window's area again.
- **Coming**: a run starts with a cap of 6..=7 wild visitors, 3 more for each player in the
  garden after the first, at most 20 (`scaled_cap`), and asks for 4 at once (fresh while nothing
  has been picked this run, within 10 s of its start); then for one at a time, 20..50 s apart,
  while there are fewer than the cap and room. Room: the list and the garden's animal and drive
  items (category 2) and 2 more stay within 32 animal entries (the mod raises the game's 10),
  and a list has at most 30 (`MAX_WILD_LIST`). With no room the host logs it once. Each
  visitor of a pick is added only while there is room. A pick from someone else, for another ask,
  with more land or pond visitors than asked, another section than asked or (not fresh) placed
  ones is dropped whole. The host puts land visitors on a land spot at least 25 from every player
  in the garden when one allows, pond ones at a pond cell with water on three sides; placed ones
  stay where they are when a land spot (place 2) or pond cell (place 3) is within 15. Land spots
  within 8 of the link machine's spot (the one every game finds in the same ground) are left out.
- **Moving**: on the ground a visitor stands 60..240 frames, then walks at 0.15 a frame to a land
  spot within 30, never onto a water cell, on the floor; players closer than 3 push it aside. A
  swimmer swims from pond cell to pond cell. A player whose state has action 42 (whistling)
  calls every visitor on the ground or swimming within 100 for 6 s (the nearest whistler wins):
  it walks to within 4 of them at 0.3 a frame, or swims to the pond cell nearest them. A held one
  is in its holder's hands (their position and turn, its offset) or, let go of, where their
  `WildHeld` says.
- **Going**: a wild visitor stays 300..480 s, then goes once on the ground or swimming with every
  player in the garden at least 60 away; one below y −50 goes at once. A partner goes when its
  bringer is settled elsewhere, leaves or stops seeing visitors. When the run ends the list goes
  (but for battled visitors, below).
- **Hands**: `WildTake` on a visitor on the ground or swimming, from a player settled in its
  garden with its print, within 30 of it and holding no other, is granted (the first one wins);
  everything else is refused. `WildHeld` and `WildPut` count only from the holder; a position more
  than 40 from the holder is taken as the holder's. A put visitor lands on the floor under it, and
  swims when that is a pond cell under the surface. Held with no `WildHeld` for 2 s: put down where
  it is. Its holder settled in another area: a wild one still in their hands leaves the garden with
  them (gone for everyone); a partner is put down where they last were, and one let go of where
  their `WildHeld` had it. Its holder disconnecting: put down there.
- **Carry and bring**: `WildCarry` from a player settled in the garden with *see* and its print
  (an empty list's: its area's or picker's, or any with neither), holding none and with no other
  carry since they came into the garden, adds it in their hands while there are fewer than 5 wild
  visitors and room. `WildBring` from a player there with *see* and *machine* and the print (the
  same way), with fewer than 4 partners of theirs there and none with that key, adds it: at v when
  v is within 15 of the machine's spot and the floor is within 10 under it, else where the host
  puts it. With no room the roaming wild visitor farthest from every player goes first; with
  none to send away it's refused.
- **Absorbing**: `WildAbsorbed` counts when the sender holds the visitor (or let it go less than
  2 s ago and nobody has taken or reserved it since) and the Chao is in that garden's run, awake, within 25 of it. The visitor is then gone
  for everyone, and a partner's bringer gets `WildUsed`. Every session of the Chao's owner with
  *see* and the visitor's print gets `WildTaught`, unless the Chao is locked against the sender
  (Locked Chao; never against its owner). `learners` in the list has the bit of each owner id
  in the garden (players there, awake held Chao) one of whose sessions has *see* and the print.
- **Battles**: `WildBattle` 0 on a wild visitor (never a partner) on the ground or swimming, from a
  player settled in its garden with its print, within 30 and with no other reservation, reserves
  it (`WildAnswer`): it stands where it is and can't be taken or reserved. Only that player's
  result counts, from any area: won or backed out, it's free again where it was; lost, it goes
  for everyone. The player disconnecting frees it; a reservation older than 15 minutes counts as
  lost. If the run ends meanwhile, the garden keeps its battled visitors (not moved, not sent, no
  asks; the others go, as the remaining player's game keeps them as its own) until the
  reservation ends and 3 minutes after, and a run on the same ground takes them up again, its
  first ask fresh.
- **The link machine**: while someone in a garden the host runs has *see* and *machine*, the
  host's Chao walk round the machine's box at the machine's spot.

## Held Chao (net/roster.rs, net/chao_host.rs, held.rs)

The host keeps what each player brings (`Brought`) by their key, and on the dedicated server in
`SA2CO_DATA/chao/<key>.txt` (one file per key: name, last seen, lock, then each Chao's garden, row,
copy number, place and record as a `.sa2chao` in hex; written like `gardens.txt`, dropped a week
after the player was last online). A Chao is (key, `ChaoData::identity`, nth), never its DNA alone.

In a garden the host runs, the Chao of players who aren't in it are shown under owner ids
8..=15 (index 0..=3, a player's 4 brought rows; 32 ids in all over the three gardens, so at
most 32 held Chao are shown at once and the rest wait): the present players' own first, then
held ones, most recently online first, at most 4 of each player, 32 a garden. A garden runs with one player
when held Chao of other players wait there (two otherwise; a player's own held Chao are theirs
again when they come in, so a garden with only them and their own Chao is their game's). A run
goes on 15 s after a second player leaves it; one only ever run for held Chao goes back to the
player's game as soon as none wait there.
- Owner online elsewhere with their save loaded: awake, run as any Chao (life tick included);
  what changes goes to them as SaveChao. A cocoon that opens evolves it there
  (`chao_sim::evolve`): its new record goes to everyone (HeldChao) and to its owner (SaveChao).
  It never dies there: at the end of its life it waits.
- Owner offline (or their save not loaded): asleep, lying where it was left (motion 0x13, eyes
  7), out of the server's world; players can carry and throw it (ChaoHeld, ChaoTouched) and it
  stays where it lands. Nothing else of it changes, and changes sent for it are dropped.
- Owner comes into its garden: their game has it again (their stream for it, or 5 s); the held
  copy goes (HeldChao `has` 0) and their game runs it.

A connected player is in the garden their last PlayerState put them in, however long ago: a game
paused on a tab-out sends nothing but is still there, so its garden keeps running for them, their
Chao stay theirs (their stream counts while it was heard within 8 s of their last state) and no
held copy of them is made. Only a state elsewhere or leaving moves them. A held Chao is never
shown in a garden where any session of its owner's key is.

## Locked Chao (net/chao_host.rs, garden.rs, protocol `ChaoLock`)

A player's lock (`Brought` lock) says when others' doings are saved to their Chao. Where the
owner is (`lock_place`): a garden 1..=3, elsewhere in Chao World (level 90: lobby, races,
Kindergarten, karate, stadium), or nowhere (another level, a menu, offline). Always never locks;
ChaoWorld locks while the owner is nowhere in Chao World; ThatGarden locks while the owner isn't
in the Chao's garden; Never always locks. A Chao is never locked against its owner's own ids.

A PlayerState with flag 128 (a load between places) keeps the owner's last place for 10 s, or
until a settled state says where they are. Disconnecting counts as nowhere at once.

A locked Chao behaves the same for everyone: it minds whoever holds it, whistles or is nearest,
plays its held reactions, lands upset when thrown and enjoys petting. What changes is what's
saved. The host:
- keeps a second, saved record while the Chao's current activity is with a player it's locked
  against (holder, thrower or the player its senses picked). The brain runs on the live record;
  the saved one gets only its own life ticks and its owner's changes. When the activity ends
  the live record is reset to the saved one;
- sends the saved record in everything that reaches the owner (ChaoChanges, ChaoLive,
  SaveChao, HeldChao);
- drops other players' ChaoChanges for it (not applied, not passed on) and the mood of their
  ChaoTouched (the position still counts);
- lets it take only its owner's items and the server's;
- tells each player which Chao are locked against them (`ChaoLocked`).

On the other player's game a copy locked against them (or a held one asleep) never reacts to
their items (its fruit, animal and seed checks say no) and lets go at once of one it got hold
of, unbitten. It sends nothing, and each frame its record is put back from the one its brain
gave it (`ChaoLive`), so its status balloon shows the saved values. Its name tag shows a padlock.

Before the server runs a garden each owner's game runs its own Chao, so their natural life goes
straight into their save; the only ChaoChanges others send are touches, which a lock drops.
