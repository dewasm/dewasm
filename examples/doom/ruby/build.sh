#!/usr/bin/env bash
# Regenerate the dewasm-generated DOOM library and syntax-check the
# frontend. doom_gen.rb is ~11MB of generated code and is gitignored, so
# this step has to run before main.rb can require it from a clean checkout.
# There's no compile step for Ruby; `ruby -c` stands in for one.
set -euo pipefail
cd "$(dirname "$0")"

repo_root="$(cd ../../.. && pwd)"

../../apps/scripts/doom.sh

(
  cd "$repo_root"
  cargo run -q -p dewasm-cli -- \
    examples/apps/cache/doom.wasm \
    --target ruby --mode library --module-name Doom \
    -o examples/doom/ruby/doom_gen.rb
)

ruby -c main.rb

echo "build complete: doom_gen.rb regenerated"
