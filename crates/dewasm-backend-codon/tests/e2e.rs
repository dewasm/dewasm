//! Codon end-to-end suites: the shared case consts (`dewasm-test-helper`) wired up for the Codon backend.
//! This file holds ONLY the [`BackendUnderTest`] impl, named glue string constants, and per-case macro invocations.
//! Codon covers full WASI preview 1 incl. the filesystem, exception handling, and tail calls, so it wires every WASI kind, the `apps`/`fs_apps`/`capi` suites, and both multi-module cases.
//!
//! Every codon build in the test suites is debug (see tests/common: ~8x faster, semantics preserved by the emission-level NaN quieting).

use std::path::Path;
use std::process::{Command, Output};
use std::sync::LazyLock;

use dewasm_backend::{Backend, RuntimeLinkage};
use dewasm_backend_codon::{find_codon, CodonBackend};
use dewasm_test_helper::BackendUnderTest;

mod common;

struct Codon;

impl BackendUnderTest for Codon {
    fn name(&self) -> &'static str {
        "codon"
    }

    fn backend(&self) -> &'static (dyn Backend + Sync) {
        &CodonBackend
    }

    /// Compile `source` to the crate's shared cache binary (debug; see the module docs) and run it with the Codon runtime dylibs beside it.
    fn run_bytes(&self, source: &str, args: &[&str], stdin: &[u8]) -> Output {
        match common::build_codon(source) {
            Err(build) => build,
            Ok(bin) => common::run_codon_binary(&bin, args, stdin),
        }
    }

    /// Build `source` (debug) and return the run recipe for a pty (the QuickJS REPL case).
    fn pty_command(&self, source: &str, args: &[&str]) -> dewasm_test_helper::PtyCommand {
        let bin = common::build_codon(source).unwrap_or_else(|build| {
            panic!(
                "codon build failed:\n{}",
                String::from_utf8_lossy(&build.stderr)
            )
        });
        dewasm_test_helper::PtyCommand {
            program: bin,
            args: args.iter().map(|a| a.to_string()).collect(),
            cwd: None,
        }
    }

    fn compose_modules(
        &self,
        dir: &Path,
        modules: &[(&str, &str)],
        shared_runtime: bool,
    ) -> String {
        let mut imports = Vec::new();
        if shared_runtime {
            let mut units = std::collections::BTreeSet::new();
            let mut classes = Vec::new();
            for (wat, name) in modules {
                let bytes = wat::parse_file(dewasm_test_helper::examples_dir().join(wat))
                    .expect("parse wat");
                let module = dewasm_core::build_module(&bytes).expect("build IR");
                let (src, u) = dewasm_backend_codon::generate_class_with_units(
                    &module,
                    name,
                    &RuntimeLinkage::Alias("Rt".to_string()),
                    false,
                )
                .expect("generate");
                units.extend(u);
                classes.push((name.to_lowercase(), *name, src));
            }
            std::fs::write(
                dir.join("rt.codon"),
                dewasm_backend_codon::shared_runtime(&units).expect("bundle runtime"),
            )
            .unwrap();
            imports.push("from rt import Rt".to_string());
            for (stem, _name, src) in classes {
                std::fs::write(
                    dir.join(format!("{stem}.codon")),
                    format!("from rt import Rt\n\n{src}"),
                )
                .unwrap();
                // The wrapper classes the boxed exports reference live next to the generated class; a star import brings them along with it.
                imports.push(format!("from {stem} import *"));
            }
        } else {
            for (wat, name) in modules {
                let stem = name.to_lowercase();
                let bytes = wat::parse_file(dewasm_test_helper::examples_dir().join(wat))
                    .expect("parse wat");
                let module = dewasm_core::build_module(&bytes).expect("build IR");
                let (src, _) = dewasm_backend_codon::generate_class_with_units(
                    &module,
                    name,
                    &RuntimeLinkage::Embedded,
                    false,
                )
                .expect("generate");
                std::fs::write(dir.join(format!("{stem}.codon")), src).unwrap();
                imports.push(format!("from {stem} import *"));
            }
        }
        imports.join("\n")
    }

    /// `codon run` executes in-process, so no dylib copies or prebuilt binary are needed; the working directory holds the module files the driver imports.
    /// Debug like every other suite build (tests/common/mod.rs).
    fn run_in_dir(&self, dir: &Path, driver: &str) -> Output {
        let path = dir.join("driver.codon");
        std::fs::write(&path, driver).unwrap();
        let codon = find_codon()
            .expect("codon toolchain not found on PATH (or $DEWASM_CODON): see docs/testing.md");
        dewasm_test_helper::run_command_bytes(
            Command::new(codon).arg("run").arg(&path).current_dir(dir),
            b"",
        )
    }
}

/// Driver for the plain-arithmetic library case, through the boxed export surface an embedder uses.
const CODON_ADD_GLUE: &str = r#"_i = Add(Dict[str, Dict[str, AddRt.Extern]](), List[str](), Dict[str, str](), Dict[str, str]())
print(_i.exports["add"].fn.invoke([AddRt.Val.of_i32(UInt[32](2)), AddRt.Val.of_i32(UInt[32](3))])[0].i32())
print(_i.exports["add"].fn.invoke([AddRt.Val.of_i32(UInt[32](4294967295)), AddRt.Val.of_i32(UInt[32](1))])[0].i32())
print(_i.exports["fib"].fn.invoke([AddRt.Val.of_i32(UInt[32](10))])[0].i32())
"#;

/// The boxed fd_write interceptor the three override glues share (identical in each; only the driver code after it differs): captures the written bytes, memory bound after construction.
const CAP_FD_CLASS: &str = r#"from C import write(int, Ptr[byte], int) -> int

class _CapFd(ProgRt.Fn):
    m: Optional[ProgRt.Memory]
    out: List[int]
    def __init__(self):
        self.m = None
        self.out = List[int]()
    def invoke(self, a: List[ProgRt.Val]) -> List[ProgRt.Val]:
        m = self.m
        iovs = int(a[1].i32())
        ptr = int(m.i32_load(iovs))
        ln = int(m.i32_load(iovs + 4))
        for k in range(ln):
            self.out.append(int(m.data[ptr + k]))
        m.i32_store(int(a[3].i32()), UInt[32](ln))
        return [ProgRt.Val.of_i32(UInt[32](0))]
