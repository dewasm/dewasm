# Decision 106: Spinel Backend, the Ruby Output with a Signed-Word i64

Status: **Accepted, 2026-11-21.**
Implemented in these files:
- the `I64Repr` dialect of [`crates/dewasm-backend-ruby/src/lib.rs`](../../crates/dewasm-backend-ruby/src/lib.rs);
- [`crates/dewasm-backend-spinel/src/lib.rs`](../../crates/dewasm-backend-spinel/src/lib.rs) and its `units/`;
- `crates/dewasm-backend-spinel/tests/{spec,e2e,convert}.rs`.

The shared specification harness passes for the backend.
Three items remain:
- the WASI conformance harness (decision 36) has no Spinel side yet;
- the library-mode e2e cases are not called;
- `Embedded` isolation (decision 62) has no check, since `embedded_coexist_e2e!` is one of those.

## Context

Spinel compiles Ruby ahead of time, and its `Integer` is one 64-bit machine word.
`spinel --int-overflow=MODE` says what an overflow does: `raise`, `wrap`, or `promote`.
The Ruby backend holds an i64 as `0..=2**64 - 1` (decision 2), which one word cannot hold.
So its output runs under `promote` only, where an overflow gives an arbitrary-precision integer.
The speed suite's `dewasm-spinel` row measured that.

`promote` costs more than the i64 values it exists for.
Under it every integer operation carries an overflow check, i32 ones included.
Two measurements on the microbenchmarks showed the size of it (arm64 macOS, 2026-11-21):

| Measurement | Result |
| --- | --- |
| `wardite` built by Spinel, `wat/i64_alu` | 84 ns for one iteration, against 895 ns for `dewasm-spinel` |
| the Ruby output built with `wrap`, i32 workloads | 2 to 12 times faster than with `promote` |

`wardite` is an interpreter, and it won because it holds an i64 as a signed word.
The second row is not a usable build.
The Ruby runtime computes `2**64` and compares against `2**63`, which a wrapping word gets wrong.

The `spinel-aot` experiment (`agents/experiments.md`, 2026-08-13) had found the same direction.
It stopped on Spinel's compile time, which has since come down.

## Decision

A `spinel` target emits the Ruby backend's output with an i64 as a signed 64-bit word.
The output is built with `--int-overflow=wrap`.

- **The representation is a parameter of the Ruby emitter, not a second emitter.**
  `I64Repr::SignedWord` changes the i64 lowering and nothing else.
  The control flow, the memory, the i32 lowering, and the WASI runtime stay the Ruby backend's.
  The criterion: the two hosts differ in integer width only, so only that is stated twice.
- **An i64 `add`, `sub`, `mul`, `shl` and `shr_s` is the host's operator, with no mask.**
  The host's wrap is the wasm result.
- **An unsigned reading has its own lowering.**
  A compare flips the sign bit of both sides and compares signed.
  `shr_u` shifts and then clears the copied sign bits; a constant count gives the mask as a literal.
  `div_u`, `rem_u`, and the unsigned conversions to a float are runtime units.
- **An i32 stays masked-unsigned, and the mask analysis (decisions 71 and 73) stays in use.**
  An unmasked i32 value is read only modulo 2^32, or is proved inside `[0, 2**32)`.
  A value that wraps at 64 bits keeps its class modulo 2^32, so both cases hold.
- **Every integer literal is the signed word congruent to its value modulo 2^64.**
  That covers the target of a rewritten equality (decision 77).
  The analysis proves one candidate modulo the type's width in the raw interval.
  2^64 is a multiple of that width, so no second value there is congruent modulo 2^64.
- **The runtime units that read or build a 64-bit pattern are replaced, by name.**
  The backend's bundler is the Ruby units with `crates/dewasm-backend-spinel/units/` on top.
  `M64` is `-1`, the word with every bit set, so a shared unit that masks with it still works.
  A float is tested against the integer range before it is truncated.
  A truncation outside 64 bits has no word to land in.
