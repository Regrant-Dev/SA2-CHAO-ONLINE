# How to play SA2 Chao Online

Play Sonic Adventure 2 with up to 8 friends: see each other in stages and Chao gardens, start
stages and races together, and bring your Chao to visit, race and fight. Everyone plays their
own game on their own save, and nobody else can change your save or your Chao.

## Install

1. Install SA Mod Manager with the SA2 Mod Loader (<https://github.com/X-Hax/SA-Mod-Manager>).
2. Extract the release zip into the game's `mods` folder (you get `mods/sa2-chao-online`),
   enable **SA2 Chao Online** in SA Mod Manager and press Save.
3. Start the game. Multiplayer is **off** until you turn it on.

Everyone in a lobby needs the same version of the mod and of the game (the Steam version).

## Everything is on F9

Press **F9** in game. Pause first when typing: the game also reads letter keys as movement.

- **Up/Down** pick a row, **Left/Right** change it, type to edit text, **Enter** runs an action
  (the menu closes and the result shows top-left). **Esc** or **F9** closes it.
- Settings are saved right away (also on SA Mod Manager's Configure page for this mod).

| Row | What it does |
|---|---|
| Start now | Only while held at a start: start it for everyone there (same as F8) |
| Multiplayer | Off / Host / Join |
| Your name | Shown above you to others (up to 10 characters) |
| Host address | Shown in Join mode: the host's or server's name or IP, e.g. `100.101.102.103` or `myserver.example.com:43902` (the port is optional; default 43902) |
| Port | Shown in Host mode: the UDP port your game listens on (default 43902) |
| Name tags | Names above other players |
| Room | Create a room, join someone's, or leave (Left/Right to choose, Enter to do it) |
| Mode (in a room) | Race or Co-op; only the room's creator can change it (see below) |
| Go to | Pick a player (Left/Right) and Enter: join them where they are (see Chat and friends) |
| Live karate vs | In a room, at Karate: who your next fight is live against (first free player, or pick one) |
| My Chao (races, karate) | Your one Chao for friends' races and karate (default: your first hatched Chao) |
| Visit friends | On: up to 2 of the Chao living in the garden you're in visit friends in that same garden. Off: they stay home (takes effect right away) |
| Friends' Chao in races | Friends' Chao take the CPU lanes of your Chao Races (default On) |
| Practice karate | CPU, or a **copy** of a friend's Chao that your own game fights (Battle DLC) |
| Export my Chao / Bring visitor Chao | Chao files (see Chao) |
| Save & Start / Apply / Stop | Connect with these settings, reconnect, or disconnect |

**The settings file.** F9 and SA Mod Manager's Configure page both save to
`mods/sa2-chao-online/config.ini` in the game folder. It doesn't exist until you change a
setting; without it everything is at its default, so multiplayer starts **Off**. It remembers
your last choice, so if you joined a friend last time you start in Join next time. You can also
edit it by hand while the game is closed:

```ini
[Multiplayer]
Mode=Off              ; Off, Host or Join (default Off)
Name=Player           ; shown above you, up to 10 characters (default Player)
Host=127.0.0.1:43902  ; when joining: the host's address and port
Port=43902            ; when hosting: the UDP port to listen on (default 43902)
NameTags=On           ; On/Off: names above other players (default On)
ChaoRace=On           ; On/Off: friends' Chao in your Chao Races (default On)
MyChao=-1             ; save slot of your Chao for races and karate, -1 = first hatched (default)
VisitWith=On          ; On or Off: your garden's Chao visit friends in the same garden (default On)
```

Delete the file to go back to all defaults.

**Live or copy?** Everything you do *in a room* is live: your friends' real Chao, the same race
and the same fight on every screen. "Practice karate" and races outside a room use **copies** of
your friends' Chao that your own game plays, so they don't need your friends to be there, but
each player can see a different result.

## Hosting and joining

One player **hosts**, everyone else **joins** the host's address. Only the host's computer has
to be reachable on the port (UDP 43902 by default):

- **Tailscale** (easiest, no router changes): everyone installs Tailscale
  (<https://tailscale.com>) and joins the same tailnet. Joiners use the host's Tailscale
  address, e.g. `100.x.y.z:43902`.
- **Port forwarding:** the host forwards **UDP** port 43902 on their router to their PC and
  gives friends their public IP, e.g. `203.0.113.5:43902`.

### Dedicated server (optional)

A dedicated server keeps a lobby up without anyone's game hosting it, so nobody has to be the
host and the lobby stays when players come and go. It isn't a player (up to 8 players can join
it) and only passes messages between players; everything else works the same. Everyone joins
it with F9 > Multiplayer: **Join** and its address, e.g. `100.101.102.103:43902`. It needs the
same mod version as the players, which it prints when it starts.

- **Docker:** `docker run -d --restart unless-stopped -p 43902:43902/udp --name sa2-chao-online-server ghcr.io/regrant-dev/sa2-chao-online-server:latest`
- **A NAS or any Docker UI:** add a container with repository
  `ghcr.io/regrant-dev/sa2-chao-online-server:latest`, network `bridge`, and port
  **43902/UDP** mapped to 43902. Nothing else to set.
- **Windows:** download `sa2co-server.exe` from the release and run it (`sa2co-server.exe 43902`
  for another port). Allow it through the firewall when Windows asks.

The server's computer is the one that needs to be reachable (Tailscale or a UDP port forward).
Update it together with the mod: an older server turns newer players away.

## Chat and friends

- **Chat:** press **T**, type, **Enter** to send (Esc cancels). Everyone in the lobby sees it at the
  top left. Your controls are off while you type; the game keeps running.
- **Go to a friend:** F9 > Go to, pick them, Enter. In the same stage or garden you're moved next
  to them. On Stage Select you're taken into their stage and mission (as their character). In
  Chao World you're taken to their garden or area. Anywhere else the menu says where to go first.
- **Who's playing:** the Boyz Hub's SA2 card lists who's online and where.

## Free roam and rooms

**Free roam (always on).** You see every player in the lobby whenever you're in the same stage
and mission, or the same Chao garden.

**Rooms** are for playing together. In F9 > Room, one player **creates** a room and the others
**join** it ("Join Sam's room (2)"). You can be in one room at a time; leaving or disconnecting
takes you out, and the room closes when its creator leaves. A message top-left tells you when
someone creates a room, and when players join or leave yours. Everyone stays in the same lobby, so
free roam keeps working. Whoever in the room picks something takes the others who are at the same
place along:

| Where | What the room does |
|---|---|
| **In stages (story too)** | Every stage freezes at the start while the room players in that stage load in, then anyone there starts it with 3-2-1-GO. Play story mode together this way, each on your own save; skipping cutscenes doesn't matter. |
| **Stage Select** | When anyone in the room picks a stage and mission, everyone in the room who is on Stage Select goes into it with the right character, then it starts together. (You can be taken into a stage your own save hasn't unlocked.) |
| **Chao Race entrance** | When anyone in the room confirms a race, everyone in the room at the race entrance (Chao chosen, course list open) races in it (see Chao). |
| **Karate** | Anyone in the room can challenge another room player at Karate to a live fight (see Chao). |

**Racing in a room.** In a stage started together, your race time counts from GO (deaths
included); the game's own clock, which rewinds to the last checkpoint when you die, is shown next
to it. In a Race room a line at the top right shows your position
("2nd / 3"), and finishes are announced to everyone. Positions need the stage's route:
the first clear of a stage records it (in `routes/`) and shares it with the others, so the first
race on a new stage shows positions only at the finish.

**Kart Race (rooms).** Whoever in the room starts a Kart Race (main menu > Kart Race) takes the
others who wait in the Kart Race course list along, each with their own driver. The CPU karts are
switched off: you race each other. Everyone starts on the same lights, sees the others' karts,
positions and finish times.

**Treasure hunt (rooms).** In a Knuckles/Rouge hunting stage (missions 1, 4 and 5) started
together, the whole room hunts the same 3 pieces. The first to grab a piece gets it and it
disappears for everyone else (the radar and hints move on). The top right shows the score; the
last piece ends the round for everyone. In a Race room the most pieces wins; in a Co-op room
you're a team (enemies you beat are gone for everyone too) and nobody is named the winner.

**Race or Co-op.** The room's creator picks how stages play in F9 > Room > Mode (members see
it; "The room is now Co-op" pops up when it changes). **Race** (the default): everyone has their
own enemies and item boxes, as in the normal game. **Co-op**: an enemy or item box destroyed by
anyone in the room is gone for everyone in that stage and mission, so whoever is ahead clears the
way. Only whoever destroys it gets the points, rings and animals. Things destroyed stay destroyed
even if you die and restart, and someone who enters the stage later finds them gone too. Bosses
and other objects aren't shared. Everyone in the room needs the same stage files (no different
SET-file mods).

**Starting together.** Each group in a room is its own game: if you and Sam go into one stage
and Alex and Kim into another, those are two separate stages that start and play on their own;
the same goes for races. While a start is held, the screen shows who's there with you
("Players: You, Sam" in stages, "Opponents: Sam" in races and karate) and what to press. **Anyone** there can start it for everyone there: **F8**,
controller **Start** (stages and story), or F9 > Start now; then 3-2-1-GO. In stages, if a room
player is still loading (slower PCs take longer), the start waits for them and shows "Waiting for
Sam to load"; press again to go without them (it also goes by itself after 20 seconds). So in story mode, if
you skip the cutscenes and your friend watches them, you wait until their name shows up and
then start. A live karate fight starts when either fighter presses F8. The room's creator is
just who made the room; they're not in charge of anything.

**Pausing and tabbing out.** While you're connected, the game keeps running when you tab out or
minimize it (sound comes back when you return); the others keep playing, so your game doesn't
freeze, and a controller disconnecting doesn't bring up the game's "Reboot to change the control
scheme" message. In anything you play live with your room (a stage started together, story included, a
room race or a live karate fight), the pause menu opens but the game keeps running behind it.
Elsewhere pausing works as normal, and the others see you stand still.

