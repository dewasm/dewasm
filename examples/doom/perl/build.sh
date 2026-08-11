#!/usr/bin/env bash
# Regenerate the dewasm-generated DOOM library and syntax-check the frontend. doom_gen.pl is ~12MB of generated code and is gitignored, so this step has to run before main.pl can require it from a clean checkout.
# There's no compile step for Perl; `perl -c` stands in for one.
set -euo pipefail
cd "$(dirname "$0")"

repo_root="$(cd ../../.. && pwd)"

../../apps/scripts/doom.sh

(
  cd "$repo_root"
  cargo run -q -p dewasm-cli -- \
    examples/apps/cache/doom.wasm \
    --target perl --mode library --module-name Doom \
    -o examples/doom/perl/doom_gen.pl
)

perl -c main.pl

echo "build complete: doom_gen.pl regenerated"
