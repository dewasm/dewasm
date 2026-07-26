# ADR-29 — Go Backend Lowering Conventions

Status: **Accepted, 2026-07-26.** First milestone ("cowsay runs", ADR-24) and
second milestone (spec-harness green + wasm-1.0 completion, ADR-3/ADR-16)
implemented in `crates/dewasm-backend-go/src/lib.rs`, `runtime/go/units/`, and
`crates/dewasm-backend-go/tests/spec.rs`. Numeric conventions are ADR-2's; this
ADR covers where Go — statically typed, with native fixed-width integers and
floats — forced a different shape from the dynamically-typed backends (Ruby
ADR-4, Python ADR-28). Go is also the first *compiled* backend, so it is the
first to use ADR-27's `run()` override.

## Context

Go's type system and native numerics remove work the interpreted backends do by
hand, and add work they never face:

1. **Native wrapping integers.** `uint32`/`uint64` arithmetic wraps, so ADR-2's
   masked-unsigned convention is native: `i32.add` is `a + b`, not
   `(a + b) & 0xFFFFFFFF`. Signed views are `int32(x)`/`int64(x)` casts;
   sign-extensions, wraps, and integer↔float conversions are native casts with
   no runtime helper.
2. **Native IEEE floats.** `float32`/`float64` are real machine floats, so f32
   arithmetic re-rounds itself (`F32Add` is `a + b`), and float division is
   trap-free (`a / b` yields inf/NaN) — no `Rt.f32`/`Rt.fdiv` helpers, unlike
   Python/Ruby/Bash. `math.Float32bits`/`Float64bits` are bit-preserving, so
   the NaN bit paths collapse to those; only demote/promote reconstruct NaN
   payloads (a float64→float32 *conversion* of a NaN may canonicalize).
