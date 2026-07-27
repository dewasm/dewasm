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
use std::path::Path;
use std::process::{Command, Output};

use dewasm_backend::Backend;
use dewasm_backend_java::{find_java, find_javac, JavaBackend};
use dewasm_test_helper::{
    apps_e2e, fs_apps_e2e, gzip_e2e, library_e2e, run_command_bytes, standalone_e2e, wasi_suite,
    BackendUnderTest, LibraryCase, WasiCase,
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
wasi_suite!(Java, Poll);
wasi_suite!(Java, Fs, java_fs_glue);
apps_e2e!(Java);
gzip_e2e!(Java);
fs_apps_e2e!(Java);
