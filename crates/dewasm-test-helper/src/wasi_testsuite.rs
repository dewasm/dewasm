//! Official WASI preview-1 conformance harness (ADR-36): drives the prebuilt
//! `WebAssembly/wasi-testsuite` modules (submodule `tests/wasi-testsuite`,
//! branch `prod/testsuite-base`) through the ADR-31 standalone interface. This
//! mirrors `spec.rs`'s structure — one libtest-mimic [`Trial`] per `.wasm`, a
//! fail-loud assert when the submodule is missing, and an
//! `EXPECTED_FAILURES` ledger checked *both ways* (an unexpectedly-passing
//! ledgered test is a hard failure, exactly like the spec harness).
//!
//! Each `<name>.wasm` may carry a co-located `<name>.json` manifest (the
//! upstream v0 schema: `args`, `env`, `root`, `exit_code`, `stdout`,
//! `stderr`). The runner converts the module in [`Mode::Standalone`], mounts
//! the manifest's `root` fixture at guest `/` (via a fresh temp copy so trials
//! stay hermetic), sets `env`/`args`, then asserts the process exit code and —
//! when the manifest pins it — stdout/stderr. The `root`→`/` mapping mirrors
//! upstream's own wasmtime adapter (`--dir {root}::/`).
//!
//! A conversion refusal attributed to a declared-unsupported feature, a wrong
//! exit code, or a stdout mismatch is a *failure*; the ledger (ADR-8) then
//! decides whether it is expected. Every ledger entry names the WASI function
//! or interface behaviour responsible.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use dewasm_backend::{Backend, GenOptions, Mode, RuntimeLinkage};
use dewasm_core::feature::UnsupportedError;
use libtest_mimic::{Failed, Trial};
use serde::Deserialize;

use crate::backend::BackendUnderTest;

/// A backend wired into the WASI-testsuite harness: the base
/// [`BackendUnderTest`] plus its known-failure ledger.
pub trait WasiTestsuiteBackend: BackendUnderTest {
    /// Known trial failures: `(trial name, attribution tag)`. The trial name
    /// is `"<suite>/<stem>"` (e.g. `"rust/path_link"`); the tag names the WASI
    /// function or interface behaviour that causes the failure (a declared
    /// ENOSYS gap, or an ADR-31 interface choice). A ledgered trial that
    /// *passes* is a hard failure — remove it (same both-ways discipline as
    /// `spec.rs`).
    fn expected_failures(&self) -> &'static [(&'static str, &'static str)];
}

/// The three prebuilt suites we drive, all `wasm32-wasip1` (the standard goal
/// for a dewasm backend: wasm 1.0 + full WASI p1). The Rust suite's
/// `wasm32-wasip3` tree is deliberately excluded — preview 3 is component-model
/// territory, rejected outright (ADR-24).
const SUITES: &[&str] = &["c", "rust", "assemblyscript"];

/// The `tests/wasi-testsuite` submodule directory.
fn testsuite_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/wasi-testsuite")
}

/// A parsed test manifest (the upstream v0 legacy schema). Every field is
/// optional (`#[serde(default)]`); an absent `.json` means "all defaults" (a
/// still-valid test). Unknown keys (the v1 `operations`/`proposals` schema, the
/// suite-level `manifest.json`) are ignored rather than rejected — serde's
/// default without `deny_unknown_fields`.
#[derive(Default, Deserialize)]
#[serde(default)]
struct Manifest {
    args: Vec<String>,
    env: BTreeMap<String, String>,
    /// The preopen directory fixture (relative to the manifest), mounted at
    /// guest `/`.
    root: Option<String>,
    exit_code: i32,
    stdout: Option<String>,
    stderr: Option<String>,
}

impl Manifest {
    fn parse(text: &str) -> anyhow::Result<Manifest> {
        Ok(serde_json::from_str(text)?)
    }
}

/// A single enumerated test: the module path, its manifest, and the trial name.
struct Case {
    trial_name: String,
    wasm: PathBuf,
    manifest: Manifest,
}

