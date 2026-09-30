# Decision 14: Ruby WASI File System Support

Status: **Accepted, 2026-07-23.**
Implemented in `runtime/ruby/units/wasi/`.
The `preopens:` keyword argument of the provider is in `crates/dewasm-backend-ruby/src/lib.rs`.
The units implement these system calls:

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

Symbolic link and rights-narrowing system calls remain ENOSYS.

**Revision, 2026-07-27:** `poll_oneoff` is no longer an intended ENOSYS gap.
It is implemented for the Ruby, Python, Go, and Java backends.
Their units are `runtime/<lang>/units/wasi/poll_oneoff.*`.
Bash stays ENOSYS for now (deferred).
The Bash file system design, including `poll_oneoff`, is [decision 34](34-bash-wasi-file-system.md).
The motivation is event-loop guests such as the QuickJS REPL.
After printing each prompt, it blocks in `poll_oneoff` on an `fd_read` subscription over standard input.
An ENOSYS return there collapses the loop, and the program exits immediately.
Only `fd_read` on standard input actually blocks (via `IO.select`/`select.select`/`syscall.Select`).
Java approximates it by polling `InputStream.available()`, a documented limitation.
Regular files, standard output and error, and every `fd_write` are treated as immediately ready.
Clock subscriptions set the wait deadline.
Symbolic link and rights-narrowing system calls still remain ENOSYS.

**Revision, 2026-07-28 ([decision 40](40-wasi-p1-completion.md)):** two groups of system calls are now implemented on every backend.
They are the rights system calls and the symbolic link family.
That family is `path_symlink`/`path_readlink`/`path_link`.
This replaces this decision's two deferral paragraphs.
Containment is checked at follow time by the existing `resolve_path` check.
Creating a link no longer waits on a security gap it cannot close.
Rights are tracked *and checked* per file descriptor.
That answers the "narrowing without enforcement misleads" objection by checking them.
The TOCTOU check-then-open limit itself is unchanged.

## Context

`Rt::WASI` (decision 7) covered stdio, arguments/environment, clock, and random.
But every `path_*` call and file-system-only `fd_*` call resolved to the ENOSYS stub.
That blocked the project's stated goal: running Rails on a SQLite driver in pure Ruby.
The driver needs the real life cycle of a main database file plus its journal/WAL.
The life cycle is create, read/write at arbitrary offsets, save to disk, remove, rename.
Real file I/O raises two questions the stdio-only design never had to answer:

- what a directory descriptor *is*;
- how a guest-supplied path gets confined to a directory the embedder explicitly allowed.
  Such a directory is a WASI preopen.

Confinement is needed since ambient authority to the whole host file system is not acceptable.
That holds even in a demonstration runtime.

## Decision

- **Preopens are a keyword argument of the provider, not a new provider shape.**
  The new keyword argument is `Rt::WASI.new(preopens: { guest_path => host_path })`.
  It extends the existing `args:`/`env:` keyword arguments (decision 7).
  `wasi_bundled`'s generated `initialize` gained `preopens: {}` alongside them.
  So the fallback construction stays `@wasi ||= Rt::WASI.new(args:, env:, preopens:)`.
  The provider protocol itself does not change.
  Standalone mode reads the `DEWASM_PREOPEN` environment variable (`guest=host,...`).
  It fills the same keyword argument.
  It is kept separate from `ARGV` because `ARGV` already mirrors the guest's own `argv` one-to-one.
- **One file descriptor table, two kinds of entry.**
  `@fds` keeps mapping file descriptor → Ruby `IO` for files and stdio.
  This is unchanged: `File` already answers every method the stdio-only units called.
  A directory may be a true preopen.
  It may also be one the guest opened itself via `path_open`'s `oflags::DIRECTORY`.
  Either way, it is a `WasiDir = Struct.new(:host_path, :preopen_name, :entries)`.
  `preopen_name` is set only for entries that came from `preopens:`.
  `fd_prestat_get`/`fd_prestat_dir_name` must be able to tell exactly those entries apart.
  The other entries are directories the guest opened for its own directory walk.
  `entries` is the `fd_readdir` listing cache.
  File descriptors are never reused after `fd_close`.
  That is simpler than tracking reuse safety, and irrelevant at the scale this runtime targets.
  Criterion: reuse the existing IO-shaped path for files.
  Every stdio unit keeps working unmodified against `File`.
  Add exactly one new shape for the one thing IO cannot represent (a directory).
  The criterion rules out wrapping every file descriptor in a new envelope type.
