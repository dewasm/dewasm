//! Ruby side of the whole-cache convert suite: converts every cached
//! real-world app with the Ruby backend and requires the conversion to complete
//! with non-empty source, without running it. The generic harness lives in
//! `dewasm-test-helper`.

use dewasm_backend_ruby::RubyBackend;

dewasm_test_helper::apps_convert_suite!(RubyBackend);
