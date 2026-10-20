# Chao that stay in the shared garden (design, 2026-10-19)

Agreed with the user. Not built yet; built in the phases below.

## What players see

- When you leave a shared garden (log off, close the game, go to a stage), the Chao you had
  there **stay**: the server keeps running them (chao_sim), friends can pet, feed and play with
  them, and they keep living, **evolving included**.
- While **nobody** is in that garden, time stops for them (no unseen hunger or loneliness, no
  evolving with no one there). They pick up where they were when someone comes in.
- When you come back (to that garden, online), what happened to them while you were away is
  **merged into your save automatically**. No buttons.
- F9 has a per-Chao option "Stays home when I leave" for Chao you don't want left behind.
- Death and rebirth stay with the owner's own game: a Chao that reaches the end of its life on
  the server waits (paused) until its owner is back.

## Merging (nothing "wins")

- The server keeps each left-behind Chao's record as it was when the owner left (the base).
- On return: server delta = `chao::changes::diff(base, server_now)` (stats/levels only grow,
  happiness/bonds/alignment add, hat/animal parts newest), applied onto the owner's save like
  `ChaoChanges`. Emotions and needs (`CHAO_MOOD`) take the server's values.
- Evolution: if the server evolved it and the owner's save did not, the save takes the server's
  evolved fields (type and the look/evolution fields `0x568630` writes). If both evolved, the
  save's form stays and the rest merges.
- If the owner played offline meanwhile, both sets of changes count.

## Pieces

1. **Player key**: a random 128-bit key made once and kept in `config.ini` (`key=`), sent in
   `Hello` (protocol 22). The server keys left-behind Chao by it (names aren't unique).
2. **Chao identity**: which save slot a left-behind Chao is, across sessions: a stable field of
   the record (to find: DNA/ID bytes), not the per-session `Body::Chao` index.
3. **Owner ids 8..=15** on the wire for the server's left-behind Chao (they fit the item-place
   nibble); `player_id` stays 0..7, a new `chao_owner` decoder accepts 0..15 for Chao owner
   fields. Clients keep a list of the server's Chao (records + label "Name's Chao") outside
   `online.remote`, spawn copies for them like visitors, and follow `ServerChao` as usual.
4. **Server**: when an owner leaves a garden their (non "stays home") Chao there become
   left-behind Chao under that key; persisted in `SA2CO_DATA/stay/` (record + base, written
   atomically); run whenever at least one player is in that garden (`MIN_PLAYERS` 1 for a
   garden with left-behind Chao); changes write back to the stored record (not to an owner);
   evolution by the server (`chao_sim::evolve`, from `0x568630`); end of life pauses.
5. **Return**: the owner enters the garden; the server sends the merge (`StayReturn`: changes,
   mood, evolved fields) for each of their left-behind Chao; the owner's game applies it to the
   matching save slot, saves to disk (saving.rs), confirms; the server then drops its copy and
   the Chao is an ordinary visitor again.
6. **Unraid**: the container needs the `/data` volume, or left-behind Chao are lost on restart.

## Phases

1. Evolution on the server (RE of `0x568630`) — in progress.
2. Player key, owner ids 8..15, server storage, showing left-behind Chao to others.
3. Merge on return, the F9 "stays home" option, failure cases (crash mid-merge, server restart).
4. Tests in game with two real players; Unraid `/data` volume.