3. **Unused variables, labels, and imports are compile errors.** This is the
   Go-specific landmine (the analogue of Python's 20-block cap).
4. **Static function values.** A dynamic `invoke(name, *args)` needs reflection;
   `call_indirect` needs a way to store heterogeneous function signatures in one
   table.

## Decision

- **Types.** i32→`uint32`, i64→`uint64`, f32→`float32`, f64→`float64`, funcref
  →`*funcref`. Every value-producing site is fully typed: integer constants are
  emitted as `uint32(v)`/`uint64(v)` and float constants as
  `Rt.f32_from_bits`/`Rt.f64_from_bits` (bit-exact, so literal formatting and
  Go's compile-time constant-float rules never enter). A stack temp is named by
  both depth *and* type (`s3_i32`), since one depth can hold values of different
  types at different points.
- **Control flow maps onto Go's labeled loops** — a natural fit, so there is no
  branch register (contrast Python's `_br`, ADR-28). A referenced block/if
  becomes `L: for { … ; break L }`, a referenced loop `L: for { … ; break L }`
  with back-edges lowered to `continue L` (`BrTarget.is_loop`). Unreferenced
  structures splice inline. `br_table` is a `switch` (labeled breaks/continues
  target the enclosing loop, not the switch).
- **Unused-symbol discipline.** Labels are emitted only when `label.referenced`.
  A pre-pass over the body computes the read/used sets of locals and temps;
  declared locals and temps that are written but never read are blanked with
  `_ = x`. Functions with results get a trailing `return <zeros>` (unreachable
  code is not a Go error). Imports are computed by scanning only the runtime
  bundle (controlled code; data blobs are hex literals via `Rt.unhex`, so no
  user string can inject a false import) plus `os` for the standalone `main`.
- **Runtime shape.** Helpers are methods on a zero-size `rt` receiver
  (`var Rt rt`), named in **snake_case matching their unit id 1:1**
  (`Rt.i32_div_s`), so the units lint stays a direct name match and a unit id
  maps to its reference without case conversion — a deliberate deviation from
  Go's PascalCase convention (correctness/tooling over idiom, ADR-1). Every
  scope's bundler wrapper is empty: Go methods and types are package-level
  regardless of the struct, so the bundle is a flat list of declarations, not
  nested classes (contrast Python). Traps/exits/link errors are `panic` of
  `rtTrap`/`rtExit`/`rtLinkError`, recovered at the standalone boundary
  (trap → stderr + exit 134, mirroring Ruby/Python).
- **`call_indirect` and imports under static typing.** A table slot is a
  `*funcref{ ty string; fn any }`; `Table.call` bounds/null/type-checks and
  returns `fn`, which the call site type-asserts to the exact signature the
  `type_idx` gives (`… .(func(uint32) uint32)(a)`). Import function fields are
  typed to the wasm signature; resolution asserts the embedder's `any` value to
  that type (a mismatch is a `link_error`), with the bundled WASI method or an
  ENOSYS stub as the fallback (ADR-7). The import object is
  `map[string]map[string]any`; the provider-object/`attach` protocol and
  import *kind* checking are deferred (the type assertion enforces the
  signature). Exports are a `map[string]any`; standalone `main` calls
  `p.Exports["_start"].(func())()`.
- **Library glue is concatenated** as a `func main` after the generated
  declarations (mirroring Ruby/Bash). Go's rule that imports precede all other
  declarations means the glue cannot carry its own `import`, so library-mode
  output imports `fmt` up front (kept live with `var _ = fmt.Sprint`) and glue
  prints through it. Glue accesses the instance's unexported fields/methods
  directly (same package).
- **Execution (`run()` override, ADR-27).** Go is compiled, so the test helper
  is overridden to `go build` the file to a content-addressed cache binary
  (identical sources — cowsay's args and stdin cases — build once) and run the
  binary. Running the binary, not `go run`, is required: `go run` prints
  "exit status N" and itself exits 1 rather than propagating the guest exit
  code, which the WASI args/env case asserts exactly. `$DEWASM_GO` overrides the
  toolchain; a missing one fails loud (ADR-15).
- **Feature scope.** `Floats` is `Supported`. WASI is the eight core syscalls
  (stdio + args/env + `random_get` + `proc_exit`); the filesystem waits for a
  later milestone.

## Second milestone: spec harness + wasm-1.0 completion

- **Boxed globals are a generic `*global[T]`** (`runtime/go/units/global/`), the
  Go analogue of Ruby's `Rt::Global` box (ADR-16): every global is a
  pointer-shared cell so one that crosses an instantiation boundary is shared,
  not copied. Generics keep it statically typed — `p.g0.value` needs no
  assertion, and an imported global resolves by asserting `v.(*global[uint32])`.
- **Imports beyond functions** resolve through the same provider map, with the
  ADR-16 kind check performed *by the Go type assertion itself*: `v.(*Memory)` /
  `v.(*Table)` / `v.(*global[T])` (and the existing func-signature assertion)
  reject a wrong-kind — and, for funcs and globals, wrong-*type* — import as a
  `link_error`. A missing non-WASI import is a `link_error` at instantiation
  (ADR-0), not a deferred call-time stub. **Consequence for the ledger:** Go's
  `import-limits` gap is *narrower* than Ruby/Python's kind-only check — only
  global mutability and table/memory min/max limits stay unchecked — so its spec
  `EXPECTED_FAILURES` counts are lower (the two `linking` failures are both
  global-mutability mismatches). Exports are a `map[string]any` over every kind
  (funcs, the global box, table, memory), so a generated instance's `Exports`
  doubles as another module's import object — what powers the harness's
  `register` support.
- **The spec harness compiles Go** (ADR-3). Each `.wast` file becomes one
  `package main` program: the runtime bundle, harness helpers, every module's
  package-level declarations, then a `func main` of instantiations and
  assertions. Because Go has no dynamic dispatch, each generated type carries a
  reflective `invoke(name, args...) []any` / `globalGet(name) []any` built where
  the export signatures are known; the harness asserts the boxed results
  bit-exactly (`math.Float32bits`). Module declarations cannot sit inside
  `main`, so they are accumulated at package scope and prepended at assembly.
- **Exhaustion is a generation-time recursion guard** (spec build only). A
  runaway recursion overflows Go's goroutine stack *fatally* — the runtime
  aborts the process uncatchably, and the harness needs the script to continue
  after the check. Of the alternatives — lowering `debug.SetMaxStack` (still
  process-fatal), or a subprocess per exhaustion assertion — the chosen one
  instruments each generated function to add its frame's slot count
  (`1 + params + locals + temps`) to a global `rtStack`, `defer`-decrement it,
  and trap `"call stack exhausted"` past a fixed budget (1024). It is
  deterministic, needs no extra process, and — sized by the slot *cost*, not raw
  depth — also trips on `skip-stack-guard-page.wast`'s `function-with-many-locals`
  (1056 locals, the only >50-slot function in the suite) even at shallow depth,
  so that file passes with no ledger entry. The guard is **off** for shipped
  standalone/library output, whose deep-but-valid recursions must not falsely
  trap.
- **Two Go-compiler float hazards** the spec suite exposed (ADR-2):
  `float_exprs.wast` requires *no contraction* (each op rounds independently),
  but Go fuses `x*y+z` into a hardware FMA on arm64; and it folds `x*1.0`→`x` /
  `x/1.0`→`x`, skipping the signaling-NaN quieting the `no_fold_*` tests demand.
  Both are defeated by routing f32/f64 **mul and div through `//go:noinline`
  runtime helpers**, so a constant operand never reaches the op and no add can
  fuse across the call boundary. Separately, min/max NaN results must be the
  *wasm* canonical NaN — Go's `math.NaN()` is not bit-canonical (its low
  mantissa bit is set), so the units build the canonical pattern explicitly.
  f32 `sqrt` via `float32(math.Sqrt(float64(x)))` was *validated* correctly
  rounded against `f32.wast` (double-rounding through float64 is safe for sqrt).

## Rejected alternatives

- **A per-function branch register (Python's `_br`, ADR-28).** Unnecessary: Go
  has labeled `break`/`continue`, which express block exits and loop back-edges
  directly.
- **PascalCase runtime method names with a snake_case-converting lint.** The
  conversion is a bug surface for no functional gain (`go build` ignores case);
  matching unit ids verbatim keeps the lint trivial.
- **Reflection-based `invoke`.** Storing exports as `map[string]any` and
  type-asserting at the (test-authored) glue site avoids pulling in `reflect`
  and its cost.
- **Blanket `_ = x` for every local/temp.** Correct but bloats cowsay's already
  large generated file and its compile time; the read/used pre-pass blanks only
  the genuinely write-only ones.

## Consequences

- Positive: cowsay is byte-identical to the wasmtime golden for both the args
  and stdin cases; the standalone, library, and WASI stdio/args-env suites pass.
  Native integers/floats make the generated arithmetic and the runtime smaller
  than the interpreted backends'. Cold `go build` of cowsay's ~170k-line file is
  a few seconds; a warm content-cache hit runs in well under 0.1 s.
- Negative: library-mode output always imports `fmt`; the generated file is
  verbose (fully-parenthesised, fully-cast expressions), per ADR-1.
- Spec milestone (done): the curated default gate is green; the full
  `DEWASM_SPEC_ALL=1` sweep is green at pass=29235 fail=38 (every failure in the
  attributed `import-limits`/`linking` ledger), slightly *ahead* of Ruby/Python
  (29233/40) because the Go type assertion catches two unlinkable cases their
  kind-only import check misses. Per-file `go build` dominates the sweep, but a
  content-addressed binary cache (shared with e2e) keeps a warm re-run to well
  under a minute. The flagged bring-up gaps were resolved, not deferred:
  NaN-payload conformance (min/max canonical NaN; `no_fold_*` sNaN quieting via
  the noinline mul/div helpers), no-contraction FMA, and f32 `sqrt` (validated
  correctly rounded) all pass.
