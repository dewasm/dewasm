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
//! byte-stdio), and the shared filesystem app cases (`fs_apps_e2e!` over
//! `FS_APP_CASES`, via `app_glue`, gated on `DEWASM_APPS_ALL`) are all wired.

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use dewasm_backend::Backend;
use dewasm_backend_java::{find_java, find_javac, JavaBackend};
use dewasm_test_helper::{
    apps_e2e, capi_apps_e2e, examples_dir, fs_apps_e2e, gzip_e2e, library_e2e, multi_module_e2e,
    qjs_repl_pty_e2e, run_command_bytes, standalone_e2e, wasi_suite, BackendUnderTest, CApiCase,
    LibraryCase, MultiModuleCase, PtyCommand, WasiCase,
};

pub struct Java;

static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// Compile `source` (a single `Main.java`) to a content-addressed class-dir
/// cache (so identical sources compile once) and return its path. `Err(Output)`
/// carries the `javac` failure so a piped run can report it via
/// `status.success()` while a pty run panics on it. A missing `javac` is a loud
/// failure (ADR-15).
fn build_java(source: &str) -> Result<PathBuf, Output> {
    let javac =
        find_javac().expect("javac not found on PATH (or $DEWASM_JAVAC) — see docs/testing.md");

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
            return Err(build);
        }
        let _ = std::fs::rename(&tmp, &classdir);
    }

    Ok(classdir)
}

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
        let java =
            find_java().expect("java not found on PATH (or $DEWASM_JAVA) — see docs/testing.md");
        match build_java(source) {
            // A compile failure is surfaced as the `javac` `Output` so the
            // caller's `status.success()` assertion reports it.
            Err(build) => build,
            Ok(classdir) => run_command_bytes(
                Command::new(&java)
                    .arg("-cp")
                    .arg(&classdir)
                    .arg("Main")
                    .args(args),
                stdin,
            ),
        }
    }

    /// Compile `source` and run `java -cp <classdir> Main <args...>` under a
    /// pty. A compile failure fails loud (ADR-15): there is no `status` for the
    /// caller to inspect on the pty path, so panic with the `javac` output.
    fn pty_command(&self, source: &str, args: &[&str]) -> PtyCommand {
        let java =
            find_java().expect("java not found on PATH (or $DEWASM_JAVA) — see docs/testing.md");
        let classdir = build_java(source).unwrap_or_else(|build| {
            panic!(
                "javac failed for the pty run:\n{}",
                String::from_utf8_lossy(&build.stderr)
            )
        });
        let mut argv = vec![
            "-cp".to_string(),
            classdir.to_string_lossy().into_owned(),
            "Main".to_string(),
        ];
        argv.extend(args.iter().map(|a| a.to_string()));
        PtyCommand {
            program: java,
            args: argv,
            cwd: None,
        }
    }

    /// QuickJS and SQLite now run to completion under Java's full WASI surface
    /// (ADR-30 third milestone) and match the wasmtime goldens, so — like the
    /// other backends — Java runs the heavy `apps` cases (qjs, sqlite3-shell)
    /// by default. Java is compiled, so its cost is bimodal but bounded by the
    /// content-addressed `javac` class-dir cache: measured locally the qjs and
    /// sqlite3-shell standalone cases each pay a one-time ~5-6 s `javac` and
    /// then run warm, well under the ADR-24 5-minute bar. The much heavier
    /// filesystem app cases (qjs/sqlite reconversion, rg's 22 MB wasm) live in
    /// the shared `FS_APP_CASES` table, gated behind `DEWASM_APPS_ALL`.
    fn run_heavy_apps(&self) -> bool {
        true
    }

    /// A `public class Main` instantiating `class` (positional ctor
    /// `new class(imports, args, env, preopens)`), running `_start`, and
    /// swallowing a clean guest `proc_exit` (`Rt.Exit`). Generalizes the
    /// hand-written glue the mirrored fs app tests used. `Map.of` tops out at
    /// 10 key/value pairs, well above the single preopen these cases use.
    fn app_glue(
        &self,
        class: &str,
        args: &[&str],
        env: &[(&str, &str)],
        preopens: &[(&str, &Path)],
    ) -> String {
        let argv = args
            .iter()
            .map(|a| format!("{a:?}"))
            .collect::<Vec<_>>()
            .join(", ");
        let env_expr = if env.is_empty() {
            "null".to_string()
        } else {
            let e = env
                .iter()
                .flat_map(|(k, v)| [format!("{k:?}"), format!("{v:?}")])
                .collect::<Vec<_>>()
                .join(", ");
            format!("java.util.Map.of({e})")
        };
        let pre_pairs = preopens
            .iter()
            .flat_map(|(guest, host)| {
                [
                    format!("{guest:?}"),
                    format!("{:?}", host.to_string_lossy()),
                ]
            })
            .collect::<Vec<_>>()
            .join(", ");
        format!(
            "public class Main {{\n\
             \x20   public static void main(String[] a) throws Exception {{\n\
             \x20       {class} inst = new {class}(null, new String[]{{{argv}}}, {env_expr}, java.util.Map.of({pre_pairs}));\n\
             \x20       try {{\n\
             \x20           ((Rt.Fn) inst.Exports.get(\"_start\")).invoke(new Object[]{{}});\n\
             \x20       }} catch (Rt.Exit e) {{\n\
             \x20       }}\n\
             \x20   }}\n\
             }}\n"
        )
    }

    /// Compose several `.wat` modules for the multi-module cases. Java only
    /// composes against one shared runtime (mirroring the spec harness's
    /// `register` path): generate each module's class with
    /// `generate_program_with_units`, union the referenced runtime units, bundle
    /// them once, and concatenate the runtime classes (`Rt`/`Memory`/`Table`/
    /// `Global`/`WASI`) followed by both module classes into ONE default-package
    /// compilation unit. The driver `public class Main` is appended by the runner.
    /// `shared_runtime=false` is never requested for Java — its only case
    /// (`embedded_runtimes_coexist`) is excluded because Java emits one flat
    /// top-level runtime shared by all modules (ADR-30) — so it is unimplemented.
    fn compose_modules(&self, modules: &[(&str, &str)], shared_runtime: bool) -> String {
        assert!(
            shared_runtime,
            "java only composes shared-runtime modules; embedded coexistence is excluded"
        );
        let mut units = std::collections::BTreeSet::new();
        let mut classes = Vec::new();
        for (wat, name) in modules {
            let bytes = wat::parse_file(examples_dir().join(wat)).expect("parse wat");
            let module = dewasm_core::build_module(&bytes).expect("build IR");
            let (src, u) =
                dewasm_backend_java::generate_program_with_units(&module, name).expect("generate");
            units.extend(u);
            classes.push(src);
        }
        format!(
            "{}\n{}",
            dewasm_backend_java::bundler()
                .bundle(&units, 0)
                .expect("bundle runtime"),
            classes.join("\n")
        )
    }
}

