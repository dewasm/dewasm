# Decision 0: Foundation and Core Architecture

Status: **Accepted, 2026-07-23.**
Backfilled; the decision was made during initial planning and implementation.
The core pipeline, the Ruby backend, and the spec harness are implemented.
Other backends are planned.
Amended by [decision 10](10-csharp-target.md): C# joins the target list, paired with Java.

## Context

Programs compiled to WebAssembly (from C, C++, Rust, ...) normally need a wasm runtime to execute.
Existing source-to-source translators remove that requirement, but each targets one language:

- wasm2c / w2c2 (C)
- wasm2js (JavaScript)
- wasm2go (Go)
- unwasm (PHP)
- wasm2lua (Lua)

No tool covers multiple target languages from one codebase.
dewasmify's goal is to translate wasm binaries into source code of *many* languages.
A tool built once (e.g. in Rust) can then run anywhere the target language runs.
That includes places with no wasm runtime at all, such as a plain Bash environment.

## Decision

- **One shared IR, pluggable language backends.**
  The core decodes, validates, and builds a language-neutral IR (decision 1).
  Each backend is a lowering of that IR plus an embedded lightweight runtime.
  That runtime is written in the target language (`runtime/<lang>/`).
  Adding a language must not require touching the core.
- **Implementation language: Rust**, using the Bytecode Alliance crates.
  They are `wasmparser` for decoding/validation, and `wast`/`wat` for the test pipeline.
  Rust also keeps the self-hosting demo possible.
  In that demo, dewasmify is compiled to wasm, then translated by itself.
- **First-release input scope: Wasm core 1.0, the default C/Rust extensions, and WASI preview 1.**
  The extensions are those C/Rust toolchains enable by default.
  They are mutable globals, sign-extension, saturating float-to-int, multi-value, and bulk memory.
  Out of scope: reference types, SIMD, threads, GC, multiple memories/tables, cross-module linking.
  They must be rejected with a clear error.
- **Two output modes**:
  - *library*: a class/module instantiated with an imports object.
    Its exports are exposed to the host language.
  - *standalone*: WASI provided, `_start` invoked, and the exit code mapped.
- **Target language priority: Ruby → Bash → Java → Go → Python → PHP.**
  Bash is the defining demonstration: it runs C/Rust tools with no hardware-specific binary at all.
  Ruby went first to validate the pipeline (implemented).
  JavaScript is deliberately absent.
- **Name: `dewasmify`**: the tool strips ("de-") the wasm out of a program.
  Crate, CLI, and repository share the name.
  *Amended by [decision 26](26-rename-dewasm.md) (2026-07-25): renamed to `dewasm`.*

## Rejected alternatives

- **Contributing to / forking the single-language translators**.
  They are six divergent codebases with different IRs and test rigs.
  The shared-IR economics are the point of this project.
- **JavaScript as an early target**: wasm2js exists, and every JS runtime ships a wasm engine.
  Little value would be added.
  May be revisited later.
- **Naming the project `wasmify`**: reads as "convert *to* wasm" and collides with an existing npm package.

## Consequences

- Positive: each backend is "lowering table + runtime + pass the shared harness" (decision 3).
  Semantics knowledge concentrates in the IR and the per-language numeric strategy (decision 2).
- Negative: unsupported-feature errors are part of the UX until Wasm 2.0+ features land.
  Modules using them fail at conversion time, not runtime.
- The milestone plan follows this priority order:
  M0 core → M1 Ruby → M2 WASI → M3 Bash → M4 Java/Go → M5 Python/PHP → M6 release/self-hosting.
