# Decision 24: 0.1 Scope Reset (Wasm 1.0 + WASI Preview 1 Only, App-Driven Goals)

Status: **Accepted, 2026-07-25; amended by [decision 69](69-exception-handling-accepted-input.md), 2026-08-14.**
The feature-audit tool (`crates/dewasm-core/src/bin/feature-audit.rs`) and the excision have landed.
Reference types, tail calls, and exception handling are removed.
The component model and WASI preview 2 are removed too.
They are gone from the IR, backends, runtime units, harness, and docs.
Their inputs are rejected at conversion time with attributed errors.
The support maturity levels' retirement and the rename land next ([decision 25](25-retire-support-levels.md)).
Supersedes:

- [decision 17](17-ruby-reference-types.md);
- [decision 18](18-ruby-tail-calls.md);
- [decision 19](19-ruby-exception-handling.md);
- [decision 20](20-component-model-core-ir-adapters.md);
- [decision 21](21-ruby-wasi-preview2.md).

Revises the target-language plan of [decision 10](10-csharp-target.md).
The retention criterion below is unchanged.
Later, mruby became a pinned app (decision 69).
The criterion's first disjunct then re-admitted exception handling.
The "everything else stays out" list no longer includes exception handling.

## Context

Right after completing wasm 1.0 for Ruby (decision 16), the project extended the Ruby backend.
In a single sprint it added reference types, tail calls, and exception handling (decisions 17-19).
The same sprint added the component model + WASI preview 2 (decisions 20-21).
A retrospective found the project harder to see whole:

- the input surface had grown faster than the number of backends able to carry it;
- goals were phrased as spec coverage rather than as programs users can convert;
- every IR construct added is a construct all future backends must either lower or reject.

Meanwhile the planned 0.1 apps and backends (below) need none of the post-1.0 features.

## Decision

For the 0.1 release, the accepted input is **wasm 1.0 + WASI preview 1**.
Here "wasm 1.0" means the MVP plus two sets:

- the universally-emitted baseline.
  That is sign extension, saturating float-to-int, multi-value, bulk memory, and mutable globals;
- the decision 16 completion set.
  That is non-function imports, multiple tables, and table bulk ops.

Everything else is **removed from the code**, not frozen.
That is reference types, tail calls, exception handling, the component model, and WASI preview 2.
It also covers all unimplemented proposals.
IR variants, lowering, feature tests, runtime units, harness support, and docs rows all go.
Unsupported input keeps failing at conversion time with a clear error (decision 0).

The app audit (`agents/apps-audit.md`) found one validator-level nuance.
LLVM-based toolchains encode `call_indirect` immediates as overlong LEBs.
They do so when the reference-types *target feature* is on (their default).
So real wasip1 binaries only validate with the reference-types feature bit enabled.
That includes the already shipping qjs and sqlite3.
The bit therefore stays on in `dewasm-core::module::features()` as a pure **encoding relaxation**.
IR building rejects every actual reference-types construct with the usual attributed error.
Those constructs are externref, table instructions, `ref.*`, and non-zero table indices.

The discriminating criterion: **a feature stays only if one of two conditions holds.**

- **A pinned target app needs it.**
- **Every 0.1 backend is expected to implement it.**

Code kept "just in case" is paid for in every exhaustive match, every new backend, and every reader.
Decisions 17-21 are kept as design records.
With git history, they make restoration cheap if the need returns.

Goals are stated app-first.
The spec testsuite remains the correctness test (decision 3), not the goal.
0.1 targets:

- cowsay (backend bring-up);
- quickjs-ng (one-shot, script with file I/O, REPL);
- sqlite3 (shell and C-API library, DB files on disk, callback binding);
- ripgrep;
- a CPython or CRuby runtime binary;
- a compression CLI.

A **feature audit** (conversion-time feature report over each pinned binary) runs before the excision.
An app that needs a dropped feature is deferred with a written note in `agents/apps-audit.md`.
It does not block the excision.
pandoc.wasm is the expected first deferral.
GHC's wasm backend is believed to emit tail calls; the audit is to confirm it.

0.1 backends: Ruby and Bash (existing) plus **Python, Go, Java**.
Each new backend's first milestone is "cowsay runs".
Its 0.1 bar is a passing spec harness plus full WASI p1 including the filesystem.
If the schedule demands, the release may relax its bar.
The relaxed bar is "Python at the full bar; Go/Java at the cowsay milestone".
That call is made at release time, not now.

Future work recorded, deliberately out of 0.1 scope:

- restoring wasm 2.0+ support;
- wasix and partial emscripten-runtime import surfaces as possible additional input dialects;
- Haskell/OCaml (and C#, per the decision 10 revision) as later target languages.

## Rejected alternatives

- **Freeze instead of delete** (keep the code, restrict new work to 1.0): dormant variants still appear.
  They appear in every exhaustive match, every harness tag, and the support matrix.
  The visible surface is what made the project hard to see.
  Deletion is the only option that shrinks it.
- **Keep Ruby's 2.0+ support as badges**: it is tested, working code.
  Removing it costs real churn.
  It lost to the criterion above.
  No pinned app needs it, and no other 0.1 backend will implement it.
  A five-backend project whose backends accept different inputs reintroduces an asymmetry.
  It is exactly the asymmetry decision 23 tried to manage.
- **Spec-coverage goals**: coverage numbers do not answer "what can I convert?"; apps do.
  Each app pins an exact WASI surface to implement.
  The spec testsuite stays as the test underneath.

## Consequences

- Positive: one input dialect shared by all five 0.1 backends.
  The IR a new backend must lower shrinks; goals become demos a user can run.
- Negative: working, spec-passing Ruby code for decisions 17-21 is deleted.
  That is ~4k LOC + 37 runtime units.
  pandoc is likely deferred; users with 2.0+ binaries are turned away at conversion time.
- Carry-over: decisions 17-21 stay in the tree as Superseded design records for a future restoration.
  The excision commit flips their statuses.
- The support maturity levels lose their reason to exist once every backend targets the same bar.
  They are retired separately in [decision 25](25-retire-support-levels.md).
