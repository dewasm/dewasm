//! App cases that use the file system, shared by every backend with WASI file system support.
//! All backends have that support, and Wasmtime re-runs the cases as the ground-truth engine.
//! Each case converts a cached app to a library-mode class.
//! It stages fixtures into a fresh scratch directory preopened into the guest.
//! It then does one or more runs and diffs their `stdout` against snapshots.
//! Those are the `examples/apps/snapshots/` files the always-on `apps` suite uses.
//! It also asserts the host-side effects the guest was supposed to produce.
//!
//! Each case is a `pub const` [`FsAppCase`] driven by a per-case macro.
//! Examples are `qjs_file_io_e2e!` and `sqlite3_shell_dbfile_e2e!`.
//! The per-language instantiation glue is a named constant passed to that macro.
//! It writes out the class name, `argv`, environment, and preopen *guest* paths literally.
//! It takes only the runtime host paths, through the `{scratch}`/`{cache}` placeholders.
//! The runner fills those placeholders.
//! Wasmtime does not use the glue.
//! It overrides [`BackendUnderTest::run_app_fs`] to run the cached binary directly.
//! It passes `--dir` preopens.
//! So the same case constants feed both the backend macros and the Wasmtime suite.
//! The Wasmtime suite calls [`run_fs_app_case`] directly.
//!
//! These cases are slow: they reconvert `qjs`/SQLite and stage `ripgrep`'s 22 MB binary.
//! So the skip for speed lives at the macro/feature level.
//! Each per-case macro (`qjs_file_io_e2e!`, ...) expands its generated `#[test]` as `#[ignore]`d.
//! It does so unless the expanding backend crate's `slow_test` feature is enabled.
//! [`run_fs_app_case`] itself just runs the case unconditionally.

use std::path::{Path, PathBuf};
use std::process::Output;

use dewasm_backend::Mode;

use crate::backend::BackendUnderTest;
use crate::fixtures::{apps_cache_dir, apps_fixtures_dir, fresh_scratch_dir};
use crate::glue::fill;

/// One fixture-staging step, run into the case's fresh scratch directory before the guest runs.
/// `src` is relative to [`apps_fixtures_dir`].
/// `dst` is relative to the scratch directory (`""` = the scratch root).
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

/// One run of a file system app case.
/// Multiple runs of a case share the same scratch directory.
/// For example, sqlite3 creates the DB file, then reopens it.
pub struct FsRun {
    /// Full `argv`, `argv[0]` included.
    /// Backends write it into the glue; Wasmtime injects `argv[0]` itself and uses `args[1..]`.
    pub args: &'static [&'static str],
    pub stdin: &'static str,
    /// The `include_str!` snapshot this run's `stdout` must match.
    /// `None` when only the host-side effect is asserted (for example the sqlite3 create run).
    pub expect_stdout: Option<&'static str>,
    /// Host-side assertion over the scratch directory after this run.
    /// An example is a file the guest was supposed to write.
    /// `assert_none` when there is nothing to check.
    pub assert_host: fn(&Path),
}

/// The static `env`/`preopens`/`cache_preopens` are used by the Wasmtime override.
/// That override runs the binary with those host mounts.
/// They are also used to stage and mount the scratch and cache trees.
/// The backends read the same facts out of the glue constant, where they are written literally.
pub struct FsAppCase {
    pub name: &'static str,
    /// Cache-binary stem (`examples/apps/cache/<wasm>.wasm`).
    /// Not the module name: `class` is, since the two differ here (see it).
    pub wasm: &'static str,
    /// The library class name the glue instantiates.
    /// It is *also* the module name every backend is converted under.
    /// Unlike the other suites this is stated rather than derived from `wasm`.
    /// That is because the two differ on purpose: cache file `ruby.wasm`, but class `Cruby`.
    /// A `Ruby` class collides with the constant MRI already defines.
    /// Every backend's grammar accepts it; Bash puts it in lower case into its prefix.
    pub class: &'static str,
    pub env: &'static [(&'static str, &'static str)],
    /// Guest path -> directory relative to scratch (`""` = scratch root).
    pub preopens: &'static [(&'static str, &'static str)],
    /// Guest path -> directory relative to the cache, preopened **directly from the app cache**.
    /// The cache path is `examples/apps/cache/<rel>`, and nothing is copied into scratch.
    /// The language-runtime apps (CPython/CRuby) mount their standard library trees this way.
    /// Those trees are multiple hundreds of MB.
    /// Copying them per run would cost too much.
    pub cache_preopens: &'static [(&'static str, &'static str)],
    pub stage: &'static [Stage],
    pub runs: &'static [FsRun],
}

