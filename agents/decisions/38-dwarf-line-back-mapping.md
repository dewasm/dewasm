# Decision 38: Optional DWARF Line-Number Back-Mapping (`--dwarf-line`)

Status: **Accepted, 2026-07-28.**
Implemented in `dewasm-core` behind the CLI's `--dwarf-line` flag.
Go renders `//line` directives, and Ruby and Python render source-line comments.
Bash and Java render nothing.
Default (no-flag) output is byte-identical to before.

## Context

A converted module is a wall of generated code with no tie back to the original C/Rust.
When it traps or misbehaves, nothing points at the source line that produced a given statement.
wasm2go's `-dwarfline` is the prior art.
It reads the `.debug_*` custom sections and emits source-position markers.
The generated code then maps back to the origin.

The wasm carries this already: Clang/`zig cc -g` emit standard DWARF as custom sections.
The open questions were these:

- where the DWARF reader lives;
- how a marker is represented in the IR;
- how dense the markers should be;
- which backends can render one at all.

## Decision

Add an **optional** `--dwarf-line` CLI flag.
It flows as `--dwarf-line` → `BuildOptions { debug_line }` → `build_module_with_options`.
`build_module` stays a wrapper with no arguments.
So every existing call site is untouched and byte-identical.
When the flag is set, core parses the `.debug_line` program and adds markers to the IR.
A backend renders or drops the marker.

- **`gimli` lives in `dewasm-core` only** (`crates/dewasm-core/src/debug_line.rs`).
  Its features are `default-features = false, features = ["read", "std"]`.
  Backends never see `gimli` or any DWARF type: they only ever see the resolved `ir::SourcePos`.
- **Marker representation:** a new IR statement `Stmt::SourceLine(SourcePos)`.
  `SourcePos` is `{ file, line, col }`, with `file` indexing `Module::debug_files`.
  The marker is emitted just before the statement it marks.
  It has no semantic effect: a backend renders it as a directive/comment or drops it.
  Its presence never changes the surrounding statements' meaning.
- **Change-points only:** `FuncBuilder` tracks a source position as the body streams.
  The position is resolved from each operator's module-file offset.
  The offset comes from `wasmparser` 0.254 `OperatorsReader::read_with_offset`.
  `FuncBuilder` emits a marker only when the position *changes*.
  Marker count is proportional to line transitions, not statements.
- **Address-base calibration (one named constant):** a wasm DWARF code address is a relative offset.
  It is relative to **the start of the code section's contents**.
  `wasmparser` reports operator positions as absolute module-file offsets.
  So the lookup subtracts the code-section content start (`address_base`).
  This is *calibrated, not assumed*.
  The fixture test asserts where `add_mul`'s marker lands.
  It must be the exact source line of the function's first statement.
  That assertion fails for any wrong base.

**Deciding rule:** source back-mapping is debug information with no semantic content.
So it is a per-run *flag*, never a default and never a backend capability flip.
The generated program's behaviour is identical with or without it.
The specification harness, which never sets the flag, still binds (decision 3).

**What each backend does with it:**

- **Go** honors a `//line file:line:col` directive *only at column 1*.
  So the marker is written through the writer's `raw` path, which skips indentation.
  It never carries a `line 0`, which `go build` rejects.
  Core drops DWARF's line-0 "no source" rows to gaps.
  This is the one backend where the marker is machine-consumed.
  It retargets compiler errors and stack traces.
- **Ruby / Python** render a `# <file>:<line>` comment, human-readable only.
  Neither language has a line-directive.
- **Bash / Java** render nothing (a one-line REASON at the match arm).
  Bash's status-chain lowering (decision 11) has no place for a line with no effect.
  Java has no directive.
  Both stay byte-identical to a non-flag build.

The two folded-code details both flow from decision 32's expression folding.
A folded function often collapses to a single fallthrough `Return`.
That `Return` is emitted off the function `end` operator.
That operator's offset sits on a line-table gap.
So the tracker holds the *last known* position across gaps rather than clearing it.
The fallthrough `Return` path does not route through `emit`, so it adds its marker explicitly.
Without both, a whole small function would carry no marker.

## Rejected alternatives

- **An optional position field on each `Stmt`.**
  Rejected: it widens every statement variant and every exhaustive match.
  It does so for information that most statements do not carry.
  A distinct `SourceLine` statement has no effect.
  It keeps the position on the statements that begin a source line.
  Those are ~0.7 of statements, and the rest are left untouched.
- **A side table (offset → position) consulted by backends.**
  Rejected: it would expose the DWARF/offset model past core into every backend.
  It would also force each backend to re-derive change-points.
  The whole point is that backends see only a resolved, pre-thinned marker.
- **Always on.**
  Rejected for the same reason as decision 37.
  It is a size/noise trade-off (the Go fixture gains ~1266 directives).
  It gives nothing for a module built without `-g`, so it belongs behind a flag.
- **Resolving position at emit-time from the emit-triggering operator.**
  The inherited first version did this.
  It dropped markers for folded bodies, since the triggering `end` lands on a gap.
  It also mis-attributed others.
  Tracking as operators stream is what makes folded code map correctly.

## Consequences

Positive: optional, correctness-neutral, default output byte-identical.
This was verified by stripping marker lines and diffing Go and Ruby before/after.
It was also verified by re-running both to identical `stdout` and exit status.
`gimli` is confined to core; the backend surface is one `SourcePos`.

Density, for the first-party `dwarf-fixture.wasm` as Go standalone: 1266 `//line` directives.
They are spread across 103 functions.
That is ~12/function, and ~0.7 per statement-bearing line.
The directives span 45 source files.
14 point into our `dwarf_fixture.c`.
The rest point into the statically linked `wasi-libc`/`musl` the fixture pulls in.
So the markers faithfully follow inlined library code too.
That is the honest picture of a `-g -O1` binary.

Limits: the address base is calibrated against Clang/LLD output (`zig cc`).
A toolchain emitting a different code-address convention would need a new `address_base` value.
That is a one-line change, guarded by the fixture test.
Most *released* wasm ships stripped of DWARF, for example the cached `qjs.wasm`, `ruby.wasm`.
So `--dwarf-line` yields no markers there.
The feature pays off for locally built, debug modules.
Column information is emitted for Go where present; Ruby/Python drop it.

Fixture: `examples/apps/src/dwarf_fixture.c` is first-party (decision 9).
`examples/apps/setup.sh` builds it with `zig cc -target wasm32-wasi -g -O1`.
Its line numbers are load-bearing for the calibration test.

Related decisions:

- decision 1 (IR design: semantics-neutral additions);
- decision 3 (the specification harness binds; the flag never changes it);
- decision 9 (first-party fixture source);
- decision 29 (Go lowering: the `raw`/column-1 constraint);
- decision 32 (expression folding: the folded-return marker detail);
- decision 37 (the related optional `--data-file`, the same shape: a flag, not a default).
