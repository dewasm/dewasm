//! Go side of the whole-cache convert suite.
//! It converts every cached real-world app with the Go backend, without compiling or running it.
//! The conversion must complete with non-empty source.
//! The generic harness lives in `dewasm-test-helper`.

use dewasm_backend_go::GoBackend;

dewasm_test_helper::apps_convert_suite!(GoBackend);
