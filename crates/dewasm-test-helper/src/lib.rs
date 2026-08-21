//! Shared test harness, case tables, and per-feature test macros for the dewasm backend crates.
//! This crate depends only on `dewasm-core` and `dewasm-backend` (never on a concrete backend crate) and is taken as a dev-dependency by each backend crate, which supplies a [`BackendUnderTest`] (and, for the spec harness, a [`SpecBackend`]) and wires up the suites it participates in via the macros below.

mod apps;
mod apps_capi;
mod apps_convert;
mod apps_fs;
mod backend;
mod doom;
mod fixtures;
mod folding;
mod glue;
mod library;
mod multimodule;
mod nes;
mod pty;
mod qjs_repl;
mod snapshots;
mod spec;
mod wasi;
mod wasi_testsuite;
mod wasmtime_backend;

pub use apps::{
    run_app_case, run_gzip_cases, run_slow_app_case, AppCase, COWSAY_ARGS, COWSAY_STDIN,
    CRUBY_PACKED_HELLO, MRUBY_EH, QJS_EVAL, SQLITE3_MOD_SHELL, SQLITE3_SHELL,
};
pub use apps_capi::{
    run_capi_case, CApiCase, EXIFTOOL_EXTRACT, LIBSQLITE3_C_API, PCAP_COMPILE,
    SQLITE3_CALLBACK_BINDING, SQLITE3_FILE_C_API, TREESITTER_PARSE, ZEROPERL_EVAL,
};
pub use apps_convert::{apps_convert_main, apps_convert_trials};
pub use apps_fs::{
    run_fs_app_case, FsAppCase, FsRun, Stage, CPYTHON_HELLO, CRUBY_HELLO, QJS_FILE_IO, RG_SEARCH,
    SQLITE3_SHELL_DBFILE, TOYWASM_COWSAY,
};
pub use backend::{
    derive_module_name, module_name_style, run_command, run_command_bytes, run_script,
    run_script_bytes, write_temp_script, BackendUnderTest, ModuleNameStyle,
};
pub use doom::{
    doom_frame_snapshot_path, doom_wasm_path, frame_to_ppm, run_doom_frame_case,
    DOOM_CLOCK_STEP_MS, DOOM_FRAME_H, DOOM_FRAME_W, DOOM_TICKS,
};
pub use fixtures::{
    apps_cache_dir, apps_fixtures_dir, apps_snapshot_dir, convert, convert_bytes,
    convert_on_big_stack, examples_dir, fresh_scratch_dir,
};
pub use folding::run_folded_temp_reuse;
pub use library::{
    run_library_case, LibraryCase, CUSTOM_WASI_PROVIDER, LIBRARY_ADD, PARTIAL_OVERRIDE,
    STDIO_CAPTURE, WASI_IMPORT_OVERRIDE,
};
pub use multimodule::{run_multi_module_case, MultiModuleCase, EMBEDDED_COEXIST, SHARED_TABLE};
pub use nes::{
    alter_ego_rom_path, nes_frame_snapshot_path, nes_frame_to_ppm, nes_wasm_path,
    run_nes_frame_case, NES_FRAMES, NES_FRAME_H, NES_FRAME_W, NES_PALETTE_ENTRIES,
};
pub use pty::{run_under_pty, PtyCommand};
pub use qjs_repl::{
    assert_transcript_eq, capture_qjs_repl_transcript, qjs_repl_snapshot_path, run_qjs_repl_pty,
    QJS_REPL_SESSION,
};
pub use snapshots::{wasmtime_snapshots, WasmtimeSnapshot};
pub use spec::{
    curated_with, heap_type_tag, nullable_heap_type, spec_main, spec_trials, Converted,
    SpecBackend, CURATED_SPEC_FILES, EXCEPTION_HANDLING_SPEC_FILES,
};
pub use wasi::{
    run_deep_recursion, run_standalone_dir, run_wasi_containment, run_wasi_fs, run_wasi_standalone,
    WasiCase, WasiCheck, WasiKind, WASI_CASES,
};
pub use wasi_testsuite::{wasi_testsuite_main, wasi_testsuite_trials, WasiTestsuiteBackend};
pub use wasmtime_backend::{wasi_runner_argv, wasi_runner_bin, Wasmtime};

