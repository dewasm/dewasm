//! The filesystem app goldens re-validated against a live `wasmtime` (ADR-27):
//! these need `wasmtime --dir` preopens rather than any single backend, so the
//! freshness check stays in the CLI crate. The stdout/exit-code and gzip
//! goldens moved to the wasmtime engine-under-test suite
//! (`dewasm-test-helper`'s `tests/apps_wasmtime.rs`), which runs them through
//! the same shared runners the real backends use.
//!
//! Opt-in behind the `wasmtime_test` feature (`#[ignore]`d otherwise) since
//! `wasmtime` is deliberately not part of the default suite's required tools
//! (ADR-15). The always-on per-backend `apps` tests already cover the other
//! half (generated output vs. golden).

use std::path::Path;
use std::process::Command;

use dewasm_test_helper::{
    apps_cache_dir, apps_fixtures_dir, apps_golden_dir, fresh_scratch_dir, run_command,
};

/// Recursively copy `src` into `dst`, to stage the committed ripgrep fixture
/// tree into a scratch dir before the `wasmtime --dir` golden run.
fn copy_tree(src: &Path, dst: &Path) {
    std::fs::create_dir_all(dst).unwrap();
    for entry in std::fs::read_dir(src).unwrap() {
        let entry = entry.unwrap();
        let to = dst.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_tree(&entry.path(), &to);
        } else {
            std::fs::copy(entry.path(), &to).unwrap();
        }
    }
}

/// The filesystem-exercising app goldens (Phase 5a) captured with
/// `wasmtime --dir <scratch>::<guest>`: QuickJS file I/O, the QuickJS scripted
/// REPL, and the sqlite3 shell writing then reopening a DB file. These are
/// Ruby-only for *execution* (only Ruby has WASI filesystem support, ADR-14),
/// but the golden is still ground-truthed against `wasmtime`, so the freshness
/// check lives here (the non-fs stdout goldens moved to
/// `dewasm-test-helper`'s `apps_wasmtime` suite). Same `wasmtime_test` gate and
/// ignore-by-default policy (ADR-15).
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

    // ripgrep (Phase 5b): stage the committed fixture tree into a scratch dir,
    // preopen it at /work, search recursively with a deterministic --sort path.
    {
        let work = fresh_scratch_dir("golden-rg-search");
        copy_tree(&fixtures.join("rg"), &work);
        let output = run_command(
            Command::new("wasmtime")
                .arg("--dir")
                .arg(format!("{}::/work", work.display()))
                .arg(cache.join("rg.wasm"))
                .arg("--sort")
                .arg("path")
                .arg("needle")
                .arg("/work"),
            "",
        );
        assert_eq!(
            String::from_utf8_lossy(&output.stdout),
            std::fs::read_to_string(golden.join("rg_search.stdout")).unwrap(),
            "rg_search: golden stdout is stale — regenerate it (docs/testing.md)"
        );
        assert_eq!(output.status.code(), Some(0), "rg_search: exit code");
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
