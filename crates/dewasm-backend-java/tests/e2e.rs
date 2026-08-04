//! Java end-to-end suites (ADR-27): the shared library / WASI / apps case consts (`dewasm-test-helper`) wired up for the Java backend. Per the ADR-27 revision this file holds ONLY the [`BackendUnderTest`] impl, named glue string constants, and per-case macro invocations; glue is a plain `&str` argument at the callsite, and which macros this file invokes is the capability declaration (with a REASON comment at any non-invocation).
//!
//! Java is a compiled backend, so it overrides `BackendUnderTest::run` (ADR-27's hook) to compile-and-execute: `javac` the generated `Main.java` into a content-addressed class-dir cache (so identical sources compile once), then run `java -cp <dir> Main` (ADR-30). Java covers full WASI preview 1 incl. the filesystem.

use std::process::{Command, Output};

use dewasm_backend::Backend;
use dewasm_backend_java::{find_java, JavaBackend};
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

    /// Compile `source` (a single `Main.java`) to a content-addressed class-dir cache and run it with `args`/`stdin`. A missing `javac`/`java` is a loud failure (ADR-15); a compile failure is surfaced as the `javac` `Output` so the caller's `status.success()` assertion reports it.
    fn run_bytes(&self, source: &str, args: &[&str], stdin: &[u8]) -> Output {
        let java =
            find_java().expect("java not found on PATH (or $DEWASM_JAVA) — see docs/testing.md");
        match build_java(source) {
            // A compile failure is surfaced as the `javac` `Output` so the caller's `status.success()` assertion reports it.
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

    /// Compile `source` and run `java -cp <classdir> Main <args...>` under a pty. A compile failure fails loud (ADR-15): there is no `status` for the caller to inspect on the pty path, so panic with the `javac` output.
    fn pty_command(&self, source: &str, args: &[&str]) -> dewasm_test_helper::PtyCommand {
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
        dewasm_test_helper::PtyCommand {
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
            let bytes =
                wat::parse_file(dewasm_test_helper::examples_dir().join(wat)).expect("parse wat");
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

// --------------------------------------------------------------------- C-API drive glue (sqlite3, libpcap, tree-sitter): malloc/pointer plumbing via Memory. No wasmtime snapshot — the results live in guest memory — so each drive's output is pinned in the shared case const. Only the file-backed case uses {scratch}.

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

/// zeroperl Perl-5.42 eval (issue #67): instantiate the reactor with a
/// zero-returning `env.call_host_function` import stub (only invoked when the
/// guest registers host callbacks — this program registers none) and a
/// `/dev/null` preopen (`zeroperl_init` returns 1 without it), then
/// `_initialize` → `zeroperl_init` → `malloc` + copy a Perl program into guest
/// memory → `zeroperl_eval` → `zeroperl_flush`. The program is a regex
/// capture and a `printf`, so its stdout is deterministic. Java has no raw string literal
/// at the JDK 11 baseline, so the Perl source is concatenated with its
/// backslashes doubled — every `\` below belongs to Perl.
const JAVA_ZEROPERL_EVAL: &str = r#"public class Main {
    static Zeroperl inst;

    static int call(String name, Object... args) {
        return (int)(Integer)((Rt.Fn) inst.Exports.get(name)).invoke(args);
    }

    public static void main(String[] a) throws Exception {
        java.util.Map<String, Object> env = new java.util.HashMap<>();
        env.put("call_host_function", (Rt.Fn) args -> 0);
        java.util.Map<String, java.util.Map<String, Object>> imports = new java.util.HashMap<>();
        imports.put("env", env);
        inst = new Zeroperl(imports, null, null, java.util.Map.of("/dev/null", "/dev/null"));
        ((Rt.Fn) inst.Exports.get("_initialize")).invoke(new Object[]{});
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
/// module tree — so `use Image::ExifTool` resolves in-guest with no module
/// preopen. Instantiated like [`JAVA_ZEROPERL_EVAL`] (the `call_host_function`
/// stub + a `/dev/null` preopen), plus the staged image at `/img`. The Perl
/// driver snippet sets `@ARGV`/`$0` and `do`es the script; it first overrides
/// `CORE::GLOBAL::exit` to a `die` so ExifTool's terminal `exit` unwinds back
/// into `eval_pv` instead of tripping `proc_exit` — then `zeroperl_flush`
/// pushes ExifTool's buffered stdout out through fd 1. Only deterministic tags
/// are requested (`-S -Make -Model -DateTimeOriginal`).
const JAVA_EXIFTOOL: &str = r#"public class Main {
    static Zeroperl inst;

    static int call(String name, Object... args) {
        return (int)(Integer)((Rt.Fn) inst.Exports.get(name)).invoke(args);
    }

    public static void main(String[] a) throws Exception {
        java.util.Map<String, Object> env = new java.util.HashMap<>();
        env.put("call_host_function", (Rt.Fn) args -> 0);
        java.util.Map<String, java.util.Map<String, Object>> imports = new java.util.HashMap<>();
        imports.put("env", env);
        java.util.Map<String, String> preopens = new java.util.HashMap<>();
        preopens.put("/dev/null", "/dev/null");
        preopens.put("/work", "{cache}/exiftool-lib");
        preopens.put("/img", "{scratch}");
        inst = new Zeroperl(imports, null, null, preopens);
        ((Rt.Fn) inst.Exports.get("_initialize")).invoke(new Object[]{});
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

/// libpcap BPF filter compilation: drive `compile_filter` on "tcp port 80" (DLT_EN10MB, snaplen 65535), then walk the serialized program `[u32 bf_len][bf_len × {u16 code; u8 jt; u8 jf; u32 k}]` in guest memory, printing each instruction as `code jt jf k`. The allocator here is plain `malloc`/`free` (libpcap is not sqlite), and `free` returns void, so it goes through `Rt.Fn.invoke` directly rather than the int-returning `call` helper.
const JAVA_PCAP_COMPILE: &str = r#"public class Main {
    static final java.nio.charset.Charset UTF_8 = java.nio.charset.StandardCharsets.UTF_8;
    static Libpcap inst;

    static int malloc(int n) {
        return (int)(Integer)((Rt.Fn) inst.Exports.get("malloc")).invoke(new Object[]{n});
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

    public static void main(String[] a) throws Exception {
        inst = new Libpcap(null, null, null, null);
        ((Rt.Fn) inst.Exports.get("_initialize")).invoke(new Object[]{});

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
        ((Rt.Fn) inst.Exports.get("free")).invoke(new Object[]{prog});
        System.out.println("BPF-OK");
    }
}
"#;

/// tree-sitter JSON parse: drive `parse_source` on the fixed snippet `{"key": [1, true, null]}` and print the parse tree's S-expression (a malloc'd NUL-terminated C string) from guest memory.
const JAVA_TREESITTER_PARSE: &str = r#"public class Main {
    static final java.nio.charset.Charset UTF_8 = java.nio.charset.StandardCharsets.UTF_8;
    static Treesitter inst;

    static int malloc(int n) {
        return (int)(Integer)((Rt.Fn) inst.Exports.get("malloc")).invoke(new Object[]{n});
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
        inst = new Treesitter(null, null, null, null);
        ((Rt.Fn) inst.Exports.get("_initialize")).invoke(new Object[]{});

        String src = "{\"key\": [1, true, null]}";
        int r = call("parse_source", cstr(src), src.getBytes(UTF_8).length);
        if (r == 0) throw new RuntimeException("parse failed");
        System.out.println(readCstr(r));
        ((Rt.Fn) inst.Exports.get("free")).invoke(new Object[]{r});
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

/// DOOM (ADR-53): deterministic drive (synthetic clock, no input) dumping the framebuffer as a P6 PPM matching the wasmtime snapshot. `{ticks}`/`{clock_step}` filled by the runner.
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

/// NES (issue #114, mirrors the DOOM glue above): load the pinned ROM into
/// `allocRom`'s buffer, tick `{frames}` times with no input, compose the frame
/// from agnes's palette-index screen buffer and its palette (issue #117; the
/// `& 0x3f` mask is load-bearing) and dump it as a P6 PPM matching the wasmtime
/// snapshot. `{rom}` (the cached ROM's host path) and `{frames}` filled by the
/// runner.
const JAVA_NES_FRAME_GLUE: &str = r#"public class Main {
    public static void main(String[] a) throws Exception {
        byte[] rom = java.nio.file.Files.readAllBytes(java.nio.file.Paths.get("{rom}"));
        Nes nes = new Nes(null, null, null, null);
        ((Rt.Fn) nes.Exports.get("_initialize")).invoke(new Object[]{});
        int ptr = (int)(Integer) ((Rt.Fn) nes.Exports.get("allocRom")).invoke(new Object[]{rom.length});
        nes.memory.init(Integer.toUnsignedLong(ptr), rom, 0, rom.length);
        int ok = (int)(Integer) ((Rt.Fn) nes.Exports.get("initGame")).invoke(new Object[]{});
        if (ok != 1) throw new RuntimeException("initGame failed: " + ok);
        Rt.Fn tick = (Rt.Fn) nes.Exports.get("tickGame");
        for (int i = 0; i < {frames}; i++) tick.invoke(new Object[]{});

        int w = (int)(Integer) ((Rt.Fn) nes.Exports.get("frameWidth")).invoke(new Object[]{});
        int h = (int)(Integer) ((Rt.Fn) nes.Exports.get("frameHeight")).invoke(new Object[]{});
        int soff = (int)(Integer) ((Rt.Fn) nes.Exports.get("screenOffset")).invoke(new Object[]{});
        int poff = (int)(Integer) ((Rt.Fn) nes.Exports.get("paletteOffset")).invoke(new Object[]{});
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

// --------------------------------------------------------------------- Suite wiring (ADR-27): each per-case macro invocation declares participation.

dewasm_test_helper::library_add_e2e!(Java, JAVA_ADD_GLUE);
dewasm_test_helper::wasi_import_override_e2e!(Java, JAVA_OVERRIDE_GLUE);
dewasm_test_helper::stdio_capture_e2e!(Java, JAVA_STDIO_CAPTURE_GLUE);
// custom_wasi_provider_e2e! / partial_override_e2e!: not invoked — Java's bundled WASI is eagerly constructed in the ctor and there is no provider-object import form (ADR-30), so the lazy-construction observable cannot hold.

dewasm_test_helper::wasi_suite!(Java, Stdio);
dewasm_test_helper::wasi_suite!(Java, ArgsEnv);
dewasm_test_helper::wasi_suite!(Java, Poll);
dewasm_test_helper::wasi_suite!(Java, Fs, JAVA_FS_GLUE);
dewasm_test_helper::wasi_root_containment_e2e!(Java, JAVA_CONTAINMENT_GLUE);
dewasm_test_helper::standalone_dir_e2e!(Java);
// The standalone entrypoint runs the guest on a dedicated 64 MiB thread (mirroring Python's ADR-28 mitigation), since Linux CI's 1 MiB default main-thread stack is marginal for 5000 guest frames.
dewasm_test_helper::deep_recursion_e2e!(Java);

dewasm_test_helper::cowsay_args_e2e!(Java);
dewasm_test_helper::cowsay_stdin_e2e!(Java);
dewasm_test_helper::qjs_eval_e2e!(Java);
dewasm_test_helper::sqlite3_shell_e2e!(Java);
dewasm_test_helper::gzip_e2e!(Java);

dewasm_test_helper::qjs_file_io_e2e!(Java, JAVA_QJS_FILE_IO_GLUE);
dewasm_test_helper::sqlite3_shell_dbfile_e2e!(Java, JAVA_SQLITE3_SHELL_GLUE);
dewasm_test_helper::rg_search_e2e!(Java, JAVA_RG_SEARCH_GLUE);
// cpython_hello_e2e!: not invoked — a CPython interpreter method overflows the JVM 64 KB per-method bytecode limit (`code too large`); the ADR-30 class-splitter does not subdivide individual methods (a hard limit; see docs/apps-audit.md). cruby_hello_e2e! / cruby_packed_hello_e2e!: not invoked — the CRuby element-segment `Elem` class overflows the JVM 64 K constant-pool limit (`too many constants`), a hard limit shared by the wasi-vfs-packed variant (ADR-61), whose element segments are the same interpreter's (docs/apps-audit.md).
dewasm_test_helper::qjs_repl_pty_e2e!(Java);

dewasm_test_helper::libsqlite3_c_api_e2e!(Java, JAVA_LIBSQLITE3_MEM);
dewasm_test_helper::sqlite3_file_c_api_e2e!(Java, JAVA_LIBSQLITE3_FILE);
dewasm_test_helper::sqlite3_callback_binding_e2e!(Java, JAVA_SQLITE3_CALLBACK);
// The zeroperl reactor cases (issue #139) are Java's first `ultra` ones (ADR-48), hence the crate's new `ultra_slow_test` feature. The 25 MB reactor becomes ~99 MB of Java, and each case `javac`s its own `Main` from cold: measured 43 s (zeroperl_eval) and 51 s (exiftool_extract) on an M-series laptop, so both sit at or past the ~1-min CI-runner bar, and they would compile concurrently in this one test binary. They also drove the ADR-30 function-partition threshold down to 2000 (`FN_PARTITION_THRESHOLD`): zeroperl's ~2450 constant-dense functions overflow a single class's 65535-entry pool (`javac`: *too many constants*).
dewasm_test_helper::zeroperl_eval_e2e!(Java, JAVA_ZEROPERL_EVAL, ultra);
dewasm_test_helper::exiftool_extract_e2e!(Java, JAVA_EXIFTOOL, ultra);
dewasm_test_helper::pcap_compile_e2e!(Java, JAVA_PCAP_COMPILE);
dewasm_test_helper::treesitter_parse_e2e!(Java, JAVA_TREESITTER_PARSE);

dewasm_test_helper::doom_frame_e2e!(Java, JAVA_DOOM_FRAME_GLUE);
dewasm_test_helper::nes_frame_e2e!(Java, JAVA_NES_FRAME_GLUE);

dewasm_test_helper::shared_table_e2e!(Java, JAVA_SHARED_TABLE_GLUE);
// embedded_coexist_e2e!: not invoked — a single flat top-level runtime is shared by all modules (ADR-30); two independent runtimes cannot coexist.
