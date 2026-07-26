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
//! First-milestone scope (ADR-30): "cowsay runs". WASI covers the eight core
//! syscalls (stdio + args/env), so the whole-program `Stdio`/`ArgsEnv` kinds and
//! the two library cases are wired, plus `apps_e2e!` (cowsay byte-identical).
//! The heavy qjs/sqlite cases and the `Fs` suite wait for later milestones.

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::process::{Command, Output};

use dewasm_backend::Backend;
use dewasm_backend_java::{find_java, find_javac, JavaBackend};
use dewasm_test_helper::{
    apps_e2e, library_e2e, run_command_bytes, standalone_e2e, wasi_suite, BackendUnderTest,
    LibraryCase,
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

    /// The heavy app cases (QuickJS, SQLite) need wasm 2.0+/filesystem support
    /// beyond the cowsay milestone (ADR-30), so they stay off for Java.
    fn run_heavy_apps(&self) -> bool {
        false
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
        Add inst = new Add(null, null, null);
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
        Prog p = new Prog(imports, null, null);
        holder[0] = p;
        ((Rt.Fn) p.Exports.get("_start")).invoke(new Object[]{}); // random_get falls back to bundled WASI
        System.out.write(captured.toByteArray());
        System.out.flush();
    }
}
"#;

standalone_e2e!(Java);
library_e2e!(Java, java_glue);
wasi_suite!(Java, Stdio);
wasi_suite!(Java, ArgsEnv);
apps_e2e!(Java);
