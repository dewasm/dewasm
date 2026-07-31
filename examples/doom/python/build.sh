#!/usr/bin/env bash
# Regenerate the dewasm-generated DOOM library and byte-compile the terminal
# frontend. doom_gen.py is ~11MB of generated code and is gitignored, so
# this step has to run before main.py can import it from a clean checkout.
set -euo pipefail
cd "$(dirname "$0")"

repo_root="$(cd ../../.. && pwd)"

../../apps/scripts/doom.sh

(
  cd "$repo_root"
  cargo run -q -p dewasm-cli -- \
    examples/apps/cache/doom.wasm \
    --target python --mode library --module-name Doom \
    -o examples/doom/python/doom_gen.py
)

python3 -m py_compile main.py

echo "built $(pwd)/main.py (run with ./run.sh)"
