#!/usr/bin/env bash
# minigzip: zlib's stdio (de)compression demo, built from the pinned zlib
# source release with zig (ADR-22). Integer-only and tiny, with binary
# stdin/stdout — the byte-exact-stdio stress that runs under BOTH backends.
# No upstream distributes a wasm32-wasi minigzip, so it is compiled locally.
# The gz stream zlib writes here is fully deterministic (mtime 0, OS byte 3),
# so wasmtime's output and the converted backends' output are byte-identical.
# shellcheck source-path=SCRIPTDIR
# shellcheck source=common.sh
source "$(dirname "${BASH_SOURCE[0]}")/common.sh"

ZLIB_URL="https://github.com/madler/zlib/releases/download/v1.3.1/zlib-1.3.1.tar.gz"
ZLIB_SHA256="9a93b2b7dfdac77ceba5a558a580e74667dd6fede4585b91eefb60f03b72df23"
ZLIB_DIR="zlib-1.3.1"
# The zlib translation units minigzip.c needs. Z_HAVE_UNISTD_H makes the
# shipped zconf.h include <unistd.h> so lseek is declared (wasi-libc has it;
# without the define, clang errors on the implicit declaration).
ZLIB_SRCS=(
  adler32.c compress.c crc32.c deflate.c gzclose.c gzlib.c gzread.c gzwrite.c
  infback.c inffast.c inflate.c inftrees.c trees.c uncompr.c zutil.c
)

minigzip_stamp="cache/minigzip.src-sha256"
if is_cached "$minigzip_stamp" "$ZLIB_SHA256" cache/minigzip.wasm; then
  echo "minigzip: cached"
  exit 0
fi

require_tool minigzip zig "install zig (e.g. brew install zig) to build the minigzip app"
echo "minigzip: fetching $ZLIB_URL"
new_tmpdir
fetch_verified "$ZLIB_URL" "$ZLIB_SHA256" "$tmp/zlib.tar.gz"
tar xzf "$tmp/zlib.tar.gz" -C "$tmp"
echo "minigzip: building minigzip.wasm (zig cc)"
srcs=()
for s in "${ZLIB_SRCS[@]}"; do srcs+=("$tmp/$ZLIB_DIR/$s"); done
zig cc -target wasm32-wasi -O2 -DZ_HAVE_UNISTD_H -I "$tmp/$ZLIB_DIR" \
  "${srcs[@]}" "$tmp/$ZLIB_DIR/test/minigzip.c" \
  -o cache/minigzip.wasm
write_stamp "$minigzip_stamp" "$ZLIB_SHA256"
echo "minigzip: -> cache/minigzip.wasm"
