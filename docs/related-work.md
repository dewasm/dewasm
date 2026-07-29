# Related Work

dewasm translates WebAssembly into **source code** of many languages from **one shared IR**. Its neighbors each hold one piece of that sentence; none holds all of it. They fall into three groups.

## Source-to-source translators — single target each

The closest relatives. Every one of them targets exactly one language, with its own IR, runtime, and test rig.

| Project | Target | Notes |
| --- | --- | --- |
| [wasm2c](https://github.com/WebAssembly/wabt/tree/main/wasm2c) (WABT) | C | Official. C99 output; imports are embedder-implemented functions taking the instance struct — the per-call-context design surveyed in [ADR-7](adr/7-import-providers.md). Its stack-flattening scheme is the model for our IR ([ADR-1](adr/1-ir-design.md)). |
| [w2c2](https://github.com/turbolent/w2c2) | C | The most mature translator: WASI support, 99.9% core spec pass rate. Its testing rigor is the bar [ADR-3](adr/3-testing-strategy.md) aims at. |
| [wasm2js](https://github.com/WebAssembly/binaryen/blob/main/src/tools/wasm2js.cpp) (Binaryen) | JavaScript | Official; asm.js-flavored output. Also why JS is deliberately *not* a dewasm target ([ADR-0](adr/0-foundation.md)). |
| [wasm2go](https://github.com/ncruces/wasm2go) | Go | |
| [unwasm](https://github.com/jasperweyne/unwasm) | PHP | |
| [wasm2lua](https://github.com/SwadicalRag/wasm2lua) | Lua | LuaJIT-oriented; the main prior art for translating into a dynamically-typed language. |

## Bytecode compilers — managed runtimes, but not source

These remove the wasm engine like dewasm does, but emit **bytecode for one VM**, not source code.

| Project | Output | Notes |
| --- | --- | --- |
| [asmble](https://github.com/cretz/asmble) | JVM bytecode | Archived. |
| [Chicory build-time compiler](https://chicory.dev/docs/usage/build-time-compiler/) | JVM bytecode | Active; part of the pure-Java Chicory runtime. |
| [wasm2cil](https://github.com/ericsink/wasm2cil) | .NET CIL assemblies | WASI support; work-in-progress, but notably ran SQLite and a raytracer on the CLR — prior art for "SQLite on a managed runtime via wasm", which dewasm pursues at the source level on Ruby (README north-star). Also why the C# backend ([ADR-10](adr/10-csharp-target.md)) still has an open niche: CIL is not C# source. |

Bytecode output is invisible to the target ecosystem's humans and tooling: it cannot be read, reviewed, patched, stepped through as ordinary code, or vendored into a codebase as a plain file — and it only exists where the VM has a bytecode story at all (there is none for Bash, and none for shipping a plain `.rb`/`.py` file).

## Runtimes — a different answer to the same question

wasmtime, wasmer, wazero, wasm3, Chicory's interpreter, and browser engines all *embed an engine* next to your program. dewasm's premise is to need no engine at run time: the translated program is native source in the host language, with a small runtime bundled inside it ([ADR-6](adr/6-runtime-units.md)).

## What dewasm adds

1. **One IR, many targets** ([ADR-0](adr/0-foundation.md), [ADR-1](adr/1-ir-design.md)). Every translator above is single-target. Here, adding a language is a lowering table plus runtime units plus turning the shared spec harness green ([ADR-3](adr/3-testing-strategy.md)) — the semantics knowledge (numerics, NaN bit-exactness, trap points) is paid for once ([ADR-2](adr/2-numeric-semantics.md)).
2. **Source output, deliberately.** Readable, reviewable, debuggable, vendorable as a file; no build toolchain or VM contract at the run site.
3. **Targets that cannot run wasm any other way.** The flagship is Bash ([ADR-5](adr/5-bash-softfloat.md)): C/Rust tools running where the only dependency is a shell.
4. **Deployment-grade output, not demo output.** Minimal runtime bundling per module and collision-free coexistence of generated artifacts ([ADR-6](adr/6-runtime-units.md)); a library mode with import providers and a default WASI fallback ([ADR-7](adr/7-import-providers.md)).
5. **Declared, enforced fidelity.** The official testsuite runs on the real target interpreters, and every skipped test must be attributable to a feature declared unsupported in the generated [support matrix](support.md) ([ADR-8](adr/8-latest-testsuite-support-matrix.md)).

dewasm stands on this prior work: wasm2c's translation scheme, w2c2's proof that spec-level fidelity is reachable, and the import-binding designs of Node's WASI, wasm2c, and the major runtimes are all cited in the ADRs above.
