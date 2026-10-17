# SA2 Chao Online

Online multiplayer for **Sonic Adventure 2** (Steam PC), up to 8 players, as an
[SA2 Mod Loader](https://github.com/X-Hax/sa2-mod-loader) mod.

- **See each other** in stages and Chao gardens: every character.
- **Rooms:** create or join a room and play together: every stage starts together (story mode
  too, each on your own save), a Stage Select pick or Chao Race takes the room along, and
  anyone can challenge anyone in the room to live karate.
- **Chao:** your Chao visit friends' gardens, race in their Chao Races (live in a room race), and
  fight in karate, including **live karate** where both players see the same fight.
- **Your save is yours.** Other players only ever get copies of Chao data; nothing they do is
  written to your save.

Everything is in one in-game menu: **F9**. Multiplayer is off until you turn it on.
An optional **dedicated server** (Docker image or Windows exe) keeps a lobby up
without anyone's game hosting it.

## Players

Download the zip from **[Releases](../../releases/latest)**, extract it into the game's `mods`
folder, enable "SA2 Chao Online" in SA Mod Manager, and read **[how to play](docs/how-to-play.md)**
(hosting, joining, Tailscale or port forwarding, every menu option). The same zip works on
Windows and on Linux (Proton). You need your own Steam copy of Sonic Adventure 2; nothing from the
game is included.

Karate needs the game's Battle DLC, like in the normal game.

## Developers

| | |
|---|---|
| [docs/setup.md](docs/setup.md) | Building on Windows and Linux, tests, tools |
| [docs/testing.md](docs/testing.md) | The fake second player and an in-game checklist |
| [docs/protocol.md](docs/protocol.md) | Network messages |
| [docs/game-version.md](docs/game-version.md) | The game build all addresses are for |
| [re/](re/addresses.md) | Reverse-engineering notes: addresses, hooks, game systems |

```
src/          the mod (Rust): lib.rs entry points, menu/hud UI, online + net/ networking,
              puppet/timeline (other players), together/room (synced starts, rooms),
              race/karate/karate_live/visitors (Chao), game/patch (game memory and hooks)
src/bin/      sa2co-server (dedicated server) and fakepeer (a fake second player for testing)
mod/          mod.ini and the SA Mod Manager settings page (configschema.xml)
scripts/      build/install/package (install.sh + package.sh on Linux, windows.ps1 on Windows)
tools/        small analysis scripts (crash dumps, character assets)
```

## License and credits

MIT, see [LICENSE](LICENSE). Built on the SA2 Mod Loader and its community headers by X-Hax.
Sonic Adventure 2 is © SEGA; this project is not affiliated with SEGA and contains no game files.
