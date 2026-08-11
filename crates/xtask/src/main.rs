//! Developer-facing workspace tasks, run as `cargo xtask <command>` (aliased in `.cargo/config.toml`). Replaces the former snapshot-regeneration env-var toggles on the `support_docs` and `apps_wasmtime` tests with explicit subcommands: those tests are now compare-only and point here when they fail.
//!
//! `update-snapshots` regenerates *every* checked-in execution snapshot from one command: the nine WASI-runner files (app stdout, the gzip stream, the filesystem-app stdout, the interactive-REPL transcript) plus the DOOM and NES frames, whose custom export/import interfaces are driven directly instead (issue #114). All of them run on the embedded `wasmtime` crate pinned by `Cargo.lock`, so regeneration reproduces the same bytes on every host. `update-support-docs` stays separate — `docs/support.md` is generated documentation, not an execution snapshot.
//!
//! `run-wasi`, `doom-frame` and `nes-frame` are the same executions as commands, for the snapshot freshness suite to spawn: it compares the checked-in files against what this binary produces, and must not embed the engine itself.
//!
//! `bench` is the cross-runtime benchmark suite: it measures every dewasm backend against wasmtime and against the wasm interpreters written in the same host languages, then writes a dated result file under `benchmarks/results/` and regenerates `docs/benchmarks/results.md`. Unlike the two commands above, neither output is a compared snapshot — a timing is not reproducible byte-for-byte, so no freshness test guards it.
//!
//! `size` is its size counterpart: per app, the wasm binary against every backend's converted source, beside the installed size of each native runtime. Its record joins the timing ones in `benchmarks/results/` and it renders `docs/sizes/results.md`. Also a measurement rather than a snapshot.
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
    update-support-docs        Regenerate docs/support.md from the backends' own
                               capability declarations. Checked by
                               `cargo test -p dewasm-cli --test support_docs`.
    update-snapshots [filter]  Regenerate every checked-in execution snapshot
                               from the embedded wasmtime: the app/gzip/filesystem
                               stdout files and the interactive-REPL transcript
                               under examples/apps/snapshots/, plus the DOOM
                               and NES frames there (doom_frame.ppm and
                               nes_frame.ppm, the compared oracles, and
                               doom_frame.png/nes_frame.png renderings for
                               human eyes). An optional substring `filter`
                               limits it to matching snapshots (e.g.
                               `update-snapshots doom`). Needs the apps cache
                               populated (examples/apps/setup.sh; the DOOM
                               frame needs examples/apps/scripts/doom.sh, the
                               NES frame examples/apps/scripts/nes.sh).
                               Checked by the compare-only freshness suite
                               (`cargo test -p dewasm-test-helper --features
                               wasmtime_test --test apps_wasmtime`) and the
                               per-backend `doom_frame` and `nes_frame` cases.
    run-wasi [opts] <wasm>     Run a WASI command on the embedded wasmtime,
             [args...]         the `wasmtime run` subset the snapshot cases
                               use: argv[0] is the wasm file's base name,
                               stdin/stdout/stderr are inherited, and the
                               guest's exit status becomes this process's.
                               Options: --dir HOST::GUEST (preopen, repeatable),
                               --env KEY=VALUE (repeatable; the guest sees
                               nothing else of the host environment).
    doom-frame                 Write the captured DOOM (or NES) framebuffer to
    nes-frame                  stdout as a binary P6 PPM — the same bytes
                               `update-snapshots` stores in
                               examples/apps/snapshots/. Needs
                               examples/apps/scripts/doom.sh (or nes.sh) run.
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
    size [--render FILE]       Record how big the delivery is: per cached app
                               (cowsay, sqlite3-shell, qjs, ruby) the wasm
                               binary and every backend's converted standalone
                               source, beside the installed size of each native
                               runtime on the host (wasmtime, wasmer, wasmedge,
                               wazero, wasm3). Raw bytes, never compressed.
                               Writes a dated record to
                               benchmarks/results/<timestamp>Z-size.json, beside
                               the speed records, and regenerates
                               docs/sizes/results.md plus the SVG figures it
                               embeds (docs/sizes/figs/; figures the record no
                               longer covers are pruned). Needs the apps cache
                               populated (examples/apps/setup.sh); a missing app
                               or runtime is reported as skipped-with-reason
                               rather than dropped. Not a compared snapshot: no
                               freshness test.
                               Options: --render FILE (re-render the document
                               and its figures from a stored
                               benchmarks/results/*-size.json without measuring
                               anything).
";

fn main() -> Result<()> {
    let mut args = std::env::args().skip(1);
    match args.next().as_deref() {
        Some("update-support-docs") => update_support_docs(),
        Some("update-snapshots") => update_snapshots(args.next().as_deref()),
        Some("run-wasi") => wasi_run::main(args),
        Some("doom-frame") => write_stdout(&capture_doom_frame()?.0),
        Some("nes-frame") => write_stdout(&capture_nes_frame()?.0),
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

/// Render `docs/support.md` from the backends' own declarations and write it to disk. The corresponding test (`crates/dewasm-cli/tests/support_docs.rs`) is compare-only and names this command in its failure message.
fn update_support_docs() -> Result<()> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/support.md");
    let rendered = render_support_docs();
    std::fs::write(&path, &rendered)?;
    println!("wrote {} ({} bytes)", path.display(), rendered.len());
    Ok(())
}

/// A capture closure's output: the `(path, bytes)` files to write for one target. Most targets yield one file; the DOOM and NES frames yield two (the compared PPM plus a human-facing PNG sidecar).
type CapturedFiles = Vec<(PathBuf, Vec<u8>)>;

/// One regenerable execution snapshot: its repo-relative `label` (used for the substring filter) and a `capture` closure that reruns the case and returns the files to write. Capture fails loud on a missing cache / missing wasmtime — the underlying runners carry the exact setup message.
struct SnapshotTarget {
    label: String,
    capture: Box<dyn Fn() -> Result<CapturedFiles>>,
}

/// Every execution snapshot `update-snapshots` regenerates: the nine WASI-runner targets from the shared registry (`dewasm_test_helper::wasmtime_snapshots`) plus the DOOM and NES frames, folded in here rather than in the helper crate so that crate keeps no `wasmtime`-crate dependency. Each of those two targets emits two files — the compared PPM (`doom_frame.ppm`, `nes_frame.ppm`) and a PNG rendering of the same frame for human inspection (never compared by a test).
fn snapshot_targets() -> Vec<SnapshotTarget> {
    let mut targets: Vec<SnapshotTarget> =
        dewasm_test_helper::wasmtime_snapshots(&EmbeddedWasmtime)
            .into_iter()
            .map(|snap| SnapshotTarget {
                label: snap.label,
                // Wrap the fail-loud capture (it panics with a setup
                // message) in `Ok` so every target shares one `Result` signature.
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

/// Regenerate every execution snapshot, or only those whose repo-relative label contains `filter`. One line per file written (path + byte count). An unmatched filter is an error, so a typo fails loud rather than silently doing nothing.
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
