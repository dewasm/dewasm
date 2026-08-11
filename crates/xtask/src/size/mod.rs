//! `cargo xtask size`: the size record, sibling of the `bench` speed record.
//!
//! It answers the distribution question with numbers: shipping a wasm program means shipping the binary *and* a runtime that can execute it, while shipping dewasm's output means shipping source to users who already have the interpreter.
//! Which is smaller is a fact about a given app and a given backend, and this command measures it: per app, the wasm binary and every backend's converted standalone source, beside the installed size of every native runtime on the host.
//!
//! Two outputs from one command, like `bench`: a dated record under `records/` (`<timestamp>Z-size.json`, beside the speed records, one home for every measurement record) and a generated `docs/sizes/results.md` with its figures under `docs/sizes/figs/`.
//! The hand-written `docs/sizes/README.md` beside it says how to run this and how to read the numbers; nothing here writes it.
//! Neither output is a compared snapshot (the sizes move with the host's runtime versions and with every codegen change), so no freshness test guards them, and `--render` regenerates the document from a stored record without measuring.
//!
//! Raw bytes throughout, never compressed: a release artifact's weight is the honest distribution figure, and compression flattens exactly the differences the record exists to track.

mod chart;
mod report;

use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use dewasm_backend::{Backend, GenOptions, Mode, RuntimeLinkage};

use crate::bench::runner::{runners, Kind};
use crate::bench::{
    apps_cache_dir, display_path, docs_dir, host_info, note_record, records_dir, utc_timestamp,
    write_file,
};
use crate::size::report::{App, Cell, Component, Outcome};

/// The corpus, in report order: a small utility, a database shell, a JavaScript engine, a whole Ruby.
/// Fixed rather than "everything in the cache": these four span two orders of magnitude of wasm size, and a record whose contents depend on which apps happen to be built is not comparable with the next one.
const CORPUS: [&str; 4] = ["cowsay.wasm", "sqlite3-shell.wasm", "qjs.wasm", "ruby.wasm"];

struct Options {
    /// Re-render `docs/sizes/results.md` and its figures from a stored record instead of measuring.
    /// Converting the corpus with six backends takes minutes, so a wording fix must not require re-measuring: the JSON is the record, the markdown is a view of it.
    render: Option<PathBuf>,
}

impl Options {
    fn parse(args: impl Iterator<Item = String>) -> Result<Self> {
        let mut opts = Options { render: None };
        let mut args = args.peekable();
        while let Some(arg) = args.next() {
            let mut value = |name: &str| -> Result<String> {
                args.next().with_context(|| format!("{name} needs a value"))
            };
            match arg.as_str() {
                "--render" => opts.render = Some(PathBuf::from(value("--render")?)),
                other => bail!("unknown size option: {other}"),
            }
        }
        Ok(opts)
    }
}

pub fn main(args: impl Iterator<Item = String>) -> Result<()> {
    let opts = Options::parse(args)?;

    if let Some(path) = &opts.render {
        let text = std::fs::read_to_string(path)
            .with_context(|| format!("failed to read {}", display_path(path)))?;
        let report: report::Report = serde_json::from_str(&text)
            .with_context(|| format!("{} is not a size record", display_path(path)))?;
        return write_doc(&report);
    }

    run()
}

/// The backends, in report order.
/// Each one converts every app in the corpus; nothing here is optional, because generating source needs no toolchain installed: only running it would.
fn backends() -> Vec<(&'static str, &'static (dyn Backend + Sync))> {
    vec![
        ("ruby", &dewasm_backend_ruby::RubyBackend),
        ("python", &dewasm_backend_python::PythonBackend),
        ("perl", &dewasm_backend_perl::PerlBackend),
        ("bash", &dewasm_backend_bash::BashBackend),
        ("go", &dewasm_backend_go::GoBackend),
        ("java", &dewasm_backend_java::JavaBackend),
    ]
}

fn run() -> Result<()> {
    let runtimes = measure_runtimes();
    let apps: Vec<App> = CORPUS.iter().map(|file| measure_app(file)).collect();

    let generated_at = utc_timestamp();
    let report = report::Report {
        schema: 1,
        generated_at: generated_at.clone(),
        host: host_info(),
        runtimes,
        apps,
    };

    // Beside the speed records, in the same dated spelling, with `-size` naming the kind.
    // `--render` on the other kind of file fails to deserialize, which is the check that matters.
    let json_path = records_dir().join(format!("{}-size.json", generated_at.replace(':', "-")));
    write_file(&json_path, &report.to_json()?)?;
    note_record(&json_path, &report.generated_at, &report.host)?;
    write_doc(&report)?;

    let failures: Vec<String> = report
        .apps
        .iter()
        .flat_map(|app| {
            app.targets
                .iter()
                .filter_map(move |cell| match &cell.outcome {
                    Outcome::Failed { reason } => {
                        Some(format!("{} on {}: {reason}", app.file, cell.target))
                    }
                    _ => None,
                })
        })
        .collect();
    if failures.is_empty() {
        return Ok(());
    }
    for failure in &failures {
        eprintln!("FAILED {failure}");
    }
    bail!(
        "{} conversion(s) failed; the record was still written to {}",
        failures.len(),
        json_path.display()
    );
}

