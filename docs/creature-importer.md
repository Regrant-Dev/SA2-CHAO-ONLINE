# Creature importer (developers)

`sa2co-import` reads a cartridge image and builds the private local cache the creature link
uses: the battle catalog, 3D actors, thumbnails, garden movement sets and the battle venue.
The mod runs it in the background when the game starts (or the creature link is opened)
without a ready cache; release packages include it as a 64-bit Windows program next to the
mod's DLL. The game itself only loads the finished cache.

## Build

The shipped importer has two commands, `build` and `verify-cache`:

```sh
cargo build --release --target x86_64-unknown-linux-gnu --features game-import --bin sa2co-import
cargo build --release --target x86_64-pc-windows-gnu --features game-import --bin sa2co-import
```

`scripts/package.sh`, `scripts/install.sh` and `scripts/windows.ps1` build the Windows one.

## Build and verify a cache

```sh
importer=target/x86_64-unknown-linux-gnu/release/sa2co-import
"$importer" build /path/to/your-dump.3ds --output target/source-import/private/cache
"$importer" verify-cache target/source-import/private/cache --actors \
  --output target/source-import/private/verification.json
```

`build` checks the image, converts everything into a temporary `.source-cache-<process id>`
folder next to the destination, and publishes the complete cache in one step. The cache is
about 3.6 GB, and `build` asks for 7.2 GB free, less the size of a cache it replaces (never
under 3.6 GB). The previous cache is kept until the new one is published, then removed. A
destination that isn't a cache, or that contains the image, is refused.

The cache's fingerprint covers only the imported data and its layout (the catalog format and
profile), so a change to the battle rules doesn't need a new build; peers compare the rules
separately.

`verify-cache` opens the catalog against the current battle rules, checks the trainer ladder
and lists the moves the battles don't support. With `--actors` it also decodes every normal
and shiny actor, poses each skeleton and checks the thumbnails. Reports never overwrite an
existing file.

## Errors

On failure the importer prints one line on stderr, `error[code]: message`; progress goes to
stdout. The codes live in `src/creature/import_error.rs`, which the mod also uses to pick the
message the player sees:

| Code | Meaning |
|---|---|
| `missing` | The image file isn't there or can't be opened |
| `format` | Not a cartridge image: an installable package, a bare partition or another file |
| `encrypted` | Encrypted or damaged: a section doesn't match its digest |
| `wrong-title` | Another game |
| `update` | Update or add-on data, or an image with an update applied |
| `changed` | The image changed while it was being read |
| `space` | Not enough free disk space |
| `cache` | The cache folder can't be used or written |
| `usage` | A wrong command line |
| `invalid` | Unreadable or unexpected data in the image |
| `failed` | The image read fine, but a conversion or write failed while preparing the cache |

## Developer commands

Inspection commands are built only with the `game-import-dev` feature and never ship:

```sh
cargo build --release --target x86_64-unknown-linux-gnu --features game-import-dev --bin sa2co-import
"$importer" --help
```

| Command | What it does |
|---|---|
| `inspect IMAGE [--full-hash] [--output REPORT.json]` | Image identity, verified sections and filesystem counts; `--full-hash` adds a SHA-256 of the whole image |
| `list IMAGE [ROMFS_PREFIX]` | Matching RomFS paths with their offsets and lengths |
| `catalog IMAGE --output CATALOG.json` | A diagnostic report of the decoded records |
| `build-data IMAGE --output CATALOG.json` | The battle catalog only |
| `assets IMAGE --output DIR [--first N] [--last N]` | Actors for a range of entries |
| `field-motions IMAGE --output DIR [--keys SPECIES:FORM,...]` | Garden movement sets |
| `venue IMAGE --output DIR` | The battle venue |
| `extract IMAGE ROMFS_PATH ENTRY [--subfile N] [--lz11] --output FILE` | One archive entry (subfile 0 by default; `--lz11` decodes it) |

Use a fresh output folder under `target/source-import/private/` for each run: the tool
refuses to replace an existing file.

## Tests

```sh
cargo test --target x86_64-unknown-linux-gnu --features launcher,game-import
SA2CO_TEST_CACHE=target/source-import/private/cache \
  cargo test --target x86_64-unknown-linux-gnu --features launcher,game-import
```

The importer's own tests use synthetic data only: malformed and truncated structures, bad
ranges and references, bounded decompression, record checks, and publishing that never
replaces files. Tests that check against a real catalog read `SA2CO_TEST_CACHE` (a cache
folder or its catalog file) and print `skipped: SA2CO_TEST_CACHE not set` without it; a cache
that is set but outdated fails. Tests that check against the installed game read
`SA2CO_GAME_DIR` (the game's folder) and skip the same way. See [testing](testing.md).

## Privacy

Cartridge images and everything extracted, converted or cached from them stay on the
developer's or player's computer. `.gitignore` covers `*.3ds`, `*.cia`, `*.cxi`, `*.cci`, the
`/creature/` folder and `target/`. Nothing from them goes into Git, Docker images, release
zips, or network and skin sharing; the skin server skips catalog and cache files.
