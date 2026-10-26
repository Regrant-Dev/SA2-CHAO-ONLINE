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

Press **F9** in game. Up/Down picks a row (hold to scroll), Left/Right changes it, Enter does
it, Esc closes. PgUp/PgDn move a page when the list is long. The selected setting stays in
view as players join or leave. Pause the game before typing, since letter keys also move your
character; hold Backspace to erase text.

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
| Chao details | Your Chao's stats: a summary of all seven abilities, evolution, alignment, life and battle level, then pages of details |
| Save Chao garden now | Save your Chao World data immediately while in a garden |
| Creature link | Your starter partner (or a reminder to use the link machine in a garden) |
| Creature battles | Battles against trainers, wild visitors and friends, or back to the garden |
| Bring Chao 1-4 | Which of your Chao you bring to a shared garden (4 at most; the rest stay home meanwhile): Auto, a Chao you pick, or None |
| Others can change my Chao | When what friends do to your Chao counts in your save: Always, While I'm in Chao World, While I'm in that garden, Never |
| Friends' Chao in races | Friends' Chao fill the other lanes in your Chao Races |
| Practice karate | Fight a copy of a friend's Chao |
| Save settings / Save & Start / Apply / Stop | Save while multiplayer is Off, connect, reconnect with new settings, or disconnect |

Settings save right away to `mods/sa2-chao-online/config.ini`.

## Hosting and joining

One player hosts and everyone else joins their address. Only the host needs to be reachable:

- **Tailscale** (easiest): everyone joins the same tailnet and uses the host's Tailscale address.
- **Port forwarding:** the host forwards UDP port 43902 to their PC and shares their public IP.

### Dedicated server

A server keeps the lobby up so nobody has to host. Everyone joins it with its address.

- **Docker:**
  ```
  docker run -d --restart unless-stopped -p 43902:43902/udp \
    -v /path/to/data:/data -v /path/to/skins:/skins:ro \
    ghcr.io/regrant-dev/sa2-chao-online-server:latest
  ```
  The container runs as user 65534, so the data folder must be writable by it:
  `chown 65534:65534 /path/to/data`. The skins folder is optional (see below).
- **Unraid:** add the container from the image above with the UDP port 43902 and two paths:
  container `/data` (read/write) on a host folder such as
  `/mnt/user/appdata/sa2-chao-online-server/data`, and optionally `/skins` (read only). Then
  `chown 65534:65534` the data folder and restart the container.
- **Windows:** run `sa2co-server.exe` from the release. To keep the gardens across restarts,
  set the environment variable `SA2CO_DATA` to a folder first (e.g. `set SA2CO_DATA=data`).

Update the server whenever you update the mod.

**Keep the shared gardens:** the server keeps the gardens' trees and items (`gardens.txt`) and
the Chao players left there (`chao/`, one file per player) in its data folder across restarts:
`/data` in Docker, the folder `SA2CO_DATA` names otherwise (none when it's not set). Each file
is rewritten whole when it changes, and a forgotten player's file is removed, so the folder
doesn't grow. Without a writable data folder the server still runs but forgets the gardens when
it restarts, and its log says "can't write /data/gardens.txt".

**Skins for everyone:** put skin folders (as in `mods/sa2-chao-online/skins/`) in a folder on the
server and mount it at `/skins` (Docker: add `-v /path/to/skins:/skins:ro`). Everyone who joins
gets them in the background, with a progress bar on the main menu; brand-new skins show after a
restart. Others then see you wearing your skin. Only share skins with your own group: they're
other people's characters.

## Playing together

You see everyone who's in the same stage or garden as you.

**Chat:** press **T**, type, Enter to send.

**Go to:** F9 > Go to, pick a friend. Their stage loads automatically from another stage,
Chao World, the main menu or Stage Select, even if you haven't visited that stage yet. You
join their party (and its Race/Co-op mode), load their mission and character, then appear next
to them once the stage is ready. If they're playing alone, you leave your previous party.
Chao World saves before you leave. A different party gets a fresh stage load so your old
party's objects and checkpoints don't carry over. If you're already in the same party and
stage, you're simply moved next to them. Going to another Chao World area still requires
being in Chao World; races and battles are joined through their entrances.

**Parties:** one player creates a party in F9 > Party and the others join it. Parties are for stages, races and Karate; gardens are shared with everyone in them, party or not. In a party:

- Stages wait at the start until everyone there has loaded, then anyone presses **F8** (or
  Start) for a 3-2-1 countdown. This works in story mode too, each on your own save. If the
  party already started before you loaded, you join the running stage without another countdown.
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
The jewel egg comes on top of the usual offers.

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

**Black Market restocks:** the game restocks the Black Market 3 to 7 minutes of play after
the last restock (stages, menus and Chao World all count). The new stock comes the next time
you open the market after that, picked by the game's own rules (eggs, fruit, seeds and hats by
your emblems) plus a jewel egg once you've unlocked them. While the market is open, the top
right shows "Next restock in m:ss", or "New stock on your next visit" once it's due (leave and
come back in). Turning on **Black Market restocks every 10 minutes** in the Mod Manager (Chao
World group; `BlackMarketRefresh` in `config.ini`) gives a steady 10 minutes of play instead:
time paused doesn't count, and it adds up across play sessions.

