# ADR-11 — Bash Backend Lowering Conventions (Integer Subset)

Status: **Accepted, 2026-07-23.** Implemented in
`crates/dewasm-backend-bash/src/lib.rs` + `runtime/bash/units/`. Covers
the integer subset; WASI and standalone mode landed the same day under
[ADR-12](12-bash-wasi.md), and the ADR-5 softfloat (conventions in
[ADR-13](13-bash-softfloat-conventions.md)) later removed the float
conversion-time gate this ADR originally imposed. Cross-module linking
conventions (imported globals, the PROVIDERS provider protocol, status-135
link errors) are extended by [ADR-35](35-bash-cross-module-linking.md).
Requires bash >= 5 (namerefs, associative arrays); macOS system bash is 3.2
and is out of scope.

## Context

Bash arithmetic (`$(( ))`) is signed 64-bit only, `(( x = 0 ))` returns
exit status 1, there are no exceptions, no binary-safe strings, and
`break N`/`continue N` count only enclosing loops. Exceeding `FUNCNEST`
kills the whole shell, not just the call. Every convention below exists to
fit wasm semantics into those facts with zero external commands (the ADR-5
dependency criterion, applied backend-wide).

## Decision

- **i32 is masked-unsigned (ADR-2); i64 is the signed-64 two's-complement
  bit pattern.** i32 fits bash's signed-64 range, so unsigned compare,
  `shr_u`, and `div_u` are native. i64 cannot be stored unsigned; unsigned
  views are derived per-op: compares flip the sign bit
  (`(a ^ 1<<63) < (b ^ 1<<63)`), `shr_u` masks the dragged-in sign bits
  (special-casing shift 0 — bash takes shift counts mod 64), and
  `div_u`/`rem_u` use the Hacker's-Delight halving trick
  (`runtime/bash/units/rt/i64_div_u.sh`). The discriminating rule: an op
  whose unsigned semantics are free on the chosen representation lowers
  inline; anything needing a trap check or a loop becomes a runtime unit.
- **Structured control flow maps to `while :; do ...; break; done`
  wrappers; `br` is `break N`/`continue N`.** `if` adds no loop level in
  bash, so the generator's label→depth stack stays exact. Unreferenced
  labels emit no wrapper (ADR-1's `referenced` flag); `br_table` is a
  `case` (also level-neutral).
- **Traps are a status cascade, not subshells.** `rt_trap` sets
  `TRAP_MSG` (ADR-2's exact message strings) and returns 134; every
  trap-capable statement is a command with `|| return $?` appended, and
  every generated/runtime function ends with an explicit `return 0`
  because a trailing arithmetic statement leaks status 1 (the units lint
  enforces this). Assertions therefore run in the parent shell and
  side effects of checked calls persist, matching Ruby's exceptions.
  Only `assert_exhaustion` runs in a subshell: FUNCNEST overflow kills the
  shell it happens in.
- **Values flow through globals `R0, R1, ...`;** locals/temps/params are
  `local` (dynamic scoping handles recursion). Statements split by shape:
  pure arithmetic lowers into one `(( dst = expr ))`, helper-backed ops
  and loads emit a command then `(( dst = R0 ))`; nested command results
  are copied to `__t<n>` scratch locals so a later helper cannot clobber
  them. Operand/destination aliasing is real: `memory.grow` must update
  pages before deriving the old size because the delta may live in the
  destination temp.
- **Linear memory is a sparse indexed array, one byte per element**, read
  as `__m[a]` inside arithmetic (unset elements are 0, so zero-init and
  `memory.grow` are free). Loads/stores are nameref units
  (`runtime/bash/units/mem/`); bounds checks compare against
  `pages * 65536`. Tables, globals, and data segments are plain per-prefix
  variables emitted inline; `call_indirect` checks the canonicalized type
  index (ADR-4's structural canonicalization, ported).
- **One instance per generation-time prefix** (`m1_f0`, `m1_g0`,
  `m1_init`, `m1_invoke`, `m1_EXPORTS`); the spec harness passes a fresh
  prefix per module directive, which is how one script hosts many
  modules. Imports resolve from the caller's `IMPORTS` associative array
  (`[module.name]=function`); `RuntimeLinkage::Embedded` prepends the
  unit bundle, `Alias` emits nothing because bash names are global.

Measured on the spec harness (ADR-3): the curated CI subset passes
1,455 assertions in ~1 s; the full-testsuite sweep passes 9,923 with only
the Ruby ledger's five linking-attributed failure groups, in ~39 s
(Ruby: ~13 s). The feared fork cost never materialized because the
cascade design forks only for exhaustion checks.

## Rejected alternatives

- **Dispatch variable for multi-level `br`** (set a level flag, re-test
  after every block) — costs a test per block exit and obscures the code;
  `break N` maps 1:1 once `if`'s level-neutrality is established.
- **Subshell per assertion for trap isolation** — loses the side effects
  of checked invokes (`set_x` then a getter is common in the testsuite)
  and pays a fork per assertion; the status cascade isolates nothing
  because it never needs to.
- **Word-packed memory (8 bytes/element)** — less RAM and faster bulk
  ops, but every load/store pays shift/mask reassembly. Byte-per-element
  with sparse reads is simpler and measured fast enough for the spec
  suite; revisit when MB-class app memories (QuickJS/SQLite) become the
  target.
- **External commands (`od`, `awk`, `bc`) for bit work** — rejected by
  ADR-5's criterion: the dependency set must be exactly a Bash
  interpreter.

## Consequences

- Positive: the shared `Backend`/`RuntimeBundler`/spec-harness machinery
  (ADR-6, ADR-3) carried over unchanged except for per-language harness
  emitters — the multi-language design is validated. Runtime speed is a
  non-issue at spec scale.
- Negative (resolved): float-using modules were refused (attributed
  `floats`, ADR-8) until the ADR-5 softfloat landed under ADR-13; the
  classic control-flow files and the pure-float suite are green since.
- Deep recursion without `FUNCNEST` segfaults bash around 10–20k frames;
  exhaustion checks must stay inside `( FUNCNEST=...; ... )` subshells.
- Bulk memory ops loop per byte; large `memory.copy`/`fill` will need
  batching before real apps run under bash.