/// Both the measuring run and `--render` go through here, so a stored record regenerates the figures as well as the prose.
///
/// Figures the record no longer covers are deleted: an orphan SVG looks current while nothing links it.
fn write_doc(report: &report::Report) -> Result<()> {
    let charts = chart::charts(report);
    let mut written: Vec<String> = Vec::new();
    for chart in &charts {
        for (suffix, contents) in [("", &chart.light), ("-dark", &chart.dark)] {
            let name = format!("{}{suffix}.svg", chart.stem);
            write_file(&figs_dir().join(&name), contents)?;
            written.push(name);
        }
    }
    prune_figs(&written)?;
    write_file(
        &sizes_dir().join("results.md"),
        &report::render_doc(report, &charts),
    )
}

fn prune_figs(written: &[String]) -> Result<()> {
    let Ok(entries) = std::fs::read_dir(figs_dir()) else {
        return Ok(());
    };
    for path in entries.flatten().map(|entry| entry.path()) {
        if path.extension().is_none_or(|ext| ext != "svg") {
            continue;
        }
        let stale = path
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| !written.iter().any(|kept| kept == name));
        if stale {
            std::fs::remove_file(&path)
                .with_context(|| format!("failed to remove the stale figure {}", path.display()))?;
            println!("removed {}", path.display());
        }
    }
    Ok(())
}

/// Every runtime that executes a `.wasm` directly, weighed as installed.
/// An uninstalled one is recorded with the reason it is missing, never dropped.
fn measure_runtimes() -> Vec<report::Runtime> {
    runners()
        .iter()
        .filter(|runner| matches!(runner.kind, Kind::Wasmtime | Kind::Native(_)))
        .map(|runner| {
            println!("  runtime {}", runner.label);
            let (outcome, components) = match runner.availability() {
                Err(reason) => (Outcome::Skipped { reason }, Vec::new()),
                Ok(()) => match runner.binary().map(|bin| weigh(&bin)) {
                    Some(Ok((bytes, components))) => (Outcome::Ok { bytes }, components),
                    Some(Err(err)) => (
                        Outcome::Failed {
                            reason: format!("{err:#}"),
                        },
                        Vec::new(),
                    ),
                    None => (
                        Outcome::Skipped {
                            reason: "not an executable this command can weigh".to_string(),
                        },
                        Vec::new(),
                    ),
                },
            };
            if let Outcome::Ok { bytes } = &outcome {
                println!("    {} bytes across {} file(s)", bytes, components.len());
            }
            report::Runtime {
                runtime: runner.label.to_string(),
                version: runner.version(),
                components,
                outcome,
            }
        })
        .collect()
}

/// A bare executable name resolved against `$PATH`, the way the shell resolves it when the runner launches it.
/// Anything that already has a directory in it is returned unchanged.
fn on_path(bin: &Path) -> PathBuf {
    if bin.components().count() > 1 {
        return bin.to_path_buf();
    }
    std::env::var_os("PATH")
        .into_iter()
        .flat_map(|paths| {
            std::env::split_paths(&paths)
                .map(|dir| dir.join(bin))
                .collect::<Vec<PathBuf>>()
        })
        .find(|candidate| candidate.is_file())
        .unwrap_or_else(|| bin.to_path_buf())
}

/// What one runtime weighs as installed: the executable with its symlinks resolved, plus any shared library of its own that the executable actually loads.
///
/// The library rule is not cosmetic in either direction.
/// Homebrew's `wasmedge` executable is 100 kB of front end over a 2.4 MB `libwasmedge`, so counting the executable alone would report it twenty times too small; the same Homebrew ships a 55 MB `lib/` beside `wasmtime` (a static archive and a dylib for embedders) that the statically linked CLI never opens, so counting everything in that directory would report wasmtime twice too large.
/// What is counted is therefore what the executable names: a candidate library beside it is included only when its filename appears in the executable's bytes, which is where the dynamic linker's own list of dependencies lives (Mach-O load commands, ELF `DT_NEEDED`).
/// Aliases resolve to one file, so a library reached through a `.0.dylib` symlink is counted once.
fn weigh(bin: &Path) -> Result<(u64, Vec<Component>)> {
    let exe = std::fs::canonicalize(on_path(bin))
        .with_context(|| format!("failed to resolve {}", bin.display()))?;
    let image = std::fs::read(&exe).with_context(|| format!("failed to read {}", exe.display()))?;
    let mut components = vec![Component {
        path: exe.display().to_string(),
        bytes: image.len() as u64,
    }];

    let lib_dir = exe.parent().and_then(Path::parent).map(|up| up.join("lib"));
    let prefix = exe
        .file_name()
        .map(|name| format!("lib{}", name.to_string_lossy()))
        .unwrap_or_default();
    if let Some(entries) = lib_dir.and_then(|dir| std::fs::read_dir(dir).ok()) {
        let mut libs: Vec<Component> = Vec::new();
        for path in entries.flatten().map(|entry| entry.path()) {
            let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
                continue;
            };
            if !name.starts_with(&prefix) || !references(&image, name) {
                continue;
            }
            let Ok(real) = std::fs::canonicalize(&path) else {
                continue;
            };
            let Ok(meta) = std::fs::metadata(&real) else {
                continue;
            };
            let real = real.display().to_string();
            if !libs.iter().any(|lib| lib.path == real) {
                libs.push(Component {
                    path: real,
                    bytes: meta.len(),
                });
            }
        }
        libs.sort_by(|a, b| a.path.cmp(&b.path));
        components.extend(libs);
    }

    Ok((components.iter().map(|c| c.bytes).sum(), components))
}