- **An f64 `add`, `mul` or `div` with a constant operand quiets its own NaN result.**
  Spinel's C compiler folds `x * 1.0` and `x / 1.0` to `x` at `-O 2`, and `x * -1.0` to a sign flip.
  None of those quiets a signaling NaN, and `float_exprs.wast` tests exactly that.
  The form is the one the Ruby backend's f64 `sub` already has: `(r = x * 1.0) == r ? r : quiet_nan(r)`.
  Only a site with a literal operand pays for it, which is Codon's criterion (decision 94).
  The dialect states it as its own field, since it is a fact of the compiler, not of the i64.
- **The runtime checks the wrap when it loads.**
  `0x7fff_ffff_ffff_ffff + 1 == I64_MIN` is false under `ruby` and under `promote`.
  It raises under `raise`.
  So a build with the wrong mode stops at once, and never computes a wrong value.
- **The tests build with `--no-bigint`.**
  Under it a value that needs an arbitrary-precision integer is a compile error.
  So no such value is left in a unit or in generated code.
- **Spinel is a tool of `mise.toml`, at one fixed commit, built from its source archive.**
  The last release is older than the fixes the backend needs.
  This revises decision 101, which kept Spinel outside the file.
- **The speed suite's `dewasm-spinel` and `wasm3-spinel` rows take this backend.**
  The Ruby output under `promote` is no longer measured.

## Rejected alternatives

- **Keep the Ruby output and build it with `wrap`.**
  It passed all 18 microbenchmarks, and it is still wrong.
  `rt/s64` subtracts `2**64`, and the float bit paths compare against `2**63`.
  No microbenchmark reached a value where that shows.
- **A copy of the Ruby backend as its own emitter, as Codon is a copy of Python (decision 94).**
  Codon's dialect differs in every statement: types, memory, and the import boundary.
  Here about 30 lines of lowering differ.
  A copy would double 3,400 lines to state them.
- **A signed-word i64 in the Ruby backend itself.**
  CRuby's integers do not wrap, so every `add` would need a mask and a sign fix.
  That is more work for each operation than the mask it has now.
- **An i32 as a signed 32-bit value too.**
  Spinel has no 32-bit integer type, so nothing wraps at 32 bits.
  The masked-unsigned i32 already measures at Wasmtime's speed on `wat/i32_alu`.
- **Spinel from `$DEWASM_SPINEL` only, as before.**
  CI would then have no Spinel, and the backend no specification run.

## Consequences

- Positive: the 18 microbenchmarks give Wasmtime's output, and run faster than the row before.
  The geometric mean is 4.9 times, `wat/i64_alu` is 378 times, and `wat/i32_alu` is 12.7 times.
  `wat/i64_alu` and `wat/i32_alu` now run at Wasmtime's speed.
- Positive: the whole specification testsuite runs for the backend in about one minute.
  Each file is one `spinel` build at `-O 0`.
  At `-O 2` the same run passes too, except the two assertions of `call.wast` below.
- Negative: the output runs on Spinel only, with one build flag.
  The load-time check turns a wrong build into an error.
- Negative: an i64 crosses the library boundary as a signed word.
  The Ruby backend gives the same bits as a value in `0..=2**64 - 1`.
- Negative: a `poll_oneoff` clock timeout of `2**63` nanoseconds or more reads as negative.
  The shared unit then treats it as a time in the past.
- Negative: at `-O 2` a recursion with no end can run forever where wasm traps.
  The C compiler turns `(func $f (call $f))` into a loop, so the stack never fills.
  `call.wast` asserts the trap twice; the tests build at `-O 0`, where it happens.
  Go and Codon guard recursion in specification builds only, so their shipped output differs too.
- Negative: `wat/tail_call` measured 0.8 times the row before, and the cause is not known.
- Carry-over: the fixed commit moves by hand.
  Spinel changes its behavior between commits, so each move runs the specification testsuite.
