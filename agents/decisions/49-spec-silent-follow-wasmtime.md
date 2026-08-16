# Decision 49: Where the WASI Spec Is Silent, Follow wasmtime; Host-Pinned Errno Modes for wasi-testsuite

Status: **Accepted, 2026-07-29.**
Implemented in PR #43 (issue #42).
Decision 80 records the one exception to date: `fd_fdstat_set_flags` accepts any open fd, where copying wasmtime would make the toywasm app unrunnable.

## Context

WASI p1 (and p2's `path-resolution.md`) says nothing about trailing-slash paths, and the shapes are inconsequential: a real program breaking on one of these errnos is astronomically unlikely.
Still, each backend needs *some* answer; the first fix for issue #42 pinned POSIX, which contradicts wasmtime on a few shapes.
The upstream wasi-testsuite's `assert_errno!` runs Permissive by default (the union of its per-OS arms), which is how the divergences went unnoticed; setting one `ERRNO_MODE_*` is its intended strict usage.

Survey (macOS; full table in PR #43): wasmer 7 can't run the mutation probes (its VFS denies them); WasmEdge 0.17 and Node 24 agree with wasmtime on the ENOTDIR resolution family but not on the quirks.
`rmdir("dir/")` EINVAL and macOS O_CREAT-through-slash EINVAL are wasmtime alone.
Noted for the record; it changed nothing.

## Decision

**Where the WASI spec is silent, backends copy wasmtime's observed behavior** (currently 47), measured on both CI hosts: host-uniform values are implemented deterministically, host splits wasmtime inherits (unlink of a directory: EPERM/EISDIR) are reproduced per host, and one-host internal artifacts (its Linux-only nofollow-stat slash strip) stay unpinned, noted at the test site.
wasmtime is the pick because the upstream testsuite encodes it, not because its behavior is better; where it is an outlier the value is arbitrary and not worth debating.

The wasi-testsuite runner injects the host-matched strict errno mode (`ERRNO_MODE_MACOS`/`ERRNO_MODE_UNIX`) into the Rust suite's guest environment, a deliberate deviation from the manifest-only-env rule (commit 02c5ef5), scoped to Rust because the C/assemblyscript suites assert exact environ contents.

## Rejected alternatives

- **POSIX semantics** (this PR's first revision): nothing tests against POSIX; fails the strict suite on at least one host.
- **Permissive errno modes**: hides real divergence.
- **Majority vote across runtimes**: effort spent on shapes that don't matter, with no canonical electorate.

## Consequences

Some units carry host-OS branches to reproduce wasmtime's splits; its quirks are copied as-is.
A future wasmtime change to a spec-silent shape forces a re-measure.
The PR #41 bash pins are re-based on this rule (`crates/dewasm-backend-bash/tests/wasi_fs_regressions.rs`).
