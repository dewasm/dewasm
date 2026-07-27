//! wasmtime as a [`BackendUnderTest`] (ADR-27): the golden-vs-wasmtime
//! freshness check run through the *same* shared app/gzip runners every real
//! backend uses, rather than a hand-written per-case loop. wasmtime does not
//! generate source — it runs the cached `.wasm` binary directly — so its
//! `convert_app` returns the path to the exact cache binary the goldens were
//! captured from, and its `run`/`run_bytes` exec `wasmtime run <path>`. The
//! per-backend `apps`/`gzip` suites cover the other half (generated output vs.
//! golden); this suite re-validates the goldens themselves against a live
//! engine.
//!
//! Named `apps_wasmtime` for the family: a future engine-under-test (wasmer,
//! wasmedge) would join here the same way. Gated behind the `wasmtime_test`
//! feature and `#[ignore]`d otherwise, because `wasmtime` is deliberately not
//! one of the default suite's required tools (ADR-15) — this suite exists to
//! check the checker, not to run on every `cargo test`.
//!
//! Run it with:
//!
//! ```console
//! $ cargo test -p dewasm-test-helper --features wasmtime_test --test apps_wasmtime
//! ```

use std::path::Path;
use std::process::{Command, Output};

use dewasm_backend::{Backend, GenOptions, Mode, OutputFile};
use dewasm_core::ir;
use dewasm_test_helper::{
    assert_transcript_eq, capture_qjs_repl_golden, capture_qjs_repl_transcript,
    qjs_repl_golden_path, run_app_cases, run_command_bytes, run_fs_app_cases_forced,
    run_gzip_cases, BackendUnderTest, PtyCommand,
};

/// A [`Backend`] that only exists to satisfy `BackendUnderTest::backend()`.
/// wasmtime runs the cached binary directly, so codegen is never reached; if
/// anything routes into `generate()` it is a wiring bug, not a supported path.
struct NeverBackend;

impl Backend for NeverBackend {
    fn name(&self) -> &str {
        "wasmtime"
    }

    fn file_extension(&self) -> &str {
        "wasm"
    }

    fn generate(
        &self,
        _module: &ir::Module,
        _opts: &GenOptions,
    ) -> anyhow::Result<Vec<OutputFile>> {
        panic!("wasmtime runs the cache binary directly; generate() must never be called")
    }
}

/// The wasmtime engine wired into the shared app/gzip runners. `convert_app`
/// hands back the cache-binary path (no codegen); `run_bytes` execs
/// `wasmtime run <path> <args...>` on that exact binary.
struct Wasmtime;

impl BackendUnderTest for Wasmtime {
    fn name(&self) -> &'static str {
        "wasmtime"
    }

    fn backend(&self) -> &'static (dyn Backend + Sync) {
        &NeverBackend
    }

    /// Skip codegen entirely: write the exact bytes the shared runner read from
    /// the cache to a temp `.wasm` (keyed by content hash so identical apps
    /// share one file) and return that path, which the runner then feeds to
    /// `run`/`run_app_fs`. Writing the bytes rather than rebuilding the cache
    /// path from `name` keeps this independent of the conversion module name,
    /// which no longer always equals the cache stem (e.g. CRuby: cache
    /// `ruby.wasm`, class `Cruby`).
    fn convert_app(&self, bytes: &[u8], _mode: Mode, _name: &str) -> String {
        use std::hash::{Hash, Hasher};
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        bytes.hash(&mut hasher);
        let path =
            std::env::temp_dir().join(format!("dewasm-wasmtime-{:016x}.wasm", hasher.finish()));
        if !path.exists() {
            std::fs::write(&path, bytes).expect("write temp wasm");
        }
        path.to_string_lossy().into_owned()
    }

    /// `source` is a cache-binary path (from `convert_app`), not generated
    /// code: exec `wasmtime run <path> <args...>` with `stdin` piped in. Per
    /// ADR-15 a missing `wasmtime` fails loud, never skips.
    fn run_bytes(&self, source: &str, args: &[&str], stdin: &[u8]) -> Output {
        assert!(
            Command::new("wasmtime").arg("--version").output().is_ok(),
            "wasmtime not found on PATH — required when running with --features wasmtime_test \
             (see docs/testing.md)"
        );
        let path = Path::new(source);
        assert!(
            path.exists(),
            "{} not cached — run examples/apps/fetch.sh (see docs/testing.md)",
            source
        );
        run_command_bytes(
            Command::new("wasmtime").arg("run").arg(path).args(args),
            stdin,
        )
    }

    /// Run the filesystem app directly on the cache binary: `program` is the
    /// wasm path (from `convert_app`), so ignore the glue-composition the
    /// default does and instead exec `wasmtime run --dir <host>::<guest>...
    /// --env K=V... <wasm> <args[1..]>` (wasmtime injects argv0 itself). Per
    /// ADR-15 a missing `wasmtime` fails loud, never skips.
    fn run_app_fs(
        &self,
        program: &str,
        _class: &str,
        args: &[&str],
        env: &[(&str, &str)],
        stdin: &[u8],
        preopens: &[(&str, &Path)],
    ) -> Output {
        assert!(
            Command::new("wasmtime").arg("--version").output().is_ok(),
            "wasmtime not found on PATH — required when running with --features wasmtime_test \
             (see docs/testing.md)"
        );
        let mut cmd = Command::new("wasmtime");
        cmd.arg("run");
        for (guest, host) in preopens {
            cmd.arg("--dir")
                .arg(format!("{}::{}", host.display(), guest));
        }
        for (k, v) in env {
            cmd.arg("--env").arg(format!("{k}={v}"));
        }
        cmd.arg(program).args(&args[1..]);
        run_command_bytes(&mut cmd, stdin)
    }

    /// Drive the cached wasm binary directly under a pty: `source` is the wasm
    /// path (from `convert_app`), so the command is `wasmtime run <path>
    /// <args...>` — the ground-truth interactive session the backends must
    /// match. wasmtime injects argv0, so a bare no-args call is the interactive
    /// REPL invocation.
    fn pty_command(&self, source: &str, args: &[&str]) -> PtyCommand {
        assert!(
            Command::new("wasmtime").arg("--version").output().is_ok(),
            "wasmtime not found on PATH — required when running with --features wasmtime_test \
             (see docs/testing.md)"
        );
        let mut argv = vec!["run".to_string(), source.to_string()];
        argv.extend(args.iter().map(|a| a.to_string()));
        PtyCommand {
            program: "wasmtime".into(),
            args: argv,
            cwd: None,
        }
    }
}