**Saving:** F9 > Save Chao garden now updates your active Chao save file, including when save
redirection is enabled. Gardens also autosave every five minutes spent in them, with a confirmation
toast. Autosaving waits until loading finishes; a manual save resets the five-minute timer.

**Detailed stats:** F9 > **Chao details** shows one of your Chao's stats. It starts with the
Chao you're holding if that Chao belongs to your save, otherwise your selected My Chao or another
Chao in your save. Left/Right turn the page, Up/Down scroll a long page, PgUp/PgDn show your
next or previous Chao and Esc/Backspace returns to F9. In a stage or garden a controller works
too: the D-pad or stick, L/R for another Chao and B to go back (while it's open your character
stands still).

The **Summary** page comes first:
- all seven abilities (Swim, Fly, Run, Power, Stamina, Luck and Intelligence), each with its
  grade letter in the grade's colour (E, D, C, B, A, S, X), level, points and a bar of its
  progress to the next level (some Chao have a Luck or Intelligence grade past X, which shows
  as "?" and still levels up);
- happiness and mood;
- alignment, and for a child its Run/Power and Swim/Fly leanings, each as a bar from its
  middle, with marks at 0.50 where the game starts counting it;
- a child's evolution progress and what it would become if its cocoon started now; an adult's
  form and how many times it has reincarnated;
- an adult's time left (in garden time) and whether it reincarnates or dies when it ends;
- its battle level with a bar of the XP toward the next, and its creature partner's types.

The other pages: **Stats** (the points each level adds at each grade; points stop at 4000),
**Growth** (the evolution rules, the Chaos checklist, age, lifespan and what rebirth needs:
happiness above 50 and stress under 7%), **Mood** (emotions and needs as percentages),
**Traits** (personality in words, which can change when it reincarnates, and the shop fruit
it refuses), **Bonds** (like and fear for each
of the six characters; above 75 fear it is scared), **Battle** (level, XP, partner types and
the moves it battles with), **Looks** (appearance and animal parts), **Health** and **Raw**
(every saved byte). Values refresh while you look. Eggs have no stats until they hatch. Type
and move names come from your creature link. Viewing a page doesn't change, export or save the
Chao.

**Quick look:** in a garden, press **C** (or **D-pad up** on a controller) to open a Chao's
status balloon without picking it up: the Chao in front of you, or the nearest one close by.
Press again for the next nearest Chao; after the last one it closes. It also closes by itself
after 6 seconds. Works on friends' Chao too (with their live stats). It doesn't open while
you're holding something or another balloon is showing.

