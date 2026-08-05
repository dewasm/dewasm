//! The module-name policy (ADR-63) for Python: a library name is one identifier taken verbatim (and carried into the `<Class>Rt` runtime name, ADR-62), an invalid one is a conversion-time error, and standalone output ignores the name for a fixed `Program`.

use dewasm_backend::{Backend, GenOptions, Mode, RuntimeLinkage};
use dewasm_backend_python::{find_python, PythonBackend};

/// Carries a memory so the artifact actually references its runtime — that is what makes the `<Class>Rt` naming observable.
const ADD_WAT: &str = r#"(module
  (memory 1)
  (func (export "add") (param i32 i32) (result i32) (i32.add (local.get 0) (local.get 1))))"#;

fn convert(name: &str, mode: Mode) -> anyhow::Result<String> {
    let bytes = wat::parse_str(ADD_WAT)?;
    let module = dewasm_core::build_module(&bytes)?;
    let mut files = PythonBackend.generate(
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

#[test]
fn invalid_library_names_are_rejected() {
    for name in ["sqlite3-shell", "", "a.b", "3add"] {
        let err = convert(name, Mode::Library)
            .expect_err("an invalid python module name must be a conversion error");
        let msg = format!("{err:#}");
        assert!(
            msg.contains("invalid python module name") && msg.contains("--module-name"),
            "the error must name the grammar and the flag, got: {msg}"
        );
    }
}

/// Standalone output is a self-contained program: the requested name never reaches the source.
#[test]
fn standalone_name_is_fixed() {
    let source = convert("whatever-the-stem-was", Mode::Standalone).expect("convert");
    assert!(source.contains("class Program"));
    assert!(!source.contains("whatever"));
}