"#;

/// The override/fallback glue: fd_write intercepted by a boxed `Fn`, random_get falls back to the bundled WASI.
/// Prints the actual bytes written.
static CODON_OVERRIDE_GLUE: LazyLock<String> = LazyLock::new(|| {
    format!(
        "{CAP_FD_CLASS}{}",
        r#"
_cap = _CapFd()
_w = Dict[str, ProgRt.Extern]()
_w["fd_write"] = ProgRt.Extern.of_fn(_cap, "i32,i32,i32,i32->i32")
_imp = Dict[str, Dict[str, ProgRt.Extern]]()
_imp["wasi_snapshot_preview1"] = _w
_inst = Prog(_imp, List[str](), Dict[str, str](), Dict[str, str]())
_cap.m = _inst.m
_inst.exports["_start"].fn.invoke(List[ProgRt.Val]())  # random_get falls back to the bundled WASI
_buf = Ptr[byte](len(_cap.out) if len(_cap.out) > 0 else 1)
for _k in range(len(_cap.out)):
    _buf[_k] = byte(_cap.out[_k])
write(1, _buf, len(_cap.out))
"#
    )
});

/// The `custom_wasi_provider` glue: the imports dict *is* the provider contract for Codon (the dynamic backends' duck-typed provider objects have no equivalent), so a dict covering every WASI import stands in for the provider object, and the bundled WASI (`_wasi`) is never lazily constructed.
static CODON_CUSTOM_PROVIDER_GLUE: LazyLock<String> = LazyLock::new(|| {
    format!(
        "{CAP_FD_CLASS}{}",
        r#"
class _RandZero(ProgRt.Fn):
    def __init__(self):
        pass
    def invoke(self, a: List[ProgRt.Val]) -> List[ProgRt.Val]:
        return [ProgRt.Val.of_i32(UInt[32](0))]

_cap = _CapFd()
_w = Dict[str, ProgRt.Extern]()
_w["fd_write"] = ProgRt.Extern.of_fn(_cap, "i32,i32,i32,i32->i32")
_w["random_get"] = ProgRt.Extern.of_fn(_RandZero(), "i32,i32->i32")
_imp = Dict[str, Dict[str, ProgRt.Extern]]()
_imp["wasi_snapshot_preview1"] = _w
_inst = Prog(_imp, List[str](), Dict[str, str](), Dict[str, str]())
_cap.m = _inst.m
_inst.exports["_start"].fn.invoke(List[ProgRt.Val]())
_buf = Ptr[byte](len(_cap.out) if len(_cap.out) > 0 else 1)
for _k in range(len(_cap.out)):
    _buf[_k] = byte(_cap.out[_k])
write(1, _buf, len(_cap.out))
print("bundled wasi constructed:", "true" if _inst._wasi is not None else "false")
"#
    )
});

/// The `partial_override_falls_back_to_bundled_wasi` glue: fd_write intercepted, random_get falls back, so the bundled WASI *was* lazily constructed.
static CODON_PARTIAL_OVERRIDE_GLUE: LazyLock<String> = LazyLock::new(|| {
    format!(
        "{CAP_FD_CLASS}{}",
        r#"
_cap = _CapFd()
_w = Dict[str, ProgRt.Extern]()
_w["fd_write"] = ProgRt.Extern.of_fn(_cap, "i32,i32,i32,i32->i32")
_imp = Dict[str, Dict[str, ProgRt.Extern]]()
_imp["wasi_snapshot_preview1"] = _w
_inst = Prog(_imp, List[str](), Dict[str, str](), Dict[str, str]())
_cap.m = _inst.m
_inst.exports["_start"].fn.invoke(List[ProgRt.Val]())  # random_get falls back to the bundled WASI
_buf = Ptr[byte](len(_cap.out) if len(_cap.out) > 0 else 1)
for _k in range(len(_cap.out)):
    _buf[_k] = byte(_cap.out[_k])
write(1, _buf, len(_cap.out))
print("bundled wasi constructed:", "true" if _inst._wasi is not None else "false")
"#
    )
});

/// The `wasi_stdio_capture` glue: the bundled WASI writes straight to the process fd 1, so the capture is an fd-level pipe redirect (dup/dup2), the Go backend's `os.Pipe` idiom on libc.
const CODON_STDIO_CAPTURE_GLUE: &str = r#"from C import pipe(Ptr[byte]) -> int
from C import dup(int) -> int
from C import dup2(int, int) -> int
from C import close(int) -> int
from C import read(int, Ptr[byte], int) -> int
from C import write(int, Ptr[byte], int) -> int

_fds = Ptr[byte](8)
pipe(_fds)
_pi = Ptr[Int[32]](_fds.as_byte())
_rd = int(_pi[0])
_wd = int(_pi[1])
_sav = dup(1)
dup2(_wd, 1)
close(_wd)
_inst = Prog(Dict[str, Dict[str, ProgRt.Extern]](), List[str](), Dict[str, str](), Dict[str, str]())
try:
    _inst.exports["_start"].fn.invoke(List[ProgRt.Val]())
except ProgRt.Exit:
    pass
dup2(_sav, 1)
close(_sav)
_buf = Ptr[byte](65536)
_n = read(_rd, _buf, 65536)
close(_rd)
if _n > 0:
    write(1, _buf, _n)
"#;

/// The shared filesystem template: preopen the scratch dir (`{host}`) at guest `{guest}` (always `/`), run `_start`, and surface a `proc_exit` code as a trailing decimal line.
const CODON_FS_GLUE: &str = r#"_pre = Dict[str, str]()
_pre["{guest}"] = "{host}"
_inst = Prog(Dict[str, Dict[str, ProgRt.Extern]](), List[str](), Dict[str, str](), _pre)
try:
    _inst.exports["_start"].fn.invoke(List[ProgRt.Val]())
except ProgRt.Exit as _e:
    print(_e.code)
"#;

