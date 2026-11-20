//! Ruby side of the official WASI p1 conformance harness.
//! It drives the prebuilt `WebAssembly/wasi-testsuite` modules through the Ruby backend.
//! The modules go through the backend's standalone interface.
//! The generic harness lives in `dewasm-test-helper`.

use std::path::PathBuf;

use dewasm_backend::Backend;
use dewasm_backend_ruby::RubyBackend;
use dewasm_test_helper::BackendUnderTest;

/// Known trial failures with their attribution: `(trial, tag)`.
/// One kind remains: declared out-of-scope system calls (`sock_shutdown`; `docs/support.md`).
/// Filling the gap later flips the entry to a hard failure.
const WASI_TESTSUITE_EXPECTED_FAILURES: &[(&str, &str)] = &[
    ("c/sock_shutdown-invalid_fd", "sock_shutdown (out of scope)"),
    ("c/sock_shutdown-not_sock", "sock_shutdown (out of scope)"),
];

struct RubyWasi;

impl BackendUnderTest for RubyWasi {
    fn name(&self) -> &'static str {
        "ruby"
    }

    fn backend(&self) -> &'static (dyn Backend + Sync) {
        &RubyBackend
    }

    fn interpreter(&self) -> PathBuf {
        dewasm_backend_ruby::find_ruby()
            .expect("ruby not found on PATH (or $DEWASM_RUBY): see docs/testing.md")
    }
}

impl dewasm_test_helper::WasiTestsuiteBackend for RubyWasi {
    fn expected_failures(&self) -> &'static [(&'static str, &'static str)] {
        WASI_TESTSUITE_EXPECTED_FAILURES
    }
}

dewasm_test_helper::wasi_testsuite_suite!(RubyWasi);
