# Decision 78: Wrapping-Add Memory Units for Dynamic Addresses

Status: **Accepted, 2026-08-16.**
It landed for Ruby and Python.
Each memory load/store unit has an `a`-suffixed twin for a dynamic `i32.add` address.
The twin takes the address as two arguments and wraps their sum.
The emitters route offset-zero dynamic-add sites to it.
Bash inlines its memory operations (decision 52), and Go and Java compile the addition natively.
Perl keeps the per-site arithmetic until it adopts the family with its own measurement.
Decision 79 later replaced the base unit names this record spells with single-character codes.
For example, `i32_load` became `iwl` and `i32_loada` became `iwla`.
The wrapping-add shape, the routing rule, and the `a` suffix stand unchanged.

## Context

After decision 76 the emitters render a dynamic address bare, and the unit reduces it.
An example is `@m.i32_load(l0 + l1)`.
The site still pays the addition, one `opt_plus` and its call data resident in the ISeq per site.
Decision 75 moved the static-offset addition into the units.
The dynamic addition is the per-site arithmetic that remains at load/store sites.

The `o` units cannot absorb it, because the two additions have incompatible semantics.
Wasm's effective address is the base reduced modulo 2^32, plus the static offset.
The static offset adds to it *without* further reduction.
`address.wast` requires a trap when base plus offset leaves u32.
So `i32_loado` adds exactly (decision 76).
A dynamic `i32.add` feeding an address must *wrap* its sum.
`memory_trap.wast` stores at `memory.size * 0x10000 + (-4)` and requires success.
The wrapped sum lands back in bounds.
One unit cannot do both.
Routing a dynamic add through the `o` form would trap where wasm wraps.
Making the `o` form wrap would succeed where wasm traps.

## Decision

**Dynamic-add addresses get their own unit family.**
**`i32_loada(a, b)` computes `(a + b) & 0xffffffff` and then runs the identical bounds check and access.**
The discriminating criterion is decision 75's.
An operation repeated at thousands of call sites is resident in the artifact.
Such an operation moves into the shared unit, even when the unit then pays it once per call.

Concretely, the units live in [`runtime/ruby/units/memory/`](../../runtime/ruby/units/memory/) and [`runtime/python/units/memory/`](../../runtime/python/units/memory/).
The routing lives in `mem_call` in [`crates/dewasm-backend-ruby/src/lib.rs`](../../crates/dewasm-backend-ruby/src/lib.rs).
Python's routing is `mem_call` in [`crates/dewasm-backend-python/src/lib.rs`](../../crates/dewasm-backend-python/src/lib.rs).
The change has these parts:

- Each `a` unit is its `o` twin with the address line replaced.
  The line is `a = (a + b) & M32` in Ruby and `a = (a + b) & 0xFFFFFFFF` in Python.
  The bounds check and the access are byte-identical.
  The delegation topology mirrors the `o` family.
  `f32_loada` goes through the bit path; Python's sign-extending loads go through their unsigned twins.
- A site routes to the `a` form exactly when its IR offset is zero and its address is a dynamic add.
  A dynamic add is `Bin(I32Add, x, y)` with neither operand a constant.
  Both operands render in `Modular` context.
  Addition preserves congruence, and the unit reduces the sum (decision 76).
  So the `Modular` context is sound.
  Decision 71's bound guard applies per operand as before.
- The name is uniformly the one-argument name plus `a`, for add.
  `a` plus the `, ` between the arguments replaces the ` + ` at byte parity.
  That is decision 75's naming rule.
- A constant add operand keeps the one-argument unit (`i32_load(l0 + 4)`).
  The unit's reduction of the site's sum already implements the wrap.
  The constant must not migrate into an offset argument, whose addition does not wrap.
- A dynamic add under a nonzero IR offset keeps the `o` shape (`i32_loado(x + y, off)`).
  The sum wraps at the site's kept mask or inside the unit's base reduction.
  Then the offset adds exactly.
  The overlap was measured by grepping the converted artifacts.
  The grep counts `o`-family calls whose base argument contains an addition.
  The overlap is 441 of 345,398 `o` sites in merman and 282 of 50,711 in sqlite3-shell.
  That is too rare for a third family taking both a dynamic pair and an offset.

Measured on the converted sqlite3-shell (standalone Ruby) and merman, before to after.
Before is decision 77's state.
merman is converted with `--target ruby --mode library --no-default-wasi`.
ISeq is via `RubyVM::InstructionSequence.compile_file` on ruby 4.0.4 arm64-darwin, children included:

| Metric | sqlite3-shell before | sqlite3-shell after | merman before | merman after |
| --- | --- | --- | --- | --- |
| Sites routed to the `a` family | — | 1,888 | — | 6,498 |
| Source bytes | 7,731,846 | 7,732,176 | 47,712,065 | 47,707,217 |
| ISeq instructions | 1,287,815 | 1,286,257 | 6,850,276 | 6,844,192 |
| ISeq memsize (bytes) | 43,927,584 | 43,855,920 | 239,615,184 | 239,326,856 |

Source bytes stay at parity by the naming rule.
The resident ISeq shrinks by 0.12% (instructions) and 0.16% (memsize) on sqlite3-shell.
On merman it shrinks by 0.09% and 0.12%.

## Rejected alternatives

- **Reusing the `o` units for dynamic adds.**
  Unsound in both directions, as in the context.
  The offset addition must not wrap and the dynamic addition must.
- **A per-site `& 0xffffffff` around the sum.**
  That is the shape decision 76 just removed.
  The unit already reduces its address, so the site's wrap is redundant work resident at every site.
- **Delegating unit bodies** (`def i32_loada(a, b) = i32_load(a + b)`).
  One line either way, but the delegation adds a dynamic dispatch per call.
  That call is on the hottest path in the runtime.
  The mirrored body keeps the `a` family at exact cost parity with its `o` twin.
- **A three-argument family for a dynamic add under a nonzero offset.**
  The overlap measured above is under 0.6% of `o` sites in both artifacts; the current shape stays.

## Consequences

- Positive: the `opt_plus` and its operand shuffling leave the resident ISeq at every dynamic-add site.
  It is the same trade as decisions 75 and 76, at source-byte parity per site.
- Negative: a third unit spelling per load/store operation (plain, `o`, `a`).
  That is 23 more units per language, which must stay in lockstep with their twins.
- Carry-over: Perl can adopt the family with its own measurement.
  Bash, Go, and Java have nothing to adopt.
