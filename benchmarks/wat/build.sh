#!/usr/bin/env bash

# Assemble the hand-written .wat microbenchmarks into benchmarks/cache/wat/<id>.wasm, so that every runner in the suite consumes byte-identical modules.
#
# The .wat sources are checked in; the built .wasm is not, like everything else under cache/.
# Run this after editing one, or via `benchmarks/setup.sh`, which calls it together with the C family's benchmarks/c/build.sh.
#
# Idempotent, and cheap enough that it just rebuilds unconditionally.

set -euo pipefail

cd "$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
mkdir -p cache/wat

# Fail loudly with an actionable message rather than half-building.
require_tool() {
  command -v "$1" >/dev/null && return
  echo "benchmarks/wat/build.sh: $1 not found: $2" >&2
  exit 1
}

require_tool wat2wasm "install wabt (brew install wabt / apt install wabt)"

# wabt turns the post-baseline proposals on by default (1.0.42 dropped the --enable-exceptions spelling entirely), so each case turns them all off and re-enables only the proposal it names.
# The universally-emitted baseline (sign extension, saturating float-to-int, multi-value, bulk memory, mutable globals) stays on everywhere, since every backend accepts it.
# reference-types is absent: wabt encodes an exception tag through it, so disabling it fails the eh_* cases.
post_baseline=(
  --disable-exceptions
  --disable-tail-call
  --disable-simd
  --disable-relaxed-simd
  --disable-memory64
  --disable-multi-memory
  --disable-extended-const
)

for src in wat/*.wat; do
  id=$(basename "$src" .wat)
  # A case is allowed past wasm 1.0 only where its whole reason is the proposal it names, and only that proposal is enabled for it: any other case reaching for a post-1.0 instruction fails to assemble here.
  keep=
  case "$id" in
    eh_*) keep=--disable-exceptions ;;
    tail_call) keep=--disable-tail-call ;;
  esac
  flags=()
  for flag in "${post_baseline[@]}"; do
    [ "$flag" = "$keep" ] || flags+=("$flag")
  done
  wat2wasm "${flags[@]}" "$src" -o "cache/wat/$id.wasm"
  echo "$id: $src -> cache/wat/$id.wasm"
done
