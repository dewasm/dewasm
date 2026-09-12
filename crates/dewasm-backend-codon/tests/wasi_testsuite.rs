//! Codon side of the official WASI p1 conformance harness: drives the prebuilt `WebAssembly/wasi-testsuite` modules through the Codon backend's standalone interface.
//! Codon is compiled, so it overrides `pty_command` to `codon build` the generated program to a content-addressed cache binary (with the runtime dylibs copied beside it, so the manifest-only child environment needs no loader-path variable).
//! The generic harness lives in `dewasm-test-helper`.

use dewasm_backend::Backend;
use dewasm_backend_codon::CodonBackend;
use dewasm_test_helper::BackendUnderTest;

mod common;

/// Known trial failures with their attribution `(trial, tag)`.
const WASI_TESTSUITE_EXPECTED_FAILURES: &[(&str, &str)] = &[
    // No socket layer in a demo runtime (out of scope, docs/support.md).
    ("c/sock_shutdown-invalid_fd", "sock_shutdown (out of scope)"),
    ("c/sock_shutdown-not_sock", "sock_shutdown (out of scope)"),
];

struct CodonWasi;

impl BackendUnderTest for CodonWasi {
    fn name(&self) -> &'static str {
        "codon"
    }

    fn backend(&self) -> &'static (dyn Backend + Sync) {
        &CodonBackend
    }

    /// Build `source` to the crate's shared cache binary and return the run recipe.
    /// A build failure panics (generated code that does not compile is a bug, not a WASI gap).
    fn pty_command(&self, source: &str, args: &[&str]) -> dewasm_test_helper::PtyCommand {
        let bin = common::build_codon(source).unwrap_or_else(|build| {
            panic!(
                "codon build failed:\n{}",
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

impl dewasm_test_helper::WasiTestsuiteBackend for CodonWasi {
    fn expected_failures(&self) -> &'static [(&'static str, &'static str)] {
        WASI_TESTSUITE_EXPECTED_FAILURES
    }

    /// macOS CoreFoundation injects `__CF_USER_TEXT_ENCODING` into every process environment, so count-exact environ assertions cannot hold there; a Codon binary on Linux inherits exactly the manifest environment, so these pass and must not be listed.
    fn expected_failures_macos(&self) -> &'static [(&'static str, &'static str)] {
        &[
            (
                "assemblyscript/environ_get-multiple-variables",
                "environ: macOS CoreFoundation env injection",
            ),
            (
                "assemblyscript/environ_sizes_get-multiple-variables",
                "environ: macOS CoreFoundation env injection",
            ),
            (
                "assemblyscript/environ_sizes_get-no-variables",
                "environ: macOS CoreFoundation env injection",
            ),
        ]
    }
}

/// The suite is 72 `-release` codon builds (about six minutes), which a pull-request CI job cannot afford; like the full spec sweep it runs in the `slow_test` lane, and locally on demand.
fn main() {
    if !cfg!(feature = "slow_test") {
        println!("codon wasi_testsuite runs under --features slow_test: 72 -release codon builds");
        return;
    }
    dewasm_test_helper::wasi_testsuite_main(&CodonWasi);
}
