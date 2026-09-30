//! Codon side of the whole-cache convert suite.
//! It converts every cached real-world app with the Codon backend, without running it.
//! Each conversion must complete with non-empty source.
//! The generic harness lives in `dewasm-test-helper`.

use dewasm_backend_codon::CodonBackend;

dewasm_test_helper::apps_convert_suite!(CodonBackend);
