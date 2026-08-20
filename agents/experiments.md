# Experiments

Past experiments: what was tried, what came of it, and where the full record lives.
The full record (logs, measurements, discussion) stays in the experiment's Issue or PR; each entry here carries the conclusion itself, so reading this index needs no network access.
An entry earns its place only if it changes what a future agent would do: it stops a re-proposal, or records a measured limit of an approach.

Each entry is one section in this shape:

```markdown
## <slug>: <one-line conclusion> (<date>)

- **Tried**: what was attempted, in one or two sentences.
- **Verdict**: the outcome in one sentence, with the deciding number.
- **Invalidated when**: what would make this worth re-testing.
- **Details**: #<issue> / PR #<pr>.
```

## lua-php-guests: neither interpreter builds for wasm 1.0 (2026-08-02)

- **Tried**: lua.wasm and php.wasm as example guests.
- **Verdict**: rejected; Lua's setjmp/longjmp needs the exception-handling proposal (out of scope) and still hits a wasm-ld bug when enabled, while PHP's wasm build line stopped in 2023-07 with setjmp removed.
- **Invalidated when**: a Lua or PHP build appears that targets plain wasm 1.0 + WASI p1 without setjmp.
- **Details**: #204.

## br-table-dispatch: Ruby's case/when on integer literals is already O(1) (2026-08-02)

- **Tried**: replacing br_table's case/when with a binary if tree, suspecting linear when-matching.
- **Verdict**: rejected; YARV compiles integer-literal when clauses to opt_case_dispatch (a hash), and the tree measured slower (0.149 s vs 0.118 s over 2M dispatches).
- **Invalidated when**: the dispatch arms stop being integer literals, which falls off opt_case_dispatch.
- **Details**: #203.

## i64-signed: signed two's-complement i64 loses to masked-unsigned in Ruby (2026-08-03)

- **Tried**: representing i64 as signed two's complement instead of the shared masked-unsigned convention; full spec pass on the preserved `i64-signed` branch.
- **Verdict**: rejected; i64_alu regressed to 0.86-0.93x (rest 0.97-1.03x), because hot i64 values in real apps are almost all non-negative, so masked-unsigned rarely boxes while the signed form pays its wrap branch everywhere.
- **Invalidated when**: a negative-value-heavy workload matters (the signed form won 2.2x on synthetic negative-heavy code), or Ruby's integer boxing boundary changes.
- **Details**: #107.

## mem-inline: inlining Ruby's memory wrappers regresses (2026-08-03)

- **Tried**: emitting linear-memory loads and stores inline instead of through the wrapper methods; preserved on the `mem-inline` branch.
- **Verdict**: rejected; mem_rw under YJIT measured 0.79x, because the cost is the bounds check and `IO::Buffer#get_value` itself, not method dispatch.
- **Invalidated when**: bounds-check removal becomes robust (PR #109's commit 4 shape; blocked on brittle `ArgumentError#message` matching across Ruby versions).
- **Details**: #108 / PR #109.

## function-dedup: identical generated functions are already merged upstream (2026-08-06)

- **Tried**: deduplicating identical Ruby function bodies to shrink the resident ISeq, sized on the 30 MB merman output.
- **Verdict**: no win; wasm-opt preprocessing already merges duplicates, leaving 2 among 7,475 functions.
- **Invalidated when**: modules reach the emitter without wasm-opt preprocessing.
- **Details**: #202.

## memory-delegate: bare delegated memory calls slow the hot path (2026-08-06)

- **Tried**: `@memory.i32_load` to a bare delegated `i32_load` at 246k call sites, to cut the receiver read.
- **Verdict**: rejected; a microbenchmark measured +27% (getivar+send becomes putself+send+send) for an ISeq saving of ~1.5%.
- **Invalidated when**: the unmeasured bmethod variant (`define_method(:i32_load, @memory.method(:i32_load))`, one frame) gets measured and wins.
- **Details**: #202.

## mask-omission-ceiling: dropping provable i32 masks caps near -5% of ISeq (2026-08-06)

- **Tried**: sizing the win from omitting `& 0xffffffff` where the result provably fits i32, on the merman output.
- **Verdict**: low ceiling; of 131.6k masks only ~12% are droppable at the expression site, the rest need per-local dataflow, and masks are ~6% of all instructions, so the ISeq ceiling is about -5 to -6%.
- **Invalidated when**: an IR-level range analysis lands (it would also serve Perl and Python), or the always-masked invariant is relaxed to observation-point masking.
- **Details**: #202.

## spinel-aot: bigint-typed i64 and compile-time scaling defeat Spinel today (2026-08-13)

- **Tried**: compiling the Ruby backend's output with Spinel (matz's AOT Ruby-to-C compiler), probing its subset and scaling with generated-code shapes.
- **Verdict**: premature; any value or literal at or above 2^63 (the M64 mask constant included) types as bigint and erases the win (a masked u64 loop measured 1.0x vs CRuby, signed int64 under `--int-overflow=wrap` is effectively free), and the front end scales worse than quadratically (391 KB of Ruby: 12 s front end, 113 s total at `-O2`), putting sqlite3-shell's 7.9 MB out of reach.
- **Invalidated when**: Spinel's compile-time scaling improves (it is pre-0.1 and moving fast) and a Spinel output profile exists: signed i64 (the `i64-signed` branch) plus an `IO::Buffer` replacement.
- **Details**: #205.

