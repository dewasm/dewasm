#!/usr/bin/env bash
# Fetch real-world example apps as wasm binaries — either directly from an
# upstream release, or (sqlite3) by building a version-pinned upstream
# source release locally with zig (ADR-22).
#
# Third-party artifacts are never committed to this repository (ADR-9):
# this script downloads version-pinned, checksum-verified files into
# examples/apps/cache/ (gitignored). The apps e2e test fails loudly when
# the cache is absent (ADR-15).
set -euo pipefail

cd "$(dirname "$0")"
mkdir -p cache

# Every download goes through this helper: the pinned hosts (GitHub,
# sqlite.org, tcpdump.org, wasmer.io) return transient 5xx often enough to
# kill a cold CI run otherwise (issue #17); curl retries 408/429/5xx and
# refused connections.
fetch_url() {
  curl -fsSL --retry 5 --retry-delay 2 --retry-connrefused -o "$2" "$1"
}

# --- wasm-opt -O2 preprocessing (ADR-39) for the locally-built modules.
# Only modules we build here (the two reactor C libs and ripgrep) are
# post-processed: it shrinks them and normalizes the overlong call_indirect
# immediates the LLVM toolchain emits (so the converter sees pure baseline
# wasm). Baseline features only — never SIMD/atomics/EH — and no
# wasm-ctor-eval. The stamp for each such module includes `wasm-opt --version`
# so a wasm-opt upgrade re-triggers the build.
WASM_OPT_FEATURES=(
  --enable-bulk-memory --enable-sign-ext --enable-nontrapping-float-to-int
  --enable-mutable-globals --enable-multivalue --enable-reference-types
)
wasm_opt_inplace() {
  wasm-opt "${WASM_OPT_FEATURES[@]}" -O2 "$1" -o "$1"
}
# The version string folded into a locally-built module's stamp (empty when
# wasm-opt is absent, so the cache misses and the loud prereq check fires).
wasm_opt_version() { wasm-opt --version 2>/dev/null || true; }

# name | URL | sha256 | path inside the tarball (empty = URL is the .wasm itself)
APPS=(
  "cowsay|https://cdn.wasmer.io/packages/syrusakbary/cowsay/cowsay-0.3.0-b185348b-2e15-480b-96ac-216064a85e0d.tar.gz|44c990f3ceec797d6e90f54e2ba72789b9544be61ee4011aa7ac6c05252ca605|target/wasm32-wasi/release/cowsay.wasm"
  "qjs|https://github.com/quickjs-ng/quickjs/releases/download/v0.15.1/qjs-wasi.wasm|b4071ef2fbb2bb693c0bbcfc07cb9d28639fd9cea2fd986824a57aeac929817b|"
)

for app in "${APPS[@]}"; do
  IFS="|" read -r name url sha256 wasm_path <<<"$app"
  out="cache/$name.wasm"
  # The stamp records which pinned source checksum the cached copy came
  # from; a re-pinned app is refetched instead of silently kept stale
  # (which would fail the golden-output comparison inscrutably).
  stamp="cache/$name.src-sha256"
  if [ -f "$out" ] && [ "$(cat "$stamp" 2>/dev/null || true)" = "$sha256" ]; then
    echo "$name: cached"
    continue
  fi
  if [ -f "$out" ]; then
    echo "$name: cached copy predates the current pin — refetching"
  fi
  echo "$name: fetching $url"
  tmp=$(mktemp -d)
  trap 'rm -rf "$tmp"' EXIT
  if [ -z "$wasm_path" ]; then
    # URL is a standalone .wasm asset (e.g. a GitHub release) — no archive.
    fetch_url "$url" "$tmp/app.wasm"
    echo "$sha256  $tmp/app.wasm" | shasum -a 256 -c - >/dev/null
    cp "$tmp/app.wasm" "$out"
  else
    fetch_url "$url" "$tmp/pkg.tar.gz"
    echo "$sha256  $tmp/pkg.tar.gz" | shasum -a 256 -c - >/dev/null
    tar xzf "$tmp/pkg.tar.gz" -C "$tmp"
    cp "$tmp/$wasm_path" "$out"
  fi
  rm -rf "$tmp"
  trap - EXIT
  printf '%s\n' "$sha256" >"$stamp"
  echo "$name: -> $out"
