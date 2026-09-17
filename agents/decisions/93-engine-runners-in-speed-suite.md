# Decision 93: Alternative Engines as Speed-Suite Runners

Status: **Accepted, 2026-09-12.** `dewasm-monoruby`, `dewasm-jruby`, `dewasm-python-jit`, `dewasm-graalpy` and `dewasm-tinygo` are runner rows in the speed suite; TruffleRuby is not.

## Context

The speed suite measured each backend's output on one engine family: the Ruby backend on CRuby's three JIT modes, the Python backend on CPython and PyPy, the Go backend on the gc toolchain.
Alternative engines run the same generated artifact and land far apart from the incumbent: monoruby beats YJIT on most microbenchmark axes, GraalPy sits between CPython and PyPy except where its JIT misses the largest generated methods, TinyGo's LLVM backend beats gc on the numeric microbenchmarks, and an earlier experiment found the same micro-versus-app split for TruffleRuby and JRuby (`agents/experiments.md`, jvm-ruby-runtimes; issue #206).
Whether these engines belong in the matrix, and how their availability and their gaps are handled, had live alternatives.

## Decision

An alternative engine joins the matrix when it runs the backend's unmodified output; a gap is bridged by *refusing*, never by shimming what is measured.

- Every engine runner follows the `dewasm-pypy` shape (`crates/xtask/src/bench/runner.rs`): the same generated artifact as the incumbent's rows, a host-provided binary (`$DEWASM_MONORUBY`, `$DEWASM_JRUBY`, `$DEWASM_GRAALPY`, `$DEWASM_TINYGO`, `$DEWASM_PYTHON_JIT`, then PATH), absence reported as a normal skip, `benchmarks/setup.sh` installs none of them.
- A capability that distinguishes usable from unusable installs is probed by *behavior*, not by version: JRuby by one `-e` run of the `source_offset` forms of `IO::Buffer#copy`/`#set_string` (jruby/jruby#9588; which release first carries the fix is not the probe's to predict, and a backport passes it just the same), CPython's experimental JIT by `sys._jit.is_enabled()` under `PYTHON_JIT=1` (distribution builds carry `sys._jit` but were compiled without the JIT).
- A compiled engine that shares a backend keys its artifact separately: Go and TinyGo share one generated source but must not share a binary (`Target::artifact_tag`), and TinyGo builds at `-opt=2`, its speed setting, because the size-oriented `-opt=z` default is not what a speed suite measures.
- Engine-specific failures are exclusions with measured reasons (`workload.rs`): the sqlite pair is excluded for JRuby (cost: the JVM's 64 KB per-method bytecode limit keeps the largest generated methods interpreted, 58-70 s per run), for GraalPy (cost: 88 s per run measured, slower than CPython's 56 s, the same largest-methods gap), and for the JIT-enabled CPython (cost: 70 s per run measured, plain CPython's cost class).
  monoruby's capability exclusion there (an upstream JIT panic aborted compilation) was lifted when the fix landed upstream (sisshiki1969/monoruby#1324): the pair measures at roughly 9 s per run, `ruby --yjit`'s cost class.

## Rejected alternatives

- **A pure-Ruby `IO::Buffer` polyfill to admit TruffleRuby.**
  The experiment measured a 60x spread between two polyfill backings (String versus Array of bytes), so the column would measure the polyfill, not the engine.
  TruffleRuby joins when it implements `IO::Buffer`, per the experiment's invalidation condition.
- **An arity shim for pre-#9588 JRuby.**
  Same objection at smaller scale: `memory/copy.rb` and `memory/init.rb` would run through byte-string round trips only on this runner, and the fix already exists upstream, so the shim's whole audience is old versions.
- **Pinning the engines in `benchmarks/setup.sh`.**
  monoruby has no versioned release channel that tracks its pace (its results moved by factors within one week), a JRuby distribution is a JVM-sized install, and a JIT-enabled CPython is a from-source build; `dewasm-pypy` already establishes host-provided engines with the version captured from the binary that actually ran.
- **`wasm3-monoruby`-style converted-interpreter rows.**
  The wasm3 build's megafunctions sit exactly in the engines' remaining JIT gaps, and the rows would double the engine surface for a comparison the incumbent rows already anchor.
- **Per-workload AOT compilers for the script backends (Nuitka, Codon; GraalVM native-image for Java).**
  Each adds a minutes-long build per workload cell for engines whose semantic compatibility with the generated code is unproven; TinyGo is the one compile-first addition because it consumes the Go backend's output unmodified (measured: the sqlite module builds in 90 s once, and the content-addressed artifact cache reuses it).

## Consequences

- Positive: the suite shows what the generated code costs on engines whose JITs are shaped unlike the incumbents', against the same byte-for-byte wasmtime oracle; a wrong engine result fails the run instead of publishing.
- Resolved: `monoruby -v` carried no build revision, so a published record had to name the built commit alongside; upstream now prints one (`monoruby 0.3.0 (revision 22a4f78ab7)`), and every other host-provided engine already did, so a record's `runtimes` block identifies each build on its own.
  TinyGo's 90 s sqlite build lands on the first full run after a module or backend change; later runs reuse the cached binary.
- Carry-over: the sqlite exclusions cite upstream states and are remeasured, not kept, when those move (monoruby's already was); the jvm-ruby-runtimes experiment entry records the JRuby half as partially invalidated.