/// Per-case Java glue (a `public class Main` appended after the generated
/// module class). A case wired to run but with no glue panics loudly (ADR-15).
fn java_glue(case: &LibraryCase) -> &'static str {
    match case.name {
        "add" => JAVA_ADD_GLUE,
        "wasi_import_override" => JAVA_OVERRIDE_GLUE,
        "wasi_stdio_capture" => JAVA_STDIO_CAPTURE_GLUE,
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

/// The `wasi_stdio_capture` glue: Java's bundled WASI is built eagerly in the
/// module ctor and holds an `OutputStream` at fd 1, so inject a
/// `ByteArrayOutputStream` into the (package-private, default-package-reachable)
/// `wasi.fds` map after construction — the Java mirror of Ruby's `$stdout`
/// redirect. Run `_start` (swallowing a clean `proc_exit`), then flush the
/// captured bytes to the real stdout.
const JAVA_STDIO_CAPTURE_GLUE: &str = r#"public class Main {
    public static void main(String[] a) throws Exception {
        Prog p = new Prog(null, null, null, null);
        java.io.ByteArrayOutputStream captured = new java.io.ByteArrayOutputStream();
        p.wasi.fds.put(1, captured);
        try {
            ((Rt.Fn) p.Exports.get("_start")).invoke(new Object[]{});
        } catch (Rt.Exit e) {
        }
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
fn java_fs_glue(case: &WasiCase, host: &Path) -> String {
    if case.name == "fs_root_preopen_containment" {
        // Probe the WASI sandbox resolver directly (no guest run): with `/`
        // preopened at host `/`, resolving a relative path off the preopen fd
        // (3) must stay contained (errno WASI_OK == 0).
        return "public class Main {\n\
                \x20   public static void main(String[] a) throws Exception {\n\
                \x20       WASI w = new WASI(null, null, java.util.Map.of(\"/\", \"/\"));\n\
                \x20       WASI.Resolved r = w.resolve_path(3, \"etc\", true);\n\
                \x20       System.out.println(r.errno == 0 ? \"contained\" : \"rejected\");\n\
                \x20   }\n\
                }\n"
        .to_string();
    }
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

// ---------------------------------------------------------------------
// C-API drive glue (sqlite3): malloc/pointer plumbing via Memory. No wasmtime
// golden — the results live in guest memory — so each drive's output is pinned
// in the shared table. Ports the Ruby/Python glues one-for-one.

fn java_capi_glue(case: &CApiCase, scratch: &Path) -> String {
    match case.name {
        "libsqlite3_c_api" => JAVA_LIBSQLITE3_MEM.replace("__CLASS__", case.class),
        "sqlite3_file_c_api" => JAVA_LIBSQLITE3_FILE
            .replace("__CLASS__", case.class)
            .replace("__DB__", &scratch.to_string_lossy()),
        "sqlite3_callback_binding" => JAVA_SQLITE3_CALLBACK.replace("__CLASS__", case.class),
        other => panic!("{other}: no java capi glue"),
    }
}

/// The sqlite3 C API driven in memory: `_initialize`, `sqlite3_malloc` +
/// `Memory` pointer plumbing, open/exec/prepare/step/column/finalize/close.
const JAVA_LIBSQLITE3_MEM: &str = r#"public class Main {
    static final java.nio.charset.Charset UTF_8 = java.nio.charset.StandardCharsets.UTF_8;
    static __CLASS__ inst;

    static int malloc(int n) {
        return (int)(Integer)((Rt.Fn) inst.Exports.get("sqlite3_malloc")).invoke(new Object[]{n});
    }

    static int call(String name, Object... args) {
        return (int)(Integer)((Rt.Fn) inst.Exports.get(name)).invoke(args);
    }

    static int cstr(String s) {
        byte[] u = s.getBytes(UTF_8);
        byte[] c = java.util.Arrays.copyOf(u, u.length + 1); // trailing NUL
        int p = malloc(c.length);
        inst.memory.init(Integer.toUnsignedLong(p), c, 0, c.length);
        return p;
    }

    static String readCstr(int ptr) {
        if (ptr == 0) return null;
        byte[] d = inst.memory.d;
        int end = ptr;
        while (d[end] != 0) end++;
        return new String(inst.memory.read_string(Integer.toUnsignedLong(ptr), end - ptr), UTF_8);
    }

    public static void main(String[] a) throws Exception {
        inst = new __CLASS__(null, null, null, null);
        ((Rt.Fn) inst.Exports.get("_initialize")).invoke(new Object[]{});

        System.out.println("version: " + readCstr(call("sqlite3_libversion")));

        int ppDb = malloc(4);
        int rc = call("sqlite3_open", cstr(":memory:"), ppDb);
        if (rc != 0) throw new RuntimeException("open rc=" + rc);
        int db = inst.memory.i32_load(Integer.toUnsignedLong(ppDb));

        rc = call("sqlite3_exec", db, cstr("create table t(a,b); insert into t values (1,'x'),(2,'y');"), 0, 0, 0);
        if (rc != 0) throw new RuntimeException("exec rc=" + rc + ": " + readCstr(call("sqlite3_errmsg", db)));

        int ppStmt = malloc(4);
        // -1 as masked-unsigned i32: read the SQL to its NUL terminator.
        rc = call("sqlite3_prepare_v2", db, cstr("select a*10, b from t order by a desc"), 0xffffffff, ppStmt, 0);
        if (rc != 0) throw new RuntimeException("prepare rc=" + rc);
        int stmt = inst.memory.i32_load(Integer.toUnsignedLong(ppStmt));

        while (call("sqlite3_step", stmt) == 100) { // SQLITE_ROW
            int n = call("sqlite3_column_count", stmt);
            StringBuilder row = new StringBuilder();
            for (int i = 0; i < n; i++) {
                if (i > 0) row.append("|");
                row.append(readCstr(call("sqlite3_column_text", stmt, i)));
            }
            System.out.println(row.toString());
        }
        call("sqlite3_finalize", stmt);
        call("sqlite3_close", db);
        System.out.println("C-API-OK");
    }
}
"#;

/// The sqlite3 C API against a file preopen: create+insert, close, reopen,
/// select — the file lifecycle through the C API (same ADR-14 fs stack as the
/// shell), leaving a nonzero DB file on the host.
const JAVA_LIBSQLITE3_FILE: &str = r#"public class Main {
    static final java.nio.charset.Charset UTF_8 = java.nio.charset.StandardCharsets.UTF_8;
    static __CLASS__ inst;

    static int malloc(int n) {
        return (int)(Integer)((Rt.Fn) inst.Exports.get("sqlite3_malloc")).invoke(new Object[]{n});
    }

    static int call(String name, Object... args) {
        return (int)(Integer)((Rt.Fn) inst.Exports.get(name)).invoke(args);
    }

    static int cstr(String s) {
        byte[] u = s.getBytes(UTF_8);
        byte[] c = java.util.Arrays.copyOf(u, u.length + 1); // trailing NUL
        int p = malloc(c.length);
        inst.memory.init(Integer.toUnsignedLong(p), c, 0, c.length);
        return p;
    }

    static String readCstr(int ptr) {
        if (ptr == 0) return null;
        byte[] d = inst.memory.d;
        int end = ptr;
        while (d[end] != 0) end++;
        return new String(inst.memory.read_string(Integer.toUnsignedLong(ptr), end - ptr), UTF_8);
    }

    static int openDb(String path) {
        int pp = malloc(4);
        int rc = call("sqlite3_open", cstr(path), pp);
        if (rc != 0) throw new RuntimeException("open rc=" + rc);
        return inst.memory.i32_load(Integer.toUnsignedLong(pp));
    }

    public static void main(String[] a) throws Exception {
        inst = new __CLASS__(null, null, null, java.util.Map.of("/db", "__DB__"));
        ((Rt.Fn) inst.Exports.get("_initialize")).invoke(new Object[]{});

        // create + insert, then close so the file is fully flushed
        int db = openDb("/db/data.db");
        int rc = call("sqlite3_exec", db, cstr("create table t(a,b); insert into t values (1,'x'),(2,'y');"), 0, 0, 0);
        if (rc != 0) throw new RuntimeException("exec rc=" + rc + ": " + readCstr(call("sqlite3_errmsg", db)));
        call("sqlite3_close", db);

        // reopen the same file and read it back
        db = openDb("/db/data.db");
        int ppStmt = malloc(4);
        rc = call("sqlite3_prepare_v2", db, cstr("select a*10, b from t order by a"), 0xffffffff, ppStmt, 0);
        if (rc != 0) throw new RuntimeException("prepare rc=" + rc);
        int stmt = inst.memory.i32_load(Integer.toUnsignedLong(ppStmt));
        while (call("sqlite3_step", stmt) == 100) { // SQLITE_ROW
            int n = call("sqlite3_column_count", stmt);
            StringBuilder row = new StringBuilder();
            for (int i = 0; i < n; i++) {
                if (i > 0) row.append("|");
                row.append(readCstr(call("sqlite3_column_text", stmt, i)));
            }
            System.out.println(row.toString());
        }
        call("sqlite3_finalize", stmt);
        call("sqlite3_close", db);
        System.out.println("FILE-OK");
    }
}
"#;

/// Guest->host callback round trip: the committed `sqlite3-binding.wasm` exports
/// `run_query`, which calls `sqlite3_exec` with a C callback forwarding each row
/// to the *imported* `env.host_row` (a `void(argc, argv_ptr)` — the lambda
/// returns null). The glue provides `host_row` via the ADR-7 import provider and
/// collects the rows.
const JAVA_SQLITE3_CALLBACK: &str = r#"public class Main {
    static final java.nio.charset.Charset UTF_8 = java.nio.charset.StandardCharsets.UTF_8;
    static __CLASS__ inst;
    static final java.util.List<String> rows = new java.util.ArrayList<>();

    static int malloc(int n) {
        return (int)(Integer)((Rt.Fn) inst.Exports.get("sqlite3_malloc")).invoke(new Object[]{n});
    }

    static int call(String name, Object... args) {
        return (int)(Integer)((Rt.Fn) inst.Exports.get(name)).invoke(args);
    }

    static int cstr(String s) {
        byte[] u = s.getBytes(UTF_8);
        byte[] c = java.util.Arrays.copyOf(u, u.length + 1); // trailing NUL
        int p = malloc(c.length);
        inst.memory.init(Integer.toUnsignedLong(p), c, 0, c.length);
        return p;
    }

    static String readCstr(int ptr) {
        if (ptr == 0) return null;
        byte[] d = inst.memory.d;
        int end = ptr;
        while (d[end] != 0) end++;
        return new String(inst.memory.read_string(Integer.toUnsignedLong(ptr), end - ptr), UTF_8);
    }

    public static void main(String[] a) throws Exception {
        Rt.Fn hostRow = args -> {
            int argc = (int)(Integer) args[0];
            int argvPtr = (int)(Integer) args[1];
            StringBuilder row = new StringBuilder();
            for (int i = 0; i < argc; i++) {
                if (i > 0) row.append("|");
                int p = inst.memory.i32_load(Integer.toUnsignedLong(argvPtr) + (long) i * 4);
                row.append(readCstr(p));
            }
            rows.add(row.toString());
            return null; // env.host_row is void
        };
        java.util.Map<String, Object> env = new java.util.HashMap<>();
        env.put("host_row", hostRow);
        java.util.Map<String, java.util.Map<String, Object>> imports = new java.util.HashMap<>();
        imports.put("env", env);
        inst = new __CLASS__(imports, null, null, null);
        ((Rt.Fn) inst.Exports.get("_initialize")).invoke(new Object[]{});

        int ppDb = malloc(4);
        int rc = call("sqlite3_open", cstr(":memory:"), ppDb);
        if (rc != 0) throw new RuntimeException("open rc=" + rc);
        int db = inst.memory.i32_load(Integer.toUnsignedLong(ppDb));

        rc = call("sqlite3_exec", db, cstr("create table t(a,b); insert into t values (1,'x'),(2,'y'),(3,'z');"), 0, 0, 0);
        if (rc != 0) throw new RuntimeException("exec rc=" + rc + ": " + readCstr(call("sqlite3_errmsg", db)));

        // guest -> host: run_query calls back into env.host_row once per row
        rc = call("run_query", db, cstr("select a, b from t where a >= 2 order by a"));
        if (rc != 0) throw new RuntimeException("run_query rc=" + rc);
        call("sqlite3_close", db);

        for (String r : rows) System.out.println("row: " + r);
        System.out.println("CALLBACK-OK");
    }
}
"#;

