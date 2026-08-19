#!/usr/bin/env bash
# Regenerate the dewasm-generated NES library and build the ebiten frontend. nes/nes_gen.go is generated code and is gitignored, so this step has to run before `go build` can succeed from a clean checkout.
# The binary goes to bin/nes: the package directory beside it is already called nes.
set -euo pipefail
cd "$(dirname "$0")"

repo_root="$(cd ../../.. && pwd)"

../../apps/scripts/nes.sh

(
  cd "$repo_root"
  cargo run -q -p dewasm-cli -- \
    examples/apps/cache/nes.wasm \
    --target go --mode library --module-name nes \
    -o examples/nes/go/nes/nes_gen.go
)

go build -o bin/nes .

echo "built $(pwd)/bin/nes"
