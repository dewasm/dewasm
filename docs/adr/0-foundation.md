# ADR-0 — Foundation and Core Architecture

Status: **Accepted, 2026-07-23.**
Backfilled; the decision was made during initial planning and implementation.
The core pipeline, the Ruby backend, and the spec harness are implemented; other backends are planned.
Amended by [ADR-10](10-csharp-target.md): C# joins the target list, paired with Java.

## Context

Programs compiled to WebAssembly (from C, C++, Rust, ...) normally need a wasm runtime to execute.
Existing source-to-source translators remove that requirement but each targets exactly one language — wasm2c / w2c2 (C), wasm2js (JavaScript), wasm2go (Go), unwasm (PHP), wasm2lua (Lua) — and no tool covers multiple target languages from one codebase.
dewasmify's goal is to translate wasm binaries into source code of *many* languages, so a tool built once (e.g. in Rust) can run anywhere the target language runs — including places with no wasm runtime at all, such as a plain Bash environment.

## Decision

- **One shared IR, pluggable language backends.**
  The core decodes, validates, and builds a language-neutral IR (ADR-1); each backend is a lowering of that IR plus an embedded lightweight runtime written in the target language (`runtime/<lang>/`).
  Adding a language must not require touching the core.
- **Implementation language: Rust**, using the Bytecode Alliance crates (`wasmparser` for decoding/validation, `wast`/`wat` for the test pipeline).
  Rust also keeps the self-hosting demo possible: dewasmify itself compiled to wasm, then translated by itself.
- **First-release input scope: Wasm core 1.0 plus the extensions C/Rust toolchains enable by default** (mutable globals, sign-extension, saturating float-to-int, multi-value, bulk memory), and **WASI preview 1**.
  Reference types, SIMD, threads, GC, multiple memories/tables, and cross-module linking are out of scope and must be rejected with a clear error.
- **Two output modes**: *library* (a class/module instantiated with an imports object, exports exposed to the host language) and *standalone* (WASI wired up, `_start` invoked, exit code mapped).
- **Target language priority: Ruby → Bash → Java → Go → Python → PHP.**
  Bash is the defining demonstration (running C/Rust tools with no hardware-specific binary at all); Ruby went first to validate the pipeline (implemented).
  JavaScript is deliberately absent.
- **Name: `dewasmify`** — the tool strips ("de-") the wasm out of a program.
  Crate, CLI, and repository share the name.
  *Amended by [ADR-26](26-rename-dewasm.md) (2026-07-25): renamed to `dewasm`.*

## Rejected alternatives

- **Contributing to / forking the single-language translators** — six divergent codebases with different IRs and test rigs; the shared-IR economics are the point of this project.
- **JavaScript as an early target** — wasm2js exists and every JS runtime ships a wasm engine; little value added.
  May be revisited later.
- **Naming the project `wasmify`** — reads as "convert *to* wasm" and collides with an existing npm package.

## Consequences

- Positive: each backend is "lowering table + runtime + pass the shared harness" (ADR-3); semantics knowledge concentrates in the IR and the per-language numeric strategy (ADR-2).
- Negative: unsupported-feature errors are part of the UX until Wasm 2.0+ features land; modules using them fail at conversion time, not runtime.
- The milestone plan (M0 core → M1 Ruby → M2 WASI → M3 Bash → M4 Java/Go → M5 Python/PHP → M6 release/self-hosting) follows this priority order.
