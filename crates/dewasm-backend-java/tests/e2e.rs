//! Java end-to-end suites: the shared library / WASI / apps case consts (`dewasm-test-helper`) wired up for the Java backend.
//! This file holds ONLY the [`BackendUnderTest`] impl, named glue string constants, and per-case macro invocations; glue is a plain `&str` argument at the callsite, and which macros this file invokes is the capability declaration (with a REASON comment at any non-invocation).
//!
//! Java is a compiled backend, so it overrides `BackendUnderTest::run` to compile-and-execute: `javac` the generated `Main.java` into a content-addressed class-dir cache (so identical sources compile once), then run `java -cp <dir> Main`.
//! Java covers full WASI preview 1 incl. the filesystem.

use std::path::Path;
use std::process::{Command, Output};

use dewasm_backend::Backend;
use dewasm_backend_java::{find_java, javac_command, JavaBackend};
use dewasm_test_helper::BackendUnderTest;

mod common;

use common::build_java;

pub struct Java;

impl BackendUnderTest for Java {
    fn name(&self) -> &'static str {
        "java"
    }

    fn backend(&self) -> &'static (dyn Backend + Sync) {
        &JavaBackend
    }

    /// Compile `source` (a single `Main.java`) to a content-addressed class-dir cache and run it with `args`/`stdin`.
    /// A missing `javac`/`java` is a loud failure; a compile failure is surfaced as the `javac` `Output` so the caller's `status.success()` assertion reports it.
    fn run_bytes(&self, source: &str, args: &[&str], stdin: &[u8]) -> Output {
        let java =
            find_java().expect("java not found on PATH (or $DEWASM_JAVA): see docs/testing.md");
        match build_java(source) {
            Err(build) => build,
            Ok(classdir) => dewasm_test_helper::run_command_bytes(
                Command::new(&java)
                    .arg("-cp")
                    .arg(&classdir)
                    .arg("Main")
                    .args(args),
                stdin,
            ),
        }
    }

    /// Compile `source` and run `java -cp <classdir> Main <args...>` under a pty.
    /// A compile failure fails loud: there is no `status` for the caller to inspect on the pty path, so panic with the `javac` output.
    fn pty_command(&self, source: &str, args: &[&str]) -> dewasm_test_helper::PtyCommand {
        let java =
            find_java().expect("java not found on PATH (or $DEWASM_JAVA): see docs/testing.md");
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
        dewasm_test_helper::PtyCommand {
            program: java,
            args: argv,
            cwd: None,
        }
    }

    /// Write each `.wat` module of a multi-module case into `dir` as its own default-package `.java` file.
    /// Java has no load statement, so the preamble is empty: [`Self::run_in_dir`] hands every file in the directory to one `javac` invocation, which is exactly how an embedder drops several converted artifacts into a project.
    /// `shared_runtime` mirrors the spec harness's `register` path: each module class comes from `generate_program_with_units` and the union of the units they reference is bundled once into `Rt.java` as top-level classes, the shape `Alias` linkage has.
    /// Otherwise each file is a self-contained library conversion whose runtime classes are `static` members of its own module class, so `Alpha.Rt.Trap` and `Beta.Rt.Trap` are different types.
    fn compose_modules(
        &self,
        dir: &Path,
        modules: &[(&str, &str)],
        shared_runtime: bool,
    ) -> String {
        if shared_runtime {
            let mut units = std::collections::BTreeSet::new();
            for (wat, name) in modules {
                let bytes = wat::parse_file(dewasm_test_helper::examples_dir().join(wat))
                    .expect("parse wat");
                let module = dewasm_core::build_module(&bytes).expect("build IR");
                let (src, u) = dewasm_backend_java::generate_program_with_units(&module, name)
                    .expect("generate");
                units.extend(u);
                std::fs::write(dir.join(format!("{name}.java")), src).unwrap();
            }
            // One file holding the whole runtime: `javac` only ties a file name to a *public* class, and these are all package-private.
            std::fs::write(
                dir.join("Rt.java"),
                dewasm_backend_java::bundler()
                    .bundle(&units, 0)
                    .expect("bundle runtime"),
            )
            .unwrap();
        } else {
            for (wat, name) in modules {
                std::fs::write(
                    dir.join(format!("{name}.java")),
                    dewasm_test_helper::convert(
                        &JavaBackend,
                        &dewasm_test_helper::examples_dir().join(wat),
                        dewasm_backend::Mode::Library,
                        name,
                    ),
                )
                .unwrap();
            }
        }
        String::new()
    }

    /// Compile every `.java` file in `dir` (the module files `compose_modules` wrote plus the driver, written here as `Main.java`) in one `javac` invocation, then run `Main` from that directory.
    /// No content-addressed cache: each case gets a fresh directory, and there are only a handful of small files.
    fn run_in_dir(&self, dir: &Path, driver: &str) -> Output {
        let java =
            find_java().expect("java not found on PATH (or $DEWASM_JAVA): see docs/testing.md");
        std::fs::write(dir.join("Main.java"), driver).unwrap();
        let mut sources: Vec<std::path::PathBuf> = std::fs::read_dir(dir)
            .unwrap()
            .map(|e| e.unwrap().path())
            .filter(|p| p.extension().is_some_and(|e| e == "java"))
            .collect();
        sources.sort();
        let build = javac_command()
            .arg("-d")
            .arg(dir)
            .args(&sources)
            .output()
            .expect("spawn javac");
        // A compile failure is surfaced as the `javac` `Output` so the caller's `status.success()` assertion reports it.
        if !build.status.success() {
            return build;
        }
        dewasm_test_helper::run_command(Command::new(&java).arg("-cp").arg(dir).arg("Main"), "")
    }
}

