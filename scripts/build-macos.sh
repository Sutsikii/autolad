#!/usr/bin/env bash
# Builds AutoLad.app and its .dmg on a Mac (Apple Silicon or Intel), ready to share.
# Needs Xcode Command Line Tools, CMake, Rust and pnpm:
#   xcode-select --install && brew install cmake pnpm && curl https://sh.rustup.rs -sSf | sh
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

for tool in cmake cargo pnpm; do
  command -v "$tool" >/dev/null || { echo "missing $tool (see the header of this script)" >&2; exit 1; }
done

# .cargo/config.toml is set up for Windows (short C:/t target dir, LLVM's libclang): environment
# variables take precedence over it.
export CARGO_BUILD_TARGET_DIR="$ROOT/target"
LIBCLANG_PATH="$(xcode-select -p)/Toolchains/XcodeDefault.xctoolchain/usr/lib"
[[ -d "$LIBCLANG_PATH" ]] || LIBCLANG_PATH="$(xcode-select -p)/usr/lib"
export LIBCLANG_PATH

bash scripts/fetch-ffmpeg.sh
pnpm install --frozen-lockfile
pnpm tauri build --bundles app,dmg

echo
echo "Done:"
ls "$ROOT"/target/release/bundle/dmg/*.dmg