/// The `harness = false` `main` of a backend's spec integration test: builds one libtest-mimic trial per `.wast` file for `$lang` (a [`SpecBackend`]) and runs them with cargo's own test arguments (name filter, `--ignored`/`--include-ignored`, thread count).
/// `$lang` must be a promotable-to-`'static` value: the backend `Spec` structs are unit structs, so `spec_suite!(RubySpec)` promotes `&RubySpec` to `&'static`.
#[macro_export]
macro_rules! spec_suite {
    ($lang:expr) => {
        fn main() {
            $crate::spec_main(&$lang, cfg!(feature = "slow_test"));
        }
    };
}

/// The `harness = false` `main` of a backend's whole-cache convert integration test: builds one libtest-mimic trial per cached app for `$backend` (a `Backend + Sync` value) and runs them with cargo's own test arguments.
/// Unlike [`spec_suite!`] this takes the plain [`Backend`]: the convert suite only lowers, it never runs generated code, so it needs no interpreter or script-phrasing layer.
/// `$backend` must be a promotable-to-`'static` value; the backend `Backend` structs are unit structs, so `apps_convert_suite!(RubyBackend)` promotes `&RubyBackend` to `&'static`.
/// Heavy trials are `#[ignore]`d unless the expanding crate's `slow_test` feature is on.
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

/// The `harness = false` `main` of a backend's WASI-testsuite integration test: builds one libtest-mimic trial per prebuilt `.wasm` for `$lang` (a [`WasiTestsuiteBackend`]) and runs them with cargo's own test arguments.
/// Like [`spec_suite!`], `$lang` is a promotable-to-`'static` unit struct.
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

/// The two halves of the module-name policy every backend states identically: a library name that does not fit the language's grammar is a conversion-time error naming the language, the offending value and the flag, and a standalone artifact ignores the requested name in favour of its fixed internal one.
/// What differs is only which names a language rejects and what its standalone output is recognised by, so those are the arguments.
///
/// Also expands to `fn convert(name, mode) -> anyhow::Result<String>`, the fixture-conversion helper: an ordinary item in the invoking file, so the per-language tests a backend keeps beside this invocation (Ruby's ancestor guards, Java's dotted names, Go's package layout) call it too.
///
/// `wat` (the fixture module) and the optional `default_wasi` are arguments because those neighbouring tests depend on them: Python needs a memory in the fixture to observe the `<Class>Rt` naming, Go converts with WASI on because it compiles its library artifact for real.
#[macro_export]
macro_rules! module_name_policy_suite {
    (
        backend: $backend:expr,
        wat: $wat:expr,
        default_wasi: $default_wasi:expr,
        invalid: [$($invalid:expr),+ $(,)?],
        error_contains: $error:expr,
        standalone_markers: [$($marker:expr),+ $(,)?] $(,)?
    ) => {
        /// Convert the fixture module under `mode`, with `name` as the requested module name, and return the first output file's source.
        fn convert(name: &str, mode: dewasm_backend::Mode) -> anyhow::Result<String> {
            let bytes = wat::parse_str($wat)?;
            let module = dewasm_core::build_module(&bytes)?;
            let mut files = dewasm_backend::Backend::generate(
                &$backend,
                &module,
                &dewasm_backend::GenOptions {
                    mode,
                    module_name: name.to_string(),
                    runtime: dewasm_backend::RuntimeLinkage::Embedded,
                    default_wasi: $default_wasi,
                    data_file: None,
                },
            )?;
            Ok(String::from_utf8(files.remove(0).contents)?)
        }

        #[test]
        fn invalid_library_names_are_rejected() {
            for name in [$($invalid),+] {
                let err = convert(name, dewasm_backend::Mode::Library)
                    .expect_err("an invalid library module name must be a conversion error");
                let msg = format!("{err:#}");
                assert!(
                    msg.contains($error)
                        && msg.contains(&format!("{name:?}"))
                        && msg.contains("--module-name"),
                    "the error must name the grammar, the offending value and the flag, got: {msg}"
                );
            }
        }

        /// Standalone output is a self-contained program: the requested name never reaches the source.
        #[test]
        fn standalone_name_is_fixed() {
            let source =
                convert("whatever-the-stem-was", dewasm_backend::Mode::Standalone).expect("convert");
            $(assert!(
                source.contains($marker),
                "standalone output does not carry {:?}:\n{source}",
                $marker
            );)+
            assert!(!source.contains("whatever"));
        }
    };
    (
        backend: $backend:expr,
        wat: $wat:expr,
        invalid: [$($invalid:expr),+ $(,)?],
        error_contains: $error:expr,
        standalone_markers: [$($marker:expr),+ $(,)?] $(,)?
    ) => {
        $crate::module_name_policy_suite!(
            backend: $backend,
            wat: $wat,
            default_wasi: false,
            invalid: [$($invalid),+],
            error_contains: $error,
            standalone_markers: [$($marker),+]
        );
    };
}

