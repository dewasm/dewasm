# Decision 91: Perl Inline Fast Paths for Float Arithmetic and Memory Access

Status: **Accepted, 2026-09-11.**
Implemented in `crates/dewasm-backend-perl/src/lib.rs` (`float_fast`, `load_expr`, `store_stmt`); the runtime helpers and memory units stay unchanged as the fallback and as the WASI/API surface, and the `float_shape`/`memory_shape` tests pin the emitted forms.
Narrows two shapes of [decision 55](55-perl-backend-lowering.md); everything else there stands.

## Context

Decision 55 lowered every f64/f32 add/sub/mul to an `Rt::` helper call and every load/store to an `Rt::Memory` method call, and accepted the cost.
The 2026-08-31 speed record (`records/2026-08-31T09-49-13Z-speed.json`) showed where that cost lands: on inline-lowered integer paths dewasm-perl sits on the CPython-level language floor (`wat/i32_alu` 1.2x dewasm-python), but it is 5.5x on `wat/f64_alu`, 15x on `c/mandelbrot`, and 1.6-3x on the memory and call microbenchmarks, exactly where each operation pays one or two sub calls.
Measured per-operation costs (perl 5.44, M1 Pro): `Rt::fadd(a, b)` 206 ns against 112 ns for the same pack round-trip inlined; `Rt::f32(Rt::fadd(a, b))` 350 ns against 188 ns inlined with a fallback; `$self->{memory}->i32_load(a)` (accessor plus `check`, two method calls) 296 ns against roughly 110 ns for an inline `unpack`/`substr` with an inline bounds test.

## Decision

*A helper whose body is one native operation plus a cheap correction is emitted inline, with the helper kept and called only for the results the inline form cannot finish; a helper whose body is control flow or software bit manipulation stays a call.*

- f64 add/sub/mul: `(($__ft = unpack('d<', pack('d<', A + B))) != 0.0 ? $__ft + 0 : Rt::fadd(A, B))`.
  Only an exact-zero result needs the helper's operand-sign fix; NaN and infinity stay inline because `pack 'd'` is a byte copy.
- f32 add/sub/mul: the same with `pack 'f'` and the guard `!= 0.0 && $__ft - $__ft == 0`, which is false exactly for NaN and infinity, so zero signs, NaN payloads, and the pack-'f' overflow clamp all stay in the unchanged `Rt::f32(Rt::fadd(...))` chain.
- The fast branch yields `$__ft + 0`, never the bare lexical: perl aliases `@_` elements to the yielded scalar until the callee copies them, so two fast-path ternaries as sibling call arguments would both alias `$__ft` and the later write would clobber the earlier argument (caught by `float_exprs.wast`); `+ 0` forces a fresh scalar and is value-exact for everything the branch can yield.
- A leaf operand (a constant, local, temp, or global) is duplicated into the fallback; a non-leaf operand is bound in place to a height-indexed lexical (`$__fa{h}`/`$__fb{h}`, height = one above everything the node contains), never duplicated, so nested float chains cannot grow the output exponentially and a binding survives the second operand's own bindings.
  Bindings nest inside the arithmetic, never behind the comma operator, which would flatten in the list context of a call's argument list.
- Loads and stores: inline `unpack`/4-arg `substr`/`vec` on `$$__mem` (a per-function ref to the memory byte string; `grow` appends in place, so the ref stays valid) behind an inline bounds test, trapping with the unchanged message.
  A store evaluates its address, then its value, then the bounds test, because wasm traps a faulting value computation before the store's own bounds check; a non-leaf value is therefore bound to `$__mv` before the test.
  The store address lexical (`$__msa`) is distinct from the load one (`$__ma`): a non-leaf store value may contain loads that reuse `$__ma` after the store address was bound.
- Kept as calls: float div (perl dies on `x / 0.0`), min/max/copysign/sqrt/ceil/floor/trunc/nearest, conversions, narrow-load sign extension (`Rt::sext` wraps the inline unsigned read), the f32 load/store bit paths (`Rt::f32_from_bits`/`f32_bits`), memory size/grow/copy/fill/init, and the WASI units.

## Rejected alternatives

- **A `do { my (...) = ...; ... }` block per operation.**
  Measured 179 ns against the 206 ns call: scope entry eats most of the win.
- **Duplicating non-leaf operands into the fallback instead of binding.**
  Output size doubles per nesting level, and left-leaning float chains (LLVM reduction idioms) are arbitrarily deep.
- **Inlining the zero-sign and boundary logic too.**
  The fallback fires only on results the guards catch; inlining its bit tests would cost every operation for the rare case, and the helpers already exist.
- **Folding `check` into each memory accessor instead of full inlining.**
  Saves one of the two method calls, measured roughly 200 ns, still about 2x the inline form.

## Consequences

- Positive: measured on perl 5.44 / M1 Pro (`/usr/bin/time -p`, byte-identical stdout pre/post): `wat/f64_alu` 1.66 s to 1.14 s and `wat/f32_alu` 3.08 s to 1.71 s at 2M iterations; `wat/mem_rw` 1.9x and `wat/mem_narrow` 1.9x; `c/mandelbrot` 56 to 37 us per iteration (1.5x) and `c/sha256` 320 to 227 us per iteration (1.4x).
  The full spec harness, the wasi-testsuite run, and the app e2e cases pass unchanged.
- Negative: generated float and memory expressions are longer and harder to read ([decision 3](3-testing-strategy.md): correctness of generated code outranks its readability), and the emitter carries per-function scratch-lexical bookkeeping (`$__ft`, `$__fa{h}`/`$__fb{h}`, `$__mem`, `$__ma`, `$__msa`, `$__mv`).
- Carry-over: every call still pays the depth counter ([decision 55](55-perl-backend-lowering.md)), measured about 48 ns; a call-graph-based exemption for functions that cannot recurse is a possible follow-up.
