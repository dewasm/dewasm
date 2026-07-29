# Java backend

`--target java`. One `.java` source file compiled with `javac` and run on the JVM; splits very large modules across multiple classes.

## Output shape

A single `.java` file: a set of package-private runtime classes (`Rt`/`Memory`/`Table`/`WASI`) plus the generated module class named after the input stem. In **standalone** mode the entry point is always a `public class Main` with `public static void main`, so name the output `Main.java`. In **library** mode the module class is package-private, so put your own `public class Main` (or other public entry) in the *same* file. Integers are native `int`/`long` as bit patterns; unsigned ops use `Integer.*`/`Long.*`. Control flow uses a per-function branch register `_br`. See [ADR-30](../adr/30-java-backend-lowering.md).

## Requirements

`java` and `javac` on `PATH` (or `$DEWASM_JAVA`/`$DEWASM_JAVAC`), **JDK 11 or newer** (standard APIs only).

## Running it

Standalone (entry class `Main`):

```console
$ dewasm prog.wasm --target java --mode standalone -o Main.java
$ javac Main.java && java Main --dir ./data::/data arg1 arg2
```

Standalone programs follow the shared runtime interface (argv, `--dir` preopens, env, exit/trap): [docs/standalone-interface.md](../standalone-interface.md). Java is the one deviation on `argv[0]`: the JVM does not pass the launched file name to `main`, so it uses the module class name.

Library (append your `public class Main` to the generated file). Constructor arguments are `(imports, argv, env, preopens)`; exports are `Rt.Fn` values in the `Exports` map:

```java
Add inst = new Add(null, null, null, null);
System.out.println((int)(Integer)((Rt.Fn) inst.Exports.get("add")).invoke(new Object[]{2, 3})); // 5
```

```console
$ dewasm add.wat --target java --mode library --module-name Add -o Main.java
$ javac Main.java && java Main   # after appending the class above
```

`proc_exit` is surfaced as a runtime exception; catch it if you drive `_start`.

## Capabilities

Full wasm core 1.0 plus the universal baseline, and **full WASI preview 1 including the filesystem** (adopting the Ruby fs model, [ADR-14](../adr/14-ruby-wasi-filesystem.md)). Non-function imports, multiple tables, and table bulk ops are supported. Authoritative matrix: [docs/support.md](../support.md).

## Providers and library usage

Any unprovided WASI import falls back to a bundled WASI. Override imports by passing a `Map<String, Map<String, Object>>` to the constructor; preopen directories via the fourth argument (`Map.of(guest, host)`). The e2e override glue in `crates/dewasm-backend-java/tests/e2e.rs` is the worked reference.

## Caveats

- **Class splitting at scale.** The JVM caps a method at 64 KB of bytecode, a class's constant pool at 65535 entries, and a string literal at 64 KB. dewasm handles all three automatically — large functions are split into numbered `part` methods over a per-call frame object, huge modules are partitioned across nested `P{k}` classes (ripgrep's ~7300 functions span five), and oversized data segments are emitted as chunked Base64. This is transparent but explains why one binary yields many classes ([ADR-30](../adr/30-java-backend-lowering.md)).
- **Compile cost** is the slow step for large modules (like Go); the e2e suite compiles to a content-addressed class-dir cache to pay `javac` once.
