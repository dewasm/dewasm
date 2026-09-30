# Decision 34: Bash WASI Filesystem

Status: **Accepted, 2026-07-27; fully landed 2026-07-28.**
The Bash backend has WASI preview-1 filesystem support.
It mirrors the Ruby design ([decision 14](14-ruby-wasi-filesystem.md)) within Bash's constraints.
It covers the following, all under `runtime/bash/units/wasi/`:

- the fd-table state and `path_open`;
- the reworked `fd_read`/`fd_write`/`fd_seek`/`fd_tell`/`fd_close` units;
- the reworked `fd_fdstat_get`/`fd_prestat_get` units, and `fd_prestat_dir_name`;
- the standalone `--dir` parser;
- the stat family, the four namespace-mutation syscalls, and `poll_oneoff`.

**Revision, 2026-07-28 ([decision 40](40-wasi-p1-completion.md)).**
The D2 external-command license extends to `ln -s`, `ln`, and `readlink` for the symlink family.
The ground is namespace mutation, plus the capability-completeness clause for `readlink`.
`fd_advise`/`fd_allocate`/`fd_renumber` and the per-fd rights model are implemented in pure Bash.
Four declared gaps remain:

- timestamps (`touch` fails the D2 criterion);
- d_ino/dev-ino (D6, no `stat`);
- the D1 cross-fd read-back;
- following a *file* symlink (D3, ELOOP).

## Context

[Decision 12](12-bash-wasi.md) built the Bash WASI surface (stdio, args/env, clock, random).
It scoped every `path_*` and filesystem-only `fd_*` call out to ENOSYS, a 15-function gap versus Ruby.
[Decision 14](14-ruby-wasi-filesystem.md) answered those questions for Ruby.
It closed noting that a Bash filesystem backend "would need its own decision".
This is it.
Bash makes the design different in kind, not degree, from Ruby's `File`-backed fds.
Bash has no random-access file object, no `openat`, and no `realpath`/`readlink`/`stat` builtin.
Under [decision 5](5-bash-softfloat.md)'s dependency criterion it has no license to shell out for them either.
The decisions below are what that leaves possible.

## Decision

### D1: Files are whole-file byte buffers, flushed on close/sync

An open file fd holds the whole file as an indexed array of byte ordinals (`<p>wbuf<fd>`).
A separate offset is kept in `<p>wtell[fd]`.
`path_open` slurps the file in, and `fd_read`/`fd_write`/`fd_seek` work on the array.
A dirty buffer is flushed on `fd_close`/`fd_sync`/`fd_datasync`.
The flush is one `exec {fd}>"$path"` plus chunked `printf` of a `'\x%02x'` format.
Criterion: a Bash redirection can only truncate-create (`>`) or append (`>>`).
It cannot write at an offset.
So a file is represented as the one thing Bash *can* rewrite atomically, the whole of it.

Caveats, recorded not fixed:

- two fds open on one file diverge (each has its own buffer, last flush wins);
- open and close are O(file size);
- the flush is non-atomic;
- `fd_sync`/`fd_datasync` are just a flush, with no separate durability barrier.

### D2: Four namespace-mutation syscalls may call one POSIX command each

**This is the headline decision.**
Only four units may invoke a POSIX-mandated command, one each:

| Unit | Command |
| --- | --- |
| `path_create_directory` | `mkdir` |
| `path_remove_directory` | `rmdir` |
| `path_unlink_file` | `rm` |
| `path_rename` | `mv` |

Each is a single `--`-guarded `command` invocation on resolved absolute paths.
The errno is derived from post-hoc `[[ -e ]]` / `[[ -d ]]` probes, not the command's own diagnostics.

The justification is impossibility, not convenience.
Pure Bash cannot create, remove, or rename a directory entry *at all*.
Everything else the surface needs is expressible in it.
That is read, write, stat by test builtins, and listing by globbing.
Runtime units are bundled per import ([decision 6](6-runtime-units.md)).
So a module that imports none of the four carries none of these commands.
It stays a pure-Bash artifact.
[Decision 5](5-bash-softfloat.md)'s promise then still holds for every program that does not need namespace mutation.
That is the property worth protecting.

### D3: Sandboxing by physical resolution plus per-dirfd containment

Preopen roots resolve to a physical path once via `$(cd -P -- "$host" && pwd -P)`.
Every guest path resolution re-derives its parent the same way.
Containment is checked against *that dirfd's own* root.
The check is `[[ $real == "$root" || $real == "${root%/}/"* ]]`.
The `${root%/}/` form makes a root of `/` contain everything.
Criterion: nesting must not launder an escape one level cheaper.
This is the model [decision 14](14-ruby-wasi-filesystem.md) uses.

