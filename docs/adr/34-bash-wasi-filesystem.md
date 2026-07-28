# ADR-34 — Bash WASI Filesystem

Status: **Accepted, 2026-07-27; fully landed 2026-07-28.** The Bash backend
gains WASI preview-1 filesystem support, mirroring the Ruby design
([ADR-14](14-ruby-wasi-filesystem.md)) within Bash's constraints: the fd-table
state, `path_open`, the reworked
`fd_read`/`fd_write`/`fd_seek`/`fd_tell`/`fd_close`/`fd_fdstat_get`/`fd_prestat_get`
units, `fd_prestat_dir_name`, the standalone `--dir` parser, the stat family,
the four namespace-mutation syscalls, and `poll_oneoff` — all under
`runtime/bash/units/wasi/`. The same syscalls that stay ENOSYS on Ruby stay
ENOSYS here.

## Context

[ADR-12](12-bash-wasi.md) built the Bash WASI surface (stdio, args/env,
clock, random) but scoped every `path_*` and filesystem-only `fd_*` call
out to ENOSYS — a 15-function gap versus Ruby. [ADR-14](14-ruby-wasi-filesystem.md)
answered those questions for Ruby and closed noting a Bash filesystem
backend "would need its own ADR". This is it. Bash makes the design
different in kind, not degree, from Ruby's `File`-backed fds: Bash has no
random-access file object, no `openat`, no `realpath`/`readlink`/`stat`
builtins, and — under [ADR-5](5-bash-softfloat.md)'s dependency criterion —
no license to shell out for them either. The decisions below are what that
leaves possible.

## Decision

### D1 — Files are whole-file byte buffers, flushed on close/sync

A Bash redirection can only truncate-create (`>`) or append (`>>`) a file;
it cannot write at an offset. So an open file fd holds the entire file
content as an indexed array of byte ordinals (`<p>wbuf<fd>`), with a
separate offset (`<p>wtell[fd]`). `path_open` slurps the file into the
array (skipping the slurp for `O_TRUNC` or a fresh `O_CREAT`); `fd_read`/
`fd_write`/`fd_seek` operate on the array and the offset; a dirty buffer is
flushed to disk on `fd_close` (and later on `fd_sync`/`fd_datasync`) by a
single `exec {fd}>"$path"` followed by chunked `printf` of a `'\x%02x'`
format built in ~4 KiB batches. Criterion: represent a file as the one
thing Bash *can* rewrite atomically — the whole thing — rather than
emulating random-access I/O Bash does not have.

Caveats, recorded not fixed: two fds open on one file diverge (each has its
own buffer; last flush wins), open and close are O(file size), the flush is
non-atomic (a crash mid-flush leaves a partial file), and `fd_sync`/
`fd_datasync` are just a flush — there is no separate durability barrier.

### D2 — Four namespace-mutation syscalls may call one POSIX command each

**This is the headline decision.** [ADR-5](5-bash-softfloat.md)'s criterion —
the generated program's dependency set is *exactly a Bash interpreter* — is
narrowed here: the four namespace-mutation units `path_create_directory`,
`path_remove_directory`, `path_unlink_file`, and `path_rename` (and **only**
they) may invoke the POSIX-mandated `mkdir` / `rmdir` / `rm` / `mv`
respectively, as a single direct `command` invocation, `--`-guarded, on
resolved absolute paths, with the errno derived from post-hoc `[[ -e ]]` /
`[[ -d ]]` probes rather than the command's own diagnostics.

The justification is impossibility, not convenience: pure Bash cannot
express the creation, removal, or renaming of a directory entry *at all* —
there is no builtin that mutates the namespace, the way `>` mutates file
content. Everything else the filesystem surface needs (read, write, stat by
test builtins, directory listing by globbing) is expressible in pure Bash;
these four are not. Because runtime units are bundled per import
([ADR-6](6-runtime-units.md)), a converted module that never imports these
four syscalls carries none of these commands and remains a pure-Bash
artifact — the ADR-5 promise still holds for the programs that do not need
namespace mutation, which is the property worth protecting.

### D3 — Sandboxing: physical resolution plus per-dirfd containment

Preopen roots are resolved to a physical path once, via
`$(cd -P -- "$host" && pwd -P)`. Every guest path resolution re-derives the
parent's physical path the same way and checks prefix-containment against
*that dirfd's own* stored root — nesting cannot launder an escape one level
cheaper, exactly the model [ADR-14](14-ruby-wasi-filesystem.md) uses. The
containment test is `[[ $real == "$root" || $real == "${root%/}/"* ]]`; the
`${root%/}/` form makes a root of `/` contain everything.

Two deviations from Ruby, both from missing builtins and both documented:
Bash has no `readlink`, so a **file** symlink as the final path component
cannot be followed and resolves to `ELOOP` (stricter than Ruby, which
follows it); a **directory** symlink is still followed, because `cd -P`
resolves it. The check-then-open TOCTOU caveat from ADR-14 carries over
unchanged: this is a single-process research/demo runtime, not a
multi-tenant sandbox host.

