# Decision 30: Java Backend Lowering Conventions

Status: **Accepted, 2026-07-26.**
Implemented in `crates/dewasm-backend-java/src/lib.rs` and `runtime/java/units/`.
The tests are `crates/dewasm-backend-java/tests/{spec,e2e,units}.rs`.
The implementation covers:

- wasm 1.0 with the spec harness passing ([decision 3](3-testing-strategy.md), [decision 16](16-ruby-wasm1-completion.md));
- full WASI preview 1 including the filesystem, adopting [decision 14](14-ruby-wasi-filesystem.md)'s model;
- the class-splitting scheme that lets the large apps convert and run.
  Those apps are qjs, sqlite3, ripgrep, CPython, and CRuby.

Numeric conventions are [decision 2](2-numeric-semantics.md)'s.
Java is statically typed with native fixed-width integers and floats.
It carries a hard **64KB-per-method bytecode limit**, though.
This decision records where Java was forced into a different shape from the other compiled backend.
That backend is Go ([decision 29](29-go-backend-lowering.md)); the interpreted ones differ too.
Java is compiled, so it uses [decision 27](27-test-helper-crate.md)'s `run()` override.

## Context

Java gives the native-numerics wins Go has.
The JVM adds constraints no other target has, though:

- **a method is capped at 64KB of bytecode** (`<clinit>`/`<init>` included);
- a string constant is capped at 65535 bytes;
- a class's **constant pool is capped at 65535 entries**.

cowsay breaks the first two immediately.
Its largest function lowers to roughly 11.7k statements, and its data segment is 69856 bytes.
So the splitting [decision 10](10-csharp-target.md) predicted is implemented rather than deferred.
Two more facts shaped the design.
First, **unreachable code is a compile error**, unlike Go, which tolerates it.
A *missing* return is also a compile error.
That makes Java's reachability model for labelled blocks fatal in both directions.
Second, there are **no tuples, no multi-value returns, and no unsigned integer types**.

## Decision

- **Types.**
  i32/i64 map to `int`/`long` as *signed bit patterns*.
  That is the hardware view rather than Ruby/Python's masked-unsigned convention.
  Both denote the same residues.
  Unsigned ops go via `Integer`/`Long.divideUnsigned` and friends.
  This makes wrapping arithmetic, shifts, sign-extension, and stores native.
  f32/f64 map to `float`/`double`.
  Java is strict IEEE with **no implicit FMA contraction**.
  So re-rounding and trap-free division need no helper.
  That makes Java safer here than Go.
  Memory is a `byte[]` with a little-endian `ByteBuffer` view, chosen over VarHandles for simplicity.
  Effective addresses are computed as unsigned `long`.
  So an address plus offset past 2^31 is bounds-checked exactly.
- **Runtime helpers exist only where the JVM's operation is not wasm's.**
  - Integer div/rem.
    Java's `/` raises neither the `INT_MIN/-1` overflow trap nor wasm's divide-by-zero message.
  - The trapping and unsigned-saturating `trunc_*` ops, since Java's float casts saturate and wrap.
    The signed saturating forms *are* the cast, though.
  - The `Math.*` calls that are not NaN-faithful:
    - `abs`/`copysign` become sign-bit ops that cannot quiet a NaN;
    - the rounding ops and `min`/`max` canonicalize a NaN result;
    - `demote`/`promote` reconstruct the payload;
    - `convert_i64_u` uses round-to-odd.

  Wrapping add/sub/mul/div need nothing: HotSpot canonicalizes a NaN operand and never contracts.
- **Control flow uses the branch-register model, not Java labelled blocks.**
  That model is [decision 28](28-python-backend-lowering.md)'s Python design.
  Block/if exits and the function return set a per-function `_br`.
  Following siblings are guarded by `if (_br == 0)`.
  Only real loops become `while (true)`, with a trailer turning `_br == <loop id>` into `continue`.
  Criterion: **the representation must split across methods** (below).
  A language-level `break` cannot cross a method boundary, while a data-carried `_br` can.
  It also dissolves the unreachable-statement landmine.
  That is because no bare mid-sequence `return`/`break` is emitted.
  The function return is register-based (`_ret = v; _br = -1;`) with one tail `return _ret`.
  `Rt.trap`/`exit`/`link_error` are `void` methods that throw, emitted as plain statements.
  (Python emits a native `return` and lets the dead code be; Java cannot.)
