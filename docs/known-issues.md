# Known issues

## Open

### Mixed characters in one stage can hide a puppet
Other characters' models/motions share the game's index tables. If a remote player's
character needs slots the local character already uses (e.g. Sonic in a mech stage), the mod
doesn't load it and that player isn't drawn (log: `puppets: can't show character N here`).
Before this check existed it crashed (2026-10-14, MechTails stage, 0x42C304).

### Walker extra parts aren't drawn
Mechs and Chao walkers draw their body only; parts using the shared WALKTEX (CharObj2+0x444)
are skipped.

### Chao fights and races outside a room are simulated per player
"Practice karate" and races outside a room run on each player's own game, so results
can differ between screens. In a room, races follow each friend's real Chao and karate is one
shared fight. A friend's race lane shows their My Chao, which may not be the Chao they picked
to race with.

### Intermittent hang on "Now Loading" (seen once, 2026-10-14)
2 of 3 early runs with the mod froze on "Now Loading"; later runs were fine. Possibly a
Proton/loader race. If it happens, leave the game open and get every thread's stack:

```sh
PX="$HOME/.local/share/Steam/steamapps/common/Proton - Experimental/files"
WINEPREFIX="<Steam library>/steamapps/compatdata/213610/pfx" \
  "$PX/bin/wine" winedbg   # then: info process, attach 0x<pid>, bt all
```

Always launch through Steam.

## Fixed

- **A controller disconnect stopped the game.** The game opens its own "Reboot to change the
  control scheme" dialog when the number of connected pads changes, and skips the whole game
  update until it's answered. While connected, the mod stops that dialog from opening
  (re/control-scheme-dialog.md); untested: whether a newly connected pad takes over without a
  restart.
- **Remote Sonic had no blue spin ball.** The ball and trail are Sonic's "JumpAura" effect
  (re/spin-ball.md); puppets now draw it from a "jumping" flag.
- **Lobby dropped during long loads.** The game runs no frames while loading, so networking
  now runs on its own thread (online.rs `pump`); clients rejoin 3 s after a disconnect.
- **Puppet crashes from mismatched model/motion pairs.** Blending poses draw their target
  animation at frame 0; per-character texture lists are read from each character's loader
  (re/characters.md).
- **Room race broke the camera / Chao slid before the start.** Friends' lanes are only moved
  while the race runs, keep their own lane, and only their position is set.

Crash dumps: `CrashDump/` in the game folder; parser: `tools/mdmp.py`.
