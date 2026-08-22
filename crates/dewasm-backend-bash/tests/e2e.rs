//! Bash end-to-end suites: the shared library / WASI / apps case consts (`dewasm-test-helper`) wired up for the Bash backend.
//! This file holds ONLY the [`BackendUnderTest`] impl, named glue string constants, and per-case macro invocations.
//! Glue is Bash function calls over the R0.. result globals and the `${prefix}mem` byte array, enough to drive the C-API and multi-module cases as well.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

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
        find_bash5().expect("bash >= 5 not found: see docs/testing.md")
    }

    /// Write each `.wat` module of a multi-module case into `dir` as its own `.sh` file and return the `source` preamble that loads them (absolute paths, so the driver does not depend on where it is run from).
    /// Sourcing only defines functions, so the only order that matters is runtime-before-init, which the preamble's order gives.
    /// `shared_runtime` builds the Alias shape: generate each module (`generate_module_with_units`, prefixed by its name via `func_prefix`) against the flat unprefixed runtime, union the units they reference, and bundle that union once into `rt.sh`: one runtime, so an imported table crosses modules.
    /// Otherwise each file is a self-contained library conversion whose runtime functions carry that artifact's own prefix (`alpha_rt_trap` beside `beta_rt_trap`), which is what lets two of them share the one flat shell namespace.
    fn compose_modules(
        &self,
        dir: &Path,
        modules: &[(&str, &str)],
        shared_runtime: bool,
    ) -> String {
        let write = |stem: &str, src: String| -> String {
            let path = dir.join(format!("{stem}.sh"));
            std::fs::write(&path, src).unwrap();
            format!("source '{}'", path.display())
        };
        if !shared_runtime {
            return modules
                .iter()
                .map(|(wat, name)| {
                    write(
                        &name.to_lowercase(),
                        dewasm_test_helper::convert(
                            &BashBackend,
                            &dewasm_test_helper::examples_dir().join(wat),
                            dewasm_backend::Mode::Library,
                            name,
                        ),
                    )
                })
                .collect::<Vec<_>>()
                .join("\n");
        }
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
            decls.push((name.to_lowercase(), src));
        }
        let mut sources = vec![write(
            "rt",
            dewasm_backend_bash::shared_runtime(&units).expect("bundle runtime"),
        )];
        for (stem, src) in decls {
            sources.push(write(&stem, src));
        }
        sources.join("\n")
    }
}

// --------------------------------------------------------------------- Library-case glue: results come back through the R0 global.

/// `add.wat`: call the exported functions and echo each result global.
const BASH_ADD_GLUE: &str = r#"add_init || exit 1
add_invoke add 2 3; echo $R0
add_invoke add 4294967295 1; echo $R0
add_invoke fib 10; echo $R0
"#;

/// The override/fallback glue: an explicit `fd_write` import wins, `random_get` falls back to the bundled WASI.
/// Captures and prints the actual bytes the module wrote, the same observable proof of interception the other backends' override glues use.
const BASH_OVERRIDE_GLUE: &str = r#"my_fd_write() {
  # (fd, iovs, iovs_len, nwritten_ptr): capture and print the actual
  # bytes the module wrote, the same observable proof of interception
  # `RUBY_OVERRIDE_GLUE` uses, via the same byte-reconstruction the
  # bundled fd_write unit itself uses (crates/dewasm-backend-bash/units/wasi/fd_write.sh).
  prog_mem_i32_load prog_ "$2" || return $?
  local ptr=$R0
  prog_mem_i32_load prog_ $(( $2 + 4 )) || return $?
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
  prog_mem_i32_store prog_ "$4" "$len" || return $?
  R0=0
  return 0
}
declare -A IMPORTS=(['wasi_snapshot_preview1.fd_write']=my_fd_write)
prog_init || { echo "init failed" >&2; exit 1; }
prog_invoke '_start'
"#;

