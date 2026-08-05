//! Multi-module scenarios (ADR-27): two generated modules living in one process. Each module is written to its own file in a fresh directory and loaded the way that language loads a file — which is how an embedder holds two converted artifacts, and what makes the coexistence case mean anything (a concatenation could never show it). The *composition* — how a backend emits several modules against one runtime, or as independent self-contained ones, and what its driver preamble has to say to load them — is backend-specific and uses each backend's own crate API, which the test-helper crate cannot depend on; it is therefore supplied via [`BackendUnderTest::compose_modules`], with [`BackendUnderTest::run_in_dir`] running the result against that directory. The *driver* (instantiate, link, assert) is per-language glue passed to the runner as a named const. What stays shared is the case content: which fixtures, the linkage model, and the one fixed expectation each backend's driver is engineered to produce (normalized so it is identical across languages).
//!
//! Each case is a `pub const` [`MultiModuleCase`] driven by a per-case macro (`shared_table_e2e!`, `embedded_coexist_e2e!`); which backends invoke it is the capability declaration (ADR-27 revision), with a REASON comment at any non-invocation.

use crate::backend::BackendUnderTest;

/// A multi-module case: emit `modules` (`(wat filename, class/type name)`) with the chosen linkage, append the backend's driver glue, run, and require exactly `expect`.
pub struct MultiModuleCase {
    pub name: &'static str,
    /// `(wat filename in examples/wat, class/type name)` for each module.
    pub modules: &'static [(&'static str, &'static str)],
    /// `true`: emit every module against ONE shared runtime, so an imported table crosses modules (structural call_indirect typing, ADR-4/16). `false`: emit independent self-contained (Embedded) runtimes that coexist without colliding.
    pub shared_runtime: bool,
    /// The one fixed output every backend's driver is engineered to produce (normalized: e.g. `distinct-rt`/`trapped` tokens rather than a language-specific `true`/trap message).
    pub expect: &'static str,
}

/// A table shared across two modules whose type sections order the same structural type differently: the call_indirect check must compare types structurally (ADR-4/16), never via a module-local id. Cross-module linking runs on one shared runtime (Ruby's Alias linkage / Go's & Java's shared program bundle, as the spec harness's `register` path does). Wired on the four ImportedTables-capable backends via `shared_table_e2e!`; Bash rejects imported tables at conversion time (ADR-12), so it does not invoke it.
pub const SHARED_TABLE: MultiModuleCase = MultiModuleCase {
    name: "shared_table_call_indirect",
    modules: &[
        ("shared_table_a.wat", "TableExp"),
        ("shared_table_b.wat", "TableImp"),
    ],
    shared_runtime: true,
    expect: "42\n",
};

/// Two self-contained artifacts must coexist in one namespace, each carrying its own runtime, so runtime types (the trap type above all) never collide. ADR-62 makes this a requirement of `RuntimeLinkage::Embedded` on every backend, whatever isolates there: Ruby nests `module Rt` in each class, Java nests its runtime classes, Perl and Python rename the runtime per artifact, Go gets it from the per-package library output. Bash, the one backend left, carries a non-invocation REASON comment (issue #141). The driver normalizes output to `distinct-rt`/`trapped`.
pub const EMBEDDED_COEXIST: MultiModuleCase = MultiModuleCase {
    name: "embedded_runtimes_coexist",
    modules: &[("div_trap.wat", "Alpha"), ("div_trap.wat", "Beta")],
    shared_runtime: false,
    expect: "3\n4294967293\ndistinct-rt\ntrapped\n",
};

/// Write `case`'s modules into a fresh directory with the backend, append its driver `glue` to the preamble that loads them, run from that directory, and check stdout against the case's fixed expectation.
pub fn run_multi_module_case(lang: &dyn BackendUnderTest, case: &MultiModuleCase, glue: &str) {
    let dir = crate::fresh_scratch_dir(&format!("multimodule-{}-{}", lang.name(), case.name));
    let driver = lang.compose_modules(&dir, case.modules, case.shared_runtime);
    let output = lang.run_in_dir(&dir, &format!("{driver}\n{glue}"));
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
