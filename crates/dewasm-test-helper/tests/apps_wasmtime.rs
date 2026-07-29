//! wasmtime as a [`BackendUnderTest`] (ADR-27): the golden-vs-wasmtime freshness check run through the *same* shared app/gzip runners every real backend uses, rather than a hand-written per-case loop. wasmtime does not generate source — it runs the cached `.wasm` binary directly — so its `convert_app` returns the path to the exact cache binary the goldens were captured from, and its `run`/`run_bytes` exec `wasmtime run <path>`. The per-backend `apps`/`gzip` suites cover the other half (generated output vs. golden); this suite re-validates the goldens themselves against a live engine.
//!
//! Named `apps_wasmtime` for the family: a future engine-under-test (wasmer, wasmedge) would join here the same way. Gated behind the `wasmtime_test` feature and `#[ignore]`d otherwise, because `wasmtime` is deliberately not one of the default suite's required tools (ADR-15) — this suite exists to check the checker, not to run on every `cargo test`.
//!
//! Run it with:
//!
//! ```console
//! $ cargo test -p dewasm-test-helper --features wasmtime_test --test apps_wasmtime
//! ```

use dewasm_test_helper::{
    assert_transcript_eq, capture_qjs_repl_transcript, qjs_repl_golden_path, run_app_case,
    run_fs_app_case, run_gzip_cases, run_slow_app_case, run_standalone_dir, Wasmtime, COWSAY_ARGS,
    COWSAY_STDIN, CPYTHON_HELLO, CRUBY_HELLO, QJS_EVAL, QJS_FILE_IO, QJS_REPL, RG_SEARCH,
    SQLITE3_SHELL, SQLITE3_SHELL_DBFILE,
};

// The wasmtime-backed `BackendUnderTest` (`Wasmtime`, plus the `NeverBackend` stand-in it uses to satisfy `BackendUnderTest::backend()`) lives in `dewasm_test_helper::wasmtime_backend` so `cargo xtask update-repl-golden` can drive it too; see that module for the implementation.

// Hand-written `#[test]` fns rather than the per-case `*_e2e!` macros: those macros take a bare `$lang:expr` and forwarding an optional leading attribute onto the generated fn is a local macro-parsing ambiguity (`#` can begin an expr fragment). Calling the shared runners directly is the simplest honest way to attach the `wasmtime_test` ignore gate while still routing through the exact same runners the real backends use. The runners themselves are ungated (the slow per-case macros carry their own `slow_test`-feature `#[ignore]` instead, ADR-27 revision), so `wasmtime_test` alone gates every test in this file.

#[cfg_attr(not(feature = "wasmtime_test"), ignore)]
#[test]
fn cowsay_args() {
    run_app_case(&Wasmtime, &COWSAY_ARGS);
}

#[cfg_attr(not(feature = "wasmtime_test"), ignore)]
#[test]
fn cowsay_stdin() {
    run_app_case(&Wasmtime, &COWSAY_STDIN);
}

#[cfg_attr(not(feature = "wasmtime_test"), ignore)]
#[test]
fn qjs_eval() {
    run_slow_app_case(&Wasmtime, &QJS_EVAL);
}

#[cfg_attr(not(feature = "wasmtime_test"), ignore)]
#[test]
fn sqlite3_shell() {
    run_slow_app_case(&Wasmtime, &SQLITE3_SHELL);
}

#[cfg_attr(not(feature = "wasmtime_test"), ignore)]
#[test]
fn gzip() {
    run_gzip_cases(&Wasmtime);
}

// The filesystem app cases: the `wasmtime_test` feature is already the opt-in, and `run_fs_app_case` is ungated, so wasmtime runs the full set with no per-case exclusion; its `run_app_fs` override ignores the glue, so each case is driven with an empty glue string. Hand-written rather than via the per-case `*_e2e!` macros because those cannot carry the `wasmtime_test` ignore gate (the same reason `apps`/`gzip` above are hand-written).
#[cfg_attr(not(feature = "wasmtime_test"), ignore)]
#[test]
fn fs_apps() {
    run_fs_app_case(&Wasmtime, &QJS_FILE_IO, "");
    run_fs_app_case(&Wasmtime, &QJS_REPL, "");
    run_fs_app_case(&Wasmtime, &SQLITE3_SHELL_DBFILE, "");
    run_fs_app_case(&Wasmtime, &RG_SEARCH, "");
    run_fs_app_case(&Wasmtime, &CPYTHON_HELLO, "");
    run_fs_app_case(&Wasmtime, &CRUBY_HELLO, "");
}

// The standalone `--dir` interface (ADR-31) run against wasmtime as ground truth: `run_standalone_dir`'s wasmtime path consumes `--dir` as a host flag (`wasmtime run --dir HOST::GUEST <wasm>`), the exact behavior the generated backends' own `--dir` parsing must reproduce. Uses no cached app, only the committed `wasi_standalone_dir.wat`, so it needs only wasmtime on PATH.
#[cfg_attr(not(feature = "wasmtime_test"), ignore)]
#[test]
fn standalone_dir() {
    run_standalone_dir(&Wasmtime);
}

// Golden freshness for the interactive-REPL transcript (Fix 4): re-capture the bare qjs REPL under a real pty from a live wasmtime and require it to equal the checked-in `qjs_repl_interactive.transcript`. Compare-only; regenerate with `cargo xtask update-repl-golden` when the pinned qjs binary or the scripted session changes.
#[cfg_attr(not(feature = "wasmtime_test"), ignore)]
#[test]
fn qjs_repl_interactive_golden() {
    let golden = std::fs::read(qjs_repl_golden_path()).unwrap_or_else(|e| {
        panic!(
            "qjs repl golden {:?} not readable ({e}) — regenerate with \
             `cargo xtask update-repl-golden` (see docs/testing.md)",
            qjs_repl_golden_path()
        )
    });
    let got = capture_qjs_repl_transcript(&Wasmtime);
    // Linux wasmtime emits one extra trailing CRLF on pty teardown that macOS wasmtime (which captured the golden) does not; the converted backends match the golden byte-for-byte on both hosts, so the difference is wasmtime's own exit behavior, outside the transcript's meaningful content (it ends at the `\q` echo). Compare modulo trailing CRLFs here only — the per-backend comparison stays exact (issue #33).
    assert_transcript_eq(
        trim_trailing_crlfs(&got),
        trim_trailing_crlfs(&golden),
        "wasmtime",
    );
    println!(
        "qjs interactive REPL under wasmtime: matches the golden ({} bytes)",
        got.len()
    );
}

/// Strip any trailing `\r\n` pairs (see `qjs_repl_interactive_golden`).
fn trim_trailing_crlfs(mut t: &[u8]) -> &[u8] {
    while let Some(rest) = t.strip_suffix(b"\r\n") {
        t = rest;
    }
    t
}
