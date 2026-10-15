#!/usr/bin/env bash
# Build a release zip for SA Mod Manager: dist/sa2-chao-online.zip
# It never contains config.ini/online.ini, so multiplayer starts Off for everyone.
set -euo pipefail
cd "$(dirname "$0")/.."

version=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)
out=dist/sa2-chao-online
rm -rf "$out" dist/sa2-chao-online.zip dist/sa2co-server.exe
mkdir -p "$out/chao/import" "$out/chao/export"

cargo build --release
cp target/i686-pc-windows-gnu/release/sa2_chao_online.dll "$out/sa2-chao-online.dll"
i686-w64-mingw32-strip "$out/sa2-chao-online.dll"
# The dedicated server for Windows, a separate download (not part of the mod folder).
cp target/i686-pc-windows-gnu/release/sa2co-server.exe dist/sa2co-server.exe
i686-w64-mingw32-strip dist/sa2co-server.exe
sed "s/^Version=.*/Version=$version/" mod/mod.ini > "$out/mod.ini"
cp mod/configschema.xml "$out/"
cp docs/how-to-play.md "$out/how-to-play.md"
cp LICENSE "$out/LICENSE.txt"
touch "$out/chao/import/.keep" "$out/chao/export/.keep"

for f in config.ini online.ini; do
    if [ -e "$out/$f" ]; then echo "refusing to ship $f" >&2; exit 1; fi
done

if command -v zip >/dev/null; then
    (cd dist && zip -qr sa2-chao-online.zip sa2-chao-online)
else
    (cd dist && 7z a -tzip -bso0 sa2-chao-online.zip sa2-chao-online)
fi
echo "Packaged dist/sa2-chao-online.zip (version $version)"