done

# --- sqlite3: built from the pinned amalgamation source with zig (ADR-22).
#
# One pinned source release yields three artifacts:
#   cache/sqlite3-shell.wasm   — the CLI shell (standalone: _start, stdio)
#   cache/libsqlite3.wasm      — a reactor library exporting the sqlite3 C
#                                API, driven from Ruby in the apps e2e
#   cache/sqlite3-binding.wasm — the same reactor library plus our own
#                                src/sqlite3_binding.c (run_query), which
#                                calls back into an imported env.host_row:
#                                a guest->host callback round-trip proof
# No upstream distributes a C-API-exporting wasm32-wasi build, which is
# why these are compiled locally rather than downloaded.
SQLITE_URL="https://sqlite.org/2026/sqlite-amalgamation-3530300.zip"
SQLITE_SHA256="646421e12aac110282ef8cc68f1a62d4bb15fc7b8f09da0b53e29ee690500431"
SQLITE_DIR="sqlite-amalgamation-3530300"
SQLITE_CFLAGS=(
  -O2
  -D_WASI_EMULATED_PROCESS_CLOCKS -lwasi-emulated-process-clocks
  -D_WASI_EMULATED_SIGNAL -lwasi-emulated-signal
  -DSQLITE_NOHAVE_SYSTEM
)
# The full statement/bind/column surface: enough to implement the sqlite3
# gem API that Rails' SQLite3Adapter uses (examples/rails) on top of it.
SQLITE_EXPORTS=(
  sqlite3_libversion sqlite3_libversion_number sqlite3_open sqlite3_open_v2
  sqlite3_close sqlite3_close_v2
  sqlite3_prepare_v2 sqlite3_step sqlite3_reset sqlite3_clear_bindings
  sqlite3_finalize
  sqlite3_column_count sqlite3_column_name sqlite3_column_decltype
  sqlite3_column_type sqlite3_column_text sqlite3_column_blob
  sqlite3_column_bytes sqlite3_column_int64 sqlite3_column_double
  sqlite3_bind_parameter_count sqlite3_bind_parameter_index
  sqlite3_bind_int64 sqlite3_bind_double sqlite3_bind_text sqlite3_bind_blob
  sqlite3_bind_null
  sqlite3_exec sqlite3_errmsg sqlite3_errcode sqlite3_extended_errcode
  sqlite3_error_offset
  sqlite3_changes sqlite3_total_changes sqlite3_last_insert_rowid
  sqlite3_get_autocommit sqlite3_busy_timeout sqlite3_complete
  sqlite3_malloc sqlite3_free
)
BINDING_EXPORTS=(
  run_query
  sqlite3_open sqlite3_close sqlite3_exec sqlite3_errmsg
  sqlite3_malloc sqlite3_free
)

# The stamp covers the source sha *and* the export lists, so editing either
# retriggers the build.
sqlite_key="$SQLITE_SHA256 exports:${SQLITE_EXPORTS[*]} binding:${BINDING_EXPORTS[*]}"
sqlite_stamp="cache/sqlite3.src-sha256"
if [ -f cache/sqlite3-shell.wasm ] && [ -f cache/libsqlite3.wasm ] \
  && [ -f cache/sqlite3-binding.wasm ] \
  && [ "$(cat "$sqlite_stamp" 2>/dev/null || true)" = "$sqlite_key" ]; then
  echo "sqlite3: cached"