## Chao

Other players only ever get **copies** of your Chao: nothing that happens to a visiting, racing
or fighting copy is saved anywhere, and your own Chao never leave your save.

**Garden visits.** When you enter a garden while connected, up to 2 of the Chao living there visit
the others in the same garden (F9 > Visit friends: On or Off). My Chao is only for races and
karate. The gardens are live:
- A friend's Chao in your garden is a copy that does exactly what their real one does (walking,
  animations), and when they pick it up you see it in their hands. The same goes for yours in
  their garden.
- You can pet a friend's Chao, pick it up, or give it a fruit, animal or Chaos Drive (hold it
  next to the Chao). Their Chao reacts in your game, and the effects (happiness, alignment, how
  much it likes your character, stats and lifespan from food, animal parts) go to their real Chao
  in their save. While you hold it, they see it in your hands. Hitting or throwing it counts too.
- Nothing about a visiting Chao is ever saved in your own save, and a visitor can't be carried
  out of your garden.
- You can take your Chao home any time: F9 > Visit friends: Off (or leave the garden, or
  disconnect). Within a few seconds they're gone from everyone else's garden; switch visits back
  on and they come again. Friends' interactions only reach your Chao while it's visiting.
- Garden items are shared: everyone sees everyone's fruit, animals, Chaos Drives, hats and seeds,
  on the ground and in hands. Each item belongs to one player and is saved only in their save.
  Pick up a friend's item (or let your Chao take it) and it becomes yours, saved in your save,
  and disappears from theirs. If two of you grab it at once, the first one gets it; an item its
  owner is holding, or their Chao is eating, can't be taken. Fruit still on a tree is each
  garden's own until it falls.

