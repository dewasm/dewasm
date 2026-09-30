//! Perl side of the whole-cache convert suite.
//! It converts every cached real-world app with the Perl backend, without running it.
//! The conversion must complete with non-empty source.
//! The generic harness lives in `dewasm-test-helper`.

use dewasm_backend_perl::PerlBackend;

dewasm_test_helper::apps_convert_suite!(PerlBackend);
