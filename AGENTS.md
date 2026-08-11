<!-- Maintainer notes. Block-level HTML comments are stripped before this file enters an agent's context:

- Claude Code reads CLAUDE.md, not AGENTS.md; CLAUDE.md pulls this file in with `@AGENTS.md`.
- Everything here loads into every session. Keep it short and keep it INSTRUCTIONS; explain a rule's why only when the why changes what you do.
- Material needed only inside one area belongs in .claude/skills/ or docs/, never here.
-->

# AGENTS.md

Agent contract for dewasm.
Project docs are written in English; `tests/spec` and `tests/wasi-testsuite` are upstream submodules: never edit them.

## Development environment

The Rust toolchain is pinned by `rust-toolchain.toml`; plain `cargo` commands pick it up.
Everything else the test suite needs (interpreters, submodules, the apps cache) and the fail-loud-not-skip policy behind it are in [`docs/testing.md`](docs/testing.md).

## Common commands

| Command | What it does |
| --- | --- |
| `cargo test` | The baseline check for every change: unit + e2e + curated spec harness. |
| `cargo fmt --check` | Verify Rust code formatting. |
| `cargo clippy --all-targets -- -D warnings` | Run the linter on all targets, failing on any warning. |
| `cargo test -p dewasm-backend-ruby --test spec i32` | Spec harness on `.wast` files whose name contains the filter; swap the crate to switch backend. |
| `cargo test -p dewasm-backend-ruby --test convert` | Convert every cached app with that backend, without running the output. |
| `cargo run -p dewasm-cli -- input.wasm --mode standalone -o out.rb` | Convert; `.wat` input works too, `-o -` for stdout. |
| `cargo xtask bench [filter]` | The cross-runtime benchmark suite; see [`docs/benchmarks/README.md`](docs/benchmarks/README.md). |
| `cargo xtask size` | The distribution-size record; see [`docs/sizes/README.md`](docs/sizes/README.md). |
| `examples/apps/setup.sh` | Fetch/build the pinned real-world apps into the gitignored cache; tool requirements are in docs/testing.md. |

After any non-trivial change, run `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, and `cargo test`.
The slower test categories are opt-in cargo features: `--features slow_test` (CI's main run) adds each backend's slow app cases and the full spec-testsuite run; `--features ultra_slow_test` adds the cases CI cannot afford, run in local pre-release verification.
Which case sits in which category, and why, is pinned at its callsite in that backend's `e2e.rs`; the mechanism is in docs/testing.md.
When support declarations or WASI units change, regenerate `docs/support.md` with `cargo xtask update-support-docs`.

## ADRs

Significant decisions, anything with real alternatives, are recorded in [`docs/adr/`](docs/adr/README.md): its README holds the index and the quality bar, and the `adr-author` skill holds the procedure.
An ADR is agent-facing memory, so only agent-facing documents (this file) may cite one.
Code and user-facing docs state their constraints in place, and the ADR links out to the code it governs, never the reverse.

## Writing style

Applies to all prose: docs, comments, PR text.

- Do not wrap lines at a fixed column; start each sentence on its own line. (Commit message bodies keep their ~72-column convention.)
- Do not coin metaphor-based vocabulary: write "CI passes", not "CI is green"; "snapshot test", not "golden test". A term must be understandable without knowing the image behind it.
- Use one term per concept; do not vary wording for style.
- One paragraph explains one thing. A side note worth keeping gets its own paragraph; usually it is worth deleting instead.
- Prefer a self-contained example, a table, or a figure over prose describing one.

## Coding style

- Express behavior through names, types, and control structure before reaching for a comment; a comment never describes what the code does.
- The comments that remain state constraints the code cannot express: an external spec's requirement, a compatibility target, a non-obvious invariant.
- A doc comment is the minimal statement of contract. If the name, parameters, and return type cannot carry a function's meaning, the function may be doing too much.

## Commit etiquette

- Imperative subject in sentence case; **no** Conventional-Commits `type:` prefixes.
- Body explains the *why*, wrapped at ~72 columns; the diff already shows the what.
- Do not commit or push unless asked.

## Implementation guidelines

Each rule is stated here in full; the cited ADR holds its rationale and rejected alternatives.

- The spec testsuite binds (ADR-3). Correctness of generated code outranks its readability; readability improvements go into optional passes, never into semantics-relevant lowering.
- Where the WASI spec is silent, copy wasmtime's behavior as measured on both CI hosts (ADR-49).
- Numeric representation conventions — masked-unsigned integers, f32 re-rounding, NaN bit paths — are shared across backends (ADR-2).
- Each backend's lowering shapes are fixed; follow them rather than restructuring in passing. They are recorded per backend: Ruby ADR-4/42-44/58/60/65, Bash ADR-5/11-13/34/35/51/52, Python ADR-28, Go ADR-29, Java ADR-30, Perl ADR-55.
- Runtime code lives as per-method units under `runtime/<lang>/units/` with `# requires:` headers, referenced as `Rt` (ADR-6); keep the headers in sync when editing a unit (the units lint enforces most of it).
- `Embedded` linkage isolates the runtime per artifact so two artifacts coexist in one namespace (ADR-62); `embedded_coexist_e2e!` is the check, and a backend that does not invoke it is unfinished, not incapable.
- A new backend is done when the shared spec harness passes for it, not before. The standard goal is wasm 1.0 + full WASI p1 (ADR-24); wasm 2.0+ proposals and the component model are rejected outright, not per backend.
- Backends declare their capabilities directly, `Backend::feature_status` and `Backend::has_wasi_p1`, rendered into `docs/support.md` (ADR-25); there is no per-backend support maturity level.
- An unsupported wasm feature fails at conversion time with a clear error, never at runtime; a backend rejects what the core IR accepts but it has not implemented via `dewasm_backend::check_module_support`.
- A library-mode `--module-name` is used verbatim or rejected with its grammar; a standalone artifact's internal name is fixed, and the option is refused there (ADR-63). Validate in `Backend::generate` only, never in the `*_with_units` APIs. Test tables carry kebab-case names, converted with `dewasm_test_helper::derive_module_name`; no name transformation belongs in the product.
