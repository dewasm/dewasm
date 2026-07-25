# ADR-21 — WASI Preview 2 Host for Ruby (CLI World)

Status: **Superseded by [ADR-24](24-01-scope-reset.md), 2026-07-26.** Kept as a design record for a future restoration of this support; git history plus this ADR make the work cheap to revive. The original acceptance note and implementation pointers below are retained as history.

Originally accepted 2026-07-24. Implemented: `runtime/ruby/units/wasi_p2/` (`Rt::WASIP2`),
bundled by `generate_component` when `default_wasi` is on; the function list is rendered into
`docs/support.md` from the units.

## Context

ADR-20's adapters deliver host calls *post-lift*: `Rt::WASIP2` receives Ruby Strings, Arrays,
Hashes, and `[:case, payload]` variants, and returns the same — no pointers, no layout. The
scope is the `wasi:cli` command world (io/streams, cli/*, filesystem, clocks, random), enough
to run real `wasm32-wasip2` binaries; sockets/http and 0.3 async are out.

## Decision

- **One resource table** (`@res`, integer handles) holds streams, descriptors, and pollables;
  `resource_drop(id, handle)` (called by the wrapper's `canon resource.drop` lambdas) closes
  IOs the host opened and never the process stdio. **Filesystem descriptors hold a host
  path, not an open IO** — each `*-via-stream` call opens its own handle, so stream offsets
  never interfere and drop order cannot double-close.
- **Name resolution is the ADR-7 provider protocol over versioned p2 names**:
  `import("wasi:cli/stdout@0.2.9#get-stdout")` strips the version and resolves to
  `p2_cli_stdout_get_stdout` by mechanical mangling — one runtime unit per function, so the
  support matrix derives from `has_unit` exactly like preview 1. **An unimplemented `wasi:*`
  function binds to a lambda that traps at call time**, not a link error: adapter modules
  import every function a binary *references*, and real binaries reference far more than they
  call (terminal-*, metadata-hash).
- **A synchronous host is always "ready"**: `pollable.block` returns immediately, `poll`
  reports every pollable ready, and the blocking stream variants stand in for the
  non-blocking ones — the guest's poll loops terminate either way.
- Sandboxing reuses ADR-14's realpath-plus-containment model (with its accepted TOCTOU
  caveat); `exit` maps `result<(),()>` to `Rt::Exit` 0/1.

## Rejected alternatives

- **Link-time failure for unimplemented functions** — would refuse every real binary over
  functions it never calls; the trap-at-call lambda keeps ADR-0's "fail loud" at the first
  observable moment instead.
- **Sharing `Rt::WASI` (preview 1) internals** — p1 units do pointer arithmetic against guest
  memory; p2 units never see memory. The fd-table *model* is shared as a pattern, not code.
- **Real non-blocking I/O + pollable wiring** — needed only for guests that genuinely
  multiplex; CLI-world binaries poll in write/read loops that the always-ready answer
  satisfies. Revisit if a real consumer misbehaves.

## Consequences

- Positive: the D0 probe binary's full surface (stdout/stderr/stdin streams, environment,
  arguments, exit, preopens, open-at/stat/via-stream file I/O, clocks, random) runs against
  wasmtime-identical behavior; a user host object can replace `Rt::WASIP2` wholesale by
  implementing `import(name)` + `resource_drop`.
- Negative / carry-over: rights/permissions are not modelled at all (beyond the sandbox
  containment); timestamps in `stat` are `none`; `metadata-hash` is ino/dev, not a real hash;
  Bash has no p2 story (and per ADR-20 would need the host vocabulary first).

See also: [ADR-20](20-component-model-core-ir-adapters.md), [ADR-14](14-ruby-wasi-filesystem.md),
[ADR-7](7-import-providers.md).
