//! Java side of the whole-cache convert suite: converts every cached
//! real-world app with the Java backend and requires the conversion to complete
//! with non-empty source, without compiling or running it. The generic harness
//! lives in `dewasm-test-helper`.

use dewasm_backend_java::JavaBackend;

dewasm_test_helper::apps_convert_suite!(JavaBackend);
