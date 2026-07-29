# ADR-49 — Where the WASI Spec Is Silent, Follow wasmtime; Host-Pinned Errno Modes for wasi-testsuite

Status: **Accepted, 2026-07-29.** Implemented in PR #43 (issue #42): the
trailing-slash behavior of every backend's `path_*` units matches wasmtime 47
as measured on macOS and Linux, and the wasi-testsuite harness injects the
host-matched strict errno mode into the Rust suite's trials.

## Context

WASI preview 1 does not define trailing-slash path semantics, and preview 2's
`path-resolution.md` is equally silent. Issue #42 showed each backend invented
its own answer (mostly: drop the slash before the host could see it); the
first fix pinned POSIX pathname-resolution semantics instead, which turned out
to *contradict* the reference runtime — wasmtime/cap-std strips the slash on a
nonexistent rename destination and renames anyway, rejects `rmdir("dir/")`
with EINVAL, and splits `open(O_CREAT, "x/")` per host (EINVAL on macOS from
its manual resolution, EISDIR on Linux via passthrough). Meanwhile the
upstream wasi-testsuite's `assert_errno!` runs *Permissive* by default —
accepting the union of its `unix`/`macos`/`windows` arms — which is exactly
how these divergences had gone undetected; its intended strict usage is to set
one `ERRNO_MODE_*` variable.

### Survey of other implementations (2026-07-29)

The 25-probe matrix was also run (macOS host) under wasmer 7.2.1, WasmEdge
0.17.1, and Node 24 (`node:wasi`/uvwasi); the full table is in PR #43. On the
resolution family (slash over an existing non-directory → ENOTDIR, missing →
ENOENT, the symlink-destination and unlink-of-directory shapes) wasmtime's
behavior is the consensus. On the contentious shapes it is not uniformly so:
`rmdir("dir/")` → EINVAL is **wasmtime alone** (WasmEdge and Node both
succeed, the POSIX reading), and EINVAL for O_CREAT through `"newf/"` on
macOS is matched by no other runtime (WasmEdge/Node pass the host's ENOENT
through). Stripping the slash on a nonexistent rename destination is shared
with WasmEdge (Node reports the host's ENOENT), as is `mkdir("file/")` →
EEXIST (Node passes the host's ENOTDIR through). WasmEdge itself silently
renames through a slash-suffixed *source* — the issue-42 bug class — and
wasmer 7's virtual-fs overlay denies all namespace mutations on mapped
volumes (EACCES/NOTCAPABLE), leaving only its resolution probes usable. The
survey did not change the decision: the user chose one reference to copy
rather than a majority vote per shape, and wasmtime — the runtime the
upstream testsuite itself encodes — remains it.

## Decision

**Where the WASI spec is silent, dewasm's WASI behavior follows wasmtime's
observed behavior (currently wasmtime 47), not POSIX and not invented
semantics.** The reusable rule: a spec-silent behavior question is settled by
measuring wasmtime on both CI hosts — if wasmtime is host-uniform, backends
implement that value deterministically; if wasmtime inherits a host split
(e.g. unlink-of-directory: EPERM on macOS, EISDIR on Linux), backends surface
the same per-host split; if wasmtime's behavior on one host is an internal
artifact with no portable expectation (its Linux-only slash-strip in
nofollow `path_filestat_get`), the shape is left unpinned and documented at
the test site (`examples/wat/wasi_trailing_slash.wat`).

Correspondingly, the wasi-testsuite harness
(`crates/dewasm-test-helper/src/wasi_testsuite.rs`) injects the host-matched
strict errno mode — `ERRNO_MODE_MACOS` on macOS, `ERRNO_MODE_UNIX` on Linux —
into the **Rust suite's** guest environment, closing the errno-union hole.
This is a deliberate, commented deviation from the manifest-only-environment
rule (commit 02c5ef5): no Rust trial counts environ entries, and the
C/assemblyscript suites — which do — are left untouched (they ignore
`ERRNO_MODE_*` anyway).

## Rejected alternatives

- **Pin POSIX pathname-resolution semantics** (the first revision of PR #43)
  — self-consistent and standard-backed, but it invents a WASI surface no
  runtime exhibits: guests are built and tested against wasmtime, and the
  upstream testsuite's strict modes assert wasmtime's host-split errnos, so a
  POSIX-pinned backend fails the strict suite on at least one host. Rejected
  by user decision: no semantics beyond the reference implementation.
- **Keep the suite Permissive** — hides real divergence (the issue-42 bugs
  passed Permissive); strictness is the upstream-intended usage.
- **Inject the errno mode into all three suites** — the C/assemblyscript
  suites assert exact environ contents in places; scoping to Rust keeps the
  02c5ef5 isolation intact where it matters.

## Consequences

- Positive: one measurable arbiter for every future spec-silent question;
  the strict suite now pins per-host errnos on all five backends, on both CI
  hosts.
- Negative: some units carry host-OS branches (`RUBY_PLATFORM`,
  `sys.platform`, `runtime.GOOS`, `os.name`, `$OSTYPE`) to reproduce
  wasmtime's splits, and wasmtime quirks are copied as-is — e.g. a rename
  onto `"newd/"` silently creates the plain file `newd`, and
  `rmdir("dir/")` is EINVAL.
- Carry-over: expectations are pinned to wasmtime 47's observed behavior; a
  future wasmtime that changes a spec-silent shape forces a re-measure and a
  revision here. The PR #41 bash pins remain, re-based on this rule
  (`crates/dewasm-backend-bash/tests/wasi_fs_regressions.rs`).
