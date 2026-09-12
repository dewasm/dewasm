# Decision 94: Codon Backend Lowering Conventions

Status: **Accepted, 2026-09-13.** Implemented in `crates/dewasm-backend-codon/src/lib.rs`, `crates/dewasm-backend-codon/units/`, and `crates/dewasm-backend-codon/tests/{spec,convert,e2e,module_name}.rs`: wasm 1.0 with the shared spec harness passing on the curated set (decision 3), a WASI surface covering the microbenchmark contract (argv/env, stdout/stderr `fd_write`, `proc_exit`, `random_get`, `sched_yield`); full WASI p1, the exception-handling and tail-call lowerings, and the shared perf passes (extract/licm/fuse/selfcall) remain (issue #310 tracks the split).

## Context

Codon compiles a statically typed Python dialect through LLVM.
Decision 93 rejected it as a speed-suite engine runner because it cannot run the Python backend's unmodified output; the 2026-09-13 feasibility investigation (issue #310) measured the dedicated-backend path instead: `UInt[32]`/`UInt[64]` arithmetic is correct where the masked-unsigned convention is silently wrong (Codon's `int` is signed 64-bit), hand-ported kernels of the generated-code shape run at wasmtime-class speed, and compile time is seconds at microbenchmark size but superlinear on huge single functions.
Codon's semantics sit between the dynamic and the compiled targets, so each lowering choice below names which existing backend's shape it takes and where Codon forced a different one.

## Decision

- **Numerics: the Go shape (decision 29), spelled `UInt[32]`/`UInt[64]`/`float32`/`float`.**
  Wrapping arithmetic, unsigned division, unsigned compares and logical shifts are the stored representation's own; signed views are `Int[32]`/`Int[64]` casts; sign-extension chains through `Int[8]`/`Int[16]` casts.
  Signed `//`/`%` are floor-based in Codon, so the div/rem trap helpers (needed anyway for the zero and `INT_MIN/-1` cases) carry the floor-to-truncation adjustment.
  Type spellings are written out in full: a module-level alias (`u32 = UInt[32]`) would be one more name two `Embedded` artifacts in one namespace both define.
- **A float operation with a constant operand is wrapped in `Rt.f32_q`/`Rt.f64_q` (quiet-if-NaN).**
  Measured on Codon 0.20.1: LLVM under `-release` folds `x * 1.0`, `x / 1.0` and `x + -0.0` to `x`, which skips the signaling-NaN quieting wasm requires of every arithmetic result.
  Go's fix (`//go:noinline` on the mul/div helpers, decision 29) has no Codon equivalent, and quieting the result afterwards is correct whether or not the fold happened, at a cost only constant-operand sites pay.
  For the same class of reasons, float division is an `@llvm` `fdiv` unit (Codon's own `/` raises on a zero divisor), i64-to-f32 conversion is a direct `sitofp`/`uitofp` `@llvm` unit (through-double double-rounds, the conversions.wast rounding-direction cases), and the reinterpret/abs/neg/copysign bit paths are `@llvm` bitcasts.
- **Control flow: the Python backend's branch-register model (decision 28), without the flat state machine yet.**
  Codon has Python's statement syntax and no labeled break, so Go's labeled-loop shape is unavailable; the `_br` register with lazily opened `if _br == 0:` regions ports directly, and the measured kernels show LLVM compiles it to wasmtime-class code.
  The deep-crossing flat dispatch (decision 60's criterion) is a perf follow-up: relays are always emitted today.
- **The dynamic boundary is uniformly boxed, the Java shape (decision 30) under Codon's nominal typing.**
  A wasm function value is an `Rt.Fn` subclass with `invoke(List[Val]) -> List[Val]`; `Val` carries one field per representation (f32 its own, so boxing never re-rounds).
  An import/export value is an `Rt.Extern` with a typed field per kind, four of them for globals: resolution checks the kind, a function's structural type key (`Extern.fn_ty`), and a global's value type by construction, mirroring what Go's type assertion checks.
  Import sources are plain `Dict[str, Dict[str, Extern]]` and an instance's `exports` dict slots in directly; the duck-typed provider objects of the dynamic backends (decision 7) have no Codon equivalent, so the dict *is* the provider contract.
  Direct calls to defined functions stay native; only imports, exports, and `call_indirect` box.
- **Codon resolves nested-class field and signature annotations in definition order (measured: method bodies are lazy, annotations are not), so the runtime's scope order is a dependency order.**
  The boxing trio (`Val`/`Fn`/`Funcref`) is one unit (`rt/boxed`): inseparable, and internally ordered.
  `Extern` and the import machinery live in a final `ext` scope because `Extern`'s field annotations name `Fn`, `Global`, `Table`, `Memory` and `Tag` across every other scope.
- **Memory is a raw `Ptr[byte]` with `@llvm` `align 1` loads and stores.**
  Codon's `Ptr[T]` indexing would emit naturally-aligned access; the explicit `align 1` units make unaligned access defined and cost nothing on the target hosts.
  Allocations are memset-zeroed explicitly (Codon's GC does not clear atomic allocations).
- **The spec harness phrases each assertion as a named thunk, and spec builds carry the Go backend's recursion guard.**
  One flat module-level run of thousands of assertions would be the megafunction shape whose `-release` compile time is superlinear (measured: 30 s at 10k statements, unfinished at 394 s for 50k); thousands of small functions compile linearly.
  A native stack overflow is fatal and uncatchable, so spec builds count frame slots into a module-level `_rt_stack` and trap at the same budget as Go's (decision 29's `SPEC_STACK_LIMIT` reasoning); shipped output carries no guard, and the standalone entrypoint has no depth mitigation yet.

## Rejected alternatives

- **The masked-unsigned convention (decision 2's dynamic-backend spelling).**
  Codon's `int` is signed 64-bit: `(a * b) & M64` wraps, and division and comparison on values at or above 2^63 are silently wrong; `-numerics=py` does not change this.
  Measured in the issue #310 investigation, which is what made this a new backend rather than an engine runner.
- **A CPython-compatible output dialect.**
  `UInt[N]`, `Ptr[byte]` and `@llvm` blocks do not run under `python3`; keeping compatibility would mean giving up exactly the native numerics and memory the target exists for.
  The readable-on-CPython artifact remains the Python backend's product.
- **Per-signature unboxed `call_indirect` dispatch (Go's typed-assertion shape).**
  Codon has no `any` to assert on and no downcast that narrows, so recovering a typed callable from an erased table slot has no direct spelling; the boxed `invoke` is correct everywhere, and an unboxed same-module fast path is a measurable follow-up, not a semantic need.
- **Suppressing the identity-fold with a noinline attribute (Go's shape).**
  Codon exposes no per-function inlining control that survives `-release`, and an `@llvm` wrapper is inlined and folded the same; quieting the result is the fix that does not depend on the optimizer's behavior.

## Consequences

- Positive: the curated spec harness passes with the failure ledger matching Go's surface (import mutability/limit checks, multi-memory-downstream linking rows); every `wat/` and `c/` microbenchmark output is byte-identical to the Python backend's, and the standalone artifacts run the microbenchmark speed contract.
- Negative: imported-function calls and `call_indirect` allocate a boxed value list per call; huge single generated functions (sqlite-class, 10k+ statements) push `codon build -release` from seconds into minutes, unsettled until measured against the extraction pass (decision 81).
- Carry-over: full WASI p1 (the fd table and filesystem), the exception-handling (decision 69) and tail-call (decision 88) lowerings, the shared perf passes, the flat dispatch, and a standalone deep-recursion mitigation are tracked in issue #310's follow-ups.