- **The dynamic boundary is uniformly boxed.**
  A wasm function value is an `Rt.Fn` (`Object invoke(Object[])`).
  It is used for imports, `call_indirect`, and exports.
  For `call_indirect`, a `Funcref` boxes a structural type string plus the `Fn`.
  **Direct calls to defined functions stay primitive**, though.
  That confines boxing to the edge, as Go's `any` boundary does.
  A multi-value signature returns a boxed `Object[]` for the same reason.
  A JVM method returns only one value.
  Every global is a boxed `Global` cell.
  It is shared rather than copied across an instantiation boundary (decision 16).
  An instance's `Exports` map doubles as a [decision 7](7-import-providers.md) provider.
  That makes `register` and cross-module linking work.
- **An import source is a map or a provider.**
  The imports parameter is `Map<String, ?>`.
  Its value is either a `Map<String, Object>` of name to value or an `Rt.ImportProvider`.
  The provider stands in for the module.
  The provider interface is `Object wasmImport(String name)`.
  The wildcard keeps the parameter source-compatible.
  An embedder's existing `Map<String, Map<String, Object>>` still passes.
  A mixed map can carry a provider.
  `Rt.ImportProvider` also carries a `default void attach(Object instance)`.
  It is called once the instance is built.
  So a provider reaches its memory without a hand-wired back-reference.
  Non-function imports are kind-checked by `instanceof` only.
  As in Go and Ruby, the bundled WASI is built on first *fallback*.
  It is not built in the constructor and not on first call.
  So `wasi == null` is the honest observable.
- **The `Embedded` runtime is nested in the module class** ([decision 62](62-embedded-runtime-isolation.md)).
  The runtime classes used to be top-level.
  Two artifacts in one package then fought over `Rt`/`Memory`/`Table`/`Global`/`WASI`.
  So the bundle is emitted as `static` **nested** classes, the shape `P{k}`/`Elem`/`Frame` already use.
  Java resolves a simple name through enclosing scopes.
  So unit bodies and every generated `Rt.trap(...)` are untouched.
  Only outside references gain a qualifier (`Program.Rt.Exit`, `Add.Rt.Fn`).
  The `Alias` path deliberately keeps top-level runtime classes.
  So the spec harness's text is byte-identical.
  The multi-module shared-runtime composition also keeps one runtime for both modules.
  The two shapes are two scope lists over the same units, differing only in `static`.
- **Exhaustion maps to `StackOverflowError`, which the JVM makes catchable.**
  This is unlike Go's fatal overflow, which forced decision 29's recursion guard.
  So `check_exhaust` catches it as Ruby's harness catches `SystemStackError`.
  **No guard is instrumented into generated functions**, so spec support leaves shipped output unchanged.
  A mid-call overflow can leave an instance partly mutated.
  That is bounded, because each assertion is independent and the stack unwinds fully before the next.
  The harness runs `java -Xss16m` so deep but terminating recursions stay under the limit.
  A runaway recursion still overflows.
- **Execution (`run()` override).**
  The helper compiles `Main.java` with `javac` into a content-addressed class-dir cache.
  It then runs `java -cp <dir> Main`.
  That beats the `java Main.java` source launcher decisively on cowsay.
  The launcher recompiles in memory on **every** run, about 3.3 s each.
  The cache costs about 2 s once plus about 0.15 s warm.
  `$DEWASM_JAVA`/`$DEWASM_JAVAC` override the toolchain, and a missing one fails loud ([decision 15](15-tests-fail-not-skip.md)).
  One public class (`Main`) per file keeps the `javac` filename contract trivial.
  The runtime classes and the module class are package-private.
  The module class is `Program` in standalone mode ([decision 63](63-module-name-policy.md)).
- **Feature scope**: wasm 1.0 and full WASI preview 1, `Floats` `Supported`.
  It has decision 14's deliberate ENOSYS gaps, the same ones the other native backends carry.

### Splitting to fit the JVM's limits

Every split is conditional on size.
So cowsay, the spec output, and even qjs/sqlite keep the plain single-class shape.
Only larger modules exercise the machinery.

- **Method split (`SPLIT_THRESHOLD`, an IR node count of 900).**
  A function over the threshold has its locals, temps, and `_br`/`_ret` registers hoisted.
  They move into a per-call **frame object**.
  Its body is split at statement-sequence boundaries into numbered part methods.
  Because control flow is the `_br` register rather than a label, the parts are **called unconditionally**.
  An escaped branch makes each later part's guarded statements no-ops.
  That lasts until the owning loop trailer or reset marker consumes it.
  A loop keeps its `while (true)` wrapper in the parent and chunks its body into parts called inside it.
  It recurses into any sub-body over the threshold.
  cowsay needs it: 61 of its 640 functions split into roughly 1335 part methods.
