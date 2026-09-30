# Decision 36: Official WASI p1 Conformance Suite as a Harness Layer

Status: **Accepted, 2026-07-28.**
Implemented:

- the `tests/wasi-testsuite` submodule (branch `prod/testsuite-base`);
- the shared runner (`crates/dewasm-test-helper/src/wasi_testsuite.rs` + `wasi_testsuite_suite!`);
- the `BackendUnderTest::run_standalone_wasi` execution path;
- a per-backend `tests/wasi_testsuite.rs` with its own attributed list for all five backends.

Builds on decision 8 (list-with-attribution) and decision 31 (standalone interface).

## Context

dewasm's own WASI p1 fixtures (`crates/dewasm-test-helper/src/wasi.rs`) are a few `.wat` probes.
They are hand-written and grouped by feature unit.
They are enough to guard the units we wrote, but not a conformance bar.
The [`WebAssembly/wasi-testsuite`](https://github.com/WebAssembly/wasi-testsuite) project publishes prebuilt `.wasm` modules.
They are compiled from C, Rust, and AssemblyScript sources.
They exercise WASI p1 system calls against expected exit codes and output.
Each test has a JSON manifest (`args`, `env`, `root`, `exit_code`, `stdout`).
It is the closest thing to an official WASI conformance suite.
dewasm already produces standalone programs that behave like the `.wasm` they came from (decision 31).
So the modules can run through that interface unchanged.

## Decision

- **Vendor the suite as a Git submodule**, `tests/wasi-testsuite` on branch `prod/testsuite-base`.
  This mirrors the `tests/spec` submodule.
  That branch carries the *prebuilt* artifacts.
  They are under `tests/{c,rust,assemblyscript}/testsuite/wasm32-wasip1/`.
  Criterion: *upstream that ships built artifacts we do not rebuild is a submodule, not a fetch script*.
  There is no toolchain step to run, and the fixed version is a commit.
  Updating that version is an intended commit like `tests/spec`.
- **Execute through the decision 31 standalone interface, not a host made for this suite.**
  A new `BackendUnderTest::run_standalone_wasi` reuses each backend's own launch recipe (`pty_command`).
  It runs a converted standalone program with the manifest translated to that interface:
  - `root` → a `--dir <root>::/` preopen, as upstream's Wasmtime adapter mounts `root` at guest `/`;
  - `args` → guest `argv[1..]`;
  - `env` → child-process environment.

  It then asserts the process exit code and expected `stdout`.
  Each `root` fixture is staged into a fresh temporary copy.
  So a test that creates or removes files stays isolated.
  The committed submodule is never changed.
  Criterion: *a conformance runner tests the shipping interface, not a path only tests use*.
  Running the modules the way a user runs a converted program is what makes a pass meaningful.
- **Scope: `c` + `rust` + `assemblyscript`, `wasm32-wasip1` only.**
  The Rust `wasm32-wasip3` tree is excluded.
  Preview 3 is component-model territory, rejected for every backend by decision 24.
  AssemblyScript is included: its modules convert cleanly and run.
- **Known failures are listed with attribution, not implemented now** (decision 8).
  Each backend's `WASI_TESTSUITE_EXPECTED_FAILURES` maps a trial to a tag naming its cause.
  There are three honest kinds:

  | Kind | Cause | Examples |
  | --- | --- | --- |
  | (a) | a declared ENOSYS / out-of-scope system call (`docs/support.md`) | `path_link`, `path_readlink`, `path_symlink`, `fd_renumber`, `fd_advise`, `fd_allocate`, `fd_fdstat_set_flags`/`set_rights`, `*_filestat_set_times`, `sock_shutdown` |
  | (b) | a semantics-precision gap on a *supported* system call, a tracked bug in the shared WASI runtime | `errno` codes, rights masking per file type, directory entries `.`/`..`, trailing-slash handling |
  | (c) | the decision 31 choice that a standalone program inherits the whole host environment | the `environ_*` count assertions, which cannot satisfy it |

  As in `spec.rs` the list is checked both ways.
  A listed trial that unexpectedly *passes* is a hard failure.
  So filling a gap (kind a) or fixing a bug (kind b) forces its entry to be removed.
  This is decision 8's contract applied to conformance points.
  A known failure must be the consequence of a declaration or a tracked defect, never silence.
- **Manifests are read with `serde_json`.**
  `dewasm-test-helper` is test-only (`publish = false`).
  So a JSON dependency never reaches a shipped artifact.
  A hand-written parser would be larger, more likely to have bugs, and its own maintenance surface.
  Deriving `Deserialize` on the `Manifest` type avoids it.

## Rejected alternatives

- **A fetch script (like `examples/apps/setup.sh`).**
  That pattern is for artifacts fetched per shape or built from a fixed source version (decision 9).
  This suite ships built `.wasm` we consume unchanged.
  So a submodule is the lighter, reproducible way to fix the version.
  It needs no build tools, and the version is a commit.
- **A dedicated WASI host/runner in the harness.**
  It would instantiate the module and service system calls directly.
  It would skip the generated standalone `main`, the very thing that makes a converted program usable.
  So a passing run would prove less.
  Reusing `run_standalone_dir`'s path tests what ships.
  That path is extended to capture the environment and the exit code.
- **Clearing the child environment to match Wasmtime's `--env`-only model**, so the `environ_*` tests pass.
  Rejected: dewasm's standalone programs inherit the whole process environment by design.
  That is decision 31.
  A special clean-environment mode would test behaviour the CLI never produces.
  Clearing the environment also risks the interpreter launch itself.
  Listing the three count-exact `environ_*` tests under `env-passthrough` is the honest record.
  **Superseded, 2026-07-28 ([decision 40](40-wasi-p1-completion.md)):** the runner now clears the child environment.
  The launch risk is handled by resolving the interpreter against the parent PATH.
  That is exactly how upstream's Wasmtime adapter isolates the guest.
  The rows that remain listed on interpreted backends are re-attributed.
  Their cause is host-interpreter environment injection, not decision 31.
- **Fixing the kind-(b) precision gaps now.**
  They live in the per-language WASI runtime.
  They would need care across all five backends without breaking the selected `wasi.rs` suite.
  That is larger than this integration.
  They are listed with a precise tag and left as tracked follow-ups.
- **Fixing the submodule at `prod/testsuite-all`** (proposals + p2/p3) instead of `-base`.
  Out of scope for a wasm-1.0 + p1 toolchain (decision 24).

## Consequences

- Positive: an independent, upstream conformance bar for WASI p1 now tests every backend.
  ~40 modules pass per backend.
  The honest list surfaces exactly which system calls are unimplemented or not precise.
  The both-ways check turns any future WASI fix into a required list edit.
  So the declaration and the tests cannot drift.
- Negative: two submodules to initialize (`docs/testing.md` covers both).
  The compiled backends (Go, Java) build one program per module.
  The content-address cache pays that cost only on the first run.
- Carry-over: the expected-failures lists track the WASI-runtime fixes still to do.
  Those are the kind-(b) precision failures and Java's `path_open`+`O_CREAT` NOENT cluster.
  Each fix flips its list entries to hard failures until the trial passes.
