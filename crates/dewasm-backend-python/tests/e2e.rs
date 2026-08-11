//! Python end-to-end suites: the shared case consts (`dewasm-test-helper`) wired up for the Python backend. This file holds ONLY the [`BackendUnderTest`] impl, named glue string constants, and per-case macro invocations. Python covers full WASI preview 1 incl. the filesystem, so it wires every WASI kind, the slow `apps`/`fs_apps`/`capi` suites, and the shared-table multi-module case.

use std::path::{Path, PathBuf};

use dewasm_backend::{Backend, Mode, RuntimeLinkage};
use dewasm_backend_python::{find_python, PythonBackend};
use dewasm_test_helper::BackendUnderTest;

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

    /// Write each `.wat` module of a multi-module case into `dir` as its own importable `.py` file and return the driver's `from <module> import ...` preamble; the interpreter puts the driver's directory first on `sys.path`, so plain imports find them. `shared_runtime` emits each module against one top-level `class Rt:` (Alias linkage) written to `rt.py`, which every module file imports — its class body binds `Rt` at import time — so an imported table crosses modules. Otherwise each file is a self-contained Embedded conversion, whose runtime class is `<Class>Rt`; the driver imports both names, which is what lets the glue name Alpha's trap type without touching Beta's.
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
                let (src, u) = dewasm_backend_python::generate_class_with_units(
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
                dir.join("rt.py"),
                dewasm_backend_python::shared_runtime(&units).expect("bundle runtime"),
            )
            .unwrap();
            imports.push("from rt import Rt".to_string());
            for (stem, name, src) in classes {
                std::fs::write(
                    dir.join(format!("{stem}.py")),
                    format!("from rt import Rt\n\n{src}"),
                )
                .unwrap();
                imports.push(format!("from {stem} import {name}"));
            }
        } else {
            for (wat, name) in modules {
                let stem = name.to_lowercase();
                std::fs::write(
                    dir.join(format!("{stem}.py")),
                    dewasm_test_helper::convert(
                        &PythonBackend,
                        &dewasm_test_helper::examples_dir().join(wat),
                        Mode::Library,
                        name,
                    ),
                )
                .unwrap();
                imports.push(format!("from {stem} import {name}, {name}Rt"));
            }
        }
        imports.join("\n")
    }
}

const PYTHON_ADD_GLUE: &str = r#"inst = Add()
print(inst.invoke("add", 2, 3))
print(inst.invoke("add", 0xffffffff, 1))
print(inst.invoke("fib", 10))
"#;

/// The override/fallback glue: fd_write intercepted, random_get falls back to the bundled WASI. Prints the actual bytes written.
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

/// The `custom_wasi_provider` glue: a provider *object* (`wasm_import`/`attach`, the Python analog of Ruby's duck-typed provider) covers every import, so the bundled WASI (`_wasi`) is never lazily constructed.
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

/// The `partial_override_falls_back_to_bundled_wasi` glue: fd_write intercepted, random_get falls back — so the bundled WASI *was* lazily constructed.
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

/// The `wasi_stdio_capture` glue: redirect `sys.stdout` (whose `.buffer` the bundled WASI captures on lazy construction) to a `BytesIO`, run, then print the captured bytes to the real stdout — the Python mirror of Ruby's StringIO idiom.
const PYTHON_STDIO_CAPTURE_GLUE: &str = r#"import io
import sys

_buf = io.BytesIO()
_orig = sys.stdout
sys.stdout = io.TextIOWrapper(_buf, write_through=True)
try:
    inst = Prog({})
    inst.invoke("_start")
except ProgRt.Exit:
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

/// The shared filesystem template: preopen the scratch dir (`{host}`) at guest `{guest}` (always `/`), run `_start`, and surface a `proc_exit` code as a trailing decimal line.
const PYTHON_FS_GLUE: &str = r#"inst = Prog({}, preopens={"{guest}": "{host}"})
try:
    inst.invoke("_start")
except ProgRt.Exit as e:
    print(e.code)
"#;

/// The root-preopen containment probe: call the WASI resolver directly with a `"/" => "/"` preopen (no guest run) and normalize the outcome to `contained`.
const PYTHON_CONTAINMENT_GLUE: &str = r#"wasi = ProgRt.WASI(preopens={"/": "/"})
_path, err = wasi.resolve_path(3, "etc")
print("contained" if err is None else "rejected")
"#;

