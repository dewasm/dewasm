//! Bash side of the official WASI p1 conformance harness (ADR-36): drives the prebuilt `WebAssembly/wasi-testsuite` modules through the Bash backend's standalone interface (real WASI filesystem, ADR-34; status-cascade exit codes, ADR-11/12). The generic harness lives in `dewasm-test-helper`.

use std::path::PathBuf;

use dewasm_backend::Backend;
use dewasm_backend_bash::BashBackend;
use dewasm_test_helper::BackendUnderTest;

/// Known trial failures with their attribution (ADR-8, policy in ADR-36): `(trial, tag)` — out-of-scope syscalls (timestamps, sockets), the D3 file-symlink-follow limit (pure bash has no `readlink` for path resolution, ADR-34), stat-precision gaps with no `stat` license (dev/ino), the D1 whole-file-buffer divergence, and environ entries bash itself exports (PWD/SHLVL/_), which count-exact `environ_*` assertions cannot absorb (ADR-40).
const WASI_TESTSUITE_EXPECTED_FAILURES: &[(&str, &str)] = &[
    // Sockets — out of scope (docs/support.md).
    ("c/sock_shutdown-invalid_fd", "sock_shutdown (out of scope)"),
    ("c/sock_shutdown-not_sock", "sock_shutdown (out of scope)"),
    // Timestamp setters — deliberately not implemented: `touch` fails the ADR-34 D2 mutation-license criterion, so fd/path_filestat_set_times and the tests that exercise them stay ENOSYS.
    ("rust/fd_filestat_set", "fd_filestat_set_times (ENOSYS)"),
    ("rust/fstflags_validate", "fd_filestat_set_times (ENOSYS)"),
    ("rust/path_filestat", "path_filestat_set_times (ENOSYS)"),
    ("rust/symlink_filestat", "path_filestat_set_times (ENOSYS)"),
    // D3: a file symlink cannot be followed in pure bash (no readlink builtin for path *resolution*), so following one during path_open/path_filestat resolves to ELOOP where a real host would reach the pointee (ADR-34 D3).
    (
        "rust/symlink_create",
        "path resolution: file-symlink follow ELOOP (ADR-34 D3)",
    ),
    (
        "rust/path_exists",
        "path resolution: file-symlink follow ELOOP (ADR-34 D3)",
    ),
    (
        "rust/nofollow_errors",
        "path resolution: file-symlink follow ELOOP (ADR-34 D3)",
    ),
    // No `stat` license (ADR-34 D6): dev/ino are always 0, so the tests that assert per-entry inode/device uniqueness cannot pass.
    (
        "rust/fd_readdir",
        "fd_readdir: d_ino uniqueness (no stat license)",
    ),
    ("c/fdopendir-with-access", "fd_readdir: d_ino uniqueness"),
    ("c/stat-dev-ino", "path_filestat_get: st_ino uniqueness"),
    // D1 whole-file-buffer model: two fds on one file each hold their own buffer, so an unbuffered cross-fd read-back sees 0 bytes (ADR-34 D1).
    (
        "rust/file_unbuffered_write",
        "fd_read: unbuffered read-back returns 0",
    ),
    // Bash itself exports PWD/SHLVL/_ into every script's environment, so count-exact environ assertions cannot hold even under the harness's cleared environment (ADR-40).
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
            "bash >= 5 not found on PATH, $DEWASM_BASH, or a Homebrew path — see docs/testing.md",
        )
    }
}

impl dewasm_test_helper::WasiTestsuiteBackend for BashWasi {
    fn expected_failures(&self) -> &'static [(&'static str, &'static str)] {
        WASI_TESTSUITE_EXPECTED_FAILURES
    }
}

dewasm_test_helper::wasi_testsuite_suite!(BashWasi);
