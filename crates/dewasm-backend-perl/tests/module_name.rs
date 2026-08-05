//! The module-name policy (ADR-63) for Perl: library names are package names taken verbatim, a nested one namespaces the embedded runtime under itself, an invalid one is a conversion-time error, and standalone output ignores the name for a fixed `Program`.

use dewasm_backend::{Backend, GenOptions, Mode, RuntimeLinkage};
use dewasm_backend_perl::{find_perl, PerlBackend};

/// One exported function, enough to prove the package is reachable under its full name.
const ADD_WAT: &str = r#"(module
  (func (export "add") (param i32 i32) (result i32) (i32.add (local.get 0) (local.get 1))))"#;

fn convert(name: &str, mode: Mode) -> anyhow::Result<String> {
    let bytes = wat::parse_str(ADD_WAT)?;
    let module = dewasm_core::build_module(&bytes)?;
    let mut files = PerlBackend.generate(
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

/// Run `source` under perl and return its stdout, failing loud on a nonzero exit (ADR-15).
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

#[test]
fn invalid_library_names_are_rejected() {
    for name in ["sqlite3-shell", "", "A::", "3Add", "Add::"] {
        let err = convert(name, Mode::Library)
            .expect_err("an invalid perl module name must be a conversion error");
        let msg = format!("{err:#}");
        assert!(
            msg.contains("invalid perl module name") && msg.contains("--module-name"),
            "the error must name the grammar and the flag, got: {msg}"
        );
    }
}

/// Standalone output is a self-contained program: the requested name never reaches the source.
#[test]
fn standalone_name_is_fixed() {
    let source = convert("whatever-the-stem-was", Mode::Standalone).expect("convert");
    assert!(source.contains("package Program"));
    assert!(!source.contains("whatever"));
}
