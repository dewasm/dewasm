//! Python end-to-end suites (ADR-27): the shared standalone / library / WASI /
//! apps case tables (`dewasm-test-helper`) wired up for the Python backend.
//!
//! First-milestone scope (ADR-24, ADR-28): "cowsay runs". The bundled WASI is
//! the eight core syscalls cowsay needs (args/environ, fd_read/fd_write,
//! proc_exit, random_get), so this crate wires the whole-program standalone
//! WASI kinds (`Stdio`, `ArgsEnv`) and the `apps` cowsay cases, but not the
//! filesystem suite (no WASI fs yet) nor `gzip_e2e!` (minigzip needs
//! fd_fdstat_get/fd_prestat_*/fd_seek/path_open, out of this milestone's
//! scope). The spec harness is a later milestone (ADR-28).

use std::path::PathBuf;

use dewasm_backend::Backend;
use dewasm_backend_python::{find_python, PythonBackend};
use dewasm_test_helper::{
    apps_e2e, library_e2e, standalone_e2e, wasi_suite, BackendUnderTest, LibraryCase,
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

    // The heavy apps (QuickJS, SQLite) convert and compile under Python, but
    // their WASI surface (filesystem, fd_fdstat_get, ...) is beyond the
    // cowsay milestone, so their execution is not asserted in the default
    // gate. Opt in with DEWASM_APPS_ALL to attempt them anyway (ADR-15's
    // perf/scope opt-out, not a missing-environment skip).
    fn run_heavy_apps(&self) -> bool {
        false
    }
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
apps_e2e!(Python);
