//! `cargo xtask bench` — the cross-runtime benchmark suite.
//!
//! It answers one question with numbers: what does a wasm program cost once dewasm has turned it into Ruby, Python, Perl, Go, Java, or Bash source, measured against the AOT ceiling (wasmtime) and against the wasm interpreters written in those same languages (pywasm, wardite). Two outputs come out of one command: a dated result file under `benchmarks/results/` and a generated `docs/benchmarks.md`. Neither is a compared snapshot — a timing is not reproducible byte-for-byte, so unlike `docs/support.md` there is no freshness test (contrast ADR-56's execution snapshots).
//!
//! The layout mirrors what has to be got right:
//!
//! * [`workload`] — what is measured: `<module> <iterations>` kernels discovered from `benchmarks/cache/`, and real cached apps with fixed argv/stdin (SQLite doing real SQL, and the same shell fed only `.quit` as a cold-start case).
//! * [`runner`] — where it is measured: availability probing, dewasm codegen through the [`Backend`](dewasm_backend::Backend) trait, and the `go build` / `javac` steps the compiled backends need.
//! * [`measure`] — how it is measured: per-runner iteration calibration, the subtracted `<iterations> = 0` run, repetitions reported as min *and* median, and a hard timeout.
//! * [`report`] — the JSON record and the markdown rendering of it.
//!
//! Two rules run through all of it. Every runner's stdout is diffed against wasmtime's at the same iteration count, and a mismatch is a **hard failure** that makes the command exit non-zero — a wrong answer produced quickly is not a result. And nothing is silently dropped: an uninstalled runner, an unbuilt module, and a deliberately excluded pair are each reported with a reason in both outputs, so an empty cell can never be mistaken for a covered one (ADR-15's fail-loud-not-skip policy).

mod measure;
mod report;
mod runner;
mod workload;

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::{bail, Context, Result};

use crate::bench::measure::{calibrate, describe_diff, repeat, run_once, stats};
use crate::bench::report::{Cell, Measurement, Outcome, Samples, Verification};
use crate::bench::runner::{Kind, Launch, Runner, Workshop};
use crate::bench::workload::Workload;

/// Default timed repetitions per measurement, after one discarded warmup.
const DEFAULT_REPS: usize = 5;
/// Default compute time the iteration calibrator aims each sample at.
const DEFAULT_TARGET_MS: u64 = 300;
/// Default per-process wall-clock ceiling. Generous — a Bash sample legitimately takes minutes — but finite, so a runner that turns out slower than expected costs one timeout instead of hanging the suite.
const DEFAULT_TIMEOUT_S: u64 = 900;

/// Parsed `cargo xtask bench` arguments.
struct Options {
    filter: Option<String>,
    reps: usize,
    target: Duration,
    timeout: Duration,
    list: bool,
}

impl Options {
    fn parse(args: impl Iterator<Item = String>) -> Result<Self> {
        let mut opts = Options {
            filter: None,
            reps: DEFAULT_REPS,
            target: Duration::from_millis(DEFAULT_TARGET_MS),
            timeout: Duration::from_secs(DEFAULT_TIMEOUT_S),
            list: false,
        };
        let mut args = args.peekable();
        while let Some(arg) = args.next() {
            let mut value = |name: &str| -> Result<String> {
                args.next().with_context(|| format!("{name} needs a value"))
            };
            match arg.as_str() {
                "--list" => opts.list = true,
                "--reps" => {
                    opts.reps = value("--reps")?
                        .parse()
                        .context("--reps must be an integer")?;
                    if opts.reps == 0 {
                        bail!("--reps must be at least 1");
                    }
                }
                "--target-ms" => {
                    opts.target = Duration::from_millis(
                        value("--target-ms")?
                            .parse()
                            .context("--target-ms must be an integer")?,
                    );
                }
                "--timeout" => {
                    opts.timeout = Duration::from_secs(
                        value("--timeout")?
                            .parse()
                            .context("--timeout must be an integer number of seconds")?,
                    );
                }
                other if other.starts_with('-') => bail!("unknown bench option: {other}"),
                other if opts.filter.is_none() => opts.filter = Some(other.to_string()),
                other => bail!("bench takes at most one filter (got a second: {other})"),
            }
        }
        Ok(opts)
    }
}

