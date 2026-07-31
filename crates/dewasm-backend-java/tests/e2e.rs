//! Java end-to-end suites (ADR-27): the shared library / WASI / apps case consts (`dewasm-test-helper`) wired up for the Java backend. Per the ADR-27 revision this file holds ONLY the [`BackendUnderTest`] impl, named glue string constants, and per-case macro invocations; glue is a plain `&str` argument at the callsite, and which macros this file invokes is the capability declaration (with a REASON comment at any non-invocation).
//!
//! Java is a compiled backend, so it overrides `BackendUnderTest::run` (ADR-27's hook) to compile-and-execute: `javac` the generated `Main.java` into a content-addressed class-dir cache (so identical sources compile once), then run `java -cp <dir> Main` (ADR-30). Java covers full WASI preview 1 incl. the filesystem.

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::path::PathBuf;
use std::process::{Command, Output};

use dewasm_backend::Backend;
use dewasm_backend_java::{find_java, find_javac, JavaBackend};
use dewasm_test_helper::{
    cowsay_args_e2e, cowsay_stdin_e2e, doom_frame_e2e, examples_dir, gzip_e2e, library_add_e2e,
    libsqlite3_c_api_e2e, qjs_eval_e2e, qjs_file_io_e2e, qjs_repl_e2e, qjs_repl_pty_e2e,
    rg_search_e2e, run_command_bytes, shared_table_e2e, sqlite3_callback_binding_e2e,
    sqlite3_file_c_api_e2e, sqlite3_shell_dbfile_e2e, sqlite3_shell_e2e, standalone_dir_e2e,
    stdio_capture_e2e, wasi_import_override_e2e, wasi_root_containment_e2e, wasi_suite,
    BackendUnderTest, PtyCommand,
};

pub struct Java;

static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// Compile `source` (a single `Main.java`) to a content-addressed class-dir cache (so identical sources compile once) and return its path. `Err(Output)` carries the `javac` failure so a piped run can report it via `status.success()` while a pty run panics on it. A missing `javac` is a loud failure (ADR-15).
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
        // Compile into a unique temp dir, then rename onto the cache key so concurrent test threads never hand out a half-written class dir.
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

    /// Compile `source` (a single `Main.java`) to a content-addressed class-dir cache and run it with `args`/`stdin`. A missing `javac`/`java` is a loud failure (ADR-15); a compile failure is surfaced as the `javac` `Output` so the caller's `status.success()` assertion reports it.
    fn run_bytes(&self, source: &str, args: &[&str], stdin: &[u8]) -> Output {
        let java =
            find_java().expect("java not found on PATH (or $DEWASM_JAVA) — see docs/testing.md");
        match build_java(source) {
            // A compile failure is surfaced as the `javac` `Output` so the caller's `status.success()` assertion reports it.
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

    /// Compile `source` and run `java -cp <classdir> Main <args...>` under a pty. A compile failure fails loud (ADR-15): there is no `status` for the caller to inspect on the pty path, so panic with the `javac` output.
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

    /// Compose several `.wat` modules for the multi-module cases. Java only composes against one shared runtime (mirroring the spec harness's `register` path): generate each module's class with `generate_program_with_units`, union the referenced runtime units, bundle them once, and concatenate the runtime classes followed by both module classes into ONE default-package compilation unit. The driver `public class Main` is appended by the runner. `shared_runtime=false` is never requested for Java — `embedded_coexist_e2e!` is not invoked because Java emits one flat top-level runtime shared by all modules (ADR-30) — so it is unimplemented.
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

// --------------------------------------------------------------------- Library-case glue (a `public class Main` appended after the generated module class).

/// `add.wat`: call the exported functions and print each result.
const JAVA_ADD_GLUE: &str = r#"public class Main {
    public static void main(String[] a) {
        Add inst = new Add(null, null, null, null);
        System.out.println((int)(Integer)((Rt.Fn) inst.Exports.get("add")).invoke(new Object[]{2, 3}));
        System.out.println((int)(Integer)((Rt.Fn) inst.Exports.get("add")).invoke(new Object[]{0xffffffff, 1}));
        System.out.println((int)(Integer)((Rt.Fn) inst.Exports.get("fib")).invoke(new Object[]{10}));
    }
}
"#;

/// The ADR-7 override/fallback glue: an explicit `fd_write` import wins, `random_get` falls back to the bundled WASI. Mirrors the other backends' override glues — intercept fd_write and print the actual bytes written.
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

/// The `wasi_stdio_capture` glue: Java's bundled WASI is built eagerly in the module ctor and holds an `OutputStream` at fd 1, so inject a `ByteArrayOutputStream` into the (package-private, default-package-reachable) `wasi.fds` map after construction — the Java mirror of Ruby's `$stdout` redirect. Run `_start` (swallowing a clean `proc_exit`), then flush the captured bytes to the real stdout.
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

// --------------------------------------------------------------------- WASI filesystem glue.

/// The shared filesystem template: preopen the scratch dir (`{host}`) at guest `{guest}` (always `/`), run `_start`, and surface a `proc_exit` code (Rt.Exit) as a trailing decimal line. rt/exit is always seeded for library-mode WASI output, so `Rt.Exit` is defined even for fixtures that never import proc_exit.
const JAVA_FS_GLUE: &str = r#"public class Main {
    public static void main(String[] a) throws Exception {
        Prog p = new Prog(null, null, null, java.util.Map.of("{guest}", "{host}"));
        try {
            ((Rt.Fn) p.Exports.get("_start")).invoke(new Object[]{});
        } catch (Rt.Exit e) {
            System.out.println(e.code);
        }
    }
}
"#;

