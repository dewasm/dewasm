//! The module-name policy (ADR-63) for Bash: a library name is one identifier, lowercased into the global function/variable prefix — the single deliberate mapping the policy keeps, because bash has no case-carrying namespace. An invalid name is a conversion-time error; standalone output uses the fixed `program_`.

use dewasm_backend::{Backend, GenOptions, Mode, RuntimeLinkage};
use dewasm_backend_bash::{find_bash5, BashBackend};

const ADD_WAT: &str = r#"(module
  (func (export "add") (param i32 i32) (result i32) (i32.add (local.get 0) (local.get 1))))"#;

fn convert(name: &str, mode: Mode) -> anyhow::Result<String> {
    let bytes = wat::parse_str(ADD_WAT)?;
    let module = dewasm_core::build_module(&bytes)?;
    let mut files = BashBackend.generate(
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

/// The prefix is the name lowercased plus `_` — a total mapping, stated rather than guessed.
#[test]
fn prefix_is_the_lowercased_name_and_runs() {
    assert_eq!(
        dewasm_backend_bash::func_prefix("Sqlite3Shell"),
        "sqlite3shell_"
    );
    let source = convert("Sqlite3Shell", Mode::Library).expect("convert");
    assert!(source.contains("sqlite3shell_init"));

    let bash = find_bash5().expect("bash >= 5 not found — see docs/testing.md");
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

#[test]
fn invalid_library_names_are_rejected() {
    for name in ["sqlite3-shell", "", "a.b", "3add"] {
        let err = convert(name, Mode::Library)
            .expect_err("an invalid bash module name must be a conversion error");
        let msg = format!("{err:#}");
        assert!(
            msg.contains("invalid bash module name") && msg.contains("--module-name"),
            "the error must name the grammar and the flag, got: {msg}"
        );
    }
}

/// Standalone output is a self-contained program: the requested name never reaches the source.
#[test]
fn standalone_prefix_is_fixed() {
    let source = convert("whatever-the-stem-was", Mode::Standalone).expect("convert");
    assert!(source.contains("program_init"));
    assert!(!source.contains("whatever"));
}
