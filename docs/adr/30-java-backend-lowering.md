# ADR-30 — Java Backend Lowering Conventions

Status: **Accepted, 2026-07-26.** First milestone ("cowsay runs", ADR-24)
implemented in `crates/dewasm-backend-java/src/lib.rs`, `runtime/java/units/`,
and `crates/dewasm-backend-java/tests/{e2e,units}.rs`. Numeric conventions are
ADR-2's; this ADR records where Java — statically typed with native fixed-width
integers and floats, but carrying a hard **64KB-per-method bytecode limit** —
forced a different shape from the other compiled backend (Go, ADR-29) and from
the interpreted ones. Java is a compiled backend, so it uses ADR-27's `run()`
override (`javac` + a class-dir cache).

## Context

Java's type system gives the native-numerics wins Go has (ADR-29), but the JVM
adds a constraint no other target has: **a single method is capped at 64KB of
bytecode**, `<clinit>`/`<init>` included, and a string constant at 65535 bytes.
cowsay already blows both: its largest function lowers to ~11.7k statements
(one ~11.3k-statement loop body), far past 64KB, and its data segment is 69856
bytes, past the string-literal cap. ADR-10 predicted this would be the
Java-specific landmine; it is real at the very first milestone, so the split
and the data-chunking are implemented now, not deferred.

Two more Java facts shaped the design:

1. **Unreachable code is a compile error** ("unreachable statement"), unlike
   Go (which tolerates it) — so an emitter must never place a statement after an
   unconditional `return`/`break`/`throw`. But *missing* a return is also an
   error. Matching Java's exact reachability model for labelled blocks is
   fiddly and bidirectionally fatal.
2. **No tuples / multi-value returns**, and **no unsigned integer types**.

## Decision

- **Types (the natural Java mapping, ADR-2).** i32→`int`, i64→`long` as
  *signed bit patterns* (the hardware view, not Ruby/Python's masked-unsigned
  convention); unsigned ops use `Integer`/`Long.divideUnsigned`/
  `remainderUnsigned`/`compareUnsigned`/`toUnsignedLong`. The visible semantics
  are identical — a masked-unsigned `x & 0xffffffff` and a signed `int` denote
  the same 2³² residues — and this mapping makes wrapping arithmetic
  (`a + b`), shifts, sign-extension, and memory stores native. f32→`float`,
  f64→`double`: Java is strict IEEE with **no implicit FMA contraction**, so f32
  re-rounding and trap-free division (`a / b` → inf/NaN) need no helper — Java is
  actually *safer* here than Go, which had to route mul/div through `noinline`
  helpers to defeat FMA and constant-folding (ADR-29). NaN bit paths go through
  `Float.floatToRawIntBits`/`intBitsToFloat` (and the `double` pair).
  Only integer div/rem carry runtime helpers (`Rt.i32_div_s`, …), for wasm's
  two trap conditions Java's `/` does not raise (`INT_MIN/-1` overflow and,
  uniformly, divide-by-zero with the spec message).
- **Memory is a `byte[]` with a little-endian `ByteBuffer` view.** Chosen over
  VarHandles for simplicity; the buffer is re-wrapped on `memory.grow`. Callers
  compute effective addresses as unsigned `long` (`Integer.toUnsignedLong(a) +
  offset`), so a guest-addr-plus-offset past 2³¹ is bounds-checked exactly, and
  the `int` index handed to the buffer is safe after the check.