// --------------------------------------------------------------------- Library-case glue (a `public class Main` appended after the generated module class).

/// `add.wat`: call the exported functions and print each result.
const JAVA_ADD_GLUE: &str = r#"public class Main {
    public static void main(String[] a) {
        Add inst = new Add(null, null, null, null);
        System.out.println((int)(Integer)((Add.Rt.Fn) inst.Exports.get("add")).invoke(new Object[]{2, 3}));
        System.out.println((int)(Integer)((Add.Rt.Fn) inst.Exports.get("add")).invoke(new Object[]{0xffffffff, 1}));
        System.out.println((int)(Integer)((Add.Rt.Fn) inst.Exports.get("fib")).invoke(new Object[]{10}));
    }
}
"#;

/// The override/fallback glue: an explicit `fd_write` import wins, `random_get` falls back to the bundled WASI.
/// Mirrors the other backends' override glues: intercept fd_write and print the actual bytes written.
const JAVA_OVERRIDE_GLUE: &str = r#"public class Main {
    public static void main(String[] a) throws Exception {
        java.io.ByteArrayOutputStream captured = new java.io.ByteArrayOutputStream();
        Prog[] holder = new Prog[1];
        Prog.Rt.Fn fdWrite = args -> {
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
        ((Prog.Rt.Fn) p.Exports.get("_start")).invoke(new Object[]{}); // random_get falls back to bundled WASI
        System.out.write(captured.toByteArray());
        System.out.flush();
    }
}
"#;

/// The `custom_wasi_provider` glue: a provider *object* replaces the bundled WASI wholesale: `wasmImport(name)` resolves every function and `attach(instance)` binds the memory (the Java shape of Ruby's `import`/`attach`), so no import falls back and `p.wasi` stays null.
const JAVA_CUSTOM_PROVIDER_GLUE: &str = r#"public class Main {
    static class MyWasi implements Prog.Rt.ImportProvider {
        Prog inst;
        java.io.ByteArrayOutputStream out = new java.io.ByteArrayOutputStream();

        public Object wasmImport(String name) {
            if (name.equals("fd_write")) {
                return (Prog.Rt.Fn) this::fdWrite;
            }
            if (name.equals("random_get")) {
                return (Prog.Rt.Fn) args -> 0;
            }
            return null;
        }

        public void attach(Object instance) {
            this.inst = (Prog) instance;
        }

        Object fdWrite(Object[] args) {
            int iovs = (int)(Integer) args[1];
            int ptr = inst.memory.i32_load(Integer.toUnsignedLong(iovs));
            int len = inst.memory.i32_load(Integer.toUnsignedLong(iovs) + 4);
            byte[] b = inst.memory.read_string(Integer.toUnsignedLong(ptr), Integer.toUnsignedLong(len));
            out.write(b, 0, b.length);
            inst.memory.i32_store(Integer.toUnsignedLong((int)(Integer) args[3]), len);
            return 0;
        }
    }

    public static void main(String[] a) throws Exception {
        MyWasi wasi = new MyWasi();
        Prog p = new Prog(java.util.Map.of("wasi_snapshot_preview1", wasi), null, null, null);
        ((Prog.Rt.Fn) p.Exports.get("_start")).invoke(new Object[]{});
        System.out.write(wasi.out.toByteArray());
        System.out.flush();
        System.out.println("bundled wasi constructed: " + (p.wasi != null));
    }
}
"#;

/// The `partial_override_falls_back_to_bundled_wasi` glue: the override glue above (fd_write intercepted, random_get falling back) plus the probe that the bundled WASI *was* built for that one fallback: `wasiInstance()` constructs it as the constructor takes the adapter.
const JAVA_PARTIAL_OVERRIDE_GLUE: &str = r#"public class Main {
    public static void main(String[] a) throws Exception {
        java.io.ByteArrayOutputStream captured = new java.io.ByteArrayOutputStream();
        Prog[] holder = new Prog[1];
        Prog.Rt.Fn fdWrite = args -> {
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
        java.util.Map<String, Object> imports = new java.util.HashMap<>();
        imports.put("wasi_snapshot_preview1", wasi);
        Prog p = new Prog(imports, null, null, null);
        holder[0] = p;
        ((Prog.Rt.Fn) p.Exports.get("_start")).invoke(new Object[]{}); // random_get falls back to bundled WASI
        System.out.write(captured.toByteArray());
        System.out.flush();
        System.out.println("bundled wasi constructed: " + (p.wasi != null));
    }
}
"#;

