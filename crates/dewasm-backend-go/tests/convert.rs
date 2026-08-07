//! Go side of the whole-cache convert suite: converts every cached
//! real-world app with the Go backend and requires the conversion to complete
//! with non-empty source, without compiling or running it. The generic harness
//! lives in `dewasm-test-helper`.

use dewasm_backend_go::GoBackend;

dewasm_test_helper::apps_convert_suite!(GoBackend);
