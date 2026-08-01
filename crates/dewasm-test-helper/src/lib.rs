//! Shared test harness, case tables, and per-feature test macros for the dewasm backend crates (ADR-27). This crate depends only on `dewasm-core` and `dewasm-backend` — never on a concrete backend crate — and is taken as a dev-dependency by each backend crate, which supplies a [`BackendUnderTest`] (and, for the spec harness, a [`SpecBackend`]) and wires up the suites it participates in via the macros below.

mod apps;
mod apps_capi;
mod apps_convert;
mod apps_fs;
mod backend;
mod doom;
mod fixtures;
mod glue;
mod library;
mod multimodule;
mod pty;
mod qjs_repl;
mod spec;
mod wasi;
mod wasi_testsuite;
mod wasmtime_backend;

pub use apps::{
    run_app_case, run_gzip_cases, run_slow_app_case, AppCase, COWSAY_ARGS, COWSAY_STDIN, QJS_EVAL,
    SQLITE3_SHELL,
};
pub use apps_capi::{
    run_capi_case, CApiCase, EXIFTOOL_EXTRACT, LIBSQLITE3_C_API, PCAP_COMPILE,
    SQLITE3_CALLBACK_BINDING, SQLITE3_FILE_C_API, TREESITTER_PARSE, ZEROPERL_EVAL,
};
pub use apps_convert::{apps_convert_main, apps_convert_trials};
pub use apps_fs::{
    run_fs_app_case, FsAppCase, FsRun, Stage, CPYTHON_HELLO, CRUBY_HELLO, QJS_FILE_IO, RG_SEARCH,
    SQLITE3_SHELL_DBFILE,
};
pub use backend::{
    run_command, run_command_bytes, run_script, run_script_bytes, write_temp_script,
    BackendUnderTest,
};
pub use doom::{
    doom_frame_snapshot_path, doom_wasm_path, frame_to_ppm, run_doom_frame_case,
    DOOM_CLOCK_STEP_MS, DOOM_FRAME_H, DOOM_FRAME_W, DOOM_TICKS,
};
pub use fixtures::{
    apps_cache_dir, apps_fixtures_dir, apps_snapshot_dir, convert, convert_bytes,
    convert_on_big_stack, examples_dir, fresh_scratch_dir,
};
pub use library::{
    run_library_case, LibraryCase, CUSTOM_WASI_PROVIDER, LIBRARY_ADD, PARTIAL_OVERRIDE,
    STDIO_CAPTURE, WASI_IMPORT_OVERRIDE,
};
pub use multimodule::{run_multi_module_case, MultiModuleCase, EMBEDDED_COEXIST, SHARED_TABLE};
pub use pty::{run_under_pty, PtyCommand};
pub use qjs_repl::{
    assert_transcript_eq, capture_qjs_repl_snapshot, capture_qjs_repl_transcript,
    qjs_repl_snapshot_path, run_qjs_repl_pty, QJS_REPL_SESSION,
};
pub use spec::{spec_main, spec_trials, Converted, SpecBackend};
pub use wasi::{
    run_deep_recursion, run_standalone_dir, run_wasi_containment, run_wasi_fs, run_wasi_standalone,
    WasiCase, WasiCheck, WasiKind, WASI_CASES,
};
pub use wasi_testsuite::{wasi_testsuite_main, wasi_testsuite_trials, WasiTestsuiteBackend};
pub use wasmtime_backend::Wasmtime;

/// The `harness = false` `main` of a backend's spec integration test: builds one libtest-mimic trial per `.wast` file for `$lang` (a [`SpecBackend`]) and runs them with cargo's own test arguments (name filter, `--ignored`/`--include-ignored`, thread count). `$lang` must be a promotable-to-`'static` value — the backend `Spec` structs are unit structs, so `spec_suite!(RubySpec)` promotes `&RubySpec` to `&'static`.
#[macro_export]
macro_rules! spec_suite {
    ($lang:expr) => {
        fn main() {
            $crate::spec_main(&$lang, cfg!(feature = "slow_test"));
        }
    };
}

