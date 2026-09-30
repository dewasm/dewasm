# Decision 84: Round to f32 by Veltkamp Splitting, with the Pack Path as Fallback

Status: **Accepted, 2026-08-22.**
Landed in [`runtime/ruby/units/rt/f32.rb`](../../runtime/ruby/units/rt/f32.rb) and [`runtime/python/units/rt/f32.py`](../../runtime/python/units/rt/f32.py).
A magnitude in [2^-126, 2^127) is rounded by three float operations.
Everything else keeps the `pack`/`unpack` path unchanged.
[`runtime/perl/units/rt/f32.pl`](../../runtime/perl/units/rt/f32.pl) keeps the pack path alone.
Perl needs two extra multiplications to reach the same result, and measures slower with them.

## Context

Every f32 operation is computed in double precision.
It is re-rounded once through `Rt.f32` ([decision 2](2-numeric-semantics.md)).
So that unit sits on the hot path of every f32-heavy program.
It costs one `pack` plus one `unpack` per arithmetic result.
Each is a C call that allocates a String (Ruby) or a `bytes` and a tuple (Python).

The cost is the gap between the two arithmetic microbenchmarks.
They differ only in operand width (`benchmarks/wat/f32_alu.wat` against `benchmarks/wat/f64_alu.wat`).
In `records/2026-08-22T06-26-23Z-speed.json`:

| runner | wat/f64_alu | wat/f32_alu |
| --- | --- | --- |
| dewasm-ruby | 102 ns/op | 677 ns/op |
| dewasm-python | 152 ns/op | 849 ns/op |
| dewasm-perl | 831 ns/op | 1547 ns/op |

Replacing the byte round trip with a reusable `IO::Buffer` scratch was tried and rejected.
The record is #261 and PR #263 (closed unmerged), and `agents/experiments.md`, float-bits-scratch.
A scratch buffer is state.
It became one per receiver after the module-level placement corrupted floats across threads.
The instance-variable read plus the `IO::Buffer` call pair then cost more than the `pack` pair.
That was measured on a real app.

## Decision

Round by arithmetic on the range where arithmetic is provably the same rounding.
Keep the existing conversion as the fallback for every other input.

For `x` with 2^-126 <= |x| < 2^127:

```
t = x * 536870913.0      # 2^29 + 1
r = t - (t - x)
```

This is Veltkamp splitting.
It returns `x` with its significand rounded to 24 bits, to nearest, ties to even.
That is exactly the single rounding the f32 convention asks for.
It is exact because a double carries 53 >= 2 * 24 + 2 bits.
Inside that range the result is always a normal f32 and no intermediate leaves the double range.

The fallback keeps every other input on `pack`, with the same behavior as before.
Those inputs are:
- Subnormal magnitudes below 2^-126.
  They are rounded at a fixed exponent, not to 24 significant bits.
- Magnitudes at or above 2^127.
  The 24-bit result can be 2^128, which is not an f32.
  So the overflow boundary at 2^128 - 2^103 decides.
- Infinities.
- NaN.
  It fails both comparisons, so no explicit test is needed.
  `pack` canonicalizes it as before.

Zero is the one input that takes neither path.
It needs no rounding and is returned as it came, sign included.

The criterion for adopting it in a backend is a measurement, not the identity.
A backend takes the arithmetic path only where it measures faster than its conversion primitive.
The measurement is on `wat/f32_alu`.

It was verified against the previous implementation over 2.4 million values per language.
There were zero mismatches.
The values are:
- random f32 sums, products, differences and quotients;
- random doubles across the full exponent range;
- exact 24-bit ties;
- the neighbours of every boundary named above.

The spec harness passes for all three backends as well.

## Rejected alternatives

- **The reusable `IO::Buffer` scratch (#261, PR #263).**
  Rejected on its own measurements, and the splitting sidesteps what killed it.
  The splitting is stateless.
  So the thread-sharing constraint that forced the per-receiver placement does not arise.
  The bit-conversion units stay on `pack` untouched.
  They are `f32_bits`, `f32_from_bits`, `f64_bits`, and `f64_from_bits`.
  Reinterpretation is not rounding, and has no arithmetic equivalent.
- **Perl.**
  Perl's arithmetic operators take an integer fast path for integral operands.
  That is the same behavior `rt/fadd` already works around.
  The fast path computes `t` and `t - x` exactly, and returns `x` unrounded.
  No value below 1 in magnitude is integral.
  Keeping every intermediate there restores the correct result.
  That was verified over the same 2.4 million values.
  It costs a multiplication in and a multiplication out.
  Measured on `wat/f32_alu`, it was 1553 ns/op before against 1701 ns/op after.
  So perl keeps the pack path.
- **Widening the fast path to the whole finite range.**
  Above 2^127 the 24-bit rounding can produce 2^128.
  The mapping of the boundary region back to the largest finite f32 differs per language.
  MRI's `pack("e")` returns infinity, and Python's `struct.pack` raises.
  Reproducing that in the fast path buys nothing.
  Those magnitudes are rare, and the fallback already decides them correctly.
- **A per-backend bit-level rounding in integer arithmetic.**
  Rounding by shifting the f64 bit pattern needs the bits in the first place.
  Getting them is the `pack` this decision is removing.

## Consequences

- Measured on `wat/f32_alu` on the same host as the record.
  Iteration counts are calibrated per runner, and startup is subtracted.

  | runner | before | after |
  | --- | --- | --- |
  | dewasm-ruby | 626 ns/op | 367 ns/op |
  | dewasm-ruby-yjit | 545 ns/op | 229 ns/op |
  | dewasm-python | 850 ns/op | 549 ns/op |
  | dewasm-pypy | 336 ns/op | 6.7 ns/op |
  | dewasm-perl | 1553 ns/op | (unchanged) |

  PyPy's factor is the interesting one.
  The whole f32 loop becomes float arithmetic its JIT can trace, which `struct.pack` blocked.
  `wat/f64_alu` is unchanged for every runner, as it must be.
- NaN still reaches `pack`, so the f32 half of [decision 47](47-ruby-f64-sub-quiet-guard.md) still holds.
  `f32.sub` needs no quiet-NaN guard because the re-round quietens a signaling operand.
- Three languages now round f32 by two different mechanisms.
  A future backend has to pick one by measuring rather than by copying.
- Two things invalidate this.
  One is a host whose float multiplication or subtraction is not IEEE double.
  Perl built with long-double NVs is the near case, and its unit does not use the splitting anyway.
  The other is an interpreter whose conversion primitive becomes cheaper than three float operations.
