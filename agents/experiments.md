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