/// Entry point for the `bench` subcommand.
pub fn main(args: impl Iterator<Item = String>) -> Result<()> {
    let opts = Options::parse(args)?;
    let runners = runner::runners();
    let workloads = workload::workloads();

    if !workloads
        .iter()
        .any(|workload| matches(&opts.filter, workload, &runners))
    {
        match &opts.filter {
            Some(needle) => bail!("no workload or runner label matched filter {needle:?}"),
            None => bail!("no workloads: neither benchmarks/cache/ nor the app cache has anything"),
        }
    }

    if opts.list {
        return list(&opts, &runners, &workloads);
    }
    run(&opts, &runners, &workloads)
}

/// Whether `workload` has at least one runner selected by `filter`. A filter matches on either side of the pair: it can name a workload (`sqlite`), a runner (`bash`), or a family (`kernel/`).
fn matches(filter: &Option<String>, workload: &Workload, runners: &[Runner]) -> bool {
    let Some(needle) = filter else {
        return true;
    };
    workload.label.contains(needle.as_str())
        || runners
            .iter()
            .any(|runner| runner.label.contains(needle.as_str()))
}

/// Whether one (workload, runner) pair is selected.
fn pair_matches(filter: &Option<String>, workload: &Workload, runner: &Runner) -> bool {
    filter.as_ref().is_none_or(|needle| {
        workload.label.contains(needle.as_str()) || runner.label.contains(needle.as_str())
    })
}

/// `--list`: the matrix and each runner's availability, without running anything.
fn list(opts: &Options, runners: &[Runner], workloads: &[Workload]) -> Result<()> {
    println!("Runners ({}):", runners.len());
    for runner in runners {
        match runner.availability() {
            Ok(()) => println!(
                "  {:<16} available    {}",
                runner.label,
                runner.version().unwrap_or_else(|| "?".to_string())
            ),
            Err(reason) => println!("  {:<16} unavailable  {reason}", runner.label),
        }
    }

    println!("\nWorkloads ({}):", workloads.len());
    for workload in workloads {
        match workload.missing_reason() {
            None => println!(
                "  {:<24} ready        {}",
                workload.label,
                describe(workload)
            ),
            Some(reason) => println!("  {:<24} missing      {reason}", workload.label),
        }
    }

    let excluded: Vec<(&str, &str, &str)> = workloads
        .iter()
        .flat_map(|workload| {
            workload
                .exclude
                .iter()
                .map(move |(runner, reason)| (workload.label.as_str(), *runner, *reason))
        })
        .collect();
    if !excluded.is_empty() {
        println!("\nDeclared exclusions ({}):", excluded.len());
        for (workload, runner, reason) in excluded {
            println!("  {workload} x {runner}: {reason}");
        }
    }

    let selected = workloads
        .iter()
        .flat_map(|workload| {
            runners
                .iter()
                .filter(move |runner| pair_matches(&opts.filter, workload, runner))
        })
        .count();
    println!(
        "\n{selected} pair(s) selected by filter {:?}; wasmtime is additionally measured for every \
         workload that runs, as the baseline and the correctness reference.",
        opts.filter.as_deref().unwrap_or("<none>")
    );
    Ok(())
}

/// One line describing a ready workload for `--list`.
fn describe(workload: &Workload) -> String {
    match &workload.kind {
        workload::Kind::Kernel { iter_cap } => {
            format!("kernel, calibrated up to a cap of {iter_cap} iterations")
        }
        workload::Kind::App { args, stdin, .. } => {
            format!("app, argv {args:?}, {} bytes of stdin", stdin.len())
        }
    }
}