else
  command -v zig >/dev/null || {
    echo "sqlite3: zig not found — install zig (e.g. brew install zig) to build the sqlite3 apps" >&2
    exit 1
  }
  command -v unzip >/dev/null || {
    echo "sqlite3: unzip not found" >&2
    exit 1
  }
  echo "sqlite3: fetching $SQLITE_URL"
  tmp=$(mktemp -d)
  trap 'rm -rf "$tmp"' EXIT
  fetch_url "$SQLITE_URL" "$tmp/sqlite.zip"
  echo "$SQLITE_SHA256  $tmp/sqlite.zip" | shasum -a 256 -c - >/dev/null
  unzip -q "$tmp/sqlite.zip" -d "$tmp"
  echo "sqlite3: building sqlite3-shell.wasm (zig cc)"
  zig cc -target wasm32-wasi "${SQLITE_CFLAGS[@]}" \
    "$tmp/$SQLITE_DIR/sqlite3.c" "$tmp/$SQLITE_DIR/shell.c" \
    -o cache/sqlite3-shell.wasm
  echo "sqlite3: building libsqlite3.wasm (zig cc, reactor)"
  exports=()
  for e in "${SQLITE_EXPORTS[@]}"; do exports+=("-Wl,--export=$e"); done
  zig cc -target wasm32-wasi -mexec-model=reactor "${SQLITE_CFLAGS[@]}" \
    -DSQLITE_OMIT_LOAD_EXTENSION \
    "$tmp/$SQLITE_DIR/sqlite3.c" \
    "${exports[@]}" \
    -o cache/libsqlite3.wasm
  # The binding artifact: the reactor library plus our own run_query, which
  # forwards each result row to the imported env.host_row (ADR-22). Only the
  # symbols this callback flow needs are exported; the import lands via the
  # import_module/import_name attributes in src/sqlite3_binding.c.
  echo "sqlite3: building sqlite3-binding.wasm (zig cc, reactor + host callback)"
  binding_exports=()
  for e in "${BINDING_EXPORTS[@]}"; do binding_exports+=("-Wl,--export=$e"); done
  zig cc -target wasm32-wasi -mexec-model=reactor "${SQLITE_CFLAGS[@]}" \
    -DSQLITE_OMIT_LOAD_EXTENSION \
    -I "$tmp/$SQLITE_DIR" \
    "$tmp/$SQLITE_DIR/sqlite3.c" src/sqlite3_binding.c \
    "${binding_exports[@]}" \
    -o cache/sqlite3-binding.wasm
  rm -rf "$tmp"
  trap - EXIT
  printf '%s\n' "$sqlite_key" >"$sqlite_stamp"
  echo "sqlite3: -> cache/sqlite3-shell.wasm, cache/libsqlite3.wasm, cache/sqlite3-binding.wasm"
fi

# --- libpcap: BPF filter compiler, built from the pinned upstream source
# release with zig (ADR-22) as a reactor library. Only the platform-independent
# filter-compilation TUs are built (no capture backend); src/pcap_config.h
# stands in for ./configure's config.h (see its header comment), and our own
# src/pcap_binding.c exports compile_filter(), which turns a textual filter
# like "tcp port 80" into a serialized BPF program in guest memory. libpcap
# 1.10.x no longer ships pre-generated grammar.c/scanner.c, so the parser is
# regenerated here with bison + flex (matching the substitution ./configure
# would apply for a bison >= 3 reentrant parser).
PCAP_URL="https://www.tcpdump.org/release/libpcap-1.10.6.tar.gz"
PCAP_SHA256="872dd11337fe1ab02ad9d4fee047c9da244d695c6ddf34e2ebb733efd4ed8aa9"
PCAP_DIR="libpcap-1.10.6"
# The filter-compiler TUs plus the libpcap internals the linker demands
# (fmtutils/etherent/strlcpy) and the generated parser (grammar/scanner).
PCAP_SRCS=(
  pcap.c gencode.c optimize.c nametoaddr.c bpf_image.c bpf_filter.c
  grammar.c scanner.c pcap-common.c fmtutils.c etherent.c missing/strlcpy.c
)

pcap_stamp="cache/libpcap.src-sha256"
pcap_want="$(printf '%s\n%s' "$PCAP_SHA256" "$(wasm_opt_version)")"
if [ -f cache/libpcap.wasm ] \
  && [ "$(cat "$pcap_stamp" 2>/dev/null || true)" = "$pcap_want" ]; then
  echo "libpcap: cached"