/// The `custom_wasi_provider` glue: a provider *prefix* replaces the bundled WASI wholesale: `PROVIDERS[wasi_snapshot_preview1]` points at `my_`, whose `my_EXPORTS` map covers both imports (the bash shape of a provider object), so no import falls back and `<p>init` never builds the bundled WASI's prefix-scoped state.
/// The probe is a real existence test on one of those variables (`declare -p prog_wfds`), the bash counterpart of Ruby's `@wasi.nil?`.
const BASH_CUSTOM_PROVIDER_GLUE: &str = r#"my_fd_write() {
  # Same interception and byte-reconstruction as `BASH_OVERRIDE_GLUE`.
  prog_mem_i32_load prog_ "$2" || return $?
  local ptr=$R0
  prog_mem_i32_load prog_ $(( $2 + 4 )) || return $?
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
  prog_mem_i32_store prog_ "$4" "$len" || return $?
  R0=0
  return 0
}
my_random_get() {
  R0=0
  return 0
}
declare -A my_EXPORTS=([fd_write]=my_fd_write [random_get]=my_random_get)
declare -A PROVIDERS=([wasi_snapshot_preview1]=my_)
prog_init || { echo "init failed" >&2; exit 1; }
prog_invoke '_start'
if declare -p prog_wfds &>/dev/null; then built=true; else built=false; fi
echo "bundled wasi constructed: $built"
"#;

/// The `partial_override_falls_back_to_bundled_wasi` glue: `BASH_OVERRIDE_GLUE` (fd_write intercepted through IMPORTS, random_get falling back) plus the same probe, which now finds the state: `<p>init` builds it for that one fallback.
const BASH_PARTIAL_OVERRIDE_GLUE: &str = r#"my_fd_write() {
  # Same interception and byte-reconstruction as `BASH_OVERRIDE_GLUE`.
  prog_mem_i32_load prog_ "$2" || return $?
  local ptr=$R0
  prog_mem_i32_load prog_ $(( $2 + 4 )) || return $?
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
  prog_mem_i32_store prog_ "$4" "$len" || return $?
  R0=0
  return 0
}
declare -A IMPORTS=(['wasi_snapshot_preview1.fd_write']=my_fd_write)
prog_init || { echo "init failed" >&2; exit 1; }
prog_invoke '_start' # random_get falls back to the bundled WASI
if declare -p prog_wfds &>/dev/null; then built=true; else built=false; fi
echo "bundled wasi constructed: $built"
"#;

/// The `wasi_stdio_capture` glue: bash's embedder-controlled sink is a command substitution: run init and `_start` inside `$( … )` and the guest's fd 1 writes land in a shell variable instead of the script's stdout, the same fd-level redirect Perl's glue uses rather than an in-memory object.
/// `$()` strips the trailing newlines, so the captured text is printed back with `%s\n` to restore the one the guest wrote.
/// The subshell's `_start` ends in `proc_exit`, i.e. the status-133 cascade, which is simply not propagated: this case asserts stdout only.
const BASH_STDIO_CAPTURE_GLUE: &str = r#"captured=$(
  prog_init || { echo "init failed" >&2; exit 1; }
  prog_invoke '_start'
)
printf '%s\n' "$captured"
"#;

/// `SHARED_TABLE`'s driver: `shared_table_a.wat`'s module has no wasm-level name, so `compose_modules` prefixes it from the case's "TableExp" label (`func_prefix`); `shared_table_b.wat` imports it under the *wasm* module name `"a"` (unrelated to "TableExp"), so PROVIDERS maps that literal key to the exporter's generation prefix.
const BASH_SHARED_TABLE_GLUE: &str = r#"tableexp_init || { echo "init failed" >&2; exit 1; }
declare -A PROVIDERS=([a]=tableexp_)
tableimp_init || { echo "init failed" >&2; exit 1; }
tableimp_invoke call0
echo $R0
"#;

