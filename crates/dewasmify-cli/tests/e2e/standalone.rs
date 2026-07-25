//! Standalone-mode scenarios over hand-written `.wat` fixtures
//! (`examples/wat/`): the compiled program *is* the runnable artifact,
//! so no host-language glue is needed at all and one data table
//! (`STANDALONE_CASES`) is exercised by both languages, checked against
//! fixed expected output.

use dewasmify_backend::Mode;

use crate::support::{convert, examples_dir, run_lang, BashLang, E2eLang, RubyLang};

struct StandaloneCase {
    name: &'static str,
    wat: &'static str,
    args: &'static [&'static str],
    expect_stdout: &'static str,
    expect_code: i32,
}

const STANDALONE_CASES: &[StandaloneCase] = &[
    StandaloneCase {
        name: "hello",
        wat: "hello.wat",
        args: &[],
        expect_stdout: "Hello, WASI!\n",
        expect_code: 0,
    },
    // argc (program name + arguments) becomes the exit code via
    // args_sizes_get + proc_exit.
    StandaloneCase {
        name: "argc",
        wat: "args_proc_exit.wat",
        args: &["foo", "bar"],
        expect_stdout: "",
        expect_code: 3,
    },
];

fn check_standalone_case(lang: &dyn E2eLang, case: &StandaloneCase) {
    let code = convert(
        lang.backend(),
        &examples_dir().join(case.wat),
        Mode::Standalone,
        case.name,
    );
    let output = run_lang(lang, &code, case.args, "");
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        case.expect_stdout,
        "{} under {}: stdout",
        case.name,
        lang.name()
    );
    assert_eq!(
        output.status.code(),
        Some(case.expect_code),
        "{} under {}: exit code",
        case.name,
        lang.name()
    );
}

#[test]
fn standalone_cases_ruby() {
    for case in STANDALONE_CASES {
        check_standalone_case(&RubyLang, case);
    }
}

#[test]
fn standalone_cases_bash() {
    for case in STANDALONE_CASES {
        check_standalone_case(&BashLang, case);
    }
}
