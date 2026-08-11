//! The module-name policy for Go output: a library artifact is a Go *package*, so its name is validated, not sanitized; a standalone artifact is a program with fixed internal names, so its bytes do not depend on the name at all.

use dewasm_backend::Mode;
use dewasm_backend_go::GoBackend;

mod common;

/// The shared two-export fixture, converted with WASI on: `library_artifact_is_importable_as_a_package` below compiles what this produces, so the fixture is the one a real embedder gets.
const ADD_WAT: &str = include_str!("../../../examples/wat/add.wat");

dewasm_test_helper::module_name_policy_suite!(
    backend: GoBackend,
    wat: ADD_WAT,
    default_wasi: true,
    invalid: ["sqlite3-shell", "ruby packed", "9lives", "my.module", "", "naïve"],
    error_contains: "invalid go module name",
    standalone_markers: ["\npackage main\n", "func NewProgram(", "func main() {"],
);

/// From a valid name, both mappings are total and fully specified: package = lowercased, type = first letter uppercased.
#[test]
fn library_package_and_type_come_from_the_module_name() {
    for (name, package, ty) in [
        ("ruby", "ruby", "Ruby"),
        ("Rg", "rg", "Rg"),
        ("MyLib", "mylib", "MyLib"),
        ("_x9", "_x9", "_x9"),
    ] {
        let src = convert(name, Mode::Library).expect("generate");
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

/// Standalone output is byte-identical however the artifact was named, including under a name library mode would reject, which is simply irrelevant here.
#[test]
fn standalone_is_byte_stable() {
    let a = convert("add", Mode::Standalone).expect("generate");
    let b = convert("somethingElse", Mode::Standalone).expect("generate");
    assert_eq!(a, b, "standalone output depends on the module name");
    let dashed = convert("sqlite3-shell", Mode::Standalone).expect("generate");
    assert_eq!(a, dashed);
}

/// The embedding shape a consumer actually uses: the artifact is imported as a package from another one, with no host code inside it. `common::build_go` is exactly that layout (temp module, `main.go` importing the package), so this drives it end to end.
#[test]
fn library_artifact_is_importable_as_a_package() {
    let src = convert("adder", Mode::Library).expect("generate");
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
