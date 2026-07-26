# ADR-29 — Go Backend Lowering Conventions

Status: **Accepted, 2026-07-26.** First milestone ("cowsay runs", ADR-24)
implemented in `crates/dewasm-backend-go/src/lib.rs` + `runtime/go/units/`.
Numeric conventions are ADR-2's; this ADR covers where Go — statically typed,
with native fixed-width integers and floats — forced a different shape from the
dynamically-typed backends (Ruby ADR-4, Python ADR-28). Go is also the first
*compiled* backend, so it is the first to use ADR-27's `run()` override.

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
- **Feature scope.** `Floats` is `Supported`; the wasm-1.0 completion set
  (imported globals/memories/tables, multiple tables, table bulk ops) is a later
  milestone and rejected at conversion time via `check_module_support`. Globals
  are plain typed fields (no `Rt::Global` box needed until imported globals
  land). WASI is the eight core syscalls (stdio + args/env + `random_get` +
  `proc_exit`); the filesystem waits for a later milestone.

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
- Carry-over for the spec milestone: `emit_*` phrasing must compile Go
  assertions and the harness generates one file per `.wast`, so each file is one
  `go build`+run — feasible given per-file compile latency, but the curated-file
  approach (as Bash/Python use) is the likely default. Float NaN-payload
  conformance, correctly-rounded f32 `sqrt` (currently double-rounded through
  float64), and int→float rounding (relies on the platform FPU's
  round-to-nearest-even, which is what wasm mandates on the targeted platforms)
  are to be validated and refined against the spec suite then.