/// The root-preopen containment probe: call the WASI resolver directly with a `"/" => "/"` preopen (no guest run) and normalize the outcome to `contained`.
const CODON_CONTAINMENT_GLUE: &str = r#"_pre = Dict[str, str]()
_pre["/"] = "/"
_w = ProgRt.WASI(List[str](), Dict[str, str](), _pre)
_host, _err = _w.resolve_path(3, "etc", True)
print("contained" if _err == 0 else "rejected")
"#;

// Filesystem app glue: class/argv/env/preopen-guest-paths are literals; only the host scratch/cache dirs come through {scratch}/{cache}.
// All of them run `_start` through the boxed export and swallow the Exit.

const CODON_QJS_FILE_IO_GLUE: &str = r#"_pre = Dict[str, str]()
_pre["/work"] = "{scratch}"
_i = Qjs(Dict[str, Dict[str, QjsRt.Extern]](), ["qjs", "/work/qjs_file_io.js"], Dict[str, str](), _pre)
try:
    _i.exports["_start"].fn.invoke(List[QjsRt.Val]())
except QjsRt.Exit:
    pass
"#;

const CODON_SQLITE3_SHELL_GLUE: &str = r#"_pre = Dict[str, str]()
_pre["/db"] = "{scratch}"
_i = Sqlite3Shell(Dict[str, Dict[str, Sqlite3ShellRt.Extern]](), ["sqlite3"], Dict[str, str](), _pre)
try:
    _i.exports["_start"].fn.invoke(List[Sqlite3ShellRt.Val]())
except Sqlite3ShellRt.Exit:
    pass
"#;

const CODON_RG_SEARCH_GLUE: &str = r#"_pre = Dict[str, str]()
_pre["/work"] = "{scratch}"
_i = Rg(Dict[str, Dict[str, RgRt.Extern]](), ["rg", "--sort", "path", "needle", "/work"], Dict[str, str](), _pre)
try:
    _i.exports["_start"].fn.invoke(List[RgRt.Val]())
except RgRt.Exit:
    pass
"#;

/// The whole app cache is preopened at `/apps` because the guest module this converted interpreter loads (`cowsay.wasm`) is itself a cached app.
const CODON_TOYWASM_GLUE: &str = r#"_pre = Dict[str, str]()
_pre["/apps"] = "{cache}"
_i = Toywasm(Dict[str, Dict[str, ToywasmRt.Extern]](), ["toywasm", "--wasi", "/apps/cowsay.wasm", "Hello", "from", "dewasm!"], Dict[str, str](), _pre)
try:
    _i.exports["_start"].fn.invoke(List[ToywasmRt.Val]())
except ToywasmRt.Exit:
    pass
"#;

/// Like the toywasm glue; wasm3's CLI takes the guest module directly, and its dispatch is a tail call the trampoline runs flat.
const CODON_WASM3_GLUE: &str = r#"_pre = Dict[str, str]()
_pre["/apps"] = "{cache}"
_i = Wasm3(Dict[str, Dict[str, Wasm3Rt.Extern]](), ["wasm3", "/apps/cowsay.wasm", "Hello", "from", "dewasm!"], Dict[str, str](), _pre)
try:
    _i.exports["_start"].fn.invoke(List[Wasm3Rt.Val]())
except Wasm3Rt.Exit:
    pass
"#;

const CODON_CPYTHON_GLUE: &str = r#"_pre = Dict[str, str]()
_pre["/lib"] = "{cache}/cpython-lib/lib"
_env = Dict[str, str]()
_env["PYTHONHOME"] = "/"
_env["PYTHONPATH"] = "/lib/python3.14"
_i = Cpython(Dict[str, Dict[str, CpythonRt.Extern]](), ["python", "-c", "print('hello from cpython', 6 * 7)"], _env, _pre)
try:
    _i.exports["_start"].fn.invoke(List[CpythonRt.Val]())
except CpythonRt.Exit:
    pass
"#;

const CODON_CRUBY_GLUE: &str = r#"_pre = Dict[str, str]()
_pre["/usr"] = "{cache}/ruby-lib/usr"
_i = Cruby(Dict[str, Dict[str, CrubyRt.Extern]](), ["ruby", "-e", "puts \"hello from cruby #{6*7}\""], Dict[str, str](), _pre)
try:
    _i.exports["_start"].fn.invoke(List[CrubyRt.Val]())
except CrubyRt.Exit:
    pass
"#;

// C-API drive glue (sqlite3): malloc/pointer plumbing via the artifact's raw memory.
// Only the file-backed case uses {scratch}.

const CODON_LIBSQLITE3_MEM: &str = r#"
_i = Libsqlite3(Dict[str, Dict[str, Libsqlite3Rt.Extern]](), List[str](), Dict[str, str](), Dict[str, str]())
_i.exports["_initialize"].fn.invoke(List[Libsqlite3Rt.Val]())

def _c(name: str, a: List[Libsqlite3Rt.Val]) -> List[Libsqlite3Rt.Val]:
    return _i.exports[name].fn.invoke(a)

def _v(x: int) -> Libsqlite3Rt.Val:
    return Libsqlite3Rt.Val.of_i32(UInt[32](x))

def _read_cstr(ptr: int) -> str:
    if ptr == 0:
        return ""
    n = 0
    while int(_i.m.data[ptr + n]) != 0:
        n += 1
    p = Ptr[UInt[8]](n if n > 0 else 1)
    str.memcpy(Ptr[byte](p.as_byte()), _i.m.data + ptr, n)
    return str(p, n)

def _cstr(s: str) -> int:
    p = int(_c("sqlite3_malloc", [_v(len(s) + 1)])[0].i32())
    str.memcpy(_i.m.data + p, Ptr[byte](s.c_str().as_byte()), len(s))
    _i.m.data[p + len(s)] = byte(0)
    return p

print("version: " + _read_cstr(int(_c("sqlite3_libversion", List[Libsqlite3Rt.Val]())[0].i32())))

_pp_db = int(_c("sqlite3_malloc", [_v(4)])[0].i32())
_rc = int(_c("sqlite3_open", [_v(_cstr(":memory:")), _v(_pp_db)])[0].i32())
assert _rc == 0, "open rc=" + str(_rc)
_db = int(_i.m.i32_load(_pp_db))