/// The measuring run: every selected pair, then the two output files.
fn run(opts: &Options, runners: &[Runner], workloads: &[Workload]) -> Result<()> {
    // wasmtime is not optional: it is both the ceiling every ratio is taken against and the oracle every runner's stdout is diffed against. Without it the suite would produce numbers with nothing to check them, so it fails loud instead (ADR-15).
    if let Err(reason) = runners
        .iter()
        .find(|runner| matches!(runner.kind, Kind::Wasmtime))
        .expect("the matrix always contains wasmtime")
        .availability()
    {
        bail!("{reason} — the benchmark suite needs wasmtime as its baseline and its correctness reference");
    }

    let runtimes: Vec<report::Runtime> = runners
        .iter()
        .map(|runner| {
            let availability = runner.availability();
            report::Runtime {
                runner: runner.label.to_string(),
                available: availability.is_ok(),
                unavailable_reason: availability.err(),
                version: runner.version(),
            }
        })
        .collect();

    let mut workshop = Workshop::default();
    let mut results: Vec<Cell> = Vec::new();
    for workload in workloads {
        if !matches(&opts.filter, workload, runners) {
            continue;
        }
        measure_workload(
            opts,
            runners,
            workload,
            &runtimes,
            &mut workshop,
            &mut results,
        );
    }

    let generated_at = utc_timestamp();
    let report = report::Report {
        schema: 1,
        host: host_info(),
        settings: report::Settings {
            reps: opts.reps,
            target_ms: opts.target.as_millis() as u64,
            timeout_s: opts.timeout.as_secs(),
            filter: opts.filter.clone(),
        },
        runtimes,
        results,
        generated_at: generated_at.clone(),
    };

    let json_path = results_dir().join(format!("{}.json", generated_at.replace(':', "-")));
    write_file(&json_path, &report.to_json()?)?;
    let doc_path = docs_dir().join("benchmarks.md");
    write_file(&doc_path, &report::render_doc(&report))?;

    let failures: Vec<&Cell> = report
        .results
        .iter()
        .filter(|cell| matches!(cell.outcome, Outcome::Failed { .. }))
        .collect();
    if failures.is_empty() {
        return Ok(());
    }
    for cell in &failures {
        if let Outcome::Failed { reason } = &cell.outcome {
            eprintln!("FAILED {} on {}: {reason}", cell.workload, cell.runner);
        }
    }
    bail!(
        "{} benchmark cell(s) failed; the results were still written to {}",
        failures.len(),
        json_path.display()
    );
}

/// Measure every selected runner for one workload, appending a [`Cell`] per pair — including the skips, which is what keeps a gap from reading as coverage.
fn measure_workload(
    opts: &Options,
    runners: &[Runner],
    workload: &Workload,
    runtimes: &[report::Runtime],
    workshop: &mut Workshop,
    results: &mut Vec<Cell>,
) {
    // wasmtime always runs: it supplies the baseline ratio and the reference stdout, whatever the filter selected.
    let selected: Vec<&Runner> = runners
        .iter()
        .filter(|runner| {
            matches!(runner.kind, Kind::Wasmtime) || pair_matches(&opts.filter, workload, runner)
        })
        .collect();

    if let Some(reason) = workload.missing_reason() {
        for runner in selected {
            results.push(skipped(workload, runner, reason.clone()));
        }
        return;
    }

    // The wasmtime stdout for each iteration count another runner calibrated to, memoized so N runners at the same N cost one reference run.
    let mut references: HashMap<u64, Vec<u8>> = HashMap::new();
    for runner in selected {
        if let Some(reason) = runtimes
            .iter()
            .find(|rt| rt.runner == runner.label)
            .and_then(|rt| rt.unavailable_reason.clone())
        {
            results.push(skipped(workload, runner, reason));
            continue;
        }
        if let Some(reason) = workload.excluded(runner.label) {
            results.push(skipped(workload, runner, reason.to_string()));
            continue;
        }
        println!("  {:<24} {}", workload.label, runner.label);
        let outcome = match measure_cell(opts, workload, runner, workshop, &mut references) {
            Ok(outcome) => outcome,
            Err(err) => Outcome::Failed {
                reason: format!("{err:#}"),
            },
        };
        if let Outcome::Ok(m) = &outcome {
            let ms = |seconds: f64| (seconds * 1000.0).round();
            let iters = m
                .iterations
                .map_or_else(String::new, |n| format!("{n} iters, "));
            let cold = m
                .cold_start
                .as_ref()
                .map_or_else(String::new, |s| format!(", cold start {} ms", ms(s.min_s)));
            println!("    {iters}{} ms total (min){cold}", ms(m.total.min_s));
        }
        results.push(Cell {
            workload: workload.label.clone(),
            runner: runner.label.to_string(),
            outcome,
        });
    }
}

fn skipped(workload: &Workload, runner: &Runner, reason: String) -> Cell {
    Cell {
        workload: workload.label.clone(),
        runner: runner.label.to_string(),
        outcome: Outcome::Skipped { reason },
    }
}

