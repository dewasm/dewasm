#!/usr/bin/env bash
# shellcheck source-path=SCRIPTDIR
# shellcheck source=common.sh

# `cowsay`: the classic example of arguments and `stdout`, our C reimplementation of `cowsay` 3.8.4.
# It prints what the Perl script of that version prints.
# The binary is 38 kB, against the 700 kB of the Rust clone the Wasmer registry serves.

source "$(dirname "${BASH_SOURCE[0]}")/common.sh"

fetch_app cowsay \
  "https://github.com/dewasm/cowsay.wasm/releases/download/v0.3.0/cowsay.wasm" \
  984e2db339d94079e07344917d6e68fdd1826d5ebde343fe91be07d10c8f5401