_rc = int(_c("sqlite3_exec", [_v(_db), _v(_cstr("create table t(a,b); insert into t values (1,'x'),(2,'y');")), _v(0), _v(0), _v(0)])[0].i32())
assert _rc == 0, "exec rc=" + str(_rc)

_pp_stmt = int(_c("sqlite3_malloc", [_v(4)])[0].i32())
_rc = int(_c("sqlite3_prepare_v2", [_v(_db), _v(_cstr("select a*10, b from t order by a desc")), _v(0xFFFFFFFF), _v(_pp_stmt), _v(0)])[0].i32())
assert _rc == 0, "prepare rc=" + str(_rc)
_stmt = int(_i.m.i32_load(_pp_stmt))

while int(_c("sqlite3_step", [_v(_stmt)])[0].i32()) == 100:
    _n = int(_c("sqlite3_column_count", [_v(_stmt)])[0].i32())
    _row = List[str]()
    for _k in range(_n):
        _row.append(_read_cstr(int(_c("sqlite3_column_text", [_v(_stmt), _v(_k)])[0].i32())))
    print("|".join(_row))
_c("sqlite3_finalize", [_v(_stmt)])
_c("sqlite3_close", [_v(_db)])
print("C-API-OK")
"#;

const CODON_LIBSQLITE3_FILE: &str = r#"
_pre = Dict[str, str]()
_pre["/db"] = "{scratch}"
_i = Libsqlite3(Dict[str, Dict[str, Libsqlite3Rt.Extern]](), List[str](), Dict[str, str](), _pre)
_i.exports["_initialize"].fn.invoke(List[Libsqlite3Rt.Val]())

def _c(name: str, a: List[Libsqlite3Rt.Val]) -> List[Libsqlite3Rt.Val]:
    return _i.exports[name].fn.invoke(a)

def _v(x: int) -> Libsqlite3Rt.Val:
    return Libsqlite3Rt.Val.of_i32(UInt[32](x))

def _read_cstr(ptr: int) -> str:
    if ptr == 0:
        return ""
    n = 0
    while int(_i.m.data[ptr + n]) != 0:
        n += 1
    p = Ptr[UInt[8]](n if n > 0 else 1)
    str.memcpy(Ptr[byte](p.as_byte()), _i.m.data + ptr, n)
    return str(p, n)

def _cstr(s: str) -> int:
    p = int(_c("sqlite3_malloc", [_v(len(s) + 1)])[0].i32())
    str.memcpy(_i.m.data + p, Ptr[byte](s.c_str().as_byte()), len(s))
    _i.m.data[p + len(s)] = byte(0)
    return p

def _open_db(path: str) -> int:
    pp = int(_c("sqlite3_malloc", [_v(4)])[0].i32())
    rc = int(_c("sqlite3_open", [_v(_cstr(path)), _v(pp)])[0].i32())
    assert rc == 0, "open rc=" + str(rc)
    return int(_i.m.i32_load(pp))

_db = _open_db("/db/data.db")
_rc = int(_c("sqlite3_exec", [_v(_db), _v(_cstr("create table t(a,b); insert into t values (1,'x'),(2,'y');")), _v(0), _v(0), _v(0)])[0].i32())
assert _rc == 0, "exec rc=" + str(_rc)
_c("sqlite3_close", [_v(_db)])

_db = _open_db("/db/data.db")
_pp_stmt = int(_c("sqlite3_malloc", [_v(4)])[0].i32())
_rc = int(_c("sqlite3_prepare_v2", [_v(_db), _v(_cstr("select a*10, b from t order by a")), _v(0xFFFFFFFF), _v(_pp_stmt), _v(0)])[0].i32())
assert _rc == 0, "prepare rc=" + str(_rc)
_stmt = int(_i.m.i32_load(_pp_stmt))
while int(_c("sqlite3_step", [_v(_stmt)])[0].i32()) == 100:
    _n = int(_c("sqlite3_column_count", [_v(_stmt)])[0].i32())
    _row = List[str]()
    for _k in range(_n):
        _row.append(_read_cstr(int(_c("sqlite3_column_text", [_v(_stmt), _v(_k)])[0].i32())))
    print("|".join(_row))
_c("sqlite3_finalize", [_v(_stmt)])
_c("sqlite3_close", [_v(_db)])
print("FILE-OK")
"#;

const CODON_SQLITE3_CALLBACK: &str = r#"
_rows = List[str]()

class _HostRow(Sqlite3BindingRt.Fn):
    m: Optional[Sqlite3BindingRt.Memory]
    def __init__(self):
        self.m = None
    def invoke(self, a: List[Sqlite3BindingRt.Val]) -> List[Sqlite3BindingRt.Val]:
        m = self.m
        argc = int(a[0].i32())
        argv = int(a[1].i32())
        row = List[str]()
        for k in range(argc):
            p = int(m.i32_load(argv + k * 4))
            if p == 0:
                row.append("")
            else:
                n = 0
                while int(m.data[p + n]) != 0:
                    n += 1
                q = Ptr[UInt[8]](n if n > 0 else 1)
                str.memcpy(Ptr[byte](q.as_byte()), m.data + p, n)
                row.append(str(q, n))
        _rows.append("|".join(row))
        return [Sqlite3BindingRt.Val.of_i32(UInt[32](0))]

_hr = _HostRow()
_env = Dict[str, Sqlite3BindingRt.Extern]()
_env["host_row"] = Sqlite3BindingRt.Extern.of_fn(_hr, "i32,i32->i32")
_imp = Dict[str, Dict[str, Sqlite3BindingRt.Extern]]()
_imp["env"] = _env
_i = Sqlite3Binding(_imp, List[str](), Dict[str, str](), Dict[str, str]())
_hr.m = _i.m
_i.exports["_initialize"].fn.invoke(List[Sqlite3BindingRt.Val]())

def _c(name: str, a: List[Sqlite3BindingRt.Val]) -> List[Sqlite3BindingRt.Val]:
    return _i.exports[name].fn.invoke(a)

