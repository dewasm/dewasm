//! Bash side of the whole-cache convert suite: converts every cached real-world app with the Bash backend and requires the conversion to complete with non-empty source, without running it.
//! The generic harness lives in
//! `dewasm-test-helper`.

use dewasm_backend_bash::BashBackend;

dewasm_test_helper::apps_convert_suite!(BashBackend);
