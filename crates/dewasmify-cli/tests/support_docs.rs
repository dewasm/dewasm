//! Golden-file gate for docs/support.md (ADR-8): the support matrix is
//! rendered from the code's own declarations, so the document cannot
//! drift from reality. Regenerate with:
//!
//!     DEWASMIFY_UPDATE_DOCS=1 cargo test -p dewasmify-cli --test support_docs

use std::fmt::Write as _;
use std::path::Path;

use dewasmify_backend::{Backend, SupportStatus};
use dewasmify_backend_ruby::{bundler, RubyBackend, WASI_PREVIEW1_FUNCTIONS};
use dewasmify_core::feature::Feature;

fn render() -> String {
    let backends: Vec<&dyn Backend> = vec![&RubyBackend];

    let mut out = String::new();
    out.push_str("# Backend Support Matrix\n\n");
    out.push_str(
        "<!-- AUTO-GENERATED from the backend declarations; do not edit by hand.\n     \
         Regenerate: DEWASMIFY_UPDATE_DOCS=1 cargo test -p dewasmify-cli --test support_docs -->\n\n",
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

    out.push_str("\n## Features\n\n");
    out.push_str("| Feature | ruby |\n| --- | --- |\n");
    for feature in Feature::ALL {
        let mut row = format!("| {} ", feature.title());
        for backend in &backends {
            let cell = match backend.feature_status(*feature) {
                SupportStatus::Supported => "✅".to_string(),
                SupportStatus::Partial(note) => format!("🟡 {note}"),
                SupportStatus::Unsupported => "❌".to_string(),
            };
            let _ = write!(row, "| {cell} ");
        }
        let _ = writeln!(out, "{row}|");
    }

    out.push_str("\n## WASI preview 1 (ruby)\n\n");
    out.push_str(
        "Derived from the runtime units; unimplemented syscalls resolve to an ENOSYS\n\
         stub ([ADR-7](adr/7-import-providers.md)).\n\n",
    );
    out.push_str("| Function | ruby |\n| --- | --- |\n");
    for name in WASI_PREVIEW1_FUNCTIONS {
        let status = if bundler().has_unit(&format!("wasi/{name}")) { "✅" } else { "❌ (ENOSYS)" };
        let _ = writeln!(out, "| {name} | {status} |");
    }
    out
}

#[test]
fn support_docs_in_sync() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/support.md");
    let rendered = render();
    if std::env::var("DEWASMIFY_UPDATE_DOCS").is_ok() {
        std::fs::write(&path, &rendered).expect("write docs/support.md");
        return;
    }
    let current = std::fs::read_to_string(&path).unwrap_or_default();
    assert!(
        current == rendered,
        "docs/support.md is out of sync with the code's support declarations.\n\
         Regenerate with: DEWASMIFY_UPDATE_DOCS=1 cargo test -p dewasmify-cli --test support_docs"
    );
}
