//! Core expression-folding semantics observed through the generated code of every backend.
//! The folding lives in `dewasm-core`, and its IR shapes are asserted there.
//! These cases run the result, because a mis-folded operand is only visible once the code executes.

use dewasm_backend::Mode;

use crate::backend::BackendUnderTest;
use crate::fixtures::{convert, examples_dir};

/// Run the folded-temp-reuse case (`folded_temp_reuse_e2e!`).
/// It converts `folded_temp_reuse.wat` in *standalone* mode and runs it.
/// The fixture computes two pure-arithmetic expressions whose operands sit in temp slots.
/// A later call result or spill reuses those slots.
/// The fixture checks both against their constant results and reports through `proc_exit`.
/// So the exit code alone says which one was clobbered (1 or 2; 42 means both are right).
/// No glue: the fixture checks itself.
pub fn run_folded_temp_reuse(lang: &dyn BackendUnderTest) {
    let src = convert(
        lang.backend(),
        &examples_dir().join("folded_temp_reuse.wat"),
        Mode::Standalone,
        "folded_temp_reuse",
    );
    let output = lang.run(&src, &[], "");
    assert_eq!(
        output.status.code(),
        Some(42),
        "folded_temp_reuse under {}: exit code (1 = the call-result shape, 2 = the spill shape)\n{}",
        lang.name(),
        String::from_utf8_lossy(&output.stderr)
    );
}
