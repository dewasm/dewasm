#!/usr/bin/env bash
# shellcheck source-path=SCRIPTDIR
# shellcheck source=common.sh

# CRuby 3.4 (ruby.wasm 2.9.4): the official ruby.wasm wasm32-wasip1 full
# build. Beyond ruby.wasm we extract the stdlib tree (usr/local/lib/ruby) the
# interpreter reads at startup; the multi-hundred-MB libruby-static.a and the
# rest of the tree are not needed at run time and are left out (the extract
# helpers unpack only the two named members). The e2e case preopens
# cache/ruby-lib/usr at guest /usr. Ruby-only, heavy — execution behind the
# `heavy_test` cargo feature. The "Ruby on Ruby" north-star demo
# (docs/apps-audit.md).

source "$(dirname "${BASH_SOURCE[0]}")/common.sh"

CRUBY_DIR="ruby-3.4-wasm32-unknown-wasip1-full"
fetch_runtime_with_stdlib ruby \
  "https://github.com/ruby/ruby.wasm/releases/download/2.9.4/ruby-3.4-wasm32-unknown-wasip1-full.tar.gz" \
  ccda86a375a4fe09849846d3b03a370172a4902a0c571087f48457388a2762c7 \
  "$CRUBY_DIR/usr/local/bin/ruby" "$CRUBY_DIR/usr/local/lib/ruby" 1
