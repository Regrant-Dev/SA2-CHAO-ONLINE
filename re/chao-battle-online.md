# Chao battles: trainers from the catalog and battles between two games

Facts for battles against the catalog's trainers and live battles between two players' games.
Everything comes from the player's own local catalog (`src/creature/`) at runtime; nothing from
it is kept in the repo.

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
- What a battle reads from the catalog (`team::Dex`): every move record, the type chart, the move a
  fighter uses with no PP left and the sound-move list. Names and descriptions only show on the
  HUD. Two games with the same mod version run the same engine code (players need the same mod
  version to join a lobby), so the catalog's battle data is the only input that can differ.
- A Chao's fighter comes from its record (stats, grades, happiness) and its battle profile
  (types, ability, four moves: `creature_link::battle_profile`, by `ChaoData::identity`). The name
  only shows on the HUD.
- `Fighter`, `Field` and the engine's other state types derive `Debug` without addresses or
  floats, so the same battle state prints the same text in both games.

## 2. The trainer ladder (`creature::ladder`, `battle::ladder`)

- `Rom::ladder` takes every trainer whose team the battles can run: a single battle, a party,
  ordinary forms with a record, supported abilities and at least one move, every move known to
  the catalog. Each is ranked by its strongest creature's level, then by id.
- The strongest is the champion, the next four the league (in rank order). Eight milestones are
  then picked for the levels 12, 20, 28, 36, 44, 52, 60 and 68 (the first trainer at or above
  each, keeping enough trainers for the milestones after it). The trainers at or below a
  milestone's level (and above the one before) are its practice tier; the ones left over are a
  ninth tier.
- A trainer's creatures (`Rom::trainer_party`) fight at their own level with the trainer's IVs,
  no EVs, the record's nature (or a fixed one from the trainer and species ids when the record
  leaves it random) and the species' base friendship. The ability is the one the party record
  selects, else one picked from the trainer and species ids; an empty ability slot falls back to
  the first.
- The first milestone and the first practice tier are open from the start. Milestone k and
  practice tier k open once milestone k−1 is beaten; the first league trainer once the last
  milestone is, each later one after the one before, and the champion after the last league
  trainer. Progress is the set of beaten trainer ids in `<mod>/creature/trainer-progress.txt`.

## 3. The host and messages between two players

- Clients only talk to the host. The host passes each `Game` message through its referee
  (`net/referee.rs`, `Referee::observe`) before relaying it to everyone else; `observe`
  returning false keeps it from the others. The referee's own messages (`Out`) go to everyone
  (`To::All`) or one player (`To::One(id)`), reliably, with a stand-in `from` when the receiver
  would otherwise see its own id. A hosting player's own messages go through the same referee
  (as id 0), and referee messages to id 0 become its own events.
- The dedicated server has no catalog and no game: the referee can order, hold back, compare and
  time messages, and make random numbers (`RandomState`-seeded, as the link epochs are), but
  cannot run a battle.
- `Referee::left(id)` runs when a player drops (Bye, or 6 s without a heartbeat); every other
  player also gets `PeerLeft`. `Referee::tick` runs on every poll of the host.
- A reliable message is at most 32 fragments of about 1180 bytes each (about 37 KB). A Chao
  record is 0x800 bytes, so six records with their battle profiles (15 bytes each: species,
  form, types ×2, ability, moves ×4, level) are about 12.4 KB.
- `PlayerState.flags` has no free bit; 32 (`POSE_KARATE_BUSY`) marks a player as busy at
  karate, and karate's opponent list leaves busy players out.
