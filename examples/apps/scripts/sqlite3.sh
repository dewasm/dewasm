#!/usr/bin/env bash
# shellcheck source-path=SCRIPTDIR
# shellcheck source=common.sh

# sqlite3: built from the amalgamation source at a fixed version with `wasi-sdk`.
#
# One source release yields four artifacts:
#
# - cache/sqlite3-shell.wasm: the CLI shell (standalone: _start, stdio).
# - cache/sqlite3-mod.wasm: the same shell, built from a patched source copy.
#   The patch is `src/sqlite3-vdbe-split.patch`.
#   It moves the hot VDBE opcode bodies into their own functions, so a JIT reaches them.
# - cache/libsqlite3.wasm: a reactor library exporting the sqlite3 C API.
#   The apps e2e drives it from Ruby.
# - cache/sqlite3-binding.wasm: the same reactor library plus our own `src/sqlite3_binding.c`.
#   Its `run_query` calls back into an imported env.host_row.
#   That is the proof of a guest->host callback round-trip.
#
# No upstream distributes a C-API-exporting wasm32-wasi build.
# That is why these are compiled locally rather than fetched.

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
# The full statement/bind/column surface.
# It is enough to implement on top of it the sqlite3 gem API that Rails' SQLite3Adapter uses.
# examples/rails does that.
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

# The C-source half of the `sqlite3-mod` build; the patch's header states what it changes and why.
SQLITE_SPLIT_PATCH="src/sqlite3-vdbe-split.patch"
# The `wasm-opt` half of the `sqlite3-mod` build, and why it cannot join the shared recipe.
# -O2 inlines a function with a single caller.
# That puts every `vdbeOp*` body straight back into the interpreter.
# The artifact then cannot be told apart from the stock shell.
# So the `sqlite3-mod` build turns that off by name.
# Matching by name needs the name section.
# So the `sqlite3-mod` link drops -Wl,--strip-debug, and the debug information leaves here instead.
# The two groups stay separate because the check after the build reruns the stripping half alone.
# That run is the check's control.
SQLITE_MOD_NO_INLINE="--no-inline=vdbeOp*"
SQLITE_MOD_STRIP=(--strip-dwarf --strip-producers)

# `patch_version_suffix <file>`: rewrite the unpacked source's `#define SQLITE_VERSION` line.
# The source has one such line.
# The new value is "3.53.3-wasm".
# The converted engine must identify itself in example output.
# So it is not mistaken for a native SQLite.
# `examples/rails` answers `/stats` with `sqlite_version()`.
# SQLITE_VERSION_NUMBER and SQLITE_SOURCE_ID stay upstream.
# Only the display string carries the suffix.
# `sed -i` is not portable across BSD and GNU, hence the temporary file.
# A source whose define does not have the assumed shape stops the build with an error.
# It does not silently produce an unpatched artifact.
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

# `wasm_func_count <wasm>`: the module's function count.
wasm_func_count() {
  wasm-opt "${WASM_OPT_FEATURES[@]}" --metrics "$1" -o /dev/null 2>&1 |
    awk '$1 == "[funcs]" { print $3 }'
}

# The stamp covers these inputs, so editing any of them retriggers the build:
# - the source checksum and the export lists;
# - the version-string patch;
# - the split patch's bytes with the `wasm-opt` flags that keep it effective;
# - the `wasm-opt` version and the toolchain token.
sqlite_key="$SQLITE_SHA256 exports:${SQLITE_EXPORTS[*]} binding:${BINDING_EXPORTS[*]} version-suffix:-wasm split:$(shasum -a 256 "$SQLITE_SPLIT_PATCH" | cut -d' ' -f1) mod-wasm-opt:$SQLITE_MOD_NO_INLINE ${SQLITE_MOD_STRIP[*]} wasm-opt:$(wasm_opt_version) $(wasi_sdk_stamp)"
sqlite_stamp="cache/sqlite3.src-sha256"
if is_cached "$sqlite_stamp" "$sqlite_key" \
  cache/sqlite3-shell.wasm cache/sqlite3-mod.wasm \
  cache/libsqlite3.wasm cache/sqlite3-binding.wasm; then
  echo "sqlite3: cached"
  exit 0
fi

require_wasi_sdk sqlite3
require_tool sqlite3 unzip
require_tool sqlite3 wasm-opt "install binaryen (e.g. brew install binaryen) to preprocess the sqlite3 apps"
# The `sqlite3-mod` build passes --no-inline.
# It keeps the split VDBE opcode functions out of Binaryen's inlining.
# That inlining targets single-caller functions.
# A Binaryen too old to know the flag would fail mid-build, so refuse it up front.
# `grep` must consume the whole help text: -q exits at the first match.
# Under `pipefail`, the SIGPIPE that gives `wasm-opt` then fails the probe.
# That happens on hosts where the help text is larger than the pipe buffer.
if ! wasm-opt --help 2>&1 | grep -c -- --no-inline > /dev/null; then
  echo "sqlite3: this wasm-opt does not support --no-inline; install the binaryen version mise.toml states" >&2
  exit 1
