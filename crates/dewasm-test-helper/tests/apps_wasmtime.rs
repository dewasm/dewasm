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
    apps_cache_dir, run_app_cases, run_command_bytes, run_gzip_cases, BackendUnderTest,
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

    /// Skip codegen entirely: return the path to the cached binary the goldens
    /// were captured from, which the shared runner then feeds to `run`.
    fn convert_app(&self, _bytes: &[u8], _mode: Mode, name: &str) -> String {
        apps_cache_dir()
            .join(format!("{name}.wasm"))
            .to_string_lossy()
            .into_owned()
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