/// The root-preopen containment probe: probe the WASI sandbox resolver directly (no guest run): with `/` preopened at host `/`, resolving a relative path off the preopen fd (3) must stay contained (errno WASI_OK == 0).
const JAVA_CONTAINMENT_GLUE: &str = r#"public class Main {
    public static void main(String[] a) throws Exception {
        WASI w = new WASI(null, null, java.util.Map.of("/", "/"));
        WASI.Resolved r = w.resolve_path(3, "etc", true);
        System.out.println(r.errno == 0 ? "contained" : "rejected");
    }
}
"#;

// --------------------------------------------------------------------- Filesystem app glue: class/argv/env/preopen-guest-paths are literals; only the host scratch dir comes through {scratch}.

const JAVA_QJS_FILE_IO_GLUE: &str = r#"public class Main {
    public static void main(String[] a) throws Exception {
        Qjs inst = new Qjs(null, new String[]{"qjs", "/work/qjs_file_io.js"}, null, java.util.Map.of("/work", "{scratch}"));
        try {
            ((Rt.Fn) inst.Exports.get("_start")).invoke(new Object[]{});
        } catch (Rt.Exit e) {
        }
    }
}
"#;

const JAVA_QJS_REPL_GLUE: &str = r#"public class Main {
    public static void main(String[] a) throws Exception {
        Qjs inst = new Qjs(null, new String[]{"qjs", "/work/qjs_repl.js"}, null, java.util.Map.of("/work", "{scratch}"));
        try {
            ((Rt.Fn) inst.Exports.get("_start")).invoke(new Object[]{});
        } catch (Rt.Exit e) {
        }
    }
}
"#;

const JAVA_SQLITE3_SHELL_GLUE: &str = r#"public class Main {
    public static void main(String[] a) throws Exception {
        Sqlite3Shell inst = new Sqlite3Shell(null, new String[]{"sqlite3"}, null, java.util.Map.of("/db", "{scratch}"));
        try {
            ((Rt.Fn) inst.Exports.get("_start")).invoke(new Object[]{});
        } catch (Rt.Exit e) {
        }
    }
}
"#;