/// Internal: wrap a generated `#[test]` item in the speed-category `#[ignore]` attribute.
/// The per-case app macros below delegate here so a callsite can pick the category without duplicating the cfg_attr.
/// `#[macro_export]` is load-bearing despite the macro being internal: the delegating macros expand inside the backend crates, where `$crate::test_speed!` resolves only to an exported macro (a plain `macro_rules!` cannot even be `pub use`d across crates, E0364).
/// `#[doc(hidden)]` keeps it out of the public docs instead.
///
/// * `slow`: conditional on the backend crate's `slow_test` feature (CI's main run category).
/// This is the default for every slow-case macro.
/// * `ultra`: conditional on `ultra_slow_test` (which implies `slow_test`), for a case measured at roughly a minute or more locally.
/// These are kept out of CI and run only under `--features ultra_slow_test`, in local pre-release verification.
#[doc(hidden)]
#[macro_export]
macro_rules! test_speed {
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
            ignore = "ultra-slow app case: --features ultra_slow_test"
        )]
        $item
    };
}

/// Per-case library macros: each expands to one `#[test] fn <case>()` running the named [`LibraryCase`] const for `$lang` with `$glue` (a named `&str` const in the backend crate).
/// A backend declares participation by invoking the macro and drops it (with a REASON comment) for a capability it lacks.
#[macro_export]
macro_rules! library_add_e2e {
    ($lang:expr, $glue:expr) => {
        #[test]
        fn library_add() {
            $crate::run_library_case(&$lang, &$crate::LIBRARY_ADD, $glue);
        }
    };
}

/// See [`library_add_e2e!`].
/// Runs [`WASI_IMPORT_OVERRIDE`](crate::WASI_IMPORT_OVERRIDE).
#[macro_export]
macro_rules! wasi_import_override_e2e {
    ($lang:expr, $glue:expr) => {
        #[test]
        fn wasi_import_override() {
            $crate::run_library_case(&$lang, &$crate::WASI_IMPORT_OVERRIDE, $glue);
        }
    };
}

/// See [`library_add_e2e!`].
/// Runs [`CUSTOM_WASI_PROVIDER`](crate::CUSTOM_WASI_PROVIDER).
#[macro_export]
macro_rules! custom_wasi_provider_e2e {
    ($lang:expr, $glue:expr) => {
        #[test]
        fn custom_wasi_provider() {
            $crate::run_library_case(&$lang, &$crate::CUSTOM_WASI_PROVIDER, $glue);
        }
    };
}

/// See [`library_add_e2e!`].
/// Runs [`PARTIAL_OVERRIDE`](crate::PARTIAL_OVERRIDE).
#[macro_export]
macro_rules! partial_override_e2e {
    ($lang:expr, $glue:expr) => {
        #[test]
        fn partial_override() {
            $crate::run_library_case(&$lang, &$crate::PARTIAL_OVERRIDE, $glue);
        }
    };
}

/// See [`library_add_e2e!`].
/// Runs [`STDIO_CAPTURE`](crate::STDIO_CAPTURE).
#[macro_export]
macro_rules! stdio_capture_e2e {
    ($lang:expr, $glue:expr) => {
        #[test]
        fn stdio_capture() {
            $crate::run_library_case(&$lang, &$crate::STDIO_CAPTURE, $glue);
        }
    };
}

/// One `#[test]` running the WASI cases of a given feature kind for `$lang`.
/// The no-glue form covers whole-program standalone kinds (`Stdio`, `ArgsEnv`, `ClockRandom`, `Poll`); the `Fs` form covers the filesystem cases (library-mode runs against a preopened host directory), taking a single per-backend glue **template** const whose `{guest}`/`{host}` placeholders the runner fills with each case's preopen pair.
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

/// One `#[test]` exercising the standalone `--dir` interface for `$lang`: convert `wasi_standalone_dir.wat` standalone, run it with a `--dir` mount, and require the file round-trip to succeed.
/// No glue: standalone needs none.
/// Wired by every filesystem backend, and re-run under wasmtime as ground truth.
#[macro_export]
macro_rules! standalone_dir_e2e {
    ($lang:expr) => {
        #[test]
        fn standalone_dir() {
            $crate::run_standalone_dir(&$lang);
        }
    };
}