/// The `wasi_stdio_capture` glue: Java's bundled WASI is built by the module ctor's `fd_write` fallback and holds an `OutputStream` at fd 1, so inject a `ByteArrayOutputStream` into the (package-private, default-package-reachable) `wasi.fds` map after construction, the Java mirror of Ruby's `$stdout` redirect.
/// Run `_start` (swallowing a clean `proc_exit`), then flush the captured bytes to the real stdout.
const JAVA_STDIO_CAPTURE_GLUE: &str = r#"public class Main {
    public static void main(String[] a) throws Exception {
        Prog p = new Prog(null, null, null, null);
        java.io.ByteArrayOutputStream captured = new java.io.ByteArrayOutputStream();
        p.wasi.fds.put(1, captured);
        try {
            ((Prog.Rt.Fn) p.Exports.get("_start")).invoke(new Object[]{});
        } catch (Prog.Rt.Exit e) {
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
            ((Prog.Rt.Fn) p.Exports.get("_start")).invoke(new Object[]{});
        } catch (Prog.Rt.Exit e) {
            System.out.println(e.code);
        }
    }
}
"#;

/// The root-preopen containment probe: probe the WASI sandbox resolver directly (no guest run): with `/` preopened at host `/`, resolving a relative path off the preopen fd (3) must stay contained (errno WASI_OK == 0).
const JAVA_CONTAINMENT_GLUE: &str = r#"public class Main {
    public static void main(String[] a) throws Exception {
        Prog.WASI w = new Prog.WASI(null, null, java.util.Map.of("/", "/"));
        Prog.WASI.Resolved r = w.resolve_path(3, "etc", true);
        System.out.println(r.errno == 0 ? "contained" : "rejected");
    }
}
"#;

// --------------------------------------------------------------------- Filesystem app glue: class/argv/env/preopen-guest-paths are literals; only the host scratch dir comes through {scratch}.

const JAVA_QJS_FILE_IO_GLUE: &str = r#"public class Main {
    public static void main(String[] a) throws Exception {
        Qjs inst = new Qjs(null, new String[]{"qjs", "/work/qjs_file_io.js"}, null, java.util.Map.of("/work", "{scratch}"));
        try {
            ((Qjs.Rt.Fn) inst.Exports.get("_start")).invoke(new Object[]{});
        } catch (Qjs.Rt.Exit e) {
        }
    }
}
"#;

const JAVA_SQLITE3_SHELL_GLUE: &str = r#"public class Main {
    public static void main(String[] a) throws Exception {
        Sqlite3Shell inst = new Sqlite3Shell(null, new String[]{"sqlite3"}, null, java.util.Map.of("/db", "{scratch}"));
        try {
            ((Sqlite3Shell.Rt.Fn) inst.Exports.get("_start")).invoke(new Object[]{});
        } catch (Sqlite3Shell.Rt.Exit e) {
        }
    }
}
"#;

const JAVA_RG_SEARCH_GLUE: &str = r#"public class Main {
    public static void main(String[] a) throws Exception {
        Rg inst = new Rg(null, new String[]{"rg", "--sort", "path", "needle", "/work"}, null, java.util.Map.of("/work", "{scratch}"));
        try {
            ((Rg.Rt.Fn) inst.Exports.get("_start")).invoke(new Object[]{});
        } catch (Rg.Rt.Exit e) {
        }
    }
}
"#;

/// The whole app cache is preopened at `/apps` because the guest module this converted interpreter loads (`cowsay.wasm`) is itself a cached app.
const JAVA_TOYWASM_GLUE: &str = r#"public class Main {
    public static void main(String[] a) throws Exception {
        Toywasm inst = new Toywasm(null, new String[]{"toywasm", "--wasi", "/apps/cowsay.wasm", "Hello", "from", "dewasm!"}, null, java.util.Map.of("/apps", "{cache}"));
        try {
            ((Toywasm.Rt.Fn) inst.Exports.get("_start")).invoke(new Object[]{});
        } catch (Toywasm.Rt.Exit e) {
        }
    }
}
"#;

/// Like the toywasm glue; wasm3's CLI takes the guest module directly (its meta-WASI build always forwards the guest's WASI).
/// Plain glue on the main thread, unlike every other converted-interpreter case here: the official asset's dispatch is a tail call, so the trampoline runs the whole chain in one JVM frame and the default stack is enough.
const JAVA_WASM3_GLUE: &str = r#"public class Main {
    public static void main(String[] a) throws Exception {
        Wasm3 inst = new Wasm3(null, new String[]{"wasm3", "/apps/cowsay.wasm", "Hello", "from", "dewasm!"}, null, java.util.Map.of("/apps", "{cache}"));
        try {
            ((Wasm3.Rt.Fn) inst.Exports.get("_start")).invoke(new Object[]{});
        } catch (Wasm3.Rt.Exit e) {
        }
    }
}
"#;

