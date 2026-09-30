# Bash backend

`--target bash`.
C and Rust tools converted to a script that runs where the only dependency is a shell.

## Output shape

A single sourceable `.sh` script.
Every function, the module's own and the bundled runtime's, carries that artifact's prefix.
Examples are `<p>invoke`, `<p>rt_trap`, and `<p>mem_i32_load`.
So several converted scripts sourced into one shell share no name at all.
What they do share is the calling protocol.
Results come back in `R0, R1, ...`.
A trap sets `TRAP_MSG`, and `proc_exit` sets `EXIT_CODE`.
Imports resolve from the caller's `IMPORTS` associative array (`[module.name]=function`).
Control flow maps onto `while :; do ...; break; done` wrappers with `break N` / `continue N`.

In **library** mode `--module-name` is required, and the prefix is that name **lowercased** plus `_`.
`Sqlite3Shell` gives `sqlite3shell_`, and `add` gives `add_`.
The name must be one identifier matching `[A-Za-z_][A-Za-z0-9_]*`.
Anything else is a conversion-time error.
The lowercasing is the one mapping the policy keeps.
Bash has no case-carrying namespace, and the mapping is total and stated rather than guessed.
In **standalone** mode the prefix is always `program_` and `--module-name` is rejected.

## Requirements

`bash` **5 or newer** (associative arrays and namerefs).

> [!IMPORTANT]
> macOS ships bash 3.2 as `/bin/bash`, which cannot run the generated script.
> Install bash 5 or newer (`brew install bash`) and invoke that one.

No other external commands are required.
Floats run on a pure-Bash IEEE-754 softfloat, so there is no dependency on `bc`, `awk`, `python`, etc.

```console
$ dewasm prog.wasm --target bash --mode standalone -o prog.sh
$ bash prog.sh arg1 arg2      # a bash 5+ on PATH
```

Standalone programs follow the shared runtime interface: argv, `--dir` preopens, env, exit/trap.
It is described in [docs/standalone-interface.md](../standalone-interface.md).

## Capabilities

Full wasm core 1.0 plus the universal baseline, with f32/f64 on the pure-Bash softfloat.
**Full WASI preview 1 including the filesystem**, with one exception.
`fd_filestat_set_times` and `path_filestat_set_times` return ENOSYS.
Non-function imports, multiple tables, and table bulk ops are supported.
The exception-handling proposal is not lowered here, since Bash has no exception mechanism.
A module using it is rejected at conversion time with an attributed error.
Authoritative matrix: [docs/support.md](../support.md).

`proc_exit` propagates as status 133.
Traps set `TRAP_MSG` and propagate status 134 through `|| return $?` chains.

## Providers and library usage

The import table is the `IMPORTS` associative array keyed `module.name`.
Set an entry to a shell function name to override an import.
A whole import module is served by pointing `PROVIDERS[module]` at another prefix `<q>`.
That prefix owns the per-kind export maps:

- `<q>EXPORTS`
- `<q>GLOBAL_EXPORTS`
- `<q>TABLE_EXPORTS`
- `<q>MEMORY_EXPORTS`

`PROVIDERS` is the shell counterpart of Ruby's provider object.
A wholesale WASI replacement uses it.
Unset entries fall back to the bundled WASI.
`<p>init` builds the bundled WASI's prefix-scoped state only if at least one import fell back.
That state is `<p>wargs`, `<p>wfds`, and so on.
So covering every WASI import leaves none of it behind.
The worked references are the e2e override, custom-provider and partial-override glue.
They are in `crates/dewasm-backend-bash/tests/e2e.rs`.

## Caveats

- **Speed.**
  Bash arithmetic is signed-64 only.
  Every float operation is softfloat integer arithmetic, so float-heavy programs are slow.
  The slow apps (QuickJS, SQLite) are `#[ignore]`d for Bash by default.
  They run only under the `slow_test` cargo feature.
  Integer-only programs (cowsay, minigzip) run fine.
  Slower cases still are in the `ultra_slow_test` category, which is kept out of CI:
  - the interactive qjs REPL pty case;
  - the interpreter and reactor giants (CPython, CRuby, zeroperl);
  - the toywasm and wasm3 cases, where a converted wasm interpreter runs a second wasm binary;
  - the DOOM and NES frame snapshots.
- Every generated function ends with an explicit `return 0`.
  Otherwise a trailing arithmetic statement would leak status 1.
  The units lint enforces this.
- Floats are their bit patterns (u32 / signed-64), not shell floats.
  Reads of the output should expect softfloat, not native, arithmetic.
