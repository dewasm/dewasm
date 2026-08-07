<!-- Maintainer notes. Block-level HTML comments are stripped before this file enters an agent's context:

- Claude Code reads CLAUDE.md, not AGENTS.md; CLAUDE.md pulls this file in with `@AGENTS.md`.
- Everything here loads into every session. Keep it short and keep it INSTRUCTIONS; explain a rule's why only when the why changes what you do.
- Material needed only inside one area belongs in .claude/skills/ or docs/, never here.
-->

# AGENTS.md

Agent contract for dewasm. Project docs are written in English; `tests/spec` is an upstream submodule: never edit it.

## Development environment

Rust toolchain is pinned by `rust-toolchain.toml` (stable); plain `cargo` commands pick it up. Required tools/setup for the test suite (ruby >= 3.4, bash >= 5, the spec submodule, the `apps` cache) and the fail-loud-not-skip policy behind it are documented in [`docs/testing.md`](docs/testing.md). Read it before wondering why a test panics with a setup instruction instead of skipping.

## Common commands

| Command | What it does |
| --- | --- |
| `cargo test` | **The baseline check for every change**: unit + e2e + curated spec harness. The spec harness is a libtest-mimic test (one trial per `.wast` file); each backend crate owns its own conformance suites, only cross-backend tests live in `dewasm-cli`. |
| `cargo fmt --check` | Verify Rust code formatting. |
| `cargo clippy --all-targets -- -D warnings` | Run linter on all targets, failing on any warnings. |
| `cargo test -p dewasm-backend-ruby --test spec i32` | Spec harness on `.wast` files whose name matches (cargo's built-in test-name filter: substring, add `--exact` for one file). Swap the crate (`-p dewasm-backend-bash`) to switch language. |
| `cargo test -p dewasm-backend-bash --test spec -- --include-ignored` | Full-testsuite run for bash (curated files are the default; the rest are `#[ignore]`d trials); trials run in parallel. Use `-- --ignored` to run only the non-curated files. |
| `cargo test -p dewasm-backend-ruby --test convert` | Whole-cache convert suite: converts every cached app with the backend, no run. One trial per app (name = cache stem); `ruby`/`cpython` are `slow_test`-conditional. Swap the crate to switch language. |
| `cargo run -p dewasm-cli -- input.wasm --mode standalone -o out.rb` | Convert; `.wat` input works too, `-o -` for stdout. |
| `cargo xtask bench [filter]` | Run the cross-runtime benchmark suite: every backend, wasmtime and the other native runtimes (wasmer, wasmedge, wazero, wasm3), and the pywasm/wardite interpreters, over the `benchmarks/wat/` and `benchmarks/c/` microbenchmarks and the app cases. Writes a dated result file to `benchmarks/results/` and regenerates `docs/benchmarks/results.md` together with the SVG charts it embeds (`docs/benchmarks/figs/`, one per workload; charts the record no longer covers are pruned): a measurement, not a snapshot, so no freshness test guards it. `--list` prints the matrix and each runner's availability without running anything; `--reps`/`--target-ms`/`--timeout` tune the measurement; `--render benchmarks/results/<file>.json` regenerates the doc from a stored record, since a full benchmark run takes tens of minutes and a wording change must not require re-measuring. Needs `wasmtime` plus `benchmarks/wat/build.sh`, `benchmarks/c/build.sh` and `benchmarks/setup.sh`; anything missing is reported as skipped-with-reason. |
| `cargo xtask size` | Run the size record, the distribution-size counterpart of the benchmark suite: per cached app (cowsay, sqlite3-shell, qjs, ruby) the wasm binary and every backend's converted standalone source, beside the installed size of each native runtime on the host (wasmtime, wasmer, wasmedge, wazero, wasm3). Raw bytes, never compressed. Writes a dated record to `benchmarks/results/<timestamp>Z-size.json`, beside the speed records, and regenerates `docs/sizes/results.md` together with the SVG figures it embeds (`docs/sizes/figs/`; figures the record no longer covers are pruned): a measurement, not a snapshot, so no freshness test guards it. `docs/sizes/README.md` is hand-written: how to run this and how to read the numbers. `--render benchmarks/results/<file>-size.json` regenerates the document from a stored record without measuring. Needs the apps cache (`examples/apps/setup.sh`); a missing app or an uninstalled runtime is reported as skipped-with-reason. |
| `examples/apps/setup.sh` | Fetch/build the pinned real-world apps (cowsay, QuickJS, the three sqlite3 shapes, minigzip, libpcap, tree-sitter, ripgrep, zeroperl, CPython, CRuby plus its wasi-vfs-packed shape) into the gitignored cache; needs a few build tools on PATH (`zig`, the `wasm32-wasip1` rustup target, `wasi-vfs`; see docs/testing.md). Enables the `apps` cases of the `e2e` test. |

## Verification

After any non-trivial change, run `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, and `cargo test`. Tests are divided by execution time: `cargo test` is the fast test. `--features slow_test` adds each backend's slow app cases (QuickJS, SQLite, the filesystem/C-API apps, the interactive-REPL pty case) plus the full spec-testsuite run. This is CI's main run, controlled by that backend's `slow_test` cargo feature rather than an environment variable. `--features ultra_slow_test` (or the everything-included `cargo test -- --include-ignored`) additionally runs the ultra-slow category: the cases a CI runner cannot afford, by wall time (~1 minute or more: the bash interactive-REPL pty case, go's giant generated-program builds) or by memory (the packed-CRuby case on Python, issue #126), which are deliberately kept out of CI and run only in local pre-release verification (the DOOM and NES framebuffer snapshots under Bash also live here; the same cases run in the slow category on the other backends). Spec-harness failures mean a semantics bug: fix the cause. Adding to a per-backend `EXPECTED_FAILURES` list in `crates/dewasm-backend-<lang>/tests/spec.rs` is a last resort and requires an attribution tag plus a reason. When support declarations or WASI units change, regenerate the matrix: `cargo xtask update-support-docs` (`cargo test -p dewasm-cli --test support_docs` fails while docs/support.md is stale).

## Implementation guidelines

- Where the WASI spec is silent (trailing-slash paths, errno flavors), behavior copies wasmtime as measured on both CI hosts, and the wasi-testsuite Rust suite runs under host-pinned strict errno modes.
- **The spec testsuite binds.** Correctness of generated code outranks its readability; readability improvements go into optional passes, never into semantics-relevant lowering.
- Numeric representation conventions (masked-unsigned integers, f32 re-rounding, NaN bit paths) are shared across backends; new backends follow them. Each backend also has fixed lowering shapes. Ruby: a branch crossing ≥16 frames is addressed by value, shallower ones relay, uncrossed frames stay structured, and expression parens are emitted only where precedence requires them. Bash: the status-cascade trap protocol, the `return 0` discipline the units lint enforces, WASI conventions of status-133 proc_exit and byte-wise binary stdio, and a softfloat of bit-pattern floats under the round_pack contract (Rust-oracle test in `crates/dewasm-backend-bash/tests/softfloat.rs`). Ruby's WASI filesystem support is complete to full WASI p1: the `preopens:` provider kwarg, the fd-table model, the symlink family, the enforced per-fd rights model, and the accepted TOCTOU sandboxing caveat.
- Runtime code lives as per-method units under `runtime/<lang>/units/` with `# requires:` headers, referenced as `Rt`; keep the headers in sync when editing a unit. The units lint test enforces most of it. `Embedded` linkage must isolate that runtime per artifact so two artifacts coexist in one namespace; `embedded_coexist_e2e!` is the check, and a backend that does not invoke it is unfinished, not incapable.
- A new backend is done when the shared spec harness passes for it, not before.
- Backends declare their capabilities directly: feature support (`Backend::feature_status`) and per-function WASI p1 coverage (`Backend::has_wasi_p1`), rendered flat into `docs/support.md`; there is no per-backend support maturity level. The standard goal for a new backend is wasm 1.0 + full WASI p1. Wasm 2.0+ proposals and the component model are rejected outright, not a backend opt-in.
- Unsupported wasm features must fail at conversion time with a clear error, never at runtime. Non-function imports, multiple tables, and table bulk ops are accepted by the core IR unconditionally; a backend that hasn't implemented one must reject it itself via `dewasm_backend::check_module_support`, which also covers the `Rt::Global` box, the import-kind check, and the spec harness's `register` support.
- A library-mode `--module-name` is used verbatim or rejected with its grammar; a standalone artifact's internal name is fixed (`Program`/`program_`) and `--module-name` is refused there. Validate in `Backend::generate` only, never in the `*_with_units` APIs the spec harness drives. Test tables carry kebab-case names and convert them with `dewasm_test_helper::derive_module_name`; no transformation belongs in the product.

## ADRs

Significant decisions, anything with real alternatives, are recorded in [`docs/adr/`](docs/adr/README.md): its README holds the index and the quality bar, and the `adr-author` skill holds the procedure (numbering, the skeleton, the index update). Docs and code comments do not cite ADRs. State the constraint in place, and let the ADR link out to the code it governs.

## Commit etiquette

- Imperative subject in sentence case; **no** Conventional-Commits `type:` prefixes.
- Body explains the *why*, wrapped at ~72 columns; the diff already shows the what.
- Do not commit or push unless asked.
