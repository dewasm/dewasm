//! Python end-to-end suites (ADR-27): the shared standalone / library / WASI /
//! apps case tables (`dewasm-test-helper`) wired up for the Python backend,
//! plus the filesystem-exercising app cases mirrored from the Ruby crate.
//!
//! Third-milestone scope (ADR-28): full WASI preview 1 including the
//! filesystem (ADR-14's model, mirrored one-for-one into
//! `runtime/python/units/wasi/`). This crate now wires every WASI kind Python
//! supports — the whole-program standalone kinds (`Stdio`, `ArgsEnv`) and the
//! library-mode `Fs` suite — plus `gzip_e2e!` (byte stdio) and the heavy
//! `apps` cases (QuickJS, SQLite) via `run_heavy_apps`. The Ruby-only fs app
//! scenarios (qjs file I/O + REPL, sqlite3 dbfile, ripgrep) are mirrored in
//! the tail of this file where they add signal for Python.

use std::path::{Path, PathBuf};

use dewasm_backend::{Backend, Mode};
use dewasm_backend_python::{find_python, PythonBackend};
use dewasm_test_helper::{
    apps_cache_dir, apps_e2e, apps_fixtures_dir, convert_on_big_stack, fresh_scratch_dir, gzip_e2e,
    library_e2e, run_script, standalone_e2e, wasi_suite, BackendUnderTest, LibraryCase, WasiCase,
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

    // The heavy apps (QuickJS, SQLite in-memory) now run to completion under
    // Python's full WASI surface (ADR-28 third milestone) and are fast enough
    // to run by default, like Ruby: measured locally qjs ~7 s and
    // sqlite3-shell ~8 s convert+run — well under the ADR-24 5-minute bar. The
    // much heavier filesystem app cases (rg's 22 MB wasm, the reconversion
    // cost of qjs/sqlite for the fs scenarios) stay behind DEWASM_APPS_ALL in
    // the tail of this file, mirroring how the Ruby crate gates CPython/CRuby.
    fn run_heavy_apps(&self) -> bool {
        true
    }
}

/// Instantiate a fs fixture with the scratch dir preopened at guest `/`, run
/// `_start`, and surface a `proc_exit` code as a trailing decimal line (via
/// `Rt.Exit`). One wrapper serves both stdout-reporting and proc_exit fixtures:
/// the former never raises `Rt.Exit`, so nothing extra is printed. Mirrors
/// `ruby_fs_glue`.
fn python_fs_glue(_case: &WasiCase, host: &Path) -> String {
    format!(
        "inst = Prog({{}}, preopens={{{:?}: {:?}}})\n\
         try:\n    inst.invoke(\"_start\")\nexcept Rt.Exit as e:\n    print(e.code)\n",
        "/",
        host.to_string_lossy()
    )
}

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
        other => panic!("{other}: no python glue"),
    }
}

/// The ADR-7 override/fallback glue (an explicit `fd_write` import wins,
/// `random_get` falls back to the bundled WASI). Mirrors the Ruby/Bash
/// override glues: intercept fd_write and print the actual bytes written.
const PYTHON_OVERRIDE_GLUE: &str = r#"_captured = bytearray()
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

standalone_e2e!(Python);
library_e2e!(Python, python_glue);
wasi_suite!(Python, Stdio);
wasi_suite!(Python, ArgsEnv);
wasi_suite!(Python, Fs, python_fs_glue);
apps_e2e!(Python);
gzip_e2e!(Python);

// ---------------------------------------------------------------------
// Filesystem-exercising app cases, mirrored from the Ruby crate now that
// Python has WASI filesystem support (ADR-28 third milestone, adopting
// ADR-14's model). Their goldens are the same wasmtime-ground-truthed files
// the Ruby cases use (crates/dewasm-cli/tests/apps_golden.rs), so the
// generated Python must produce byte-identical output.
//
// These are the heaviest cases in the suite (qjs/sqlite3 softfloat, rg's
// 22 MB wasm), so they are gated behind DEWASM_APPS_ALL — the same deliberate
// perf opt-out `run_heavy_apps` and the Ruby crate's language-runtime cases
// use (ADR-15's documented scope: a perf gate, not a missing-environment
// skip). The task's gate runs them via
// `DEWASM_APPS_ALL=1 cargo test -p dewasm-backend-python --test e2e`.

