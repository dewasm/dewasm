# Decision 80: `fd_fdstat_set_flags` Accepts Any Open Fd, a Recorded Exception to Decision 49

Status: **Accepted, 2026-08-15.**
Recorded with the toywasm app case (issue #101, the PR adding `examples/apps/scripts/toywasm.sh`).
The units already behaved this way.
What landed is the justification for keeping the shape.
The toywasm work revealed that this shape diverges from wasmtime.

## Context

Decision 49 pins spec-silent WASI shapes to wasmtime's observed behavior.
wasmtime's preview1 layer routes `fd_fdstat_set_flags` through its filesystem interface.
So it accepts the call on regular files only and answers EBADF on stdio.
The successor interface specifies the function for the non-blocking flag on filesystem handles.
So upstream regards the restrictive shape as intended (bytecodealliance/wasmtime#6713).

The pinned toywasm app is a WASI implementation itself.
Its instance setup sets NONBLOCK on every non-tty host fd, stdio included.
The setup is `wasi_instance_add_hostfd` in `libwasi/wasi.c`, and it treats failure as fatal.
The pinned binary therefore does not run under wasmtime at all.
toywasm upstream never claims it does.
Its wasm32-wasi CI runs the wasm build on toywasm itself.
Its own host side (`libwasi/wasi_abi_fd.c`) accepts NONBLOCK on any user fd and records the bit.
That is exactly the shape dewasm's units already had.

dewasm's units accept the call on any open non-directory fd and store the fdflags word.
The units are `runtime/<lang>/units/wasi/fd_fdstat_set_flags.*`.
`fd_write` consults APPEND.
NONBLOCK is stored with no further effect, because every host IO path in the runtimes is blocking.
The WASI conformance suite does not assert wasmtime's restrictive shape.
The suite passes on both CI hosts with the permissive one.

## Decision

**Decision 49 yields when all three hold:**

- **copying wasmtime would make an in-scope pinned app unrunnable;**
- **the permissive shape has a reference implementation on the calling side;**
- **the conformance suite does not assert wasmtime's shape.**

Decision 49 itself states wasmtime is the pick because the upstream testsuite encodes it.
It is not the pick because its behavior is better.
The tiebreaker inverts when both of these hold:

- the testsuite does not encode the shape;
- an app does depend on the alternative.

Each such exception is recorded as a decision; the rule of decision 49 is otherwise unchanged.

Concretely, `fd_fdstat_set_flags` does four things:

- it accepts any open non-directory fd;
- it stores the fdflags word;
- it honors APPEND through `fd_write`;
- it records NONBLOCK without changing the blocking IO model.

## Rejected alternatives

- **Copy wasmtime (EBADF on non-regular fds).**
  Makes the pinned toywasm binary unrunnable on every backend.
  The restrictive shape is an artifact of wasmtime routing preview1 through its filesystem interface.
  The calling side's own host implementation contradicts it.
- **Accept the call on stdio only.**
  A third shape with no reference implementation anywhere.
  toywasm's host accepts any user fd, and narrowing it buys nothing.
- **Drop the toywasm app instead.**
  Gives up the only wasm-interpreter app (issue #101) over a shape decision 49 itself calls arbitrary.

## Consequences

- The converted toywasm runs on every backend, and the units state the constraint in place.
- wasmtime cannot provide ground truth for an app that calls the function on stdio.
  So such a case needs a different oracle.
  The toywasm case pins the existing `cowsay_args` snapshot.
  `crates/dewasm-test-helper/tests/apps_wasmtime.rs` records its exclusion from the wasmtime app suite.
- If wasmtime later relaxes the shape, the divergence disappears.
  This exception can then be retired.
  A future wasmtime change in the other direction changes nothing here.