/// Two self-contained artifacts sourced into one shell: each carries its own runtime under its own prefix, so `alpha_rt_trap` and `beta_rt_trap` are two functions, not one that the second `source` overwrote.
/// `declare -F` on both names is the structural probe (the bash counterpart of Ruby comparing `Alpha::Rt::Trap` with `Beta::Rt::Trap`), and the trap that follows is raised by Alpha's own copy through the status-134 cascade, with `TRAP_MSG` staying the shared protocol both artifacts write.
const BASH_EMBEDDED_COEXIST_GLUE: &str = r#"alpha_init || { echo "alpha_init failed" >&2; exit 1; }
beta_init || { echo "beta_init failed" >&2; exit 1; }
alpha_invoke div 7 2 || { echo "alpha div failed: ${TRAP_MSG-}" >&2; exit 1; }
echo $R0
beta_invoke div 4294967289 2 || { echo "beta div failed: ${TRAP_MSG-}" >&2; exit 1; }
echo $R0
if declare -F alpha_rt_trap >/dev/null && declare -F beta_rt_trap >/dev/null; then
  echo distinct-rt
else
  echo same-rt
fi
alpha_invoke div 1 0
if (( $? == 134 )); then echo trapped; fi
exit 0
"#;

/// The `wasi_suite!(Bash, Fs, ...)` template: fill `WASI_DIRS` with the one preopen pair, init, invoke `_start`, then surface a `proc_exit` call the same way the standalone main does (`invoke` returns status 133 with the code in `$EXIT_CODE`) as a trailing decimal line, the same observable the Ruby glue's `rescue Prog::Rt::Exit` produces.
/// A case that never calls `proc_exit` just falls off the end of `_start`, so nothing is appended and the script exits 0.
const BASH_FS_GLUE: &str = r#"WASI_DIRS=('{host}::{guest}')
prog_init || { echo "init failed" >&2; exit 1; }
prog_invoke '_start'
status=$?
if [[ $status -eq 133 ]]; then
  echo "$EXIT_CODE"
fi
exit 0
"#;

/// The root-preopen containment probe's glue: preopen the filesystem root at guest `/` and call the WASI resolver directly (`<p>wasi_resolve_path <p> <dirfd> <path> <follow>`) instead of running a guest, the bash analogue of Ruby's `wasi.send(:resolve_path, ...)`.
/// `follow=1` matches Ruby's `resolve_path`'s `follow_last: true` default.
const BASH_CONTAINMENT_GLUE: &str = r#"WASI_DIRS=('/::/')
prog_init || { echo "init failed" >&2; exit 1; }
prog_wasi_resolve_path prog_ 3 etc 1
if [[ $R0 -eq 0 ]]; then
  echo "contained"
else
  echo "rejected"
fi
"#;

// --------------------------------------------------------------------- Filesystem app glue: class/argv/env/preopen-guest-paths are literals (`WASI_ARGS`/`WASI_ENV`/`WASI_DIRS`, the Bash analogue of Ruby's `args:`/`env:`/`preopens:` kwargs); only the host scratch dir comes through `{scratch}`.
// `invoke`'s status-133 cascade is discarded (`exit 0`), the same way the Ruby glue's empty `rescue ...::Rt::Exit` swallows it: these cases assert stdout/host state, never the guest's own exit code.

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

/// The whole app cache is preopened at `/apps` because the guest module this converted interpreter loads (`cowsay.wasm`) is itself a cached app.
const BASH_TOYWASM_GLUE: &str = r#"WASI_ARGS=(toywasm --wasi /apps/cowsay.wasm Hello from 'dewasm!')
WASI_ENV=()
WASI_DIRS=('{cache}::/apps')
toywasm_init || { echo "init failed" >&2; exit 1; }
toywasm_invoke '_start'
exit 0
"#;

