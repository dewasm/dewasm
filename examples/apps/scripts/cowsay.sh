#!/usr/bin/env bash
# shellcheck source-path=SCRIPTDIR
# shellcheck source=common.sh

# cowsay: the classic args+stdout demo, from our own C reimplementation of cowsay 3.03.
# Its output is byte-identical to the original Perl script, and the binary is a tenth of the Rust clone the Wasmer registry serves.

source "$(dirname "${BASH_SOURCE[0]}")/common.sh"

fetch_app cowsay \
  "https://github.com/dewasm/cowsay.wasm/releases/download/v0.1.0/cowsay.wasm" \
  e7c54f5959605d509344b6793bbaaab4cfa6b240fbcbf266c6dc83597b10411d
