//! Snapshot freshness: re-validates the checked-in `examples/apps/snapshots/` against wasmtime through the same shared app/gzip/fs runners every backend uses; the per-backend `apps`/`gzip`/`fs_apps` suites cover the other half (generated output vs. snapshot).
//!
//! Every case here runs wasm through the `xtask` binary, which embeds the `wasmtime` crate pinned by `Cargo.lock` (`cargo xtask test-wasmtime-wasi`, plus the `test-wasmtime-doom-frame`/`test-wasmtime-nes-frame` captures).
//! Build it once with `cargo build -p xtask`; this suite never builds it and never skips when it is missing.
//!
//! Named `apps_wasmtime` for the family: a future engine-under-test (wasmer, wasmedge) would join here the same way.
//! Conditional behind the `wasmtime_test` feature and `#[ignore]`d otherwise: this suite exists to check the checker, not to run on every `cargo test`.
//!
//! Run it with:
//!
//! ```console
//! $ cargo build -p xtask
//! $ cargo test -p dewasm-test-helper --features wasmtime_test --test apps_wasmtime
//! ```

// Hand-written `#[test]` fns rather than the per-case `*_e2e!` macros: those macros take a bare `$lang:expr` and forwarding an optional leading attribute onto the generated fn is a local macro-parsing ambiguity (`#` can begin an expr fragment).
// Calling the shared runners directly is the simplest honest way to attach the `wasmtime_test` `#[ignore]` attribute while still routing through the exact same runners the real backends use.
// The runners themselves run unconditionally (the slow per-case macros carry their own `slow_test`-feature `#[ignore]` instead), so `wasmtime_test` alone decides whether every test in this file runs.

#[cfg_attr(not(feature = "wasmtime_test"), ignore)]
#[test]
fn cowsay_args() {
    dewasm_test_helper::run_app_case(
        &dewasm_test_helper::Wasmtime,
        &dewasm_test_helper::COWSAY_ARGS,
    );
}

#[cfg_attr(not(feature = "wasmtime_test"), ignore)]
#[test]
fn cowsay_stdin() {
    dewasm_test_helper::run_app_case(
        &dewasm_test_helper::Wasmtime,
        &dewasm_test_helper::COWSAY_STDIN,
    );
}

#[cfg_attr(not(feature = "wasmtime_test"), ignore)]
#[test]
fn mruby_eh() {
    dewasm_test_helper::run_app_case(&dewasm_test_helper::Wasmtime, &dewasm_test_helper::MRUBY_EH);
}

#[cfg_attr(not(feature = "wasmtime_test"), ignore)]
#[test]
fn qjs_eval() {
    dewasm_test_helper::run_slow_app_case(
        &dewasm_test_helper::Wasmtime,
        &dewasm_test_helper::QJS_EVAL,
    );
}

#[cfg_attr(not(feature = "wasmtime_test"), ignore)]
#[test]
fn sqlite3_shell() {
    dewasm_test_helper::run_slow_app_case(
        &dewasm_test_helper::Wasmtime,
        &dewasm_test_helper::SQLITE3_SHELL,
    );
}

// The wasi-vfs-packed CRuby: its `AppCase` expectation is an inline string (deterministic one-liner, same convention as the interpreter fs hellos), so this run is what validates it against a live engine, with zero preopens, which is the point of the packed shape.
#[cfg_attr(not(feature = "wasmtime_test"), ignore)]
#[test]
fn cruby_packed_hello() {
    dewasm_test_helper::run_slow_app_case(
        &dewasm_test_helper::Wasmtime,
        &dewasm_test_helper::CRUBY_PACKED_HELLO,
    );
}

#[cfg_attr(not(feature = "wasmtime_test"), ignore)]
#[test]
fn gzip() {
    dewasm_test_helper::run_gzip_cases(&dewasm_test_helper::Wasmtime);
}

// The filesystem app cases: the `wasmtime_test` feature is already the opt-in, and `run_fs_app_case` runs unconditionally, so wasmtime runs every case it can here; its `run_app_fs` override ignores the glue, so each case is driven with an empty glue string.
// `TOYWASM_COWSAY` is the one exclusion: wasmtime answers `fd_fdstat_set_flags(0, NONBLOCK)` with EBADF and toywasm's WASI setup treats that as fatal, so wasmtime cannot run that binary.
// The case takes its ground truth from the `cowsay_args` snapshot instead (see the case const).
// Hand-written rather than via the per-case `*_e2e!` macros because those cannot carry the `wasmtime_test` `#[ignore]` attribute (the same reason `apps`/`gzip` above are hand-written).
#[cfg_attr(not(feature = "wasmtime_test"), ignore)]
#[test]
fn fs_apps() {
    dewasm_test_helper::run_fs_app_case(
        &dewasm_test_helper::Wasmtime,
        &dewasm_test_helper::QJS_FILE_IO,
        "",
    );
    dewasm_test_helper::run_fs_app_case(
        &dewasm_test_helper::Wasmtime,
        &dewasm_test_helper::SQLITE3_SHELL_DBFILE,
        "",
    );
    dewasm_test_helper::run_fs_app_case(
        &dewasm_test_helper::Wasmtime,
        &dewasm_test_helper::RG_SEARCH,
        "",
    );
    dewasm_test_helper::run_fs_app_case(
        &dewasm_test_helper::Wasmtime,
        &dewasm_test_helper::CPYTHON_HELLO,
        "",
    );
    dewasm_test_helper::run_fs_app_case(
        &dewasm_test_helper::Wasmtime,
        &dewasm_test_helper::CRUBY_HELLO,
        "",
    );
}

