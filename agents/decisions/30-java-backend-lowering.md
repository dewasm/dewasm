# Decision 30: Java Backend Lowering Conventions

Status: **Accepted, 2026-07-26.**
Implemented in `crates/dewasm-backend-java/src/lib.rs`, `runtime/java/units/`, and `crates/dewasm-backend-java/tests/{spec,e2e,units}.rs`: wasm 1.0 with the spec harness passing ([decision 3](3-testing-strategy.md), [decision 16](16-ruby-wasm1-completion.md)), full WASI preview 1 including the filesystem (adopting [decision 14](14-ruby-wasi-filesystem.md)'s model), and the class-splitting scheme that lets the large apps (qjs, sqlite3, ripgrep, CPython, CRuby) convert and run.
Numeric conventions are [decision 2](2-numeric-semantics.md)'s; this decision records where Java, statically typed with native fixed-width integers and floats but carrying a hard **64KB-per-method bytecode limit**, forced a different shape from the other compiled backend (Go, [decision 29](29-go-backend-lowering.md)) and from the interpreted ones.
Java is compiled, so it uses [decision 27](27-test-helper-crate.md)'s `run()` override.

## Context

Java gives the native-numerics wins Go has, but the JVM adds constraints no other target has: **a method is capped at 64KB of bytecode** (`<clinit>`/`<init>` included), a string constant at 65535 bytes, and a class's **constant pool at 65535 entries**.
cowsay blows the first two immediately (its largest function lowers to roughly 11.7k statements, its data segment is 69856 bytes), so the splitting [decision 10](10-csharp-target.md) predicted is implemented rather than deferred.
Two more facts shaped the design: **unreachable code is a compile error** (unlike Go, which tolerates it) while a *missing* return is also one, making Java's reachability model for labelled blocks fatal in both directions; and there are **no tuples, no multi-value returns, and no unsigned integer types**.

## Decision

- **Types.**
  i32/i64 to `int`/`long` as *signed bit patterns*, the hardware view rather than Ruby/Python's masked-unsigned convention (both denote the same residues), with unsigned ops via `Integer`/`Long.divideUnsigned` and friends: this makes wrapping arithmetic, shifts, sign-extension, and stores native.
  f32/f64 to `float`/`double`: Java is strict IEEE with **no implicit FMA contraction**, so re-rounding and trap-free division need no helper, making Java safer here than Go.
  Memory is a `byte[]` with a little-endian `ByteBuffer` view, chosen over VarHandles for simplicity, with effective addresses computed as unsigned `long` so an address plus offset past 2^31 is bounds-checked exactly.
- **Runtime helpers only where the JVM's operation is not wasm's**: integer div/rem (Java's `/` raises neither the `INT_MIN/-1` overflow trap nor wasm's divide-by-zero message); the trapping and unsigned-saturating `trunc_*` ops, since Java's float casts saturate and wrap, though the signed saturating forms *are* the cast; and the `Math.*` calls that are not NaN-faithful, where `abs`/`copysign` become sign-bit ops that cannot quiet a NaN, the rounding ops and `min`/`max` canonicalize a NaN result, `demote`/`promote` reconstruct the payload, and `convert_i64_u` uses round-to-odd.
  Wrapping add/sub/mul/div need nothing: HotSpot canonicalizes a NaN operand and never contracts.
- **Control flow uses the branch-register model ([decision 28](28-python-backend-lowering.md)'s Python design), not Java labelled blocks.**
  Block/if exits and the function return set a per-function `_br`, following siblings are guarded by `if (_br == 0)`, and only real loops become `while (true)` with a trailer turning `_br == <loop id>` into `continue`.
  Criterion: **the representation must split across methods** (below), and a language-level `break` cannot cross a method boundary while a data-carried `_br` can.
  It also dissolves the unreachable-statement landmine, since no bare mid-sequence `return`/`break` is emitted: the function return is register-based (`_ret = v; _br = -1;`) with one tail `return _ret`, and `Rt.trap`/`exit`/`link_error` are `void` methods that throw, emitted as plain statements.
  (Python emits a native `return` and lets the dead code be; Java cannot.)
- **The dynamic boundary is uniformly boxed.**
  A wasm function value is an `Rt.Fn` (`Object invoke(Object[])`) used for imports, `call_indirect` (a `Funcref` boxes a structural type string plus the `Fn`), and exports, while **direct calls to defined functions stay primitive**, confining boxing to the edge as Go's `any` boundary does; a multi-value signature returns a boxed `Object[]` for the same reason, a JVM method returning only one value.
  Every global is a boxed `Global` cell, shared rather than copied across an instantiation boundary (decision 16), and an instance's `Exports` map doubles as an [decision 7](7-import-providers.md) provider, which makes `register` and cross-module linking work.
- **An import source is a map or a provider.**
  The imports parameter is `Map<String, ?>`, whose value is either a `Map<String, Object>` of name to value or an `Rt.ImportProvider` (`Object wasmImport(String name)`) standing in for the module; the wildcard keeps it source-compatible, an embedder's existing `Map<String, Map<String, Object>>` still passing while a mixed map can carry a provider.
  `Rt.ImportProvider` also carries a `default void attach(Object instance)` called once the instance is built, so a provider reaches its memory without a hand-wired back-reference.
  Non-function imports are kind-checked by `instanceof` only.
  The bundled WASI is built on first *fallback*, not in the constructor and not on first call, as in Go and Ruby, so `wasi == null` is the honest observable.
- **The `Embedded` runtime is nested in the module class** ([decision 62](62-embedded-runtime-isolation.md)): top-level runtime classes meant two artifacts in one package fought over `Rt`/`Memory`/`Table`/`Global`/`WASI`, so the bundle is emitted as `static` **nested** classes, the shape `P{k}`/`Elem`/`Frame` already use.
  Java resolves a simple name through enclosing scopes, so unit bodies and every generated `Rt.trap(...)` are untouched and only outside references gain a qualifier (`Program.Rt.Exit`, `Add.Rt.Fn`).
  The `Alias` path deliberately keeps top-level runtime classes, so the spec harness's text is byte-identical and the multi-module shared-runtime composition keeps one runtime for both modules; the two shapes are two scope lists over the same units, differing only in `static`.
- **Exhaustion maps to `StackOverflowError`, which the JVM makes catchable**, unlike Go's fatal overflow that forced decision 29's recursion guard, so `check_exhaust` catches it as Ruby's harness catches `SystemStackError` and **no guard is instrumented into generated functions**, leaving shipped output unchanged by spec support.
  A mid-call overflow leaving an instance partly mutated is bounded because each assertion is independent and the stack unwinds fully before the next; the harness runs `java -Xss16m` so deep but terminating recursions stay under the limit while a runaway still overflows.
- **Execution (`run()` override).**
  The helper compiles `Main.java` with `javac` into a content-addressed class-dir cache and runs `java -cp <dir> Main`, which beats the `java Main.java` source launcher decisively on cowsay: the launcher recompiles in memory on **every** run (about 3.3 s each) against about 2 s once plus about 0.15 s warm.
  `$DEWASM_JAVA`/`$DEWASM_JAVAC` override the toolchain, and a missing one fails loud ([decision 15](15-tests-fail-not-skip.md)).
  One public class (`Main`) per file keeps the `javac` filename contract trivial; the runtime classes and the module class (`Program` in standalone mode, [decision 63](63-module-name-policy.md)) are package-private.
- **Feature scope**: wasm 1.0 and full WASI preview 1, `Floats` `Supported`, with decision 14's deliberate ENOSYS gaps, the same ones the other native backends carry.

### Splitting to fit the JVM's limits

Every split is conditional on size, so cowsay, the spec output, and even qjs/sqlite keep the plain single-class shape and only larger modules exercise the machinery.

- **Method split (`SPLIT_THRESHOLD`, an IR node count of 900).**
  A function over the threshold has its locals, temps, and `_br`/`_ret` registers hoisted into a per-call **frame object** and its body split at statement-sequence boundaries into numbered part methods.
  Because control flow is the `_br` register rather than a label, the parts are **called unconditionally**: an escaped branch makes each later part's guarded statements no-ops until the owning loop trailer or reset marker consumes it.
  A loop keeps its `while (true)` wrapper in the parent and chunks its body into parts called inside it, recursing into any sub-body over the threshold.
  cowsay needs it: 61 of its 640 functions split into roughly 1335 part methods.
- **A `br_table` splits at its case ranges**, statement boundaries not always being enough: CPython's largest interpreter function holds a 3202-target table, one statement whose `switch` alone exceeds 64KB (issue #142).
  It becomes a range dispatch (index read once, out-of-range index to the default, then an `if`/`else if` cascade into part methods that each `switch` over their own range), budgeted with the same threshold.
  The cost model counts one node per target: counting only a target's assignments made a thousands-of-targets table look free and left its function unsplit.
- **Data segments are chunked Base64** (`Rt.data_from_b64`, 32KB raw per chunk to stay under the string limit), each materialized in its own `initData{i}()` so `<init>` never accumulates data-init bytecode; hex was rejected as it doubles the constant size for no benefit.
  Honest finding: the predicted multi-MB overflow does not occur for the pinned binaries (ripgrep's largest segment is about 36 chunks at roughly 8 bytes of bytecode each), so one method would have sufficed; the per-segment split is kept as the general bound, always exercised and free.
- **Oversized element segments move to a nested `Elem` class.**
  ripgrep's 4915-entry funcref table blew both `<init>`'s limit and the module class's pool inline, so a segment over `ELEM_SPLIT` (1024 entries) is built by a nested class with its own pool via chunked part methods of `ELEM_PART` (512) entries, while qjs and sqlite (about 550) stay inline.
  One class is not enough for CRuby: each entry costs a pool roughly ten entries (a lambda's invokedynamic, method handle, and synthetic method, plus the method reference it calls), so its 8737-entry table saturated one pool alone and the fillers live in `ElemF{c}` classes of at most `ELEM_PER_CLASS` (2048) entries.
  **Reading javac's diagnosis:** CRuby reported "too many constants" 1059 times, once per class in the nested tree, but only the **first** is genuine, because after one class overflows `javac` repeats the error for every class it writes afterwards (confirmed with a two-class probe, the first oversized and the second trivial).
  Splitting the one oversized class cleared all 1059.
- **Oversized modules split their functions across nested `P{k}` classes.**
  Moving the element lambdas out was necessary but not sufficient for ripgrep: its roughly 7300 functions' own literals, method references, and names still overflow one pool.
  Over `FN_PARTITION_THRESHOLD` (2000) defined functions they become `static` methods over nested `P{k}` classes of `FN_PER_PARTITION` (1500) each, taking the module instance as their first parameter and called class-qualified.
  The threshold sits just above sqlite's proven single-class size (about 1970 functions), so a module partitions only once it exceeds the largest size measured to fit; zeroperl at about 2450 functions is constant-dense enough that it overflowed under the former 3000 bound.
  The conditioning is load-bearing for safety: with partitioning off the output is byte-identical to the unpartitioned shape, so the spec suite and qjs/sqlite stay on their proven path and only ripgrep-scale modules exercise the new one, validated by `rg_search_java`'s byte-identical snapshot (convert about 2 s, `javac` about 10 s over 5 partition classes).

### WASI: where Java's standard library forced a different shape

- **The fd table** is a `Map<Integer, Object>` of an `InputStream`/`OutputStream` (inherited stdio), a `Handle` (a guest-opened file over a seekable `FileChannel`), or a `Dir`.
  One `FileChannel` per file gives coherent read/write/seek/tell plus positional `pread`/`pwrite` that do not move the channel's own position; `O_APPEND` is reproduced by seeking to end before each write, FileChannel's APPEND option not combining with READ or TRUNCATE.
  Preopens are a constructor parameter assigned fds in sorted guest-path order for determinism.
- **The errno map is exception-typed, not errno-typed**, because NIO raises typed `IOException` subclasses (`NoSuchFileException` to ENOENT, `AccessDeniedException` to EACCES, and so on, everything else to EIO).
  The honest deviation from the backends reading a raw `errno`: Java exposes no distinct exception for EISDIR, ELOOP, or ENAMETOOLONG at open/stat time, so those surface as EIO unless a syscall detects them itself, which is why `path_remove_directory`/`path_unlink_file` pre-check `isDirectory` to reproduce rmdir's ENOTDIR and unlink's EISDIR that `Files.delete` would otherwise accept.
  Sandboxing is decision 14's discipline expressed with `Path` (join, `normalize()`, `toRealPath()` the parent, re-validate containment with `startsWith` on every call).
  `wasi_filetype` collapses devices and sockets to "unknown", which `BasicFileAttributes` cannot tell apart, and reports a piped stdin as filetype 4 and a tty as 2 via `System.console()`, so a guest's `isatty` stays false under the piped harness.

## Rejected alternatives

- **Java labelled blocks (`L: { … } break L;`)**: cleaner to read, but a `break L` cannot cross a method boundary, so oversized functions could not be split without first rewriting their control flow into exactly the register form adopted here.
  Correctness under the 64KB limit outranks readability ([decision 1](1-ir-design.md)), and labelled blocks reintroduce the unreachable-statement and missing-return tightrope.
- **A relooper or big-`switch` state machine**: a heavier rewrite for the same depth-insensitivity; decision 28 already proved the register model on cowsay.
- **One `callByIndex` dispatch method instead of per-entry lambdas**, and equally **a big `switch` instead of the `Elem` class**: a giant `switch` is itself a 64KB-method hazard needing its own split, and it does not address the class pool at all.
- **Native `(int)` casts for trapping float-to-int**: Java's cast saturates, so the trapping ops would silently saturate; accepted as an early gap and closed by the runtime helpers above.
- **A spec-build recursion guard (Go's counter)**: unnecessary given the catchable `StackOverflowError`, and it would change generated functions only for the spec build.
- **A per-signature record or tuple class for multi-value**: `Object[]` reuses the boxed dynamic-edge convention with no extra generated types, at autoboxing cost already paid at every import, indirect call, and export.
- **Typing the `Global` box (`Global<T>` or a value-type tag)**: Java generics cannot hold a primitive, and a tag would still not close the func-signature gap at the `Rt.Fn` boundary, so it only partially narrows an already-documented gap.
- **Inheritance (`Rg extends RgP1 extends RgP0`) to distribute functions without call-site qualification**: a base class cannot call a subclass's methods, so mutual recursion across partitions would not resolve, and declaring every function abstract in the base reinstates the pool pressure it was meant to relieve.
- **Always partitioning, or always routing element segments through `Elem`**: would change the generated code for already-passing outputs for no benefit; conditioning on size confines the risk to the modules that need it.

## Consequences

- cowsay is byte-identical to the wasmtime snapshot, and the standalone, library, WASI (including `Fs`), `gzip_e2e!`, and filesystem app suites match the same snapshots; `has_wasi_p1` derives true for the full surface, so `docs/support.md` puts Java at parity with the other native backends.
  Native integers and floats keep the arithmetic and runtime small, and strict IEEE avoids Go's FMA and folding workarounds entirely.
- The generated file is large and verbose (frame classes, part methods, per-statement `_br` guards), the boxed `Fn` boundary autoboxes at every import, indirect call, and export, and one binary yields many classes.
- **Java's import-limits gap is wider than Go's and equals Ruby's**: because a func value is one uniform `Rt.Fn` carrying no signature and a `Global` boxes an untyped `Object`, an import of the right *kind* but the wrong *type* (a mismatched func signature, a global's value type or mutability, a table/memory's limits) is not rejected.
  Its `EXPECTED_FAILURES` list therefore matches Ruby's (imports 28, imports2 2, linking 4) rather than Go's lower counts, where a typed assertion catches the func-signature and global-value-type cases.
  `linking0` (1) and `load1` (5) are the shared artefact of an unrelated multi-memory module that also uses `register`, not a cross-module-linking gap.