def _v(x: int) -> Sqlite3BindingRt.Val:
    return Sqlite3BindingRt.Val.of_i32(UInt[32](x))

def _cstr(s: str) -> int:
    p = int(_c("sqlite3_malloc", [_v(len(s) + 1)])[0].i32())
    str.memcpy(_i.m.data + p, Ptr[byte](s.c_str().as_byte()), len(s))
    _i.m.data[p + len(s)] = byte(0)
    return p

_pp_db = int(_c("sqlite3_malloc", [_v(4)])[0].i32())
_rc = int(_c("sqlite3_open", [_v(_cstr(":memory:")), _v(_pp_db)])[0].i32())
assert _rc == 0, "open rc=" + str(_rc)
_db = int(_i.m.i32_load(_pp_db))

_rc = int(_c("sqlite3_exec", [_v(_db), _v(_cstr("create table t(a,b); insert into t values (1,'x'),(2,'y'),(3,'z');")), _v(0), _v(0), _v(0)])[0].i32())
assert _rc == 0, "exec rc=" + str(_rc)

_rc = int(_c("run_query", [_v(_db), _v(_cstr("select a, b from t where a >= 2 order by a"))])[0].i32())
assert _rc == 0, "run_query rc=" + str(_rc)
_c("sqlite3_close", [_v(_db)])

for _r in _rows:
    print("row: " + _r)
print("CALLBACK-OK")
"#;

/// libpcap BPF filter compilation: drive `compile_filter` on "tcp port 80" (DLT_EN10MB, snaplen 65535), then walk the serialized program `[u32 bf_len][bf_len x {u16 code; u8 jt; u8 jf; u32 k}]` in guest memory, printing each instruction as `code jt jf k`.
const CODON_PCAP_COMPILE: &str = r#"
_i = Libpcap(Dict[str, Dict[str, LibpcapRt.Extern]](), List[str](), Dict[str, str](), Dict[str, str]())
_i.exports["_initialize"].fn.invoke(List[LibpcapRt.Val]())

def _c(name: str, a: List[LibpcapRt.Val]) -> List[LibpcapRt.Val]:
    return _i.exports[name].fn.invoke(a)

def _v(x: int) -> LibpcapRt.Val:
    return LibpcapRt.Val.of_i32(UInt[32](x))

def _cstr(s: str) -> int:
    p = int(_c("malloc", [_v(len(s) + 1)])[0].i32())
    str.memcpy(_i.m.data + p, Ptr[byte](s.c_str().as_byte()), len(s))
    _i.m.data[p + len(s)] = byte(0)
    return p

_prog = int(_c("compile_filter", [_v(_cstr("tcp port 80")), _v(1), _v(65535)])[0].i32())
assert _prog != 0, "compile failed"
_n = int(_i.m.i32_load(_prog))
for _k in range(_n):
    _base = _prog + 4 + _k * 8
    _code = int(_i.m.i32_load16_u(_base))
    _jt = int(_i.m.data[_base + 2])
    _jf = int(_i.m.data[_base + 3])
    _kk = int(_i.m.i32_load(_base + 4))
    print(str(_code) + " " + str(_jt) + " " + str(_jf) + " " + str(_kk))
_c("free", [_v(_prog)])
print("BPF-OK")
"#;

/// tree-sitter JSON parse: drive `parse_source` on the fixed snippet and print the parse tree's S-expression (a malloc'd NUL-terminated C string) from guest memory.
const CODON_TREESITTER_PARSE: &str = r#"
_i = Treesitter(Dict[str, Dict[str, TreesitterRt.Extern]](), List[str](), Dict[str, str](), Dict[str, str]())
_i.exports["_initialize"].fn.invoke(List[TreesitterRt.Val]())

def _c(name: str, a: List[TreesitterRt.Val]) -> List[TreesitterRt.Val]:
    return _i.exports[name].fn.invoke(a)

def _v(x: int) -> TreesitterRt.Val:
    return TreesitterRt.Val.of_i32(UInt[32](x))

def _cstr(s: str) -> int:
    p = int(_c("malloc", [_v(len(s) + 1)])[0].i32())
    str.memcpy(_i.m.data + p, Ptr[byte](s.c_str().as_byte()), len(s))
    _i.m.data[p + len(s)] = byte(0)
    return p

_src = "{\"key\": [1, true, null]}"
_r = int(_c("parse_source", [_v(_cstr(_src)), _v(len(_src))])[0].i32())
assert _r != 0, "parse failed"
_n = 0
while int(_i.m.data[_r + _n]) != 0:
    _n += 1
_q = Ptr[UInt[8]](_n if _n > 0 else 1)
str.memcpy(Ptr[byte](_q.as_byte()), _i.m.data + _r, _n)
print(str(_q, _n))
_c("free", [_v(_r)])
print("TS-OK")
"#;

/// zeroperl Perl-5.42 eval: instantiate the reactor with a zero-returning `env.call_host_function` import stub and a `/dev/null` preopen, then `_initialize` -> `zeroperl_init` -> `malloc` + copy a Perl program into guest memory -> `zeroperl_eval` -> `zeroperl_flush`.
/// The Perl source's backslashes belong to Perl, so they are escaped once for the Codon string literal.
const CODON_ZEROPERL_EVAL: &str = r#"
class _HostFn(ZeroperlRt.Fn):
    def __init__(self):
        pass
    def invoke(self, a: List[ZeroperlRt.Val]) -> List[ZeroperlRt.Val]:
        return [ZeroperlRt.Val.of_i32(UInt[32](0))]

_env = Dict[str, ZeroperlRt.Extern]()
_env["call_host_function"] = ZeroperlRt.Extern.of_fn(_HostFn(), "i32,i32,i32->i32")
_imp = Dict[str, Dict[str, ZeroperlRt.Extern]]()
_imp["env"] = _env
_pre = Dict[str, str]()
_pre["/dev/null"] = "/dev/null"
_i = Zeroperl(_imp, List[str](), Dict[str, str](), _pre)
_i.exports["_initialize"].fn.invoke(List[ZeroperlRt.Val]())
_rc = int(_i.exports["zeroperl_init"].fn.invoke(List[ZeroperlRt.Val]())[0].i32())
assert _rc == 0, "zeroperl_init rc=" + str(_rc)

