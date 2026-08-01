//! What gets benchmarked: the two workload kinds and how the suite finds them.
//!
//! **Kernels** are WASI command modules built into `benchmarks/cache/` by `benchmarks/kernels/build.sh`. Their contract is fixed: invoked as `<module> <iterations>` with a non-negative decimal integer, they do that many units of work, print exactly one decimal result line, and exit 0 — and `<iterations> = 0` still prints a result line while doing no work. That zero run is what makes the whole comparison honest (see [`crate::bench::measure`]): it isolates process startup + module load from compute. Same `<iterations>` must give byte-identical stdout on every runtime, which is what the cross-check against wasmtime verifies.
//!
//! The kernel set is **discovered from disk**, not hardcoded, so `benchmarks/kernels/` can grow without touching this file. The declared table below only supplies a per-kernel *iteration cap* (and makes a not-yet-built kernel show up in `--list` as missing rather than silently absent); a kernel found on disk without a table entry gets [`DEFAULT_ITER_CAP`].
//!
//! **Apps** are the real cached programs from `examples/apps/cache/` with fixed argv and optional stdin — no iteration parameter, wall time only. They are a declared table because each one is a hand-chosen argv/stdin pair, not a discoverable artifact.

use std::path::PathBuf;

use crate::bench::{apps_cache_dir, bench_cache_dir, display_path};

/// The iteration cap for a kernel found on disk but absent from [`KERNEL_ITER_CAPS`]. Only a ceiling: the harness calibrates upward from a tiny count and stops at whichever comes first, the per-runner time target or this cap.
const DEFAULT_ITER_CAP: u64 = 100_000_000;

/// Per-kernel iteration caps, in units of the kernel's own `<iterations>` parameter. A **ceiling**, not a fixed count: the harness calibrates each runner separately (a single fixed count is useless across a ~1000x spread of runner speeds) and stops at whichever comes first, the compute target or this cap.
///
/// So the cap only ever binds the *fastest* runners — nothing else gets near it — and its job is to stop a kernel whose per-iteration cost is not constant (one that allocates, or grows a buffer) from being driven somewhere silly. Each entry is generous enough that wasmtime still reaches the compute target: the micro-kernels' unit is one trip through a tight loop (~2 ns there), the C kernels' unit is a whole hash / image / document pass. These are first-cut numbers — retune them against the real kernels once `benchmarks/kernels/build.sh` has produced them.
const KERNEL_ITER_CAPS: &[(&str, u64)] = &[
    ("i32_alu", 500_000_000),
    ("i64_alu", 500_000_000),
    ("f64_alu", 500_000_000),
    ("mem_rw", 500_000_000),
    ("call_direct", 500_000_000),
    ("call_indirect", 500_000_000),
    ("sha256", 2_000_000),
    ("mandelbrot", 200_000),
    ("wordcount", 2_000_000),
];

/// The SQL script the `sqlite3_query` app case feeds to `sqlite3-shell.wasm`: build a 20k-row table inside one transaction (a recursive CTE, so the work is the engine's, not the parser's), then an aggregate and a `LIKE` scan over it. `.quit` is implicit at EOF, but stating it keeps the two sqlite cases visibly the same shape.
const SQLITE_QUERY_SQL: &str = "\
.bail on
PRAGMA journal_mode = memory;
CREATE TABLE t(id INTEGER PRIMARY KEY, name TEXT, v REAL);
BEGIN;
INSERT INTO t(id, name, v)
  WITH RECURSIVE seq(i) AS (SELECT 1 UNION ALL SELECT i + 1 FROM seq WHERE i < 20000)
  SELECT i, 'row-' || i, i * 1.5 FROM seq;
COMMIT;
SELECT count(*), sum(v), avg(v) FROM t;
SELECT count(*) FROM t WHERE name LIKE '%7%';
.quit
";

/// The cold-start script: load the 1.3 MB module, start the shell, quit. No SQL at all, so the whole wall time is process startup + module load + instantiation — the one axis on which an interpreter legitimately beats an AOT compiler, and the reason `t(0)` is reported as its own column everywhere.
const SQLITE_QUIT_SQL: &str = ".quit\n";

/// What a workload is and how to invoke it.
pub enum Kind {
    /// A `<module> <iterations>` micro/C kernel. `iter_cap` bounds the harness's calibration.
    Kernel { iter_cap: u64 },
    /// A real cached app: fixed argv, optional stdin, no iteration parameter.
    App {
        args: Vec<String>,
        stdin: String,
        /// The app's own "do nothing" analogue, if it has one — for the sqlite query case, the same shell fed only `.quit`. Timed and subtracted exactly like a kernel's `<iterations> = 0` run; `None` means the app *is* its own cold-start measurement and no compute time is derived.
        zero_stdin: Option<String>,
    },
}

/// One benchmarkable program.
pub struct Workload {
    /// The filter/report label, e.g. `kernel/i32_alu` or `app/sqlite3_query`.
    pub label: String,
    /// The name handed to the backend as the generated module/class name.
    pub module_name: String,
    /// Path to the `.wasm`; may not exist yet (see [`Workload::missing_reason`]).
    pub wasm: PathBuf,
    pub kind: Kind,
    /// Runner labels this workload deliberately does not run on, each with the reason reported in the JSON and the doc. Never a silent omission (ADR-15's spirit: a gap is stated, not hidden).
    pub exclude: &'static [(&'static str, &'static str)],
}