/// Enumerate every `.wasm` under each suite's `wasm32-wasip1` directory (the
/// suite-level `manifest.json` has no `.wasm`, so it is never a case), pairing
/// it with its optional `<stem>.json`.
fn enumerate() -> anyhow::Result<Vec<Case>> {
    let root = testsuite_dir();
    assert!(
        root.exists(),
        "tests/wasi-testsuite not found — run \
         `git submodule update --init tests/wasi-testsuite` (see docs/testing.md)"
    );
    let mut cases = Vec::new();
    for suite in SUITES {
        let dir = root
            .join("tests")
            .join(suite)
            .join("testsuite")
            .join("wasm32-wasip1");
        assert!(
            dir.exists(),
            "{} missing — the wasi-testsuite submodule must be on branch \
             prod/testsuite-base (see docs/testing.md)",
            dir.display()
        );
        let mut entries: Vec<PathBuf> = std::fs::read_dir(&dir)?
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|p| p.extension().and_then(|x| x.to_str()) == Some("wasm"))
            .collect();
        entries.sort();
        for wasm in entries {
            let stem = wasm.file_stem().unwrap().to_string_lossy().to_string();
            let manifest_path = wasm.with_extension("json");
            let manifest = if manifest_path.exists() {
                let text = std::fs::read_to_string(&manifest_path)?;
                Manifest::parse(&text)
                    .map_err(|e| anyhow::anyhow!("{}: {e:#}", manifest_path.display()))?
            } else {
                Manifest::default()
            };
            cases.push(Case {
                trial_name: format!("{suite}/{stem}"),
                wasm,
                manifest,
            });
        }
    }
    Ok(cases)
}

/// Build one libtest-mimic [`Trial`] per prebuilt module (the
/// `wasi_testsuite_suite!` macro's entry point). The trial name is
/// `"<suite>/<stem>"`, so `cargo test --test wasi_testsuite rust` filters by
/// suite and `-- --exact rust/path_link` selects one module.
pub fn wasi_testsuite_trials(lang: &'static dyn WasiTestsuiteBackend) -> Vec<Trial> {
    let cases = enumerate().expect("enumerate wasi-testsuite");
    let ledger = lang.expected_failures();
    cases
        .into_iter()
        .map(|case| {
            let name = case.trial_name.clone();
            Trial::test(name.clone(), move || run_trial(lang, ledger, &name, &case))
        })
        .collect()
}

/// `harness = false` entry point: parse cargo's test arguments and run every
/// trial.
pub fn wasi_testsuite_main(lang: &'static dyn WasiTestsuiteBackend) {
    let args = libtest_mimic::Arguments::from_args();
    libtest_mimic::run(&args, wasi_testsuite_trials(lang)).exit();
}

/// Outcome of actually running a case, before the ledger is consulted.
enum Outcome {
    Pass,
    Fail(String),
}

fn run_trial(
    lang: &dyn WasiTestsuiteBackend,
    ledger: &[(&str, &str)],
    name: &str,
    case: &Case,
) -> Result<(), Failed> {
    let outcome = evaluate(lang, case);
    let ledgered = ledger.iter().find(|(n, _)| *n == name).map(|(_, tag)| *tag);
    match (outcome, ledgered) {
        (Outcome::Pass, None) => Ok(()),
        (Outcome::Pass, Some(tag)) => Err(Failed::from(format!(
            "{name}: ledgered as an expected failure ({tag}) but PASSED — \
             remove it from WASI_TESTSUITE_EXPECTED_FAILURES"
        ))),
        (Outcome::Fail(detail), None) => Err(Failed::from(format!(
            "{name}: unexpected failure: {detail}\n(if this is a declared WASI \
             gap, add it to WASI_TESTSUITE_EXPECTED_FAILURES with an attribution tag)"
        ))),
        (Outcome::Fail(_), Some(_)) => Ok(()),
    }
}

