#!/usr/bin/env bash
# macOS counterpart of fetch-ffmpeg.ps1: downloads the pinned static GPL ffmpeg/ffprobe build
# (martin-riedl.de, BtbN has no macOS builds) and installs it as a Tauri sidecar.
# Idempotent: skips work when the binaries are already in place.
# NOTE: to upgrade, bump BUILD and every SHA-256 together (published next to each zip).
set -euo pipefail

BUILD_BASE='https://ffmpeg.martin-riedl.de/download/macos'
case "$(uname -m)" in
  arm64)
    TRIPLE='aarch64-apple-darwin'
    BUILD='arm64/1783011502_8.1.2'
    FFMPEG_SHA='ef1aa60006c7b77ce170c1608c08d8e4ba1c30c5746f2ac986ded932d0ac2c3c'
    FFPROBE_SHA='c39787f4af7a3932502d2d48db6f6feaaa836b48a73ef78c32cc3285df61dfaf'
    ;;
  x86_64)
    TRIPLE='x86_64-apple-darwin'
    BUILD='amd64/1783018342_8.1.2'
    FFMPEG_SHA='a52ef43883f44c219766d4b3bdde4e635b35465d0b704c01c3a0566b59775df9'
    FFPROBE_SHA='5408ca588c8c72b0dde3afe676d0a7acf25ef97e55ae6eba5c7bede1cda42695'
    ;;
  *) echo "unsupported architecture: $(uname -m)" >&2; exit 1 ;;
esac

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
DEST="$ROOT/src-tauri/binaries"
CACHE="$ROOT/.cache/ffmpeg-macos-${BUILD%%/*}"
mkdir -p "$DEST" "$CACHE"

install_tool() {
  local tool=$1 sha=$2
  local target="$DEST/$tool-$TRIPLE"
  if [[ -x "$target" ]]; then return; fi
  local zip="$CACHE/$tool.zip"
  if [[ ! -f "$zip" ]]; then
    echo "Downloading $BUILD_BASE/$BUILD/$tool.zip"
    curl -fsSL "$BUILD_BASE/$BUILD/$tool.zip" -o "$zip"
  fi
  local actual
  actual="$(shasum -a 256 "$zip" | cut -d' ' -f1)"
  if [[ "$actual" != "$sha" ]]; then
    rm -f "$zip"
    echo "SHA-256 mismatch for $tool.zip (expected $sha, got $actual)" >&2
    exit 1
  fi
  unzip -o -q "$zip" "$tool" -d "$CACHE"
  mv "$CACHE/$tool" "$target"
  chmod +x "$target"
}

install_tool ffmpeg "$FFMPEG_SHA"
install_tool ffprobe "$FFPROBE_SHA"
echo "Installed ffmpeg sidecars in $DEST"
