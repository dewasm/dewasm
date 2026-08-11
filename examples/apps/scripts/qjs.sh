#!/usr/bin/env bash
# shellcheck source-path=SCRIPTDIR shellcheck source=common.sh

# qjs: the quickjs-ng JavaScript engine, official WASI CLI release asset.

source "$(dirname "${BASH_SOURCE[0]}")/common.sh"

fetch_app qjs \
  "https://github.com/quickjs-ng/quickjs/releases/download/v0.15.1/qjs-wasi.wasm" \
  b4071ef2fbb2bb693c0bbcfc07cb9d28639fd9cea2fd986824a57aeac929817b
