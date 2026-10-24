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
cargo test --target i686-pc-windows-msvc --lib menu_textures  # Windows: native x86 hook ABI
```

The updater tests exercise archive validation, DLL checksums, preserving player files, and
rolling back an interrupted installation. CI runs them on Windows too. The Steam wrapper
tests capture arguments, working directory and Steam's environment using a fake executable;
they also cover Steam library/preload isolation, older updaters and failed updates. They
never launch the game. The Unix updater test runs a child process to verify host downloads
do not inherit Steam's library overrides. For a real update check without launching, run the installed
`sa2co-launcher` (or `.exe`) with `--update-only` while the game is closed.

The menu recovery tests validate GVM bounds and repeated header restoration. Set `SA2_DIR`
to the game folder to include the two locally installed menu archives. Windows also runs
the actual naked x86 shims with controlled codec/parser callees and every stack alignment;
this uses no game process or saves. For gameplay verification, leave the main menu idle,
switch focus away and back, and enter/leave Stage Select repeatedly. The log should show
`menu textures: checked PRS fallback installed`; `recovered stageMap`/`stageMapBG` means
the guarded fallback was actually exercised. See [native evidence](../re/menu-textures.md).

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
| `FAKEPEER_ROOM=1` | Create its own party (join it with F9 > Party). Without it, fakepeer copies your state and so is in whatever party you create. |
| `FAKEPEER_ROOM_PICK=level,mission,char` | Pick this stage every 30 s (reaches you in its party) |
| `FAKEPEER_RACE_PICK=cat,course,level` | Pick this Chao Race every 30 s (reaches you in its party) |
| `FAKEPEER_KARATE=1` | 5 s after you reach the karate area (in its party), start a live karate fight against you and replay the recorded fight, or a pretend one (once per run) |
| `FAKEPEER_KARATE_FILE=<file>` | Where a live fight you lead against fakepeer is recorded (default `target/karate-fight.rec`) |
| `FAKEPEER_BATTLE=accept` | Accept your live Chao battle challenges and multi battle invites with its `FAKEPEER_CHAO` Chao (Normal, basic moves; up to 3 for a multi battle), choosing with the battle AI for its own Chao. `singles` or `doubles`: also challenge you 5 s after you reach the karate area (again a minute after each battle). `multi`: invite you to a multi battle as its ally against two computer places (copies of its Chao) at those times; `multi-foe`: as foe 1, the other two places the computer's. A place nobody takes it gives to the computer |
| `FAKEPEER_ROM=<file>` | The GBA ROM for live battles and shared wild visitors (the same as your game's; default `~/Games/gba/emerald.gba`) |
| `FAKEPEER_WILD=<mode>[,<mode>]` | Share the garden's wild visitors with you (its ROM's visitor print, seeing, picking, with the machine). `pick`: answer the host's asks from its ROM (it picks while it was in the garden first). `take`: every 30 s pick up the visitor nearest it, hold it 10 s, put it down; with `carry` it goes to the lobby holding it instead and comes back 10 s later; with `absorb` it hands it to your nearest Chao. `whistle`: whistle 2 s every 20 s. `battle-won` / `battle-lost`: every 40 s reserve the nearest visitor, go to the karate area and send that result 25 s later (longer than a run with one player in it lasts), then come back. `hostile`: every 5 s send visitor messages with unknown ids, picks without an ask and values out of range |
| `FAKEPEER_WILD_LEAVE=<secs>` | With `FAKEPEER_WILD`: go to the lobby for good after that many seconds (your game takes over picking) |

Rebuild and restart it after any protocol change (`PROTOCOL_VERSION`, docs/protocol.md).
In game: F9 > Multiplayer = Join, Host address = `127.0.0.1:43902`, Save & Start.

## Checklist

- **Lobby:** connect/join/leave toasts; stopping fakepeer shows "left" and play goes on.
- **Party toasts:** "X created a party", "X joined the party", "X left the party", "X's party closed".
- **Puppets:** the fake player appears with a name tag; other characters with `FAKEPEER_CHAR`.
- **Parties:** create a party (fakepeer joins by copying you): a stage freezes at load and shows
  "Players: You, fakepeer"; F8, Start or F9 > Start now starts it (3-2-1-GO). With `FAKEPEER_ROOM=1`,
  its party is listed in F9 > Party and you can join it; stopping fakepeer closes it.
- **Pause in a synced stage** keeps the stage running; elsewhere pause freezes as normal.
- **Stage Select:** `FAKEPEER_ROOM=1 FAKEPEER_ROOM_PICK=...`, join it and wait on Stage
  Select; or create a party and pick a stage (the fakepeer log shows the pick).
- **Chao visits:** entering a garden brings Chao; visitors appear and are never saved.
- **Chao Race:** friends' Chao take CPU lanes with labels; `FAKEPEER_ROOM=1
  FAKEPEER_RACE_PICK=...`, join, wait at the race entrance with the course list open; the start
  holds until you press F8; its lane follows your Chao after the gun.
- **Practice karate** (Battle DLC): F9 > Practice karate `fakepeer's Chao (copy)`, then any
  karate fight.
