<!-- Maintainer notes. Block-level HTML comments are stripped before this file enters an agent's context:

- Claude Code reads CLAUDE.md, not AGENTS.md; CLAUDE.md pulls this file in with `@AGENTS.md`.
- Everything here loads into every session. Keep it short and keep it INSTRUCTIONS; explain a rule's why only when the why changes what you do.
- Material needed only inside one area belongs in agents/ or docs/, never here; a .claude/skills/ entry only routes to it.
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
| `cargo run -p dewasm-cli -- input.wasm --target ruby --mode standalone -o out.rb` | Convert; `.wat` input works too, `-o -` for stdout. |
| `cargo xtask record-speed [filter]` | Measure the cross-runtime benchmark suite into a record under `records/`; see [`docs/benchmarks/README.md`](docs/benchmarks/README.md). |
| `cargo xtask record-size` | Measure the distribution sizes into a record under `records/`; see [`docs/sizes/README.md`](docs/sizes/README.md). |
| `examples/apps/setup.sh` | Fetch/build the pinned real-world apps into the gitignored cache; tool requirements are in docs/testing.md. |

After any non-trivial change, run `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, and `cargo test`.
The slower test categories are opt-in cargo features: `--features slow_test` (CI's main run) adds each backend's slow app cases and the full spec-testsuite run; `--features ultra_slow_test` adds the cases CI cannot afford, run in local pre-release verification.
Do not use `-- --include-ignored`: the set it selects is not a designed configuration in this project, so what it runs can change without notice; opt in through the features instead.
Which case sits in which category, and why, is pinned at its callsite in that backend's `e2e.rs`; the mechanism is in docs/testing.md.
How the suites are laid out and what a new case must look like (the `e2e.rs` contract, the speed tokens, the `EXPECTED_FAILURES` policy) is in [`agents/test-authoring.md`](agents/test-authoring.md).
When support declarations or WASI units change, regenerate `docs/support.md` with `cargo xtask update-support-docs`.

## Decisions

Significant decisions, anything with real alternatives, are recorded in [`agents/decisions/`](agents/decisions/README.md): its README holds the index, the authoring procedure, and the quality bar.
An entry is `agents/decisions/<N>-<slug>.md`, cited as "decision N".
Nothing outside `agents/` references anything under it (no decision citation, no link); this file, `CLAUDE.md`, `.claude/`, and the app audit tooling's citations of `agents/apps-audit.md` (the record its verdicts land in) are the exceptions.
Code and user-facing docs state their constraints in place, and the decision links out to the code it governs, never the reverse.
`agents/` is for documents an agent reads while working; `docs/` is for documents a human reads ([`agents/docs-policy.md`](agents/docs-policy.md)).

## Writing style

Applies to all prose: docs, comments, PR text.

- Do not wrap lines at a fixed column; start each sentence on its own line. (Commit message bodies keep their ~72-column convention.)
- Do not coin metaphor-based vocabulary: write "CI passes", not "CI is green"; "snapshot test", not "golden test". A term must be understandable without knowing the image behind it.
- Use one term per concept; do not vary wording for style.
- Do not use dashes (`—`, `–`, or a spaced `--`) as punctuation; use a colon, a comma, parentheses, or a new sentence. Hyphens in words and ranges, `--` in command lines, and `—` as a table placeholder stay.
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

Each rule is stated here in full; the cited decision holds its rationale and rejected alternatives.

- The spec testsuite binds (decision 3). Correctness of generated code outranks its readability; readability improvements go into optional passes, never into semantics-relevant lowering.
- Where the WASI spec is silent, copy wasmtime's behavior as measured on both CI hosts (decision 49). An exception needs all three of: wasmtime's shape breaks an in-scope app, the alternative has a reference implementation, the conformance suite does not assert wasmtime's shape; record it as a decision (decision 80 is the one to date).
- Numeric representation conventions (masked-unsigned integers, f32 re-rounding, NaN bit paths) are shared across backends (decision 2).
  A backend skips a result mask only through the shared analyses in `dewasm_backend::masking`, never by its own reasoning: the consumption-context and bound analysis inside one expression tree (decision 71), and the per-function variable dataflow for unmasked local and temp stores (decision 73); a shift-count reduction folds or drops only through the same module's `shift_count_mode` (decision 74); constant AND operands, identity masks, and constant equalities go through the same machinery (decision 77).
- Each backend's lowering shapes are fixed; follow them rather than restructuring in passing. They are recorded per backend: Ruby decisions 4/42-44/58/60/65/72/75/76/78/79, Bash decisions 5/11-13/34/35/51/52, Python decisions 28/75/76/78/79, Go decision 29, Java decision 30, Perl decision 55.
- Runtime code lives as per-method units under `runtime/<lang>/units/` with `# requires:` headers, referenced as `Rt` (decision 6); keep the headers in sync when editing a unit (the units lint enforces most of it).
- Mutable runtime state added for speed hangs off a receiver that already cannot be shared between threads, never off the runtime module or a constant (decision 84); Ruby's float bit conversions borrow a per-receiver `IO::Buffer` scratch this way.
- `Embedded` linkage isolates the runtime per artifact so two artifacts coexist in one namespace (decision 62); `embedded_coexist_e2e!` is the check, and a backend that does not invoke it is unfinished, not incapable.
- A new backend is done when the shared spec harness passes for it, not before. The standard goal is wasm 1.0 + full WASI p1 (decision 24); the final exception-handling proposal is additionally accepted input, declared per backend and rejected at conversion time by backends without the lowering (decision 69). Other wasm 2.0+ proposals and the component model are rejected outright, not per backend.
- Backends declare their capabilities directly, `Backend::feature_status` and `Backend::has_wasi_p1`, rendered into `docs/support.md` (decision 25); there is no per-backend support maturity level.
- An unsupported wasm feature fails at conversion time with a clear error, never at runtime; a backend rejects what the core IR accepts but it has not implemented via `dewasm_backend::check_module_support`.
- A new `Stmt` variant declares its nested sequences in `Stmt::child_seqs` (the exhaustive match makes forgetting a compile error); a recursive search over statement trees rides `Stmt::any`/`child_seqs` and never writes its own recursion, and a `Stmt` match that keeps a silent wildcard states the invariant that makes silence safe (decision 70).
- Hot loop bodies are extracted into per-iteration functions by the shared `dewasm_backend::extract` pass, with thresholds a per-backend judgement (decision 81); a backend adopts it by swapping in the pass's rewritten function list at emission time, never by splitting generated text.
- Constant-address loads are hoisted out of loops by the shared `dewasm_backend::licm` pass, which guards every store in the loop with a runtime address check and reloads on overlap instead of attempting an alias proof (decision 82); it runs before extraction, which removes the loops it needs.
- The four-byte scatter-store loop idiom is fused into one 32-bit store behind a runtime precondition by the shared `dewasm_backend::fuse` pass (decision 83); it runs before load hoisting so the fused store carries one aliasing guard, and a shape miss leaves the loop untouched.
- A library-mode `--module-name` is used verbatim or rejected with its grammar; a standalone artifact's internal name is fixed, and the option is refused there (decision 63). Validate in `Backend::generate` only, never in the `*_with_units` APIs. Test tables carry kebab-case names, converted with `dewasm_test_helper::derive_module_name`; no name transformation belongs in the product.