/// The whole measurement of one (workload, runner) pair: time the zero run, calibrate the iteration count against it, take the timed repetitions, then diff the resulting stdout against wasmtime at the same count.
fn measure_cell(
    opts: &Options,
    workload: &Workload,
    runner: &Runner,
    workshop: &mut Workshop,
    references: &mut HashMap<u64, Vec<u8>>,
) -> Result<Outcome> {
    let launch = workshop.launch(runner, &workload.wasm, &workload.module_name)?;
    let (iterations, zero, total, last) = match &workload.kind {
        workload::Kind::Kernel { iter_cap } => {
            let (zero_samples, _) = repeat(
                &launch,
                &["0".to_string()],
                b"",
                opts.reps,
                opts.timeout,
                "the zero run",
            )?;
            let (zero_min, zero_median) = stats(&zero_samples);
            let iterations = calibrate(
                |n| run_once(&launch, &[n.to_string()], b"", opts.timeout),
                zero_min,
                opts.target,
                *iter_cap,
            )?;
            let (samples, last) = repeat(
                &launch,
                &[iterations.to_string()],
                b"",
                opts.reps,
                opts.timeout,
                "the timed run",
            )?;
            (
                Some(iterations),
                Some(samples_of(zero_min, zero_median, zero_samples)),
                samples_of_slice(&samples),
                last,
            )
        }
        workload::Kind::App {
            args,
            stdin,
            zero_stdin,
        } => {
            let zero = match zero_stdin {
                Some(script) => {
                    let (samples, _) = repeat(
                        &launch,
                        args,
                        script.as_bytes(),
                        opts.reps,
                        opts.timeout,
                        "the do-nothing run",
                    )?;
                    Some(samples_of_slice(&samples))
                }
                None => None,
            };
            let (samples, last) = repeat(
                &launch,
                args,
                stdin.as_bytes(),
                opts.reps,
                opts.timeout,
                "the timed run",
            )?;
            (None, zero, samples_of_slice(&samples), last)
        }
    };

    let verification = if matches!(runner.kind, Kind::Wasmtime) {
        // The reference run for this iteration count is already in hand: it is this very measurement.
        references.insert(iterations.unwrap_or(0), last.stdout.clone());
        Verification::Reference
    } else {
        let expected = reference_stdout(workload, iterations, opts, references)?;
        if expected != last.stdout {
            return Ok(Outcome::Failed {
                reason: describe_diff(&expected, &last.stdout),
            });
        }
        Verification::Match
    };

    let compute_s = zero
        .as_ref()
        .map(|z| total.min_s - z.min_s)
        .filter(|compute| *compute > 0.0);
    let compute_median = zero
        .as_ref()
        .map(|z| total.median_s - z.median_s)
        .filter(|compute| *compute > 0.0);
    let per_op = |seconds: Option<f64>| match (seconds, iterations) {
        (Some(seconds), Some(n)) if n > 0 => Some(seconds * 1e9 / n as f64),
        _ => None,
    };

    Ok(Outcome::Ok(Measurement {
        iterations,
        reps: opts.reps,
        cold_start: zero,
        total,
        compute_s,
        ns_per_op_min: per_op(compute_s),
        ns_per_op_median: per_op(compute_median),
        load_ms: last.load_ms(),
        verification,
    }))
}

/// wasmtime's stdout for `workload` at `iterations`, run on demand and memoized. This is what makes the cross-check exact: the reference is produced at the *same* iteration count the runner was benchmarked at, not at some separate nominal count.
fn reference_stdout(
    workload: &Workload,
    iterations: Option<u64>,
    opts: &Options,
    references: &mut HashMap<u64, Vec<u8>>,
) -> Result<Vec<u8>> {
    let key = iterations.unwrap_or(0);
    if let Some(cached) = references.get(&key) {
        return Ok(cached.clone());
    }
    let launch = Launch {
        program: runner::wasmtime_bin().context("wasmtime not found on PATH")?,
        args: vec![
            "run".to_string(),
            workload.wasm.to_string_lossy().into_owned(),
        ],
        env: Vec::new(),
    };
    let (args, stdin): (Vec<String>, &[u8]) = match (&workload.kind, iterations) {
        (workload::Kind::Kernel { .. }, Some(n)) => (vec![n.to_string()], b""),
        (workload::Kind::App { args, stdin, .. }, _) => (args.clone(), stdin.as_bytes()),
        _ => bail!("internal: a kernel workload must carry an iteration count"),
    };
    let outcome = run_once(&launch, &args, stdin, opts.timeout)?;
    outcome.require_success("the wasmtime reference run")?;
    references.insert(key, outcome.stdout.clone());
    Ok(outcome.stdout)
}

fn samples_of_slice(samples: &[f64]) -> Samples {
    let (min, median) = stats(samples);
    samples_of(min, median, samples.to_vec())
}

fn samples_of(min_s: f64, median_s: f64, samples_s: Vec<f64>) -> Samples {
    Samples {
        min_s,
        median_s,
        samples_s,
    }
}

// ------------------------------------------------------------------ Paths.

/// The repo root, canonicalized so the `crates/xtask/../..` spelling never reaches a message a human reads.
fn repo_root() -> PathBuf {
    let raw = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    raw.canonicalize().unwrap_or(raw)
}