/// The interpreter stdlib trees mount straight from the app cache ({cache}), never copied.
const JAVA_CPYTHON_HELLO_GLUE: &str = r#"public class Main {
    public static void main(String[] a) throws Exception {
        Cpython inst = new Cpython(null, new String[]{"python", "-c", "print('hello from cpython', 6 * 7)"}, new String[]{"PYTHONHOME=/", "PYTHONPATH=/lib/python3.14"}, java.util.Map.of("/lib", "{cache}/cpython-lib/lib"));
        try {
            ((Cpython.Rt.Fn) inst.Exports.get("_start")).invoke(new Object[]{});
        } catch (Cpython.Rt.Exit e) {
        }
    }
}
"#;

const JAVA_CRUBY_HELLO_GLUE: &str = r#"public class Main {
    public static void main(String[] a) throws Exception {
        Cruby inst = new Cruby(null, new String[]{"ruby", "-e", "puts \"hello from cruby #{6*7}\""}, null, java.util.Map.of("/usr", "{cache}/ruby-lib/usr"));
        try {
            ((Cruby.Rt.Fn) inst.Exports.get("_start")).invoke(new Object[]{});
        } catch (Cruby.Rt.Exit e) {
        }
    }
}
"#;

// --------------------------------------------------------------------- C-API drive glue (sqlite3): malloc/pointer plumbing via Memory.
// No wasmtime snapshot (the results live in guest memory), so each drive's output is pinned in the shared case const.
// Only the file-backed case uses {scratch}.

