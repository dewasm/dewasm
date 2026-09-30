# Decision 49: Where WASI Is Silent, Follow Wasmtime; Host-Matched `errno` Modes for `wasi-testsuite`

Status: **Accepted, 2026-07-29.**
Implemented in PR #43 (issue #42).
Decision 80 records the one exception to date: `fd_fdstat_set_flags` accepts any open file descriptor.
There, copying Wasmtime would make the `toywasm` app unrunnable.

## Context

WASI p1 (and p2's `path-resolution.md`) says nothing about trailing-slash paths.
The shapes do not matter in practice.
A real program breaking on one of these `errno` values is unlikely.
Still, each backend needs *some* answer.
The first fix for issue #42 chose POSIX, which contradicts Wasmtime on a few shapes.
The upstream `wasi-testsuite`'s `assert_errno!` runs Permissive by default.
Permissive is the union of its arms for each OS.
That is how the differences went unnoticed.
Setting one `ERRNO_MODE_*` is its intended strict usage.

Survey (macOS; full table in PR #43):

- Wasmer 7 can't run the mutation probes: its VFS denies them.
- WasmEdge 0.17 and Node 24 agree with Wasmtime on the ENOTDIR resolution family.
  They do not agree on the special cases.
- `rmdir("dir/")` EINVAL and the EINVAL of macOS `O_CREAT` through a slash are Wasmtime alone.

Noted for the record; it changed nothing.

## Decision

**Where the WASI specification is silent, backends copy Wasmtime's observed behavior** (currently 47).
It is measured on both CI hosts:

- Host-uniform values are implemented deterministically.
- Host splits Wasmtime inherits are reproduced per host.
  An example is unlink of a directory: EPERM/EISDIR.
- One-host internal artifacts are not asserted, and are noted at the test site.
  An example is Wasmtime's Linux-only slash strip in a NOFOLLOW `stat`.

Wasmtime is the pick because the upstream testsuite encodes it, not because its behavior is better.
Where it differs from the others, the value is arbitrary and not worth debating.

The `wasi-testsuite` runner injects the host-matched strict `errno` mode.
The mode is `ERRNO_MODE_MACOS` or `ERRNO_MODE_UNIX`, set in the Rust suite's guest environment.
This is an intended deviation from the rule that the environment comes only from the manifest.
That rule came in commit 02c5ef5.
It is scoped to Rust because the C and AssemblyScript suites assert exact `environ` contents.

## Rejected alternatives

- **POSIX semantics** (this PR's first revision): nothing tests against POSIX.
  It fails the strict suite on at least one host.
- **Permissive `errno` modes**: hides a real difference.
- **Majority vote across runtimes**: effort spent on shapes that don't matter.
  It also has no agreed set of voters.

## Consequences

Some units carry host OS branches to reproduce Wasmtime's splits.
Its special cases are copied as-is.
A future Wasmtime change to a shape the specification leaves open forces a re-measure.
The PR #41 Bash regression tests are re-based on this rule.
They are in `crates/dewasm-backend-bash/tests/wasi_fs_regressions.rs`.
