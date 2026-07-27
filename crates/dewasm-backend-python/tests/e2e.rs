//! Python end-to-end suites (ADR-27): the shared case consts (`dewasm-test-helper`)
//! wired up for the Python backend. Per the ADR-27 revision this file holds ONLY
//! the [`BackendUnderTest`] impl, named glue string constants, and per-case
//! macro invocations. Python covers full WASI preview 1 incl. the filesystem
//! (ADR-28), so it wires every WASI kind, the heavy `apps`/`fs_apps`/`capi`
//! suites, and the shared-table multi-module case.

use std::path::PathBuf;

use dewasm_backend::{Backend, Mode, RuntimeLinkage};
use dewasm_backend_python::{find_python, PythonBackend};
use dewasm_test_helper::{
    convert, cowsay_args_e2e, cowsay_stdin_e2e, cpython_hello_e2e, cruby_hello_e2e,
    custom_wasi_provider_e2e, examples_dir, gzip_e2e, library_add_e2e, libsqlite3_c_api_e2e,
    partial_override_e2e, qjs_eval_e2e, qjs_file_io_e2e, qjs_repl_e2e, qjs_repl_pty_e2e,
    rg_search_e2e, shared_table_e2e, sqlite3_callback_binding_e2e, sqlite3_file_c_api_e2e,
    sqlite3_shell_dbfile_e2e, sqlite3_shell_e2e, standalone_dir_e2e, stdio_capture_e2e,
    wasi_import_override_e2e, wasi_root_containment_e2e, wasi_suite, BackendUnderTest,
};

pub struct Python;

impl BackendUnderTest for Python {
    fn name(&self) -> &'static str {
        "python"
    }

    fn backend(&self) -> &'static (dyn Backend + Sync) {
        &PythonBackend
    }

    fn interpreter(&self) -> PathBuf {
        find_python().expect("python3 >= 3.9 not found on PATH — see docs/testing.md")
    }

    /// Compose several `.wat` modules. `shared_runtime` emits each against one
    /// top-level `class Rt:` (Alias linkage) plus a single bundled runtime, so
    /// an imported table crosses modules; otherwise it concatenates independent
    /// Embedded conversions (only used by cases Python invokes).
    fn compose_modules(&self, modules: &[(&str, &str)], shared_runtime: bool) -> String {
        if shared_runtime {
            let mut units = std::collections::BTreeSet::new();
            let mut classes = Vec::new();
            for (wat, name) in modules {
                let bytes = wat::parse_file(examples_dir().join(wat)).expect("parse wat");
                let module = dewasm_core::build_module(&bytes).expect("build IR");
                let (src, u) = dewasm_backend_python::generate_class_with_units(
                    &module,
                    name,
                    &RuntimeLinkage::Alias("Rt".to_string()),
                    false,
                )
                .expect("generate");
                units.extend(u);
                classes.push(src);
            }
            format!(
                "{}\n{}",
                dewasm_backend_python::shared_runtime(&units).expect("bundle runtime"),
                classes.join("\n")
            )
        } else {
            modules
                .iter()
                .map(|(wat, name)| {
                    convert(
                        &PythonBackend,
                        &examples_dir().join(wat),
                        Mode::Library,
                        name,
                    )
                })
                .collect::<Vec<_>>()
                .join("\n")
        }
    }
}

// ---------------------------------------------------------------------
// Library-case glue.

/// `add.wat`: call the exported functions and print each result.
const PYTHON_ADD_GLUE: &str = r#"inst = Add()
print(inst.invoke("add", 2, 3))
print(inst.invoke("add", 0xffffffff, 1))
print(inst.invoke("fib", 10))
"#;

/// The ADR-7 override/fallback glue: fd_write intercepted, random_get falls
/// back to the bundled WASI. Prints the actual bytes written.
const PYTHON_OVERRIDE_GLUE: &str = r#"import sys
_captured = bytearray()
_holder = {}


def _fd_write(fd, iovs, iovs_len, out_ptr):
    mem = _holder["inst"].memory
    ptr = mem.i32_load(iovs)
    length = mem.i32_load(iovs + 4)
    _captured.extend(mem.read_string(ptr, length))
    mem.i32_store(out_ptr, length)
    return 0


inst = Prog({"wasi_snapshot_preview1": {"fd_write": _fd_write}})
_holder["inst"] = inst
inst.invoke("_start")  # random_get falls back to the bundled WASI
sys.stdout.write(_captured.decode("utf-8", "surrogateescape"))
"#;

/// The `custom_wasi_provider` glue: a provider *object* (`wasm_import`/`attach`,
/// the Python analog of Ruby's duck-typed provider) covers every import, so the
/// bundled WASI (`_wasi`) is never lazily constructed.
const PYTHON_CUSTOM_PROVIDER_GLUE: &str = r#"import sys


