//! Filesystem-exercising app cases (ADR-27), shared across every backend with
//! WASI filesystem support (Ruby/Python/Go/Java) and re-run by wasmtime as the
//! ground-truth engine. Each case converts a cached app to a library-mode
//! class, stages fixtures into a fresh scratch dir preopened into the guest,
//! and runs one or more invocations, diffing stdout against the same
//! `examples/apps/golden/` files the always-on `apps` suite uses (ADR-15) and
//! asserting the host-side effects the guest was supposed to produce.
//!
//! Backends supply the per-language instantiation glue via
//! [`BackendUnderTest::app_glue`]; wasmtime overrides
//! [`BackendUnderTest::run_app_fs`] to exec the cached binary with `--dir`
//! preopens directly. The whole table is gated behind `DEWASM_APPS_ALL` — the
//! same deliberate perf opt-out the heavy `apps` cases use (these reconvert
//! qjs/sqlite and stage ripgrep's 22 MB binary), a perf gate rather than a
//! missing-environment skip. `run_fs_app_cases_forced` runs it unconditionally
//! (used by the wasmtime suite, whose `wasmtime_test` feature is the opt-in).

use std::path::{Path, PathBuf};

use dewasm_backend::Mode;

use crate::backend::BackendUnderTest;
use crate::fixtures::{apps_cache_dir, apps_fixtures_dir, fresh_scratch_dir};

/// One fixture-staging step, run into the case's fresh scratch dir before the
/// guest runs. `src` is relative to [`apps_fixtures_dir`]; `dst` is relative to
/// the scratch dir (`""` = the scratch root).
pub enum Stage {
    /// Copy a single file `src` -> `dst`.
    File {
        src: &'static str,
        dst: &'static str,
    },
    /// Copy a directory tree `src` -> `dst` (contents merged into `dst`).
    Tree {
        src: &'static str,
        dst: &'static str,
    },
}

/// One invocation of a filesystem-app case. Multiple runs of a case share the
/// same scratch dir (e.g. sqlite3: create the DB file, then reopen it).
pub struct FsRun {
    /// Full argv, argv0 included (backends pass it to the ctor; wasmtime
    /// injects argv0 itself and uses `args[1..]`).
    pub args: &'static [&'static str],
    pub stdin: &'static str,
    /// The `include_str!` golden this run's stdout must match, or `None` when
    /// only the host-side effect is asserted (e.g. the sqlite3 create run).
    pub expect_stdout: Option<&'static str>,
    /// Host-side assertion over the scratch dir after this run (e.g. a file
    /// the guest was supposed to write). `|_| {}` when there is nothing to
    /// check.
    pub assert_host: fn(&Path),
}

/// A filesystem-exercising app case: convert `wasm` (cache stem) to library
/// class `class`, stage `stage` into a scratch dir, preopen `preopens`, and
/// run each of `runs`.
pub struct FsAppCase {
    pub name: &'static str,
    /// Cache-binary stem (`examples/apps/cache/<wasm>.wasm`), also the
    /// conversion module name — every backend PascalCases it to `class`.
    pub wasm: &'static str,
    /// The library class name the glue instantiates (PascalCase of `wasm`).
    pub class: &'static str,
    pub env: &'static [(&'static str, &'static str)],
    /// Guest path -> scratch-relative subdir (`""` = scratch root).
    pub preopens: &'static [(&'static str, &'static str)],
    /// Guest path -> cache-relative subdir, preopened **directly from the app
    /// cache** (`examples/apps/cache/<rel>`) rather than copied into scratch.
    /// The language-runtime apps (CPython/CRuby) mount their multi-hundred-MB
    /// stdlib trees this way — copying them per run would be prohibitive.
    pub cache_preopens: &'static [(&'static str, &'static str)],
    pub stage: &'static [Stage],
    pub runs: &'static [FsRun],
    /// Backends excluded from this case, each `(lang name, human reason)`
    /// (ADR-27 revision): a data-level capability/practicality exclusion the
    /// runner prints and honors, instead of a bespoke per-backend test. Used
    /// where a backend hits a hard limit (e.g. a 30 MB wasm whose generated
    /// method overflows the JVM's 64 KB bytecode limit) or exceeds the ADR-24
    /// practicality bar. Never excludes `wasmtime` (the ground-truth engine).
    pub exclude: &'static [(&'static str, &'static str)],
}