/// The `harness = false` `main` of a backend's whole-cache convert integration test (ADR-54): builds one libtest-mimic trial per cached app for `$backend` (a `Backend + Sync` value) and runs them with cargo's own test arguments. Unlike [`spec_suite!`] this takes the plain [`Backend`] — the convert suite only lowers, it never runs generated code, so it needs no interpreter or script-phrasing layer. `$backend` must be a promotable-to-`'static` value; the backend `Backend` structs are unit structs, so `apps_convert_suite!(RubyBackend)` promotes `&RubyBackend` to `&'static`. Heavy trials are `#[ignore]`d unless the expanding crate's `slow_test` feature is on (ADR-48).
///
/// [`Backend`]: dewasm_backend::Backend
#[macro_export]
macro_rules! apps_convert_suite {
    ($backend:expr) => {
        fn main() {
            $crate::apps_convert_main(&$backend, cfg!(feature = "slow_test"));
        }
    };
}

/// The `harness = false` `main` of a backend's WASI-testsuite integration test (ADR-36): builds one libtest-mimic trial per prebuilt `.wasm` for `$lang` (a [`WasiTestsuiteBackend`]) and runs them with cargo's own test arguments. Like [`spec_suite!`], `$lang` is a promotable-to-`'static` unit struct.
///
/// [`WasiTestsuiteBackend`]: crate::WasiTestsuiteBackend
#[macro_export]
macro_rules! wasi_testsuite_suite {
    ($lang:expr) => {
        fn main() {
            $crate::wasi_testsuite_main(&$lang);
        }
    };
}

/// Internal: attach a two-tier `#[ignore]` gate to a generated `#[test]` item (ADR-48). The per-case app macros below delegate here so a callsite can pick the tier without duplicating the gate. `#[macro_export]` is load-bearing despite the macro being internal: the delegating macros expand inside the backend crates, where `$crate::slow_tier_test!` resolves only to an exported macro (a plain `macro_rules!` cannot even be `pub use`d across crates, E0364) — `#[doc(hidden)]` keeps it out of the public docs instead.
///
/// * `slow` — gated on the backend crate's `slow_test` feature (CI's main sweep tier). This is the default for every slow-case macro.
/// * `ultra` — gated on `ultra_slow_test` (which implies `slow_test`), for a case measured at roughly a minute or more on a CI runner. These are kept out of CI and run only under `--features ultra_slow_test` or `-- --include-ignored`, in local pre-release verification.
#[doc(hidden)]
#[macro_export]
macro_rules! slow_tier_test {
    (slow, $item:item) => {
        #[cfg_attr(
            not(feature = "slow_test"),
            ignore = "slow app case: --features slow_test"
        )]
        $item
    };
    (ultra, $item:item) => {
        #[cfg_attr(
            not(feature = "ultra_slow_test"),
            ignore = "ultra-slow app case (1min+ on a CI runner): --features ultra_slow_test"
        )]
        $item
    };
}

/// Per-case library macros (ADR-27 revision): each expands to one `#[test] fn <case>()` running the named [`LibraryCase`] const for `$lang` with `$glue` (a named `&str` const in the backend crate). A backend declares participation by invoking the macro and drops it (with a REASON comment) for a capability it lacks.
///
/// [`LIBRARY_ADD`]: crate::LIBRARY_ADD
#[macro_export]
macro_rules! library_add_e2e {
    ($lang:expr, $glue:expr) => {
        #[test]
        fn library_add() {
            $crate::run_library_case(&$lang, &$crate::LIBRARY_ADD, $glue);
        }
    };
}

/// See [`library_add_e2e!`]. Runs [`WASI_IMPORT_OVERRIDE`](crate::WASI_IMPORT_OVERRIDE).
#[macro_export]
macro_rules! wasi_import_override_e2e {
    ($lang:expr, $glue:expr) => {
        #[test]
        fn wasi_import_override() {
            $crate::run_library_case(&$lang, &$crate::WASI_IMPORT_OVERRIDE, $glue);
        }
    };
}

