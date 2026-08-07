//! Java side of the official WASI p1 conformance harness: drives the prebuilt `WebAssembly/wasi-testsuite` modules through the Java backend's standalone interface. Java is compiled, so it overrides `pty_command` to `javac` the generated `Main.java` to a content-addressed class-dir cache — the launch recipe the shared `run_standalone_wasi` runs with the manifest's env/args/dirs applied. The generic harness lives in `dewasm-test-helper`.

use dewasm_backend::Backend;
use dewasm_backend_java::{find_java, JavaBackend};
use dewasm_test_helper::BackendUnderTest;

mod common;

use common::build_java;

/// Known trial failures with their attribution: `(trial, tag)`. Host-specific ones live in the two consts below.
const WASI_TESTSUITE_EXPECTED_FAILURES: &[(&str, &str)] = &[
    // Declared ENOSYS / out-of-scope syscalls (docs/support.md).
    ("c/sock_shutdown-invalid_fd", "sock_shutdown (out of scope)"),
    ("c/sock_shutdown-not_sock", "sock_shutdown (out of scope)"),
];

/// Host-scoped failures on a macOS host: the JVM host injects environ entries of its own (macOS CoreFoundation's `__CF_USER_TEXT_ENCODING`), so count-exact environ assertions cannot hold even under the harness's cleared environment. A plain Linux JVM injects nothing, so these pass there.
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

/// Host-scoped failures on a Linux host: the unit passes ns-precision FileTime to `BasicFileAttributeView.setTimes` with `NOFOLLOW_LINKS`, but the Linux JDK routes the NOFOLLOW case through µs-precision `lutimes`, so the suite's ns `mtim` round-trip is truncated and fails; macOS preserves ns. Symmetric to the Go backend's listed lutimes gap.
const WASI_TESTSUITE_EXPECTED_FAILURES_LINUX: &[(&str, &str)] = &[(
    "rust/symlink_filestat",
    "path_filestat_set_times: Linux JDK sets NOFOLLOW symlink times via microsecond lutimes, truncating ns",
)];

struct JavaWasi;

impl BackendUnderTest for JavaWasi {
    fn name(&self) -> &'static str {
        "java"
    }

    fn backend(&self) -> &'static (dyn Backend + Sync) {
        &JavaBackend
    }

    /// Compile `source` (one `Main.java`) to the content-addressed class-dir cache and return the run recipe. A missing `javac`/`java` fails loud; a compile failure panics (generated code that does not compile is a bug, not a WASI gap).
    fn pty_command(&self, source: &str, args: &[&str]) -> dewasm_test_helper::PtyCommand {
        let java =
            find_java().expect("java not found on PATH (or $DEWASM_JAVA) — see docs/testing.md");
        let classdir = build_java(source).unwrap_or_else(|build| {
            panic!("javac failed:\n{}", String::from_utf8_lossy(&build.stderr))
        });
        let mut argv = vec![
            "-Xss16m".to_string(),
            "-cp".to_string(),
            classdir.to_string_lossy().into_owned(),
            "Main".to_string(),
        ];
        argv.extend(args.iter().map(|a| a.to_string()));
        dewasm_test_helper::PtyCommand {
            program: java,
            args: argv,
            cwd: None,
        }
    }
}

impl dewasm_test_helper::WasiTestsuiteBackend for JavaWasi {
    fn expected_failures(&self) -> &'static [(&'static str, &'static str)] {
        WASI_TESTSUITE_EXPECTED_FAILURES
    }

    fn expected_failures_macos(&self) -> &'static [(&'static str, &'static str)] {
        WASI_TESTSUITE_EXPECTED_FAILURES_MACOS
    }

    fn expected_failures_linux(&self) -> &'static [(&'static str, &'static str)] {
        WASI_TESTSUITE_EXPECTED_FAILURES_LINUX
    }
}

dewasm_test_helper::wasi_testsuite_suite!(JavaWasi);
