//! Perl side of the whole-cache convert suite (ADR-54): converts every cached
//! real-world app with the Perl backend and requires the conversion to complete
//! with non-empty source, without running it. The generic harness lives in
//! `dewasm-test-helper`.

use dewasm_backend_perl::PerlBackend;
use dewasm_test_helper::apps_convert_suite;

apps_convert_suite!(PerlBackend);
