# Decision 80: `fd_fdstat_set_flags` Accepts Any Open Descriptor, a Recorded Exception to Decision 49

Status: **Accepted, 2026-08-15.**
Recorded with the `toywasm` app case (issue #101, the PR adding `examples/apps/scripts/toywasm.sh`).
The units already behaved this way.
What landed is the justification for keeping the shape.
The `toywasm` work revealed that this shape differs from Wasmtime.

## Context

Decision 49 copies Wasmtime's observed behavior where the WASI specification is silent.
Wasmtime's preview1 layer routes `fd_fdstat_set_flags` through its file system interface.
So it accepts the call on regular files only and answers EBADF on standard input, output and error.
The next interface specifies the function for the non-blocking flag on file system handles.
So upstream regards the strict shape as intended (`bytecodealliance/wasmtime#6713`).

The `toywasm` app at its recorded version is a WASI implementation itself.
Its instance set-up step sets NONBLOCK on every host file descriptor that is not a TTY.
That includes standard input, output and error.
The set-up step is `wasi_instance_add_hostfd` in `libwasi/wasi.c`.
A failure there stops the instance from starting.
The binary at that version therefore does not run under Wasmtime at all.
`toywasm` upstream never claims it does.
Its wasm32-wasi CI runs the wasm build on `toywasm` itself.
Its own host side (`libwasi/wasi_abi_fd.c`) accepts NONBLOCK on any user file descriptor.
It records the bit.
That is exactly the shape dewasm's units already had.

dewasm's units accept the call on any open file descriptor that is not a directory.
They store the `fdflags` word.
The units are `runtime/<lang>/units/wasi/fd_fdstat_set_flags.*`.
`fd_write` consults APPEND.
NONBLOCK is stored with no further effect, because every host IO path in the runtimes is blocking.
The WASI conformance suite does not assert Wasmtime's strict shape.
The suite passes on both CI hosts with the looser one.

## Decision

**Decision 49 yields when all three hold:**

- **copying Wasmtime would make an in-scope app at its recorded version unrunnable;**
- **the looser shape has a reference implementation on the calling side;**
- **the conformance suite does not assert Wasmtime's shape.**

Decision 49 itself states Wasmtime is the pick because the upstream testsuite encodes it.
It is not the pick because its behavior is better.
The choice inverts when both of these hold:

- the testsuite does not encode the shape;
- an app does depend on the alternative.

Each such exception is recorded as a decision; the rule of decision 49 is otherwise unchanged.

Concretely, `fd_fdstat_set_flags` does four things:

- it accepts any open file descriptor that is not a directory;
- it stores the `fdflags` word;
- it honors APPEND through `fd_write`;
- it records NONBLOCK without changing the blocking IO model.

## Rejected alternatives

- **Copy Wasmtime (EBADF on any file descriptor that is not a regular file).**
  Makes the recorded `toywasm` binary unrunnable on every backend.
  The strict shape is an artifact of Wasmtime routing preview1 through its file system interface.
  The calling side's own host implementation contradicts it.
- **Accept the call on standard input, output and error only.**
  A third shape with no reference implementation anywhere.
  The host of `toywasm` accepts any user file descriptor, and narrowing it buys nothing.
- **Drop the `toywasm` app instead.**
  Gives up the only wasm-interpreter app (issue #101) over a shape decision 49 itself calls arbitrary.

## Consequences

- The converted `toywasm` runs on every backend, and the units state the constraint in place.
- Wasmtime cannot provide ground truth for an app that calls the function on standard input.
  The same holds for standard output and error.
  So such a case needs a different oracle.
  The `toywasm` case checks against the existing `cowsay_args` snapshot.
  `crates/dewasm-test-helper/tests/apps_wasmtime.rs` records its exclusion from the Wasmtime app suite.
- If Wasmtime later relaxes the shape, the difference disappears.
  This exception can then be retired.
  A future Wasmtime change in the other direction changes nothing here.
