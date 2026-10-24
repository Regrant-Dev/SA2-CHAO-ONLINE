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
| Chao details | Your Chao's stats: a summary of all seven abilities, evolution, alignment, life and battle level, then pages of details |
| Save Chao garden now | Save your Chao World data immediately while in a garden |
| GBA link | Points to the link machine in each garden (with the machine turned off: brings a partner from your own GBA save into the garden; see Chao > GBA link) |
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

**Go to:** F9 > Go to, pick a friend. If you're in the same stage or garden you're moved next to
them. From Stage Select, or from Chao World (you leave it first), you're taken into their stage
as their character. From inside another stage, go to Stage Select first.

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
- its battle level with a bar of the XP toward the next, and its GBA link partner's types.

The other pages: **Stats** (the points each level adds at each grade; points stop at 4000),
**Growth** (the evolution rules, the Chaos checklist, age, lifespan and what rebirth needs:
happiness above 50 and stress under 7%), **Mood** (emotions and needs as percentages),
**Traits** (personality in words, which can change when it reincarnates, and the shop fruit
it refuses), **Bonds** (like and fear for each
of the six characters; above 75 fear it is scared), **Battle** (level, XP, partner types and
the moves it battles with), **Looks** (appearance and animal parts), **Health** and **Raw**
(every saved byte). Values refresh while you look. Eggs have no stats until they hatch. Type
and move names come from your own GBA ROM. Viewing a page doesn't change, export or save the
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
- its GBA link partner's types over its battle level.

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

**GBA link:** bring creatures from your own GBA save into a garden, where your Chao can pick
them up and absorb them like animals. It needs your own copy of Emerald for the Game Boy
Advance: a ROM of the US release (game code BPEE, the first version) dumped from a cartridge you
own. Its 128 KB save file is optional. Nothing from the game comes with the mod: names, types
and pictures are read from your ROM, and the save is only read, never changed.

- Put the ROM (`.gba`) and the save (`.sav`) in `mods/sa2-chao-online/gba/` (the mod makes this
  folder the first time it runs, with a note inside), or set their paths
  in the Mod Manager (GBA link group; `GbaRom` and `GbaSave` in `config.ini`).
- Each garden has a **link machine** beside its
  transporter, a short walk to one side of its doorway: plain in the Neutral Garden, white and gold with a halo and wings in the Hero
  Garden, dark and spiky in the Dark Garden. Stand next to it (any side), with empty hands and
  nothing to pick up closer to you, and press the action button to open its list of
  the save's party and boxes and the ones the mod keeps, with each one's level and types. The
  list's first row, **Moves, type & ability**, is for choosing what your Chao battle with
  (below).
- Move through the list with Up/Down on the controller or keyboard, and the machine's screen
  shows the picture of the one highlighted. A or Enter picks it: it pops out of the machine's
  hatch as a partner, drawn as its picture, and walks into the garden. B, Esc or Backspace
  closes the list. While it is open your character stands still and the game doesn't take the
  presses. When the garden is full of animals, a wild visitor leaves to make room.
- **Long lists:** in every list of the link machine, Left/Right (D-pad or arrow keys) jump 10
  rows and PgUp/PgDn a page. Typing on the keyboard searches the list by name (the creature's,
  the Chao's, the move's, type's or ability's), whatever the case and anywhere in the name: a
  "Search:" line above the list shows what you typed and only the rows that match stay, with
  rows like **Done** always there. Backspace deletes a letter and Esc clears the search; with
  no search, they go back as before. While the list is open, letters only type into the search
  (the chat's T and the status balloon's C wait), so on the keyboard pick with Enter.
- Carry a partner to the Chao you want it to teach: a Chao only takes one it's handed, as with
  animals, so Chao walking past leave it alone.
- Without a save, the list offers a one-time gift instead: one of the game's three first
  partners at level 5, picked once and kept by the mod (in `gba/partners.txt`). The link machine
  brings it out right away; after that it is on the list like the save's.
