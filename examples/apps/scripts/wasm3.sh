#!/usr/bin/env bash
# shellcheck source-path=SCRIPTDIR
# shellcheck source=common.sh

# wasm3: a WebAssembly interpreter written in C, from its official wasm32-wasi release asset.
# The asset is the meta-WASI build, which forwards the guest's WASI calls to the outer host: the shape a converted interpreter needs, and the reason this app takes the module directly with no `--wasi` flag.
# Its dispatch is a musttail chain, so it needs the tail-call proposal; every backend but Bash lowers that, and Bash's e2e case is the one that stays commented out.

source "$(dirname "${BASH_SOURCE[0]}")/common.sh"

fetch_app wasm3 \
  "https://github.com/wasm3/wasm3/releases/download/v0.9.0/wasm3-wasi.wasm" \
  b8d07723d7c09516360a6196bdf6265d47f48cd7766fff7a92e68dd4854159e8
