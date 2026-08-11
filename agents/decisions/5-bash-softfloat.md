# ADR-5 — Bash Floats: Pure-Bash Softfloat

Status: **Accepted, 2026-07-23.**
Backfilled; the policy was fixed during initial planning.
The implementation landed after the integers/memory/WASI milestones, with its conventions recorded in [ADR-13](13-bash-softfloat-conventions.md); the spec float files pass and real binaries run under bash.

**Revision, 2026-07-27:** the "dependency set is exactly a Bash interpreter" criterion is narrowly amended by [ADR-34](34-bash-wasi-filesystem.md) — the four WASI namespace-mutation syscalls may each call one POSIX command (`mkdir`/`rmdir`/`rm`/`mv`), which pure Bash cannot express.

## Context

Bash arithmetic (`$(( ))`) is signed 64-bit integers only; there is no float type.
The Bash backend is the project's defining demonstration (ADR-0): C/Rust tools running with no architecture-specific binary and no runtime dependency.
f32/f64 support therefore needs an emulation strategy, and the choice decides whether that promise actually holds.

## Decision

Implement IEEE 754 f32/f64 as a **softfloat library written in pure Bash**, operating on i64 bit patterns with integer arithmetic (add/sub/mul/div/ sqrt, comparisons, roundings, int↔float conversions).
Criterion: *the generated program's dependency set must be exactly "a Bash interpreter"* — the same property that makes the backend interesting — and the implementation must be able to pass the spec testsuite's float files (ADR-3), including NaN patterns and rounding modes.

Sequencing: the Bash backend ships integer/memory/control-flow/WASI support first; float-using modules fail with a clear conversion-time error until softfloat lands (consistent with ADR-0's unsupported-feature contract).

## Rejected alternatives

- **Delegating to `awk` / `bc`** — adds external-command dependencies (portability loss, huge per-call fork cost) and neither implements IEEE 754 semantics bit-exactly (NaN payloads, −0.0, round-to-nearest- even at the format boundary); the spec testsuite would not pass.
- **Integer-only forever** — leaves the defining demo unable to run most real Rust/C programs (`std` formatting paths alone pull in floats).

## Consequences

- Positive: "runs anywhere Bash runs" stays literally true; softfloat in ~pure shell is also simply a compelling artifact.
- Negative: large implementation cost and dire performance — floats in Bash will be orders of magnitude slower than integers, which are already slow.
  Accepted: the Bash backend's value is existence, not speed (stated in the README).
- The softfloat routines live in `runtime/bash/` like any other embedded runtime; the spec float files become the acceptance test.
