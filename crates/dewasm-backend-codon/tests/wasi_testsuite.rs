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

/// The pull-request category: no filesystem fixture, one trial per always-on interface (args, environ, stdout, exit, random, stdio round-trip).
const FAST_TRIALS: &[&str] = &[
    "assemblyscript/args_get-multiple-arguments",
    "assemblyscript/environ_get-multiple-variables",
    "assemblyscript/fd_write-to-stdout",
    "assemblyscript/proc_exit-failure",
    "assemblyscript/random_get-non-zero-length",
    "rust/stdio",
];

/// What `slow_test` adds on top of [`FAST_TRIALS`] (the union is built in `curated_trials`, so the slow category is a superset by construction): the trials pinning the layout-decode paths (stat and dirent, this backend's platform-conditional risk area), the open/read/write core, and the `sock_shutdown` rows so the failure ledger stays exercised.
/// Sized against the slow category and its ~5-minute target: each trial is a codon build (measured ~7 seconds each there), so breadth beyond that belongs to the ultra sweep.
const SLOW_EXTRA_TRIALS: &[&str] = &[
    "c/sock_shutdown-invalid_fd",
    "c/sock_shutdown-not_sock",
    "c/stat-dev-ino",
    "rust/fd_readdir",
    "rust/path_open_read_write",
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

    /// Every trial is a codon build, so the suite is tiered like the spec harness: a handful of no-fixture trials on a pull request, one representative per interface area under `slow_test`, and the full sweep only under `ultra_slow_test`.
    fn curated_trials(&self) -> Option<&'static [&'static str]> {
        if cfg!(feature = "ultra_slow_test") {
            None
        } else if cfg!(feature = "slow_test") {
            static SLOW: std::sync::OnceLock<Vec<&'static str>> = std::sync::OnceLock::new();
            Some(SLOW.get_or_init(|| {
                FAST_TRIALS
                    .iter()
                    .chain(SLOW_EXTRA_TRIALS)
                    .copied()
                    .collect()
            }))
        } else {
            Some(FAST_TRIALS)
        }
    }
}

fn main() {
    dewasm_test_helper::wasi_testsuite_main(&CodonWasi);
}
