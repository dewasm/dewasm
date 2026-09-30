//! Multi-module scenarios: each module is written to its own file in a fresh directory.
//! Each is loaded the way that language loads a file.
//! Concatenating them instead would make the case where two artifacts coexist prove nothing.
//! Composition is backend-specific, since this crate cannot depend on a concrete backend.
//! It covers how a backend emits several modules against one runtime, or as self-contained ones.
//! It also covers what the backend's driver preamble has to say to load them.
//! [`BackendUnderTest::compose_modules`] supplies it, and [`BackendUnderTest::run_in_dir`] runs it.
//! What stays shared is the case content: which fixtures, the linkage model, and one expectation.
//! Each backend's driver is engineered to produce that fixed expectation.
//! It is normalized so it is identical across languages.
//!
//! Each case is a `pub const` [`MultiModuleCase`] driven by a per-case macro.
//! The macros are `shared_table_e2e!` and `embedded_coexist_e2e!`.
//! Which backends call it is the capability declaration.
//! A backend that does not call it carries a REASON comment.

use crate::backend::BackendUnderTest;

/// A multi-module case: the modules to emit, the linkage between them, and the expected output.
/// The expected output is fixed.
pub struct MultiModuleCase {
    pub name: &'static str,
    /// `(wat filename in examples/wat, class/type name)` for each module.
    pub modules: &'static [(&'static str, &'static str)],
    /// `true`: emit every module against ONE shared runtime.
    /// So an imported table crosses modules (structural `call_indirect` typing).
    /// `false`: emit independent self-contained (Embedded) runtimes that coexist without colliding.
    pub shared_runtime: bool,
    /// The one fixed output every backend's driver is engineered to produce.
    /// It is normalized: for example `distinct-rt`/`trapped` tokens.
    /// It is never a language-specific `true` or trap message.
    pub expect: &'static str,
}

/// A table shared across two modules.
/// Their type sections order the same structural type differently.
/// The `call_indirect` check must compare types structurally, never via a module-local id.
/// Cross-module linking uses one shared runtime, as the specification harness's `register` does.
/// That is Ruby's Alias linkage, or Go's and Java's shared program bundle.
/// Every backend calls `shared_table_e2e!`.
pub const SHARED_TABLE: MultiModuleCase = MultiModuleCase {
    name: "shared_table_call_indirect",
    modules: &[
        ("shared_table_a.wat", "TableExp"),
        ("shared_table_b.wat", "TableImp"),
    ],
    shared_runtime: true,
    expect: "42\n",
};

/// Two self-contained artifacts must coexist in one namespace, each carrying its own runtime.
/// So runtime types (the trap type above all) never collide.
/// This is a requirement of `RuntimeLinkage::Embedded` on every backend, whatever isolates there:
///
/// * Ruby nests `module Rt` in each class;
/// * Java nests its runtime classes;
/// * Perl and Python rename the runtime per artifact;
/// * Bash prefixes its runtime function names;
/// * Go gets it from the per-package library output.
///
/// Every backend calls it (issue #141).
/// The driver normalizes output to `distinct-rt`/`trapped`.
pub const EMBEDDED_COEXIST: MultiModuleCase = MultiModuleCase {
    name: "embedded_runtimes_coexist",
    modules: &[("div_trap.wat", "Alpha"), ("div_trap.wat", "Beta")],
    shared_runtime: false,
    expect: "3\n4294967293\ndistinct-rt\ntrapped\n",
};

/// Write `case`'s modules into a fresh directory with the backend.
/// Append its driver `glue` to the preamble that loads them, and run from that directory.
/// Check `stdout` against the case's fixed expectation.
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