/// One `#[test]` requiring the standalone entrypoint to survive deep-but-valid guest recursion for `$lang`: convert `deep_recursion.wat` (5000-frame recursion) standalone, run it, and require the guest's `proc_exit(42)` as the exit code (see [`run_deep_recursion`](crate::run_deep_recursion)).
/// No glue: this exercises the emitted entrypoint itself.
/// Wired by all six backends; each callsite notes whether its entrypoint needed a mitigation for this depth (Python's big-stack thread, issue #31; Java's equivalent, issue #137) or survives unmitigated (Ruby's host stack, Go's growable goroutine stacks, Bash's `ulimit -s` line, Perl's heap-allocated recursion).
#[macro_export]
macro_rules! deep_recursion_e2e {
    ($lang:expr) => {
        #[test]
        fn deep_recursion() {
            $crate::run_deep_recursion(&$lang);
        }
    };
}

/// One `#[test]` requiring `$lang`'s generated code to keep folded operands alive across temp-slot reuse: convert `folded_temp_reuse.wat` standalone, run it, and require the guest's `proc_exit(42)` (see [`run_folded_temp_reuse`](crate::run_folded_temp_reuse)).
/// No glue: the fixture checks its own arithmetic.
/// Core folding is language-independent, so every backend wires this.
#[macro_export]
macro_rules! folded_temp_reuse_e2e {
    ($lang:expr) => {
        #[test]
        fn folded_temp_reuse() {
            $crate::run_folded_temp_reuse(&$lang);
        }
    };
}

/// One `#[test]` running the root-preopen containment probe for `$lang` with `$glue` (a named `&str` const that drives the WASI resolver directly).
/// Split out of `wasi_suite!(Fs)` because it does not fit the shared preopen-and-run template (see [`run_wasi_containment`](crate::run_wasi_containment)).
#[macro_export]
macro_rules! wasi_root_containment_e2e {
    ($lang:expr, $glue:expr) => {
        #[test]
        fn wasi_root_containment() {
            $crate::run_wasi_containment(&$lang, $glue);
        }
    };
}

/// Per-case app macros: each expands to one `#[test] fn <case>()` running the named [`AppCase`] const for `$lang` (a [`BackendUnderTest`]).
/// No glue argument: these are standalone-mode stdin/args cases, so no host-language glue is needed.
/// `cowsay_args_e2e!` and `cowsay_stdin_e2e!` always run; `qjs_eval_e2e!` and `sqlite3_shell_e2e!` are slow: their generated `#[test]` is `#[ignore]`d unless the expanding backend crate's `slow_test` feature is enabled (run with `--features slow_test`).
/// A callsite may pass a trailing speed token (`slow`, the default, or `ultra`); see [`test_speed!`].
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

/// See [`cowsay_args_e2e!`].
/// Runs [`COWSAY_STDIN`](crate::COWSAY_STDIN).
#[macro_export]
macro_rules! cowsay_stdin_e2e {
    ($lang:expr) => {
        #[test]
        fn cowsay_stdin() {
            $crate::run_app_case(&$lang, &$crate::COWSAY_STDIN);
        }
    };
}

/// See [`cowsay_args_e2e!`].
/// Runs the slow [`QJS_EVAL`](crate::QJS_EVAL) case.
/// Slow: the generated `#[test]` is `#[ignore]`d unless the expanding backend crate's `slow_test` feature is enabled (run it with `--features slow_test`).
/// Pass a trailing `ultra` to promote it to the ultra-slow category ([`test_speed!`]).
#[macro_export]
macro_rules! qjs_eval_e2e {
    ($lang:expr) => {
        $crate::qjs_eval_e2e!($lang, slow);
    };
    ($lang:expr, $speed:tt) => {
        $crate::test_speed! { $speed,
            #[test]
            fn qjs_eval() {
                $crate::run_slow_app_case(&$lang, &$crate::QJS_EVAL);
            }
        }
    };
}

