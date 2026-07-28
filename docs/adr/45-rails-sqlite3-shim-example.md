# ADR-45 — Rails Demo via a sqlite3-Gem Shim over Converted libsqlite3

Status: **Accepted, 2026-07-28.** `examples/rails` runs an unmodified Rails 8
app on `libsqlite3.wasm` converted to Ruby; the sqlite3-gem-compatible shim,
the extended `SQLITE_EXPORTS` list in `examples/apps/fetch.sh`, and the
end-to-end `run.sh` all landed.

## Context

The Ruby backend's north-star demo (`docs/backends/ruby.md`) is real software
using converted SQLite. Rails is the strongest form of that claim, but
something must bridge ActiveRecord's SQLite3Adapter to the converted module's
`invoke`/`Rt::Memory` interface — and the converted library cannot call back
into arbitrary host Ruby (a guest function pointer cannot be conjured for a
host lambda; only declared imports can, per ADR-7's provider table).

## Decision

Bridge at the **sqlite3 gem API layer**: a path gem named `sqlite3`
(`examples/rails/sqlite3`) implements the surface Rails 8.1 actually calls —
verified against the adapter and gem sources, not guessed — so Rails and
ActiveRecord run unmodified. The criterion, reusable for future "run X on a
converted library" work: **shim at the narrowest public API whose consumer
you refuse to fork, and keep the guest callback-free** — every gem feature
that would need a guest→host callback is re-expressed on guest-side
primitives (`busy_handler_timeout=` → `sqlite3_busy_timeout`;
`execute_batch2` → a prepare/`remainder` loop instead of `sqlite3_exec`).

Each `SQLite3::Database` instantiates its own wasm module (isolated heap;
a mutex serializes entry), so a connection-pool entry is an isolated SQLite
and thread-safety never depends on guest-global state. The C surface this
requires is exported by extending `SQLITE_EXPORTS` in
`examples/apps/fetch.sh` (stamp now covers the export lists, so edits
retrigger the build).

## Rejected alternatives

- **Patch/replace the ActiveRecord adapter.** Chases Rails internals across
  releases and weakens the demo ("Rails, if you modify it"). The gem API is
  the narrower, slower-moving seam.
- **Host-callback binding shape (`sqlite3-binding.wasm`, ADR-22).** Needs
  bespoke C per feature and still cannot register runtime-chosen callbacks
  (`busy_handler`, `create_function`); fine as a linking proof, wrong as a
  compatibility layer.
- **One shared wasm instance for all connections.** Smaller footprint, but
  Ruby threads interleave at arbitrary points, so guest-global sqlite state
  would need one big lock — serializing the whole pool and losing isolation.

## Consequences

- Positive: `examples/rails/run.sh` proves wasm 1.0 + WASI p1 (ADR-40)
  carries a real framework end-to-end; the shim doubles as a reference
  embedding of a converted reactor library (ADR-14 preopens, ADR-2 i64
  masking at the boundary).
- Negative: the shim tracks the sqlite3 gem's Rails-facing surface and can
  drift with new Rails releases; `strict:`, extensions, custom functions and
  collations are unsupported (callback-free rule); WAL silently degrades to
  rollback journal (no WASI shared memory) and cross-process locking is
  absent — single-process embedding only.
- Carry-over: performance work on converted SQLite (ADR-32/33/41–44)
  directly improves this demo; a future backend can reuse the same shim
  design against its own runtime.
