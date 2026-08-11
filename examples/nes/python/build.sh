#!/usr/bin/env bash
# Regenerate the dewasm-generated NES library and byte-compile the terminal frontend. nes_gen.py is generated code and is gitignored, so this step has to run before main.py can import it from a clean checkout.
set -euo pipefail
cd "$(dirname "$0")"

repo_root="$(cd ../../.. && pwd)"

../../apps/scripts/nes.sh

(
  cd "$repo_root"
  cargo run -q -p dewasm-cli -- \
    examples/apps/cache/nes.wasm \
    --target python --mode library --module-name Nes \
    -o examples/nes/python/nes_gen.py
)

python3 -m py_compile main.py

echo "built $(pwd)/main.py (run with ./run.sh)"
