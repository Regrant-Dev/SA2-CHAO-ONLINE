# Testing (developers)

Players: see [how-to-play.md](how-to-play.md).

Always launch through Steam. `scripts/install.sh` builds, backs up `SAVEDATA` to
`~/sa2-savedata-backups/` whenever it changed, and installs into the game's
`mods/sa2-chao-online/`. Log: `mods/sa2-chao-online/sa2-chao-online.log`.
Crash dumps: `CrashDump/` in the game folder (`tools/mdmp.py <dump>`).

```sh
cargo test --target x86_64-unknown-linux-gnu        # unit + localhost network tests
cargo clippy --release                              # the game DLL (i686)
cargo clippy --release --target x86_64-unknown-linux-gnu --all-targets --features fakepeer
cargo test --target x86_64-unknown-linux-gnu --features launcher --bin sa2co-launcher
python3 scripts/test-steam-launch.py
```

The updater tests exercise archive validation, DLL checksums, preserving player files, and
rolling back an interrupted installation. CI runs them on Windows too. The Steam wrapper
tests capture arguments, working directory and Steam's environment using a fake executable;
they also cover Steam library/preload isolation, older updaters and failed updates. They
never launch the game. The Unix updater test runs a child process to verify host downloads
do not inherit Steam's library overrides. For a real update check without launching, run the installed
`sa2co-launcher` (or `.exe`) with `--update-only` while the game is closed.

## The fake player (one PC)

`fakepeer` joins or hosts a lobby and plays a second player. Once it hears from you it copies
your state half a second later (40 units to the side; a racing Chao trails without offset).

```sh
cargo build --release --target x86_64-unknown-linux-gnu --features fakepeer --bin fakepeer
target/x86_64-unknown-linux-gnu/release/fakepeer host 43902 fakepeer       # game joins it
target/x86_64-unknown-linux-gnu/release/fakepeer client 127.0.0.1:43902    # game hosts
```

In client mode fakepeer exits if nobody is hosting yet; start hosting first, or keep it
retrying: `while true; do target/.../fakepeer client 127.0.0.1:43902 fakepeer; sleep 3; done`.

| Variable | Effect |
|---|---|
| `FAKEPEER_CHAR=<id>` | Appear as this character instead of copying yours (0 Sonic, 4 Knuckles, ...) |
| `FAKEPEER_CHAO=<file>[,<file>]` | Bring these `.sa2chao` Chao (hatched only) to gardens, races and karate |
| `FAKEPEER_ROOM=1` | Create its own room (join it with F9 > Room). Without it, fakepeer copies your state and so is in whatever room you create. |
| `FAKEPEER_ROOM_PICK=level,mission,char` | Pick this stage every 30 s (reaches you in its room) |
| `FAKEPEER_RACE_PICK=cat,course,level` | Pick this Chao Race every 30 s (reaches you in its room) |
| `FAKEPEER_KARATE=1` | 5 s after you reach the karate area (in its room), start a live karate fight against you and replay the recorded fight, or a pretend one (once per run) |
| `FAKEPEER_KARATE_FILE=<file>` | Where a live fight you lead against fakepeer is recorded (default `target/karate-fight.rec`) |

Rebuild and restart it after any protocol change (`PROTOCOL_VERSION`, docs/protocol.md).
In game: F9 > Multiplayer = Join, Host address = `127.0.0.1:43902`, Save & Start.

## Checklist

- **Lobby:** connect/join/leave toasts; stopping fakepeer shows "left" and play goes on.
- **Room toasts:** "X created a room", "X joined the room", "X left the room", "X's room closed".
- **Puppets:** the fake player appears with a name tag; other characters with `FAKEPEER_CHAR`.
- **Rooms:** create a room (fakepeer joins by copying you): a stage freezes at load and shows
  "Players: You, fakepeer"; F8, Start or F9 > Start now starts it (3-2-1-GO). With `FAKEPEER_ROOM=1`,
  its room is listed in F9 > Room and you can join it; stopping fakepeer closes it.
- **Pause in a synced stage** keeps the stage running; elsewhere pause freezes as normal.
- **Stage Select:** `FAKEPEER_ROOM=1 FAKEPEER_ROOM_PICK=...`, join it and wait on Stage
  Select; or create a room and pick a stage (the fakepeer log shows the pick).
- **Chao visits:** entering a garden brings Chao; visitors appear and are never saved.
- **Chao Race:** friends' Chao take CPU lanes with labels; `FAKEPEER_ROOM=1
  FAKEPEER_RACE_PICK=...`, join, wait at the race entrance with the course list open; the start
  holds until you press F8; its lane follows your Chao after the gun.
- **Practice karate** (Battle DLC): F9 > Practice karate `fakepeer's Chao (copy)`, then any
  karate fight.
- **Live karate, leading:** create a room (fakepeer joins by copying you), go to Karate
  and pick a fight as usual; it becomes a live fight against fakepeer, holds until you press F8
  ("Opponent: fakepeer"), then runs, and fakepeer records it.
- **Live karate, following:** `FAKEPEER_ROOM=1 FAKEPEER_KARATE=1`, join its room and wait
  at the first karate menu; the fight starts by itself and replays the recorded fight.
- **Chat:** T, type, Enter; fakepeer answers "heard: ...".
- **Room battles:** main menu 2P button > Kart Race (fakepeer's kart, no CPU karts) and
  Treasure Hunt.

These need a second real player (fakepeer only copies your state):

- **Co-op stage** (a room in Co-op mode): enemies, boxes, breakables and rings someone takes are
  gone for both; keys and switches open both doors; mech shots show. One passes a checkpoint,
  the other dies and restarts there (log: `coop: checkpoint N passed by a friend`).
- **Co-op boss:** both hit it; the bar drops for both, and each one's last hit defeats it
  (log: `bosses: boss up`, `bosses: a friend's hit`).
- **Shared garden:** kick the ball, it rolls the same way for the other (log: `toys: we have the
  ball`); fruit piles up no faster than alone; Chao instruments and toys show on the copies.
- **Kart Race:** the other kart's exhaust brightens on the accelerator and it tilts over jumps.
