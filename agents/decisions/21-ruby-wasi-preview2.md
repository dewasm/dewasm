# Decision 21: WASI Preview 2 Host for Ruby (CLI World)

Status: **Superseded (decision 24), 2026-07-26.**
[Decision 24](24-01-scope-reset.md) replaced it.
Kept as a design record for a future restoration of this support.
Git history plus this decision make the work cheap to bring back.
The original status note and implementation pointers below are retained as history.

Originally accepted 2026-07-24.
Implemented: `runtime/ruby/units/wasi_p2/` (`Rt::WASIP2`).
`generate_component` bundles it when `default_wasi` is on.
The function list is rendered into `docs/support.md` from the units.

## Context

Decision 20's adapters deliver host calls *post-lift*.
`Rt::WASIP2` receives Ruby Strings, Arrays, Hashes, and `[:case, payload]` variants.
It returns the same: no pointers, no layout.
The scope is the `wasi:cli` command world: `io/streams`, `cli/*`, `filesystem`, `clocks`, `random`.
That is enough to run real `wasm32-wasip2` binaries; `sockets`/`http` and 0.3 `async` are out.

## Decision

- **One resource table** (`@res`, integer handles) holds streams, descriptors, and pollables.
  The wrapper's `canon resource.drop` lambdas call `resource_drop(id, handle)`.
  It closes IOs the host opened and never the process stdio.
  **File system descriptors hold a host path, not an open IO**.
  Each `*-via-stream` call opens its own handle.
  So stream offsets never interfere and drop order cannot double-close.
- **Name resolution is the decision 7 provider protocol over versioned p2 names**.
  `import("wasi:cli/stdout@0.2.9#get-stdout")` strips the version.
  It then resolves to `p2_cli_stdout_get_stdout` by a fixed rewrite of the name.
  There is one runtime unit per function.
  So the support matrix derives from `has_unit` exactly like Preview 1.
  **An unimplemented `wasi:*` function binds to a lambda that traps at call time**, not a link error.
  That is because adapter modules import every function a binary *references*.
  Real binaries reference far more than they call (`terminal-*`, `metadata-hash`).
- **A blocking host is always "ready"**.
  `pollable.block` returns immediately, and `poll` reports every pollable ready.
  The blocking stream variants stand in for the non-blocking ones.
  The guest's poll loops end either way.
- Sandboxing reuses decision 14's `realpath` plus containment model (with its accepted TOCTOU limit).
  `exit` maps `result<(),()>` to `Rt::Exit` 0/1.

## Rejected alternatives

- **Link-time failure for unimplemented functions**: would refuse every real binary.
  The refusal would be over functions it never calls.
  The trap-at-call lambda keeps decision 0's "fail loud" at the first observable moment instead.
- **Sharing `Rt::WASI` (Preview 1) internals**: p1 units do pointer arithmetic against guest memory.
  p2 units never see memory.
  The file descriptor table *model* is shared as a pattern, not code.
- **Real non-blocking I/O + connected pollables**: needed only for guests that wait on many streams.
  CLI-world binaries poll in write/read loops that the always-ready answer satisfies.
  Revisit if a real consumer misbehaves.

## Consequences

- Positive: the D0 probe binary's full surface runs with behavior identical to Wasmtime's.
  That surface is `stdout`/`stderr`/`stdin` streams, environment, arguments, exit, and preopens.
  It also covers open-at/`stat`/via-stream file I/O, clocks, and random.
  A user host object can replace `Rt::WASIP2` whole by implementing `import(name)` + `resource_drop`.
- Negative / carry-over: rights/permissions are not modelled at all (beyond the sandbox containment).
  Timestamps in `stat` are `none`; `metadata-hash` is `ino`/`dev`, not a real hash.
  Bash has no p2 story (and per decision 20 would need the host vocabulary first).

See also: [decision 20](20-component-model-core-ir-adapters.md), [decision 14](14-ruby-wasi-file-system.md), [decision 7](7-import-providers.md).
