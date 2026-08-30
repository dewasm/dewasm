//! Filesystem-exercising app cases, shared across every backend with WASI filesystem support (all of them) and re-run by wasmtime as the ground-truth engine.
//! Each case converts a cached app to a library-mode class, stages fixtures into a fresh scratch dir preopened into the guest, and runs one or more invocations, diffing stdout against the same `examples/apps/snapshots/` files the always-on `apps` suite uses and asserting the host-side effects the guest was supposed to produce.
//!
//! Each case is a `pub const` [`FsAppCase`] driven by a per-case macro (`qjs_file_io_e2e!`, `sqlite3_shell_dbfile_e2e!`, ...).
//! The per-language instantiation glue is a named const passed to that macro: it writes out the class name, argv, env, and preopen *guest* paths literally and takes only the runtime host paths through the `{scratch}`/`{cache}` placeholders the runner fills. wasmtime does not use the glue: it overrides [`BackendUnderTest::run_app_fs`] to exec the cached binary with `--dir` preopens directly, so the same case consts feed both the backend macros and the wasmtime suite (via [`run_fs_app_case`], called directly).
//!
//! These cases are slow (they reconvert qjs/sqlite and stage ripgrep's 22 MB binary), so the perf opt-out lives at the macro/feature level: each per-case macro (`qjs_file_io_e2e!`, ...) expands its generated `#[test]` as `#[ignore]`d unless the expanding backend crate's `slow_test` feature is enabled.
//! [`run_fs_app_case`] itself just runs the case unconditionally.

use std::path::{Path, PathBuf};
use std::process::Output;

use dewasm_backend::Mode;

use crate::backend::BackendUnderTest;
use crate::fixtures::{apps_cache_dir, apps_fixtures_dir, fresh_scratch_dir};
use crate::glue::fill;

/// One fixture-staging step, run into the case's fresh scratch dir before the guest runs.
/// `src` is relative to [`apps_fixtures_dir`]; `dst` is relative to the scratch dir (`""` = the scratch root).
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

/// One invocation of a filesystem-app case.
/// Multiple runs of a case share the same scratch dir (e.g. sqlite3: create the DB file, then reopen it).
pub struct FsRun {
    /// Full argv, argv0 included (backends bake it into the glue; wasmtime injects argv0 itself and uses `args[1..]`).
    pub args: &'static [&'static str],
    pub stdin: &'static str,
    /// The `include_str!` snapshot this run's stdout must match, or `None` when only the host-side effect is asserted (e.g. the sqlite3 create run).
    pub expect_stdout: Option<&'static str>,
    /// Host-side assertion over the scratch dir after this run (e.g. a file the guest was supposed to write).
    /// `assert_none` when there is nothing to check.
    pub assert_host: fn(&Path),
}

