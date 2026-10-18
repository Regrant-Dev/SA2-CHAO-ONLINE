# Making skins

A skin is a folder in `mods/sa2-chao-online/skins/<id>/` (the id: lowercase letters, digits and
`_`). The mod has no skin built in: everything about one lives in its folder, and a server
shares the folders with everyone who joins (how-to-play.md).

| File | What |
|---|---|
| `skin.ini` | Its settings (below). |
| `model.sa2mdl` | The model (made by the tools below). |
| `motions.sa2mot` | An animated skin's animations. |
| `textures/` | Its texture pack: `index.txt` and PNGs. |
| `sounds/<moment>/*.wav` | Its voice, optional. |

Third-party characters stay out of the public repo: the sources go in `skins-src/` and the
built skins in `skins/`, both ignored by git.

## Kinds

- **Animated** (`tools/skinanim.py`): the model on its own skeleton with its own animations
  (Mixamo's, merged with `tools/blender_merge_anims.py`). It plays idle, walk, run, jump and
  fall from how the character moves, and the garden animations from what it's doing. Vertices
  moved by several bones keep their weights (up to 3 bones each), so joints bend smoothly;
  `--rigid` puts each on its strongest bone instead. `--clean-sides` takes the other side's
  leg and arm bones off vertices clearly on one side, for auto-rigged models whose legs nearly
  touch: the rigging bleeds weights between them and each leg pulls at the other (not for a
  skirt that joins the legs).
- **Solid** (`tools/skinconv.py`): one model riding on the character, like a car.
- **Rigged** (`tools/skinrig.py`): the model fitted onto the characters' own skeletons and moved
  by their own animations: one model per character (`--character all`: Sonic, Shadow, Amy,
  Metal Sonic, Knuckles, Rouge, Tails and Eggman on foot; not the walkers).

Whatever kind, nothing of the character shows while a skin is on: not its body, wings,
upgrades, effects drawn with it or its jump ball, and the shadow is the skin's (re/skins.md "Hiding the
character").

`skins-src/build.sh` builds all of ours; each tool's `--help` lists its options.

## skin.ini

The tools write most of it: `name`, `textures`, `height`, `character_scale` (the size it's drawn
at), `characters` (who can wear it; empty for anyone; a rigged skin: whoever it has a
`rig_<character>=<body slots>;<texture field>;<height>` line for), and for animated skins the animations
and which one plays when (`idle=`, `walk=`, `run=`, `jump=`, `fall=`, `pickup=`, `carry=`,
`carry_walk=`, `carry_run=`, `throw=`, `put_down=`, `pet=`, `shake=`, `whistle=`).

How an animated skin's walks and runs keep up with the character (optional):

| Line | Default | What |
|---|---|---|
| `anim_walk_from` | 0.15 | ground speed (units a frame) where it starts walking |
| `anim_run_from` | 2.0 | where it runs, if its walk and run don't say how far they go (Mixamo ones do; then it's halfway between their paces) |
| `anim_walk_speed` | 0.8 | the speed an in-place walk looks right at, at size 1 |
| `anim_run_speed` | 3.0 | the same for an in-place run |
| `anim_max_rate` | 6 | the fastest a walk or run plays, times its own pace |

## Voice

`sounds/` holds a folder per moment; each WAV (16-bit PCM, mono or stereo) in it is one clip,
picked at random and never twice in a row:

| Folder | When |
|---|---|
| `jump`, `attack`, `boost`, `hurt` | Quick grunts (clips over a second are skipped here). |
| `death`, `fall_death` | Losing a life (falling, for `fall_death`). |
| `pickup`, `throw`, `put_down`, `pet`, `whistle`, `shake_tree` | The garden actions. |
| `happy`, `idle` | Standing about now and then. |
| `signature` | The skin's catchphrase: on some jumps, now and then going fast, in idle chatter. |

While a skin with sounds is on, the character's own voice is muted (cutscenes, Omochao and the
story still speak). File names: letters, digits, spaces and `_ - . ( ) ! ' , & +` only, and the
whole path under 96 characters, or it won't be shared.

How chatty it is goes in `skin.ini` (all optional; in our build, `skins-src/sounds/<who>/voice.ini`
is added to it):

| Line | Default | What |
|---|---|---|
| `voice_jump` | 3 | 1 in this many jumps gets a voice (0: none) |
| `voice_signature_jump` | 4 | of those, 1 in this many is a signature line (0: none) |
| `voice_signature_every` | 25 | going fast: at most one signature line this often, seconds (0: none) |
| `voice_fast` | 4.0 | the speed that counts as fast (units a frame) |
| `voice_idle_after` | 20 | standing still this long starts idle chatter, seconds |
| `voice_idle_every` | 60 | then at most one line this often, seconds |
| `voice_quiet` | 10 | idle, happy and signature lines wait this long after any line, seconds |
| `voice_grunt_rest` | 4 | the same short clip doesn't play again this soon, seconds |
| `voice_line_rest` | 30 | nor the same longer clip, seconds |
| `voice_volume` | 0.8 | loudness, 0 to 1 |
| `voice_cooldown_<folder>` | per moment | least time between two lines of that moment, seconds (e.g. `voice_cooldown_hurt=2`) |
