//! Bash end-to-end suites (ADR-27): the shared library / WASI / apps case consts (`dewasm-test-helper`) wired up for the Bash backend. Per the ADR-27 revision this file holds ONLY the [`BackendUnderTest`] impl, named glue string constants, and per-case macro invocations. Glue is Bash function calls over the R0.. result globals (ADR-11) and the `${prefix}mem` byte array — enough to drive the C-API cases as well; what Bash drops is only what needs a host-language *object* model (a WASI provider object and its lazy construction) and the flat-namespace multi-module limit (ADR-35).

use std::collections::BTreeSet;
use std::path::PathBuf;

use dewasm_backend::Backend;
use dewasm_backend_bash::{find_bash5, BashBackend};
use dewasm_test_helper::BackendUnderTest;

pub struct Bash;

impl BackendUnderTest for Bash {
    fn name(&self) -> &'static str {
        "bash"
    }

    fn backend(&self) -> &'static (dyn Backend + Sync) {
        &BashBackend
    }

    fn interpreter(&self) -> PathBuf {
        find_bash5().expect("bash >= 5 not found — see docs/testing.md")
    }

    /// Compose several `.wat` modules for the multi-module cases (ADR-35). Bash has no namespacing at all — every generated module is already a bare, prefix-scoped family of global functions/arrays sharing one flat process, so there is no separate "shared runtime" linkage to build: generate each module (`generate_module_with_units`, prefixed by its name via `func_prefix`), union the runtime units they reference, bundle that union once, and concatenate the bundle with the module bodies. `shared_runtime=false` (independent Embedded runtimes) is never exercised for Bash: there is only ever one flat namespace, so two independent runtimes cannot coexist without their `rt_*`/`mem_*` function names colliding (`embedded_coexist_e2e!` is not invoked).
    fn compose_modules(&self, modules: &[(&str, &str)], shared_runtime: bool) -> String {
        assert!(
            shared_runtime,
            "bash multi-module: shared_runtime=false is excluded — bash has one \
             flat global namespace, so two independent runtimes cannot coexist \
             without their rt_*/mem_* names colliding (ADR-11/ADR-35)"
        );
        let mut units = BTreeSet::new();
        let mut decls = Vec::new();
        for (wat, name) in modules {
            let bytes =
                wat::parse_file(dewasm_test_helper::examples_dir().join(wat)).expect("parse wat");
            let module = dewasm_core::build_module(&bytes).expect("build IR");
            let prefix = dewasm_backend_bash::func_prefix(name);
            let (src, u) = dewasm_backend_bash::generate_module_with_units(&module, &prefix, false)
                .expect("generate");
            units.extend(u);
            decls.push(src);
        }
        format!(
            "{}\n{}",
            dewasm_backend_bash::shared_runtime(&units).expect("bundle runtime"),
            decls.join("\n")
        )
    }
}

// --------------------------------------------------------------------- Library-case glue: results come back through the R0 global (ADR-11).

/// `add.wat`: call the exported functions and echo each result global.
const BASH_ADD_GLUE: &str = r#"add_init || exit 1
add_invoke add 2 3; echo $R0
add_invoke add 4294967295 1; echo $R0
add_invoke fib 10; echo $R0
"#;

/// The ADR-7 override/fallback glue: an explicit `fd_write` import wins, `random_get` falls back to the bundled WASI. Captures and prints the actual bytes the module wrote — the same observable proof of interception the other backends' override glues use.
const BASH_OVERRIDE_GLUE: &str = r#"my_fd_write() {
  # (fd, iovs, iovs_len, nwritten_ptr): capture and print the actual
  # bytes the module wrote, the same observable proof of interception
  # `RUBY_OVERRIDE_GLUE` uses, via the same byte-reconstruction the
  # bundled fd_write unit itself uses (runtime/bash/units/wasi/fd_write.sh).
  mem_i32_load prog_ "$2" || return $?
  local ptr=$R0
  mem_i32_load prog_ $(( $2 + 4 )) || return $?
  local len=$R0
  local -n mem=prog_mem
  local out='' chunk bytes=() j k
  for (( j = 0; j < len; j++ )); do
    k=$(( ptr + j ))
    bytes+=("$(( mem[$k] ))")
  done
  printf -v chunk '\\x%02x' "${bytes[@]}"
  out+=$chunk
  printf "$out"
  mem_i32_store prog_ "$4" "$len" || return $?
  R0=0
  return 0
}
declare -A IMPORTS=(['wasi_snapshot_preview1.fd_write']=my_fd_write)
prog_init || { echo "init failed" >&2; exit 1; }
prog_invoke '_start'
"#;

