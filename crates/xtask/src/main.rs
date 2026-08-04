//! Developer-facing workspace tasks, run as `cargo xtask <command>` (aliased in `.cargo/config.toml`). Replaces the former snapshot-regeneration env-var toggles on the `support_docs` and `apps_wasmtime` tests with explicit subcommands: those tests are now compare-only and point here when they fail.
//!
//! `update-snapshots` regenerates *every* checked-in execution snapshot from one command (ADR-56): the nine wasmtime-CLI-driven files (app stdout, the gzip stream, the filesystem-app stdout, the interactive-REPL transcript) plus the DOOM frame, which stays on the embedded `wasmtime` crate because its custom-import interface can't run through `wasmtime run` (ADR-53). `update-support-docs` stays separate — `docs/support.md` is generated documentation, not an execution snapshot.
//!
//! `bench` is the cross-runtime benchmark suite: it measures every dewasm backend against wasmtime and against the wasm interpreters written in the same host languages, then writes a dated result file under `benchmarks/results/` and regenerates `docs/benchmarks/results.md`. Unlike the two commands above, neither output is a compared snapshot — a timing is not reproducible byte-for-byte, so no freshness test guards it.
//!
//! No `clap` dependency: a couple of subcommands and a help message do not need one.

mod bench;
mod doom_snapshot;
mod nes_snapshot;

use std::path::{Path, PathBuf};

use anyhow::{bail, Result};
use dewasm_cli::support_docs::render_support_docs;
use dewasm_test_helper::{doom_frame_snapshot_path, nes_frame_snapshot_path, wasmtime_snapshots};

use crate::doom_snapshot::capture_doom_frame;
use crate::nes_snapshot::capture_nes_frame;

const USAGE: &str = "\
Usage: cargo xtask <command> [args]

Commands:
    update-support-docs        Regenerate docs/support.md from the backends' own
                               capability declarations. Checked by
                               `cargo test -p dewasm-cli --test support_docs`.
    update-snapshots [filter]  Regenerate every checked-in execution snapshot
                               from a live wasmtime: the app/gzip/filesystem
                               stdout files and the interactive-REPL transcript
                               under examples/apps/snapshots/, plus the DOOM
                               frame there (doom_frame.ppm, the compared oracle,
                               and a doom_frame.png rendering for human eyes). An
                               optional substring `filter` limits it to matching
                               snapshots (e.g. `update-snapshots doom`). Needs
                               `wasmtime` on PATH and the apps cache populated
                               (examples/apps/setup.sh; the DOOM frame
                               needs examples/apps/scripts/doom.sh). Checked by
                               the compare-only wasmtime freshness suite
                               (`cargo test -p dewasm-test-helper --features
                               wasmtime_test --test apps_wasmtime`) and the
                               per-backend `doom_frame` cases.
    bench [filter] [options]   Run the cross-runtime benchmark suite: every
                               workload in benchmarks/ (and the app cases) on
                               every dewasm backend, on wasmtime and the other
                               native runtimes (wasmer, wasmedge, wazero,
                               wasm3), and on the pywasm/wardite interpreters.
                               Writes a dated result file to benchmarks/results/
                               and regenerates docs/benchmarks/results.md plus
                               the SVG charts it embeds (docs/benchmarks/figs/, one per
                               workload). An optional substring `filter` limits
                               it to matching workload/runner labels (wasmtime
                               always runs, as the baseline and the correctness
                               reference); an unmatched filter is an error.
                               Needs `wasmtime` on PATH, the microbenchmarks built
                               (benchmarks/wat/build.sh, benchmarks/c/build.sh),
                               the interpreter deps installed
                               (benchmarks/setup.sh) and the apps cache
                               populated; anything missing is reported as
                               skipped-with-reason rather than dropped. Not a
                               compared snapshot: no freshness test.
                               Options: --list (print the matrix and each
                               runner's availability, run nothing), --reps N
                               (timed runs per measurement, default 5),
                               --target-ms MS (compute time the iteration
                               calibrator aims at, default 300),
                               --timeout SECS (per-process ceiling, default
                               900), --render FILE (re-render
                               the results doc and its charts from a stored
                               benchmarks/results/*.json without measuring
                               anything, for when only the wording changed).
";

fn main() -> Result<()> {
    let mut args = std::env::args().skip(1);
    match args.next().as_deref() {
        Some("update-support-docs") => update_support_docs(),
        Some("update-snapshots") => update_snapshots(args.next().as_deref()),
        Some("bench") => bench::main(args),
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

/// Render `docs/support.md` from the backends' own declarations and write it to disk (ADR-8). The corresponding test (`crates/dewasm-cli/tests/support_docs.rs`) is compare-only and names this command in its failure message.
fn update_support_docs() -> Result<()> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/support.md");
    let rendered = render_support_docs();
    std::fs::write(&path, &rendered)?;
    println!("wrote {} ({} bytes)", path.display(), rendered.len());
    Ok(())
}

/// A capture closure's output: the `(path, bytes)` files to write for one target. Most targets yield one file; the DOOM frame yields two (the compared PPM plus a human-facing PNG sidecar).
type CapturedFiles = Vec<(PathBuf, Vec<u8>)>;

/// One regenerable execution snapshot: its repo-relative `label` (used for the substring filter) and a `capture` closure that reruns the case and returns the files to write. Capture fails loud (ADR-15) on a missing cache / missing wasmtime — the underlying runners carry the exact setup message.
struct SnapshotTarget {
    label: String,
    capture: Box<dyn Fn() -> Result<CapturedFiles>>,
}

/// Every execution snapshot `update-snapshots` regenerates: the nine wasmtime-CLI targets from the shared registry (`dewasm_test_helper::wasmtime_snapshots`) plus the embedded-wasmtime DOOM frame, folded in here rather than in the helper crate so that crate keeps no `wasmtime`-crate dependency (ADR-53). The DOOM target emits two files — the compared `doom_frame.ppm` and a `doom_frame.png` rendering of the same frame for human inspection (never compared by a test).
fn snapshot_targets() -> Vec<SnapshotTarget> {
    let mut targets: Vec<SnapshotTarget> = wasmtime_snapshots()
        .into_iter()
        .map(|snap| SnapshotTarget {
            label: snap.label,
            // Wrap the fail-loud capture (it panics with an ADR-15 setup
            // message) in `Ok` so every target shares one `Result` signature.
            capture: Box::new(move || Ok(vec![(snap.path.clone(), (snap.capture)())])),
        })
        .collect();
    targets.push(SnapshotTarget {
        label: "examples/apps/snapshots/doom_frame.ppm".to_string(),
        capture: Box::new(|| {
            let ppm_path = doom_frame_snapshot_path();
            let png_path = ppm_path.with_extension("png");
            let (ppm, png) = capture_doom_frame()?;
            Ok(vec![(ppm_path, ppm), (png_path, png)])
        }),
    });
    targets.push(SnapshotTarget {
        label: "examples/apps/snapshots/nes_frame.ppm".to_string(),
        capture: Box::new(|| {
            let ppm_path = nes_frame_snapshot_path();
            let png_path = ppm_path.with_extension("png");
            let (ppm, png) = capture_nes_frame()?;
            Ok(vec![(ppm_path, ppm), (png_path, png)])
        }),
    });
    targets
}

/// Regenerate every execution snapshot (ADR-56), or only those whose repo-relative label contains `filter`. One line per file written (path + byte count). An unmatched filter is an error, so a typo fails loud rather than silently doing nothing.
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