class MyWasi:
    def __init__(self):
        self.out = bytearray()

    def wasm_import(self, name):
        if name == "fd_write":
            return self._fd_write
        if name == "random_get":
            return lambda buf, ln: 0
        return None

    def attach(self, instance):
        self.memory = instance.memory

    def _fd_write(self, fd, iovs, iovs_len, out_ptr):
        ptr = self.memory.i32_load(iovs)
        length = self.memory.i32_load(iovs + 4)
        self.out.extend(self.memory.read_string(ptr, length))
        self.memory.i32_store(out_ptr, length)
        return 0


wasi = MyWasi()
inst = Prog({"wasi_snapshot_preview1": wasi})
inst.invoke("_start")
sys.stdout.write(wasi.out.decode("utf-8", "surrogateescape"))
print("bundled wasi constructed:", "true" if inst._wasi is not None else "false")
"#;

/// The `partial_override_falls_back_to_bundled_wasi` glue: fd_write intercepted,
/// random_get falls back — so the bundled WASI *was* lazily constructed.
const PYTHON_PARTIAL_OVERRIDE_GLUE: &str = r#"import sys
_captured = bytearray()
_holder = {}


def _fd_write(fd, iovs, iovs_len, out_ptr):
    mem = _holder["inst"].memory
    ptr = mem.i32_load(iovs)
    length = mem.i32_load(iovs + 4)
    _captured.extend(mem.read_string(ptr, length))
    mem.i32_store(out_ptr, length)
    return 0


inst = Prog({"wasi_snapshot_preview1": {"fd_write": _fd_write}})
_holder["inst"] = inst
inst.invoke("_start")  # random_get falls back to the bundled WASI
sys.stdout.write(_captured.decode("utf-8", "surrogateescape"))
print("bundled wasi constructed:", "true" if inst._wasi is not None else "false")
"#;

/// The `wasi_stdio_capture` glue: redirect `sys.stdout` (whose `.buffer` the
/// bundled WASI captures on lazy construction) to a `BytesIO`, run, then print
/// the captured bytes to the real stdout — the Python mirror of Ruby's StringIO
/// idiom.
const PYTHON_STDIO_CAPTURE_GLUE: &str = r#"import io
import sys

_buf = io.BytesIO()
_orig = sys.stdout
sys.stdout = io.TextIOWrapper(_buf, write_through=True)
try:
    inst = Prog({})
    inst.invoke("_start")
except Rt.Exit:
    pass
finally:
    sys.stdout.flush()
    # Read the captured bytes before restoring: reassigning sys.stdout drops
    # the TextIOWrapper, which closes the underlying BytesIO on finalization.
    _data = _buf.getvalue()
    sys.stdout = _orig
sys.stdout.buffer.write(_data)
sys.stdout.flush()
"#;

// ---------------------------------------------------------------------
// WASI filesystem glue.

/// The shared filesystem template: preopen the scratch dir (`{host}`) at guest
/// `{guest}` (always `/`), run `_start`, and surface a `proc_exit` code as a
/// trailing decimal line.
const PYTHON_FS_GLUE: &str = r#"inst = Prog({}, preopens={"{guest}": "{host}"})
try:
    inst.invoke("_start")
except Rt.Exit as e:
    print(e.code)
"#;

/// The root-preopen containment probe: call the WASI resolver directly with a
/// `"/" => "/"` preopen (no guest run) and normalize the outcome to `contained`.
const PYTHON_CONTAINMENT_GLUE: &str = r#"wasi = Rt.WASI(preopens={"/": "/"})
_path, err = wasi.resolve_path(3, "etc")
print("contained" if err is None else "rejected")
"#;

// ---------------------------------------------------------------------
// Filesystem app glue: class/argv/env/preopen-guest-paths are literals; only
// the host scratch/cache dirs come through {scratch}/{cache}.

const PYTHON_QJS_FILE_IO_GLUE: &str = r#"inst = Qjs({}, args=["qjs", "/work/qjs_file_io.js"], env={}, preopens={"/work": "{scratch}"})
try:
    inst.invoke("_start")
except Rt.Exit:
    pass
"#;

const PYTHON_QJS_REPL_GLUE: &str = r#"inst = Qjs({}, args=["qjs", "/work/qjs_repl.js"], env={}, preopens={"/work": "{scratch}"})
try:
    inst.invoke("_start")
except Rt.Exit:
    pass
"#;

const PYTHON_SQLITE3_SHELL_GLUE: &str = r#"inst = Sqlite3Shell({}, args=["sqlite3"], env={}, preopens={"/db": "{scratch}"})
try:
    inst.invoke("_start")
except Rt.Exit:
    pass
"#;

const PYTHON_RG_SEARCH_GLUE: &str = r#"inst = Rg({}, args=["rg", "--sort", "path", "needle", "/work"], env={}, preopens={"/work": "{scratch}"})
try:
    inst.invoke("_start")