### D4 — `poll_oneoff` (later step)

Implemented in a later step, summarized here so the shape is on record: an
`fd_read`-on-stdin subscription waits with `read -t <deadline> -n 1`, and if
a byte arrives it is held in a one-byte pushback slot (`<p>wpush`) for the
next `fd_read` to consume; a clock-only subscription sleeps via a bash-only
`coproc` timer (`coproc __slp { read _; }` then `read -rt <secs> -u
"${__slp[0]}"`) rather than the process-substitution idiom
`read -t <secs> <> <(:)` — the latter is rejected (`Permission denied`) on
some hosts (macOS), so the coproc is the portable one. Every other
subscription (regular files, stdout/stderr, `fd_write`) is immediately
ready, as on Ruby. One deviation: when a stdin wait hits real EOF (not a
timeout), this unit reports each waiting `fd_read` ready with `nbytes` 0,
where Ruby's `IO.select`-based equivalent instead reports the closed fd
itself as readable and lets the subsequent `fd_read` discover EOF — both
converge on the guest's next `fd_read` returning 0 bytes, so the observable
behavior matches even though the intermediate event differs.

### D5 — fd-table shape

One kind table `<p>wfds` (stdio=1, file=2, dir=3) keyed by fd, with parallel
indexed arrays for the rest: `<p>wtell` (offset), `<p>wpath` (physical host
path), `<p>wname` (preopen guest name — set iff prestat-visible), `<p>wrd`/
`<p>wwr`/`<p>wapp` (open-mode flags, derived once and never re-checked, the
ADR-14 rights policy), `<p>wdirty`, and the per-fd `<p>wbuf<fd>` byte array.
`<p>wnext` is the next fd (starting past the preopens, never reused, per
ADR-14). Preopens arrive through an ordered indexed array
`WASI_DIRS=('HOST::GUEST' ...)` — the Bash analogue of Ruby's `preopens:`
kwarg — consumed by `init_preopens` into dir fds from 3 upward. The
standalone main fills `WASI_DIRS` from repeated `--dir` flags
([ADR-31](31-standalone-runtime-interface.md)).

### D6 — stat fidelity (stat family lands later)

Filetype comes from the test builtins (`-d`/`-f`/`-h`/`-t`); `size` is the
live buffer length for an open fd. `atim`/`mtim`/`ctim` report 0 and
`dev`/`ino` report 0 — Bash cannot stat a file for real numbers — a
documented deviation, the filesystem analogue of ADR-12's clock fallback.

## Rejected alternatives

- **Leave the four namespace-mutation syscalls ENOSYS.** Honest to ADR-5
  but leaves the surface permanently unable to do what SQLite's journal/WAL
  lifecycle (ADR-14's north star) needs — create and delete files and
  directories. The impossibility argument (D2) is what tips it: this is the
  one capability pure Bash *cannot* provide, so it is the one place the
  criterion earns a narrow exception.
- **Loadable builtins (`enable -f mkdir.so`).** A platform-specific `.so`
  is a heavier and less portable dependency than a POSIX command already
  guaranteed on every system with a Bash; it defeats the "runs anywhere
  Bash runs" property more than one `command mkdir` does.
- **A virtual filesystem overlay in Bash arrays.** State would diverge from
  the host (the whole point of `--dir` is to touch real host files) and the
  standalone `--dir` goldens, captured under wasmtime (ADR-9), would not
  match.
- **General external-command use for the rest of the surface** (`od`/`dd`
  for bytes, `stat` for metadata). Rejected as ADR-5 always rejected them:
  those capabilities *are* expressible in pure Bash (byte-wise `read`/
  `printf`, test builtins), so there is no impossibility to license the
  exception. D2 is scoped to exactly the operations that have no pure-Bash
  form.

## Consequences

- Positive: the Bash backend reaches the same WASI p1 filesystem surface as
  Ruby, so `--dir` round-trips and the shared filesystem e2e suite apply to
  it; a module that imports no namespace-mutation syscall stays pure Bash.
- Negative / accepted: whole-file buffering makes open/close O(size) and the
  slurp/flush cost ~microseconds per byte — in line with ADR-5's "value is
  existence, not speed". The D2 dependency-statement change is real: a
  module importing the four mutation syscalls now also depends on
  `mkdir`/`rmdir`/`rm`/`mv`. The D1/D3/D6 caveats (two-fd divergence,
  non-atomic flush, TOCTOU, file-symlink ELOOP, zeroed timestamps/dev/ino)
  are standing deviations, not bugs.
- Carry-over: `fd_advise`, `fd_allocate`, `fd_renumber`, the symlink
  syscalls, rights-narrowing (`fd_fdstat_set_flags`/`_rights`), and
  `path_filestat_set_times` stay ENOSYS, exactly as on Ruby (ADR-14) — the
  lower-value and security-motivated gaps are not reopened here.