// Filesystem app glue: class/argv/env/preopen-guest-paths are literals; only the host scratch/cache dirs come through {scratch}/{cache}.

const PYTHON_QJS_FILE_IO_GLUE: &str = r#"inst = Qjs({}, args=["qjs", "/work/qjs_file_io.js"], env={}, preopens={"/work": "{scratch}"})
try:
    inst.invoke("_start")
except QjsRt.Exit:
    pass
"#;

const PYTHON_SQLITE3_SHELL_GLUE: &str = r#"inst = Sqlite3Shell({}, args=["sqlite3"], env={}, preopens={"/db": "{scratch}"})
try:
    inst.invoke("_start")
except Sqlite3ShellRt.Exit:
    pass
"#;

const PYTHON_RG_SEARCH_GLUE: &str = r#"inst = Rg({}, args=["rg", "--sort", "path", "needle", "/work"], env={}, preopens={"/work": "{scratch}"})
try:
    inst.invoke("_start")
except RgRt.Exit:
    pass
"#;

const PYTHON_CPYTHON_GLUE: &str = r#"inst = Cpython({}, args=["python", "-c", "print('hello from cpython', 6 * 7)"], env={"PYTHONHOME": "/", "PYTHONPATH": "/lib/python3.14"}, preopens={"/lib": "{cache}/cpython-lib/lib"})
try:
    inst.invoke("_start")
except CpythonRt.Exit:
    pass
"#;

const PYTHON_CRUBY_GLUE: &str = r#"inst = Cruby({}, args=["ruby", "-e", "puts \"hello from cruby #{6*7}\""], env={}, preopens={"/usr": "{cache}/ruby-lib/usr"})
try:
    inst.invoke("_start")
except CrubyRt.Exit:
    pass
"#;

// C-API drive glue (sqlite3): malloc/pointer plumbing via the artifact's runtime Memory. Only the file-backed case uses {scratch}.

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

/// libpcap BPF filter compilation: drive `compile_filter` on "tcp port 80" (DLT_EN10MB, snaplen 65535), then walk the serialized program `[u32 bf_len][bf_len × {u16 code; u8 jt; u8 jf; u32 k}]` in guest memory, printing each instruction as `code jt jf k`.
const PYTHON_PCAP_COMPILE: &str = r#"
inst = Libpcap({})
inst.invoke("_initialize")
mem = inst.memory


def cstr(s):
    b = s.encode("utf-8") + b"\x00"
    p = inst.invoke("malloc", len(b))
    mem.init(p, b, 0, len(b))
    return p


prog = inst.invoke("compile_filter", cstr("tcp port 80"), 1, 65535)
assert prog != 0, "compile failed"
n = mem.i32_load(prog)
for i in range(n):
    base = prog + 4 + i * 8
    code = mem.i32_load16_u(base)
    jt = mem.i32_load8_u(base + 2)
    jf = mem.i32_load8_u(base + 3)
    k = mem.i32_load(base + 4)
    print("%d %d %d %d" % (code, jt, jf, k))
inst.invoke("free", prog)
print("BPF-OK")
"#;

/// tree-sitter JSON parse: drive `parse_source` on the fixed snippet `{"key": [1, true, null]}` and print the parse tree's S-expression (a malloc'd NUL-terminated C string) from guest memory.
const PYTHON_TREESITTER_PARSE: &str = r#"
inst = Treesitter({})
inst.invoke("_initialize")
mem = inst.memory


def cstr(s):
    b = s.encode("utf-8") + b"\x00"
    p = inst.invoke("malloc", len(b))
    mem.init(p, b, 0, len(b))
    return p


src = '{"key": [1, true, null]}'
b = src.encode("utf-8")
r = inst.invoke("parse_source", cstr(src), len(b))
assert r != 0, "parse failed"
end = mem.data.index(0, r)
print(mem.read_string(r, end - r).decode("utf-8"))
inst.invoke("free", r)
print("TS-OK")
"#;

