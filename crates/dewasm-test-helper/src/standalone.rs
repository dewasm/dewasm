//! Standalone-mode scenarios over hand-written `.wat` fixtures
//! (`examples/wat/`): the compiled program *is* the runnable artifact, so no
//! host-language glue is needed and one data table is exercised by every
//! backend, checked against fixed expected output.
//!
//! Currently empty: every existing standalone fixture exercises WASI and so
//! lives in [`crate::WASI_CASES`] instead (ADR-27 groups WASI fixtures into
//! the project's own WASI p1 conformance suite). This table and the
//! `standalone_e2e!` macro remain the extension point for a future
//! non-WASI standalone artifact.

use dewasm_backend::Mode;

use crate::backend::BackendUnderTest;
use crate::fixtures::{convert, examples_dir};

pub struct StandaloneCase {
    pub name: &'static str,
    pub wat: &'static str,
    pub args: &'static [&'static str],
    pub expect_stdout: &'static str,
    pub expect_code: i32,
}

pub const STANDALONE_CASES: &[StandaloneCase] = &[];

/// Convert `case` in standalone mode, run it under `lang`, and check stdout
/// and exit code against the case's fixed expectation.
pub fn run_standalone_case(lang: &dyn BackendUnderTest, case: &StandaloneCase) {
    let code = convert(
        lang.backend(),
        &examples_dir().join(case.wat),
        Mode::Standalone,
        case.name,
    );
    let output = lang.run(&code, case.args, "");
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
