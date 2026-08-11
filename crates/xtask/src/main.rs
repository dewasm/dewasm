//! Developer-facing workspace tasks, run as `cargo xtask <command>` (aliased in `.cargo/config.toml`).
//! Replaces the former snapshot-regeneration env-var toggles on the `support_docs` and `apps_wasmtime` tests with explicit subcommands: those tests are now compare-only and point here when they fail.
//!
//! `update-snapshots` regenerates *every* checked-in execution snapshot from one command: the nine WASI-runner files (app stdout, the gzip stream, the filesystem-app stdout, the interactive-REPL transcript) plus the DOOM and NES frames, whose custom export/import interfaces are driven directly instead (issue #114).
//! All of them run on the embedded `wasmtime` crate pinned by `Cargo.lock`, so regeneration reproduces the same bytes on every host.
//! `update-support-docs` stays separate: `docs/support.md` is generated documentation, not an execution snapshot.
//!
//! `test-wasmtime-wasi`, `test-wasmtime-doom-frame` and `test-wasmtime-nes-frame` are the same executions as commands, for the snapshot freshness suite to spawn: it compares the checked-in files against what this binary produces, and must not embed the engine itself.
//!
//! `bench` is the cross-runtime benchmark suite: it measures every dewasm backend against wasmtime and against the wasm interpreters written in the same host languages, then writes a dated result file under `benchmarks/results/` and regenerates `docs/benchmarks/results.md`.
//! Unlike the two commands above, neither output is a compared snapshot: a timing is not reproducible byte-for-byte, so no freshness test guards it.
//!
//! `size` is its size counterpart: per app, the wasm binary against every backend's converted source, beside the installed size of each native runtime.
//! Its record joins the timing ones in `benchmarks/results/` and it renders `docs/sizes/results.md`.
//! Also a measurement rather than a snapshot.
//!
//! No `clap` dependency: a couple of subcommands and a help message do not need one.

mod bench;
mod doom_snapshot;
mod nes_snapshot;
mod size;
mod snapshot_engine;
mod wasi_run;

use std::io::Write;
use std::path::{Path, PathBuf};

use anyhow::{bail, Result};
use dewasm_cli::support_docs::render_support_docs;

use crate::doom_snapshot::capture_doom_frame;
use crate::nes_snapshot::capture_nes_frame;
use crate::snapshot_engine::EmbeddedWasmtime;

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
    bench [filter] [--list] [--reps N] [--target-ms MS] [--timeout SECS] [--render FILE]
        Run the cross-runtime benchmark suite and regenerate docs/benchmarks/results.md (see docs/benchmarks/README.md).
    size [--render FILE]
        Record the distribution sizes and regenerate docs/sizes/results.md (see docs/sizes/README.md).
";

fn main() -> Result<()> {
    let mut args = std::env::args().skip(1);
    match args.next().as_deref() {
        Some("update-support-docs") => update_support_docs(),
        Some("update-snapshots") => update_snapshots(args.next().as_deref()),
        Some("test-wasmtime-wasi") => wasi_run::main(args),
        Some("test-wasmtime-doom-frame") => write_stdout(&capture_doom_frame()?.0),
        Some("test-wasmtime-nes-frame") => write_stdout(&capture_nes_frame()?.0),
        Some("bench") => bench::main(args),
        Some("size") => size::main(args),
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

/// Emit captured snapshot bytes on stdout, where the freshness suite reads them.
fn write_stdout(bytes: &[u8]) -> Result<()> {
    let mut out = std::io::stdout().lock();
    out.write_all(bytes)?;
    out.flush()?;
    Ok(())
}

/// Render `docs/support.md` from the backends' own declarations and write it to disk.
/// The corresponding test (`crates/dewasm-cli/tests/support_docs.rs`) is compare-only and names this command in its failure message.
fn update_support_docs() -> Result<()> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/support.md");
    let rendered = render_support_docs();
    std::fs::write(&path, &rendered)?;
    println!("wrote {} ({} bytes)", path.display(), rendered.len());
    Ok(())
}

/// A capture closure's output: the `(path, bytes)` files to write for one target.
/// Most targets yield one file; the DOOM and NES frames yield two (the compared PPM plus a human-facing PNG sidecar).
type CapturedFiles = Vec<(PathBuf, Vec<u8>)>;

/// One regenerable execution snapshot: its repo-relative `label` (used for the substring filter) and a `capture` closure that reruns the case and returns the files to write.
/// Capture fails loud on a missing cache / missing wasmtime: the underlying runners carry the exact setup message.
struct SnapshotTarget {
    label: String,
    capture: Box<dyn Fn() -> Result<CapturedFiles>>,
}

/// Every execution snapshot `update-snapshots` regenerates: the nine WASI-runner targets from the shared registry (`dewasm_test_helper::wasmtime_snapshots`) plus the DOOM and NES frames, folded in here rather than in the helper crate so that crate keeps no `wasmtime`-crate dependency.
/// Each of those two targets emits two files: the compared PPM (`doom_frame.ppm`, `nes_frame.ppm`) and a PNG rendering of the same frame for human inspection (never compared by a test).
fn snapshot_targets() -> Vec<SnapshotTarget> {
    let mut targets: Vec<SnapshotTarget> =
        dewasm_test_helper::wasmtime_snapshots(&EmbeddedWasmtime)
            .into_iter()
            .map(|snap| SnapshotTarget {
                label: snap.label,
                // Wrap the fail-loud capture (it panics with a setup message) in `Ok` so every target shares one `Result` signature.
                capture: Box::new(move || Ok(vec![(snap.path.clone(), (snap.capture)())])),
            })
            .collect();
    targets.push(SnapshotTarget {
        label: "examples/apps/snapshots/doom_frame.ppm".to_string(),
        capture: Box::new(|| {
            let ppm_path = dewasm_test_helper::doom_frame_snapshot_path();
            let png_path = ppm_path.with_extension("png");
            let (ppm, png) = capture_doom_frame()?;
            Ok(vec![(ppm_path, ppm), (png_path, png)])
        }),
    });
    targets.push(SnapshotTarget {
        label: "examples/apps/snapshots/nes_frame.ppm".to_string(),
        capture: Box::new(|| {
            let ppm_path = dewasm_test_helper::nes_frame_snapshot_path();
            let png_path = ppm_path.with_extension("png");
            let (ppm, png) = capture_nes_frame()?;
            Ok(vec![(ppm_path, ppm), (png_path, png)])
        }),
    });
    targets
}

/// Regenerate every execution snapshot, or only those whose repo-relative label contains `filter`.
/// One line per file written (path + byte count).
/// An unmatched filter is an error, so a typo fails loud rather than silently doing nothing.
fn update_snapshots(filter: Option<&str>) -> Result<()> {
    let mut wrote = 0usize;
    for target in snapshot_targets() {
        if let Some(needle) = filter {
            if !target.label.contains(needle) {
                continue;
            }
        }
        for (path, bytes) in (target.capture)()? {
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::write(&path, &bytes)?;
            println!("wrote {} ({} bytes)", path.display(), bytes.len());
            wrote += 1;
        }
    }
    if wrote == 0 {
        match filter {
            Some(needle) => bail!("no snapshot label matched filter {needle:?}"),
            None => bail!("no snapshots to regenerate"),
        }
    }
    Ok(())
}
