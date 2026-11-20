#!/usr/bin/env bash
# shellcheck source-path=SCRIPTDIR
# shellcheck source=common.sh

# `qjs`: the `quickjs-ng` JavaScript engine, official WASI CLI release asset.

source "$(dirname "${BASH_SOURCE[0]}")/common.sh"

fetch_app qjs \
  "https://github.com/quickjs-ng/quickjs/releases/download/v0.18.0/qjs-wasi.wasm" \
  2c218412cb8b17e46ee91bea7556ec38f5b9b6bd6312fddd3ceeed14aa71eaaa
