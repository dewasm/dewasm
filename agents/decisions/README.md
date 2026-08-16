# Decision records

This directory contains dewasm's decision records.
Each document captures a significant design decision: its context, the decision with its rationale, the rejected alternatives, and the consequences.
An entry is numbered: `<N>-<slug>.md`, cited as "decision N".

## How to read

- **Decision 0** is the foundation document: start there for the project's goal, scope, and architecture.
- Higher-numbered decisions build on it and can be read as needed.
- Each decision opens with a **Status** paragraph: the status label, a date, and a one-paragraph "what landed / what remains" summary.
- The status label is exactly one of two values: `Accepted`, or `Superseded (decision N)` naming what replaced it.
  The parenthetical is required on `Superseded`; a record that does not name its successor leaves the reader with nowhere to go.
- There is no `Proposed`: a decision is recorded once it is made, and a tentative idea lives in an issue until then.
- Scope and progress qualifiers never go in the label.
  That a decision covers one backend only, or that part of it has since been replaced, belongs in the Status paragraph, which has room to say what and why.

## Index

| # | Title | Status |
| --- | --- | --- |
| 0 | [Foundation and Core Architecture](0-foundation.md) | Accepted |
| 1 | [IR Design (Structured Control Flow + Stack-Slot Temps)](1-ir-design.md) | Accepted |
| 2 | [Numeric Semantics Strategy for Dynamically-Typed Targets](2-numeric-semantics.md) | Accepted |
| 3 | [Testing Strategy (Spec Testsuite on Real Interpreters)](3-testing-strategy.md) | Accepted |
| 4 | [Ruby Backend Lowering Conventions](4-ruby-backend-lowering.md) | Accepted |
| 5 | [Bash Floats (Pure-Bash Softfloat)](5-bash-softfloat.md) | Accepted |
| 6 | [Runtime as Per-Method Units with Selectable Linkage](6-runtime-units.md) | Accepted |
| 7 | [Import Providers and the Default WASI Fallback](7-import-providers.md) | Accepted |
| 8 | [Track the Latest Testsuite; Attribute Skips to a Support Matrix](8-latest-testsuite-support-matrix.md) | Accepted |
| 9 | [Example Apps Fetched from Upstream, Never Committed](9-example-apps-from-registry.md) | Accepted |
| 10 | [Add C# to the Target Languages, Paired with Java](10-csharp-target.md) | Accepted |
| 11 | [Bash Backend Lowering Conventions (Integer Subset)](11-bash-backend-lowering.md) | Accepted |
| 12 | [Bash WASI Conventions](12-bash-wasi.md) | Accepted |
| 13 | [Bash Softfloat Conventions](13-bash-softfloat-conventions.md) | Accepted |
| 14 | [Ruby WASI Filesystem Support](14-ruby-wasi-filesystem.md) | Accepted |
| 15 | [Tests Fail Loud on Missing Environment, Never Skip](15-tests-fail-not-skip.md) | Accepted |
| 16 | [Completing Wasm 1.0 for Ruby (Non-Function Imports, Multiple Tables, Table Bulk Ops, Linking)](16-ruby-wasm1-completion.md) | Accepted |
| 17 | [Reference Types in the Ruby Backend (funcref = the Table Pair, externref = a Raw Host Value)](17-ruby-reference-types.md) | Superseded (decision 24) |
| 18 | [Tail Calls in the Ruby Backend (Flat Trampoline with a Body/Entry Split)](18-ruby-tail-calls.md) | Superseded (decision 24) |
| 19 | [Exception Handling in the Ruby Backend (Tags as Identity Objects, Exceptions as Native Exceptions)](19-ruby-exception-handling.md) | Accepted |
| 20 | [Component Model (Canonical-ABI Adapters Synthesized as Core IR, Host Boundary as a Fixed Vocabulary)](20-component-model-core-ir-adapters.md) | Superseded (decision 24) |
| 21 | [WASI Preview 2 Host for Ruby (CLI World)](21-ruby-wasi-preview2.md) | Superseded (decision 24) |
| 22 | [Build the sqlite3 Apps From Pinned Source With zig, Both Standalone and Library](22-sqlite3-built-from-source.md) | Accepted |
| 23 | [Backend Support Maturity Levels, Specialized to Wasm 1.0 + WASI Preview 1](23-backend-support-levels.md) | Superseded (decision 25) |
| 24 | [0.1 Scope Reset (Wasm 1.0 + WASI Preview 1 Only, App-Driven Goals)](24-01-scope-reset.md) | Accepted |
| 25 | [Retire the Support Maturity Levels for Plain Capability Declarations](25-retire-support-levels.md) | Accepted |
| 26 | [Rename the Project (dewasmify → dewasm)](26-rename-dewasm.md) | Accepted |
| 27 | [Shared Test-Helper Crate with Per-Feature Test Macros](27-test-helper-crate.md) | Accepted |
| 28 | [Python Backend Lowering Conventions](28-python-backend-lowering.md) | Accepted |
| 29 | [Go Backend Lowering Conventions](29-go-backend-lowering.md) | Accepted |
| 30 | [Java Backend Lowering Conventions](30-java-backend-lowering.md) | Accepted |
| 31 | [Standalone Runtime Interface (argv, --dir, env, exit)](31-standalone-runtime-interface.md) | Accepted |
| 32 | [Build-time Expression Folding](32-expression-folding.md) | Accepted |
| 33 | [`IO::Buffer`-Backed Linear Memory for Ruby](33-ruby-io-buffer-memory.md) | Accepted |
| 34 | [Bash WASI Filesystem](34-bash-wasi-filesystem.md) | Accepted |
| 35 | [Bash Cross-Module Linking](35-bash-cross-module-linking.md) | Accepted |
| 36 | [Official WASI p1 Conformance Suite as a Harness Layer](36-wasi-testsuite-conformance.md) | Accepted |
| 37 | [Opt-in Data-Segment Externalization (`--data-file`)](37-data-segment-externalization.md) | Accepted |
| 38 | [Opt-in DWARF Line-Number Back-Mapping (`--dwarf-line`)](38-dwarf-line-back-mapping.md) | Accepted |
| 39 | [wasm-opt Preprocessing of Locally-Built App Modules](39-wasm-opt-preprocessing.md) | Accepted |
| 40 | [WASI p1 Completion (Symlink Family, Enforced Per-Fd Rights, and the Conformance-Runner Environment)](40-wasi-p1-completion.md) | Accepted |
| 41 | [Merge Adjacent Active Data Segments at Build Time](41-adjacent-data-segment-merging.md) | Accepted |
| 42 | [Ruby Backend Label-Variable Cascade for Multi-Level `br`](42-ruby-label-variable-cascade.md) | Accepted |
| 43 | [Ruby Backend i64 Mask Fixnum Fast Path](43-ruby-i64-mask-fast-path.md) | Accepted |
| 44 | [Ruby Backend Fixed-Arity `call_indirect` Dispatch](44-ruby-call-indirect-arity.md) | Accepted |
| 45 | [Rails Demo via a sqlite3-Gem Shim over Converted libsqlite3](45-rails-sqlite3-shim-example.md) | Accepted |
| 46 | [Host-OS-Scoped Expected-Failure Lists for the WASI Testsuite Harness](46-host-scoped-wasi-expected-failures.md) | Accepted |
| 47 | [Inline Quiet-NaN Guard for Ruby f64.sub](47-ruby-f64-sub-quiet-guard.md) | Accepted |
| 48 | [Two-Speed Slow-Test Classification (slow_test / ultra_slow_test)](48-slow-test-speeds.md) | Accepted |
| 49 | [Where the WASI Spec Is Silent, Follow wasmtime; Host-Pinned Errno Modes for wasi-testsuite](49-spec-silent-follow-wasmtime.md) | Accepted |
| 50 | [DOOM Demo (One Wasm Binary, Per-Language Native Frontends)](50-doom-example-shape.md) | Accepted |
| 51 | [Bash Linear Memory as an Associative Array](51-bash-assoc-memory.md) | Accepted |
| 52 | [Bash Emitter Inlines Linear-Memory Loads and Stores](52-bash-inline-memops.md) | Accepted |
| 53 | [Test DOOM by a Deterministic Framebuffer Snapshot](53-doom-frame-snapshot.md) | Accepted |
| 54 | [Whole-Cache Per-Backend Conversion Suite](54-apps-convert-suite.md) | Accepted |
| 55 | [Perl Backend Lowering Conventions](55-perl-backend-lowering.md) | Accepted |
| 56 | [One Command Regenerates Every Execution Snapshot](56-unified-snapshot-regeneration.md) | Accepted |
| 57 | [Benchmark by Calibrated Per-Runner Iteration Counts, Net of a Measured Baseline](57-benchmark-harness.md) | Accepted |
| 58 | [Ruby Backend Addresses a Branch by Value, Not by Lexical Scope](58-ruby-branch-by-value.md) | Accepted |
| 59 | [NES Demo (A Self-Built Guest with a File-Based ROM and an Export-Only Interface)](59-nes-example-agnes.md) | Accepted |
| 60 | [Ruby Backend Flattens Only Deep Crossings](60-ruby-flatten-only-deep-crossings.md) | Accepted |
| 61 | [Cover ruby.wasm's wasi-vfs-Packed Shape by Packing In-Cache](61-wasi-vfs-packed-cruby.md) | Accepted |
| 62 | [`Embedded` Output Isolates Its Runtime per Artifact](62-embedded-runtime-isolation.md) | Accepted |
| 63 | [`--module-name` Fixed in Standalone, Validated Verbatim in Library Mode](63-module-name-policy.md) | Accepted |
| 64 | [Record Distribution Size Beside Speed, in Raw Bytes](64-size-record.md) | Accepted |
| 65 | [Precedence-Aware Parenthesis Emission in the Ruby Backend](65-ruby-paren-elision.md) | Accepted |
| 66 | [`agents/` for Agent-Facing Documents, `docs/` for Human-Facing Ones](66-agents-directory.md) | Accepted |
| 67 | [An Experiments Index over Issues and PRs](67-experiments-index.md) | Accepted |
| 68 | [Measurement Records at the Top Level, Written and Rendered by Symmetric Commands](68-records-directory.md) | Accepted |
| 69 | [Exception Handling Joins the Accepted Input, Declared Per Backend](69-exception-handling-accepted-input.md) | Accepted |
| 70 | [Shared Statement Traversal for IR Analyses](70-shared-statement-traversal.md) | Accepted |
| 71 | [Mask Elision Inside One Expression Tree Under a Modular Consumer](71-mask-elision-modular-consumers.md) | Accepted |
| 72 | [Ruby Backend Omits Dead Method-Level `__br` Clears](72-ruby-dead-br-clear-elision.md) | Accepted |
| 73 | [Mask Elision Across Statements via a Per-Function Variable Dataflow](73-mask-elision-variable-dataflow.md) | Accepted |
| 74 | [Shift-Count Reduction Folded for Constants, Dropped Only on an Exact-Value Proof](74-shift-count-reduction-elision.md) | Accepted |
| 75 | [Pass the Static Load/Store Offset as a Second Argument](75-memory-offset-argument.md) | Accepted |
| 76 | [Memory Units Reduce Their Address and Stored-Value Operands](76-memory-unit-operand-reduction.md) | Accepted |
| 77 | [Constant AND Operands, Identity Masks, and Pinned Constant Equalities Extend Mask Elision](77-mask-constant-folds.md) | Accepted |
| 78 | [Wrapping-Add Memory Units for Dynamic Addresses](78-memory-dynamic-add-units.md) | Accepted |
| 79 | [Single-Character Codes for the Memory Load/Store Unit Names](79-memory-unit-name-codes.md) | Accepted |

## Adding a new decision

### Does the decision need one?

An entry records a decision with rationale and rejected alternatives, or a standing policy.
If no alternatives were weighed, there is nothing to record:

- A mechanical change with no live alternatives → the commit message is enough.
- Behavior the spec harness already enforces → the harness binds (decision 3), and a decision records *why*, never a normative description the harness already carries.
- A survey or a measurement with no decision attached → leave it in the issue or the pull request; when the outcome changes what a future agent would do, add an entry to [`agents/experiments.md`](../experiments.md).

### Procedure

1. Take the next free number: `ls agents/decisions/` gives the highest `N`, and yours is `N + 1`, with no zero padding.
   Create `agents/decisions/<N>-<slug>.md`.
2. Follow the skeleton:

   ```markdown
   # Decision N: <title>

   Status: **Accepted, <YYYY-MM-DD>.** <one paragraph: what landed / what remains.>

   ## Context
   ## Decision              <- the discriminating criterion, as a reusable rule
   ## Rejected alternatives <- each with the reason it lost
   ## Consequences          <- positive / negative / carry-over
   ```

3. Add a row to the index above, in ascending order, carrying the same status label as the file.
4. Cross-reference: link related decisions, and link from the decision out to the code and docs it governs.
   Files outside `agents/` never cite a decision; they state their constraint in place.
   If the decision changes how contributors must work, add or adjust the one-line rule in `AGENTS.md` citing the decision: the rule there, the why here, never both in full.
5. If it supersedes an earlier decision, set that one's label to `Superseded (decision N)` in both the file and the index, and link forward from its Status paragraph.

Then verify:

- The index row count matches the file count: `ls agents/decisions/*.md | grep -v README | wc -l` against the table.
- Every relative link in the new decision resolves.

### Quality bar

- State the *criterion* that discriminated between the options as a reusable rule, not just "we picked B".
- **Length tracks stakes.**
  The common failure mode is writing too much, not too little.
  Decision 5 is a reasonable length for a policy-sized decision, decision 0 for a foundation-sized one.
- Move research material (surveys, comparison tables) out of the record and cite it; the record is the decision, not the research.
- Anchor claims to real code (`crates/.../file.rs`, `runtime/<lang>/`) where possible.

## Relationship to other documents

- **`AGENTS.md`**: the development contract for agents (and humans) working in this repository; it states each rule in full and cites the decision that holds the rationale.
- **`agents/docs-policy.md`**: the document taxonomy, which file each kind of content belongs in, why `agents/` and `docs/` are split by audience, and why `docs/support.md` is generated, never hand-edited.
- **`README.md`**: user-facing overview.
  It points onward to `docs/getting-started.md`, `docs/backends/`, and `docs/support.md`, not into this directory.
- **`docs/getting-started.md`** and **`docs/backends/`**: the user tutorial and per-target reference.
  They state the lowering rules in place and name no decision; the rationale behind those rules lives here (decision 4, decisions 11 to 13, decisions 28 to 30, decision 55).
