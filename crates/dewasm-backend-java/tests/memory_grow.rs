//! Regression tests for issue #27's memory-size overflow: Java's linear memory is a single `byte[]`, capped at `Integer.MAX_VALUE` bytes, so a spec-legal size of 32768+ pages (2 GiB+) cannot be represented. `memory.grow` must answer -1 (never `NegativeArraySizeException` from the overflowing int multiply), and instantiating a module whose *initial* size already exceeds the cap must fail with a clear trap, not the raw exception.
//!
//! These cases are Java-only (the cap is a JVM artifact — the other backends can grow to 32768 pages for real, which a shared case must not force), so they live here with their own minimal `javac`-and-run plumbing, mirroring the e2e suite's compile-and-execute recipe (ADR-30). A missing `javac`/`java` fails loud (ADR-15).

use std::process::{Command, Output};

use dewasm_backend::Mode;
use dewasm_backend_java::{find_java, find_javac, JavaBackend};
use dewasm_test_helper::convert_bytes;

/// Convert `wat` in library mode, append `glue` (a `public class Main`), compile the single compilation unit into a fresh scratch dir keyed by `name`, and run `java -cp <dir> Main`.
fn convert_and_run(wat: &str, glue: &str, name: &str) -> Output {
    let javac =
        find_javac().expect("javac not found on PATH (or $DEWASM_JAVAC) — see docs/testing.md");
    let java = find_java().expect("java not found on PATH (or $DEWASM_JAVA) — see docs/testing.md");

    let bytes = wat::parse_str(wat).expect("parse wat");
    let source = format!(
        "{}\n{glue}",
        convert_bytes(&JavaBackend, &bytes, Mode::Library, "prog")
    );

    let dir = std::env::temp_dir().join(format!("dewasm-java-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let src = dir.join("Main.java");
    std::fs::write(&src, source).unwrap();
    let build = Command::new(&javac)
        .arg("-d")
        .arg(&dir)
        .arg(&src)
        .output()
        .expect("spawn javac");
    assert!(
        build.status.success(),
        "javac failed:\n{}",
        String::from_utf8_lossy(&build.stderr)
    );
    Command::new(&java)
        .arg("-cp")
        .arg(&dir)
        .arg("Main")
        .output()
        .expect("spawn java")
}

/// `memory.grow` to 32768 pages (2^31 bytes, one past the `byte[]` cap) must return -1 and leave the memory intact and still growable — with no declared max, `maxPages` defaults to 65536, so only the byte-size guard stands between the request and the overflowing allocation.
#[test]
fn grow_beyond_byte_array_cap_returns_minus_one() {
    let wat = r#"(module
      (memory 1)
      (func (export "grow") (param i32) (result i32) (memory.grow (local.get 0)))
      (func (export "size") (result i32) (memory.size)))"#;
    let glue = r#"public class Main {
    public static void main(String[] a) {
        Prog p = new Prog(null, null, null, null);
        System.out.println((int)(Integer)((Rt.Fn) p.Exports.get("grow")).invoke(new Object[]{32768}));
        System.out.println((int)(Integer)((Rt.Fn) p.Exports.get("grow")).invoke(new Object[]{1}));
        System.out.println((int)(Integer)((Rt.Fn) p.Exports.get("size")).invoke(new Object[]{}));
    }
}
"#;
    let out = convert_and_run(wat, glue, "grow-cap");
    assert!(
        out.status.success(),
        "run failed: {}\n{}",
        out.status,
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&out.stdout), "-1\n1\n2\n");
}

/// A module declaring an *initial* memory past the `byte[]` cap cannot be instantiated on the JVM; that failure must be a clear `Rt.Trap` naming the page count, not a `NegativeArraySizeException` (which would escape `Main` and fail the run's exit status here).
#[test]
fn initial_memory_beyond_byte_array_cap_traps_clearly() {
    let wat = r#"(module (memory 32768) (func (export "f")))"#;
    let glue = r#"public class Main {
    public static void main(String[] a) {
        try {
            new Prog(null, null, null, null);
            System.out.println("instantiated");
        } catch (Rt.Trap e) {
            System.out.println("trap: " + e.getMessage());
        }
    }
}
"#;
    let out = convert_and_run(wat, glue, "grow-initial-cap");
    assert!(
        out.status.success(),
        "run failed: {}\n{}",
        out.status,
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.starts_with("trap: ") && stdout.contains("32768 pages"),
        "expected a clear instantiation trap, got: {stdout:?}"
    );
}