/// Convert, run, and check one case against its manifest. Any deviation
/// (conversion refusal, wrong exit code, wrong stdout/stderr) is a
/// [`Outcome::Fail`]; the ledger in [`run_trial`] decides if it is expected.
fn evaluate(lang: &dyn WasiTestsuiteBackend, case: &Case) -> Outcome {
    let bytes = match std::fs::read(&case.wasm) {
        Ok(b) => b,
        Err(e) => return Outcome::Fail(format!("reading {}: {e}", case.wasm.display())),
    };

    let program = match convert_standalone(lang.backend(), &bytes) {
        Ok(p) => p,
        Err(Attribution::Tag(tag)) => {
            return Outcome::Fail(format!("conversion rejected (declared unsupported: {tag})"))
        }
        Err(Attribution::Bug(detail)) => {
            return Outcome::Fail(format!("conversion bug (unattributed): {detail}"))
        }
    };

    // Mount the `root` fixture at guest `/` from a fresh temp copy, so a test
    // that creates/removes files never mutates the committed submodule and
    // re-runs cleanly.
    let m = &case.manifest;
    let _scratch = match &m.root {
        Some(root) => match stage_root(&case.wasm, root) {
            Ok(dir) => Some(dir),
            Err(e) => return Outcome::Fail(format!("staging root {root:?}: {e}")),
        },
        None => None,
    };
    let dirs: Vec<(&str, &Path)> = match &_scratch {
        Some(dir) => vec![("/", dir.path())],
        None => vec![],
    };

    let args: Vec<&str> = m.args.iter().map(String::as_str).collect();
    let env: Vec<(&str, &str)> = m
        .env
        .iter()
        .map(|(k, v)| (k.as_str(), v.as_str()))
        .collect();

    let output = lang.run_standalone_wasi(&program, &dirs, &args, &env, b"");

    let code = output.status.code();
    if code != Some(m.exit_code) {
        return Outcome::Fail(format!(
            "exit code {code:?}, expected {}\nstderr: {}",
            m.exit_code,
            truncate(&String::from_utf8_lossy(&output.stderr))
        ));
    }
    if let Some(expected) = &m.stdout {
        let actual = String::from_utf8_lossy(&output.stdout);
        if actual != *expected {
            return Outcome::Fail(format!(
                "stdout {:?}, expected {expected:?}",
                truncate(&actual)
            ));
        }
    }
    if let Some(expected) = &m.stderr {
        let actual = String::from_utf8_lossy(&output.stderr);
        if actual != *expected {
            return Outcome::Fail(format!(
                "stderr {:?}, expected {expected:?}",
                truncate(&actual)
            ));
        }
    }
    Outcome::Pass
}

fn truncate(s: &str) -> String {
    if s.len() > 400 {
        format!("{}…", &s[..400])
    } else {
        s.to_string()
    }
}

/// A conversion refusal, split like `spec.rs`: attributable (a declared gap) or
/// an unattributed dewasm bug.
enum Attribution {
    Tag(String),
    Bug(String),
}

/// Map a conversion error to its attribution: an [`UnsupportedError`] anywhere
/// in the chain names the responsible feature(s); anything else is a bug.
fn attribute(err: &anyhow::Error) -> Attribution {
    match err
        .chain()
        .find_map(|e| e.downcast_ref::<UnsupportedError>())
    {
        Some(unsupported) if unsupported.features.is_empty() => {
            Attribution::Tag(format!("unknown-proposal: {}", unsupported.detail))
        }
        Some(unsupported) => {
            let ids: Vec<&str> = unsupported.features.iter().map(|f| f.id()).collect();
            Attribution::Tag(ids.join("+"))
        }
        None => Attribution::Bug(format!("{err:#}")),
    }
}

/// Convert `bytes` to a standalone program with `backend`, returning the source
/// text or an attributed refusal.
fn convert_standalone(backend: &(dyn Backend + Sync), bytes: &[u8]) -> Result<String, Attribution> {
    let module = dewasm_core::build_module(bytes).map_err(|e| attribute(&e))?;
    let mut units = backend
        .generate(
            &module,
            &GenOptions {
                mode: Mode::Standalone,
                module_name: "prog".to_string(),
                runtime: RuntimeLinkage::Embedded,
                default_wasi: true,
                data_file: None,
            },
        )
        .map_err(|e| attribute(&e))?;
    // The primary source is always UTF-8 (generated code); only the optional
    // data sidecar is raw bytes, and this runner never requests one.
    Ok(String::from_utf8(units.remove(0).contents).expect("generated source is valid UTF-8"))
}

/// A staged copy of a `root` fixture, removed on drop.
struct Scratch {
    dir: PathBuf,
}

impl Scratch {
    fn path(&self) -> &Path {
        &self.dir
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

/// Copy the `root` fixture (relative to the module) into a fresh, uniquely
/// named temp directory so parallel trials never share host state.
fn stage_root(wasm: &Path, root: &str) -> std::io::Result<Scratch> {
    static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let src = wasm.parent().unwrap().join(root);
    let dst = std::env::temp_dir().join(format!(
        "dewasm-wasi-testsuite-{}-{}",
        std::process::id(),
        COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    ));
    copy_dir_all(&src, &dst)?;
    Ok(Scratch { dir: dst })
}

fn copy_dir_all(src: &Path, dst: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dst)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let ty = entry.file_type()?;
        let to = dst.join(entry.file_name());
        if ty.is_dir() {
            copy_dir_all(&entry.path(), &to)?;
        } else if ty.is_symlink() {
            #[cfg(unix)]
            {
                let target = std::fs::read_link(entry.path())?;
                std::os::unix::fs::symlink(target, &to)?;
            }
            #[cfg(not(unix))]
            {
                std::fs::copy(entry.path(), &to)?;
            }
        } else {
            std::fs::copy(entry.path(), &to)?;
        }
    }
    Ok(())
}