/// Whether the heavy filesystem app cases should actually run.
fn apps_all() -> bool {
    std::env::var("DEWASM_APPS_ALL").is_ok()
}

/// Convert a cached app binary to a library-mode Python class (roomy stack for
/// SQLite's deep functions, `convert_on_big_stack`).
fn convert_app_library(wasm: &str, class: &str) -> String {
    let path = apps_cache_dir().join(format!("{wasm}.wasm"));
    assert!(
        path.exists(),
        "{wasm} not cached — run examples/apps/fetch.sh (see docs/testing.md)"
    );
    let bytes = std::fs::read(&path).expect("read wasm");
    convert_on_big_stack(&PythonBackend, &bytes, Mode::Library, class)
}

/// Run `script` under `python` with `stdin`, returning the raw `Output`.
fn run_python_io(script: &str, stdin: &str) -> std::process::Output {
    let python = find_python().expect("python3 >= 3.9 not found on PATH — see docs/testing.md");
    run_script(&python, script, "py", &[], stdin)
}

/// QuickJS with file I/O (Phase 5a #1a, Ruby's `qjs_file_io_ruby`): the
/// `qjs:std` module writes a file into a preopened scratch dir, reads it back,
/// and prints it. Asserts both the guest stdout golden (captured from
/// wasmtime) and the host-side file content.
#[test]
fn qjs_file_io_python() {
    if !apps_all() {
        println!("qjs_file_io: heavy case skipped (DEWASM_APPS_ALL=1 to run)");
        return;
    }
    let work = fresh_scratch_dir("python-qjs-file-io");
    std::fs::copy(
        apps_fixtures_dir().join("qjs_file_io.js"),
        work.join("qjs_file_io.js"),
    )
    .unwrap();
    let class = convert_app_library("qjs", "Qjs");
    let glue = format!(
        "inst = Qjs({{}}, args=[\"qjs\", \"/work/qjs_file_io.js\"], env={{}}, preopens={{\"/work\": {:?}}})\n\
         try:\n    inst.invoke(\"_start\")\nexcept Rt.Exit:\n    pass\n",
        work.to_string_lossy()
    );
    let output = run_python_io(&format!("{class}\n{glue}"), "");
    assert!(
        output.status.success(),
        "qjs_file_io under python failed: {}\n{}",
        output.status,
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        include_str!("../../../examples/apps/golden/qjs_file_io.stdout"),
        "qjs_file_io: guest stdout differs from the wasmtime golden"
    );
    assert_eq!(
        std::fs::read_to_string(work.join("io_out.txt")).unwrap(),
        "hello from qjs file io\n",
        "qjs_file_io: the host file the guest wrote is wrong"
    );
}

/// QuickJS scripted REPL over piped stdin (Phase 5a #1b, Ruby's
/// `qjs_repl_ruby`): the pinned read-eval-print loop fixture exercises the
/// stdin-read + evalScript path byte-identically to wasmtime.
#[test]
fn qjs_repl_python() {
    if !apps_all() {
        println!("qjs_repl: heavy case skipped (DEWASM_APPS_ALL=1 to run)");
        return;
    }
    let work = fresh_scratch_dir("python-qjs-repl");
    std::fs::copy(
        apps_fixtures_dir().join("qjs_repl.js"),
        work.join("qjs_repl.js"),
    )
    .unwrap();
    let class = convert_app_library("qjs", "Qjs");
    let glue = format!(
        "inst = Qjs({{}}, args=[\"qjs\", \"/work/qjs_repl.js\"], env={{}}, preopens={{\"/work\": {:?}}})\n\
         try:\n    inst.invoke(\"_start\")\nexcept Rt.Exit:\n    pass\n",
        work.to_string_lossy()
    );
    let output = run_python_io(
        &format!("{class}\n{glue}"),
        "1+2\n[3,1,2].sort()\nMath.max(4,9)\n\\q\n",
    );
    assert!(
        output.status.success(),
        "qjs_repl under python failed: {}\n{}",
        output.status,
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        include_str!("../../../examples/apps/golden/qjs_repl.stdout"),
        "qjs_repl: guest stdout differs from the wasmtime golden"
    );
}

