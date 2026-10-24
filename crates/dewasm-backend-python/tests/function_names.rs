//! `GenOptions::function_names`: a generated method carries the module's own name after its index.
//! The output still runs.

use dewasm_backend::{Backend, GenOptions, Mode, RuntimeLinkage};
use dewasm_backend_python::{find_python, PythonBackend};

/// Two named functions, one calling the other; a name with characters Python cannot spell.
const WAT: &str = r#"(module
  (func $twice (param i32) (result i32) (i32.mul (local.get 0) (i32.const 2)))
  (func $add::twice (export "add_twice") (param i32 i32) (result i32)
    (call $twice (i32.add (local.get 0) (local.get 1)))))"#;

fn convert(function_names: bool) -> String {
    let bytes = wat::parse_str(WAT).expect("wat");
    let module = dewasm_core::build_module(&bytes).expect("module");
    let mut files = PythonBackend
        .generate(
            &module,
            &GenOptions {
                mode: Mode::Library,
                module_name: "Named".to_string(),
                runtime: RuntimeLinkage::Embedded,
                default_wasi: true,
                data_file: None,
                function_names,
            },
        )
        .expect("generate");
    String::from_utf8(files.remove(0).contents).expect("utf-8")
}

#[test]
fn off_by_default_keeps_the_index_alone() {
    let source = convert(false);
    assert!(source.contains("def _f0("));
    assert!(!source.contains("_f0_twice"));
}

#[test]
fn on_appends_the_sanitized_name_and_runs() {
    let source = convert(true);
    assert!(source.contains("def _f0_twice("), "{source}");
    assert!(source.contains("def _f1_add__twice("), "{source}");
    assert!(
        source.contains("self._f0_twice("),
        "the call site uses the same name"
    );
    let python = find_python().expect("python3 not found on PATH: see docs/testing.md");
    let glue = "print(Named().invoke(\"add_twice\", 2, 3))\n";
    let out = dewasm_test_helper::run_script(&python, &format!("{source}\n{glue}"), "py", &[], "");
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&out.stdout), "10\n");
}
