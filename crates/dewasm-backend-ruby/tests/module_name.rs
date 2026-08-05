//! The module-name policy (ADR-63) for Ruby: library names are Ruby constant paths taken verbatim, a nested one defines its ancestors under a guard, an invalid one is a conversion-time error, and standalone output ignores the name for a fixed `Program`.

use dewasm_backend::{Backend, GenOptions, Mode, RuntimeLinkage};
use dewasm_backend_ruby::{find_ruby, RubyBackend};

/// One exported function, enough to prove the class is reachable under its full path.
const ADD_WAT: &str = r#"(module
  (func (export "add") (param i32 i32) (result i32) (i32.add (local.get 0) (local.get 1))))"#;

fn convert(name: &str, mode: Mode) -> anyhow::Result<String> {
    let bytes = wat::parse_str(ADD_WAT)?;
    let module = dewasm_core::build_module(&bytes)?;
    let mut files = RubyBackend.generate(
        &module,
        &GenOptions {
            mode,
            module_name: name.to_string(),
            runtime: RuntimeLinkage::Embedded,
            default_wasi: false,
            data_file: None,
        },
    )?;
    Ok(String::from_utf8(files.remove(0).contents)?)
}

/// Run `source` under ruby and return its stdout, failing loud on a nonzero exit (ADR-15).
fn run(source: &str) -> String {
    let ruby = find_ruby().expect("ruby >= 3.4 not found on PATH — see docs/testing.md");
    let out = dewasm_test_helper::run_script(&ruby, source, "rb", &[], "");
    assert!(
        out.status.success(),
        "ruby failed: {}\n{}",
        out.status,
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).into_owned()
}

#[test]
fn nested_name_defines_its_ancestors_and_runs() {
    let source = convert("Dewasm::Nested::Add", Mode::Library).expect("convert");
    assert!(source.contains("unless defined?(Dewasm)"));
    assert!(source.contains("unless defined?(Dewasm::Nested)"));
    assert!(source.contains("class Dewasm::Nested::Add"));
    let glue = "print Dewasm::Nested::Add.new.invoke(\"add\", 2, 3), \"\\n\"\n";
    assert_eq!(run(&format!("{source}\n{glue}")), "5\n");
}

/// An ancestor that already exists — as a *class*, which `module Dewasm; end` could not reopen — is left alone by the guard, so the generated file loads next to it.
#[test]
fn preexisting_class_ancestor_is_not_redefined() {
    let source = convert("Dewasm::Nested::Add", Mode::Library).expect("convert");
    let program = format!(
        "class Dewasm\n  def self.marker = \"host class kept\"\nend\n\
         {source}\n\
         print Dewasm.marker, \"\\n\"\n\
         print Dewasm::Nested::Add.new.invoke(\"add\", 2, 3), \"\\n\"\n"
    );
    assert_eq!(run(&program), "host class kept\n5\n");
}

/// A single-segment name emits no guards at all: the output is what it always was.
#[test]
fn single_segment_name_emits_no_guards() {
    let source = convert("Add", Mode::Library).expect("convert");
    assert!(!source.contains("unless defined?"));
    assert!(source.contains("class Add\n"));
}

#[test]
fn invalid_library_names_are_rejected() {
    for name in ["add", "sqlite3-shell", "", "A::", "_Add", "3Add", "A::b"] {
        let err = convert(name, Mode::Library)
            .expect_err("an invalid ruby module name must be a conversion error");
        let msg = format!("{err:#}");
        assert!(
            msg.contains("invalid ruby module name") && msg.contains("--module-name"),
            "the error must name the grammar and the flag, got: {msg}"
        );
    }
}

/// Standalone output is a self-contained program: the requested name never reaches the source.
#[test]
fn standalone_name_is_fixed() {
    let source = convert("whatever-the-stem-was", Mode::Standalone).expect("convert");
    assert!(source.contains("class Program\n"));
    assert!(!source.contains("whatever"));
}
