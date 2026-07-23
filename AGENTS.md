<!--
Maintainer notes. Block-level HTML comments are stripped before this file enters an agent's
context:

- Claude Code reads CLAUDE.md, not AGENTS.md; CLAUDE.md pulls this file in with `@AGENTS.md`.
- Everything here loads into every session. Keep it short and keep it INSTRUCTIONS; explain a
  rule's why only when the why changes what you do.
- Material needed only inside one area belongs in .claude/skills/ or docs/, never here.
-->

# AGENTS.md

Agent contract for dewasmify. Project docs are written in English; `tests/spec` is an upstream
submodule — never edit it.

## Development environment

Rust toolchain is pinned by `rust-toolchain.toml` (stable); plain `cargo` commands pick it up.
Required tools/setup for the test suite (ruby, bash >= 5, the spec submodule, the `apps` cache)
and the fail-loud-not-skip policy behind it (ADR-15) are documented in
[`docs/testing.md`](docs/testing.md) — read it before wondering why a test panics with a setup
instruction instead of skipping.

## Common commands

| Command | What it does |
| --- | --- |
| `cargo test` | **The gate**: unit + e2e + full spec harness (~5 s for the harness). |
| `cargo fmt --check` | Verify Rust code formatting. |
| `cargo clippy --all-targets -- -D warnings` | Run linter on all targets, failing on any warnings. |
| `DEWASMIFY_SPEC=i32,br cargo test -p dewasmify-cli --test spec -- --nocapture` | Spec harness on selected `.wast` files only; prints per-file pass/fail/skip. Add a test-name filter (`spec_ruby`/`spec_bash`) for one language. |
| `DEWASMIFY_SPEC_ALL=1 cargo test -p dewasmify-cli --test spec spec_bash -- --nocapture` | Full-testsuite sweep for bash (~60 s); `cargo test` alone runs bash on a curated file list. |
| `cargo run -p dewasmify-cli -- input.wasm --mode standalone -o out.rb` | Convert; `.wat` input works too, `-o -` for stdout. |
| `examples/apps/fetch.sh` | Fetch pinned real-world apps (cowsay, QuickJS) into the gitignored cache; enables the `apps` cases of the `e2e` test. |

## Verification

After any non-trivial change, run `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`,
and `cargo test`. Spec-harness failures mean a semantics bug: fix the cause. Adding to a
per-language `EXPECTED_FAILURES` ledger in `crates/dewasmify-cli/tests/spec/` is a last resort and
requires an attribution tag plus a reason ([ADR-8](docs/adr/8-latest-testsuite-support-matrix.md)).
When support declarations or WASI units change, regenerate the matrix:
`DEWASMIFY_UPDATE_DOCS=1 cargo test -p dewasmify-cli --test support_docs` (the test fails while
docs/support.md is stale).

## Implementation guidelines

- **The spec testsuite binds; an ADR says why** ([ADR-3](docs/adr/3-testing-strategy.md)).
  Correctness of generated code outranks its readability ([ADR-1](docs/adr/1-ir-design.md));
  readability improvements go into optional passes, never into semantics-relevant lowering.
- Numeric representation conventions (masked-unsigned integers, f32 re-rounding, NaN bit paths)
  are fixed in [ADR-2](docs/adr/2-numeric-semantics.md); new backends follow them. Per-backend
  lowering shapes live in [ADR-4](docs/adr/4-ruby-backend-lowering.md) (Ruby) and
  [ADR-11](docs/adr/11-bash-backend-lowering.md) (Bash — incl. the status-cascade trap
  protocol and the `return 0` discipline the units lint enforces); Bash WASI conventions
  (status-133 proc_exit, byte-wise binary stdio) in [ADR-12](docs/adr/12-bash-wasi.md);
  the Bash softfloat (bit-pattern floats, the round_pack contract, the Rust-oracle test
  in `crates/dewasmify-backend-bash/tests/softfloat.rs`) in
  [ADR-13](docs/adr/13-bash-softfloat-conventions.md); Ruby WASI filesystem support (the
  `preopens:` provider kwarg, the fd-table model, and the accepted TOCTOU/symlink sandboxing
  caveat) in [ADR-14](docs/adr/14-ruby-wasi-filesystem.md).
- Runtime code lives as per-method units under `runtime/<lang>/units/` with `# requires:`
  headers, referenced as `Rt` ([ADR-6](docs/adr/6-runtime-units.md)); keep the headers in sync
  when editing a unit — the units lint test enforces most of it.
- A new backend is done when the shared spec harness passes for it — not before
  ([ADR-3](docs/adr/3-testing-strategy.md)).
- Unsupported wasm features must fail at conversion time with a clear error, never at runtime
  ([ADR-0](docs/adr/0-foundation.md)). Non-function imports, multiple tables, and table bulk ops
  are accepted by the core IR unconditionally; a backend that hasn't implemented one must reject
  it itself via `dewasmify_backend::check_module_support` ([ADR-16](docs/adr/16-ruby-wasm1-completion.md)),
  which also covers the `Rt::Global` box, the import-kind check, and the spec harness's `register`
  support.

## ADRs

Significant decisions — anything with real alternatives — are recorded in
[`docs/adr/`](docs/adr/README.md). The `adr-author` skill carries the procedure (numbering, the
skeleton, the index update). Do not restate ADR content elsewhere; link to it.

## Commit etiquette

- Imperative subject in sentence case; **no** Conventional-Commits `type:` prefixes.
- Body explains the *why*, wrapped at ~72 columns; the diff already shows the what.
- Do not commit or push unless asked.
