# Decision 94: Codon Backend Lowering Conventions

Status: **Accepted, 2026-09-13.**
Implemented in these files:
- `crates/dewasm-backend-codon/src/lib.rs`;
- `crates/dewasm-backend-codon/units/`;
- `crates/dewasm-backend-codon/tests/{spec,wasi_testsuite,convert,e2e,module_name}.rs`.

The implementation covers:
- wasm 1.0, with the shared specification harness passing (decision 3);
- full WASI p1 including the file system, with the conformance harness passing (decision 36);
- the exception-handling (decision 69) and tail-call (decision 88) lowerings;
- the full e2e surface.

Three items remain (issue #311):
- the shared performance passes (`extract`/`licm`/`fuse`/`selfcall`);
- the deep-crossing flat dispatch;
- an unboxed same-module `call_indirect` fast path.

## Context

Codon compiles a statically typed Python dialect through LLVM.
Decision 93 rejected it as a speed-suite engine runner.
That is because it cannot run the Python backend's unmodified output.
The 2026-09-13 investigation (issue #310) measured whether a dedicated backend could work instead:
- `UInt[32]`/`UInt[64]` arithmetic is correct where the masked-unsigned convention is silently wrong.
  Codon's `int` is signed 64-bit.
- Small compute loops ported by hand in the generated-code shape run near Wasmtime speed.
- Compile time is seconds at microbenchmark size, but grows fast on the biggest artifacts.
  This was attributed here to huge single functions.
  The issue #319 investigation later split the cost into two terms.
  One is a parser term that grows with the square of a single literal's length.
  The other is a `-release`-only capture-analysis term on the largest functions.

Codon's semantics sit between the dynamic and the compiled targets.
So each lowering choice below names which existing backend's shape it takes.
It also names where Codon forced a different one.

## Decision

- **Numerics: the Go shape (decision 29), spelled `UInt[32]`/`UInt[64]`/`float32`/`float`.**
  Wrapping arithmetic and unsigned division are the stored representation's own.
  So are unsigned compares and logical shifts.
  Signed views are `Int[32]`/`Int[64]` casts.
  Sign-extension chains through `Int[8]`/`Int[16]` casts.
  Signed `//`/`%` are floor-based in Codon.
  So the `div`/`rem` trap helpers carry the adjustment from rounding down to rounding toward zero.
  Those helpers are needed anyway for the zero and `INT_MIN/-1` cases.
  Type spellings are written out in full.
  Two `Embedded` artifacts can share one namespace.
  A module-level alias (`u32 = UInt[32]`) would be one more name both of them define.
- **A float operation with a constant operand is wrapped in `Rt.f32_q`/`Rt.f64_q` (quiet if NaN).**
  Measured on Codon 0.20.1: LLVM under `-release` folds `x * 1.0`, `x / 1.0` and `x + -0.0` to `x`.
  That skips the quieting of a signaling NaN that wasm requires of every arithmetic result.
  Go's fix (`//go:noinline` on the `mul`/`div` helpers, decision 29) has no Codon equivalent.
  Quieting the result afterwards is correct whether or not the fold happened.
  Only constant-operand sites pay its cost.
  For the same class of reasons, three more operations are `@llvm` units:
  - Float division is an `@llvm` `fdiv` unit, because Codon's own `/` raises when dividing by zero.
  - i64-to-f32 conversion is a direct `sitofp`/`uitofp` `@llvm` unit.
    Going through double double-rounds, which the conversions.wast rounding-direction cases catch.
  - The `reinterpret`/`abs`/`neg`/`copysign` bit paths are `@llvm` `bitcast` units.
- **Control flow: the Python backend's branch-register model (decision 28).**
  The flat state machine is not part of it yet.
  Codon has Python's statement syntax and no labeled break.
  So Go's labeled-loop shape is unavailable.
  The `_br` register with lazily opened `if _br == 0:` regions ports directly.
  The measured loops show LLVM compiles it to code that runs near Wasmtime speed.
  The deep-crossing flat dispatch (decision 60's criterion) is a performance follow-up.
  Relays are always emitted today.
- **Every tail call parks; none runs its callee inline.**
  The trampoline is the Go shape (decision 29's body/entry split).
  It has typed per-(position, type) argument slots and one entry table per result signature.
  Entries exist for the tail callers *and* every direct `return_call` target.
  A callee with no entry here parks as a boxed pending call instead.
  Such a callee is an import, or another instance's `funcref`.
  The criterion: a tail call must run its callee only after this frame is gone.
  The frame's exception handlers must be gone too.
  The return-call-in-try-catch case of `try_table.wast` tests it.
  So "complete it inline in one frame" is never a correct fallback.
- **The dynamic boundary is uniformly boxed, the Java shape (decision 30) under Codon's nominal typing.**
  A wasm function value is an `Rt.Fn` subclass, with `invoke(List[Val]) -> List[Val]`.
  `Val` carries one field per representation.
  f32 has its own field, so boxing never re-rounds.
  An import/export value is an `Rt.Extern` with a typed field per kind, four of them for globals.
  Resolution checks the kind and a function's structural type key (`Extern.fn_ty`).
  It checks a global's value type by construction.
  That mirrors what Go's type assertion checks.
  Import sources are plain `Dict[str, Dict[str, Extern]]`.
  An instance's `exports` dictionary fits in directly.
  The dynamic backends accept any provider object that has the needed methods (decision 7).
  Codon has no equivalent.
  So the dictionary *is* the provider contract.
  Direct calls to defined functions stay native; only imports, exports, and `call_indirect` box.
- **Codon resolves the declared types of nested-class fields and method signatures in definition order.**
  **So the runtime's scope order is a dependency order.**
  It was measured that method bodies are resolved late and declared types are not.
  The three boxing types (`Val`/`Fn`/`Funcref`) are one unit (`rt/boxed`), in a fixed order inside it.
  `Extern` and the import machinery live in a final `ext` scope.
  That is because the declared field types of `Extern` name types across every other scope.
  Those types are `Fn`, `Global`, `Table`, `Memory` and `Tag`.
- **Memory is a raw `Ptr[byte]` with `@llvm` `align 1` loads and stores.**
  Codon's `Ptr[T]` indexing would assume an address that is a multiple of the value's size.
  The explicit `align 1` units make access at any address defined, and cost nothing on the target hosts.
  Allocations are set to zero with `memset` (Codon's GC does not clear atomic allocations).
- **The specification harness phrases each assertion as a named thunk.**
  **Specification builds carry the Go backend's recursion guard.**
  One flat module-level run of thousands of assertions would be one huge function.
  That is the shape whose `-release` compile time collapses in Codon's capture analysis (issue #319).
  Thousands of small functions compile linearly.
  A native stack overflow ends the process and is uncatchable.
  So specification builds count frame slots into a module-level `_rt_stack`.
  They trap at the same budget as Go's (decision 29's `SPEC_STACK_LIMIT` reasoning).
  Shipped output carries no guard.
  The standalone entry point has no guard against deep recursion yet.

## Rejected alternatives

- **The masked-unsigned convention (decision 2's dynamic-backend spelling).**
  Codon's `int` is signed 64-bit, so `(a * b) & M64` wraps.
  Division and comparison on values at or above 2^63 are silently wrong.
  `-numerics=py` does not change this.
  This was measured in the issue #310 investigation.
  The measurement is what made this a new backend rather than an engine runner.
- **A CPython-compatible output dialect.**
  `UInt[N]`, `Ptr[byte]` and `@llvm` blocks do not run under `python3`.
  Keeping compatibility would give up exactly the native numerics and memory the target exists for.
  An artifact readable by CPython remains the Python backend's product.
- **Per-signature unboxed `call_indirect` dispatch (Go's typed-assertion shape).**
  Codon has no `any` to assert on, and no cast that narrows a type.
  So recovering a typed callable from an erased table slot has no direct spelling.
  The boxed `invoke` is correct everywhere.
  An unboxed same-module fast path is a measurable follow-up, not a semantic need.
- **Stopping the identity-fold with a `noinline` attribute (Go's shape).**
  Codon exposes no per-function inlining control that survives `-release`.
  An `@llvm` wrapper is inlined and folded the same.
  Quieting the result is the fix that does not depend on the optimizer's behavior.

## Consequences

- Positive: the selected specification harness passes with the failure list matching Go's surface.
  That surface is the import mutability/limit checks and the linking rows that depend on multi-memory.
  Every `wat/` and `c/` microbenchmark output is byte-identical to the Python backend's.
  The standalone artifacts run the microbenchmark speed contract.
- Negative: imported-function calls and `call_indirect` allocate a boxed value list per call.
  A data segment is emitted as adjacent chunk literals.
  That is because Codon's parser cost grows with the square of a single literal's length (issue #319).
  `codon build -release` does not finish on artifacts with functions the size of SQLite's.
  The cause is Codon's capture analysis, which grows faster than linearly in function size.
  That keeps the SQLite cases out of the speed suite.
  It stays unsettled until measured against the extraction pass (decision 81).
- Carry-over: issue #311 tracks four items:
  - the shared performance passes (decisions 81-83, 90);
  - the deep-crossing flat dispatch (decision 60's criterion);
  - an unboxed same-module `call_indirect` fast path;
  - the giant-artifact `-release` compile cost.

  The test suites build debug throughout.
  No release-mode verification pass exists in any category; only the benchmarks build `-release`.
  The emission-level NaN quieting keeps the generated code's semantics optimizer-independent.
  So a difference that only appears at `-release` would be a Codon miscompilation.
  That is the compiler's bug to fix, rather than this backend's to test for.

  WASI p1 rests on `libc` through calls into C, with `__apple__`-conditional layouts.
  Those layouts are verified on Darwin arm64 and Linux x86_64.
  Any other machine is refused when the bundled WASI is constructed (`rt/host_check`).
  On Darwin x86_64 the un-suffixed `stat`/`readdir` symbols are the legacy 32-bit-inode variants.
  Linux aarch64 `glibc` lays `struct stat` out differently.
