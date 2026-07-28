# ADR-38 — Opt-in DWARF Line-Number Back-Mapping (`--dwarf-line`)

Status: **Accepted, 2026-07-28.** Implemented in `dewasm-core` behind the CLI's
`--dwarf-line` flag; Go renders `//line` directives, Ruby and Python source-line
comments, Bash and Java nothing. Default (no-flag) output is byte-identical to
before.

## Context

A converted module is a wall of generated code with no tie back to the original
C/Rust. When it traps or misbehaves, there is nothing pointing at the source line
that produced a given statement. wasm2go's `-dwarfline` is the prior art: read the
`.debug_*` custom sections and emit source-position markers so the generated code
maps back to the origin.

The wasm carries this already — clang/`zig cc -g` emit standard DWARF as custom
sections. The open questions were where the DWARF reader lives, how a marker is
represented in the IR, how dense the markers should be, and which backends can
render one at all.

## Decision

Add an **opt-in** `--dwarf-line` CLI flag → `BuildOptions { debug_line }` →
`build_module_with_options` (`build_module` stays a zero-arg wrapper, so every
existing call site is untouched and byte-identical). When set, core parses the
`.debug_line` program and annotates the IR; a backend renders or drops the
annotation.

- **gimli lives in `dewasm-core` only** (`crates/dewasm-core/src/debug_line.rs`),
  `default-features = false, features = ["read", "std"]`. Backends never see
  gimli or any DWARF type — they only ever see the resolved `ir::SourcePos`.
- **Marker representation:** a new IR statement `Stmt::SourceLine(SourcePos)`
  (`{ file, line, col }`, `file` indexing `Module::debug_files`), emitted just
  before the statement it annotates. It is semantically inert — a backend renders
  it as a directive/comment or drops it, and its presence never changes the
  surrounding statements' meaning.
- **Change-points only:** `FuncBuilder` tracks the source position resolved from
  each operator's module-file offset (wasmparser 0.254
  `OperatorsReader::read_with_offset`) as the body streams, and emits a marker
  only when the position *changes*. Marker count is proportional to line
  transitions, not statements.
- **Address-base calibration (one named constant):** a wasm DWARF code address is
  an offset **relative to the start of the code section's contents**, whereas
  wasmparser reports operator positions as absolute module-file offsets, so the
  lookup subtracts the code-section content start (`address_base`). This is
  *calibrated, not assumed*: the fixture test asserts that `add_mul`'s marker
  lands on the exact source line of its first statement, which fails for any
  wrong base.

**Discriminating rule:** source back-mapping is debug metadata with no semantic
content, so it is a per-invocation *flag*, never a default and never a backend
capability flip — the generated program's behaviour is identical with or without
it (the spec harness, which never sets the flag, still binds; ADR-3).

**Per-backend applicability:**

- **Go** honors a `//line file:line:col` directive *only at column 1*, so the
  marker is written through the writer's `raw` path (bypassing indentation) and
  never carries a `line 0` (which `go build` rejects — DWARF's line-0 "no source"
  rows are dropped to gaps in core). This is the one backend where the marker is
  machine-consumed: it retargets compiler errors and stack traces.
- **Ruby / Python** render a `# <file>:<line>` comment — human-readable only;
  neither language has a line-directive.
- **Bash / Java** render nothing (a one-line REASON at the match arm). Bash's
  status-cascade lowering (ADR-11) has no place for an inert line, and Java has
  no directive; both stay byte-identical to a non-flag build.

The two folded-code subtleties both flow from ADR-32's expression folding: a
folded function often collapses to a single fallthrough `Return` emitted off the
function `end` operator, whose offset sits on a line-table gap — so the tracker
holds the *last known* position across gaps rather than clearing it, and the
fallthrough-return path (which does not route through `emit`) is annotated
explicitly. Without both, a whole small function would carry no marker.

## Rejected alternatives

- **A per-`Stmt` optional position field.** Rejected: it widens every statement
  variant and every exhaustive match for metadata that most statements do not
  carry; a distinct inert `SourceLine` statement keeps the position on the ~0.7
  of statements that begin a new source line and leaves the rest untouched.
- **A side table (offset → position) consulted by backends.** Rejected: it would
  leak the DWARF/offset model past core into every backend and force each to
  re-derive change-points; the whole point is that backends see only a resolved,
  pre-thinned marker.
- **Always on.** Rejected for the same reason as ADR-37: it is a size/noise
  trade-off (the Go fixture gains ~1266 directives) with no upside for a module
  built without `-g`, so it belongs behind a flag.
- **Resolving position at emit-time from the emit-triggering operator.** The
  inherited first cut did this; it dropped markers for folded bodies (the
  triggering `end` lands on a gap) and mis-attributed others. Tracking as
  operators stream is what makes folded code map correctly.

## Consequences

Positive: opt-in, correctness-neutral, default output byte-identical (verified by
stripping marker lines and diffing Go and Ruby before/after, and by re-running
both to identical stdout/exit). gimli is confined to core; the backend surface is
one `SourcePos`.

Density (first-party `dwarf-fixture.wasm`, Go standalone): 1266 `//line`
directives across 103 functions (~12/function, ~0.7 per statement-bearing line),
spanning 45 source files — 14 into our `dwarf_fixture.c`, the rest into the
statically linked wasi-libc/musl the fixture pulls in. So the markers faithfully
follow inlined library code too, which is the honest picture of a `-g -O1` binary.

Caveats: the address base is calibrated against clang/lld output (`zig cc`); a
toolchain emitting a different code-address convention would need `address_base`
re-pinned (a one-line change, guarded by the fixture test). Most *released* wasm
(e.g. the cached `qjs.wasm`, `ruby.wasm`) ships stripped of DWARF, so `--dwarf-line`
simply yields no markers there — the feature pays off for locally built, debug
modules. Column info is emitted for Go where present; Ruby/Python drop it.

Fixture: `examples/apps/src/dwarf_fixture.c` is first-party (ADR-9), built by
`examples/apps/fetch.sh` with `zig cc -target wasm32-wasi -g -O1`; its line
numbers are load-bearing for the calibration test.

Cross-refs: ADR-1 (IR design — semantics-neutral additions), ADR-3 (the spec
harness binds; the flag never changes it), ADR-9 (first-party fixture source),
ADR-29 (Go lowering — the `raw`/column-1 constraint), ADR-32 (expression folding —
the folded-return marker subtlety), ADR-37 (the sibling opt-in `--data-file`, same
flag-not-default shape).
