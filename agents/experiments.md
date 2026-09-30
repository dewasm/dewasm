# Experiments

Past experiments: what was tried, what came of it, and where the full record lives.
The full record (logs, measurements, discussion) stays in the experiment's Issue or PR.
Each entry here carries the conclusion itself, so reading this index needs no network access.
An entry earns its place only if it changes what a future agent would do.
Such an entry stops a re-proposal, or it records a measured limit of an approach.

Each entry is one section in this shape:

```markdown
## <slug> (<date>)

<The conclusion, in one sentence.>

- **Tried**: what was attempted, in one or two sentences.
- **Verdict**: the outcome in one sentence, with the deciding number.
- **Invalidated when**: what would make this worth re-testing.
- **Details**: #<issue> / PR #<pr>.
```

## `lua-php-guests` (2026-08-02)

Neither interpreter builds for wasm 1.0.

- **Tried**: lua.wasm and php.wasm as example guests.
- **Verdict**: rejected; Lua's `setjmp`/`longjmp` needs the exception-handling proposal (out of scope).
  With that proposal enabled, Lua still hits a `wasm-ld` bug.
  PHP's wasm build line stopped in 2023-07 with `setjmp` removed.
- **Invalidated when**: a Lua or PHP build appears that targets plain wasm 1.0 + WASI p1 without `setjmp`.
- **Details**: #204.

## `br-table-dispatch` (2026-08-02)

Ruby's case/when on integer literals is already O(1).

- **Tried**: replacing `br_table`'s case/when with a binary if tree, suspecting linear when-matching.
- **Verdict**: rejected; YARV compiles integer-literal when clauses to `opt_case_dispatch` (a hash).
  The tree measured slower (0.149 s vs. 0.118 s over 2M dispatches).
- **Invalidated when**: the dispatch arms stop being integer literals, which falls off `opt_case_dispatch`.
- **Details**: #203.

## `i64-signed` (2026-08-03)

Signed two's-complement i64 loses to masked-unsigned in Ruby.

- **Tried**: representing i64 as signed two's complement instead of the shared masked-unsigned convention.
  A full specification testsuite pass exists on the preserved `i64-signed` branch.
- **Verdict**: rejected; i64_alu dropped to 0.86-0.93x (rest 0.97-1.03x).
  The reason is that hot i64 values in real apps are almost all non-negative.
  So masked-unsigned rarely boxes, while the signed form pays its wrap branch everywhere.
- **Invalidated when**: a negative-value-heavy workload matters.
  The signed form won 2.2x on synthetic negative-heavy code.
  Or Ruby's integer boxing boundary changes.
- **Details**: #107.

## `mem-inline` (2026-08-03)

Inlining Ruby's memory wrappers is slower.

- **Tried**: emitting linear-memory loads and stores inline instead of through the wrapper methods.
  The attempt is preserved on the `mem-inline` branch.
- **Verdict**: rejected; `mem_rw` under YJIT measured 0.79x.
  It loses because the cost is the bounds check and `IO::Buffer#get_value` itself, not method dispatch.
- **Invalidated when**: bounds-check removal becomes reliable (PR #109's commit 4 shape).
  That shape is blocked on `ArgumentError#message` matching, which can break across Ruby versions.
- **Details**: #108 / PR #109.

## `function-dedup` (2026-08-06)

Identical generated functions are already merged upstream.

- **Tried**: merging identical Ruby function bodies to reduce the resident ISeq.
  The win was sized on the 30 MB `merman` output.
- **Verdict**: no win; `wasm-opt` already merged identical functions: 2 remain among 7,475.
- **Invalidated when**: modules reach the emitter without a `wasm-opt` pass first.
- **Details**: #202.

## `memory-delegate` (2026-08-06)

Memory calls without a receiver, forwarded to the memory object, slow the hot path.

- **Tried**: rewriting `@memory.i32_load` to a forwarded `i32_load` without a receiver at 246k call sites.
  The aim was to cut the receiver read.
- **Verdict**: rejected; a microbenchmark measured +27% (`getivar+send` becomes `putself+send+send`).
  The ISeq saving was only ~1.5%.
- **Invalidated when**: the unmeasured `bmethod` variant gets measured and wins.
  That variant is `define_method(:i32_load, @memory.method(:i32_load))`, which costs one frame.
- **Details**: #202.

## `mask-omission-ceiling` (2026-08-06)

Dropping provable i32 masks caps near -5% of ISeq.

- **Tried**: sizing the win from leaving out `& 0xffffffff` where the result provably fits i32.
  The sizing ran on the `merman` output.
- **Verdict**: low upper limit; of 131.6k masks only ~12% are droppable at the expression site.
  The rest need per-local dataflow.
  Masks are ~6% of all instructions, so the ISeq change is at best about -5 to -6%.
- **Invalidated when**: an IR-level range analysis lands; it would also serve Perl and Python.
  Or the always-masked invariant is relaxed to observation-point masking.
- **Details**: #202.

## `spinel-aot` (2026-08-13)

Typing i64 as `bigint` and compile-time scaling defeat Spinel today.

- **Tried**: compiling the Ruby backend's output with Spinel (Matz's AOT Ruby-to-C compiler).
  The run probed Spinel's subset, and its scaling with generated-code shapes.
