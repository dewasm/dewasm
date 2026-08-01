<!-- Maintainer notes. Block-level HTML comments are stripped before this file enters an agent's context:

- Claude Code reads CLAUDE.md, not AGENTS.md; CLAUDE.md pulls this file in with `@AGENTS.md`.
- Everything here loads into every session. Keep it short and keep it INSTRUCTIONS; explain a rule's why only when the why changes what you do.
- Material needed only inside one area belongs in .claude/skills/ or docs/, never here.
-->

# AGENTS.md

Agent contract for dewasm. Project docs are written in English; `tests/spec` is an upstream submodule — never edit it.

## Development environment

Rust toolchain is pinned by `rust-toolchain.toml` (stable); plain `cargo` commands pick it up. Required tools/setup for the test suite (ruby >= 3.4, bash >= 5, the spec submodule, the `apps` cache) and the fail-loud-not-skip policy behind it (ADR-15) are documented in [`docs/testing.md`](docs/testing.md) — read it before wondering why a test panics with a setup instruction instead of skipping.

## Common commands

| Command | What it does |
| --- | --- |
| `cargo test` | **The gate**: unit + e2e + curated spec harness. The spec harness is a libtest-mimic test (one trial per `.wast` file); each backend crate owns its own conformance suites (ADR-27), only cross-backend tests live in `dewasm-cli`. |
| `cargo fmt --check` | Verify Rust code formatting. |
| `cargo clippy --all-targets -- -D warnings` | Run linter on all targets, failing on any warnings. |
| `cargo test -p dewasm-backend-ruby --test spec i32` | Spec harness on `.wast` files whose name matches (cargo's built-in test-name filter — substring, add `--exact` for one file). Swap the crate (`-p dewasm-backend-bash`) to switch language. |
| `cargo test -p dewasm-backend-bash --test spec -- --include-ignored` | Full-testsuite sweep for bash (curated files are the default; the rest are `#[ignore]`d trials); trials run in parallel. Use `-- --ignored` to run only the non-curated files. |
| `cargo test -p dewasm-backend-ruby --test convert` | Whole-cache convert suite (ADR-54): converts every cached app with the backend, no run. One trial per app (name = cache stem); `ruby`/`cpython` are `slow_test`-gated. Swap the crate to switch language. |
| `cargo run -p dewasm-cli -- input.wasm --mode standalone -o out.rb` | Convert; `.wat` input works too, `-o -` for stdout. |
| `examples/apps/fetch-and-build.sh` | Fetch/build the pinned real-world apps (cowsay, QuickJS, the three sqlite3 shapes, minigzip, ripgrep, CPython, CRuby) into the gitignored cache; needs a few build tools on PATH (`zig`, the `wasm32-wasip1` rustup target — see docs/testing.md). Enables the `apps` cases of the `e2e` test. |

## Verification

After any non-trivial change, run `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, and `cargo test`. Tests run in tiers ([ADR-48](docs/adr/48-slow-test-tiers.md)): `cargo test` is the fast gate. `--features slow_test` adds each backend's slow app cases (QuickJS, SQLite, the filesystem/C-API apps, the interactive-REPL pty case) plus the full spec-testsuite sweep — this is CI's main sweep, gated by that backend's `slow_test` cargo feature rather than an environment variable. `--features ultra_slow_test` (or the everything-included `cargo test -- --include-ignored`) additionally runs the ultra tier — the cases measured at ~1 minute or more on a CI runner (the bash interactive-REPL pty case, go's giant generated-program builds), which are deliberately kept out of CI and run only in local pre-release verification (the DOOM framebuffer snapshot under Bash also lives here, [ADR-53](docs/adr/53-doom-frame-golden.md); the same DOOM case runs at the `slow` tier on the other backends). Spec-harness failures mean a semantics bug: fix the cause. Adding to a per-backend `EXPECTED_FAILURES` ledger in `crates/dewasm-backend-<lang>/tests/spec.rs` is a last resort and requires an attribution tag plus a reason ([ADR-8](docs/adr/8-latest-testsuite-support-matrix.md)). When support declarations or WASI units change, regenerate the matrix: `cargo xtask update-support-docs` (`cargo test -p dewasm-cli --test support_docs` fails while docs/support.md is stale).

## Implementation guidelines

- Where the WASI spec is silent (trailing-slash paths, errno flavors), behavior copies wasmtime as measured on both CI hosts, and the wasi-testsuite Rust suite runs under host-pinned strict errno modes ([ADR-49](docs/adr/49-spec-silent-follow-wasmtime.md)).
- **The spec testsuite binds; an ADR says why** ([ADR-3](docs/adr/3-testing-strategy.md)). Correctness of generated code outranks its readability ([ADR-1](docs/adr/1-ir-design.md)); readability improvements go into optional passes, never into semantics-relevant lowering.
- Numeric representation conventions (masked-unsigned integers, f32 re-rounding, NaN bit paths) are fixed in [ADR-2](docs/adr/2-numeric-semantics.md); new backends follow them. Per-backend lowering shapes live in [ADR-4](docs/adr/4-ruby-backend-lowering.md) (Ruby) and [ADR-11](docs/adr/11-bash-backend-lowering.md) (Bash — incl. the status-cascade trap protocol and the `return 0` discipline the units lint enforces); Bash WASI conventions (status-133 proc_exit, byte-wise binary stdio) in [ADR-12](docs/adr/12-bash-wasi.md); the Bash softfloat (bit-pattern floats, the round_pack contract, the Rust-oracle test in `crates/dewasm-backend-bash/tests/softfloat.rs`) in [ADR-13](docs/adr/13-bash-softfloat-conventions.md); Ruby WASI filesystem support (the `preopens:` provider kwarg, the fd-table model, and the accepted TOCTOU sandboxing caveat) in [ADR-14](docs/adr/14-ruby-wasi-filesystem.md), completed to full WASI p1 — the symlink family and the enforced per-fd rights model — by [ADR-40](docs/adr/40-wasi-p1-completion.md).
- Runtime code lives as per-method units under `runtime/<lang>/units/` with `# requires:` headers, referenced as `Rt` ([ADR-6](docs/adr/6-runtime-units.md)); keep the headers in sync when editing a unit — the units lint test enforces most of it.
- A new backend is done when the shared spec harness passes for it — not before ([ADR-3](docs/adr/3-testing-strategy.md)).
- Backends declare their capabilities directly — feature support (`Backend::feature_status`) and per-function WASI p1 coverage (`Backend::has_wasi_p1`) — rendered flat into `docs/support.md`; the standard goal for a new backend is wasm 1.0 + full WASI p1. Wasm 2.0+ proposals and the component model are rejected outright, not a backend opt-in (ADR-24) — see [ADR-25](docs/adr/25-retire-support-tiers.md) for why the former per-backend tier ladder was retired.
- Unsupported wasm features must fail at conversion time with a clear error, never at runtime ([ADR-0](docs/adr/0-foundation.md)). Non-function imports, multiple tables, and table bulk ops are accepted by the core IR unconditionally; a backend that hasn't implemented one must reject it itself via `dewasm_backend::check_module_support` ([ADR-16](docs/adr/16-ruby-wasm1-completion.md)), which also covers the `Rt::Global` box, the import-kind check, and the spec harness's `register` support.

## ADRs

Significant decisions — anything with real alternatives — are recorded in [`docs/adr/`](docs/adr/README.md). The `adr-author` skill carries the procedure (numbering, the skeleton, the index update). Do not restate ADR content elsewhere; link to it.

## Commit etiquette

- Imperative subject in sentence case; **no** Conventional-Commits `type:` prefixes.
- Body explains the *why*, wrapped at ~72 columns; the diff already shows the what.
- Do not commit or push unless asked.
