//! Python end-to-end suites (ADR-27): the shared standalone / library / WASI /
//! apps case tables (`dewasm-test-helper`) wired up for the Python backend.
//!
//! Third-milestone scope (ADR-28): full WASI preview 1 including the
//! filesystem (ADR-14's model, mirrored one-for-one into
//! `runtime/python/units/wasi/`). This crate now wires every WASI kind Python
//! supports — the whole-program standalone kinds (`Stdio`, `ArgsEnv`) and the
//! library-mode `Fs` suite — plus `gzip_e2e!` (byte stdio), the heavy `apps`
//! cases (QuickJS, SQLite) via `run_heavy_apps`, and the shared filesystem app
//! cases (`fs_apps_e2e!` over `FS_APP_CASES`, via `app_glue`).

use std::path::{Path, PathBuf};

use dewasm_backend::Backend;
use dewasm_backend_python::{find_python, PythonBackend};
use dewasm_test_helper::{
    apps_e2e, fs_apps_e2e, gzip_e2e, library_e2e, qjs_repl_pty_e2e, standalone_e2e, wasi_suite,
    BackendUnderTest, LibraryCase, WasiCase,
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
    // cost of qjs/sqlite for the fs scenarios) live in the shared
    // `FS_APP_CASES` table and stay behind DEWASM_APPS_ALL (`fs_apps_e2e!`).
    fn run_heavy_apps(&self) -> bool {
        true
    }

    /// Instantiate `class` with kwargs (args/env/preopens), run `_start`, and
    /// swallow a clean guest `proc_exit` (`Rt.Exit`). Generalizes the
    /// hand-written glue the mirrored fs app tests used.
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
wasi_suite!(Python, Poll);
wasi_suite!(Python, Fs, python_fs_glue);
apps_e2e!(Python);
gzip_e2e!(Python);
fs_apps_e2e!(Python);
qjs_repl_pty_e2e!(Python);
