# Decision 34 — Bash WASI Filesystem

Status: **Accepted, 2026-07-27; fully landed 2026-07-28.**
The Bash backend has WASI preview-1 filesystem support, mirroring the Ruby design ([decision 14](14-ruby-wasi-filesystem.md)) within Bash's constraints: the fd-table state, `path_open`, the reworked `fd_read`/`fd_write`/`fd_seek`/`fd_tell`/`fd_close`/`fd_fdstat_get`/`fd_prestat_get` units, `fd_prestat_dir_name`, the standalone `--dir` parser, the stat family, the four namespace-mutation syscalls, and `poll_oneoff`, all under `runtime/bash/units/wasi/`.

**Revision, 2026-07-28 ([decision 40](40-wasi-p1-completion.md)):** the D2 external-command license extends to `ln -s`, `ln`, and `readlink` for the symlink family (namespace mutation, plus the capability-completeness clause for `readlink`); `fd_advise`/`fd_allocate`/`fd_renumber` and the per-fd rights model are implemented in pure Bash.
Timestamps (`touch` fails the D2 criterion), d_ino/dev-ino (D6, no `stat`), the D1 cross-fd read-back, and following a *file* symlink (D3, ELOOP) remain the declared gaps.

## Context

[Decision 12](12-bash-wasi.md) built the Bash WASI surface (stdio, args/env, clock, random) but scoped every `path_*` and filesystem-only `fd_*` call out to ENOSYS, a 15-function gap versus Ruby.
[Decision 14](14-ruby-wasi-filesystem.md) answered those questions for Ruby and closed noting that a Bash filesystem backend "would need its own decision".
This is it.
Bash makes the design different in kind, not degree, from Ruby's `File`-backed fds: there is no random-access file object, no `openat`, no `realpath`/`readlink`/`stat` builtin, and under [decision 5](5-bash-softfloat.md)'s dependency criterion no license to shell out for them either.
The decisions below are what that leaves possible.

## Decision

### D1 — Files are whole-file byte buffers, flushed on close/sync

An open file fd holds the whole file as an indexed array of byte ordinals (`<p>wbuf<fd>`) with a separate offset (`<p>wtell[fd]`); `path_open` slurps it in, `fd_read`/`fd_write`/`fd_seek` work on the array, and a dirty buffer is flushed on `fd_close`/`fd_sync`/`fd_datasync` by one `exec {fd}>"$path"` plus chunked `printf` of a `'\x%02x'` format.
Criterion: a Bash redirection can only truncate-create (`>`) or append (`>>`) and cannot write at an offset, so a file is represented as the one thing Bash *can* rewrite atomically, the whole of it.

Caveats, recorded not fixed: two fds open on one file diverge (each has its own buffer, last flush wins), open and close are O(file size), the flush is non-atomic, and `fd_sync`/`fd_datasync` are just a flush, with no separate durability barrier.

### D2 — Four namespace-mutation syscalls may call one POSIX command each

**This is the headline decision.**
The units `path_create_directory`, `path_remove_directory`, `path_unlink_file`, and `path_rename`, and **only** they, may invoke the POSIX-mandated `mkdir` / `rmdir` / `rm` / `mv` respectively, as a single `--`-guarded `command` invocation on resolved absolute paths, with the errno derived from post-hoc `[[ -e ]]` / `[[ -d ]]` probes rather than the command's own diagnostics.

The justification is impossibility, not convenience: pure Bash cannot create, remove, or rename a directory entry *at all*, whereas everything else the surface needs (read, write, stat by test builtins, listing by globbing) is expressible in it.
Runtime units are bundled per import ([decision 6](6-runtime-units.md)), so a module that imports none of the four carries none of these commands and stays a pure-Bash artifact: [decision 5](5-bash-softfloat.md)'s promise still holds for every program that does not need namespace mutation, which is the property worth protecting.

### D3 — Sandboxing: physical resolution plus per-dirfd containment

Preopen roots resolve to a physical path once via `$(cd -P -- "$host" && pwd -P)`, every guest path resolution re-derives its parent the same way, and containment is checked against *that dirfd's own* root with `[[ $real == "$root" || $real == "${root%/}/"* ]]` (the `${root%/}/` form makes a root of `/` contain everything).
Criterion: nesting must not launder an escape one level cheaper, exactly the model [decision 14](14-ruby-wasi-filesystem.md) uses.

