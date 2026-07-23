//! Shared plumbing for the e2e test modules: fixture paths, conversion
//! (the one `GenOptions` policy the whole suite uses), and running
//! generated output under a real interpreter.

use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

use dewasmify_backend::{Backend, GenOptions, Mode, RuntimeLinkage};

/// `examples/wat/`, home of the hand-written `.wat` fixtures.
pub fn examples_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/wat")
}

/// `examples/apps/cache/`, populated by `examples/apps/fetch.sh` (ADR-9).
pub fn apps_cache_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/apps/cache")
}

/// Convert raw wasm bytes with `backend`.
pub fn convert_bytes(backend: &dyn Backend, bytes: &[u8], mode: Mode, name: &str) -> String {
    let module = dewasmify_core::build_module(bytes).expect("build IR");
    backend
        .generate(
            &module,
            &GenOptions {
                mode,
                module_name: name.to_string(),
                runtime: RuntimeLinkage::Embedded,
                default_wasi: true,
            },
        )
        .expect("generate")
        .remove(0)
        .contents
}

/// Convert a `.wat` fixture with `backend`.
pub fn convert(backend: &dyn Backend, wat_path: &Path, mode: Mode, name: &str) -> String {
    let bytes = wat::parse_file(wat_path).expect("parse wat");
    convert_bytes(backend, &bytes, mode, name)
}

/// Spawn `cmd` with `stdin` piped in and both output streams captured,
/// returning the raw `Output`.
pub fn run_command(cmd: &mut Command, stdin: &str) -> Output {
    let mut child = cmd
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn");
    use std::io::Write as _;
    child
        .stdin
        .take()
        .unwrap()
        .write_all(stdin.as_bytes())
        .unwrap();
    child.wait_with_output().expect("wait")
}

/// Write `script` to a temp file (extension `ext`) and run it under
/// `interpreter`, returning the raw `Output`. The pid + counter pair
/// keeps paths unique across both parallel test threads and concurrent
/// `cargo test` processes.
pub fn run_script(
    interpreter: &Path,
    script: &str,
    ext: &str,
    args: &[&str],
    stdin: &str,
) -> Output {
    static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let path = std::env::temp_dir().join(format!(
        "dewasmify-e2e-{}-{}.{ext}",
        std::process::id(),
        COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    ));
    std::fs::write(&path, script).unwrap();
    run_command(Command::new(interpreter).arg(&path).args(args), stdin)
}

/// Run `script` under `ruby`, returning the raw `Output`. Callers that
/// expect a specific (possibly non-zero) exit code want this; callers
/// that just want stdout on a clean run want `run_ruby` below.
/// Interpreter discovery lives here (not per test) so the interpreter
/// that is validated is always the one that runs.
pub fn run_ruby_capture(script: &str, args: &[&str]) -> Output {
    let ruby =
        dewasmify_backend_ruby::find_ruby().expect("ruby not found on PATH — see docs/testing.md");
    run_script(&ruby, script, "rb", args, "")
}

/// Run `script` under `ruby`, asserting success, and return stdout.
pub fn run_ruby(script: &str, args: &[&str]) -> String {
    let output = run_ruby_capture(script, args);
    assert!(
        output.status.success(),
        "ruby failed: {}\n{}",
        output.status,
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).into_owned()
}

/// Run `script` under `bash`, returning the raw `Output` (unlike
/// `run_ruby`, callers here care about specific non-zero exit codes too,
/// so this doesn't assert success itself).
pub fn run_bash(bash: &Path, script: &str, args: &[&str]) -> Output {
    run_script(bash, script, "sh", args, "")
}
