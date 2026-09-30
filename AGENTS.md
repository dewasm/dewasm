<!-- Maintainer notes.
Block-level HTML comments are stripped before this file enters an agent's context:

- Claude Code reads CLAUDE.md, not AGENTS.md; CLAUDE.md pulls this file in with `@AGENTS.md`.
- Everything here loads into every session.
  Keep it short and keep it INSTRUCTIONS; explain a why only when it changes what you do.
- Material needed only inside one area belongs in agents/ or docs/, never here.
  A .claude/skills/ entry only routes to it.
-->

# AGENTS.md

Agent contract for dewasm.
Project docs are written in English.
`tests/spec` and `tests/wasi-testsuite` are upstream submodules: never edit them.

## Development environment

The Rust toolchain is pinned by `rust-toolchain.toml`; plain `cargo` commands pick it up.
Everything else the test suite needs is in [`docs/testing.md`](docs/testing.md).
That covers the interpreters, the submodules, the apps cache, and the fail-loud-not-skip policy.

## Common commands

| Command | What it does |
| --- | --- |
| `cargo test` | The baseline check for every change: unit + e2e + curated spec harness. |
| `cargo fmt --check` | Verify Rust code formatting. |
| `cargo clippy --all-targets -- -D warnings` | Run the linter on all targets, failing on any warning. |
| `cargo test -p dewasm-backend-ruby --test spec i32` | Spec harness on `.wast` files whose name contains the filter; swap the crate to switch backend. |
| `cargo test -p dewasm-backend-ruby --test convert` | Convert every cached app with that backend, without running the output. |
| `cargo run -p dewasm -- input.wasm --target ruby --mode standalone -o out.rb` | Convert; `.wat` input works too, `-o -` for stdout. |
| `cargo xtask record-speed [filter]` | Measure the cross-runtime benchmark suite into a record under `records/`; see [`docs/benchmarks/README.md`](docs/benchmarks/README.md). |
| `cargo xtask record-size` | Measure the distribution sizes into a record under `records/`; see [`docs/sizes/README.md`](docs/sizes/README.md). |
| `examples/apps/setup.sh` | Fetch/build the pinned real-world apps into the gitignored cache; `--check` verifies without fetching and names anything stale. Tool requirements are in docs/testing.md. |

After any non-trivial change, run the first three commands of the table: fmt, clippy, and test.
The slower test categories are opt-in cargo features.
`--features slow_test`, CI's main run, adds the slow app cases and the full spec testsuite.
Codon, whose per-file cost is a compile, runs a curated spec list there instead.
`--features ultra_slow_test` adds the cases CI cannot afford, run in local pre-release verification.
Do not use `-- --include-ignored`; opt in through the features instead.
The set it selects is not a designed configuration, so what it runs can change without notice.
Which case sits in which category, and why, is pinned at its callsite in that backend's `e2e.rs`.
The mechanism is in docs/testing.md.
Suite layout and the shape of a new case: [`agents/test-authoring.md`](agents/test-authoring.md).
It holds the `e2e.rs` contract, the category tokens, and the `EXPECTED_FAILURES` policy.
When support declarations or WASI units change, regenerate `docs/support.md`.
`cargo xtask update-support-docs` does it.

## Decisions

A decision with real alternatives is recorded in [`agents/decisions/`](agents/decisions/README.md).
Its README holds the index, the authoring procedure, and the quality bar.
An entry is `agents/decisions/<N>-<slug>.md`, cited as "decision N".
Nothing outside `agents/` references anything under it: no decision citation, no link.
The exceptions are this file, `CLAUDE.md`, and `.claude/`.
The app audit tooling is one more: it cites `agents/apps-audit.md`, the record its verdicts land in.
Code and user-facing docs state their constraints in place.
The decision links out to the code it governs, never the reverse.
`agents/` is for documents an agent reads while working; `docs/` is for documents a human reads.
The taxonomy is in [`agents/docs-policy.md`](agents/docs-policy.md).

## Writing style

Applies to all prose: docs, comments, PR text.
The rules form one set.
A sentence has a length bound, and the other rules remove every way to meet it except saying less.

- Start each sentence on its own line, and never wrap one.
  A line is then a sentence, so a long line shows a long sentence.
  Commit message bodies keep their ~72-column convention.
- A sentence fits in 100 columns (decision 98).
  Shorten it by removing words that add no information.
  If it is still too long, it states two things: split it at a sentence or at a `;`.
  A table row is one line by syntax, so the bound does not apply to it.
- Remove words, not connectives.
  "So", "since" and "then" make short sentences read as prose rather than as a list.
- Do not coin metaphor-based vocabulary.
  Write "CI passes", not "CI is green"; "snapshot test", not "golden test".
  A term must be understandable without knowing the image behind it.
- Use one term per concept.
  Do not vary wording for style, and do not swap in a shorter word that means less.
- State facts, not intensifiers: a size, a count, a version.
  "Byte for byte" and "fully" add nothing a reader can check.
- Do not use dashes (`—`, `–`, or a spaced `--`) as punctuation.
  Use a colon, a comma, parentheses, or a new sentence instead.
  Hyphens in words and ranges, `--` in command lines, and `—` as a table placeholder stay.
- One paragraph explains one thing.
  A side note worth keeping gets its own paragraph; usually it is worth deleting instead.
- Prefer a self-contained example, a table, or a figure over prose describing one.
- Existing text is brought under the rules when it is edited, not in passing.

## Coding style

- Express behavior through names, types, and control structure before reaching for a comment.
  A comment never describes what the code does.
- The comments that remain state constraints the code cannot express.
  Those are an external spec's requirement, a compatibility target, or a non-obvious invariant.