def _v(x: int) -> ZeroperlRt.Val:
    return ZeroperlRt.Val.of_i32(UInt[32](x))

_prog = "my $s = \"hello world 42\";\nif ($s =~ /(\\w+)\\s+(\\w+)\\s+(\\d+)/) {\n  printf(\"m=%s|%s|%d sum=%d\\n\", $1, $2, $3, $3 + 8);\n}\n"
_ptr = int(_i.exports["malloc"].fn.invoke([_v(len(_prog) + 1)])[0].i32())
str.memcpy(_i.m.data + _ptr, Ptr[byte](_prog.c_str().as_byte()), len(_prog))
_i.m.data[_ptr + len(_prog)] = byte(0)
_i.exports["zeroperl_eval"].fn.invoke([_v(_ptr), _v(0), _v(0), _v(0)])
_i.exports["zeroperl_flush"].fn.invoke(List[ZeroperlRt.Val]())
"#;

/// ExifTool on zeroperl: the flattened `exiftool` CLI driver run on the same reactor (see the Python backend's callsite for the full contract); only deterministic tags are requested.
const CODON_EXIFTOOL: &str = r#"
class _HostFn(ZeroperlRt.Fn):
    def __init__(self):
        pass
    def invoke(self, a: List[ZeroperlRt.Val]) -> List[ZeroperlRt.Val]:
        return [ZeroperlRt.Val.of_i32(UInt[32](0))]

_env = Dict[str, ZeroperlRt.Extern]()
_env["call_host_function"] = ZeroperlRt.Extern.of_fn(_HostFn(), "i32,i32,i32->i32")
_imp = Dict[str, Dict[str, ZeroperlRt.Extern]]()
_imp["env"] = _env
_pre = Dict[str, str]()
_pre["/dev/null"] = "/dev/null"
_pre["/work"] = "{cache}/exiftool-lib"
_pre["/img"] = "{scratch}"
_i = Zeroperl(_imp, List[str](), Dict[str, str](), _pre)
_i.exports["_initialize"].fn.invoke(List[ZeroperlRt.Val]())
_rc = int(_i.exports["zeroperl_init"].fn.invoke(List[ZeroperlRt.Val]())[0].i32())
assert _rc == 0, "zeroperl_init rc=" + str(_rc)

def _v(x: int) -> ZeroperlRt.Val:
    return ZeroperlRt.Val.of_i32(UInt[32](x))

_drv = "BEGIN { *CORE::GLOBAL::exit = sub (;$) { die \"zeroperl_exit\\n\" }; }\n@ARGV = ('-S', '-Make', '-Model', '-DateTimeOriginal', '/img/exif_fixture.jpg');\n$0 = '/work/exiftool';\ndo '/work/exiftool';\n"
_ptr = int(_i.exports["malloc"].fn.invoke([_v(len(_drv) + 1)])[0].i32())
str.memcpy(_i.m.data + _ptr, Ptr[byte](_drv.c_str().as_byte()), len(_drv))
_i.m.data[_ptr + len(_drv)] = byte(0)
_i.exports["zeroperl_eval"].fn.invoke([_v(_ptr), _v(0), _v(0), _v(0)])
_i.exports["zeroperl_flush"].fn.invoke(List[ZeroperlRt.Val]())
"#;

/// Driver for the shared-table case: instantiate the exporter and the importer linked against it, then print `call0` (call_indirect through the shared table -> 42).
const CODON_SHARED_TABLE_GLUE: &str = r#"_a = TableExp(Dict[str, Dict[str, Rt.Extern]](), List[str](), Dict[str, str](), Dict[str, str]())
_b = TableImp({"a": _a.exports}, List[str](), Dict[str, str](), Dict[str, str]())
print(_b.exports["call0"].fn.invoke(List[Rt.Val]())[0].i32())
"#;

/// Driver for the embedded-coexistence case: two independent Embedded artifacts in one namespace.
/// Each carries its own runtime class (`AlphaRt`/`BetaRt`), so their trap types are distinct: Beta's except arm must not catch Alpha's trap.
const CODON_EMBEDDED_COEXIST_GLUE: &str = r#"_a = Alpha(Dict[str, Dict[str, AlphaRt.Extern]](), List[str](), Dict[str, str](), Dict[str, str]())
_b = Beta(Dict[str, Dict[str, BetaRt.Extern]](), List[str](), Dict[str, str](), Dict[str, str]())
print(_a.exports["div"].fn.invoke([AlphaRt.Val.of_i32(UInt[32](7)), AlphaRt.Val.of_i32(UInt[32](2))])[0].i32())
print(_b.exports["div"].fn.invoke([BetaRt.Val.of_i32(UInt[32](4294967289)), BetaRt.Val.of_i32(UInt[32](2))])[0].i32())
_distinct = True
try:
    _a.exports["div"].fn.invoke([AlphaRt.Val.of_i32(UInt[32](1)), AlphaRt.Val.of_i32(UInt[32](0))])
except BetaRt.Trap:
    _distinct = False
except AlphaRt.Trap:
    pass
print("distinct-rt" if _distinct else "same-rt")
try:
    _a.exports["div"].fn.invoke([AlphaRt.Val.of_i32(UInt[32](1)), AlphaRt.Val.of_i32(UInt[32](0))])
except AlphaRt.Trap:
    print("trapped")
"#;

/// DOOM: drive the converted library under the deterministic contract (synthetic clock, no input) and dump the framebuffer as a P6 PPM matching the wasmtime snapshot.
/// `{ticks}`/`{clock_step}` are filled by the runner.
const CODON_DOOM_FRAME_GLUE: &str = r#"from C import write(int, Ptr[byte], int) -> int

class _Sink(DoomRt.Fn):
    def __init__(self):
        pass
    def invoke(self, a: List[DoomRt.Val]) -> List[DoomRt.Val]:
        return List[DoomRt.Val]()

