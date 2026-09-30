# Decision 31: Standalone Runtime Interface (`argv`, `--dir`, `env`, `exit`)

**Status:** Accepted, 2026-07-27.
The runtime conventions of a `--mode standalone` program are now one interface.
They cover how it receives `argv` and directory preopens.
They also cover how `proc_exit`/trap map to a process exit.
The interface is uniform across all five backends and modelled on Wasmtime's CLI.
Landed:

- repeatable `--dir HOST::GUEST` flags parsed by the generated main;
- `DEWASM_PREOPEN` removed;
- `argv[0]`, the environment, and the trap exit code unified;
- Bash rejects `--dir` loudly.

The reference is [`docs/standalone-interface.md`](../../docs/standalone-interface.md).

**Revision, 2026-07-27:** Bash's `--dir` rejection is replaced by [decision 34](34-bash-wasi-file-system.md).
The Bash backend now honors `--dir` with real file system support.

## Context

A converted `--mode standalone` program is a real CLI: a WASI guest wrapped in a generated `main`.
That `main` supplies `argv`, the environment, and file system preopens.
It also translates the guest's exit/trap into a process exit code.
That wrapper is cross-backend product surface.
It grew backend-by-backend, though, and drifted apart:

- **Preopens** arrived through a `DEWASM_PREOPEN` environment variable (`"guest=host,..."`).
  Each generated main parsed it (~9 sites across the five backends, decision 14/29/30).
  An environment variable is hidden from `ps` and hard to use for several mounts.
  It is also unlike every wasm runtime's CLI.
- **`argv[0]`** was the program's base name in Ruby/Python/Bash.
  It was the *full* `os.Args[0]` path in Go, and the module class name in Java.
- **The environment** passed through in Ruby/Python/Go/Bash, but Java handed the guest an empty one.
- The **trap** exit code (134) and the `proc_exit`/`_start`-return mapping were already uniform.

Wasmtime is the ground-truth engine the app snapshots are captured from (decision 9/15).
It takes `wasmtime run [--dir HOST::GUEST]... <wasm> [args...]`.
It gives the guest `argv[0] = basename(wasm)`.
Matching it makes the generated programs behave like the binary they were converted from.

## Decision

One documented interface, implemented in every backend's generated standalone main.
Full reference: [`docs/standalone-interface.md`](../../docs/standalone-interface.md).

- **Command line:** `<runner> <program> [--dir HOST::GUEST]... [--] [guest args...]`.
  The generated main consumes a leading run of `--dir` flags (both `--dir X` and `--dir=X`).
  It stops at `--` or the first token that is not `--dir`.
  Everything after is the guest's `argv[1..]`.
  `HOST::GUEST` mirrors Wasmtime; a value without `::` mounts host==guest.
  This is a *shim in the generated program*, not a host runtime flag.
  The criterion that shapes it: match Wasmtime's user-facing CLI.
  That holds even though the layer that consumes it differs.
- **`argv[0]`** is the program name: the base name of the program file that was run.
  Examples are `hello.rb`, `hello`, and `hello.sh`.
  That matches `basename(wasm)` under Wasmtime.
  Java is the one deviation, documented, not hidden.
  The JVM does not pass the launched file name to `main`, so it uses the module class name.
- **Environment:** the whole process environment passes through to the guest.
  Java was fixed to `System.getenv()` from an empty array.
- **exit:** `proc_exit(N)` → process exit `N`; `_start` returning → `0`.
  A trap → `trap: <message>` on standard error + exit **134**.
  Bash reaches the same surface via its status chain protocol (decision 11/12).
  In it, 133 = `proc_exit` and 134 = trap.
- **Bash + `--dir`:** the Bash backend has no file system support (decision 12).
  So a leading `--dir` fails loudly with a clear message and a nonzero exit.
  It is not silently ignored.
  That follows decision 0: unsupported features fail, never do nothing.

## Rejected alternatives

- **Keep `DEWASM_PREOPEN` (environment variable for preopens).**
  Hidden from `ps`, hard to use for multiple mounts, and unlike every wasm runtime's CLI.
  Replaced by `--dir`.
- **A separate host launcher/wrapper script per backend.**
  More moving parts and another artifact to ship.
  The parsing is a few lines inline in the main.
- **Ignore `--dir` under Bash silently.**
  Breaks decision 0's fail-at-the-boundary rule.
  A user mounting a directory must be told Bash cannot honor it.
- **`argv[0]` = the full command path.**
  This was Go's prior behavior.
  Wasmtime uses the base name, and the base name is the stable, backend-independent choice.

## Consequences

- The generated mains gained a tiny argument parser.
  `DEWASM_PREOPEN` is gone from all code and current documents (historical decisions keep it).
- One shared end-to-end case exercises the interface on the four file system backends.
  It is `wasi_standalone_dir`, a `path_open` round-trip run with `--dir`.
  It is re-run under Wasmtime as ground truth (decision 27).
  A related case asserts Bash's `--dir` rejection.
- Behavior changes for callers:
  - Go's `argv[0]` is now a base name;
  - Java now receives the real environment;
  - preopens are CLI flags, not an environment variable.
