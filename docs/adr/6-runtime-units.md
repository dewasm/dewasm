# ADR-6 — Runtime as Per-Method Units with Selectable Linkage

Status: **Accepted, 2026-07-23.** Implemented for Ruby: the runtime lives in `runtime/ruby/units/` (118 units), the generic bundler in `crates/dewasm-backend/src/lib.rs` (`RuntimeBundler`), and generated code references the runtime via the relative name `Rt`. The external/gem linkage is designed for but not shipped.

## Context

The runtime was two monolithic files (`runtime.rb`, `wasi.rb`) embedded wholesale into every generated program. Three pressures broke that: the Bash backend's softfloat (ADR-5) will be ~1000 lines a float-free program must not carry (shells parse the whole file at startup); WASI keeps growing but a module's imports name exactly which syscalls it can ever call; and two generated files loaded into one Ruby process both reopened the global `Dewasmify` module — colliding constants and, worse, silently mixing runtimes from different dewasmify versions.

## Decision

Two orthogonal mechanisms:

- **Per-method runtime units, bundled on demand.** One file per runtime method under `runtime/<lang>/units/<scope>/<name>`, dependencies declared in `# requires:` header lines, inseparable class skeletons as `_class`/`_module` prelude units. Code generation records every helper it references; the build bundles only that closure. Criterion: *the generated artifact carries only code the module can reach.*
- **Runtime linkage behind one name.** Generated code and units refer to the runtime only as `Rt`; `RuntimeLinkage` decides where `Rt` lives: `Embedded` nests `module Rt` inside the generated class (self-contained file, `A::Rt` and `B::Rt` fully independent — naive multi-require is safe), `Alias(path)` emits one `Rt = <path>` line for a shared bundle (the spec harness) or, later, a `dewasm-runtime` gem dependency for programs using many modules. Criterion: *the runtime's location must be a one-line concern of the generated code.* Ruby's lexical constant resolution makes the same unit source work in every placement.

The declared-dependency drift risk (edit the code, forget the header) is mitigated twice: a lint test (a `#[cfg(test)] mod units` at the bottom of `crates/dewasm-backend-ruby/src/lib.rs`) extracts `Rt.x` / `Rt::X` / `@memory.x` / bare sibling-call references from unit bodies and checks them against the header, and the spec harness runs its 19k assertions against minimal bundles, so an undeclared dependency fails as a NoMethodError at the exact assertion.

## Rejected alternatives

- **Keep the monolith** — unacceptable output cost once Bash softfloat exists, and no answer to multi-artifact collisions.
- **Coarse feature groups** (memory / floats / wasi) — most of the machinery for a fraction of the precision; any float instruction would still drag the whole float group into Bash output.
- **A fixed global namespace for the embedded runtime** — multi-module programs collide; version skew between artifacts goes undetected.
- **Runtime `require` of shared files at run time** — breaks the single-file, no-dependency output contract (ADR-0); dependency-based distribution is instead the future gem linkage, chosen explicitly at build time.

## Consequences

- Positive: float-free WASI programs shrank (hello: 430 → 160 lines); generated files coexist in one process; WASI syscalls are bundled by import name, so unimplemented ones cost a stub lambda, not code; the bundler and the unit convention are language-agnostic and ready for `runtime/bash/units/`.
- Negative: 118 small files instead of two readable ones, and the `requires:` headers are hand-maintained (residual risk: a parenless bare call the lint cannot see — the harness net catches those).
- Carry-over: gem packaging (`Alias` pointing at an installed runtime, version compatibility checking) is a future ADR once a second consumer of the shared linkage exists.

## Relationship to other ADRs

- ADR-4's lowering conventions now emit `Rt.*` instead of `Dewasmify.*`.
- ADR-3's harness doubles as the dynamic dependency check here.
- ADR-5's softfloat will land directly as `runtime/bash/units/`.