/// See [`cowsay_args_e2e!`].
/// Runs the slow [`SQLITE3_SHELL`](crate::SQLITE3_SHELL) case.
/// Slow: see [`qjs_eval_e2e!`] for the `#[ignore]`/`slow_test` feature test and the trailing speed token.
#[macro_export]
macro_rules! sqlite3_shell_e2e {
    ($lang:expr) => {
        $crate::sqlite3_shell_e2e!($lang, slow);
    };
    ($lang:expr, $speed:tt) => {
        $crate::test_speed! { $speed,
            #[test]
            fn sqlite3_shell() {
                $crate::run_slow_app_case(&$lang, &$crate::SQLITE3_SHELL);
            }
        }
    };
}

/// See [`cowsay_args_e2e!`].
/// Runs the slow [`SQLITE3_MOD_SHELL`](crate::SQLITE3_MOD_SHELL) case: the opcode-split shell against the stock shell's snapshot, so a patch or a build-flag change that alters behavior fails here.
/// Slow: see [`qjs_eval_e2e!`] for the `#[ignore]`/`slow_test` feature test and the trailing speed token.
#[macro_export]
macro_rules! sqlite3_mod_shell_e2e {
    ($lang:expr) => {
        $crate::sqlite3_mod_shell_e2e!($lang, slow);
    };
    ($lang:expr, $speed:tt) => {
        $crate::test_speed! { $speed,
            #[test]
            fn sqlite3_mod_shell() {
                $crate::run_slow_app_case(&$lang, &$crate::SQLITE3_MOD_SHELL);
            }
        }
    };
}

/// See [`cowsay_args_e2e!`].
/// Runs [`MRUBY_EH`](crate::MRUBY_EH): raise/rescue/ensure/retry through the converted mruby interpreter, the execution proof for the exception-handling lowering beyond the spec harness.
/// Fast by default (the module is 735 kB and the program is tiny); a backend whose measured run crosses the slow line passes a trailing `slow`/`ultra` speed token, pinned at the callsite like every other case.
#[macro_export]
macro_rules! mruby_eh_e2e {
    ($lang:expr) => {
        #[test]
        fn mruby_eh() {
            $crate::run_app_case(&$lang, &$crate::MRUBY_EH);
        }
    };
    ($lang:expr, $speed:tt) => {
        $crate::test_speed! { $speed,
            #[test]
            fn mruby_eh() {
                $crate::run_app_case(&$lang, &$crate::MRUBY_EH);
            }
        }
    };
}

/// See [`cowsay_args_e2e!`].
/// Runs the slow [`CRUBY_PACKED_HELLO`](crate::CRUBY_PACKED_HELLO) case: the wasi-vfs-packed CRuby, a plain no-preopen app case unlike [`cruby_hello_e2e!`]'s filesystem case.
/// Slow: see [`qjs_eval_e2e!`] for the `#[ignore]`/`slow_test` feature test and the trailing speed token.
#[macro_export]
macro_rules! cruby_packed_hello_e2e {
    ($lang:expr) => {
        $crate::cruby_packed_hello_e2e!($lang, slow);
    };
    ($lang:expr, $speed:tt) => {
        $crate::test_speed! { $speed,
            #[test]
            fn cruby_packed_hello() {
                $crate::run_slow_app_case(&$lang, &$crate::CRUBY_PACKED_HELLO);
            }
        }
    };
}

/// One `#[test]` running the gzip byte-stdio stress cases (minigzip) for `$lang`.
/// Separate from the app macros above because those cases carry binary stdin/stdout an `&str`/`include_str!` `AppCase` cannot represent (`run_gzip_cases`).
#[macro_export]
macro_rules! gzip_e2e {
    ($lang:expr) => {
        #[test]
        fn gzip() {
            $crate::run_gzip_cases(&$lang);
        }
    };
}

/// One `#[test]` driving the bare QuickJS interactive REPL under a real pty for `$lang` and comparing the transcript byte-for-byte to the wasmtime snapshot.
/// Slow: see [`qjs_eval_e2e!`] for the `#[ignore]`/`slow_test` feature test and the trailing speed token.
#[macro_export]
macro_rules! qjs_repl_pty_e2e {
    ($lang:expr) => {
        $crate::qjs_repl_pty_e2e!($lang, slow);
    };
    ($lang:expr, $speed:tt) => {
        $crate::test_speed! { $speed,
            #[test]
            fn qjs_repl_pty() {
                $crate::run_qjs_repl_pty(&$lang);
            }
        }
    };
}