- **Verdict**: too early; any value or literal at or above 2^63 types as `bigint`.
  That includes the M64 mask constant, and the `bigint` typing erases the win.
  A masked u64 loop measured 1.0x vs. CRuby.
  Signed int64 under `--int-overflow=wrap` is effectively free.
  The front end also scales worse than the square of the input size.
  391 KB of Ruby took 12 s in the front end and 113 s in total at `-O2`.
  That puts sqlite3-shell's 7.9 MB out of reach.
- **Invalidated when**: Spinel's compile-time scaling improves, and a Spinel output profile exists.
  Spinel is pre-0.1 and moving fast.
  That profile is signed i64 (the `i64-signed` branch) plus an `IO::Buffer` replacement.
- **Details**: #205.

## `jvm-ruby-runtimes` (2026-08-13)

Generated methods exceed per-method JIT limits.

- **Tried**: running the Ruby backend's output on TruffleRuby 33.0.1 and JRuby 10.0.3.0.
  TruffleRuby used an `IO::Buffer` replacement written in Ruby.
  JRuby used an `IO::Buffer` arity shim.
  The runs covered microbenchmarks and apps.
- **Verdict**: rejected as suite runners; both beat YJIT on microbenchmarks yet lose on real apps.
  On microbenchmarks, TruffleRuby's f64_alu ran at 4x Wasmtime vs. YJIT's 78x.
  On real apps, sqlite3_query took 58-70 s on JRuby vs. 9.4 s on YJIT.
  TruffleRuby was unfinished after 48 minutes.
  The loss comes from the largest generated methods (~13k lines).
  They exceed the JVM's 64 KB bytecode limit per method.
  So the hottest functions stay interpreted.
  JRuby raises MethodTooLargeException once `-Xjit.maxsize` allows the attempt.
- **Invalidated when**: a pass caps generated method size by splitting functions.
  Such a pass is also relevant to the Java backend's 64 KB constraint.
  The other case needs both runtimes to change.
  JRuby's `IO::Buffer` must gain the four-argument `copy`/`set_string` forms.
  TruffleRuby must gain `IO::Buffer` at all.
  Partly invalidated on 2026-09-12: JRuby fixed the arity gap upstream (`jruby/jruby#9588`).
  That fix is merged, and its release is pending.
  The suite now carries a `dewasm-jruby` runner.
  Its availability probe checks those forms by behavior (decision 93).
  The method-size finding stands, and it keeps the SQLite pair excluded for that runner.
- **Details**: #206.

## `step-lambda-dispatch` (2026-08-21)

A flat dispatch wrapped in a `lambda` loses in every JIT configuration.

- **Tried**: emitting a flat-dispatch function's `case state` inside `__step = lambda do ... end`.
  The states then run in a closure called repeatedly.
  The first variant called it once per transition, and the second ran 1024 transitions per call.
  YJIT compiles that closure even though the outer function is entered once.
  The measurement used sqlite3-shell's interpreter function, which has 453 states.
  That function takes 34.6% self time in the `query` workload profile.