/// The `wasi_stdio_capture` glue: bash's embedder-controlled sink is a command substitution — run init and `_start` inside `$( … )` and the guest's fd 1 writes land in a shell variable instead of the script's stdout, the same fd-level redirect Perl's glue uses rather than an in-memory object. `$()` strips the trailing newlines, so the captured text is printed back with `%s\n` to restore the one the guest wrote. The subshell's `_start` ends in `proc_exit`, i.e. the status-133 cascade (ADR-12), which is simply not propagated: this case asserts stdout only.
const BASH_STDIO_CAPTURE_GLUE: &str = r#"captured=$(
  prog_init || { echo "init failed" >&2; exit 1; }
  prog_invoke '_start'
)
printf '%s\n' "$captured"
"#;

/// `SHARED_TABLE`'s driver: `shared_table_a.wat`'s module has no wasm-level name, so `compose_modules` prefixes it from the case's "TableExp" label (`func_prefix`); `shared_table_b.wat` imports it under the *wasm* module name `"a"` (unrelated to "TableExp"), so PROVIDERS maps that literal key to the exporter's generation prefix (ADR-35).
const BASH_SHARED_TABLE_GLUE: &str = r#"tableexp_init || { echo "init failed" >&2; exit 1; }
declare -A PROVIDERS=([a]=tableexp_)
tableimp_init || { echo "init failed" >&2; exit 1; }
tableimp_invoke call0
echo $R0
"#;

/// The `wasi_suite!(Bash, Fs, ...)` template (ADR-34): fill `WASI_DIRS` with the one preopen pair, init, invoke `_start`, then surface a `proc_exit` call the same way the standalone main does — `invoke` returns status 133 (ADR-12) with the code in `$EXIT_CODE` — as a trailing decimal line, the same observable the Ruby glue's `rescue Prog::Rt::Exit` produces. A case that never calls `proc_exit` just falls off the end of `_start`, so nothing is appended and the script exits 0.
const BASH_FS_GLUE: &str = r#"WASI_DIRS=('{host}::{guest}')
prog_init || { echo "init failed" >&2; exit 1; }
prog_invoke '_start'
status=$?
if [[ $status -eq 133 ]]; then
  echo "$EXIT_CODE"
fi
exit 0
"#;

/// The root-preopen containment probe's glue: preopen the filesystem root at guest `/` and call the WASI resolver directly (`wasi_resolve_path <p> <dirfd> <path> <follow>`, ADR-34) instead of running a guest — the bash analogue of Ruby's `wasi.send(:resolve_path, ...)`. `follow=1` matches Ruby's `resolve_path`'s `follow_last: true` default.
const BASH_CONTAINMENT_GLUE: &str = r#"WASI_DIRS=('/::/')
prog_init || { echo "init failed" >&2; exit 1; }
wasi_resolve_path prog_ 3 etc 1
if [[ $R0 -eq 0 ]]; then
  echo "contained"
else
  echo "rejected"
fi
"#;

// --------------------------------------------------------------------- Filesystem app glue (ADR-34): class/argv/env/preopen-guest-paths are literals (`WASI_ARGS`/`WASI_ENV`/`WASI_DIRS`, the Bash analogue of Ruby's `args:`/`env:`/`preopens:` kwargs); only the host scratch dir comes through `{scratch}`. `invoke`'s status-133 cascade is discarded (`exit 0`), the same way the Ruby glue's empty `rescue ...::Rt::Exit` swallows it — these cases assert stdout/host state, never the guest's own exit code.

const BASH_QJS_FILE_IO_GLUE: &str = r#"WASI_ARGS=(qjs /work/qjs_file_io.js)
WASI_ENV=()
WASI_DIRS=('{scratch}::/work')
qjs_init || { echo "init failed" >&2; exit 1; }
qjs_invoke '_start'
exit 0
"#;