else
  command -v zig >/dev/null || {
    echo "libpcap: zig not found — install zig (e.g. brew install zig) to build the libpcap app" >&2
    exit 1
  }
  command -v wasm-opt >/dev/null || {
    echo "libpcap: wasm-opt not found — install binaryen (e.g. brew install binaryen) to preprocess the libpcap app (ADR-39)" >&2
    exit 1
  }
  command -v bison >/dev/null || {
    echo "libpcap: bison not found — install bison (e.g. brew install bison) to regenerate the filter grammar" >&2
    exit 1
  }
  command -v flex >/dev/null || {
    echo "libpcap: flex not found — install flex (e.g. brew install flex) to regenerate the filter scanner" >&2
    exit 1
  }
  echo "libpcap: fetching $PCAP_URL"
  tmp=$(mktemp -d)
  trap 'rm -rf "$tmp"' EXIT
  fetch_url "$PCAP_URL" "$tmp/libpcap.tar.gz"
  echo "$PCAP_SHA256  $tmp/libpcap.tar.gz" | shasum -a 256 -c - >/dev/null
  tar xzf "$tmp/libpcap.tar.gz" -C "$tmp"
  pdir="$tmp/$PCAP_DIR"
  echo "libpcap: generating grammar.c/scanner.c (bison/flex)"
  sed 's/@REENTRANT_PARSER@/%define api.pure/' "$pdir/grammar.y.in" >"$pdir/grammar.y"
  ( cd "$pdir" && bison -p pcap_ -o grammar.c -d grammar.y \
    && flex -P pcap_ --header-file=scanner.h --nounput -o scanner.c scanner.l )
  cp src/pcap_config.h "$pdir/config.h"
  echo "libpcap: building libpcap.wasm (zig cc, reactor)"
  psrcs=()
  for s in "${PCAP_SRCS[@]}"; do psrcs+=("$pdir/$s"); done
  # pcap_compile_nopcap() is the documented filter-only entry point but is
  # marked deprecated (thread-safety of its error buffer); silence that here.
  # --strip-debug drops the DWARF wasm-opt cannot process.
  zig cc -target wasm32-wasi -mexec-model=reactor -O2 \
    -DBUILDING_PCAP -D_WASI_EMULATED_SIGNAL -lwasi-emulated-signal \
    -Wno-deprecated-declarations -Wl,--strip-debug -I "$pdir" \
    "${psrcs[@]}" src/pcap_binding.c \
    -Wl,--export=compile_filter -Wl,--export=malloc -Wl,--export=free \
    -o cache/libpcap.wasm
  echo "libpcap: wasm-opt -O2 (ADR-39)"
  wasm_opt_inplace cache/libpcap.wasm
  rm -rf "$tmp"
  trap - EXIT
  printf '%s' "$pcap_want" >"$pcap_stamp"
  echo "libpcap: -> cache/libpcap.wasm"
fi

# --- tree-sitter: the incremental-parsing runtime plus the tree-sitter-json
# grammar, built from the pinned upstream releases with zig (ADR-22) as a
# reactor library. The runtime is a single-TU amalgamation (lib/src/lib.c);
# tree-sitter-json ships a pre-generated src/parser.c (no grammar codegen). Our
# own src/treesitter_binding.c exports parse_source(), which parses a source
# string and returns the parse tree's S-expression (ts_node_string). One
# combined stamp covers both source checksums.
TS_URL="https://github.com/tree-sitter/tree-sitter/archive/refs/tags/v0.26.11.tar.gz"
TS_SHA256="1bab01ed21464f3272665b9c60e39ee79f68da1333e80b23f2c9356569d06971"
TS_DIR="tree-sitter-0.26.11"
TSJSON_URL="https://github.com/tree-sitter/tree-sitter-json/archive/refs/tags/v0.24.8.tar.gz"
TSJSON_SHA256="acf6e8362457e819ed8b613f2ad9a0e1b621a77556c296f3abea58f7880a9213"
TSJSON_DIR="tree-sitter-json-0.24.8"

# One stamp covering both pinned checksums (order-fixed) plus wasm-opt version.
ts_stamp="cache/treesitter.src-sha256"
ts_want="$(printf '%s %s\n%s' "$TS_SHA256" "$TSJSON_SHA256" "$(wasm_opt_version)")"
if [ -f cache/treesitter.wasm ] \
  && [ "$(cat "$ts_stamp" 2>/dev/null || true)" = "$ts_want" ]; then
  echo "treesitter: cached"
