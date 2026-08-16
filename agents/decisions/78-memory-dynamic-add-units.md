# Decision 78: Wrapping-Add Memory Units for Dynamic Addresses

Status: **Accepted, 2026-08-16.** Landed for Ruby and Python: each memory load/store unit has an `a`-suffixed twin taking a dynamic `i32.add` address as two arguments and wrapping their sum, and the emitters route offset-zero dynamic-add sites to it. Bash inlines its memory operations (decision 52), Go and Java compile the addition natively; Perl keeps the per-site arithmetic until it adopts the family with its own measurement.

## Context

After decision 76 the emitters render a dynamic address bare and the unit reduces it: `@m.i32_load(l0 + l1)`.
The site still pays the addition, one `opt_plus` and its call data resident in the ISeq per site.
Decision 75 moved the static-offset addition into the units; the dynamic addition is the per-site arithmetic that remains at load/store sites.

The `o` units cannot absorb it, because the two additions have incompatible semantics.
Wasm's effective address is the base reduced modulo 2^32 plus the static offset *without* further reduction: `address.wast` requires a trap when base plus offset leaves u32, so `i32_loado` adds exactly (decision 76).
A dynamic `i32.add` feeding an address must *wrap* its sum: `memory_trap.wast` stores at `memory.size * 0x10000 + (-4)` and requires success, the wrapped sum landing back in bounds.
One unit cannot do both: routing a dynamic add through the `o` form would trap where wasm wraps, and making the `o` form wrap would succeed where wasm traps.

## Decision

**Dynamic-add addresses get their own unit family: `i32_loada(a, b)` computes `(a + b) & 0xffffffff` and then runs the identical bounds check and access.**
The discriminating criterion is decision 75's: an operation repeated at thousands of call sites, resident in the artifact, moves into the shared unit even when the unit then pays it once per call.

Concretely, in [`runtime/ruby/units/memory/`](../../runtime/ruby/units/memory/) and [`runtime/python/units/memory/`](../../runtime/python/units/memory/), and in `mem_call` in [`crates/dewasm-backend-ruby/src/lib.rs`](../../crates/dewasm-backend-ruby/src/lib.rs) and [`crates/dewasm-backend-python/src/lib.rs`](../../crates/dewasm-backend-python/src/lib.rs):

- Each `a` unit is its `o` twin with the address line replaced, `a = (a + b) & M32` in Ruby and `a = (a + b) & 0xFFFFFFFF` in Python; the bounds check and the access are byte-identical, and the delegation topology (`f32_loada` through the bit path, Python's sign-extending loads through their unsigned twins) mirrors the `o` family.
- A site routes to the `a` form exactly when its IR offset is zero and its address is `Bin(I32Add, x, y)` with neither operand a constant; both operands render in `Modular` context, sound because addition preserves congruence and the unit reduces the sum (decision 76), with decision 71's bound guard applying per operand as before.
- The name is uniformly the one-argument name plus `a`, for add: `a` plus the `, ` between the arguments replaces the ` + ` at byte parity, decision 75's naming rule.
- A constant add operand keeps the one-argument unit (`i32_load(l0 + 4)`): the unit's reduction of the site's sum already implements the wrap, and the constant must not migrate into an offset argument, whose addition does not wrap.
- A dynamic add under a nonzero IR offset keeps the `o` shape (`i32_loado(x + y, off)`): the sum wraps at the site's kept mask or inside the unit's base reduction, then the offset adds exactly.
  Measured by grepping the converted artifacts for `o`-family calls whose base argument contains an addition, the overlap is 441 of 345,398 `o` sites in merman and 282 of 50,711 in sqlite3-shell, too rare for a third family taking both a dynamic pair and an offset.

Measured on the converted sqlite3-shell (standalone Ruby) and merman (`--target ruby --mode library --no-default-wasi`), before (decision 77's state) to after; ISeq via `RubyVM::InstructionSequence.compile_file` on ruby 4.0.4 arm64-darwin, children included:

| Metric | sqlite3-shell before | sqlite3-shell after | merman before | merman after |
| --- | --- | --- | --- | --- |
| Sites routed to the `a` family | — | 1,888 | — | 6,498 |
| Source bytes | 7,731,846 | 7,732,176 | 47,712,065 | 47,707,217 |
| ISeq instructions | 1,287,815 | 1,286,257 | 6,850,276 | 6,844,192 |
| ISeq memsize (bytes) | 43,927,584 | 43,855,920 | 239,615,184 | 239,326,856 |

Source bytes stay at parity by the naming rule; the resident ISeq shrinks by 0.12% (instructions) and 0.16% (memsize) on sqlite3-shell and 0.09% and 0.12% on merman.

## Rejected alternatives

- **Reusing the `o` units for dynamic adds.**
  Unsound in both directions, as in the context: the offset addition must not wrap and the dynamic addition must.
- **A per-site `& 0xffffffff` around the sum.**
  That is the shape decision 76 just removed; the unit already reduces its address, so the site's wrap is redundant work resident at every site.
- **Delegating unit bodies** (`def i32_loada(a, b) = i32_load(a + b)`).
  One line either way, but the delegation adds a dynamic dispatch per call on the hottest path in the runtime; the mirrored body keeps the `a` family at exact cost parity with its `o` twin.
- **A three-argument family for a dynamic add under a nonzero offset.**
  The overlap measured above is under 0.6% of `o` sites in both artifacts; the current shape stays.

## Consequences

- Positive: the per-site `opt_plus` and its operand shuffling leave the resident ISeq at every dynamic-add site, the same trade as decisions 75 and 76, at source-byte parity per site.
- Negative: a third unit spelling per load/store operation (plain, `o`, `a`), 23 more units per language that must stay in lockstep with their twins.
- Carry-over: Perl can adopt the family with its own measurement; Bash, Go, and Java have nothing to adopt.
