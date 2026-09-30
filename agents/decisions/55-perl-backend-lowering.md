# Decision 55: Perl Backend Lowering Conventions

Status: **Accepted, 2026-08-01.**
First milestone: the specification harness passes (issue #68).
It is implemented in `crates/dewasm-backend-perl/src/lib.rs` + `runtime/perl/units/`.
The full-testsuite run passes with the same `import-limits`/`linking` list as Python's.
[Decision 91](91-perl-inline-fast-paths.md) narrows the float helper calls and memory method calls accepted here to fallback paths.
Everything else stands.
Numeric conventions are [decision 2](2-numeric-semantics.md)'s.
This decision covers where Perl forced (or spared) a different shape from Ruby and Python.
The Ruby shapes are [decision 4](4-ruby-backend-lowering.md)/[42](42-ruby-label-variable-chain.md); the Python shape is [decision 28](28-python-backend-lowering.md).
Each choice comes with the measured Perl behaviors it rests on (`perl` 5.42, `ivsize=8`/`nvsize=8`).
The header of the generated code verifies those sizes at load ([decision 15](15-tests-fail-not-skip.md)).
`find_perl` tests at >= 5.26: the features used floor out at POSIX C99 math, 5.22, plus margin.
WASI Preview 1 is follow-up work (issue #69).

## Context

Perl is dynamically typed like Ruby/Python, but its scalars are *not* bignums.
An SV holds a 64-bit IV/UV or an NV (double), and the interpreter switches representations silently.
Three measured behaviors shaped the numeric lowering:

1. **Arithmetic takes an integer fast path.**
   For integral operands, `+`/`-`/`*`/`/` compute integer-exact results and turn `-0.0` operands into `+0`.
   The results stay exact beyond double precision: `0.0 + 2^53+1` stays `2^53+1`.
   In the other direction, `use integer` arithmetic reads the raw 64-bit slots and wraps at 2^64.
   Under it, `/`/`%` become C division (toward zero, dividend-signed), and `>>` becomes arithmetic.
2. **Unsigned 64-bit works natively when values stay IV/UV.**
   Masking (`& 0xFFFFFFFFFFFFFFFF`) yields exact UVs including `-1` → `2^64-1`.
   Comparison operators order two IV/UV operands exactly.
   `($n - $n % $d) / $d` divides exactly (Perl's even-division integer path).
   UV `<<` wraps at 2^64.
   But UV+UV overflow, and any operation with an NV operand, silently fall to NV precision.
3. **Float edges die or lie.**
   `x / 0.0`, `x % 0`, and `sqrt(-1)` all die.
   `pack 'f'` clamps out-of-float-range values to infinity instead of IEEE-rounding them.
   So the `2^128 - 2^103` boundary needs the same software handling as Ruby (decision 2).
   `pack 'f'` also canonicalizes NaN payloads, while `pack 'd'` is a bit-exact byte copy.
   `POSIX::nearbyint` is round-half-even and preserves `-0.0`.

On control flow Perl is *stronger* than Ruby/Python.
`last LABEL`/`next LABEL` exit or continue any outer labeled block or loop at arbitrary depth.
They do not cross sub boundaries, or escape `do {}`/`eval {}` blocks.
The lowering puts branch-crossing code inside neither kind of block.
And Perl recursion is heap-allocated: it neither overflows a C stack nor raises anything catchable.
An unbounded recursion just uses up memory.

## Decision

- **Branches lower to native labels; no flag machinery.**
  `Block` → `Ln: { ... }`, and `Loop` → `Ln: while (1) { ...; last Ln; }`.
  A wasm loop label is a continue-target, so `br` to it is `next Ln`.
  Falling off the loop body must exit.
  A referenced `If` → a labeled block around the `if/else`.
  Every `br` is a direct `last Ln`/`next Ln`, and `br_table` dispatches an `if/elsif` chain of them.
  Unreferenced labels emit no frame (bodies splice inline).
  A gap forced Ruby's `__br` chain (decision 42) and Python's `_br` register (decision 28).
  That exact gap does not exist in Perl.
  A code-shape test (`mod branch_shape`) checks that no such scheme reappears.
  Criterion: *use the target language's structured non-local exit directly when it has one.*
  *That exit must cover wasm's label discipline exactly.*
  *Synthetic control state is only for languages that lack it.*
- **`use integer` only inside runtime helpers, never in generated function bodies.**
  Operations that need C-style 64-bit semantics are `Rt::` helpers.
  Each is built on a tightly-scoped `do { use integer; ... }`.
  The unsigned mask is applied *outside* the scope of `use integer`.
  The operations are `i32.mul`, `i64.add`/`sub`/`mul`, signed `div`/`rem`, `shr_s`, and `s64`.
  `i64.add`/`sub`/`mul` need it by behavior 1/2: plain arithmetic would round through NVs or overflow.
  `i32.add`/`sub` and all shifts, bit operations and unsigned compares stay inline plain expressions.
  They are exact per behavior 2.
  Unsigned division uses the exact form `($n - $n % $d) / $d`.
  Signed division/remainder use C division under `use integer`.
  The `INT_MIN / -1` (SIGFPE) case is trapped first.
- **Float arithmetic goes through `Rt::fadd`/`fsub`/`fmul`/`fdiv`.**
  Each does the native operation, then a `pack 'd'` round-trip to restore IEEE rounding.
  The round-trip undoes behavior 1's excess integer exactness.
  Then it re-signs an exact-zero result by the IEEE rule the integer path dropped.
  That rule is `-0` for `-0 + -0` and for `(-0) - (+0)`.
  Products and division results take the XOR of the signs.
  `Rt::f32` re-rounds through `pack 'f'` with the overflow boundary mapped back to ±`FLT_MAX` in software.
  NaN bit paths go through `pack 'd'` software widening (`Rt::f32_bits`/`f32_from_bits`).
  This mirrors decision 2.
  `fceil`/`ffloor`/`ftrunc`/`fnearest` are `POSIX` C99 calls plus explicit NaN quieting.
  `int -> f64` conversion also takes the `pack 'd'` round-trip (`Rt::cvt_f64_i`).
- **Exhaustion is an explicit frame-weighted depth counter.**
  Every generated function opens with `local $Rt::DEPTH = $Rt::DEPTH + <weight>;`.
  It traps `call stack exhausted` past `$Rt::LIMIT` (100000).
  `local` restores the counter on every exit path, including a trap's die-unwind.
  The weight is the compile-time constant `1 + (params + locals + temps) / 8`.
  It approximates the byte-bounded native stack rather than counting calls.
  A pure count lets an unbounded recursion with large frames heap-allocate count × frame-size.
  Only then does the count stop it.
  The measured case is the specification test `skip-stack-guard-page`.
  It has 1056 locals and unconditional recursion.
  On it, the count-based scheme peaked at 10.6 GB / 59 s per assertion.
  That scheme OOM-killed 16 GB CI runners.
  The weighted scheme (weight 133 → ~750 frames) traps on it in ~70 MB.
  Small functions keep weight 1, so legitimate deep recursion is unchanged.
  The measured cost is ~1.75x per call, and it is accepted.
  Without the counter, `assert_exhaustion` is an OOM kill.
- **Module = one package of blessed hash reference instances; the embedded runtime is prefix-namespaced.**
  `Package->new(\%imports)` builds a blessed hash reference.
  It holds memory/table/global objects, import code references, and an `exports` closure map.
  `invoke`/`global_get`/`wasm_import` mirror Python's [decision 7](7-import-providers.md) surface.
  So `register`ed instances serve as import providers.
  The harness then runs with `supports_registered_imports`.
  Runtime units live in `Rt`-rooted packages (`Rt`, `Rt::Memory`, `Rt::Table`, `Rt::Global`).
  Perl package names are absolute (no lexical nesting).
  So `Embedded` linkage rewrites the `Rt::` prefix to `<Package>::Rt::` at bundle time.
  Two generated artifacts in one process then keep independent runtimes.
  Ruby gets this from constant nesting; Perl gets it by a text rewrite.
  `Alias("Rt")` (the specification harness) keeps the shared top-level name.
- **Linear memory is one byte string changed in place.**
  Access to more than one byte uses four-argument `substr` + `pack`/`unpack` (`V`/`v`/`Q<`/`d<`).
  Single bytes use `vec`.
  Measured on a 2M-operation loop, `vec` is fastest for bytes (0.07s vs. 0.14s for `unpack`+`substr`).
  But `vec` is only big-endian beyond 8 bits, so wider access goes through `unpack`.
  Its cost, 0.14s/2M ≈ 70ns, is acceptable.
  Traps (bounds, division by zero, overflow, a `trunc` of NaN or out of range) `die` a blessed `Rt::Trap`.
  It carries the specification interpreter's message wording.

## Rejected alternatives

- **A branch register / `catch`-`throw`-style chain (Ruby decision 42, Python decision 28).**
  Perl's labels already express arbitrary-depth exits directly.
  Any synthetic scheme would add per-statement cost and code for nothing.
- **`use integer` at function scope.**
  Its lexical scope changes `/`, `%`, `>>` and bit operation signedness for *everything*.
  That includes float code (its fraction is silently dropped).
  Tightly-scoped helper subs keep the audit surface to one file per operation.
- **Native float operators (`$a + $b`) for f64 arithmetic.**
  Measured wrong twice over: integer-exact results beyond 2^53 and `-0.0` loss (behavior 1).
  The helper + `pack 'd'` round-trip restores IEEE semantics for ~one extra opcode per operation.
- **NV (float) division for `i64.div_u`.**
  Loses low bits for results above 2^53; the exact remainder-adjusted form costs one extra `%`.
- **Rescuing real unbounded recursion instead of counting.**
  Perl offers nothing to rescue: recursion is heap-allocated and uncatchable at exhaustion.
  The "deep recursion" warning fires at 100 and is only a warning.
  A counter is the only deterministic trap.
- **A single shared top-level `Rt` for embedded output (Python's shape).**
  Python files are modules with separate namespaces.
  Perl `require`s share one global namespace.
  So two artifacts would both claim `Rt` exactly as [decision 6](6-runtime-units.md) warns.
  The text prefix rewrite is one string replace at bundle time.

## Consequences

- Positive: the full specification testsuite run passes (257 files).
  Its list is identical in shape to Python's `import-limits`/`linking` entries.
  The branch lowering is the simplest of any dewasm backend: no pre-pass, no epilogues, no guards.
  The whole-cache convert suite passes ([decision 54](54-apps-convert-suite.md)).
- Negative: every `f64.add`/`sub`/`mul` is a sub call plus a `pack` round-trip.
  Every call pays the depth-counter `local`.
  Perl output will be slower than Ruby's on hot float/call paths.
  `$Rt::LIMIT` bounds legitimate deep recursion (raisable by the embedder).
- Carry-over: WASI Preview 1 (`runtime/perl/units/wasi/`, the `e2e`/`wasi_testsuite` suites) is issue #69.
  `check_import_kind` shares Python's `import-limits` gap (kind checked, finer wasm type not).
