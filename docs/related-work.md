# Related Work

dewasm translates WebAssembly into **source code** of many languages from **one shared IR**.
The prior art falls into three groups:

- source-to-source compilers, each with a single target;
- compilers that emit bytecode for one VM;
- runtimes that embed an engine.

## Source-to-source compilers: single target each

The closest relatives.
Every one of them targets exactly one language, with its own IR, runtime, and test harness.

| Project | Target | Notes |
| --- | --- | --- |
| [wasm2c](https://github.com/WebAssembly/wabt/tree/main/wasm2c) (WABT) | C | Official. C99 output; imports are embedder-implemented functions taking the instance structure, a per-call-context design dewasm surveyed before settling on import providers. Its stack-flattening scheme is the model for our IR. |
| [w2c2](https://github.com/turbolent/w2c2) | C | The most mature source-to-source compiler: WASI support, 99.9% core specification pass rate. Its strict testing is the bar dewasm aims at. |
| [wasm2js](https://github.com/WebAssembly/binaryen/blob/main/src/tools/wasm2js.cpp) (Binaryen) | JavaScript | Official; asm.js-flavored output. Also why JS is deliberately *not* a dewasm target. |
| [wasm2go](https://github.com/ncruces/wasm2go) | Go | |
| [`unwasm`](https://github.com/jasperweyne/unwasm) | PHP | |
| [wasm2lua](https://github.com/SwadicalRag/wasm2lua) | Lua | LuaJIT-oriented; the main prior art for translating into a dynamically-typed language. |

## Bytecode compilers: managed runtimes, but not source

These remove the wasm engine like dewasm does, but emit **bytecode for one VM**, not source code.

| Project | Output | Notes |
| --- | --- | --- |
| [`asmble`](https://github.com/cretz/asmble) | JVM bytecode | Archived. |
| [Chicory build-time compiler](https://chicory.dev/docs/usage/build-time-compiler/) | JVM bytecode | Active; part of the Chicory runtime, written in Java with no native code. |
| [wasm2cil](https://github.com/ericsink/wasm2cil) | .NET CIL assemblies | WASI support; work-in-progress, but notably ran SQLite and a ray tracer on the CLR, prior art for "SQLite on a managed runtime via wasm", which dewasm pursues at the source level on Ruby (the README's stated goal). Also why the planned C# backend still has an open niche: CIL is not C# source. |

Bytecode is hidden from the target language's tooling.
It cannot be read, patched, or vendored into a repository as a plain file.
It also only exists where the VM has a bytecode format at all.
That rules out Bash and shipping a plain `.rb`/`.py`.

## Runtimes: a different answer to the same question

Each of these runtimes *embeds an engine* next to your program.
They are Wasmtime, Wasmer, `wazero`, wasm3, Chicory's interpreter, and browser engines.
dewasm's premise is to need no engine at run time.
The translated program is native source in the host language.
A small runtime is bundled inside it.

## What dewasm adds

1. **One IR, many targets.**
   Every source-to-source compiler above is single-target.
   Here, a new language needs a lowering table and runtime units.
   Then it must pass the shared specification harness.
   So the semantics knowledge (numerics, NaN bit-exactness, trap points) is paid for once.
2. **Source output, deliberately.**
   The output is readable, reviewable, debuggable, and vendorable as a file.
   The run site needs no build toolchain or VM contract.
3. **Targets that cannot run wasm any other way.**
   The defining example is Bash: C/Rust tools running where the only dependency is a shell.
4. **Output for production use, not example output.**
   It bundles a minimal runtime per module.
   Generated artifacts load into one namespace without a name conflict.
   A library mode offers import providers and a default WASI fallback.
5. **Declared, checked conformance.**
   The official testsuite runs on the real target interpreters.
   Every skipped test must be attributable to a feature declared unsupported.
   The generated [support matrix](support.md) holds those declarations.

dewasm stands on this prior work.
These all shaped dewasm's own design:

- wasm2c's translation scheme;
- w2c2's proof that conformance to the specification is reachable;
- the import-binding designs of Node's WASI, wasm2c, and the major runtimes.
