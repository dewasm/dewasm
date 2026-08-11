# Bash backend

`--target bash`. C and Rust tools converted to a script that runs where the only dependency is a shell.

## Output shape

A single sourceable `.sh` script whose every function, the module's own and the bundled runtime's, carries that artifact's prefix (`<p>invoke`, `<p>rt_trap`, `<p>mem_i32_load`), so several converted scripts sourced into one shell share no name at all. What they do share is the calling protocol: results come back in `R0, R1, ...`, a trap sets `TRAP_MSG` and `proc_exit` sets `EXIT_CODE`. Imports resolve from the caller's `IMPORTS` associative array (`[module.name]=function`). Control flow maps onto `while :; do ...; break; done` wrappers with `break N` / `continue N`.

In **library** mode the prefix is `--module-name` (required in library mode) **lowercased** plus `_`: `Sqlite3Shell` gives `sqlite3shell_`, `add` gives `add_`. The name must be one identifier matching `[A-Za-z_][A-Za-z0-9_]*`; anything else is a conversion-time error. The lowercasing is the one mapping the policy keeps: bash has no case-carrying namespace, and the mapping is total and stated rather than guessed. In **standalone** mode the prefix is always `program_` and `--module-name` is rejected.

## Requirements

`bash` **5 or newer** (associative arrays and namerefs; macOS's system `/bin/bash` is 3.2 and does **not** qualify, so `brew install bash`). No other external commands: floats run on a pure-Bash IEEE-754 softfloat, so there is no dependency on `bc`, `awk`, `python`, etc.

```console
$ dewasm prog.wasm --target bash --mode standalone -o prog.sh
$ bash prog.sh arg1 arg2      # a bash 5+ on PATH
```

Standalone programs follow the shared runtime interface (argv, `--dir` preopens, env, exit/trap): [docs/standalone-interface.md](../standalone-interface.md).

## Capabilities

Full wasm core 1.0 plus the universal baseline, with f32/f64 on the pure-Bash softfloat, and **full WASI preview 1 including the filesystem**; `fd_filestat_set_times` and `path_filestat_set_times` are the exception and return ENOSYS. Non-function imports, multiple tables, and table bulk ops are supported. Authoritative matrix: [docs/support.md](../support.md).

`proc_exit` propagates as status 133; traps set `TRAP_MSG` and propagate status 134 through `|| return $?` chains.

## Providers and library usage

The import table is the `IMPORTS` associative array keyed `module.name`; set an entry to a shell function name to override an import. A whole import module is served by pointing `PROVIDERS[module]` at another prefix `<q>` owning the per-kind export maps `<q>EXPORTS` / `<q>GLOBAL_EXPORTS` / `<q>TABLE_EXPORTS` / `<q>MEMORY_EXPORTS`, the shell counterpart of Ruby's provider object and what a wholesale WASI replacement uses. Unset entries fall back to the bundled WASI, and `<p>init` builds the bundled WASI's prefix-scoped state (`<p>wargs`, `<p>wfds`, …) only if at least one import actually fell back, so covering every WASI import leaves none of it behind. The e2e override, custom-provider and partial-override glue (`crates/dewasm-backend-bash/tests/e2e.rs`) are the worked references.

## Caveats

- **Speed.** Bash arithmetic is signed-64 only, and every float operation is softfloat integer arithmetic, so float-heavy programs are slow. The slow apps (QuickJS, SQLite) are `#[ignore]`d for Bash by default and run only under the `slow_test` cargo feature (or `cargo test -- --include-ignored`). Slower still, in the `ultra_slow_test` category (kept out of CI): the interactive qjs REPL pty case, the interpreter and reactor giants (CPython, CRuby, zeroperl), and the DOOM and NES frame snapshots. Integer-only programs (cowsay, minigzip) run fine.
- Every generated function ends with an explicit `return 0`; a trailing arithmetic statement would otherwise leak status 1 (the units lint enforces this).
- Floats are their bit patterns (u32 / signed-64), not shell floats; reads of the output should expect softfloat, not native, arithmetic.
