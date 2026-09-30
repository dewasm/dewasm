# Decision 13: Bash Softfloat Conventions

Status: **Accepted, 2026-07-23.**
Implements decision 5.
Implemented as ~60 units under `runtime/bash/units/rt/`.
The lowering tables are in `crates/dewasm-backend-bash/src/lib.rs`.
The units cover round-pack cores, f64 arithmetic, pattern operations, conversions, and f32 wrappers.
`Feature::Floats` is flipped to Supported.
The full-testsuite run now matches the Ruby backend's totals exactly.
Those are pass 24,338 and the same five linking-attributed failure groups.
`cowsay` and QuickJS run standalone under Bash (`crates/dewasm-test-helper/src/apps.rs`).

## Context

Decision 5 decided *that* floats would be an IEEE 754 softfloat in pure Bash.
This decision records *how*.
Bash offers only signed 64-bit integer arithmetic.
Its shift counts are modulo 64, and its multiplication wraps.
The harness compares results bit-exactly including NaN patterns (decision 8/decision 3).

## Decision

- **Floats are stored as their bit patterns**: f64 as the signed-64 pattern, f32 as a u32.
  Every operation is integer arithmetic on those patterns.
  No host float exists anywhere, so NaN bit-exactness holds by construction.
  The Ruby backend's fixes around `pack` (decision 2) have no equivalent here.
  Reinterprets are the identity, and float loads/stores reuse the integer memory units.
  So a float-free module's bundle is unchanged (decision 6).
- **f64 is the core; f32 arithmetic is `promote` → f64 operation → `demote`.**
  `promote` is exact, and 53 ≥ 2·24+2 means double rounding causes no error for `add`/`sub`/`mul`/`div`/`sqrt`.
  This is the same theorem the Ruby backend rests on, proven by the specification suite.
  Everything that needs no rounding theorem operates directly on u32 patterns.
  That covers comparisons, `min`/`max`, the `ceil`/`floor`/`trunc`/`nearest` family, and integer→f32.
  Integer→f32 rounds once from the full 64-bit integer, removing Ruby's round-to-odd first step.
- **All rounding goes through one shared core**, `rt_f64_round_pack s e m sk`.
  `rt_f32_round_pack` is the same core for f32.
  Its value is (−1)^s·m·2^(e−53) with a sticky flag.
  It implements RNE, gradual underflow, and overflow to ±Inf.
  Its contract is m < 2^63, and never a left normalization with sticky pending.
  Each caller's pre-normalization exists to satisfy that contract.
  The reusable rule applies to any operation producing a rounded float.
  It reduces itself to four values and hands them to the core.
  Those are (sign, exponent, ≥54-bit significand, sticky).
- **Wide arithmetic avoids 64-bit overflow by construction**:

  - `mul` splits significands 26/27 bits, so every partial stays < 2^55;
  - `div` is long division in chunks (9 bits × 6 steps, remainder < 2^53);
  - `sqrt` is a 55-iteration restoring root whose remainder stays < 2^59.

  Variable shifts are proven ≤ 63 or clamped (Bash takes shift counts modulo 64).
  `-INT64_MIN` is special-cased in the i64 converts (0 minus it wraps back to the same value).
- **Arithmetic NaN results are always the canonical quiet NaN** (0x7ff8…0 / 0x7fc00000).
  `demote` keeps the sign.
  The specification's canonical/arithmetic NaN masks are satisfied by canonical always.
  `abs`/`neg`/`copysign` stay bit operations in generated code, and keep the payload.
- **A Rust-oracle test harness is the development-time check.**
  It is `crates/dewasm-backend-bash/tests/softfloat.rs`.
  Each run has ~100k edge and seeded-random vectors.
  Expected values come from host IEEE-754 adjusted to wasm semantics.
  The adjustments are wasm `min`/`max`, canonical NaNs, and the Ruby `trunc` trap table.
  The specification harness remains the bar (decision 3).
  The oracle exists to name the exact operation and operands on a regression.

## Rejected alternatives

- **Host floats with fixes, as in Ruby** (compute in some host numeric type, fix up NaNs).
  Bash has no float type at all; there is nothing to fix up from.
- **Native 24-bit f32 arithmetic**: repeats every algorithm for a second precision.
  The `promote`/operation/`demote` route reuses the f64 core.
  It is covered by the double-rounding theorem plus ~40k oracle vectors biased at the 24-bit boundary.
- **32-bit halves for the significand product**: partials reach ~2^64 and wrap (verified).
  The 26-bit split keeps everything provably in range.
- **128-bit high/low arithmetic for `sqrt`**: not needed.
  The low half of the number under the root is all zeros.
  So a 2-bits-per-step restoring loop never exceeds 2^59.
- **Payload-propagating NaNs**: the specification only ever checks quiet-bit masks for arithmetic results.
  Carrying payloads through would complicate every special-value path for zero observable benefit.

## Consequences

- Positive: the Bash backend reaches the same results as Ruby on the specification testsuite.
  Real binaries run under plain Bash.
  `cowsay` output is identical to Wasmtime's in ~2 s, and QuickJS runs `console.log` in ~23 s.
  The defining demonstration of decision 0 exists.
  The feared data-segment scaling limit did not appear at `cowsay`/QuickJS size.
- Negative: softfloat operations cost 40-300 µs each.
  Float-heavy hot loops are orders of magnitude slower than Ruby's host floats.
  The selected specification suite grew from ~1 s to ~5 s, and the full run from ~39 s to ~58 s.
- The `floats` skip tag is dead.
  `Feature::Floats` Supported means any float-related skip is now a hard harness failure.
  This is decision 8's one-way discipline.
