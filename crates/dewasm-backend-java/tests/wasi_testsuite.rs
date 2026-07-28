//! Java side of the official WASI p1 conformance harness (ADR-36): drives the
//! prebuilt `WebAssembly/wasi-testsuite` modules through the Java backend's
//! standalone interface. Java is compiled, so it overrides `pty_command` to
//! `javac` the generated `Main.java` to a content-addressed class-dir cache —
//! the launch recipe the shared `run_standalone_wasi` runs with the manifest's
//! env/args/dirs applied. The generic harness lives in `dewasm-test-helper`.

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::path::PathBuf;
use std::process::{Command, Output};

use dewasm_backend::Backend;
use dewasm_backend_java::{find_java, find_javac, JavaBackend};
use dewasm_test_helper::{
    wasi_testsuite_suite, BackendUnderTest, PtyCommand, WasiTestsuiteBackend,
};

/// Known trial failures with their attribution (ADR-8, policy in ADR-36):
/// `(trial, tag)` — declared ENOSYS/out-of-scope syscalls, semantics-precision
/// gaps on supported syscalls (tracked bugs in the shared WASI runtime), and
/// the ADR-31 whole-environment passthrough the `environ_*` count assertions
/// cannot satisfy. Java additionally has a cluster where `path_open` with
/// `O_CREAT` fails to create a file (NOENT) in these fixtures — a Java-runtime
/// bug tracked here (its e2e filesystem create path still works).
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
    (
        "rust/unlink_file_trailing_slashes",
        "path_unlink_file: trailing slash not rejected",
    ),
    // Java-specific: path_open O_CREAT create fails NOENT in these fixtures.
    (
        "rust/path_rename",
        "path_open: O_CREAT create returns NOENT (java)",
    ),
    (
        "rust/file_unbuffered_write",
        "path_open: O_CREAT create returns NOENT (java)",
    ),
    (
        "rust/dangling_fd",
        "path_open: O_CREAT create returns NOENT (java)",
    ),
    (
        "rust/isatty",
        "path_open: O_CREAT create returns NOENT (java)",
    ),
    (
        "rust/path_open_create_existing",
        "path_open: O_CREAT create returns NOENT (java)",
    ),
    (
        "rust/remove_directory_trailing_slashes",
        "path_open: O_CREAT create returns NOENT (java)",
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

static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

struct JavaWasi;

impl BackendUnderTest for JavaWasi {
    fn name(&self) -> &'static str {
        "java"
    }

    fn backend(&self) -> &'static (dyn Backend + Sync) {
        &JavaBackend
    }

    /// Compile `source` (one `Main.java`) to the content-addressed class-dir
    /// cache and return the run recipe. A missing `javac`/`java` fails loud
    /// (ADR-15); a compile failure panics (generated code that does not compile
    /// is a bug, not a WASI gap).
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

/// Compile `source` to a content-addressed class-dir cache (identical programs
/// compile once).
fn build_java(source: &str) -> Result<PathBuf, Output> {
    let javac =
        find_javac().expect("javac not found on PATH (or $DEWASM_JAVAC) — see docs/testing.md");

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
        let build = Command::new(&javac)
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
}

wasi_testsuite_suite!(JavaWasi);
