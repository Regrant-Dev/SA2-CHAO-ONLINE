#!/usr/bin/env bash
# Steam launch options: bash "/absolute/path/to/scripts/steam-launch.sh" %command%
# Keep Steam's Proton command and environment, opening SA Mod Manager before the game.
set -euo pipefail

steam_command=("$@")
for i in "${!steam_command[@]}"; do
    case "${steam_command[$i],,}" in
        */launcher.exe|*/sonic2app.exe)
            game_dir="${steam_command[$i]%/*}"
            manager="$game_dir/SAModManager.exe"
            if [ ! -f "$manager" ]; then
                echo "SA Mod Manager not found at $manager. Install it in the game folder first." >&2
                exit 1
            fi
            steam_command[$i]="$manager"
            cd -- "$game_dir"
            exec "${steam_command[@]}"
            ;;
    esac
done

echo 'Use this script in Steam Launch Options followed by %command%.' >&2
exit 1