class _RetZero(DoomRt.Fn):
    def __init__(self):
        pass
    def invoke(self, a: List[DoomRt.Val]) -> List[DoomRt.Val]:
        return [DoomRt.Val.of_i32(UInt[32](0))]

class _WriteSave(DoomRt.Fn):
    def __init__(self):
        pass
    def invoke(self, a: List[DoomRt.Val]) -> List[DoomRt.Val]:
        return [DoomRt.Val.of_i32(a[2].i32())]

class _Clock(DoomRt.Fn):
    ms: int
    def __init__(self):
        self.ms = 0
    def invoke(self, a: List[DoomRt.Val]) -> List[DoomRt.Val]:
        # Self-advancing per call: matches the oracle so startup/inter-tic spins terminate and the frame stays deterministic.
        self.ms += {clock_step}
        return [DoomRt.Val.of_i64(UInt[64](self.ms))]

class _DrawFrame(DoomRt.Fn):
    off: int
    def __init__(self):
        self.off = 0
    def invoke(self, a: List[DoomRt.Val]) -> List[DoomRt.Val]:
        self.off = int(a[0].i32())
        return List[DoomRt.Val]()

class _OnInit(DoomRt.Fn):
    w: int
    h: int
    def __init__(self):
        self.w = 0
        self.h = 0
    def invoke(self, a: List[DoomRt.Val]) -> List[DoomRt.Val]:
        self.w = int(a[0].i32())
        self.h = int(a[1].i32())
        return List[DoomRt.Val]()

_draw = _DrawFrame()
_init = _OnInit()
_imp = Dict[str, Dict[str, DoomRt.Extern]]()
_console = Dict[str, DoomRt.Extern]()
_console["onErrorMessage"] = DoomRt.Extern.of_fn(_Sink(), "i32,i32->")
_console["onInfoMessage"] = DoomRt.Extern.of_fn(_Sink(), "i32,i32->")
_imp["console"] = _console
_saving = Dict[str, DoomRt.Extern]()
_saving["sizeOfSaveGame"] = DoomRt.Extern.of_fn(_RetZero(), "i32->i32")
_saving["readSaveGame"] = DoomRt.Extern.of_fn(_RetZero(), "i32,i32->i32")
_saving["writeSaveGame"] = DoomRt.Extern.of_fn(_WriteSave(), "i32,i32,i32->i32")
_imp["gameSaving"] = _saving
_rc = Dict[str, DoomRt.Extern]()
_rc["timeInMilliseconds"] = DoomRt.Extern.of_fn(_Clock(), "->i64")
_imp["runtimeControl"] = _rc
_ui = Dict[str, DoomRt.Extern]()
_ui["drawFrame"] = DoomRt.Extern.of_fn(_draw, "i32->")
_imp["ui"] = _ui
_loading = Dict[str, DoomRt.Extern]()
_loading["onGameInit"] = DoomRt.Extern.of_fn(_init, "i32,i32->")
_loading["wadSizes"] = DoomRt.Extern.of_fn(_Sink(), "i32,i32->")
_loading["readWads"] = DoomRt.Extern.of_fn(_Sink(), "i32,i32->")
_imp["loading"] = _loading

_doom = Doom(_imp, List[str](), Dict[str, str](), Dict[str, str]())
_doom.exports["initGame"].fn.invoke(List[DoomRt.Val]())
for _t in range(1, {ticks} + 1):
    _doom.exports["tickGame"].fn.invoke(List[DoomRt.Val]())

_w = _init.w
_h = _init.h
_off = _draw.off
_hdr = "P6\n" + str(_w) + " " + str(_h) + "\n255\n"
write(1, Ptr[byte](_hdr.c_str().as_byte()), len(_hdr))
_rgb = Ptr[byte](_w * _h * 3)
_j = 0
for _k in range(_w * _h):
    _p = _off + _k * 4
    _rgb[_j] = _doom.m.data[_p + 2]
    _rgb[_j + 1] = _doom.m.data[_p + 1]
    _rgb[_j + 2] = _doom.m.data[_p]
    _j += 3
write(1, _rgb, _w * _h * 3)
"#;

/// NES (mirrors the DOOM glue): load the pinned ROM into `allocRom`'s buffer, tick `{frames}` times with no input, compose the frame from agnes's palette-index screen buffer and its palette (the `& 0x3f` mask is load-bearing) and dump it as a P6 PPM matching the wasmtime snapshot.
/// `{rom}` (the cached ROM's host path) and `{frames}` filled by the runner.
const CODON_NES_FRAME_GLUE: &str = r#"from C import write(int, Ptr[byte], int) -> int
from C import read(int, Ptr[byte], int) -> int
from C import open(cobj, int) -> int
from C import close(int) -> int

_nes = Nes(Dict[str, Dict[str, NesRt.Extern]](), List[str](), Dict[str, str](), Dict[str, str]())
_nes.exports["_initialize"].fn.invoke(List[NesRt.Val]())

def _v(x: int) -> NesRt.Val:
    return NesRt.Val.of_i32(UInt[32](x))

def _c1(name: str) -> int:
    return int(_nes.exports[name].fn.invoke(List[NesRt.Val]())[0].i32())

_rom_path = "{rom}"
_fd = open(_rom_path.c_str(), 0)
assert _fd >= 0, "open rom failed"
_cap = 4 * 1024 * 1024
_buf = Ptr[byte](_cap)
_rom_len = 0
while True:
    _r = read(_fd, _buf + _rom_len, _cap - _rom_len)
    if _r <= 0:
        break
    _rom_len += _r
close(_fd)

_ptr = int(_nes.exports["allocRom"].fn.invoke([_v(_rom_len)])[0].i32())
str.memcpy(_nes.m.data + _ptr, _buf, _rom_len)
_ok = _c1("initGame")
assert _ok == 1, "initGame failed: " + str(_ok)
for _t in range(1, {frames} + 1):
    _nes.exports["tickGame"].fn.invoke(List[NesRt.Val]())

