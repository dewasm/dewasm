//! Java end-to-end suites (ADR-27): the shared standalone / library / WASI /
//! apps case tables (`dewasm-test-helper`) wired up for the Java backend.
//!
//! Java is a compiled backend, so it overrides `BackendUnderTest::run` (ADR-27's
//! hook) to compile-and-execute: `javac` the generated `Main.java` into a
//! content-addressed class-dir cache (so identical sources — e.g. cowsay's args
//! and stdin cases — compile once), then run `java -cp <dir> Main`. Measured on
//! cowsay, this beats the `java Main.java` single-file source launcher, which
//! recompiles in memory on every run (~3.3 s each) versus a warm ~0.15 s here
//! after a one-time ~2 s compile (ADR-30).
//!
//! Third-milestone scope (ADR-30): full WASI preview 1 incl. the filesystem.
//! The whole-program `Stdio`/`ArgsEnv`/`Fs` WASI kinds, both library cases,
//! `apps_e2e!` (cowsay/qjs/sqlite3-shell byte-identical), `gzip_e2e!` (binary
//! byte-stdio), and the mirrored filesystem app cases (qjs/sqlite/rg, gated on
//! `DEWASM_APPS_ALL`) are all wired.

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::path::Path;
use std::process::{Command, Output};

use dewasm_backend::{Backend, Mode};
use dewasm_backend_java::{find_java, find_javac, JavaBackend};
use dewasm_test_helper::{
    apps_cache_dir, apps_e2e, apps_fixtures_dir, convert_on_big_stack, fresh_scratch_dir, gzip_e2e,
    library_e2e, run_command_bytes, standalone_e2e, wasi_suite, BackendUnderTest, LibraryCase,
    WasiCase,
};

pub struct Java;

static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

impl BackendUnderTest for Java {
    fn name(&self) -> &'static str {
        "java"
    }

    fn backend(&self) -> &'static (dyn Backend + Sync) {
        &JavaBackend
    }

    /// Compile `source` (a single `Main.java`) to a content-addressed class-dir
    /// cache and run it with `args`/`stdin`. A missing `javac`/`java` is a loud
    /// failure (ADR-15); a compile failure is surfaced as the `javac` `Output`
    /// so the caller's `status.success()` assertion reports it.
    fn run_bytes(&self, source: &str, args: &[&str], stdin: &[u8]) -> Output {
        let javac =
            find_javac().expect("javac not found on PATH (or $DEWASM_JAVAC) — see docs/testing.md");
        let java =
            find_java().expect("java not found on PATH (or $DEWASM_JAVA) — see docs/testing.md");

        let mut hasher = DefaultHasher::new();
        source.hash(&mut hasher);
        let hash = hasher.finish();

        let cache = std::env::temp_dir().join("dewasm-java-cache");
        std::fs::create_dir_all(&cache).unwrap();
        let classdir = cache.join(format!("prog-{hash:016x}"));

        if !classdir.join("Main.class").exists() {
            // Compile into a unique temp dir, then rename onto the cache key so
            // concurrent test threads never hand out a half-written class dir.
            let tmp = cache.join(format!(
                "prog-{hash:016x}.{}.{}",
                std::process::id(),
                COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
            ));
            std::fs::create_dir_all(&tmp).unwrap();
            let src = tmp.join("Main.java");
            std::fs::write(&src, source).unwrap();
            let build = Command::new(&javac)
                .arg("-d")
                .arg(&tmp)
                .arg(&src)
                .output()
                .expect("spawn javac");
            if !build.status.success() {
                return build;
            }
            let _ = std::fs::rename(&tmp, &classdir);
        }

        run_command_bytes(
            Command::new(&java)
                .arg("-cp")
                .arg(&classdir)
                .arg("Main")
                .args(args),
            stdin,
        )
    }

    /// QuickJS and SQLite now run to completion under Java's full WASI surface
    /// (ADR-30 third milestone) and match the wasmtime goldens, so — like the
    /// other backends — Java runs the heavy `apps` cases (qjs, sqlite3-shell)
    /// by default. Java is compiled, so its cost is bimodal but bounded by the
    /// content-addressed `javac` class-dir cache: measured locally the qjs and
    /// sqlite3-shell standalone cases each pay a one-time ~5-6 s `javac` and
    /// then run warm, well under the ADR-24 5-minute bar. The much heavier
    /// filesystem app cases (qjs/sqlite reconversion, rg's 22 MB wasm) stay
    /// behind `DEWASM_APPS_ALL` in the tail of this file.
    fn run_heavy_apps(&self) -> bool {
        true
    }
}

