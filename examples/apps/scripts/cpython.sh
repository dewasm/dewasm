#!/usr/bin/env bash
# shellcheck source-path=SCRIPTDIR
# shellcheck source=common.sh

# CPython 3.14.9: an unofficial wasm32-wasip1 build (`brettcannon/cpython-wasi-build`).
# The PSF distributes no WASI binaries; this is the build of a CPython core developer.
# Beyond python.wasm we also extract the standard library tree (`lib/python3.14`).
# The interpreter reads it at start from a preopened directory.
# The e2e case preopens `cache/cpython-lib/lib` at guest `/lib`.
# It runs with `PYTHONHOME=/` and `PYTHONPATH=/lib/python3.14`.
# Every backend converts and runs it, behind the `slow_test`/`ultra_slow_test` Cargo features.
# The speed category varies by backend.
# The audit record is agents/apps-audit.md.

source "$(dirname "${BASH_SOURCE[0]}")/common.sh"

fetch_runtime_with_stdlib cpython \
  "https://github.com/brettcannon/cpython-wasi-build/releases/download/v3.14.9/python-3.14.9-wasi_sdk-24.zip" \
  8de7834105edef8db8a13299e2587cf8945021dd8764f2691d83bbc25129dfa2 \
  python.wasm lib/python3.14
