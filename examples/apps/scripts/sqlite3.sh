#!/usr/bin/env bash
# shellcheck source-path=SCRIPTDIR
# shellcheck source=common.sh

# sqlite3: built from the pinned amalgamation source with zig.
#
# One pinned source release yields three artifacts:
# cache/sqlite3-shell.wasm:   the CLI shell (standalone: _start, stdio)
# cache/libsqlite3.wasm:      a reactor library exporting the sqlite3 C
# API, driven from Ruby in the apps e2e
# cache/sqlite3-binding.wasm: the same reactor library plus our own
# src/sqlite3_binding.c (run_query), which
# calls back into an imported env.host_row:
# a guest->host callback round-trip proof
# No upstream distributes a C-API-exporting wasm32-wasi build, which is why these are compiled locally rather than downloaded.

source "$(dirname "${BASH_SOURCE[0]}")/common.sh"

SQLITE_URL="https://sqlite.org/2026/sqlite-amalgamation-3530300.zip"
SQLITE_SHA256="646421e12aac110282ef8cc68f1a62d4bb15fc7b8f09da0b53e29ee690500431"
SQLITE_DIR="sqlite-amalgamation-3530300"
SQLITE_CFLAGS=(
  -O2
  -D_WASI_EMULATED_PROCESS_CLOCKS -lwasi-emulated-process-clocks
  -D_WASI_EMULATED_SIGNAL -lwasi-emulated-signal
  -DSQLITE_NOHAVE_SYSTEM
)
# The full statement/bind/column surface: enough to implement the sqlite3 gem API that Rails' SQLite3Adapter uses (examples/rails) on top of it.
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

# The stamp covers the source sha, the export lists, and the wasm-opt version, so editing any of them retriggers the build.
sqlite_key="$SQLITE_SHA256 exports:${SQLITE_EXPORTS[*]} binding:${BINDING_EXPORTS[*]} wasm-opt:$(wasm_opt_version)"
sqlite_stamp="cache/sqlite3.src-sha256"
if is_cached "$sqlite_stamp" "$sqlite_key" \
  cache/sqlite3-shell.wasm cache/libsqlite3.wasm cache/sqlite3-binding.wasm; then
  echo "sqlite3: cached"
  exit 0
fi

require_tool sqlite3 zig "install zig (e.g. brew install zig) to build the sqlite3 apps"
require_tool sqlite3 unzip
require_tool sqlite3 wasm-opt "install binaryen (e.g. brew install binaryen) to preprocess the sqlite3 apps"

echo "sqlite3: fetching $SQLITE_URL"
new_tmpdir
fetch_verified "$SQLITE_URL" "$SQLITE_SHA256" "$tmp/sqlite.zip"
unzip -q "$tmp/sqlite.zip" -d "$tmp"
# --strip-debug (all three builds) drops the DWARF wasm-opt cannot parse.
echo "sqlite3: building sqlite3-shell.wasm (zig cc)"
zig_cc_wasi "${SQLITE_CFLAGS[@]}" -Wl,--strip-debug \
  "$tmp/$SQLITE_DIR/sqlite3.c" "$tmp/$SQLITE_DIR/shell.c" \
  -o cache/sqlite3-shell.wasm

echo "sqlite3: building libsqlite3.wasm (zig cc, reactor)"
mapfile -t exports < <(wl_exports "${SQLITE_EXPORTS[@]}")
zig_cc_wasi -mexec-model=reactor "${SQLITE_CFLAGS[@]}" -Wl,--strip-debug \
  -DSQLITE_OMIT_LOAD_EXTENSION \
  "$tmp/$SQLITE_DIR/sqlite3.c" \
  "${exports[@]}" \
  -o cache/libsqlite3.wasm

# The binding artifact: the reactor library plus our own run_query, which forwards each result row to the imported env.host_row.
# Only the symbols this callback flow needs are exported; the import lands via the import_module/import_name attributes in src/sqlite3_binding.c.
echo "sqlite3: building sqlite3-binding.wasm (zig cc, reactor + host callback)"
mapfile -t binding_exports < <(wl_exports "${BINDING_EXPORTS[@]}")
zig_cc_wasi -mexec-model=reactor "${SQLITE_CFLAGS[@]}" -Wl,--strip-debug \
  -DSQLITE_OMIT_LOAD_EXTENSION \
  -I "$tmp/$SQLITE_DIR" \
  "$tmp/$SQLITE_DIR/sqlite3.c" src/sqlite3_binding.c \
  "${binding_exports[@]}" \
  -o cache/sqlite3-binding.wasm

echo "sqlite3: wasm-opt -O2"
for w in cache/sqlite3-shell.wasm cache/libsqlite3.wasm cache/sqlite3-binding.wasm; do
  wasm_opt_inplace "$w"
done

write_stamp "$sqlite_stamp" "$sqlite_key"
echo "sqlite3: -> cache/sqlite3-shell.wasm, cache/libsqlite3.wasm, cache/sqlite3-binding.wasm"
