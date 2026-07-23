# ADR-7 — Import Providers and the Default WASI Fallback

Status: **Accepted, 2026-07-23.** Implemented for Ruby: the provider
protocol and resolution order in generated `initialize`
(`crates/dewasmify-backend-ruby/src/lib.rs`), `Rt.resolve_import`
(`runtime/ruby/units/rt/resolve_import.rb`), and `Rt::WASI` implementing
the protocol itself (`runtime/ruby/units/wasi/_class.rb`). Generalized to
non-function imports and to generated classes implementing the protocol
themselves by [ADR-16](16-ruby-wasm1-completion.md).

## Context

Imports could only be supplied as a Hash of per-function callables, which
made a whole-runtime replacement impractical: a WASI implementation is
coupled to the guest memory, but the memory exists only after `new`
returns while imports must go *into* `new` — the classic instantiation
circularity. Library mode also refused to instantiate WASI-importing
modules at all unless the embedder hand-implemented preview 1.

Survey of how real systems break the circularity:

- **Node.js `node:wasi`**: a WASI object yields the import object;
  `wasi.start(instance)` binds the exported memory before execution — a
  provider with a bind step.
- **wasm2c / w2c2** (source translators): every embedder-implemented
  import receives the module instance as its first argument
  (`u32 w2c_host_fill_buf(w2c_host* instance, ...)`) — per-call context.
- **wasmtime / wazero / Chicory**: host functions receive a per-call
  context (Caller / api.Module / Instance) and fetch memory from it.

## Decision

- **Provider protocol.** A value in the imports table is either a Hash
  (name → callable, unchanged) or a *provider*: an object with
  `import(name) → callable | nil` and optionally `attach(instance)`.
  Generated `initialize` calls `attach` once the instance is fully
  constructed, before the start function (and thus before any wasm code
  can invoke an import). Passing the *instance* rather than just the
  memory is Node's bind step generalized to cover wasm2c's power
  (providers can reach exports too).
- **Resolution order per imported function**: explicit entry from the
  embedder → bundled WASI (for `wasi_snapshot_preview1`, when enabled) →
  ENOSYS stub for syscalls dewasmify has not implemented; non-WASI
  imports remain mandatory (`ArgumentError`).
- **The bundled WASI is constructed only when needed**: the first
  fallback resolution runs `@wasi ||= Rt::WASI.new(args:, env:)`. If the
  embedder covers every WASI import, no instance is created and its side
  effects (stdio `binmode`) never happen. Unimplemented syscalls resolve
  to stubs at generation time, so they cannot trigger construction.
- `Rt::WASI` implements the provider protocol itself, so a custom WASI
  runtime is two methods away from a wholesale swap, and the standalone
  main collapses to `Klass.new({}, args:, env:)` + `invoke("_start")`.
- `--no-default-wasi` (library mode only) disables the fallback for
  embedders that want zero ambient authority; `proc_exit` in library
  mode surfaces as `Klass::Rt::Exit` for the embedder to rescue.

## Rejected alternatives

- **Per-call context argument on every import callable** (wasm2c style) —
  taxes the common case (plain lambdas, the spectest harness) with a
  changed signature; Ruby closures plus `attach` reach the same power.
- **Strict imports only (status quo)** — makes WASI-importing modules
  unusable as libraries in practice.
- **Memory-name binding only** (Node's `start` looks up `exports.memory`)
  — `attach(instance)` is a superset and needs no export-name contract.
- **Unconditional eager construction of the bundled WASI** — pays the
  `binmode` side effect and an object even when the embedder provided
  everything.
- **Deferring construction to the first syscall invocation** — extra
  lambda indirection for little gain over construction-time laziness
  (user call).

## Consequences

- Positive: `Hello.new` works out of the box for WASI programs in
  library mode; custom runtimes swap in wholesale; spec harness needed no
  changes (Hash path untouched).
- Negative / caveat: with minimal Embedded bundles, `instance.memory`
  only carries the typed accessors the module itself uses; the stable
  surface a provider may rely on is `memory.bytes` (a mutable binary
  String). A fuller guaranteed API is a carry-over for the shared/gem
  linkage (ADR-6).
- The provider protocol is intentionally not WASI-specific: any import
  namespace (e.g. an emscripten-style `env`) can be served by a provider
  with instance access.
