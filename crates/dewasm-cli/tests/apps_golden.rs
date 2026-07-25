//! The one app-level test that inherently needs every backend's toolchain
//! (here, a real `wasmtime`) rather than a single backend, so it stays in the
//! CLI crate rather than moving to a backend crate (ADR-27): re-validates the
//! checked-in golden files against a live `wasmtime run`.
//!
//! `apps_golden_matches_wasmtime` re-runs every `apps` case through
//! `wasmtime run` and compares its output against the golden files in
//! `examples/apps/golden/` — the check the old wasmtime-diffing design did on
//! every run, now opt-in behind the `wasmtime_test` feature (`#[ignore]`d
//! otherwise) since `wasmtime` is deliberately not part of the default suite's
//! required tools (ADR-15). The always-on per-backend `apps` tests already
//! cover the other half (generated output vs. golden).

use std::process::Command;

use dewasm_test_helper::{apps_cache_dir, run_command, APP_CASES};

/// Golden-file freshness check: does `examples/apps/golden/*.stdout` still
/// match what the currently-cached binary actually produces under a real
/// `wasmtime run`? Run after re-pinning an app version in
/// `examples/apps/fetch.sh`, or any time you doubt a golden file (see
/// docs/testing.md):
///
/// ```console
/// $ cargo test -p dewasm-cli --test apps_golden --features wasmtime_test apps_golden_matches_wasmtime
/// ```
///
/// Ignored by default and behind the `wasmtime_test` feature (rather than
/// always-on) because `wasmtime` is deliberately not one of the default
/// suite's required tools (ADR-15) — this test exists to check the checker,
/// not to run on every `cargo test`.
#[cfg_attr(not(feature = "wasmtime_test"), ignore)]
#[test]
fn apps_golden_matches_wasmtime() {
    assert!(
        Command::new("wasmtime").arg("--version").output().is_ok(),
        "wasmtime not found on PATH — required when running with --features wasmtime_test"
    );
    let cache = apps_cache_dir();
    for case in APP_CASES {
        let wasm_path = cache.join(format!("{}.wasm", case.name));
        assert!(
            wasm_path.exists(),
            "{} not cached — run examples/apps/fetch.sh (see docs/testing.md)",
            case.name
        );
        let output = run_command(
            Command::new("wasmtime").arg(&wasm_path).args(case.args),
            case.stdin,
        );
        assert_eq!(
            String::from_utf8_lossy(&output.stdout),
            case.expect_stdout,
            "{} {:?}: golden stdout is stale — regenerate it (docs/testing.md)",
            case.name,
            case.args
        );
        assert_eq!(
            output.status.code(),
            Some(case.expect_code),
            "{} {:?}: golden exit code is stale — regenerate it (docs/testing.md)",
            case.name,
            case.args
        );
        println!(
            "{} {:?}: golden output matches wasmtime",
            case.name, case.args
        );
    }
}
