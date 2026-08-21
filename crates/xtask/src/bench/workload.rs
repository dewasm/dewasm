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

/// Deterministic compressible text for `app/minigzip`, ~1.2 MB.
/// A fixed-seed linear congruential generator (Knuth's MMIX constants) picks one of a dozen words each step and joins them with spaces: pure integer arithmetic, so the bytes are exactly reproducible on every platform and run, which the byte-for-byte stdout comparison against wasmtime depends on.
/// 1.2 MB because wasmtime compresses it in ~70 ms, measured, the same order as `app/sqlite3_query`'s ~75 ms baseline.
fn minigzip_input() -> String {
    const WORDS: &[&str] = &[
        "dewasm",
        "minigzip",
        "byte",
        "stdio",
        "stress",
        "lorem",
        "ipsum",
        "dolor",
        "sit",
        "amet",
        "consectetur",
        "wasm",
    ];
    const TARGET_BYTES: usize = 1_200_000;
    let mut state: u64 = 1;
    let mut out = String::with_capacity(TARGET_BYTES + 32);
    while out.len() < TARGET_BYTES {
        state = state
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        let word = WORDS[(state >> 33) as usize % WORDS.len()];
        out.push_str(word);
        out.push(' ');
    }
    out.push('\n');
    out
}

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

/// The declared app cases: `cowsay`, a startup-dominated real program on a mid-sized module where every runner in the matrix competes, `sqlite3_query` for sustained real work, `sqlite3_mod_query`, the same script on the opcode-split build of the same engine, and `minigzip`, a byte-granular compression workload the other three do not exercise.
/// All are timed as whole wall time: an app has no iteration parameter to calibrate, so there is no `t(0)` to subtract.
fn app_workloads() -> Vec<Workload> {
    let cache = apps_cache_dir();
    // `-batch` pins the shell to non-interactive mode.
    // Otherwise it decides from `isatty`, and a runtime that misreports the standard fds runs a different program: pywasm calls every fd a character device (`wasi.py:429`) and got a banner and box-drawing output.
    let sqlite_query = || Kind::App {
        args: ["-batch", ":memory:"].map(String::from).to_vec(),
        stdin: SQLITE_QUERY_SQL.to_string(),
    };
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
            kind: sqlite_query(),
            exclude: SQLITE_QUERY_EXCLUDES,
        },
        // Script, argv and exclusions are the stock case's, so the pair differs only in the artifact: `cache/sqlite3-mod.wasm` carries `examples/apps/src/sqlite3-vdbe-split.patch`, which moves the hot VDBE opcode bodies into their own functions.
        Workload {
            label: "app/sqlite3_mod_query".to_string(),
            wasm: cache.join("sqlite3-mod.wasm"),
            kind: sqlite_query(),
            exclude: SQLITE_QUERY_EXCLUDES,
        },
        Workload {
            label: "app/minigzip".to_string(),
            wasm: cache.join("minigzip.wasm"),
            kind: Kind::App {
                // No argv: minigzip with no arguments compresses stdin to stdout.
                args: Vec::new(),
                stdin: minigzip_input(),
            },
            exclude: MINIGZIP_EXCLUDES,
        },
    ]
}

/// Runners excluded from both SQL query cases, which run the same script on two builds of the same engine.
/// Every reason below is measured, not guessed: an earlier draft guessed "do not finish in a practical time" for all four interpreter entries and was wrong on both counts (wardite fails outright; pywasm runs it fine, just slowly).
/// The measurements were taken on `sqlite3-shell.wasm`; each names the engine rather than the file, because the opcode-split build is the same program.
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
const WARDITE_SQLITE_REASON: &str = "excluded: wardite loads the sqlite3 shell but cannot execute a query, raising Wardite::EvalError (\"maybe empty or invalid stack\", convert.generated.rb:200) as soon as any SQL runs";

/// Cost, not capability: pywasm runs this correctly (byte-identical under `-batch`) at ~17.9 ms/row, so 100k rows is ~half an hour per sample.
/// The row count cannot be lowered to meet it: below ~20k rows wasmtime's side is all process startup and the baseline dissolves.
const PYWASM_SQLITE_REASON: &str = "excluded on cost, not capability: pywasm runs this program correctly (byte-identical to wasmtime under -batch) at ~17.9 ms/row: measured 358 s at 20k rows, so the 100k-row script needs roughly half an hour per sample";

/// Runners excluded from the compression case; each reason is a measurement, not a guess (see the module doc comment on [`SQLITE_QUERY_EXCLUDES`] for why that discipline matters here too).
const MINIGZIP_EXCLUDES: &[(&str, &str)] = &[
    ("dewasm-bash", BASH_MINIGZIP_REASON),
    ("pywasm-cpython", PYWASM_MINIGZIP_REASON),
    ("wardite", WARDITE_MINIGZIP_REASON),
    ("wardite-yjit", WARDITE_MINIGZIP_REASON),
];

/// Extrapolated linearly from two prefix sizes (20000 and 50000 bytes) of the same generated text, both close enough to the per-byte rate that the fit is not just two points hiding curvature.
/// At 6 runs per cell (one warmup plus 5 reps) that is over an hour on this workload alone, which is why the input size is fixed for wasmtime rather than calibrated down to fit bash: the doc comment on [`minigzip_input`] gives the reason it must stay put.
const BASH_MINIGZIP_REASON: &str = "excluded on cost, not capability: bash compresses this workload's generated text at ~0.61 ms/byte (measured 30.6 s on a 50000-byte prefix), so the full 1.2 MB input needs roughly 12 minutes per run";

/// Extrapolated the same way as [`BASH_MINIGZIP_REASON`], from prefixes of 5000 and 20000 bytes.
/// pypy runs the same driver roughly 6.6x faster (measured 84 s on the full 1.2 MB input, byte-identical to wasmtime), which is why only the CPython interpreter is excluded here.
const PYWASM_MINIGZIP_REASON: &str = "excluded on cost, not capability: pywasm under CPython compresses this workload's generated text at ~0.46 ms/byte (measured 9.2 s on a 20000-byte prefix), so the full 1.2 MB input needs roughly 9 minutes per run";

/// wardite runs the compression correctly (byte-identical to wasmtime, verified on small inputs) but the driver crashes on exit: minigzip closes stdout at the end of the run, wardite's `fd_close` closes the underlying host fd along with it, and the driver's own trailing `$stdout.flush` then raises `IOError: closed stream`, so the process exits 1 and the harness's success check fails every run.
const WARDITE_MINIGZIP_REASON: &str = "excluded: wardite computes the correct compressed output but its driver crashes on exit (IOError: closed stream at wardite.rb:40) because minigzip closes stdout itself and wardite's fd_close closes the real fd under it, so the driver's own trailing flush fails and the process exits 1";
