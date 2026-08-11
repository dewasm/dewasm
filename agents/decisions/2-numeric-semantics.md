# Decision 2 — Numeric Semantics Strategy for Dynamically-Typed Targets

Status: **Accepted, 2026-07-23.**
Backfilled; implemented for Ruby in `runtime/ruby/runtime.rb`.
The conventions below bind every backend whose language lacks fixed-width/unsigned integers or 32-bit floats (Ruby, Python, PHP, Bash); Java/Go map to native types instead.

## Context

Wasm requires bit-exact numerics: wrapping two's-complement i32/i64 with signed *and* unsigned views, IEEE 754 f32/f64 including NaN bit patterns, and precise trap conditions.
Ruby/Python have arbitrary-precision integers and only doubles; the spec testsuite (decision 3) checks all of it, including NaN payloads through `reinterpret` and memory.

## Decision

- **Integers are stored as masked unsigned values** (`x & 0xffffffff` / `& 0xffff...`), the signed view derived only where an instruction needs it (`div_s`, `lt_s`, `shr_s`, ...) via `s32`/`s64` helpers.
  Criterion: the *storage* representation should make the more mechanical operation free — masking after add/sub/mul is unavoidable either way, while unsigned compare/div/shift come for free on non-negative integers and memory stores need no sign fix-up.
  This convention is shared by all bignum-style backends so lowering tables stay parallel.
- **f32/f64 are host doubles; every f32 operation result is re-rounded to single precision.**
  Sound for add/sub/mul/div/sqrt because 53 ≥ 2·24 + 2 (double rounding is innocuous at that precision gap).
  Integer→f32 conversions of values above 2^53 pre-round to odd before the double→single step for the same reason.
- **NaN bit-exactness is achieved with software bit conversions exactly where the host rounds through a lossy path.**
  Measured on MRI: `pack("e")` canonicalizes NaN sign and payload, and overflows straight to infinity instead of rounding near f32-max.
  So f32 bit extraction/injection takes a software path for NaNs, f32 memory traffic goes through those helpers, and the overflow boundary (2^128 − 2^103) is handled explicitly.
  Operations required by the spec to *quiet* NaNs (floor/ceil/trunc/ nearest/sqrt/promote) set the quiet bit via bit manipulation.
- **Trap conditions are explicit checks in helpers** (`div` by zero, `INT_MIN / −1`, out-of-range `trunc`, memory bounds), with the spec interpreter's trap message strings, which the harness matches.

## Rejected alternatives

- **Representing floats as bit-pattern integers everywhere** — makes every arithmetic op a pack/unpack round-trip; only needed at the (few) lossy conversion points.
- **Signed storage representation** — every memory store, unsigned compare, and unsigned div/shift would need fix-ups; the testsuite is dominated by unsigned-view operations.

## Consequences

- Positive: f32/f64/f32_bitwise/float_memory/conversions spec files pass on Ruby, including NaN sign/payload assertions.
- Known limitation: a *signaling* NaN can be quieted by hardware float↔double conversion on paths we do not intercept; no spec test currently catches this on the Ruby backend, but it is a standing caveat for future backends.
- Exported function results are unsigned integers by ABI; embedders wanting signed views apply `s32`/`s64` themselves.