/// No host-side effect to assert for this run.
fn assert_none(_: &Path) {}

/// `qjs` file I/O: the guest wrote `io_out.txt` into the preopened directory.
fn assert_qjs_io_out(scratch: &Path) {
    assert_eq!(
        std::fs::read_to_string(scratch.join("io_out.txt")).unwrap(),
        "hello from qjs file io\n",
        "qjs_file_io: the host file the guest wrote is wrong"
    );
}

/// sqlite3 DB file create: the first run must leave a nonzero DB file behind.
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

/// QuickJS with file I/O.
/// The `qjs:std` module writes a file into the preopened directory, reads it back, and prints it.
/// Asserts both guest `stdout` (snapshot) and the host-side file content.
pub const QJS_FILE_IO: FsAppCase = FsAppCase {
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
            "../../../examples/apps/snapshots/qjs_file_io.stdout"
        )),
        assert_host: assert_qjs_io_out,
    }],
};

/// sqlite3 shell reading/writing a DB *file*.
/// One run creates and populates `/db/test.db`, and a second reopens it and SELECTs.
/// Both runs share the scratch directory.
pub const SQLITE3_SHELL_DBFILE: FsAppCase = FsAppCase {
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
                "../../../examples/apps/snapshots/sqlite3_shell_dbfile.stdout"
            )),
            assert_host: assert_none,
        },
    ],
};

/// `ripgrep` searching a small fixture directory tree.
/// It walks the preopened tree recursively.
/// `--sort path` forces a deterministic order so the `wasmtime --dir` snapshot is stable.
pub const RG_SEARCH: FsAppCase = FsAppCase {
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
            "../../../examples/apps/snapshots/rg_search.stdout"
        )),
        assert_host: assert_none,
    }],
};

/// CPython 3.14.6 executing a one-liner.
/// It reads its standard library from guest `/lib`.
/// That is the cache-preopened `cache/cpython-lib/lib` tree.
/// The heaviest interpreter case: a ~30 MB wasm.
/// Ground truth (Wasmtime):
///
/// ```console
/// wasmtime --dir cache/cpython-lib/lib::/lib --env PYTHONHOME=/ \
///   --env PYTHONPATH=/lib/python3.14 cache/cpython.wasm \
///   -c 'print("hello from cpython", 6 * 7)'
/// ```
pub const CPYTHON_HELLO: FsAppCase = FsAppCase {
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
};

/// CRuby 3.4 executing a one-liner (the "Ruby on Ruby" goal example).
/// It reads its standard library from guest `/usr`.
/// That is the cache-preopened `cache/ruby-lib/usr` tree.
/// The heaviest case overall: a ~35 MB wasm.
/// Ground truth (Wasmtime):
///
/// ```console
/// wasmtime --dir cache/ruby-lib/usr::/usr cache/ruby.wasm \
///   -e 'puts "hello from cruby #{6*7}"'
/// ```
pub const CRUBY_HELLO: FsAppCase = FsAppCase {
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
};