/// CPython reading its stdlib from the cache-preopened tree at `/lib`; `WASI_ENV` carries `PYTHONHOME`/`PYTHONPATH` as the `NAME=value` strings the standalone main also builds.
///
/// The leading `ulimit` is the one thing these two interpreters need that the smaller filesystem apps do not.
/// Each wasm call nests one native bash call, and CPython's boot recurses far enough to exhaust the 8 MB default process stack: a real SIGSEGV, not a trappable wasm stack overflow (measured: without this line the trial dies of signal 11 partway through the boot, ~90 s in).
/// The generated *standalone* entrypoint raises the soft rlimit for exactly this reason; a library-mode embedder has to do it itself, and the glue is that embedder, so it repeats the line, with the same `unlimited`-then-hard-limit fallback and the same silent degradation if a sandbox refuses both.
const BASH_CPYTHON_GLUE: &str = r#"ulimit -s unlimited 2>/dev/null || ulimit -s "$(ulimit -Hs)" 2>/dev/null || true
WASI_ARGS=(python -c "print('hello from cpython', 6 * 7)")
WASI_ENV=(PYTHONHOME=/ PYTHONPATH=/lib/python3.14)
WASI_DIRS=('{cache}/cpython-lib/lib::/lib')
cpython_init || { echo "init failed" >&2; exit 1; }
cpython_invoke '_start'
exit 0
"#;

/// CRuby reading its stdlib from the cache-preopened tree at `/usr`: Ruby on Bash.
/// Raises the stack rlimit for the same reason [`BASH_CPYTHON_GLUE`] does.
const BASH_CRUBY_GLUE: &str = r#"ulimit -s unlimited 2>/dev/null || ulimit -s "$(ulimit -Hs)" 2>/dev/null || true
WASI_ARGS=(ruby -e 'puts "hello from cruby #{6*7}"')
WASI_ENV=()
WASI_DIRS=('{cache}/ruby-lib/usr::/usr')
cruby_init || { echo "init failed" >&2; exit 1; }
cruby_invoke '_start'
exit 0
"#;

// --------------------------------------------------------------------- C-API drive glue: the same pointer plumbing the other backends write as host-language closures, in Bash's vocabulary: results in the `R0` global, guest memory as the `${P}mem` associative array (one decimal byte per index), and the generated module's own runtime units for the transfers, spelled `"${P}mem_init"` / `"${P}mem_i32_load"`, since an artifact's runtime carries that artifact's prefix and `$P` is the very prefix these drives already carry.
// No wasmtime snapshot exists (the results live in guest memory), so each drive's output is pinned in the shared case const.
// Only the file-backed case uses {scratch}.

/// The front matter every C-API glue below shares, parameterized by the module's generation prefix and its allocator export (`sqlite3_malloc` for the sqlite artifacts, plain `malloc` for the reactor libraries).
/// A macro rather than a plain const so the pieces can be `concat!`ed into a `&'static str` glue const, keeping each case's drive readable as one literal.
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
# form (`printf '%d' "'c"`), the byte array then goes in through `mem_init`,
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
  "${P}mem_init" "$P" CSTR_BYTES "$p" 0 "${#CSTR_BYTES[@]}" || exit 1
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

/// The sqlite3 C API driven in memory: `_initialize`, `sqlite3_malloc` + pointer plumbing, open/exec/prepare/step/column/finalize/close.
/// `sqlite3_prepare_v2`'s -1 length is written as its masked-unsigned i32.
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
"${P}mem_i32_load" "$P" "$ppdb" || exit 1
db=$R0

capi_cstr "create table t(a,b); insert into t values (1,'x'),(2,'y');"
capi_call sqlite3_exec "$db" "$R0" 0 0 0
(( R0 == 0 )) || { echo "exec rc=$R0" >&2; exit 1; }

capi_call "$MALLOC" 4
ppstmt=$R0
capi_cstr 'select a*10, b from t order by a desc'
capi_call sqlite3_prepare_v2 "$db" "$R0" 4294967295 "$ppstmt" 0
(( R0 == 0 )) || { echo "prepare rc=$R0" >&2; exit 1; }
"${P}mem_i32_load" "$P" "$ppstmt" || exit 1
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

