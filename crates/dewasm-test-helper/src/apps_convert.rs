//! Whole-cache per-backend conversion suite.
//! It converts every cached real-world app under `examples/apps/cache/` with a backend.
//! The conversion must complete with non-empty source.
//! The generated program is never executed.
//!
//! The execution e2e suites feed the emitter only some pairs.
//! Those suites are `apps`, `apps_fs`, `apps_capi`, and `doom`.
//! Those are the (backend × app) pairs a backend also *runs*.
//! CRuby is only converted under Go, and CPython only under Java.
//! The whole filesystem family never reaches the Bash emitter.
//! This suite closes that gap: every backend converts every app, whether or not it runs it.
//! A conversion regression on an un-run pair now fails a fast, deterministic test.
//! It no longer hides until someone adds an execution case.
//!
//! One libtest-mimic [`Trial`] per manifest entry, and the trial name is the cache-file stem.
//! So cargo's own name filter works (`cargo test --test convert qjs`).
//! The manifest is fixed, one entry per `.wasm` the fetch scripts produce.
//! The fetch scripts are `examples/apps/scripts/*.sh`.
//! Each entry carries the conversion [`Mode`] the app's shape and the execution suites use.
//! A missing cache file fails the trial, it never skips.
//!
//! `slow_test` mirrors the backend crate's feature of the same name.
//! Heavy trials are `#[ignore]`d unless it is on.
//! Those are the ones whose dev-profile conversion measurably hurts the fast test.
//! Which trials are heavy comes from measurement, not the artifact size alone.

use dewasm_backend::{Backend, GenOptions, Mode, RuntimeLinkage, SupportStatus};
use dewasm_core::feature::Feature;
use libtest_mimic::{Failed, Trial};

use crate::backend::{derive_module_name, module_name_style};
use crate::fixtures::apps_cache_dir;

struct AppConvert {
    /// Cache-file stem: `<stem>.wasm` under `examples/apps/cache/`.
    /// It is also the trial name cargo's `--test convert <stem>` filter matches.
    stem: &'static str,
    /// The shape the app converts under.
    /// `Standalone` is for command-shaped apps (a `_start`).
    /// `Library` is for reactor/library artifacts.
    /// It is the same mode each execution e2e suite already converts the artifact with.
    mode: Mode,
    /// Heavy: dev-profile conversion exceeds ~2 s on every backend.
    /// That measurably slows the fast test.
    /// So the trial is `#[ignore]`d unless the backend crate's `slow_test` feature is on.
    /// Measured, not guessed: only the three giant artifacts cross the line.
    /// Those are `ruby` (~7-13 s), `cpython` (~2.6-5 s), and the 25 MB `zeroperl`.
    /// `zeroperl` is Perl 5.42 and takes ~4-5 s on Ruby and Python.
    /// The next-slowest, `rg`, stays ~1.1-2.1 s, in the same cluster as the sqlite cases.
    /// It is left in the fast test.
    heavy: bool,
    /// A wasm proposal beyond the wasm 1.0 baseline that this app's module uses.
    /// The expectation flips per backend on [`Backend::feature_status`].
    /// A backend declaring the feature `Supported` must convert the app.
    /// Any other backend must reject it with the attributed `check_module_support` error.
    /// Both directions are asserted.
    /// So a backend gaining the feature without flipping its declaration (or the reverse) fails.
    requires: Option<Feature>,
}

/// Every `.wasm` the fetch scripts (`examples/apps/scripts/*.sh`) drop into `examples/apps/cache/`.
/// Command-shaped apps (with a `_start`) convert `Standalone`.
/// Reactor/library artifacts convert `Library`, doom included: every backend converts it `Library`.
/// The `heavy` flags are derived from measurement; see the module docs.
const MANIFEST: &[AppConvert] = &[
    AppConvert {
        stem: "cowsay",
        mode: Mode::Standalone,
        heavy: false,
        requires: None,
    },
    AppConvert {
        stem: "cpython",
        mode: Mode::Standalone,
        heavy: true,
        requires: None,
    },
    AppConvert {
        stem: "dwarf-fixture",
        mode: Mode::Standalone,
        heavy: false,
        requires: None,
    },
    AppConvert {
        stem: "minigzip",
        mode: Mode::Standalone,
        heavy: false,
        requires: None,
    },
    AppConvert {
        stem: "mruby",
        mode: Mode::Standalone,
        heavy: false,
        requires: Some(Feature::ExceptionHandling),
    },
    AppConvert {
        stem: "qjs",
        mode: Mode::Standalone,
        heavy: false,
        requires: None,
    },
    AppConvert {
        stem: "rg",
        mode: Mode::Standalone,
        heavy: false,
        requires: None,
    },
    AppConvert {
        stem: "ruby",
        mode: Mode::Standalone,
        heavy: true,
        requires: None,
    },
    AppConvert {
        stem: "ruby-packed",
        mode: Mode::Standalone,
        heavy: true,
        requires: None,
    },
    AppConvert {
        stem: "sqlite3-mod",
        mode: Mode::Standalone,
        heavy: false,
        requires: None,
    },
    AppConvert {
        stem: "sqlite3-shell",
        mode: Mode::Standalone,
        heavy: false,
        requires: None,
    },
    AppConvert {
        stem: "toywasm",
        mode: Mode::Standalone,
        heavy: false,
        requires: None,
    },
    AppConvert {
        stem: "wasm3",
        mode: Mode::Standalone,
        heavy: false,
        requires: Some(Feature::TailCall),
    },
    AppConvert {
        stem: "doom",
        mode: Mode::Library,
        heavy: false,
        requires: None,
    },
    AppConvert {
        stem: "libpcap",
        mode: Mode::Library,
        heavy: false,
        requires: None,
    },
    AppConvert {
        stem: "libsqlite3",
        mode: Mode::Library,
        heavy: false,
        requires: None,
    },
    AppConvert {
        stem: "sqlite3-binding",
        mode: Mode::Library,
        heavy: false,
        requires: None,
    },
    AppConvert {
        stem: "treesitter",
        mode: Mode::Library,
        heavy: false,
        requires: None,
    },
    AppConvert {
        stem: "zeroperl",
        mode: Mode::Library,
        heavy: true,
        requires: None,
    },
];

