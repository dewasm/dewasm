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

for src in wat/*.wat; do
  id=$(basename "$src" .wat)
  # A case is allowed past wasm 1.0 only where its whole reason is the proposal it names, and only that proposal is enabled for it: any other case reaching for a post-1.0 instruction fails to assemble here.
  case "$id" in
    eh_*) wat2wasm --enable-exceptions "$src" -o "cache/wat/$id.wasm" ;;
    tail_call) wat2wasm --enable-tail-call "$src" -o "cache/wat/$id.wasm" ;;
    *) wat2wasm "$src" -o "cache/wat/$id.wasm" ;;
  esac
  echo "$id: $src -> cache/wat/$id.wasm"
done
