//! Filesystem-exercising app cases (ADR-27), shared across every backend with WASI filesystem support (Ruby/Python/Go/Java) and re-run by wasmtime as the ground-truth engine. Each case converts a cached app to a library-mode class, stages fixtures into a fresh scratch dir preopened into the guest, and runs one or more invocations, diffing stdout against the same `examples/apps/snapshots/` files the always-on `apps` suite uses (ADR-15) and asserting the host-side effects the guest was supposed to produce.
//!
//! Each case is a `pub const` [`FsAppCase`] driven by a per-case macro (`qjs_file_io_e2e!`, `sqlite3_shell_dbfile_e2e!`, ...). The per-language instantiation glue is a named const passed to that macro: it writes out the class name, argv, env, and preopen *guest* paths literally and takes only the runtime host paths through the `{scratch}`/`{cache}` placeholders the runner fills. wasmtime does not use the glue: it overrides [`BackendUnderTest::run_app_fs`] to exec the cached binary with `--dir` preopens directly, so the same case consts feed both the backend macros and the wasmtime suite (via [`run_fs_app_case`], called directly).
//!
//! These cases are slow (they reconvert qjs/sqlite and stage ripgrep's 22 MB binary), so the perf opt-out lives at the macro/feature level: each per-case macro (`qjs_file_io_e2e!`, ...) expands its generated `#[test]` as `#[ignore]`d unless the expanding backend crate's `slow_test` feature is enabled. [`run_fs_app_case`] itself just runs the case unconditionally.

use std::path::{Path, PathBuf};

use dewasm_backend::Mode;

use crate::backend::BackendUnderTest;
use crate::fixtures::{apps_cache_dir, apps_fixtures_dir, fresh_scratch_dir};
use crate::glue::fill;

/// One fixture-staging step, run into the case's fresh scratch dir before the guest runs. `src` is relative to [`apps_fixtures_dir`]; `dst` is relative to the scratch dir (`""` = the scratch root).
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

/// One invocation of a filesystem-app case. Multiple runs of a case share the same scratch dir (e.g. sqlite3: create the DB file, then reopen it).
pub struct FsRun {
    /// Full argv, argv0 included (backends bake it into the glue; wasmtime injects argv0 itself and uses `args[1..]`).
    pub args: &'static [&'static str],
    pub stdin: &'static str,
    /// The `include_str!` snapshot this run's stdout must match, or `None` when only the host-side effect is asserted (e.g. the sqlite3 create run).
    pub expect_stdout: Option<&'static str>,
    /// Host-side assertion over the scratch dir after this run (e.g. a file the guest was supposed to write). `assert_none` when there is nothing to check.
    pub assert_host: fn(&Path),
}

/// A filesystem-exercising app case: convert `wasm` (cache stem) to library class `class`, stage `stage` into a scratch dir, preopen `preopens`, and run each of `runs`. The static `env`/`preopens`/`cache_preopens` are used by the wasmtime override (which execs the binary with those host mounts) and to stage/mount the scratch and cache trees; the backends read the same facts out of the glue const, where they are written literally.
pub struct FsAppCase {
    pub name: &'static str,
    /// Cache-binary stem (`examples/apps/cache/<wasm>.wasm`), also the conversion module name — every backend PascalCases it to `class`.
    pub wasm: &'static str,
    /// The library class name the glue instantiates (PascalCase of `wasm`).
    pub class: &'static str,
    pub env: &'static [(&'static str, &'static str)],
    /// Guest path -> scratch-relative subdir (`""` = scratch root).
    pub preopens: &'static [(&'static str, &'static str)],
    /// Guest path -> cache-relative subdir, preopened **directly from the app cache** (`examples/apps/cache/<rel>`) rather than copied into scratch. The language-runtime apps (CPython/CRuby) mount their multi-hundred-MB stdlib trees this way — copying them per run would be prohibitive.
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

/// QuickJS with file I/O (Phase 5a #1a): the `qjs:std` module writes a file into the preopened dir, reads it back, and prints it. Asserts both guest stdout (snapshot) and the host-side file content.
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

/// sqlite3 shell reading/writing a DB *file* (Phase 5a #2a): one invocation creates and populates `/db/test.db`, a second reopens it and SELECTs. Both runs share the scratch dir.
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

/// ripgrep searching a small fixture directory tree (Phase 5b): recursive directory walking over a preopened tree. `--sort path` forces a deterministic order so the `wasmtime --dir` snapshot is stable.
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

/// CPython 3.14.6 executing a one-liner (Phase 5b), reading its stdlib from the cache-preopened `cache/cpython-lib/lib` tree at guest `/lib`. The heaviest interpreter case: a ~30 MB wasm. Ground truth (wasmtime): wasmtime --dir cache/cpython-lib/lib::/lib --env PYTHONHOME=/ \ --env PYTHONPATH=/lib/python3.14 cache/cpython.wasm \ -c 'print("hello from cpython", 6 * 7)'
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

/// CRuby 3.4 executing a one-liner (Phase 5b) — the "Ruby on Ruby" north-star demo — reading its stdlib from the cache-preopened `cache/ruby-lib/usr` tree at guest `/usr`. The heaviest case overall: a ~35 MB wasm. Ground truth (wasmtime): wasmtime --dir cache/ruby-lib/usr::/usr cache/ruby.wasm \ -e 'puts "hello from cruby #{6*7}"'
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

/// Apply each [`Stage`] step into `scratch`, copying from `fixtures` (the
/// shared [`apps_fixtures_dir`]). Shared by the filesystem-app runner and the
/// C-API runner (the exiftool case stages its image fixture the same way),
/// so fixture staging lives in one place.
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

/// Run one [`FsAppCase`] for `lang` with its per-language `glue` unconditionally. The perf opt-out lives at the macro/feature level (see the module docs), so this runner — also called directly by the wasmtime suite, which passes an empty `glue` since its `run_app_fs` override ignores it — never needs to gate itself.
pub fn run_fs_app_case(lang: &dyn BackendUnderTest, case: &FsAppCase, glue: &str) {
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
                "{} cache tree {rel} not present — run examples/apps/fetch-and-build.sh (see docs/testing.md)",
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
        "{} not cached — run examples/apps/fetch-and-build.sh (see docs/testing.md)",
        case.wasm
    );
    let bytes = std::fs::read(&wasm_path).expect("read wasm");
    // Convert under `class`, not the cache stem: the stem and the class name diverge for CRuby (cache file `ruby.wasm`, but a `Ruby` class collides with MRI's predefined `Ruby` constant, so the class is `Cruby`). The class name is already PascalCase, and every backend's PascalCasing is idempotent, so this yields exactly `class` for every case. wasmtime ignores the name (it runs the bytes directly).
    let program = lang.convert_app(&bytes, Mode::Library, case.class);
    // Resolve the runtime host paths the glue const left as placeholders. The scratch preopen and the cache root are all any current case needs.
    let filled_glue = fill(
        glue,
        &[
            ("scratch", &scratch.to_string_lossy()),
            ("cache", &cache.to_string_lossy()),
        ],
    );

    for run in case.runs {
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
