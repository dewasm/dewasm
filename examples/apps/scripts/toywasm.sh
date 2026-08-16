#!/usr/bin/env bash
# shellcheck source-path=SCRIPTDIR
# shellcheck source=common.sh

# toywasm: a WebAssembly interpreter written in C, from its official wasm32-wasi release asset.
# The default (non-`full-`) configuration is the one pinned here: `--print-build-options` reports tail-call, threads, multi-memory, extended-const and custom-page-sizes off, and the binary itself audits as baseline wasm.

source "$(dirname "${BASH_SOURCE[0]}")/common.sh"

fetch_app toywasm \
  "https://github.com/yamt/toywasm/releases/download/v76.0.0/toywasm-v76.0.0-wasm32-wasi.tgz" \
  17deb0437afc26fc4f9081e3e2330f925576f92c586edbd255f5403c2fb2678f \
  bin/toywasm
