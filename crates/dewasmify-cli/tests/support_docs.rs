//! Golden-file gate for docs/support.md (ADR-8): the support matrix is
//! rendered from the code's own declarations, so the document cannot
//! drift from reality. Regenerate with:
//!
//!     DEWASMIFY_UPDATE_DOCS=1 cargo test -p dewasmify-cli --test support_docs

use std::fmt::Write as _;
use std::path::Path;

use dewasmify_backend::{
    achieved_tier, feature_tier, is_extension, Backend, SupportStatus, Tier,
    WASI_PREVIEW1_FUNCTIONS,
};
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

    out.push_str("## Tiers\n\n");
    out.push_str(
        "Zig-style support tiers specialized to wasm 1.0 + WASI preview 1\n\
         ([ADR-23](adr/23-backend-support-tiers.md) has the requirement\n\
         checklists); post-1.0 proposals and the component model are the\n\
         extension badges below and never affect a tier. `Current` is derived\n\
         from the declarations on this page.\n\n",
    );
    for tier in Tier::ALL {
        let _ = writeln!(out, "- **{tier}** — {}", tier.summary());
    }
    out.push_str("\n| Backend | Current | Target |\n| --- | --- | --- |\n");
    for backend in &backends {
        let _ = writeln!(
            out,
            "| {} | {} | {} |",
            backend.name(),
            achieved_tier(*backend),
            backend.target_tier()
        );
    }

    out.push_str("\n## Baseline\n\n");
    for backend in &backends {
        let _ = writeln!(out, "- **{}**: {}", backend.name(), backend.baseline());
    }

    let feature_cell =
        |backend: &&dyn Backend, feature: Feature| match backend.feature_status(feature) {
            SupportStatus::Supported => "✅".to_string(),
            SupportStatus::Partial(note) => format!("🟡 {note}"),
            SupportStatus::Unsupported => "❌".to_string(),
        };

    out.push_str("\n## Wasm 1.0 features (tiered)\n\n");
    let mut header = String::from("| Feature | Tier ");
    let mut rule = String::from("| --- | --- ");
    for backend in &backends {
        let _ = write!(header, "| {} ", backend.name());
        rule.push_str("| --- ");
    }
    let _ = writeln!(out, "{header}|\n{rule}|");
    for feature in Feature::ALL.iter().filter(|f| !is_extension(**f)) {
        let tier = feature_tier(*feature).expect("tiered feature");
        let mut row = format!("| {} | {} ", feature.title(), tier);
        for backend in &backends {
            let _ = write!(row, "| {} ", feature_cell(backend, *feature));
        }
        let _ = writeln!(out, "{row}|");
    }

    out.push_str("\n## Extensions (badges)\n\n");
    out.push_str(
        "Per-backend opt-ins, orthogonal to the tiers ([ADR-23](adr/23-backend-support-tiers.md)).\n\n",
    );
    let mut header = String::from("| Feature ");
    let mut rule = String::from("| --- ");
    for backend in &backends {
        let _ = write!(header, "| {} ", backend.name());
        rule.push_str("| --- ");
    }
    let _ = writeln!(out, "{header}|\n{rule}|");
    for feature in Feature::ALL.iter().filter(|f| is_extension(**f)) {
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
         [ADR-12](adr/12-bash-wasi.md)). The tier is the one that requires the\n\
         function; `—` marks the out-of-scope surface no tier requires (ADR-23).\n\n",
    );
    let mut header = String::from("| Function | Tier ");
    let mut rule = String::from("| --- | --- ");
    for backend in &backends {
        let _ = write!(header, "| {} ", backend.name());
        rule.push_str("| --- ");
    }
    let _ = writeln!(out, "{header}|\n{rule}|");
    for (name, tier) in WASI_PREVIEW1_FUNCTIONS {
        let tier_cell = match tier {
            Some(tier) => tier.to_string(),
            None => "—".to_string(),
        };
        let mut row = format!("| {name} | {tier_cell} ");
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

    out.push_str("\n## WASI preview 2 (ruby, components only)\n\n");
    out.push_str(
        "The `Rt::WASIP2` host functions the runtime units implement\n\
         ([ADR-21](adr/21-ruby-wasi-preview2.md)); a component importing an\n\
         unimplemented `wasi:*` function still links but traps if it calls it.\n\n",
    );
    let mut p2_units: Vec<&str> = dewasmify_backend_ruby::bundler()
        .units()
        .filter_map(|u| u.id.strip_prefix("wasi_p2/"))
        .filter(|n| !n.starts_with('_'))
        .collect();
    p2_units.sort_unstable();
    for name in p2_units {
        let _ = writeln!(out, "- `{name}`");
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
