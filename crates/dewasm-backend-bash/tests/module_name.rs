//! The module-name policy for Bash: a library name is one identifier, lowercased into the global function/variable prefix, the single deliberate mapping the policy keeps, because bash has no case-carrying namespace.
//! An invalid name is a conversion-time error; standalone output uses the fixed `program_`.

use dewasm_backend::Mode;
use dewasm_backend_bash::{find_bash5, BashBackend};

const ADD_WAT: &str = r#"(module
  (func (export "add") (param i32 i32) (result i32) (i32.add (local.get 0) (local.get 1))))"#;

dewasm_test_helper::module_name_policy_suite!(
    backend: BashBackend,
    wat: ADD_WAT,
    invalid: ["sqlite3-shell", "", "a.b", "3add"],
    error_contains: "invalid bash module name",
    standalone_markers: ["program_init"],
);

/// The prefix is the name lowercased plus `_`: a total mapping, stated rather than guessed.
#[test]
fn prefix_is_the_lowercased_name_and_runs() {
    assert_eq!(
        dewasm_backend_bash::func_prefix("Sqlite3Shell"),
        "sqlite3shell_"
    );
    let source = convert("Sqlite3Shell", Mode::Library).expect("convert");
    assert!(source.contains("sqlite3shell_init"));

    let bash = find_bash5().expect("bash >= 5 not found: see docs/testing.md");
    let glue = "sqlite3shell_init || exit 1\nsqlite3shell_invoke add 2 3\necho \"$R0\"\n";
    let out = dewasm_test_helper::run_script(&bash, &format!("{source}\n{glue}"), "sh", &[], "");
    assert!(
        out.status.success(),
        "bash failed: {}\n{}",
        out.status,
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&out.stdout), "5\n");
}
