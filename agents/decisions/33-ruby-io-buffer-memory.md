# Decision 33: `IO::Buffer`-Backed Linear Memory for Ruby

Status: **Accepted, 2026-07-28.**
Implemented in:

- `runtime/ruby/units/memory/*.rb`;
- `crates/dewasm-backend-ruby/src/lib.rs` (`find_ruby`);
- `docs/testing.md`;
- `AGENTS.md`.

## Context

The measurement ran a trivial script in a converted `ruby.wasm` (35MB) under the standalone runtime.
It took 63s total.
`stackprof` attributed 41% of samples to GC (39% of all samples in run alone).
It attributed ~9% self time to memory load/store operations.
The prior `Rt::Memory` represented linear memory as a single binary `String` (`@bytes`), changed in place.
Loads went via `String#unpack1`/`#getbyte`, and stores via `String#[]=`/`#setbyte`.
`String#unpack1` with a format string allocates the format's intermediate Array.
It does so before returning the scalar.
That is 1 allocation per load.
Byte-range assignment (`@bytes[a, n] = ...`) allocates the packed source string.
Inside, it also allocates new storage for the changed string, so a store costs 2 allocations.
At the density of WASI system calls or C API calls, this dominates GC pressure.
That density means every `fd_write`, every SQLite row, every `printf`.

Ruby's `IO::Buffer` wraps a raw native memory region.
It has been stable since 3.0, and marked experimental through recent releases.
It exposes typed `get_value`/`set_value` accessors.
They read/write directly without an intermediate Ruby object.
They measured 2-3x faster, with zero allocations, for scalar loads/stores on Ruby 4.0.4.
That is the version available in this environment.

## Decision

- **Linear memory is `IO::Buffer`-backed.**
  `Rt::Memory#initialize` allocates `@buffer = IO::Buffer.new(@size)`.
  `@size` is tracked as a plain instance variable, updated on `grow`.
  That is because `IO::Buffer` has no cheap size read worth relying on for the hot bounds check.
  `attr_reader :buffer` replaces the former `attr_reader :bytes`.
- **The experimental warning is silenced around allocation only.**
  The code saves and restores `Warning[:experimental]`.
  It is `false` for the `IO::Buffer.new` call and restored in an `ensure` immediately after.
  This matters beyond looks.
  The `qjs` pseudo-terminal snapshot captures standard error unchanged.
  An experimental warning left on would break it.
  That snapshot is in `crates/dewasm-backend-ruby/tests/e2e.rs`.
- **Explicit bounds checks are kept as the trap mechanism, not `IO::Buffer`'s `ArgumentError`.**
  `Rt::Memory#check(addr, len)` still raises `Rt.trap("out of bounds memory access")`.
  That is the exact message the specification harness's assertions match.
  They are in `address.wast`, `memory_trap.wast`, `align.wast`, and `traps.wast`.
  In the one-liner load/store units the bounds test is inlined rather than calling `check`.
  The inlined test is `Rt.trap(...) if a + N > @size`.
  It trades one dispatch for a `# requires: rt/trap` header.
  `copy`/`fill`/`init`/`read_string` keep calling `check`, since they are not single-expression hot paths.
- **Signed narrow loads read the signed view directly and mask.**
  That replaces the previous two-step `Rt.sext(unsigned_load, bits, mask)`.
  `i32_load8_s` is `get_value(:S8, a) & M32`, `i64_load16_s` is `get_value(:s16, a) & M64`, etc.
  `IO::Buffer` names 8-bit types in capitals only (`:U8`/`:S8`).
  Multi-byte lower-case names (`:u16`/`:s16`/`:u32`/`:s32`/`:u64`/`:s64`) are little-endian.
  That matches wasm's memory byte order directly.
  No explicit `<` suffix is needed the way `Array#pack`/`String#unpack1` required (`"L<"`, `"S<"`).
- **`f32_load`/`f32_store` are unchanged.**
  They are still routed through the `i32` bit path to preserve NaN sign/payload per decision 2.
  The path is `Rt.f32_from_bits(i32_load(a))` / `i32_store(a, Rt.f32_bits(v))`.
  They get `IO::Buffer`'s gain in speed through `i32_load`/`i32_store`.
  `f64_load`/`f64_store` switch to `get_value(:f64, a)`/`set_value(:f64, a, v)` directly.
  An 8-byte little-endian IEEE double is bit-preserving with no host path that loses bits.
  That is unlike the f32 double-rounding concern decision 2 documents.
- **`grow` uses `@buffer.resize(@size)`** (after raising `@size`).
  `IO::Buffer#resize` zero-fills the new tail, matching wasm's `memory.grow` semantics for free.
  **`copy` uses `@buffer.copy(@buffer, dst, len, src)`.**
  `IO::Buffer#copy` has `memmove` overlap semantics.
  That matches wasm's `memory.copy`, which is defined for overlapping regions.
  The old code needed a `String#byteslice` fix for that.
  **`fill` uses `@buffer.clear(val & 0xff, dst, len)`.**
  **`init`** uses `@buffer.set_string(data, dst, len, src)`.
  `init` is also used for active data-segment instantiation.
  It keeps its existing out-of-bounds-source trap ahead of the bounds `check`.
  **`read_string`** uses `@buffer.get_string(ptr, len)`.
