//! wasmtime as a [`BackendUnderTest`] (ADR-27): the golden-vs-wasmtime freshness checks run through the *same* shared app/gzip runners every real backend uses, rather than a hand-written per-case loop. wasmtime does not generate source — it runs the cached `.wasm` binary directly — so its `convert_app` returns the path to the exact cache binary the goldens were captured from, and its `run`/`run_bytes` exec `wasmtime run <path>`.
//!
//! Public (not gated behind the `wasmtime_test` feature, which only gates the *tests* in `tests/apps_wasmtime.rs`) so that both that test file and `cargo xtask update-repl-golden` can drive the same wasmtime-backed [`BackendUnderTest`] to (re)capture the interactive-REPL golden.

use std::path::Path;
use std::process::{Command, Output};

use dewasm_backend::{Backend, GenOptions, Mode, OutputFile};
use dewasm_core::ir;

use crate::backend::{run_command_bytes, BackendUnderTest};
use crate::pty::PtyCommand;

/// A [`Backend`] that only exists to satisfy `BackendUnderTest::backend()`. wasmtime runs the cached binary directly, so codegen is never reached; if anything routes into `generate()` it is a wiring bug, not a supported path.
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

/// The wasmtime engine wired into the shared app/gzip runners. `convert_app` hands back the cache-binary path (no codegen); `run_bytes` execs `wasmtime run <path> <args...>` on that exact binary.
pub struct Wasmtime;

impl BackendUnderTest for Wasmtime {
    fn name(&self) -> &'static str {
        "wasmtime"
    }

    fn backend(&self) -> &'static (dyn Backend + Sync) {
        &NeverBackend
    }

    /// Skip codegen entirely: write the exact bytes the shared runner read from the cache to a temp `.wasm` (keyed by content hash so identical apps share one file) and return that path, which the runner then feeds to `run`/`run_app_fs`. Writing the bytes rather than rebuilding the cache path from `name` keeps this independent of the conversion module name, which no longer always equals the cache stem (e.g. CRuby: cache `ruby.wasm`, class `Cruby`).
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

    /// `source` is a cache-binary path (from `convert_app`), not generated code: exec `wasmtime run <path> <args...>` with `stdin` piped in. Per ADR-15 a missing `wasmtime` fails loud, never skips.
    fn run_bytes(&self, source: &str, args: &[&str], stdin: &[u8]) -> Output {
        assert!(
            Command::new("wasmtime").arg("--version").output().is_ok(),
            "wasmtime not found on PATH — required for the wasmtime-backed suites \
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

    /// Run the filesystem app directly on the cache binary: `program` is the wasm path (from `convert_app`), so ignore the appended `glue` the default composes and instead exec `wasmtime run --dir <host>::<guest>... --env K=V... <wasm> <args[1..]>` (wasmtime injects argv0 itself). Per ADR-15 a missing `wasmtime` fails loud, never skips.
    fn run_app_fs(
        &self,
        program: &str,
        _glue: &str,
        args: &[&str],
        env: &[(&str, &str)],
        stdin: &[u8],
        preopens: &[(&str, &Path)],
    ) -> Output {
        assert!(
            Command::new("wasmtime").arg("--version").output().is_ok(),
            "wasmtime not found on PATH — required for the wasmtime-backed suites \
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

    /// Standalone `--dir` ground truth (ADR-31): `program` is the wasm path (from `convert_app`), and `--dir` is a wasmtime host flag, so run `wasmtime run --dir HOST::GUEST... <wasm> args`. This is what the generated backends' own `--dir` parsing must reproduce. Per ADR-15 a missing `wasmtime` fails loud, never skips.
    fn run_standalone_dir(
        &self,
        program: &str,
        preopens: &[(&str, &Path)],
        args: &[&str],
        stdin: &[u8],
    ) -> Output {
        assert!(
            Command::new("wasmtime").arg("--version").output().is_ok(),
            "wasmtime not found on PATH — required for the wasmtime-backed suites \
             (see docs/testing.md)"
        );
        let mut cmd = Command::new("wasmtime");
        cmd.arg("run");
        for (guest, host) in preopens {
            cmd.arg("--dir")
                .arg(format!("{}::{}", host.display(), guest));
        }
        cmd.arg(program).args(args);
        run_command_bytes(&mut cmd, stdin)
    }

    /// Drive the cached wasm binary directly under a pty: `source` is the wasm path (from `convert_app`), so the command is `wasmtime run <path> <args...>` — the ground-truth interactive session the backends must match. wasmtime injects argv0, so a bare no-args call is the interactive REPL invocation.
    fn pty_command(&self, source: &str, args: &[&str]) -> PtyCommand {
        assert!(
            Command::new("wasmtime").arg("--version").output().is_ok(),
            "wasmtime not found on PATH — required for the wasmtime-backed suites \
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
