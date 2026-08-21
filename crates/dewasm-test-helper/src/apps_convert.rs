//! Whole-cache per-backend conversion suite: convert every cached real-world app under `examples/apps/cache/` with a backend and require the conversion to complete with non-empty source.
//! The generated program is never executed.
//!
//! The execution e2e suites (`apps`, `apps_fs`, `apps_capi`, `doom`) only ever feed the emitter the (backend × app) pairs a backend also *runs*: CRuby is only converted under Go, CPython only under Java, and the whole filesystem family never reaches the Bash emitter.
//! This suite closes that gap so every backend converts every app independent of whether it runs it: a conversion regression on an un-run pair now fails a fast, deterministic test instead of hiding until someone wires an execution case.
//!
//! One libtest-mimic [`Trial`] per manifest entry, trial name = the cache-file stem so cargo's own name filter works (`cargo test --test convert qjs`).
//! The manifest is fixed, one entry per `.wasm` the fetch scripts
//! (`examples/apps/scripts/*.sh`) produce, with the conversion [`Mode`] each app's shape and the execution suites already use.
//! A missing cache file fails the trial, it never skips.
//!
//! `slow_test` mirrors the backend crate's feature of the same name: heavy trials (the ones whose dev-profile conversion measurably hurts the fast test) are `#[ignore]`d unless it is on.
//! Which trials are heavy comes from measurement, not the artifact size alone.

use dewasm_backend::{Backend, GenOptions, Mode, RuntimeLinkage, SupportStatus};
use dewasm_core::feature::Feature;
use libtest_mimic::{Failed, Trial};

use crate::backend::{derive_module_name, module_name_style};
use crate::fixtures::apps_cache_dir;

struct AppConvert {
    /// Cache-file stem: `<stem>.wasm` under `examples/apps/cache/`, and the trial name cargo's `--test convert <stem>` filter matches.
    stem: &'static str,
    /// The shape the app converts under: `Standalone` for command-shaped apps
    /// (a `_start`), `Library` for reactor/library artifacts, the same mode each execution e2e suite already converts the artifact with.
    mode: Mode,
    /// Heavy: dev-profile conversion exceeds ~2 s on every backend, measurably slowing the fast test, so the trial is `#[ignore]`d unless the backend crate's `slow_test` feature is on.
    /// Measured, not guessed: only the three giant artifacts cross the line: `ruby` (~7-13 s), `cpython` (~2.6-5 s), and the 25 MB `zeroperl` (Perl 5.42, ~4-5 s on Ruby and Python); the next-slowest, `rg`, stays ~1.1-2.1 s in the same cluster as the sqlite cases and is left in the fast test.
    heavy: bool,
    /// A wasm proposal beyond the wasm 1.0 baseline that this app's module uses.
    /// The expectation flips per backend on [`Backend::feature_status`]: a backend declaring the feature `Supported` must convert the app, any other backend must reject it with the attributed `check_module_support` error.
    /// Both directions are asserted, so a backend gaining the feature without flipping its declaration (or the reverse) fails this suite.
    requires: Option<Feature>,
}

/// Every `.wasm` the fetch scripts (`examples/apps/scripts/*.sh`) drop into
/// `examples/apps/cache/`.
/// Command-shaped apps (with a `_start`) convert
/// `Standalone`; reactor/library artifacts convert `Library` (doom included, which every backend converts `Library`).
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

/// Build one [`Trial`] per manifest entry for `backend` (the `apps_convert_suite!`
/// entry point).
/// Heavy trials are marked `#[ignore]`d unless `slow_test` (mirroring the backend crate's feature of the same name) is on: the same slow/fast split the spec harness applies to its non-curated files.
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
    // Cache stems are kebab-case (`sqlite3-shell`); the backends take a module name in their own grammar and refuse to guess, so convert it here.
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

/// Convert `bytes` with `backend`, returning the primary output file's bytes or the conversion error.
/// Runs on a roomy stack for the same reason as
/// [`crate::convert_on_big_stack`] (SQLite-class control-flow nesting overflows the 2 MiB test-thread default); using it uniformly is harmless.
/// Unlike that helper this one is fallible: a convert suite reports a failure, it does not panic on one.
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
