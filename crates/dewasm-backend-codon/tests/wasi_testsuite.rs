//! Codon side of the official WASI p1 conformance harness.
//! It drives the prebuilt `WebAssembly/wasi-testsuite` modules.
//! They run through the Codon backend's standalone interface.
//! Codon is compiled, so it overrides `pty_command` to `codon build` the generated program.
//! The build goes to a content-addressed cache binary, with the runtime dylibs copied beside it.
//! So the manifest-only child environment needs no loader-path variable.
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

/// The pull-request category: no filesystem fixture, one trial per always-on interface.
/// Those interfaces are args, environ, stdout, exit, random, and stdio round-trip.
const FAST_TRIALS: &[&str] = &[
    "assemblyscript/args_get-multiple-arguments",
    "assemblyscript/environ_get-multiple-variables",
    "assemblyscript/fd_write-to-stdout",
    "assemblyscript/proc_exit-failure",
    "assemblyscript/random_get-non-zero-length",
    "rust/stdio",
];

/// What `slow_test` adds on top of [`FAST_TRIALS`].
/// The union is built in `curated_trials`, so the slow category is a superset by construction.
/// The added trials are:
/// - the trials pinning the layout-decode paths (stat and dirent);
///   those are this backend's platform-conditional risk area;
/// - the open/read/write core;
/// - the `sock_shutdown` rows, so the failure ledger stays exercised.
///
/// It is sized against the slow category's budget.
/// Each trial pays a codon build, so breadth beyond this list belongs to the ultra sweep.
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

    /// macOS CoreFoundation injects `__CF_USER_TEXT_ENCODING` into every process environment.
    /// So count-exact environ assertions cannot hold there.
    /// A Codon binary on Linux inherits exactly the manifest environment.
    /// So these pass there and must not be listed.
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

    /// Every trial is a codon build, so the suite is split into categories like the spec harness.
    /// A pull request runs a handful of no-fixture trials.
    /// Under `slow_test` it runs one representative per interface area.
    /// The full sweep runs only under `ultra_slow_test`.
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
