//! What gets benchmarked: the two workload kinds and how the suite finds them.
//!
//! **Microbenchmarks** live in two families (hand-written `benchmarks/wat/`, zig-cc-compiled `benchmarks/c/`), each built by its own `build.sh` into `benchmarks/cache/<family>/`.
//! The contract: `<module> <iterations>` does that many units of work, prints exactly one decimal result line, exits 0; `<iterations> = 0` does no work but still prints, which is what lets [`crate::bench::measure`] separate startup + load from compute.
//! Same `<iterations>` must give byte-identical stdout on every runtime: the wasmtime cross-check enforces it.
//!
//! The set is discovered from disk; the table below only adds per-workload iteration caps. **Apps** are real cached programs with fixed argv/stdin, wall time only, hand-declared.

use std::path::PathBuf;

use crate::bench::{apps_cache_dir, bench_cache_dir, display_path};

/// Cap for a microbenchmark absent from [`MICRO_ITER_CAPS`]: a ceiling on calibration, not a target.
const DEFAULT_ITER_CAP: u64 = 100_000_000;

/// The families; each prefix names the source directory, the cache subdirectory, and the build script.
const MICRO_FAMILIES: &[&str] = &["wat", "c"];

/// Per-workload calibration **ceilings**, not fixed counts: they bind only the fastest runners, and exist to stop a workload with non-constant per-iteration cost from being driven absurdly far.
/// Set to roughly 3x what wasmtime needs for the default 300 ms target; retune when a body changes.
const MICRO_ITER_CAPS: &[(&str, u64)] = &[
    ("wat/i32_alu", 500_000_000),
    ("wat/i64_alu", 500_000_000),
    ("wat/f64_alu", 500_000_000),
    ("wat/mem_rw", 500_000_000),
    ("wat/call_direct", 500_000_000),
    ("wat/call_indirect", 500_000_000),
    ("c/sha256", 10_000_000),
    ("c/mandelbrot", 20_000_000),
    ("c/wordcount", 500_000_000),
];

/// The `sqlite3_query` script: a 100k-row table in one transaction (recursive CTE, so the work is the engine's), then an aggregate and a `LIKE` scan. 100k rows because at 20k wasmtime finished in ~30 ms (nearly all process startup), leaving the baseline unresolvable.
/// The script is fixed rather than calibrated per runner (that is what makes it realistic), which is why the slowest runners are excluded instead of measured at their own size.
const SQLITE_QUERY_SQL: &str = "\
.bail on
PRAGMA journal_mode = memory;
CREATE TABLE t(id INTEGER PRIMARY KEY, name TEXT, v REAL);
BEGIN;
INSERT INTO t(id, name, v)
  WITH RECURSIVE seq(i) AS (SELECT 1 UNION ALL SELECT i + 1 FROM seq WHERE i < 100000)
  SELECT i, 'row-' || i, i * 1.5 FROM seq;
COMMIT;
SELECT count(*), sum(v), avg(v) FROM t;
SELECT count(*) FROM t WHERE name LIKE '%7%';
.quit
";

pub enum Kind {
    /// A `<module> <iterations>` microbenchmark from either family (`wat`, `c`).
    /// `iter_cap` bounds the harness's calibration.
    Micro { iter_cap: u64 },
    /// A real cached app: fixed argv, optional stdin, no iteration parameter.
    /// Timed as whole wall time, so there is no zero run.
    App { args: Vec<String>, stdin: String },
}

pub struct Workload {
    /// The filter/report label, e.g. `wat/i32_alu`, `c/sha256` or `app/sqlite3_query`.
    /// The part before the slash is the family, which is also the source directory for a microbenchmark.
    pub label: String,
    /// Path to the `.wasm`; may not exist yet (see [`Workload::missing_reason`]).
    pub wasm: PathBuf,
    pub kind: Kind,
    /// Runner labels this workload deliberately does not run on, each with the reason reported in the JSON and the doc.
    /// Never a silent omission (a gap is stated, not hidden).
    pub exclude: &'static [(&'static str, &'static str)],
}

impl Workload {
    /// `Some(reason)` when the module is not on disk, phrased as the setup command that produces it.
    pub fn missing_reason(&self) -> Option<String> {
        if self.wasm.is_file() {
            return None;
        }
        let build = match self.kind {
            Kind::Micro { .. } => format!("benchmarks/{}/build.sh", self.family()),
            Kind::App { .. } => "examples/apps/setup.sh".to_string(),
        };
        Some(format!(
            "{} not built: run {build}",
            display_path(&self.wasm)
        ))
    }

    /// The label's family segment: `wat`, `c` or `app`.
    pub fn family(&self) -> &str {
        self.label.split('/').next().unwrap_or(&self.label)
    }

    pub fn excluded(&self, runner: &str) -> Option<&'static str> {
        self.exclude
            .iter()
            .find(|(label, _)| *label == runner)
            .map(|(_, reason)| *reason)
    }
}

