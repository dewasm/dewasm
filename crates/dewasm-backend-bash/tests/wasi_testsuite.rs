//! Bash side of the official WASI p1 conformance harness.
//! It drives the prebuilt `WebAssembly/wasi-testsuite` modules.
//! They run through the Bash backend's standalone interface.
//! That interface has a real WASI file system and status chain exit codes.
//! The generic harness lives in `dewasm-test-helper`.

use std::path::PathBuf;

use dewasm_backend::Backend;
use dewasm_backend_bash::BashBackend;
use dewasm_test_helper::BackendUnderTest;

/// Known trial failures with their attribution `(trial, tag)`:
///
/// - out-of-scope system calls (timestamps, sockets);
/// - the limit on following a file symbolic link (pure Bash has no `readlink` for path resolution);
/// - gaps in `stat` precision with no `stat` license (`dev`/`ino`);
/// - the whole-file-buffer difference;
/// - `environ` entries Bash itself exports (PWD/SHLVL/_).
///   Count-exact `environ_*` assertions cannot absorb those.
const WASI_TESTSUITE_EXPECTED_FAILURES: &[(&str, &str)] = &[
    // Sockets: out of scope (`docs/support.md`).
    ("c/sock_shutdown-invalid_fd", "sock_shutdown (out of scope)"),
    ("c/sock_shutdown-not_sock", "sock_shutdown (out of scope)"),
    // Timestamp setters are deliberately not implemented.
    // `touch` is not a namespace-mutation operation.
    // So it falls outside the narrow license that lets Bash shell out to `mkdir`/`rmdir`/`rm`/`mv`.
    // `fd_filestat_set_times`, `path_filestat_set_times`, and their tests therefore stay ENOSYS.
    ("rust/fd_filestat_set", "fd_filestat_set_times (ENOSYS)"),
    ("rust/fstflags_validate", "fd_filestat_set_times (ENOSYS)"),
    ("rust/path_filestat", "path_filestat_set_times (ENOSYS)"),
    ("rust/symlink_filestat", "path_filestat_set_times (ENOSYS)"),
    // Pure Bash cannot follow a file symbolic link (no `readlink` builtin for path *resolution*).
    // So following one during `path_open`/`path_filestat` resolves to ELOOP.
    // A real host would reach its target.
    (
        "rust/symlink_create",
        "path resolution: file-symlink follow ELOOP",
    ),
    (
        "rust/path_exists",
        "path resolution: file-symlink follow ELOOP",
    ),
    (
        "rust/nofollow_errors",
        "path resolution: file-symlink follow ELOOP",
    ),
    // No `stat` license: `dev`/`ino` are always 0.
    // So the tests that assert per-entry inode/device uniqueness cannot pass.
    (
        "rust/fd_readdir",
        "fd_readdir: d_ino uniqueness (no stat license)",
    ),
    ("c/fdopendir-with-access", "fd_readdir: d_ino uniqueness"),
    ("c/stat-dev-ino", "path_filestat_get: st_ino uniqueness"),
    // The whole-file-buffer model: two file descriptors on one file each hold their own buffer.
    // So an unbuffered read-back across file descriptors sees 0 bytes.
    (
        "rust/file_unbuffered_write",
        "fd_read: unbuffered read-back returns 0",
    ),
    // Bash itself exports PWD/SHLVL/_ into every script's environment.
    // So count-exact `environ` assertions cannot hold even under the harness's cleared environment.
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

struct BashWasi;

impl BackendUnderTest for BashWasi {
    fn name(&self) -> &'static str {
        "bash"
    }

    fn backend(&self) -> &'static (dyn Backend + Sync) {
        &BashBackend
    }

    fn interpreter(&self) -> PathBuf {
        dewasm_backend_bash::find_bash5().expect(
            "bash >= 5 not found on PATH, $DEWASM_BASH, or a Homebrew path: see docs/testing.md",
        )
    }
}

impl dewasm_test_helper::WasiTestsuiteBackend for BashWasi {
    fn expected_failures(&self) -> &'static [(&'static str, &'static str)] {
        WASI_TESTSUITE_EXPECTED_FAILURES
    }
}

dewasm_test_helper::wasi_testsuite_suite!(BashWasi);
