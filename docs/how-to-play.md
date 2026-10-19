# How to play

## Install

1. Install SA Mod Manager with the SA2 Mod Loader (<https://github.com/X-Hax/SA-Mod-Manager>).
2. Extract the release zip into the game's `mods` folder and enable **SA2 Chao Online** in SA
   Mod Manager.
3. Start the game. Multiplayer is off until you turn it on.

Everyone playing together needs the same version of the mod. SA Mod Manager checks for a new
version when it starts and asks before installing it; your settings are kept.

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
| Party | Create, join or leave a party |
| Mode | Race or Co-op (set by whoever made the party) |
| Go to | Jump to a friend |
| My Chao | The Chao you race and fight with |
| Save Chao garden now | Save your Chao World data immediately while in a garden |
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

**Skins for everyone:** put skin folders (as in `mods/sa2-chao-online/skins/`) in a folder on the
server and mount it at `/skins` (Docker: add `-v /path/to/skins:/skins:ro`). Everyone who joins
gets them in the background, with a progress bar on the main menu; brand-new skins show after a
restart. Others then see you wearing your skin. Only share skins with your own group: they're
other people's characters.

## Playing together

You see everyone who's in the same stage or garden as you.

**Chat:** press **T**, type, Enter to send.

**Go to:** F9 > Go to, pick a friend. If you're in the same stage or garden you're moved next to
them. From Stage Select you're taken into their stage.

**Parties:** one player creates a party in F9 > Party and the others join it. Parties are for stages, races and Karate; gardens are shared with everyone in them, party or not. In a party:

- Stages wait at the start until everyone there has loaded, then anyone presses **F8** (or
  Start) for a 3-2-1 countdown. This works in story mode too, each on your own save. If the
  others already started before you loaded, your countdown starts by itself.
- A stage someone picks on Stage Select takes along everyone in the party who's on Stage Select.
- **Race mode:** everyone has their own enemies. Your position shows at the top right.
- **Co-op mode:** enemies, item boxes and other breakable things someone destroys are gone for
  everyone. A ring one of you picks up is gone for both, and you both get it. Keys and
  switches open everyone's doors and gates, and you see each other's mech shots. A checkpoint
  one of you passes counts for everyone: if you fall behind and die, you restart there, unless
  you passed a checkpoint further along yourself.
  Bosses share one health bar: every hit anyone lands counts, and each of you lands your own
  final hit.

**Party battles:** while online, the main menu's 2P button becomes **PARTY BATTLES**:

- **Kart Race:** pick a course and your party races each other, without CPU karts.
- **Treasure Hunt:** pick a Knuckles or Rouge hunting stage. Everyone hunts the same 3 pieces and
  whoever grabs a piece first gets it.

Tabbing out pauses your game like normal SA2: you stay connected and the others see you
stand still (the garden's keeper passes to someone else meanwhile). Choose Fullscreen,
Borderless or Windowed in SA Mod Manager; the mod keeps your display choice. If an earlier
build left you in a small custom window, select Fullscreen or Borderless again in SA Mod
Manager before launching with the updated mod. Pausing during a party stage, or in Chao World
with others around, doesn't stop the game for anyone.

## Chao

**Saving:** F9 > Save Chao garden now updates your active Chao save file, including when save
redirection is enabled. Gardens also autosave every five minutes spent in them, with a confirmation
toast. Autosaving waits until loading finishes; a manual save resets the five-minute timer.

**Visiting:** when you're in a garden with friends, up to 4 of your Chao from that garden visit
theirs. Pick them in F9 > Visitor 1 to Visitor 4, or leave them on Auto (your My Chao first).
A Chao only visits while you're in the garden it lives in, and the same Chao can't fill both
rows. Visiting Chao show who they belong to over their heads, like "Mason 1" (Mason's visitor 1)
or "You 2", with the Chao's name if it has one. Eggs and Chao without a tag stay in your game
only; an egg that hatches can visit right away.

You can pet, hold and feed each other's Chao, and that affects the real Chao in its owner's
save. Picking one up interrupts petting on the other screens too. Garden items are shared:
pick up a friend's fruit and it's yours. The garden ball is
shared: kick it and it rolls the same way for everyone. Trees drop ripe fruit by themselves for
only one of you, so fruit doesn't pile up twice as fast; shaking a tree works for everyone.
With friends in a garden, one player's game runs it (whoever was keeping it already, else the
lowest player number): every Chao there, the trees and the toys. So everyone's Chao meet, play,
fight and even have babies together, and whatever happens to your Chao lands in your save. A
baby made there is laid in that player's garden. If they leave or tab out, someone else takes
over after a moment.

Everyone sees that player's trees, and planting a seed plants it for everyone. The TV, the radio
and the jack-in-the-box are shared too: switch one on and it's on for everyone. Whistle and your
friends' Chao come to you. Visiting eggs show up for everyone (only the owner's egg hatches).

Set **Visit friends** to Off and your Chao go home within a few seconds.

**Chao Race:** friends' Chao run in the other lanes of your races. In a party, everyone at the
race entrance races together live.

**Karate** (needs the Battle DLC): in a party, start a karate fight and it becomes a live fight
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
