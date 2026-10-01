//! The checked-in execution snapshots, behind `cargo xtask update-snapshots` and its runners.
//! Explicit subcommands replace the former environment variables that regenerated snapshots.
//! Those variables sat on the `support_docs` and `apps_wasmtime` tests.
//! The tests are now compare-only and point here when they fail.
//!
//! `update-snapshots` regenerates *every* checked-in execution snapshot from one command.
//! Those are the nine WASI-runner files plus the DOOM and NES frames (issue #114).
//! The WASI-runner files hold app `stdout`, including that of apps that use the file system.
//! They also hold the `gzip` stream and the interactive-REPL transcript.
//! The frames have custom export/import interfaces, which are driven directly instead.
//! All of them run on the embedded `wasmtime` crate, at the version `Cargo.lock` records.
//! Regeneration then reproduces the same bytes on every host.
//!
//! `test-wasmtime-wasi`, `test-wasmtime-doom-frame` and `test-wasmtime-nes-frame` are commands.
//! They run the same executions, for the snapshot freshness suite to spawn.
//! That suite compares the checked-in files against what the `xtask` binary produces.
//! It must not embed the engine itself.
//! The REPL capture also spawns the running binary as `test-wasmtime-wasi`.
//! So the binary that calls [`update_snapshots`] must route that command to [`test_wasi`].

mod doom_snapshot;
mod nes_snapshot;
mod snapshot_engine;
mod wasi_run;

use std::io::Write;
use std::path::PathBuf;

use anyhow::{bail, Result};

use crate::doom_snapshot::capture_doom_frame;
use crate::nes_snapshot::capture_nes_frame;
use crate::snapshot_engine::EmbeddedWasmtime;

pub use crate::wasi_run::main as test_wasi;

/// `test-wasmtime-doom-frame`: writes the captured DOOM framebuffer to `stdout`.
pub fn test_doom_frame() -> Result<()> {
    write_stdout(&capture_doom_frame()?.0)
}

/// `test-wasmtime-nes-frame`: writes the captured NES framebuffer to `stdout`.
pub fn test_nes_frame() -> Result<()> {
    write_stdout(&capture_nes_frame()?.0)
}

/// Emit captured snapshot bytes on `stdout`, where the freshness suite reads them.
fn write_stdout(bytes: &[u8]) -> Result<()> {
    let mut out = std::io::stdout().lock();
    out.write_all(bytes)?;
    out.flush()?;
    Ok(())
}

/// A capture closure's output: the `(path, bytes)` files to write for one target.
/// Most targets yield one file.
/// The DOOM and NES frames yield two: the compared PPM plus a human-facing PNG of the same frame.
type CapturedFiles = Vec<(PathBuf, Vec<u8>)>;

/// One execution snapshot to regenerate.
/// Its `label`, a path relative to the repository, is used for the substring filter.
/// Its `capture` closure reruns the case and returns the files to write.
/// Capture fails loud on a missing cache or missing `wasmtime`.
/// The underlying runners carry the exact set-up message.
struct SnapshotTarget {
    label: String,
    capture: Box<dyn Fn() -> Result<CapturedFiles>>,
}

/// Every execution snapshot `update-snapshots` regenerates.
/// Those are the nine WASI-runner targets from `dewasm_test_helper::wasmtime_snapshots`.
/// The DOOM and NES frames are added here rather than in the helper crate.
/// That crate then keeps no `wasmtime`-crate dependency.
/// Each of the two frame targets emits two files.
/// One is the compared PPM (`doom_frame.ppm`, `nes_frame.ppm`).
/// The other is a PNG of the same frame for people to view, never compared by a test.
fn snapshot_targets() -> Vec<SnapshotTarget> {
    let mut targets: Vec<SnapshotTarget> =
        dewasm_test_helper::wasmtime_snapshots(&EmbeddedWasmtime)
            .into_iter()
            .map(|snap| SnapshotTarget {
                label: snap.label,
                // Wrap the fail-loud capture (it panics with a set-up message) in `Ok`.
                // Every target then shares one `Result` signature.
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

/// `update-snapshots [filter]`: regenerates every execution snapshot.
/// With a filter, only those whose repository-relative label contains it.
/// One line per file written (path + byte count).
/// An unmatched filter is an error, so a mistyped one fails loud rather than doing nothing.
pub fn update_snapshots(mut args: impl Iterator<Item = String>) -> Result<()> {
    let filter = args.next();
    let filter = filter.as_deref();
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
