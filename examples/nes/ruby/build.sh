#!/usr/bin/env bash
# Regenerate the dewasm-generated NES library and syntax-check the frontend. nes_gen.rb is generated code and is gitignored, so this step has to run before main.rb can require it from a clean checkout.
# There's no compile step for Ruby; `ruby -c` stands in for one.
set -euo pipefail
cd "$(dirname "$0")"

repo_root="$(cd ../../.. && pwd)"

../../apps/scripts/nes.sh

(
  cd "$repo_root"
  cargo run -q -p dewasm-cli -- \
    examples/apps/cache/nes.wasm \
    --target ruby --mode library --module-name Nes \
    -o examples/nes/ruby/nes_gen.rb
)

ruby -c main.rb

echo "build complete: nes_gen.rb regenerated"