/// Per-case Java glue (a `public class Main` appended after the generated
/// module class). A case wired to run but with no glue panics loudly (ADR-15).
fn java_glue(case: &LibraryCase) -> &'static str {
    match case.name {
        "add" => JAVA_ADD_GLUE,
        "wasi_import_override" => JAVA_OVERRIDE_GLUE,
        other => panic!("{other}: no java glue"),
    }
}

const JAVA_ADD_GLUE: &str = r#"public class Main {
    public static void main(String[] a) {
        Add inst = new Add(null, null, null, null);
        System.out.println((int)(Integer)((Rt.Fn) inst.Exports.get("add")).invoke(new Object[]{2, 3}));
        System.out.println((int)(Integer)((Rt.Fn) inst.Exports.get("add")).invoke(new Object[]{0xffffffff, 1}));
        System.out.println((int)(Integer)((Rt.Fn) inst.Exports.get("fib")).invoke(new Object[]{10}));
    }
}
"#;

/// The ADR-7 override/fallback glue: an explicit `fd_write` import wins,
/// `random_get` falls back to the bundled WASI. Mirrors the other backends'
/// override glues — intercept fd_write and print the actual bytes written.
const JAVA_OVERRIDE_GLUE: &str = r#"public class Main {
    public static void main(String[] a) throws Exception {
        java.io.ByteArrayOutputStream captured = new java.io.ByteArrayOutputStream();
        Prog[] holder = new Prog[1];
        Rt.Fn fdWrite = args -> {
            int iovs = (int)(Integer) args[1];
            int ptr = holder[0].memory.i32_load(Integer.toUnsignedLong(iovs));
            int len = holder[0].memory.i32_load(Integer.toUnsignedLong(iovs) + 4);
            byte[] b = holder[0].memory.read_string(Integer.toUnsignedLong(ptr), Integer.toUnsignedLong(len));
            captured.write(b, 0, b.length);
            holder[0].memory.i32_store(Integer.toUnsignedLong((int)(Integer) args[3]), len);
            return 0;
        };
        java.util.Map<String, Object> wasi = new java.util.HashMap<>();
        wasi.put("fd_write", fdWrite);
        java.util.Map<String, java.util.Map<String, Object>> imports = new java.util.HashMap<>();
        imports.put("wasi_snapshot_preview1", wasi);
        Prog p = new Prog(imports, null, null, null);
        holder[0] = p;
        ((Rt.Fn) p.Exports.get("_start")).invoke(new Object[]{}); // random_get falls back to bundled WASI
        System.out.write(captured.toByteArray());
        System.out.flush();
    }
}
"#;

/// Instantiate an fs fixture with the scratch dir preopened at guest `/`, run
/// `_start`, and surface a `proc_exit` code (Rt.Exit) as a trailing decimal
/// line. One glue serves both stdout-reporting and proc_exit fixtures: the
/// former return from `_start` normally, so nothing extra is printed. rt/exit
/// is always seeded for library-mode WASI output (see lib.rs), so `Rt.Exit` is
/// defined even for fixtures that never import proc_exit. Mirrors
/// `go_fs_glue`/`python_fs_glue`/`ruby_fs_glue`.
fn java_fs_glue(_case: &WasiCase, host: &Path) -> String {
    format!(
        "public class Main {{\n\
         \x20   public static void main(String[] a) throws Exception {{\n\
         \x20       Prog p = new Prog(null, null, null, java.util.Map.of({:?}, {:?}));\n\
         \x20       try {{\n\
         \x20           ((Rt.Fn) p.Exports.get(\"_start\")).invoke(new Object[]{{}});\n\
         \x20       }} catch (Rt.Exit e) {{\n\
         \x20           System.out.println(e.code);\n\
         \x20       }}\n\
         \x20   }}\n\
         }}\n",
        "/",
        host.to_string_lossy()
    )
}

standalone_e2e!(Java);
library_e2e!(Java, java_glue);
wasi_suite!(Java, Stdio);
wasi_suite!(Java, ArgsEnv);
wasi_suite!(Java, Fs, java_fs_glue);
apps_e2e!(Java);
gzip_e2e!(Java);

