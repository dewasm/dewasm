//! Python end-to-end suites (ADR-27): the shared case tables (`dewasm-test-helper`)
//! wired up for the Python backend. Per the ADR-27 revision this file holds ONLY
//! the [`BackendUnderTest`] impl, glue strings / glue-producing functions, and
//! macro invocations. Python covers full WASI preview 1 incl. the filesystem
//! (ADR-28), so it wires every WASI kind, the heavy `apps`/`fs_apps`/`capi_apps`
//! suites, and the shared-table multi-module case (`embedded_runtimes_coexist`
//! is excluded — Python's library Embedded output uses one top-level `Rt`).

use std::path::{Path, PathBuf};

use dewasm_backend::{Backend, Mode, RuntimeLinkage};
use dewasm_backend_python::{find_python, PythonBackend};
use dewasm_test_helper::{
    apps_e2e, capi_apps_e2e, convert, examples_dir, fs_apps_e2e, gzip_e2e, library_e2e,
    multi_module_e2e, qjs_repl_pty_e2e, standalone_e2e, wasi_suite, BackendUnderTest, CApiCase,
    LibraryCase, MultiModuleCase, WasiCase,
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

    fn run_heavy_apps(&self) -> bool {
        true
    }

    /// Instantiate `class` with kwargs (args/env/preopens), run `_start`, and
    /// swallow a clean guest `proc_exit` (`Rt.Exit`).
    fn app_glue(
        &self,
        class: &str,
        args: &[&str],
        env: &[(&str, &str)],
        preopens: &[(&str, &Path)],
    ) -> String {
        let argv = args
            .iter()
            .map(|a| format!("{a:?}"))
            .collect::<Vec<_>>()
            .join(", ");
        let envs = env
            .iter()
            .map(|(k, v)| format!("{k:?}: {v:?}"))
            .collect::<Vec<_>>()
            .join(", ");
        let pres = preopens
            .iter()
            .map(|(guest, host)| format!("{guest:?}: {:?}", host.to_string_lossy()))
            .collect::<Vec<_>>()
            .join(", ");
        format!(
            "inst = {class}({{}}, args=[{argv}], env={{{envs}}}, preopens={{{pres}}})\n\
             try:\n    inst.invoke(\"_start\")\nexcept Rt.Exit:\n    pass\n"
        )
    }

    /// Compose several `.wat` modules. `shared_runtime` emits each against one
    /// top-level `class Rt:` (Alias linkage) plus a single bundled runtime, so
    /// an imported table crosses modules; otherwise it concatenates independent
    /// Embedded conversions (only used by cases Python is not excluded from).
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

/// Per-case Python glue. A case Python is wired to run but has no glue for
/// panics loudly (ADR-15).
fn python_glue(case: &LibraryCase) -> &'static str {
    match case.name {
        "add" => {
            "inst = Add()\n\
             print(inst.invoke(\"add\", 2, 3))\n\
             print(inst.invoke(\"add\", 0xffffffff, 1))\n\
             print(inst.invoke(\"fib\", 10))\n"
        }
        "wasi_import_override" => PYTHON_OVERRIDE_GLUE,
        "custom_wasi_provider" => PYTHON_CUSTOM_PROVIDER_GLUE,
        "partial_override_falls_back_to_bundled_wasi" => PYTHON_PARTIAL_OVERRIDE_GLUE,
        "wasi_stdio_capture" => PYTHON_STDIO_CAPTURE_GLUE,
        other => panic!("{other}: no python glue"),
    }
}

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

/// Instantiate a fs fixture with the scratch dir preopened at guest `/`, run
/// `_start`, and surface a `proc_exit` code as a trailing decimal line. The
/// `fs_root_preopen_containment` case instead probes the WASI resolver with a
/// `"/" => "/"` preopen (no guest run).
fn python_fs_glue(case: &WasiCase, host: &Path) -> String {
    if case.name == "fs_root_preopen_containment" {
        return "wasi = Rt.WASI(preopens={\"/\": \"/\"})\n\
                _path, err = wasi.resolve_path(3, \"etc\")\n\
                print(\"contained\" if err is None else \"rejected\")\n"
            .to_string();
    }
    format!(
        "inst = Prog({{}}, preopens={{{:?}: {:?}}})\n\
         try:\n    inst.invoke(\"_start\")\nexcept Rt.Exit as e:\n    print(e.code)\n",
        "/",
        host.to_string_lossy()
    )
}

// ---------------------------------------------------------------------
// C-API drive glue (sqlite3): malloc/pointer plumbing via Rt.Memory.

fn python_capi_glue(case: &CApiCase, scratch: &Path) -> String {
    match case.name {
        "libsqlite3_c_api" => PYTHON_LIBSQLITE3_MEM.replace("__CLASS__", case.class),
        "sqlite3_file_c_api" => PYTHON_LIBSQLITE3_FILE
            .replace("__CLASS__", case.class)
            .replace("__DB__", &scratch.to_string_lossy()),
        "sqlite3_callback_binding" => PYTHON_SQLITE3_CALLBACK.replace("__CLASS__", case.class),
        other => panic!("{other}: no python capi glue"),
    }
}

const PYTHON_LIBSQLITE3_MEM: &str = r#"
db_mod = __CLASS__({})
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
db_mod = __CLASS__({}, preopens={"/db": "__DB__"})
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


db_mod = __CLASS__({"env": {"host_row": host_row}})
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

fn python_multi_module_glue(case: &MultiModuleCase) -> &'static str {
    match case.name {
        "shared_table_call_indirect" => {
            "a = TableExp()\n\
             b = TableImp({\"a\": a})\n\
             print(b.invoke(\"call0\"))\n"
        }
        other => panic!("{other}: no python multi-module glue"),
    }
}

// ---------------------------------------------------------------------
// Suite wiring (ADR-27).

standalone_e2e!(Python);
library_e2e!(Python, python_glue);
wasi_suite!(Python, Stdio);
wasi_suite!(Python, ArgsEnv);
wasi_suite!(Python, Poll);
wasi_suite!(Python, Fs, python_fs_glue);
apps_e2e!(Python);
gzip_e2e!(Python);
fs_apps_e2e!(Python);
qjs_repl_pty_e2e!(Python);
capi_apps_e2e!(Python, python_capi_glue);
multi_module_e2e!(Python, python_multi_module_glue);
