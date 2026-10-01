# Decision 34: Bash WASI File System

Status: **Accepted, 2026-07-27.**
It landed on 2026-07-28.
The Bash backend has WASI Preview 1 file system support.
It mirrors the Ruby design ([decision 14](14-ruby-wasi-file-system.md)) within Bash's constraints.
It covers the following, all under `runtime/bash/units/wasi/`:

- the descriptor table state and `path_open`;
- the reworked `fd_read`/`fd_write`/`fd_seek`/`fd_tell`/`fd_close` units;
- the reworked `fd_fdstat_get`/`fd_prestat_get` units, and `fd_prestat_dir_name`;
- the standalone `--dir` parser;
- the `stat` family, the four namespace-mutation system calls, and `poll_oneoff`.

**Revision, 2026-07-28 ([decision 40](40-wasi-p1-completion.md)).**
The D2 external-command license extends to `ln -s`, `ln`, and `readlink` for the symbolic link family.
The ground is namespace mutation, plus the capability-completeness clause for `readlink`.
`fd_advise`/`fd_allocate`/`fd_renumber` and the rights model per descriptor are implemented in pure Bash.
Four declared gaps remain:

- timestamps (`touch` fails the D2 criterion);
- `d_ino` and `dev`/`ino` (D6, no `stat`);
- the D1 read-back across descriptors;
- following a *file* symbolic link (D3, ELOOP).

## Context

[Decision 12](12-bash-wasi.md) built the Bash WASI surface (stdio, arguments, environment, clock, random).
It left every `path_*` call, and each `fd_*` call only a file system needs, out of scope as ENOSYS.
That left a 15-function gap versus Ruby.
[Decision 14](14-ruby-wasi-file-system.md) answered those questions for Ruby.
It closed noting that a Bash file system backend "would need its own decision".
This is it.
Bash makes the design different in kind, not degree, from Ruby's `File`-backed descriptors.
That is because Bash has no random-access file object, no `openat`, no `realpath`/`readlink`/`stat` builtin.
Under [decision 5](5-bash-softfloat.md)'s dependency criterion it has no license to shell out for them either.
The decisions below are what that leaves possible.

## Decision

### D1: Files are whole-file byte buffers, flushed on `fd_close` and `fd_sync`

An open file descriptor holds the whole file as an indexed array of byte values (`<p>wbuf<fd>`).
A separate offset is kept in `<p>wtell[fd]`.
`path_open` reads the whole file in, and `fd_read`/`fd_write`/`fd_seek` work on the array.
A dirty buffer is flushed on `fd_close`/`fd_sync`/`fd_datasync`.
The flush is one `exec {fd}>"$path"` plus chunked `printf` of a `'\x%02x'` format.
Criterion: a Bash redirection can only empty or create a file (`>`), or add to its end (`>>`).
It cannot write at an offset.
So a file is represented as the one thing Bash *can* rewrite atomically, the whole of it.

Limits, recorded not fixed:

- two descriptors open on one file differ (each has its own buffer, last flush wins);
- open and close are O(file size);
- the flush is non-atomic;
- `fd_sync`/`fd_datasync` are just a flush, with no separate step that makes the data permanent.

### D2: Four namespace-mutation system calls may call one POSIX command each

**This is the main decision.**
Only four units may run a POSIX-mandated command, one each:

| Unit | Command |
| --- | --- |
| `path_create_directory` | `mkdir` |
| `path_remove_directory` | `rmdir` |
| `path_unlink_file` | `rm` |
| `path_rename` | `mv` |

Each is a single `--`-guarded `command` run on resolved absolute paths.
The WASI `errno` is derived from later `[[ -e ]]` / `[[ -d ]]` probes, not the command's own diagnostics.

The justification is impossibility, not ease.
Pure Bash cannot create, remove, or rename a directory entry *at all*.
Everything else the surface needs can be expressed in it.
That is read, write, `stat` by test builtins, and listing by globbing.
Runtime units are bundled per import ([decision 6](6-runtime-units.md)).
So a module that imports none of the four carries none of these commands.
It stays a pure Bash artifact.
So [decision 5](5-bash-softfloat.md)'s promise still holds for every program that does not need namespace mutation.
That is the property worth protecting.

### D3: Sandboxing by physical resolution plus containment per directory descriptor

Preopen roots resolve to a physical path once via `$(cd -P -- "$host" && pwd -P)`.
Every guest path resolution re-derives its parent the same way.
Containment is checked against *that directory descriptor's own* root.
The check is `[[ $real == "$root" || $real == "${root%/}/"* ]]`.
The `${root%/}/` form makes a root of `/` contain everything.
Criterion: nesting must not make an escape one level cheaper.
This is the model [decision 14](14-ruby-wasi-file-system.md) uses.