/// zeroperl Perl-5.42 eval (issue #67): instantiate the reactor with a
/// zero-returning `env.call_host_function` import stub (only invoked when the
/// guest registers host callbacks — this program registers none) and a
/// `/dev/null` preopen (`zeroperl_init` returns 1 without it), then
/// `_initialize` → `zeroperl_init` → `malloc` + copy a Perl program into guest
/// memory → `zeroperl_eval` → `zeroperl_flush`. The program is a regex
/// capture and a `printf`, so its stdout is deterministic. The Perl source is a raw byte
/// literal: its backslash escapes belong to Perl, not to Python.
const PYTHON_ZEROPERL_EVAL: &str = r#"
inst = Zeroperl(
    {"env": {"call_host_function": lambda a, b, c: 0}},
    preopens={"/dev/null": "/dev/null"},
)
inst.invoke("_initialize")
rc = inst.invoke("zeroperl_init")
assert rc == 0, "zeroperl_init rc=%d" % rc
mem = inst.memory

prog = rb"""my $s = "hello world 42";
if ($s =~ /(\w+)\s+(\w+)\s+(\d+)/) {
  printf("m=%s|%s|%d sum=%d\n", $1, $2, $3, $3 + 8);
}
""" + b"\x00"
ptr = inst.invoke("malloc", len(prog))
mem.init(ptr, prog, 0, len(prog))
inst.invoke("zeroperl_eval", ptr, 0, 0, 0)
inst.invoke("zeroperl_flush")
"#;

/// ExifTool on zeroperl (issue #70): the flattened `exiftool` CLI driver
/// (`{cache}/exiftool-lib/exiftool`, preopened at `/work`) run on the same
/// `cache/zeroperl.wasm` reactor, whose SFS blob embeds the `Image::ExifTool`
/// module tree — so `use Image::ExifTool` resolves in-guest with no module
/// preopen. Instantiated like [`PYTHON_ZEROPERL_EVAL`] (the
/// `call_host_function` stub + a `/dev/null` preopen), plus the staged image at
/// `/img`. The Perl driver snippet sets `@ARGV`/`$0` and `do`es the script; it
/// first overrides `CORE::GLOBAL::exit` to a `die` so ExifTool's terminal
/// `exit` unwinds back into `eval_pv` instead of tripping `proc_exit` — then
/// `zeroperl_flush` pushes ExifTool's buffered stdout out through fd 1. Only
/// deterministic tags are requested (`-S -Make -Model -DateTimeOriginal`).
const PYTHON_EXIFTOOL: &str = r#"
inst = Zeroperl(
    {"env": {"call_host_function": lambda a, b, c: 0}},
    preopens={
        "/dev/null": "/dev/null",
        "/work": "{cache}/exiftool-lib",
        "/img": "{scratch}",
    },
)
inst.invoke("_initialize")
rc = inst.invoke("zeroperl_init")
assert rc == 0, "zeroperl_init rc=%d" % rc
mem = inst.memory

driver = rb"""BEGIN { *CORE::GLOBAL::exit = sub (;$) { die "zeroperl_exit\n" }; }
@ARGV = ('-S', '-Make', '-Model', '-DateTimeOriginal', '/img/exif_fixture.jpg');
$0 = '/work/exiftool';
do '/work/exiftool';
""" + b"\x00"
ptr = inst.invoke("malloc", len(driver))
mem.init(ptr, driver, 0, len(driver))
inst.invoke("zeroperl_eval", ptr, 0, 0, 0)
inst.invoke("zeroperl_flush")
"#;

/// Driver for the shared-table case: instantiate the exporter and the importer linked against it, then print `call0` (call_indirect through the shared table -> 42).
const PYTHON_SHARED_TABLE_GLUE: &str = r#"a = TableExp()
b = TableImp({"a": a})
print(b.invoke("call0"))
"#;

/// Driver for the embedded-coexistence case: two independent Embedded artifacts concatenated into one module. Each carries its own runtime class (`AlphaRt`/`BetaRt`), so their trap types are distinct objects and Alpha's trap is catchable by name.
const PYTHON_EMBEDDED_COEXIST_GLUE: &str = r#"a = Alpha()
b = Beta()
print(a.invoke("div", 7, 2))
print(b.invoke("div", 0xfffffff9, 2))
print("distinct-rt" if AlphaRt.Trap is not BetaRt.Trap else "same-rt")
try:
    a.invoke("div", 1, 0)
