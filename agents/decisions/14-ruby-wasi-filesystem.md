# Decision 14: Ruby WASI Filesystem Support

Status: **Accepted, 2026-07-23.**
Implemented in `runtime/ruby/units/wasi/`.
The `preopens:` provider kwarg is in `crates/dewasm-backend-ruby/src/lib.rs`.
The units implement these syscalls:

- `path_open`;
- `fd_pread`/`fd_pwrite`;
- `fd_filestat_get`;
- `path_filestat_get`;
- `fd_readdir`;
- `path_create_directory`;
- `path_remove_directory`;
- `path_unlink_file`;
- `path_rename`;
- `fd_sync`;
- `fd_datasync`;
- `fd_filestat_set_size`;
- `fd_prestat_dir_name`.

Symlink and rights-narrowing syscalls remain ENOSYS.

**Revision, 2026-07-27:** `poll_oneoff` is no longer a deliberate ENOSYS gap.
It is implemented for the Ruby, Python, Go, and Java backends.
Their units are `runtime/<lang>/units/wasi/poll_oneoff.*`.
Bash stays ENOSYS for now (deferred).
The Bash filesystem design, including `poll_oneoff`, is [decision 34](34-bash-wasi-filesystem.md).
The motivation is event-loop guests such as the QuickJS REPL.
After printing each prompt, it blocks in `poll_oneoff` on an fd_read subscription over stdin.
An ENOSYS return there collapses the loop, and the program exits immediately.
Only fd_read on stdin actually blocks (via `IO.select`/`select.select`/`syscall.Select`).
Java approximates it by polling `InputStream.available()`, a documented limitation.
Regular files, stdout/stderr, and every fd_write are treated as immediately ready.
Clock subscriptions set the wait deadline.
Symlink and rights-narrowing syscalls still remain ENOSYS.

**Revision, 2026-07-28 ([decision 40](40-wasi-p1-completion.md)):** two syscall groups are now implemented on every backend.
They are the symlink family (`path_symlink`/`path_readlink`/`path_link`) and the rights syscalls.
This supersedes this decision's two deferral paragraphs.
Containment is enforced at follow time by the existing `resolve_path` check.
Creating a link no longer waits on a posture it cannot close.
Rights are tracked *and enforced* per fd.
That answers the "narrowing without enforcement misleads" objection by enforcing.
The TOCTOU check-then-open caveat itself is unchanged.

## Context

`Rt::WASI` (decision 7) covered stdio, args/env, clock, and random.
But every `path_*` call and filesystem-only `fd_*` call resolved to the ENOSYS stub.
That blocked the project's stated goal: running Rails on a pure-Ruby SQLite driver.
The driver needs a real main-DB-plus-journal/WAL file lifecycle.
The lifecycle is create, read/write at arbitrary offsets, sync, delete, rename.
Adding real file I/O means answering two questions the stdio-only design never had to:

- what a directory descriptor *is*;
- how a guest-supplied path gets confined to a directory the embedder explicitly authorized.
  Such a directory is a WASI preopen.

Confinement is needed since ambient authority to the whole host filesystem is not acceptable.
That holds even in a demo runtime.

## Decision

- **Preopens are a provider kwarg, not a new provider shape.**
  The new kwarg is `Rt::WASI.new(preopens: { guest_path => host_path })`.
  It extends the existing `args:`/`env:` kwargs (decision 7).
  `wasi_bundled`'s generated `initialize` gained `preopens: {}` alongside them.
  So the fallback construction stays `@wasi ||= Rt::WASI.new(args:, env:, preopens:)`.
  The provider protocol itself does not change.
  Standalone mode reads a `DEWASM_PREOPEN` env var (`guest=host,...`) into the same kwarg.
  It is kept separate from `ARGV` because `ARGV` already mirrors the guest's own argv one-to-one.
- **One fd table, two kinds of entry.**
  `@fds` keeps mapping fd → Ruby `IO` for files and stdio.
  This is unchanged: `File` already answers every method the stdio-only units called.
  A directory may be a true preopen.
  It may also be one the guest opened itself via `path_open`'s `oflags::DIRECTORY`.
  Either way, it is a `WasiDir = Struct.new(:host_path, :preopen_name, :entries)`.
  `preopen_name` is set only for entries that came from `preopens:`.
  `fd_prestat_get`/`fd_prestat_dir_name` must be able to tell exactly those entries apart.
  The other entries are directories the guest opened for its own traversal.
  `entries` is the `fd_readdir` listing cache.
  Fds are never reused after `fd_close`.
  That is simpler than tracking reuse safety, and irrelevant at the scale this runtime targets.
  Criterion: reuse the existing IO-shaped path for files.
  Every stdio unit keeps working unmodified against `File`.
  Add exactly one new shape for the one thing IO cannot represent (a directory).
  The criterion rules out wrapping every fd in a new envelope type.