- **Control flow uses the branch-register model (ADR-28's Python design),
  not Java labelled blocks — the load-bearing deviation from the task's
  stylistic preference.** Block/if exits and the function return set a
  per-function register `_br`; following siblings are guarded by `if (_br == 0)`;
  only real loops become `while (true)` with a trailer that turns `_br == <loop
  id>` into `continue`. The criterion: **the representation must split across
  methods** (see below), and language-level labelled `break`/`continue` cannot
  cross a method boundary, whereas a data-carried `_br` can. This choice also
  dissolves Java's unreachable-statement landmine for free: no bare
  mid-sequence `return`/`break` is ever emitted, so there is nothing to be
  unreachable. Two Java-specific adaptations of ADR-28:
    * **The function return is itself register-based** (`_ret = v; _br = -1;`)
      with a single tail `return _ret`, precisely so a mid-function `return`
      never becomes a bare Java `return` with dead code behind it. (Python emits
      native `return` and lets the dead code be; Java cannot.)
    * **`Rt.trap`/`exit`/`link_error` are `void` methods that throw**, emitted
      as plain statements (`Rt.trap("unreachable");`), not `throw`, so no
      "unreachable statement" follows them.
- **The 64KB method split.** A function whose estimated cost (an IR node count)
  crosses a threshold has its locals, temps, and the `_br`/`_ret` registers
  hoisted to a per-call **frame object** (`FrameNN`), and its body split at
  statement-sequence boundaries into numbered `void fNN_pK(FrameNN f)` methods,
  called in order. Because control flow is the `_br` data register — not
  language labels — the parts are simply **called unconditionally**: an escaped
  branch (`_br != 0`) makes each subsequent part's guarded statements no-op
  until the owning loop trailer / block reset-marker consumes it. A loop keeps
  its `while (true)` wrapper in the parent method and chunks its (possibly
  huge) body into parts called inside the loop; the split recurses into any
  sub-body over the threshold. This is the split ADR-10 anticipated ("numbered
  helper methods … with locals hoisted to fields of a per-call frame object").
  cowsay needs it: 61 of its 640 functions split, into ~1335 part methods, and
  the whole file compiles with no "code too large".
- **Data segments are chunked Base64.** A segment is emitted as
  `Rt.data_from_b64(new String[]{"…", "…"})` with each chunk's Base64 under the
  65535-byte string-literal limit (32KB raw per chunk); `java.util.Base64`
  decodes and concatenates at instantiation. This is the honest general
  mechanism; hex was rejected as it doubles the constant size for no benefit.
- **Import/table/export values use one boxed calling convention at the dynamic
  boundary.** A wasm function value is an `Rt.Fn` (`Object invoke(Object[])`);
  the import object is `Map<String, Map<String, Object>>` (ADR-7's shape).
  Imports resolve to an embedder `Rt.Fn`, else a bundled-WASI adapter lambda,
  else an ENOSYS stub / link error. `call_indirect` and exports go through the
  same `Rt.Fn` (a `Funcref` boxes a structural type string + the `Fn`, checked
  in `Table.call`). **Direct calls to defined functions stay primitive**
  (`fN(a, b)`); boxing is only at the dynamic edge, mirroring Go's `any`
  boundary. The honest cost: with one uniform `Fn`, a wrong-*signature* import
  is not rejected (a wrong-*kind* one is, via `instanceof`); this is a wider
  import-limits gap than Go's, acceptable for the milestone.
- **Execution (`run()` override, ADR-27).** Java is compiled, so the test helper
  compiles the single generated `Main.java` with `javac` into a
  content-addressed class-dir cache and runs `java -cp <dir> Main`. Measured on
  cowsay, this beats the `java Main.java` single-file source launcher decisively:
  the launcher recompiles in memory on **every** run (~3.3 s each), while
  `javac` + cache pays a one-time ~2 s compile and then runs warm in ~0.15 s.
  `$DEWASM_JAVA`/`$DEWASM_JAVAC` override the toolchain; a missing one fails
  loud (ADR-15). One public class (`Main`) per file keeps the `javac` filename
  contract trivial; the runtime classes and the module class are package-private.
- **Feature scope.** `Floats` is `Supported`. WASI is the eight core syscalls
  cowsay imports (args/env, `fd_read`, `fd_write`, `proc_exit`, `random_get`).
  Non-function imports, multiple tables, and table bulk ops are out of scope for
  the milestone and rejected at conversion time (`check_module_support`).

## Rejected alternatives

- **Java labelled blocks (`L: { … } break L;`) for control flow** — the task's
  stylistic preference and cleaner to read, but a `break L` cannot cross a
  method boundary, so oversized functions could not be split without first
  rewriting their control flow into exactly the register form adopted here.
  Correctness under the 64KB limit outranks readability (ADR-1). It would also
  reintroduce the unreachable-statement / missing-return tightrope.
- **A relooper / big-`switch` state machine** — a heavier rewrite than the
  register model for the same depth-insensitivity; ADR-28 already proved the
  register model on cowsay.
- **One `Funcref` dispatch method (`callByIndex`) instead of per-entry
  lambdas** — a single giant `switch` over hundreds of functions is itself a
  64KB-method hazard needing its own split; lambdas distribute the code.
- **Native `(int)` casts for trapping float→int** — Java's cast saturates, so
  the *saturating* ops (`trunc_sat_*`) are exact, but the *trapping* ones
  silently saturate instead of trapping. Accepted as a documented milestone-1
  gap (cowsay exercises neither); the trapping form and full NaN-payload
  conformance for the rounding ops are spec-milestone work.

## Consequences

- Positive: cowsay is byte-identical to the wasmtime golden for both the args
  and stdin cases; the standalone, library, and WASI stdio/args-env suites pass.
  Native integers/floats keep the arithmetic and runtime small; Java's strict
  IEEE avoids Go's FMA/fold workarounds entirely.
- Negative: the generated file is large and verbose (frame classes, part
  methods, per-statement `_br` guards); the boxed `Fn` boundary autoboxes at
  every import/indirect/export call.
- Deferred, recorded for later milestones: the trapping float→int conversions
  and spec-grade NaN/rounding conformance (spec milestone); multi-value function
  results; wasm-1.0 completion (imported globals/memories/tables, multiple
  tables, table bulk ops); the full WASI preview 1 surface incl. the filesystem.
  Multi-MB data segments (qjs/sqlite) will also need the chunked-Base64 *array
  initialiser* itself split across methods — the `<clinit>`/`<init>` 64KB limit —
  which cowsay's three chunks do not reach.
