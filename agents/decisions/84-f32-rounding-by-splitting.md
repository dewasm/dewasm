# Decision 84: Round to f32 by Veltkamp Splitting, with the Pack Path as Fallback

Status: **Accepted, 2026-08-22.**
Landed in [`runtime/ruby/units/rt/f32.rb`](../../runtime/ruby/units/rt/f32.rb) and [`runtime/python/units/rt/f32.py`](../../runtime/python/units/rt/f32.py): a magnitude in [2^-126, 2^127) is rounded by three float operations, everything else keeps the `pack`/`unpack` path unchanged.
[`runtime/perl/units/rt/f32.pl`](../../runtime/perl/units/rt/f32.pl) keeps the pack path alone, because perl needs two extra multiplications to reach the same result and measures slower with them.

## Context

Every f32 operation is computed in double precision and re-rounded once through `Rt.f32` ([decision 2](2-numeric-semantics.md)), so that unit sits on the hot path of every f32-heavy program: one `pack` plus one `unpack` per arithmetic result, each a C call that allocates a String (Ruby) or a `bytes` and a tuple (Python).

The cost is the gap between the two arithmetic microbenchmarks, which differ only in the width of their operands (`benchmarks/wat/f32_alu.wat` against `benchmarks/wat/f64_alu.wat`).
In `records/2026-08-22T06-26-23Z-speed.json`:

| runner | wat/f64_alu | wat/f32_alu |
| --- | --- | --- |
| dewasm-ruby | 102 ns/op | 677 ns/op |
| dewasm-python | 152 ns/op | 849 ns/op |
| dewasm-perl | 831 ns/op | 1547 ns/op |

Replacing the byte round trip with a reusable `IO::Buffer` scratch was tried and rejected (#261, PR #263, closed unmerged; `agents/experiments.md`, float-bits-scratch): a scratch buffer is state, one per receiver after the module-level placement corrupted floats across threads, and the instance-variable read plus the `IO::Buffer` call pair cost more than the `pack` pair on a real app.

## Decision

Round by arithmetic on the range where arithmetic is provably the same rounding, and keep the existing conversion as the fallback for every other input.

For `x` with 2^-126 <= |x| < 2^127:

```
t = x * 536870913.0      # 2^29 + 1
r = t - (t - x)
```

This is Veltkamp splitting.
It returns `x` with its significand rounded to 24 bits, to nearest, ties to even, which is exactly the single rounding the f32 convention asks for, and it is exact because a double carries 53 >= 2 * 24 + 2 bits.
Inside that range the result is always a normal f32 and no intermediate leaves the double range.

The fallback keeps every other input on `pack`, byte for byte the behavior of before: subnormal magnitudes below 2^-126 (rounded at a fixed exponent, not to 24 significant bits), magnitudes at or above 2^127 (where the 24-bit result can be 2^128, which is not an f32, so the overflow boundary at 2^128 - 2^103 decides), infinities, and NaN (which fails both comparisons, so no explicit test is needed, and which `pack` canonicalizes as before).
Zero is the one input that takes neither path: it needs no rounding and is returned as it came, sign included.

The criterion for adopting it in a backend is a measurement, not the identity: a backend takes the arithmetic path only where the arithmetic path measures faster than its conversion primitive on `wat/f32_alu`.

Verified against the previous implementation over 2.4 million values per language (random f32 sums, products, differences and quotients, random doubles across the full exponent range, exact 24-bit ties, and the neighbours of every boundary named above), zero mismatches, plus the spec harness for all three backends.

## Rejected alternatives

- **The reusable `IO::Buffer` scratch (#261, PR #263).**
  Rejected on its own measurements, and the splitting sidesteps what killed it: it is stateless, so the thread-sharing constraint that forced the per-receiver placement does not arise, and the bit-conversion units (`f32_bits`, `f32_from_bits`, `f64_bits`, `f64_from_bits`) stay on `pack` untouched, since reinterpretation is not rounding and has no arithmetic equivalent.
- **Perl.**
  Perl's arithmetic operators take an integer fast path for integral operands (the same behavior `rt/fadd` already works around), which computes `t` and `t - x` exactly and returns `x` unrounded.
  Keeping every intermediate below 1 in magnitude, where no value is integral, restores the correct result (verified over the same 2.4 million values) but costs a multiplication in and a multiplication out: measured on `wat/f32_alu`, 1553 ns/op before against 1701 ns/op after, so perl keeps the pack path.
- **Widening the fast path to the whole finite range.**
  Above 2^127 the 24-bit rounding can produce 2^128, and the mapping of the boundary region back to the largest finite f32 differs per language (MRI's `pack("e")` returns infinity, Python's `struct.pack` raises).
  Reproducing that in the fast path buys nothing: those magnitudes are rare, and the fallback already decides them correctly.
- **A per-backend bit-level rounding in integer arithmetic.**
  Rounding by shifting the f64 bit pattern needs the bits in the first place, which is the `pack` this decision is removing.

## Consequences

- Measured on `wat/f32_alu` (same host as the record, iteration counts calibrated per runner, startup subtracted):

  | runner | before | after |
  | --- | --- | --- |
  | dewasm-ruby | 626 ns/op | 367 ns/op |
  | dewasm-ruby-yjit | 545 ns/op | 229 ns/op |
  | dewasm-python | 850 ns/op | 549 ns/op |
  | dewasm-pypy | 336 ns/op | 6.7 ns/op |
  | dewasm-perl | 1553 ns/op | (unchanged) |

  PyPy's factor is the interesting one: the whole f32 loop becomes float arithmetic its JIT can trace, which `struct.pack` blocked.
  `wat/f64_alu` is unchanged for every runner, as it must be.
- NaN still reaches `pack`, so the f32 half of [decision 47](47-ruby-f64-sub-quiet-guard.md) still holds: `f32.sub` needs no quiet-NaN guard because the re-round quietens a signaling operand.
- Three languages now round f32 by two different mechanisms, and a future backend has to pick one by measuring rather than by copying.
- What invalidates this: a host whose float multiplication or subtraction is not IEEE double (perl built with long-double NVs is the near case, and its unit does not use the splitting anyway), or an interpreter whose conversion primitive becomes cheaper than three float operations.