const JAVA_RG_SEARCH_GLUE: &str = r#"public class Main {
    public static void main(String[] a) throws Exception {
        Rg inst = new Rg(null, new String[]{"rg", "--sort", "path", "needle", "/work"}, null, java.util.Map.of("/work", "{scratch}"));
        try {
            ((Rt.Fn) inst.Exports.get("_start")).invoke(new Object[]{});
        } catch (Rt.Exit e) {
        }
    }
}
"#;

// --------------------------------------------------------------------- C-API drive glue (sqlite3): malloc/pointer plumbing via Memory. No wasmtime golden — the results live in guest memory — so each drive's output is pinned in the shared case const. Only the file-backed case uses {scratch}.

/// The sqlite3 C API driven in memory: `_initialize`, `sqlite3_malloc` + `Memory` pointer plumbing, open/exec/prepare/step/column/finalize/close.
const JAVA_LIBSQLITE3_MEM: &str = r#"public class Main {
    static final java.nio.charset.Charset UTF_8 = java.nio.charset.StandardCharsets.UTF_8;
    static Libsqlite3 inst;

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
        inst = new Libsqlite3(null, null, null, null);
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

/// The sqlite3 C API against a file preopen: create+insert, close, reopen, select — the file lifecycle through the C API (same ADR-14 fs stack as the shell), leaving a nonzero DB file on the host.
const JAVA_LIBSQLITE3_FILE: &str = r#"public class Main {
    static final java.nio.charset.Charset UTF_8 = java.nio.charset.StandardCharsets.UTF_8;
    static Libsqlite3 inst;

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
        inst = new Libsqlite3(null, null, null, java.util.Map.of("/db", "{scratch}"));
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

/// Guest->host callback round trip: the committed `sqlite3-binding.wasm` exports `run_query`, which calls `sqlite3_exec` with a C callback forwarding each row to the *imported* `env.host_row` (a `void(argc, argv_ptr)` — the lambda returns null). The glue provides `host_row` via the ADR-7 import provider and collects the rows.
const JAVA_SQLITE3_CALLBACK: &str = r#"public class Main {
    static final java.nio.charset.Charset UTF_8 = java.nio.charset.StandardCharsets.UTF_8;
    static Sqlite3Binding inst;
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
        inst = new Sqlite3Binding(imports, null, null, null);
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

// --------------------------------------------------------------------- Multi-module drive glue.

/// Instantiate the table exporter, then the importer with the exporter's `Exports` map as its `"a"` import provider (the exporter's `"tab"` table), and print the `call0` result (call_indirect through the shared table → 42).
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

// --------------------------------------------------------------------- Suite wiring (ADR-27): each per-case macro invocation declares participation.

library_add_e2e!(Java, JAVA_ADD_GLUE);
wasi_import_override_e2e!(Java, JAVA_OVERRIDE_GLUE);
stdio_capture_e2e!(Java, JAVA_STDIO_CAPTURE_GLUE);
// custom_wasi_provider_e2e! / partial_override_e2e!: not invoked — Java's bundled WASI is eagerly constructed in the ctor and there is no provider-object import form (ADR-30), so the lazy-construction observable cannot hold.

wasi_suite!(Java, Stdio);
wasi_suite!(Java, ArgsEnv);
wasi_suite!(Java, Poll);
wasi_suite!(Java, Fs, JAVA_FS_GLUE);
wasi_root_containment_e2e!(Java, JAVA_CONTAINMENT_GLUE);
standalone_dir_e2e!(Java);

cowsay_args_e2e!(Java);
cowsay_stdin_e2e!(Java);
qjs_eval_e2e!(Java);
sqlite3_shell_e2e!(Java);
gzip_e2e!(Java);

qjs_file_io_e2e!(Java, JAVA_QJS_FILE_IO_GLUE);
qjs_repl_e2e!(Java, JAVA_QJS_REPL_GLUE);
sqlite3_shell_dbfile_e2e!(Java, JAVA_SQLITE3_SHELL_GLUE);
rg_search_e2e!(Java, JAVA_RG_SEARCH_GLUE);
// cpython_hello_e2e!: not invoked — a CPython interpreter method overflows the JVM 64 KB per-method bytecode limit (`code too large`); the ADR-30 class-splitter does not subdivide individual methods (a hard limit; see docs/apps-audit.md). cruby_hello_e2e!: not invoked — the CRuby element-segment `Elem` class overflows the JVM 64 K constant-pool limit (`too many constants`), a hard limit (docs/apps-audit.md).
qjs_repl_pty_e2e!(Java);

