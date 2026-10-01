# Decision 40: WASI p1 Completion (Symbolic Links, Checked `fd` Rights, Conformance-Runner Environment)

Status: **Accepted, 2026-07-28.**
Implemented across three places:

- all five backends (`runtime/<lang>/units/wasi/`);
- the `wasi-testsuite` runner (`crates/dewasm-test-helper/src/{backend,wasi_testsuite}.rs`);
- the five per-backend expected-failures lists.

The lists fell from ~30 rows per backend to the honest residue listed under Consequences.
`docs/support.md` now shows full WASI p1 columns for Ruby/Python/Go/Java and near-full for Bash.

## Context

Decision 36's conformance harness attributed every known failure to a declared gap.
Three of those declarations were themselves decisions this decision revisits:

- Decision 14 deferred the symbolic link family of system calls.
  Its reason was that creating links widens the accepted TOCTOU surface.
  It also deferred the rights system calls.
  Its reason was that reporting narrowed rights while checking nothing would mislead.
- Decision 34 capped Bash's external-command license at `mkdir`/`rmdir`/`rm`/`mv`.
- Decision 36 rejected clearing the child environment for the count-exact `environ_*` trials.

Filling the remaining gaps honestly meant deciding each one, not just coding it.

## Decision

1. **The symbolic link family is implemented; containment moves from create-time to follow-time.**
   `path_symlink` writes the guest's target string *unchanged*; it is never pre-resolved.
   That is because a link pointing outside the sandbox is legal until followed.
   Every follow already passes the `realpath` + prefix-containment check (`resolve_path`, decision 14).
   `path_readlink` and `path_link` resolve the link itself with the NOFOLLOW shape.
   `path_link` contains both paths.
   Criterion: *check at the operation that can escape, not the operation that merely stores a name.*
   This deliberately widens the surface decision 14 passively accepted.
   The TOCTOU limit itself is unchanged (check-then-open, not `openat`-beneath).
2. **Rights per `fd` are tracked, reported, and checked.**
   Each `fd` carries `(rights_base, rights_inheriting, fdflags)` in a parallel table.
   `path_open` grants `requested ∩ dirfd.inheriting`, capped to masks per file type.
   The masks are Wasmtime's directory/regular-file sets.
   `fd_fdstat_get` reports the stored values, and `fd_fdstat_set_rights` narrows only.
   Some system calls return `NOTCAPABLE` when the right is missing.
   They are `fd_read`/`fd_write`/`fd_seek`/`fd_readdir`/`fd_filestat_set_size`.
   Criterion: *report only what is checked.*
   This inverts decision 14's deferral for decision 14's own reason.
   APPEND lives in the stored `fdflags` and is honored by `fd_write` seeking to end.
   So `fd_fdstat_set_flags` can turn it off at runtime, which an OS `O_APPEND` handle cannot.
3. **Bash's external-command license (decision 34 D2) extends to `ln -s`, `ln`, and `readlink`.**
   `ln`/`ln -s` are namespace mutations with no form in Bash alone, D2's own criterion.
   `readlink` is a read, licensed under a related clause.
   The clause is *a deliberately added capability must be complete*.
   Creating links but not reading them back makes no sense.
   The line holds against `stat`: `dev`/`ino` stays zeroed (D6).
   It also holds against `touch`: timestamps stay ENOSYS, since setting them is not namespace mutation.
4. **The conformance runner clears the child environment** and sets exactly the manifest's environment.
   The code is `run_standalone_wasi`.
   This reverses decision 36's rejected alternative.
   Upstream's Wasmtime adapter passes the environment only through `--env`.
   So a cleared child reproduces the same observable guest environment.
   It does so without touching decision 31: generated programs still pass the whole environment.
   The first attempt surfaced two consequences:
   - Interpreter names without a path must be resolved against the *parent* PATH before starting the child.
     An `exec` with a cleared environment falls back to the OS default path.
     It then picks the system Ruby 2.6 / Bash 3.2.
   - Interpreted hosts inject `environ` entries past a cleared environment.
     The guest legitimately observes them.
     They are CoreFoundation's `__CF_USER_TEXT_ENCODING` and CPython's PEP 538 `LC_CTYPE`.
     They are also Bash's `PWD`/`SHLVL`/`_`.

   Those rows stay listed, re-attributed to the injection.

## Rejected alternatives

- **Track-and-report rights without checking them.**
  Decision 14's original objection stands: it looks like a sandbox and isn't.
  Checking at five system call entry points is cheap (one mask test).
- **Pre-resolving symbolic link targets at create time.**
  It breaks legal guests, such as relative links into not-yet-mounted trees.
  It also adds nothing: escape is only possible at follow time, where the existing check already sits.
- **Licensing `stat`/`touch` for Bash alongside `ln`.**
  Neither is required by a capability this decision adds.
  D6 already accepts zeroed `dev`/`ino`, and timestamp system calls stay declared as ENOSYS on Bash.
- **Keeping the `environ` rows attributed to decision 31.**
  The interface passes the environment through faithfully.
  After the runner fix the remaining mismatch is host injection.
  Blaming the interface would misdirect any future fix.

## Consequences

- Positive: the five `wasi-testsuite` lists drop to their honest floor:

  | Rows | Backends | Cause |
  | --- | --- | --- |
  | `sock_shutdown` ×2 | all | out of scope, decision 24 |
  | `environ` ×3 | the four interpreted backends | host injection; Go's compiled binaries pass |
  | `path_link` | Java | hard-linking a symbolic link whose target is missing needs `linkat(2)` without following, which NIO cannot express; Ruby reaches it via Fiddle, Go by recreating the link |
  | `rust/symlink_filestat` | Go | no portable `lutimes` in the Go standard library that needs no build tag |
  | the declared set | Bash | timestamps ×2 under D4, `d_ino` and `dev`/`ino` ×3 under D6, read-back across `fd`s under D1, and three ELOOP rows for following a file symbolic link, re-attributed under D3 |

- Positive: real-app suites (SQLite, QuickJS, CPython, CRuby, `ripgrep`) keep passing under the checks.
  The reason is that preopens seed the canonical directory rights with full `rights_inheriting` sets.
- Negative: the rights table adds a lookup to the hot `fd_read`/ `fd_write` paths on every backend.
  The Bash symbolic link units start licensed external commands.
- Carry-over: closing the TOCTOU gap for real (`openat`-beneath, as `cap-std` does) remains out of scope.
  This is as in decision 14.

Revises:

- [decision 14](14-ruby-wasi-file-system.md) (rights + symbolic link deferrals);
- [decision 34](34-bash-wasi-file-system.md) (D2 license, carry-over list);
- [decision 36](36-wasi-testsuite-conformance.md) (environment-clearing alternative).