- **Sandboxing is `File.realpath` plus prefix-containment.**
  **It is checked fresh on every path resolution against that specific directory fd's own root.**
  That root is already realpath'd.
  The check is `resolve_path` in `runtime/ruby/units/wasi/_class.rb`.
  The containment check is re-derived locally per dirfd, not once against a single global root.
  So a directory fd opened three levels deep still gets the same check as a preopen.
  Nesting can't launder an escape one level cheaper.
  This is a check-then-open, not an atomic `openat(2)`-beneath resolution.
  The window is between the realpath check and the actual `File.open`/`Dir.mkdir`/etc. call.
  A symlink planted inside the sandbox in that window could in principle be used to escape (TOCTOU).
  Accepted for a single-process research/demo runtime embedding a trusted or semi-trusted guest.
  It is not accepted for a multi-tenant sandbox host.
  The alternative is a component-by-component `openat`-style walk (`cap-std`'s approach).
  It rejects symlinks one path segment at a time.
  It is real defense-in-depth but a much larger implementation.
  This project's correctness bar (decision 3) is the wasm spec testsuite.
  It is not a security-hardened sandbox.
- **Rights (`fs_rights_base`/`fs_rights_inheriting`) are read only to pick a `File.open` mode in `path_open`.**
  **They are never stored or checked again.**
  The mode is read/write/both.
  Access control in this design is entirely "which directories did the embedder preopen".
  It is not capability narrowing within that access.
  Consequently `fd_fdstat_set_flags`/`fd_fdstat_set_rights` stay ENOSYS.
  Nothing would enforce the narrowed rights afterward.
  So implementing them to *appear* to narrow rights would be misleading rather than merely incomplete.
- **Symlink syscalls (`path_symlink`, `path_link`, `path_readlink`) stay ENOSYS.**
  The sandboxing caveat above already accepts one escape vector passively: a *preexisting* host symlink.
  A symlink written by the guest is precisely that vector.
  Letting the guest *create* new ones on demand widens that surface actively.
  So it is deferred rather than half-solved.
  `fd_renumber`, `fd_advise`, `fd_allocate`, and `path_filestat_set_times` stay ENOSYS.
  They are lower-value gaps, not security-motivated ones.
- **`fd_readdir`'s cookie is a 1-based index into a listing snapshot.**
  **The snapshot is cached on `WasiDir#entries` at the first call for that fd.**
  The cookie is not a live cursor coherent under concurrent directory mutation.
  This matches the spec's "cookie is an opaque resume point" contract.
  It needs no stable-under-mutation iteration order.
  POSIX `readdir` itself does not guarantee one either.

## Rejected alternatives

- **Per-component `openat`-beneath path resolution**: closes the TOCTOU gap properly.
  But it requires walking and re-validating every path segment against the live filesystem.
  That is well past what a single-process demo runtime needs.
  Revisit if dewasmify ever targets untrusted-guest multi-tenancy.
- **A capability object per fd carrying its granted rights, checked on every subsequent call.**
  It is the honest version of rights support.
  But it is pointless without also being asked for by a real target program.
  Adding the bookkeeping now would be speculative.
- **Reusing fd numbers after `fd_close`**: saves nothing at this scale.
  It also reopens fd-confusion bug classes for no benefit.
- **A separate `@dirs` hash instead of folding directories into `@fds`.**
  It was considered so `fd_read`/`fd_write` wouldn't need an `is_a?(IO)` guard.
  But every WASI syscall taking an `fd` must look in exactly one table regardless.
  Examples are close, fdstat, and prestat.
  One table with a type-tagged value is simpler than keeping two tables in sync.

## Consequences

- Positive: SQLite's file-backed VFS lifecycle is implementable against this syscall set.
  The lifecycle is create, random-access read/write, sync, delete journal/WAL, rename.
  It works entirely in library mode via `preopens:`, without CLI changes.
- Negative / accepted: the sandboxing TOCTOU/symlink-escape gap above is a standing caveat.
  It is not a bug to fix under this decision.
  Anyone embedding this runtime for genuinely untrusted guests needs to know that.
- Carry-over: rights enforcement, symlink support, and `fd_renumber` are not oversights.
  They are explicit future work if a target program needs them.
- The provider surface (`preopens:`) is Ruby-specific for now.
  A Bash filesystem backend would need its own decision.
  Decision 12 explicitly scoped filesystem syscalls out.
