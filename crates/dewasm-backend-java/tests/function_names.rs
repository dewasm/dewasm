//! `GenOptions::function_names`: a generated method carries the module's own name after its index.
//! The output still compiles and runs.

use std::process::Command;

use dewasm_backend::{Backend, GenOptions, Mode, RuntimeLinkage};
use dewasm_backend_java::{find_java, javac_command, JavaBackend};

/// Two named functions, one calling the other; a name with characters Java cannot spell.
const WAT: &str = r#"(module
  (func $twice (param i32) (result i32) (i32.mul (local.get 0) (i32.const 2)))
  (func $add::twice (export "add_twice") (param i32 i32) (result i32)
    (call $twice (i32.add (local.get 0) (local.get 1)))))"#;

fn convert(function_names: bool) -> String {
    let bytes = wat::parse_str(WAT).expect("wat");
    let module = dewasm_core::build_module(&bytes).expect("module");
    let mut files = JavaBackend
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
    assert!(source.contains(" f0("), "{source}");
    assert!(!source.contains("f0_twice"));
}

#[test]
fn on_appends_the_sanitized_name_and_runs() {
    let source = convert(true);
    assert!(source.contains(" f0_twice("), "{source}");
    assert!(source.contains(" f1_add__twice("), "{source}");
    assert!(
        source.contains("f0_twice("),
        "the call site uses the same name"
    );

    let program = format!(
        "{source}\npublic class Main {{\n    \
         public static void main(String[] a) {{\n        \
         Named inst = new Named(null, null, null, null);\n        \
         System.out.println((int)(Integer)((Named.Rt.Fn) inst.Exports.get(\"add_twice\")).invoke(new Object[]{{2, 3}}));\n    \
         }}\n}}\n"
    );
    let dir =
        std::env::temp_dir().join(format!("dewasm-java-function-names-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let src = dir.join("Main.java");
    std::fs::write(&src, &program).unwrap();
    let build = javac_command()
        .arg("-d")
        .arg(&dir)
        .arg(&src)
        .output()
        .expect("spawn javac");
    assert!(
        build.status.success(),
        "javac failed:\n{}",
        String::from_utf8_lossy(&build.stderr)
    );

    let java = find_java().expect("java not found on PATH (or $DEWASM_JAVA): see docs/testing.md");
    let out = Command::new(&java)
        .arg("-cp")
        .arg(&dir)
        .arg("Main")
        .output()
        .expect("spawn java");
    assert!(
        out.status.success(),
        "java failed: {}\n{}",
        out.status,
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&out.stdout), "10\n");
    let _ = std::fs::remove_dir_all(&dir);
}
