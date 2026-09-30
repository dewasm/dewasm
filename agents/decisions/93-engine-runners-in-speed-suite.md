# Decision 93: Alternative Engines as Speed-Suite Runners

Status: **Accepted, 2026-09-12.**
The speed suite has these runner rows:
- `dewasm-monoruby`;
- `dewasm-jruby`;
- `dewasm-python-jit`;
- `dewasm-graalpy`;
- `dewasm-tinygo`.

TruffleRuby is not a runner row.

## Context

The speed suite measured each backend's output on one engine family:
- the Ruby backend on CRuby's three JIT modes;
- the Python backend on CPython and PyPy;
- the Go backend on the `gc` toolchain.

Alternative engines run the same generated artifact and land far apart from the incumbent:
- `monoruby` beats YJIT on most microbenchmarks.
- GraalPy sits between CPython and PyPy, except where its JIT misses the largest generated methods.
- TinyGo's LLVM backend beats `gc` on the numeric microbenchmarks.
- An earlier experiment found the same micro-versus-app split for TruffleRuby and JRuby.
  The record is `agents/experiments.md`, `jvm-ruby-runtimes` (issue #206).

Whether these engines belong in the matrix had live alternatives.
So did how their availability and their gaps are handled.

## Decision

An alternative engine joins the matrix when it runs the backend's unmodified output.
A gap is bridged by *refusing*, never by shimming what is measured.

- Every engine runner follows the `dewasm-pypy` shape (`crates/xtask/src/bench/runner.rs`).
  It runs the same generated artifact as the incumbent's rows.
  It takes a host-provided binary from an environment variable, then from PATH.
  The variables are `$DEWASM_MONORUBY`, `$DEWASM_JRUBY`, and `$DEWASM_GRAALPY`.
  The others are `$DEWASM_TINYGO` and `$DEWASM_PYTHON_JIT`.
  Absence is reported as a normal skip, and `benchmarks/setup.sh` installs none of them.
- A capability that distinguishes usable from unusable installs is probed by *behavior*, not by version.
  - JRuby is probed by one `-e` run of the `source_offset` forms of `IO::Buffer#copy`/`#set_string`.
    The upstream issue is `jruby/jruby#9588`.
    Which release first carries the fix is not the probe's to predict.
    A backport passes the probe just the same.
  - CPython's experimental JIT is probed by `sys._jit.is_enabled()` under `PYTHON_JIT=1`.
    Distribution builds carry `sys._jit` but were compiled without the JIT.
- A compiled engine that shares a backend keys its artifact separately.
  Go and TinyGo share one generated source but must not share a binary (`Target::artifact_tag`).
  TinyGo builds at `-opt=2`, its speed setting.
  That is because the size-oriented `-opt=z` default is not what a speed suite measures.
- Engine-specific failures are exclusions with measured reasons (`workload.rs`).
  The SQLite pair is excluded for three runners:

  | Runner | Class | Reason |
  | --- | --- | --- |
  | JRuby | cost | the JVM's 64 KB per-method bytecode limit keeps the largest generated methods interpreted, 58-70 s per run |
  | GraalPy | cost | 88 s per run measured, slower than CPython's 56 s, the same largest-methods gap |
  | JIT-enabled CPython | cost | 70 s per run measured, plain CPython's cost class |

  `monoruby` had a capability exclusion there: an upstream JIT panic stopped compilation.
  It was lifted when the fix landed upstream (`sisshiki1969/monoruby#1324`).
  The pair measures at roughly 9 s per run, `ruby --yjit`'s cost class.

## Rejected alternatives

- **An `IO::Buffer` written in Ruby, to admit TruffleRuby.**
  The experiment measured a 60x spread between two backings of it (String versus Array of bytes).
  So the column would measure that implementation, not the engine.
  TruffleRuby joins when it implements `IO::Buffer`, per the experiment's invalidation condition.
- **An arity shim for pre-#9588 JRuby.**
  The same objection applies at smaller scale.
  `memory/copy.rb` and `memory/init.rb` would run through byte-string round trips only on this runner.
  The fix already exists upstream, so the shim's whole audience is old versions.
- **Fixing the engine versions in `benchmarks/setup.sh`.**
  `monoruby` has no versioned release channel that tracks its pace.
  Its results moved by factors within one week.
  A JRuby distribution is a JVM-sized install, and a JIT-enabled CPython is a from-source build.
  `dewasm-pypy` already establishes host-provided engines.
  Their version is captured from the binary that actually ran.
- **`wasm3-monoruby`-style converted-interpreter rows.**
  The wasm3 build's largest functions sit exactly in the engines' remaining JIT gaps.
  The rows would also double the engine surface for a comparison the incumbent rows already cover.
- **Per-workload AOT compilers for the script backends.**
  The candidates were Nuitka and Codon, and GraalVM native-image for Java.
  Each adds a minutes-long build per workload cell.
  Those engines' semantic compatibility with the generated code is unproven.
  TinyGo is the one compile-first addition, because it consumes the Go backend's output unmodified.
  It was measured: the SQLite module builds in 90 s once.
  The content-addressed artifact cache then reuses it.

## Consequences

- Positive: the suite shows what the generated code costs on more engines.
  Those engines have JITs shaped unlike the incumbents'.
  It checks that each output is identical to the output of the same Wasmtime oracle.
  A wrong engine result fails the run instead of publishing.
- Resolved: `monoruby -v` carried no build revision.
  So a published record had to name the built commit alongside.
  Upstream now prints one: `monoruby 0.3.0 (revision 22a4f78ab7)`.
  Every other host-provided engine already did.
  So a record's `runtimes` block identifies each build on its own.
  TinyGo's 90 s SQLite build lands on the first full run after a module or backend change.
  Later runs reuse the cached binary.
- Carry-over: the SQLite exclusions cite upstream states.
  They are remeasured, not kept, when those states move (`monoruby`'s already was).
  The `jvm-ruby-runtimes` experiment entry records the JRuby half as partly invalidated.
