//! The module-name policy for Go output: a library artifact is a Go *package*, so its name is validated, not sanitized; a standalone artifact is a program with fixed internal names, so its bytes do not depend on the name at all.

use dewasm_backend::{Backend, GenOptions, Mode, RuntimeLinkage};
use dewasm_backend_go::GoBackend;

mod common;

fn generate(mode: Mode, module_name: &str) -> anyhow::Result<String> {
    let bytes = wat::parse_file(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/wat/add.wat"),
    )?;
    let module = dewasm_core::build_module(&bytes)?;
    let files = GoBackend.generate(
        &module,
        &GenOptions {
            mode,
            module_name: module_name.to_string(),
            runtime: RuntimeLinkage::Embedded,
            default_wasi: true,
            data_file: None,
        },
    )?;
    Ok(String::from_utf8(files.into_iter().next().unwrap().contents).unwrap())
}

/// A library name that is not a Go identifier fails at conversion time (ADR-0), naming both the grammar and the offending value. Nothing is dropped, folded or prefixed to make it fit.
#[test]
fn library_rejects_a_non_identifier_module_name() {
    for name in [
        "sqlite3-shell",
        "ruby packed",
        "9lives",
        "my.module",
        "",
        "naïve",
    ] {
        let err = generate(Mode::Library, name).expect_err("expected a rejection");
        let msg = format!("{err:#}");
        assert!(
            msg.contains("[A-Za-z_][A-Za-z0-9_]*") && msg.contains(&format!("{name:?}")),
            "error for {name:?} names neither the grammar nor the value: {msg}"
        );
    }
}

/// From a valid name, both mappings are total and fully specified: package = lowercased, type = first letter uppercased.
#[test]
fn library_package_and_type_come_from_the_module_name() {
    for (name, package, ty) in [
        ("ruby", "ruby", "Ruby"),
        ("Rg", "rg", "Rg"),
        ("MyLib", "mylib", "MyLib"),
        ("_x9", "_x9", "_x9"),
    ] {
        let src = generate(Mode::Library, name).expect("generate");
        assert!(
            src.contains(&format!("\npackage {package}\n")),
            "{name:?} did not yield `package {package}`"
        );
        assert!(
            src.contains(&format!("func New{ty}(")),
            "{name:?} did not yield the type {ty}"
        );
    }
}

/// Standalone output is `package main` with the fixed type `Program`, byte-identical however the artifact was named — the name reaches nothing an outsider can see.
#[test]
fn standalone_is_byte_stable_and_fixed_shape() {
    let a = generate(Mode::Standalone, "add").expect("generate");
    let b = generate(Mode::Standalone, "somethingElse").expect("generate");
    assert_eq!(a, b, "standalone output depends on the module name");
    assert!(a.contains("\npackage main\n"));
    assert!(a.contains("func NewProgram("));
    assert!(a.contains("func main() {"));
    // A name that library mode would reject is simply irrelevant here.
    let dashed = generate(Mode::Standalone, "sqlite3-shell").expect("generate");
    assert_eq!(a, dashed);
}

/// The embedding shape a consumer actually uses: the artifact is imported as a package from another one, with no host code inside it. `common::build_go` is exactly that layout (temp module, `main.go` importing the package), so this drives it end to end.
#[test]
fn library_artifact_is_importable_as_a_package() {
    let src = generate(Mode::Library, "adder").expect("generate");
    let glue = r#"func RunTest() {
	inst := NewAdder(nil, nil, nil, nil)
	fmt.Println(inst.Exports["add"].(func(uint32, uint32) uint32)(2, 3))
}
"#;
    let bin = common::build_go(&format!("{src}\n{glue}")).unwrap_or_else(|build| {
        panic!(
            "go build failed:\n{}",
            String::from_utf8_lossy(&build.stderr)
        )
    });
    let out = std::process::Command::new(&bin)
        .output()
        .expect("run the built program");
    assert!(out.status.success(), "{:?}", out.status);
    assert_eq!(String::from_utf8_lossy(&out.stdout), "5\n");
}