- **Verdict**: rejected; per-transition calls measured 83 M JIT-boundary crossings.
  They measured 9.51 s to 10.21 s under `--yjit`, and the second variant only recovered to 9.96 s.
  The interpreter measured 19.63 s to 20.67 s.
  It loses because two costs survive.
  One is closure-environment variable access.
  Every local becomes a heap-environment slot with a write barrier.
  The other is the compiled size of a 453-state method (12.4 MB of generated machine code).
  `optcarrot`'s generated core shows the same large-compiled-method loss against its small-method core.
- **Invalidated when**: YJIT gains on-stack replacement, which makes the whole approach unnecessary.
  Or compiled closure-environment access stops costing more than the interpreter saves.
- **Details**: measurements in this experiment were taken before any issue existed.
  The step emission itself was removed again, and only this entry records it.

## `jit-coverage-per-case` (2026-08-21)

Only SQLite's interpreter function still misses compilation.

- **Tried**: verifying per case whether hot generated code actually gets compiled.
  This ran after loop-body extraction (decision 81).
  The method crossed `stackprof` wall profiles under `--yjit` with measured call counts.
  Those counts were compared against YJIT's default call threshold of 30.
- **Verdict**: `app/sqlite3_query`'s dominant frame `_f157` is the 453-state flat dispatch.
  That frame takes 34.6% self time, is called 13 times, and is never compiled.
  So SQLite is the one case where "hot code is not compiled" holds.
  DOOM's dominant `_f406` (58 lines, 29.8% self) runs 26,400 calls per 60 ticks.
  The NES frame function `_f9` (1,072 lines, 69.5% self) runs once per tick.
  On NES, YJIT measured 3.7x over the interpreter (14.9 vs. 4.0 ticks/sec).
  So both are compiled, and their remaining cost is the byte-at-a-time linear-memory path.
  That path is `IO::Buffer` get/set plus the unit wrappers.
  It takes 52% of DOOM's samples, 24% of NES's, and 16.9% of SQLite's.
  For NES, the compiled quality of one huge method adds to the remaining cost.
- **Invalidated when**: YJIT gains on-stack replacement.
  Or YJIT raises what a once-called method can get compiled to.
  Or the memory units change shape enough to shift the profile.
- **Details**: measured in-session; no issue yet.
  The tools were `stackprof` and method-alias call counting on the `smoke` and `query` workloads.

## `byte-memory-strategies` (2026-08-21)

A software cache line loses to direct access; reading a span ahead wins.

- **Tried**: two byte-read strategies against the current unit shape.
  That shape is a bounds-checked wrapper method + `IO::Buffer#get_value(:U8)`.
  It runs at 28.7M operations/s under `--yjit`.
  One strategy is a 64-byte software cache line with a tag check per access.
  The cache line refills with `get_string` on a miss.
  The other reads a whole span ahead once with `get_string`, then reads it with `String#getbyte`.
- **Verdict**: the cache line loses everywhere against 26.9M operations/s for direct `get_value`.
  It measured 20.2M operations/s in order (its best case, one miss per 64 accesses) and 8.7M random.
  The cause is the Ruby-level tag check plus offset masking.
  Together they cost more than the C call they try to avoid.
  Reading the span ahead wins clearly at 43.6M operations/s.
  That is 1.5x the wrapper shape under `--yjit` and 2.5x under the interpreter.
  The win is independent of the JIT, since the cost is C calls, not Ruby dispatch.
- **Invalidated when**: `IO::Buffer` gains a byte accessor as cheap as `String#getbyte`.
  Or the conditions for reading ahead stop matching the hot loops.
  Those conditions are:
  - a provable in-bounds range;
  - no aliasing store or call between the read-ahead and use;
  - reads only.
- **Details**: measured in-session (20M-access loop microbenchmarks, Ruby 4.0.4 arm64).
  Reading ahead is assessed, not implemented.

## `f406-hand-optimization` (2026-08-21)

DOOM's hot loop is dominated by rehoistable loads and a 32-bit store split into bytes.