_w = _c1("frameWidth")
_h = _c1("frameHeight")
_soff = _c1("screenOffset")
_poff = _c1("paletteOffset")
_hdr = "P6\n" + str(_w) + " " + str(_h) + "\n255\n"
write(1, Ptr[byte](_hdr.c_str().as_byte()), len(_hdr))
_rgb = Ptr[byte](_w * _h * 3)
_j = 0
for _k in range(_w * _h):
    _ix = (int(_nes.m.data[_soff + _k]) & 0x3F) * 4
    _rgb[_j] = _nes.m.data[_poff + _ix]
    _rgb[_j + 1] = _nes.m.data[_poff + _ix + 1]
    _rgb[_j + 2] = _nes.m.data[_poff + _ix + 2]
    _j += 3
write(1, _rgb, _w * _h * 3)
"#;

dewasm_test_helper::library_add_e2e!(Codon, CODON_ADD_GLUE);
dewasm_test_helper::wasi_import_override_e2e!(Codon, CODON_OVERRIDE_GLUE.as_str());
dewasm_test_helper::custom_wasi_provider_e2e!(Codon, CODON_CUSTOM_PROVIDER_GLUE.as_str());
dewasm_test_helper::partial_override_e2e!(Codon, CODON_PARTIAL_OVERRIDE_GLUE.as_str());
dewasm_test_helper::stdio_capture_e2e!(Codon, CODON_STDIO_CAPTURE_GLUE);

dewasm_test_helper::wasi_suite!(Codon, Stdio);
dewasm_test_helper::wasi_suite!(Codon, ArgsEnv);
dewasm_test_helper::wasi_suite!(Codon, Poll);
// The eight filesystem fixtures each pay a codon build; slow keeps the pull-request tier inside its ~3-minute target.
dewasm_test_helper::wasi_suite!(Codon, Fs, CODON_FS_GLUE, slow);
dewasm_test_helper::wasi_root_containment_e2e!(Codon, CODON_CONTAINMENT_GLUE);
dewasm_test_helper::standalone_dir_e2e!(Codon);
// The native 8 MB main stack carries the 5000-frame recursion unmitigated (like Ruby's host stack).
dewasm_test_helper::deep_recursion_e2e!(Codon);
dewasm_test_helper::folded_temp_reuse_e2e!(Codon);

// The codon speed-token criterion, per the build cost the case's own artifact pays (CI has no persistent codon build cache, so every run pays it fresh): the whole slow lane targets ~5 minutes, so only cases whose artifacts build in seconds (nes 5k lines, minigzip 18.5k, treesitter 22.5k) stay `slow`; everything from the cowsay class (72k lines) upward is `ultra`, run locally.
// Every `ultra` case still runs at slow on the interpreted backends, so CI keeps covering the cases themselves.
dewasm_test_helper::mruby_eh_e2e!(Codon, ultra);
dewasm_test_helper::cowsay_args_e2e!(Codon, ultra);
dewasm_test_helper::cowsay_stdin_e2e!(Codon, ultra);
dewasm_test_helper::qjs_eval_e2e!(Codon, ultra);
dewasm_test_helper::sqlite3_shell_e2e!(Codon, ultra);
dewasm_test_helper::gzip_e2e!(Codon);

dewasm_test_helper::qjs_file_io_e2e!(Codon, CODON_QJS_FILE_IO_GLUE, ultra);
dewasm_test_helper::sqlite3_shell_dbfile_e2e!(Codon, CODON_SQLITE3_SHELL_GLUE, ultra);
// The giant tier of the same criterion: these debug builds measured in the tens of minutes (rg 474k lines, CPython 1.25M, CRuby larger, DOOM megafunction-heavy).
dewasm_test_helper::rg_search_e2e!(Codon, CODON_RG_SEARCH_GLUE, ultra);
dewasm_test_helper::cpython_hello_e2e!(Codon, CODON_CPYTHON_GLUE, ultra);
dewasm_test_helper::cruby_hello_e2e!(Codon, CODON_CRUBY_GLUE, ultra);
// Ultra-slow category on every backend that runs it (issue #126's memory criterion for the host-compile of a CRuby-class artifact); for Codon the equivalent cost is the giant `codon build`, shared with the zeroperl pair below.
dewasm_test_helper::cruby_packed_hello_e2e!(Codon, ultra);
dewasm_test_helper::toywasm_cowsay_e2e!(Codon, CODON_TOYWASM_GLUE, ultra);
dewasm_test_helper::wasm3_cowsay_e2e!(Codon, CODON_WASM3_GLUE, ultra);
dewasm_test_helper::qjs_repl_pty_e2e!(Codon, ultra);

dewasm_test_helper::libsqlite3_c_api_e2e!(Codon, CODON_LIBSQLITE3_MEM, ultra);
dewasm_test_helper::sqlite3_file_c_api_e2e!(Codon, CODON_LIBSQLITE3_FILE, ultra);
dewasm_test_helper::sqlite3_callback_binding_e2e!(Codon, CODON_SQLITE3_CALLBACK, ultra);
dewasm_test_helper::pcap_compile_e2e!(Codon, CODON_PCAP_COMPILE, ultra);
dewasm_test_helper::treesitter_parse_e2e!(Codon, CODON_TREESITTER_PARSE);
// Ultra-slow category (the Python backend's issue #139 criterion, translated: the 25 MB zeroperl reactor's generated source is the biggest single `codon build` in the suite, and the two cases share the one oversized module).
dewasm_test_helper::zeroperl_eval_e2e!(Codon, CODON_ZEROPERL_EVAL, ultra);
dewasm_test_helper::exiftool_extract_e2e!(Codon, CODON_EXIFTOOL, ultra);

// Ultra-slow category: DOOM's generated code is megafunction-heavy enough that even the debug build measured over 20 minutes on the dev host (see the reclassification note above); NES builds in seconds and stays slow.
dewasm_test_helper::doom_frame_e2e!(Codon, CODON_DOOM_FRAME_GLUE, ultra);
dewasm_test_helper::nes_frame_e2e!(Codon, CODON_NES_FRAME_GLUE);

dewasm_test_helper::shared_table_e2e!(Codon, CODON_SHARED_TABLE_GLUE);
dewasm_test_helper::embedded_coexist_e2e!(Codon, CODON_EMBEDDED_COEXIST_GLUE);
