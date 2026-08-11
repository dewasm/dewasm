//! Go side of the official WASI p1 conformance harness: drives the prebuilt `WebAssembly/wasi-testsuite` modules through the Go backend's standalone interface. Go is compiled, so it overrides `pty_command` to `go build` the generated program to a content-addressed cache binary, the launch recipe the shared `run_standalone_wasi` runs with the manifest's env/args/dirs applied. The generic harness lives in `dewasm-test-helper`.

use dewasm_backend::Backend;
use dewasm_backend_go::GoBackend;
use dewasm_test_helper::BackendUnderTest;

mod common;

/// Known trial failures with their attribution `(trial, tag)`: out-of-scope syscalls and the one std-portability gap the full WASI p1 filesystem support cannot close on Go.
const WASI_TESTSUITE_EXPECTED_FAILURES: &[(&str, &str)] = &[
    // No socket layer in a demo runtime (out of scope, docs/support.md).
    ("c/sock_shutdown-invalid_fd", "sock_shutdown (out of scope)"),
    ("c/sock_shutdown-not_sock", "sock_shutdown (out of scope)"),
    // Setting a symlink's own times (NOFOLLOW) needs lutimes, which Go's std exposes no portable (darwin+linux, build-tag-free) way to call: os.Chtimes follows the link. Every regular-file times path is supported; only the symlink-target case in this one trial is out of reach.
    (
        "rust/symlink_filestat",
        "path_filestat_set_times: no portable std lutimes for a NOFOLLOW symlink",
    ),
];

struct GoWasi;

impl BackendUnderTest for GoWasi {
    fn name(&self) -> &'static str {
        "go"
    }

    fn backend(&self) -> &'static (dyn Backend + Sync) {
        &GoBackend
    }

    /// Build `source` to the crate's shared cache binary and return the run recipe. A build failure panics (generated code that does not compile is a bug, not a WASI gap).
    fn pty_command(&self, source: &str, args: &[&str]) -> dewasm_test_helper::PtyCommand {
        let bin = common::build_go(source).unwrap_or_else(|build| {
            panic!(
                "go build failed:\n{}",
                String::from_utf8_lossy(&build.stderr)
            )
        });
        dewasm_test_helper::PtyCommand {
            program: bin,
            args: args.iter().map(|a| a.to_string()).collect(),
            cwd: None,
        }
    }
}

impl dewasm_test_helper::WasiTestsuiteBackend for GoWasi {
    fn expected_failures(&self) -> &'static [(&'static str, &'static str)] {
        WASI_TESTSUITE_EXPECTED_FAILURES
    }
}

dewasm_test_helper::wasi_testsuite_suite!(GoWasi);
