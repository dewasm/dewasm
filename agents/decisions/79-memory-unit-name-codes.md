# Decision 79: Single-Character Codes for the Memory Load/Store Unit Names

Status: **Accepted, 2026-08-16.**
It landed for Ruby and Python.
Every memory load/store unit is named by a code matching `[iuf][bhwd][ls][bhwd]?[oa]?`.
The emitters read the codes from `load_code`/`store_code`.
Those live in [`crates/dewasm-backend/src/lib.rs`](../../crates/dewasm-backend/src/lib.rs).
Go, Java, and Perl keep the wasm-spelled names (`load_method`/`store_method`).
Bash keeps its own table.
Each can adopt the codes with its own measurement.

## Context

Memory load/store calls dominate a converted artifact's source.
The converted `merman` (`--target ruby --mode library --no-default-wasi`) has 433,438 such call sites.
Their method names alone (`i32_load`, `i64_load32_uo`, …) take 4,073,915 bytes.
That is 8.5% of the whole 47.7 MB file.
Decisions 75 and 78 already chose one-letter `o`/`a` suffixes for their two-argument twins.
The suffixes keep source size the same per site.
The base names stayed at the wasm spelling, eight to twelve characters each.
The names are interior to the artifact.
The generated code and the bundled runtime are the only callers.
The one addition is the host glue a user writes against `instance.memory`.

## Decision

**A name that appears once per call site in an artifact of several MB is priced in bytes.
It gets the shortest spelling that still encodes every distinction the unit family needs.
Readability belongs to names read where they are defined.
These names are read at generated call sites.**

Each load/store unit is named by a code, one character per distinction:

1. Value kind: `i` integer, `u` a zero-extending narrow load, `f` float.
   `u` exists only where wasm distinguishes signedness (the narrow loads).
   Full-width loads and every store use the type's own `i`/`f`.
2. Value width: `b`/`h`/`w`/`d` for 8/16/32/64 bits.
   This is the width of the value the unit produces or consumes.
   So `i32_load8_s` has value width `w`.
3. Operation: `l` load, `s` store.
4. Memory width for the narrow operations, same `b`/`h`/`w`/`d` codes.
   It is missing when it equals the value width.
5. Added last by `mem_call`: `o` for the static-offset twin (decision 75).
   `a` is for the wrapping-add twin (decision 78).
   The suffix is missing for the one-argument form.

The table below is the full base mapping.
It is applied in [`runtime/ruby/units/memory/`](../../runtime/ruby/units/memory/) and [`runtime/python/units/memory/`](../../runtime/python/units/memory/).
Each row also renames its `o` and `a` twins, for example `i32_loado` → `iwlo`.

| Wasm operation | Code | Wasm operation | Code |
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

The other memory units keep their names.
`copy` (9,064 `merman` sites) is already as short as a code.
`fill` (129), `init` (489), `grow` (1), `size`, and `read_string` are too rare for a rename to buy anything.
`read_string` is additionally the host-glue API the documents teach.

Measured on the converted `sqlite3-shell` (standalone Ruby) and `merman` (as above), before to after.
Before is decision 78's state.
ISeq is via `RubyVM::InstructionSequence.compile_file` on Ruby 4.0.4 `arm64-darwin`, children included:

| Measure | `sqlite3-shell` before | `sqlite3-shell` after | `merman` before | `merman` after |
| --- | --- | --- | --- | --- |
| Source bytes | 7,732,176 | 7,295,696 | 47,707,217 | 45,315,890 |
| Load/store method-name bytes | — | — | 4,073,915 | 1,683,020 |
| ISeq instructions | 1,286,867 | 1,286,867 | — | — |

A rename cannot change the compiled instruction stream, and the unchanged ISeq count confirms it.
The saving is source bytes, 5.6% of `sqlite3-shell` and 5.0% of `merman`.

## Rejected alternatives

- **Keep the wasm spellings (the current state).**
  4.07 MB of method-name bytes on `merman`, repeated in every future artifact.
- **Readable short names** (`ld32`, `st8u`, …).
  Every character above the minimum repeats 433k times on `merman`, and buys nothing back.
  Once the name is not the wasm spelling, the reader consults the scheme either way.
- **Codes for `copy`/`fill`/`init`/`grow`/`size`/`read_string` too.**
  The site counts above are three orders of magnitude below the load/store family's.
  `read_string` is user-facing.
- **Codes for the other backends in the same change.**
  The motivation is measured on Ruby and Python artifacts.
  Go, Java, Perl, and Bash adopt with their own measurement or not at all.
  That is the same boundary as decisions 75, 76, and 78.

## Consequences

- Positive: 2.4 MB off `merman` and 436 KB off `sqlite3-shell` at zero semantic and zero runtime cost.
  Every future load/store site is born about five bytes cheaper.
- Negative: generated call sites and host glue read as codes (`mem.iwl(p)`).
  The glue examples in the documents carry a one-line decoding comment.
  The scheme above is the reference.
- Carry-over: the `o`/`a` suffix rule of decisions 75 and 78 composes with the codes unchanged.
  A backend adopting those decisions later can take the codes in the same step.
