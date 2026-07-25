//! Standalone-mode scenarios over hand-written `.wat` fixtures
//! (`examples/wat/`): the compiled program *is* the runnable artifact,
//! so no host-language glue is needed at all and one data table
//! (`STANDALONE_CASES`) is exercised by both languages, checked against
//! fixed expected output. Every case here only needs Tier 3 (WASI core,
//! ADR-23); a case requiring more would be skipped for a backend that
//! hasn't reached it.

use dewasmify_backend::{Mode, Tier};

use crate::support::{
    convert, examples_dir, print_tier_skip, run_lang, tier_ok, BashLang, E2eLang, RubyLang,
};

struct StandaloneCase {
    name: &'static str,
    wat: &'static str,
    args: &'static [&'static str],
    expect_stdout: &'static str,
    expect_code: i32,
    tier: Tier,
}

const STANDALONE_CASES: &[StandaloneCase] = &[
    StandaloneCase {
        name: "hello",
        wat: "hello.wat",
        args: &[],
        expect_stdout: "Hello, WASI!\n",
        expect_code: 0,
        tier: Tier::Tier3,
    },
    // argc (program name + arguments) becomes the exit code via
    // args_sizes_get + proc_exit.
    StandaloneCase {
        name: "argc",
        wat: "args_proc_exit.wat",
        args: &["foo", "bar"],
        expect_stdout: "",
        expect_code: 3,
        tier: Tier::Tier3,
    },
];

fn check_standalone_case(lang: &dyn E2eLang, case: &StandaloneCase) {
    if !tier_ok(lang, case.tier) {
        print_tier_skip(case.name, lang, case.tier);
        return;
    }
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
        check_standalone_case(&RubyLang, case);
    }
}

#[test]
fn standalone_cases_bash() {
    for case in STANDALONE_CASES {
        check_standalone_case(&BashLang, case);
    }
}
