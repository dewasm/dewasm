#!/usr/bin/env bash
# shellcheck source-path=SCRIPTDIR shellcheck source=common.sh

# CRuby 3.4 (ruby.wasm 2.9.4): the official ruby.wasm wasm32-wasip1 full build.
# Beyond ruby.wasm we extract the stdlib tree (usr/local/lib/ruby) the interpreter reads at startup; the multi-hundred-MB libruby-static.a and the rest of the tree are not needed at run time and are left out (the extract helpers unpack only the two named members).
# The e2e case preopens cache/ruby-lib/usr at guest /usr.
# Ruby-only, heavy: execution behind the
# `heavy_test` cargo feature.
# The "Ruby on Ruby" goal demo
# (docs/apps-audit.md).
#
# A second artifact, cache/ruby-packed.wasm, covers ruby.wasm's intended deployment shape: the same module with the stdlib embedded by
# `wasi-vfs pack` (wizer pre-initialization), self-contained: no preopens.
# The official build links libwasi_vfs.a, which is what makes packing work.

source "$(dirname "${BASH_SOURCE[0]}")/common.sh"

CRUBY_SHA=ccda86a375a4fe09849846d3b03a370172a4902a0c571087f48457388a2762c7
CRUBY_DIR="ruby-3.4-wasm32-unknown-wasip1-full"
fetch_runtime_with_stdlib ruby \
  "https://github.com/ruby/ruby.wasm/releases/download/2.9.4/ruby-3.4-wasm32-unknown-wasip1-full.tar.gz" \
  "$CRUBY_SHA" \
  "$CRUBY_DIR/usr/local/bin/ruby" "$CRUBY_DIR/usr/local/lib/ruby" 1

# ruby-packed: cache/ruby.wasm with cache/ruby-lib/usr embedded at guest /usr.
# The stamp folds the archive sha and `wasi-vfs --version` (same discipline as the wasm-opt stamps): a re-pin or a CLI upgrade re-packs instead of keeping a stale module.
require_tool ruby-packed wasi-vfs \
  "prebuilt CLI on https://github.com/kateinoigakukun/wasi-vfs/releases, or \`cargo install wasi-vfs-cli\`"
packed=cache/ruby-packed.wasm
packed_stamp=cache/ruby-packed.src-sha256
packed_key="$CRUBY_SHA
$(wasi-vfs --version)"
if is_cached "$packed_stamp" "$packed_key" "$packed"; then
  echo "ruby-packed: cached"
else
  echo "ruby-packed: packing cache/ruby.wasm + cache/ruby-lib/usr"
  wasi-vfs pack cache/ruby.wasm --dir cache/ruby-lib/usr::/usr -o "$packed"
  write_stamp "$packed_stamp" "$packed_key"
  echo "ruby-packed: -> $packed"
fi