/// `path` relative to the repo root when it is inside it, for readable diagnostics ("benchmarks/cache/i32_alu.wasm not built" rather than an absolute path with `../..` in the middle).
pub fn display_path(path: &Path) -> String {
    path.strip_prefix(repo_root())
        .unwrap_or(path)
        .display()
        .to_string()
}

/// `benchmarks/`, the suite's own tree: `kernels/` (sources + `build.sh`), `cache/` (built kernels, the pywasm venv, the wardite `GEM_HOME`), `drivers/`, `results/`.
fn bench_root() -> PathBuf {
    repo_root().join("benchmarks")
}

/// `benchmarks/cache/` — built kernel modules *and* the interpreter dependencies `benchmarks/setup.sh` provisions.
pub fn bench_cache_dir() -> PathBuf {
    bench_root().join("cache")
}

/// `benchmarks/drivers/` — the pywasm and wardite driver scripts.
pub fn drivers_dir() -> PathBuf {
    bench_root().join("drivers")
}

fn results_dir() -> PathBuf {
    bench_root().join("results")
}

fn docs_dir() -> PathBuf {
    repo_root().join("docs")
}

/// `examples/apps/cache/`, where the app workloads' modules live (ADR-9).
pub fn apps_cache_dir() -> PathBuf {
    dewasm_test_helper::apps_cache_dir()
}

fn write_file(path: &Path, contents: &str) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }
    std::fs::write(path, contents)
        .with_context(|| format!("failed to write {}", path.display()))?;
    println!("wrote {} ({} bytes)", path.display(), contents.len());
    Ok(())
}

// ------------------------------------------------------------------ Host description.

fn host_info() -> report::Host {
    report::Host {
        os: os_description(),
        arch: std::env::consts::ARCH.to_string(),
        kernel: shell_line("uname", &["-sr"]).unwrap_or_else(|| "unknown".to_string()),
        cpu: cpu_description(),
    }
}

fn os_description() -> String {
    if let (Some(name), Some(version)) = (
        shell_line("sw_vers", &["-productName"]),
        shell_line("sw_vers", &["-productVersion"]),
    ) {
        return format!("{name} {version}");
    }
    if let Ok(release) = std::fs::read_to_string("/etc/os-release") {
        if let Some(pretty) = release
            .lines()
            .find_map(|line| line.strip_prefix("PRETTY_NAME="))
        {
            return pretty.trim_matches('"').to_string();
        }
    }
    std::env::consts::OS.to_string()
}

fn cpu_description() -> String {
    if let Some(brand) = shell_line("sysctl", &["-n", "machdep.cpu.brand_string"]) {
        return brand;
    }
    if let Ok(info) = std::fs::read_to_string("/proc/cpuinfo") {
        if let Some(model) = info
            .lines()
            .find_map(|line| line.strip_prefix("model name"))
        {
            return model.trim_start_matches([':', ' ']).to_string();
        }
    }
    "unknown".to_string()
}

/// The first non-empty stdout line of `program args...`, or `None` if it does not run here.
fn shell_line(program: &str, args: &[&str]) -> Option<String> {
    let out = std::process::Command::new(program)
        .args(args)
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .map(str::to_string)
}

// ------------------------------------------------------------------ Time.

/// The current UTC time as `YYYY-MM-DDTHH:MM:SSZ`. Hand-rolled rather than pulling `chrono` in for one timestamp in a dev tool.
fn utc_timestamp() -> String {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |since| since.as_secs());
    let (days, rest) = ((secs / 86_400) as i64, secs % 86_400);
    let (year, month, day) = civil_from_days(days);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}Z",
        rest / 3600,
        (rest / 60) % 60,
        rest % 60
    )
}

/// Days since the Unix epoch to a civil `(year, month, day)` — Howard Hinnant's `civil_from_days`, which is exact for the whole proleptic Gregorian range.
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let shifted = days + 719_468;
    let era = if shifted >= 0 {
        shifted
    } else {
        shifted - 146_096
    } / 146_097;
    let day_of_era = (shifted - era * 146_097) as u64;
    let year_of_era =
        (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let year = year_of_era as i64 + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_index = (5 * day_of_year + 2) / 153;
    let day = (day_of_year - (153 * month_index + 2) / 5 + 1) as u32;
    let month = if month_index < 10 {
        month_index + 3
    } else {
        month_index - 9
    } as u32;
    (if month <= 2 { year + 1 } else { year }, month, day)
}
