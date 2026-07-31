#!/usr/bin/env bash
# CPython 3.14.6: the official wasm32-wasip1 build
# (brettcannon/cpython-wasi-build). Beyond python.wasm we also extract the
# stdlib tree (lib/python3.14) the interpreter reads at startup from a
# preopened directory: the e2e case preopens cache/cpython-lib/lib at guest
# /lib (PYTHONHOME=/, PYTHONPATH=/lib/python3.14). Ruby-only, heavy —
# execution behind the `heavy_test` cargo feature (docs/apps-audit.md).
# shellcheck source-path=SCRIPTDIR
# shellcheck source=common.sh
source "$(dirname "${BASH_SOURCE[0]}")/common.sh"

CPYTHON_URL="https://github.com/brettcannon/cpython-wasi-build/releases/download/v3.14.6/python-3.14.6-wasi_sdk-24.zip"
CPYTHON_SHA256="73bf2e9774c4d8820d0877ec5db0b963df3a9611fc2a63838aeaee29dfd034e6"

cpython_stamp="cache/cpython.src-sha256"
if is_cached "$cpython_stamp" "$CPYTHON_SHA256" \
  cache/cpython.wasm cache/cpython-lib/lib/python3.14; then
  echo "cpython: cached"
  exit 0
fi

require_tool cpython unzip
echo "cpython: fetching $CPYTHON_URL"
new_tmpdir
fetch_verified "$CPYTHON_URL" "$CPYTHON_SHA256" "$tmp/py.zip"
unzip -qo "$tmp/py.zip" python.wasm -d "$tmp"
cp "$tmp/python.wasm" cache/cpython.wasm
rm -rf cache/cpython-lib
mkdir -p cache/cpython-lib
unzip -qo "$tmp/py.zip" 'lib/python3.14/*' -d cache/cpython-lib
write_stamp "$cpython_stamp" "$CPYTHON_SHA256"
echo "cpython: -> cache/cpython.wasm, cache/cpython-lib/lib/python3.14"
