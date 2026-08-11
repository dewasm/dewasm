# Decision 31: Standalone Runtime Interface (argv, --dir, env, exit)

**Status:** Accepted — 2026-07-27.
The runtime conventions of a `--mode standalone` program — how it receives argv and directory preopens, and how `proc_exit`/trap map to a process exit — are now one interface, uniform across all five backends and modelled on wasmtime's CLI.
Landed: repeatable `--dir HOST::GUEST` flags parsed by the generated main; `DEWASM_PREOPEN` removed; argv[0], env, and the trap exit code unified; Bash rejects `--dir` loudly.
The reference is [docs/standalone-interface.md](../../docs/standalone-interface.md).

**Revision, 2026-07-27:** Bash's `--dir` rejection is superseded by [decision 34](34-bash-wasi-filesystem.md) — the Bash backend now honors `--dir` with real filesystem support.

## Context

A converted `--mode standalone` program is a real CLI: a WASI guest wrapped in a generated `main` that supplies argv, env, filesystem preopens, and translates the guest's exit/trap into a process exit code.
That wrapper is cross-backend product surface, but it grew backend-by-backend and diverged:

- **Preopens** arrived through a `DEWASM_PREOPEN` env var (`"guest=host,..."`) parsed in each generated main (~9 sites across the five backends, decision 14/29/30).
  An env var is invisible in `ps`, awkward for several mounts, and unlike every wasm runtime's CLI.
- **argv[0]** was the program basename in Ruby/Python/Bash, the *full* `os.Args[0]` path in Go, and the module class name in Java.
- **env** passed through in Ruby/Python/Go/Bash, but Java handed the guest an empty environment.
- The **trap** exit code (134) and the `proc_exit`/`_start`-return mapping were already uniform.

wasmtime — the ground-truth engine the app snapshots are captured from (decision 9/15) — takes `wasmtime run [--dir HOST::GUEST]... <wasm> [args...]` and gives the guest `argv[0] = basename(wasm)`.
Aligning to it makes the generated programs behave like the binary they were converted from.

## Decision

One documented interface, implemented in every backend's generated standalone main.
Full reference: [docs/standalone-interface.md](../../docs/standalone-interface.md).

- **Invocation:** `<runner> <program> [--dir HOST::GUEST]... [--] [guest args...]`.
  The generated main consumes a leading run of `--dir` flags (both `--dir X` and `--dir=X`), stopping at `--` or the first non-`--dir` token; everything after is the guest's `argv[1..]`.
  `HOST::GUEST` mirrors wasmtime; a value without `::` mounts host==guest.
  This is a *shim in the generated program*, not a host runtime flag — the criterion that shapes it: match wasmtime's user-facing CLI even though the layer that consumes it differs.
- **argv[0]** is the program name: the basename of the invoked program file (`hello.rb`, `hello`, `hello.sh`, ...), matching `basename(wasm)` under wasmtime.
  Java is the one deviation — the JVM does not pass the launched file name to `main`, so it uses the module class name — documented, not hidden.
- **env:** the whole process environment passes through to the guest (Java fixed to `System.getenv()` from an empty array).
- **exit:** `proc_exit(N)` → process exit `N`; `_start` returning → `0`; a trap → `trap: <message>` on stderr + exit **134**.
  Bash reaches the same surface via its status-cascade protocol (133 = proc_exit, 134 = trap; decision 11/12).
- **Bash + `--dir`:** the Bash backend has no filesystem support (decision 12), so a leading `--dir` fails loudly with a clear message and a nonzero exit rather than being silently ignored (decision 0: unsupported features fail, never no-op).

## Rejected alternatives

- **Keep `DEWASM_PREOPEN` (env var for preopens).**
  Invisible in `ps`, clumsy for multiple mounts, and diverges from every wasm runtime's CLI.
  Replaced by `--dir`.
- **A separate host launcher/wrapper script per backend.**
  More moving parts and another artifact to ship; the parsing is a handful of lines inline in the main.
- **Ignore `--dir` under Bash silently.**
  Violates decision 0's fail-at-the-boundary rule; a user mounting a directory must be told Bash cannot honor it.
- **argv[0] = the full invocation path.**
  Go's prior behavior; wasmtime uses the basename, and the basename is the stable, backend-independent choice.

## Consequences

- The generated mains gained a tiny arg-parser; `DEWASM_PREOPEN` is gone from all code and living docs (historical decisions keep their mentions).
- One shared end-to-end case (`wasi_standalone_dir`, a `path_open` round-trip run with `--dir`) exercises the interface on the four filesystem backends and is re-run under wasmtime as ground truth (decision 27); a companion case asserts Bash's `--dir` rejection.
- Behavior changes for callers: Go's argv[0] is now a basename; Java now receives the real environment; preopens are CLI flags, not an env var.