else
  command -v zig >/dev/null || {
    echo "treesitter: zig not found — install zig (e.g. brew install zig) to build the tree-sitter app" >&2
    exit 1
  }
  command -v wasm-opt >/dev/null || {
    echo "treesitter: wasm-opt not found — install binaryen (e.g. brew install binaryen) to preprocess the tree-sitter app (ADR-39)" >&2
    exit 1
  }
  echo "treesitter: fetching $TS_URL"
  tmp=$(mktemp -d)
  trap 'rm -rf "$tmp"' EXIT
  fetch_url "$TS_URL" "$tmp/ts.tar.gz"
  echo "$TS_SHA256  $tmp/ts.tar.gz" | shasum -a 256 -c - >/dev/null
  echo "treesitter: fetching $TSJSON_URL"
  fetch_url "$TSJSON_URL" "$tmp/tsjson.tar.gz"
  echo "$TSJSON_SHA256  $tmp/tsjson.tar.gz" | shasum -a 256 -c - >/dev/null
  tar xzf "$tmp/ts.tar.gz" -C "$tmp"
  tar xzf "$tmp/tsjson.tar.gz" -C "$tmp"
  echo "treesitter: building treesitter.wasm (zig cc, reactor)"
  # --strip-debug drops the DWARF wasm-opt cannot process.
  zig cc -target wasm32-wasi -mexec-model=reactor -O2 -Wl,--strip-debug \
    -I "$tmp/$TS_DIR/lib/include" -I "$tmp/$TS_DIR/lib/src" \
    -I "$tmp/$TSJSON_DIR/src" \
    "$tmp/$TS_DIR/lib/src/lib.c" "$tmp/$TSJSON_DIR/src/parser.c" src/treesitter_binding.c \
    -Wl,--export=parse_source -Wl,--export=malloc -Wl,--export=free \
    -o cache/treesitter.wasm
  echo "treesitter: wasm-opt -O2 (ADR-39)"
  wasm_opt_inplace cache/treesitter.wasm
  rm -rf "$tmp"
  trap - EXIT
  printf '%s' "$ts_want" >"$ts_stamp"
  echo "treesitter: -> cache/treesitter.wasm"
fi

# --- minigzip: zlib's stdio (de)compression demo, built from the pinned
# zlib source release with zig (ADR-22). Integer-only and tiny, with binary
# stdin/stdout — the byte-exact-stdio stress that runs under BOTH backends.
# No upstream distributes a wasm32-wasi minigzip, so it is compiled locally.
# The gz stream zlib writes here is fully deterministic (mtime 0, OS byte 3),
# so wasmtime's output and the converted backends' output are byte-identical.
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
if [ -f cache/minigzip.wasm ] \
  && [ "$(cat "$minigzip_stamp" 2>/dev/null || true)" = "$ZLIB_SHA256" ]; then
  echo "minigzip: cached"
else
  command -v zig >/dev/null || {
    echo "minigzip: zig not found — install zig (e.g. brew install zig) to build the minigzip app" >&2
    exit 1
  }
  echo "minigzip: fetching $ZLIB_URL"
  tmp=$(mktemp -d)
  trap 'rm -rf "$tmp"' EXIT
  fetch_url "$ZLIB_URL" "$tmp/zlib.tar.gz"
  echo "$ZLIB_SHA256  $tmp/zlib.tar.gz" | shasum -a 256 -c - >/dev/null
  tar xzf "$tmp/zlib.tar.gz" -C "$tmp"
  echo "minigzip: building minigzip.wasm (zig cc)"
  srcs=()
  for s in "${ZLIB_SRCS[@]}"; do srcs+=("$tmp/$ZLIB_DIR/$s"); done
  zig cc -target wasm32-wasi -O2 -DZ_HAVE_UNISTD_H -I "$tmp/$ZLIB_DIR" \
    "${srcs[@]}" "$tmp/$ZLIB_DIR/test/minigzip.c" \
    -o cache/minigzip.wasm
  rm -rf "$tmp"
  trap - EXIT
  printf '%s\n' "$ZLIB_SHA256" >"$minigzip_stamp"
  echo "minigzip: -> cache/minigzip.wasm"
fi

# --- dwarf-fixture: a first-party C fixture built WITH DWARF (`-g`) so the
# --dwarf-line source back-mapping test has a module carrying `.debug_line`
# (ADR-38). The source is committed (src/dwarf_fixture.c, first-party — ADR-9);
# unlike the other apps there is nothing to download, so the "pin" is the
# sha256 of that source file: editing it rebuilds. Built at -O1 (not -O0) so the
# fixture exercises the folded-expression marker path the core test calibrates.
DWARF_FIXTURE_SRC="src/dwarf_fixture.c"
DWARF_FIXTURE_SHA256="$(shasum -a 256 "$DWARF_FIXTURE_SRC" | cut -d' ' -f1)"