/// The static `env`/`preopens`/`cache_preopens` are used by the wasmtime override (which execs the binary with those host mounts) and to stage/mount the scratch and cache trees; the backends read the same facts out of the glue const, where they are written literally.
pub struct FsAppCase {
    pub name: &'static str,
    /// Cache-binary stem (`examples/apps/cache/<wasm>.wasm`).
    /// Not the module name: `class` is, since the two diverge here (see it).
    pub wasm: &'static str,
    /// The library class name the glue instantiates, *and* the module name every backend is converted under.
    /// Unlike the other suites this is stated rather than derived from `wasm`, because the two deliberately diverge (cache file `ruby.wasm`, but class `Cruby`: a `Ruby` class collides with MRI's predefined constant).
    /// It is a valid module name under every backend's grammar; Bash lowercases it into its prefix.
    pub class: &'static str,
    pub env: &'static [(&'static str, &'static str)],
    /// Guest path -> scratch-relative subdir (`""` = scratch root).
    pub preopens: &'static [(&'static str, &'static str)],
    /// Guest path -> cache-relative subdir, preopened **directly from the app cache** (`examples/apps/cache/<rel>`) rather than copied into scratch.
    /// The language-runtime apps (CPython/CRuby) mount their multi-hundred-MB stdlib trees this way: copying them per run would be prohibitive.
    pub cache_preopens: &'static [(&'static str, &'static str)],
    pub stage: &'static [Stage],
    pub runs: &'static [FsRun],
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

/// QuickJS with file I/O: the `qjs:std` module writes a file into the preopened dir, reads it back, and prints it.
/// Asserts both guest stdout (snapshot) and the host-side file content.
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

/// sqlite3 shell reading/writing a DB *file*: one invocation creates and populates `/db/test.db`, a second reopens it and SELECTs.
/// Both runs share the scratch dir.
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

/// ripgrep searching a small fixture directory tree: recursive directory walking over a preopened tree.
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

/// CPython 3.14.6 executing a one-liner, reading its stdlib from the cache-preopened `cache/cpython-lib/lib` tree at guest `/lib`.
/// The heaviest interpreter case: a ~30 MB wasm.
/// Ground truth (wasmtime): wasmtime --dir cache/cpython-lib/lib::/lib --env PYTHONHOME=/ \ --env PYTHONPATH=/lib/python3.14 cache/cpython.wasm \ -c 'print("hello from cpython", 6 * 7)'
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

/// CRuby 3.4 executing a one-liner (the "Ruby on Ruby" goal demo), reading its stdlib from the cache-preopened `cache/ruby-lib/usr` tree at guest `/usr`.
/// The heaviest case overall: a ~35 MB wasm.
/// Ground truth (wasmtime): wasmtime --dir cache/ruby-lib/usr::/usr cache/ruby.wasm \ -e 'puts "hello from cruby #{6*7}"'
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

/// toywasm (a WebAssembly interpreter written in C) interpreting a second wasm binary: the converted interpreter loads the cached `cowsay.wasm` out of the app cache, preopened at `/apps`, and `--wasi` gives that guest its own WASI.
/// The expected stdout is the [`COWSAY_ARGS`](crate::COWSAY_ARGS) snapshot, so the case asserts that running cowsay *through* the converted interpreter produces the same bytes as running cowsay directly under wasmtime.
/// That indirect ground truth is the only one available: wasmtime answers `fd_fdstat_set_flags(0, NONBLOCK)` with `EBADF`, and toywasm's WASI setup treats the failure as fatal, so wasmtime cannot run the pinned toywasm binary at all.
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

/// wasm3 (a second WebAssembly interpreter written in C, the meta-WASI build of the pinned source) interpreting the cached `cowsay.wasm` out of the app cache, preopened at `/apps`.
/// Same shape as [`TOYWASM_COWSAY`], with two differences: wasm3's CLI takes the guest module directly (no `--wasi` flag; the meta-WASI build always forwards the guest's WASI to the outer host), and the artifact runs under wasmtime, so the `fs_apps` freshness run covers this case against a live engine where toywasm's ground truth is indirect.
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

/// Apply each [`Stage`] step into `scratch`, copying from `fixtures` (the shared [`apps_fixtures_dir`]).
/// Shared by the filesystem-app runner and the
/// C-API runner (the exiftool case stages its image fixture the same way), so fixture staging lives in one place.
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

/// Stage, preopen, convert, and run every [`FsRun`] of `case` under `lang` with its per-language `glue`, returning the fresh scratch dir and each run's [`Output`] (in `case.runs` order).
/// The shared core of [`run_fs_app_case`] (which then asserts stdout + host effects) and [`capture_fs_app_stdout`] (which extracts one run's stdout for the snapshot).
/// Every run must exit zero (fail loud): the multi-run cases depend on earlier runs' host effects (e.g. sqlite3 creates the DB file before the reopen), so a broken run must stop both the compare and the capture.
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
        // Cache-preopened stdlib trees are mounted straight from the app cache (read-only), never copied into scratch (they are hundreds of MB).
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
    // Convert under `class`, not the cache stem: the two diverge for CRuby (see the field).
    // `class` is already a valid module name for every backend, so it is passed verbatim rather than derived. wasmtime ignores the name (it runs the bytes directly).
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
/// The perf opt-out lives at the macro/feature level (see the module docs), so this runner (also called directly by the wasmtime suite, which passes an empty `glue` since its `run_app_fs` override ignores it) never needs its own opt-out.
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

/// Rerun a filesystem app `case` under `lang` (the wasmtime engine) and return the raw stdout of its snapshot-bearing run: the bytes to write into that case's `.stdout` snapshot.
/// Only the cases with a checked-in snapshot file (`QJS_FILE_IO`, `SQLITE3_SHELL_DBFILE`, `RG_SEARCH`) are captured this way; each has exactly one run whose `expect_stdout` is `Some` (the others assert only host-side effects, or pin an inline string with no file).
/// All runs execute in sequence (the earlier ones set up host state the captured run reads), but only that one run's stdout is returned, so it byte-matches the `include_str!` the case compares against.
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
