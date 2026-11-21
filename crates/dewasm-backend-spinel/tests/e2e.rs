//! Spinel end-to-end suites: the shared case constants (`dewasm-test-helper`) set up for Spinel.
//! This file holds ONLY:
//! - the [`BackendUnderTest`] implementation;
//! - per-case macro calls.
//!
//! Every case here is a standalone program, built by `spinel` and run as a native executable.
//! Which macros this file calls is the capability declaration.
//! A macro left uncalled carries a REASON comment.

use std::process::Output;

use dewasm_backend::Backend;
use dewasm_backend_spinel::SpinelBackend;
use dewasm_test_helper::BackendUnderTest;

mod common;

struct Spinel;

impl BackendUnderTest for Spinel {
    fn name(&self) -> &'static str {
        "spinel"
    }

    fn backend(&self) -> &'static (dyn Backend + Sync) {
        &SpinelBackend
    }

    fn run_bytes(&self, source: &str, args: &[&str], stdin: &[u8]) -> Output {
        common::build_and_run(source, args, stdin)
    }
}

// The library-mode cases are not called.
// Their glue is a second Ruby file that requires the converted one.
// The Ruby backend runs the same glue shape, and the i64 dialect does not reach it.
// The specification harness covers this backend's library output.

dewasm_test_helper::wasi_suite!(Spinel, Stdio);
dewasm_test_helper::wasi_suite!(Spinel, ArgsEnv);
dewasm_test_helper::wasi_suite!(Spinel, Poll);
dewasm_test_helper::standalone_dir_e2e!(Spinel);
dewasm_test_helper::standalone_env_e2e!(Spinel);
dewasm_test_helper::deep_recursion_e2e!(Spinel);
dewasm_test_helper::folded_temp_reuse_e2e!(Spinel);

// The category criterion is the build cost of the case's own artifact, as for Codon.
// `cowsay` builds in seconds, and `mruby` and `minigzip` in about a minute.
dewasm_test_helper::cowsay_args_e2e!(Spinel);
dewasm_test_helper::cowsay_stdin_e2e!(Spinel);
dewasm_test_helper::mruby_eh_e2e!(Spinel, slow);
dewasm_test_helper::gzip_e2e!(Spinel, slow);
// The `ultra` category: each of these builds takes three minutes or more.
// Both run at `slow` on the Ruby backend.
dewasm_test_helper::qjs_eval_e2e!(Spinel, ultra);
dewasm_test_helper::sqlite3_shell_e2e!(Spinel, ultra);
