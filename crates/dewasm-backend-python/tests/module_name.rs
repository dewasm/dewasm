//! The module-name policy for Python: a library name is one identifier taken verbatim (and carried into the `<Class>Rt` runtime name), an invalid one is a conversion-time error, and standalone output ignores the name for a fixed `Program`.

use dewasm_backend::Mode;
use dewasm_backend_python::{find_python, PythonBackend};

/// Carries a memory so the artifact actually references its runtime — that is what makes the `<Class>Rt` naming observable.
const ADD_WAT: &str = r#"(module
  (memory 1)
  (func (export "add") (param i32 i32) (result i32) (i32.add (local.get 0) (local.get 1))))"#;

dewasm_test_helper::module_name_policy_suite!(
    backend: PythonBackend,
    wat: ADD_WAT,
    invalid: ["sqlite3-shell", "", "a.b", "3add"],
    error_contains: "invalid python module name",
    standalone_markers: ["class Program"],
);

/// A name is used exactly as given — no capitalization, no case folding — and the per-artifact runtime is named after it.
#[test]
fn library_name_is_verbatim_and_runs() {
    let source = convert("sqlite3", Mode::Library).expect("convert");
    assert!(source.contains("class sqlite3"));
    assert!(source.contains("class sqlite3Rt"));

    let python = find_python().expect("python3 not found on PATH — see docs/testing.md");
    let glue = "print(sqlite3().invoke(\"add\", 2, 3))\n";
    let out = dewasm_test_helper::run_script(&python, &format!("{source}\n{glue}"), "py", &[], "");
    assert!(
        out.status.success(),
        "python failed: {}\n{}",
        out.status,
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&out.stdout), "5\n");
}
