# Decision 49: Where WASI Is Silent, Follow wasmtime; Host-Pinned Errno Modes for wasi-testsuite

Status: **Accepted, 2026-07-29.**
Implemented in PR #43 (issue #42).
Decision 80 records the one exception to date: `fd_fdstat_set_flags` accepts any open fd.
There, copying wasmtime would make the toywasm app unrunnable.

## Context

WASI p1 (and p2's `path-resolution.md`) says nothing about trailing-slash paths.
The shapes are inconsequential.
A real program breaking on one of these errnos is astronomically unlikely.
Still, each backend needs *some* answer.
The first fix for issue #42 pinned POSIX, which contradicts wasmtime on a few shapes.
The upstream wasi-testsuite's `assert_errno!` runs Permissive by default.
Permissive is the union of its per-OS arms.
That is how the divergences went unnoticed.
Setting one `ERRNO_MODE_*` is its intended strict usage.

Survey (macOS; full table in PR #43):

- wasmer 7 can't run the mutation probes: its VFS denies them.
- WasmEdge 0.17 and Node 24 agree with wasmtime on the ENOTDIR resolution family.
  They do not agree on the quirks.
- `rmdir("dir/")` EINVAL and macOS O_CREAT-through-slash EINVAL are wasmtime alone.

Noted for the record; it changed nothing.

## Decision

**Where the WASI spec is silent, backends copy wasmtime's observed behavior** (currently 47).
It is measured on both CI hosts:

- Host-uniform values are implemented deterministically.
- Host splits wasmtime inherits are reproduced per host.
  An example is unlink of a directory: EPERM/EISDIR.
- One-host internal artifacts stay unpinned, noted at the test site.
  An example is wasmtime's Linux-only nofollow-stat slash strip.

wasmtime is the pick because the upstream testsuite encodes it, not because its behavior is better.
Where it is an outlier, the value is arbitrary and not worth debating.

The wasi-testsuite runner injects the host-matched strict errno mode.
The mode is `ERRNO_MODE_MACOS` or `ERRNO_MODE_UNIX`, set in the Rust suite's guest environment.
This is a deliberate deviation from the manifest-only-env rule (commit 02c5ef5).
It is scoped to Rust because the C/assemblyscript suites assert exact environ contents.

## Rejected alternatives

- **POSIX semantics** (this PR's first revision): nothing tests against POSIX.
  It fails the strict suite on at least one host.
- **Permissive errno modes**: hides real divergence.
- **Majority vote across runtimes**: effort spent on shapes that don't matter.
  It also has no canonical electorate.

## Consequences

Some units carry host-OS branches to reproduce wasmtime's splits; its quirks are copied as-is.
A future wasmtime change to a spec-silent shape forces a re-measure.
The PR #41 bash pins are re-based on this rule.
They are in `crates/dewasm-backend-bash/tests/wasi_fs_regressions.rs`.
