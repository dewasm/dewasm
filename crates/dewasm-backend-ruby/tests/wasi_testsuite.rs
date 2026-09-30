//! Ruby side of the official WASI p1 conformance harness.
//! It drives the prebuilt `WebAssembly/wasi-testsuite` modules through the Ruby backend.
//! The modules go through the backend's standalone interface.
//! The generic harness lives in `dewasm-test-helper`.

use std::path::PathBuf;

use dewasm_backend::Backend;
use dewasm_backend_ruby::RubyBackend;
use dewasm_test_helper::BackendUnderTest;

/// Known trial failures with their attribution: `(trial, tag)`.
/// Two kinds remain, both attributed:
/// - Declared out-of-scope syscalls (`sock_shutdown`; docs/support.md).
///   Filling the gap later flips the entry to a hard failure.
/// - Environment variables the host interpreter itself injects.
///   One is macOS CoreFoundation's `__CF_USER_TEXT_ENCODING`.
///   The guest legitimately observes it.
///   So count-exact `environ_*` assertions cannot hold.
///   That holds even though the harness runs trials with a cleared environment.
const WASI_TESTSUITE_EXPECTED_FAILURES: &[(&str, &str)] = &[
    // Declared out-of-scope syscalls.
    ("c/sock_shutdown-invalid_fd", "sock_shutdown (out of scope)"),
    ("c/sock_shutdown-not_sock", "sock_shutdown (out of scope)"),
];

/// Host-scoped failures on a macOS host.
/// macOS CoreFoundation injects `__CF_USER_TEXT_ENCODING` into the CF-linked ruby process.
/// So the guest sees one extra environ entry, and count-exact `environ_*` assertions cannot hold.
/// Plain Linux ruby injects nothing, so these pass there.
const WASI_TESTSUITE_EXPECTED_FAILURES_MACOS: &[(&str, &str)] = &[
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

    fn expected_failures_macos(&self) -> &'static [(&'static str, &'static str)] {
        WASI_TESTSUITE_EXPECTED_FAILURES_MACOS
    }
}

dewasm_test_helper::wasi_testsuite_suite!(RubyWasi);
