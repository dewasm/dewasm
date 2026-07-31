#!/usr/bin/env bash
# CRuby 3.4 (ruby.wasm 2.9.4): the official ruby.wasm wasm32-wasip1 full
# build. Beyond ruby.wasm we extract the stdlib tree (usr/local/lib/ruby) the
# interpreter reads at startup; the multi-hundred-MB libruby-static.a and the
# rest of the tree are not needed at run time and are left out. The e2e case
# preopens cache/ruby-lib/usr at guest /usr. Ruby-only, heavy — execution
# behind the `heavy_test` cargo feature. The "Ruby on Ruby" north-star demo
# (docs/apps-audit.md).
# shellcheck source-path=SCRIPTDIR
# shellcheck source=common.sh
source "$(dirname "${BASH_SOURCE[0]}")/common.sh"

CRUBY_URL="https://github.com/ruby/ruby.wasm/releases/download/2.9.4/ruby-3.4-wasm32-unknown-wasip1-full.tar.gz"
CRUBY_SHA256="ccda86a375a4fe09849846d3b03a370172a4902a0c571087f48457388a2762c7"
CRUBY_DIR="ruby-3.4-wasm32-unknown-wasip1-full"

cruby_stamp="cache/ruby.src-sha256"
if is_cached "$cruby_stamp" "$CRUBY_SHA256" \
  cache/ruby.wasm cache/ruby-lib/usr/local/lib/ruby; then
  echo "ruby: cached"
  exit 0
fi

echo "ruby: fetching $CRUBY_URL"
new_tmpdir
fetch_verified "$CRUBY_URL" "$CRUBY_SHA256" "$tmp/ruby.tar.gz"
tar xzf "$tmp/ruby.tar.gz" -C "$tmp" "$CRUBY_DIR/usr/local/bin/ruby"
cp "$tmp/$CRUBY_DIR/usr/local/bin/ruby" cache/ruby.wasm
rm -rf cache/ruby-lib
mkdir -p cache/ruby-lib
tar xzf "$tmp/ruby.tar.gz" -C cache/ruby-lib --strip-components=1 \
  "$CRUBY_DIR/usr/local/lib/ruby"
write_stamp "$cruby_stamp" "$CRUBY_SHA256"
echo "ruby: -> cache/ruby.wasm, cache/ruby-lib/usr/local/lib/ruby"
