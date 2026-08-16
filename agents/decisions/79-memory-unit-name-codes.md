# Decision 79: Single-Character Codes for the Memory Load/Store Unit Names

Status: **Accepted, 2026-08-16.** Landed for Ruby and Python: every memory load/store unit is named by a code matching `[iuf][bhwd][ls][bhwd]?[oa]?`, and the emitters read the codes from `load_code`/`store_code` in [`crates/dewasm-backend/src/lib.rs`](../../crates/dewasm-backend/src/lib.rs). Go, Java, and Perl keep the wasm-spelled names (`load_method`/`store_method`), and Bash keeps its own table; each can adopt the codes with its own measurement.

## Context

Memory load/store calls dominate a converted artifact's source.
The converted merman (`--target ruby --mode library --no-default-wasi`) has 433,438 such call sites, and their method names alone (`i32_load`, `i64_load32_uo`, …) take 4,073,915 bytes, 8.5% of the whole 47.7 MB file.
Decisions 75 and 78 already chose one-letter `o`/`a` suffixes for their two-argument twins to hold source size at parity per site; the base names stayed at the wasm spelling, eight to twelve characters each.
The names are interior to the artifact: the generated code and the bundled runtime are the only callers, plus the host glue a user writes against `instance.memory`.

## Decision

**A name that recurs once per call site in a multi-megabyte artifact is priced in bytes: it gets the shortest spelling that still encodes every distinction the unit family needs.
Readability belongs to names read where they are defined, and these are read at generated call sites.**

Each load/store unit is named by a code, one character per distinction:

1. Value kind: `i` integer, `u` a zero-extending narrow load, `f` float.
   `u` exists only where wasm distinguishes signedness (the narrow loads); full-width loads and every store use the type's own `i`/`f`.
2. Value width: `b`/`h`/`w`/`d` for 8/16/32/64 bits.
   This is the width of the value the unit produces or consumes, so `i32_load8_s` has value width `w`.
3. Operation: `l` load, `s` store.
4. Memory width for the narrow operations, same `b`/`h`/`w`/`d` codes; absent when it equals the value width.
5. Appended by `mem_call`: `o` for the static-offset twin (decision 75), `a` for the wrapping-add twin (decision 78); absent for the one-argument form.

The full base mapping, applied in [`runtime/ruby/units/memory/`](../../runtime/ruby/units/memory/) and [`runtime/python/units/memory/`](../../runtime/python/units/memory/) (each row also renames its `o` and `a` twins, `i32_loado` → `iwlo`):

| Wasm op | Code | Wasm op | Code |
| --- | --- | --- | --- |
| `i32_load` | `iwl` | `i32_store` | `iws` |
| `i64_load` | `idl` | `i64_store` | `ids` |
| `f32_load` | `fwl` | `f32_store` | `fws` |
| `f64_load` | `fdl` | `f64_store` | `fds` |
| `i32_load8_s` | `iwlb` | `i32_store8` | `iwsb` |
| `i32_load8_u` | `uwlb` | `i32_store16` | `iwsh` |
| `i32_load16_s` | `iwlh` | `i64_store8` | `idsb` |
| `i32_load16_u` | `uwlh` | `i64_store16` | `idsh` |
| `i64_load8_s` | `idlb` | `i64_store32` | `idsw` |
| `i64_load8_u` | `udlb` | | |
| `i64_load16_s` | `idlh` | | |
| `i64_load16_u` | `udlh` | | |
| `i64_load32_s` | `idlw` | | |
| `i64_load32_u` | `udlw` | | |

The other memory units keep their names: `copy` (9,064 merman sites) is already as short as a code, and `fill` (129), `init` (489), `grow` (1), `size`, and `read_string` are too rare for a rename to buy anything; `read_string` is additionally the host-glue API the docs teach.

Measured on the converted sqlite3-shell (standalone Ruby) and merman (as above), before (decision 78's state) to after; ISeq via `RubyVM::InstructionSequence.compile_file` on ruby 4.0.4 arm64-darwin, children included:

| Metric | sqlite3-shell before | sqlite3-shell after | merman before | merman after |
| --- | --- | --- | --- | --- |
| Source bytes | 7,732,176 | 7,295,696 | 47,707,217 | 45,315,890 |
| Load/store method-name bytes | — | — | 4,073,915 | 1,683,020 |
| ISeq instructions | 1,286,867 | 1,286,867 | — | — |

A rename cannot change the compiled instruction stream, and the unchanged ISeq count confirms it; the saving is source bytes, 5.6% of sqlite3-shell and 5.0% of merman.

## Rejected alternatives

- **Keep the wasm spellings (status quo).** 4.07 MB of method-name bytes on merman, recurring at every future artifact.
- **Readable abbreviations** (`ld32`, `st8u`, …). Every character above the minimum recurs 433k times on merman, and buys nothing back: once the name is not the wasm spelling, the reader consults the scheme either way.
- **Codes for `copy`/`fill`/`init`/`grow`/`size`/`read_string` too.** The site counts above are three orders of magnitude below the load/store family's, and `read_string` is user-facing.
- **Codes for the other backends in the same change.** The motivation is measured on Ruby and Python artifacts; Go, Java, Perl, and Bash adopt with their own measurement or not at all, the same boundary as decisions 75, 76, and 78.

## Consequences

- Positive: 2.4 MB off merman and 436 KB off sqlite3-shell at zero semantic and zero runtime cost; every future load/store site is born about five bytes cheaper.
- Negative: generated call sites and host glue read as codes (`mem.iwl(p)`); the docs' glue examples carry a one-line decoding comment, and the scheme above is the reference.
- Carry-over: the `o`/`a` suffix rule of decisions 75 and 78 composes with the codes unchanged; a backend adopting those decisions later can take the codes in the same step.
