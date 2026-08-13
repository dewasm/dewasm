#!/usr/bin/env bash
# Regenerate the dewasm-generated DOOM library and byte-compile the terminal frontend. doom_gen.py is ~11MB of generated code and is gitignored, so this step has to run before main.py can import it from a clean checkout.
# The byte-compile check runs under the same interpreter run.sh will use: PyPy when installed, unless $PYTHON says otherwise.
set -euo pipefail
cd "$(dirname "$0")"

if [[ -z "${PYTHON:-}" ]]; then
  if command -v pypy3 >/dev/null 2>&1; then PYTHON=pypy3; else PYTHON=python3; fi
fi

repo_root="$(cd ../../.. && pwd)"

../../apps/scripts/doom.sh

(
  cd "$repo_root"
  cargo run -q -p dewasm-cli -- \
    examples/apps/cache/doom.wasm \
    --target python --mode library --module-name Doom \
    -o examples/doom/python/doom_gen.py
)

"$PYTHON" -m py_compile main.py

echo "built $(pwd)/main.py (run with ./run.sh)"
