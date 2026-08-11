//! The module-name policy for Ruby: library names are Ruby constant paths taken verbatim, a nested one defines its ancestors under a guard, an invalid one is a conversion-time error, and standalone output ignores the name for a fixed `Program`.

use dewasm_backend::Mode;
use dewasm_backend_ruby::{find_ruby, RubyBackend};

/// One exported function, enough to prove the class is reachable under its full path.
const ADD_WAT: &str = r#"(module
  (func (export "add") (param i32 i32) (result i32) (i32.add (local.get 0) (local.get 1))))"#;

dewasm_test_helper::module_name_policy_suite!(
    backend: RubyBackend,
    wat: ADD_WAT,
    invalid: ["add", "sqlite3-shell", "", "A::", "_Add", "3Add", "A::b"],
    error_contains: "invalid ruby module name",
    standalone_markers: ["class Program\n"],
);

/// Run `source` under ruby and return its stdout, failing loud on a nonzero exit.
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
