# Decision 75: Pass the Static Load/Store Offset as a Second Argument

Status: **Accepted, 2026-08-15.** Landed for Ruby and Python: each memory load/store unit has an `o`-suffixed twin taking the static offset as an argument, and the emitters select between the two per site. Bash inlines its memory operations (decision 52) and Go/Java compile the addition natively, so nothing applies there; Perl keeps the per-site addition for now.

## Context

A wasm load/store instruction carries a static offset immediate, and the IR keeps it (`Expr::Load { op, addr, offset }`, `Stmt::Store { op, addr, value, offset }` in [`crates/dewasm-core/src/ir.rs`](../../crates/dewasm-core/src/ir.rs)).
The Ruby and Python backends rendered it as a per-site addition: `@m.i32_load(l17 + 12)`.

That addition dominates the artifact.
On the converted sqlite3-shell, 50,711 of 75,845 load/store call sites (67%) carry a nonzero offset, and each pays one `opt_plus` instruction plus its call data in the resident ISeq.
A mechanical transformation to a two-argument form measured ISeq instructions 1,359,849 to 1,311,311 (3.57% fewer) and ISeq memsize 47,196,232 to 44,854,280 bytes (4.96% smaller).
A microbenchmark with the real unit shape (bounds check included, `IO::Buffer`, 20M calls) measured the per-call cost of the extra argument: interpreter 1.07 s inline vs 1.13 s two-argument (+6% per call, about 3 ns); under `--yjit` 0.657 s vs 0.648 s (neutral).

## Decision

Resident code size and the JIT path outrank the interpreter path: a cost paid once per call site in a multi-megabyte artifact moves into the shared runtime units, and the interpreter-only per-call cost is accepted because JIT execution is the common case.

Concretely, per site:

- Offset zero: the one-argument unit, unchanged. It must not grow an optional parameter, so the 25k offset-zero sites on sqlite3-shell pay nothing.
- Constant base: fold base + offset into one literal at conversion time. Wasm's effective address does not wrap, so the folded literal may exceed 32 bits and the unit's bounds check still applies unchanged. (The case does not occur in sqlite3-shell at all; it is folded because calling the two-argument unit with two constants would be strictly worse than one folded literal.)
- Otherwise: the two-argument unit, `i32_loado(a, off)`; a store takes the value last, `i32_storeo(a, off, v)`.
  The name is uniformly the one-argument name plus `o`, for offset (so `i64_store32` gets `i64_store32o`): `o` plus the `, ` between the arguments is exactly the three bytes of the ` + ` it replaces, keeping source size neutral per site.

Inside a unit the effective address is computed once (`a += off`), then the existing bounds check and buffer access run unchanged; Ruby and Python integer addition is exact, so the check needs no adjustment.
The selection lives in `mem_call` in [`crates/dewasm-backend-ruby/src/lib.rs`](../../crates/dewasm-backend-ruby/src/lib.rs) and [`crates/dewasm-backend-python/src/lib.rs`](../../crates/dewasm-backend-python/src/lib.rs); the units are in [`runtime/ruby/units/memory/`](../../runtime/ruby/units/memory/) and [`runtime/python/units/memory/`](../../runtime/python/units/memory/) (decision 6), and only the units a module uses get bundled, as before.

Measured on sqlite3-shell after landing (Ruby, ruby 4.0.4 arm64-darwin):

| Metric | Before | After | Delta |
| --- | --- | --- | --- |
| Source bytes | 7,868,630 | 7,870,302 | +0.02% |
| ISeq instructions | 1,360,259 | 1,309,998 | -3.7% |
| ISeq memsize (bytes) | 47,212,640 | 44,792,656 | -5.1% |
| Workload, plain (s user) | 5.91 | 6.07 | +2.7% |
| Workload, `--yjit` (s user) | 3.35 | 3.30 | -1.5% |

The workload is a recursive CTE inserting 30,000 rows, aggregates over them, and an unindexed self-join; times are the stable user-CPU medians of 5 alternating runs.
Source bytes stay neutral by the naming rule above, while the resident ISeq, which is what stays in memory, shrinks.

## Rejected alternatives

- **Status quo (per-site inline addition).** Keeps 50k `opt_plus` instructions and their call data resident, and the measured ISeq savings on the table.
- **An optional parameter on the existing units** (`def i32_load(a, off = 0)`). One unit instead of two, but every offset-zero site then pays the optional-argument setup; the 25k offset-zero sites on sqlite3-shell must stay exactly as fast as today.
- **Folding the addition into the unit's `get_value`/`check` call sites** (`check(a + off, 4)` then `get_value(:u32, a + off)`). Computes the effective address twice per call; `a += off` computes it once and leaves the rest of the unit body identical to its one-argument twin.
- **A descriptive `_offset` suffix for the twin's name.** Measured +3.9% source bytes on sqlite3-shell (7,868,630 to 8,174,700), six extra bytes per site times 50k sites; the `o` suffix carries the same information at byte parity.
- **A `2` suffix (for the twin's arity).** The same byte parity, but many unit names already end in a size digit, and the digit suffix collides with it: `i32_store82` and `i32_store162` no longer read as `i32_store8` and `i32_store16` plus a marker.

## Consequences

- Positive: 3.7% fewer resident ISeq instructions and 5.1% less ISeq memory on sqlite3-shell, with a small `--yjit` speedup; the same shape lands in Python's bytecode.
- Negative: the plain-interpreter path is 2.7% slower on the sqlite3-shell workload (accepted above), and the one-letter `o` names read less plainly than a spelled-out suffix would.
- Carry-over: 23 two-argument units per language mirror their one-argument twins and must stay in lockstep with them; Perl can adopt the same shape later with the same measurements.
