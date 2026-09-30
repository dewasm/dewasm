# Decision records

This directory contains dewasm's decision records.
Each document captures one significant design decision.
It holds the context, the decision and its reasons, the rejected alternatives, and the consequences.
An entry is numbered: `<N>-<slug>.md`, cited as "decision N".

## How to read

- **Decision 0** is the foundation document.
  Start there for the project's goal, scope, and architecture.
- Higher-numbered decisions build on it and can be read as needed.
- Each decision opens with a **Status** paragraph.
  It holds the status label, a date, and a one-paragraph "what landed / what remains" summary.
- The status label is one of two values: `Accepted`, or `Superseded (decision N)`.
  The number in parentheses names what replaced it, and it is required on `Superseded`.
  A record that does not name the decision that replaced it leaves the reader with nowhere to go.
- There is no `Proposed`: a decision is recorded once it is made.
  An idea not yet decided lives in an issue until then.
- Scope and progress qualifiers never go in the label.
  A decision may cover one backend only, or part of it may have been replaced.
  Either fact goes in the Status paragraph.
  That paragraph has room to say what and why.

## Index

| # | Title | Status |
| --- | --- | --- |
| 0 | [Foundation and Core Architecture](0-foundation.md) | Accepted |
| 1 | [IR Design (Structured Control Flow + Stack-Slot Temps)](1-ir-design.md) | Accepted |
| 2 | [Numeric Semantics Strategy for Dynamically-Typed Targets](2-numeric-semantics.md) | Accepted |
| 3 | [Testing Strategy (Specification Testsuite on Real Interpreters)](3-testing-strategy.md) | Accepted |
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
| 14 | [Ruby WASI File System Support](14-ruby-wasi-file-system.md) | Accepted |
| 15 | [Tests Fail Loud on Missing Environment, Never Skip](15-tests-fail-not-skip.md) | Accepted |
| 16 | [Wasm 1.0 in Ruby: Non-Function Imports, Multiple Tables, Bulk Table Operations, Linking](16-ruby-wasm1-completion.md) | Accepted |
| 17 | [Ruby Reference Types (`funcref` = the Table Pair, `externref` = a Raw Host Value)](17-ruby-reference-types.md) | Superseded (decision 24) |
| 18 | [Tail Calls in the Ruby Backend (Flat Trampoline with a Body/Entry Split)](18-ruby-tail-calls.md) | Accepted |
| 19 | [Ruby Exception Handling (Tags as Identity Objects, Exceptions as Native Exceptions)](19-ruby-exception-handling.md) | Accepted |
| 20 | [Component Model (Canonical-ABI Adapters Built as Core IR, Fixed Host Vocabulary)](20-component-model-core-ir-adapters.md) | Superseded (decision 24) |
| 21 | [WASI Preview 2 Host for Ruby (CLI World)](21-ruby-wasi-preview2.md) | Superseded (decision 24) |
| 22 | [Build the sqlite3 Apps From a Fixed Source Version With Zig, Standalone and Library](22-sqlite3-built-from-source.md) | Accepted |
| 23 | [Backend Support Maturity Levels, Specialized to Wasm 1.0 + WASI Preview 1](23-backend-support-levels.md) | Superseded (decision 25) |
| 24 | [0.1 Scope Reset (Wasm 1.0 + WASI Preview 1 Only, App-Driven Goals)](24-01-scope-reset.md) | Accepted |
| 25 | [Retire the Support Maturity Levels for Plain Capability Declarations](25-retire-support-levels.md) | Accepted |
| 26 | [Rename the Project (`dewasmify` → dewasm)](26-rename-dewasm.md) | Accepted |
| 27 | [Shared Test-Helper Crate with Per-Feature Test Macros](27-test-helper-crate.md) | Accepted |
| 28 | [Python Backend Lowering Conventions](28-python-backend-lowering.md) | Accepted |
| 29 | [Go Backend Lowering Conventions](29-go-backend-lowering.md) | Accepted |
| 30 | [Java Backend Lowering Conventions](30-java-backend-lowering.md) | Accepted |
| 31 | [Standalone Runtime Interface (`argv`, `--dir`, `env`, `exit`)](31-standalone-runtime-interface.md) | Accepted |
| 32 | [Build-time Expression Folding](32-expression-folding.md) | Accepted |
| 33 | [`IO::Buffer`-Backed Linear Memory for Ruby](33-ruby-io-buffer-memory.md) | Accepted |
| 34 | [Bash WASI File System](34-bash-wasi-file-system.md) | Accepted |
| 35 | [Bash Cross-Module Linking](35-bash-cross-module-linking.md) | Accepted |
| 36 | [Official WASI p1 Conformance Suite as a Harness Layer](36-wasi-testsuite-conformance.md) | Accepted |
| 37 | [Data Segments Moved to a Separate File on Request (`--data-file`)](37-data-segments-in-a-file.md) | Accepted |
| 38 | [Optional DWARF Line-Number Back-Mapping (`--dwarf-line`)](38-dwarf-line-back-mapping.md) | Accepted |
| 39 | [Running `wasm-opt` on Locally-Built App Modules](39-running-wasm-opt.md) | Accepted |
| 40 | [WASI p1 Completion (Symbolic Links, Checked `fd` Rights, Conformance-Runner Environment)](40-wasi-p1-completion.md) | Accepted |
| 41 | [Merge Adjacent Active Data Segments at Build Time](41-adjacent-data-segment-merging.md) | Accepted |
| 42 | [Ruby Backend Label-Variable Chain for Multi-Level `br`](42-ruby-label-variable-chain.md) | Accepted |
| 43 | [Ruby Backend i64 Mask Fixnum Fast Path](43-ruby-i64-mask-fast-path.md) | Accepted |
| 44 | [Ruby Backend Fixed-Arity `call_indirect` Dispatch](44-ruby-call-indirect-arity.md) | Accepted |
| 45 | [Rails Example via a sqlite3-Gem Shim over Converted libsqlite3](45-rails-sqlite3-shim-example.md) | Accepted |
| 46 | [Host-OS-Scoped Expected-Failure Lists for the WASI Testsuite Harness](46-host-scoped-wasi-expected-failures.md) | Accepted |
| 47 | [Inline Quiet-NaN Guard for Ruby f64.sub](47-ruby-f64-sub-quiet-guard.md) | Accepted |
| 48 | [Two-Speed Slow-Test Classification (`slow_test` / `ultra_slow_test`)](48-slow-test-speeds.md) | Accepted |
| 49 | [Where WASI Is Silent, Follow Wasmtime; `errno` Modes Fixed per Host for `wasi-testsuite`](49-spec-silent-follow-wasmtime.md) | Accepted |
| 50 | [DOOM Example (One Wasm Binary, Per-Language Native Frontends)](50-doom-example-shape.md) | Accepted |
| 51 | [Bash Linear Memory as an Associative Array](51-bash-assoc-memory.md) | Accepted |
| 52 | [Bash Emitter Inlines Linear-Memory Loads and Stores](52-bash-inline-memops.md) | Accepted |
| 53 | [Test DOOM by a Deterministic Framebuffer Snapshot](53-doom-frame-snapshot.md) | Accepted |
| 54 | [Whole-Cache Per-Backend Conversion Suite](54-apps-convert-suite.md) | Accepted |
| 55 | [Perl Backend Lowering Conventions](55-perl-backend-lowering.md) | Accepted |
| 56 | [One Command Regenerates Every Execution Snapshot](56-unified-snapshot-regeneration.md) | Accepted |
| 57 | [Benchmark by Calibrated Per-Runner Iteration Counts, Net of a Measured Baseline](57-benchmark-harness.md) | Accepted |
| 58 | [Ruby Backend Addresses a Branch by Value, Not by Lexical Scope](58-ruby-branch-by-value.md) | Accepted |
| 59 | [NES Example (A Self-Built Guest with a File-Based ROM and an Export-Only Interface)](59-nes-example-agnes.md) | Accepted |
| 60 | [Ruby Backend Flattens Only Deep Crossings](60-ruby-flatten-only-deep-crossings.md) | Accepted |
| 61 | [Cover ruby.wasm's `wasi-vfs`-Packed Shape by Packing In-Cache](61-wasi-vfs-packed-cruby.md) | Accepted |
| 62 | [`Embedded` Output Isolates Its Runtime per Artifact](62-embedded-runtime-isolation.md) | Accepted |
| 63 | [`--module-name` Fixed in Standalone, Validated and Used as Given in Library Mode](63-module-name-policy.md) | Accepted |
| 64 | [Record Distribution Size Beside Speed, in Raw Bytes](64-size-record.md) | Accepted |
| 65 | [Precedence-Aware Parenthesis Emission in the Ruby Backend](65-ruby-paren-elision.md) | Accepted |
| 66 | [`agents/` for Agent-Facing Documents, `docs/` for Human-Facing Ones](66-agents-directory.md) | Accepted |
| 67 | [An Experiments Index over Issues and PRs](67-experiments-index.md) | Accepted |
| 68 | [Measurement Records at the Top Level, Written and Rendered by Paired Commands](68-records-directory.md) | Accepted |
| 69 | [Exception Handling Joins the Accepted Input, Declared Per Backend](69-exception-handling-accepted-input.md) | Accepted |
| 70 | [Shared Statement Walk for IR Analyses](70-shared-statement-walk.md) | Accepted |
| 71 | [Mask Elision Inside One Expression Tree Under a Modular Consumer](71-mask-elision-modular-consumers.md) | Accepted |
| 72 | [Ruby Backend Drops Dead Method-Level `__br` Clears](72-ruby-dead-br-clear-elision.md) | Accepted |
| 73 | [Mask Elision Across Statements via a Per-Function Variable Dataflow](73-mask-elision-variable-dataflow.md) | Accepted |
| 74 | [Shift-Count Reduction Folded for Constants, Dropped Only on an Exact-Value Proof](74-shift-count-reduction-elision.md) | Accepted |
| 75 | [Pass the Static Load/Store Offset as a Second Argument](75-memory-offset-argument.md) | Accepted |
| 76 | [Memory Units Reduce Their Address and Stored-Value Operands](76-memory-unit-operand-reduction.md) | Accepted |
| 77 | [Mask Elision for Constant AND Operands, Identity Masks, and One-Candidate Equalities](77-mask-constant-folds.md) | Accepted |
| 78 | [Wrapping-Add Memory Units for Dynamic Addresses](78-memory-dynamic-add-units.md) | Accepted |
| 79 | [Single-Character Codes for the Memory Load/Store Unit Names](79-memory-unit-name-codes.md) | Accepted |
| 80 | [`fd_fdstat_set_flags` Accepts Any Open Descriptor, a Recorded Exception to Decision 49](80-fdstat-set-flags-any-fd.md) | Accepted |
| 81 | [Loop-Body Extraction into Per-Iteration Functions](81-loop-body-extraction.md) | Accepted |
| 82 | [Hoist Invariant Constant-Address Loads with a Runtime Store Guard](82-licm-runtime-store-guard.md) | Accepted |
| 83 | [Byte-Scatter Store Fusion Behind a Runtime Precondition](83-byte-scatter-store-fusion.md) | Accepted |
| 84 | [Round to f32 by Veltkamp Splitting, with the Pack Path as Fallback](84-f32-rounding-by-splitting.md) | Accepted |
| 85 | [crates.io Publish Layout (Units Inside Their Crates, CLI Crate Named `dewasm`)](85-crates-io-publish-layout.md) | Accepted |
| 86 | [wasm3 as the Converted-Interpreter Benchmark Runner](86-converted-interpreter-benchmark-runner.md) | Accepted |
| 87 | [Record Schema Evolution by In-Place Migration](87-record-schema-migration.md) | Accepted |
| 88 | [Tail Calls Join the Accepted Input, Declared Per Backend](88-tail-calls-accepted-input.md) | Accepted |
| 89 | [Park a Pending Tail Call, Never Allocate One](89-park-the-pending-tail-call.md) | Accepted |
| 90 | [Rewrite a Self Tail Call into a Loop](90-self-tail-call-to-loop.md) | Accepted |
| 91 | [Perl Inline Fast Paths for Float Arithmetic and Memory Access](91-perl-inline-fast-paths.md) | Accepted |
| 92 | [`wasi-sdk` as the C Toolchain for the Locally-Built Modules](92-wasi-sdk-c-toolchain.md) | Accepted |
| 93 | [Alternative Engines as Speed-Suite Runners](93-engine-runners-in-speed-suite.md) | Accepted |
| 94 | [Codon Backend Lowering Conventions](94-codon-backend-lowering.md) | Accepted |
| 95 | [`cowsay` Comes From Our Own Implementation, Published Upstream](95-cowsay-own-implementation.md) | Accepted |
| 96 | [Generated Output Must Survive Ahead-of-Time Compilation](96-generated-code-compiles-ahead-of-time.md) | Accepted |
| 97 | [A Host Library the Runtime May Lack Is Optional; Its Absence Is Refused, Not Faked](97-optional-host-libraries.md) | Accepted |
| 98 | [A Sentence Is at Most 100 Characters as Read](98-sentence-length-bound.md) | Accepted |
| 99 | [Text Uses a Learner Word List Plus Project Terms](99-allowed-vocabulary.md) | Accepted |

## Adding a new decision

### Does the decision need one?

An entry records a decision with its reasons and rejected alternatives, or a standing policy.
If no alternatives were weighed, there is nothing to record:

- A mechanical change with no live alternatives → the commit message is enough.
- Behavior the specification harness already checks → the harness binds (decision 3).
  A decision records *why*, never a statement of rules the harness already carries.
- A survey or a measurement with no decision attached → leave it in the issue or the pull request.
  When the outcome changes what a future agent would do, add an entry to [`agents/experiments.md`](../experiments.md).

### Procedure

1. Take the next free number: `ls agents/decisions/` gives the highest `N`, and yours is `N + 1`.
   The number has no leading zeros.
   Create `agents/decisions/<N>-<slug>.md`.
2. Follow the template:

   ```markdown
   # Decision N: <title>

   Status: **Accepted, <YYYY-MM-DD>.** <one paragraph: what landed / what remains.>

   ## Context
   ## Decision              <- the discriminating criterion, as a reusable rule
   ## Rejected alternatives <- each with the reason it lost
   ## Consequences          <- positive / negative / carry-over
   ```

3. Add a row to the index above, in order of number, carrying the same status label as the file.
4. Cross-reference: link related decisions.
   Link from the decision out to the code and documents it governs.
   Files outside `agents/` never cite a decision; they state their constraint in place.
   If the decision changes how people must work here, add or adjust the one-line rule in `AGENTS.md`.
   That rule cites the decision: the rule there, the why here, never both in full.
5. If it replaces an earlier decision, set that one's label to `Superseded (decision N)`.
   Set it in both the file and the index, and link forward from its Status paragraph.

Then verify:

- The index row count matches the file count.
  Compare `ls agents/decisions/*.md | grep -v README | wc -l` against the table.
- Every relative link in the new decision resolves.

### Quality bar

- State the *criterion* that decided between the options as a reusable rule.
  "We picked B" is not enough.
- **Length tracks stakes.**
  The common failure mode is writing too much, not too little.
  Decision 5 is a reasonable length for a policy-sized decision.
  Decision 0 is one for a foundation-sized decision.
- Move research material (surveys, comparison tables) out of the record and cite it.
  The record is the decision, not the research.
- Base claims on real code (`crates/.../file.rs`, `crates/dewasm-backend-<lang>/units/`) where possible.

## Relationship to other documents

- **`AGENTS.md`**: the development contract for agents (and humans) working in this repository.
  It states each rule in full and cites the decision that holds the reasons.
- **`agents/docs-policy.md`**: the kinds of document, and which file each kind of content belongs in.
  It says why `agents/` and `docs/` are split by audience.
  It also says why `docs/support.md` is generated, never hand-edited.
- **`README.md`**: user-facing overview.
  It points to `docs/getting-started.md`, `docs/backends/`, and `docs/support.md`.
  It does not point into this directory.
- **`docs/getting-started.md`** and **`docs/backends/`**: the user guide and per-target reference.
  They state the lowering rules in place and name no decision.
  The reasons for those rules live here: decision 4, decisions 11 to 13, 28 to 30, and 55.
