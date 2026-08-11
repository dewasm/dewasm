//! Rendering for `docs/support.md`: the support matrix is rendered from the code's own declarations, so the document cannot drift from reality. Shared by the compare-only `support_docs_in_sync` test (`tests/support_docs.rs`) and `cargo xtask update-support-docs`, which writes the rendered output to disk.

use std::fmt::Write as _;

use dewasm_backend::{Backend, SupportStatus, WASI_PREVIEW1_FUNCTIONS};
use dewasm_backend_bash::BashBackend;
use dewasm_backend_go::GoBackend;
use dewasm_backend_java::JavaBackend;
use dewasm_backend_perl::PerlBackend;
use dewasm_backend_python::PythonBackend;
use dewasm_backend_ruby::RubyBackend;
use dewasm_core::feature::Feature;

/// The features a backend can meaningfully differ on: everything the core IR accepts unconditionally and leaves to each backend to reject or implement. The remaining `Feature` variants (SIMD, reference types, the component model, ...) are rejected by the core for every backend, so a per-backend row for them would always read "unsupported" and says nothing.
const IN_SCOPE_FEATURES: &[Feature] = &[
    Feature::ImportedGlobals,
    Feature::ImportedMemories,
    Feature::ImportedTables,
    Feature::MultipleTables,
    Feature::TableBulkOps,
    Feature::Floats,
];

pub fn render_support_docs() -> String {
    let backends: Vec<&dyn Backend> = vec![
        &RubyBackend,
        &BashBackend,
        &PythonBackend,
        &PerlBackend,
        &GoBackend,
        &JavaBackend,
    ];

    let mut out = String::new();
    out.push_str("# Backend Support Matrix\n\n");
    out.push_str(
        "<!-- AUTO-GENERATED from the backend declarations; do not edit by hand. Regenerate: cargo xtask update-support-docs -->\n\n",
    );
    out.push_str(
        "The spec harness only tolerates test skips attributable to a feature that is not `Supported` here; an unattributable failure is treated as a bug. Flipping a feature to supported turns its remaining skips into hard failures until the tests pass.\n\n",
    );

    let feature_cell =
        |backend: &&dyn Backend, feature: Feature| match backend.feature_status(feature) {
            SupportStatus::Supported => "✅".to_string(),
            SupportStatus::Partial(note) => format!("🟡 {note}"),
            SupportStatus::Unsupported => "❌".to_string(),
        };

    out.push_str("## Features\n\n");
    out.push_str(
        "The wasm 1.0 features a backend can meaningfully differ on; every other `Feature` variant is rejected by the core for every backend.\n\n",
    );
    let mut header = String::from("| Feature ");
    let mut rule = String::from("| --- ");
    for backend in &backends {
        let _ = write!(header, "| {} ", backend.name());
        rule.push_str("| --- ");
    }
    let _ = writeln!(out, "{header}|\n{rule}|");
    for feature in IN_SCOPE_FEATURES {
        let mut row = format!("| {} ", feature.title());
        for backend in &backends {
            let _ = write!(row, "| {} ", feature_cell(backend, *feature));
        }
        let _ = writeln!(out, "{row}|");
    }

    out.push_str("\n## WASI preview 1\n\n");
    out.push_str(
        "Derived from the runtime units; unimplemented syscalls resolve to an ENOSYS stub. `—` marks the out-of-scope surface (sockets, `proc_raise`) no toolchain output exercises.\n\n",
    );
    let mut header = String::from("| Function ");
    let mut rule = String::from("| --- ");
    for backend in &backends {
        let _ = write!(header, "| {} ", backend.name());
        rule.push_str("| --- ");
    }
    let _ = writeln!(out, "{header}|\n{rule}|");
    for (name, in_scope) in WASI_PREVIEW1_FUNCTIONS {
        if !in_scope {
            let mut row = format!("| {name} ");
            for _ in &backends {
                row.push_str("| — ");
            }
            let _ = writeln!(out, "{row}|");
            continue;
        }
        let mut row = format!("| {name} ");
        for backend in &backends {
            let status = if backend.has_wasi_p1(name) {
                "✅"
            } else {
                "❌ (ENOSYS)"
            };
            let _ = write!(row, "| {status} ");
        }
        let _ = writeln!(out, "{row}|");
    }

    out
}