dwarf_stamp="cache/dwarf-fixture.src-sha256"
if [ -f cache/dwarf-fixture.wasm ] \
  && [ "$(cat "$dwarf_stamp" 2>/dev/null || true)" = "$DWARF_FIXTURE_SHA256" ]; then
  echo "dwarf-fixture: cached"
else
  command -v zig >/dev/null || {
    echo "dwarf-fixture: zig not found — install zig (e.g. brew install zig) to build the DWARF fixture" >&2
    exit 1
  }
  echo "dwarf-fixture: building dwarf-fixture.wasm (zig cc -g -O1)"
  zig cc -target wasm32-wasi -g -O1 -o cache/dwarf-fixture.wasm "$DWARF_FIXTURE_SRC"
  printf '%s\n' "$DWARF_FIXTURE_SHA256" >"$dwarf_stamp"
  echo "dwarf-fixture: -> cache/dwarf-fixture.wasm"
fi

# --- ripgrep: built from the pinned source release with cargo for
# wasm32-wasip1. Default features (which already exclude pcre2); no tweaks
# needed — ripgrep 14.1.1 builds clean for wasip1 as-is. A Ruby-only
# filesystem demo (recursive directory search over a preopened tree).
RG_URL="https://github.com/BurntSushi/ripgrep/archive/refs/tags/14.1.1.tar.gz"
RG_SHA256="4dad02a2f9c8c3c8d89434e47337aa654cb0e2aa50e806589132f186bf5c2b66"
RG_DIR="ripgrep-14.1.1"

rg_stamp="cache/rg.src-sha256"
rg_want="$(printf '%s\n%s' "$RG_SHA256" "$(wasm_opt_version)")"
if [ -f cache/rg.wasm ] && [ "$(cat "$rg_stamp" 2>/dev/null || true)" = "$rg_want" ]; then
  echo "rg: cached"
else
  command -v cargo >/dev/null || {
    echo "rg: cargo not found — install the Rust toolchain to build ripgrep" >&2
    exit 1
  }
  command -v wasm-opt >/dev/null || {
    echo "rg: wasm-opt not found — install binaryen (e.g. brew install binaryen) to preprocess ripgrep (ADR-39)" >&2
    exit 1
  }
  rustup target list --installed 2>/dev/null | grep -qx wasm32-wasip1 || {
    echo "rg: wasm32-wasip1 target not installed — run: rustup target add wasm32-wasip1" >&2
    exit 1
  }
  echo "rg: fetching $RG_URL"
  tmp=$(mktemp -d)
  trap 'rm -rf "$tmp"' EXIT
  fetch_url "$RG_URL" "$tmp/rg.tar.gz"
  echo "$RG_SHA256  $tmp/rg.tar.gz" | shasum -a 256 -c - >/dev/null
  tar xzf "$tmp/rg.tar.gz" -C "$tmp"
  echo "rg: building rg.wasm (cargo build --release --target wasm32-wasip1)"
  ( cd "$tmp/$RG_DIR" && cargo build --release --target wasm32-wasip1 )
  cp "$tmp/$RG_DIR/target/wasm32-wasip1/release/rg.wasm" cache/rg.wasm
  echo "rg: wasm-opt -O2 (ADR-39)"
  wasm_opt_inplace cache/rg.wasm
  rm -rf "$tmp"
  trap - EXIT
  printf '%s' "$rg_want" >"$rg_stamp"
  echo "rg: -> cache/rg.wasm"
fi

# --- CPython 3.14.6: promoted from fetch-candidates.sh (Phase 5b). The
# official wasm32-wasip1 build (brettcannon/cpython-wasi-build). Beyond
# python.wasm we also extract the stdlib tree (lib/python3.14) the interpreter
# reads at startup from a preopened directory: the e2e case preopens
# cache/cpython-lib/lib at guest /lib (PYTHONHOME=/, PYTHONPATH=/lib/python3.14).
# Ruby-only, heavy — execution behind the `heavy_test` cargo feature
# (docs/apps-audit.md).
CPYTHON_URL="https://github.com/brettcannon/cpython-wasi-build/releases/download/v3.14.6/python-3.14.6-wasi_sdk-24.zip"
CPYTHON_SHA256="73bf2e9774c4d8820d0877ec5db0b963df3a9611fc2a63838aeaee29dfd034e6"

