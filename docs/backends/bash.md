# Bash backend

`--target bash`. The flagship "runs where the only dependency is a shell" target: C/Rust tools converted to a script with no external commands.

## Output shape

A single sourceable `.sh` script: the runtime is a set of flat `rt_*`/`mem_*` functions and the module's functions are prefixed per generation so multiple modules can coexist in one shell. Imports resolve from the caller's `IMPORTS` associative array (`[module.name]=function`). Control flow maps onto `while :; do ...; break; done` wrappers with `break N` / `continue N`. See [ADR-11](../adr/11-bash-backend-lowering.md) for lowering, [ADR-12](../adr/12-bash-wasi.md) for WASI conventions, and [ADR-13](../adr/13-bash-softfloat-conventions.md) for the softfloat.

## Requirements

`bash` **5 or newer** (associative arrays and namerefs; macOS's system `/bin/bash` is 3.2 and does **not** qualify — `brew install bash`). No other external commands: floats run on a pure-Bash IEEE-754 softfloat, so there is no dependency on `bc`, `awk`, `python`, etc.

```console
$ dewasm prog.wasm --target bash --mode standalone -o prog.sh
$ bash prog.sh arg1 arg2      # a bash 5+ on PATH
```

Standalone programs follow the shared runtime interface (argv, env, exit/trap): [docs/standalone-interface.md](../standalone-interface.md). Bash has no filesystem support, so `--dir` is rejected with a clear error rather than silently ignored.

## Capabilities

Wasm core 1.0 plus the universal baseline, with f32/f64 on the pure-Bash softfloat. Deliberately the narrowest backend:

- **No WASI filesystem** — WASI core only (stdio, args, environ, clocks, random, proc_exit, byte-wise binary stdio). See [docs/support.md](../support.md).
- **Non-function imports, multiple tables, and table bulk ops are rejected** at conversion time with a clear error (a plain associative-array import table has no object model for them).

`proc_exit` propagates as status 133; traps set `TRAP_MSG` and propagate status 134 through `|| return $?` chains.

## Providers and library usage

The import table is the `IMPORTS` associative array keyed `module.name`; there is no duck-typed provider *object* as on Ruby/Python. Set an entry to a shell function name to override an import; unset entries fall back to the bundled WASI. The e2e override glue (`crates/dewasm-backend-bash/tests/e2e.rs`) is the worked reference.

## Caveats

- **Speed.** Bash arithmetic is signed-64 only, and every float operation is softfloat integer arithmetic, so float-heavy programs are slow — the slow apps (QuickJS, SQLite) are `#[ignore]`d for Bash by default and run only under the `slow_test` cargo feature (or `cargo test -- --include-ignored`); the interactive qjs REPL pty case is slower still and sits at the `ultra_slow_test` tier ([ADR-48](../adr/48-slow-test-tiers.md), kept out of CI). Integer-only programs (cowsay, minigzip) run fine.
- Every generated function ends with an explicit `return 0`; a trailing arithmetic statement would otherwise leak status 1 (the units lint enforces this — [ADR-11](../adr/11-bash-backend-lowering.md)).
- Floats are their bit patterns (u32 / signed-64), not shell floats; reads of the output should expect softfloat, not native, arithmetic.