- Each one can come to each garden once a day, and a garden holds 4 waiting partners at most.
  Waiting partners are still there the next time the garden loads.
- Turn the machines off with **Link machine in the gardens** in the Mod Manager (GBA link
  group; `LinkMachine` in `config.ini`): F9 > **GBA link** then lists the same creatures, and
  Enter brings the one picked in front of you.
- A Chao that absorbs one keeps it as its partner, and the partner teaches it its moves, its
  types and its special ability for Chao battles. The Chao's own stats don't change.
- A Chao remembers every move it has learned (from partners, wild visitors, the gift and TM
  moves) and battles with four of them. New moves fill its four when it has fewer; otherwise
  its four stay as they are. Either way "<Chao> learned <move>!" shows for each move new to it.
  It also remembers every type and every ability its partners gave it (wild visitors and the
  gift too), and battles as one or two of those types with one of those abilities, or none.
  Its first partner's types and ability are the ones it starts with; later partners add theirs
  to the lists without changing what it battles with. A Chao without a partner is Normal type
  with a few basic moves and no ability.
- **Choosing moves, type and ability:** pick **Moves, type & ability** at the link machine,
  then one of your Chao by name. Its page has a row each for its moves, its type and its
  ability (showing what it battles with now), and **Done**:
  - **Moves** shows every move it has learned with the move's type and power; `[x]` marks the
    four (at most) it battles with. A or Enter switches the move under the cursor on or off: a
    fifth one can't go on until one is off, and the last one can't go off. A slot left empty
    battles with a basic move.
  - **Type** shows every type its partners gave it, by the ROM's names; `[x]` marks the one or
    two it battles as. A or Enter switches one on or off: a third can't go on until one is off,
    and the last one can't go off.
  - **Ability** shows **None** and every ability its partners gave it; `(o)` marks the one it
    battles with, and the line under the list is the ROM's description of the one highlighted.
    A or Enter picks it.

  `(GBA save)` marks moves, types and abilities only a partner from a GBA save gave it.
  Changes are kept at once; **Done** or B goes back. The battle team screen has the same
  choices (Chao battles, below).
- The status balloon's last row shows the types the Chao battles as over its battle level, with
  a bar of its XP toward the next level for your own Chao (a friend's shows the level their
  game sent) (Chao battles, below). Partners, moves, types, abilities and battle levels
  are kept in `gba/partners.txt`; in the Chao save absorbing one only adds the happiness an
  animal gives.

**Wild visitors:** each garden is linked to one area of the GBA game, a different one each time
you walk into it (each of its areas comes once before any comes again). The
Neutral Garden gets grassy routes and woods, the Hero Garden sea, sky and shore areas, and the
Dark Garden caves, mountains and sandy or ashy routes. Only the ROM is needed, not a save.

- The area's name shows for a few seconds at the top of the screen when you walk into the garden.
- Wild creatures of that area come by now and then, picked with the same chances and levels as
  in the GBA game: four when you arrive, then one every 20 to 50 seconds, up to 6 or 7 at a
  time. In a shared online garden 3 more can be about for each other player there, up to 20.
  They don't crowd out the garden's own animals: the mod lets a garden hold 32 animals instead
  of the game's 10.
  Land creatures walk on the grass and water creatures swim in the pond. Now and then one is
  shiny, in the GBA game's shiny colours.
- Whistle and nearby ones come to you: land creatures walk over and stop a few steps away,
  and water creatures swim to the edge of the pond nearest you.
- Carry one to a Chao and it absorbs it like a partner (Chao walking past leave visitors alone,
  so you choose who learns what): a wild one teaches a Chao its four
  moves, its types and its ability. Its moves are picked at random from the ones its kind
  learns by its level, so two of a kind can teach different moves. Absorbing gives no battle
  XP.
- Stand next to one, or pick it up, and a card at the top right shows it: name, level, types,
  ability and the four moves a Chao that absorbs it learns (a Chao gets those, not its stats).
  Partners brought from the link machine get the same card.
