# Downloads the pinned GPL ffmpeg/ffprobe build and installs it as a Tauri sidecar.
# Idempotent: skips work when the binaries are already in place.
# NOTE: BtbN prunes old autobuild tags; when the URL 404s, bump $Tag/$Asset/$Sha256 together.
$ErrorActionPreference = 'Stop'

$Tag    = 'autobuild-2026-09-29-13-10'
$Asset  = 'ffmpeg-n8.1.3-6-gff48edd8b2-win64-gpl-8.1.zip'
$Sha256 = '6e3294ba26c4a21c267ca1dc8a029268b2b89ce61496d9d4be8ed820a42a0c27'
$Triple = 'x86_64-pc-windows-msvc'

$Root = Split-Path -Parent $PSScriptRoot
$Dest = Join-Path $Root 'src-tauri/binaries'
$Ffmpeg = Join-Path $Dest "ffmpeg-$Triple.exe"
$Ffprobe = Join-Path $Dest "ffprobe-$Triple.exe"

if ((Test-Path $Ffmpeg) -and (Test-Path $Ffprobe)) { Write-Host 'ffmpeg sidecars already present'; return }

$Cache = Join-Path $Root '.cache'
New-Item -ItemType Directory -Force $Cache, $Dest | Out-Null
$Zip = Join-Path $Cache $Asset

if (-not (Test-Path $Zip)) {
    $Url = "https://github.com/BtbN/FFmpeg-Builds/releases/download/$Tag/$Asset"
    Write-Host "Downloading $Url"
    Invoke-WebRequest $Url -OutFile $Zip
}

$Actual = (Get-FileHash $Zip -Algorithm SHA256).Hash.ToLower()
if ($Actual -ne $Sha256) {
    Remove-Item -LiteralPath $Zip
    throw "SHA-256 mismatch for $Asset (expected $Sha256, got $Actual)"
}

$Extract = Join-Path $Cache 'ffmpeg-extract'
if (Test-Path $Extract) { Remove-Item -LiteralPath $Extract -Recurse -Force }
Expand-Archive $Zip -DestinationPath $Extract
$Inner = (Get-ChildItem $Extract -Directory | Select-Object -First 1).FullName

Copy-Item (Join-Path $Inner 'bin/ffmpeg.exe') $Ffmpeg -Force
Copy-Item (Join-Path $Inner 'bin/ffprobe.exe') $Ffprobe -Force
Copy-Item (Join-Path $Inner 'LICENSE.txt') (Join-Path $Root 'third-party/ffmpeg/LICENSE.txt') -Force
Remove-Item -LiteralPath $Extract -Recurse -Force
Write-Host "Installed ffmpeg sidecars in $Dest"