const BASH_SQLITE3_SHELL_DBFILE_GLUE: &str = r#"WASI_ARGS=(sqlite3)
WASI_ENV=()
WASI_DIRS=('{scratch}::/db')
sqlite3shell_init || { echo "init failed" >&2; exit 1; }
sqlite3shell_invoke '_start'
exit 0
"#;

const BASH_RG_SEARCH_GLUE: &str = r#"WASI_ARGS=(rg --sort path needle /work)
WASI_ENV=()
WASI_DIRS=('{scratch}::/work')
rg_init || { echo "init failed" >&2; exit 1; }
rg_invoke '_start'
exit 0
"#;

// --------------------------------------------------------------------- C-API drive glue: the same pointer plumbing the other backends write as host-language closures, in Bash's vocabulary — results in the `R0` global (ADR-11), guest memory as the `${P}mem` associative array (one decimal byte per index), and the generated module's own runtime units (`mem_init`, `mem_i32_load`) for the transfers. No wasmtime snapshot exists (the results live in guest memory), so each drive's output is pinned in the shared case const. Only the file-backed case uses {scratch}.

/// The front matter every C-API glue below shares, parameterized by the module's generation prefix and its allocator export (`sqlite3_malloc` for the sqlite artifacts, plain `malloc` for the reactor libraries). A macro rather than a plain const so the pieces can be `concat!`ed into a `&'static str` glue const, keeping each case's drive readable as one literal.
macro_rules! bash_capi_prelude {
    ($prefix:literal, $malloc:literal) => {
        concat!(
            "P=",
            $prefix,
            "\nMALLOC=",
            $malloc,
            "\n",
            r#"
# Call an export, failing loud on a trap (the drive is a straight line: any
# trap means the case is broken, not that the guest reported an error).
capi_call() {
  "${P}invoke" "$@" || { echo "$1 failed (status $?): ${TRAP_MSG-}" >&2; exit 1; }
}

# Copy a string into freshly allocated guest memory with a NUL terminator and
# return the pointer in R0. Character codes come from printf's leading-quote
# form (`printf '%d' "'c"`), the byte array then goes in through `mem_init` —
# the same runtime helper an active data segment uses.
capi_cstr() {
  local s=$1 i c
  CSTR_BYTES=()
  for (( i = 0; i < ${#s}; i++ )); do
    printf -v c '%d' "'${s:i:1}"
    CSTR_BYTES+=("$c")
  done
  CSTR_BYTES+=(0)
  capi_call "$MALLOC" "${#CSTR_BYTES[@]}"
  local p=$R0
  mem_init "$P" CSTR_BYTES "$p" 0 "${#CSTR_BYTES[@]}" || exit 1
  R0=$p
}

# Read the NUL-terminated C string at a guest pointer back into CSTR. An unset
# memory index reads as 0 in arithmetic, i.e. as the terminator. The bytes are
# turned back into text through a `\xNN` printf format, the same reconstruction
# the bundled fd_write unit uses.
capi_read_cstr() {
  local -n __m=${P}mem
  local p=$1 b bytes=() fmt
  CSTR=''
  while (( b = __m[$p] )); do
    bytes+=("$b")
    (( p++ ))
  done
  (( ${#bytes[@]} )) || return 0
  printf -v fmt '\\x%02x' "${bytes[@]}"
  printf -v CSTR "$fmt"
}
"#
        )
    };
}

/// The sqlite3 C API driven in memory: `_initialize`, `sqlite3_malloc` + pointer plumbing, open/exec/prepare/step/column/finalize/close. `sqlite3_prepare_v2`'s -1 length is written as its masked-unsigned i32 (ADR-2).
const BASH_LIBSQLITE3_MEM: &str = concat!(
    bash_capi_prelude!("libsqlite3_", "sqlite3_malloc"),
    r#"
libsqlite3_init || { echo "init failed" >&2; exit 1; }
capi_call '_initialize'

capi_call sqlite3_libversion
capi_read_cstr "$R0"
printf 'version: %s\n' "$CSTR"

capi_call "$MALLOC" 4
ppdb=$R0
capi_cstr ':memory:'
capi_call sqlite3_open "$R0" "$ppdb"
(( R0 == 0 )) || { echo "open rc=$R0" >&2; exit 1; }
mem_i32_load "$P" "$ppdb" || exit 1
db=$R0

capi_cstr "create table t(a,b); insert into t values (1,'x'),(2,'y');"
capi_call sqlite3_exec "$db" "$R0" 0 0 0
(( R0 == 0 )) || { echo "exec rc=$R0" >&2; exit 1; }

capi_call "$MALLOC" 4
ppstmt=$R0
capi_cstr 'select a*10, b from t order by a desc'
capi_call sqlite3_prepare_v2 "$db" "$R0" 4294967295 "$ppstmt" 0
(( R0 == 0 )) || { echo "prepare rc=$R0" >&2; exit 1; }
mem_i32_load "$P" "$ppstmt" || exit 1
stmt=$R0

while capi_call sqlite3_step "$stmt"; [[ $R0 == 100 ]]; do  # SQLITE_ROW
  capi_call sqlite3_column_count "$stmt"
  ncol=$R0
  row=''
  for (( i = 0; i < ncol; i++ )); do
    capi_call sqlite3_column_text "$stmt" "$i"
    capi_read_cstr "$R0"
    (( i )) && row+='|'
    row+=$CSTR
  done
  printf '%s\n' "$row"
done
capi_call sqlite3_finalize "$stmt"
capi_call sqlite3_close "$db"
echo 'C-API-OK'
"#
);

/// The sqlite3 C API against a file preopen: create+insert, close, reopen, select — the file lifecycle through the C API (same ADR-34 fs stack as the shell), leaving a nonzero DB file on the host.
const BASH_LIBSQLITE3_FILE: &str = concat!(
    r#"WASI_ARGS=(libsqlite3)
WASI_ENV=()
WASI_DIRS=('{scratch}::/db')
"#,
    bash_capi_prelude!("libsqlite3_", "sqlite3_malloc"),
    r#"
open_db() {
  capi_call "$MALLOC" 4
  local pp=$R0
  capi_cstr "$1"
  capi_call sqlite3_open "$R0" "$pp"
  (( R0 == 0 )) || { echo "open rc=$R0" >&2; exit 1; }
  mem_i32_load "$P" "$pp" || exit 1
}

libsqlite3_init || { echo "init failed" >&2; exit 1; }
capi_call '_initialize'

# create + insert, then close so the file is fully flushed
open_db /db/data.db
db=$R0
capi_cstr "create table t(a,b); insert into t values (1,'x'),(2,'y');"
capi_call sqlite3_exec "$db" "$R0" 0 0 0
(( R0 == 0 )) || { echo "exec rc=$R0" >&2; exit 1; }
capi_call sqlite3_close "$db"

# reopen the same file and read it back
open_db /db/data.db
db=$R0
capi_call "$MALLOC" 4
ppstmt=$R0
capi_cstr 'select a*10, b from t order by a'
capi_call sqlite3_prepare_v2 "$db" "$R0" 4294967295 "$ppstmt" 0
(( R0 == 0 )) || { echo "prepare rc=$R0" >&2; exit 1; }
mem_i32_load "$P" "$ppstmt" || exit 1
stmt=$R0
while capi_call sqlite3_step "$stmt"; [[ $R0 == 100 ]]; do  # SQLITE_ROW
  capi_call sqlite3_column_count "$stmt"
  ncol=$R0
  row=''
  for (( i = 0; i < ncol; i++ )); do
    capi_call sqlite3_column_text "$stmt" "$i"
    capi_read_cstr "$R0"
    (( i )) && row+='|'
    row+=$CSTR
  done
  printf '%s\n' "$row"
done
capi_call sqlite3_finalize "$stmt"
capi_call sqlite3_close "$db"
echo 'FILE-OK'
"#
);

/// Guest->host callback round trip: the committed `sqlite3-binding.wasm` exports `run_query`, which calls `sqlite3_exec` with a C callback forwarding each row to the *imported* `env.host_row`. The glue provides `host_row` through the ADR-7 `IMPORTS` array — a void import, so it leaves `R0` empty — and collects the rows.
const BASH_SQLITE3_CALLBACK: &str = concat!(
    bash_capi_prelude!("sqlite3_binding_", "sqlite3_malloc"),
    r#"
ROWS=()
host_row() {
  local argc=$1 argv=$2 row='' i
  for (( i = 0; i < argc; i++ )); do
    mem_i32_load "$P" $(( argv + i * 4 )) || return $?
    capi_read_cstr "$R0"
    (( i )) && row+='|'
    row+=$CSTR
  done
  ROWS+=("$row")
  R0=
  return 0
}
declare -A IMPORTS=(['env.host_row']=host_row)

sqlite3_binding_init || { echo "init failed" >&2; exit 1; }
capi_call '_initialize'

capi_call "$MALLOC" 4
ppdb=$R0
capi_cstr ':memory:'
capi_call sqlite3_open "$R0" "$ppdb"
(( R0 == 0 )) || { echo "open rc=$R0" >&2; exit 1; }
mem_i32_load "$P" "$ppdb" || exit 1
db=$R0

capi_cstr "create table t(a,b); insert into t values (1,'x'),(2,'y'),(3,'z');"
capi_call sqlite3_exec "$db" "$R0" 0 0 0
(( R0 == 0 )) || { echo "exec rc=$R0" >&2; exit 1; }

# guest -> host: run_query calls back into env.host_row once per row
capi_cstr 'select a, b from t where a >= 2 order by a'
capi_call run_query "$db" "$R0"
(( R0 == 0 )) || { echo "run_query rc=$R0" >&2; exit 1; }
capi_call sqlite3_close "$db"

for row in "${ROWS[@]}"; do printf 'row: %s\n' "$row"; done
echo 'CALLBACK-OK'
"#
);

/// libpcap BPF filter compilation: drive `compile_filter` on "tcp port 80" (DLT_EN10MB, snaplen 65535), then walk the serialized program `[u32 bf_len][bf_len × {u16 code; u8 jt; u8 jf; u32 k}]` in guest memory, printing each instruction as `code jt jf k`. The u16/u8 fields are composed from single memory bytes (little-endian), the u32s go through `mem_i32_load`.
const BASH_PCAP_COMPILE: &str = concat!(
    bash_capi_prelude!("libpcap_", "malloc"),
    r#"
libpcap_init || { echo "init failed" >&2; exit 1; }
capi_call '_initialize'

capi_cstr 'tcp port 80'
capi_call compile_filter "$R0" 1 65535
prog=$R0
(( prog )) || { echo "compile failed" >&2; exit 1; }
mem_i32_load "$P" "$prog" || exit 1
n=$R0
declare -n mem=${P}mem
for (( i = 0; i < n; i++ )); do
  base=$(( prog + 4 + i * 8 ))
  code=$(( mem[$base] | mem[$(( base + 1 ))] << 8 ))
  jt=$(( mem[$(( base + 2 ))] ))
  jf=$(( mem[$(( base + 3 ))] ))
  mem_i32_load "$P" $(( base + 4 )) || exit 1
  printf '%d %d %d %d\n' "$code" "$jt" "$jf" "$R0"
done
capi_call free "$prog"
echo 'BPF-OK'
"#
);

/// tree-sitter JSON parse: drive `parse_source` on the fixed snippet `{"key": [1, true, null]}` and print the parse tree's S-expression (a malloc'd NUL-terminated C string) from guest memory.
const BASH_TREESITTER_PARSE: &str = concat!(
    bash_capi_prelude!("treesitter_", "malloc"),
    r#"
src='{"key": [1, true, null]}'
treesitter_init || { echo "init failed" >&2; exit 1; }
capi_call '_initialize'

capi_cstr "$src"
capi_call parse_source "$R0" "${#src}"
tree=$R0
(( tree )) || { echo "parse failed" >&2; exit 1; }
capi_read_cstr "$tree"
printf '%s\n' "$CSTR"
capi_call free "$tree"
echo 'TS-OK'
"#
);

/// DOOM (ADR-53): the frame snapshot, modelled on the Bash frontend (examples/doom/bash/main.sh) — imp_* handlers set `R0`, `IMPORTS[mod.name]` wires them, `doom_init`/`doom_invoke` drive, and `doom_mem` holds the pixels. The P6 frame goes out through a chunked `printf` `\xNN` format string (which carries the NUL bytes a Bash variable cannot). `{ticks}`/`{clock_step}` filled by the runner.
const BASH_DOOM_FRAME_GLUE: &str = r#"DOOM_MS=0
FRAME_BUF_OFF=0
FRAME_W=0
FRAME_H=0

imp_noop() { R0=; return 0; }
imp_zero() { R0=0; return 0; }
imp_time_ms() { DOOM_MS=$(( DOOM_MS + {clock_step} )); R0=$DOOM_MS; return 0; }
imp_draw_frame() { FRAME_BUF_OFF=$1; R0=; return 0; }
imp_on_game_init() { FRAME_W=$1; FRAME_H=$2; R0=; return 0; }

declare -A IMPORTS=(
  ['console.onErrorMessage']=imp_noop
  ['console.onInfoMessage']=imp_noop
  ['gameSaving.sizeOfSaveGame']=imp_zero
  ['gameSaving.readSaveGame']=imp_zero
  ['gameSaving.writeSaveGame']=imp_zero
  ['runtimeControl.timeInMilliseconds']=imp_time_ms
  ['ui.drawFrame']=imp_draw_frame
  ['loading.onGameInit']=imp_on_game_init
  ['loading.wadSizes']=imp_noop
  ['loading.readWads']=imp_noop
)

doom_init || { echo "doom_init failed" >&2; exit 1; }
doom_invoke initGame || { echo "initGame failed: ${TRAP_MSG-}" >&2; exit 1; }
for (( _t = 0; _t < {ticks}; _t++ )); do
  doom_invoke tickGame || { echo "tickGame failed: ${TRAP_MSG-}" >&2; exit 1; }
done

w=$FRAME_W
h=$FRAME_H
off=$FRAME_BUF_OFF
n=$(( w * h * 4 ))
printf 'P6\n%d %d\n255\n' "$w" "$h"
fmt=''
cnt=0
for (( i = 0; i < n; i += 4 )); do
  r=${doom_mem[$(( off + i + 2 ))]-0}
  g=${doom_mem[$(( off + i + 1 ))]-0}
  b=${doom_mem[$(( off + i ))]-0}
  printf -v px '\\x%02x\\x%02x\\x%02x' "$r" "$g" "$b"
  fmt+=$px
  if (( ++cnt >= 4096 )); then printf "$fmt"; fmt=''; cnt=0; fi
done
[[ -n $fmt ]] && printf "$fmt"
"#;

/// NES (issue #114, mirrors the DOOM glue above): load the pinned ROM into
/// `allocRom`'s buffer via `mem_init` (the same runtime helper active data
/// segments use), tick `{frames}` times with no input, compose the frame from
/// agnes's palette-index screen buffer and its palette (issue #117 — one
/// `nes_mem` read per pixel instead of three, against a 64-entry `\xNN\xNN\xNN`
/// lookup table built once; the `& 0x3f` mask is load-bearing), and dump it
/// through the same chunked `printf` pipeline as DOOM. `ROM_BYTES` is
/// built with `od`/`mapfile` rather than a bash loop — the ROM is 41 KB, and
/// `mem_init`'s own copy loop already walks it once. `{rom}` (the cached
/// ROM's host path) and `{frames}` filled by the runner. The trailing `exit
/// 0` matters here in a way it doesn't for DOOM: at NES's 256x240 the pixel
/// count divides the 4096-pixel flush chunk exactly, so the final `[[ -n
/// $fmt ]]` is false (everything already flushed inside the loop) and,
/// uncorrected, that would leave the script's own exit status at the `[[
/// ]]`'s 1 — a false failure despite a byte-correct frame on stdout.
const BASH_NES_FRAME_GLUE: &str = r#"mapfile -t ROM_BYTES < <(od -An -v -tu1 "{rom}" | tr -s ' \n' '\n' | sed '/^$/d')

nes_init || { echo "nes_init failed" >&2; exit 1; }
rom_len=${#ROM_BYTES[@]}
nes_invoke allocRom "$rom_len" || { echo "allocRom failed: ${TRAP_MSG-}" >&2; exit 1; }
rom_ptr=$R0
mem_init nes_ ROM_BYTES "$rom_ptr" 0 "$rom_len" || { echo "mem_init failed: ${TRAP_MSG-}" >&2; exit 1; }
nes_invoke initGame || { echo "initGame failed: ${TRAP_MSG-}" >&2; exit 1; }
[[ $R0 == 1 ]] || { echo "initGame returned $R0" >&2; exit 1; }
for (( _t = 0; _t < {frames}; _t++ )); do
  nes_invoke tickGame || { echo "tickGame failed: ${TRAP_MSG-}" >&2; exit 1; }
done

nes_invoke frameWidth || { echo "frameWidth failed: ${TRAP_MSG-}" >&2; exit 1; }
w=$R0
nes_invoke frameHeight || { echo "frameHeight failed: ${TRAP_MSG-}" >&2; exit 1; }
h=$R0
nes_invoke screenOffset || { echo "screenOffset failed: ${TRAP_MSG-}" >&2; exit 1; }
soff=$R0
nes_invoke paletteOffset || { echo "paletteOffset failed: ${TRAP_MSG-}" >&2; exit 1; }
poff=$R0
declare -a PAL
for (( e = 0; e < 64; e++ )); do
  c=$(( poff + e * 4 ))
  printf -v "PAL[$e]" '\\x%02x\\x%02x\\x%02x' \
    "${nes_mem[$c]-0}" "${nes_mem[$(( c + 1 ))]-0}" "${nes_mem[$(( c + 2 ))]-0}"
done
n=$(( w * h ))
printf 'P6\n%d %d\n255\n' "$w" "$h"
fmt=''
cnt=0
for (( i = 0; i < n; i++ )); do
  fmt+=${PAL[$(( ${nes_mem[$(( soff + i ))]-0} & 0x3f ))]}
  if (( ++cnt >= 4096 )); then printf "$fmt"; fmt=''; cnt=0; fi
done
[[ -n $fmt ]] && printf "$fmt"
exit 0
"#;

// --------------------------------------------------------------------- Suite wiring (ADR-27): each per-case macro invocation declares participation.

dewasm_test_helper::library_add_e2e!(Bash, BASH_ADD_GLUE);
dewasm_test_helper::wasi_import_override_e2e!(Bash, BASH_OVERRIDE_GLUE);
dewasm_test_helper::stdio_capture_e2e!(Bash, BASH_STDIO_CAPTURE_GLUE);
// custom_wasi_provider_e2e! / partial_override_e2e!: not invoked — Bash has no host-language object model to replace WASI wholesale or probe its lazy construction (ADR-12).

dewasm_test_helper::wasi_suite!(Bash, Stdio);
dewasm_test_helper::wasi_suite!(Bash, ArgsEnv);
dewasm_test_helper::wasi_suite!(Bash, Poll);
dewasm_test_helper::wasi_suite!(Bash, Fs, BASH_FS_GLUE);
dewasm_test_helper::wasi_root_containment_e2e!(Bash, BASH_CONTAINMENT_GLUE);
dewasm_test_helper::standalone_dir_e2e!(Bash);

dewasm_test_helper::cowsay_args_e2e!(Bash);
dewasm_test_helper::cowsay_stdin_e2e!(Bash);
// qjs_eval_e2e! / sqlite3_shell_e2e!: invoked, but slow — Bash's softfloat makes QuickJS/SQLite take tens of seconds, so the generated tests are `#[ignore]`d by default; `--features slow_test` or `-- --include-ignored` runs them anyway (same as every other backend, ADR-27 revision).
dewasm_test_helper::qjs_eval_e2e!(Bash);
dewasm_test_helper::sqlite3_shell_e2e!(Bash);
// minigzip is integer-only (no softfloat), so it runs under Bash by default, unlike the slow floating-point apps (QuickJS/SQLite).
dewasm_test_helper::gzip_e2e!(Bash);

// Filesystem app cases (ADR-34): Bash's WASI filesystem now covers preopens, path_open, and positioned I/O, so the small-fixture fs apps are wired (all slow, softfloat-bound QuickJS/SQLite — see qjs_eval_e2e! above).
dewasm_test_helper::qjs_file_io_e2e!(Bash, BASH_QJS_FILE_IO_GLUE);
dewasm_test_helper::sqlite3_shell_dbfile_e2e!(Bash, BASH_SQLITE3_SHELL_DBFILE_GLUE);
// ripgrep: an 18 MB wasm becomes a 70 MB script, which bash parses in 1.7 s and then walks the fixture tree in the rest of a 22 s run — the slowest of the Bash `slow` cases but the same cluster as qjs_eval (13.6 s) and sqlite3_shell_dbfile (10.0 s), not the ~1-minute `ultra` line.
dewasm_test_helper::rg_search_e2e!(Bash, BASH_RG_SEARCH_GLUE);
// qjs_repl_pty is not a filesystem case (no preopens) but is wired here alongside them: it shares their standalone-mode QuickJS conversion. It is markedly slower than every other case — every keystroke of the scripted session re-enters QuickJS's interactive line editor (redraw/completion), and each successive evaluation measured slower than the last (first prompt ~135s, then +~65s, then +~330s for `[3,1,2].sort()`), so it exceeds the shared 180s per-prompt `PTY_TIMEOUT` (`crates/dewasm-test-helper/src/qjs_repl.rs`) and timed out on CI (#22). It is therefore pinned to the `ultra` category (ADR-48): kept out of CI's `slow_test` run and run only under `--features ultra_slow_test` or `-- --include-ignored`, in local pre-release verification. Left wired rather than unwired per ADR-15 (fail loud, not silently skip): a timeout is still an honest signal.
dewasm_test_helper::qjs_repl_pty_e2e!(Bash, ultra);
// cpython_hello_e2e! / cruby_hello_e2e! / cruby_packed_hello_e2e!: not invoked — wall time, not parse feasibility, is what stops these. The generated scripts do load: the 18 MB rg.wasm's 70 MB script parses in 1.7 s and finishes the fixture search in 21 s (wired above), and CPython's 30 MB wasm yields a 226 MB script that bash parses in 4.6 s. What is unmeasured is how long an interpreter boot then takes under ADR-13's softfloat, where a single arithmetic op costs a bash function call; CRuby is the same shape, one interpreter larger, and its wasi-vfs-packed variant (ADR-61) larger still.

dewasm_test_helper::doom_frame_e2e!(Bash, BASH_DOOM_FRAME_GLUE, ultra);
// Ultra-slow category (ADR-48): tens of seconds per tick locally (mem_init's own copy loop over the 41 KB ROM, then agnes's per-frame interpretation), ~20 min for the full 40-frame run — well past the ~1-minute CI-runner line, like the DOOM case above. (It was ~25 min before issue #117 moved the per-pixel frame composition out of the guest.)
dewasm_test_helper::nes_frame_e2e!(Bash, BASH_NES_FRAME_GLUE, ultra);

dewasm_test_helper::shared_table_e2e!(Bash, BASH_SHARED_TABLE_GLUE);
// The C-API cases run under Bash too: a pointer is a decimal in `R0` and guest memory is the `${P}mem` array, which is all the plumbing these drives need — the NES glue above already does the same in the other direction. The artifacts are within reach as well (41.6 MB libsqlite3, 18.1 MB libpcap, 3.0 MB tree-sitter scripts, all at or below the 45 MB sqlite3-shell script Bash already runs), and all five stay in the `slow` category: 0.7 s tree-sitter, 1.4 s libpcap, 4.0/4.2 s the in-memory sqlite drives, 7.7 s the file-backed one.
dewasm_test_helper::libsqlite3_c_api_e2e!(Bash, BASH_LIBSQLITE3_MEM);
dewasm_test_helper::sqlite3_file_c_api_e2e!(Bash, BASH_LIBSQLITE3_FILE);
dewasm_test_helper::sqlite3_callback_binding_e2e!(Bash, BASH_SQLITE3_CALLBACK);
dewasm_test_helper::pcap_compile_e2e!(Bash, BASH_PCAP_COMPILE);
dewasm_test_helper::treesitter_parse_e2e!(Bash, BASH_TREESITTER_PARSE);
// embedded_coexist_e2e!: not invoked — Bash has one flat global namespace (no nested runtime/class construct), so two independently-generated runtimes cannot coexist in one process without their rt_*/mem_* function names colliding (ADR-11).
