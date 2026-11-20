//! Python side of the official WASI p1 conformance harness.
//! It drives the prebuilt `WebAssembly/wasi-testsuite` modules through the Python backend.
//! The modules go through the backend's standalone interface.
//! The generic harness lives in `dewasm-test-helper`.

use std::path::PathBuf;

use dewasm_backend::Backend;
use dewasm_backend_python::PythonBackend;
use dewasm_test_helper::BackendUnderTest;

/// Known trial failures with their attribution, `(trial, tag)`.
/// The one cause is the out-of-scope `sock_shutdown` system call (`docs/support.md`).
const WASI_TESTSUITE_EXPECTED_FAILURES: &[(&str, &str)] = &[
    ("c/sock_shutdown-invalid_fd", "sock_shutdown (out of scope)"),
    ("c/sock_shutdown-not_sock", "sock_shutdown (out of scope)"),
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
