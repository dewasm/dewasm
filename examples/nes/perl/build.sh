#!/usr/bin/env bash
# Regenerate the dewasm-generated NES library and syntax-check the frontend.
# nes_gen.pl is gitignored, so this step has to run before main.pl can require it from a clean checkout.
# There's no compile step for Perl;
# `perl -c` stands in for one.
set -euo pipefail
cd "$(dirname "$0")"

repo_root="$(cd ../../.. && pwd)"

../../apps/scripts/nes.sh

(
  cd "$repo_root"
  cargo run -q -p dewasm-cli -- \
    examples/apps/cache/nes.wasm \
    --target perl --mode library --module-name Nes \
    -o examples/nes/perl/nes_gen.pl
)

perl -c main.pl

echo "build complete: nes_gen.pl regenerated"
