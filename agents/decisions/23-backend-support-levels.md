# Decision 23: Backend Support Maturity Levels, Specialized to Wasm 1.0 + WASI Preview 1

Status: **Superseded (decision 25), 2026-07-26.**
[Decision 25](25-retire-support-levels.md) replaced it.
Originally accepted 2026-07-24 and implemented as described below.
It is kept here as history of the maturity levels' design and reasons.
Decision 25 removes the level machinery entirely.

## Context

`Feature` (decision 8) is a flat matrix: 20 rows, each `Supported`/`Partial`/`Unsupported` per backend.
It has no vocabulary for "how far along is this backend overall".
So Ruby and Bash's actual standing had to be reconstructed by reading the table.
More backends are being added (Java+C# per decision 10, then Go/Python/PHP).
So each needs a stated goal a reviewer can check against.
Zig's [platform support levels](https://ziglang.org/learn/platform-support/) work that way.
There, a target's status is read off a scale instead of a feature-by-feature diff.

The natural single-number scale holds only if every wasm binary can be measured against its scope.
Wasm 2.0+ proposals (reference types, tail calls, GC, SIMD, threads, ...) do not qualify.
Neither do the component model / WASI Preview 2.
That is because their adoption is concentrated in the Bytecode Alliance projects.
It still changes often (WASI 0.3 is next).
The common case is wasm 1.0 plus WASI Preview 1, a frozen ABI.
That is what Zig, Go, and most existing wasm binaries actually emit.
A scale built to include CM would force every future backend toward an uncertain target.
Otherwise it would make Level 1 a moving one.

## Decision

**The maturity levels cover wasm 1.0 + WASI p1 only**, best-first:

- **Level 1 (Full)**: wasm 1.0 handled completely.
  Imported globals/memories/tables + floats are all `Supported`.
  WASI p1 is handled completely short of the out-of-scope surface.
  That surface is `sock_accept/recv/send/shutdown` and `proc_raise`.
  Wasmtime itself leaves these unimplemented; no toolchain output exercises them.
  Also requires the harness's `EXPECTED_FAILURES` list to hold no wasm-1.0-attributable entries.
  For example, Ruby's `import-limits` tag (decision 16) blocks Level 1 today.
- **Level 2 (Production)**: wasm 1.0 imports complete.
  Plus the WASI p1 file system functions (`path_open` and friends, 14 total).
  The default target for new backends: the scope a major toolchain's typical CLI/library output needs.
- **Level 3 (Core)**: a single wasm 1.0 module with function-only imports.
  Plus the 16-function WASI p1 core.
  That core is `args`/`environ`/clock/`fd` read-write-seek-close/`proc_exit`/random/`sched_yield`.
  Self-contained CLI tools live here.
- **Level 4 (Experimental)**: run by the specification harness but outside the default test.
  Its pass/fail/skip totals feed the achieved level.
  The starting point for a new backend.

Requirements build on each other (Level 2 implies Level 3, etc.).
Each `Feature`/WASI function names the level whose requirements include it.
This mapping, not a restated table, is the source of truth.
`docs/support.md`'s level section and per-row level columns were generated from it.

**Everything outside wasm 1.0 + WASI p1 is an independent extension label**.
The existing `Feature` rows for post-1.0 proposals and the component model (`is_extension`) are labels.
They stay exactly as decision 8 defined them: optional per backend, never affecting a level.
`MultipleTables`/`TableBulkOps` are wasm-2.0-adjacent bulk-memory-proposal features.
They sit on the label side too.
That holds even though `ImportedGlobals/Memories/Tables` (true wasm 1.0) are level-conditional.

The achieved level and its gap report only check what's statically knowable.
That is a backend's own declarations: feature status, `has_wasi_p1`, the list flag.
The test suite itself checks the dynamic half: the specification run and e2e cases passing.
Decision 8 already draws the same split between declaration and enforcement.
The target level is a second, independent declaration.
It is what the backend *aims* for, not what it has reached, so the generated documents can show both.

The shared e2e case tables (`StandaloneCase`/`LibraryCase`/`AppCase`) carry a required level.
A case only runs for a language whose achieved level meets it.
Otherwise it prints a skip line rather than failing.
This is a declared-level gap, not a decision 15 missing-tool failure.
Every case in the suite today only needs Level 3.
Re-running each under Bash before assigning levels confirmed this.
That includes `sqlite3-shell`, which looked file-system-heavy but takes no database file.
Component-model e2e is conditional on the component-model label directly, not a level.
It is the model for label-conditional cases once one exists.

## Rejected alternatives

- **Include wasm 2.0+/CM in the scale.**
  For example, Level 1 would be "every feature dewasmify tracks, including CM".
  Rejected: it ties Level 1 to the changes of WASI 0.3.
  It also ties Level 1 to proposals of uncertain adoption outside the Bytecode Alliance projects.
  It would implicitly commit every future backend to a CM port to reach the top level.
- **A second, independent WASI-support scale**.
  That is, instead of folding WASI p1 into the same scale as wasm-core features.
  Rejected: a backend's wasm-core completeness and its WASI completeness move together in practice.
  Ruby's decision 16 work touched both.
  Two scales would need a combination rule to answer "what level is this backend" anyway.
  One scale with WASI folded in is more direct.
  It matches decision 8's existing practice of listing WASI p1 alongside the `Feature` rows.
- **Numbering upward, each level adding to the last** (Level N ⊇ Level N-1, 1 = weakest).
  That is, instead of Zig's best-first convention, which counts down.
  Rejected per explicit user preference for the Zig framing users may already recognize.

## Consequences

- Positive: The achieved and target levels give a one-line answer to "how far along is this backend".
  Currently Ruby = Level 2 targeting Level 1.
  Bash = Level 3, its settled target.
  That is because the core intended use case, running self-contained C/Rust CLI tools, is met there.
  New backends have a concrete Level 2 checklist.
  They need not copy Ruby's full feature set.
- Positive: e2e cases self-describe their requirement instead of being hand-conditional per language.
  A future language automatically inherits every case its level covers.
- Negative / limits: the Level 1 list-cleanliness declaration is a second place to update.
  The other is the harness's `EXPECTED_FAILURES`; both must be updated when a list entry is cleared.
  The declaration can drift out of date, staying negative.
  That only under-states a level, never over-states one.
  So the failure mode is safe, but it requires remembering to flip it.
  Ruby has not reached Level 1: 12 WASI p1 functions and the `import-limits` list are the gap.
  The functions are:
  - `fd_advise`/`allocate`/`fdstat_set_*`/`renumber`;
  - `poll_oneoff`;
  - `path_link`/`readlink`/`symlink`;
  - `fd_filestat_set_times`/`path_filestat_set_times`.

See also:

- [decision 8](8-latest-testsuite-support-matrix.md) (the `Feature` matrix and attribution this builds on);
- [decision 15](15-tests-fail-not-skip.md) (the fail-loud policy level skips deliberately don't fall under);
- [decision 16](16-ruby-wasm1-completion.md) (the `import-limits` list gap blocking Ruby's Level 1).
