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
/// So the cap only ever binds the *fastest* runners — nothing else gets near it — and its job is to stop a kernel whose per-iteration cost is not constant (one that allocates, or grows a buffer) from being driven somewhere silly. Each entry is generous enough that wasmtime still reaches the compute target: the micro-kernels' unit is one trip through a tight loop (~2 ns there), the C kernels' unit is a whole hash / image / document pass. Each cap is set to roughly 3x the iteration count wasmtime needs to reach the default 300 ms compute target at its measured per-iteration cost, so raising `--target-ms` a little does not silently start clipping the baseline. Retune whenever a kernel's body changes.
const KERNEL_ITER_CAPS: &[(&str, u64)] = &[
    ("i32_alu", 500_000_000),
    ("i64_alu", 500_000_000),
    ("f64_alu", 500_000_000),
    ("mem_rw", 500_000_000),
    ("call_direct", 500_000_000),
    ("call_indirect", 500_000_000),
    ("sha256", 10_000_000),
    ("mandelbrot", 20_000_000),
    ("wordcount", 500_000_000),
];

/// The SQL script the `sqlite3_query` app case feeds to `sqlite3-shell.wasm`: build a 100k-row table inside one transaction (a recursive CTE, so the work is the engine's, not the parser's), then an aggregate and a `LIKE` scan over it.
///
/// The row count exists to make wasmtime resolvable: at 20k rows it finished the whole script in ~30 ms, nearly all of it process startup, leaving the baseline indistinguishable from noise and every ratio against it meaningless. Unlike a kernel this case is not calibrated per runner — a *fixed* script is what makes it a realistic workload rather than a synthetic one — so the size is one compromise for everybody, and it is why the slowest runners are excluded rather than measured at their own size.
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

/// The query case's zero run: load the 1.3 MB module, start the shell, run one trivial query, quit. Subtracted from the timed run like a kernel's `<iterations> = 0`, and reported on its own as the cold-start figure for the largest module in the suite — the axis where an interpreter legitimately beats an AOT compiler, since it reads wasm while a dewasm backend must first parse megabytes of generated source.
///
/// `SELECT 1;` is here for the oracle, not the timing. `.quit` alone produces no stdout at all, which made the cross-check vacuous: a runner that loaded nothing and exited 0 matched wasmtime's empty output exactly. That is not hypothetical — wardite passed this way until the query was added, and then failed outright.
const SQLITE_QUIT_SQL: &str = "SELECT 1;\n.quit\n";

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

/// The declared app cases: `cowsay` for startup on a mid-sized module, `sqlite3_query` for sustained real work.
///
/// Together with each kernel's `t(0)` these give three module sizes on the startup axis — ~20 KB kernel, 772 KB cowsay, 1.3 MB sqlite (the query case's own `t(0)`) — which is what makes the startup comparison say something. A dewasm backend must parse generated source that grows with the module, while an interpreter only reads the wasm, so the two curves cross somewhere; one module size cannot show that and would just report whichever side of the crossing it happened to land on.
///
/// cowsay is the case where the interpreters can actually compete: they run it correctly (verified byte-identical against wasmtime), it is large enough for load cost to dominate, and it finishes fast enough that every runner in the matrix can be measured on it.
fn app_workloads() -> Vec<Workload> {
    let cache = apps_cache_dir();
    vec![
        Workload {
            label: "app/cowsay".to_string(),
            module_name: "cowsay".to_string(),
            wasm: cache.join("cowsay.wasm"),
            kind: Kind::App {
                args: ["Hello", "from", "dewasm!"]
                    .iter()
                    .map(|s| (*s).to_string())
                    .collect(),
                stdin: String::new(),
                // No zero run: cowsay's work is a few milliseconds of text formatting, so the whole wall time already *is* the startup measurement this case exists for.
                zero_stdin: None,
            },
            exclude: &[],
        },
        Workload {
            label: "app/sqlite3_query".to_string(),
            module_name: "sqlite3_shell".to_string(),
            wasm: cache.join("sqlite3-shell.wasm"),
            kind: Kind::App {
                // `-batch` forces non-interactive mode. Without it the shell decides how to behave from `isatty`, and a runtime that misreports the standard fds gets a different program: pywasm reports all three as character devices (`wasi.py:429`), so the shell printed a version banner and rendered results as a box-drawing table where wasmtime printed a bare value. `-batch` removes that confound for every runner rather than special-casing one.
                args: ["-batch", ":memory:"]
                    .iter()
                    .map(|s| (*s).to_string())
                    .collect(),
                stdin: SQLITE_QUERY_SQL.to_string(),
                zero_stdin: Some(SQLITE_QUIT_SQL.to_string()),
            },
            exclude: SQLITE_QUERY_EXCLUDES,
        },
    ]
}

/// Runners excluded from the SQL query case.
///
/// Only the Bash entry is a cost judgement. The four interpreter entries are **measured facts**: neither pure-language interpreter can run this program at all, which is a more interesting result than a slow number would have been and is stated rather than left as an empty cell. An earlier draft of this table guessed that all four merely "do not finish in a practical time"; that was wrong, and the harness's own cross-check is what proved it.
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

/// Why wardite cannot run the sqlite shell. It loads the module fine (~440 ms) and handles a bare `.quit`, but any actual query dies with `Wardite::EvalError: maybe empty or invalid stack` from `convert.generated.rb:200`. Worth stating rather than hiding: it is the concrete limit of what a pure-Ruby interpreter reaches today, and it is why the cold-start script runs `SELECT 1;` — under `.quit` alone wardite exited 0 with empty output, matching wasmtime's empty output and passing a check it had not actually earned.
const WARDITE_SQLITE_REASON: &str = "excluded: wardite loads sqlite3-shell.wasm but cannot execute a query — it raises Wardite::EvalError (\"maybe empty or invalid stack\", convert.generated.rb:200) as soon as any SQL runs";

/// Why pywasm is not measured on the SQL case. It *can* run this program: under `-batch` its output is byte-identical to wasmtime's, and saying otherwise would be false — "pure-Python wasm cannot run SQLite" is not the finding here. It is excluded on measured cost: ~17.9 ms per row, flat across sizes (19 s at 1k rows, 89 s at 5k, 358 s at 20k), putting the 100k-row script near half an hour per sample.
///
/// The row count cannot simply be lowered to accommodate it, which is the real tension. At 5k rows wasmtime finishes the whole script in ~20 ms, nearly all of it process startup, so the baseline stops being resolvable exactly where pywasm becomes affordable. No single size serves both, and unlike a kernel this workload is deliberately not calibrated per runner — a fixed script is what makes it a realistic case rather than a synthetic one.
const PYWASM_SQLITE_REASON: &str = "excluded on cost, not capability: pywasm runs this program correctly (byte-identical to wasmtime under -batch) at ~17.9 ms/row — measured 358 s at 20k rows, so the 100k-row script needs roughly half an hour per sample";