/// Whether `image` mentions `name` anywhere in its bytes.
/// The dynamic linker's dependency list is stored as plain strings in the executable, so this is how a library beside the binary is told apart from one merely installed in the same directory.
fn references(image: &[u8], name: &str) -> bool {
    image
        .windows(name.len())
        .any(|window| window == name.as_bytes())
}

/// One app: the wasm binary's size, then every backend's converted source.
/// A cache file that is not there makes the app and all six of its targets skipped-with-reason, naming the script that would fix it.
fn measure_app(file: &str) -> App {
    let app = file.trim_end_matches(".wasm").to_string();
    let path = apps_cache_dir().join(file);
    let Ok(bytes) = std::fs::read(&path) else {
        let reason = format!(
            "{} is not in the app cache: run examples/apps/setup.sh",
            display_path(&path)
        );
        return App {
            app,
            file: file.to_string(),
            wasm: Outcome::Skipped {
                reason: reason.clone(),
            },
            targets: backends()
                .into_iter()
                .map(|(name, _)| Cell {
                    target: name.to_string(),
                    outcome: Outcome::Skipped {
                        reason: reason.clone(),
                    },
                })
                .collect(),
        };
    };

    println!("  {file} ({} bytes)", bytes.len());
    let targets = backends()
        .into_iter()
        .map(|(name, backend)| {
            print!("    {name} ");
            let _ = std::io::Write::flush(&mut std::io::stdout());
            let outcome = match generated_bytes(backend, &bytes) {
                Ok(size) => Outcome::Ok { bytes: size },
                Err(err) => Outcome::Failed {
                    reason: format!("{err:#}"),
                },
            };
            match &outcome {
                Outcome::Ok { bytes } => println!("{bytes} bytes"),
                Outcome::Skipped { reason } | Outcome::Failed { reason } => println!("{reason}"),
            }
            Cell {
                target: name.to_string(),
                outcome,
            }
        })
        .collect();

    App {
        app,
        file: file.to_string(),
        wasm: Outcome::Ok {
            bytes: bytes.len() as u64,
        },
        targets,
    }
}

/// Total bytes of the standalone source `backend` generates from `bytes`, across every file it emits: Java returns more than one, and the delivery is all of them.
///
/// Converted on a 64 MiB stack: codegen recurses with the IR's control-flow nesting, and a SQLite-class module's deepest functions overflow the default stack (the same reason `dewasm_test_helper::convert_on_big_stack` exists).
/// Nothing is written to disk; only the byte count is kept, so the several hundred MB a large module generates lives briefly in memory and is dropped.
fn generated_bytes(backend: &'static (dyn Backend + Sync), bytes: &[u8]) -> Result<u64> {
    std::thread::scope(|scope| {
        std::thread::Builder::new()
            .stack_size(64 << 20)
            .spawn_scoped(scope, || -> Result<u64> {
                let module = dewasm_core::build_module(bytes)?;
                let files = backend.generate(
                    &module,
                    &GenOptions {
                        mode: Mode::Standalone,
                        // Only the output file's stem, which this command never writes: a standalone artifact's internal names are fixed.
                        module_name: "prog".to_string(),
                        runtime: RuntimeLinkage::Embedded,
                        default_wasi: true,
                        data_file: None,
                    },
                )?;
                Ok(files.iter().map(|file| file.contents.len() as u64).sum())
            })
            .context("spawn the codegen thread")?
            .join()
            .map_err(|_| anyhow::anyhow!("the codegen thread panicked"))?
    })
}

/// `docs/sizes/`: the generated `results.md` and its figures, beside the hand-written `README.md`.
fn sizes_dir() -> PathBuf {
    docs_dir().join("sizes")
}

fn figs_dir() -> PathBuf {
    sizes_dir().join("figs")
}