// ---------------------------------------------------------------------
// Multi-module drive glue.

fn java_multi_module_glue(case: &MultiModuleCase) -> &'static str {
    match case.name {
        "shared_table_call_indirect" => JAVA_SHARED_TABLE_GLUE,
        other => panic!("{other}: no java multi-module glue"),
    }
}

/// Instantiate the table exporter, then the importer with the exporter's
/// `Exports` map as its `"a"` import provider (the exporter's `"tab"` table),
/// and print the `call0` result (call_indirect through the shared table → 42).
const JAVA_SHARED_TABLE_GLUE: &str = r#"public class Main {
    public static void main(String[] a) throws Exception {
        TableExp exp = new TableExp(null, null, null, null);
        java.util.Map<String, java.util.Map<String, Object>> imports = new java.util.HashMap<>();
        imports.put("a", exp.Exports);
        TableImp imp = new TableImp(imports, null, null, null);
        System.out.println((int)(Integer)((Rt.Fn) imp.Exports.get("call0")).invoke(new Object[]{}));
    }
}
"#;

// ---------------------------------------------------------------------
// Suite wiring (ADR-27): each macro invocation declares participation.

standalone_e2e!(Java);
library_e2e!(Java, java_glue);
wasi_suite!(Java, Stdio);
wasi_suite!(Java, ArgsEnv);
wasi_suite!(Java, Poll);
wasi_suite!(Java, Fs, java_fs_glue);
apps_e2e!(Java);
gzip_e2e!(Java);
fs_apps_e2e!(Java);
qjs_repl_pty_e2e!(Java);
capi_apps_e2e!(Java, java_capi_glue);
multi_module_e2e!(Java, java_multi_module_glue);
