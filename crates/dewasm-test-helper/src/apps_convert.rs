//! Whole-cache per-backend conversion suite (ADR-54): convert every cached
//! real-world app under `examples/apps/cache/` with a backend and require the
//! conversion to complete with non-empty source — the generated program is
//! never executed.
//!
//! The execution e2e suites (`apps`, `apps_fs`, `apps_capi`, `doom`) only ever
//! feed the emitter the (backend × app) pairs a backend also *runs*: CRuby is
//! only converted under Go, CPython only under Java, and the whole filesystem
//! family never reaches the Bash emitter. This suite closes that gap so every
//! backend converts every app independent of whether it runs it — a conversion
//! regression on an un-run pair now fails a fast, deterministic test instead of
//! hiding until someone wires an execution case (ADR-54).
//!
//! One libtest-mimic [`Trial`] per manifest entry, trial name = the cache-file
//! stem so cargo's own name filter works (`cargo test --test convert qjs`). The
//! manifest is fixed — one entry per `.wasm` the fetch scripts
//! (`examples/apps/scripts/*.sh`) produce — with the conversion [`Mode`] each
//! app's shape and the execution suites already use. A missing cache file fails
//! the trial (ADR-15), it never skips.
//!
//! `slow_tier` is the backend crate's `slow_test` feature (ADR-48): heavy
//! trials — the ones whose dev-profile conversion measurably hurts the fast
//! gate — are `#[ignore]`d unless it is on. Which trials are heavy comes from
//! measurement (ADR-54), not the artifact size alone.

use dewasm_backend::{Backend, GenOptions, Mode, RuntimeLinkage};
use libtest_mimic::{Failed, Trial};

use crate::fixtures::apps_cache_dir;

/// One cached app: the cache-file stem (also the trial name), the conversion
/// [`Mode`], and whether the trial is heavy enough to gate behind `slow_test`.
struct AppConvert {
    /// Cache-file stem: `<stem>.wasm` under `examples/apps/cache/`, and the
    /// trial name cargo's `--test convert <stem>` filter matches.
    stem: &'static str,
    /// The shape the app converts under: `Standalone` for command-shaped apps
    /// (a `_start`), `Library` for reactor/library artifacts — the same mode
    /// each execution e2e suite already converts the artifact with.
    mode: Mode,
    /// Heavy: dev-profile conversion exceeds ~2 s on every backend, measurably
    /// slowing the fast gate, so the trial is `#[ignore]`d unless the backend
    /// crate's `slow_test` feature is on. Measured, not guessed: only the three
    /// giant artifacts — `ruby` (~7–13 s), `cpython` (~2.6–5 s), and the 25 MB
    /// `zeroperl` (Perl 5.42, ~4–5 s on Ruby and Python) — cross the line; the
    /// next-slowest, `rg`, stays ~1.1–2.1 s in the same cluster as the sqlite
    /// cases and is left in the fast gate (ADR-54).
    heavy: bool,
}

/// Every `.wasm` the fetch scripts (`examples/apps/scripts/*.sh`) drop into
/// `examples/apps/cache/`. Command-shaped apps (with a `_start`) convert
/// `Standalone`; reactor/library artifacts convert `Library` — doom included,
/// which every backend converts `Library` (ADR-53). The `heavy` flags are
/// derived from measurement (ADR-54); see the module docs.
const MANIFEST: &[AppConvert] = &[
    // Command-shaped (Standalone: a `_start`).
    AppConvert {
        stem: "cowsay",
        mode: Mode::Standalone,
        heavy: false,
    },
    AppConvert {
        stem: "cpython",
        mode: Mode::Standalone,
        heavy: true,
    },
    AppConvert {
        stem: "dwarf-fixture",
        mode: Mode::Standalone,
        heavy: false,
    },
    AppConvert {
        stem: "minigzip",
        mode: Mode::Standalone,
        heavy: false,
    },
    AppConvert {
        stem: "qjs",
        mode: Mode::Standalone,
        heavy: false,
    },
    AppConvert {
        stem: "rg",
        mode: Mode::Standalone,
        heavy: false,
    },
    AppConvert {
        stem: "ruby",
        mode: Mode::Standalone,
        heavy: true,
    },
    AppConvert {
        stem: "sqlite3-shell",
        mode: Mode::Standalone,
        heavy: false,
    },
    // Library/reactor artifacts.
    AppConvert {
        stem: "doom",
        mode: Mode::Library,
        heavy: false,
    },
    AppConvert {
        stem: "libpcap",
        mode: Mode::Library,
        heavy: false,
    },
    AppConvert {
        stem: "libsqlite3",
        mode: Mode::Library,
        heavy: false,
    },
    AppConvert {
        stem: "sqlite3-binding",
        mode: Mode::Library,
        heavy: false,
    },
    AppConvert {
        stem: "treesitter",
        mode: Mode::Library,
        heavy: false,
    },
    AppConvert {
        stem: "zeroperl",
        mode: Mode::Library,
        heavy: true,
    },
];

