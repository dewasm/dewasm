//! Go side of the official WASI p1 conformance harness (ADR-36): drives the prebuilt `WebAssembly/wasi-testsuite` modules through the Go backend's standalone interface. Go is compiled, so it overrides `pty_command` to `go build` the generated program to a content-addressed cache binary — the launch recipe the shared `run_standalone_wasi` runs with the manifest's env/args/dirs applied. The generic harness lives in `dewasm-test-helper`.

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::path::PathBuf;
use std::process::{Command, Output};

use dewasm_backend::Backend;
use dewasm_backend_go::{find_go, GoBackend};
use dewasm_test_helper::BackendUnderTest;

/// Known trial failures with their attribution (ADR-8, policy in ADR-40): `(trial, tag)` — out-of-scope syscalls and the one std-portability gap the full WASI p1 filesystem support (ADR-40) cannot close on Go.
const WASI_TESTSUITE_EXPECTED_FAILURES: &[(&str, &str)] = &[
    // No socket layer in a demo runtime (out of scope, docs/support.md).
    ("c/sock_shutdown-invalid_fd", "sock_shutdown (out of scope)"),
    ("c/sock_shutdown-not_sock", "sock_shutdown (out of scope)"),
    // Setting a symlink's own times (NOFOLLOW) needs lutimes, which Go's std exposes no portable (darwin+linux, build-tag-free) way to call — os.Chtimes follows the link. Every regular-file times path is supported; only the symlink-target case in this one trial is out of reach (ADR-40).
    (
        "rust/symlink_filestat",
        "path_filestat_set_times: no portable std lutimes for a NOFOLLOW symlink",
    ),
];

static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

struct GoWasi;

impl BackendUnderTest for GoWasi {
    fn name(&self) -> &'static str {
        "go"
    }

    fn backend(&self) -> &'static (dyn Backend + Sync) {
        &GoBackend
    }

    /// Build `source` to the content-addressed cache binary and return the run recipe. A missing `go` toolchain fails loud (ADR-15); a build failure panics (generated code that does not compile is a bug, not a WASI gap).
    fn pty_command(&self, source: &str, args: &[&str]) -> dewasm_test_helper::PtyCommand {
        let bin = build_go(source).unwrap_or_else(|build| {
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

/// Compile `source` to a content-addressed cache binary (identical programs build once).
fn build_go(source: &str) -> Result<PathBuf, Output> {
    let go =
        find_go().expect("go toolchain not found on PATH (or $DEWASM_GO) — see docs/testing.md");

    let mut hasher = DefaultHasher::new();
    source.hash(&mut hasher);
    let hash = hasher.finish();

    let cache = std::env::temp_dir().join("dewasm-go-cache");
    std::fs::create_dir_all(&cache).unwrap();
    let bin = cache.join(format!("wasitest-{hash:016x}"));

    if !bin.exists() {
        let src = cache.join(format!("wasitest-{hash:016x}.go"));
        std::fs::write(&src, source).unwrap();
        let tmp_bin = cache.join(format!(
            "wasitest-{hash:016x}.{}.{}",
            std::process::id(),
            COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        let build = Command::new(&go)
            .arg("build")
            .arg("-o")
            .arg(&tmp_bin)
            .arg(&src)
            .output()
            .expect("spawn go build");
        if !build.status.success() {
            return Err(build);
        }
        let _ = std::fs::rename(&tmp_bin, &bin);
    }

    Ok(bin)
}

impl dewasm_test_helper::WasiTestsuiteBackend for GoWasi {
    fn expected_failures(&self) -> &'static [(&'static str, &'static str)] {
        WASI_TESTSUITE_EXPECTED_FAILURES
    }
}

dewasm_test_helper::wasi_testsuite_suite!(GoWasi);
