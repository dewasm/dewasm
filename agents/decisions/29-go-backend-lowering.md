# Decision 29: Go Backend Lowering Conventions

Status: **Accepted, 2026-07-26.**
Implemented in `crates/dewasm-backend-go/src/lib.rs`, `runtime/go/units/`, and `crates/dewasm-backend-go/tests/{spec,e2e}.rs`: wasm 1.0 with the spec harness passing ([decision 3](3-testing-strategy.md), [decision 16](16-ruby-wasm1-completion.md)) and full WASI preview 1 including the filesystem, adopting [decision 14](14-ruby-wasi-filesystem.md)'s model.
Numeric conventions are [decision 2](2-numeric-semantics.md)'s; this decision covers where Go, statically typed with native fixed-width integers and floats, forced a different shape from the dynamically-typed backends (Ruby [decision 4](4-ruby-backend-lowering.md), Python [decision 28](28-python-backend-lowering.md)).
Go is the first *compiled* backend, so it is the first to use [decision 27](27-test-helper-crate.md)'s `run()` override.

## Context

Go's native numerics remove work the interpreted backends do by hand: integer arithmetic wraps, so decision 2's masked-unsigned convention is free and sign-extensions, wraps, and conversions are casts; floats are machine floats, so f32 re-rounds itself, division is trap-free, and only demote/promote reconstruct NaN payloads.
Go adds two problems they never face: **unused variables, labels, and imports are compile errors** (the Go-specific landmine, analogous to Python's 20-block cap), and function values are **statically typed**, so a dynamic `invoke` would need reflection and `call_indirect` needs one table holding heterogeneous signatures.

## Decision

- **Types.**
  i32/i64/f32/f64 to `uint32`/`uint64`/`float32`/`float64`, funcref to `*funcref`, every value-producing site fully typed.
  Float constants are emitted as `Rt.f32_from_bits`/`Rt.f64_from_bits`, keeping literal formatting and Go's constant-float rules out of the picture, and a stack temp is named by depth *and* type (`s3_i32`), since one depth holds different types at different points.
- **Control flow maps onto Go's labeled loops**, so there is no branch register (contrast Python's `_br`): a referenced block/if or loop is `L: for { … ; break L }` with back-edges as `continue L`, unreferenced structures splice inline, and `br_table` is a `switch` whose labeled breaks target the enclosing loop, not the switch.
  A pre-pass drops what cannot execute before emission (sequence tails after a statement that ends unreachable, the closing `break L`/default `return` after a terminated body, catch clauses after a tag-less one, self-assigning local moves), demoting labels no surviving branch targets, because `go vet` rejects unreachable statements in consumers of the generated source.
- **Unused-symbol discipline.**
  Labels are emitted only when referenced, and a pre-pass blanks write-only locals and temps with `_ = x`.
  The import set is computed by scanning the runtime bundle alone, since generated code emits no package-qualified selectors and data blobs are hex literals; the scan strips line comments so prose cannot pull in a package, and it never rewrites output, which is why `//go:embed` needs the blank import [decision 37](37-data-segment-externalization.md) adds.
- **Runtime shape.**
  Helpers are methods on a zero-size `rt` receiver named in **snake_case matching their unit id 1:1** (`Rt.i32_div_s`), a deliberate break with Go's PascalCase convention so a unit id maps to its reference without case conversion and the units lint stays a direct name match (correctness and tooling over idiom, [decision 1](1-ir-design.md)).
  All bundler scope wrappers are empty, Go methods and types being package-level whatever struct they belong to, so the bundle is a flat declaration list.
  Traps, exits, and link errors are `panic` of `rtTrap`/`rtExit`/`rtLinkError`, recovered at the standalone boundary (trap to stderr, exit 134).
- **Static typing is the import check.**
  Import fields are typed to the wasm signature and a table slot is a `*funcref{ ty string; fn any }`, so the type assertion at each site performs decision 16's kind check and additionally catches a wrong *type* for funcs and globals, rejecting a bad import as a `link_error`; a missing non-WASI import is a `link_error` at instantiation ([decision 0](0-foundation.md)), and a WASI import falls back to the bundled method or an ENOSYS stub ([decision 7](7-import-providers.md)).
  Globals are a generic `*global[T]`, the Go analogue of Ruby's `Rt::Global`, shared across an instantiation boundary while `p.g0.value` still needs no assertion; `Exports` is a `map[string]any` over every kind, so one instance's exports serve as another module's import object, which powers `register`.
- **An import source is a value, not only a map**: either a name-to-value `map[string]any` or an `ImportProvider` (`WasmImport(name string) any`) standing in for the module, optionally also an `ImportAttacher` (`Attach(instance any)`) called once the instance is built.
  That is the Go spelling of Ruby's duck-typed `attach`, and the only way a struct-shaped provider reaches the instance's memory without a back-reference wired by the embedder.
  The bundled WASI is built on first *fallback*, not in the constructor and not on first call, matching Ruby's `@wasi ||=`, so `p.wasi == nil` is the honest observable for a provider covering every WASI import.
