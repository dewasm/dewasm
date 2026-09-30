# Decision 6: Runtime as Per-Method Units with Selectable Linkage

Status: **Accepted, 2026-07-23.**
Implemented for Ruby: the runtime lives in per-method units (118 at the time).
The language-independent bundler is in `crates/dewasm-backend/src/lib.rs` (`RuntimeBundler`).
Generated code references the runtime via the relative name `Rt`.
The units originally lived under `runtime/<lang>/units/`.
Decision 85 moved them to `crates/dewasm-backend-<lang>/units/` for crates.io packaging.
The mechanism is unchanged.
The external/gem linkage is designed for but not shipped.

## Context

The runtime was two files (`runtime.rb`, `wasi.rb`) holding everything.
They were embedded whole into every generated program.
Three pressures broke that:

- The Bash backend's softfloat (decision 5) will be ~1000 lines a float-free program must not carry.
  Shells parse the whole file at start.
- WASI keeps growing, but a module's imports name exactly which system calls it can ever call.
- Two generated files loaded into one Ruby process both reopened the global `Dewasmify` module.
  That defined the same constants twice.
  Worse, it silently mixed runtimes from different `dewasmify` versions.

## Decision

Two independent mechanisms:

- **Per-method runtime units, bundled on demand.**
  One file per runtime method under `runtime/<lang>/units/<scope>/<name>`.
  Dependencies are declared in `# requires:` header lines.
  The parts of a class or module that cannot be split off are `_class`/`_module` units, placed first.
  Code generation records every helper it references; the build bundles only that closure.
  Criterion: *the generated artifact carries only code the module can reach.*
- **Runtime linkage behind one name.**
  Generated code and units refer to the runtime only as `Rt`.
  `RuntimeLinkage` decides where `Rt` lives:

  - `Embedded` nests `module Rt` inside the generated class.
    The file is self-contained, and `A::Rt` and `B::Rt` are independent.
    So a plain `require` of many files is safe.
  - `Alias(path)` emits one `Rt = <path>` line.
    The line serves a shared bundle (the specification harness).
    Later, it will serve a `dewasm-runtime` gem dependency for programs using many modules.

  Criterion: *the runtime's location must be a one-line concern of the generated code.*
  Ruby's lexical constant resolution makes the same unit source work in every placement.

The declared-dependency drift risk (edit the code, forget the header) is reduced twice:

- A lint test extracts references from unit bodies.
  They are `Rt.x`, `Rt::X`, `@memory.x`, and calls without a receiver to other units of the same scope.
  It checks them against the header.
  The test is a `#[cfg(test)] mod units` at the bottom of `crates/dewasm-backend-ruby/src/lib.rs`.
- The specification harness runs its 19k assertions against minimal bundles.
  So an undeclared dependency fails as a NoMethodError at the exact assertion.

## Rejected alternatives

- **Keep the single large file**: unacceptable output cost once Bash softfloat exists.
  It also gives no answer to name conflicts between artifacts.
- **Large feature groups** (memory / floats / WASI).
  They cost most of the machinery for a fraction of the precision.
  Any float instruction would still drag the whole float group into Bash output.
- **A fixed global namespace for the embedded runtime**: multi-module programs have conflicting names.
  Version differences between artifacts go undetected.
- **Runtime `require` of shared files at run time.**
  It breaks the output contract of a single file with no dependency (decision 0).
  Distribution as a dependency is the future gem linkage instead, chosen explicitly at build time.

## Consequences

- Positive: float-free WASI programs got smaller (hello: 430 → 160 lines).
  Generated files can live together in one process.
  WASI system calls are bundled by import name.
  So unimplemented ones cost a stub lambda, not code.
  The bundler and the unit convention are language-independent and ready for `runtime/bash/units/`.
- Negative: 118 small files instead of two readable ones.
  The `requires:` headers are also hand-maintained.
  The residual risk is a call with neither parentheses nor a receiver, which the lint cannot see.
  The specification harness catches those.
- Carry-over: gem packaging is a future decision once a second consumer of the shared linkage exists.
  Gem packaging means `Alias` pointing at an installed runtime, plus version compatibility checking.

## Relationship to other decisions

- Decision 4's lowering conventions now emit `Rt.*` instead of `Dewasmify.*`.
- Decision 3's harness doubles as the dynamic dependency check here.
- Decision 5's softfloat will land directly as `runtime/bash/units/`.
