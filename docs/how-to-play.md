# How to play

## Install

1. Install SA Mod Manager with the SA2 Mod Loader (<https://github.com/X-Hax/SA-Mod-Manager>).
2. Extract the release zip into the game's `mods` folder and enable **SA2 Chao Online** in SA
   Mod Manager.
3. Start the game. Multiplayer is off until you turn it on.

Everyone playing together needs the same version of the mod. The optional Steam launcher
checks and installs new releases automatically before the game starts; configure it once
using [the setup instructions](https://github.com/Regrant-Dev/SA2-CHAO-ONLINE/blob/main/docs/setup.md#automatic-updates-when-launching-from-steam-windows).
Otherwise SA Mod Manager checks when it starts and asks before installing. Settings and
saves are kept. Updates take effect on the next game launch.

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
| Chao details | Read your Chao's hidden stats, personality, emotions, alignment and bonds |
| Save Chao garden now | Save your Chao World data immediately while in a garden |
| Bring Chao 1-4 | Which of your Chao you bring to a shared garden (4 at most; the rest stay home meanwhile): Auto, a Chao you pick, or None |
| Others can change my Chao | When what friends do to your Chao counts in your save: Always, While I'm in Chao World, While I'm in that garden, Never |
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

**Keep the shared gardens:** the server keeps the gardens' trees and items and the Chao players
left there (`chao/`, one file per player) in the folder `SA2CO_DATA` names, across restarts.
Docker: mount a folder the container can write at `/data` (`-v /path/to/data:/data`).

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
- **Co-op mode:** enemies, item boxes and supported breakable things someone destroys are gone for
  everyone. A ring one of you picks up is gone for both, and you both get it. Keys and
  switches open everyone's doors and gates, and you see each other's mech shots. A checkpoint
  one of you passes counts for everyone: if you fall behind and die, you restart there, unless
  you passed a checkpoint further along yourself.
  Bosses share one health bar. When a party member clears the fight, everyone in the same
  boss stage and mission gets the game's normal victory/results sequence. This covers all
  boss arenas, both Egg Golem fights and the character rival fights. A paused or dying player
  finishes after resuming or respawning; an empty HP phase alone does not finish the fight.
  Standard wooden and iron crates stay per player: they can be stepping stones, so a friend
  breaking one cannot remove your route. They respawn normally when you die or restart.

**Party battles:** while online, the main menu's 2P button becomes **PARTY BATTLES**:

- **Kart Race:** pick a course and your party races each other, without CPU karts.
- **Treasure Hunt:** pick a Knuckles or Rouge hunting stage. Everyone hunts the same 3 pieces and
  whoever grabs a piece first gets it.
  Shared hunts use fixed native spots, including buried and hidden treasure. Moving,
  enemy-held and upgrade-only choices are replaced with fixed candidates so every player
  has the same physical pickups. Retries keep the shared selection and pickup credit.

Tabbing out pauses your game like normal SA2: you stay connected and the others see you
stand still (the host keeps running the garden's Chao meanwhile). Choose Fullscreen,
Borderless or Windowed in SA Mod Manager; the mod keeps your display choice. Pausing during a
party stage, or in Chao World with others around, doesn't stop the game for anyone.

If you die while the online pause menu is open, choose **Resume** to finish the normal death
and respawn transition. Once a death/restart is pending, the local scene waits behind the
menu instead of continuing to update a dead character.

## Chao

**Jewel eggs:** built into the mod's Black Market progression. Normal restocks add one
randomly chosen jewel egg you have unlocked by collecting emblems, when the shop has space.
No F9 or Mod Manager setting is required.
Existing stock stays available; offers rotate with the game's usual restock timing.

| Egg | Emblems | Rings |
|---|---:|---:|
| Gold | 120 | 10,000 |
| Silver | 125 | 12,000 |
| Ruby | 130 | 15,000 |
| Sapphire | 135 | 18,000 |
| Emerald | 140 | 20,000 |
| Amethyst | 145 | 22,000 |
| Aquamarine | 150 | 24,000 |
| Garnet | 155 | 26,000 |
| Onyx | 160 | 28,000 |
| Peridot | 165 | 30,000 |
| Topaz | 170 | 35,000 |
| Pearl | 180 | 40,000 |

Buying, delivery, hatching and saving use the game's normal egg system.
The added names/descriptions use English in every language. A conflicting shop or message
mod can prevent activation; the reason appears in `sa2-chao-online.log`.

**Saving:** F9 > Save Chao garden now updates your active Chao save file, including when save
redirection is enabled. Gardens also autosave every five minutes spent in them, with a confirmation
toast. Autosaving waits until loading finishes; a manual save resets the five-minute timer.

**Detailed stats:** F9 > **Chao details** opens a read-only viewer. It starts with the Chao
you're holding if that Chao belongs to your save, otherwise your selected My Chao or another
Chao in your save. Select **Your Chao** with Up/Down and use Left/Right to browse. Select
**Page** and use Left/Right (or Enter for the next page). Esc/Backspace returns to F9.

Pages cover abilities (including luck and intelligence), happiness and evolution, emotions,
needs, temperament/personality, bonds with all six characters, appearance and health counters.
Values refresh while you look. Numbers are exact; gauges show each field's normal range.
Life counters use game units. Some hidden traits are unused or have limited effects in the
base game; a larger number isn't always better. Eggs have no traits until they hatch.
**Raw record** shows every saved byte, including unknown fields; PgUp/PgDn scrolls. Viewing
any page does not edit, export or save the Chao.

**Quick look:** in a garden, press **C** (or **D-pad up** on a controller) to open a Chao's
status balloon without picking it up: the Chao in front of you, or the nearest one close by.
Press again for the next nearest Chao; after the last one it closes. It also closes by itself
after 6 seconds. Works on friends' Chao too (with their live stats). It doesn't open while
you're holding something or another balloon is showing.

**Visiting:** when you're in a garden with friends, you each bring up to 4 of your Chao from that
garden. Pick them in F9 > Bring Chao 1 to 4, or leave them on Auto (your My Chao first,
then the first ones in your save); set a row to None to bring fewer. Your other Chao and eggs in that garden stay home while it's
shared: they aren't in the garden, so everyone sees the same Chao. Nothing happens to them, and
they're untouched in your save (like Chao in a garden you're not in). They come back when you're
alone in the garden again (about 20 seconds after the last friend leaves), when you go offline,
or the next time you enter the garden alone.
A Chao only visits while you're in the garden it lives in, and the same Chao can't fill two
rows. Visiting Chao show who they belong to over their heads, like "Player1 1" (Player1's visitor 1)
or "You 2", with the Chao's name if it has one. Eggs and Chao without a tag stay in your game
only; an egg that hatches can visit right away.

You can pet, hold and feed each other's Chao, and that affects the real Chao in its owner's
save, unless its owner said no (below). Picking one up interrupts petting on the other screens too. Garden items are shared:
pick up a friend's fruit and it's yours. The garden ball is
shared: kick it and it rolls the same way for everyone. Trees drop ripe fruit by themselves for
only one of you, so fruit doesn't pile up twice as fast; shaking a tree works for everyone.
With friends in a garden, the host (the dedicated server, or the player hosting) runs every
Chao there, so everyone's Chao meet, play and fight together, and whatever happens to your Chao
lands in your save. One player's game keeps the trees and toys (whoever was keeping them
already, else the lowest player number); if they leave or tab out, someone else takes over
after a moment.

Everyone sees that player's trees, and planting a seed plants it for everyone. The TV, the radio
and the jack-in-the-box are shared too: switch one on and it's on for everyone. Whistle and your
friends' Chao come to you. Visiting eggs show up for everyone (only the owner's egg hatches).

**Others can change my Chao** (F9, saved in `config.ini` as `OthersChangeChao`, changeable any
time) decides when what other players do to your Chao is saved:

| Setting | Saved when |
|---|---|
| Always (default) | always |
| While I'm in Chao World | you're anywhere in Chao World: a garden, the lobby, the Kindergarten (and Black Market), races or karate |
| While I'm in that garden | you're in the garden that Chao is in |
| Never | never |

When it isn't saved, your Chao is *locked* for them and shows a padlock next to its name. A
locked Chao still plays with everyone as usual: it comes to their whistle, enjoys being petted
and held, and gets upset when thrown. None of that reaches your save, and it won't take or eat
their items. Its own life in the garden is still saved.

Offline counts as being nowhere. A loading screen between two places in Chao World doesn't
count as leaving (for up to 10 seconds).

**Chao that stay:** on a server, the Chao you bring stay in their garden when you're not there.
While you're online somewhere else (another garden, a stage) they're awake whenever someone is
in their garden: they play, eat and grow there, can even evolve, and all of it goes straight
into your save. When you're offline they sleep where they were left, with a tag like
"Player1's Chao (asleep)": friends can pick them up and carry them about, nothing else happens to
them. When you come back into their garden they're yours to run again. Your save always has
your real Chao: the server only keeps a copy of the ones you bring, updated from your save
every time you join, and forgets them after a week without you. It knows them by the
`PlayerKey` the mod writes into your `config.ini` (keep it; updates do).

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
