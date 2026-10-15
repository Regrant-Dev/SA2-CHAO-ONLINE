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
| Host address | When joining: the host's address, e.g. `100.64.0.2:43902` |
| Port | When hosting: the UDP port to listen on (default 43902) |
| Name tags | Names above other players |
| Room | Create a room, join someone's, or leave (Left/Right to choose, Enter to do it) |
| Live karate vs | In a room, at Karate: who your next fight is live against (first free player, or pick one) |
| My Chao | Your Chao for friends' races and karate (default: your first hatched Chao) |
| Garden visits | Which of your Chao visit friends' gardens: Garden Chao (up to 2 from that garden), My Chao, or Off |
| Friends' Chao in races | Friends' Chao take the CPU lanes of your Chao Races (default On) |
| Practice karate | CPU, or a **copy** of a friend's Chao that your own game fights (Battle DLC) |
| Export my Chao / Bring visitor Chao | Chao files (see Chao) |
| Save & Start / Apply / Stop | Connect with these settings, reconnect, or disconnect |

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

**Starting together.** Each group in a room is its own game: if you and Sam go into one stage
and Alex and Kim into another, those are two separate stages that start and play on their own;
the same goes for races. While a start is held, the screen shows who's there with you
("Players: You, Sam" in stages, "Opponents: Sam" in races and karate) and what to press. **Anyone** there can start it for everyone there: **F8**,
controller **Start** (stages and story), or F9 > Start now; then 3-2-1-GO. So in story mode, if
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

**Garden visits.** When you enter a garden while connected, your Chao visit the others in the
same garden (F9 > Garden visits: Garden Chao, My Chao or Off).

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
can't be picked; turn Multiplayer off in F9 to play local 2P. If a 2P game is running anyway,
online play steps aside for it: nobody else is drawn, there are no synced starts or room picks, and the others don't see
you until the 2P game ends.

## If something goes wrong

- **Lost connection:** you keep playing; the others disappear after a few seconds and you
  rejoin automatically when the connection is back.
- **Can't connect:** check the address and port, and that the host's port is reachable
  (Tailscale or port forwarding). Host and joiners need the same mod version.
- **Log file:** `mods/sa2-chao-online/sa2-chao-online.log`; include it when reporting a problem.
