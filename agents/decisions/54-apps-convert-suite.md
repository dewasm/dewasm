# Decision 54 — Whole-Cache Per-Backend Conversion Suite

Status: **Accepted, 2026-08-01.**
Implemented: the shared harness (`crates/dewasm-test-helper/src/apps_convert.rs`, `apps_convert_suite!`), a `convert` integration test in each of the five backend crates, and a fixed 13-entry manifest covering every `.wasm` the fetch scripts produce.
Every app converts under every backend today; `ruby` and `cpython` are conditional behind `slow_test`, the other eleven run in the fast test.

## Context

Conversion — wasm → backend source — is dewasm's core, but until now it was only ever exercised where an *execution* e2e case runs it.
The execution suites (`apps`, `apps_fs`, `apps_capi`, `doom`) are wired per (backend × app) pair by capability (decision 27): a backend invokes a case's macro only for a pair it can run end-to-end.
That leaves conversion of many pairs completely untested — CRuby is converted only under Go, CPython only under Java, and the entire filesystem app family never reaches the Bash emitter at all.
A codegen regression on one of those un-run pairs ships silently until someone happens to wire an execution case for it.
The gap is real: the emitter path each pair would take (SQLite-class control-flow depth, a 30–35 MB interpreter's data segments and `call_indirect` tables) is exactly where a backend-specific lowering bug hides.

Conversion is also cheap and deterministic where running is not.
Running CRuby under Bash is infeasible; *converting* it is a couple of CPU-bound seconds with no interpreter, toolchain, or snapshot needed.
So the coverage a convert-only assertion buys is available for every pair, including the ones no execution case will ever cover.

Decision 53 noted, for DOOM, that "a convert-only assertion would be an idiom no other suite uses" and so folded DOOM's convert coverage into its frame-snapshot run.
That reasoning was local to a single module; generalized across the whole app cache the idiom pays for itself, and this decision establishes it (see the note added to decision 53).

## Decision

Add a **whole-cache per-backend conversion suite**: for every backend, convert every cached app and assert the conversion completes with non-empty source — never run the generated program.

- **Shape mirrors the spec harness** (`crates/dewasm-test-helper/src/spec.rs`).
  One libtest-mimic `Trial` per manifest entry, trial name = the cache-file stem, so cargo's own name filter selects files (`cargo test --test convert qjs`).
  An `apps_convert_suite!(<Backend>)` macro supplies the `harness = false` `main`; each backend crate carries a one-line `tests/convert.rs` and a `[[test]] name = "convert"` entry.
  Unlike `spec_suite!`, the macro takes the plain `Backend` — the suite only lowers, so it needs no interpreter or script-phrasing (`SpecBackend`) layer, just `Backend::generate` on a roomy stack (`convert_on_big_stack`'s reason: SQLite-class nesting overflows the 2 MiB test-thread stack).

- **Fixed manifest, not directory discovery.**
  The manifest lists the 13 `.wasm` files the fetch scripts (`examples/apps/scripts/*.sh`) produce, each with its conversion `Mode` — `Standalone` for the command-shaped apps (a `_start`: cowsay, cpython, dwarf-fixture, minigzip, qjs, rg, ruby, sqlite3-shell), `Library` for the reactor/library artifacts (doom, libpcap, libsqlite3, sqlite3-binding, treesitter), the same mode each execution suite already uses.
  A fixed list means a stale or missing cache entry is a *failure* (decision 15), and the mode is chosen deliberately per artifact rather than guessed from the file.

- **Fail loud, never skip** (decision 15).
  A missing cache file fails the trial with the standard `run examples/apps/setup.sh` message; it does not skip.

- **Two-speed classification by measurement** (decision 48).
  Heavy trials are `#[ignore]`d unless the backend crate's `slow_test` feature is on.
  "Heavy" is *measured*, not inferred from artifact size: every (backend × app) conversion was timed at the dev profile — the build the fast test pays.
  Only the two giant interpreter artifacts cross ~2 s on every backend (`ruby` ~7–13 s, `cpython` ~2.6–5 s); they are conditional.
  The next-slowest, `rg` (~1.1–2.1 s), sits in the same cluster as the sqlite cases and stays in the fast test.
  The rule is one shared threshold applied to the measured times, not a hand-curated per-backend list — the data showed no backend needs a different set.

**Discriminating criterion:** *conversion is worth asserting on its own wherever running is infeasible or merely unwired — it is cheap, deterministic, and needs no oracle, so the whole cache is covered for every backend regardless of which pairs an execution suite reaches.*
What puts a convert trial behind `slow_test` is its measured dev-profile time against the fast test, nothing else.

## Rejected alternatives

- **Per-case convert-only smokes derived from the e2e macro callsites, conditional one speed category below the parent case.**
  This was issue #60's shape: for each existing execution macro invocation, emit a sibling convert-only `#[test]` one category down.
  It couples convert coverage to the execution wiring — a pair with no execution callsite (every un-run pair, which is exactly the gap) gets no convert smoke either, so the blind spot survives.
  It also scatters ~N×5 generated tests across the backend crates and re-derives the category per callsite.
  A single whole-cache suite covers every pair uniformly and keeps the manifest in one place; #60 is closed as superseded by #65.

- **Discover the cache directory at runtime instead of a fixed manifest.**
  Would need no edit when an app is added, but it cannot tell a legitimately-absent entry (cache not fetched — must fail, decision 15) from a genuinely-empty set, and it has no place to record each artifact's `Mode`.
  The fetch scripts are the source of truth for what exists; the manifest tracks them explicitly.

- **Also assert the generated source compiles/runs.**
  That is the execution suites' job and needs the toolchain, snapshot, and wall time this suite exists to avoid.
  The value here is the convert step in isolation; a non-empty-source assertion is the whole contract.

## Consequences

- Positive: every backend now converts every cached app on every fast-test run (eleven of thirteen; the two interpreter giants join under `slow_test`).
  A lowering regression on a pair no execution case covers now fails a fast, deterministic test.
- Positive: the suite doubled as an audit — all 13 apps convert cleanly under all five backends today, with no `check_module_support` rejection or codegen error.
- Cost: the fast test gains eleven convert trials per backend; measured at well under the `rg`/sqlite ~1–2 s cluster and run in parallel within each `convert` binary, so the added wall time is small (a couple of seconds per backend, the `rg` pole).
- Carry-over: the manifest is hand-maintained.
  A new app needs a manifest row with its `Mode`; a `.wasm` the scripts stop producing needs its row removed.
  The heavy set is pinned to today's measurement — revisit if a backend's conversion cost shifts (e.g. an artifact pin bumps its size across the ~2 s line).