**Chao Race.** While connected, friends' Chao take the other lanes of your Chao Races (F9 >
Friends' Chao in races): each friend's My Chao first (or the Chao
they picked to race with, while they're at a race too), then another of their Chao while lanes
are left; the rest are CPU. Labels show "You" and the owner of each friend's Chao. Outside a
room each game runs its own race with those Chao, so results can differ between screens. In a **room**, everyone goes to the
race entrance, chooses their Chao and opens the course list; whoever confirms a race takes the
others along, the start waits until someone starts it (see below), and each friend's lane follows their real Chao
live, so their cheering, boosts and finish happen as they really did.

**Karate** (needs the Battle DLC, like the normal game):
- **Practice karate:** F9 > Practice karate, pick e.g. `Sam's Chao (copy)`, then start any
  karate fight. A copy of their Chao replaces the CPU opponent; you play on your own.
- **Live karate:** in a room, go to Karate in the Chao Stadium. **Anyone** can start a fight:
  pick F9 > Live karate vs (or leave it on "first free player"), then a fight as usual in the
  karate menu (your Chao, a tournament). Instead of the CPU it becomes a live fight against that
  player's My Chao, if they're on the first karate menu. The challenger's Chao is on the left
  and their game runs the fight; both games wait until both are in the ring, then both screens
  show the same fight and winner, and both players can cheer (A) their own Chao. Players in a
  fight show as busy, so several pairs can fight at once. If two players start a fight against
  each other at the same moment, each gets their own fight instead, so let one of you start.

**Chao files.** F9 > Export my Chao writes your Chao to `chao/export/` in the mod folder. Put
`.sa2chao` files in `chao/import/` and use F9 > Bring visitor Chao (in a garden) to have them
visit without being online together.

## Local two-player

While you're connected, the main menu's 1P button reads **MULTIPLAYER** and the 2P button
becomes **ROOM BATTLES**: it opens a picker with **Kart Race** (the course list; Kart Race doesn't
need to be unlocked) and **Treasure Hunt** (Stage Select, to pick a hunt stage); see the room
sections above. Chao Race and Karate stay in Chao World. Only you go: the rest of your room
is asked to come along, and anyone busy elsewhere (say, in their garden) keeps playing. Local split-screen 2P can't be picked online; turn Multiplayer off in F9 for it. If a 2P game is running anyway,
online play steps aside for it: nobody else is drawn, there are no synced starts or room picks, and the others don't see
you until the 2P game ends.

## If something goes wrong

- **Lost connection:** you keep playing; the others disappear after a few seconds and you
  rejoin automatically when the connection is back.
- **Can't connect:** check the address and port, and that the host's port is reachable
  (Tailscale or port forwarding). On Windows, allow the game through the firewall when
  Windows asks (or add an inbound rule for UDP port 43902 on the host).
- **"rejected: ..." top-left:** the host turned you away. `mod 0.1.0, host has 0.2.0` or
  `protocol 14, host has 15` means you have different versions of the mod: everyone should
  download the latest release. `different game version` means one of you doesn't have the
  current Steam version of the game. `lobby full` means 8 players are already in.
- **"can't listen on port ..." when hosting:** something else already uses that port (often
  a second copy of the game). Close it or pick another Port in F9; joiners then use that port.
- **Stuck in Join or Host every launch:** the mod remembers your last choice (see the settings
  file above). Set Multiplayer to Off in F9, or delete `config.ini`.
- **Log file:** `mods/sa2-chao-online/sa2-chao-online.log`; include it when reporting a problem.
