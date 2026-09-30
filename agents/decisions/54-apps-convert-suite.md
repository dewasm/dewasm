# Decision 54: Whole-Cache Per-Backend Conversion Suite

Status: **Accepted, 2026-08-01.**
Implemented:
- the shared harness (`crates/dewasm-test-helper/src/apps_convert.rs`, `apps_convert_suite!`);
- a `convert` integration test in each of the five backend crates;
- a fixed 13-entry manifest covering every `.wasm` the fetch scripts produce.

Every app converts under every backend today.
`ruby` and `cpython` are conditional behind `slow_test`; the other eleven run in the fast test.

## Context

Conversion (wasm → backend source) is dewasm's core.
Until now it was only ever exercised where an *execution* e2e case runs it.
The execution suites (`apps`, `apps_fs`, `apps_capi`, `doom`) are attached by capability (decision 27).
Each is attached per (backend × app) pair.
A backend invokes a case's macro only for a pair it can run end-to-end.
That leaves conversion of many pairs completely untested.
CRuby is converted only under Go, and CPython only under Java.
The entire filesystem app family never reaches the Bash emitter at all.
A codegen regression on one of those un-run pairs ships silently.
It does so until someone happens to add an execution case for it.
The gap is real: a backend-specific lowering bug hides exactly in the emitter path of such a pair.
That path has SQLite-class control-flow depth.
Or it has a 30-35 MB interpreter's data segments and `call_indirect` tables.

Conversion is also cheap and deterministic where running is not.
Running CRuby under Bash is infeasible.
*Converting* it is a couple of CPU-bound seconds with no interpreter, toolchain, or snapshot needed.
So the coverage a convert-only assertion buys is available for every pair.
That includes the ones no execution case will ever cover.

Decision 53 noted, for DOOM, that "a convert-only assertion would be an idiom no other suite uses".
So it folded DOOM's convert coverage into its frame-snapshot run.
That reasoning was local to a single module.
Generalized across the whole app cache, the idiom pays for itself.
This decision establishes it (see the note added to decision 53).

## Decision

Add a **whole-cache per-backend conversion suite**.
For every backend, convert every cached app.
Assert that the conversion completes with non-empty source.
The suite never runs the generated program.

- **Shape mirrors the spec harness** (`crates/dewasm-test-helper/src/spec.rs`).
  There is one libtest-mimic `Trial` per manifest entry, and the trial name is the cache-file stem.
  So cargo's own name filter selects files (`cargo test --test convert qjs`).
  An `apps_convert_suite!(<Backend>)` macro supplies the `harness = false` `main`.
  Each backend crate carries a one-line `tests/convert.rs` and a `[[test]] name = "convert"` entry.
  Unlike `spec_suite!`, the macro takes the plain `Backend`, because the suite only lowers.
  So it needs no interpreter or script-phrasing (`SpecBackend`) layer.
  It needs just `Backend::generate` on a roomy stack.
  `convert_on_big_stack`'s reason: SQLite-class nesting overflows the 2 MiB test-thread stack.

- **Fixed manifest, not directory discovery.**
  The manifest lists the 13 `.wasm` files the fetch scripts (`examples/apps/scripts/*.sh`) produce.
  Each carries its conversion `Mode`, the same mode each execution suite already uses:

  | Mode | Apps |
  | --- | --- |
  | `Standalone` (command-shaped, a `_start`) | cowsay, cpython, dwarf-fixture, minigzip, qjs, rg, ruby, sqlite3-shell |
  | `Library` (reactor/library artifacts) | doom, libpcap, libsqlite3, sqlite3-binding, treesitter |

  A fixed list means a stale or missing cache entry is a *failure* (decision 15).
  The mode is chosen deliberately per artifact rather than guessed from the file.

- **Fail loud, never skip** (decision 15).
  A missing cache file fails the trial with the standard `run examples/apps/setup.sh` message.
  It does not skip.

- **Two-speed classification by measurement** (decision 48).
  Heavy trials are `#[ignore]`d unless the backend crate's `slow_test` feature is on.
  "Heavy" is *measured*, not inferred from artifact size.
  Every (backend × app) conversion was timed at the dev profile, the build the fast test pays.
  Only the two giant interpreter artifacts cross ~2 s on every backend.
  They take `ruby` ~7-13 s and `cpython` ~2.6-5 s.
  They are conditional.
  The next-slowest, `rg` (~1.1-2.1 s), sits in the same cluster as the sqlite cases.
  It stays in the fast test.
  The rule is one shared threshold applied to the measured times, not a hand-curated per-backend list.
  The data showed no backend needs a different set.

**Discriminating criterion:** *conversion is worth asserting on its own.*
*That holds wherever running is infeasible or merely not set up.*
*It is cheap, deterministic, and needs no oracle.*
*So the whole cache is covered for every backend.*
*This holds regardless of which pairs an execution suite reaches.*
What puts a convert trial behind `slow_test` is its measured dev-profile time against the fast test.
Nothing else does.

## Rejected alternatives

- **Per-case convert-only smokes derived from the e2e macro callsites.**
  Each smoke would be conditional one speed category below the parent case.
  This was issue #60's shape.
  For each existing execution macro invocation, it emits a sibling convert-only `#[test]`.
  The sibling sits one category down.
  It couples convert coverage to the execution callsites.
  A pair with no execution callsite gets no convert smoke either, so the blind spot survives.
  Those pairs are every un-run pair, which is exactly the gap.
  It also scatters ~N×5 generated tests across the backend crates.
  And it re-derives the category per callsite.
  A single whole-cache suite covers every pair uniformly and keeps the manifest in one place.
  #60 is closed as superseded by #65.

- **Discover the cache directory at runtime instead of a fixed manifest.**
  This would need no edit when an app is added.
  But it cannot tell a legitimately-absent entry from a genuinely-empty set.
  A legitimately-absent entry means the cache is not fetched, which must fail (decision 15).
  It also has no place to record each artifact's `Mode`.
  The fetch scripts are the source of truth for what exists; the manifest tracks them explicitly.

- **Also assert the generated source compiles/runs.**
  That is the execution suites' job.
  It needs the toolchain, snapshot, and wall time this suite exists to avoid.
  The value here is the convert step in isolation; a non-empty-source assertion is the whole contract.

## Consequences

- Positive: every backend now converts every cached app on every fast-test run.
  That is eleven of thirteen; the two interpreter giants join under `slow_test`.
  A lowering regression on a pair no execution case covers now fails a fast, deterministic test.
- Positive: the suite doubled as an audit.
  All 13 apps convert cleanly under all five backends today.
  There is no `check_module_support` rejection or codegen error.
- Cost: the fast test gains eleven convert trials per backend.
  They measured well under the `rg`/sqlite ~1-2 s cluster.
  They run in parallel within each `convert` binary.
  So the added wall time is small: a couple of seconds per backend, set by `rg`.
- Carry-over: the manifest is hand-maintained.
  A new app needs a manifest row with its `Mode`.
  A `.wasm` the scripts stop producing needs its row removed.
  The heavy set is pinned to today's measurement.
  Revisit if a backend's conversion cost shifts.
  An example is an artifact pin that bumps its size across the ~2 s line.
