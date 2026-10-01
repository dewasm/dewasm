//! Regression tests for issue #27's memory-size overflow.
//! Java's linear memory is a single `byte[]`, capped at `Integer.MAX_VALUE` bytes.
//! So a size of 32768+ pages (2 GiB+), which the specification allows, cannot be represented.
//! `memory.grow` must answer -1.
//! It must never throw `NegativeArraySizeException` from the overflowing `int` multiply.
//! A module whose *initial* size already exceeds the cap can also not be instantiated.
//! That failure must be a clear trap, not the raw exception.
//!
//! These cases are Java-only, so they live here.
//! They run on the crate's shared compile-and-cache recipe.
//! The cap is a JVM artifact: the other backends can grow to 32768 pages for real.
//! A shared case must not force that.
//! A missing `javac`/`java` fails loud.

use std::process::{Command, Output};

use dewasm_backend::Mode;
use dewasm_backend_java::{find_java, JavaBackend};

mod common;

/// Convert `wat` in library mode, and append `glue` (a `public class Main`).
/// Then compile the single compilation unit, and run `java -cp <classdir> Main`.
/// Generated code that does not compile is a bug here, not an observable.
/// So a `javac` failure panics.
fn convert_and_run(wat: &str, glue: &str) -> Output {
    let java = find_java().expect("java not found on PATH (or $DEWASM_JAVA): see docs/testing.md");

    let bytes = wat::parse_str(wat).expect("parse wat");
    let source = format!(
        "{}\n{glue}",
        dewasm_test_helper::convert_bytes(&JavaBackend, &bytes, Mode::Library, "Prog")
    );

    let classdir = common::build_java(&source).unwrap_or_else(|build| {
        panic!("javac failed:\n{}", String::from_utf8_lossy(&build.stderr))
    });
    Command::new(&java)
        .arg("-cp")
        .arg(&classdir)
        .arg("Main")
        .output()
        .expect("spawn java")
}

/// `memory.grow` to 32768 pages (2^31 bytes, one past the `byte[]` cap) must return -1.
/// It must also leave the memory unchanged and still growable.
/// With no declared maximum, `maxPages` defaults to 65536.
/// So only the byte-size guard stands between the request and the overflowing allocation.
#[test]
fn grow_beyond_byte_array_cap_returns_minus_one() {
    let wat = r#"(module
      (memory 1)
      (func (export "grow") (param i32) (result i32) (memory.grow (local.get 0)))
      (func (export "size") (result i32) (memory.size)))"#;
    let glue = r#"public class Main {
    public static void main(String[] a) {
        Prog p = new Prog(null, null, null, null);
        System.out.println((int)(Integer)((Prog.Rt.Fn) p.Exports.get("grow")).invoke(new Object[]{32768}));
        System.out.println((int)(Integer)((Prog.Rt.Fn) p.Exports.get("grow")).invoke(new Object[]{1}));
        System.out.println((int)(Integer)((Prog.Rt.Fn) p.Exports.get("size")).invoke(new Object[]{}));
    }
}
"#;
    let out = convert_and_run(wat, glue);
    assert!(
        out.status.success(),
        "run failed: {}\n{}",
        out.status,
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&out.stdout), "-1\n1\n2\n");
}

/// A module declaring an *initial* memory past the `byte[]` cap cannot be instantiated on the JVM.
/// That failure must be a clear `Rt.Trap` naming the page count.
/// It must not be a `NegativeArraySizeException`.
/// That exception would escape `Main` and fail the run's exit status here.
#[test]
fn initial_memory_beyond_byte_array_cap_traps_clearly() {
    let wat = r#"(module (memory 32768) (func (export "f")))"#;
    let glue = r#"public class Main {
    public static void main(String[] a) {
        try {
            new Prog(null, null, null, null);
            System.out.println("instantiated");
        } catch (Prog.Rt.Trap e) {
            System.out.println("trap: " + e.getMessage());
        }
    }
}
"#;
    let out = convert_and_run(wat, glue);
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