cpython_stamp="cache/cpython.src-sha256"
if [ -f cache/cpython.wasm ] && [ -d cache/cpython-lib/lib/python3.14 ] \
  && [ "$(cat "$cpython_stamp" 2>/dev/null || true)" = "$CPYTHON_SHA256" ]; then
  echo "cpython: cached"
else
  command -v unzip >/dev/null || { echo "cpython: unzip not found" >&2; exit 1; }
  echo "cpython: fetching $CPYTHON_URL"
  tmp=$(mktemp -d)
  trap 'rm -rf "$tmp"' EXIT
  fetch_url "$CPYTHON_URL" "$tmp/py.zip"
  echo "$CPYTHON_SHA256  $tmp/py.zip" | shasum -a 256 -c - >/dev/null
  unzip -qo "$tmp/py.zip" python.wasm -d "$tmp"
  cp "$tmp/python.wasm" cache/cpython.wasm
  rm -rf cache/cpython-lib
  mkdir -p cache/cpython-lib
  unzip -qo "$tmp/py.zip" 'lib/python3.14/*' -d cache/cpython-lib
  rm -rf "$tmp"
  trap - EXIT
  printf '%s\n' "$CPYTHON_SHA256" >"$cpython_stamp"
  echo "cpython: -> cache/cpython.wasm, cache/cpython-lib/lib/python3.14"
fi

# --- CRuby 3.4 (ruby.wasm 2.9.4): promoted from fetch-candidates.sh (Phase
# 5b). The official ruby.wasm wasm32-wasip1 full build. Beyond ruby.wasm we
# extract the stdlib tree (usr/local/lib/ruby) the interpreter reads at
# startup; the multi-hundred-MB libruby-static.a and the rest of the tree are
# not needed at run time and are left out. The e2e case preopens
# cache/ruby-lib/usr at guest /usr. Ruby-only, heavy — execution behind the
# `heavy_test` cargo feature. The "Ruby on Ruby" north-star demo
# (docs/apps-audit.md).
CRUBY_URL="https://github.com/ruby/ruby.wasm/releases/download/2.9.4/ruby-3.4-wasm32-unknown-wasip1-full.tar.gz"
CRUBY_SHA256="ccda86a375a4fe09849846d3b03a370172a4902a0c571087f48457388a2762c7"
CRUBY_DIR="ruby-3.4-wasm32-unknown-wasip1-full"

cruby_stamp="cache/ruby.src-sha256"
if [ -f cache/ruby.wasm ] && [ -d cache/ruby-lib/usr/local/lib/ruby ] \
  && [ "$(cat "$cruby_stamp" 2>/dev/null || true)" = "$CRUBY_SHA256" ]; then
  echo "ruby: cached"
else
  echo "ruby: fetching $CRUBY_URL"
  tmp=$(mktemp -d)
  trap 'rm -rf "$tmp"' EXIT
  fetch_url "$CRUBY_URL" "$tmp/ruby.tar.gz"
  echo "$CRUBY_SHA256  $tmp/ruby.tar.gz" | shasum -a 256 -c - >/dev/null
  tar xzf "$tmp/ruby.tar.gz" -C "$tmp" "$CRUBY_DIR/usr/local/bin/ruby"
  cp "$tmp/$CRUBY_DIR/usr/local/bin/ruby" cache/ruby.wasm
  rm -rf cache/ruby-lib
  mkdir -p cache/ruby-lib
  tar xzf "$tmp/ruby.tar.gz" -C cache/ruby-lib --strip-components=1 \
    "$CRUBY_DIR/usr/local/lib/ruby"
  rm -rf "$tmp"
  trap - EXIT
  printf '%s\n' "$CRUBY_SHA256" >"$cruby_stamp"
  echo "ruby: -> cache/ruby.wasm, cache/ruby-lib/usr/local/lib/ruby"
fi
