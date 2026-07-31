#!/usr/bin/env bash
# shellcheck source-path=SCRIPTDIR
# shellcheck source=common.sh

# CPython 3.14.6: an unofficial wasm32-wasip1 build
# (brettcannon/cpython-wasi-build — the PSF distributes no WASI binaries; this
# is a CPython core dev's build). Beyond python.wasm we also extract the
# stdlib tree (lib/python3.14) the interpreter reads at startup from a
# preopened directory: the e2e case preopens cache/cpython-lib/lib at guest
# /lib (PYTHONHOME=/, PYTHONPATH=/lib/python3.14). Ruby-only, heavy —
# execution behind the `heavy_test` cargo feature (docs/apps-audit.md).

source "$(dirname "${BASH_SOURCE[0]}")/common.sh"

fetch_runtime_with_stdlib cpython \
  "https://github.com/brettcannon/cpython-wasi-build/releases/download/v3.14.6/python-3.14.6-wasi_sdk-24.zip" \
  73bf2e9774c4d8820d0877ec5db0b963df3a9611fc2a63838aeaee29dfd034e6 \
  python.wasm lib/python3.14
