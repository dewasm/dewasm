//! The module-name policy for Perl: library names are package names taken verbatim, a nested one namespaces the embedded runtime under itself, an invalid one is a conversion-time error, and standalone output ignores the name for a fixed `Program`.

use dewasm_backend::Mode;
use dewasm_backend_perl::{find_perl, PerlBackend};

/// One exported function, enough to prove the package is reachable under its full name.
const ADD_WAT: &str = r#"(module
  (func (export "add") (param i32 i32) (result i32) (i32.add (local.get 0) (local.get 1))))"#;

dewasm_test_helper::module_name_policy_suite!(
    backend: PerlBackend,
    wat: ADD_WAT,
    invalid: ["sqlite3-shell", "", "A::", "3Add", "Add::"],
    error_contains: "invalid perl module name",
    standalone_markers: ["package Program"],
);

/// Run `source` under perl and return its stdout, failing loud on a nonzero exit.
fn run(source: &str) -> String {
    let perl = find_perl().expect("perl >= 5.26 not found on PATH — see docs/testing.md");
    let out = dewasm_test_helper::run_script(&perl, source, "pl", &[], "");
    assert!(
        out.status.success(),
        "perl failed: {}\n{}",
        out.status,
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).into_owned()
}

/// Perl package names are absolute, so the embedded runtime is prefixed with the *whole* name — the nested case is where a half-done prefixing would show up.
#[test]
fn nested_name_runs_with_a_nested_runtime() {
    let source = convert("Dewasm::Nested::Add", Mode::Library).expect("convert");
    assert!(source.contains("package Dewasm::Nested::Add"));
    assert!(source.contains("package Dewasm::Nested::Add::Rt"));
    let glue = "print Dewasm::Nested::Add->new({})->invoke('add', 2, 3), \"\\n\";\n";
    assert_eq!(run(&format!("{source}\n{glue}")), "5\n");
}
