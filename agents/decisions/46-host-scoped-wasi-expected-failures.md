# Decision 46: Host-OS-Scoped Expected-Failure Lists for the WASI Testsuite Harness

Status: **Accepted, 2026-07-29.**
Implemented in `crates/dewasm-test-helper/src/wasi_testsuite.rs`.
The trait methods are `expected_failures_macos`/`expected_failures_linux`.
The first scoped entries (Java, Ruby) landed with them.
The Java `path_link` entry was fixed instead of scoped.

## Context

The `wasi-testsuite` expected-failures lists are checked both ways.
They follow the [decision 8](8-latest-testsuite-support-matrix.md) discipline in the [decision 36](36-wasi-testsuite-conformance.md) harness.
A listed trial that *passes* is a hard failure.
Every entry was written against a macOS development host.
The first Linux CI run (issue #7) broke that assumption in both directions:

- Some entries' cause is macOS host behaviour, and those entries *pass* on Linux.
  An example is CoreFoundation injecting `__CF_USER_TEXT_ENCODING` into the `environ` of JVM and `ruby`.
  They trip the unexpectedly-passing check.
- A trial can fail on Linux only.
  The Linux JDK sets NOFOLLOW symbolic link times through `lutimes`, which has microsecond precision.
  That drops the nanosecond part of the `mtim` the suite round-trips (`rust/symlink_filestat`).
  macOS preserves nanoseconds.

So a single flat list cannot pass on both hosts at once.
Yet the both-ways check is worth keeping.
It is what caught the Go `path_link` bug (#5) hiding behind host `link(2)` differences.

## Decision

List entries may be scoped to the host OS.
`WasiTestsuiteBackend` gains two default-empty methods.
They are `expected_failures_macos()` and `expected_failures_linux()`.
The harness merges the host-matching list into the base list before the (unchanged) both-ways check.

The deciding criterion concerns the entry's attributed cause.
**An entry is host-scoped when its cause is host-side behaviour outside the runtime's reach.**
One example is `environ` injection by `libc` or an interpreter.
Another is a precision gap in a host standard library.
If the cause is in the generated runtime, fix the unit instead.
The Java `path_link` case was exactly that, and it was removed from the list, not scoped.
On macOS, `link(2)` follows a symbolic link.
That gap is handled there by recreating the symbolic link, mirroring the Go unit.
An entry that fails identically on both hosts stays in the base list.

## Rejected alternatives

- **Fix every host difference portably.**
  Not reachable: some causes are outside what generated code can influence without FFI.
  They are `environ` injection (CoreFoundation, PEP 538) and the JDK's µs `lutimes` path.
  The backends deliberately avoid FFI.
- **Relax the unexpectedly-passing check.**
  It would have silently absorbed the list drift.
  The check is precisely what surfaced #5 as a real bug.
- **List arrays copied under `#[cfg]` per backend.**
  Same effect, but each backend re-states the shared entries twice.
  The harness API would also not know the semantics.
  The trait methods keep scoping explicit, and a scoped list only adds to the base list.

## Consequences

- CI on `ubuntu-latest` and local macOS runs both pass against one list declaration.
  Each host still checks the both-ways discipline for the entries that apply to it.
- A scoped entry is only ever *verified* on its own host.
  macOS entries are exercised locally, Linux entries only by CI.
  On a Linux development host it is the reverse.
  An out-of-date scoped entry therefore surfaces one environment later, not never.
- New backends state host-dependent gaps where they belong.
  They do not paper over them with the flat list.
  The specification harness list ([decision 8](8-latest-testsuite-support-matrix.md)) stays flat until it meets the same problem.
