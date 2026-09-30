# Decision 5: Bash Floats (Pure-Bash Softfloat)

Status: **Accepted, 2026-07-23.**
Recorded afterwards; the policy was fixed during initial planning.
The implementation landed after the integers/memory/WASI milestones.
Its conventions are recorded in [decision 13](13-bash-softfloat-conventions.md).
The specification float files pass, and real binaries run under `bash`.

**Revision, 2026-07-27:** [decision 34](34-bash-wasi-file-system.md) narrowly changes one criterion.
The changed criterion is "dependency set is exactly a Bash interpreter".
The four WASI namespace-mutation system calls may each call one POSIX command.
The commands are `mkdir`, `rmdir`, `rm` and `mv`.
Pure Bash cannot express those four operations.

## Context

Bash arithmetic (`$(( ))`) is signed 64-bit integers only; there is no float type.
The Bash backend is the project's defining demonstration (decision 0).
It demonstrates C/Rust tools running with no architecture-specific binary and no runtime dependency.
f32/f64 support therefore needs to be built in software.
The choice decides whether that promise actually holds.

## Decision

Implement IEEE 754 f32/f64 as a **softfloat library written in pure Bash**.
It operates on i64 bit patterns with integer arithmetic.
It covers `add`/`sub`/`mul`/`div`/`sqrt`, comparisons, roundings, and integer↔float conversions.
Criterion: *the generated program's dependency set must be exactly "a Bash interpreter"*.
That is the same property that makes the backend interesting.
The implementation must also be able to pass the specification testsuite's float files (decision 3).
That includes NaN patterns and rounding modes.

Sequencing: the Bash backend ships integer/memory/control-flow/WASI support first.
Until softfloat lands, float-using modules fail with a clear conversion-time error.
This is consistent with decision 0's unsupported-feature contract.

## Rejected alternatives

- **Handing floats to `awk` / `bc`**: adds dependencies on external commands.
  Those mean portability loss and a huge per-call fork cost.
  Neither tool implements IEEE 754 semantics bit-exactly.
  The gaps are NaN payloads, −0.0, and round-to-nearest-even at the format boundary.
  So the specification testsuite would not pass.
- **Integer-only forever**: leaves the defining demonstration unable to run most real Rust/C programs.
  `std` formatting paths alone pull in floats.

## Consequences

- Positive: "runs anywhere Bash runs" stays literally true.
  Softfloat in ~pure shell is also an interesting artifact in itself.
- Negative: large implementation cost and poor performance.
  Floats in Bash will be orders of magnitude slower than integers, which are already slow.
  Accepted: the Bash backend's value is existence, not speed (stated in the README).
- The softfloat routines live in `runtime/bash/` like any other embedded runtime.
  The specification float files become the test that decides completion.
