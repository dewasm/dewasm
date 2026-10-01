//! Developer-facing workspace tasks, run as `cargo xtask <command>`.
//! The alias is in `.cargo/config.toml`.
//! This binary only selects the command; each command lives in an `xtask-*` library crate.
//! The binary keeps the name `xtask`: the snapshot freshness suite spawns it by that name.
//!
//! No `clap` dependency: a couple of subcommands and a help message do not need one.

use anyhow::{bail, Result};

const USAGE: &str = "\
Usage: cargo xtask <command> [args]

Commands:
    update-support-docs
        Regenerate docs/support.md from the backends' capability declarations.
    update-snapshots [filter]
        Regenerate the checked-in execution snapshots under examples/apps/snapshots/ whose name contains the filter.
    test-wasmtime-wasi [--dir HOST::GUEST]... [--env K=V]... <wasm> [args...]
        Run a WASI command on the embedded wasmtime, for the snapshot freshness suite.
    test-wasmtime-doom-frame
        Write the captured DOOM framebuffer to stdout as a binary P6 PPM.
    test-wasmtime-nes-frame
        Write the captured NES framebuffer to stdout as a binary P6 PPM.
    record-speed [filter] [--list] [--reps N] [--target-ms MS] [--timeout SECS]
        Run the cross-runtime benchmark suite and write a speed record to records/ (see docs/benchmarks/README.md).
    record-size
        Weigh the corpus and write a size record to records/ (see docs/sizes/README.md).
    render-speed [record]
        Regenerate docs/benchmarks/results.md and its charts from the named speed record, or from the newest one.
    render-size [record]
        Regenerate docs/sizes/results.md and its figures from the named size record, or from the newest one.
    feature-audit <file.wasm>...
        Report each binary's post-baseline feature needs and WASI p1 import surface; fails when one needs a proposal outside the 0.1 scope (verdicts are recorded in agents/apps-audit.md).
    check-text [path]...
        Report the lines of text that break the AGENTS.md writing style, in the named files or in all.
    migrate-records
        Upgrade every record under records/ to its kind's current schema, in place; the render commands read only the current schema.
";

fn main() -> Result<()> {
    let mut args = std::env::args().skip(1);
    match args.next().as_deref() {
        Some("update-support-docs") => xtask_support_docs::update(),
        Some("update-snapshots") => xtask_snapshot::update_snapshots(args),
        Some("test-wasmtime-wasi") => xtask_snapshot::test_wasi(args),
        Some("test-wasmtime-doom-frame") => xtask_snapshot::test_doom_frame(),
        Some("test-wasmtime-nes-frame") => xtask_snapshot::test_nes_frame(),
        Some("record-speed") => xtask_records::record_speed(args),
        Some("record-size") => xtask_records::record_size(args),
        Some("render-speed") => xtask_records::render_speed(args),
        Some("render-size") => xtask_records::render_size(args),
        Some("feature-audit") => xtask_feature_audit::run(args),
        Some("migrate-records") => xtask_records::migrate_records(),
        Some("check-text") => xtask_text_check::run(args),
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
