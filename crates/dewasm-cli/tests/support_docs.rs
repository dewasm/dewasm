//! Golden-file gate for docs/support.md (ADR-8): the support matrix is
//! rendered from the code's own declarations, so the document cannot
//! drift from reality. Regenerate with:
//!
//!     DEWASM_UPDATE_DOCS=1 cargo test -p dewasm-cli --test support_docs

use std::fmt::Write as _;
use std::path::Path;

use dewasm_backend::{Backend, SupportStatus, WASI_PREVIEW1_FUNCTIONS};
use dewasm_backend_bash::BashBackend;
use dewasm_backend_ruby::RubyBackend;
use dewasm_core::feature::Feature;

/// The features a backend can meaningfully differ on post-excision
/// (ADR-25): everything the core IR accepts unconditionally and leaves to
/// each backend to reject or implement. The remaining `Feature` variants
/// (SIMD, reference types, the component model, ...) are rejected by the
/// core for every backend (ADR-24), so a per-backend row for them would
/// always read "unsupported" and says nothing.
const IN_SCOPE_FEATURES: &[Feature] = &[
    Feature::ImportedGlobals,
    Feature::ImportedMemories,
    Feature::ImportedTables,
    Feature::MultipleTables,
    Feature::TableBulkOps,
    Feature::Floats,
];

fn render() -> String {
    let backends: Vec<&dyn Backend> = vec![&RubyBackend, &BashBackend];

    let mut out = String::new();
    out.push_str("# Backend Support Matrix\n\n");
    out.push_str(
        "<!-- AUTO-GENERATED from the backend declarations; do not edit by hand.\n     \
         Regenerate: DEWASM_UPDATE_DOCS=1 cargo test -p dewasm-cli --test support_docs -->\n\n",
    );
    out.push_str(
        "The spec harness only tolerates test skips attributable to a feature that is\n\
         not `Supported` here ([ADR-8](adr/8-latest-testsuite-support-matrix.md)); an\n\
         unattributable failure is treated as a bug. Flipping a feature to supported\n\
         turns its remaining skips into hard failures until the tests pass.\n\n",
    );

    out.push_str("## Baseline\n\n");
    for backend in &backends {
        let _ = writeln!(out, "- **{}**: {}", backend.name(), backend.baseline());
    }

    let feature_cell =
        |backend: &&dyn Backend, feature: Feature| match backend.feature_status(feature) {
            SupportStatus::Supported => "✅".to_string(),
            SupportStatus::Partial(note) => format!("🟡 {note}"),
            SupportStatus::Unsupported => "❌".to_string(),
        };

    out.push_str("\n## Features\n\n");
    out.push_str(
        "The wasm 1.0 features a backend can meaningfully differ on ([ADR-25](adr/25-retire-support-tiers.md));\n\
         every other `Feature` variant is rejected by the core for every backend ([ADR-24](adr/24-01-scope-reset.md)).\n\n",
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
        "Derived from the runtime units; unimplemented syscalls resolve to an ENOSYS\n\
         stub ([ADR-7](adr/7-import-providers.md), bash conventions in\n\
         [ADR-12](adr/12-bash-wasi.md)). `—` marks the out-of-scope surface (sockets,\n\
         `proc_raise`) no toolchain output exercises (ADR-25).\n\n",
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

    out.push_str("\n## Spec testsuite ledger\n\n");
    out.push_str(
        "Whether the backend's spec `EXPECTED_FAILURES` ledger holds any wasm-1.0-\n\
         attributable entries ([ADR-16](adr/16-ruby-wasm1-completion.md) is the current\n\
         example).\n\n",
    );
    for backend in &backends {
        let clean = if backend.wasm10_ledger_clean() {
            "yes"
        } else {
            "no"
        };
        let _ = writeln!(out, "- **{}**: Spec ledger clean: {clean}", backend.name());
    }

    out
}

#[test]
fn support_docs_in_sync() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/support.md");
    let rendered = render();
    if std::env::var("DEWASM_UPDATE_DOCS").is_ok() {
        std::fs::write(&path, &rendered).expect("write docs/support.md");
        return;
    }
    let current = std::fs::read_to_string(&path).unwrap_or_default();
    assert!(
        current == rendered,
        "docs/support.md is out of sync with the code's support declarations.\n\
         Regenerate with: DEWASM_UPDATE_DOCS=1 cargo test -p dewasm-cli --test support_docs"
    );
}
