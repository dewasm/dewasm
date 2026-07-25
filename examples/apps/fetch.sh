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
    curl -fsSL -o "$tmp/app.wasm" "$url"
    echo "$sha256  $tmp/app.wasm" | shasum -a 256 -c - >/dev/null
    cp "$tmp/app.wasm" "$out"
  else
    curl -fsSL -o "$tmp/pkg.tar.gz" "$url"
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
SQLITE_EXPORTS=(
  sqlite3_libversion sqlite3_open sqlite3_close
  sqlite3_prepare_v2 sqlite3_step sqlite3_finalize
  sqlite3_column_count sqlite3_column_text sqlite3_column_type
  sqlite3_exec sqlite3_errmsg sqlite3_malloc sqlite3_free
)

sqlite_stamp="cache/sqlite3.src-sha256"
if [ -f cache/sqlite3-shell.wasm ] && [ -f cache/libsqlite3.wasm ] \
  && [ -f cache/sqlite3-binding.wasm ] \
  && [ "$(cat "$sqlite_stamp" 2>/dev/null || true)" = "$SQLITE_SHA256" ]; then
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
  curl -fsSL -o "$tmp/sqlite.zip" "$SQLITE_URL"
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
  BINDING_EXPORTS=(
    run_query
    sqlite3_open sqlite3_close sqlite3_exec sqlite3_errmsg
    sqlite3_malloc sqlite3_free
  )
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
  printf '%s\n' "$SQLITE_SHA256" >"$sqlite_stamp"
  echo "sqlite3: -> cache/sqlite3-shell.wasm, cache/libsqlite3.wasm, cache/sqlite3-binding.wasm"
fi