/// `toywasm` (a WebAssembly interpreter written in C) interpreting a second wasm binary.
/// The converted interpreter loads the cached `cowsay.wasm` out of the app cache.
/// The cache is preopened at `/apps`.
/// `--wasi` gives that guest its own WASI.
/// The expected `stdout` is the [`COWSAY_ARGS`](crate::COWSAY_ARGS) snapshot.
/// So the case asserts that running `cowsay` *through* the converted interpreter matches.
/// It must produce the same bytes as running `cowsay` directly under Wasmtime.
/// That indirect ground truth is the only one available.
/// Wasmtime answers `fd_fdstat_set_flags(0, NONBLOCK)` with `EBADF`.
/// `toywasm`'s WASI set-up treats the failure as fatal.
/// So Wasmtime cannot run the fixed-version `toywasm` binary at all.
pub const TOYWASM_COWSAY: FsAppCase = FsAppCase {
    name: "toywasm_cowsay",
    wasm: "toywasm",
    class: "Toywasm",
    env: &[],
    preopens: &[],
    cache_preopens: &[("/apps", "")],
    stage: &[],
    runs: &[FsRun {
        args: &[
            "toywasm",
            "--wasi",
            "/apps/cowsay.wasm",
            "Hello",
            "from",
            "dewasm!",
        ],
        stdin: "",
        expect_stdout: Some(include_str!(
            "../../../examples/apps/snapshots/cowsay_args.stdout"
        )),
        assert_host: assert_none,
    }],
};

/// wasm3 interpreting the cached `cowsay.wasm` out of the app cache, preopened at `/apps`.
/// wasm3 is a second WebAssembly interpreter written in C.
/// This is its MetaWASI build, from source at a fixed version.
/// Same shape as [`TOYWASM_COWSAY`], with two differences.
/// First, wasm3's CLI takes the guest module directly, with no `--wasi` flag.
/// That build always forwards the guest's WASI to the outer host.
/// Second, the artifact runs under Wasmtime.
/// So the `fs_apps` freshness run covers this case against a live engine.
/// `toywasm`'s ground truth is indirect instead.
pub const WASM3_COWSAY: FsAppCase = FsAppCase {
    name: "wasm3_cowsay",
    wasm: "wasm3",
    class: "Wasm3",
    env: &[],
    preopens: &[],
    cache_preopens: &[("/apps", "")],
    stage: &[],
    runs: &[FsRun {
        args: &["wasm3", "/apps/cowsay.wasm", "Hello", "from", "dewasm!"],
        stdin: "",
        expect_stdout: Some(include_str!(
            "../../../examples/apps/snapshots/cowsay_args.stdout"
        )),
        assert_host: assert_none,
    }],
};

/// Apply each [`Stage`] step into `scratch`.
/// Copy from `fixtures`, the shared [`apps_fixtures_dir`].
/// The file system app and C-API runners share it, so fixture staging lives in one place.
/// The ExifTool case stages its image fixture the same way.
pub(crate) fn stage_into(fixtures: &Path, scratch: &Path, stage: &[Stage]) {
    for step in stage {
        match step {
            Stage::File { src, dst } => {
                std::fs::copy(fixtures.join(src), scratch.join(dst)).unwrap();
            }
            Stage::Tree { src, dst } => {
                let to = if dst.is_empty() {
                    scratch.to_path_buf()
                } else {
                    scratch.join(dst)
                };
                copy_tree(&fixtures.join(src), &to);
            }
        }
    }
}

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

