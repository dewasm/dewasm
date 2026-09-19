#!/usr/bin/env bash
# shellcheck source-path=SCRIPTDIR
# shellcheck source=common.sh

# cowsay: the classic args+stdout demo, from our own C reimplementation of cowsay 3.03.
# Its output is byte-identical to the original Perl script, and the binary is a tenth of the Rust clone the Wasmer registry serves.

source "$(dirname "${BASH_SOURCE[0]}")/common.sh"

fetch_app cowsay \
  "https://github.com/dewasm/cowsay.wasm/releases/download/v0.2.0/cowsay.wasm" \
  97ff518a9e005edc3a008754f282c77b459e30e3c2c59609db95af380fdfad50
