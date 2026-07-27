//! C-API-driving app cases (ADR-27): a converted library-mode artifact whose
//! C API is driven directly from host-language glue — `sqlite3_malloc`,
//! guest-memory pointer plumbing, and (for the callback case) an imported
//! `env.host_row` provider. Unlike the `apps`/`fs_apps` suites there is **no
//! wasmtime golden**: the CLI cannot drive a C-API flow whose results live in
//! guest memory, so each case pins a fixed expected string, every value in it
//! anchored by the amalgamation version pinned in `examples/apps/fetch.sh`.
//!
//! The case table is shared; the per-language variation is one glue resolver
//! `fn(&CApiCase, &Path) -> String` supplied by each backend crate, returning
//! the full driver program in that language (malloc/pointer plumbing/memory
//! access/provider registration). Which backends invoke `capi_apps_e2e!` is
//! the capability declaration (ADR-27 revision): Bash is not wired — it has no
//! WASI filesystem and no host-language object model to plumb a C API through
//! (ADR-12). The whole suite is gated behind `DEWASM_APPS_ALL` (it reconverts
//! the ~5 MB sqlite3 artifacts).

use std::path::{Path, PathBuf};

use dewasm_backend::Mode;

use crate::backend::BackendUnderTest;
use crate::fixtures::{apps_cache_dir, fresh_scratch_dir};

/// A C-API-driving case: convert `wasm` (cache stem) to library class `class`,
/// append the backend's glue, run it, and require exactly `expect_stdout`.
pub struct CApiCase {
    pub name: &'static str,
    /// Cache-binary stem (`examples/apps/cache/<wasm>.wasm`), also the
    /// conversion module name (every backend PascalCases it to `class`).
    pub wasm: &'static str,
    /// The library class name the glue instantiates (PascalCase of `wasm`).
    pub class: &'static str,
    /// The fixed stdout the drive must produce (no wasmtime golden possible).
    pub expect_stdout: &'static str,
    /// `Some(guest)` preopens a fresh scratch dir at guest `guest` for a
    /// file-backed DB drive; `None` is an in-memory (`:memory:`) drive that
    /// touches no host files. The glue bakes the preopen into its ctor call.
    pub preopen: Option<&'static str>,
    /// Host-side assertion over the scratch dir after the run (file cases);
    /// `|_| {}` when there is nothing to check.
    pub assert_host: fn(&Path),
    /// Backends excluded from this case, each `(lang name, human reason)`
    /// (ADR-27 revision) — a data-level capability/practicality exclusion the
    /// runner prints and honors.
    pub exclude: &'static [(&'static str, &'static str)],
}

/// No host-side effect to assert for this case.
fn assert_none(_: &Path) {}

/// The file-backed drive must leave a nonzero DB file behind at `<scratch>/data.db`.
fn assert_dbfile(scratch: &Path) {
    assert!(
        scratch
            .join("data.db")
            .metadata()
            .map(|m| m.len() > 0)
            .unwrap_or(false),
        "sqlite3 file C API: no nonzero DB file left on the host"
    );
}

pub const CAPI_CASES: &[CApiCase] = &[
    // The library half of the sqlite3 build (ADR-22): the C API driven in
    // memory. version + two SELECT rows + a sentinel, all pinned by the
    // amalgamation version.
    CApiCase {
        name: "libsqlite3_c_api",
        wasm: "libsqlite3",
        class: "Libsqlite3",
        expect_stdout: "version: 3.53.3\n20|y\n10|x\nC-API-OK\n",
        preopen: None,
        assert_host: assert_none,
        exclude: &[],
    },
    // The same C API opening a *file* under a preopen (ADR-22): create+insert,
    // close, reopen, select — proving the C-API path hits the same ADR-14 fs
    // stack as the shell. Leaves a nonzero DB file on the host.
    CApiCase {
        name: "sqlite3_file_c_api",
        wasm: "libsqlite3",
        class: "Libsqlite3",
        expect_stdout: "10|x\n20|y\nFILE-OK\n",
        preopen: Some("/db"),
        assert_host: assert_dbfile,
        exclude: &[],
    },
    // Guest->host callback round trip (ADR-22): our own committed C
    // (examples/apps/src/sqlite3_binding.c) exports `run_query`, which calls
    // `sqlite3_exec` with a C callback forwarding each row to the *imported*
    // `env.host_row`. The glue provides `host_row` via the ADR-7 import
    // provider and collects the rows.
    CApiCase {
        name: "sqlite3_callback_binding",
        wasm: "sqlite3-binding",
        class: "Sqlite3Binding",
        expect_stdout: "row: 2|y\nrow: 3|z\nCALLBACK-OK\n",
        preopen: None,
        assert_host: assert_none,
        exclude: &[],
    },
];

/// Resolve a case's per-language driver glue. Receives the case and a fresh
/// scratch dir (used only by the file-backed case, via `case.preopen`). A case
/// a backend is wired to run but has no glue for panics loudly (ADR-15).
pub type CApiGlue = fn(&CApiCase, &Path) -> String;

/// Run every [`CAPI_CASES`] entry for `lang`, gated behind `DEWASM_APPS_ALL`
/// (uniform perf opt-out; the cases reconvert the ~5 MB sqlite3 artifacts).
pub fn run_capi_cases(lang: &dyn BackendUnderTest, glue: CApiGlue) {
    if std::env::var("DEWASM_APPS_ALL").is_err() {
        println!(
            "C-API app cases skipped for {} (DEWASM_APPS_ALL=1 to run)",
            lang.name()
        );
        return;
    }
    let cache = apps_cache_dir();
    for case in CAPI_CASES {
        if let Some((_, reason)) = case.exclude.iter().find(|(l, _)| *l == lang.name()) {
            println!("{} excluded for {}: {reason}", case.name, lang.name());
            continue;
        }
        let wasm_path = cache.join(format!("{}.wasm", case.wasm));
        assert!(
            wasm_path.exists(),
            "{} not cached — run examples/apps/fetch.sh (see docs/testing.md)",
            case.wasm
        );
        let bytes = std::fs::read(&wasm_path).expect("read wasm");
        let class = lang.convert_app(&bytes, Mode::Library, case.wasm);

        let scratch: PathBuf = fresh_scratch_dir(&format!("{}-{}", lang.name(), case.name));
        let output = lang.run(&format!("{class}\n{}", glue(case, &scratch)), &[], "");
        assert!(
            output.status.success(),
            "{} under {}: nonzero exit {}\n{}",
            case.name,
            lang.name(),
            output.status,
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(
            String::from_utf8_lossy(&output.stdout),
            case.expect_stdout,
            "{} under {}: C-API drive output differs\nstderr: {}",
            case.name,
            lang.name(),
            String::from_utf8_lossy(&output.stderr)
        );
        (case.assert_host)(&scratch);
        println!("{} under {}: C-API drive matches", case.name, lang.name());
    }
}
