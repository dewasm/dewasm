# Decision 12: Bash WASI Conventions

Status: **Accepted, 2026-07-23.**
Implemented in `runtime/bash/units/wasi/`.
Also implemented in the standalone emitter in `crates/dewasm-backend-bash/src/lib.rs`.
The units have the same 16 system calls as Ruby, plus the `write_string_list` helper.
File system calls remain ENOSYS, as on Ruby.
The file system scope excluded here is replaced by [decision 34](34-bash-wasi-file-system.md).

## Context

WASI needs three things Bash does not natively offer:

- a non-local exit (`proc_exit`);
- binary-safe stdio;
- a source of random data and of time.

All three must meet decision 5's dependency criterion and decision 11's status chain protocol.
Decision 5's criterion means exactly a Bash interpreter, with no external commands.
WASI units also need the per-module state prefix.
But imports are bound as command names alone, with no arguments added.

## Decision

- **`proc_exit` is a second status chain**: `rt_exit` sets `EXIT_CODE` and returns 133.
  133 sits next to `rt_trap`'s 134.
  The unit is `runtime/bash/units/rt/exit.sh`.
  The standalone main maps 133 to `exit $(( EXIT_CODE & 0xff ))`.
  A sourced library surfaces it as `invoke`'s return status.
  The reusable rule: any non-local wasm exit is a reserved status code, never a Bash `exit`.
  The code passes up through the existing `|| return $?` chains.
- **Binary-safe stdio is byte-wise through builtins.**
  Writes collect memory bytes into an every-byte `'\\x%02x'` `printf` format.
  The format is NUL/%/`\` safe and must stay single-quoted.
  Reads use `IFS= LC_ALL=C read -r -d '' -n 1`.
  There, `''` with success is a NUL byte, and failure is EOF.
  `random_get` reads `/dev/urandom` the same way.
  A device file via the `read` builtin is within decision 5's criterion.
- **Clocks come from `$EPOCHREALTIME`** (microsecond steps, so `clock_res_get` reports 1000 ns).
  Clock identifiers 1-3 fall back to the real-time clock, because pure Bash has no monotonic source.
  This is an accepted, documented deviation.
- **Imports bind through per-module wrapper functions.**
  The wrapper form is `<p>imp_wasi_<name>() { <p>wasi_<name> <p> "$@"; }`.
  It builds the state prefix into a command name alone, the form the import table expects.
  (Revision, [decision 62](62-embedded-runtime-isolation.md): the wrapper was `<p>wasi_<name>` calling the flat `wasi_<name>`.
  Once each artifact's runtime carries its own prefix, that name *is* the unit's.
  A wrapper of the same name would call itself.)
  Resolution order is decision 7's: `IMPORTS` entry → bundled unit wrapper → `<p>rt_enosys`.
  State is per-prefix (`<p>wargs`, `<p>wenv`, `<p>wfds`, `<p>wtell`).
  Callers set the `WASI_ARGS`/`WASI_ENV` arrays before `<p>init`.
  The standalone main fills them from `$0`/`$@` and `compgen -e`.
- **The file descriptor model is stdio-only**:
  - file descriptors 0/1/2 preopened;
  - `fd_seek` answers ESPIPE;
  - `fd_tell` reports the byte counters `fd_read`/`fd_write` track;
  - `fd_prestat_get` answers EBADF to stop `libc` preopen scans.

## Rejected alternatives

- **`exit` inside `proc_exit`**: kills the caller's shell when the module is sourced as a library.
  It also cannot be caught by the specification harness or an embedder.
- **`$SRANDOM` / `$RANDOM` for `random_get`**: SRANDOM needs Bash 5.1 (the floor is 5.0).
  RANDOM is 15-bit and unseedable-weak.
- **`od`/`dd`/`head` for binary I/O**: external commands, rejected by decision 5's criterion.
- **A global current-instance variable instead of prefix wrappers.**
  It breaks the moment two instances interleave calls.
  The wrapper costs one function definition per bundled system call.

## Consequences

- Positive: `hello.wat` runs standalone under Bash with the same standard output and exit code as Ruby.
  The decision 7 override/fallback semantics carry over (`crates/dewasm-backend-bash/tests/e2e.rs`).
- Negative: byte-wise stdio is slow for large payloads.
  Batching depends on decision 11's bulk-memory scaling work.
  That work happens when real apps (after softfloat, decision 5) demand it.
- Time from clock identifiers 1-3 can go backwards with the real-time fallback.
  Programs timing themselves may misbehave.
- `wasi_unstable` (snapshot 0) shares the implemented ABI except `fd_seek`'s `whence` encoding.
  That difference does not matter while `fd_seek` only answers ESPIPE.