except Rt.Exit:
    pass
"#;

const PYTHON_CPYTHON_GLUE: &str = r#"inst = Cpython({}, args=["python", "-c", "print('hello from cpython', 6 * 7)"], env={"PYTHONHOME": "/", "PYTHONPATH": "/lib/python3.14"}, preopens={"/lib": "{cache}/cpython-lib/lib"})
try:
    inst.invoke("_start")
except Rt.Exit:
    pass
"#;

const PYTHON_CRUBY_GLUE: &str = r#"inst = Cruby({}, args=["ruby", "-e", "puts \"hello from cruby #{6*7}\""], env={}, preopens={"/usr": "{cache}/ruby-lib/usr"})
try:
    inst.invoke("_start")
except Rt.Exit:
    pass
"#;

// ---------------------------------------------------------------------
// C-API drive glue (sqlite3): malloc/pointer plumbing via Rt.Memory. Only the
// file-backed case uses {scratch}.

const PYTHON_LIBSQLITE3_MEM: &str = r#"
db_mod = Libsqlite3({})
db_mod.invoke("_initialize")
mem = db_mod.memory


def read_cstr(ptr):
    if ptr == 0:
        return None
    end = mem.data.index(0, ptr)
    return mem.read_string(ptr, end - ptr).decode("utf-8")


def cstr(s):
    b = s.encode("utf-8") + b"\x00"
    p = db_mod.invoke("sqlite3_malloc", len(b))
    mem.init(p, b, 0, len(b))
    return p


print("version: " + read_cstr(db_mod.invoke("sqlite3_libversion")))

pp_db = db_mod.invoke("sqlite3_malloc", 4)
rc = db_mod.invoke("sqlite3_open", cstr(":memory:"), pp_db)
assert rc == 0, "open rc=%d" % rc
db = mem.i32_load(pp_db)

rc = db_mod.invoke("sqlite3_exec", db, cstr("create table t(a,b); insert into t values (1,'x'),(2,'y');"), 0, 0, 0)
assert rc == 0, "exec rc=%d: %s" % (rc, read_cstr(db_mod.invoke("sqlite3_errmsg", db)))

pp_stmt = db_mod.invoke("sqlite3_malloc", 4)
rc = db_mod.invoke("sqlite3_prepare_v2", db, cstr("select a*10, b from t order by a desc"), 0xffffffff, pp_stmt, 0)
assert rc == 0, "prepare rc=%d" % rc
stmt = mem.i32_load(pp_stmt)

while db_mod.invoke("sqlite3_step", stmt) == 100:
    n = db_mod.invoke("sqlite3_column_count", stmt)
    row = [read_cstr(db_mod.invoke("sqlite3_column_text", stmt, i)) for i in range(n)]
    print("|".join(row))
db_mod.invoke("sqlite3_finalize", stmt)
db_mod.invoke("sqlite3_close", db)
print("C-API-OK")
"#;

const PYTHON_LIBSQLITE3_FILE: &str = r#"
db_mod = Libsqlite3({}, preopens={"/db": "{scratch}"})
db_mod.invoke("_initialize")
mem = db_mod.memory


def read_cstr(ptr):
    if ptr == 0:
        return None
    end = mem.data.index(0, ptr)
    return mem.read_string(ptr, end - ptr).decode("utf-8")


def cstr(s):
    b = s.encode("utf-8") + b"\x00"
    p = db_mod.invoke("sqlite3_malloc", len(b))
    mem.init(p, b, 0, len(b))
    return p


def open_db(path):
    pp = db_mod.invoke("sqlite3_malloc", 4)
    rc = db_mod.invoke("sqlite3_open", cstr(path), pp)
    assert rc == 0, "open rc=%d" % rc
    return mem.i32_load(pp)


db = open_db("/db/data.db")
rc = db_mod.invoke("sqlite3_exec", db, cstr("create table t(a,b); insert into t values (1,'x'),(2,'y');"), 0, 0, 0)
assert rc == 0, "exec rc=%d: %s" % (rc, read_cstr(db_mod.invoke("sqlite3_errmsg", db)))
db_mod.invoke("sqlite3_close", db)

db = open_db("/db/data.db")
pp_stmt = db_mod.invoke("sqlite3_malloc", 4)
rc = db_mod.invoke("sqlite3_prepare_v2", db, cstr("select a*10, b from t order by a"), 0xffffffff, pp_stmt, 0)
assert rc == 0, "prepare rc=%d" % rc
stmt = mem.i32_load(pp_stmt)
while db_mod.invoke("sqlite3_step", stmt) == 100:
    n = db_mod.invoke("sqlite3_column_count", stmt)
    row = [read_cstr(db_mod.invoke("sqlite3_column_text", stmt, i)) for i in range(n)]
    print("|".join(row))