- **Live karate, leading:** create a party (fakepeer joins by copying you), go to Karate
  and pick a fight as usual; it becomes a live fight against fakepeer, holds until you press F8
  ("Opponent: fakepeer"), then runs, and fakepeer records it.
- **Live karate, following:** `FAKEPEER_ROOM=1 FAKEPEER_KARATE=1`, join its party and wait
  at the first karate menu; the fight starts by itself and replays the recorded fight.
- **Live Chao battle:** `FAKEPEER_CHAO=... FAKEPEER_BATTLE=accept`: in the battle menu, your
  team, then "fakepeer (live)"; it accepts and the battle plays with "Waiting for fakepeer..."
  between your choices; its log shows each step and the result. `FAKEPEER_BATTLE=doubles`: the
  challenge pops up on the karate menu; accept it and you play side 1 (your Chao still on the
  left).
- **Multi battle, one fakepeer:** `FAKEPEER_CHAO=... FAKEPEER_BATTLE=accept`: battle menu >
  Multi battle, up to 3 Chao, Ally = "fakepeer (live)", the foes the computer, Send the invites;
  its log shows `accepting player N's multi battle invite to place 1` and `multi battle ...
  starts at place 1`. Only your Chao get menus; "Waiting for fakepeer..." between your choices;
  every Chao has a box. Put fakepeer at Foe 1 instead: your side is on the left, it's against
  you. `FAKEPEER_BATTLE=multi` (or `multi-foe`): its invite pops up on the karate menu ("fakepeer
  invites you to a multi battle!", your place and the rules); accept with up to 3 Chao.
- **Multi battle, several fakepeers:** one hosts and the others join it, each with its own name
  and `FAKEPEER_BATTLE=accept`:

  ```sh
  FAKEPEER_CHAO=a.sa2chao FAKEPEER_BATTLE=accept target/.../fakepeer host 43902 fp1
  FAKEPEER_CHAO=b.sa2chao FAKEPEER_BATTLE=accept target/.../fakepeer client 127.0.0.1:43902 fp2
  FAKEPEER_CHAO=c.sa2chao FAKEPEER_BATTLE=accept target/.../fakepeer client 127.0.0.1:43902 fp3
  ```

  Join the game to it; set up you and fp1 against fp2 and fp3 (four players), or you against fp1
  and the computer with fp2 as your ally, and so on. Stop one fakepeer mid-battle: the others
  show "fp2 left the battle; the computer plays their place." and the battle plays on to its
  end with the same result in every log. Stop one after it accepted but before the others did:
  its place shows "left", and A on it gives it to the computer; with `multi`, fakepeer does that
  itself when you decline.
- **Multi battle, computer only:** Multi battle with the computer in all three other places:
  Start plays it in your game alone (no XP).
- **Chat:** T, type, Enter; fakepeer answers "heard: ...".
- **Shared wild visitors:** `FAKEPEER_WILD=pick` with SharedVisitors on and the same ROM: in
  a garden the server runs, the visitors fakepeer picks show as copies (its log: `wild: picked
  N for garden G`); with `FAKEPEER_WILD_LEAVE=60` your game picks the next ones. `take`, `carry`:
  the one it holds is in its puppet's hands, and carried out it's gone. `absorb`: your
  partners.txt changes only when F9 "Others can change my Chao" allows fakepeer. `battle-won`,
  `battle-lost`: fakepeer's puppet leaves the garden and the visitor shows busy; 15 s later the
  garden is yours alone (its other visitors stay as yours, the battled one goes), and when
  fakepeer comes back it's shared again with your visitors where they were and the battled one
  back (won) or not (lost). `hostile`: nothing changes and
  nothing crashes. In your game the link machine is there; a partner you bring out of it, and
  the ones waiting in that garden, join the visitors (log: `gba link: species S (level L) came
  to shared garden G`), and one you hand to your own Chao teaches it (`a Chao absorbed a wild
  species`). When fakepeer leaves and the server stops running the garden, the visitors about
  stay as your own (`the garden is ours again`).
- **Party battles:** main menu 2P button (PARTY BATTLES) > Kart Race (fakepeer's kart, no CPU karts) and
  Treasure Hunt.

These need a second real player (fakepeer only copies your state):

- **Co-op stage** (a party in Co-op mode): enemies, boxes, breakables and rings someone takes are
  gone for both; keys and switches open both doors; mech shots show. One passes a checkpoint,
  the other dies and restarts there (log: `coop: checkpoint N passed by a friend`).
- **Co-op boss:** both hit it; the bar drops for both, and each one's last hit defeats it
  (log: `bosses: boss up`, `bosses: a friend's hit`).
- **Shared garden:** kick the ball, it rolls the same way for the other (log: `toys: we have the
  ball`); fruit piles up no faster than alone; Chao instruments and toys show on the copies.
- **Kart Race:** the other kart's exhaust brightens on the accelerator and it tilts over jumps.