/// Per-case filesystem-app macros: each expands to one `#[test] fn <case>()` running the named [`FsAppCase`] const for `$lang` with `$glue` (a named `&str` const in the backend crate whose `{scratch}`/`{cache}` placeholders the runner fills).
/// A backend declares participation by invoking the macro and drops it (with a REASON comment) for a case it cannot run.
/// Slow: the generated `#[test]` is `#[ignore]`d unless the expanding backend crate's `slow_test` feature is enabled (see [`qjs_eval_e2e!`]); a trailing speed token after `$glue` promotes a case to the ultra-slow category ([`test_speed!`]).
///
/// [`FsAppCase`]: crate::FsAppCase
#[macro_export]
macro_rules! qjs_file_io_e2e {
    ($lang:expr, $glue:expr) => {
        $crate::qjs_file_io_e2e!($lang, $glue, slow);
    };
    ($lang:expr, $glue:expr, $speed:tt) => {
        $crate::test_speed! { $speed,
            #[test]
            fn qjs_file_io() {
                $crate::run_fs_app_case(&$lang, &$crate::QJS_FILE_IO, $glue);
            }
        }
    };
}

/// See [`qjs_file_io_e2e!`].
/// Runs [`SQLITE3_SHELL_DBFILE`](crate::SQLITE3_SHELL_DBFILE).
#[macro_export]
macro_rules! sqlite3_shell_dbfile_e2e {
    ($lang:expr, $glue:expr) => {
        $crate::sqlite3_shell_dbfile_e2e!($lang, $glue, slow);
    };
    ($lang:expr, $glue:expr, $speed:tt) => {
        $crate::test_speed! { $speed,
            #[test]
            fn sqlite3_shell_dbfile() {
                $crate::run_fs_app_case(&$lang, &$crate::SQLITE3_SHELL_DBFILE, $glue);
            }
        }
    };
}

/// See [`qjs_file_io_e2e!`].
/// Runs [`RG_SEARCH`](crate::RG_SEARCH).
#[macro_export]
macro_rules! rg_search_e2e {
    ($lang:expr, $glue:expr) => {
        $crate::rg_search_e2e!($lang, $glue, slow);
    };
    ($lang:expr, $glue:expr, $speed:tt) => {
        $crate::test_speed! { $speed,
            #[test]
            fn rg_search() {
                $crate::run_fs_app_case(&$lang, &$crate::RG_SEARCH, $glue);
            }
        }
    };
}

/// See [`qjs_file_io_e2e!`].
/// Runs [`CPYTHON_HELLO`](crate::CPYTHON_HELLO).
#[macro_export]
macro_rules! cpython_hello_e2e {
    ($lang:expr, $glue:expr) => {
        $crate::cpython_hello_e2e!($lang, $glue, slow);
    };
    ($lang:expr, $glue:expr, $speed:tt) => {
        $crate::test_speed! { $speed,
            #[test]
            fn cpython_hello() {
                $crate::run_fs_app_case(&$lang, &$crate::CPYTHON_HELLO, $glue);
            }
        }
    };
}

/// See [`qjs_file_io_e2e!`].
/// Runs [`CRUBY_HELLO`](crate::CRUBY_HELLO).
#[macro_export]
macro_rules! cruby_hello_e2e {
    ($lang:expr, $glue:expr) => {
        $crate::cruby_hello_e2e!($lang, $glue, slow);
    };
    ($lang:expr, $glue:expr, $speed:tt) => {
        $crate::test_speed! { $speed,
            #[test]
            fn cruby_hello() {
                $crate::run_fs_app_case(&$lang, &$crate::CRUBY_HELLO, $glue);
            }
        }
    };
}

/// See [`qjs_file_io_e2e!`].
/// Runs [`TOYWASM_COWSAY`](crate::TOYWASM_COWSAY): a wasm interpreter, itself converted out of wasm, interpreting a second cached wasm binary.
#[macro_export]
macro_rules! toywasm_cowsay_e2e {
    ($lang:expr, $glue:expr) => {
        $crate::toywasm_cowsay_e2e!($lang, $glue, slow);
    };
    ($lang:expr, $glue:expr, $speed:tt) => {
        $crate::test_speed! { $speed,
            #[test]
            fn toywasm_cowsay() {
                $crate::run_fs_app_case(&$lang, &$crate::TOYWASM_COWSAY, $glue);
            }
        }
    };
}