- **A `br_table` splits at its case ranges**, since statement boundaries are not always enough.
  CPython's largest interpreter function holds a 3202-target table (issue #142).
  It is one statement whose `switch` alone exceeds 64KB.
  It becomes a range dispatch, budgeted with the same threshold:
  - the index is read once;
  - an out-of-range index goes to the default;
  - then an `if`/`else if` cascade goes into part methods that each `switch` over their own range.

  The cost model counts one node per target.
  Counting only a target's assignments made a thousands-of-targets table look free.
  That left its function unsplit.
- **Data segments are chunked Base64** (`Rt.data_from_b64`).
  A chunk is 32KB raw, to stay under the string limit.
  Each is materialized in its own `initData{i}()`, so `<init>` never accumulates data-init bytecode.
  Hex was rejected, as it doubles the constant size for no benefit.
  Honest finding: the predicted multi-MB overflow does not occur for the pinned binaries.
  Ripgrep's largest segment is about 36 chunks at roughly 8 bytes of bytecode each.
  So one method would have sufficed.
  The per-segment split is kept as the general bound, always exercised and free.
- **Oversized element segments move to a nested `Elem` class.**
  ripgrep's 4915-entry funcref table exceeded both `<init>`'s limit and the module class's pool inline.
  So a segment over `ELEM_SPLIT` (1024 entries) is built by a nested class with its own pool.
  That class fills it via chunked part methods of `ELEM_PART` (512) entries.
  qjs and sqlite (about 550) stay inline.
  One class is not enough for CRuby: each entry costs a pool roughly ten entries.
  Those are a lambda's invokedynamic, method handle, and synthetic method.
  The method reference it calls adds one more.
  So its 8737-entry table saturated one pool alone.
  The fillers live in `ElemF{c}` classes of at most `ELEM_PER_CLASS` (2048) entries.
  **Reading javac's diagnosis:** CRuby reported "too many constants" 1059 times.
  That is once per class in the nested tree.
  Only the **first** is genuine.
  After one class overflows, `javac` repeats the error for every class it writes afterwards.
  A two-class probe confirmed it, the first class oversized and the second trivial.
  Splitting the one oversized class cleared all 1059.
- **Oversized modules split their functions across nested `P{k}` classes.**
  Moving the element lambdas out was necessary but not sufficient for ripgrep.
  Its roughly 7300 functions' own literals, method references, and names still overflow one pool.
  Over `FN_PARTITION_THRESHOLD` (2000) defined functions, they become `static` methods.
  Those methods sit in nested `P{k}` classes.
  Each class holds `FN_PER_PARTITION` (1500) functions.
  They take the module instance as their first parameter and are called class-qualified.
  The threshold sits just above sqlite's proven single-class size (about 1970 functions).
  So a module partitions only once it exceeds the largest size measured to fit.
  zeroperl has about 2450 functions.
  It is constant-dense enough that it overflowed under the former 3000 bound.
  The conditioning is load-bearing for safety.
  With partitioning off, the output is byte-identical to the unpartitioned shape.
  So the spec suite and qjs/sqlite stay on their proven path.
  Only ripgrep-scale modules exercise the new one.
  `rg_search_java`'s byte-identical snapshot validates it.
  Converting takes about 2 s, and `javac` about 10 s over 5 partition classes.

### WASI: where Java's standard library forced a different shape

- **The fd table** is a `Map<Integer, Object>`.
  A value is one of:
  - an `InputStream`/`OutputStream` (inherited stdio);
  - a `Handle` (a guest-opened file over a seekable `FileChannel`);
  - a `Dir`.

  One `FileChannel` per file gives coherent read/write/seek/tell.
  It also gives positional `pread`/`pwrite` that do not move the channel's own position.
  `O_APPEND` is reproduced by seeking to end before each write.
  That is because FileChannel's APPEND option does not combine with READ or TRUNCATE.
  Preopens are a constructor parameter assigned fds in sorted guest-path order for determinism.
