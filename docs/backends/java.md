# Java backend

`--target java`.
One `.java` source file compiled with `javac` and run on the JVM; splits very large modules across multiple classes.

## Output shape

A single `.java` file: the generated module class, with the runtime (`Rt`/`Memory`/`Table`/`WASI`) as `static` nested classes inside it.
Nesting is what lets two converted artifacts sit in one package without their runtimes colliding, each one's trap type being its own, so from outside the module class they are spelled `Add.Rt.Fn`, `Add.Rt.Trap`, `Add.WASI`.
In **standalone** mode the entry point is always a `public class Main` with `public static void main`, so name the output `Main.java`; the module class is always `Program` and `--module-name` is rejected.
In **library** mode the module class is package-private, so put your own `public class Main` (or other public entry) in the *same* file.

The library-mode name is `--module-name` (required in library mode) taken verbatim, dot-separated: the last segment is the class name, anything before it becomes the file's `package` declaration, so `--module-name com.github.dewasm.Sqlite3` gives `package com.github.dewasm;` and `class Sqlite3` (your appended `Main` then shares that package).
Every segment must match `[A-Za-z_$][A-Za-z0-9_$]*`; anything else is a conversion-time error, nothing is sanitized (Java keywords pass the character-level grammar and fail in `javac` with the compiler's own message).

Integers are native `int`/`long` as bit patterns; unsigned ops use `Integer.*`/`Long.*`.
Control flow uses a per-function branch register `_br`.

## Requirements

`java` and `javac` on `PATH`, **JDK 11 or newer** (standard APIs only).

## Running it

Standalone (entry class `Main`):

```console
$ dewasm prog.wasm --target java --mode standalone -o Main.java
$ javac Main.java && java Main --dir ./data::/data arg1 arg2
```

Standalone programs follow the shared runtime interface (argv, `--dir` preopens, env, exit/trap): [docs/standalone-interface.md](../standalone-interface.md).
Java is the one deviation on `argv[0]`: the JVM does not pass the launched file name to `main`, so it uses the module class name, which in standalone mode is the fixed `Program`.

Library (append your `public class Main` to the generated file, or drop it beside it in the same package).
Constructor arguments are `(imports, argv, env, preopens)`; exports are `<Class>.Rt.Fn` values in the `Exports` map:

```java
Add inst = new Add(null, null, null, null);
System.out.println((int)(Integer)((Add.Rt.Fn) inst.Exports.get("add")).invoke(new Object[]{2, 3})); // 5
```

```console
$ dewasm add.wat --target java --mode library --module-name Add -o Main.java
$ javac Main.java && java Main   # after appending the class above
```

`proc_exit` is surfaced as a runtime exception (`<Class>.Rt.Exit`, with `.code`); catch it if you drive `_start`.

## Capabilities

Full wasm core 1.0 plus the universal baseline, and **full WASI preview 1 including the filesystem** (adopting the Ruby fs model).
Non-function imports, multiple tables, and table bulk ops are supported.
Authoritative matrix: [docs/support.md](../support.md).

## Providers and library usage

Any unprovided WASI import falls back to a bundled WASI, which is built the first time an import actually falls back to it: cover every WASI import and none is ever constructed.
Override imports by passing a `Map<String, ?>` of module → source to the constructor; preopen directories via the fourth argument (`Map.of(guest, host)`).
A source is either a `Map<String, Object>` of name → value (so the older `Map<String, Map<String, Object>>` shape still passes), or an object implementing `<Class>.Rt.ImportProvider` (`Object wasmImport(String name)`) that resolves names itself; its `attach(Object instance)` default method is called once the instance is fully built, so a provider can reach its memory.
The e2e override and custom-provider glues in `crates/dewasm-backend-java/tests/e2e.rs` are the worked reference.

## Caveats

- **Class splitting at scale.**
  The JVM caps a method at 64 KB of bytecode, a class's constant pool at 65535 entries, and a string literal at 64 KB.
  dewasm handles all three automatically: large functions are split into numbered `part` methods over a per-call frame object, huge modules are partitioned across nested `P{k}` classes (a large binary spans several), and oversized data segments are emitted as chunked Base64.
  This is transparent but explains why one binary yields many classes.
- **Compile cost** is the slow step for large modules (like Go); the e2e suite compiles to a content-addressed class-dir cache to pay `javac` once.
