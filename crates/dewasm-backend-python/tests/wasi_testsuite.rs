//! Python side of the official WASI p1 conformance harness: drives the prebuilt `WebAssembly/wasi-testsuite` modules through the Python backend's standalone interface. The generic harness lives in `dewasm-test-helper`.

use std::path::PathBuf;

use dewasm_backend::Backend;
use dewasm_backend_python::PythonBackend;
use dewasm_test_helper::BackendUnderTest;

/// Known trial failures with their attribution, `(trial, tag)`: the out-of-scope `sock_shutdown` syscall and the environ entries the CPython host injects itself, which count-exact `environ_*` assertions cannot absorb.
const WASI_TESTSUITE_EXPECTED_FAILURES: &[(&str, &str)] = &[
    // Declared out-of-scope syscall (docs/support.md).
    ("c/sock_shutdown-invalid_fd", "sock_shutdown (out of scope)"),
    ("c/sock_shutdown-not_sock", "sock_shutdown (out of scope)"),
    // The CPython host injects environ entries of its own (macOS CoreFoundation's __CF_USER_TEXT_ENCODING plus the PEP 538 LC_CTYPE locale coercion), so count-exact environ assertions cannot hold even under the harness's cleared environment.
    (
        "assemblyscript/environ_get-multiple-variables",
        "environ: host-interpreter env injection",
    ),
    (
        "assemblyscript/environ_sizes_get-multiple-variables",
        "environ: host-interpreter env injection",
    ),
    (
        "assemblyscript/environ_sizes_get-no-variables",
        "environ: host-interpreter env injection",
    ),
];

struct PythonWasi;

impl BackendUnderTest for PythonWasi {
    fn name(&self) -> &'static str {
        "python"
    }

    fn backend(&self) -> &'static (dyn Backend + Sync) {
        &PythonBackend
    }

    fn interpreter(&self) -> PathBuf {
        dewasm_backend_python::find_python()
            .expect("python3 >= 3.9 not found on PATH: see docs/testing.md")
    }
}

impl dewasm_test_helper::WasiTestsuiteBackend for PythonWasi {
    fn expected_failures(&self) -> &'static [(&'static str, &'static str)] {
        WASI_TESTSUITE_EXPECTED_FAILURES
    }
}

dewasm_test_helper::wasi_testsuite_suite!(PythonWasi);
