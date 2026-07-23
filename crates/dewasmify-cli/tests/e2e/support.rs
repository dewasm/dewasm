//! Shared plumbing for `scripted` and `apps`: fixture paths, `.wat`
//! conversion, and running generated output under a real interpreter.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use dewasmify_backend::{Backend, GenOptions, Mode, RuntimeLinkage};

/// `examples/wat/`, home of the hand-written `.wat` fixtures.
pub fn examples_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/wat")
}

/// `examples/apps/cache/`, populated by `examples/apps/fetch.sh` (ADR-9).
pub fn apps_cache_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/apps/cache")
}

/// Convert a `.wat` fixture with `backend`.
pub fn convert(backend: &dyn Backend, wat_path: &Path, mode: Mode, name: &str) -> String {
    let bytes = wat::parse_file(wat_path).expect("parse wat");
    let module = dewasmify_core::build_module(&bytes).expect("build IR");
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

/// Write `script` to a temp file (extension `ext`) and run it under
/// `interpreter`, returning the raw `Output`.
fn run_script(interpreter: &Path, script: &str, ext: &str, args: &[&str]) -> Output {
    let path = std::env::temp_dir().join(format!(
        "dewasmify-e2e-{}.{ext}",
        std::process::id() as u64 + script.len() as u64
    ));
    std::fs::write(&path, script).unwrap();
    Command::new(interpreter)
        .arg(&path)
        .args(args)
        .output()
        .unwrap_or_else(|e| panic!("run {}: {e}", interpreter.display()))
}

/// Run `script` under `ruby`, returning the raw `Output`. Callers that
/// expect a specific (possibly non-zero) exit code want this; callers
/// that just want stdout on a clean run want `run_ruby` below.
pub fn run_ruby_capture(script: &str, args: &[&str]) -> Output {
    run_script(Path::new("ruby"), script, "rb", args)
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
    run_script(bash, script, "sh", args)
}