/// Stage, preopen, convert, and run every [`FsRun`] of `case` under `lang`.
/// Use its per-language `glue`.
/// Return the fresh scratch directory and each run's [`Output`] (in `case.runs` order).
/// It is the shared core of [`run_fs_app_case`] and [`capture_fs_app_stdout`].
/// The first then asserts `stdout` + host effects.
/// The second extracts one run's `stdout` for the snapshot.
/// Every run must exit zero (fail loud).
/// The multi-run cases depend on earlier runs' host effects.
/// For example, sqlite3 creates the DB file before the reopen.
/// So a broken run must stop both the compare and the capture.
fn drive_fs_app_case(
    lang: &dyn BackendUnderTest,
    case: &FsAppCase,
    glue: &str,
) -> (PathBuf, Vec<Output>) {
    let cache = apps_cache_dir();
    let fixtures = apps_fixtures_dir();
    let scratch = fresh_scratch_dir(&format!("{}-{}", lang.name(), case.name));

    stage_into(&fixtures, &scratch, case.stage);

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
        // Cache-preopened standard library trees mount straight from the app cache (read-only).
        // They are never copied into scratch: they are hundreds of MB.
        .chain(case.cache_preopens.iter().map(|(guest, rel)| {
            let host = cache.join(rel);
            assert!(
                host.is_dir(),
                "{} cache tree {rel} not present: run examples/apps/setup.sh (see docs/testing.md)",
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
        "{} not cached: run examples/apps/setup.sh (see docs/testing.md)",
        case.wasm
    );
    let bytes = std::fs::read(&wasm_path).expect("read wasm");
    // Convert under `class`, not the cache stem: the two differ for CRuby (see the field).
    // `class` is already a valid module name for every backend.
    // So it is passed unchanged rather than derived.
    // Wasmtime ignores the name (it runs the bytes directly).
    let program = lang.convert_app(&bytes, Mode::Library, case.class);
    let filled_glue = fill(
        glue,
        &[
            ("scratch", &scratch.to_string_lossy()),
            ("cache", &cache.to_string_lossy()),
        ],
    );

    let outputs = case
        .runs
        .iter()
        .map(|run| {
            let output = lang.run_app_fs(
                &program,
                &filled_glue,
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
            output
        })
        .collect();
    (scratch, outputs)
}

/// Run one [`FsAppCase`] for `lang` with its per-language `glue` unconditionally.
/// The skip for speed lives at the macro/feature level (see the module documentation).
/// So this runner never needs its own skip.
/// The Wasmtime suite also calls it directly.
/// It passes an empty `glue`, since its `run_app_fs` override ignores it.
pub fn run_fs_app_case(lang: &dyn BackendUnderTest, case: &FsAppCase, glue: &str) {
    let (scratch, outputs) = drive_fs_app_case(lang, case, glue);
    for (run, output) in case.runs.iter().zip(&outputs) {
        if let Some(snapshot) = run.expect_stdout {
            assert_eq!(
                String::from_utf8_lossy(&output.stdout),
                snapshot,
                "{} {:?} under {}: stdout differs from the wasmtime snapshot",
                case.name,
                run.args,
                lang.name()
            );
        }
        (run.assert_host)(&scratch);
    }
    println!(
        "{} under {}: matches snapshot output",
        case.name,
        lang.name()
    );
}

/// Rerun a file system app `case` under `lang` (the Wasmtime engine).
/// Return the raw `stdout` of its snapshot-bearing run.
/// Those are the bytes to write into that case's `.stdout` snapshot.
/// Only the cases with a checked-in snapshot file are captured this way.
/// Those are `QJS_FILE_IO`, `SQLITE3_SHELL_DBFILE`, and `RG_SEARCH`.
/// Each has exactly one run whose `expect_stdout` is `Some`.
/// The others assert only host-side effects, or compare with an inline string and no file.
/// All runs execute in sequence: the earlier ones set up host state the captured run reads.
/// But only that one run's `stdout` is returned.
/// So it byte-matches the `include_str!` the case compares against.
pub fn capture_fs_app_stdout(lang: &dyn BackendUnderTest, case: &FsAppCase) -> Vec<u8> {
    let (_scratch, outputs) = drive_fs_app_case(lang, case, "");
    let idx = case
        .runs
        .iter()
        .position(|run| run.expect_stdout.is_some())
        .unwrap_or_else(|| {
            panic!(
                "{}: no run with an expected stdout to capture a snapshot from",
                case.name
            )
        });
    outputs[idx].stdout.clone()
}
