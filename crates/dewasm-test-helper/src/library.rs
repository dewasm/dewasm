//! Library-mode scenarios over hand-written `.wat` fixtures
//! (`examples/wat/`). These need per-language glue (Ruby method calls vs.
//! Bash function calls against globals) — that can't be shared, so glue is
//! *not* in this table (ADR-27); each backend crate passes a glue resolver
//! to the runner. Each glue is engineered to observe the same thing the same
//! way (e.g. both intercept fd_write and print the literal bytes written),
//! so one `expect` per scenario is pinned instead of one per language that
//! could quietly drift apart.

use dewasm_backend::Mode;

use crate::backend::BackendUnderTest;
use crate::fixtures::{convert, examples_dir};

pub struct LibraryCase {
    pub name: &'static str,
    pub wat: &'static str,
    pub module_name: &'static str,
    /// Both sides are engineered to produce this same string — the glue
    /// captures and prints the actual bytes the wasm module wrote, rather
    /// than a language-specific diagnostic, so there is exactly one
    /// expectation per scenario.
    pub expect: &'static str,
}

pub const LIBRARY_CASES: &[LibraryCase] = &[
    LibraryCase {
        name: "add",
        wat: "add.wat",
        module_name: "add",
        expect: "5\n0\n55\n",
    },
    // The ADR-7 override/fallback semantics: an explicit import wins, an
    // unhandled one falls back to the bundled WASI. Both glues intercept
    // fd_write and print the actual bytes the module wrote (rather than a
    // fd/len diagnostic) — the one observable both languages can produce
    // identically. Both sides only touch fd_write/random_get (WASI core).
    LibraryCase {
        name: "wasi_import_override",
        wat: "wasi_imports.wat",
        module_name: "prog",
        expect: "ok\n",
    },
];

/// Resolve a case's per-language glue. A backend's resolver panics loudly if
/// a case it is wired to run has no glue (the ADR-15 fail-loud discipline the
/// old `glue_for` enforced).
pub type GlueResolver = fn(&LibraryCase) -> &'static str;

/// Convert `case` in library mode, append `lang`'s glue for it, run it, and
/// check stdout against the case's fixed expectation. A non-zero exit is a
/// failure regardless of stdout.
pub fn run_library_case(lang: &dyn BackendUnderTest, case: &LibraryCase, glue: GlueResolver) {
    let code = convert(
        lang.backend(),
        &examples_dir().join(case.wat),
        Mode::Library,
        case.module_name,
    );
    let output = lang.run(&format!("{code}\n{}", glue(case)), &[], "");
    assert!(
        output.status.success(),
        "{}: {} failed: {}\n{}",
        case.name,
        lang.name(),
        output.status,
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        case.expect,
        "{}: {} stdout",
        case.name,
        lang.name()
    );
}
