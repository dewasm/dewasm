//! Perl side of the official WASI p1 conformance harness.
//! It drives the prebuilt `WebAssembly/wasi-testsuite` modules through the Perl backend.
//! The modules run through its standalone interface.
//! The generic harness lives in `dewasm-test-helper`.

use std::path::PathBuf;

use dewasm_backend::Backend;
use dewasm_backend_perl::PerlBackend;
use dewasm_test_helper::BackendUnderTest;

/// Known trial failures with their attribution, `(trial, tag)`.
/// Like Ruby/Python, one is the out-of-scope `sock_shutdown` syscall (docs/support.md).
/// Unlike the Ruby/Python hosts, perl injects no environ entries of its own.
/// So the `environ_*` trials pass without host-scoped entries.
const WASI_TESTSUITE_EXPECTED_FAILURES: &[(&str, &str)] = &[
    // Declared out-of-scope syscall (docs/support.md).
    ("c/sock_shutdown-invalid_fd", "sock_shutdown (out of scope)"),
    ("c/sock_shutdown-not_sock", "sock_shutdown (out of scope)"),
    // Core perl's only sub-second file-time APIs (Time::HiRes utime/stat) pass NV seconds.
    // Their resolution at the current epoch is ~400ns.
    // So the suite's set-then-get of `mtim - 100` nanoseconds cannot round-trip exactly.
    // The runtime unit comments carry the same attribution.
    (
        "rust/fd_filestat_set",
        "filestat times: perl NV-seconds utime caps precision below ns",
    ),
    (
        "rust/path_filestat",
        "filestat times: perl NV-seconds utime caps precision below ns",
    ),
    // Core perl has no lutimes/utimensat(AT_SYMLINK_NOFOLLOW).
    // So a final-component symlink cannot carry its own times.
    // The suite's set-then-lstat on the link itself then cannot hold.
    // The Linux JDK list has the microsecond analog of this gap.
    (
        "rust/symlink_filestat",
        "path_filestat_set_times NOFOLLOW: core perl lacks lutimes/utimensat",
    ),
];

struct PerlWasi;

impl BackendUnderTest for PerlWasi {
    fn name(&self) -> &'static str {
        "perl"
    }

    fn backend(&self) -> &'static (dyn Backend + Sync) {
        &PerlBackend
    }

    fn interpreter(&self) -> PathBuf {
        dewasm_backend_perl::find_perl()
            .expect("perl >= 5.26 with 64-bit IVs/NVs not found on PATH: see docs/testing.md")
    }
}

impl dewasm_test_helper::WasiTestsuiteBackend for PerlWasi {
    fn expected_failures(&self) -> &'static [(&'static str, &'static str)] {
        WASI_TESTSUITE_EXPECTED_FAILURES
    }
}

dewasm_test_helper::wasi_testsuite_suite!(PerlWasi);
