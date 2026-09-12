//! The module-name policy for Codon: a library name is one identifier taken verbatim (and carried into the `<Class>Rt` runtime name), an invalid one is a conversion-time error, and standalone output ignores the name for a fixed `Program`.

use dewasm_backend::Mode;
use dewasm_backend_codon::CodonBackend;

mod common;

/// Carries a memory so the artifact actually references its runtime.
/// That is what makes the `<Class>Rt` naming observable.
const ADD_WAT: &str = r#"(module
  (memory 1)
  (func (export "add") (param i32 i32) (result i32) (i32.add (local.get 0) (local.get 1))))"#;

dewasm_test_helper::module_name_policy_suite!(
    backend: CodonBackend,
    wat: ADD_WAT,
    invalid: ["sqlite3-shell", "", "a.b", "3add"],
    error_contains: "invalid codon module name",
    standalone_markers: ["class Program"],
);

/// A name is used exactly as given (no capitalization, no case folding), the per-artifact runtime is named after it, and the boxed export surface runs.
#[test]
fn library_name_is_verbatim_and_runs() {
    let source = convert("sqlite3", Mode::Library).expect("convert");
    assert!(source.contains("class sqlite3"));
    assert!(source.contains("class sqlite3Rt"));

    let glue = "_i = sqlite3(Dict[str, Dict[str, sqlite3Rt.Extern]](), List[str](), Dict[str, str](), Dict[str, str]())\n\
                _r = _i.exports[\"add\"].fn.invoke([sqlite3Rt.Val.of_i32(UInt[32](2)), sqlite3Rt.Val.of_i32(UInt[32](3))])\n\
                print(_r[0].i32())\n";
    let program = format!("{source}\n{glue}");
    let bin = common::build_codon(&program).unwrap_or_else(|out| {
        panic!(
            "codon build failed:\n{}",
            String::from_utf8_lossy(&out.stderr)
        )
    });
    let out = common::run_codon_binary(&bin, &[], b"");
    assert!(
        out.status.success(),
        "codon program failed: {}\n{}",
        out.status,
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&out.stdout), "5\n");
}
