#!/usr/bin/env bash
# Download the doom.wasm release binary (jacobenget/doom.wasm v0.1.0) into the
# gitignored cache. The module embeds the Doom shareware WAD, so no game data
# is needed beyond this one file.
set -euo pipefail
cd "$(dirname "$0")"

mkdir -p cache
if [ ! -f cache/doom.wasm ]; then
  curl -fL -o cache/doom.wasm.tmp \
    https://github.com/jacobenget/doom.wasm/releases/download/v0.1.0/doom-v0.1.0.wasm
  mv cache/doom.wasm.tmp cache/doom.wasm
fi
echo "cache/doom.wasm ready"
