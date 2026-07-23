//! Golden-file gate for docs/support.md (ADR-8): the support matrix is
//! rendered from the code's own declarations, so the document cannot
//! drift from reality. Regenerate with:
//!
//!     DEWASMIFY_UPDATE_DOCS=1 cargo test -p dewasmify-cli --test support_docs

use std::fmt::Write as _;
use std::path::Path;

use dewasmify_backend::{Backend, SupportStatus, WASI_PREVIEW1_FUNCTIONS};
use dewasmify_backend_bash::BashBackend;
use dewasmify_backend_ruby::RubyBackend;
use dewasmify_core::feature::Feature;

fn render() -> String {
    let backends: Vec<&dyn Backend> = vec![&RubyBackend, &BashBackend];

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
    let mut header = String::from("| Feature ");
    let mut rule = String::from("| --- ");
    for backend in &backends {
        let _ = write!(header, "| {} ", backend.name());
        rule.push_str("| --- ");
    }
    let _ = writeln!(out, "{header}|\n{rule}|");
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

    out.push_str("\n## WASI preview 1\n\n");
    out.push_str(
        "Derived from the runtime units; unimplemented syscalls resolve to an ENOSYS\n\
         stub ([ADR-7](adr/7-import-providers.md), bash conventions in\n\
         [ADR-12](adr/12-bash-wasi.md)).\n\n",
    );
    let wasi_backends: Vec<(&str, fn(&str) -> bool)> = vec![
        ("ruby", |id| dewasmify_backend_ruby::bundler().has_unit(id)),
        ("bash", |id| dewasmify_backend_bash::bundler().has_unit(id)),
    ];
    let mut header = String::from("| Function ");
    let mut rule = String::from("| --- ");
    for (name, _) in &wasi_backends {
        let _ = write!(header, "| {name} ");
        rule.push_str("| --- ");
    }
    let _ = writeln!(out, "{header}|\n{rule}|");
    for name in WASI_PREVIEW1_FUNCTIONS {
        let mut row = format!("| {name} ");
        for (_, has_unit) in &wasi_backends {
            let status =
                if has_unit(&format!("wasi/{name}")) { "✅" } else { "❌ (ENOSYS)" };
            let _ = write!(row, "| {status} ");
        }
        let _ = writeln!(out, "{row}|");
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