- **The errno map is exception-typed, not errno-typed**, because NIO raises typed `IOException` subclasses.
  `NoSuchFileException` maps to ENOENT, `AccessDeniedException` to EACCES, and so on.
  Everything else maps to EIO.
  This is the honest deviation from the backends reading a raw `errno`.
  Java exposes no distinct exception for EISDIR, ELOOP, or ENAMETOOLONG at open/stat time.
  So those surface as EIO unless a syscall detects them itself.
  That is why `path_remove_directory`/`path_unlink_file` pre-check `isDirectory`.
  The check reproduces rmdir's ENOTDIR and unlink's EISDIR, which `Files.delete` would otherwise accept.
  Sandboxing is decision 14's discipline expressed with `Path`, applied on every call:
  - join and `normalize()`;
  - `toRealPath()` the parent;
  - re-validate containment with `startsWith`.

  `wasi_filetype` collapses devices and sockets to "unknown".
  That is because `BasicFileAttributes` cannot tell them apart.
  It reports a piped stdin as filetype 4 and a tty as 2 via `System.console()`.
  So a guest's `isatty` stays false under the piped harness.

## Rejected alternatives

- **Java labelled blocks (`L: { … } break L;`)**: cleaner to read.
  A `break L` cannot cross a method boundary, though.
  So oversized functions could not be split without first rewriting their control flow.
  That rewrite yields exactly the register form adopted here.
  Correctness under the 64KB limit outranks readability ([decision 1](1-ir-design.md)).
  Labelled blocks also reintroduce the unreachable-statement and missing-return tightrope.
- **A relooper or big-`switch` state machine**: a heavier rewrite for the same depth-insensitivity.
  Decision 28 already proved the register model on cowsay.
- **One `callByIndex` dispatch method instead of per-entry lambdas.**
  The same holds for **a big `switch` instead of the `Elem` class**.
  A giant `switch` is itself a 64KB-method hazard needing its own split.
  It does not address the class pool at all.
- **Native `(int)` casts for trapping float-to-int.**
  Java's cast saturates, so the trapping ops would silently saturate.
  It was accepted as an early gap and closed by the runtime helpers above.
- **A spec-build recursion guard (Go's counter)**: unnecessary given the catchable `StackOverflowError`.
  It would also change generated functions only for the spec build.
- **A per-signature record or tuple class for multi-value**: `Object[]` needs no extra generated types.
  It reuses the boxed dynamic-edge convention.
  Its autoboxing cost is already paid at every import, indirect call, and export.
- **Typing the `Global` box (`Global<T>` or a value-type tag)**: Java generics cannot hold a primitive.
  A tag would still not close the func-signature gap at the `Rt.Fn` boundary.
  So it only partially narrows an already-documented gap.
- **Inheritance (`Rg extends RgP1 extends RgP0`) to distribute functions without call-site qualification.**
  A base class cannot call a subclass's methods.
  So mutual recursion across partitions would not resolve.
  Declaring every function abstract in the base reinstates the pool pressure it was meant to relieve.
- **Always partitioning, or always routing element segments through `Elem`.**
  It would change the generated code for already-passing outputs for no benefit.
  Conditioning on size confines the risk to the modules that need it.

## Consequences

- cowsay is byte-identical to the wasmtime snapshot.
  The standalone, library, WASI (including `Fs`), and `gzip_e2e!` suites match the same snapshots.
  So does the filesystem app suite.
  `has_wasi_p1` derives true for the full surface.
  So `docs/support.md` puts Java at parity with the other native backends.
  Native integers and floats keep the arithmetic and runtime small.
  Strict IEEE avoids Go's FMA and folding workarounds entirely.
- The generated file is large and verbose: frame classes, part methods, per-statement `_br` guards.
  The boxed `Fn` boundary autoboxes at every import, indirect call, and export.
  One binary yields many classes.
- **Java's import-limits gap is wider than Go's and equals Ruby's.**
  A func value is one uniform `Rt.Fn` carrying no signature, and a `Global` boxes an untyped `Object`.
  So an import of the right *kind* but the wrong *type* is not rejected.
  That covers a mismatched func signature and a global's value type or mutability.
  It also covers a table/memory's limits.
  Its `EXPECTED_FAILURES` list therefore matches Ruby's (imports 28, imports2 2, linking 4).
  It does not match Go's lower counts.
  In Go, a typed assertion catches the func-signature and global-value-type cases.
  `linking0` (1) and `load1` (5) are the shared artefact of an unrelated multi-memory module.
  That module also uses `register`; it is not a cross-module-linking gap.