except AlphaRt.Trap:
    print("trapped")
"#;

/// DOOM: drive the converted library under the deterministic contract (synthetic clock, no input) and dump the framebuffer as a P6 PPM matching the wasmtime snapshot. `{ticks}`/`{clock_step}` are filled by the runner.
const PYTHON_DOOM_FRAME_GLUE: &str = r#"import sys

_frame = {"off": None, "w": 0, "h": 0}
_ms = {"v": 0}

def _clock():
    # Self-advancing per call: matches the oracle so startup/inter-tic spins
    # terminate and the frame stays deterministic.
    _ms["v"] += {clock_step}
    return _ms["v"]

IMPORTS = {
    "console": {"onErrorMessage": lambda o, n: None, "onInfoMessage": lambda o, n: None},
    "gameSaving": {
        "sizeOfSaveGame": lambda i: 0,
        "readSaveGame": lambda i, d: 0,
        "writeSaveGame": lambda i, s, n: n,
    },
    "runtimeControl": {"timeInMilliseconds": _clock},
    "ui": {"drawFrame": lambda off: _frame.__setitem__("off", off)},
    "loading": {
        "onGameInit": lambda w, h: (_frame.__setitem__("w", w), _frame.__setitem__("h", h)),
        "wadSizes": lambda a, b: None,
        "readWads": lambda a, b: None,
    },
}

doom = Doom(IMPORTS)
doom.invoke("initGame")
for _t in range(1, {ticks} + 1):
    doom.invoke("tickGame")

w, h = _frame["w"], _frame["h"]
off = _frame["off"]
frame = bytes(doom.memory.data[off:off + w * h * 4])
out = sys.stdout.buffer
out.write(b"P6\n%d %d\n255\n" % (w, h))
rgb = bytearray(w * h * 3)
j = 0
for i in range(0, len(frame), 4):
    rgb[j] = frame[i + 2]
    rgb[j + 1] = frame[i + 1]
    rgb[j + 2] = frame[i]
    j += 3
out.write(rgb)
out.flush()
"#;

/// NES (issue #114, mirrors the DOOM glue above): load the pinned ROM into
/// `allocRom`'s buffer, tick `{frames}` times with no input, compose the frame
/// from agnes's palette-index screen buffer and its palette (issue #117; the
/// `& 0x3f` mask is load-bearing) and dump it as a P6 PPM matching the wasmtime
/// snapshot. `{rom}` (the cached ROM's host path) and `{frames}` filled by the
/// runner.
const PYTHON_NES_FRAME_GLUE: &str = r#"import sys

nes = Nes()
nes.invoke("_initialize")
mem = nes.memory
rom = open("{rom}", "rb").read()
ptr = nes.invoke("allocRom", len(rom))
mem.init(ptr, rom, 0, len(rom))
ok = nes.invoke("initGame")
assert ok == 1, "initGame failed: %r" % (ok,)
for _t in range(1, {frames} + 1):
    nes.invoke("tickGame")

w = nes.invoke("frameWidth")
h = nes.invoke("frameHeight")
soff = nes.invoke("screenOffset")
poff = nes.invoke("paletteOffset")
screen = bytes(mem.data[soff:soff + w * h])
palette = bytes(mem.data[poff:poff + 64 * 4])
out = sys.stdout.buffer
out.write(b"P6\n%d %d\n255\n" % (w, h))
rgb = bytearray(w * h * 3)
j = 0
for ix in screen:
    c = (ix & 0x3F) * 4
    rgb[j] = palette[c]
    rgb[j + 1] = palette[c + 1]
    rgb[j + 2] = palette[c + 2]
    j += 3
out.write(rgb)
out.flush()
"#;

dewasm_test_helper::library_add_e2e!(Python, PYTHON_ADD_GLUE);
dewasm_test_helper::wasi_import_override_e2e!(Python, PYTHON_OVERRIDE_GLUE);
dewasm_test_helper::custom_wasi_provider_e2e!(Python, PYTHON_CUSTOM_PROVIDER_GLUE);
dewasm_test_helper::partial_override_e2e!(Python, PYTHON_PARTIAL_OVERRIDE_GLUE);
dewasm_test_helper::stdio_capture_e2e!(Python, PYTHON_STDIO_CAPTURE_GLUE);

