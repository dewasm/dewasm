# Decision 7: Import Providers and the Default WASI Fallback

Status: **Accepted, 2026-07-23.**
Implemented for Ruby:

| Part | Implementation |
| --- | --- |
| The provider protocol and resolution order | generated `initialize` (`crates/dewasm-backend-ruby/src/lib.rs`) |
| `Rt.resolve_import` | `runtime/ruby/units/rt/resolve_import.rb` |
| `Rt::WASI` implementing the protocol itself | `runtime/ruby/units/wasi/_class.rb` |

[Decision 16](16-ruby-wasm1-completion.md) generalized this to non-function imports.
It also generalized it to generated classes implementing the protocol themselves.

## Context

Imports could only be supplied as a Hash of per-function callables.
That made a whole-runtime replacement impractical.
A WASI implementation is coupled to the guest memory.
But the memory exists only after `new` returns, while imports must go *into* `new`.
This is the classic instantiation circularity.
Library mode also refused to instantiate WASI-importing modules at all.
The exception was an embedder that hand-implemented preview 1.

Survey of how real systems break the circularity:

- **Node.js `node:wasi`**: a WASI object yields the import object.
  `wasi.start(instance)` binds the exported memory before execution.
  This is a provider with a bind step.
- **wasm2c / w2c2** (source translators): per-call context.
  Every embedder-implemented import receives the module instance as its first argument.
  An example is `u32 w2c_host_fill_buf(w2c_host* instance, ...)`.
- **wasmtime / wazero / Chicory**: host functions receive a per-call context.
  The context is Caller / api.Module / Instance, and they fetch memory from it.

## Decision

- **Provider protocol.**
  A value in the imports table is either a Hash (name → callable, unchanged) or a *provider*.
  A provider is an object with `import(name) → callable | nil` and optionally `attach(instance)`.
  Generated `initialize` calls `attach` once the instance is fully constructed, before the start function.
  So `attach` runs before any wasm code can invoke an import.
  Passing the *instance* rather than just the memory is Node's bind step generalized.
  It covers wasm2c's power: providers can reach exports too.
- **Resolution order per imported function**:

  1. explicit entry from the embedder;
  2. bundled WASI (for `wasi_snapshot_preview1`, when enabled);
  3. ENOSYS stub for syscalls dewasmify has not implemented.

  Non-WASI imports remain mandatory (`Rt::LinkError`).
  A present import of the wrong kind also raises `Rt::LinkError`.
- **The bundled WASI is constructed only when needed.**
  The first fallback resolution runs `@wasi ||= Rt::WASI.new(args:, env:)`.
  If the embedder covers every WASI import, no instance is created.
  Its side effects (stdio `binmode`) then never happen.
  Unimplemented syscalls resolve to stubs at generation time, so they cannot trigger construction.
- `Rt::WASI` implements the provider protocol itself.
  So a custom WASI runtime is two methods away from a wholesale swap.
  The standalone main also collapses to `Klass.new({}, args:, env:)` + `invoke("_start")`.
- `--no-default-wasi` (library mode only) disables the fallback.
  It serves embedders that want zero ambient authority.
  In library mode, `proc_exit` surfaces as `Klass::Rt::Exit` for the embedder to rescue.

## Rejected alternatives

- **Per-call context argument on every import callable** (wasm2c style).
  It taxes the common case (plain lambdas, the spectest harness) with a changed signature.
  Ruby closures plus `attach` reach the same power.
- **Strict imports only (status quo)**: makes WASI-importing modules unusable as libraries in practice.
- **Memory-name binding only** (Node's `start` looks up `exports.memory`).
  `attach(instance)` is a superset and needs no export-name contract.
- **Unconditional eager construction of the bundled WASI.**
  It pays the `binmode` side effect and an object even when the embedder provided everything.
- **Deferring construction to the first syscall invocation.**
  It adds lambda indirection for little gain over construction-time laziness (user call).

## Consequences

- Positive: `Hello.new` works out of the box for WASI programs in library mode.
  Custom runtimes swap in wholesale.
  The spec harness needed no changes (Hash path untouched).
- Negative / caveat: this concerns minimal Embedded bundles.
  There, `instance.memory` only carries the typed accessors the module itself uses.
  The stable surface a provider may rely on is `memory.bytes` (a mutable binary String).
  A fuller guaranteed API is a carry-over for the shared/gem linkage (decision 6).
- The provider protocol is intentionally not WASI-specific.
  Any import namespace can be served by a provider with instance access.
  An emscripten-style `env` is one example.
