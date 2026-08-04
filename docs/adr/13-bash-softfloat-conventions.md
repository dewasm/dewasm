# ADR-13 — Bash Softfloat Conventions

Status: **Accepted, 2026-07-23.** Implements ADR-5. ~60 units under `runtime/bash/units/rt/` (round-pack cores, f64 arithmetic, pattern ops, conversions, f32 wrappers) plus the lowering tables in `crates/dewasm-backend-bash/src/lib.rs`. `Feature::Floats` is flipped to Supported; the full-testsuite run now matches the Ruby backend's totals exactly (pass 24,338 / the same five linking-attributed failure groups), and cowsay/QuickJS run standalone under bash (`crates/dewasm-test-helper/src/apps.rs`).

## Context

ADR-5 decided *that* floats would be a pure-Bash IEEE-754 softfloat; this ADR records *how*. Bash offers only signed 64-bit integer arithmetic with mod-64 shift counts and wrapping multiplication, and the harness compares results bit-exactly including NaN patterns (ADR-8/ADR-3).

## Decision

- **Floats are stored as their bit patterns** — f64 as the signed-64 pattern, f32 as a u32 — and every operation is integer arithmetic on those patterns. No host float exists anywhere, so NaN bit-exactness holds by construction (the Ruby backend's `pack`-canonicalization fixups, ADR-2, have no analogue here). Reinterprets are the identity and float loads/stores reuse the integer memory units, so a float-free module's bundle is unchanged (ADR-6).
- **f64 is the core; f32 arithmetic is promote → f64 op → demote.** Promote is exact and 53 ≥ 2·24+2 makes the double rounding innocuous for add/sub/mul/div/sqrt (the same theorem the Ruby backend rests on, spec-suite-proven). Everything that needs no rounding theorem — comparisons, min/max, the ceil/floor/trunc/nearest family, int→f32 — operates directly on u32 patterns; int→f32 rounds once from the full 64-bit integer, eliminating Ruby's round-to-odd pre-step.
- **All rounding funnels through one shared core**, `rt_f64_round_pack s e m sk` (`rt_f32_round_pack` sibling): value = (−1)^s·m·2^(e−53) with a sticky flag, RNE, gradual underflow, and overflow to ±Inf. Its contract — m < 2^63, and never a left normalization with sticky pending — is what each caller's pre-normalization exists to satisfy. The reusable rule: any operation producing a rounded float reduces itself to (sign, exponent, ≥54-bit mantissa, sticky) and delegates.
- **Wide arithmetic avoids 64-bit overflow by construction**: mul splits mantissas 26/27 bits so every partial stays < 2^55; div is chunked long division (9 bits × 6 steps, remainder < 2^53); sqrt is a 55-iteration restoring root whose remainder stays < 2^59. Variable shifts are proven ≤ 63 or clamped (bash takes shift counts mod 64) and `-INT64_MIN` is special-cased in the i64 converts (negation is a wrapping no-op).
- **Arithmetic NaN results are always the canonical quiet NaN** (0x7ff8…0 / 0x7fc00000; demote keeps the sign). The spec's canonical/arithmetic NaN masks are satisfied by canonical always; abs/neg/copysign stay payload-preserving bit ops in the codegen.
- **A Rust-oracle test harness is the development net** (`crates/dewasm-backend-bash/tests/softfloat.rs`): ~100k edge and seeded-random vectors per run, expected values from host IEEE-754 adjusted to wasm semantics (wasm min/max, canonical NaNs, the Ruby trunc trap table). The spec harness remains the bar (ADR-3); the oracle exists to pinpoint the exact op and operands on a regression.

## Rejected alternatives

- **Host-float emulation à la Ruby** (compute in some host numeric type, fix up NaNs) — bash has no float type at all; there is nothing to fix up from.
- **Native 24-bit f32 arithmetic** — duplicates every algorithm for a second precision; the promote/op/demote route reuses the f64 core and is covered by the double-rounding theorem plus ~40k oracle vectors biased at the 24-bit boundary.
- **32-bit halves for the mantissa product** — partials reach ~2^64 and wrap (verified); the 26-bit split keeps everything provably in range.
- **128-bit hi/lo arithmetic for sqrt** — unnecessary: the radicand's low half is all zeros, so a 2-bits-per-step restoring loop never exceeds 2^59.
- **Payload-propagating NaNs** — the spec only ever checks quiet-bit masks for arithmetic results; propagating payloads would complicate every special-value path for zero observable benefit.

## Consequences

- Positive: the Bash backend reaches full parity with Ruby on the spec testsuite; real binaries (cowsay byte-identical to wasmtime in ~2 s, QuickJS `console.log` in ~23 s) run under plain bash — the ADR-0 flagship demo exists. The feared data-segment scaling wall did not materialize at cowsay/QuickJS size.
- Negative: softfloat ops cost 40–300 µs each; float-heavy hot loops are orders of magnitude slower than Ruby's host floats. The curated spec suite grew from ~1 s to ~5 s and the full run from ~39 s to ~58 s.
- The `floats` skip tag is dead: `Feature::Floats` Supported means any float-related skip is now a hard harness failure (ADR-8's ratchet).
