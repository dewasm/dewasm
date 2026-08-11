# Decision 12 — Bash WASI Conventions

Status: **Accepted, 2026-07-23.**
Implemented in `runtime/bash/units/wasi/` (the same 16-syscall surface as Ruby, plus the `write_string_list` helper) and the standalone emitter in `crates/dewasm-backend-bash/src/lib.rs`.
Filesystem syscalls remain ENOSYS, as on Ruby.
The filesystem scope excluded here is superseded by [decision 34](34-bash-wasi-filesystem.md).

## Context

WASI needs three things bash does not natively offer: a non-local exit (`proc_exit`), binary-safe stdio, and an entropy/clock source — all under decision 5's dependency criterion (exactly a Bash interpreter, no external commands) and decision 11's status-cascade protocol.
WASI units also need the per-module state prefix, but imports are bound as bare command names.

## Decision

- **`proc_exit` is a second status cascade**: `rt_exit` sets `EXIT_CODE` and returns 133 (`runtime/bash/units/rt/exit.sh`), the sibling of `rt_trap`'s 134.
  The standalone main maps 133 to `exit $(( EXIT_CODE & 0xff ))`; a sourced library surfaces it as `invoke`'s return status.
  The reusable rule: any non-local wasm exit is a reserved status code riding the existing `|| return $?` chains, never a bash `exit`.
- **Binary-safe stdio is byte-wise through builtins.**
  Writes collect memory bytes into an every-byte `'\\x%02x'` printf format (NUL/%/`\` safe; the format must stay single-quoted).
  Reads use `IFS= LC_ALL=C read -r -d '' -n 1`, where `''` with success is a NUL byte and failure is EOF.
  `random_get` reads `/dev/urandom` the same way — a device file via the `read` builtin is within decision 5's criterion.
- **Clocks come from `$EPOCHREALTIME`** (microsecond granularity, so `clock_res_get` reports 1000 ns); clock ids 1–3 fall back to realtime because pure bash has no monotonic source — an accepted, documented deviation.
- **Imports bind through per-module wrapper functions**: `<p>imp_wasi_<name>() { <p>wasi_<name> <p> "$@"; }` bakes the state prefix into the bare command name the import table expects.
  (Revision, [decision 62](62-embedded-runtime-isolation.md): the wrapper was `<p>wasi_<name>` calling the flat `wasi_<name>`; once each artifact's runtime carries its own prefix, that name *is* the unit's, and a wrapper of the same name would call itself.) Resolution order is decision 7's: `IMPORTS` entry → bundled unit wrapper → `<p>rt_enosys`.
  State is per-prefix (`<p>wargs`, `<p>wenv`, `<p>wfds`, `<p>wtell`); callers set the `WASI_ARGS`/`WASI_ENV` arrays before `<p>init` (the standalone main fills them from `$0`/`$@` and `compgen -e`).
- **The fd model is stdio-only**: fds 0/1/2 preopened, `fd_seek` answers ESPIPE, `fd_tell` reports the byte counters `fd_read`/`fd_write` track, `fd_prestat_get` answers EBADF to stop libc preopen scans.

## Rejected alternatives

- **`exit` inside `proc_exit`** — kills the caller's shell when the module is sourced as a library, and cannot be intercepted by the spec harness or an embedder.
- **`$SRANDOM` / `$RANDOM` for random_get** — SRANDOM needs bash 5.1 (the floor is 5.0); RANDOM is 15-bit and unseedable-weak.
- **`od`/`dd`/`head` for binary I/O** — external commands, rejected by decision 5's criterion.
- **A global current-instance variable instead of prefix wrappers** — breaks the moment two instances interleave calls; the wrapper costs one function definition per bundled syscall.

## Consequences

- Positive: `hello.wat` runs standalone under bash with the same stdout and exit code as Ruby; the decision 7 override/fallback semantics carry over (`crates/dewasm-backend-bash/tests/e2e.rs`).
- Negative: byte-wise stdio is slow for large payloads; batching rides on decision 11's bulk-memory scaling work when real apps (post-softfloat, decision 5) demand it.
- Time from ids 1–3 can go backwards with the realtime fallback; programs timing themselves may misbehave.
- `wasi_unstable` (snapshot 0) shares the implemented ABI except fd_seek's whence encoding, which is moot while fd_seek is ESPIPE-only.
