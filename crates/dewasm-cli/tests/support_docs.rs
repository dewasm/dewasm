//! Generated-docs test for docs/support.md (ADR-8): the support matrix is rendered from the code's own declarations, so the document cannot drift from reality. Compare-only; regenerate with:
//!
//! cargo xtask update-support-docs

use std::path::Path;

use dewasm_cli::support_docs::render_support_docs;

#[test]
fn support_docs_in_sync() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/support.md");
    let rendered = render_support_docs();
    let current = std::fs::read_to_string(&path).unwrap_or_default();
    assert!(
        current == rendered,
        "docs/support.md is out of sync with the code's support declarations.\n\
         Regenerate with: cargo xtask update-support-docs"
    );
}