impl Workload {
    /// `Some(reason)` when the module is not on disk, phrased as the setup command that produces it.
    pub fn missing_reason(&self) -> Option<String> {
        if self.wasm.is_file() {
            return None;
        }
        let build = match self.kind {
            Kind::Kernel { .. } => "benchmarks/kernels/build.sh",
            Kind::App { .. } => "examples/apps/fetch-and-build.sh",
        };
        Some(format!(
            "{} not built — run {build}",
            display_path(&self.wasm)
        ))
    }

    /// The exclusion reason for `runner`, if this workload declares one.
    pub fn excluded(&self, runner: &str) -> Option<&'static str> {
        self.exclude
            .iter()
            .find(|(label, _)| *label == runner)
            .map(|(_, reason)| *reason)
    }
}

/// Every workload the suite knows about: the declared kernels unioned with whatever `benchmarks/cache/` actually holds, then the declared app cases. A kernel present on disk but absent from the table is included with the default cap; a kernel in the table but absent from disk is included as missing so `--list` names it.
pub fn workloads() -> Vec<Workload> {
    let mut stems: Vec<String> = KERNEL_ITER_CAPS
        .iter()
        .map(|(id, _)| (*id).to_string())
        .collect();
    for found in discovered_kernel_stems() {
        if !stems.contains(&found) {
            stems.push(found);
        }
    }
    stems.sort();

    let cache = bench_cache_dir();
    let mut out: Vec<Workload> = stems
        .into_iter()
        .map(|stem| {
            let iter_cap = KERNEL_ITER_CAPS
                .iter()
                .find(|(id, _)| *id == stem)
                .map_or(DEFAULT_ITER_CAP, |(_, cap)| *cap);
            Workload {
                label: format!("kernel/{stem}"),
                wasm: cache.join(format!("{stem}.wasm")),
                module_name: stem,
                kind: Kind::Kernel { iter_cap },
                exclude: &[],
            }
        })
        .collect();
    out.extend(app_workloads());
    out
}

/// The `.wasm` stems actually present in `benchmarks/cache/`. A missing directory is not an error here — it just means nothing is built yet, which `--list` and the per-workload `missing_reason` report with the build command.
fn discovered_kernel_stems() -> Vec<String> {
    let Ok(entries) = std::fs::read_dir(bench_cache_dir()) else {
        return Vec::new();
    };
    entries
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "wasm"))
        .filter_map(|path| path.file_stem()?.to_str().map(str::to_string))
        .collect()
}

/// The declared app cases. SQLite carries the suite's realistic workload: one case does real SQL work, the other measures nothing but load, and the pair also lets the query case subtract its own cold start.
fn app_workloads() -> Vec<Workload> {
    let cache = apps_cache_dir();
    vec![
        Workload {
            label: "app/sqlite3_query".to_string(),
            module_name: "sqlite3_shell".to_string(),
            wasm: cache.join("sqlite3-shell.wasm"),
            kind: Kind::App {
                args: Vec::new(),
                stdin: SQLITE_QUERY_SQL.to_string(),
                zero_stdin: Some(SQLITE_QUIT_SQL.to_string()),
            },
            exclude: SQLITE_QUERY_EXCLUDES,
        },
        Workload {
            label: "app/sqlite3_cold_start".to_string(),
            module_name: "sqlite3_shell".to_string(),
            wasm: cache.join("sqlite3-shell.wasm"),
            kind: Kind::App {
                args: Vec::new(),
                stdin: SQLITE_QUIT_SQL.to_string(),
                zero_stdin: None,
            },
            exclude: SQLITE_COLD_EXCLUDES,
        },
    ]
}

/// Runners excluded from the SQL query case. Every entry is a *declared* judgement about cost, not a measurement — revisit each after the first full run rather than trusting the wording. They are reported as skipped-with-reason, so an excluded cell never reads as a covered one.
const SQLITE_QUERY_EXCLUDES: &[(&str, &str)] = &[
    (
        "dewasm-bash",
        "excluded: bash runs ~1000x slower than wasmtime, so 20k SQL inserts do not finish in a practical time",
    ),
    (
        "pywasm-cpython",
        "excluded: a wasm interpreter written in Python over a 1.3 MB module does not finish 20k SQL inserts in a practical time",
    ),
    (
        "pywasm-pypy",
        "excluded: same interpreter, same order of magnitude — see pywasm-cpython",
    ),
    (
        "wardite",
        "excluded: a wasm interpreter written in Ruby over a 1.3 MB module does not finish 20k SQL inserts in a practical time",
    ),
    (
        "wardite-yjit",
        "excluded: same interpreter, same order of magnitude — see wardite",
    ),
];

/// Runners excluded from the cold-start case. Only Bash: sourcing its generated program *is* the load, and that program is tens of MB, so the case measures the shell's parser rather than a comparable load path. The wasm interpreters stay in — cold start is exactly where they are expected to win.
const SQLITE_COLD_EXCLUDES: &[(&str, &str)] = &[(
    "dewasm-bash",
    "excluded: the generated Bash program is tens of MB, so this measures the shell's parser rather than a comparable module load",
)];