- **Battle one:** with a wild visitor's card showing and your hands empty, press **Y** (needs
  the Battle DLC). You go straight to the karate arena, pick your team against it (singles),
  and when the battle ends you're back in the garden where you stood. It fights at its level
  with the four moves, types and ability its card shows, and its own stats for that level. Win
  and your Chao that were sent out get battle XP and it stays in the garden; lose or give up
  and it runs away. B on the team
  screen goes back without a battle and leaves it as it was. In a shared garden others see it
  busy while you battle it (nobody can pick it up), and one that runs away leaves the garden for
  everyone. A visitor never needs to be battled before a Chao absorbs it.
- Carry one through the transporter and it comes along (in the lobby a note at the top says
  you're carrying it; your hands look empty there): it is in your hands when you reach
  the next garden, ready to give to a Chao there.
- Wild visitors are never saved; one that has been about for a few minutes leaves once you are
  away from it, and new ones keep coming.
- Turn them off with **Wild visitors in the gardens** in the Mod Manager (GBA link group;
  `WildVisitors` in `config.ini`).

**Wild visitors in shared gardens:** online, a garden shared with friends has one set of wild
visitors that everyone in it sees. The server keeps them and moves them about.

- The player who has been in the garden longest picks its area and its visitors from their ROM.
  When they leave, the next one takes over, and the visitors already there stay. The area's name
  shows when you walk in. Your own visitors leave when the garden becomes shared; the first
  player's stay where they are as the garden's.
- Each game draws them from its own ROM, so everyone needs the same ROM: a player with another
  ROM, or none, sees no shared visitors and no link machine there.
- Picking one up, carrying it, putting it down and whistling work as in your own garden, and
  everyone sees the same thing. Only one player can take a visitor at a time, and one someone
  holds is in their hands for everyone; its card says who holds it.
- Hand one to a Chao and it's gone for everyone. What it teaches goes to the Chao's owner:
  always for your own Chao, and for a friend's only when their **Others can change my Chao**
  lets you change it (a Chao locked for you doesn't take it). It is kept in the owner's
  `gba/partners.txt`, never in yours.
- Carry one out through the transporter and it leaves the garden for everyone and comes along to
  your next garden. Carry one into a shared garden and it joins the garden's visitors, still in
  your hands (when the garden is full of animals or already has five wild visitors, it waits for
  your next garden).
- The **link machine** is there too, and each player uses it for themselves: their own GBA save,
  kept ones, gift and Moves. A partner brought out of it, and the ones waiting for you in that
  garden, join the garden's visitors: anyone can see one, carry it and hand it to a Chao, with
  the same rule for teaching. A waiting partner leaves the garden with you and is back on your
  next visit, until a Chao absorbs it.
- When the garden is yours alone again, the wild visitors about stay as your own. One a friend is
  battling isn't among them: it waits for them, and when they come back to the garden (and it's
  shared again) it's there, unless it ran away.
- Turn them off with **Wild visitors in shared gardens** in the Mod Manager (GBA link group;
  `SharedVisitors` in `config.ini`): shared gardens then have no wild visitors or link machine
  for you, and your Chao learn nothing from the ones friends hand them.

**Chao files:** your Chao are saved as files in `chao/export`. Put a friend's `.sa2chao` files in
`chao/import` and they visit your garden, even when you're not online together.

## Chao battles

Chao battle each other in turns, team against team, with the moves, types and ability their
GBA link partner taught them (Chao > GBA link). Battles need the Battle DLC (they use the
karate arena) and your own ROM of the GBA game, set up as for the GBA link: every move, type
and ability name and number is read from it. A GBA save isn't needed.

**Starting one:** go to Chao Karate in the Chao Stadium. On the karate menu, press Down on the
last button to reach **BATTLE** and press A. Then pick:

1. **Singles** (one Chao a side at a time) or **Doubles** (two a side), or **Trainers** to
   battle the GBA game's trainers (below).
2. **Your team:** up to 6 of the Chao in your save, each with its battle level and a bar of
   its XP toward the next. A picks or drops the Chao under the cursor; they go out in the order
   you picked them. The line below the list shows the Chao's battle level and XP, types,
   ability, moves and battle stats. **Choose moves, type & ability** picks a Chao, then
   chooses its moves, type and ability as at the link machine (Chao > GBA link). Choose
   **Done** at the bottom of the list (Up from the top gets there).
3. **Who to battle:** a friend online at Chao Karate for a live battle (below), a team of each
   online friend's Chao (their My Chao and the Chao they bring), the `.sa2chao` files in
   `chao/import` (six to a team), or copies of your other Chao (copies of your own team when
   you have no others).

**Controls:** the game's menu buttons, on a controller or the keyboard: Up/Down moves the
cursor, A chooses, B goes back. Start pauses the game as usual, and the battle waits.

**Long lists:** in the menus' lists (the team, the Chao and their moves, types and abilities,
the opponents, the trainers and the TM moves), Left/Right jump 10 rows, as do PgUp/PgDn on the
keyboard. In the lists with names (Chao, moves, types, abilities, creatures, trainers and
teams), typing on the keyboard searches the list by name, whatever the case and anywhere in the
name: a "Search:" line above the list shows what you typed and only the rows that match stay,
with rows like **Done** always there. Backspace deletes a letter and Esc clears the search,
or goes back when there's none. While such a list shows, letters only type into the search, so
on the keyboard Enter chooses and Esc goes back; a controller works as before.

