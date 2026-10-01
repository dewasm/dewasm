#!/usr/bin/env bash
# shellcheck source-path=SCRIPTDIR
# shellcheck source=common.sh

# `doom`: the `jacobenget/doom.wasm` v0.1.0 release binary (DOOM shareware, WAD embedded).
# The `examples/doom` example frontends and the deterministic framebuffer-snapshot test share it.
# So it lives in the apps cache like every other fixture, checked against a fixed checksum here.
# The old `examples/doom/fetch.sh` fetched it unverified.

source "$(dirname "${BASH_SOURCE[0]}")/common.sh"

fetch_app doom \
  "https://github.com/jacobenget/doom.wasm/releases/download/v0.1.0/doom-v0.1.0.wasm" \
  8edfe49a7583fd975199969302d8e9adcf8e714d0af72bf3e672f991fd810faa