/// Build one [`Trial`] per manifest entry for `backend` (the `apps_convert_suite!` entry point).
/// Heavy trials are marked `#[ignore]`d unless `slow_test` is on.
/// `slow_test` mirrors the backend crate's feature of the same name.
/// This is the same slow/fast split the spec harness applies to its non-curated files.
pub fn apps_convert_trials(backend: &'static (dyn Backend + Sync), slow_test: bool) -> Vec<Trial> {
    MANIFEST
        .iter()
        .map(|entry| {
            let ignored = entry.heavy && !slow_test;
            Trial::test(entry.stem, move || run_convert(backend, entry)).with_ignored_flag(ignored)
        })
        .collect()
}

/// harness=false entry point: parse cargo's test arguments (name filter,
/// `--ignored`/`--include-ignored`, thread count) and run the trials.
pub fn apps_convert_main(backend: &'static (dyn Backend + Sync), slow_test: bool) {
    let args = libtest_mimic::Arguments::from_args();
    libtest_mimic::run(&args, apps_convert_trials(backend, slow_test)).exit();
}

/// Convert one cached app and require non-empty source.
/// A missing cache file fails loud; a conversion error surfaces with its full chain so a
/// `check_module_support` rejection or a codegen bug reads plainly.
fn run_convert(backend: &'static (dyn Backend + Sync), entry: &AppConvert) -> Result<(), Failed> {
    let wasm = apps_cache_dir().join(format!("{}.wasm", entry.stem));
    if !wasm.exists() {
        return Err(Failed::from(format!(
            "{} not cached: run examples/apps/setup.sh (see docs/testing.md)",
            entry.stem
        )));
    }
    let bytes = std::fs::read(&wasm).map_err(|e| format!("read {}: {e}", wasm.display()))?;
    // Cache stems are kebab-case (`sqlite3-shell`).
    // The backends take a module name in their own grammar and refuse to guess, so convert it here.
    // Standalone entries do not use the name internally, but deriving uniformly keeps one rule.
    let module_name = derive_module_name(module_name_style(backend.name()), entry.stem);
    if let Some(feature) = entry.requires {
        if backend.feature_status(feature) != SupportStatus::Supported {
            return match convert_source(backend, &bytes, entry.mode, &module_name) {
                Ok(_) => Err(Failed::from(format!(
                    "{} converted, but {} declares {} unsupported: flip feature_status or fix check_module_support",
                    entry.stem,
                    backend.name(),
                    feature.id(),
                ))),
                Err(e) if format!("{e:#}").contains(feature.id()) => Ok(()),
                Err(e) => Err(Failed::from(format!(
                    "{} was rejected, but not attributed to {}: {e:#}",
                    entry.stem,
                    feature.id(),
                ))),
            };
        }
    }
    let source = convert_source(backend, &bytes, entry.mode, &module_name)
        .map_err(|e| format!("{} convert failed: {e:#}", entry.stem))?;
    if source.is_empty() {
        return Err(Failed::from(format!(
            "{} converted to empty source",
            entry.stem
        )));
    }
    Ok(())
}

/// Convert `bytes` with `backend`.
/// Return the primary output file's bytes or the conversion error.
/// Runs on a roomy stack for the same reason as [`crate::convert_on_big_stack`].
/// SQLite-class control-flow nesting overflows the 2 MiB test-thread default.
/// Using it uniformly is harmless.
/// Unlike that helper this one is fallible: a convert suite reports a failure, it does not panic.
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
