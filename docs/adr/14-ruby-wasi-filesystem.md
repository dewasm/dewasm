# ADR-14 — Ruby WASI Filesystem Support

Status: **Accepted, 2026-07-23.** Implemented in `runtime/ruby/units/wasi/`
(`path_open`, `fd_pread`/`fd_pwrite`, `fd_filestat_get`,
`path_filestat_get`, `fd_readdir`, `path_create_directory`,
`path_remove_directory`, `path_unlink_file`, `path_rename`, `fd_sync`,
`fd_datasync`, `fd_filestat_set_size`, `fd_prestat_dir_name`) and the
`preopens:` provider kwarg in `crates/dewasm-backend-ruby/src/lib.rs`.
Symlink and rights-narrowing syscalls remain ENOSYS.

**Revision, 2026-07-27:** `poll_oneoff` is no longer a deliberate ENOSYS gap.
It is implemented for the Ruby, Python, Go, and Java backends
(`runtime/<lang>/units/wasi/poll_oneoff.*`); Bash stays ENOSYS for now
(deferred; the Bash filesystem design, including `poll_oneoff`, is
[ADR-34](34-bash-wasi-filesystem.md)). The motivation is event-loop guests such as the QuickJS REPL,
which after printing each prompt blocks in `poll_oneoff` on an fd_read
subscription over stdin — an ENOSYS return there collapses the loop and the
program exits immediately. Only fd_read on stdin actually blocks (via
`IO.select`/`select.select`/`syscall.Select`; Java approximates it by polling
`InputStream.available()`, a documented limitation); regular files,
stdout/stderr, and every fd_write are treated as immediately ready, and clock
subscriptions set the wait deadline. Symlink and rights-narrowing syscalls
still remain ENOSYS.

**Revision, 2026-07-28 ([ADR-40](40-wasi-p1-completion.md)):** the symlink
family (`path_symlink`/`path_readlink`/`path_link`) and the rights syscalls
are now implemented on every backend, superseding this ADR's two deferral
paragraphs: containment is enforced at follow time by the existing
`resolve_path` check (creating a link no longer waits on a posture it cannot
close), and rights are tracked *and enforced* per fd, answering the
"narrowing without enforcement misleads" objection by enforcing. The TOCTOU
check-then-open caveat itself is unchanged.

## Context

`Rt::WASI` (ADR-7) covered stdio, args/env, clock, and random, but every
`path_*` call and filesystem-only `fd_*` call resolved to the ENOSYS stub
— blocking the project's north star (running Rails on a pure-Ruby SQLite
driver, which needs a real main-DB-plus-journal/WAL file lifecycle:
create, read/write at arbitrary offsets, sync, delete, rename). Adding
real file I/O means answering two questions the stdio-only design never
had to: what a directory descriptor *is*, and how a guest-supplied path
gets confined to a directory the embedder explicitly authorized (a WASI
preopen), since ambient authority to the whole host filesystem is not
acceptable even in a demo runtime.

## Decision

- **Preopens are a provider kwarg, not a new provider shape.**
  `Rt::WASI.new(preopens: { guest_path => host_path })` extends the
  existing `args:`/`env:` kwargs (ADR-7); `wasi_bundled`'s generated
  `initialize` gained `preopens: {}` alongside them, so the fallback
  construction stays `@wasi ||= Rt::WASI.new(args:, env:, preopens:)`
  with no change to the provider protocol itself. Standalone mode reads
  a `DEWASM_PREOPEN` env var (`guest=host,...`) into the same kwarg,
  kept separate from `ARGV` because `ARGV` already mirrors the guest's
  own argv one-to-one.
- **One fd table, two kinds of entry.** `@fds` keeps mapping fd → Ruby
  `IO` for files and stdio (unchanged — `File` already answers every
  method the stdio-only units called). Directories, whether a true
  preopen or one the guest opened itself via `path_open`'s
  `oflags::DIRECTORY`, are a `WasiDir = Struct.new(:host_path,
  :preopen_name, :entries)`. `preopen_name` is set only for entries that
  came from `preopens:`, which is exactly what `fd_prestat_get`/
  `fd_prestat_dir_name` must be able to tell apart from a directory the
  guest opened for its own traversal. `entries` is the `fd_readdir`
  listing cache. Fds are never reused after `fd_close` — simpler than
  tracking reuse safety, and irrelevant at the scale this runtime targets.
  Criterion: reuse the existing IO-shaped path for files (every stdio
  unit keeps working unmodified against `File`) and add exactly one new
  shape for the one thing IO cannot represent (a directory), rather than
  wrapping every fd in a new envelope type.
