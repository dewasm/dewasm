//! End-to-end tests: convert .wat examples to Ruby and run them with the
//! real interpreter.

use std::path::Path;
use std::process::Command;

use dewasmify_backend::{Backend, GenOptions, Mode};
use dewasmify_backend_ruby::RubyBackend;

fn ruby_available() -> bool {
    Command::new("ruby").arg("--version").output().is_ok()
}

fn convert(wat_path: &Path, mode: Mode, name: &str) -> String {
    let bytes = wat::parse_file(wat_path).expect("parse wat");
    let module = dewasmify_core::build_module(&bytes).expect("build IR");
    let files = RubyBackend
        .generate(&module, &GenOptions { mode, module_name: name.to_string() })
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