db_mod.invoke("sqlite3_finalize", stmt)
db_mod.invoke("sqlite3_close", db)
print("FILE-OK")
"#;

const PYTHON_SQLITE3_CALLBACK: &str = r#"
ROWS = []
_holder = {}


def host_row(argc, argv_ptr):
    mem = _holder["mem"]
    row = []
    for i in range(argc):
        p = mem.i32_load(argv_ptr + i * 4)
        if p == 0:
            row.append(None)
        else:
            end = mem.data.index(0, p)
            row.append(mem.read_string(p, end - p).decode("utf-8"))
    ROWS.append(row)
    return 0


db_mod = Sqlite3Binding({"env": {"host_row": host_row}})
db_mod.invoke("_initialize")
mem = db_mod.memory
_holder["mem"] = mem


def read_cstr(ptr):
    if ptr == 0:
        return None
    end = mem.data.index(0, ptr)
    return mem.read_string(ptr, end - ptr).decode("utf-8")


def cstr(s):
    b = s.encode("utf-8") + b"\x00"
    p = db_mod.invoke("sqlite3_malloc", len(b))
    mem.init(p, b, 0, len(b))
    return p


pp_db = db_mod.invoke("sqlite3_malloc", 4)
rc = db_mod.invoke("sqlite3_open", cstr(":memory:"), pp_db)
assert rc == 0, "open rc=%d" % rc
db = mem.i32_load(pp_db)

rc = db_mod.invoke("sqlite3_exec", db, cstr("create table t(a,b); insert into t values (1,'x'),(2,'y'),(3,'z');"), 0, 0, 0)
assert rc == 0, "exec rc=%d: %s" % (rc, read_cstr(db_mod.invoke("sqlite3_errmsg", db)))

rc = db_mod.invoke("run_query", db, cstr("select a, b from t where a >= 2 order by a"))
assert rc == 0, "run_query rc=%d" % rc
db_mod.invoke("sqlite3_close", db)

for r in ROWS:
    print("row: " + "|".join(r))
print("CALLBACK-OK")
"#;

// ---------------------------------------------------------------------
// Multi-module drive glue.

/// Driver for the shared-table case: instantiate the exporter and the importer
/// linked against it, then print `call0` (call_indirect through the shared
/// table -> 42).
const PYTHON_SHARED_TABLE_GLUE: &str = r#"a = TableExp()
b = TableImp({"a": a})
print(b.invoke("call0"))
"#;

// ---------------------------------------------------------------------
// Suite wiring (ADR-27): each per-case macro invocation declares participation.

library_add_e2e!(Python, PYTHON_ADD_GLUE);
wasi_import_override_e2e!(Python, PYTHON_OVERRIDE_GLUE);
custom_wasi_provider_e2e!(Python, PYTHON_CUSTOM_PROVIDER_GLUE);
partial_override_e2e!(Python, PYTHON_PARTIAL_OVERRIDE_GLUE);
stdio_capture_e2e!(Python, PYTHON_STDIO_CAPTURE_GLUE);

wasi_suite!(Python, Stdio);
wasi_suite!(Python, ArgsEnv);
wasi_suite!(Python, Poll);
wasi_suite!(Python, Fs, PYTHON_FS_GLUE);
wasi_root_containment_e2e!(Python, PYTHON_CONTAINMENT_GLUE);
standalone_dir_e2e!(Python);

cowsay_args_e2e!(Python);
cowsay_stdin_e2e!(Python);
qjs_eval_e2e!(Python);
sqlite3_shell_e2e!(Python);
gzip_e2e!(Python);

qjs_file_io_e2e!(Python, PYTHON_QJS_FILE_IO_GLUE);
qjs_repl_e2e!(Python, PYTHON_QJS_REPL_GLUE);
sqlite3_shell_dbfile_e2e!(Python, PYTHON_SQLITE3_SHELL_GLUE);
rg_search_e2e!(Python, PYTHON_RG_SEARCH_GLUE);
cpython_hello_e2e!(Python, PYTHON_CPYTHON_GLUE);
cruby_hello_e2e!(Python, PYTHON_CRUBY_GLUE);
qjs_repl_pty_e2e!(Python);

libsqlite3_c_api_e2e!(Python, PYTHON_LIBSQLITE3_MEM);
sqlite3_file_c_api_e2e!(Python, PYTHON_LIBSQLITE3_FILE);
sqlite3_callback_binding_e2e!(Python, PYTHON_SQLITE3_CALLBACK);

shared_table_e2e!(Python, PYTHON_SHARED_TABLE_GLUE);
// embedded_coexist_e2e!: not invoked — Python's library Embedded output emits
// one top-level `class Rt:` (a sibling, redefined on concatenation), not a
// per-class nested runtime, so two independent runtimes cannot coexist
// (docs/apps-audit.md).