// ---------------------------------------------------------------------
// Filesystem-exercising app cases, mirrored from the Go/Ruby/Python crates now
// that Java has WASI filesystem support (ADR-30 third milestone, adopting
// ADR-14's model). Their goldens are the same wasmtime-ground-truthed files the
// other backends' cases use (crates/dewasm-cli/tests/apps_golden.rs), so the
// generated Java must produce byte-identical output.
//
// These are the heaviest cases in the suite (qjs/sqlite3 softfloat, rg's 22 MB
// wasm) and Java pays a cold `javac` on top, so they are gated behind
// DEWASM_APPS_ALL — the same deliberate perf opt-out `run_heavy_apps` uses. The
// task's gate runs them via
// `DEWASM_APPS_ALL=1 cargo test -p dewasm-backend-java --test e2e`.

/// Whether the heavy filesystem app cases should actually run.
fn apps_all() -> bool {
    std::env::var("DEWASM_APPS_ALL").is_ok()
}

/// Convert a cached app binary to a library-mode Java file (roomy stack for
/// SQLite's deep functions, `convert_on_big_stack`). `module_name` derives the
/// exported class name (`New<Type>` — here the constructor `new <Type>(...)`).
fn convert_app_library(wasm: &str, module_name: &str) -> String {
    let path = apps_cache_dir().join(format!("{wasm}.wasm"));
    assert!(
        path.exists(),
        "{wasm} not cached — run examples/apps/fetch.sh (see docs/testing.md)"
    );
    let bytes = std::fs::read(&path).expect("read wasm");
    convert_on_big_stack(&JavaBackend, &bytes, Mode::Library, module_name)
}

/// Compile `source` (generated classes + an appended `public class Main` glue)
/// and run it with `stdin`, returning the raw `Output`. Reuses the content-hash
/// class-dir cache in `Java::run_bytes`.
fn run_java_io(source: &str, stdin: &str) -> Output {
    Java.run_bytes(source, &[], stdin.as_bytes())
}

/// A `public class Main` glue instantiating `type_name` with `args` and a
/// single preopen at guest `guest` → host `host`, running `_start` and
/// swallowing a clean guest `proc_exit` (Rt.Exit). Mirrors Go's CATCH_EXIT.
fn app_glue(type_name: &str, args: &[&str], guest: &str, host: &Path) -> String {
    let argv = args
        .iter()
        .map(|a| format!("{a:?}"))
        .collect::<Vec<_>>()
        .join(", ");
    format!(
        "public class Main {{\n\
         \x20   public static void main(String[] a) throws Exception {{\n\
         \x20       {type_name} inst = new {type_name}(null, new String[]{{{argv}}}, null, java.util.Map.of({guest:?}, {host:?}));\n\
         \x20       try {{\n\
         \x20           ((Rt.Fn) inst.Exports.get(\"_start\")).invoke(new Object[]{{}});\n\
         \x20       }} catch (Rt.Exit e) {{\n\
         \x20       }}\n\
         \x20   }}\n\
         }}\n",
        host = host.to_string_lossy()
    )
}

/// QuickJS with file I/O (Go's `qjs_file_io_go`): the `qjs:std` module writes a
/// file into a preopened scratch dir, reads it back, and prints it. Asserts
/// both the guest stdout golden and the host-side file content.
#[test]
fn qjs_file_io_java() {
    if !apps_all() {
        println!("qjs_file_io: heavy case skipped (DEWASM_APPS_ALL=1 to run)");
        return;
    }
    let work = fresh_scratch_dir("java-qjs-file-io");
    std::fs::copy(
        apps_fixtures_dir().join("qjs_file_io.js"),
        work.join("qjs_file_io.js"),
    )
    .unwrap();
    let class = convert_app_library("qjs", "qjs");
    let glue = app_glue("Qjs", &["qjs", "/work/qjs_file_io.js"], "/work", &work);
    let output = run_java_io(&format!("{class}\n{glue}"), "");
    assert!(
        output.status.success(),
        "qjs_file_io under java failed: {}\n{}",
        output.status,
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        include_str!("../../../examples/apps/golden/qjs_file_io.stdout"),
        "qjs_file_io: guest stdout differs from the wasmtime golden"
    );
    assert_eq!(
        std::fs::read_to_string(work.join("io_out.txt")).unwrap(),
        "hello from qjs file io\n",
        "qjs_file_io: the host file the guest wrote is wrong"
    );
}