- **Tried**: hand-editing the DOOM module's hottest function in the generated Ruby.
  That function is `_f406`, the per-pixel image copy, at 29.8% self time.
  The aim was to find where its 13 memory accesses per pixel go:
  - seven loop-invariant constant-address loads (six format flags and one word) re-read every iteration;
  - one data-dependent palette word load;
  - one in-order source byte read;
  - four byte stores that are one 32-bit little-endian store written out byte by byte.
- **Verdict**: hoisting the seven invariant loads to the function entry alone gave 1.7x.
  It took the 300-tick smoke run from 13.5 to 23 ticks/sec.
  Additionally fusing the four byte stores into one `iws` reached 33 to 37 ticks/sec (2.5x).
  That result was frame-identical at the static 60-tick point.
  The attempt to read the source span ahead was abandoned mid-way after demonstrating its own risk.
  The loop skips its reads when a memory-resident count is below one.
  So reading the full range ahead read past what the program reads, and it trapped.
  The upper limit of reading ahead is small here anyway: one access of the thirteen.
- **Invalidated when**: the hoisting is implemented soundly, or the store-fusion peephole lands.
  Either makes the hand numbers out of date.
  Sound hoisting needs either a store-alias proof or a no-store restriction.
  The hand edit assumed no aliasing.
  Note that the 300-tick frames legitimately differ across speeds.
  They differ because the frontend feeds DOOM a real monotonic clock.
  So correctness comparisons belong at the static 60-tick frame or the deterministic snapshot harness.
- **Details**: measured in-session on the extracted artifact; no issue yet.
  The measurements used Ruby 4.0.4 with `--yjit`, the 300-tick smoke run, and runs taken in turn.

## `call-crossing-licm-ceiling` (2026-08-21)

Load hoisting across calls gains nothing on NES or SQLite.

- **Tried**: measuring the upper limit of a call-crossing extension of the load hoisting in decision 82.
  The measurement counted constant-address loads in the profiled hot functions.
  For SQLite, every one of those loads was hoisted by hand to the function entry.
  Those are 30 distinct addresses, at 124 sites in the interpreter function.
  The hoist had no guards and no aliasing checks, an oracle no real pass could beat.
- **Verdict**: nothing to gain; the NES hot path has almost no targets.
  The 69.5%-self frame function has one constant-address load site.
  The helpers it calls 10.5 M times per smoke run have zero.
  The 124 SQLite sites are dynamically cold.
  The oracle measured 9.34 s to 9.31 s under `--yjit` and 19.41 s to 19.48 s interpreted.
  Both changes are inside noise, and the output was exact.
  Both programs' memory time is dynamic-address traffic (emulator state, record and page decoding).
  The values of that traffic genuinely change, which no invariant-load transform touches.
- **Invalidated when**: a profiled hot loop appears whose constant-address loads are dynamically hot.
  Invariant-address loads count as well.
  Those loads must be separated from the loop only by calls.
  The DOOM image copy was exactly that shape minus the calls, so the shape exists.
- **Details**: measured in-session; no issue.
  The site counts come from generated code, and the oracle hand-edit ran on the `query` workload.

## `ivar-localization` (2026-08-21)

Caching instance variables in locals gains under 1% on modern Ruby.

- **Tried**: `optcarrot`'s second-largest lever (its ablation measured 21 to 38%).
  It was applied to the generated code's dominant instance variable, `@m`.
  `@m` was copied to a local at the entry of every method that touches memory.
  That covers 1,767 methods in sqlite3-shell and 748 in DOOM.
  Isolated-process microbenchmarks measured the per-reference delta as well.
- **Verdict**: rejected as a pass.
  On Ruby 4.0.4 an instance-variable read costs only 0.3 to 0.6 ns more than a local under `--yjit`.
  Interpreted, the delta is 1.7 to 2.5 ns.
  Writes are similar, since fixnum stores skip the write barrier.
  So the whole-program transforms moved sqlite3_query, `c/sha256`, and the DOOM smoke run by under 1%.
  That change is inside noise.
  On the Ruby of its era, `optcarrot`'s technique collected a real gain.
  Object shapes and warm inline caches have already collected that gain.
  The arithmetic sets a density a method must reach before the delta becomes visible.
  Its instance-variable references must be a dominant fraction of all executed operations.
  The generated code never reaches that density.
  It does one `@m` read per memory access, and that access itself costs 12 to 30 ns.