/// No host-side effect to assert for this run.
fn assert_none(_: &Path) {}

/// qjs file I/O: the guest wrote `io_out.txt` into the preopened dir.
fn assert_qjs_io_out(scratch: &Path) {
    assert_eq!(
        std::fs::read_to_string(scratch.join("io_out.txt")).unwrap(),
        "hello from qjs file io\n",
        "qjs_file_io: the host file the guest wrote is wrong"
    );
}

/// sqlite3 dbfile create: the first run must leave a nonzero DB file behind.
fn assert_sqlite_dbfile(scratch: &Path) {
    assert!(
        scratch
            .join("test.db")
            .metadata()
            .map(|m| m.len() > 0)
            .unwrap_or(false),
        "sqlite3 dbfile: the first run left no nonzero DB file"
    );
}

pub const FS_APP_CASES: &[FsAppCase] = &[
    // QuickJS with file I/O (Phase 5a #1a): the `qjs:std` module writes a file
    // into the preopened dir, reads it back, and prints it. Asserts both guest
    // stdout (golden) and the host-side file content.
    FsAppCase {
        name: "qjs_file_io",
        wasm: "qjs",
        class: "Qjs",
        env: &[],
        preopens: &[("/work", "")],
        cache_preopens: &[],
        stage: &[Stage::File {
            src: "qjs_file_io.js",
            dst: "qjs_file_io.js",
        }],
        runs: &[FsRun {
            args: &["qjs", "/work/qjs_file_io.js"],
            stdin: "",
            expect_stdout: Some(include_str!(
                "../../../examples/apps/golden/qjs_file_io.stdout"
            )),
            assert_host: assert_qjs_io_out,
        }],
        exclude: &[],
    },
    // QuickJS scripted REPL over piped stdin (Phase 5a #1b): the pinned
    // read-eval-print loop fixture exercises the stdin-read + evalScript path.
    FsAppCase {
        name: "qjs_repl",
        wasm: "qjs",
        class: "Qjs",
        env: &[],
        preopens: &[("/work", "")],
        cache_preopens: &[],
        stage: &[Stage::File {
            src: "qjs_repl.js",
            dst: "qjs_repl.js",
        }],
        runs: &[FsRun {
            args: &["qjs", "/work/qjs_repl.js"],
            stdin: "1+2\n[3,1,2].sort()\nMath.max(4,9)\n\\q\n",
            expect_stdout: Some(include_str!(
                "../../../examples/apps/golden/qjs_repl.stdout"
            )),
            assert_host: assert_none,
        }],
        exclude: &[],
    },
    // sqlite3 shell reading/writing a DB *file* (Phase 5a #2a): one invocation
    // creates and populates `/db/test.db`, a second reopens it and SELECTs.
    // Both runs share the scratch dir.
    FsAppCase {
        name: "sqlite3_shell_dbfile",
        wasm: "sqlite3-shell",
        class: "Sqlite3Shell",
        env: &[],
        preopens: &[("/db", "")],
        cache_preopens: &[],
        stage: &[],
        runs: &[
            FsRun {
                args: &["sqlite3"],
                stdin: ".open /db/test.db\n\
                        CREATE TABLE t(id INTEGER PRIMARY KEY, v TEXT);\n\
                        INSERT INTO t(v) VALUES ('alpha'),('beta');\n\
                        .exit\n",
                expect_stdout: None,
                assert_host: assert_sqlite_dbfile,
            },
            FsRun {
                args: &["sqlite3"],
                stdin: ".open /db/test.db\nSELECT id, v FROM t ORDER BY id;\n.exit\n",
                expect_stdout: Some(include_str!(
                    "../../../examples/apps/golden/sqlite3_shell_dbfile.stdout"
                )),
                assert_host: assert_none,
            },
        ],
        exclude: &[],
    },
    // ripgrep searching a small fixture directory tree (Phase 5b): recursive
    // directory walking over a preopened tree. `--sort path` forces a
    // deterministic order so the `wasmtime --dir` golden is stable.
    FsAppCase {
        name: "rg_search",
        wasm: "rg",
        class: "Rg",
        env: &[],
        preopens: &[("/work", "")],
        cache_preopens: &[],
        stage: &[Stage::Tree { src: "rg", dst: "" }],
        runs: &[FsRun {
            args: &["rg", "--sort", "path", "needle", "/work"],
            stdin: "",
            expect_stdout: Some(include_str!(
                "../../../examples/apps/golden/rg_search.stdout"
            )),
            assert_host: assert_none,
        }],
        exclude: &[],
    },
    // CPython 3.14.6 executing a one-liner (Phase 5b), reading its stdlib from
    // the cache-preopened `cache/cpython-lib/lib` tree at guest `/lib`. The
    // heaviest interpreter case: a ~30 MB wasm. Ground truth (wasmtime):
    //   wasmtime --dir cache/cpython-lib/lib::/lib --env PYTHONHOME=/ \
    //     --env PYTHONPATH=/lib/python3.14 cache/cpython.wasm \
    //     -c 'print("hello from cpython", 6 * 7)'
    // Java is excluded: a single generated method of the CPython interpreter
    // overflows the JVM's 64 KB per-method bytecode limit (`code too large`),
    // which the class-splitter (ADR-30) partitions classes but not oversized
    // individual methods against — a hard limit, not a perf ceiling.
    FsAppCase {
        name: "cpython_hello",
        wasm: "cpython",
        class: "Cpython",
        env: &[("PYTHONHOME", "/"), ("PYTHONPATH", "/lib/python3.14")],
        preopens: &[],
        cache_preopens: &[("/lib", "cpython-lib/lib")],
        stage: &[],
        runs: &[FsRun {
            args: &["python", "-c", "print('hello from cpython', 6 * 7)"],
            stdin: "",
            expect_stdout: Some("hello from cpython 42\n"),
            assert_host: assert_none,
        }],
        exclude: &[(
            "java",
            "a CPython interpreter method overflows the JVM 64 KB per-method \
             bytecode limit (`code too large`); the ADR-30 class-splitter does \
             not subdivide individual methods — a hard limit",
        )],
    },
    // CRuby 3.4 executing a one-liner (Phase 5b) — the "Ruby on Ruby"
    // north-star demo — reading its stdlib from the cache-preopened
    // `cache/ruby-lib/usr` tree at guest `/usr`. The heaviest case overall: a
    // ~35 MB wasm. Ground truth (wasmtime):
    //   wasmtime --dir cache/ruby-lib/usr::/usr cache/ruby.wasm \
    //     -e 'puts "hello from cruby #{6*7}"'
    // Excluded on Go (the ~35 MB wasm's ~242 MB Go source exceeds the ADR-24
    // ~5-minute practicality bar under `go build` — measured >6 min) and on
    // Java (the element-segment `Elem` class overflows the JVM 64 K
    // constant-pool limit, `too many constants` — a hard limit the ADR-30
    // class-splitter does not subdivide further). CPython, the smaller binary,
    // clears Go's bar but hits Java's per-method limit; CRuby hits both.
    FsAppCase {
        name: "cruby_hello",
        wasm: "ruby",
        class: "Cruby",
        env: &[],
        preopens: &[],
        cache_preopens: &[("/usr", "ruby-lib/usr")],
        stage: &[],
        runs: &[FsRun {
            args: &["ruby", "-e", "puts \"hello from cruby #{6*7}\""],
            stdin: "",
            expect_stdout: Some("hello from cruby 42\n"),
            assert_host: assert_none,
        }],
        exclude: &[
            (
                "go",
                "the ~35 MB CRuby wasm's ~242 MB Go source exceeds the ADR-24 \
                 ~5-minute practicality bar under `go build` (measured >6 min)",
            ),
            (
                "java",
                "the CRuby element-segment `Elem` class overflows the JVM 64 K \
                 constant-pool limit (`too many constants`); a hard limit",
            ),
        ],
    },
];

