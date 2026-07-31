#!/usr/bin/env bash
# Regenerate the dewasm-generated DOOM library and build the ebiten
# frontend. doom_gen.go is ~10MB of generated code and is gitignored, so
# this step has to run before `go build` can succeed from a clean checkout.
set -euo pipefail
cd "$(dirname "$0")"

repo_root="$(cd ../../.. && pwd)"

../../apps/scripts/doom.sh

(
  cd "$repo_root"
  cargo run -q -p dewasm-cli -- \
    examples/apps/cache/doom.wasm \
    --target go --mode library --module-name doom \
    -o examples/doom/go/doom_gen.go
)

go build -o doom .

echo "built $(pwd)/doom"
