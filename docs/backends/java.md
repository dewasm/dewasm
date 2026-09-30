# Java backend

`--target java`.
One `.java` source file compiled with `javac` and run on the JVM.
Very large modules are split across multiple classes.

## Output shape

A single `.java` file holds the generated module class.
The runtime (`Rt`/`Memory`/`Table`/`WASI`) sits inside it as `static` nested classes.
Nesting lets two converted artifacts share one package without a name conflict between runtimes.
Each one's trap type is its own.
So from outside the module class they are spelled `Add.Rt.Fn`, `Add.Rt.Trap`, and `Add.WASI`.
In **standalone** mode the entry point is always a `public class Main` with `public static void main`.
So name the output `Main.java`.
The module class is always `Program`, and `--module-name` is rejected.
In **library** mode the module class is package-private.
So put your own `public class Main` (or other public entry) in the *same* file.

Library mode requires `--module-name` and takes it unchanged as a dot-separated name.
The last segment is the class name.
Anything before it becomes the file's `package` declaration.
So `--module-name com.github.dewasm.Sqlite3` gives `package com.github.dewasm;` and `class Sqlite3`.
Your `Main`, added at the end of the file, then shares that package.
Every segment must match `[A-Za-z_$][A-Za-z0-9_$]*`.
Anything else is a conversion-time error, and nothing is rewritten.
Java keywords pass the character-level grammar and fail in `javac` with the compiler's own message.

Integers are native `int`/`long` as bit patterns; unsigned operations use `Integer.*`/`Long.*`.
Control flow uses a per-function branch register `_br`.

## Requirements

`java` and `javac` on `PATH`, **JDK 11 or newer** (standard APIs only).

## Running it

Standalone (entry class `Main`):

```console
$ dewasm prog.wasm --target java --mode standalone -o Main.java
$ javac Main.java && java Main --dir ./data::/data arg1 arg2
```

Standalone programs share one runtime interface: `argv`, `--dir` preopens, environment, exit/trap.
It is described in [`docs/standalone-interface.md`](../standalone-interface.md).
Java is the one deviation on `argv[0]`.
The JVM does not pass the launched file name to `main`, so Java uses the module class name.
In standalone mode that name is the fixed `Program`.

Library: add your `public class Main` at the end of the generated file.
Or put it beside the generated file in the same package.
Constructor arguments are `(imports, argv, env, preopens)`.
Exports are `<Class>.Rt.Fn` values in the `Exports` map:

```java
Add inst = new Add(null, null, null, null);
System.out.println((int)(Integer)((Add.Rt.Fn) inst.Exports.get("add")).invoke(new Object[]{2, 3})); // 5
```

```console
$ dewasm add.wat --target java --mode library --module-name Add -o Main.java
$ javac Main.java && java Main   # after appending the class above
```

`proc_exit` is surfaced as a runtime exception (`<Class>.Rt.Exit`, with `.code`).
Catch it if you drive `_start`.

## Capabilities

Full wasm core 1.0 plus the universal baseline.
**Full WASI Preview 1 including the file system**, adopting the Ruby file system model.
Non-function imports, multiple tables, and table bulk operations are supported.
The final exception-handling proposal is supported.
A thrown wasm exception is a native exception carrying its tag.
`catch_all` cannot observe traps.
C programs based on `setjmp`/`longjmp` convert and run; `mruby` is the covered app case.
The official support table: [`docs/support.md`](../support.md).

## Providers and library usage

Any unprovided WASI import falls back to a bundled WASI.
The bundled WASI is built the first time an import falls back to it.
Cover every WASI import and none is ever constructed.
Override imports by passing a `Map<String, ?>` of module → source to the constructor.
Preopen directories via the fourth argument (`Map.of(guest, host)`).
A source is one of two things:

- a `Map<String, Object>` of name → value.
  So the older `Map<String, Map<String, Object>>` shape still passes.
- an object implementing `<Class>.Rt.ImportProvider`, which resolves names itself.
  Its method is `Object wasmImport(String name)`.

`ImportProvider` has a default method `attach(Object instance)`.
It is called once the instance is built.
So a provider can reach its memory.
The e2e override and custom-provider glues are the worked reference.
They are in `crates/dewasm-backend-java/tests/e2e.rs`.

## Limits

- **Class splitting at scale.**
  The JVM caps three sizes:
  - a method at 64 KB of bytecode;
  - a class's constant pool at 65535 entries;
  - a string literal at 64 KB.

  dewasm handles all three automatically:
  - large functions are split into numbered `part` methods over a per-call frame object;
  - huge modules are partitioned across nested `P{k}` classes, and a large binary spans several;
  - data segments that are too large are emitted as chunked Base64.

  This needs nothing from the user, but explains why one binary yields many classes.
- **Compile cost** is the slow step for large modules, as in Go.
  The e2e suite compiles to a content-addressed cache directory of classes to pay `javac` once.
