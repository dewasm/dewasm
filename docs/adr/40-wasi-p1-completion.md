# ADR-40 — WASI p1 Completion: Symlink Family, Enforced Per-Fd Rights, and the Conformance-Runner Environment

Status: **Accepted, 2026-07-28.**
Implemented across all five backends (`runtime/<lang>/units/wasi/`), the wasi-testsuite runner (`crates/dewasm-test-helper/src/{backend,wasi_testsuite}.rs`), and the five per-backend expected-failures lists.
The lists shrank from ~30 rows per backend to the honest residue listed under Consequences; `docs/support.md` now shows full WASI p1 columns for Ruby/Python/Go/Java and near-full for Bash.

## Context

ADR-36's conformance harness attributed every known failure to a declared gap.
Three of those declarations were themselves decisions this ADR revisits: ADR-14 deferred the symlink-family syscalls (creating links widens the accepted TOCTOU surface) and the rights syscalls (reporting narrowed rights while enforcing nothing would mislead); ADR-34 capped Bash's external-command license at `mkdir`/`rmdir`/`rm`/`mv`; and ADR-36 rejected clearing the child environment for the count-exact `environ_*` trials.
Filling the remaining gaps honestly meant deciding each one, not just coding it.

## Decision

1. **The symlink family is implemented; containment moves from create-time to follow-time.**
   `path_symlink` writes the guest's target string *verbatim* — it is never pre-resolved, because a link pointing outside the sandbox is legal until followed, and every follow already passes the realpath + prefix-containment check (`resolve_path`, ADR-14).
   `path_readlink` and `path_link` resolve the link itself with the NOFOLLOW shape; `path_link` contains both endpoints.
   Criterion: *enforce at the operation that can escape, not the operation that merely stores a name.*
   This deliberately widens the surface ADR-14 passively accepted; the TOCTOU caveat itself is unchanged (check-then-open, not `openat`-beneath).
2. **Per-fd rights are tracked, reported, and enforced.**
   Each fd carries `(rights_base, rights_inheriting, fdflags)` in a parallel meta table; `path_open` grants `requested ∩ dirfd.inheriting` capped to per-filetype masks (wasmtime's directory/regular-file sets); `fd_fdstat_get` reports the stored values; `fd_fdstat_set_rights` narrows only; and `fd_read`/`fd_write`/`fd_seek`/`fd_readdir`/`fd_filestat_set_size` return `NOTCAPABLE` when the right is absent.
   Criterion (inverting ADR-14's deferral for its own reason): *report only what is enforced.*
   APPEND lives in the fdflags meta and is honored by `fd_write` seeking to end, so `fd_fdstat_set_flags` can turn it off at runtime — which a kernel `O_APPEND` handle cannot.
3. **Bash's external-command license (ADR-34 D2) extends to `ln -s`, `ln`, and `readlink`.**
   `ln`/`ln -s` are namespace mutations with no pure-Bash form — D2's own criterion.
   `readlink` is a read, licensed under a companion clause: *a deliberately added capability must be complete* (creating links but not reading them back is incoherent).
   The line holds against `stat` (dev/ino stays zeroed, D6) and `touch` (timestamps stay ENOSYS — not namespace mutation).
4. **The conformance runner clears the child environment** and sets exactly the manifest env (`run_standalone_wasi`), reversing ADR-36's rejected alternative: upstream's wasmtime adapter passes env solely via `--env`, so a cleared child reproduces the same observable guest environment without touching ADR-31's whole-env passthrough in generated programs.
   Two consequences the first attempt surfaced: bare interpreter names must be resolved against the *parent* PATH before spawning (an env-cleared exec falls back to the OS default path and picks the system ruby 2.6 / bash 3.2), and interpreted hosts inject environ entries past a cleared environment (CoreFoundation's `__CF_USER_TEXT_ENCODING`, CPython's PEP 538 `LC_CTYPE`, bash's `PWD`/`SHLVL`/`_`) which the guest legitimately observes — those rows stay listed, re-attributed to the injection.

## Rejected alternatives

- **Track-and-report rights without enforcement.**
  ADR-14's original objection stands: it looks like a sandbox and isn't.
  Enforcement at five syscall entry points is cheap (one mask test).
- **Pre-resolving symlink targets at create time.**
  Breaks legal guests (relative links into not-yet-mounted trees) and adds nothing: escape is only possible at follow time, where the existing check already sits.
- **Licensing `stat`/`touch` for Bash alongside `ln`.**
  Neither is required by a capability this ADR adds; D6 already accepts zeroed dev/ino and timestamp syscalls stay declared-ENOSYS on Bash.
- **Keeping the environ rows attributed to ADR-31.**
  The interface passes the env through faithfully; after the runner fix the remaining mismatch is host injection, and blaming the interface would misdirect any future fix.

## Consequences

- Positive: the five wasi-testsuite lists drop to their honest floor — `sock_shutdown` ×2 everywhere (out of scope, ADR-24); `environ` ×3 on the four interpreted backends (host injection; Go's compiled binaries pass); Java `path_link` (hard-linking a dangling symlink needs `linkat(2)` nofollow, inexpressible in NIO — Ruby reaches it via Fiddle, Go by recreating the link); Go `rust/symlink_filestat` (no portable build-tag-free `lutimes` in Go std); Bash's declared set (timestamps ×2 under D4, d_ino/dev-ino ×3 under D6, cross-fd read-back under D1, and three file-symlink-follow ELOOP re-tags under D3).
- Positive: real-app suites (SQLite, QuickJS, CPython, CRuby, ripgrep) keep passing under enforcement because preopens seed the canonical directory rights with full inheriting sets.
- Negative: the rights meta table adds a lookup to the hot `fd_read`/ `fd_write` paths on every backend; the Bash symlink units spawn licensed external commands.
- Carry-over: closing the TOCTOU gap for real (cap-std-style `openat`- beneath) remains out of scope, as in ADR-14.

Revises: [ADR-14](14-ruby-wasi-filesystem.md) (rights + symlink deferrals), [ADR-34](34-bash-wasi-filesystem.md) (D2 license, carry-over list), [ADR-36](36-wasi-testsuite-conformance.md) (environment-clearing alternative).
