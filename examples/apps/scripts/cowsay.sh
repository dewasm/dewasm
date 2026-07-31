#!/usr/bin/env bash
# shellcheck source-path=SCRIPTDIR
# shellcheck source=common.sh

# cowsay: the classic args+stdout demo, from the Wasmer registry.

source "$(dirname "${BASH_SOURCE[0]}")/common.sh"

fetch_app cowsay \
  "https://cdn.wasmer.io/packages/syrusakbary/cowsay/cowsay-0.3.0-b185348b-2e15-480b-96ac-216064a85e0d.tar.gz" \
  44c990f3ceec797d6e90f54e2ba72789b9544be61ee4011aa7ac6c05252ca605 \
  target/wasm32-wasi/release/cowsay.wasm
