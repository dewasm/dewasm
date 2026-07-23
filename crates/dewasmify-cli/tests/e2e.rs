//! End-to-end tests: convert .wat examples to Ruby and run them with the
//! real interpreter.

use std::path::Path;
use std::process::Command;

use dewasmify_backend::{Backend, GenOptions, Mode, RuntimeLinkage};
use dewasmify_backend_ruby::RubyBackend;

fn ruby_available() -> bool {
    Command::new("ruby").arg("--version").output().is_ok()
}

fn convert(wat_path: &Path, mode: Mode, name: &str) -> String {
    let bytes = wat::parse_file(wat_path).expect("parse wat");
    let module = dewasmify_core::build_module(&bytes).expect("build IR");
    let files = RubyBackend
        .generate(
            &module,
            &GenOptions {
                mode,
                module_name: name.to_string(),
                runtime: RuntimeLinkage::Embedded,
            },
        )
        .expect("generate ruby");
    files.into_iter().next().unwrap().contents
}

fn examples_dir() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/wat")
}

#[test]
fn library_mode_add() {
    if !ruby_available() {
        eprintln!("ruby not found; skipping");
        return;
    }
    let code = convert(&examples_dir().join("add.wat"), Mode::Library, "add");
    let script = format!(
        "{code}\ninst = Add.new\nprint inst.invoke(\"add\", 2, 3), \"\\n\"\nprint inst.invoke(\"add\", 0xffffffff, 1), \"\\n\"\nprint inst.invoke(\"fib\", 10), \"\\n\""
    );
    let out = run_ruby(&script, &[]);
    assert_eq!(out, "5\n0\n55\n");
}

#[test]
fn standalone_mode_wasi_hello() {
    if !ruby_available() {
        eprintln!("ruby not found; skipping");
        return;
    }
    let code = convert(&examples_dir().join("hello.wat"), Mode::Standalone, "hello");
    let out = run_ruby(&code, &[]);
    assert_eq!(out, "Hello, WASI!\n");
}

/// Two Embedded-linkage artifacts must coexist in one process: each
/// carries its own nested `Rt`, so runtime classes (and even different
/// runtime versions, eventually) never collide.
#[test]
fn embedded_runtimes_coexist() {
    if !ruby_available() {
        eprintln!("ruby not found; skipping");
        return;
    }
    let wat = r#"
        (module
          (memory 1)
          (func (export "div") (param i32 i32) (result i32)
            local.get 0
            local.get 1
            i32.div_s))
    "#;
    let bytes = wat::parse_str(wat).expect("parse wat");
    let module = dewasmify_core::build_module(&bytes).expect("build IR");
    let gen = |name: &str| {
        RubyBackend
            .generate(
                &module,
                &GenOptions {
                    mode: Mode::Library,
                    module_name: name.to_string(),
                    runtime: RuntimeLinkage::Embedded,
                },
            )
            .expect("generate ruby")
            .remove(0)
            .contents
    };
    let script = format!(
        "{}\n{}\n{}",
        gen("alpha"),
        gen("beta"),
        r#"
a = Alpha.new
b = Beta.new
print a.invoke("div", 7, 2), "\n"
print b.invoke("div", 0xfffffff9, 2), "\n"
print (Alpha::Rt::Trap != Beta::Rt::Trap), "\n"
begin
  a.invoke("div", 1, 0)
rescue Alpha::Rt::Trap => e
  print "trap: ", e.message, "\n"
end
"#
    );
    let out = run_ruby(&script, &[]);
    assert_eq!(out, "3\n4294967293\ntrue\ntrap: integer divide by zero\n");
}

fn run_ruby(script: &str, args: &[&str]) -> String {
    let path = std::env::temp_dir().join(format!(
        "dewasmify-e2e-{}.rb",
        std::process::id() as u64 + script.len() as u64
    ));
    std::fs::write(&path, script).unwrap();
    let output = Command::new("ruby").arg(&path).args(args).output().expect("run ruby");
    assert!(
        output.status.success(),
        "ruby failed: {}\n{}",
        output.status,
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).into_owned()
}