- **Packaging follows the mode** ([decision 63](63-module-name-policy.md)): standalone is `package main` with the fixed type `Program`; library output is a package an embedder imports (`package <name lowercased>`, type `<name capitalized>`), validated as a Go identifier at conversion time.
  Host code in that package cannot carry its own `import`, Go requiring imports first, so library output imports `fmt` up front and keeps it live with `var _ = fmt.Sprint`.
- **Execution (`run()` override).**
  The helper `go build`s into a content-addressed cache binary and runs the binary rather than `go run`, which prints "exit status N" and exits 1 instead of propagating the guest exit code the WASI args/env case asserts.
  `$DEWASM_GO` overrides the toolchain; a missing one fails loud ([decision 15](15-tests-fail-not-skip.md)).
- **The spec harness compiles Go**: one `package main` program per `.wast` file, module declarations accumulated at package scope since they cannot sit inside `main`.
  Go has no dynamic dispatch, so each generated type carries a reflective `invoke(name, args...) []any` / `globalGet(name) []any` built where the export signatures are known.
- **Exhaustion is a generation-time recursion guard, in spec builds only**, because a runaway recursion overflows Go's goroutine stack *fatally* while the harness must continue past the check.
  Each function adds its frame's slot count to a global `rtStack`, `defer`-decrements it, and traps `"call stack exhausted"` past 1024 slots.
  Sizing by slot cost rather than depth also trips `skip-stack-guard-page.wast`'s 1056-local function at shallow depth, so that file needs no list entry.
  The guard is **off** in shipped output, whose deep but valid recursions must not falsely trap.
- **Two Go-compiler float hazards** the spec suite exposed: Go fuses `x*y+z` into an arm64 FMA, which `float_exprs.wast` forbids, and folds `x*1.0` / `x/1.0` to `x`, skipping the signaling-NaN quieting `no_fold_*` demands.
  Both are defeated by routing f32/f64 **mul and div through `//go:noinline` helpers**, so a constant operand never reaches the op and no add fuses across the call boundary.
  `math.NaN()` is also not bit-canonical, so min/max build wasm's pattern explicitly, and f32 `sqrt` through float64 was validated correctly rounded against `f32.wast`.
- **Feature scope**: wasm 1.0 and full WASI preview 1; `Floats` is `Supported`.

### WASI: where Go's standard library forced a different shape

Decision 14's fd-table model, preopen sandboxing, and deliberate ENOSYS gaps are mirrored into `runtime/go/units/wasi/` as the unit set Ruby and Python carry.
Go-specific:

- The fd table is `map[uint32]any` holding an `*os.File` or a `*wasiDir`, so every fd-taking syscall type-asserts; stdio special cases key on pointer identity with `os.Stdin`/`Stdout`/`Stderr`, and `fd_datasync` falls back to a full `Sync`, Go exposing no portable `fdatasync`.
- Preopens are the constructor's fourth parameter, Go having no keyword arguments, with fds assigned in sorted key order so random map iteration adds no nondeterminism.
- The runtime stays one build-tag-free `.go` file, which is why `stat` fields named differently on darwin and linux are reached without build tags ([decision 40](40-wasi-p1-completion.md)).
- `resolve_path` derives the final component from the raw guest string, not `filepath.Base(filepath.Join(base, rel))`: `Join` *Cleans*, folding a trailing `.` or `..` away, so `Base` returns the parent's own name and the AT_SYMLINK_NOFOLLOW branch would wrongly resolve and reject it with ERRNO_NOTCAPABLE.
  Taking the substring after the final `/` restores what Python's non-cleaning join gives for free.
- Library-mode WASI output always seeds `rt/exit`: host glue catches `*rtExit` for the exit code and Go asserts the concrete type at compile time, so it must exist even for a fixture that never imports `proc_exit`.

## Rejected alternatives

- **A per-function branch register** (Python's `_br`): unnecessary, since labeled `break`/`continue` express block exits and loop back-edges directly.
- **PascalCase runtime method names with a converting lint**: a bug surface for no gain, since `go build` ignores case.
- **Reflection-based `invoke`**: `map[string]any` exports plus a type assertion at the glue site avoid pulling in `reflect` and its cost.
- **Blanket `_ = x` for every local and temp**: correct, but it bloats cowsay's already large file and its compile time.
- **Lowering `debug.SetMaxStack`, or a subprocess per exhaustion assertion**: the former is still process-fatal, the latter costs a process per check.

## Consequences

- cowsay is byte-identical to the wasmtime snapshot, and the WASI `Fs` suite, `gzip_e2e!` (byte stdio through compiled output), and the filesystem app cases match the same snapshots the Ruby/Python cases use.
  Native integers and floats keep the generated arithmetic and the runtime smaller than the interpreted backends', at the cost of a verbose, fully cast source (decision 1) that always imports `fmt` in library mode.
- **Go's import-limits gap is narrower than Ruby's and Java's**, because the type assertion rejects the func-signature and global-value-type mismatches a kind-only check misses; only global mutability and table/memory limits stay unchecked.
  Its `EXPECTED_FAILURES` list is correspondingly shorter (`linking` 2 against their 4), and every failure on a full run is in the attributed `import-limits`/`linking` list.
- Cold `go build` of cowsay's roughly 170k-line file takes a few seconds, a warm cache hit well under 0.1 s; per-file `go build` dominates a full spec run, which the cache (shared with e2e) keeps under a minute when warm.
