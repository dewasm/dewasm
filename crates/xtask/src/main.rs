//! Developer-facing workspace tasks, run as `cargo xtask <command>`.
//! The alias is in `.cargo/config.toml`.
//! Explicit subcommands replace the former snapshot-regeneration env-var toggles.
//! Those toggles sat on the `support_docs` and `apps_wasmtime` tests.
//! The tests are now compare-only and point here when they fail.
//!
//! `update-snapshots` regenerates *every* checked-in execution snapshot from one command.
//! Those are the nine WASI-runner files plus the DOOM and NES frames (issue #114).
//! The WASI-runner files hold app stdout, the gzip stream, and the filesystem-app stdout.
//! They also hold the interactive-REPL transcript.
//! The frames have custom export/import interfaces, which are driven directly instead.
//! All of them run on the embedded `wasmtime` crate pinned by `Cargo.lock`.
//! Regeneration then reproduces the same bytes on every host.
//! `update-support-docs` stays separate.
//! `docs/support.md` is generated documentation, not an execution snapshot.
//!
//! `test-wasmtime-wasi`, `test-wasmtime-doom-frame` and `test-wasmtime-nes-frame` are commands.
//! They run the same executions, for the snapshot freshness suite to spawn.
//! That suite compares the checked-in files against what this binary produces.
//! It must not embed the engine itself.
//!
//! The two measurements are `record-speed` and `record-size`.
//! `record-speed` runs every dewasm backend against wasmtime.
//! It also runs them against the wasm interpreters written in the same host languages.
//! `record-size` compares, per app, the wasm binary against every backend's converted source.
//! It lists the installed size of each native runtime beside them.
//! Each writes a dated record under `records/` and renders nothing.
//! `render-speed` and `render-size` turn a record into a results page.
//! Those are `docs/benchmarks/results.md` and `docs/sizes/results.md`.
//! Unlike the commands above, none of those outputs is a compared snapshot.
//! Neither a timing nor an installed size is reproducible byte-for-byte.
//! So no freshness test guards them.
//!
//! `feature-audit` is the app audit test.
//! It reports each candidate app binary's post-baseline feature needs and WASI p1 import surface.
//! The verdicts are recorded in `agents/apps-audit.md`.
//!
//! No `clap` dependency: a couple of subcommands and a help message do not need one.

mod bench;
mod doom_snapshot;
mod feature_audit;
mod migrate;
mod nes_snapshot;
mod size;
mod snapshot_engine;
mod support_docs;
mod wasi_run;

use std::io::Write;
use std::path::{Path, PathBuf};

use anyhow::{bail, Result};

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
        Some("update-support-docs") => update_support_docs(),
        Some("update-snapshots") => update_snapshots(args.next().as_deref()),
        Some("test-wasmtime-wasi") => wasi_run::main(args),
        Some("test-wasmtime-doom-frame") => write_stdout(&capture_doom_frame()?.0),
        Some("test-wasmtime-nes-frame") => write_stdout(&capture_nes_frame()?.0),
        Some("record-speed") => bench::record(args),
        Some("record-size") => size::record(args),
        Some("render-speed") => bench::render(args),
        Some("render-size") => size::render(args),
        Some("feature-audit") => feature_audit::main(args),
        Some("migrate-records") => migrate::run(),
        Some("check-text") => check_text(args),
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
/// The corresponding `support_docs_in_sync` unit test (`src/support_docs.rs`) is compare-only.
/// Its failure message names this command.
fn update_support_docs() -> Result<()> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/support.md");
    let rendered = support_docs::render_support_docs();
    std::fs::write(&path, &rendered)?;
    println!("wrote {} ({} bytes)", path.display(), rendered.len());
    Ok(())
}

/// A capture closure's output: the `(path, bytes)` files to write for one target.
/// Most targets yield one file.
/// The DOOM and NES frames yield two: the compared PPM plus a human-facing PNG of the same frame.
type CapturedFiles = Vec<(PathBuf, Vec<u8>)>;

/// One regenerable execution snapshot.
/// Its repo-relative `label` is used for the substring filter.
/// Its `capture` closure reruns the case and returns the files to write.
/// Capture fails loud on a missing cache or missing wasmtime.
/// The underlying runners carry the exact setup message.
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
/// The other is a PNG of the same frame for human inspection, never compared by a test.
fn snapshot_targets() -> Vec<SnapshotTarget> {
    let mut targets: Vec<SnapshotTarget> =
        dewasm_test_helper::wasmtime_snapshots(&EmbeddedWasmtime)
            .into_iter()
            .map(|snap| SnapshotTarget {
                label: snap.label,
                // Wrap the fail-loud capture (it panics with a setup message) in `Ok`.
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

/// `check-text [path]...`: prints every defect in the named files, or in every tracked text file.
fn check_text(args: impl Iterator<Item = String>) -> Result<()> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let mut paths: Vec<String> = args.collect();
    if paths.is_empty() {
        paths = text_check::tracked_files(&root);
    }
    let vocabulary = text_check::vocabulary::Vocabulary::load(&root).map_err(anyhow::Error::msg)?;
    let mut count = 0;
    for path in &paths {
        if !text_check::is_checked_text(path) {
            bail!("{path} is neither Markdown nor a source file with known comment markers");
        }
        let mut defects = text_check::file_defects(&root, path);
        defects.extend(vocabulary.file_defects(&root, path));
        for defect in defects {
            println!("{defect}");
            count += 1;
        }
    }
    if count > 0 {
        bail!("{count} text defects");
    }
    Ok(())
}
