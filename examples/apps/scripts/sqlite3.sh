#!/usr/bin/env bash
# shellcheck source-path=SCRIPTDIR
# shellcheck source=common.sh

# sqlite3: built from the pinned amalgamation source with zig.
#
# One pinned source release yields four artifacts:
# cache/sqlite3-shell.wasm:   the CLI shell (standalone: _start, stdio)
# cache/sqlite3-mod.wasm:     the same shell built from a source copy
# carrying src/sqlite3-vdbe-split.patch, which
# moves the hot VDBE opcode bodies into their
# own functions so a JIT reaches them
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

# The mod build's C-source half; the patch's own header states what it changes and why.
SQLITE_SPLIT_PATCH="src/sqlite3-vdbe-split.patch"
# The mod build's wasm-opt half, and the reason it cannot be folded into the shared recipe.
# -O2 inlines a function with a single caller, which puts every vdbeOp* body straight back into the interpreter and leaves an artifact indistinguishable from the stock shell, so the mod build turns that off by name.
# Matching by name needs the name section, so the mod link omits -Wl,--strip-debug and the debug info leaves here instead.
# The two groups stay separate because the check after the build reruns the stripping half alone as its control.
SQLITE_MOD_NO_INLINE="--no-inline=vdbeOp*"
SQLITE_MOD_STRIP=(--strip-dwarf --strip-producers)

# patch_version_suffix <file>: rewrite the single `#define SQLITE_VERSION` line of the unpacked source to "3.53.3-wasm".
# The converted engine must identify itself in demo output (examples/rails answers /stats with sqlite_version()), so it is not mistaken for a native SQLite.
# SQLITE_VERSION_NUMBER and SQLITE_SOURCE_ID stay upstream: only the display string carries the suffix.
# sed -i is not portable across BSD and GNU, hence the temp file; a source whose define does not have the assumed shape aborts the build instead of silently producing an unpatched artifact.
patch_version_suffix() {
  local f="$1"
  sed 's/^#define SQLITE_VERSION  *"3\.53\.3"$/#define SQLITE_VERSION        "3.53.3-wasm"/' "$f" >"$f.patched"
  mv "$f.patched" "$f"
  grep -q '^#define SQLITE_VERSION  *"3\.53\.3-wasm"$' "$f" || {
    echo "sqlite3: the -wasm version suffix did not apply to $f" >&2
    exit 1
  }
  if grep -q '^#define SQLITE_VERSION  *"3\.53\.3"$' "$f"; then
    echo "sqlite3: an unpatched version define remains in $f" >&2
    exit 1
  fi
}

# wasm_func_count <wasm>: the module's function count.
wasm_func_count() {
  wasm-opt "${WASM_OPT_FEATURES[@]}" --metrics "$1" -o /dev/null 2>&1 |
    awk '$1 == "[funcs]" { print $3 }'
}

# The stamp covers the source sha, the export lists, the version-string patch, the split patch's bytes with the wasm-opt flags that keep it effective, and the wasm-opt version, so editing any of them retriggers the build.
sqlite_key="$SQLITE_SHA256 exports:${SQLITE_EXPORTS[*]} binding:${BINDING_EXPORTS[*]} version-suffix:-wasm split:$(shasum -a 256 "$SQLITE_SPLIT_PATCH" | cut -d' ' -f1) mod-wasm-opt:$SQLITE_MOD_NO_INLINE ${SQLITE_MOD_STRIP[*]} wasm-opt:$(wasm_opt_version)"
sqlite_stamp="cache/sqlite3.src-sha256"
if is_cached "$sqlite_stamp" "$sqlite_key" \
  cache/sqlite3-shell.wasm cache/sqlite3-mod.wasm \
  cache/libsqlite3.wasm cache/sqlite3-binding.wasm; then
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
# The amalgamation embeds a copy of the header in sqlite3.c, and shell.c includes sqlite3.h, so both files carry the define.
patch_version_suffix "$tmp/$SQLITE_DIR/sqlite3.c"
patch_version_suffix "$tmp/$SQLITE_DIR/sqlite3.h"
# --strip-debug (the three stock builds) drops the DWARF wasm-opt cannot parse; the mod build strips it in wasm-opt instead, see SQLITE_MOD_STRIP.
echo "sqlite3: building sqlite3-shell.wasm (zig cc)"
zig_cc_wasi "${SQLITE_CFLAGS[@]}" -Wl,--strip-debug \
  "$tmp/$SQLITE_DIR/sqlite3.c" "$tmp/$SQLITE_DIR/shell.c" \
  -o cache/sqlite3-shell.wasm

echo "sqlite3: building sqlite3-mod.wasm (zig cc, VDBE opcodes split out)"
cp -R "$tmp/$SQLITE_DIR" "$tmp/$SQLITE_DIR-mod"
patch -s -p1 -F 0 -d "$tmp/$SQLITE_DIR-mod" -i "$PWD/$SQLITE_SPLIT_PATCH" || {
  echo "sqlite3: $SQLITE_SPLIT_PATCH does not apply to the pinned source; regenerate it" >&2
  exit 1
}
zig_cc_wasi "${SQLITE_CFLAGS[@]}" \
  "$tmp/$SQLITE_DIR-mod/sqlite3.c" "$tmp/$SQLITE_DIR-mod/shell.c" \
  -o cache/sqlite3-mod.wasm

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
cp cache/sqlite3-mod.wasm "$tmp/sqlite3-mod-inlined.wasm"
wasm_opt_inplace cache/sqlite3-mod.wasm "$SQLITE_MOD_NO_INLINE" "${SQLITE_MOD_STRIP[@]}"
wasm_opt_inplace "$tmp/sqlite3-mod-inlined.wasm" "${SQLITE_MOD_STRIP[@]}"

# Undoing the split changes nothing an app case or a snapshot can see, so the mod artifact would go on passing every test while being a copy of the stock one.
# The control is the same module optimized without --no-inline: the split survived exactly when the artifact carries the functions the patch adds and the control does not.
split_fns=$(grep -c '^static SQLITE_NOINLINE [a-z]* vdbeOp' "$tmp/$SQLITE_DIR-mod/sqlite3.c")
kept_fns=$(($(wasm_func_count cache/sqlite3-mod.wasm) - $(wasm_func_count "$tmp/sqlite3-mod-inlined.wasm")))
[ "$kept_fns" -ge "$split_fns" ] || {
  echo "sqlite3: sqlite3-mod.wasm kept $kept_fns of the $split_fns split opcode functions: wasm-opt inlined them back into the interpreter" >&2
  exit 1
}

write_stamp "$sqlite_stamp" "$sqlite_key"
echo "sqlite3: -> cache/sqlite3-shell.wasm, cache/sqlite3-mod.wasm, cache/libsqlite3.wasm, cache/sqlite3-binding.wasm"