**A turn:** choose for each of your Chao in the ring:

- **Fight:** pick one of its 4 moves. Each shows its type and PP; below the list is whether
  the move is physical, special or status, its power and accuracy, who it hits and what it
  does. Moves that can't be used now (no PP left, or blocked) are greyed out. In doubles, a
  move aimed at one Chao then asks for its target: either foe, or your other Chao. Moves that
  hit both foes or everyone don't ask. A move that passes the Chao's stat changes on to a
  teammate asks who gets them.
- **Switch:** send in another Chao from your team. The list shows each one's level, HP and
  status.
- **Give up:** ends the battle as a loss, after asking.

In doubles, B on the second Chao's menu goes back to the first one's. The foes choose at the
same time; the computer runs them, mostly picking moves that hit hard, with some randomness.

The turn then plays out in the ring, with messages at the bottom ("... uses ...!", strong and
weak hits, critical hits, misses, status, stat changes, weather, fainting); A moves on to the
next one. A box at the top for each Chao in the ring shows its name, HP bar, status (SLP,
PSN, BRN, FRZ, PAR) and level; yours also show their HP and a thin bar of their XP toward the
next level. When one of your Chao faints you pick who replaces it. A team with no Chao left
loses; the result screen shows who's still standing and the XP your Chao earned (Up/Down
scrolls a long one), and A goes back to the karate menu. A battle that reaches 200 turns is a
draw.