**Status balloon:** the balloon that opens when you pick up or look at a Chao shows all seven
abilities: Luck and Intelligence come after the game's five, the same way (level, points and
progress bar), and each ability has its grade letter at the left. Under them:
- happiness (blue when happy, red when not) and alignment (Hero from +0.50, Dark from -0.50);
- for a child, its evolution progress and what it would become now; for an adult, its form and
  its time left (red when it wouldn't reincarnate);
- hunger and mood (a pressing need such as Sleepy, Lonely or Bored, else its strongest
  feeling, else Calm);
- its creature partner's types over its battle level.

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

**Chao files:** your Chao are saved as files in `chao/export`. Put a friend's `.sa2chao` files
in `chao/import` and they visit your garden, even when you're not online together. Exporting
also writes `partners.txt` with what the Chao learned through the creature link: put it in
`chao/import` with the friend's `.sa2chao` files so their Chao battle with their own moves,
types, ability and level. With Chao from more than one friend, add the lines of each friend's
`partners.txt` to the one already there.

## Creature link and battles

The creature link brings creatures from your own game cartridge into your Chao gardens. They
teach your Chao moves, types and an ability, and your Chao use them in turn-based creature
battles against trainers, wild visitors and friends. Everything else in the mod works without
it.

### Setup

You need a decrypted `.3ds` dump of your own **Pokémon Ultra Sun** cartridge (any region),
without the update applied. Other games, an eShop download or another `.cia` file don't work.
The mod doesn't decrypt or download anything.

1. Put the `.3ds` file in `mods/sa2-chao-online/creature/` (the mod makes that folder, with a
   "PUT YOUR GAME FILE HERE.txt" note inside, the first time the game starts), or set
   **Source game image** in SA Mod Manager (group **Creature link and battles**) to where it
   is. On Linux with Proton, paths look like `Z:\home\you\games\dump.3ds`.
2. Start the game. With your file in place, the mod prepares the creature link from it by
   itself in the background while you play; it needs about 7.2 GB of free disk space while it
   works, and the finished cache takes about 3.6 GB. Opening **F9 > Creature link** or a
   garden's link machine meanwhile shows "Preparing the creature link from your game file.
   This takes a few minutes." If you quit before it's done, it starts over the next time you
   play.
3. When "Creature link is ready" shows, open **F9 > Creature link** or the link machine.

The cache goes in `mods/sa2-chao-online/creature/cache/`, or in the folder you set as
**Private asset cache**. Your file and the cache stay on your computer: they're never part of
the mod, sent to friends or shared through the skin service. Some mod updates prepare the
creature link again the same way; next to an existing cache that needs about 3.6 GB free, since
the old cache is removed once the new one is ready. Deleting the `cache` folder doesn't help:
without it, preparing needs the full 7.2 GB. If there isn't enough space, free some elsewhere
on that drive, or set **Private asset cache** to a folder on another drive.

If your file can't be used, the message says why:

| Message | What to do |
|---|---|
| This file is encrypted. Use a decrypted copy of your own cartridge. | Use a decrypted dump |
| This isn't the supported game. | Use a dump of the game named above |
| Use the original cartridge dump, without the update. | Use the plain cartridge dump, not update data, a `.cia` or a dump with the update applied |
| Choose your game file in SA Mod Manager > Source game image, or put it in the mod's creature folder. | No `.3ds` file was found |
| Your game file changed while it was being read. Try again. | Leave the file alone while it's prepared |
| There isn't enough free disk space to prepare the creature link. | Free some space on the cache's drive |
| The creature link's cache folder can't be used. Check Private asset cache in SA Mod Manager. | Pick a new or empty folder you can write to, not the one with your game file |
| Your game file couldn't be read. Use a decrypted copy of your own cartridge, without the update. | Make a fresh dump |
| The creature link couldn't be prepared. Try again; if it keeps failing, include sa2-chao-online.log in a report. | Try again; your file is fine, so a new dump won't help |
| The creature link couldn't be prepared. Try again. | Try again |
| The creature link's importer is missing. Reinstall the mod. | Reinstall the release zip |
| The creature link's importer couldn't start. Reinstall the mod. | Reinstall the release zip, and check that antivirus software isn't blocking `sa2co-import.exe` |

The message shows as a notice when preparing fails, and again on the next page that needs the
creature link. After that, opening **F9 > Creature link** tries again; after replacing your game
file, the first open tries again.

Your partners and your Chao's learned choices are kept in
`mods/sa2-chao-online/creature/partners.txt`, and the trainers you've beaten in
`trainer-progress.txt` next to it. Your Chao save itself isn't changed by any of it.

### Partners and learned choices

Each garden has a **creature link machine** near its transporter. Walk up to it with empty
hands and press the action button. Its list has a starter gift (you can take one, once), and
then the starter you took. Pick it with A or Enter and it comes out of the machine as a
partner for your Chao (each once a day per garden, 4 at a time); B goes back. With
**Creature link machine in the gardens** turned off, **F9 > Creature link** shows the same
list, but it brings partners only in your own garden: in a garden the server runs, turn the
machine back on and use the shared one.

Carry a partner to a Chao to teach it moves, types and an ability. Chao walking past leave it
alone. The Chao remembers what it learned and battles with up to four moves, one or two types
and one ability (or none), on top of its raised SA2 stats. Choose **Moves, type & ability** at
the machine, or **Choose moves, type & ability** on the battle team screen, to change them.
`[x]` marks the active moves and types and `(o)` the active ability; **Done** goes back.
Changes are saved right away. A move battles can't use is marked "(unavailable)": you can
switch it off but not on, and when the creature link is ready, any such move that was on is
switched off for you, with a notice.

Long lists can be searched: type part of a name (case doesn't matter). Backspace deletes, Esc
clears the search before going back, and rows such as **Done** always show. Up/Down moves one
row and Left/Right ten; PgUp/PgDn moves a page at the machine and ten rows in battle lists. Going
back from a page keeps the search and position of the page before.

### Wild visitors

Each garden has a wild area that changes every ten minutes, on a clock everyone shares. An
area is a type habitat: creatures of one type, matched to the garden (grassy routes and woods
for the Neutral Garden; sea, sky and shore for the Hero Garden; caves, mountains and sand for
the Dark Garden). Each garden goes through all of its areas before any comes back.

The machine's **Who can come here** page shows each garden's current and next area, with its
visitors, their levels and chances. Land creatures roam, water creatures swim, and whistling
brings nearby visitors to you. Hand one to a Chao to teach it what its info card shows.

Online, everyone in a shared garden sees the same visitors. One player at a time can pick a
visitor up, carry it or battle it, and the others see that. A friend's **Others can change my
Chao** setting still decides whether teaching their Chao counts in their save. Turn off **Wild
visitors in the gardens** or **Wild visitors in shared gardens** to hide them.

### Creature battles

In a Neutral, Hero or Dark Garden, put down anything you're holding, unpause and open
**F9 > Creature battles**. The battle takes place in its own scene, with copies of your Chao,
3D creatures and a battle field; the garden stays loaded behind it and you go back to the
same spot afterwards. It doesn't use Karate or need the Battle DLC. The garden needs four free
Chao display slots for the battle copies.

Pick **Singles (1 on 1)**, **Doubles (2 on 2)**, **Multi battle** or **Trainers**, then your
team: A picks or drops a Chao, and **Done** continues. You can battle friends in loaded
gardens, copies of your own Chao, and Chao files from `chao/import`. Live battles need your
friend's creature link to come from the same game; otherwise you see "...'s creature link comes
from a different game file."

**Trainers** lists practice trainers by level, then milestones that open one after another,
finalists and a champion. Trainers keep their own levels and can be battled again. Clearing a
milestone can win a move: **Teach a milestone move** at the bottom of the list teaches a won
move to one of your Chao in place of one of the moves it battles with, kept in `partners.txt`.

The first battle can take a moment: "Preparing battle venue..." and "Preparing creature
models..." show meanwhile. If the field or models can't be loaded, the scene closes with a
message saying so. Attacks are shown with type-coloured sparks, orbs and status marks, Chao
motions, particles, flashes and camera shake (**Creature battle effects** turns them off).

Each turn choose **Fight**, **Switch** or **Give up**. Move details show the category
(Physical, Special or Status), type, PP, power, accuracy and targets. In doubles you choose for
both of your Chao, and pick targets where needed. The result screen shows who won and the
battle XP earned; confirming takes you back to the garden. Start opens the normal pause menu.
**F9 > Creature battles: return to garden** closes the scene; a battle already started counts
as given up.

To battle a wild visitor, walk up to it with empty hands until its info card shows
"Y: Battle", and press **Y**. In a shared garden the host keeps it for you first ("Asking to
battle..."). Win and it stays in the garden; lose or give up and it runs away. Backing out
before the fight, or a battle that can't load, leaves it where it was.

Battles use the mod's own rules: no held items, and no power-up moves or transformations.
Moves the battles don't support are unavailable rather than played as plain attacks. Battle
levels and XP are kept apart from your Chao save.

Normal **Chao Karate** is unchanged: its own Stadium menus, fights and Battle DLC.

## Problems

- **Can't connect:** check the address and port, and that the host is reachable. On Windows,
  allow the game through the firewall.
- **"rejected: mod ..." or "protocol ...":** someone has a different version. Everyone should get
  the latest release.
- **"can't listen on port":** another program, often a second copy of the game, is using the port.
- **Log file:** `mods/sa2-chao-online/sa2-chao-online.log`. Include it when reporting a problem.