/// The sqlite3 C API driven in memory: `_initialize`, `sqlite3_malloc` + `Memory` pointer plumbing, open/exec/prepare/step/column/finalize/close.
const JAVA_LIBSQLITE3_MEM: &str = r#"public class Main {
    static final java.nio.charset.Charset UTF_8 = java.nio.charset.StandardCharsets.UTF_8;
    static Libsqlite3 inst;

    static int malloc(int n) {
        return (int)(Integer)((Libsqlite3.Rt.Fn) inst.Exports.get("sqlite3_malloc")).invoke(new Object[]{n});
    }

    static int call(String name, Object... args) {
        return (int)(Integer)((Libsqlite3.Rt.Fn) inst.Exports.get(name)).invoke(args);
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
        ((Libsqlite3.Rt.Fn) inst.Exports.get("_initialize")).invoke(new Object[]{});

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

/// The sqlite3 C API against a file preopen: create+insert, close, reopen, select: the file lifecycle through the C API (same fs stack as the shell), leaving a nonzero DB file on the host.
const JAVA_LIBSQLITE3_FILE: &str = r#"public class Main {
    static final java.nio.charset.Charset UTF_8 = java.nio.charset.StandardCharsets.UTF_8;
    static Libsqlite3 inst;

    static int malloc(int n) {
        return (int)(Integer)((Libsqlite3.Rt.Fn) inst.Exports.get("sqlite3_malloc")).invoke(new Object[]{n});
    }

    static int call(String name, Object... args) {
        return (int)(Integer)((Libsqlite3.Rt.Fn) inst.Exports.get(name)).invoke(args);
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
        ((Libsqlite3.Rt.Fn) inst.Exports.get("_initialize")).invoke(new Object[]{});

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

/// Guest->host callback round trip: the committed `sqlite3-binding.wasm` exports `run_query`, which calls `sqlite3_exec` with a C callback forwarding each row to the *imported* `env.host_row` (a `void(argc, argv_ptr)`, so the lambda returns null).
/// The glue provides `host_row` via the import-provider mechanism and collects the rows.
const JAVA_SQLITE3_CALLBACK: &str = r#"public class Main {
    static final java.nio.charset.Charset UTF_8 = java.nio.charset.StandardCharsets.UTF_8;
    static Sqlite3Binding inst;
    static final java.util.List<String> rows = new java.util.ArrayList<>();

    static int malloc(int n) {
        return (int)(Integer)((Sqlite3Binding.Rt.Fn) inst.Exports.get("sqlite3_malloc")).invoke(new Object[]{n});
    }

    static int call(String name, Object... args) {
        return (int)(Integer)((Sqlite3Binding.Rt.Fn) inst.Exports.get(name)).invoke(args);
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
        Sqlite3Binding.Rt.Fn hostRow = args -> {
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
        ((Sqlite3Binding.Rt.Fn) inst.Exports.get("_initialize")).invoke(new Object[]{});

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

/// zeroperl Perl-5.42 eval (issue #67): instantiate the reactor with a zero-returning `env.call_host_function` import stub (only invoked when the guest registers host callbacks, and this program registers none) and a
/// `/dev/null` preopen (`zeroperl_init` returns 1 without it), then
/// `_initialize` → `zeroperl_init` → `malloc` + copy a Perl program into guest memory → `zeroperl_eval` → `zeroperl_flush`.
/// The program is a regex capture and a `printf`, so its stdout is deterministic.
/// Java has no raw string literal at the JDK 11 baseline, so the Perl source is concatenated with its backslashes doubled: every `\` below belongs to Perl.
const JAVA_ZEROPERL_EVAL: &str = r#"public class Main {
    static Zeroperl inst;

    static int call(String name, Object... args) {
        return (int)(Integer)((Zeroperl.Rt.Fn) inst.Exports.get(name)).invoke(args);
    }

    public static void main(String[] a) throws Exception {
        java.util.Map<String, Object> env = new java.util.HashMap<>();
        env.put("call_host_function", (Zeroperl.Rt.Fn) args -> 0);
        java.util.Map<String, java.util.Map<String, Object>> imports = new java.util.HashMap<>();
        imports.put("env", env);
        inst = new Zeroperl(imports, null, null, java.util.Map.of("/dev/null", "/dev/null"));
        ((Zeroperl.Rt.Fn) inst.Exports.get("_initialize")).invoke(new Object[]{});
        int rc = call("zeroperl_init");
        if (rc != 0) throw new RuntimeException("zeroperl_init rc=" + rc);

        String prog =
            "my $s = \"hello world 42\";\n"
            + "if ($s =~ /(\\w+)\\s+(\\w+)\\s+(\\d+)/) {\n"
            + "  printf(\"m=%s|%s|%d sum=%d\\n\", $1, $2, $3, $3 + 8);\n"
            + "}\n";
        byte[] u = prog.getBytes(java.nio.charset.StandardCharsets.UTF_8);
        byte[] c = java.util.Arrays.copyOf(u, u.length + 1); // trailing NUL
        int ptr = call("malloc", c.length);
        inst.memory.init(Integer.toUnsignedLong(ptr), c, 0, c.length);
        call("zeroperl_eval", ptr, 0, 0, 0);
        call("zeroperl_flush");
    }
}
"#;

/// ExifTool on zeroperl (issue #70): the flattened `exiftool` CLI driver
/// (`{cache}/exiftool-lib/exiftool`, preopened at `/work`) run on the same
/// `cache/zeroperl.wasm` reactor, whose SFS blob embeds the `Image::ExifTool`
/// module tree, so `use Image::ExifTool` resolves in-guest with no module preopen.
/// Instantiated like [`JAVA_ZEROPERL_EVAL`] (the `call_host_function`
/// stub + a `/dev/null` preopen), plus the staged image at `/img`.
/// The Perl driver snippet sets `@ARGV`/`$0` and `do`es the script; it first overrides
/// `CORE::GLOBAL::exit` to a `die` so ExifTool's terminal `exit` unwinds back into `eval_pv` instead of tripping `proc_exit`, then `zeroperl_flush`
/// pushes ExifTool's buffered stdout out through fd 1.
/// Only deterministic tags are requested (`-S -Make -Model -DateTimeOriginal`).
const JAVA_EXIFTOOL: &str = r#"public class Main {
    static Zeroperl inst;

    static int call(String name, Object... args) {
        return (int)(Integer)((Zeroperl.Rt.Fn) inst.Exports.get(name)).invoke(args);
    }

    public static void main(String[] a) throws Exception {
        java.util.Map<String, Object> env = new java.util.HashMap<>();
        env.put("call_host_function", (Zeroperl.Rt.Fn) args -> 0);
        java.util.Map<String, java.util.Map<String, Object>> imports = new java.util.HashMap<>();
        imports.put("env", env);
        java.util.Map<String, String> preopens = new java.util.HashMap<>();
        preopens.put("/dev/null", "/dev/null");
        preopens.put("/work", "{cache}/exiftool-lib");
        preopens.put("/img", "{scratch}");
        inst = new Zeroperl(imports, null, null, preopens);
        ((Zeroperl.Rt.Fn) inst.Exports.get("_initialize")).invoke(new Object[]{});
        int rc = call("zeroperl_init");
        if (rc != 0) throw new RuntimeException("zeroperl_init rc=" + rc);

        String driver =
            "BEGIN { *CORE::GLOBAL::exit = sub (;$) { die \"zeroperl_exit\\n\" }; }\n"
            + "@ARGV = ('-S', '-Make', '-Model', '-DateTimeOriginal', '/img/exif_fixture.jpg');\n"
            + "$0 = '/work/exiftool';\n"
            + "do '/work/exiftool';\n";
        byte[] u = driver.getBytes(java.nio.charset.StandardCharsets.UTF_8);
        byte[] c = java.util.Arrays.copyOf(u, u.length + 1); // trailing NUL
        int ptr = call("malloc", c.length);
        inst.memory.init(Integer.toUnsignedLong(ptr), c, 0, c.length);
        call("zeroperl_eval", ptr, 0, 0, 0);
        call("zeroperl_flush");
    }
}
"#;

/// libpcap BPF filter compilation: drive `compile_filter` on "tcp port 80" (DLT_EN10MB, snaplen 65535), then walk the serialized program `[u32 bf_len][bf_len × {u16 code; u8 jt; u8 jf; u32 k}]` in guest memory, printing each instruction as `code jt jf k`.
/// The allocator here is plain `malloc`/`free` (libpcap is not sqlite), and `free` returns void, so it goes through `Rt.Fn.invoke` directly rather than the int-returning `call` helper.
const JAVA_PCAP_COMPILE: &str = r#"public class Main {
    static final java.nio.charset.Charset UTF_8 = java.nio.charset.StandardCharsets.UTF_8;
    static Libpcap inst;

    static int malloc(int n) {
        return (int)(Integer)((Libpcap.Rt.Fn) inst.Exports.get("malloc")).invoke(new Object[]{n});
    }

    static int call(String name, Object... args) {
        return (int)(Integer)((Libpcap.Rt.Fn) inst.Exports.get(name)).invoke(args);
    }

    static int cstr(String s) {
        byte[] u = s.getBytes(UTF_8);
        byte[] c = java.util.Arrays.copyOf(u, u.length + 1); // trailing NUL
        int p = malloc(c.length);
        inst.memory.init(Integer.toUnsignedLong(p), c, 0, c.length);
        return p;
    }

    public static void main(String[] a) throws Exception {
        inst = new Libpcap(null, null, null, null);
        ((Libpcap.Rt.Fn) inst.Exports.get("_initialize")).invoke(new Object[]{});

        int prog = call("compile_filter", cstr("tcp port 80"), 1, 65535);
        if (prog == 0) throw new RuntimeException("compile failed");
        byte[] d = inst.memory.d;
        int n = inst.memory.i32_load(Integer.toUnsignedLong(prog));
        for (int i = 0; i < n; i++) {
            int base = prog + 4 + i * 8;
            int code = (d[base] & 0xff) | ((d[base + 1] & 0xff) << 8);
            int jt = d[base + 2] & 0xff;
            int jf = d[base + 3] & 0xff;
            int k = inst.memory.i32_load(Integer.toUnsignedLong(base + 4));
            System.out.println(code + " " + jt + " " + jf + " " + k);
        }
        ((Libpcap.Rt.Fn) inst.Exports.get("free")).invoke(new Object[]{prog});
        System.out.println("BPF-OK");
    }
}
"#;

/// tree-sitter JSON parse: drive `parse_source` on the fixed snippet `{"key": [1, true, null]}` and print the parse tree's S-expression (a malloc'd NUL-terminated C string) from guest memory.
const JAVA_TREESITTER_PARSE: &str = r#"public class Main {
    static final java.nio.charset.Charset UTF_8 = java.nio.charset.StandardCharsets.UTF_8;
    static Treesitter inst;

    static int malloc(int n) {
        return (int)(Integer)((Treesitter.Rt.Fn) inst.Exports.get("malloc")).invoke(new Object[]{n});
    }

    static int call(String name, Object... args) {
        return (int)(Integer)((Treesitter.Rt.Fn) inst.Exports.get(name)).invoke(args);
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
        inst = new Treesitter(null, null, null, null);
        ((Treesitter.Rt.Fn) inst.Exports.get("_initialize")).invoke(new Object[]{});

        String src = "{\"key\": [1, true, null]}";
        int r = call("parse_source", cstr(src), src.getBytes(UTF_8).length);
        if (r == 0) throw new RuntimeException("parse failed");
        System.out.println(readCstr(r));
        ((Treesitter.Rt.Fn) inst.Exports.get("free")).invoke(new Object[]{r});
        System.out.println("TS-OK");
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

/// Driver for the Embedded-coexistence case: two independently converted classes in one default package, each carrying its runtime as `static` nested classes, so `Alpha.Rt.Trap` and `Beta.Rt.Trap` are different types and Alpha's trap is catchable by name.
/// `Class` objects of two unrelated types are incomparable with `!=` in Java (the compiler rejects `Class<Alpha.Rt.Trap> != Class<Beta.Rt.Trap>`), so the distinctness check goes through `equals`.
const JAVA_EMBEDDED_COEXIST_GLUE: &str = r#"public class Main {
    public static void main(String[] a) throws Exception {
        Alpha alpha = new Alpha(null, null, null, null);
        Beta beta = new Beta(null, null, null, null);
        System.out.println((int)(Integer)((Alpha.Rt.Fn) alpha.Exports.get("div")).invoke(new Object[]{7, 2}));
        System.out.println(Integer.toUnsignedString((int)(Integer)((Beta.Rt.Fn) beta.Exports.get("div")).invoke(new Object[]{0xfffffff9, 2})));
        System.out.println(Alpha.Rt.Trap.class.equals(Beta.Rt.Trap.class) ? "same-rt" : "distinct-rt");
        try {
            ((Alpha.Rt.Fn) alpha.Exports.get("div")).invoke(new Object[]{1, 0});
        } catch (Alpha.Rt.Trap e) {
            System.out.println("trapped");
        }
    }
}
"#;

/// DOOM: deterministic drive (synthetic clock, no input) dumping the framebuffer as a P6 PPM matching the wasmtime snapshot.
/// `{ticks}`/`{clock_step}` filled by the runner.
const JAVA_DOOM_FRAME_GLUE: &str = r#"public class Main {
    public static void main(String[] a) throws Exception {
        final long[] ms = {0};
        final int[] fw = {0}, fh = {0}, foff = {0};
        java.util.Map<String, java.util.Map<String, Object>> imports = new java.util.HashMap<>();
        java.util.Map<String, Object> console = new java.util.HashMap<>();
        console.put("onErrorMessage", (Doom.Rt.Fn)(args -> null));
        console.put("onInfoMessage", (Doom.Rt.Fn)(args -> null));
        imports.put("console", console);
        java.util.Map<String, Object> gameSaving = new java.util.HashMap<>();
        gameSaving.put("sizeOfSaveGame", (Doom.Rt.Fn)(args -> 0));
        gameSaving.put("readSaveGame", (Doom.Rt.Fn)(args -> 0));
        gameSaving.put("writeSaveGame", (Doom.Rt.Fn)(args -> args[2]));
        imports.put("gameSaving", gameSaving);
        java.util.Map<String, Object> runtimeControl = new java.util.HashMap<>();
        runtimeControl.put("timeInMilliseconds", (Doom.Rt.Fn)(args -> { ms[0] += {clock_step}; return ms[0]; }));
        imports.put("runtimeControl", runtimeControl);
        java.util.Map<String, Object> ui = new java.util.HashMap<>();
        ui.put("drawFrame", (Doom.Rt.Fn)(args -> { foff[0] = (int)(Integer) args[0]; return null; }));
        imports.put("ui", ui);
        java.util.Map<String, Object> loading = new java.util.HashMap<>();
        loading.put("onGameInit", (Doom.Rt.Fn)(args -> { fw[0] = (int)(Integer) args[0]; fh[0] = (int)(Integer) args[1]; return null; }));
        loading.put("wadSizes", (Doom.Rt.Fn)(args -> null));
        loading.put("readWads", (Doom.Rt.Fn)(args -> null));
        imports.put("loading", loading);

        Doom doom = new Doom(imports, null, null, null);
        ((Doom.Rt.Fn) doom.Exports.get("initGame")).invoke(new Object[]{});
        Doom.Rt.Fn tick = (Doom.Rt.Fn) doom.Exports.get("tickGame");
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

/// NES (issue #114, mirrors the DOOM glue above): load the pinned ROM into
/// `allocRom`'s buffer, tick `{frames}` times with no input, compose the frame from agnes's palette-index screen buffer and its palette (issue #117; the
/// `& 0x3f` mask is load-bearing) and dump it as a P6 PPM matching the wasmtime snapshot.
/// `{rom}` (the cached ROM's host path) and `{frames}` filled by the runner.
const JAVA_NES_FRAME_GLUE: &str = r#"public class Main {
    public static void main(String[] a) throws Exception {
        byte[] rom = java.nio.file.Files.readAllBytes(java.nio.file.Paths.get("{rom}"));
        Nes nes = new Nes(null, null, null, null);
        ((Nes.Rt.Fn) nes.Exports.get("_initialize")).invoke(new Object[]{});
        int ptr = (int)(Integer) ((Nes.Rt.Fn) nes.Exports.get("allocRom")).invoke(new Object[]{rom.length});
        nes.memory.init(Integer.toUnsignedLong(ptr), rom, 0, rom.length);
        int ok = (int)(Integer) ((Nes.Rt.Fn) nes.Exports.get("initGame")).invoke(new Object[]{});
        if (ok != 1) throw new RuntimeException("initGame failed: " + ok);
        Nes.Rt.Fn tick = (Nes.Rt.Fn) nes.Exports.get("tickGame");
        for (int i = 0; i < {frames}; i++) tick.invoke(new Object[]{});

        int w = (int)(Integer) ((Nes.Rt.Fn) nes.Exports.get("frameWidth")).invoke(new Object[]{});
        int h = (int)(Integer) ((Nes.Rt.Fn) nes.Exports.get("frameHeight")).invoke(new Object[]{});
        int soff = (int)(Integer) ((Nes.Rt.Fn) nes.Exports.get("screenOffset")).invoke(new Object[]{});
        int poff = (int)(Integer) ((Nes.Rt.Fn) nes.Exports.get("paletteOffset")).invoke(new Object[]{});
        byte[] d = nes.memory.d;
        byte[] out = new byte[w * h * 3];
        int j = 0;
        for (int i = 0; i < w * h; i++) {
            int c = poff + (d[soff + i] & 0x3f) * 4;
            out[j++] = d[c];
            out[j++] = d[c + 1];
            out[j++] = d[c + 2];
        }
        System.out.write(("P6\n" + w + " " + h + "\n255\n").getBytes(java.nio.charset.StandardCharsets.US_ASCII));
        System.out.write(out);
        System.out.flush();
    }
}
"#;

dewasm_test_helper::library_add_e2e!(Java, JAVA_ADD_GLUE);
dewasm_test_helper::wasi_import_override_e2e!(Java, JAVA_OVERRIDE_GLUE);
dewasm_test_helper::stdio_capture_e2e!(Java, JAVA_STDIO_CAPTURE_GLUE);
dewasm_test_helper::custom_wasi_provider_e2e!(Java, JAVA_CUSTOM_PROVIDER_GLUE);
dewasm_test_helper::partial_override_e2e!(Java, JAVA_PARTIAL_OVERRIDE_GLUE);

dewasm_test_helper::wasi_suite!(Java, Stdio);
dewasm_test_helper::wasi_suite!(Java, ArgsEnv);
dewasm_test_helper::wasi_suite!(Java, Poll);
dewasm_test_helper::wasi_suite!(Java, Fs, JAVA_FS_GLUE);
dewasm_test_helper::wasi_root_containment_e2e!(Java, JAVA_CONTAINMENT_GLUE);
dewasm_test_helper::standalone_dir_e2e!(Java);
// The standalone entrypoint runs the guest on a dedicated 64 MiB thread (mirroring Python's mitigation), since Linux CI's 1 MiB default main-thread stack is marginal for 5000 guest frames.
dewasm_test_helper::deep_recursion_e2e!(Java);
dewasm_test_helper::folded_temp_reuse_e2e!(Java);

dewasm_test_helper::cowsay_args_e2e!(Java);
dewasm_test_helper::cowsay_stdin_e2e!(Java);
// Slow: measured 5.6 s end to end (javac on the generated interpreter dominates; cowsay is 0.4 s in this suite).
dewasm_test_helper::mruby_eh_e2e!(Java, slow);
dewasm_test_helper::qjs_eval_e2e!(Java);
dewasm_test_helper::sqlite3_shell_e2e!(Java);
dewasm_test_helper::gzip_e2e!(Java);

dewasm_test_helper::qjs_file_io_e2e!(Java, JAVA_QJS_FILE_IO_GLUE);
dewasm_test_helper::sqlite3_shell_dbfile_e2e!(Java, JAVA_SQLITE3_SHELL_GLUE);
dewasm_test_helper::rg_search_e2e!(Java, JAVA_RG_SEARCH_GLUE);
// The three interpreter giants (issue #142), excluded until the splitter learned to subdivide an oversized `br_table` (CPython's largest function holds a 3202-target table, one statement with no boundary to split at) and to spread funcref-table fillers over `ElemF{c}` classes (CRuby's 8737-entry table saturated one 65535-entry pool).
// Ultra: measured 42 s (CPython), 132 s (CRuby) and 137 s (packed CRuby), at up to ~8 GB of `javac` heap.
dewasm_test_helper::cpython_hello_e2e!(Java, JAVA_CPYTHON_HELLO_GLUE, ultra);
dewasm_test_helper::cruby_hello_e2e!(Java, JAVA_CRUBY_HELLO_GLUE, ultra);
dewasm_test_helper::cruby_packed_hello_e2e!(Java, ultra);
// Slow, like the other filesystem app cases: measured 7.6 s including `javac` (convert the interpreter, then interpret the cowsay guest).
dewasm_test_helper::toywasm_cowsay_e2e!(Java, JAVA_TOYWASM_GLUE);
// Slow for the same reason as the toywasm case above.
dewasm_test_helper::wasm3_cowsay_e2e!(Java, JAVA_WASM3_GLUE);
dewasm_test_helper::qjs_repl_pty_e2e!(Java);

dewasm_test_helper::libsqlite3_c_api_e2e!(Java, JAVA_LIBSQLITE3_MEM);
dewasm_test_helper::sqlite3_file_c_api_e2e!(Java, JAVA_LIBSQLITE3_FILE);
dewasm_test_helper::sqlite3_callback_binding_e2e!(Java, JAVA_SQLITE3_CALLBACK);
// The zeroperl reactor cases (issue #139) are Java's `ultra` ones: the 25 MB reactor becomes ~99 MB of Java, measured 43 s (zeroperl_eval) and 51 s (exiftool_extract).
// They also drove `FN_PARTITION_THRESHOLD` down to 2000: zeroperl's ~2450 constant-dense functions overflow a single class's 65535-entry pool.
dewasm_test_helper::zeroperl_eval_e2e!(Java, JAVA_ZEROPERL_EVAL, ultra);
dewasm_test_helper::exiftool_extract_e2e!(Java, JAVA_EXIFTOOL, ultra);
dewasm_test_helper::pcap_compile_e2e!(Java, JAVA_PCAP_COMPILE);
dewasm_test_helper::treesitter_parse_e2e!(Java, JAVA_TREESITTER_PARSE);

dewasm_test_helper::doom_frame_e2e!(Java, JAVA_DOOM_FRAME_GLUE);
dewasm_test_helper::nes_frame_e2e!(Java, JAVA_NES_FRAME_GLUE);

dewasm_test_helper::shared_table_e2e!(Java, JAVA_SHARED_TABLE_GLUE);
dewasm_test_helper::embedded_coexist_e2e!(Java, JAVA_EMBEDDED_COEXIST_GLUE);
