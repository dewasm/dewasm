//! Shared test harness, case tables, and per-feature test macros for the
//! dewasm backend crates (ADR-27). This crate depends only on `dewasm-core`
//! and `dewasm-backend` — never on a concrete backend crate — and is taken as
//! a dev-dependency by each backend crate, which supplies a
//! [`BackendUnderTest`] (and, for the spec harness, a [`SpecBackend`]) and
//! wires up the suites it participates in via the macros below.

mod apps;
mod backend;
mod fixtures;
mod library;
mod spec;
mod standalone;
mod wasi;

pub use apps::{run_app_cases, AppCase, APP_CASES};
pub use backend::{run_command, run_script, BackendUnderTest};
pub use fixtures::{apps_cache_dir, convert, convert_bytes, convert_on_big_stack, examples_dir};
pub use library::{run_library_case, GlueResolver, LibraryCase, LIBRARY_CASES};
pub use spec::{run_spec_suite, Converted, SpecBackend};
pub use standalone::{run_standalone_case, StandaloneCase, STANDALONE_CASES};
pub use wasi::{
    run_wasi_fs, run_wasi_standalone, WasiCase, WasiCheck, WasiFsGlue, WasiKind, WASI_CASES,
};

/// One `#[test]` running the shared spec harness for `$lang` (a
/// [`SpecBackend`]).
#[macro_export]
macro_rules! spec_suite {
    ($lang:expr) => {
        #[test]
        fn spec() {
            $crate::run_spec_suite(&$lang);
        }
    };
}

/// One `#[test]` iterating [`STANDALONE_CASES`] for `$lang` (a
/// [`BackendUnderTest`]).
#[macro_export]
macro_rules! standalone_e2e {
    ($lang:expr) => {
        #[test]
        fn standalone() {
            for case in $crate::STANDALONE_CASES {
                $crate::run_standalone_case(&$lang, case);
            }
        }
    };
}

/// One `#[test]` iterating [`LIBRARY_CASES`] for `$lang`, resolving each
/// case's glue with `$glue` (a `fn(&LibraryCase) -> &'static str`).
#[macro_export]
macro_rules! library_e2e {
    ($lang:expr, $glue:expr) => {
        #[test]
        fn library() {
            for case in $crate::LIBRARY_CASES {
                $crate::run_library_case(&$lang, case, $glue);
            }
        }
    };
}

/// One `#[test]` running the WASI cases of a given feature kind for `$lang`.
/// The no-glue form covers whole-program standalone kinds (`Stdio`,
/// `ArgsEnv`, `ClockRandom`); the `$glue` form covers `Fs` (library-mode runs
/// against a preopened host directory, so a per-backend instantiation glue
/// `fn(&WasiCase, &Path) -> String` is required).
#[macro_export]
macro_rules! wasi_suite {
    ($lang:expr, Stdio) => {
        #[test]
        fn wasi_stdio() {
            $crate::run_wasi_standalone(&$lang, $crate::WasiKind::Stdio);
        }
    };
    ($lang:expr, ArgsEnv) => {
        #[test]
        fn wasi_args_env() {
            $crate::run_wasi_standalone(&$lang, $crate::WasiKind::ArgsEnv);
        }
    };
    ($lang:expr, ClockRandom) => {
        #[test]
        fn wasi_clock_random() {
            $crate::run_wasi_standalone(&$lang, $crate::WasiKind::ClockRandom);
        }
    };
    ($lang:expr, Fs, $glue:expr) => {
        #[test]
        fn wasi_fs() {
            $crate::run_wasi_fs(&$lang, $glue);
        }
    };
}

/// One `#[test]` iterating [`APP_CASES`] for `$lang` (a [`BackendUnderTest`]).
#[macro_export]
macro_rules! apps_e2e {
    ($lang:expr) => {
        #[test]
        fn apps() {
            $crate::run_app_cases(&$lang);
        }
    };
}