- **Ruby >= 3.4 is now the floor**, checked in `find_ruby()` (`crates/dewasm-backend-ruby/src/lib.rs`).
  It runs `ruby -e 'print RUBY_VERSION'` and rejects anything below `3.4`.
  It is modeled on `dewasm_backend_bash::find_bash5`'s fail-loud version test.
  Decision 15 says to fail loud with a set-up instruction, never silently skip.
  `docs/testing.md` and `AGENTS.md` were updated to state the floor and its reason.

## Rejected alternatives

- **Keep `String` + `pack`/`unpack1` (as before)**, the measured baseline.
  It costs 1 allocation per load, 2 per store, and 41% GC time (39% run) on the ruby.wasm benchmark.
  Rejected on measured evidence, not a guess.
- **A byte `Array`**: trades one allocation problem for a worse one.
  Its elements are boxed `Integer`s, using far more memory than a packed `String`.
  It was never seriously considered, given `IO::Buffer` exists.
- **Rescue `IO::Buffer`'s `ArgumentError` as the bounds-trap mechanism.**
  `IO::Buffer` does raise `ArgumentError` on out-of-range access.
  That could in principle replace the explicit `check`.
  It was rejected for two reasons.
  First, the specification harness fixes an exact trap *message*, `"out of bounds memory access"`.
  `ArgumentError`'s wording does not produce it.
  Second, relying on the accessor's own bounds behavior would couple correctness to it.
  Correctness would then depend on an experimental API's error text.
  Correctness should rest on a rule dewasm controls instead.
- **Direct `:f32` loads/stores via `IO::Buffer`.**
  `get_value(:f32, ...)` widens the packed 32-bit float to a Ruby `Float` (host double).
  The widening is a conversion in the processor.
  That conversion quiets signaling NaNs in the widening step.
  Decision 2 requires bit-exact NaN payload preservation through `reinterpret` and memory traffic.
  Only the existing bit-pattern path satisfies that, so f32 memory operations keep the indirection.
  That path is `Rt.f32_from_bits`/`Rt.f32_bits` over `i32_load`/`i32_store`.

## Consequences

- Positive: eliminates 1 allocation per scalar load and 2 per scalar store.
  `IO::Buffer#get_value`/`#set_value` measured zero allocations on Ruby 4.0.4.
  That was the largest single cause of the measured GC/run dominance in the ruby.wasm benchmark.
- **Measured, with two other changes landed alongside it.**
  They are the depth-1 `br`/`next` (decision 4) and global-unboxing (decision 16) changes.
  All three are in the same `crates/dewasm-backend-ruby/src/lib.rs` change series.
  So isolating this change's wall-clock share alone wasn't practical.
  `stackprof`'s GC line attributes cleanly to this one; the two others' lines don't.
  The run converted `ruby.wasm` (35MB) to the standalone Ruby runtime.
  It ran `ruby out.rb --dir <ruby-lib>::/usr -- -e 'puts "hello #{6*7}"'`.
  The machine was the same as for the original 63s baseline.
  Each side was averaged over two runs.
  **Before: 67.2s wall / 63.1s user.**
  **After: 53.6s wall / 51.5s user (~1.26x wall-clock, ~18% reduction in absolute terms).**
  A fresh `stackprof` capture on the *after* build shows GC collapsed from the baseline's 41% (39% run).
  It is now **0.93%** (0.5% marking + 0.4% running) of samples.
  The memory representation was in fact the dominant GC driver, exactly as expected.
  The wall-clock gain is smaller than the GC collapse alone would suggest, for two reasons.
  First, `Kernel#catch`/`#throw` now accounts for ~18.6% of samples and dominates what's left.
  It is still used for every `br` whose target isn't the nearest frame.
  Decision 4's depth-1 optimization doesn't reach multi-level branches.
  Second, raw Ruby method-call dispatch is the CPU-bound floor under all of it.
  That dispatch is one method per wasm function and one line per wasm instruction.
  GC no longer limits speed; control-flow dispatch does.
- Positive: `copy`/`fill` gain native `memmove`/`memset`-equivalent implementations.
  They replace Ruby-level string slicing and reassembly.
- Negative: Ruby >= 3.4 is now a hard requirement to run *any* generated Ruby output.
  It is not just a requirement to develop dewasm itself.
  That is a real constraint for embedders on older Ruby.
  It is accepted because `IO::Buffer`'s typed accessors are unavailable earlier.
  So is their measured performance.
  `IO::Buffer` itself remains marked experimental upstream.
  The warning save/restore above reduces that.
  The API could still change in a future Ruby release we'd need to track, though.
- Carry-over: `IO::Buffer` is explicitly experimental in the Ruby API itself.
  A future Ruby release may change its interface.
  The units under `runtime/ruby/units/memory/` are then the only integration point to update.

See also:

- [decision 2](2-numeric-semantics.md): masked-unsigned integers, f32 re-rounding, NaN bit paths this preserves;
- [decision 6](6-runtime-units.md): per-method unit / `# requires:` convention;
- [decision 15](15-tests-fail-not-skip.md): fail-loud version conditioning this follows for `find_ruby`.