// Hand-written `#[test]` fns rather than `apps_e2e!`/`gzip_e2e!`: those macros
// take a bare `$lang:expr` and forwarding an optional leading attribute onto
// the generated fn is a local macro-parsing ambiguity (`#` can begin an expr
// fragment). Calling the shared runners directly is the simplest honest way to
// attach the `wasmtime_test` ignore gate while still routing through the exact
// same `run_app_cases`/`run_gzip_cases` the real backends use.

#[cfg_attr(not(feature = "wasmtime_test"), ignore)]
#[test]
fn apps() {
    run_app_cases(&Wasmtime);
}

#[cfg_attr(not(feature = "wasmtime_test"), ignore)]
#[test]
fn gzip() {
    run_gzip_cases(&Wasmtime);
}

// The filesystem app cases: the `wasmtime_test` feature is already the opt-in,
// so run the whole table unconditionally (`_forced`) rather than also requiring
// `DEWASM_APPS_ALL` — the gated `run_fs_app_cases` is for the codegen backends.
#[cfg_attr(not(feature = "wasmtime_test"), ignore)]
#[test]
fn fs_apps() {
    run_fs_app_cases_forced(&Wasmtime);
}

// Golden freshness for the interactive-REPL transcript (Fix 4): re-capture the
// bare qjs REPL under a real pty from a live wasmtime and require it to equal
// the checked-in `qjs_repl_interactive.transcript`. Set `DEWASM_UPDATE_GOLDEN=1`
// to (re)generate the golden instead of comparing — the one sanctioned way to
// refresh it when the pinned qjs binary or the scripted session changes.
#[cfg_attr(not(feature = "wasmtime_test"), ignore)]
#[test]
fn qjs_repl_interactive_golden() {
    if std::env::var("DEWASM_UPDATE_GOLDEN").is_ok() {
        let bytes = capture_qjs_repl_golden(&Wasmtime);
        println!(
            "wrote qjs interactive REPL golden ({} bytes) to {:?}",
            bytes.len(),
            qjs_repl_golden_path()
        );
        return;
    }
    let golden = std::fs::read(qjs_repl_golden_path()).unwrap_or_else(|e| {
        panic!(
            "qjs repl golden {:?} not readable ({e}) — regenerate with \
             DEWASM_UPDATE_GOLDEN=1 (see docs/testing.md)",
            qjs_repl_golden_path()
        )
    });
    let got = capture_qjs_repl_transcript(&Wasmtime);
    assert_transcript_eq(&got, &golden, "wasmtime");
    println!(
        "qjs interactive REPL under wasmtime: matches the golden ({} bytes)",
        got.len()
    );
}