- **Sandboxing is `File.realpath` plus prefix-containment.**
  **It is checked fresh on every path resolution against that directory file descriptor's own root.**
  That root has already been through `File.realpath`.
  The check is `resolve_path` in `runtime/ruby/units/wasi/_class.rb`.
  The containment check is derived again for each directory file descriptor.
  It is not made once against a single global root.
  So a directory file descriptor opened three levels deep still gets the same check as a preopen.
  Nesting cannot make an escape one level cheaper.
  This is a check-then-open, not an atomic `openat(2)`-beneath resolution.
  The window is between the `realpath` check and the actual `File.open`/`Dir.mkdir`/etc. call.
  A symbolic link planted inside the sandbox in that window could in principle escape (TOCTOU).
  Accepted for a single-process research/demonstration runtime with a trusted or semi-trusted guest.
  It is not accepted for a sandbox host shared by many users.
  The alternative is a component-by-component `openat`-style walk (`cap-std`'s approach).
  It rejects symbolic links one path segment at a time.
  It is real defense-in-depth but a much larger implementation.
  This project's correctness bar (decision 3) is the wasm specification testsuite.
  It is not a security-hardened sandbox.
- **Rights (`fs_rights_base`/`fs_rights_inheriting`) are read only to pick a `File.open` mode in `path_open`.**
  **They are never stored or checked again.**
  The mode is read/write/both.
  Access control in this design is entirely "which directories did the embedder preopen".
  It is not capability narrowing within that access.
  Consequently `fd_fdstat_set_flags`/`fd_fdstat_set_rights` stay ENOSYS.
  Nothing would check the narrowed rights afterward.
  So implementing them to *appear* to narrow rights would be misleading rather than merely unfinished.
- **Symbolic link system calls (`path_symlink`, `path_link`, `path_readlink`) stay ENOSYS.**
  The sandboxing limit above already accepts one escape vector passively.
  That vector is a host symbolic link that *already exists*.
  A symbolic link written by the guest is precisely that vector.
  Letting the guest *create* new ones on demand widens that surface actively.
  So it is deferred rather than half-solved.
  `fd_renumber`, `fd_advise`, `fd_allocate`, and `path_filestat_set_times` stay ENOSYS.
  They are lower-value gaps, not security-motivated ones.
- **`fd_readdir`'s `cookie` is a 1-based index into a listing snapshot.**
  **The snapshot is cached on `WasiDir#entries` at the first call for that file descriptor.**
  The `cookie` is not a live cursor that stays correct while the directory changes.
  This matches the specification's "cookie is an opaque resume point" contract.
  It needs no stable-under-mutation iteration order.
  POSIX `readdir` itself does not guarantee one either.

## Rejected alternatives

- **Per-component `openat`-beneath path resolution**: closes the TOCTOU gap properly.
  But it requires walking and re-validating every path segment against the live file system.
  That is well past what a single-process demonstration runtime needs.
  Revisit if `dewasmify` ever targets many untrusted guests on one host.
- **A capability object per file descriptor with its granted rights, checked on every subsequent call.**
  It is the honest version of rights support.
  But it has no point unless a real target program asks for it.
  Adding that tracking now would be a guess.
- **Reusing file descriptor numbers after `fd_close`**: saves nothing at this scale.
  It also brings back bugs that mix up file descriptors, for no benefit.
- **A separate `@dirs` hash instead of folding directories into `@fds`.**
  It was considered so `fd_read`/`fd_write` wouldn't need an `is_a?(IO)` guard.
  But every WASI system call taking an `fd` must look in exactly one table regardless.
  Examples are `close`, `fdstat`, and `prestat`.
  One table whose values carry their type is simpler than keeping two tables in step.

## Consequences

- Positive: SQLite's file-backed VFS life cycle can be implemented against this set of system calls.
  The life cycle is create, random-access read/write, save to disk, remove journal/WAL, rename.
  It works entirely in library mode via `preopens:`, without CLI changes.
- Negative / accepted: the sandboxing TOCTOU gap above is a standing limit.
  It is an escape through a symbolic link.
  It is not a bug to fix under this decision.
  Anyone embedding this runtime for genuinely untrusted guests needs to know that.
- Carry-over: rights enforcement, symbolic link support, and `fd_renumber` were not forgotten.
  They are explicit future work if a target program needs them.
- The provider surface (`preopens:`) is Ruby-specific for now.
  A Bash file system backend would need its own decision.
  Decision 12 explicitly scoped file system calls out.
