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

use dewasm_test_helper::{
    apps_cache_dir, apps_fixtures_dir, apps_golden_dir, fresh_scratch_dir, run_command, APP_CASES,
};

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

/// The filesystem-exercising app goldens (Phase 5a) captured with
/// `wasmtime --dir <scratch>::<guest>`: QuickJS file I/O, the QuickJS scripted
/// REPL, and the sqlite3 shell writing then reopening a DB file. These are
/// Ruby-only for *execution* (only Ruby has WASI filesystem support, ADR-14),
/// but the golden is still ground-truthed against `wasmtime`, so the freshness
/// check lives here beside `apps_golden_matches_wasmtime`. Same `wasmtime_test`
/// gate and ignore-by-default policy (ADR-15).
///
/// Exact provenance is in docs/testing.md; each block below mirrors one
/// `wasmtime` invocation.
#[cfg_attr(not(feature = "wasmtime_test"), ignore)]
#[test]
fn apps_golden_fs_matches_wasmtime() {
    assert!(
        Command::new("wasmtime").arg("--version").output().is_ok(),
        "wasmtime not found on PATH — required when running with --features wasmtime_test"
    );
    let cache = apps_cache_dir();
    let fixtures = apps_fixtures_dir();
    let golden = apps_golden_dir();

    // QuickJS file I/O: preopen a scratch dir at /work, run the fixture, check
    // both stdout and the file the guest wrote back on the host.
    {
        let work = fresh_scratch_dir("golden-qjs-file-io");
        std::fs::copy(fixtures.join("qjs_file_io.js"), work.join("qjs_file_io.js")).unwrap();
        let output = run_command(
            Command::new("wasmtime")
                .arg("--dir")
                .arg(format!("{}::/work", work.display()))
                .arg(cache.join("qjs.wasm"))
                .arg("/work/qjs_file_io.js"),
            "",
        );
        assert_eq!(
            String::from_utf8_lossy(&output.stdout),
            std::fs::read_to_string(golden.join("qjs_file_io.stdout")).unwrap(),
            "qjs_file_io: golden stdout is stale — regenerate it (docs/testing.md)"
        );
        assert_eq!(output.status.code(), Some(0), "qjs_file_io: exit code");
        assert_eq!(
            std::fs::read_to_string(work.join("io_out.txt")).unwrap(),
            "hello from qjs file io\n",
            "qjs_file_io: host file the guest wrote is wrong"
        );
    }

    // QuickJS scripted REPL over piped stdin (the tty-free equivalent; see
    // docs/apps-audit.md).
    {
        let work = fresh_scratch_dir("golden-qjs-repl");
        std::fs::copy(fixtures.join("qjs_repl.js"), work.join("qjs_repl.js")).unwrap();
        let output = run_command(
            Command::new("wasmtime")
                .arg("--dir")
                .arg(format!("{}::/work", work.display()))
                .arg(cache.join("qjs.wasm"))
                .arg("/work/qjs_repl.js"),
            "1+2\n[3,1,2].sort()\nMath.max(4,9)\n\\q\n",
        );
        assert_eq!(
            String::from_utf8_lossy(&output.stdout),
            std::fs::read_to_string(golden.join("qjs_repl.stdout")).unwrap(),
            "qjs_repl: golden stdout is stale — regenerate it (docs/testing.md)"
        );
        assert_eq!(output.status.code(), Some(0), "qjs_repl: exit code");
    }

    // sqlite3 shell writing a DB file, then a second invocation reopening it:
    // only the second run's SELECT output is the golden; the first run just
    // has to leave a nonzero-size DB file behind.
    {
        let db = fresh_scratch_dir("golden-sqlite3-dbfile");
        let create = run_command(
            Command::new("wasmtime")
                .arg("--dir")
                .arg(format!("{}::/db", db.display()))
                .arg(cache.join("sqlite3-shell.wasm")),
            ".open /db/test.db\n\
             CREATE TABLE t(id INTEGER PRIMARY KEY, v TEXT);\n\
             INSERT INTO t(v) VALUES ('alpha'),('beta');\n\
             .exit\n",
        );
        assert_eq!(create.status.code(), Some(0), "sqlite3 dbfile create: exit");
        let db_file = db.join("test.db");
        assert!(
            db_file.metadata().map(|m| m.len() > 0).unwrap_or(false),
            "sqlite3 dbfile: the first run left no nonzero DB file"
        );
        let select = run_command(
            Command::new("wasmtime")
                .arg("--dir")
                .arg(format!("{}::/db", db.display()))
                .arg(cache.join("sqlite3-shell.wasm")),
            ".open /db/test.db\nSELECT id, v FROM t ORDER BY id;\n.exit\n",
        );
        assert_eq!(
            String::from_utf8_lossy(&select.stdout),
            std::fs::read_to_string(golden.join("sqlite3_shell_dbfile.stdout")).unwrap(),
            "sqlite3_shell_dbfile: golden stdout is stale — regenerate it (docs/testing.md)"
        );
        assert_eq!(select.status.code(), Some(0), "sqlite3 dbfile select: exit");
    }

    println!("filesystem app goldens match wasmtime");
}