## jvm-ruby-runtimes: generated methods exceed method-granularity JIT limits (2026-08-13)

- **Tried**: running the Ruby backend's output on TruffleRuby 33.0.1 (pure-Ruby `IO::Buffer` polyfill) and JRuby 10.0.3.0 (an `IO::Buffer` arity shim), microbenchmarks and apps.
- **Verdict**: rejected as suite runners; both beat YJIT on microbenchmarks (TruffleRuby's f64_alu at 4x wasmtime vs YJIT's 78x) yet lose on real apps (sqlite3_query: JRuby 58-70 s vs YJIT 9.4 s; TruffleRuby unfinished after 48 min), because the largest generated methods (~13k lines) exceed the JVM's 64 KB per-method bytecode limit (JRuby raises MethodTooLargeException once `-Xjit.maxsize` allows the attempt), so the hottest functions stay interpreted.
- **Invalidated when**: a pass caps generated method size by splitting functions (also relevant to the Java backend's 64 KB constraint), or JRuby's `IO::Buffer` gains the four-argument `copy`/`set_string` forms and TruffleRuby gains `IO::Buffer` at all.
- **Details**: #206.

## step-lambda-dispatch: wrapping a flat dispatch loop in a per-batch lambda loses under every JIT configuration (2026-08-21)

- **Tried**: emitting a flat-dispatch function's `case state` inside `__step = lambda do ... end` so the states run in a closure invoked repeatedly (once per transition, then batched at 1024 transitions per call), which YJIT compiles even though the enclosing function is entered once; measured on sqlite3-shell's interpreter function (453 states, 34.6% self time in the query workload profile).
- **Verdict**: rejected; per-transition calls measured 83 M JIT-boundary crossings and 9.51 s to 10.21 s under `--yjit`, and batching only recovered to 9.96 s (interpreter 19.63 s to 20.67 s), because the surviving costs are closure-environment variable access (every local becomes a heap-environment slot with a write barrier) and the compiled size of a 453-state method (12.4 MB of generated machine code), the same large-compiled-method loss optcarrot's generated core shows against its small-method core.
- **Invalidated when**: YJIT gains on-stack replacement (the whole workaround becomes unnecessary), or compiled closure-environment access stops costing more than the interpreter saves.
- **Details**: measurements in this experiment predate an issue; the step emission itself was reverted and only this entry records it.

## jit-coverage-per-case: only sqlite's interpreter function still misses compilation (2026-08-21)

- **Tried**: per-case verification (after loop-body outlining, decision 81) of whether hot generated code actually gets compiled, by crossing stackprof wall profiles under `--yjit` with measured call counts against YJIT's default call threshold of 30.
- **Verdict**: `app/sqlite3_query`'s dominant frame `_f157` (the 453-state flat dispatch, 34.6% self time) is called 13 times and is never compiled, so sqlite is the one case where "hot code is not compiled" holds; DOOM's dominant `_f406` (58 lines, 29.8% self) runs 26,400 calls per 60 ticks and the NES frame function `_f9` (1,072 lines, 69.5% self) runs once per tick with YJIT measured 3.7x over the interpreter (14.9 vs 4.0 ticks/sec), so both are compiled and their remaining cost is the byte-granular linear-memory path (`IO::Buffer` get/set plus the unit wrappers: 52% of DOOM's samples, 24% of NES's, 16.9% of sqlite's) and, for NES, the compiled quality of one huge method.
- **Invalidated when**: YJIT gains on-stack replacement or raises what a once-called method can get compiled to; or the memory units change shape enough to shift the profile.
- **Details**: measured in-session (stackprof + method-alias call counting on the smoke/query workloads); no issue yet.

## byte-memory-strategies: a software cache line loses to direct access; preloading a span wins (2026-08-21)

- **Tried**: two byte-read strategies against the current unit shape (bounds-checked wrapper method + `IO::Buffer#get_value(:U8)`, 28.7M ops/s under `--yjit`): a 64-byte software cache line (tag check per access, `get_string` refill on miss) and preloading a whole span once with `get_string` then reading with `String#getbyte`.
- **Verdict**: the cache line loses everywhere, 20.2M ops/s sequential (its best case, one miss per 64 accesses) and 8.7M random versus 26.9M for direct `get_value`, because a Ruby-level tag check plus offset masking costs more than the C call it tries to avoid; span preloading wins clearly, 43.6M ops/s (1.5x the wrapper shape under `--yjit`, 2.5x under the interpreter), with the win independent of the JIT since the cost is C calls, not Ruby dispatch.
- **Invalidated when**: `IO::Buffer` gains a byte accessor as cheap as `String#getbyte`, or the preload's applicability conditions (provable in-bounds range, no aliasing store or call between preload and use, reads only) stop matching the hot loops.
- **Details**: measured in-session (20M-access loop microbenchmarks, Ruby 4.0.4 arm64); preloading is assessed, not implemented.

## f406-hand-optimization: DOOM's hot loop is dominated by rehoistable loads and a decomposed 32-bit store (2026-08-21)

- **Tried**: hand-editing the DOOM module's hottest function (`_f406`, 29.8% self time: the per-pixel blit) in the generated Ruby to decompose where its 13 memory accesses per pixel go: seven loop-invariant constant-address loads (six format flags plus one word) re-read every iteration, one data-dependent palette word load, one sequential source byte read, and four byte stores that are one 32-bit little-endian store written out byte by byte.
- **Verdict**: hoisting the seven invariant loads to the function entry alone took the 300-tick smoke run from 13.5 to 23 ticks/sec (1.7x), and additionally fusing the four byte stores into one `iws` reached 33 to 37 ticks/sec (2.5x, frame-identical at the static 60-tick point); the sequential-read span preload prototype was abandoned mid-way after demonstrating its own hazard (the loop skips its reads when a memory-resident count is below one, so preloading the full range read past what the program reads and trapped), and its ceiling is small here anyway (one access of the thirteen).
- **Invalidated when**: the hoisting is implemented soundly (it needs either a store-alias proof or a no-store restriction, and the hand edit assumed no aliasing), or the store-fusion peephole lands, either of which makes the hand numbers obsolete; note the 300-tick frames legitimately differ across speeds because the frontend feeds DOOM a real monotonic clock, so correctness comparisons belong at the static 60-tick frame or the deterministic snapshot harness.
- **Details**: measured in-session on the outlined artifact (Ruby 4.0.4, `--yjit`, 300-tick smoke, alternating runs); no issue yet.

## call-crossing-licm-ceiling: extending load hoisting across calls would gain nothing on NES or sqlite (2026-08-21)

- **Tried**: measuring the ceiling of a call-crossing extension of the load hoisting in decision 82, by counting constant-address loads in the profiled hot functions and, for sqlite, hand-hoisting every one of them (30 distinct addresses, 124 sites in the interpreter function) to the function entry with no guards and no aliasing checks, an oracle no real pass could beat.
- **Verdict**: nothing to gain; the NES hot path has almost no targets (one constant-address load site in the 69.5%-self frame function, zero in the helpers it calls 10.5 M times per smoke run), and sqlite's 124 sites are dynamically cold (the oracle measured 9.34 s to 9.31 s under `--yjit` and 19.41 s to 19.48 s interpreted, both inside noise, output exact); both programs' memory time is dynamic-address traffic (emulator state, record and page decoding) whose values genuinely change, which no invariant-load transform touches.
- **Invalidated when**: a profiled hot loop appears whose constant-address (or invariant-address) loads are dynamically hot and separated from the loop only by calls; the DOOM blit was exactly that shape minus the calls, so the shape exists.
- **Details**: measured in-session (site counts from generated code, oracle hand-edit on the query workload); no issue.
