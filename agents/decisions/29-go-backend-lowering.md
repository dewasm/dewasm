# Decision 29: Go Backend Lowering Conventions

Status: **Accepted, 2026-07-26.**
Implemented in `crates/dewasm-backend-go/src/lib.rs` and `runtime/go/units/`.
The tests are in `crates/dewasm-backend-go/tests/{spec,e2e}.rs`.
It covers wasm 1.0 with the specification harness passing ([decision 3](3-testing-strategy.md), [decision 16](16-ruby-wasm1-completion.md)).
It also covers full WASI Preview 1 including the file system, adopting [decision 14](14-ruby-wasi-file-system.md)'s model.
Numeric conventions are [decision 2](2-numeric-semantics.md)'s.
Go is statically typed, with native fixed-width integers and floats.
This decision covers where that forced a different shape from the dynamically-typed backends.
Those are Ruby ([decision 4](4-ruby-backend-lowering.md)) and Python ([decision 28](28-python-backend-lowering.md)).
Go is the first *compiled* backend, so it is the first to use [decision 27](27-test-helper-crate.md)'s `run()` override.

## Context

Go's native numerics remove work the interpreted backends do by hand.
Integer arithmetic wraps, so decision 2's masked-unsigned convention is free.
Sign-extensions, wraps, and conversions are casts.
Floats are machine floats, so f32 re-rounds itself, and division is trap-free.
Only `demote`/`promote` reconstruct NaN payloads.
Go adds two problems they never face:

- **Unused variables, labels, and imports are compile errors.**
  That is the Go-specific danger, like Python's 20-block cap.
- Function values are **statically typed**, so a dynamic `invoke` would need reflection.
  `call_indirect` also needs one table holding mixed signatures.

## Decision

- **Types.**
  i32/i64/f32/f64 map to `uint32`/`uint64`/`float32`/`float64`, and `funcref` maps to `*funcref`.
  Every value-producing site is typed.
  Float constants are emitted as `Rt.f32_from_bits`/`Rt.f64_from_bits`.
  That keeps literal formatting and Go's constant-float rules out of the picture.
  A stack temp is named by depth *and* type (`s3_i32`).
  That is because one depth holds different types at different points.
- **Control flow maps onto Go's labeled loops**, so there is no branch register (contrast Python's `_br`).
  A referenced block/if or loop is `L: for { … ; break L }` with back-edges as `continue L`.
  Unreferenced structures splice inline.
  `br_table` is a `switch` whose labeled breaks target the outer loop, not the switch.
  A pre-pass drops what cannot execute before emission:
  - sequence tails after a statement that ends unreachable;
  - the closing `break L`/default `return` after a body that always exits;
  - catch clauses after a tag-less one;
  - self-assigning local moves.

  The pre-pass also drops labels that no surviving branch targets.
  That is because `go vet` rejects unreachable statements in consumers of the generated source.
- **Unused-symbol discipline.**
  Labels are emitted only when referenced.
  A pre-pass blanks write-only locals and temps with `_ = x`.
  The import set is computed by scanning the runtime bundle alone.
  That works since generated code emits no package-qualified selectors.
  Data is written as hexadecimal literals.
  The scan strips line comments, so comment text cannot pull in a package.
  It never rewrites output.
  That is why `//go:embed` needs the blank import [decision 37](37-data-segments-in-a-file.md) adds.
- **Runtime shape.**
  Helpers are methods on a zero-size `rt` receiver.
  They are named in **`snake_case` matching their unit identifier 1:1** (`Rt.i32_div_s`).
  That is an intended break with Go's PascalCase convention.
  So a unit identifier maps to its reference without case conversion.
  The units lint then stays a direct name match.
  Correctness and tools come before Go's usual naming ([decision 1](1-ir-design.md)).
  All bundler scope wrappers are empty.
  Go methods and types are package-level whatever `struct` they belong to.
  So the bundle is a flat declaration list.
  Traps, exits, and link errors are `panic` of `rtTrap`/`rtExit`/`rtLinkError`.
  They are recovered at the standalone boundary (trap to standard error, exit 134).