/// See [`library_add_e2e!`]. Runs [`CUSTOM_WASI_PROVIDER`](crate::CUSTOM_WASI_PROVIDER).
#[macro_export]
macro_rules! custom_wasi_provider_e2e {
    ($lang:expr, $glue:expr) => {
        #[test]
        fn custom_wasi_provider() {
            $crate::run_library_case(&$lang, &$crate::CUSTOM_WASI_PROVIDER, $glue);
        }
    };
}

/// See [`library_add_e2e!`]. Runs [`PARTIAL_OVERRIDE`](crate::PARTIAL_OVERRIDE).
#[macro_export]
macro_rules! partial_override_e2e {
    ($lang:expr, $glue:expr) => {
        #[test]
        fn partial_override() {
            $crate::run_library_case(&$lang, &$crate::PARTIAL_OVERRIDE, $glue);
        }
    };
}

/// See [`library_add_e2e!`]. Runs [`STDIO_CAPTURE`](crate::STDIO_CAPTURE).
#[macro_export]
macro_rules! stdio_capture_e2e {
    ($lang:expr, $glue:expr) => {
        #[test]
        fn stdio_capture() {
            $crate::run_library_case(&$lang, &$crate::STDIO_CAPTURE, $glue);
        }
    };
}

/// One `#[test]` running the WASI cases of a given feature kind for `$lang`. The no-glue form covers whole-program standalone kinds (`Stdio`, `ArgsEnv`, `ClockRandom`, `Poll`); the `Fs` form covers the filesystem cases (library-mode runs against a preopened host directory), taking a single per-backend glue **template** const whose `{guest}`/`{host}` placeholders the runner fills with each case's preopen pair (ADR-27 revision).
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
    ($lang:expr, Poll) => {
        #[test]
        fn wasi_poll() {
            $crate::run_wasi_standalone(&$lang, $crate::WasiKind::Poll);
        }
    };
    ($lang:expr, Fs, $template:expr) => {
        #[test]
        fn wasi_fs() {
            $crate::run_wasi_fs(&$lang, $template);
        }
    };
}

/// One `#[test]` exercising the standalone `--dir` interface (ADR-31) for `$lang`: convert `wasi_standalone_dir.wat` standalone, run it with a `--dir` mount, and require the file round-trip to succeed. No glue — standalone needs none. Wired by every filesystem backend (and re-run under wasmtime as ground truth); Bash joined once its WASI filesystem landed (ADR-34).
#[macro_export]
macro_rules! standalone_dir_e2e {
    ($lang:expr) => {
        #[test]
        fn standalone_dir() {
            $crate::run_standalone_dir(&$lang);
        }
    };
}

/// One `#[test]` requiring the standalone entrypoint to survive deep-but-valid guest recursion for `$lang`: convert `deep_recursion.wat` (5000-frame recursion) standalone, run it, and require the guest's `proc_exit(42)` as the exit code — see [`run_deep_recursion`](crate::run_deep_recursion). No glue — this exercises the emitted entrypoint itself. Wired by Python, whose entrypoint applies ADR-28's mitigation (issue #31); each other backend's host recursion depth is its own concern — it invokes this once its entrypoint needs (and has) such a mitigation.
#[macro_export]
macro_rules! deep_recursion_e2e {
    ($lang:expr) => {
        #[test]
        fn deep_recursion() {
            $crate::run_deep_recursion(&$lang);
        }
    };
}

/// One `#[test]` running the root-preopen containment probe for `$lang` with `$glue` (a named `&str` const that drives the WASI resolver directly). Split out of `wasi_suite!(Fs)` because it does not fit the shared preopen-and-run template — see [`run_wasi_containment`](crate::run_wasi_containment).
#[macro_export]
macro_rules! wasi_root_containment_e2e {
    ($lang:expr, $glue:expr) => {
        #[test]
        fn wasi_root_containment() {
            $crate::run_wasi_containment(&$lang, $glue);
        }
    };
}