/// Per-case C-API macros: each expands to one `#[test] fn <case>()` running the named [`CApiCase`] const for `$lang` with `$glue` (a named `&str` const in the backend crate; the file-backed case's `{scratch}` placeholder is filled by the runner).
/// Which backends invoke these is the capability declaration; every backend does (issue #138).
/// Slow: the generated `#[test]` is `#[ignore]`d unless the expanding backend crate's `slow_test` feature is enabled (see [`qjs_eval_e2e!`]).
///
/// [`CApiCase`]: crate::CApiCase
#[macro_export]
macro_rules! libsqlite3_c_api_e2e {
    ($lang:expr, $glue:expr) => {
        $crate::libsqlite3_c_api_e2e!($lang, $glue, slow);
    };
    ($lang:expr, $glue:expr, $speed:tt) => {
        $crate::test_speed! { $speed,
            #[test]
            fn libsqlite3_c_api() {
                $crate::run_capi_case(&$lang, &$crate::LIBSQLITE3_C_API, $glue);
            }
        }
    };
}

/// See [`libsqlite3_c_api_e2e!`].
/// Runs [`SQLITE3_FILE_C_API`](crate::SQLITE3_FILE_C_API).
#[macro_export]
macro_rules! sqlite3_file_c_api_e2e {
    ($lang:expr, $glue:expr) => {
        $crate::sqlite3_file_c_api_e2e!($lang, $glue, slow);
    };
    ($lang:expr, $glue:expr, $speed:tt) => {
        $crate::test_speed! { $speed,
            #[test]
            fn sqlite3_file_c_api() {
                $crate::run_capi_case(&$lang, &$crate::SQLITE3_FILE_C_API, $glue);
            }
        }
    };
}

/// See [`libsqlite3_c_api_e2e!`].
/// Runs the libpcap BPF-compile case [`PCAP_COMPILE`](crate::PCAP_COMPILE): drives `compile_filter` on "tcp port 80" and prints the serialized BPF program.
/// Slow (a ~2 MB reactor artifact reconverted per run), so conditional like the sqlite C-API cases.
#[macro_export]
macro_rules! pcap_compile_e2e {
    ($lang:expr, $glue:expr) => {
        $crate::pcap_compile_e2e!($lang, $glue, slow);
    };
    ($lang:expr, $glue:expr, $speed:tt) => {
        $crate::test_speed! { $speed,
            #[test]
            fn pcap_compile() {
                $crate::run_capi_case(&$lang, &$crate::PCAP_COMPILE, $glue);
            }
        }
    };
}

/// See [`libsqlite3_c_api_e2e!`].
/// Runs the tree-sitter JSON-parse case [`TREESITTER_PARSE`](crate::TREESITTER_PARSE): drives `parse_source` on a fixed JSON snippet and prints the parse tree's S-expression.
/// Slow (a ~1.5 MB reactor artifact reconverted per run), so conditional like the sqlite C-API cases.
#[macro_export]
macro_rules! treesitter_parse_e2e {
    ($lang:expr, $glue:expr) => {
        $crate::treesitter_parse_e2e!($lang, $glue, slow);
    };
    ($lang:expr, $glue:expr, $speed:tt) => {
        $crate::test_speed! { $speed,
            #[test]
            fn treesitter_parse() {
                $crate::run_capi_case(&$lang, &$crate::TREESITTER_PARSE, $glue);
            }
        }
    };
}

/// See [`libsqlite3_c_api_e2e!`].
/// Runs the zeroperl Perl-5.42 eval case [`ZEROPERL_EVAL`](crate::ZEROPERL_EVAL): drives the embedding C API to evaluate a Perl program and pins its stdout.
/// Slow (a 25 MB reactor artifact reconverted to a ~120 MB / ~1M-line program per run), so conditional like the other C-API cases.
#[macro_export]
macro_rules! zeroperl_eval_e2e {
    ($lang:expr, $glue:expr) => {
        $crate::zeroperl_eval_e2e!($lang, $glue, slow);
    };
    ($lang:expr, $glue:expr, $speed:tt) => {
        $crate::test_speed! { $speed,
            #[test]
            fn zeroperl_eval() {
                $crate::run_capi_case(&$lang, &$crate::ZEROPERL_EVAL, $glue);
            }
        }
    };
}

