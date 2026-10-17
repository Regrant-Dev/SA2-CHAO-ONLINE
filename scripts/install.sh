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

cargo build --release
mkdir -p "$DEST/chao/import"
cp mod/mod.ini mod/configschema.xml "$DEST/"
cp target/i686-pc-windows-gnu/release/sa2_chao_online.dll "$DEST/sa2-chao-online.dll"
echo "Installed to $DEST"
