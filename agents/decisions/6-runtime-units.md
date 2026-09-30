# Decision 6: Runtime as Per-Method Units with Selectable Linkage

Status: **Accepted, 2026-07-23.**
Implemented for Ruby: the runtime lives in per-method units (118 at the time).
The generic bundler is in `crates/dewasm-backend/src/lib.rs` (`RuntimeBundler`).
Generated code references the runtime via the relative name `Rt`.
The units originally lived under `runtime/<lang>/units/`.
Decision 85 moved them to `crates/dewasm-backend-<lang>/units/` for crates.io packaging.
The mechanism is unchanged.
The external/gem linkage is designed for but not shipped.

## Context

The runtime was two monolithic files (`runtime.rb`, `wasi.rb`).
They were embedded wholesale into every generated program.
Three pressures broke that:

- The Bash backend's softfloat (decision 5) will be ~1000 lines a float-free program must not carry.
  Shells parse the whole file at startup.
- WASI keeps growing, but a module's imports name exactly which syscalls it can ever call.
- Two generated files loaded into one Ruby process both reopened the global `Dewasmify` module.
  That collided constants.
  Worse, it silently mixed runtimes from different dewasmify versions.

## Decision

Two orthogonal mechanisms:

- **Per-method runtime units, bundled on demand.**
  One file per runtime method under `runtime/<lang>/units/<scope>/<name>`.
  Dependencies are declared in `# requires:` header lines.
  Inseparable class skeletons are `_class`/`_module` prelude units.
  Code generation records every helper it references; the build bundles only that closure.
  Criterion: *the generated artifact carries only code the module can reach.*
- **Runtime linkage behind one name.**
  Generated code and units refer to the runtime only as `Rt`.
  `RuntimeLinkage` decides where `Rt` lives:

  - `Embedded` nests `module Rt` inside the generated class.
    The file is self-contained, and `A::Rt` and `B::Rt` are fully independent.
    So naive multi-require is safe.
  - `Alias(path)` emits one `Rt = <path>` line.
    The line serves a shared bundle (the spec harness).
    Later, it will serve a `dewasm-runtime` gem dependency for programs using many modules.

  Criterion: *the runtime's location must be a one-line concern of the generated code.*
  Ruby's lexical constant resolution makes the same unit source work in every placement.

The declared-dependency drift risk (edit the code, forget the header) is mitigated twice:

- A lint test extracts `Rt.x` / `Rt::X` / `@memory.x` / bare sibling-call references from unit bodies.
  It checks them against the header.
  The test is a `#[cfg(test)] mod units` at the bottom of `crates/dewasm-backend-ruby/src/lib.rs`.
- The spec harness runs its 19k assertions against minimal bundles.
  So an undeclared dependency fails as a NoMethodError at the exact assertion.

## Rejected alternatives

- **Keep the monolith**: unacceptable output cost once Bash softfloat exists.
  It also gives no answer to multi-artifact collisions.
- **Coarse feature groups** (memory / floats / wasi).
  They cost most of the machinery for a fraction of the precision.
  Any float instruction would still drag the whole float group into Bash output.
- **A fixed global namespace for the embedded runtime**: multi-module programs collide.
  Version skew between artifacts goes undetected.
- **Runtime `require` of shared files at run time.**
  It breaks the single-file, no-dependency output contract (decision 0).
  Dependency-based distribution is instead the future gem linkage, chosen explicitly at build time.

## Consequences

- Positive: float-free WASI programs shrank (hello: 430 → 160 lines).
  Generated files coexist in one process.
  WASI syscalls are bundled by import name.
  So unimplemented ones cost a stub lambda, not code.
  The bundler and the unit convention are language-agnostic and ready for `runtime/bash/units/`.
- Negative: 118 small files instead of two readable ones.
  The `requires:` headers are also hand-maintained.
  The residual risk is a parenless bare call the lint cannot see; the spec harness catches those.
- Carry-over: gem packaging is a future decision once a second consumer of the shared linkage exists.
  Gem packaging means `Alias` pointing at an installed runtime, plus version compatibility checking.

## Relationship to other decisions

- Decision 4's lowering conventions now emit `Rt.*` instead of `Dewasmify.*`.
- Decision 3's harness doubles as the dynamic dependency check here.
- Decision 5's softfloat will land directly as `runtime/bash/units/`.
