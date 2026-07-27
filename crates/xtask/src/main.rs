//! Developer-facing workspace tasks, run as `cargo xtask <command>` (aliased
//! in `.cargo/config.toml`). Replaces the former golden-regeneration env-var
//! toggles on the `support_docs` and `apps_wasmtime` tests with explicit
//! subcommands: those tests are now compare-only and point here when they
//! fail.
//!
//! No `clap` dependency: two subcommands and a help message do not need one.

use std::path::Path;

use anyhow::{bail, Result};
use dewasm_cli::support_docs::render_support_docs;
use dewasm_test_helper::{capture_qjs_repl_golden, qjs_repl_golden_path, Wasmtime};

const USAGE: &str = "\
Usage: cargo xtask <command>

Commands:
    update-support-docs   Regenerate docs/support.md from the backends' own
                           capability declarations. Checked by
                           `cargo test -p dewasm-cli --test support_docs`.
    update-repl-golden     Recapture the interactive QuickJS REPL transcript
                           (examples/apps/golden/qjs_repl_interactive.transcript)
                           against a live wasmtime under a pty. Requires
                           `wasmtime` on PATH and the qjs app cached
                           (examples/apps/fetch.sh). Checked by
                           `cargo test -p dewasm-test-helper --features
                           wasmtime_test --test apps_wasmtime`.
";

fn main() -> Result<()> {
    let mut args = std::env::args().skip(1);
    match args.next().as_deref() {
        Some("update-support-docs") => update_support_docs(),
        Some("update-repl-golden") => update_repl_golden(),
        Some("-h") | Some("--help") | Some("help") => {
            print!("{USAGE}");
            Ok(())
        }
        Some(other) => {
            eprint!("{USAGE}");
            bail!("unknown command: {other}");
        }
        None => {
            eprint!("{USAGE}");
            bail!("missing command");
        }
    }
}

/// Render `docs/support.md` from the backends' own declarations and write it
/// to disk (ADR-8). The corresponding test (`crates/dewasm-cli/tests/
/// support_docs.rs`) is compare-only and names this command in its failure
/// message.
fn update_support_docs() -> Result<()> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/support.md");
    let rendered = render_support_docs();
    std::fs::write(&path, &rendered)?;
    println!("wrote {} ({} bytes)", path.display(), rendered.len());
    Ok(())
}

/// Recapture the interactive QuickJS REPL transcript against a live wasmtime
/// under a pty and write it to the checked-in golden path (Fix 4,
/// `crates/dewasm-test-helper/src/qjs_repl.rs`). The corresponding freshness
/// test (`crates/dewasm-test-helper/tests/apps_wasmtime.rs`) is compare-only
/// and names this command in its failure message.
fn update_repl_golden() -> Result<()> {
    let bytes = capture_qjs_repl_golden(&Wasmtime);
    println!(
        "wrote {} ({} bytes)",
        qjs_repl_golden_path().display(),
        bytes.len()
    );
    Ok(())
}
