# Decision 47: Inline Quiet-NaN Guard for Ruby f64.sub

Status: **Accepted, 2026-07-29.**
Implemented in the binary operation lowering of the Ruby backend, at `F64Sub`.
It is in `crates/dewasm-backend-ruby/src/lib.rs`.
It reuses the existing `rt/quiet_nan` unit.

## Context

Wasm requires float arithmetic to return *arithmetic* (quiet-bit-set) NaNs.
The Ruby lowering emitted plain `(a - b)` for `f64.sub` ([decision 4](4-ruby-backend-lowering.md)).
That implicitly assumed the host FPU executes the subtraction and quietens a signaling NaN operand.
**GCC-built MRI** breaks that assumption.
There `a - b` with `b == +0.0` returns the LHS bits unquieted (a fresh Float, same bits).
This held on every version probed (3.4.5, 3.4.10, 4.0.6, head) and on every call shape.
It held on both x86_64 and arm64 Linux.
Ruby builds made with Clang (macOS) quieten correctly.

The mechanism was isolated with a minimal C model:

- MRI's flonum decode (`rb_float_flonum_value`) special-cases the flonum encoding of +0.0.
  It does so with a literal `return 0.0`.
- GCC copies the subtraction into that branch and folds `a - 0.0 → a`.
  MRI ships `-fno-fast-math` but not `-fsignaling-nans`.
  The fold is IEEE-valid once signaling NaNs are assumed away.
  It skips the FPU sub.
- Clang performs no such fold.

`x - (+0.0)` is the only affected identity.
That is because `x - (-0.0)` and `x + (±0.0)` are not foldable under signed zeros.
Only +0.0 has the literal decode branch.
Upstream, NaN payloads are explicitly not considered by Ruby (bugs.ruby-lang.org #20662).
So this is behavior dewasm must own, not a bug fix to wait for.
Caught by the first Linux CI runs (issue #11): f64.wast:742,746 and float_exprs.wast:76,2409.

## Decision

Lower `f64.sub` to an inline self-compare guard:

```ruby
((r = a - b) == r ? r : Rt.quiet_nan(r))
```

`r == r` is false only for NaN.
So the common path adds one C-level compare (no method call, no allocation).
That matches the Ruby performance approach (decision 32/33/42-44).
The NaN path routes through the existing `rt/quiet_nan`.
It preserves sign and payload, hence the result is still an arithmetic NaN.

The criterion for guarding an operation concerns observation.
**An operation is guarded only when a real host was observed returning a signaling NaN from it.**
Today that is `f64.sub` alone.
`f32.sub` is already safe: the `Rt.f32` re-round's `pack("e")` narrowing quietens on every probed host.
Add, multiply, and divide quietened everywhere.
The same criterion applies to other backends if they exhibit the class.

## Rejected alternatives

- **Guard every float binary operation, broken or not.**
  It pays the guard cost on operations no host has been observed to break.
  The specification harness runs on both development (macOS) and CI (Linux) hosts.
  So a newly broken operation surfaces as a failing specification trial.
  It gets its guard then, with evidence.
- **Keep the CI and development Ruby at an unaffected build.**
  Every GCC-built MRI folds (all probed versions, both architectures).
  In practice that is every Linux Ruby.
  Generated code must be correct on whatever host Ruby a user runs.
- **An `Rt.fsub` helper unit.**
  It costs a method call per subtraction on the hot path.
  The inline guard is both faster and no less clear.

## Consequences

- `f64.sub` output grows a temp-and-ternary wrapper.
  Correctness comes before generated-code readability ([decision 1](1-ir-design.md)).
- The `r` temp is written then immediately read within one expression.
  So nested subs re-assign it only after the inner value is consumed.
  It cannot share a name with the generated `l*`/`s*` locals.
- The quiet guard's absence elsewhere is an intended, evidence-driven gap.
  A future host might fold another identity, for example `x + (-0.0)`.
  It will then fail the specification harness loudly, not silently.
