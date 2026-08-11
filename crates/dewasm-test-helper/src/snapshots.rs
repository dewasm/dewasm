//! The execution-snapshot registry: one entry per checked-in snapshot the WASI runner can regenerate, so `cargo xtask update-snapshots` regenerates every one from a single table instead of a per-case manual capture.
//!
//! Each [`WasmtimeSnapshot`] pairs a snapshot's write path with a capture closure that reruns the case under the `engine` given to [`wasmtime_snapshots`] and returns the bytes to write. The DOOM and NES frames are deliberately **not** here: each drives a custom-import interface through the `wasmtime` crate directly (issue #114), which this test-only helper crate must not depend on — xtask appends those targets itself.
//!
//! The compare-only `apps`/`gzip`/`fs_apps` suites never touch this module: capture stays a separate, explicit code path (no env-var "update mode" on the tests), keeping the freshness comparison honest (docs/testing.md).

use std::path::PathBuf;

use crate::apps::{
    capture_app_stdout, capture_gzip_compress, COWSAY_ARGS, COWSAY_STDIN, QJS_EVAL, SQLITE3_SHELL,
};
use crate::apps_fs::{capture_fs_app_stdout, QJS_FILE_IO, RG_SEARCH, SQLITE3_SHELL_DBFILE};
use crate::backend::BackendUnderTest;
use crate::fixtures::apps_snapshot_dir;
use crate::qjs_repl::{capture_qjs_repl_transcript, qjs_repl_snapshot_path};

/// One regenerable execution snapshot: a repo-relative `label` (used for the optional substring filter and the printed line), the absolute `path` to write, and a `capture` closure that reruns the case and returns the bytes. `capture` fails loud on a missing cache or a missing runner — the underlying runners carry the exact setup message.
pub struct WasmtimeSnapshot {
    pub label: String,
    pub path: PathBuf,
    pub capture: Box<dyn Fn() -> Vec<u8>>,
}

/// Every execution snapshot the WASI runner can regenerate — the nine targets excluding DOOM and NES, each mapped to its snapshot file by the `<case>.stdout`/`.gz`/`.transcript` convention. xtask iterates these (plus the DOOM and NES frames) for `update-snapshots`, passing the wasmtime `engine` that runs each case.
pub fn wasmtime_snapshots(engine: &'static dyn BackendUnderTest) -> Vec<WasmtimeSnapshot> {
    let dir = apps_snapshot_dir();
    let app = |file: &str, capture: Box<dyn Fn() -> Vec<u8>>| WasmtimeSnapshot {
        label: format!("examples/apps/snapshots/{file}"),
        path: dir.join(file),
        capture,
    };
    vec![
        app(
            "cowsay_args.stdout",
            Box::new(|| capture_app_stdout(engine, &COWSAY_ARGS)),
        ),
        app(
            "cowsay_stdin.stdout",
            Box::new(|| capture_app_stdout(engine, &COWSAY_STDIN)),
        ),
        app(
            "qjs.stdout",
            Box::new(|| capture_app_stdout(engine, &QJS_EVAL)),
        ),
        app(
            "sqlite3_shell.stdout",
            Box::new(|| capture_app_stdout(engine, &SQLITE3_SHELL)),
        ),
        app(
            "minigzip_compress.gz",
            Box::new(|| capture_gzip_compress(engine)),
        ),
        app(
            "qjs_file_io.stdout",
            Box::new(|| capture_fs_app_stdout(engine, &QJS_FILE_IO)),
        ),
        app(
            "sqlite3_shell_dbfile.stdout",
            Box::new(|| capture_fs_app_stdout(engine, &SQLITE3_SHELL_DBFILE)),
        ),
        app(
            "rg_search.stdout",
            Box::new(|| capture_fs_app_stdout(engine, &RG_SEARCH)),
        ),
        WasmtimeSnapshot {
            label: "examples/apps/snapshots/qjs_repl_interactive.transcript".to_string(),
            path: qjs_repl_snapshot_path(),
            capture: Box::new(|| capture_qjs_repl_transcript(engine)),
        },
    ]
}