- A doc comment is the minimal statement of contract.
  When a function's name, parameters, and return type cannot carry its meaning, it may do too much.

## Commit etiquette

- Imperative subject in sentence case; **no** Conventional-Commits `type:` prefixes.
- Body explains the *why*, wrapped at ~72 columns; the diff already shows the what.
- Do not commit or push unless asked.

## Implementation guidelines

Each rule is stated here in full; the cited decision holds its rationale and rejected alternatives.

- The spec testsuite binds (decision 3).
  Correctness of generated code outranks its readability.
  Readability improvements go into optional passes, never into semantics-relevant lowering.
- Where WASI is silent, copy wasmtime's behavior as measured on both CI hosts (decision 49).
  An exception is recorded as a decision (decision 80 is the one to date) and needs all three of:
  - wasmtime's shape breaks an in-scope app;
  - the alternative has a reference implementation;
  - the conformance suite does not assert wasmtime's shape.
- Numeric representation conventions are shared across backends (decision 2).
  Those are masked-unsigned integers, f32 re-rounding, and NaN bit paths.
  A backend skips a result mask only through the shared analyses in `dewasm_backend::masking`.
  One is the consumption-context and bound analysis inside one expression tree (decision 71).
  The other is the per-function variable dataflow for unmasked local and temp stores (decision 73).
  A backend never skips a mask by its own reasoning.
  A shift-count reduction folds or drops only through its `shift_count_mode` (decision 74).
  Constant AND operands, identity masks, and constant equalities use the same module (decision 77).
  The f64-to-f32 re-round takes the arithmetic splitting path only by measurement (decision 84).
  The path must measure faster than the backend's conversion primitive on `wat/f32_alu`.
  Another backend's measurement does not transfer.
- Each backend's lowering shapes are fixed; follow them rather than restructuring in passing.
  They are recorded per backend:

  | Backend | Decisions |
  | --- | --- |
  | Ruby | 4, 18, 42 to 44, 58, 60, 65, 72, 75, 76, 78, 79 |
  | Bash | 5, 11 to 13, 18, 34, 35, 51, 52 |
  | Python | 18, 28, 75, 76, 78, 79 |
  | Go | 18, 29 |
  | Java | 18, 30 |
  | Perl | 18, 55, 91 |
  | Codon | 94 |

- Generated output must survive ahead-of-time compilation (decision 96).
  A name-to-member step over a table fixed at conversion time is emitted as a literal dispatch.
  It is never a run-time lookup under a computed name.
  Where a language spells one test several ways, generated code takes the spelling that compiles.
  A Ruby `Errno` class is named in a `rescue` clause, never as a hash key or a `case/when` operand.
  A `--mode standalone` artifact runs on load in every backend, behind no main guard.
  `--mode library` is the mode for output other code loads.
- Runtime code is per-method units under `crates/dewasm-backend-<lang>/units/` (decisions 6/85).
  A unit carries a `# requires:` header, and the runtime is referenced as `Rt`.
  Keep the headers in sync when editing a unit; the units lint enforces most of it.
- `Embedded` linkage isolates the runtime per artifact (decision 62).
  Two artifacts then coexist in one namespace.
  `embedded_coexist_e2e!` is the check.
  A backend that does not invoke it is unfinished, not incapable.
- A new backend is done when the shared spec harness passes for it, not before.
  The standard goal is wasm 1.0 + full WASI p1 (decision 24).
  Two more are accepted input: final exception handling (decision 69) and tail calls (decision 88).
  Each is declared per backend, and a backend without the lowering rejects it at conversion time.
  Other wasm 2.0+ proposals and the component model are rejected outright, not per backend.
- A backend declares its capabilities in `Backend::feature_status` and `Backend::has_wasi_p1`.
  These render into `docs/support.md` (decision 25); there is no per-backend support maturity level.
- An unsupported wasm feature fails at conversion time with a clear error, never at runtime.
  A backend rejects what the IR accepts but it lacks through `dewasm_backend::check_module_support`.
- A new `Stmt` variant declares its nested sequences in `Stmt::child_seqs` (decision 70).
  The exhaustive match makes forgetting one a compile error.
  A search over statement trees rides `Stmt::any`/`child_seqs`, never its own recursion.
  A `Stmt` match that keeps a silent wildcard states the invariant that makes silence safe.
- Hot loop bodies become per-iteration functions in `dewasm_backend::extract` (decision 81).
  The pass is shared; its thresholds are a per-backend judgement.
  A backend adopts it by swapping in the pass's rewritten function list at emission time.
  It never splits generated text.
- Constant-address loads are hoisted out of loops by the `dewasm_backend::licm` pass (decision 82).
  It guards every store in the loop with a runtime address check and reloads on overlap.
  It attempts no alias proof.
  It runs before extraction, which removes the loops it needs.
- A four-byte scatter-store loop becomes one 32-bit store in `dewasm_backend::fuse` (decision 83).
  The pass is shared, and the store sits behind a runtime precondition.
  The pass runs before load hoisting, so the fused store carries one aliasing guard.
  A shape miss leaves the loop untouched.
- A tail call to a function's own index becomes a loop in `dewasm_backend::selfcall` (decision 90).
  The pass is shared and runs before the other passes.
  It leaves alone a function that can fall off its end or declares a reference-typed local.
- A library-mode `--module-name` is used verbatim or rejected with its grammar (decision 63).
  A standalone artifact's internal name is fixed, and the option is refused there.
  Validate in `Backend::generate` only, never in the `*_with_units` APIs.
  Test tables carry kebab-case names, converted with `dewasm_test_helper::derive_module_name`.
  No name transformation belongs in the product.