/// Per-case app macros (ADR-27 revision): each expands to one `#[test] fn <case>()` running the named [`AppCase`] const for `$lang` (a [`BackendUnderTest`]). No glue argument — these are standalone-mode stdin/args cases, so no host-language glue is needed. `cowsay_args_e2e!` and `cowsay_stdin_e2e!` always run; `qjs_eval_e2e!` and `sqlite3_shell_e2e!` are slow — their generated `#[test]` is `#[ignore]`d unless the expanding backend crate's `slow_test` feature is enabled (run with `--features slow_test` or `cargo test -- --include-ignored`). A callsite may pass a trailing tier token (`slow`, the default, or `ultra`); see [`slow_tier_test!`] (ADR-48).
///
/// [`AppCase`]: crate::AppCase
#[macro_export]
macro_rules! cowsay_args_e2e {
    ($lang:expr) => {
        #[test]
        fn cowsay_args() {
            $crate::run_app_case(&$lang, &$crate::COWSAY_ARGS);
        }
    };
}

/// See [`cowsay_args_e2e!`]. Runs [`COWSAY_STDIN`](crate::COWSAY_STDIN).
#[macro_export]
macro_rules! cowsay_stdin_e2e {
    ($lang:expr) => {
        #[test]
        fn cowsay_stdin() {
            $crate::run_app_case(&$lang, &$crate::COWSAY_STDIN);
        }
    };
}

/// See [`cowsay_args_e2e!`]. Runs the slow [`QJS_EVAL`](crate::QJS_EVAL) case. Slow: the generated `#[test]` is `#[ignore]`d unless the expanding backend crate's `slow_test` feature is enabled (ADR-27 revision) — run it with `--features slow_test` or `cargo test -- --include-ignored`. Pass a trailing `ultra` to promote it to the ultra tier ([`slow_tier_test!`], ADR-48).
#[macro_export]
macro_rules! qjs_eval_e2e {
    ($lang:expr) => {
        $crate::qjs_eval_e2e!($lang, slow);
    };
    ($lang:expr, $tier:tt) => {
        $crate::slow_tier_test! { $tier,
            #[test]
            fn qjs_eval() {
                $crate::run_slow_app_case(&$lang, &$crate::QJS_EVAL);
            }
        }
    };
}

/// See [`cowsay_args_e2e!`]. Runs the slow [`SQLITE3_SHELL`](crate::SQLITE3_SHELL) case. Slow: see [`qjs_eval_e2e!`] for the `#[ignore]`/`slow_test` feature gate and the trailing tier token.
#[macro_export]
macro_rules! sqlite3_shell_e2e {
    ($lang:expr) => {
        $crate::sqlite3_shell_e2e!($lang, slow);
    };
    ($lang:expr, $tier:tt) => {
        $crate::slow_tier_test! { $tier,
            #[test]
            fn sqlite3_shell() {
                $crate::run_slow_app_case(&$lang, &$crate::SQLITE3_SHELL);
            }
        }
    };
}

/// One `#[test]` running the gzip byte-stdio stress cases (minigzip) for `$lang`. Separate from the app macros above because those cases carry binary stdin/stdout an `&str`/`include_str!` `AppCase` cannot represent (`run_gzip_cases`).
#[macro_export]
macro_rules! gzip_e2e {
    ($lang:expr) => {
        #[test]
        fn gzip() {
            $crate::run_gzip_cases(&$lang);
        }
    };
}

/// One `#[test]` driving the bare QuickJS interactive REPL under a real pty for `$lang` and comparing the transcript byte-for-byte to the wasmtime snapshot (Fix 4). Slow: see [`qjs_eval_e2e!`] for the `#[ignore]`/`slow_test` feature gate and the trailing tier token.
#[macro_export]
macro_rules! qjs_repl_pty_e2e {
    ($lang:expr) => {
        $crate::qjs_repl_pty_e2e!($lang, slow);
    };
    ($lang:expr, $tier:tt) => {
        $crate::slow_tier_test! { $tier,
            #[test]
            fn qjs_repl_pty() {
                $crate::run_qjs_repl_pty(&$lang);
            }
        }
    };
}

