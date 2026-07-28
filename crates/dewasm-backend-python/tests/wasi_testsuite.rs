//! Python side of the official WASI p1 conformance harness (ADR-36): drives the
//! prebuilt `WebAssembly/wasi-testsuite` modules through the Python backend's
//! standalone interface. The generic harness lives in `dewasm-test-helper`.

use std::path::PathBuf;

use dewasm_backend::Backend;
use dewasm_backend_python::PythonBackend;
use dewasm_test_helper::{wasi_testsuite_suite, BackendUnderTest, WasiTestsuiteBackend};

/// Known trial failures with their attribution (ADR-8, policy in ADR-36):
/// `(trial, tag)` — declared ENOSYS/out-of-scope syscalls, semantics-precision
/// gaps on supported syscalls (tracked bugs in the shared WASI runtime), and
/// the ADR-31 whole-environment passthrough the `environ_*` count assertions
/// cannot satisfy.
const WASI_TESTSUITE_EXPECTED_FAILURES: &[(&str, &str)] = &[
    // Declared ENOSYS / out-of-scope syscalls (docs/support.md).
    ("c/sock_shutdown-invalid_fd", "sock_shutdown (out of scope)"),
    ("c/sock_shutdown-not_sock", "sock_shutdown (out of scope)"),
    ("rust/fd_advise", "fd_advise (ENOSYS)"),
    ("rust/fd_fdstat_set_rights", "fd_fdstat_set_rights (ENOSYS)"),
    ("rust/fd_flags_set", "fd_fdstat_set_flags (ENOSYS)"),
    ("rust/fd_filestat_set", "fd_filestat_set_times (ENOSYS)"),
    ("rust/fstflags_validate", "fd_filestat_set_times (ENOSYS)"),
    ("rust/file_allocate", "fd_allocate (ENOSYS)"),
    ("rust/path_link", "path_link (ENOSYS)"),
    ("rust/readlink", "path_readlink (ENOSYS)"),
    ("rust/renumber", "fd_renumber (ENOSYS)"),
    ("rust/stdio", "fd_renumber (ENOSYS)"),
    ("rust/overwrite_preopen", "fd_renumber (ENOSYS)"),
    ("rust/symlink_create", "path_symlink (ENOSYS)"),
    ("rust/symlink_filestat", "path_symlink (ENOSYS)"),
    (
        "rust/path_symlink_trailing_slashes",
        "path_symlink (ENOSYS)",
    ),
    ("rust/path_exists", "path_symlink (ENOSYS)"),
    ("rust/nofollow_errors", "path_symlink (ENOSYS)"),
    ("rust/dir_fd_op_failures", "fd_advise+fd_allocate (ENOSYS)"),
    // Semantics-precision gaps on supported syscalls (tracked bugs, ADR-36).
    (
        "rust/truncation_rights",
        "fd_fdstat_get: per-filetype rights not masked",
    ),
    (
        "rust/directory_seek",
        "fd_fdstat_get: per-filetype rights not masked",
    ),
    (
        "rust/path_open_read_write",
        "fd_fdstat_get: per-open rights not masked",
    ),
    (
        "rust/path_filestat",
        "fd_fdstat_get: open fdflags (APPEND) not reflected",
    ),
    (
        "rust/fd_readdir",
        "fd_readdir: '.'/'..' dot-entries + d_ino",
    ),
    (
        "rust/path_open_preopen",
        "path_open: rights-restricted reopen",
    ),
    (
        "rust/interesting_paths",
        "path_open: absolute / '..' path resolution",
    ),
    (
        "rust/path_open_dirfd_not_dir",
        "path_open: non-dir base errno NOTDIR vs BADF",
    ),
    (
        "rust/file_seek_tell",
        "fd_seek: negative-offset errno INVAL vs IO",
    ),
    // Python-specific runtime quirks (tracked bugs).
    (
        "rust/path_rename_dir_trailing_slashes",
        "path_rename: trailing-slash handling",
    ),
    (
        "rust/path_open_nonblock",
        "path_open: O_NONBLOCK on a directory (ISDIR)",
    ),
    // ADR-31: a standalone program inherits the whole host environment.
    (
        "assemblyscript/environ_get-multiple-variables",
        "env-passthrough (ADR-31)",
    ),
    (
        "assemblyscript/environ_sizes_get-multiple-variables",
        "env-passthrough (ADR-31)",
    ),
    (
        "assemblyscript/environ_sizes_get-no-variables",
        "env-passthrough (ADR-31)",
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
            .expect("python3 >= 3.9 not found on PATH — see docs/testing.md")
    }
}

impl WasiTestsuiteBackend for PythonWasi {
    fn expected_failures(&self) -> &'static [(&'static str, &'static str)] {
        WASI_TESTSUITE_EXPECTED_FAILURES
    }
}

wasi_testsuite_suite!(PythonWasi);
