# ADR-46 — Host-OS-Scoped Expected-Failure Ledgers for the WASI Testsuite Harness

Status: **Accepted, 2026-07-29.** Implemented in `crates/dewasm-test-helper/src/wasi_testsuite.rs` (the `expected_failures_macos`/`expected_failures_linux` trait methods) together with the first scoped entries (Java, Ruby); the Java `path_link` entry was fixed outright instead of scoped.

## Context

The wasi-testsuite ledgers ([ADR-8](8-latest-testsuite-support-matrix.md) discipline, [ADR-36](36-wasi-testsuite-conformance.md) harness) are checked both ways: a ledgered trial that *passes* is a hard failure. Every entry was written against a macOS development host. The first Linux CI run (issue #7) broke that assumption in both directions:

- Entries whose cause is macOS host behaviour — CoreFoundation injecting `__CF_USER_TEXT_ENCODING` into the JVM/ruby environ — *pass* on Linux and trip the unexpectedly-passing check.
- A trial can fail on Linux only: the Linux JDK sets NOFOLLOW symlink times through microsecond `lutimes`, truncating the ns `mtim` the suite round-trips (`rust/symlink_filestat`), while macOS preserves ns.

So a single flat ledger cannot be simultaneously green on both hosts, yet the both-ways check is worth keeping — it is what caught the Go `path_link` bug (#5) hiding behind host `link(2)` differences.

## Decision

Ledger entries may be scoped to the host OS. `WasiTestsuiteBackend` gains two default-empty methods, `expected_failures_macos()` and `expected_failures_linux()`; the harness merges the host-matching list into the base ledger before the (unchanged) both-ways check.

The discriminating criterion: **an entry is host-scoped when its attributed cause is host-side behaviour outside the runtime's reach** (libc/interpreter environ injection, a host stdlib precision gap). If the cause is in the generated runtime, fix the unit instead — the Java `path_link` case was exactly that (the macOS `link(2)`-follows gap is emulated by recreating the symlink, mirroring the Go unit) and was removed from the ledger, not scoped. An entry that fails identically on both hosts stays in the base list.

## Rejected alternatives

- **Fix every host difference portably.** Not reachable: environ injection (CoreFoundation, PEP 538) and the JDK's µs `lutimes` path are outside what generated code can influence without FFI, which the backends deliberately avoid.
- **Relax the unexpectedly-passing check.** Would have silently absorbed the ledger drift — the check is precisely what surfaced #5 as a real bug.
- **`#[cfg]`-duplicated ledger arrays per backend.** Same effect, but each backend re-states the shared entries twice and the harness API stays ignorant of the semantics; the trait methods keep scoping explicit and additive.

## Consequences

- CI on ubuntu-latest and local macOS runs are both green against one ledger declaration, and each host still enforces the both-ways discipline for the entries that apply to it.
- A scoped entry is only ever *verified* on its own host: macOS entries are exercised locally, Linux entries only by CI (and vice versa). A stale scoped entry therefore surfaces one environment later, not never.
- New backends state host-dependent gaps where they belong instead of papering over them with the flat list; the spec harness ledger ([ADR-8](8-latest-testsuite-support-matrix.md)) stays flat until it meets the same problem.
