# Network protocol (v20)

Code: `src/net/`. All multi-byte values are little-endian. Every received byte goes through
the bounds-checked `wire::Reader`; bad data is dropped, never trusted. Players need the same
`PROTOCOL_VERSION`, mod version and game build to join.

## Transport (`udp.rs`)

Packet header (14 bytes): `magic "SA2O" u32 | version u8 = 1 | kind u8 | epoch u32 | seq u32`.

| kind | meaning |
|---|---|
| 0 unreliable | payload delivered as-is (player state, karate state, heartbeats) |
| 1 reliable | payload = `fragment index u8 | fragment count u8 | data`; acked, resent every 150 ms (40 tries), delivered in order |
| 2 ack | acks `seq` |

- Max packet 1200 bytes; reliable messages up to 32 fragments (~37 KB). A Chao (2 KB) is 2 fragments.
- `epoch` is random per process. A new epoch from a known address means the peer restarted: both directions reset.
- At most 16 links (connected players included); the session drops anything that isn't a valid
  Hello. A link that has heard nothing for 20 s and has nothing left to deliver is dropped.
- A reliable packet too far ahead of a missing one (256+) isn't kept or acked; it's sent again.

## Session (`session.rs`)

Star: the host is player 0 and assigns 1..=7 (a dedicated host, `sa2co-server`, isn't a
player: it's left out of `Welcome` and assigns 0..=7). Clients talk only to the host; the host relays
`Game` messages to everyone else and **overwrites `from`** with the sender's real id.

- Join: the client sends `Hello` (reliable). The host checks protocol version, mod version and
  game build, then answers `Welcome` (your id + all players) and broadcasts `PeerJoined`;
  otherwise `Reject` with a reason (e.g. "lobby full").
- Heartbeat every 1 s each way; 6 s of silence = gone (`PeerLeft`). `Bye` on a clean exit.
  Clients rejoin 3 s after losing the host.

## Messages (`protocol.rs`)

| tag | message | fields |
|---|---|---|
| 1 | Hello | protocol u16, mod_version str≤16, game_build [u8;8], name str 1..10 |
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
| 1 | PlayerState | 30 Hz, unreliable | time u32 (sender frame), level i16, mission u8, chao_area i8, character u8, position f32×3 (finite, \|v\| ≤ 1e6), rotation i32×3 (BAMS), anim u16, frame f32 (finite, \|v\| ≤ 1e5), flags u8, room (owner u8, kind u8), upgrades u32 (`CharObj2Base::Upgrades`, for mech upgrade parts), action u8 (EntityData1 action), status u8 (1 on the ground, 2 holding something): the last two drive skins' animations |
| 2 | Chao | reliable | index u8 (0..=1 garden Chao, 2 = My Chao), 0x800 bytes `ChaoData` (must pass `ChaoData::validate`) |
| 4 | RoomPick | reliable | level i16 (1..=100), mission u8 (0..=4), character u8 (0..=15): a room player's Stage Select pick |
| 5 | RacePick | reliable | category u8 (0..=4), course u8 (0..=12), level u8 (0..=11): a room player's race |
| 6 | KaratePick | reliable | opponent u8: a room player starts a live VS fight against this player |
| 7 | KarateState | ~30 Hz, unreliable | clock u16, 2 × (position f32×3, rotation i32×3, hp f32, morale f32, motion u8) |
| 8 | KarateEnd | reliable | code u8 (2 knockout/ring-out, 3 time up), result u8 (0/1 winning side, 2 draw), hp f32×2 |
| 9 | KarateCheer | reliable | count u8: the member cheered (A) this many times |
| 10 | StageKills | reliable | level i16 (0..=100), mission u8 (0..=4), count u16 (≤ 0x600), then count × u16 object index (< 0x600): co-op room, these stage objects (`SETObjectData` record indices, re/stage-objects.md) were destroyed. New kills as they happen; the whole list when a room player enters the stage (sent by the room players already in it) |
| 11 | GardenChao | 30 Hz, unreliable | owner u8, count u8 (≤ 4), then per Chao: index u8 (< 4), position f32×3, rotation_y i32, motion u16, held u8 (0/1), offset f32×3, mood (current state i16, timer i32, length i32), eyes i16, mouth i16, props u8×3 (per attach point 4/11/35: 0 none, else 1 + index into `garden::PROPS`, ≤ 20), drum u8 (size 0..=255, 0 none), cocoon u8 (0 none, 1 + kind 0..=2): player `owner`'s visiting Chao as they are in the sender's game (re/chao-garden-sync.md). Each player sends their own (that's how the others know they visit); the garden's keeper also sends everyone else's, and the others' copies follow the keeper's; offset = where it is relative to the sender while held, in the sender's frame |
| 12 | ChaoChanges | reliable | owner u8, index u8 (< 4), count u8 (≤ 128), then count × (offset u16 (< 0x800), value f32): the sender's interaction changed `owner`'s visiting Chao; `chao::changes` deltas (or values for animal parts) for the owner to apply to the real one, and for the garden's keeper to apply to the copy it runs. The keeper also sends what its visitors' AI changed, about once a second |
| 13 | ChaoHeld | 30 Hz, unreliable | owner u8, index u8 (< 4), offset f32×3: the sender is holding `owner`'s Chao `index`; the game running it (the keeper's, or with nobody keeping the owner's) shows it in their hands |
| 14 | GardenItems | ~10 Hz, unreliable | area u8 (1..=3), count u8 (≤ 24), then per item: id (origin u8, serial u16), category u8 (2/3/7/9), kind u8, bites u8 (≤ 4), place u8 (0 ground, 1 held by the sender, 2 held/eaten by their visiting Chao, 3 in use unseen), chao u8 (place 2: Chao index < 2), position f32×3 (ground/unseen: where; held: offset in the holder's frame): items the sender owns in that garden (src/items.rs) |
| 15 | ItemClaim | reliable, to the host only | id: the sender picked up (or a Chao in their game took) item `id` and asks to own it. The host's referee answers; it is never passed on |
| 16 | ItemOwner | reliable, from the host | id, owner u8: item `id` belongs to `owner` (to everyone when a claim is granted, to the claimer alone when refused) |
| 17 | Chat | reliable | text str (1..=80 printable ASCII) |
| 18 | RaceStatus | ~4 Hz unreliable, reliable at the finish | level i16, mission u8, progress u16 (0..=10000, 0xFFFF unknown), finished u8 (0/1) [race u32, game u32 centiseconds]: room race status |
| 19 | Route | reliable | level i16, mission u8, count u16 (≤ 2000), count × f32×3: a stage route (first clear), for race positions |
| 20 | KartPick | reliable | course u8 (0..=2): a room player started a Kart Race |
| 21 | KartState | 30 Hz unreliable | position f32×3, rotation i32×3, yaw i32, wheel i32, kind u8 (2..=7), accel u8 (0/1, bright exhaust), tilt i32×3 (body tilt sums), frame u16 (the sender's game frame, wrapping; repeats and late ones are dropped, speeds come from it): the sender's kart |
| 22 | HuntSet | ~1 Hz unreliable | level i16, mission u8, 3 × (kind u8, id u8, position f32×3): the room's treasure hunt pieces |
| 23 | HuntClaim | reliable, to the host only | level, mission, sig u32, slot u8 (0..=2): we collected this piece. Not passed on |
| 24 | HuntTaken | reliable, from the host | level, mission, sig, slot, player u8: who got the piece first (everyone; a late claimer alone) |
| 25 | KeyUsed | reliable | level i16, mission u8, key u8 (0/1): co-op room, the sender's stage key went into its keyhole; the others' keys go in too (from the ground), so their doors open |
| 26 | Shot | ~unreliable, per shot | level i16, mission u8, kind u8 (0 bullet, 1 bazooka), position f32×3, direction f32×3: co-op room, the sender's mech fired; the others fire the same shot |
| 27 | Switch | reliable | level i16, mission u8, id u8 (< 32), on u8 (always 1; off isn't shared, since a death restart clears every switch): co-op room, a stage switch turned on; the others turn it on too (their doors and gates follow) |
| 28 | Checkpoint | reliable | level i16, mission u8, record u16 (< 0x600), position f32×3, rotation i32×3: co-op room, the sender passed this checkpoint standing there; the others light it and restart there if they have no checkpoint yet or it's further along the stage's route |
| 29 | Ball | 10 Hz unreliable | area u8 (1..=3), claim u16, position f32×3, velocity f32×3: shared garden, the ball as whoever kicked or held it last has it (a higher claim takes over; equal claims go to the lower id) |
| 30 | BossHp | reliable | level i16, mission u8, hp f32, full f32: co-op room, the sender's boss is down to `hp`; the others' boss with the same `full` drops to it (never below a sliver: the last hit is each player's own) |
| 31 | SkinWorn | reliable, on change and every 10 s | skin str ≤ 24 (a skin id: `[a-z0-9_]`, or "" for none): what the sender wears; nothing shows on screen |
| 32 | SkinListGet | reliable, to the host | page u16 |
| 33 | SkinList | reliable, from the host | page u16, pages u16, count u8 (≤ 48), then count × (path str ≤ 96, size u32 (≤ 16 MB), hash [u8;8] FNV-1a 64): the host's shared skin files (file index = page × 48 + position) |
| 34 | SkinGet | unreliable, to the host | file u16 (< 1024), offset u32 |
| 35 | SkinChunk | unreliable, from the host | file u16, offset u32, len u16 (1..=1024), data |
| 36 | GardenTrees | 1 Hz unreliable | area u8 (1..=3), 7 × 8 bytes: shared garden, the sender keeps this garden's trees (the lowest player id there) and these are its 7 tree records as their save has them (kind, state, growth, life, fruit ×3, facing); the others show copies of them (`trees.rs`) |
| 37 | TreeAction | reliable | area u8 (1..=3), slot u8 (< 7), action u8 (0 plant, 1 shake), kind u8 (the seed's): the sender planted in or shook their copy of that tree; its keeper does it for real (a planted seed, or one shake of ripe fruit) |
| 38 | GardenEggs | ~10 Hz unreliable | area u8 (1..=3), count u8, count × (index u8 (< 4), position f32×3, held u8 (0/1)): the sender's visiting eggs (their Chao index) where they lie, or held by the sender (`eggs.rs`) |
| 39 | EggHeld | reliable | owner u8, index u8 (< 4), held u8 (0/1), position f32×3: the sender picked up or put down a copy of player `owner`'s egg `index`; the owner's game holds the real one there |
| 40 | ToyMode | reliable | area u8 (1..=3), toy u8 (0 TV, 1 radio, 2 jack-in-the-box), mode u8 (<= 12): the sender's toy jumped to that mode. From the toys' keeper (the garden's lowest player id), the others' toy jumps too; from anyone else (a touch), the keeper makes the jump and sends it on (`garden_toys.rs`) |
| 41 | GardenKeeper | 4 Hz unreliable | area u8 (1..=3), term u32: the sender keeps that garden (runs its Chao, trees and toys; `keeper.rs`). The others follow the claim with the higher term (then the lower id); with nobody claiming for a moment the lowest id in the garden claims with a higher term |

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

Players announce their skin with SkinWorn. A player wearing an animated or solid skin we have
is drawn with it, animated from their PlayerState (position, action, status, spin ball);
skins built on a character's skeleton (rigged) show the character to others.

`PlayerState.flags` (`game::POSE_*`): 1 spin ball (jump), 2 blending into `anim`, 4 waiting at
a synced start, 8 "start now", 16 in a Chao Race the state is the player's racing Chao (position
and rotation y are the Chao's, `frame` its race progress), 32 in a live karate fight (busy).

`PlayerState.room`: (0xFF, 0) = none, (creator id, kind) = in that player's room; kind 1 =
Race, 2 = Co-op (the creator's setting; any non-zero kind is a room). A room exists
while its creator is in it (`room.rs`).

Strings: u8 length + UTF-8, no control characters. Ids must be < 8. Trailing bytes are an error.

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
