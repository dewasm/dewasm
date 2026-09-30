# Decision 2: Numeric Semantics Strategy for Dynamically-Typed Targets

Status: **Accepted, 2026-07-23.**
Recorded afterwards; implemented for Ruby in `runtime/ruby/runtime.rb`.
The conventions below bind every backend whose language lacks fixed-width/unsigned integers.
They also bind every backend whose language lacks 32-bit floats.
Those are Ruby, Python, PHP, and Bash; Java/Go map to native types instead.
[Decision 71](71-mask-elision-modular-consumers.md) loosens the always-masked convention to masked at observation points.
Inside one expression tree, a value whose consumer restores the mask may stay unmasked.
The storage representation itself is unchanged.

## Context

Wasm requires bit-exact numerics:

- wrapping two's-complement i32/i64 with signed *and* unsigned views;
- IEEE 754 f32/f64 including NaN bit patterns;
- precise trap conditions.

Ruby/Python have arbitrary-precision integers and only doubles.
The specification testsuite (decision 3) checks all of it.
That includes NaN payloads through `reinterpret` and memory.

## Decision

- **Integers are stored as masked unsigned values** (`x & 0xffffffff` / `& 0xffff...`).
  The signed view is derived only where an instruction needs it (`div_s`, `lt_s`, `shr_s`, ...).
  The `s32`/`s64` helpers derive it.
  Criterion: the *storage* representation should make the more mechanical operation free.
  Masking after `add`/`sub`/`mul` is unavoidable either way.
  Unsigned comparison, division and shift, however, come for free on non-negative integers.
  Memory stores need no sign fix-up.
  This convention is shared by all bignum-style backends, so lowering tables stay parallel.
- **f32/f64 are host doubles; every f32 operation result is re-rounded to single precision.**
  Sound for `add`/`sub`/`mul`/`div`/`sqrt` because 53 ≥ 2·24 + 2.
  Double rounding causes no error at that precision gap.
  Integer→f32 conversions of values above 2^53 pre-round to odd before the double→single step.
  The reason is the same.
- **Software bit conversions give NaN bit-exactness where the host's rounding path loses bits.**
  Measured on MRI: `pack("e")` canonicalizes NaN sign and payload.
  It also overflows straight to infinity instead of rounding near f32-max.
  So f32 bit extraction/injection takes a software path for NaNs.
  Memory traffic of f32 values goes through those helpers.
  The overflow boundary (2^128 − 2^103) is handled explicitly.
  The specification requires some operations to *quiet* NaNs.
  Those are `floor`/`ceil`/`trunc`/`nearest`/`sqrt`/`promote`, and they set the quiet bit via bit manipulation.
- **Trap conditions are explicit checks in helpers.**
  They cover `div` by zero, `INT_MIN / −1`, out-of-range `trunc`, and memory bounds.
  The checks use the reference interpreter's trap message strings, which the harness matches.

## Rejected alternatives

- **Representing floats as bit-pattern integers everywhere.**
  It makes every arithmetic operation a pack/unpack round-trip.
  It is only needed at the (few) conversion points that lose bits.
- **Signed storage representation**: every memory store would need a fix-up.
  So would every unsigned comparison and unsigned division and shift.
  The testsuite is dominated by unsigned-view operations.

## Consequences

- Positive: the `f32`/`f64`/`f32_bitwise`/`float_memory`/`conversions` specification files pass on Ruby.
  That includes their NaN sign/payload assertions.
- Known limitation: a *signaling* NaN can be quieted by the processor's float↔double conversion.
  This happens on paths our helpers do not cover.
  No specification test currently catches this on the Ruby backend.
  But it is a standing limit for future backends.
- Exported function results are unsigned integers by ABI.
  Embedders wanting signed views apply `s32`/`s64` themselves.
