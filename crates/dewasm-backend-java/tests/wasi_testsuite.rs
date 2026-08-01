//! Java side of the official WASI p1 conformance harness (ADR-36): drives the prebuilt `WebAssembly/wasi-testsuite` modules through the Java backend's standalone interface. Java is compiled, so it overrides `pty_command` to `javac` the generated `Main.java` to a content-addressed class-dir cache — the launch recipe the shared `run_standalone_wasi` runs with the manifest's env/args/dirs applied. The generic harness lives in `dewasm-test-helper`.

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::path::PathBuf;
use std::process::Output;

use dewasm_backend::Backend;
use dewasm_backend_java::{find_java, javac_command, JavaBackend};
use dewasm_test_helper::{
    wasi_testsuite_suite, BackendUnderTest, PtyCommand, WasiTestsuiteBackend,
};

/// Known trial failures with their attribution (ADR-8, policy in ADR-36): `(trial, tag)` — declared ENOSYS/out-of-scope syscalls, semantics-precision gaps on supported syscalls (tracked bugs in the shared WASI runtime), and environ entries the JVM host injects itself, which count-exact `environ_*` assertions cannot absorb (ADR-40).
const WASI_TESTSUITE_EXPECTED_FAILURES: &[(&str, &str)] = &[
    // Declared ENOSYS / out-of-scope syscalls (docs/support.md).
    ("c/sock_shutdown-invalid_fd", "sock_shutdown (out of scope)"),
    ("c/sock_shutdown-not_sock", "sock_shutdown (out of scope)"),
];

/// Host-scoped failures on a macOS host: the JVM host injects environ entries of its own (macOS CoreFoundation's `__CF_USER_TEXT_ENCODING`), so count-exact environ assertions cannot hold even under the harness's cleared environment. A plain Linux JVM injects nothing, so these pass there (ADR-40).
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

/// Host-scoped failures on a Linux host: the unit passes ns-precision FileTime to `BasicFileAttributeView.setTimes` with `NOFOLLOW_LINKS`, but the Linux JDK routes the NOFOLLOW case through µs-precision `lutimes`, so the suite's ns `mtim` round-trip is truncated and fails; macOS preserves ns. Symmetric to the Go backend's ledgered lutimes gap (ADR-40).
const WASI_TESTSUITE_EXPECTED_FAILURES_LINUX: &[(&str, &str)] = &[(
    "rust/symlink_filestat",
    "path_filestat_set_times: Linux JDK sets NOFOLLOW symlink times via microsecond lutimes, truncating ns",
)];

static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

struct JavaWasi;

impl BackendUnderTest for JavaWasi {
    fn name(&self) -> &'static str {
        "java"
    }

    fn backend(&self) -> &'static (dyn Backend + Sync) {
        &JavaBackend
    }

    /// Compile `source` (one `Main.java`) to the content-addressed class-dir cache and return the run recipe. A missing `javac`/`java` fails loud (ADR-15); a compile failure panics (generated code that does not compile is a bug, not a WASI gap).
    fn pty_command(&self, source: &str, args: &[&str]) -> PtyCommand {
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
        PtyCommand {
            program: java,
            args: argv,
            cwd: None,
        }
    }
}

/// Compile `source` to a content-addressed class-dir cache (identical programs compile once).
fn build_java(source: &str) -> Result<PathBuf, Output> {
    let mut hasher = DefaultHasher::new();
    source.hash(&mut hasher);
    let hash = hasher.finish();

    let cache = std::env::temp_dir().join("dewasm-java-cache");
    std::fs::create_dir_all(&cache).unwrap();
    let classdir = cache.join(format!("wasitest-{hash:016x}"));

    if !classdir.join("Main.class").exists() {
        let tmp = cache.join(format!(
            "wasitest-{hash:016x}.{}.{}",
            std::process::id(),
            COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&tmp).unwrap();
        let src = tmp.join("Main.java");
        std::fs::write(&src, source).unwrap();
        let build = javac_command()
            .arg("-d")
            .arg(&tmp)
            .arg(&src)
            .output()
            .expect("spawn javac");
        if !build.status.success() {
            return Err(build);
        }
        let _ = std::fs::rename(&tmp, &classdir);
    }

    Ok(classdir)
}

impl WasiTestsuiteBackend for JavaWasi {
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

wasi_testsuite_suite!(JavaWasi);