- **Sandboxing is `File.realpath` plus prefix-containment, checked fresh
  on every path resolution against that specific directory fd's own
  (already-realpath'd) root** — `resolve_path` in
  `runtime/ruby/units/wasi/_class.rb`. Re-deriving the containment check
  locally per dirfd, rather than once against a single global root, means
  a directory fd opened three levels deep still gets the same check as a
  preopen: nesting can't launder an escape one level cheaper. This is a
  check-then-open, not an atomic `openat(2)`-beneath resolution: a
  symlink planted inside the sandbox between the realpath check and the
  actual `File.open`/`Dir.mkdir`/etc. call could in principle be used to
  escape (TOCTOU). Accepted for a single-process research/demo runtime
  embedding a trusted or semi-trusted guest, not a multi-tenant sandbox
  host — the alternative (a component-by-component `openat`-style walk
  rejecting symlinks one path segment at a time, `cap-std`'s approach) is
  real defense-in-depth but a much larger implementation for a project
  whose correctness bar (ADR-3) is the wasm spec testsuite, not a
  security-hardened sandbox.
- **Rights (`fs_rights_base`/`fs_rights_inheriting`) are read only to pick
  a `File.open` mode (read/write/both) in `path_open`, never stored or
  checked again.** Access control in this design is entirely "which
  directories did the embedder preopen", not capability narrowing within
  that access. Consequently `fd_fdstat_set_flags`/`fd_fdstat_set_rights`
  stay ENOSYS: implementing them to *appear* to narrow rights while
  nothing enforces the narrowed rights afterward would be misleading
  rather than merely incomplete.
- **Symlink syscalls (`path_symlink`, `path_link`, `path_readlink`) stay
  ENOSYS.** A symlink written by the guest is precisely the escape vector
  the sandboxing caveat above already accepts passively (a *preexisting*
  host symlink); letting the guest *create* new ones on demand widens
  that surface actively, so it is deferred rather than half-solved.
  `fd_renumber`, `fd_advise`, `fd_allocate`, and
  `path_filestat_set_times` stay ENOSYS as lower-value gaps, not
  security-motivated ones.
- **`fd_readdir`'s cookie is a 1-based index into a listing snapshot
  cached on `WasiDir#entries` at the first call for that fd**, not a
  live cursor coherent under concurrent directory mutation. Matches the
  spec's "cookie is an opaque resume point" contract without needing a
  stable-under-mutation iteration order, which POSIX `readdir` itself
  does not guarantee either.

## Rejected alternatives

- **Per-component `openat`-beneath path resolution** — closes the TOCTOU
  gap properly but requires walking and re-validating every path segment
  against the live filesystem, well past what a single-process demo
  runtime needs; revisit if dewasmify ever targets untrusted-guest
  multi-tenancy.
- **A capability object per fd carrying its granted rights, checked on
  every subsequent call** — the honest version of rights support, but
  pointless without also being asked for by a real target program; adding
  the bookkeeping now would be speculative.
- **Reusing fd numbers after `fd_close`** — saves nothing at this scale
  and reopens fd-confusion bug classes for no benefit.
- **A separate `@dirs` hash instead of folding directories into `@fds`**
  — considered so `fd_read`/`fd_write` wouldn't need an `is_a?(IO)`
  guard, but every WASI syscall that takes an `fd` (close, fdstat,
  prestat) has to look in exactly one table regardless; one table with a
  type-tagged value is simpler than keeping two tables in sync.

## Consequences

- Positive: SQLite's file-backed VFS lifecycle (create, random-access
  read/write, sync, delete journal/WAL, rename) is implementable against
  this syscall set entirely in library mode via `preopens:`, without
  CLI changes.
- Negative / accepted: the sandboxing TOCTOU/symlink-escape gap above is
  a standing caveat, not a bug to fix under this ADR — anyone embedding
  this runtime for genuinely untrusted guests needs to know that.
- Carry-over: rights enforcement, symlink support, and `fd_renumber` are
  explicit future work if a target program needs them, not oversights.
- The provider surface (`preopens:`) is Ruby-specific for now; a Bash
  filesystem backend would need its own ADR (ADR-12 explicitly scoped
  filesystem syscalls out).
