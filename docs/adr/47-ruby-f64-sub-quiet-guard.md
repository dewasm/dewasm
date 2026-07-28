# ADR-47 — Inline Quiet-NaN Guard for Ruby f64.sub

Status: **Accepted, 2026-07-29.** Implemented in the Ruby backend's binop
lowering (`crates/dewasm-backend-ruby/src/lib.rs`, `F64Sub`), reusing the
existing `rt/quiet_nan` unit.

## Context

Wasm requires float arithmetic to return *arithmetic* (quiet-bit-set) NaNs.
The Ruby lowering emitted plain `(a - b)` for `f64.sub` ([ADR-4](4-ruby-backend-lowering.md)),
implicitly assuming the host FPU executes the subtraction and quietens a
signaling-NaN operand. Linux builds of MRI break that assumption: `a - b`
with `b == +0.0` returns the LHS bits unquieted (a fresh Float, same bits) on
every version probed (3.4.5, 3.4.10, 4.0.6, head), on every call shape.
`x - (+0.0) → x` is the one sub identity that is IEEE-valid *except* for sNaN
quieting, which is exactly the shape a compiler building the interpreter may
fold; macOS/arm64 builds execute the real subtraction. Caught by the first
Linux CI runs (issue #11): f64.wast:742,746 and float_exprs.wast:76,2409.

## Decision

Lower `f64.sub` to an inline self-compare guard:

```ruby
((r = a - b) == r ? r : Rt.quiet_nan(r))
```

`r == r` is false only for NaN, so the common path adds one C-level compare —
no method call, no allocation — matching the Ruby perf posture
(ADR-32/33/42–44); the NaN path routes through the existing `rt/quiet_nan`
(sign and payload preserved, hence still an arithmetic NaN).

The criterion for guarding an op: **an op gets a quiet guard only when a real
host was observed returning a signaling NaN from it.** Today that is `f64.sub`
alone — `f32.sub` is already safe because the `Rt.f32` re-round's `pack("e")`
narrowing quietens on every probed host, and add/mul/div quietened everywhere.
The same criterion applies to other backends if they exhibit the class.

## Rejected alternatives

- **Guard every float binop defensively.** Pays the guard cost on ops no host
  has been observed to break; the spec harness runs on both dev (macOS) and CI
  (Linux) hosts, so a newly broken op surfaces as a red spec trial and gets
  its guard then, with evidence.
- **Pin the CI/dev Ruby to an unaffected build.** No unaffected Linux MRI
  exists (all probed versions fold), and generated code must be correct on
  whatever host ruby a user runs.
- **An `Rt.fsub` helper unit.** A method call per subtraction on the hot
  path; the inline guard is both faster and no less clear.

## Consequences

- `f64.sub` output grows a temp-and-ternary wrapper; correctness outranks
  generated-code readability ([ADR-1](1-ir-design.md)).
- The `r` temp is written then immediately read within one expression, so
  nested subs re-assign it only after the inner value is consumed; it cannot
  collide with the generated `l*`/`s*` locals.
- The quiet guard's absence elsewhere is a deliberate, evidence-driven gap:
  a future host that folds another identity (e.g. `x + (-0.0)`) will fail the
  spec harness loudly, not silently.
