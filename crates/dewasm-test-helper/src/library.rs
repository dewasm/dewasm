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
    /// Backends excluded from this case, each `(lang name, human reason)`
    /// (ADR-27 revision) — a data-level capability exclusion the runner prints
    /// and honors, rather than a bespoke per-backend test. Empty means every
    /// backend wired to `library_e2e!` runs it.
    pub exclude: &'static [(&'static str, &'static str)],
}

/// Backends whose bundled WASI is eagerly constructed and which have no
/// duck-typed provider-object import form, so the "was the bundled WASI
/// constructed?" observable (ADR-7's lazy `@wasi`) cannot be probed: Go and
/// Java build `WASI` in the module ctor (ADR-29/30); Bash has no host-language
/// object model at all (ADR-12).
const NO_LAZY_WASI: &[(&str, &str)] = &[
    (
        "go",
        "bundled WASI is eagerly constructed in the ctor and there is no \
         provider-object import form (ADR-29), so the lazy-construction \
         observable cannot hold",
    ),
    (
        "java",
        "bundled WASI is eagerly constructed in the ctor and there is no \
         provider-object import form (ADR-30), so the lazy-construction \
         observable cannot hold",
    ),
    (
        "bash",
        "no host-language object model to replace WASI wholesale or probe its \
         construction (ADR-12)",
    ),
];

pub const LIBRARY_CASES: &[LibraryCase] = &[
    LibraryCase {
        name: "add",
        wat: "add.wat",
        module_name: "add",
        expect: "5\n0\n55\n",
        exclude: &[],
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
        exclude: &[],
    },
    // A provider *object* (ADR-7) replaces the bundled WASI wholesale:
    // `import`/`wasm_import` resolves every function, `attach` binds the
    // memory, and — because every import is covered — the bundled WASI is
    // never lazily constructed. The glue prints the intercepted bytes plus the
    // "not constructed" observable. Ruby (duck-typed object) and Python
    // (`wasm_import`/`attach` provider + lazy `_wasi`) express it; Go/Java/Bash
    // are excluded (see NO_LAZY_WASI).
    LibraryCase {
        name: "custom_wasi_provider",
        wat: "wasi_imports.wat",
        module_name: "prog",
        expect: "ok\nbundled wasi constructed: false\n",
        exclude: NO_LAZY_WASI,
    },
    // The `true` counterpart: a *partial* override (fd_write only) still lets
    // the bundled WASI be lazily constructed for the one import it doesn't
    // cover (random_get) — ADR-7's `@wasi ||= ...`. Same fallback the always-on
    // `wasi_import_override` case proves, but here the glue additionally probes
    // that the bundled WASI *was* built. Ruby/Python only (see NO_LAZY_WASI).
    LibraryCase {
        name: "partial_override_falls_back_to_bundled_wasi",
        wat: "wasi_imports.wat",
        module_name: "prog",
        expect: "ok\nbundled wasi constructed: true\n",
        exclude: NO_LAZY_WASI,
    },
    // Library-mode stdio redirected to an in-memory object: the embedder wires
    // the module's stdout to a language-native in-memory sink (Ruby StringIO,
    // Python BytesIO, Java ByteArrayOutputStream) and reads the guest output
    // back. Go is excluded — its WASI fd table holds `*os.File` only, with no
    // `io.Writer` indirection to inject a buffer (ADR-29); Bash writes to
    // process stdio with no in-memory IO object model (ADR-12).
    LibraryCase {
        name: "wasi_stdio_capture",
        wat: "hello.wat",
        module_name: "prog",
        expect: "Hello, WASI!\n",
        exclude: &[
            (
                "go",
                "WASI fds hold *os.File only; no io.Writer indirection to inject \
                 an in-memory buffer (ADR-29)",
            ),
            (
                "bash",
                "WASI writes to process stdio; no in-memory IO object to \
                 redirect into (ADR-12)",
            ),
        ],
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
    if let Some((_, reason)) = case.exclude.iter().find(|(l, _)| *l == lang.name()) {
        println!("{} excluded for {}: {reason}", case.name, lang.name());
        return;
    }
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
