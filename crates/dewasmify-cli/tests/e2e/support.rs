//! Shared plumbing for the e2e test modules: fixture paths, conversion
//! (the one `GenOptions` policy the whole suite uses), and running
//! generated output under a real interpreter.

use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

use dewasmify_backend::{achieved_tier, Backend, GenOptions, Mode, RuntimeLinkage, Tier};
use dewasmify_backend_bash::{find_bash5, BashBackend};
use dewasmify_backend_ruby::{find_ruby, RubyBackend};

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

/// A target language wired into the shared e2e case tables
/// (`StandaloneCase`/`LibraryCase`/`AppCase`) so cases only need to be
/// written once and gated per language by tier, not hand-duplicated.
/// Onboarding a new backend is: implement this trait for a unit struct,
/// give each applicable `LibraryCase` a `glues` entry, and write the
/// language's own `#[test]` functions (a few lines each, see
/// `standalone.rs`/`library.rs`) — the tier field on each case then
/// decides which cases the new backend is expected to pass (ADR-23).
pub trait E2eLang: Sync {
    fn name(&self) -> &'static str;
    fn backend(&self) -> &'static dyn Backend;
    /// Per ADR-15, a missing interpreter fails the suite rather than
    /// skipping it.
    fn interpreter(&self) -> PathBuf;
}

pub struct RubyLang;

impl E2eLang for RubyLang {
    fn name(&self) -> &'static str {
        "ruby"
    }
    fn backend(&self) -> &'static dyn Backend {
        &RubyBackend
    }
    fn interpreter(&self) -> PathBuf {
        find_ruby().expect("ruby not found on PATH — see docs/testing.md")
    }
}

pub struct BashLang;

impl E2eLang for BashLang {
    fn name(&self) -> &'static str {
        "bash"
    }
    fn backend(&self) -> &'static dyn Backend {
        &BashBackend
    }
    fn interpreter(&self) -> PathBuf {
        find_bash5().expect("bash >= 5 not found — see docs/testing.md")
    }
}

/// Run `script` under `lang`'s interpreter, using its backend's file
/// extension and forwarding `args`/`stdin`.
pub fn run_lang(lang: &dyn E2eLang, script: &str, args: &[&str], stdin: &str) -> Output {
    run_script(
        &lang.interpreter(),
        script,
        lang.backend().file_extension(),
        args,
        stdin,
    )
}

/// Whether `lang`'s backend has reached the tier a shared case requires
/// (ADR-23; tiers are ordered best-first, so meeting a requirement means
/// being at or above it).
pub fn tier_ok(lang: &dyn E2eLang, required: Tier) -> bool {
    achieved_tier(lang.backend()) <= required
}

/// Standard "this case doesn't apply to this language yet" skip line —
/// a declared-tier gap, not a missing-tool failure, so ADR-15 doesn't
/// apply (see `tier_ok`).
pub fn print_tier_skip(case_name: &str, lang: &dyn E2eLang, required: Tier) {
    println!(
        "{case_name}: skipped for {} (requires {required}, {} is at {})",
        lang.name(),
        lang.name(),
        achieved_tier(lang.backend())
    );
}
