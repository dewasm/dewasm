# ADR-36 — Official WASI p1 Conformance Suite as a Harness Layer

Status: **Accepted, 2026-07-28.**
Implemented: the `tests/wasi-testsuite` submodule (branch `prod/testsuite-base`), the shared runner (`crates/dewasm-test-helper/src/wasi_testsuite.rs` + `wasi_testsuite_suite!`), the `BackendUnderTest::run_standalone_wasi` execution path, and a per-backend `tests/wasi_testsuite.rs` with its own attributed list for all five backends.
Builds on ADR-8 (list-with-attribution) and ADR-31 (standalone interface).

## Context

dewasm's own WASI p1 fixtures (`crates/dewasm-test-helper/src/wasi.rs`) are a handful of hand-written `.wat` probes grouped by feature unit — enough to guard the units we wrote, but not a conformance bar.
The [WebAssembly/wasi-testsuite](https://github.com/WebAssembly/wasi-testsuite) project publishes prebuilt `.wasm` modules (compiled from C, Rust, and AssemblyScript sources) that exercise WASI p1 syscalls against expected exit codes and output, with a per-test JSON manifest (`args`, `env`, `root`, `exit_code`, `stdout`).
It is the closest thing to an official WASI conformance suite, and dewasm already produces standalone programs that behave like the `.wasm` they came from (ADR-31), so the modules can run through that interface unchanged.

## Decision

- **Vendor the suite as a git submodule**, `tests/wasi-testsuite` on branch `prod/testsuite-base` (the branch carrying the *prebuilt* artifacts under `tests/{c,rust,assemblyscript}/testsuite/wasm32-wasip1/`), mirroring the `tests/spec` submodule.
  Criterion: *upstream that ships built artifacts we do not rebuild is a submodule, not a fetch script* — there is no toolchain step to run, the pin is a commit, and updating it is a deliberate commit like `tests/spec`.
- **Execute through the ADR-31 standalone interface, not a bespoke host.**
  A new `BackendUnderTest::run_standalone_wasi` reuses each backend's own launch recipe (`pty_command`) to run a converted standalone program with the manifest translated to that interface: `root` → a `--dir <root>::/` preopen (mirroring upstream's own wasmtime adapter, which mounts `root` at guest `/`), `args` → guest `argv[1..]`, `env` → child-process environment, then assert the process exit code and pinned stdout.
  Each `root` fixture is staged into a fresh temp copy so a test that creates or removes files stays hermetic and the committed submodule is never mutated.
  Criterion: *a conformance runner tests the shipping interface, not a test-only shortcut* — running the modules the way a user runs a converted program is what makes a pass meaningful.
- **Scope: c + rust + assemblyscript, `wasm32-wasip1` only.**
  The Rust `wasm32-wasip3` tree is excluded (preview 3 is component-model territory, rejected outright by ADR-24).
  AssemblyScript is included: its modules convert cleanly and run.
- **Known failures are listed with attribution, not implemented now** (ADR-8).
  Each backend's `WASI_TESTSUITE_EXPECTED_FAILURES` maps a trial to a tag naming its cause, in three honest kinds: (a) a declared ENOSYS / out-of-scope syscall (`docs/support.md` — `path_link`, `path_readlink`, `path_symlink`, `fd_renumber`, `fd_advise`, `fd_allocate`, `fd_fdstat_set_flags`/`set_rights`, `*_filestat_set_times`, `sock_shutdown`); (b) a semantics-precision gap on a *supported* syscall (errno codes, per-filetype rights masking, dirent `.`/`..`, trailing-slash handling) — tracked bugs in the shared WASI runtime; (c) the ADR-31 choice that a standalone program inherits the whole host environment, which the `environ_*` count assertions cannot satisfy.
  As in `spec.rs` the list is checked both ways — a listed trial that unexpectedly *passes* is a hard failure, so filling a gap (kind a) or fixing a bug (kind b) forces its entry to be removed.
  This is ADR-8's contract applied to conformance points: a known failure must be the consequence of a declaration or a tracked defect, never silence.
- **Manifests are read with `serde_json`.**
  `dewasm-test-helper` is test-only (`publish = false`), so a JSON dependency never reaches a shipped artifact; deriving `Deserialize` on the `Manifest` struct is smaller and less bug-prone than a hand-written parser, which would be its own maintenance surface.

## Rejected alternatives

- **A fetch script (like `examples/apps/setup.sh`).**
  That pattern exists for artifacts we rebuild from pinned source or download per-shape (ADR-9); this suite ships built `.wasm` we consume verbatim, so a submodule is the lighter, reproducible pin — no build tools, and the version is a commit.
- **A dedicated WASI host/runner in the harness** that instantiates the module and services syscalls directly.
  It would bypass the generated standalone `main` — the very thing that makes a converted program usable — so a passing run would prove less.
  Reusing `run_standalone_dir`'s path (extended with env and exit-code capture) tests what ships.
- **Clearing the child environment to match wasmtime's `--env`-only model**, so the `environ_*` tests pass.
  Rejected: dewasm's standalone programs inherit the whole process environment by design (ADR-31); a special clean-env mode would test behaviour the CLI never produces, and env-clearing risks the interpreter launch itself.
  Listing the three count-exact `environ_*` tests under `env-passthrough` is the honest record.
  **Superseded, 2026-07-28 ([ADR-40](40-wasi-p1-completion.md)):** the runner now clears the child environment (the launch risk is handled by resolving the interpreter against the parent PATH), which is exactly how upstream's wasmtime adapter isolates the guest; the rows that remain listed on interpreted backends are re-attributed to host-interpreter env injection, not to ADR-31.
- **Fixing the kind-(b) precision gaps now.**
  They live in the per-language WASI runtime and would need care across all five backends without regressing the curated `wasi.rs` suite — larger than this integration.
  They are listed with a precise tag and left as tracked follow-ups.
- **Pinning `prod/testsuite-all`** (proposals + p2/p3) instead of `-base`.
  Out of scope for a wasm-1.0 + p1 toolchain (ADR-24).

## Consequences

- Positive: an independent, upstream conformance bar for WASI p1 now tests every backend; ~40 modules pass per backend and the honest list surfaces exactly which syscalls are unimplemented or imprecise.
  The both-ways check turns any future WASI fix into a required list edit, so the declaration and the tests cannot drift.
- Negative: two submodules to initialize (`docs/testing.md` covers both).
  The compiled backends (Go, Java) build one program per module; the content-address cache amortizes it after the first run.
- Carry-over: the kind-(b) precision failures and Java's `path_open`+`O_CREAT` NOENT cluster are tracked in the expected-failures lists as the backlog of WASI-runtime fixes; each fix flips its list entries to hard failures until the trial passes.