/// Build one [`Trial`] per manifest entry for `backend` (the `apps_convert_suite!`
/// entry point). Heavy trials are marked `#[ignore]`d unless `slow_tier` (the
/// backend crate's `slow_test` feature) is on — the same two-tier gate the spec
/// harness applies to its non-curated files (ADR-48).
pub fn apps_convert_trials(backend: &'static (dyn Backend + Sync), slow_tier: bool) -> Vec<Trial> {
    MANIFEST
        .iter()
        .map(|entry| {
            let ignored = entry.heavy && !slow_tier;
            Trial::test(entry.stem, move || run_convert(backend, entry)).with_ignored_flag(ignored)
        })
        .collect()
}

/// harness=false entry point: parse cargo's test arguments (name filter,
/// `--ignored`/`--include-ignored`, thread count) and run the trials.
pub fn apps_convert_main(backend: &'static (dyn Backend + Sync), slow_tier: bool) {
    let args = libtest_mimic::Arguments::from_args();
    libtest_mimic::run(&args, apps_convert_trials(backend, slow_tier)).exit();
}

/// Convert one cached app and require non-empty source. A missing cache file
/// fails loud (ADR-15); a conversion error surfaces with its full chain so a
/// `check_module_support` rejection or a codegen bug reads plainly.
fn run_convert(backend: &'static (dyn Backend + Sync), entry: &AppConvert) -> Result<(), Failed> {
    let wasm = apps_cache_dir().join(format!("{}.wasm", entry.stem));
    if !wasm.exists() {
        return Err(Failed::from(format!(
            "{} not cached — run examples/apps/setup.sh (see docs/testing.md)",
            entry.stem
        )));
    }
    let bytes = std::fs::read(&wasm).map_err(|e| format!("read {}: {e}", wasm.display()))?;
    let source = convert_source(backend, &bytes, entry.mode, entry.stem)
        .map_err(|e| format!("{} convert failed: {e:#}", entry.stem))?;
    if source.is_empty() {
        return Err(Failed::from(format!(
            "{} converted to empty source",
            entry.stem
        )));
    }
    Ok(())
}

/// Convert `bytes` with `backend`, returning the primary output file's bytes or
/// the conversion error. Runs on a roomy stack for the same reason as
/// [`crate::convert_on_big_stack`] (SQLite-class control-flow nesting overflows
/// the 2 MiB test-thread default); using it uniformly is harmless. Unlike that
/// helper this one is fallible — a convert suite reports a failure, it does not
/// panic on one.
fn convert_source(
    backend: &(dyn Backend + Sync),
    bytes: &[u8],
    mode: Mode,
    name: &str,
) -> anyhow::Result<Vec<u8>> {
    std::thread::scope(|scope| {
        std::thread::Builder::new()
            .stack_size(64 << 20)
            .spawn_scoped(scope, || -> anyhow::Result<Vec<u8>> {
                let module = dewasm_core::build_module(bytes)?;
                let mut files = backend.generate(
                    &module,
                    &GenOptions {
                        mode,
                        module_name: name.to_string(),
                        runtime: RuntimeLinkage::Embedded,
                        default_wasi: true,
                        data_file: None,
                    },
                )?;
                anyhow::ensure!(!files.is_empty(), "backend produced no output file");
                Ok(files.remove(0).contents)
            })
            .expect("spawn codegen thread")
            .join()
            .expect("codegen thread")
    })
}