/// sqlite3 shell reading/writing a DB *file* (Phase 5a #2a, Ruby's
/// `sqlite3_shell_dbfile_ruby`): one invocation creates and populates
/// `/db/test.db`, a second reopens it and SELECTs. Asserts the second run's
/// stdout (golden) and that the DB file exists on the host with nonzero size.
#[test]
fn sqlite3_shell_dbfile_python() {
    if !apps_all() {
        println!("sqlite3_shell_dbfile: heavy case skipped (DEWASM_APPS_ALL=1 to run)");
        return;
    }
    let db = fresh_scratch_dir("python-sqlite3-dbfile");
    let class = convert_app_library("sqlite3-shell", "Sqlite3Shell");
    let glue = format!(
        "inst = Sqlite3Shell({{}}, args=[\"sqlite3\"], env={{}}, preopens={{\"/db\": {:?}}})\n\
         try:\n    inst.invoke(\"_start\")\nexcept Rt.Exit:\n    pass\n",
        db.to_string_lossy()
    );
    let program = format!("{class}\n{glue}");

    let create = run_python_io(
        &program,
        ".open /db/test.db\n\
         CREATE TABLE t(id INTEGER PRIMARY KEY, v TEXT);\n\
         INSERT INTO t(v) VALUES ('alpha'),('beta');\n\
         .exit\n",
    );
    assert!(
        create.status.success(),
        "sqlite3 dbfile create under python failed: {}\n{}",
        create.status,
        String::from_utf8_lossy(&create.stderr)
    );
    let db_file = db.join("test.db");
    assert!(
        db_file.metadata().map(|m| m.len() > 0).unwrap_or(false),
        "sqlite3 dbfile: the first run left no nonzero DB file"
    );

    let select = run_python_io(
        &program,
        ".open /db/test.db\nSELECT id, v FROM t ORDER BY id;\n.exit\n",
    );
    assert!(
        select.status.success(),
        "sqlite3 dbfile select under python failed: {}\n{}",
        select.status,
        String::from_utf8_lossy(&select.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&select.stdout),
        include_str!("../../../examples/apps/golden/sqlite3_shell_dbfile.stdout"),
        "sqlite3_shell_dbfile: reopened SELECT stdout differs from the wasmtime golden"
    );
}

/// Recursively copy `src` into `dst` (both directories), used to stage the
/// committed ripgrep fixture tree into a fresh scratch dir before the run.
fn copy_tree(src: &Path, dst: &Path) {
    std::fs::create_dir_all(dst).unwrap();
    for entry in std::fs::read_dir(src).unwrap() {
        let entry = entry.unwrap();
        let to = dst.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_tree(&entry.path(), &to);
        } else {
            std::fs::copy(entry.path(), &to).unwrap();
        }
    }
}

/// ripgrep searching a small fixture directory tree (Phase 5b, Ruby's
/// `rg_search_ruby`): recursive directory walking (fd_readdir, path_open,
/// path_filestat_get) over a preopened tree. `--sort path` forces a
/// deterministic order so the `wasmtime --dir` golden is stable.
#[test]
fn rg_search_python() {
    if !apps_all() {
        println!("rg_search: heavy case skipped (DEWASM_APPS_ALL=1 to run)");
        return;
    }
    let work = fresh_scratch_dir("python-rg-search");
    copy_tree(&apps_fixtures_dir().join("rg"), &work);
    let class = convert_app_library("rg", "Rg");
    let glue = format!(
        "inst = Rg({{}}, args=[\"rg\", \"--sort\", \"path\", \"needle\", \"/work\"], env={{}}, preopens={{\"/work\": {:?}}})\n\
         try:\n    inst.invoke(\"_start\")\nexcept Rt.Exit:\n    pass\n",
        work.to_string_lossy()
    );
    let output = run_python_io(&format!("{class}\n{glue}"), "");
    assert!(
        output.status.success(),
        "rg_search under python failed: {}\n{}",
        output.status,
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        include_str!("../../../examples/apps/golden/rg_search.stdout"),
        "rg_search: guest stdout differs from the wasmtime golden"
    );
}
