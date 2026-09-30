# Decision 40: WASI p1 Completion (Symlinks, Enforced Per-Fd Rights, Conformance-Runner Environment)

Status: **Accepted, 2026-07-28.**
Implemented across three places:

- all five backends (`runtime/<lang>/units/wasi/`);
- the wasi-testsuite runner (`crates/dewasm-test-helper/src/{backend,wasi_testsuite}.rs`);
- the five per-backend expected-failures lists.

The lists shrank from ~30 rows per backend to the honest residue listed under Consequences.
`docs/support.md` now shows full WASI p1 columns for Ruby/Python/Go/Java and near-full for Bash.

## Context

Decision 36's conformance harness attributed every known failure to a declared gap.
Three of those declarations were themselves decisions this decision revisits:

- Decision 14 deferred the symlink-family syscalls.
  Its reason was that creating links widens the accepted TOCTOU surface.
  It also deferred the rights syscalls.
  Its reason was that reporting narrowed rights while enforcing nothing would mislead.
- Decision 34 capped Bash's external-command license at `mkdir`/`rmdir`/`rm`/`mv`.
- Decision 36 rejected clearing the child environment for the count-exact `environ_*` trials.

Filling the remaining gaps honestly meant deciding each one, not just coding it.

## Decision

1. **The symlink family is implemented; containment moves from create-time to follow-time.**
   `path_symlink` writes the guest's target string *verbatim*; it is never pre-resolved.
   A link pointing outside the sandbox is legal until followed.
   Every follow already passes the realpath + prefix-containment check (`resolve_path`, decision 14).
   `path_readlink` and `path_link` resolve the link itself with the NOFOLLOW shape.
   `path_link` contains both endpoints.
   Criterion: *enforce at the operation that can escape, not the operation that merely stores a name.*
   This deliberately widens the surface decision 14 passively accepted.
   The TOCTOU caveat itself is unchanged (check-then-open, not `openat`-beneath).
2. **Per-fd rights are tracked, reported, and enforced.**
   Each fd carries `(rights_base, rights_inheriting, fdflags)` in a parallel meta table.
   `path_open` grants `requested ∩ dirfd.inheriting`, capped to per-filetype masks.
   The masks are wasmtime's directory/regular-file sets.
   `fd_fdstat_get` reports the stored values, and `fd_fdstat_set_rights` narrows only.
   Some syscalls return `NOTCAPABLE` when the right is absent.
   They are `fd_read`/`fd_write`/`fd_seek`/`fd_readdir`/`fd_filestat_set_size`.
   Criterion: *report only what is enforced.*
   This inverts decision 14's deferral for decision 14's own reason.
   APPEND lives in the fdflags meta and is honored by `fd_write` seeking to end.
   So `fd_fdstat_set_flags` can turn it off at runtime, which a kernel `O_APPEND` handle cannot.
3. **Bash's external-command license (decision 34 D2) extends to `ln -s`, `ln`, and `readlink`.**
   `ln`/`ln -s` are namespace mutations with no pure-Bash form, D2's own criterion.
   `readlink` is a read, licensed under a companion clause.
   The clause is *a deliberately added capability must be complete*.
   Creating links but not reading them back is incoherent.
   The line holds against `stat`: dev/ino stays zeroed (D6).
   It also holds against `touch`: timestamps stay ENOSYS, since setting them is not namespace mutation.
4. **The conformance runner clears the child environment** and sets exactly the manifest env.
   The code is `run_standalone_wasi`.
   This reverses decision 36's rejected alternative.
   Upstream's wasmtime adapter passes env solely via `--env`.
   So a cleared child reproduces the same observable guest environment.
   It does so without touching decision 31's whole-env passthrough in generated programs.
   The first attempt surfaced two consequences:
   - Bare interpreter names must be resolved against the *parent* PATH before spawning.
     An env-cleared exec falls back to the OS default path and picks the system ruby 2.6 / bash 3.2.
   - Interpreted hosts inject environ entries past a cleared environment.
     The guest legitimately observes them.
     They are CoreFoundation's `__CF_USER_TEXT_ENCODING` and CPython's PEP 538 `LC_CTYPE`.
     They are also bash's `PWD`/`SHLVL`/`_`.

   Those rows stay listed, re-attributed to the injection.

## Rejected alternatives

- **Track-and-report rights without enforcement.**
  Decision 14's original objection stands: it looks like a sandbox and isn't.
  Enforcement at five syscall entry points is cheap (one mask test).
- **Pre-resolving symlink targets at create time.**
  It breaks legal guests, such as relative links into not-yet-mounted trees.
  It also adds nothing: escape is only possible at follow time, where the existing check already sits.
- **Licensing `stat`/`touch` for Bash alongside `ln`.**
  Neither is required by a capability this decision adds.
  D6 already accepts zeroed dev/ino, and timestamp syscalls stay declared-ENOSYS on Bash.
- **Keeping the environ rows attributed to decision 31.**
  The interface passes the env through faithfully.
  After the runner fix the remaining mismatch is host injection.
  Blaming the interface would misdirect any future fix.

## Consequences

- Positive: the five wasi-testsuite lists drop to their honest floor:

  | Rows | Backends | Cause |
  | --- | --- | --- |
  | `sock_shutdown` ×2 | all | out of scope, decision 24 |
  | `environ` ×3 | the four interpreted backends | host injection; Go's compiled binaries pass |
  | `path_link` | Java | hard-linking a dangling symlink needs `linkat(2)` nofollow, inexpressible in NIO; Ruby reaches it via Fiddle, Go by recreating the link |
  | `rust/symlink_filestat` | Go | no portable build-tag-free `lutimes` in Go std |
  | the declared set | Bash | timestamps ×2 under D4, d_ino/dev-ino ×3 under D6, cross-fd read-back under D1, and three file-symlink-follow ELOOP re-tags under D3 |

- Positive: real-app suites (SQLite, QuickJS, CPython, CRuby, ripgrep) keep passing under enforcement.
  The reason is that preopens seed the canonical directory rights with full inheriting sets.
- Negative: the rights meta table adds a lookup to the hot `fd_read`/ `fd_write` paths on every backend.
  The Bash symlink units spawn licensed external commands.
- Carry-over: closing the TOCTOU gap for real (cap-std-style `openat`- beneath) remains out of scope.
  This is as in decision 14.

Revises:

- [decision 14](14-ruby-wasi-filesystem.md) (rights + symlink deferrals);
- [decision 34](34-bash-wasi-filesystem.md) (D2 license, carry-over list);
- [decision 36](36-wasi-testsuite-conformance.md) (environment-clearing alternative).