/// Per-case filesystem-app macros (ADR-27 revision): each expands to one `#[test] fn <case>()` running the named [`FsAppCase`] const for `$lang` with `$glue` (a named `&str` const in the backend crate whose `{scratch}`/`{cache}` placeholders the runner fills). A backend declares participation by invoking the macro and drops it (with a REASON comment) for a case it cannot run. Slow: the generated `#[test]` is `#[ignore]`d unless the expanding backend crate's `slow_test` feature is enabled (see [`qjs_eval_e2e!`]); a trailing tier token after `$glue` promotes a case to the ultra tier ([`slow_tier_test!`]).
///
/// [`FsAppCase`]: crate::FsAppCase
#[macro_export]
macro_rules! qjs_file_io_e2e {
    ($lang:expr, $glue:expr) => {
        $crate::qjs_file_io_e2e!($lang, $glue, slow);
    };
    ($lang:expr, $glue:expr, $tier:tt) => {
        $crate::slow_tier_test! { $tier,
            #[test]
            fn qjs_file_io() {
                $crate::run_fs_app_case(&$lang, &$crate::QJS_FILE_IO, $glue);
            }
        }
    };
}

/// See [`qjs_file_io_e2e!`]. Runs [`SQLITE3_SHELL_DBFILE`](crate::SQLITE3_SHELL_DBFILE).
#[macro_export]
macro_rules! sqlite3_shell_dbfile_e2e {
    ($lang:expr, $glue:expr) => {
        $crate::sqlite3_shell_dbfile_e2e!($lang, $glue, slow);
    };
    ($lang:expr, $glue:expr, $tier:tt) => {
        $crate::slow_tier_test! { $tier,
            #[test]
            fn sqlite3_shell_dbfile() {
                $crate::run_fs_app_case(&$lang, &$crate::SQLITE3_SHELL_DBFILE, $glue);
            }
        }
    };
}

/// See [`qjs_file_io_e2e!`]. Runs [`RG_SEARCH`](crate::RG_SEARCH).
#[macro_export]
macro_rules! rg_search_e2e {
    ($lang:expr, $glue:expr) => {
        $crate::rg_search_e2e!($lang, $glue, slow);
    };
    ($lang:expr, $glue:expr, $tier:tt) => {
        $crate::slow_tier_test! { $tier,
            #[test]
            fn rg_search() {
                $crate::run_fs_app_case(&$lang, &$crate::RG_SEARCH, $glue);
            }
        }
    };
}

/// See [`qjs_file_io_e2e!`]. Runs [`CPYTHON_HELLO`](crate::CPYTHON_HELLO).
#[macro_export]
macro_rules! cpython_hello_e2e {
    ($lang:expr, $glue:expr) => {
        $crate::cpython_hello_e2e!($lang, $glue, slow);
    };
    ($lang:expr, $glue:expr, $tier:tt) => {
        $crate::slow_tier_test! { $tier,
            #[test]
            fn cpython_hello() {
                $crate::run_fs_app_case(&$lang, &$crate::CPYTHON_HELLO, $glue);
            }
        }
    };
}

/// See [`qjs_file_io_e2e!`]. Runs [`CRUBY_HELLO`](crate::CRUBY_HELLO).
#[macro_export]
macro_rules! cruby_hello_e2e {
    ($lang:expr, $glue:expr) => {
        $crate::cruby_hello_e2e!($lang, $glue, slow);
    };
    ($lang:expr, $glue:expr, $tier:tt) => {
        $crate::slow_tier_test! { $tier,
            #[test]
            fn cruby_hello() {
                $crate::run_fs_app_case(&$lang, &$crate::CRUBY_HELLO, $glue);
            }
        }
    };
}

