# How to play

## Install

1. Install SA Mod Manager with the SA2 Mod Loader (<https://github.com/X-Hax/SA-Mod-Manager>).
2. Extract the release zip into the game's `mods` folder and enable **SA2 Chao Online** in SA
   Mod Manager.
3. Start the game. Multiplayer is off until you turn it on.

Everyone playing together needs the same version of the mod.

## The F9 menu

Press **F9** in game. Up/Down picks a row, Left/Right changes it, Enter does it, Esc closes.
Pause the game before typing, since letter keys also move your character.

| Row | What it does |
|---|---|
| Multiplayer | Off, Host or Join |
| Your name | Shown above you |
| Host address | When joining: the host's or server's address, e.g. `100.101.102.103` |
| Port | When hosting: the port to use (default 43902) |
| Name tags | Show names above players |
| Room | Create, join or leave a room |
| Mode | Race or Co-op (set by whoever made the room) |
| Go to | Jump to a friend |
| My Chao | The Chao you race and fight with |
| Visit friends | Whether your Chao visit friends' gardens |
| Visitor 1 / 2 | Which Chao visit: Auto, a Chao you pick, or None |
| Friends' Chao in races | Friends' Chao fill the other lanes in your Chao Races |
| Practice karate | Fight a copy of a friend's Chao |
| Save & Start / Apply / Stop | Connect, reconnect with new settings, or disconnect |

Settings save right away to `mods/sa2-chao-online/config.ini`.

## Hosting and joining

One player hosts and everyone else joins their address. Only the host needs to be reachable:

- **Tailscale** (easiest): everyone joins the same tailnet and uses the host's Tailscale address.
- **Port forwarding:** the host forwards UDP port 43902 to their PC and shares their public IP.

### Dedicated server

A server keeps the lobby up so nobody has to host. Everyone joins it with its address.

- **Docker:** `docker run -d --restart unless-stopped -p 43902:43902/udp ghcr.io/regrant-dev/sa2-chao-online-server:latest`
- **Windows:** run `sa2co-server.exe` from the release.

Update the server whenever you update the mod.

## Playing together

You see everyone who's in the same stage or garden as you.

**Chat:** press **T**, type, Enter to send.

**Go to:** F9 > Go to, pick a friend. If you're in the same stage or garden you're moved next to
them. From Stage Select you're taken into their stage.

**Rooms:** one player creates a room in F9 > Room and the others join it. In a room:

- Stages wait at the start until everyone there has loaded, then anyone presses **F8** (or
  Start) for a 3-2-1 countdown. This works in story mode too, each on your own save.
- A stage someone picks on Stage Select takes along everyone in the room who's on Stage Select.
- **Race mode:** everyone has their own enemies. Your position shows at the top right.
- **Co-op mode:** enemies and item boxes someone destroys are gone for everyone.

**Room battles:** while online, the main menu's 2P button becomes **ROOM BATTLES**:

- **Kart Race:** pick a course and your room races each other, without CPU karts.
- **Treasure Hunt:** pick a Knuckles or Rouge hunting stage. Everyone hunts the same 3 pieces and
  whoever grabs a piece first gets it.

While connected the game keeps running when you tab out, and pausing during a room stage
doesn't stop it for the others.

## Chao

**Visiting:** when you're in a garden with friends, up to 2 of your Chao from that garden visit
theirs. Pick them in F9 > Visitor 1 and Visitor 2, or leave them on Auto (your My Chao first).
A Chao only visits while you're in the garden it lives in. You can pet, hold and feed each other's Chao,
and that affects the real Chao in its owner's save. Garden items are shared too: pick up a
friend's fruit and it's yours.

Set **Visit friends** to Off and your Chao go home within a few seconds.

**Chao Race:** friends' Chao run in the other lanes of your races. In a room, everyone at the
race entrance races together live.

**Karate** (needs the Battle DLC): in a room, start a karate fight and it becomes a live fight
against a friend's Chao. Practice karate lets you fight a copy of a friend's Chao on your own.

Races and karate never change your Chao's save, win or lose.

**Chao files:** your Chao are saved as files in `chao/export`. Put a friend's `.sa2chao` files in
`chao/import` and they visit your garden, even when you're not online together.

## Problems

- **Can't connect:** check the address and port, and that the host is reachable. On Windows,
  allow the game through the firewall.
- **"rejected: mod ..." or "protocol ...":** someone has a different version. Everyone should get
  the latest release.
- **"can't listen on port":** another program, often a second copy of the game, is using the port.
- **Log file:** `mods/sa2-chao-online/sa2-chao-online.log`. Include it when reporting a problem.