Two deviations from Ruby come from missing builtins.
With no `readlink`, a **file** symlink as the final path component cannot be followed.
It resolves to `ELOOP`, stricter than Ruby, which follows it.
A **directory** symlink is still followed because `cd -P` resolves it.
Decision 14's check-then-open TOCTOU caveat carries over unchanged.
This is a single-process research/demo runtime, not a multi-tenant sandbox host.

### D4: `poll_oneoff` waits in pure Bash

An `fd_read`-on-stdin subscription waits with `read -t <deadline> -n 1`.
It holds an arriving byte in a one-byte pushback slot (`<p>wpush`) for the next `fd_read` to consume.
A clock-only subscription sleeps on a `coproc` timer.
The timer is `coproc __slp { read _; }` then `read -rt <secs> -u "${__slp[0]}"`.
It replaces the process-substitution idiom `read -t <secs> <> <(:)`.
Some hosts (macOS) reject that idiom with `Permission denied`.
Every other subscription (regular files, stdout/stderr, `fd_write`) is immediately ready, as on Ruby.
One deviation concerns a real stdin EOF.
This unit then reports each waiting `fd_read` ready with `nbytes` 0.
Ruby's `IO.select` equivalent reports the closed fd itself readable.
Both converge on the guest's next `fd_read` returning 0 bytes, so the observable behavior matches.

### D5: fd-table shape

Bash has no record type, so parallel arrays keyed by fd *are* the fd table.
The arrays are:

- one kind table `<p>wfds` (stdio=1, file=2, dir=3);
- `<p>wtell` (offset);
- `<p>wpath` (physical host path);
- `<p>wname` (preopen guest name, set iff prestat-visible);
- `<p>wdirty`;
- the open-mode and rights flags (derived at open; enforced per fd since [decision 40](40-wasi-p1-completion.md));
- the per-fd `<p>wbuf<fd>` byte array.

`<p>wnext` is the next fd, starting past the preopens and never reused, per decision 14.
Preopens arrive through an ordered `WASI_DIRS=('HOST::GUEST' ...)` array.
It is the Bash analogue of Ruby's `preopens:` kwarg.
`init_preopens` turns it into dir fds from 3 upward.
The standalone main fills it from repeated `--dir` flags ([decision 31](31-standalone-runtime-interface.md)).

### D6: stat fidelity

Filetype comes from the test builtins (`-d`/`-f`/`-h`/`-t`).
`size` comes from the live buffer length for an open fd.
`atim`/`mtim`/`ctim` and `dev`/`ino` report 0.
Bash cannot stat a file for real numbers, so the zeros are a documented deviation.
They are the filesystem analogue of decision 12's clock fallback.

## Rejected alternatives

- **Leave the four namespace-mutation syscalls ENOSYS.**
  Honest to decision 5, but it leaves the surface permanently unable to create and delete files.
  SQLite's journal/WAL lifecycle (decision 14's stated goal) needs those files and directories.
  The impossibility argument (D2) tips it.
  This is the one capability pure Bash *cannot* provide.
  So it is the one place the criterion earns a narrow exception.
- **Loadable builtins (`enable -f mkdir.so`).**
  A platform-specific `.so` is a heavier and less portable dependency than a POSIX command.
  A POSIX command is already guaranteed wherever Bash runs.
- **A virtual filesystem overlay in Bash arrays.**
  State would diverge from the host; the whole point of `--dir` is to touch real host files.
  The standalone `--dir` snapshots, captured under wasmtime (decision 9), would not match.
- **General external-command use for the rest of the surface** (`od`/`dd` for bytes, `stat` for metadata).
  Rejected as decision 5 always rejected them.
  Those capabilities *are* expressible in pure Bash (byte-wise `read`/`printf`, test builtins).
  So there is no impossibility to license the exception.

## Consequences

- Positive: the Bash backend reaches the same WASI p1 filesystem surface as Ruby.
  So `--dir` round-trips and the shared filesystem e2e suite apply to it.
  A module that imports no namespace-mutation syscall stays pure Bash.
- Negative / accepted: whole-file buffering makes open/close O(size).
  The slurp/flush costs ~microseconds per byte.
  That is in line with decision 5's "value is existence, not speed".
  The D2 dependency-statement change is real.
  A module importing the four mutation syscalls now also depends on `mkdir`/`rmdir`/`rm`/`mv`.
  The D1/D3/D6 caveats are standing deviations, not bugs.
  They are two-fd divergence, non-atomic flush, and TOCTOU.
  They also include file-symlink ELOOP and zeroed timestamps/dev/ino.
- After the decision 40 revision above, two WASI p1 functions are still ENOSYS on Bash.
  They are the timestamp setters `fd_filestat_set_times` and `path_filestat_set_times` (`docs/support.md`).
  The reason is that `touch` fails D2's criterion: setting a timestamp is not namespace mutation.