Two deviations from Ruby, both from missing builtins: with no `readlink`, a **file** symlink as the final path component cannot be followed and resolves to `ELOOP` (stricter than Ruby, which follows it), while a **directory** symlink is still followed because `cd -P` resolves it.
Decision 14's check-then-open TOCTOU caveat carries over unchanged: this is a single-process research/demo runtime, not a multi-tenant sandbox host.

### D4 — `poll_oneoff` waits in pure Bash

An `fd_read`-on-stdin subscription waits with `read -t <deadline> -n 1` and holds an arriving byte in a one-byte pushback slot (`<p>wpush`) for the next `fd_read` to consume; a clock-only subscription sleeps on a `coproc` timer (`coproc __slp { read _; }` then `read -rt <secs> -u "${__slp[0]}"`) rather than the process-substitution idiom `read -t <secs> <> <(:)`, which some hosts (macOS) reject with `Permission denied`.
Every other subscription (regular files, stdout/stderr, `fd_write`) is immediately ready, as on Ruby.
One deviation: on a real stdin EOF this unit reports each waiting `fd_read` ready with `nbytes` 0 where Ruby's `IO.select` equivalent reports the closed fd itself readable, but both converge on the guest's next `fd_read` returning 0 bytes, so the observable behavior matches.

### D5 — fd-table shape

Bash has no record type, so parallel arrays keyed by fd *are* the fd table: one kind table `<p>wfds` (stdio=1, file=2, dir=3) beside `<p>wtell` (offset), `<p>wpath` (physical host path), `<p>wname` (preopen guest name, set iff prestat-visible), `<p>wdirty`, the open-mode and rights flags (derived at open; enforced per fd since [decision 40](40-wasi-p1-completion.md)), and the per-fd `<p>wbuf<fd>` byte array.
`<p>wnext` is the next fd, starting past the preopens and never reused, per decision 14.
Preopens arrive through an ordered `WASI_DIRS=('HOST::GUEST' ...)` array, the Bash analogue of Ruby's `preopens:` kwarg, which `init_preopens` turns into dir fds from 3 upward and which the standalone main fills from repeated `--dir` flags ([decision 31](31-standalone-runtime-interface.md)).

### D6 — stat fidelity

Filetype comes from the test builtins (`-d`/`-f`/`-h`/`-t`) and `size` from the live buffer length for an open fd, while `atim`/`mtim`/`ctim` and `dev`/`ino` report 0.
Bash cannot stat a file for real numbers, so the zeros are a documented deviation, the filesystem analogue of decision 12's clock fallback.

## Rejected alternatives

- **Leave the four namespace-mutation syscalls ENOSYS.**
  Honest to decision 5, but it leaves the surface permanently unable to create and delete the files and directories SQLite's journal/WAL lifecycle (decision 14's stated goal) needs.
  The impossibility argument (D2) tips it: this is the one capability pure Bash *cannot* provide, so it is the one place the criterion earns a narrow exception.
- **Loadable builtins (`enable -f mkdir.so`).**
  A platform-specific `.so` is a heavier and less portable dependency than a POSIX command already guaranteed wherever Bash runs.
- **A virtual filesystem overlay in Bash arrays.**
  State would diverge from the host (the whole point of `--dir` is to touch real host files) and the standalone `--dir` snapshots, captured under wasmtime (decision 9), would not match.
- **General external-command use for the rest of the surface** (`od`/`dd` for bytes, `stat` for metadata).
  Rejected as decision 5 always rejected them: those capabilities *are* expressible in pure Bash (byte-wise `read`/`printf`, test builtins), so there is no impossibility to license the exception.

## Consequences

- Positive: the Bash backend reaches the same WASI p1 filesystem surface as Ruby, so `--dir` round-trips and the shared filesystem e2e suite apply to it; a module that imports no namespace-mutation syscall stays pure Bash.
- Negative / accepted: whole-file buffering makes open/close O(size) and the slurp/flush cost ~microseconds per byte, in line with decision 5's "value is existence, not speed".
  The D2 dependency-statement change is real: a module importing the four mutation syscalls now also depends on `mkdir`/`rmdir`/`rm`/`mv`.
  The D1/D3/D6 caveats (two-fd divergence, non-atomic flush, TOCTOU, file-symlink ELOOP, zeroed timestamps/dev/ino) are standing deviations, not bugs.
- After the decision 40 revision above, the only WASI p1 functions still ENOSYS on Bash are the timestamp setters `fd_filestat_set_times` and `path_filestat_set_times` (`docs/support.md`), because `touch` fails D2's criterion: setting a timestamp is not namespace mutation.