/// Recursively copy the contents of `src` into `dst`, creating `dst`.
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

/// Run every [`FS_APP_CASES`] entry for `lang`, gated behind `DEWASM_APPS_ALL`
/// (uniform perf opt-out; prints a skip note and returns when it is unset,
/// mirroring the heavy `apps` cases). See [`run_fs_app_cases_forced`] for the
/// ungated entry point.
pub fn run_fs_app_cases(lang: &dyn BackendUnderTest) {
    if std::env::var("DEWASM_APPS_ALL").is_err() {
        println!(
            "fs app cases skipped for {} (DEWASM_APPS_ALL=1 to run)",
            lang.name()
        );
        return;
    }
    run_fs_app_cases_forced(lang);
}

/// Run every [`FS_APP_CASES`] entry for `lang` unconditionally (ignoring the
/// `DEWASM_APPS_ALL` gate). Used by the wasmtime suite, whose `wasmtime_test`
/// feature is already the opt-in — requiring both flags would be unergonomic.
pub fn run_fs_app_cases_forced(lang: &dyn BackendUnderTest) {
    let cache = apps_cache_dir();
    let fixtures = apps_fixtures_dir();
    for case in FS_APP_CASES {
        if let Some((_, reason)) = case.exclude.iter().find(|(l, _)| *l == lang.name()) {
            println!("{} excluded for {}: {reason}", case.name, lang.name());
            continue;
        }
        let scratch = fresh_scratch_dir(&format!("{}-{}", lang.name(), case.name));

        for stage in case.stage {
            match stage {
                Stage::File { src, dst } => {
                    std::fs::copy(fixtures.join(src), scratch.join(dst)).unwrap();
                }
                Stage::Tree { src, dst } => {
                    let to = if dst.is_empty() {
                        scratch.clone()
                    } else {
                        scratch.join(dst)
                    };
                    copy_tree(&fixtures.join(src), &to);
                }
            }
        }

        let preopen_paths: Vec<(&str, PathBuf)> = case
            .preopens
            .iter()
            .map(|(guest, rel)| {
                let host = if rel.is_empty() {
                    scratch.clone()
                } else {
                    scratch.join(rel)
                };
                (*guest, host)
            })
            // Cache-preopened stdlib trees are mounted straight from the app
            // cache (read-only), never copied into scratch (they are hundreds
            // of MB).
            .chain(case.cache_preopens.iter().map(|(guest, rel)| {
                let host = cache.join(rel);
                assert!(
                    host.is_dir(),
                    "{} cache tree {rel} not present — run examples/apps/fetch.sh (see docs/testing.md)",
                    case.name
                );
                (*guest, host)
            }))
            .collect();
        let preopens: Vec<(&str, &Path)> = preopen_paths
            .iter()
            .map(|(guest, host)| (*guest, host.as_path()))
            .collect();

        let wasm_path = cache.join(format!("{}.wasm", case.wasm));
        assert!(
            wasm_path.exists(),
            "{} not cached — run examples/apps/fetch.sh (see docs/testing.md)",
            case.wasm
        );
        let bytes = std::fs::read(&wasm_path).expect("read wasm");
        // Convert under `class`, not the cache stem: the stem and the class name
        // diverge for CRuby (cache file `ruby.wasm`, but a `Ruby` class collides
        // with MRI's predefined `Ruby` constant, so the class is `Cruby`). The
        // class name is already PascalCase, and every backend's PascalCasing is
        // idempotent, so this yields exactly `class` for every case. wasmtime
        // ignores the name (it runs the bytes directly).
        let program = lang.convert_app(&bytes, Mode::Library, case.class);

        for run in case.runs {
            let output = lang.run_app_fs(
                &program,
                case.class,
                run.args,
                case.env,
                run.stdin.as_bytes(),
                &preopens,
            );
            assert!(
                output.status.success(),
                "{} {:?} under {}: nonzero exit {}\n{}",
                case.name,
                run.args,
                lang.name(),
                output.status,
                String::from_utf8_lossy(&output.stderr)
            );
            if let Some(golden) = run.expect_stdout {
                assert_eq!(
                    String::from_utf8_lossy(&output.stdout),
                    golden,
                    "{} {:?} under {}: stdout differs from the wasmtime golden",
                    case.name,
                    run.args,
                    lang.name()
                );
            }
            (run.assert_host)(&scratch);
        }
        println!("{} under {}: matches golden output", case.name, lang.name());
    }
}
