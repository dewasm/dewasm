//! Java side of the whole-cache convert suite.
//! It converts every cached real-world app with the Java backend.
//! Each conversion must complete with non-empty source; the suite neither compiles nor runs it.
//! The generic harness lives in `dewasm-test-helper`.

use dewasm_backend_java::JavaBackend;

dewasm_test_helper::apps_convert_suite!(JavaBackend);