- **Static typing is the import check.**
  Import fields are typed to the wasm signature.
  A table slot is a `*funcref{ ty string; fn any }`.
  So the type assertion at each site performs decision 16's kind check.
  It also catches a wrong *type* for functions and globals, rejecting a bad import as a `link_error`.
  A missing import other than WASI is a `link_error` at instantiation ([decision 0](0-foundation.md)).
  A WASI import falls back to the bundled method or an ENOSYS stub ([decision 7](7-import-providers.md)).
  Globals are a generic `*global[T]`, the Go equivalent of Ruby's `Rt::Global`.
  A global is shared across an instantiation boundary, while `p.g0.value` still needs no assertion.
  `Exports` is a `map[string]any` over every kind.
  So one instance's exports serve as another module's import object, which powers `register`.
- **An import source is a value, not only a map.**
  It is either a name-to-value `map[string]any` or an `ImportProvider` (`WasmImport(name string) any`).
  An `ImportProvider` stands in for the module.
  It is optionally also an `ImportAttacher` (`Attach(instance any)`), called once the instance is built.
  That is the Go spelling of Ruby's `attach`, which any object may define.
  It is also the only way a provider that is a `struct` reaches the instance's memory.
  Otherwise the embedder would have to set up a back-reference.
  The bundled WASI is built on first *fallback*, not in the constructor and not on first call.
  That matches Ruby's `@wasi ||=`.
  So `p.wasi == nil` is the honest observable for a provider covering every WASI import.
- **Packaging follows the mode** ([decision 63](63-module-name-policy.md)).
  Standalone is `package main` with the fixed type `Program`.
  Library output is a package an embedder imports.
  It is `package <name lowercased>` with type `<name capitalized>`.
  The name is validated as a Go identifier at conversion time.
  Host code in that package cannot carry its own `import`, since Go requires imports first.
  So library output imports `fmt` up front and keeps it live with `var _ = fmt.Sprint`.
- **Execution (`run()` override).**
  The helper `go build`s into a content-addressed cache binary and runs the binary rather than `go run`.
  `go run` prints "exit status N" and exits 1.
  It does not pass on the guest exit code that the WASI arguments/environment case asserts.
  `$DEWASM_GO` overrides the toolchain; a missing one fails loud ([decision 15](15-tests-fail-not-skip.md)).
- **The specification harness compiles Go**: one `package main` program per `.wast` file.
  Module declarations are accumulated at package scope, since they cannot sit inside `main`.
  Go has no dynamic dispatch.
  So each generated type carries a by-name `invoke(name, args...) []any` / `globalGet(name) []any`.
  They are built where the export signatures are known.
- **Exhaustion is a generation-time recursion guard, in specification builds only.**
  That is because a recursion that never stops overflows Go's goroutine stack *fatally*.
  The harness must continue past the check.
  Each function adds its frame's slot count to a global `rtStack` and `defer`-decrements it.
  It traps `"call stack exhausted"` past 1024 slots.
  Sizing by slot cost rather than depth also trips `skip-stack-guard-page.wast`'s 1056-local function.
  It trips at shallow depth, so that file needs no list entry.
  The guard is **off** in shipped output, whose deep but valid recursions must not falsely trap.
- **Two Go-compiler float problems** the specification suite exposed, each with its own fix.
  Go fuses `x*y+z` into an arm64 FMA, which `float_exprs.wast` does not allow.
  So every f32/f64 `mul` and `div` is emitted **inside an explicit conversion**, `float64(a * b)`.
  The Go specification defines that as rounding to the target type's precision.
  The compiler realizes it as a `Round32F`/`Round64F` value the fusion rewrite does not match through.
  Go also rewrites `x * 1.0` and `x / 1.0` to `x` and `x * -1.0` to `-x`.
  That skips the quieting of a signaling NaN that `no_fold_*` demands.
  The conversion does not stop that: `float64` of a value that is already `float64` does nothing.
  The compiler also folds the `from_bits` call to a machine constant the rewrite matches on.
  So a `mul` or `div` **with a constant operand** is wrapped in `Rt.f32_q`/`Rt.f64_q`.
  That is [decision 94](94-codon-backend-lowering.md)'s helper that quiets a NaN.
  It is correct whether or not the rewrite fired, and it is emitted at no other site.
  `math.NaN()` also lacks the canonical bits, so `min`/`max` build wasm's pattern explicitly.
  f32 `sqrt` through float64 was validated correctly rounded against `f32.wast`.
