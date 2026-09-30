# Decision 28: Python Backend Lowering Conventions

Status: **Accepted, 2026-07-26.**
First milestone ("cowsay runs", decision 24) implemented.
It lives in `crates/dewasm-backend-python/src/lib.rs` + `runtime/python/`.

**Second milestone (the spec harness passes) landed 2026-07-26.**
It is the wasm-1.0 completion plus the shared spec harness.
The harness is `crates/dewasm-backend-python/tests/spec.rs`.
The completion follows decision 16's model: boxed globals, imported globals/memories/tables.
It also covers multiple tables and the table half of bulk memory.
Numeric conventions are decision 2's.
This decision covers the places Python forced a different shape from Ruby (decision 4).
Those are control flow, float division, and (below) the harness's recursion/exhaustion handling.

**Third milestone (full WASI preview 1, incl. the filesystem) landed 2026-07-26.**
`runtime/python/units/wasi/` mirrors the Ruby WASI unit set one-for-one.
It adopts decision 14's filesystem model wholesale:
- the `preopens=` provider kwarg;
- the single fd-table with a `WasiDir` entry kind;
- the realpath-plus-prefix-containment sandboxing (with the same accepted TOCTOU/symlink caveat);
- the ENOSYS gaps:
  - `fd_fdstat_set_flags`/`fd_fdstat_set_rights`;
  - the symlink and `path_filestat_set_times` syscalls.

The Python fd model diverges only in mechanics forced by the host stdlib, not in policy:
- Files are unbuffered `os.fdopen(..., buffering=0)` handles.
  So `os.pread`/`os.pwrite` stay coherent with `read`/`write`/`seek`, which sqlite mixes on one fd.
- Directory listing uses `os.listdir`.
- The errno map keys on `OSError.errno` (the `errno` module) rather than exception classes.

With this, `has_wasi_p1` reports the same surface as Ruby.
The shared WASI `Fs` suite runs under Python.
So do the gzip byte-stdio and heavy filesystem app cases (QuickJS, SQLite, ripgrep).

