#!/usr/bin/env bash
# Build the mod and copy it into the game's mods folder.
# Backs up SAVEDATA first whenever it changed since the last backup.
set -euo pipefail
GAME="${SA2_DIR:-$HOME/.local/share/Steam/steamapps/common/Sonic Adventure 2}"
DEST="$GAME/mods/sa2-chao-online"
SAVES="$GAME/resource/gd_PC/SAVEDATA"
BACKUPS="${SA2_BACKUP_DIR:-$HOME/sa2-savedata-backups}"
cd "$(dirname "$0")/.."

if [ ! -f "$GAME/sonic2app.exe" ]; then
    echo "No game in $GAME. Set SA2_DIR to your Sonic Adventure 2 folder." >&2
    exit 1
fi

if [ -d "$SAVES" ] && [ -n "$(ls -A "$SAVES")" ]; then
    hash=$(cd "$SAVES" && sha256sum * | sha256sum | cut -c1-12)
    if ! ls "$BACKUPS" 2>/dev/null | grep -q -- "-$hash$"; then
        mkdir -p "$BACKUPS"
        cp -a "$SAVES" "$BACKUPS/$(date +%Y%m%d-%H%M%S)-$hash"
        echo "Backed up saves to $BACKUPS"
    fi
fi

cargo build --release --features launcher
cargo build --release --target x86_64-pc-windows-gnu --features game-import --bin sa2co-import
cargo build --release --target x86_64-unknown-linux-musl --features launcher --bin sa2co-launcher
mkdir -p "$DEST/chao/import"
cp mod/mod.ini mod/configschema.xml docs/how-to-play.md tools/source-model-notices.txt "$DEST/"
cp LICENSE "$DEST/LICENSE.txt"
cp --remove-destination target/i686-pc-windows-gnu/release/sa2_chao_online.dll "$DEST/sa2-chao-online.dll"
cp --remove-destination target/i686-pc-windows-gnu/release/sa2co-launcher.exe "$DEST/"
cp --remove-destination target/x86_64-pc-windows-gnu/release/sa2co-import.exe "$DEST/"
cp --remove-destination target/x86_64-unknown-linux-musl/release/sa2co-launcher "$DEST/"
git rev-parse HEAD > "$DEST/mod.build"
sha256sum "$DEST/sa2-chao-online.dll" | cut -d ' ' -f1 > "$DEST/mod.sha256"
# SA Mod Manager offers a GitHub release as an update when it's newer than this: a local build
# counts as current until something newer is published.
date -u +%Y-%m-%dT%H:%M:%SZ > "$DEST/mod.version"
# Your own skins (made with tools/skinconv.py into skins/, which git ignores).
if [ -d skins ] && [ -n "$(ls -A skins)" ]; then
    mkdir -p "$DEST/skins"
    for skin in skins/*/; do
        rm -rf "$DEST/skins/$(basename "$skin")"
    done
    cp -a skins/. "$DEST/skins/"
    # Their textures where the mod loader finds texture packs (the mod also does this at start).
    for skin in skins/*/; do
        id=$(basename "$skin")
        if [ -d "$skin/textures" ]; then
            pack="$DEST/textures/SKIN_${id^^}"
            rm -rf "$pack"
            mkdir -p "$pack"
            cp -a "$skin/textures/." "$pack/"
        fi
    done
    echo "Copied skins: $(ls skins | tr '\n' ' ')"
fi
echo "Installed to $DEST"
