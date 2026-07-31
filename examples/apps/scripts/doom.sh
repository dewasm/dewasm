#!/usr/bin/env bash
# shellcheck source-path=SCRIPTDIR
# shellcheck source=common.sh

# doom: the jacobenget/doom.wasm v0.1.0 release binary (DOOM shareware, WAD
# embedded). Shared between the examples/doom demo frontends (ADR-50) and the
# deterministic framebuffer-golden test (ADR-53), so it lives in the apps
# cache like every other fixture — checksum-pinned here, unlike the old
# examples/doom/fetch.sh which downloaded it unverified.

source "$(dirname "${BASH_SOURCE[0]}")/common.sh"

fetch_app doom \
  "https://github.com/jacobenget/doom.wasm/releases/download/v0.1.0/doom-v0.1.0.wasm" \
  8edfe49a7583fd975199969302d8e9adcf8e714d0af72bf3e672f991fd810faa
