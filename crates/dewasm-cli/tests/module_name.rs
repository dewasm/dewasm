//! The CLI half of the module-name policy: `--module-name` is a library-mode flag, required there and rejected for standalone, and an invalid name is rejected by the backend before anything is written.

use std::path::PathBuf;
use std::process::{Command, Output};

fn dewasm_bin() -> &'static str {
    env!("CARGO_BIN_EXE_dewasm")
}

fn run_dewasm(args: &[&str]) -> Output {
    Command::new(dewasm_bin())
        .args(args)
        .output()
        .expect("spawn dewasm")
}

/// A fixture `.wat` named `stem.wat` in a fresh temp dir, so the default module name is exactly `stem`. The counter keeps parallel test threads (and concurrent `cargo test` processes) off each other's dirs.
fn fixture(stem: &str) -> PathBuf {
    static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let dir = std::env::temp_dir().join(format!(
        "dewasm-module-name-{}-{}-{stem}",
        std::process::id(),
        COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let wat = dir.join(format!("{stem}.wat"));
    std::fs::write(&wat, "(module (func (export \"f\")))").unwrap();
    wat
}

fn stderr(out: &Output) -> String {
    String::from_utf8_lossy(&out.stderr).into_owned()
}

/// A standalone artifact's internal name is fixed, so naming it is a mistake worth reporting.
#[test]
fn module_name_with_standalone_is_rejected() {
    let wat = fixture("prog");
    let out = run_dewasm(&[
        wat.to_str().unwrap(),
        "-t",
        "ruby",
        "-m",
        "standalone",
        "--module-name",
        "Prog",
        "-o",
        "-",
    ]);
    assert!(!out.status.success(), "expected a rejection");
    assert!(
        stderr(&out).contains("standalone output has a fixed internal name"),
        "got: {}",
        stderr(&out)
    );
}

/// Standalone conversion of a hyphenated stem still works: the stem is not a module name there.
#[test]
fn standalone_accepts_any_stem() {
    let wat = fixture("sqlite3-shell");
    let out = run_dewasm(&[
        wat.to_str().unwrap(),
        "-t",
        "ruby",
        "-m",
        "standalone",
        "-o",
        "-",
    ]);
    assert!(out.status.success(), "failed: {}", stderr(&out));
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("class Program\n"),
        "expected the fixed standalone class"
    );
}

/// Library mode has no default name at all: omitting --module-name is an error before any backend runs.
#[test]
fn library_requires_module_name() {
    let wat = fixture("sqlite3-shell");
    let out = run_dewasm(&[
        wat.to_str().unwrap(),
        "-t",
        "ruby",
        "-m",
        "library",
        "-o",
        "-",
    ]);
    assert!(!out.status.success(), "expected a rejection");
    let err = stderr(&out);
    assert!(
        err.contains("library mode requires --module-name"),
        "got: {err}"
    );
}

#[test]
fn library_uses_an_explicit_name_verbatim() {
    let wat = fixture("sqlite3-shell");
    let out = run_dewasm(&[
        wat.to_str().unwrap(),
        "-t",
        "ruby",
        "-m",
        "library",
        "--module-name",
        "Dewasm::Sqlite3",
        "-o",
        "-",
    ]);
    assert!(out.status.success(), "failed: {}", stderr(&out));
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("class Dewasm::Sqlite3\n"), "got:\n{stdout}");
}