- **Invalidated when**: a Ruby release makes instance-variable access slower.
  Or generated code starts reading many distinct instance variables per operation.
  Nothing emits that today.
- **Details**: measured in-session; no issue.
  The microbenchmarks ran in isolated processes after a same-process harness mis-measured.
  The whole-program hand transforms ran with output equality checks.

## `vdbe-forced-compilation` (2026-08-21)

Forcing YJIT to compile SQLite's interpreter function loses at every measured scale.

- **Tried**: answering whether the 453-state interpreter function can be JIT-compiled at all.
  The experiment also asked whether that helps.
  It put 60 `SELECT 0;` statements at the start of the `query` workload.
  The function then passes YJIT's 30-call threshold (about 120 calls) before the heavy statements run.
  The interpreter-mode control confirmed the extra statements themselves cost 0.1 s.
- **Verdict**: it compiles completely and still loses.
  Compilation added 47 k blocks and 14.5 MB of machine code.
  Compile time went from 1.0 s to 4.4 s, with no code collection and four invalidations.
  The workload with 100 k rows measured 9.40 s to 11.73 s.
  At 300 k rows, it measured 26.71 s to 29.50 s.
  It loses because the compile cost is fixed.
  The execution saving meanwhile stays flat instead of scaling.
  The fixed cost is about 3.4 s of user time plus 1.7 s of system time spent changing code memory.
  The saving is 2.7 s at 100 k and 2.4 s at 300 k.
  Execution is fast early.
  It then converges toward interpreted speed as the 23 MB code region's working set grows.
  So three times the workload does not close the gap, and the fixed cost is never paid back.
- **Invalidated when**: YJIT improves materially on methods several MB in size.
  The improvement can be in generated-code density or in instruction-cache behavior.
  Or the function stops being one method (the split-with-exit-protocol route).
  Either change deserves a re-measurement.
- **Details**: measured in-session.
  The extra statements went into the SQL input, and `--yjit-stats` supplied the compile counters.
  Interpreter runs served as the added-statement control.
  This is the third independent confirmation of the large-compiled-method wall.
  The first two are the `step-lambda-dispatch` experiment and `optcarrot`'s own core comparison.

## `vdbe-opcode-splitting` (2026-08-21)

Splitting hot opcodes out of SQLite's interpreter in the C source wins 9.4% under YJIT.

- **Tried**: patching the SQLite 3.53.3 amalgamation, the version the apps are fixed at.
  The patch extracts the 13 hot opcode bodies of `sqlite3VdbeExec` into new functions.
  Each of those functions is `static SQLITE_NOINLINE`.
  A program made the patch mechanically: 1413 insertions over the 269k-line file.
  Case exits become return codes, and dispatcher locals like `rc`/`iCompare` pass by pointer.
  The patched source was then rebuilt, converted, and measured on the `query` workload.
  The control was a stock build made with the same recipe.
  The opcodes are:
  - `Column`, `MakeRecord`, `Insert`, `Yield`, `Copy`, `NewRowid`, `Concat`;
  - the arithmetic and comparison families;
  - Rewind, Next/Prev/SorterNext, and the AggStep pair.
- **Verdict**: it works, but only after stopping Binaryen from undoing it.
  `wasm-opt -O2`'s default single-caller inlining pulled all 13 functions straight back in.
  The converted interpreter method *grew*.
  The fix is `--no-inline=vdbeOp*`, which needs the name section.
  So strip via `wasm-opt --strip-dwarf` instead of `-Wl,--strip-debug`.
  With that fix, the interpreter method is reduced from 12,516 to 10,380 lines.
  Twelve of the new methods get hot enough for YJIT to compile (+12 ISeqs, +170 ms compile time).
  The workload runs 9.37 s to **8.48 s (9.4% faster)** under `--yjit`, at a 6.0% cost without a JIT.
  Outputs are byte-identical across a 41-statement sanity file and a 30-statement stress file.
  That holds for both the native and the converted build.
- **Invalidated when**: the interpreter function stops being too cold for YJIT to compile.
  The win exists precisely because the containing method never compiles.
  Force-compiling it via extra statements is still a net loss for both builds.
  Or the fixed app versions move to a SQLite whose `sqlite3VdbeExec` shape changed.