/// Per-case C-API macros (ADR-27 revision): each expands to one `#[test] fn <case>()` running the named [`CApiCase`] const for `$lang` with `$glue` (a named `&str` const in the backend crate; the file-backed case's `{scratch}` placeholder is filled by the runner). Which backends invoke these is the capability declaration (ADR-27): Ruby/Python/Go/Java, not Bash (ADR-12). Slow: the generated `#[test]` is `#[ignore]`d unless the expanding backend crate's `slow_test` feature is enabled (see [`qjs_eval_e2e!`]).
///
/// [`CApiCase`]: crate::CApiCase
#[macro_export]
macro_rules! libsqlite3_c_api_e2e {
    ($lang:expr, $glue:expr) => {
        $crate::libsqlite3_c_api_e2e!($lang, $glue, slow);
    };
    ($lang:expr, $glue:expr, $tier:tt) => {
        $crate::slow_tier_test! { $tier,
            #[test]
            fn libsqlite3_c_api() {
                $crate::run_capi_case(&$lang, &$crate::LIBSQLITE3_C_API, $glue);
            }
        }
    };
}

/// See [`libsqlite3_c_api_e2e!`]. Runs [`SQLITE3_FILE_C_API`](crate::SQLITE3_FILE_C_API).
#[macro_export]
macro_rules! sqlite3_file_c_api_e2e {
    ($lang:expr, $glue:expr) => {
        $crate::sqlite3_file_c_api_e2e!($lang, $glue, slow);
    };
    ($lang:expr, $glue:expr, $tier:tt) => {
        $crate::slow_tier_test! { $tier,
            #[test]
            fn sqlite3_file_c_api() {
                $crate::run_capi_case(&$lang, &$crate::SQLITE3_FILE_C_API, $glue);
            }
        }
    };
}

/// See [`libsqlite3_c_api_e2e!`]. Runs the libpcap BPF-compile case [`PCAP_COMPILE`](crate::PCAP_COMPILE): drives `compile_filter` on "tcp port 80" and prints the serialized BPF program. Slow (a ~2 MB reactor artifact reconverted per run), so gated like the sqlite C-API cases.
#[macro_export]
macro_rules! pcap_compile_e2e {
    ($lang:expr, $glue:expr) => {
        $crate::pcap_compile_e2e!($lang, $glue, slow);
    };
    ($lang:expr, $glue:expr, $tier:tt) => {
        $crate::slow_tier_test! { $tier,
            #[test]
            fn pcap_compile() {
                $crate::run_capi_case(&$lang, &$crate::PCAP_COMPILE, $glue);
            }
        }
    };
}

/// See [`libsqlite3_c_api_e2e!`]. Runs the tree-sitter JSON-parse case [`TREESITTER_PARSE`](crate::TREESITTER_PARSE): drives `parse_source` on a fixed JSON snippet and prints the parse tree's S-expression. Slow (a ~1.5 MB reactor artifact reconverted per run), so gated like the sqlite C-API cases.
#[macro_export]
macro_rules! treesitter_parse_e2e {
    ($lang:expr, $glue:expr) => {
        $crate::treesitter_parse_e2e!($lang, $glue, slow);
    };
    ($lang:expr, $glue:expr, $tier:tt) => {
        $crate::slow_tier_test! { $tier,
            #[test]
            fn treesitter_parse() {
                $crate::run_capi_case(&$lang, &$crate::TREESITTER_PARSE, $glue);
            }
        }
    };
}

/// See [`libsqlite3_c_api_e2e!`]. Runs the zeroperl Perl-5.42 eval case [`ZEROPERL_EVAL`](crate::ZEROPERL_EVAL): drives the embedding C API to evaluate a Perl program and pins its stdout. Slow (a 25 MB reactor artifact reconverted to a ~120 MB / ~1M-line program per run), so gated like the other C-API cases.
#[macro_export]
macro_rules! zeroperl_eval_e2e {
    ($lang:expr, $glue:expr) => {
        $crate::zeroperl_eval_e2e!($lang, $glue, slow);
    };
    ($lang:expr, $glue:expr, $tier:tt) => {
        $crate::slow_tier_test! { $tier,
            #[test]
            fn zeroperl_eval() {
                $crate::run_capi_case(&$lang, &$crate::ZEROPERL_EVAL, $glue);
            }
        }
    };
}