**In the ring:** each Chao moves by the kind of move it uses, and each move has its own motion
within its kind, so two moves of a kind rarely look alike: lunges, swings, kicks, head-butts,
pounces and diving slams for moves that touch the foe; a throw or a swing for other physical
moves; a jump and a stomp for moves that hit everyone; a swim, a roll or a splash for water
moves; taking off for flying moves; a focus pose for other special moves, or rearing up with a
flame for fire and dragon ones; singing or a tune for sound moves; a flurry that steps in for
every hit of a multi-hit move; a head-down charge for moves with recoil; reaching out to soak
it up for draining moves; a quick dash for moves that go first; its biggest jump or kick for
one-hit knock-outs; a dance for weather; a yawn, a spin, a swoon, a glare or a tease for moves
that give the foe a status; cheering for raising its own stats, taunts for lowering the foe's,
a guard, a dodge or a cower for shielding, lying down for resting and eating or soaking up for
healing. A two-turn move gets ready its own way on its first turn (rising up, digging, diving
or focusing) and strikes on the second; a Chao that rises up hovers in the air until then. The move's own effect from the GBA game (read from
your ROM) moves the way it does there: a shot, a lob, a volley or a beam from the user to its
target, a spiral, a rain from above, a rise from the ground, a ring around the user, a drain
back to the user, or a burst on the target, with sparkles, splashes, flames or snow where they
fit. Effects on the user move with it as it steps in or jumps. A damaging move's effect lands
just before the target takes the hit; a status move's makes the target shudder. Effects the
GBA game draws see-through let the Chao show through them. A move that tints the GBA screen
flashes the arena in its colour. In doubles, a move that hits both foes lands on each, and one
that hits everyone else lands on the user's partner too. A critical or strong hit,
and a move that shakes the GBA screen, flashes the screen white and shakes the view. Status
shows on the Chao: an asleep Chao sleeps between turns, a confused one stumbles under a swirl,
a flinching one shows "!", a paralyzed one shivers, a frozen one sits in ice, and a fainting
one falls over and cries. **Battle effects** in the mod's settings (GBA link group) turns all
this off, leaving the karate punches and hits.

**Rules:** the GBA game's battle rules: its type chart, accuracy and evasion, critical hits,
the same-type bonus, priority, PP, status conditions, stat stages, weather, move effects and
abilities. Chao hold no items. Every Chao battles at its battle level (below), with stats from
its own:

| Chao stat | Battle stat |
|---|---|
| Stamina | HP |
| Power | Attack |
| Swim | Defense |
| Run | Speed |
| Fly | Sp. Defense, and a slight evasion edge (up to 5% off foes' accuracy) |
| Intelligence | Sp. Attack |
| Luck | Critical-hit chance |

A stat counts for more the more points it has, up to 3000 points (fully raised); a better
grade adds a little. Each counts as a base stat from 20 to 160 in the GBA game's stat formula at
the Chao's level. A new Chao is weak but can battle, and a well-raised one clearly beats a
poorly raised one at the same level.
Intelligence and luck count as at least half the Chao's average of the other five stats.
Happiness powers the moves that depend on the bond with the owner.

**Levels and XP:** each Chao has a battle level from 1 to 100, starting at 5, kept by the mod
in `gba/partners.txt` (never in your save). XP comes only from battles:

- Trainer battles give the GBA game's trainer XP for each foe that fainted (by its kind and
  level), shared among your Chao that were sent out. A loss gives only what the fainted foes
  were worth. Beaten trainers can be battled again for more.
- Battling a wild visitor in a garden (above) gives the GBA game's XP for a wild one (by its
  kind and level, without the trainer battles' extra), shared among your Chao that were sent
  out, and only for a win.
- Live battles give both players XP, by each Chao's own level, more for the winner and more
  for each of the other team's Chao that fainted (less for one below your Chao's battle level,
  whatever the level rule). Against a team of fewer than three, each of its Chao counts as one
  of a team of three, so a smaller team gives less.
- Only a battle played to the end gives XP: none when either player gives up, leaves or runs
  out of time, and none for a live battle whose two games went apart.

After the battle's last messages "<Chao> grew to Lv 23!" shows for each Chao that went up (A
moves on, as with the other messages), and the result screen lists the XP each one earned. The
first levels come quickly; level 100 takes about seven to eight hours of trainer and live
battles for a Chao that fights them alone (Chao that are sent out together share the XP, so
each takes longer). A Chao's level and XP show on the team screen, its box in the ring and its
status balloon. Under the Level 50 rule the box shows Lv 50 without the XP bar, and when you
answer such a challenge the team screen shows the stats at level 50.

