//! Standalone-mode scenarios over hand-written `.wat` fixtures
//! (`examples/wat/`): the compiled program *is* the runnable artifact,
//! so no host-language glue is needed at all and one data table
//! (`STANDALONE_CASES`) is exercised by both languages, checked against
//! fixed expected output.

use std::process::Output;

use dewasmify_backend::{Backend, Mode};
use dewasmify_backend_bash::{find_bash5, BashBackend};
use dewasmify_backend_ruby::RubyBackend;

use crate::support::{convert, examples_dir, run_bash, run_ruby_capture};

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

fn check_standalone_case(
    backend: &dyn Backend,
    case: &StandaloneCase,
    run: impl Fn(&str, &[&str]) -> Output,
) {
    let code = convert(
        backend,
        &examples_dir().join(case.wat),
        Mode::Standalone,
        case.name,
    );
    let output = run(&code, case.args);
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        case.expect_stdout,
        "{}: stdout",
        case.name
    );
    assert_eq!(
        output.status.code(),
        Some(case.expect_code),
        "{}: exit code",
        case.name
    );
}

#[test]
fn standalone_cases_ruby() {
    for case in STANDALONE_CASES {
        check_standalone_case(&RubyBackend, case, run_ruby_capture);
    }
}

#[test]
fn standalone_cases_bash() {
    let bash = find_bash5().expect("bash >= 5 not found — see docs/testing.md");
    for case in STANDALONE_CASES {
        check_standalone_case(&BashBackend, case, |code, args| run_bash(&bash, code, args));
    }
}
