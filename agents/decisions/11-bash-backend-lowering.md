# Decision 11: Bash Backend Lowering Conventions (Integer Subset)

Status: **Accepted, 2026-07-23.**
Implemented in `crates/dewasm-backend-bash/src/lib.rs` + `runtime/bash/units/`.
Covers the integer subset.
WASI and standalone mode landed the same day under [decision 12](12-bash-wasi.md).
This decision originally imposed a float conversion-time rejection.
The decision 5 softfloat later removed it.
The softfloat conventions are in [decision 13](13-bash-softfloat-conventions.md).
[Decision 35](35-bash-cross-module-linking.md) extends the cross-module linking conventions.
They cover imported globals, the PROVIDERS provider protocol, and status-135 link errors.
Requires bash >= 5 (namerefs, associative arrays).
macOS system bash is 3.2 and is out of scope.

## Context

Bash arithmetic (`$(( ))`) is signed 64-bit only.
`(( x = 0 ))` returns exit status 1.
There are no exceptions and no binary-safe strings.
`break N`/`continue N` count only enclosing loops.
Exceeding `FUNCNEST` kills the whole shell, not just the call.
Every convention below exists to fit wasm semantics into those facts with zero external commands.
This is the decision 5 dependency criterion, applied backend-wide.

## Decision

- **i32 is masked-unsigned (decision 2); i64 is the signed-64 two's-complement bit pattern.**
  i32 fits bash's signed-64 range, so unsigned compare, `shr_u`, and `div_u` are native.
  i64 cannot be stored unsigned.
  Its unsigned views are derived per-op:

  - Compares flip the sign bit (`(a ^ 1<<63) < (b ^ 1<<63)`).
  - `shr_u` masks the dragged-in sign bits and special-cases shift 0.
    Bash takes shift counts mod 64.
  - `div_u`/`rem_u` use the Hacker's-Delight halving trick (`runtime/bash/units/rt/i64_div_u.sh`).

  The discriminating rule is the cost of the unsigned semantics on the chosen representation.
  An op whose unsigned semantics are free there lowers inline.
  Anything needing a trap check or a loop becomes a runtime unit.
- **Structured control flow maps to `while :; do ...; break; done` wrappers.**
  **`br` is `break N`/`continue N`.**
  `if` adds no loop level in bash, so the generator's label→depth stack stays exact.
  Unreferenced labels emit no wrapper (decision 1's `referenced` flag).
  `br_table` is a `case` (also level-neutral).
- **Traps are a status cascade, not subshells.**
  `rt_trap` sets `TRAP_MSG` (decision 2's exact message strings) and returns 134.
  Every trap-capable statement is a command with `|| return $?` appended.
  Every generated/runtime function ends with an explicit `return 0` (the units lint enforces this).
  That is because a trailing arithmetic statement leaks status 1.
  Assertions therefore run in the parent shell, and side effects of checked calls persist.
  This matches Ruby's exceptions.
  Only `assert_exhaustion` runs in a subshell: FUNCNEST overflow kills the shell it happens in.
- **Values flow through globals `R0, R1, ...`.**
  Locals/temps/params are `local` (dynamic scoping handles recursion).
  Statements split by shape:

  - pure arithmetic lowers into one `(( dst = expr ))`;
  - helper-backed ops and loads emit a command then `(( dst = R0 ))`.

  Nested command results are copied to `__t<n>` scratch locals.
  So a later helper cannot clobber them.
  Operand/destination aliasing is real.
  The delta of `memory.grow` may live in the destination temp.
  So `memory.grow` must update pages before deriving the old size.
- **Linear memory is a sparse indexed array, one byte per element.**
  It is read as `__m[a]` inside arithmetic.
  Unset elements are 0, so zero-init and `memory.grow` are free.
  Loads/stores are nameref units (`runtime/bash/units/mem/`).
  Bounds checks compare against `pages * 65536`.
  Tables, globals, and data segments are plain per-prefix variables emitted inline.
  `call_indirect` checks the canonicalized type index.
  This ports decision 4's structural canonicalization.
- **One instance per generation-time prefix** (`m1_f0`, `m1_g0`, `m1_init`, `m1_invoke`, `m1_EXPORTS`).
  The spec harness passes a fresh prefix per module directive.
  That is how one script hosts many modules.
  Imports resolve from the caller's `IMPORTS` associative array (`[module.name]=function`).
  `RuntimeLinkage::Embedded` prepends the unit bundle.
  `Alias` emits nothing because bash names are global.
  (Revision, [decision 62](62-embedded-runtime-isolation.md): the `Embedded` bundle's own function names take the same prefix.
  Examples are `<p>rt_trap` and `<p>mem_i32_load`.
  So two converted scripts in one shell no longer redefine each other's runtime.
  `Alias` keeps the flat names, which is what the shared bundle defines.)

Measured on the spec harness (decision 3):

- the curated CI subset passes 1,455 assertions in ~1 s;
- the full-testsuite run passes 9,923 in ~39 s (Ruby: ~13 s).
  Its only failures are the Ruby list's five linking-attributed failure groups.

The feared fork cost never materialized because the cascade design forks only for exhaustion checks.

## Rejected alternatives

- **Dispatch variable for multi-level `br`** (set a level flag, re-test after every block).
  It costs a test per block exit and obscures the code.
  `break N` maps 1:1 once `if`'s level-neutrality is established.
- **Subshell per assertion for trap isolation**: loses the side effects of checked invokes.
  `set_x` then a getter is common in the testsuite.
  It also pays a fork per assertion.
  The status cascade isolates nothing because it never needs to.
- **Word-packed memory (8 bytes/element)**: less RAM and faster bulk ops.
  But every load/store pays shift/mask reassembly.
  Byte-per-element with sparse reads is simpler and measured fast enough for the spec suite.
  Revisit when MB-class app memories (QuickJS/SQLite) become the target.
- **External commands (`od`, `awk`, `bc`) for bit work**: rejected by decision 5's criterion.
  The dependency set must be exactly a Bash interpreter.

## Consequences

- Positive: the shared `Backend`/`RuntimeBundler`/spec-harness machinery carried over unchanged.
  That machinery comes from decision 6 and decision 3.
  The exceptions are the per-language harness emitters.
  So the multi-language design is validated.
  Runtime speed is a non-issue at spec scale.
- Negative (resolved): float-using modules were refused (attributed `floats`, decision 8).
  That lasted until the decision 5 softfloat landed under decision 13.
  The classic control-flow files and the pure-float suite have passed since.
- Deep recursion without `FUNCNEST` segfaults bash around 10-20k frames.
  Exhaustion checks must stay inside `( FUNCNEST=...; ... )` subshells.
- Bulk memory ops loop per byte.
  Large `memory.copy`/`fill` will need batching before real apps run under bash.