/// See [`libsqlite3_c_api_e2e!`].
/// Runs the ExifTool-on-zeroperl case [`EXIFTOOL_EXTRACT`](crate::EXIFTOOL_EXTRACT): drives the flattened `exiftool` CLI driver on `cache/zeroperl.wasm` through the embedding C API and pins the extracted EXIF tags.
/// Slow (the same 25 MB reactor reconverted per run), so conditional like the other C-API cases.
#[macro_export]
macro_rules! exiftool_extract_e2e {
    ($lang:expr, $glue:expr) => {
        $crate::exiftool_extract_e2e!($lang, $glue, slow);
    };
    ($lang:expr, $glue:expr, $speed:tt) => {
        $crate::test_speed! { $speed,
            #[test]
            fn exiftool_extract() {
                $crate::run_capi_case(&$lang, &$crate::EXIFTOOL_EXTRACT, $glue);
            }
        }
    };
}

/// See [`libsqlite3_c_api_e2e!`].
/// Runs [`SQLITE3_CALLBACK_BINDING`](crate::SQLITE3_CALLBACK_BINDING).
#[macro_export]
macro_rules! sqlite3_callback_binding_e2e {
    ($lang:expr, $glue:expr) => {
        $crate::sqlite3_callback_binding_e2e!($lang, $glue, slow);
    };
    ($lang:expr, $glue:expr, $speed:tt) => {
        $crate::test_speed! { $speed,
            #[test]
            fn sqlite3_callback_binding() {
                $crate::run_capi_case(&$lang, &$crate::SQLITE3_CALLBACK_BINDING, $glue);
            }
        }
    };
}

/// The DOOM framebuffer-snapshot case: expands to `#[test] fn doom_frame()` driving the converted `doom.wasm` for `$lang` with `$glue` (a named `&str` const in the backend crate providing the ten imports, the self-advancing synthetic clock, and the P6-PPM framebuffer dump), then diffing stdout against `examples/apps/snapshots/doom_frame.ppm`.
/// The speed follows the backend's convention for a comparably heavy execution case: `slow` by default (every backend but Bash, like the qjs/sqlite e2e), passed `ultra` for Bash (its run is minutes, like the bash qjs-REPL pty case).
/// See [`test_speed!`].
#[macro_export]
macro_rules! doom_frame_e2e {
    ($lang:expr, $glue:expr) => {
        $crate::doom_frame_e2e!($lang, $glue, slow);
    };
    ($lang:expr, $glue:expr, $speed:tt) => {
        $crate::test_speed! { $speed,
            #[test]
            fn doom_frame() {
                $crate::run_doom_frame_case(&$lang, $glue);
            }
        }
    };
}

/// The NES framebuffer-snapshot case (issue #114, mirroring [`doom_frame_e2e!`]): expands to `#[test] fn nes_frame()` driving the converted `nes.wasm` for `$lang` with `$glue` (a named `&str` const in the backend crate, or, where the host language cannot open a host file from *library-mode* glue without an import the generated module doesn't itself pull in (Go), a function computing an equivalent `String` at test time) that loads the pinned ROM, ticks the deterministic no-input contract, and dumps the frame as a P6 PPM, then diffing stdout against `examples/apps/snapshots/nes_frame.ppm`.
/// Speed assignment mirrors [`doom_frame_e2e!`]: `slow` by default, passed `ultra` for Bash.
#[macro_export]
macro_rules! nes_frame_e2e {
    ($lang:expr, $glue:expr) => {
        $crate::nes_frame_e2e!($lang, $glue, slow);
    };
    ($lang:expr, $glue:expr, $speed:tt) => {
        $crate::test_speed! { $speed,
            #[test]
            fn nes_frame() {
                $crate::run_nes_frame_case(&$lang, $glue);
            }
        }
    };
}

/// Per-case multi-module macros: each expands to one `#[test] fn <case>()` running the named [`MultiModuleCase`] const for `$lang` with `$glue` (a named `&str` driver const in the backend crate).
/// The backend must implement [`BackendUnderTest::compose_modules`].
/// Which backends invoke these is the capability declaration: the ImportedTables-capable backends for the shared-table case, and the nested-runtime backends for the coexistence case.
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

/// See [`shared_table_e2e!`].
/// Runs [`EMBEDDED_COEXIST`](crate::EMBEDDED_COEXIST).
#[macro_export]
macro_rules! embedded_coexist_e2e {
    ($lang:expr, $glue:expr) => {
        #[test]
        fn embedded_coexist() {
            $crate::run_multi_module_case(&$lang, &$crate::EMBEDDED_COEXIST, $glue);
        }
    };
}