/// QuickJS scripted REPL over piped stdin (Go's `qjs_repl_go`): the pinned
/// read-eval-print loop fixture exercises the stdin-read + evalScript path
/// byte-identically to wasmtime.
#[test]
fn qjs_repl_java() {
    if !apps_all() {
        println!("qjs_repl: heavy case skipped (DEWASM_APPS_ALL=1 to run)");
        return;
    }
    let work = fresh_scratch_dir("java-qjs-repl");
    std::fs::copy(
        apps_fixtures_dir().join("qjs_repl.js"),
        work.join("qjs_repl.js"),
    )
    .unwrap();
    let class = convert_app_library("qjs", "qjs");
    let glue = app_glue("Qjs", &["qjs", "/work/qjs_repl.js"], "/work", &work);
    let output = run_java_io(
        &format!("{class}\n{glue}"),
        "1+2\n[3,1,2].sort()\nMath.max(4,9)\n\\q\n",
    );
    assert!(
        output.status.success(),
        "qjs_repl under java failed: {}\n{}",
        output.status,
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        include_str!("../../../examples/apps/golden/qjs_repl.stdout"),
        "qjs_repl: guest stdout differs from the wasmtime golden"
    );
}

/// sqlite3 shell reading/writing a DB *file* (Go's `sqlite3_shell_dbfile_go`):
/// one invocation creates and populates `/db/test.db`, a second reopens it and
/// SELECTs. Asserts the second run's stdout (golden) and that the DB file
/// exists on the host with nonzero size.
#[test]
fn sqlite3_shell_dbfile_java() {
    if !apps_all() {
        println!("sqlite3_shell_dbfile: heavy case skipped (DEWASM_APPS_ALL=1 to run)");
        return;
    }
    let db = fresh_scratch_dir("java-sqlite3-dbfile");
    let class = convert_app_library("sqlite3-shell", "sqlite3_shell");
    let glue = app_glue("Sqlite3Shell", &["sqlite3"], "/db", &db);
    let program = format!("{class}\n{glue}");

    let create = run_java_io(
        &program,
        ".open /db/test.db\n\
         CREATE TABLE t(id INTEGER PRIMARY KEY, v TEXT);\n\
         INSERT INTO t(v) VALUES ('alpha'),('beta');\n\
         .exit\n",
    );
    assert!(
        create.status.success(),
        "sqlite3 dbfile create under java failed: {}\n{}",
        create.status,
        String::from_utf8_lossy(&create.stderr)
    );
    let db_file = db.join("test.db");
    assert!(
        db_file.metadata().map(|m| m.len() > 0).unwrap_or(false),
        "sqlite3 dbfile: the first run left no nonzero DB file"
    );

    let select = run_java_io(
        &program,
        ".open /db/test.db\nSELECT id, v FROM t ORDER BY id;\n.exit\n",
    );
    assert!(
        select.status.success(),
        "sqlite3 dbfile select under java failed: {}\n{}",
        select.status,
        String::from_utf8_lossy(&select.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&select.stdout),
        include_str!("../../../examples/apps/golden/sqlite3_shell_dbfile.stdout"),
        "sqlite3_shell_dbfile: reopened SELECT stdout differs from the wasmtime golden"
    );
}

/// Recursively copy `src` into `dst` (both directories), used to stage the
/// committed ripgrep fixture tree into a fresh scratch dir before the run.
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

/// ripgrep searching a small fixture directory tree (Go's `rg_search_go`):
/// recursive directory walking (fd_readdir, path_open, path_filestat_get) over
/// a preopened tree. `--sort path` forces a deterministic order so the
/// `wasmtime --dir` golden is stable.
#[test]
fn rg_search_java() {
    if !apps_all() {
        println!("rg_search: heavy case skipped (DEWASM_APPS_ALL=1 to run)");
        return;
    }
    let work = fresh_scratch_dir("java-rg-search");
    copy_tree(&apps_fixtures_dir().join("rg"), &work);
    let class = convert_app_library("rg", "rg");
    let glue = app_glue(
        "Rg",
        &["rg", "--sort", "path", "needle", "/work"],
        "/work",
        &work,
    );
    let output = run_java_io(&format!("{class}\n{glue}"), "");
    assert!(
        output.status.success(),
        "rg_search under java failed: {}\n{}",
        output.status,
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        include_str!("../../../examples/apps/golden/rg_search.stdout"),
        "rg_search: guest stdout differs from the wasmtime golden"
    );
}