/// See [`libsqlite3_c_api_e2e!`]. Runs the ExifTool-on-zeroperl case [`EXIFTOOL_EXTRACT`](crate::EXIFTOOL_EXTRACT): drives the flattened `exiftool` CLI driver on `cache/zeroperl.wasm` through the embedding C API and pins the extracted EXIF tags. Slow (the same 25 MB reactor reconverted per run), so gated like the other C-API cases.
#[macro_export]
macro_rules! exiftool_extract_e2e {
    ($lang:expr, $glue:expr) => {
        $crate::exiftool_extract_e2e!($lang, $glue, slow);
    };
    ($lang:expr, $glue:expr, $tier:tt) => {
        $crate::slow_tier_test! { $tier,
            #[test]
            fn exiftool_extract() {
                $crate::run_capi_case(&$lang, &$crate::EXIFTOOL_EXTRACT, $glue);
            }
        }
    };
}

/// See [`libsqlite3_c_api_e2e!`]. Runs [`SQLITE3_CALLBACK_BINDING`](crate::SQLITE3_CALLBACK_BINDING).
#[macro_export]
macro_rules! sqlite3_callback_binding_e2e {
    ($lang:expr, $glue:expr) => {
        $crate::sqlite3_callback_binding_e2e!($lang, $glue, slow);
    };
    ($lang:expr, $glue:expr, $tier:tt) => {
        $crate::slow_tier_test! { $tier,
            #[test]
            fn sqlite3_callback_binding() {
                $crate::run_capi_case(&$lang, &$crate::SQLITE3_CALLBACK_BINDING, $glue);
            }
        }
    };
}

/// The DOOM framebuffer-snapshot case (ADR-53): expands to `#[test] fn doom_frame()` driving the converted `doom.wasm` for `$lang` with `$glue` (a named `&str` const in the backend crate providing the ten imports, the self-advancing synthetic clock, and the P6-PPM framebuffer dump), then diffing stdout against `examples/doom/snapshots/frame.ppm`. The tier follows the backend's convention for a comparably heavy execution case: `slow` by default (Ruby/Python/Go/Java, like the qjs/sqlite e2e), passed `ultra` for Bash (its run is minutes, like the bash qjs-REPL pty case, ADR-48). See [`slow_tier_test!`].
#[macro_export]
macro_rules! doom_frame_e2e {
    ($lang:expr, $glue:expr) => {
        $crate::doom_frame_e2e!($lang, $glue, slow);
    };
    ($lang:expr, $glue:expr, $tier:tt) => {
        $crate::slow_tier_test! { $tier,
            #[test]
            fn doom_frame() {
                $crate::run_doom_frame_case(&$lang, $glue);
            }
        }
    };
}

/// Per-case multi-module macros (ADR-27 revision): each expands to one `#[test] fn <case>()` running the named [`MultiModuleCase`] const for `$lang` with `$glue` (a named `&str` driver const in the backend crate). The backend must implement [`BackendUnderTest::compose_modules`]. Which backends invoke these is the capability declaration (ADR-27): the ImportedTables-capable backends for the shared-table case, and the nested-runtime backends for the coexistence case.
///
/// [`MultiModuleCase`]: crate::MultiModuleCase
#[macro_export]
macro_rules! shared_table_e2e {
    ($lang:expr, $glue:expr) => {
        #[test]
        fn shared_table() {
            $crate::run_multi_module_case(&$lang, &$crate::SHARED_TABLE, $glue);
        }
    };
}

/// See [`shared_table_e2e!`]. Runs [`EMBEDDED_COEXIST`](crate::EMBEDDED_COEXIST).
#[macro_export]
macro_rules! embedded_coexist_e2e {
    ($lang:expr, $glue:expr) => {
        #[test]
        fn embedded_coexist() {
            $crate::run_multi_module_case(&$lang, &$crate::EMBEDDED_COEXIST, $glue);
        }
    };
}
