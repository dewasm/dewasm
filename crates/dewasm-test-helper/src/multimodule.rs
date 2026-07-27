//! Multi-module scenarios (ADR-27): two generated modules living in one
//! process. The *composition* — how a backend emits several modules against
//! one runtime, or as independent self-contained ones — is backend-specific and
//! uses each backend's own crate API, which the test-helper crate cannot depend
//! on; it is therefore supplied via [`BackendUnderTest::compose_modules`]. The
//! *driver* (instantiate, link, assert) is per-language glue passed to the
//! runner. What stays shared is the case content: which fixtures, the linkage
//! model, and the one fixed expectation each backend's driver is engineered to
//! produce (normalized so it is identical across languages).
//!
//! Which backends invoke `multi_module_e2e!` is the capability declaration
//! (ADR-27 revision), refined per case by [`MultiModuleCase::exclude`].

use crate::backend::BackendUnderTest;

/// A multi-module case: emit `modules` (`(wat filename, class/type name)`) with
/// the chosen linkage, append the backend's driver glue, run, and require
/// exactly `expect`.
pub struct MultiModuleCase {
    pub name: &'static str,
    /// `(wat filename in examples/wat, class/type name)` for each module.
    pub modules: &'static [(&'static str, &'static str)],
    /// `true`: emit every module against ONE shared runtime, so an imported
    /// table crosses modules (structural call_indirect typing, ADR-4/16).
    /// `false`: emit independent self-contained (Embedded) runtimes that
    /// coexist without colliding.
    pub shared_runtime: bool,
    /// The one fixed output every backend's driver is engineered to produce
    /// (normalized: e.g. `distinct-rt`/`trapped` tokens rather than a
    /// language-specific `true`/trap message).
    pub expect: &'static str,
    /// Backends excluded from this case, each `(lang name, human reason)`.
    pub exclude: &'static [(&'static str, &'static str)],
}

pub const MULTI_MODULE_CASES: &[MultiModuleCase] = &[
    // A table shared across two modules whose type sections order the same
    // structural type differently: the call_indirect check must compare types
    // structurally (ADR-4/16), never via a module-local id. Cross-module
    // linking runs on one shared runtime (Ruby's Alias linkage / Go's & Java's
    // shared program bundle, as the spec harness's `register` path does).
    // Wired on the four ImportedTables-capable backends; Bash rejects imported
    // tables at conversion time (ADR-12), so it does not invoke the macro.
    MultiModuleCase {
        name: "shared_table_call_indirect",
        modules: &[
            ("shared_table_a.wat", "TableExp"),
            ("shared_table_b.wat", "TableImp"),
        ],
        shared_runtime: true,
        expect: "42\n",
        exclude: &[],
    },
    // Two self-contained artifacts must coexist in one process, each carrying
    // its own runtime, so runtime classes (and the trap type) never collide.
    // Inherently a *nested*-runtime capability, and only Ruby nests: its
    // Embedded output puts `module Rt` inside each class, so `Alpha::Rt` and
    // `Beta::Rt` are distinct types. Python's library Embedded output emits a
    // single top-level `class Rt:` (a sibling of the module class), which a
    // second module redefines on concatenation — no per-class runtime; Go and
    // Java emit one flat top-level runtime shared by all modules (ADR-29/30);
    // Bash's runtime is global shell functions (ADR-11). All four are excluded.
    // The driver normalizes output to `distinct-rt`/`trapped`.
    MultiModuleCase {
        name: "embedded_runtimes_coexist",
        modules: &[("div_trap.wat", "Alpha"), ("div_trap.wat", "Beta")],
        shared_runtime: false,
        expect: "3\n4294967293\ndistinct-rt\ntrapped\n",
        exclude: &[
            (
                "python",
                "library Embedded output emits one top-level `class Rt:` (a \
                 sibling, redefined on concatenation), not a per-class nested \
                 runtime",
            ),
            (
                "go",
                "a single flat top-level runtime is shared by all modules \
                 (ADR-29); two independent runtimes cannot coexist",
            ),
            (
                "java",
                "a single flat top-level runtime is shared by all modules \
                 (ADR-30); two independent runtimes cannot coexist",
            ),
            (
                "bash",
                "the runtime is global shell functions (ADR-11); no per-module \
                 runtime to keep distinct",
            ),
        ],
    },
];

/// Resolve a case's per-language driver glue (instantiate the composed modules,
/// link them, and print the normalized observable). A case a backend is wired
/// to run but has no glue for panics loudly (ADR-15).
pub type MultiModuleGlue = fn(&MultiModuleCase) -> &'static str;

/// Compose `case`'s modules with the backend, append its driver glue, run, and
/// check stdout against the case's fixed expectation.
pub fn run_multi_module_case(
    lang: &dyn BackendUnderTest,
    case: &MultiModuleCase,
    glue: MultiModuleGlue,
) {
    if let Some((_, reason)) = case.exclude.iter().find(|(l, _)| *l == lang.name()) {
        println!("{} excluded for {}: {reason}", case.name, lang.name());
        return;
    }
    let modules = lang.compose_modules(case.modules, case.shared_runtime);
    let output = lang.run(&format!("{modules}\n{}", glue(case)), &[], "");
    assert!(
        output.status.success(),
        "{} under {}: nonzero exit {}\n{}",
        case.name,
        lang.name(),
        output.status,
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        case.expect,
        "{} under {}: output differs\nstderr: {}",
        case.name,
        lang.name(),
        String::from_utf8_lossy(&output.stderr)
    );
    println!("{} under {}: matches", case.name, lang.name());
}
