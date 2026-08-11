# ADR-28 — Python Backend Lowering Conventions

Status: **Accepted, 2026-07-26.**
First milestone ("cowsay runs", ADR-24) implemented in `crates/dewasm-backend-python/src/lib.rs` + `runtime/python/`.
**Second milestone (the spec harness passes) landed 2026-07-26**: the wasm-1.0 completion (ADR-16's model — boxed globals, imported globals/memories/tables, multiple tables, the table half of bulk memory) plus the shared spec harness (`crates/dewasm-backend-python/tests/spec.rs`).
Numeric conventions are ADR-2's; this ADR covers the places Python forced a different shape from Ruby (ADR-4): control flow, float division, and (below) the harness's recursion/exhaustion handling.
**Third milestone (full WASI preview 1, incl. the filesystem) landed 2026-07-26**: `runtime/python/units/wasi/` mirrors the Ruby WASI unit set one-for-one, adopting ADR-14's filesystem model wholesale — the `preopens=` provider kwarg, the single fd-table with a `WasiDir` entry kind, the realpath-plus-prefix-containment sandboxing (with the same accepted TOCTOU/symlink caveat), and the ENOSYS gaps (`fd_fdstat_set_flags`/`fd_fdstat_set_rights`, symlink and `path_filestat_set_times` syscalls).
The Python fd model diverges only in mechanics forced by the host stdlib, not in policy: files are unbuffered `os.fdopen(..., buffering=0)` handles (so `os.pread`/`os.pwrite` stay coherent with `read`/`write`/`seek`, which sqlite mixes on one fd), directory listing uses `os.listdir`, and the errno map keys on `OSError.errno` (the `errno` module) rather than exception classes.
With this, `has_wasi_p1` reports the same surface as Ruby, and the shared WASI `Fs` suite plus the gzip byte-stdio and heavy filesystem app cases (QuickJS, SQLite, ripgrep) run under Python.
**Revision, 2026-07-29 (issue #31):** the recursion/exhaustion mitigation below is no longer harness-only — the emitted standalone entrypoint applies the same raised recursion limit and big-stack guest thread itself, carrying the guest's exit/trap back to the main thread so ADR-31's exit codes are unchanged.
**Revision, 2026-08-05 ([ADR-62](62-embedded-runtime-isolation.md)):** the top-level runtime below keeps its placement but not its fixed name — under `Embedded` linkage it is `<Class>Rt`, so two artifacts coexist in one namespace.
**Revision, 2026-08-02:** the harness's two constants were retuned after measuring that `assert_exhaustion` cost was linear in the recursion limit — `skip-stack-guard-page.wast` alone was 80% of the 257-file run — so `check_exhaust` now runs at its own low limit and the harness thread stack shrank to fit it (the standalone entrypoint keeps the generous pairing, since a real guest's recursion depth is not known in advance).

## Context

Python is dynamically typed with arbitrary-precision ints and IEEE doubles, so ADR-2's masked-unsigned integers and double-backed f32-with-re-rounding transfer from Ruby almost verbatim (`Rt.s32`/`s64`, `Rt.f32`, the software NaN bit paths).
Three language facts did *not* transfer:

1. **Python has no non-local control transfer.**
   Ruby's whole control-flow story is `catch`/`throw` (ADR-4); Python has neither that nor `goto` nor labeled `break`/`continue`.
2. **Python caps statically-nested loops/`try` at 20** ("too many statically nested blocks"), while `if` nests ~100 deep.
   Real wasm binaries nest far deeper: cowsay's hottest function nests referenced blocks/loops/ifs 42 deep (measured), and blocks (forward branches) dominate that.
3. **Python raises on `x / 0.0`**, on `math.sqrt` of a negative, and `struct.pack("<f")` raises `OverflowError` past the f32 range — where Ruby returns `inf`/`nan`.
   Integer `//`/`%` also floor rather than truncate.

## Decision

- **Forward branches (block/if exits) use a per-function branch register `_br`, not a loop or `try`.**
  A `br` to a block/if sets `_br = <label id>`; each statement after a possible branch is guarded by `if _br == 0:`; a referenced label emits an `if _br == <id>: _br = 0` reset marker at its end.
  Because only `while`/`try` count toward the 20-block cap, this keeps blocks free of it.
- **Block/if bodies are spliced inline into the enclosing statement list**, so block *nesting* adds zero Python nesting; the guards are siblings, so sequence *length* adds none either.
  cowsay's 42-deep wasm nesting lowers to a max Python indent of 9.
  Guards are emitted only after a statement whose subtree can leave `_br` set (`stmt_free_targets`), so straight-line code is unguarded.
- **Only real loops become `while True:`** with a trailer `if _br == <id>: _br = 0; continue` / `break`; a back-edge `br` sets `_br` and the trailer turns it into `continue`.
  Loop nesting is small (5 in cowsay) and is the *only* contributor to the 20-block budget.
  A guarded `if` folds its guard into the condition (`if _br == 0 and (cond) != 0:`) so a trapping `cond` is not evaluated while a branch is pending.
- **`Rt.fdiv` wraps float division** (returns IEEE `inf`/`nan` instead of raising); `Rt.f32` catches `OverflowError`; `fsqrt`/`div_s`/`rem_s` guard the negative/zero/truncation cases exactly as the Ruby units do (integer `div_s`/`rem_s` use `abs`-based truncation, never `//`/`%`).
- **Runtime lives at module top level, not nested in the generated class.**
  Python method scopes cannot see an enclosing class scope, so a nested `class Rt` would make `Rt.trap` unresolvable inside a method; the runtime is emitted as a top-level class with `class Memory`/`Table`/`WASI` nested inside it — named `<Class>Rt` under `Embedded` linkage and `Rt` under `Alias` (ADR-62) — one self-contained module per file.
  Module = one class; imports resolved in `__init__` (`self.ifN`), own globals as plain `self.gN` attributes, exports in a `self.exports` dict with `invoke(name, *args)` as the entry point.
- **`call_indirect` compares structural type strings** (`"i32,i64->i32"`), like Ruby's symbols and for the same reason (shared tables, ADR-4).
- **Every global is a boxed `Rt.Global`** (`value` attribute), not a plain `self.gN` attribute holding the value — reversing the first milestone's choice now that imported globals are supported (ADR-16).
  A global crossing an instantiation boundary must be a shared mutable cell, and `Memory`/`Table` are already objects for the same reason, so one representation keeps `GlobalGet`/`GlobalSet` a single rule.
  `global_get` reads `.value`; `global_export`/`wasm_import` hand out the box itself.
- **The spec harness runs the whole assertion body inside `def _main()` on a large-stack thread.**
  Python's default 1000 recursion limit is far below what call/fac-style deep guest recursion needs, but simply raising `sys.setrecursionlimit` on the main thread risks a C-stack overflow (a segfault, not a catchable error) before the Python limit trips.
  So the harness sets a generous recursion limit *and* launches `_main` on a big-stack thread — the guest-side analogue of the build's `convert_on_big_stack`.
  A runaway recursion then surfaces as a `RecursionError` well inside that stack, which `check_exhaust` maps to wasm's `call stack exhausted` (mirroring Ruby's `SystemStackError` and Bash's `FUNCNEST` subshell).
  The limit is pure headroom for the checks that recurse legitimately, but for `check_exhaust` it *is* the cost — the descent always runs to the limit — so `check_exhaust` lowers it around itself; the constants and their measured margins are documented at the harness's `_EXHAUST_RECURSION_LIMIT`.
  Because the class definitions live inside `_main` as local classes whose methods still resolve the module-level `Rt`, the generated `Rt = Rt` alias line is dropped for the harness (it would rebind `Rt` as a `_main` local).

## Rejected alternatives

- **Exceptions for `br` (a `_Br` exception per label, or one per function).**
  A `try` per block hits the 20-block cap exactly as loops would; a single per-function `try` cannot express "resume after *this* block" without a dispatch loop.
  The branch register is flat and cap-free.
- **Mirror Ruby's `catch`/`throw` shape with single-iteration `while` loops for blocks.**
  Correct, but every block then costs a loop, so cowsay's 38-deep block+loop nesting blows the 20-loop cap immediately.
- **Keep own globals as plain `self.gN` attributes (the first-milestone choice).**
  Adopted while imported globals were rejected at conversion time; reversed in the second milestone (see the boxing decision above), because supporting imported/exported-shared globals reintroduces exactly the cross-boundary-sharing need that made plain attributes insufficient, and two representations would fork every global read/write/export site.

## Consequences

- Positive: cowsay is byte-identical to the wasmtime snapshot for both the args and stdin cases; qjs and sqlite3 convert and compile.
  The control-flow scheme is depth-insensitive, so no relooper/label-dispatch was needed.
- Negative: guards add an `if _br == 0:` and a comparison per branchy statement — more lines and a small per-statement cost versus Ruby's `catch`/`throw`.
  `_br` is a whole-function register, so it serializes control flow textually rather than structurally.
- Carry-over: the first milestone bundled only the eight WASI syscalls cowsay needs; the second added the spec harness; the third (above) fills in the full WASI preview 1 surface and the filesystem, so gzip, QuickJS, SQLite, and ripgrep now run under Python.
  What remains ENOSYS matches Ruby (ADR-14): rights narrowing, symlink creation/read, `fd_renumber`/`fd_advise`/ `fd_allocate`, `path_filestat_set_times`, and `poll_oneoff`.