/// Every workload the suite knows about: the declared microbenchmarks unioned with whatever `benchmarks/cache/<family>/` actually holds, then the declared app cases.
/// One present on disk but absent from the table is included with the default cap; one in the table but absent from disk is included as missing so `--list` names it.
pub fn workloads() -> Vec<Workload> {
    let mut ids: Vec<String> = MICRO_ITER_CAPS
        .iter()
        .map(|(id, _)| (*id).to_string())
        .collect();
    for found in discovered_micro_ids() {
        if !ids.contains(&found) {
            ids.push(found);
        }
    }
    ids.sort();

    let cache = bench_cache_dir();
    let mut out: Vec<Workload> = ids
        .into_iter()
        .map(|id| {
            let iter_cap = MICRO_ITER_CAPS
                .iter()
                .find(|(known, _)| *known == id)
                .map_or(DEFAULT_ITER_CAP, |(_, cap)| *cap);
            Workload {
                wasm: cache.join(format!("{id}.wasm")),
                label: id,
                kind: Kind::Micro { iter_cap },
                exclude: &[],
            }
        })
        .collect();
    out.extend(app_workloads());
    out
}

/// The `<family>/<stem>` ids actually present under `benchmarks/cache/`, walking each family's own subdirectory.
/// A missing directory is not an error here: it just means that family is not built yet, which `--list` and the per-workload `missing_reason` report with the build command.
fn discovered_micro_ids() -> Vec<String> {
    let cache = bench_cache_dir();
    MICRO_FAMILIES
        .iter()
        .flat_map(|family| {
            let entries = std::fs::read_dir(cache.join(family)).ok();
            entries
                .into_iter()
                .flatten()
                .flatten()
                .map(|entry| entry.path())
                .filter(|path| path.extension().is_some_and(|ext| ext == "wasm"))
                .filter_map(move |path| {
                    let stem = path.file_stem()?.to_str()?;
                    Some(format!("{family}/{stem}"))
                })
        })
        .collect()
}

/// The declared app cases: `cowsay`, a startup-dominated real program on a mid-sized module where every runner in the matrix competes, and `sqlite3_query` for sustained real work.
/// Both are timed as whole wall time: an app has no iteration parameter to calibrate, so there is no `t(0)` to subtract.
fn app_workloads() -> Vec<Workload> {
    let cache = apps_cache_dir();
    vec![
        Workload {
            label: "app/cowsay".to_string(),
            wasm: cache.join("cowsay.wasm"),
            kind: Kind::App {
                // The message goes in on stdin (cowsay reads stdin when given no argv).
                args: Vec::new(),
                stdin: "Hello from dewasm!\n".to_string(),
            },
            exclude: &[],
        },
        Workload {
            label: "app/sqlite3_query".to_string(),
            wasm: cache.join("sqlite3-shell.wasm"),
            kind: Kind::App {
                // `-batch` pins the shell to non-interactive mode.
                // Otherwise it decides from `isatty`, and a runtime that misreports the standard fds runs a different program: pywasm calls every fd a character device (`wasi.py:429`) and got a banner and box-drawing output.
                args: ["-batch", ":memory:"].map(String::from).to_vec(),
                stdin: SQLITE_QUERY_SQL.to_string(),
            },
            exclude: SQLITE_QUERY_EXCLUDES,
        },
    ]
}

/// Runners excluded from the SQL query case.
/// Every reason below is measured, not guessed: an earlier draft guessed "do not finish in a practical time" for all four interpreter entries and was wrong on both counts (wardite fails outright; pywasm runs it fine, just slowly).
const SQLITE_QUERY_EXCLUDES: &[(&str, &str)] = &[
    (
        "dewasm-bash",
        "excluded: bash runs ~10000x slower than wasmtime on compute, so 100k SQL inserts do not finish in a practical time",
    ),
    ("pywasm-cpython", PYWASM_SQLITE_REASON),
    ("pywasm-pypy", PYWASM_SQLITE_REASON),
    ("wardite", WARDITE_SQLITE_REASON),
    ("wardite-yjit", WARDITE_SQLITE_REASON),
];

/// wardite loads the module and handles a bare `.quit`, but any actual query dies.
const WARDITE_SQLITE_REASON: &str = "excluded: wardite loads sqlite3-shell.wasm but cannot execute a query, raising Wardite::EvalError (\"maybe empty or invalid stack\", convert.generated.rb:200) as soon as any SQL runs";

/// Cost, not capability: pywasm runs this correctly (byte-identical under `-batch`) at ~17.9 ms/row, so 100k rows is ~half an hour per sample.
/// The row count cannot be lowered to meet it: below ~20k rows wasmtime's side is all process startup and the baseline dissolves.
const PYWASM_SQLITE_REASON: &str = "excluded on cost, not capability: pywasm runs this program correctly (byte-identical to wasmtime under -batch) at ~17.9 ms/row: measured 358 s at 20k rows, so the 100k-row script needs roughly half an hour per sample";