- **Details**: measured in-session by a subagent.
  The program that made the patch, and its diff, are at `/tmp/vdbe_exp/` (a temporary directory).
  The artifacts are at `/tmp/vdbe_{stock,split}*.rb`.
  The build recipe in the repository is untouched.
  Adopting this needs both the C patch and the `wasm-opt --no-inline` change.
  It also needs a decision about what the benchmark then claims to measure.

## `float-bits-scratch` (2026-08-21)

A race-free reused `IO::Buffer` loses to `pack` in wall time on real apps.

- **Tried**: replacing `pack`/`unpack1` in the five float bit-conversion units.
  Those units are `f32`, `f32_bits`, `f32_from_bits`, `f64_bits`, and `f64_from_bits`.
  The replacement is `set_value`/`get_value` on a reusable 8-byte `IO::Buffer`.
  The `IO::Buffer` is placed per receiver (`@scratch`).
  A module-level constant placement had measurably produced wrong floats across threads.
  The corruption hit 29 of 480 runs, with four threads on four instances.
  The `IO::Buffer` path also keeps NaN payloads where `pack` canonicalizes.
  The PR's decision draft records that.
- **Verdict**: rejected; a tight conversion micro benchmark gains 25% wall.
  It also drops 93% of its allocations.
  The f32-heavy app measured is `sghtmltopdf`, rendering a payment record.
  There these units produce 1.09M of its 1.10M String allocations.
  On it, the safe placement measures +2.5 to +3.5% wall despite 36 to 80% fewer allocations.
  The reason is that the instance-variable read plus the `IO::Buffer` call pair costs more.
  It costs more than the `pack` pair once the conversions sit inside mixed work.
  The benchmark suite does not move, since no suite workload leans on these units.
- **Invalidated when**: Ruby gains a cheaper bit-reinterpretation primitive.
  The primitive to beat is `IO::Buffer#get_value`/`set_value`.
  Or a workload appears where the cost of frequent allocation is more than the per-call cost.
  Or the thread-sharing constraint changes so the constant placement becomes acceptable.
- **Details**: #261 / PR #263 (closed unmerged).
  The unit diff, the placement measurements, and the decision draft live there.

## `jruby-script-precompile` (2026-09-18)

JRuby's `cowsay` time tracks whether its script compiles at all.

- **Tried**: chasing `app/cowsay` on `dewasm-jruby`, which read 1.97 s before the `cowsay` replacement (#322).
  It read 3.52 s after.
  Across that change the module went from 772 kB to 68 kB, and the generated source 1.91 MB to 361 kB.
- **Verdict**: not a regression in the generated code.
  JRuby compiles the whole script when it starts (`Ruby.precompileCLI`).
  The old artifact's compile *gave up* on a data segment emitted as a 136.5 kB string literal.
  That literal is over the JVM's 64 kB constant limit.
  `IndyValueCompiler.pushString` throws, and JRuby falls back to the interpreter.
  The new artifact's largest literal is 47.3 kB, so the compile succeeds.
  It costs 1.9 s for a program whose own work is 0.29 s.
  The program phase is unchanged (0.28 s old, 0.29 s new).
  With `-X-C` both interpret, and the new artifact is the faster one (1.41 s against 1.68 s).
  Splitting the old artifact's literals under the limit makes it compile, and it takes **131 s**.
  So the old figure was never a cheap compile but a missing one.
  Method-level JIT settings (`jit.maxsize`, `jit.threshold`) and `compile.invokedynamic` move none of it.
- **Invalidated when**: the Ruby backend stops emitting a data segment as one literal.
  Chunking it, as the Codon backend does per #320, would make every artifact compile.
  That could cost minutes on a large module.
  Or JRuby raises or removes the compile of the whole script before the run.
  Or a JRuby workload appears whose run is long enough to earn the compile back.
- **Details**: measured in-session against the pre-#322 registry build.
  The phase split and the 60/70 kB literal probe are in the PR that adds this entry.
  The split-literal reversal is in that PR too.
  Related: #206 (the largest generated methods never JIT).