**Revision, 2026-07-29 (issue #31).**
The recursion/exhaustion mitigation below is no longer harness-only.
The emitted standalone entrypoint itself applies the same raised recursion limit.
It also applies the same big-stack guest thread.
It carries the guest's exit/trap back to the main thread, so decision 31's exit codes are unchanged.

**Revision, 2026-08-05 ([decision 62](62-embedded-runtime-isolation.md)).**
The top-level runtime below keeps its placement but not its fixed name.
Under `Embedded` linkage it is `<Class>Rt`, so two artifacts coexist in one namespace.

**Revision, 2026-08-02:** the harness's two constants were retuned.
Measurement showed that `assert_exhaustion` cost was linear in the recursion limit.
`skip-stack-guard-page.wast` alone was 80% of the 257-file run.
So `check_exhaust` now runs at its own low limit, and the harness thread stack shrank to fit it.
The standalone entrypoint keeps the generous pairing.
It does so since a real guest's recursion depth is not known in advance.

## Context

Python is dynamically typed with arbitrary-precision ints and IEEE doubles.
So decision 2's conventions transfer from Ruby almost verbatim.
Those are masked-unsigned integers and double-backed f32-with-re-rounding.
The shared parts are `Rt.s32`/`s64`, `Rt.f32`, and the software NaN bit paths.
Three language facts did *not* transfer:

1. **Python has no non-local control transfer.**
   Ruby's whole control-flow story is `catch`/`throw` (decision 4).
   Python has neither that nor `goto` nor labeled `break`/`continue`.
2. **Python caps statically-nested loops/`try` at 20** ("too many statically nested blocks").
   By contrast, `if` nests ~100 deep.
   Real wasm binaries nest far deeper.
   cowsay's hottest function nests referenced blocks/loops/ifs 42 deep (measured).
   Blocks (forward branches) dominate that.
3. **Python raises on `x / 0.0`** and on `math.sqrt` of a negative.
   `struct.pack("<f")` also raises `OverflowError` past the f32 range.
   In all three cases Ruby returns `inf`/`nan`.
   Integer `//`/`%` also floor rather than truncate.

## Decision

- **Forward branches (block/if exits) use a per-function branch register `_br`, not a loop or `try`.**
  A `br` to a block/if sets `_br = <label id>`.
  Each statement after a possible branch is guarded by `if _br == 0:`.
  A referenced label emits an `if _br == <id>: _br = 0` reset marker at its end.
  Because only `while`/`try` count toward the 20-block cap, this keeps blocks free of it.
- **Block/if bodies are spliced inline into the enclosing statement list**.
  So block *nesting* adds zero Python nesting.
  The guards are siblings, so sequence *length* adds none either.
  cowsay's 42-deep wasm nesting lowers to a max Python indent of 9.
  Guards are emitted only after a statement whose subtree can leave `_br` set (`stmt_free_targets`).
  So straight-line code is unguarded.
- **Only real loops become `while True:`** with a trailer `if _br == <id>: _br = 0; continue` / `break`.
  A back-edge `br` sets `_br`, and the trailer turns it into `continue`.
  Loop nesting is small (5 in cowsay) and is the *only* contributor to the 20-block budget.
  A guarded `if` folds its guard into the condition (`if _br == 0 and (cond) != 0:`).
  So a trapping `cond` is not evaluated while a branch is pending.
- **`Rt.fdiv` wraps float division** (returns IEEE `inf`/`nan` instead of raising).
  `Rt.f32` catches `OverflowError`.
  `fsqrt`/`div_s`/`rem_s` guard the negative/zero/truncation cases exactly as the Ruby units do.
  Integer `div_s`/`rem_s` use `abs`-based truncation, never `//`/`%`.
- **Runtime lives at module top level, not nested in the generated class.**
  Python method scopes cannot see an enclosing class scope.
  So a nested `class Rt` would make `Rt.trap` unresolvable inside a method.
  The runtime is emitted as a top-level class with `class Memory`/`Table`/`WASI` nested inside it.
  It is named `<Class>Rt` under `Embedded` linkage and `Rt` under `Alias` (decision 62).
  There is one self-contained module per file.
  Module = one class; imports are resolved in `__init__` (`self.ifN`).
  Own globals are plain `self.gN` attributes.
  Exports sit in a `self.exports` dict, with `invoke(name, *args)` as the entry point.
- **`call_indirect` compares structural type strings** (`"i32,i64->i32"`), like Ruby's symbols.
  The reason is the same (shared tables, decision 4).
- **Every global is a boxed `Rt.Global`** (`value` attribute).
  It is not a plain `self.gN` attribute holding the value.
  This reverses the first milestone's choice now that imported globals are supported (decision 16).
  A global crossing an instantiation boundary must be a shared mutable cell.
  `Memory`/`Table` are already objects for the same reason.
  So one representation keeps `GlobalGet`/`GlobalSet` a single rule.
  `global_get` reads `.value`; `global_export`/`wasm_import` hand out the box itself.
- **The spec harness runs the whole assertion body inside `def _main()` on a large-stack thread.**
  Python's default 1000 recursion limit is far below what call/fac-style deep guest recursion needs.
  But simply raising `sys.setrecursionlimit` on the main thread risks a C-stack overflow.
  That is a segfault, not a catchable error, and it comes before the Python limit trips.
  So the harness sets a generous recursion limit *and* launches `_main` on a big-stack thread.
  That is the guest-side analogue of the build's `convert_on_big_stack`.
  A runaway recursion then surfaces as a `RecursionError` well inside that stack.
  `check_exhaust` maps it to wasm's `call stack exhausted`.
  This mirrors Ruby's `SystemStackError` and Bash's `FUNCNEST` subshell.
  The limit is pure headroom for the checks that recurse legitimately.
  But for `check_exhaust` it *is* the cost (the descent always runs to the limit).
  So `check_exhaust` lowers it around itself.
  The constants and their measured margins are documented at the harness's `_EXHAUST_RECURSION_LIMIT`.
  The class definitions live inside `_main` as local classes.
  Their methods still resolve the module-level `Rt`.
  Because of that, the generated `Rt = Rt` alias line is dropped for the harness.
  It would rebind `Rt` as a `_main` local.

## Rejected alternatives

- **Exceptions for `br` (a `_Br` exception per label, or one per function).**
  A `try` per block hits the 20-block cap exactly as loops would.
  A single per-function `try` cannot express "resume after *this* block" without a dispatch loop.
  The branch register is flat and cap-free.
- **Mirror Ruby's `catch`/`throw` shape with single-iteration `while` loops for blocks.**
  Correct, but every block then costs a loop.
  So cowsay's 38-deep block+loop nesting blows the 20-loop cap immediately.
- **Keep own globals as plain `self.gN` attributes (the first-milestone choice).**
  Adopted while imported globals were rejected at conversion time.
  Reversed in the second milestone (see the boxing decision above).
  It was reversed because imported/exported-shared globals reintroduce a cross-boundary-sharing need.
  That need is exactly what made plain attributes insufficient.
  Two representations would also fork every global read/write/export site.

## Consequences

- Positive: cowsay is byte-identical to the wasmtime snapshot for both the args and stdin cases.
  qjs and sqlite3 convert and compile.
  The control-flow scheme is depth-insensitive, so no relooper/label-dispatch was needed.
- Negative: guards add an `if _br == 0:` and a comparison per branchy statement.
  That is more lines and a small per-statement cost versus Ruby's `catch`/`throw`.
  `_br` is a whole-function register, so it serializes control flow textually rather than structurally.
- Carry-over: the first milestone bundled only the eight WASI syscalls cowsay needs.
  The second added the spec harness.
  The third (above) fills in the full WASI preview 1 surface and the filesystem.
  So gzip, QuickJS, SQLite, and ripgrep now run under Python.
  What remains ENOSYS matches Ruby (decision 14).
  That is rights narrowing, symlink creation/read, and `fd_renumber`/`fd_advise`/ `fd_allocate`.
  It is also `path_filestat_set_times` and `poll_oneoff`.