fi

echo "sqlite3: fetching $SQLITE_URL"
new_tmpdir
fetch_verified "$SQLITE_URL" "$SQLITE_SHA256" "$tmp/sqlite.zip"
unzip -q "$tmp/sqlite.zip" -d "$tmp"
# The amalgamation embeds a copy of the header in sqlite3.c, and shell.c includes sqlite3.h.
# So both files carry the define.
patch_version_suffix "$tmp/$SQLITE_DIR/sqlite3.c"
patch_version_suffix "$tmp/$SQLITE_DIR/sqlite3.h"
# --strip-debug (the three stock builds) drops the DWARF `wasm-opt` cannot parse.
# The `sqlite3-mod` build strips it in `wasm-opt` instead, see SQLITE_MOD_STRIP.
# The two shell builds add `src/sqlite3_shell_wasi_compat.c`.
# That file is a `getpid` stand-in shell.c's debug path links.
# Its header comment states why the engine needs none.
echo "sqlite3: building sqlite3-shell.wasm (wasi-sdk clang)"
wasi_sdk_clang "${SQLITE_CFLAGS[@]}" -Wl,--strip-debug \
  "$tmp/$SQLITE_DIR/sqlite3.c" "$tmp/$SQLITE_DIR/shell.c" src/sqlite3_shell_wasi_compat.c \
  -o cache/sqlite3-shell.wasm

echo "sqlite3: building sqlite3-mod.wasm (wasi-sdk clang, VDBE opcodes split out)"
cp -R "$tmp/$SQLITE_DIR" "$tmp/$SQLITE_DIR-mod"
patch -s -p1 -F 0 -d "$tmp/$SQLITE_DIR-mod" -i "$PWD/$SQLITE_SPLIT_PATCH" || {
  echo "sqlite3: $SQLITE_SPLIT_PATCH does not apply to the pinned source; regenerate it" >&2
  exit 1
}
wasi_sdk_clang "${SQLITE_CFLAGS[@]}" \
  "$tmp/$SQLITE_DIR-mod/sqlite3.c" "$tmp/$SQLITE_DIR-mod/shell.c" src/sqlite3_shell_wasi_compat.c \
  -o cache/sqlite3-mod.wasm

echo "sqlite3: building libsqlite3.wasm (wasi-sdk clang, reactor)"
mapfile -t exports < <(wl_exports "${SQLITE_EXPORTS[@]}")
wasi_sdk_clang -mexec-model=reactor "${SQLITE_CFLAGS[@]}" -Wl,--strip-debug \
  -DSQLITE_OMIT_LOAD_EXTENSION \
  "$tmp/$SQLITE_DIR/sqlite3.c" \
  "${exports[@]}" \
  -o cache/libsqlite3.wasm

# The binding artifact: the reactor library plus our own `run_query`.
# `run_query` forwards each result row to the imported env.host_row.
# Only the symbols this callback flow needs are exported.
# The import lands via the `import_module`/`import_name` attributes in `src/sqlite3_binding.c`.
echo "sqlite3: building sqlite3-binding.wasm (wasi-sdk clang, reactor + host callback)"
mapfile -t binding_exports < <(wl_exports "${BINDING_EXPORTS[@]}")
wasi_sdk_clang -mexec-model=reactor "${SQLITE_CFLAGS[@]}" -Wl,--strip-debug \
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

# Undoing the split changes nothing an app case or a snapshot can see.
# So the `sqlite3-mod` artifact would go on passing every test while being a copy of the stock one.
# The control is the same module optimized without --no-inline.
# The split survived exactly when the artifact has the patch's functions and the control lacks them.
split_fns=$(grep -c '^static SQLITE_NOINLINE [a-z]* vdbeOp' "$tmp/$SQLITE_DIR-mod/sqlite3.c")
kept_fns=$(($(wasm_func_count cache/sqlite3-mod.wasm) - $(wasm_func_count "$tmp/sqlite3-mod-inlined.wasm")))
[ "$kept_fns" -ge "$split_fns" ] || {
  echo "sqlite3: sqlite3-mod.wasm kept $kept_fns of the $split_fns split opcode functions: wasm-opt inlined them back into the interpreter" >&2
  exit 1
}

write_stamp "$sqlite_stamp" "$sqlite_key"
echo "sqlite3: -> cache/sqlite3-shell.wasm, cache/sqlite3-mod.wasm, cache/libsqlite3.wasm, cache/sqlite3-binding.wasm"