- **Feature scope**: wasm 1.0 and full WASI Preview 1; `Floats` is `Supported`.
- **Memory units are shaped for Go's inline budget in large functions.**
  Every large wasm function becomes a Go function over 5000 AST nodes.
  Go inlines into such a function only a callee costing at most 20 (`inlineBigFunctionMaxCost`).
  The original units cost 29 to 32.
  So sqlite3's 13,910-line VDBE function paid an out-of-line call per memory access.
  It has 4,216 of them.
  It spent half its run time in them.
  The eight units are `i32_load`, `i32_load8_u`, `i32_load16_u`, `i64_load` and the four stores.
  Each does three things, for a cost of 16 to 18:
  - it checks the address against a length mirrored into a field;
  - it reads through the mirrored base pointer once in host byte order;
  - it raises the trap as a value built in advance.

  A unit that calls another unit costs 24 to 27.
  So the sign extensions, the i64 narrow forms, and the float reinterpretations are casts.
  The emitter spells them around the raw accessor.
  The units lint asserts the budget.
  So a Go release that changes the cost model fails loudly rather than silently losing the inlining.
  Host byte order restricts the output to little-endian targets.
  Those are every `GOARCH` but `mips`, `mips64`, `ppc64` and `s390x`.
  The memory declarations at the top carry a compile-time assertion on `runtime.GOARCH`.
  So a big-endian build fails instead of computing wrong values.
  Measured on `app/sqlite3_query` (Darwin arm64, output identical to Wasmtime's):
  - The shipped units take the run from 0.268 s to 0.153 s of CPU time (minimum of nine).
    Wasmtime takes 0.124 s.
    That run was on a loaded machine, so the figure is CPU time rather than wall time.
  - A first version of the same shape went from 0.167 s to 0.092 s of wall time, with no load.
    Wasmtime takes 0.080 s.

### WASI: where Go's standard library forced a different shape

Three parts of decision 14 are mirrored into `runtime/go/units/wasi/`.
They are its file descriptor table model, preopen sandboxing, and intended ENOSYS gaps.
That is the unit set Ruby and Python carry.
Go-specific:

- The file descriptor table is `map[uint32]any` holding an `*os.File` or a `*wasiDir`.
  So every system call that takes a file descriptor asserts the type.
  Special cases for the standard streams key on pointer identity with `os.Stdin`/`Stdout`/`Stderr`.
  `fd_datasync` falls back to a full `Sync`, since Go exposes no portable `fdatasync`.
- Preopens are the constructor's fourth parameter, since Go has no keyword arguments.
  File descriptors are assigned in sorted key order, so map iteration order does not change results.
- The runtime stays one build-tag-free `.go` file.
  So `stat` fields named differently on Darwin and Linux are reached without build tags.
  [Decision 40](40-wasi-p1-completion.md) records this.
- `resolve_path` derives the final component from the raw guest string.
  It does not use `filepath.Base(filepath.Join(base, rel))`.
  `Join` *Cleans*, folding a trailing `.` or `..` away, so `Base` returns the parent's own name.
  Then the AT_SYMLINK_NOFOLLOW branch would wrongly resolve it and reject it with ERRNO_NOTCAPABLE.
  Taking the text after the final `/` restores what Python's non-cleaning join gives for free.
- Library-mode WASI output always seeds `rt/exit`.
  Host glue catches `*rtExit` for the exit code, and Go asserts the concrete type at compile time.
  So it must exist even for a fixture that never imports `proc_exit`.

## Rejected alternatives

- **A per-function branch register** (Python's `_br`): unnecessary.
  Labeled `break`/`continue` express block exits and loop back-edges directly.
- **PascalCase runtime method names with a converting lint**: a bug surface for no gain.
  That is because `go build` ignores case.
- **Reflection-based `invoke`.**
  `map[string]any` exports plus a type assertion at the glue site avoid pulling in `reflect` and its cost.
- **`_ = x` for every local and temp**: correct.
  It grows `cowsay`'s already large file and its compile time, though.
- **Lowering `debug.SetMaxStack`, or a child process per exhaustion assertion.**
  The former still ends the process; the latter costs a process per check.
- **`//go:noinline` helpers for `mul` and `div`**, the shape until 2026-09-21.
  One call defeats both problems at once.
  A constant operand never reaches the operation, and no add fuses across the call boundary.
  But a call that cannot be inlined per multiply is the whole cost of a float loop.
  Measured on Darwin arm64 against Wasmtime at the same iteration count, minimum of three:

  | Case | Helper shape | Conversion shape |
  | --- | --- | --- |
  | `wat/f32_alu` | 3.59x | 0.97x |
  | `wat/f64_alu` | 3.73x | 1.04x |
  | `c/mandelbrot` | 2.43x | 0.97x |

  The call bought something the conversion does not.
  That is a constant the optimizer carries into the operation from outside the expression.
  A local first assigned `f64.const 1.0` and then multiplied folds the same way.
  The operand check does not see it, so such a multiply leaves a signaling NaN signaling.
  The specification suite writes the constant at the operator, where the check does see it.
  Decision 94's Codon wrapper has the same shape and the same gap.

- **Dropping the memory bounds check**, which is what `goccy/wasm2go` does.
  That is how it reaches 0.064 s on the same workload.
  A check-free access is 3 instructions against 6.
  But an out-of-bounds guest access then reads or writes the host heap.
  `memory_trap.wast` binds ([decision 3](3-testing-strategy.md)).
  The remaining gap to a check-free converter is the price of keeping the trap.
  A guard-page scheme could remove the check while keeping the trap.
  It would be a reserved mapping with `debug.SetPanicOnFault`.
  That is runtime engineering with a per-platform mapping layer, though, not a unit change.
- **Hoisting the base pointer into a function local**, refreshed after every call and `memory.grow`.
  That is what `goccy/wasm2go` does.
  It measured at 3 ms on `app/sqlite3_query` once the units inline, not worth the emitter change.
- **`binary.LittleEndian` over a `(*[N]byte)` or `unsafe.Slice` view of the base pointer.**
  It is portable to any byte order, at a cost of 18 to 19.
  It is one instruction when inlined into a small function.
  Inside a large function, the `binary` method is a second inline decision under the same budget of 20.
  There it stays a call.
  The artifact measured no faster than the original units on `app/sqlite3_query` under load.
  It took 0.32 s, against 0.29 s for the original and 0.15 s for the direct read.

## Consequences

- `cowsay` output is identical to the Wasmtime snapshot.
  Other cases match the same snapshots the Ruby/Python cases use.
  They are the WASI `Fs` suite, `gzip_e2e!`, and the file system app cases.
  `gzip_e2e!` checks byte-level standard input/output through compiled output.
  Native integers and floats keep the generated arithmetic smaller than the interpreted backends'.
  They keep the runtime smaller too.
  The cost is long source with casts everywhere (decision 1), always importing `fmt` in library mode.
- **Go's import-limits gap is narrower than Ruby's and Java's.**
  That is because the type assertion rejects wrong function signatures and wrong global value types.
  A kind-only check misses those.
  Only global mutability and table/memory limits stay unchecked.
  Its `EXPECTED_FAILURES` list is correspondingly shorter (`linking` 2 against their 4).
  Every failure on a full run is in the attributed `import-limits`/`linking` list.
- Cold `go build` of `cowsay`'s roughly 170k-line file takes a few seconds.
  A warm cache hit takes well under 0.1 s.
  Per-file `go build` dominates a full specification run.
  The cache (shared with e2e) keeps that run under a minute when warm.
