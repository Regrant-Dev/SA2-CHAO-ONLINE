<#
.SYNOPSIS
  Build SA2 Chao Online on Windows, install it into the game, and/or make the release zip.

.DESCRIPTION
  Needs Rust (https://rustup.rs) and the "Desktop development with C++" workload of the
  Visual Studio Build Tools (for the MSVC linker). The 32-bit target is added automatically.

  -Install (default): back up SAVEDATA when it changed, build, and copy the mod into
                      <GameDir>\mods\sa2-chao-online.
  -Package:           build and write dist\sa2-chao-online.zip (never contains
                      config.ini/online.ini, so multiplayer starts Off for everyone).

.EXAMPLE
  powershell -ExecutionPolicy Bypass -File scripts\windows.ps1
  powershell -ExecutionPolicy Bypass -File scripts\windows.ps1 -Package
  powershell -ExecutionPolicy Bypass -File scripts\windows.ps1 -GameDir "D:\Games\Sonic Adventure 2"
#>
param(
    [string]$GameDir = "${env:ProgramFiles(x86)}\Steam\steamapps\common\Sonic Adventure 2",
    [switch]$Package,
    [switch]$Install
)
$ErrorActionPreference = "Stop"
Set-Location (Split-Path $PSScriptRoot -Parent)
if (-not $Package -and -not $Install) { $Install = $true }

$Target = "i686-pc-windows-msvc"
$ImportTarget = "x86_64-pc-windows-msvc"
$Dll = "target\$Target\release\sa2_chao_online.dll"

function Build {
    rustup target add $Target $ImportTarget | Out-Null
    cargo build --release --target $Target --features launcher
    if ($LASTEXITCODE -ne 0) { throw "cargo build failed" }
    cargo build --release --target $ImportTarget --features game-import --bin sa2co-import
    if ($LASTEXITCODE -ne 0) { throw "local importer build failed" }
}

Build

if ($Install) {
    if (-not (Test-Path "$GameDir\sonic2app.exe")) {
        throw "Sonic Adventure 2 not found in '$GameDir' (use -GameDir)"
    }
    # Back up the saves whenever they changed since the last backup.
    $Saves = "$GameDir\resource\gd_PC\SAVEDATA"
    $Backups = "$env:USERPROFILE\sa2-savedata-backups"
    if ((Test-Path $Saves) -and (Get-ChildItem $Saves)) {
        $hash = (Get-ChildItem $Saves -File | Get-FileHash | ForEach-Object Hash) -join ""
        $hash = (Get-FileHash -InputStream ([IO.MemoryStream]::new([Text.Encoding]::UTF8.GetBytes($hash)))).Hash.Substring(0, 12)
        if (-not (Get-ChildItem $Backups -Directory -Filter "*-$hash" -ErrorAction SilentlyContinue)) {
            $dest = "$Backups\$(Get-Date -Format yyyyMMdd-HHmmss)-$hash"
            New-Item -ItemType Directory -Force $dest | Out-Null
            Copy-Item "$Saves\*" $dest -Recurse
            Write-Host "Backed up saves to $dest"
        }
    }
    $Mod = "$GameDir\mods\sa2-chao-online"
    New-Item -ItemType Directory -Force "$Mod\chao\import" | Out-Null
    Copy-Item mod\mod.ini, mod\configschema.xml, docs\how-to-play.md $Mod
    Copy-Item LICENSE "$Mod\LICENSE.txt"
    Copy-Item $Dll "$Mod\sa2-chao-online.dll"
    (Get-FileHash "$Mod\sa2-chao-online.dll").Hash.ToLowerInvariant() | Set-Content "$Mod\mod.sha256"
    Copy-Item "target\$Target\release\sa2co-launcher.exe" $Mod
    Copy-Item "target\$ImportTarget\release\sa2co-import.exe" $Mod
    Copy-Item tools\source-model-notices.txt $Mod
    git rev-parse HEAD | Set-Content "$Mod\mod.build"
    # SA Mod Manager offers a GitHub release as an update when it's newer than this.
    (Get-Date).ToUniversalTime().ToString("yyyy-MM-ddTHH:mm:ssZ") | Set-Content "$Mod\mod.version"
    Write-Host "Installed to $Mod"
}

if ($Package) {
    $version = (Select-String -Path Cargo.toml -Pattern '^version = "(.*)"').Matches[0].Groups[1].Value
    $out = "dist\sa2-chao-online"
    Remove-Item -Recurse -Force $out, "dist\sa2-chao-online.zip", "dist\sa2co-server.exe" -ErrorAction SilentlyContinue
    New-Item -ItemType Directory -Force "$out\chao\import", "$out\chao\export" | Out-Null
    Copy-Item $Dll "$out\sa2-chao-online.dll"
    (Get-FileHash "$out\sa2-chao-online.dll").Hash.ToLowerInvariant() | Set-Content "$out\mod.sha256"
    Copy-Item "target\$Target\release\sa2co-launcher.exe" $out
    Copy-Item "target\$ImportTarget\release\sa2co-import.exe" $out
    Copy-Item tools\source-model-notices.txt $out
    git rev-parse HEAD | Set-Content "$out\mod.build"
    (Get-Content mod\mod.ini) -replace '^Version=.*', "Version=$version" | Set-Content "$out\mod.ini"
    Copy-Item mod\configschema.xml $out
    Copy-Item docs\how-to-play.md $out
    Copy-Item LICENSE "$out\LICENSE.txt"
    Copy-Item "target\$Target\release\sa2co-server.exe" "dist\sa2co-server.exe"
    New-Item -ItemType File -Force "$out\chao\import\.keep", "$out\chao\export\.keep" | Out-Null
    Compress-Archive -Path $out -DestinationPath "dist\sa2-chao-online.zip"
    Write-Host "Packaged dist\sa2-chao-online.zip (version $version)"
}