Two deviations from Ruby come from missing builtins.
With no `readlink`, a **file** symbolic link as the final path component cannot be followed.
It resolves to `ELOOP`, stricter than Ruby, which follows it.
A **directory** symbolic link is still followed because `cd -P` resolves it.
Decision 14's check-then-open TOCTOU limit carries over unchanged.
That is because this is a single-process research and example runtime.
It is not a sandbox host shared by many users.

### D4: `poll_oneoff` waits in pure Bash

An `fd_read` subscription on `stdin` waits with `read -t <deadline> -n 1`.
It holds an arriving byte in a one-byte slot (`<p>wpush`) for the next `fd_read` to consume.
A clock-only subscription sleeps on a `coproc` timer.
The timer is `coproc __slp { read _; }` then `read -rt <secs> -u "${__slp[0]}"`.
It replaces the process-substitution form `read -t <secs> <> <(:)`.
Some hosts (macOS) reject that form with `Permission denied`.
Every other subscription (regular files, `stdout`, `stderr`, `fd_write`) is ready at once, as on Ruby.
One deviation concerns a real `stdin` EOF.
This unit then reports each waiting `fd_read` ready with `nbytes` 0.
Ruby's `IO.select` equivalent reports the closed descriptor itself readable.
Both converge on the guest's next `fd_read` returning 0 bytes, so the observable behavior matches.

### D5: Descriptor table shape

Bash has no record type, so parallel arrays keyed by `fd` *are* the descriptor table.
The arrays are:

- one kind table `<p>wfds` (`stdio=1`, `file=2`, `dir=3`);
- `<p>wtell` (offset);
- `<p>wpath` (physical host path);
- `<p>wname` (preopen guest name, set when, and only when, `fd_prestat_get` shows it);
- `<p>wdirty`;
- the open-mode and rights flags (derived at open; checked per descriptor since [decision 40](40-wasi-p1-completion.md));
- the `<p>wbuf<fd>` byte array of each descriptor.

`<p>wnext` is the next descriptor, starting past the preopens and never reused, per decision 14.
Preopens arrive through an ordered `WASI_DIRS=('HOST::GUEST' ...)` array.
It is the Bash equivalent of Ruby's `preopens:` argument.
`init_preopens` turns it into directory descriptors from 3 upward.
The standalone main fills it from repeated `--dir` flags ([decision 31](31-standalone-runtime-interface.md)).

### D6: Accuracy of `stat`

The file type comes from the test builtins (`-d`/`-f`/`-h`/`-t`).
`size` comes from the live buffer length for an open descriptor.
`atim`/`mtim`/`ctim` and `dev`/`ino` report 0.
Bash cannot read a file's status for real numbers, so the zeros are a documented deviation.
They are the file system equivalent of decision 12's clock fallback.

## Rejected alternatives

- **Leave the four namespace-mutation system calls ENOSYS.**
  Honest to decision 5, but it leaves the surface permanently unable to create and remove files.
  SQLite's journal/WAL life cycle (decision 14's stated goal) needs those files and directories.
  The impossibility argument (D2) tips it.
  That is because this is the one capability pure Bash *cannot* provide.
  So it is the one place the criterion earns a narrow exception.
- **Loadable builtins (`enable -f mkdir.so`).**
  A platform-specific `.so` is a heavier and less portable dependency than a POSIX command.
  A POSIX command is already guaranteed wherever Bash runs.
- **A virtual file system held in Bash arrays over the host's.**
  State would differ from the host; the whole point of `--dir` is to touch real host files.
  The standalone `--dir` snapshots, captured under Wasmtime (decision 9), would not match.
- **General external-command use for the rest of the surface** (`od`/`dd` for bytes, `stat` for status).
  Rejected as decision 5 always rejected them.
  That is because those *can* be expressed in pure Bash (byte-wise `read`/`printf`, test builtins).
  So there is no impossibility to license the exception.

## Consequences

- Positive: the Bash backend reaches the same WASI p1 file system surface as Ruby.
  So `--dir` round-trips and the shared file system e2e suite apply to it.
  A module that imports no namespace-mutation system call stays pure Bash.
- Negative / accepted: whole-file buffering makes open and close O(size).
  Reading in and flushing cost microseconds per byte.
  That is in line with decision 5's "value is existence, not speed".
  The D2 dependency-statement change is real.
  A module importing the four mutation system calls now also depends on `mkdir`/`rmdir`/`rm`/`mv`.
  The D1/D3/D6 limits are standing deviations, not bugs.
  They are the difference between two descriptors on one file, non-atomic flush, and TOCTOU.
  They also include ELOOP on a file symbolic link and zeroed timestamps and `dev`/`ino`.
- After the decision 40 revision above, two WASI p1 functions are still ENOSYS on Bash.
  They are the timestamp setters `fd_filestat_set_times` and `path_filestat_set_times` (`docs/support.md`).
  The reason is that `touch` fails D2's criterion: setting a timestamp is not namespace mutation.
