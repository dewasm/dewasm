# Decision 20: Component Model (Canonical-ABI Adapters Synthesized as Core IR, Fixed Host Vocabulary)

Status: **Superseded by [decision 24](24-01-scope-reset.md), 2026-07-26.**
Kept as a design record for a future restoration of this support.
Git history plus this decision make the work cheap to revive.
The original acceptance note and implementation pointers below are retained as history.

Originally accepted 2026-07-24.
Implemented for the Ruby backend in:

- `crates/dewasm-core/src/{component,canon}.rs`;
- `crates/dewasm-backend-ruby/src/lib.rs` (`generate_component`);
- CLI auto-detection;
- the component e2e fixtures (`examples/wat/component_*.wat`).

Remaining: a real `wasm32-wasip2`-binary e2e, and the Bash side, which stays rejected.
How to fetch or build that binary is an open decision 15 question.

## Context

WASI preview 2 binaries are components (layer-1 wrappers).
A component holds N core modules, an instantiation graph, and `canon lift`/`canon lower` adapters.
The adapters translate between WIT values and core values via linear memory.
dewasmify targets many languages (decision 0).
So the load-bearing question was where the canonical ABI lives.
It could be implemented once per backend (jco-style host glue), or once centrally.
A D0 probe of a real Rust `wasm32-wasip2` binary fixed the required shape:

- 17 versioned `wasi:*` instance imports;
- a shim module whose funcref `$imports` table is fixed up post-instantiation;
- 26 lowers/1 lift;
- decisively, a trivial *nested component* wrapping the lifted `run` into an instance export;
- core-level import names whose versions (`@0.2.0`) differ from the component-level ones (`@0.2.9`).

## Decision

- **The canonical ABI is compiled away in `dewasm-core`, not interpreted per backend.**
  `component.rs` parses the binary into core modules (via the ordinary `build_module`).
  It also yields typed interface imports, an ordered instantiation plan, and lift/lower definitions.
  `canon.rs` synthesizes each adapter as a function of a **regular `ir::Module`**.
  That function's body does all layout walking with ordinary loads/stores/calls.
  **Criterion: a backend must be able to support components without knowing the canonical ABI exists.**
  A backend's entire component cost is:
  - (a) the host-boundary vocabulary below;
  - (b) per-language host units (decision 21);
  - (c) a wrapper emitter executing the plan.
- **The host boundary is a fixed IR vocabulary.**
  It is `ValType::Host` (opaque host value) plus ~25 `Expr::Host*`/2 `Stmt::Host*` ops.
  The ops cover:
  - string/bytes lift-store;
  - list new/get/len/push;
  - record/tuple construction and field access;
  - variant `[case, payload]` pairs;
  - enum symbols;
  - bool/char/option bridges;
  - sign/mask integer bridges.

  Host *calls* need no new op: host functions are imported funcs with Host-typed signatures.
  So `Stmt::Call` carries them.
  The decision 7 provider protocol resolves them (`host.<interface>#<func>`).
- **Adapter modules use conventional import names.**
  They are `canon.memory`, `canon.realloc`, `host.<iface>#<func>`, and `core.<instance>:<export>`.
  Index conventions apply too.
  The j-th `Lower` in plan order is the lowers module's j-th defined func.
  `lifted[i]` is the lifts module's i-th.
  The generated wrapper class executes the plan in the component's own section order.
  That order is what makes the shim/fixups cycle-free.
  The class constructs the Lowers adapter instance with placeholder canon imports.
  It rebinds memory/realloc the moment their providing instance exists.
  Scalar-only lowers, the only ones callable earlier, never touch memory.
- **The accepted subset is what `wit-component` command components need.**
  Everything else is refused at conversion time with `UnsupportedError(ComponentModel)` (decision 0).
  The subset is:
  - instance-kind imports;
  - utf8 strings;
  - the wrapper-shaped nested component;
  - `resource.drop` (handles are plain i32 end-to-end; drops become host calls).

  Flat-position variants *with payloads* are refused.
  They do not occur in CLI-world signatures; memory-position variants are fully general.
  The Ruby declaration is deliberately **`Partial`, never `Supported`**.
  The spec harness's component-model-tagged directive skips must stay legitimate.
  Decision 8's anti-regression turns a `Supported` flip into hard failures.
  Those failures would come from the unexecuted `.wast` component directives.
- **Wiring follows instantiation arguments, never name matching.**
  The version-mismatch finding makes name matching wrong by construction.

## Rejected alternatives

- **Per-backend canonical-ABI runtime glue (jco / wit-bindgen-host style).**
  Each backend reimplements lift/lower.
  That is exactly the N-times cost decision 0 exists to avoid.
  It is also unverifiable except end-to-end per language.
  Rejected by the user at planning time.
- **A host-side canonical-ABI interpreter driven by type descriptors.**
  It is one runtime implementation per language again.
  It is just data-driven.
  It has the same multi-backend bill, plus interpretation overhead on every boundary crossing.
- **External preprocessing (`wasm-tools` demote/compose).**
  It breaks the single-tool conversion contract (decision 0).
  It also still leaves the host boundary unsolved.
- **Rejecting nested components outright**: would reject every Rust wasip2 binary.
  Instead the one observed wrapper shape is modelled, and all else is refused.
  That shape is function imports re-exported, possibly as an instance.

## Consequences

- Positive: a real Rust `wasm32-wasip2` binary converts to ~1 MB of Ruby.
  The binary is 103 KB, with 3 core modules and 26 lowers.
  It runs byte-identically to wasmtime (stdout, stdin, env, preopened file I/O, exit semantics).
  The Ruby backend has zero canonical-ABI knowledge.
  The committed `.wat` component fixtures give interpreter-level e2e without binary artifacts.
- Positive: a future backend (decision 10's C#/Java) gets components by implementing three things.
  They are the Host vocabulary (bounded, enumerable), host units, and a wrapper emitter.
  `ValType::Host` maps to `Object`.
  No vocabulary op mixes host and core types in one slot, so static typing stays clean.
- Negative / carry-over: Bash rejection of Host constructs relies on routing components only to Ruby.
  That is a CLI-level check, not `check_module_support`.
  It is acceptable while the component path is single-backend, to revisit when a second backend lands.
  Four things remain out of scope.
  They are flat variants with payloads, non-utf8 encodings, `wasi:sockets/http`, and WASI 0.3 async.
  The `run` result is `result<(),()>`: nonzero guest exit codes collapse to 1.
  That is a WASI 0.2 limitation, not ours.

See also:

- [decision 7](7-import-providers.md): the provider protocol the wiring reuses;
- [decision 16](16-ruby-wasm1-completion.md): imported memories/tables the adapters lean on;
- [decision 21](21-ruby-wasi-preview2.md): the host side.