libsqlite3_c_api_e2e!(Java, JAVA_LIBSQLITE3_MEM);
sqlite3_file_c_api_e2e!(Java, JAVA_LIBSQLITE3_FILE);
sqlite3_callback_binding_e2e!(Java, JAVA_SQLITE3_CALLBACK);

// DOOM (ADR-53): deterministic drive (synthetic clock, no input) dumping the
// framebuffer as a P6 PPM matching the wasmtime golden. `{ticks}`/`{clock_step}`
// filled by the runner.
const JAVA_DOOM_FRAME_GLUE: &str = r#"public class Main {
    public static void main(String[] a) throws Exception {
        final long[] ms = {0};
        final int[] fw = {0}, fh = {0}, foff = {0};
        java.util.Map<String, java.util.Map<String, Object>> imports = new java.util.HashMap<>();
        java.util.Map<String, Object> console = new java.util.HashMap<>();
        console.put("onErrorMessage", (Rt.Fn)(args -> null));
        console.put("onInfoMessage", (Rt.Fn)(args -> null));
        imports.put("console", console);
        java.util.Map<String, Object> gameSaving = new java.util.HashMap<>();
        gameSaving.put("sizeOfSaveGame", (Rt.Fn)(args -> 0));
        gameSaving.put("readSaveGame", (Rt.Fn)(args -> 0));
        gameSaving.put("writeSaveGame", (Rt.Fn)(args -> args[2]));
        imports.put("gameSaving", gameSaving);
        java.util.Map<String, Object> runtimeControl = new java.util.HashMap<>();
        runtimeControl.put("timeInMilliseconds", (Rt.Fn)(args -> { ms[0] += {clock_step}; return ms[0]; }));
        imports.put("runtimeControl", runtimeControl);
        java.util.Map<String, Object> ui = new java.util.HashMap<>();
        ui.put("drawFrame", (Rt.Fn)(args -> { foff[0] = (int)(Integer) args[0]; return null; }));
        imports.put("ui", ui);
        java.util.Map<String, Object> loading = new java.util.HashMap<>();
        loading.put("onGameInit", (Rt.Fn)(args -> { fw[0] = (int)(Integer) args[0]; fh[0] = (int)(Integer) args[1]; return null; }));
        loading.put("wadSizes", (Rt.Fn)(args -> null));
        loading.put("readWads", (Rt.Fn)(args -> null));
        imports.put("loading", loading);

        Doom doom = new Doom(imports, null, null, null);
        ((Rt.Fn) doom.Exports.get("initGame")).invoke(new Object[]{});
        Rt.Fn tick = (Rt.Fn) doom.Exports.get("tickGame");
        for (int i = 0; i < {ticks}; i++) tick.invoke(new Object[]{});

        int w = fw[0], h = fh[0], off = foff[0];
        byte[] d = doom.memory.d;
        byte[] out = new byte[w * h * 3];
        int j = 0;
        for (int i = 0; i < w * h * 4; i += 4) {
            out[j++] = d[off + i + 2];
            out[j++] = d[off + i + 1];
            out[j++] = d[off + i];
        }
        System.out.write(("P6\n" + w + " " + h + "\n255\n").getBytes(java.nio.charset.StandardCharsets.US_ASCII));
        System.out.write(out);
        System.out.flush();
    }
}
"#;

doom_frame_e2e!(Java, JAVA_DOOM_FRAME_GLUE);

shared_table_e2e!(Java, JAVA_SHARED_TABLE_GLUE);
// embedded_coexist_e2e!: not invoked — a single flat top-level runtime is shared by all modules (ADR-30); two independent runtimes cannot coexist.