**Moves, types and ability** come from the Chao's partners: absorbing one teaches its moves,
types and ability, and a Chao battles with 4 of the moves it has learned, one or two of the
types and one of the abilities (or none) its partners gave it, chosen at the link machine or
with **Choose moves, type & ability** on the team screen (Chao > GBA link). A Chao without a
partner is Normal type with a few basic moves and no ability. A friend's Chao battles with the
types, ability and four moves they chose and its battle level from their game, sent while
you're online together; your own Chao only ever use what your game keeps for them. Exporting
Chao files also writes `partners.txt` next to them: put it in `chao/import` with their
`.sa2chao` files (add its lines to the one there when you have Chao from more than one friend).

**Trainers:** the GBA game's own trainers with their teams from your ROM. The list has the
route trainers first (warm-ups, in groups by strength), then the 8 gym leaders, the league's
four and the champion. The first gym leader is open from the start, and each leader, league
trainer and the champion opens once you've beaten the one before; each group of route trainers
opens with the gym leader before it. The line below the list shows the trainer's team and
prize, and beaten trainers have a `*`. Pick a trainer, then your team: the battle is the
trainer's own kind, singles or doubles. Their creatures stand on the far side of the ring as
their pictures from the GBA game, at their own levels with their own moves, types and
abilities, against your Chao at theirs: the ladder is the level check. They hop when they act,
shake when hit and sink away when they faint.

Beating a gym leader earns their badge and the TM move their gym gives. **Teach a TM move** at
the bottom of the list teaches a won move to any of your Chao in place of one of the moves it
battles with; the TM move joins the Chao's move list, and the one it replaced stays there.
Friends' copies of that Chao, online or exported, battle with it too. Your progress is kept in
the mod's `gba/trainers.txt` and the Chao's moves in `gba/partners.txt`, never in your save.
**Battle the GBA game's trainers** in the mod's settings (GBA link group) turns Trainers off.

**Live battles:** friends who are online and at Chao Karate too are at the top of the "Battle
against" list, marked "live" ("busy" while they're in a battle). Pick one to challenge them
with your team and format; "Waiting for ... to answer" shows until they do (B takes the
challenge back). On their side the challenge pops up on the karate menu, or in the battle menu
if it's open: **Accept** and pick a team, or **Decline** (back to the karate menu when the
challenge opened the battle menu). Each of you then battles from your own
game, your own Chao on the left, and chooses for your own Chao; once you've chosen,
"Waiting for ..." shows until your friend has too (B there gives up). Both games show the same
turns and the same result. Giving up, leaving the karate area or going offline loses the
battle, and a player who takes more than 2 minutes to choose once the other has chosen loses
too. Both games need the same GBA ROM.

**Live rule, GBA save partners:** the row under your friends in the "Battle against" list
(A switches it) says whether your challenges allow what partners from a GBA save taught.
**Not allowed** means both teams battle without it: moves, types and abilities only partners
from a GBA save gave are left out. The Chao's other moves, then basic moves, fill the empty
move slots; its other types take a type's place (Normal when it has none), and its other
abilities the ability's (none when it has none). What wild visitors, the gift and trainers'
TMs gave always counts. The challenge shows the rule to your friend, and each game
applies it to its own team.

**Live rule, levels:** the next row says whether your challenges are **Level 50** (every Chao
on both teams battles at level 50; the default) or **actual levels** (each Chao battles at its
own battle level). The challenge shows the rule to your friend, and both games apply it to
both teams. The XP a live battle gives doesn't depend on it.

A challenge that comes while you're elsewhere, in
another battle or in live karate is turned down on its own. **Live battles with friends
online** in the mod's settings (GBA link group) turns them off: friends' challenges are turned
down and the list has no live entries.

Battles never change your save: every Chao in them is a copy, win or lose.

## Problems

- **Can't connect:** check the address and port, and that the host is reachable. On Windows,
  allow the game through the firewall.
- **"rejected: mod ..." or "protocol ...":** someone has a different version. Everyone should get
  the latest release.
- **"can't listen on port":** another program, often a second copy of the game, is using the port.
- **Log file:** `mods/sa2-chao-online/sa2-chao-online.log`. Include it when reporting a problem.
