# Decision 103: Function Names From the Name Section Are an Option, After the Index

Status: **Accepted, 2026-10-23.**
`--function-names` (`GenOptions::function_names`) names a generated method after the `name` section.
The index stays in front: `_f289_sqlite3VdbeExec`.
`dewasm_core::ir::Module::func_names` carries the section; the Ruby backend's `func_name` reads it.
Off, the output is unchanged.

## Context

A profile of the Rails example (decision 102) showed its time in `Sqlite3Ext#_f1286`.
The wasm module knew the function as `sqlite3VdbeExec`.
`wasm-ld` keeps the `name` section under `--strip-debug`, and only `--strip-all` removes it.
The backend ignored the section and named every function `_f{index}`.

## Decision

**A debug name joins the generated name only when asked for, and the index stays first.**

- Off unless asked for, because the names come from a build's debug information.
  Two builds of one program differ in them, and so would the converted output.
  The default output depends on the module's meaning alone.
- Index first, because the name section is not checked by validation.
  Two functions may share a name, and a name may hold characters Ruby cannot spell.
  `_f{index}_` keeps every method name unique and valid; the rest is reduced to `[A-Za-z0-9_]`.
- The core parses the section for every backend; today only the Ruby backend uses it.
  Another backend adopts it by routing its function names through one function, as Ruby does.

## Rejected alternatives

- **Always append the name.**
  Every snapshot and size record would change, and would change again with a toolchain update.
- **The name instead of the index.**
  Collisions and spelling make it a validation problem the section does not solve.
- **Parse the section in the backend.**
  Each backend would parse wasm; the core already reads every section once.

## Consequences

- Positive: a profile or a stack trace reads like the C source.
  The Rails example turns it on for every converted extension.
- Negative: a name can reach 64 characters, so the output grows a little.
- Carry-over: extracted loop bodies (decision 81) take the index alone.
  They could carry their parent's name.