/// The sqlite3 C API against a file preopen: create+insert, close, reopen, select: the file lifecycle through the C API (same fs stack as the shell), leaving a nonzero DB file on the host.
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
  "${P}mem_i32_load" "$P" "$pp" || exit 1
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
"${P}mem_i32_load" "$P" "$ppstmt" || exit 1
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

/// Guest->host callback round trip: the committed `sqlite3-binding.wasm` exports `run_query`, which calls `sqlite3_exec` with a C callback forwarding each row to the *imported* `env.host_row`.
/// The glue provides `host_row` through the `IMPORTS` array (a void import, so it leaves `R0` empty) and collects the rows.
const BASH_SQLITE3_CALLBACK: &str = concat!(
    bash_capi_prelude!("sqlite3_binding_", "sqlite3_malloc"),
    r#"
ROWS=()
host_row() {
  local argc=$1 argv=$2 row='' i
  for (( i = 0; i < argc; i++ )); do
    "${P}mem_i32_load" "$P" $(( argv + i * 4 )) || return $?
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
"${P}mem_i32_load" "$P" "$ppdb" || exit 1
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

/// libpcap BPF filter compilation: drive `compile_filter` on "tcp port 80" (DLT_EN10MB, snaplen 65535), then walk the serialized program `[u32 bf_len][bf_len × {u16 code; u8 jt; u8 jf; u32 k}]` in guest memory, printing each instruction as `code jt jf k`.
/// The u16/u8 fields are composed from single memory bytes (little-endian), the u32s go through `mem_i32_load`.
const BASH_PCAP_COMPILE: &str = concat!(
    bash_capi_prelude!("libpcap_", "malloc"),
    r#"
libpcap_init || { echo "init failed" >&2; exit 1; }
capi_call '_initialize'

capi_cstr 'tcp port 80'
capi_call compile_filter "$R0" 1 65535
prog=$R0
(( prog )) || { echo "compile failed" >&2; exit 1; }
"${P}mem_i32_load" "$P" "$prog" || exit 1
n=$R0
declare -n mem=${P}mem
for (( i = 0; i < n; i++ )); do
  base=$(( prog + 4 + i * 8 ))
  code=$(( mem[$base] | mem[$(( base + 1 ))] << 8 ))
  jt=$(( mem[$(( base + 2 ))] ))
  jf=$(( mem[$(( base + 3 ))] ))
  "${P}mem_i32_load" "$P" $(( base + 4 )) || exit 1
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

/// zeroperl Perl-5.42 eval (issue #67): instantiate the reactor with a zero-returning `env.call_host_function` import stub (only invoked when the guest registers host callbacks, and this program registers none) and a `/dev/null` preopen (`zeroperl_init` returns 1 without it; the Bash runtime accepts a single-file preopen since issue #143), then `_initialize` → `zeroperl_init` → `malloc` + copy a Perl program into guest memory → `zeroperl_eval` → `zeroperl_flush`.
/// The guest program is a quoted heredoc, so its bytes are identical to the other backends'.
const BASH_ZEROPERL_EVAL: &str = concat!(
    r#"WASI_ARGS=(zeroperl)
WASI_ENV=()
WASI_DIRS=('/dev/null::/dev/null')
imp_call_host_function() { R0=0; return 0; }
declare -A IMPORTS=(['env.call_host_function']=imp_call_host_function)
"#,
    bash_capi_prelude!("zeroperl_", "malloc"),
    r#"
zeroperl_init || { echo "init failed" >&2; exit 1; }
capi_call '_initialize'
capi_call zeroperl_init
(( R0 == 0 )) || { echo "zeroperl_init rc=$R0" >&2; exit 1; }

PERL_PROG=$(cat <<'GUEST_PROGRAM'
my $s = "hello world 42";
if ($s =~ /(\w+)\s+(\w+)\s+(\d+)/) {
  printf("m=%s|%s|%d sum=%d\n", $1, $2, $3, $3 + 8);
}
GUEST_PROGRAM
)
capi_cstr "$PERL_PROG"
capi_call zeroperl_eval "$R0" 0 0 0
capi_call zeroperl_flush
"#
);

/// ExifTool on zeroperl (issue #70): the flattened `exiftool` CLI driver (`{cache}/exiftool-lib`, preopened at `/work`) run on the same `cache/zeroperl.wasm` reactor, whose SFS blob embeds the `Image::ExifTool` module tree.
/// Instantiated like [`BASH_ZEROPERL_EVAL`] plus the staged image at `/img`.
/// The guest driver snippet (identical bytes to the other backends') overrides `CORE::GLOBAL::exit` to a `die` so ExifTool's terminal `exit` unwinds back into `eval_pv` instead of tripping `proc_exit`, sets `@ARGV`/`$0`, and `do`es the script; `zeroperl_flush` then pushes ExifTool's buffered stdout out through fd 1.
const BASH_EXIFTOOL: &str = concat!(
    r#"WASI_ARGS=(zeroperl)
WASI_ENV=()
WASI_DIRS=('/dev/null::/dev/null' '{cache}/exiftool-lib::/work' '{scratch}::/img')
imp_call_host_function() { R0=0; return 0; }
declare -A IMPORTS=(['env.call_host_function']=imp_call_host_function)
"#,
    bash_capi_prelude!("zeroperl_", "malloc"),
    r#"
zeroperl_init || { echo "init failed" >&2; exit 1; }
capi_call '_initialize'
capi_call zeroperl_init
(( R0 == 0 )) || { echo "zeroperl_init rc=$R0" >&2; exit 1; }

PERL_DRIVER=$(cat <<'GUEST_PROGRAM'
BEGIN { *CORE::GLOBAL::exit = sub (;$) { die "zeroperl_exit\n" }; }
@ARGV = ('-S', '-Make', '-Model', '-DateTimeOriginal', '/img/exif_fixture.jpg');
$0 = '/work/exiftool';
do '/work/exiftool';
GUEST_PROGRAM
)
capi_cstr "$PERL_DRIVER"
capi_call zeroperl_eval "$R0" 0 0 0
capi_call zeroperl_flush
"#
);

/// DOOM: the frame snapshot, modelled on the Bash frontend (examples/doom/bash/main.sh): imp_* handlers set `R0`, `IMPORTS[mod.name]` wires them, `doom_init`/`doom_invoke` drive, and `doom_mem` holds the pixels.
/// The P6 frame goes out through a chunked `printf` `\xNN` format string (which carries the NUL bytes a Bash variable cannot).
/// `{ticks}`/`{clock_step}` filled by the runner.
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
/// `allocRom`'s buffer via `nes_mem_init`, tick `{frames}` times with no input, compose the frame from agnes's palette-index screen buffer against a 64-entry
/// `\xNN\xNN\xNN` lookup table built once (issue #117; the `& 0x3f` mask is load-bearing), and dump it through the same chunked `printf` pipeline as
/// DOOM.
/// `{rom}` (the cached ROM's host path) and `{frames}` filled by the runner.
/// The trailing `exit 0` matters here in a way it doesn't for DOOM: at
/// 256x240 the pixel count divides the 4096-pixel flush chunk exactly, so the final `[[ -n $fmt ]]` is false and would otherwise leave the script's status at 1 despite a byte-correct frame on stdout.
const BASH_NES_FRAME_GLUE: &str = r#"mapfile -t ROM_BYTES < <(od -An -v -tu1 "{rom}" | tr -s ' \n' '\n' | sed '/^$/d')

nes_init || { echo "nes_init failed" >&2; exit 1; }
rom_len=${#ROM_BYTES[@]}
nes_invoke allocRom "$rom_len" || { echo "allocRom failed: ${TRAP_MSG-}" >&2; exit 1; }
rom_ptr=$R0
nes_mem_init nes_ ROM_BYTES "$rom_ptr" 0 "$rom_len" || { echo "mem_init failed: ${TRAP_MSG-}" >&2; exit 1; }
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

dewasm_test_helper::library_add_e2e!(Bash, BASH_ADD_GLUE);
dewasm_test_helper::wasi_import_override_e2e!(Bash, BASH_OVERRIDE_GLUE);
dewasm_test_helper::stdio_capture_e2e!(Bash, BASH_STDIO_CAPTURE_GLUE);
dewasm_test_helper::custom_wasi_provider_e2e!(Bash, BASH_CUSTOM_PROVIDER_GLUE);
dewasm_test_helper::partial_override_e2e!(Bash, BASH_PARTIAL_OVERRIDE_GLUE);

dewasm_test_helper::wasi_suite!(Bash, Stdio);
dewasm_test_helper::wasi_suite!(Bash, ArgsEnv);
dewasm_test_helper::wasi_suite!(Bash, Poll);
dewasm_test_helper::wasi_suite!(Bash, Fs, BASH_FS_GLUE);
dewasm_test_helper::wasi_root_containment_e2e!(Bash, BASH_CONTAINMENT_GLUE);
dewasm_test_helper::standalone_dir_e2e!(Bash);
// The standalone entrypoint already raises the process stack rlimit before running any guest code; that covers 5000 guest frames too.
dewasm_test_helper::deep_recursion_e2e!(Bash);
dewasm_test_helper::folded_temp_reuse_e2e!(Bash);

dewasm_test_helper::cowsay_args_e2e!(Bash);
dewasm_test_helper::cowsay_stdin_e2e!(Bash);
// qjs_eval_e2e! / sqlite3_shell_e2e!: invoked, but slow: Bash's softfloat makes QuickJS/SQLite take tens of seconds, so the generated tests are `#[ignore]`d by default; `--features slow_test` runs them anyway (same as every other backend).
dewasm_test_helper::qjs_eval_e2e!(Bash);
dewasm_test_helper::sqlite3_shell_e2e!(Bash);
// minigzip is integer-only (no softfloat), so it runs under Bash by default, unlike the slow floating-point apps (QuickJS/SQLite).
dewasm_test_helper::gzip_e2e!(Bash);

// Filesystem app cases: Bash's WASI filesystem now covers preopens, path_open, and positioned I/O, so the small-fixture fs apps are wired (all slow, softfloat-bound QuickJS/SQLite, see qjs_eval_e2e! above).
dewasm_test_helper::qjs_file_io_e2e!(Bash, BASH_QJS_FILE_IO_GLUE);
dewasm_test_helper::sqlite3_shell_dbfile_e2e!(Bash, BASH_SQLITE3_SHELL_DBFILE_GLUE);
// ripgrep: an 18 MB wasm becomes a 70 MB script, which bash parses in 1.7 s and then walks the fixture tree in the rest of a 22 s run: the slowest of the Bash `slow` cases but the same cluster as qjs_eval (13.6 s) and sqlite3_shell_dbfile (10.0 s), not the ~1-minute `ultra` line.
dewasm_test_helper::rg_search_e2e!(Bash, BASH_RG_SEARCH_GLUE);
// qjs_repl_pty is wired here because it shares the filesystem cases' standalone QuickJS conversion, though it has no preopens.
// Ultra: every keystroke re-enters QuickJS's interactive line editor, and each successive evaluation is slower than the last (~135s to the first prompt, ~330s for `[3,1,2].sort()`), which exceeds the shared 180s per-prompt `PTY_TIMEOUT` and timed out on CI (#22).
dewasm_test_helper::qjs_repl_pty_e2e!(Bash, ultra);
// The two language-runtime giants and the packed CRuby run under Bash too (issue #143), all three ultra: CPython's 30 MB wasm becomes a 226 MB script whose one-liner measured ~9.5 min, and CRuby's measured 71 min: a single arithmetic op costs a bash function call under Bash's softfloat.
// The wasi-vfs-packed CRuby serves its stdlib from guest memory, so it needs no preopens.
// All three run at `slow` on the other backends, so the apps stay CI-covered.
dewasm_test_helper::cpython_hello_e2e!(Bash, BASH_CPYTHON_GLUE, ultra);
dewasm_test_helper::cruby_hello_e2e!(Bash, BASH_CRUBY_GLUE, ultra);
dewasm_test_helper::cruby_packed_hello_e2e!(Bash, ultra);
// Ultra: interpreting the cowsay guest through the converted interpreter measured 697 s, the same order as the language-runtime giants above (an interpreter's dispatch loop is one bash function call per executed guest instruction).
// It runs at `slow` on every other backend, so the case itself stays CI-covered.
dewasm_test_helper::toywasm_cowsay_e2e!(Bash, BASH_TOYWASM_GLUE, ultra);

dewasm_test_helper::doom_frame_e2e!(Bash, BASH_DOOM_FRAME_GLUE, ultra);
// Ultra-slow category: tens of seconds per tick locally (mem_init's own copy loop over the 41 KB ROM, then agnes's per-frame interpretation), ~20 min for the full 40-frame run, well past the ~1-minute CI-runner line, like the DOOM case above. (It was ~25 min before issue #117 moved the per-pixel frame composition out of the guest.)
dewasm_test_helper::nes_frame_e2e!(Bash, BASH_NES_FRAME_GLUE, ultra);

dewasm_test_helper::shared_table_e2e!(Bash, BASH_SHARED_TABLE_GLUE);
// The C-API cases run under Bash too: a pointer is a decimal in `R0` and guest memory is the `${P}mem` array, which is all the plumbing these drives need, and the NES glue above already does the same in the other direction.
// The artifacts are within reach as well (41.6 MB libsqlite3, 18.1 MB libpcap, 3.0 MB tree-sitter scripts, all at or below the 45 MB sqlite3-shell script Bash already runs), and all five stay in the `slow` category: 0.7 s tree-sitter, 1.4 s libpcap, 4.0/4.2 s the in-memory sqlite drives, 7.7 s the file-backed one.
dewasm_test_helper::libsqlite3_c_api_e2e!(Bash, BASH_LIBSQLITE3_MEM);
dewasm_test_helper::sqlite3_file_c_api_e2e!(Bash, BASH_LIBSQLITE3_FILE);
dewasm_test_helper::sqlite3_callback_binding_e2e!(Bash, BASH_SQLITE3_CALLBACK);
dewasm_test_helper::pcap_compile_e2e!(Bash, BASH_PCAP_COMPILE);
dewasm_test_helper::treesitter_parse_e2e!(Bash, BASH_TREESITTER_PARSE);
// The two zeroperl reactor cases (issue #143) are ultra instead.
// The 25 MB reactor becomes a 229 MB script whose module init alone (the SFS blob carrying the whole Perl core into `zeroperl_mem`) runs 1 m 39 s at ~6 GB of host RSS; the eval case measured 1 m 55 s end to end.
// ExifTool has no completion evidence at all: a hand-run was still inside `zeroperl_eval` at 69 minutes when it was cut, so its wall time is unknown rather than merely long.
// Ruby covers it in CI at `slow`.
dewasm_test_helper::zeroperl_eval_e2e!(Bash, BASH_ZEROPERL_EVAL, ultra);
dewasm_test_helper::exiftool_extract_e2e!(Bash, BASH_EXIFTOOL, ultra);
dewasm_test_helper::embedded_coexist_e2e!(Bash, BASH_EMBEDDED_COEXIST_GLUE);