// The standalone `--dir` interface run against wasmtime as ground truth: `run_standalone_dir`'s wasmtime path consumes `--dir` as a host flag, the exact behavior the generated backends' own `--dir` parsing must reproduce.
// Uses no cached app, only the committed `wasi_standalone_dir.wat`.
#[cfg_attr(not(feature = "wasmtime_test"), ignore)]
#[test]
fn standalone_dir() {
    dewasm_test_helper::run_standalone_dir(&dewasm_test_helper::Wasmtime);
}

// The two framebuffer snapshots: neither module runs as a WASI command (each has its own export/import interface), so the runner exposes their capture as its own subcommand and the frame arrives on stdout.
// Compare-only, like every other case here.
#[cfg_attr(not(feature = "wasmtime_test"), ignore)]
#[test]
fn doom_frame() {
    assert_frame_snapshot(
        "test-wasmtime-doom-frame",
        &dewasm_test_helper::doom_frame_snapshot_path(),
    );
}

#[cfg_attr(not(feature = "wasmtime_test"), ignore)]
#[test]
fn nes_frame() {
    assert_frame_snapshot(
        "test-wasmtime-nes-frame",
        &dewasm_test_helper::nes_frame_snapshot_path(),
    );
}

/// Run the runner's `subcommand` capture and require its stdout to equal the snapshot at `path`.
fn assert_frame_snapshot(subcommand: &str, path: &std::path::Path) {
    let output = std::process::Command::new(dewasm_test_helper::wasi_runner_bin())
        .arg(subcommand)
        .output()
        .expect("spawn the xtask runner");
    assert!(
        output.status.success(),
        "xtask {subcommand}: exit {}\n{}",
        output.status,
        String::from_utf8_lossy(&output.stderr)
    );
    let snapshot = std::fs::read(path).unwrap_or_else(|e| {
        panic!(
            "{} not readable ({e}): regenerate with `cargo xtask update-snapshots` \
             (see docs/testing.md)",
            path.display()
        )
    });
    assert!(
        output.stdout == snapshot,
        "xtask {subcommand}: frame differs from {} ({} vs {} bytes)",
        path.display(),
        output.stdout.len(),
        snapshot.len()
    );
    println!(
        "{subcommand}: matches the snapshot ({} bytes)",
        snapshot.len()
    );
}

// Snapshot freshness for the interactive-REPL transcript: re-capture the bare qjs REPL under a real pty and require it to equal the checked-in `qjs_repl_interactive.transcript`.
// Compare-only; regenerate with `cargo xtask update-snapshots` when the pinned qjs binary or the scripted session changes.
#[cfg_attr(not(feature = "wasmtime_test"), ignore)]
#[test]
fn qjs_repl_interactive_snapshot() {
    let snapshot =
        std::fs::read(dewasm_test_helper::qjs_repl_snapshot_path()).unwrap_or_else(|e| {
            panic!(
                "qjs repl snapshot {:?} not readable ({e}): regenerate with \
             `cargo xtask update-snapshots` (see docs/testing.md)",
                dewasm_test_helper::qjs_repl_snapshot_path()
            )
        });
    let got = dewasm_test_helper::capture_qjs_repl_transcript(&dewasm_test_helper::Wasmtime);
    // Under Linux the engine leaves one extra trailing CRLF on pty teardown that macOS (where the snapshot was captured) does not; the converted backends match the snapshot byte-for-byte on both hosts, so the difference is in the engine's exit path, outside the transcript's meaningful content (it ends at the `\q` echo).
    // Compare modulo trailing CRLFs here only: the per-backend comparison stays exact (issue #33).
    dewasm_test_helper::assert_transcript_eq(
        trim_trailing_crlfs(&got),
        trim_trailing_crlfs(&snapshot),
        "wasmtime",
    );
    println!(
        "qjs interactive REPL under wasmtime: matches the snapshot ({} bytes)",
        got.len()
    );
}

/// Strip any trailing `\r\n` pairs (see `qjs_repl_interactive_snapshot`).
fn trim_trailing_crlfs(mut t: &[u8]) -> &[u8] {
    while let Some(rest) = t.strip_suffix(b"\r\n") {
        t = rest;
    }
    t
}
