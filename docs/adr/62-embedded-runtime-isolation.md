# ADR-62 — `Embedded` Output Isolates Its Runtime per Artifact

Status: **Accepted, 2026-08-05.**
All six backends conform (issue #141 complete).
Ruby (lexical nesting) and Perl ([ADR-55](55-perl-backend-lowering.md), textual package prefix) already did; Python conforms as of this decision — `RuntimeLinkage::Embedded` names its runtime class `<Class>Rt` (`crates/dewasm-backend-python/src/lib.rs`, `runtime_name`).
Java nests its runtime classes in the module class, Go gets isolation from the per-package library output ([#155](https://github.com/dewasm/dewasm/pull/155)), and Bash prefixes its runtime function names per artifact.
Every backend invokes `embedded_coexist_e2e!`, which is the check.

## Context

[ADR-6](6-runtime-units.md) put the runtime behind one name and made its location a linkage choice, explicitly rejecting *"a fixed global namespace for the embedded runtime — multi-module programs collide; version skew between artifacts goes undetected"*, and claiming coexistence as a consequence (*"generated files coexist in one process"*).
Ruby got that for free from constant nesting, which is what the decision was written against.

The backends since have flat runtimes, and each quietly reintroduced the rejected shape.
Python's `Embedded` output emits a top-level `class Rt:` — nesting it is impossible, because a Python method scope cannot see its enclosing class scope ([ADR-28](28-python-backend-lowering.md)).
Go and Java emit one top-level runtime; Bash has a single flat namespace of `rt_*`/`mem_*` functions ([ADR-11](11-bash-backend-lowering.md)).
Perl hit the same wall and solved it (ADR-55), citing ADR-6.

Two Python artifacts in one namespace therefore lose one runtime silently.
Trap identity collapses first: an embedder catching one artifact's trap catches the other's.
Worse, the bundle is a closure over the units each module reaches ([ADR-6](6-runtime-units.md)), so when the *first* artifact has the larger closure, the second `class Rt` deletes helpers the first still calls — an `AttributeError` at some later call site, nowhere near the cause.
Nothing warns at conversion time, which ADR-0 says is where such failures belong.

`EMBEDDED_COEXIST` (`crates/dewasm-test-helper/src/multimodule.rs`) has covered exactly this since ADR-27, and the four flat-runtime backends carried non-invocation REASON comments describing the collision as a property of the language.

## Decision

**`RuntimeLinkage::Embedded` output must be self-isolating.**
Two artifacts generated independently and placed in one namespace each reach their own runtime, with distinct trap, exit and link-error types.
The criterion: *an `Embedded` artifact names nothing a sibling artifact also names.*
`Alias` is the only way to share a runtime, and sharing is then a choice made at generation time rather than an accident of the target language's namespace.

The mechanism is per-backend — whatever isolation that language makes cheapest:

| Backend | Mechanism |
| --- | --- |
| Ruby | `module Rt` nested in the generated class; lexical constant lookup resolves it with no rewriting. |
| Perl | `Rt::` → `<Package>::Rt::` textual prefix at bundle time (ADR-55). |
| Python | the runtime class is `<Class>Rt`, and the bundle's `Rt.` references are rewritten once at bundle time. |
| Java | `static` nested classes under the generated class (landed). Java resolves simple names through enclosing class scopes, so nothing inside the artifact is rewritten; only outside references gain the `<Class>.` qualifier ([ADR-30](30-java-backend-lowering.md) revision). |
| Bash | `rt_`/`mem_`/`tab_`/`wasi_` function-name prefixing at bundle time (landed): the artifact's own prefix goes on every runtime name, `rt_trap` -> `<p>rt_trap`, and the generator emits its call sites already prefixed. `TRAP_MSG`/`EXIT_CODE`/`R0..` and `IMPORTS`/`PROVIDERS` stay global — they are the cross-module calling protocol two artifacts must share to link at all ([ADR-35](35-bash-cross-module-linking.md)). Only the names change: each `Embedded` artifact already carried its own copy of the runtime text, so nothing is duplicated that was not, and parse cost is unchanged. |
| Go | one package per artifact — library output declares `package <module name>` ([ADR-63](63-module-name-policy.md), #155), and a Go package *is* a namespace, so two artifacts share no identifier at all (landed). |

A backend is done when it invokes `embedded_coexist_e2e!`.
Until then its REASON comment is a to-do, not a capability declaration.

`Alias` output is untouched by all of this — the shared runtime keeps the plain name `Rt`, so the spec harness's generated text is byte-identical.

## Rejected alternatives

- **Nest the runtime for real on Python** (`class Rt` inside the generated class, Ruby's shape).
  Python method scopes cannot see the enclosing class scope, so every helper reference would have to spell the class (`Prog.Rt.trap`) — a global lookup plus two attribute lookups instead of one plus one, on every masked-integer op, every load and every store, in the slowest backend's hottest path.
  The rename costs nothing at run time: the generated code still resolves one module-level global by one name.
- **Accept flat runtimes as a permanent limitation** and keep the REASON comments.
  The failure is silent and is not only about trap identity — a smaller sibling bundle removes helpers the other artifact calls.
  ADR-6 rejected this shape before any of these backends existed; the deviation was drift, not a decision.
- **Split one artifact across files and lean on the target's module system** (Python `import`).
  Fixes Python only, contradicts the single-file self-contained output contract ([ADR-0](0-foundation.md)), and does nothing for Bash, Go or Java, where the namespace really is flat.
  Go's landed mechanism is not this: its `package` clause is a line *inside* the one generated file, so the artifact stays a single file and the isolation costs nothing.
- **Detect the collision at run time** (a guard that raises when a second runtime redefines the first).
  Converts a silent bug into a loud one but still refuses a program that has every right to run.

## Consequences

- Positive: ADR-6's coexistence promise holds where it is claimed.
  An embedder can hold two converted libraries — two versions of the same one included — in one namespace and catch each one's traps by name.
  Python's latent smaller-bundle-wins helper loss disappears.
- Negative: the runtime name is no longer the fixed literal `Rt` for `Embedded` output, so glue, docs and embedder code must derive it (Python: `<Class>Rt`).
  Backends whose mechanism is a textual rewrite (Perl, Python) require that no runtime unit mentions the runtime prefix inside a string literal; on Python a units-lint test enforces that.
- On Bash the per-module WASI import wrapper had to be renamed `<p>imp_wasi_<name>` ([ADR-12](12-bash-wasi.md) revision): its old name, `<p>wasi_<name>`, is what the prefixed unit itself is now called, so the wrapper would have called itself.
- The Go mechanism planned here — package-level identifier prefixing plus a header-less compose entry point, with `GenOptions.runtime` honored — was never built, and should not be: while it waited, ADR-63/#155 made library output declare `package <module name>` for unrelated reasons, and a package is a real namespace.
  Two artifacts now isolate with no renaming, no new entry point and no linkage plumbing; what was going to be the largest of the three took none of the work estimated for it.
  The observable in `embedded_coexist_e2e!` shifts accordingly: an importer cannot name the unexported `rtTrap`, so the driver recovers each panic and compares the values' dynamic types (`%T` prints them `*alpha.rtTrap` / `*beta.rtTrap`).