dewasm_test_helper::wasi_suite!(Python, Stdio);
dewasm_test_helper::wasi_suite!(Python, ArgsEnv);
dewasm_test_helper::wasi_suite!(Python, Poll);
dewasm_test_helper::wasi_suite!(Python, Fs, PYTHON_FS_GLUE);
dewasm_test_helper::wasi_root_containment_e2e!(Python, PYTHON_CONTAINMENT_GLUE);
dewasm_test_helper::standalone_dir_e2e!(Python);
// The standalone entrypoint's recursion mitigation (issue #31).
dewasm_test_helper::deep_recursion_e2e!(Python);
dewasm_test_helper::folded_temp_reuse_e2e!(Python);

dewasm_test_helper::cowsay_args_e2e!(Python);
dewasm_test_helper::cowsay_stdin_e2e!(Python);
dewasm_test_helper::qjs_eval_e2e!(Python);
dewasm_test_helper::sqlite3_shell_e2e!(Python);
dewasm_test_helper::gzip_e2e!(Python);

dewasm_test_helper::qjs_file_io_e2e!(Python, PYTHON_QJS_FILE_IO_GLUE);
dewasm_test_helper::sqlite3_shell_dbfile_e2e!(Python, PYTHON_SQLITE3_SHELL_GLUE);
dewasm_test_helper::rg_search_e2e!(Python, PYTHON_RG_SEARCH_GLUE);
dewasm_test_helper::cpython_hello_e2e!(Python, PYTHON_CPYTHON_GLUE);
dewasm_test_helper::cruby_hello_e2e!(Python, PYTHON_CRUBY_GLUE);
// Ultra-slow category (issue #126): a CRuby-class program peaks at ~12 GB host-CPython RSS, and the
// e2e binary starts the alphabetically adjacent giants (cpython_hello, cruby_hello, this) on
// concurrent threads — three of them exhausted the 16 GB CI runner (SIGTERM, the #23 signature),
// where the pre-existing two fit. The packed case is the newcomer, so it leaves the CI run; it
// still runs on Ruby and under wasmtime in CI, and still converts here.
dewasm_test_helper::cruby_packed_hello_e2e!(Python, ultra);
dewasm_test_helper::qjs_repl_pty_e2e!(Python);

dewasm_test_helper::libsqlite3_c_api_e2e!(Python, PYTHON_LIBSQLITE3_MEM);
dewasm_test_helper::sqlite3_file_c_api_e2e!(Python, PYTHON_LIBSQLITE3_FILE);
dewasm_test_helper::sqlite3_callback_binding_e2e!(Python, PYTHON_SQLITE3_CALLBACK);
dewasm_test_helper::pcap_compile_e2e!(Python, PYTHON_PCAP_COMPILE);
dewasm_test_helper::treesitter_parse_e2e!(Python, PYTHON_TREESITTER_PARSE);
// Ultra-slow category (issue #139): the 25 MB zeroperl reactor becomes a ~97 MB / ~930k-line
// Python module, and host CPython peaks at ~4.9 GB RSS compiling it — the memory criterion that put
// the packed-CRuby case here (issue #126), and these would run on concurrent threads next to it.
// Wall times are 12 s (zeroperl_eval) and 42-67 s (exiftool_extract), so memory, not the clock, is
// what puts the eval case here; the two share the one oversized module.
dewasm_test_helper::zeroperl_eval_e2e!(Python, PYTHON_ZEROPERL_EVAL, ultra);
dewasm_test_helper::exiftool_extract_e2e!(Python, PYTHON_EXIFTOOL, ultra);

dewasm_test_helper::doom_frame_e2e!(Python, PYTHON_DOOM_FRAME_GLUE);
dewasm_test_helper::nes_frame_e2e!(Python, PYTHON_NES_FRAME_GLUE);

dewasm_test_helper::shared_table_e2e!(Python, PYTHON_SHARED_TABLE_GLUE);
dewasm_test_helper::embedded_coexist_e2e!(Python, PYTHON_EMBEDDED_COEXIST_GLUE);
