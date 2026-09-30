# Decision 62: `Embedded` Output Isolates Its Runtime per Artifact

Status: **Accepted, 2026-08-05.**
All six backends follow it (issue #141 complete).
Ruby (lexical nesting) and Perl ([decision 55](55-perl-backend-lowering.md), a package prefix added to the text) already did.
Python follows it as of this decision: `RuntimeLinkage::Embedded` names its runtime class `<Class>Rt`.
That is `runtime_name` in `crates/dewasm-backend-python/src/lib.rs`.
Java nests its runtime classes in the module class.
Go gets isolation from the per-package library output ([#155](https://github.com/dewasm/dewasm/pull/155)).
Bash prefixes its runtime function names per artifact.
Every backend calls `embedded_coexist_e2e!`, which is the check.

## Context

[Decision 6](6-runtime-units.md) put the runtime behind one name and made its location a linkage choice.
It explicitly rejected *"a fixed global namespace for the embedded runtime"*.
Its reason: *"multi-module programs collide; version skew between artifacts goes undetected"*.
It claimed this as a consequence: *"generated files coexist in one process"*.
Ruby got that for free from constant nesting, which is what the decision was written against.

The backends since have flat runtimes, and each quietly reintroduced the rejected shape.
Python's `Embedded` output emits a top-level `class Rt:`.
Nesting it is impossible, because a Python method scope cannot see its outer class scope.
That scoping rule is recorded in [decision 28](28-python-backend-lowering.md).
Go and Java emit one top-level runtime.
Bash has a single flat namespace of `rt_*`/`mem_*` functions ([decision 11](11-bash-backend-lowering.md)).
Perl hit the same wall and solved it (decision 55), citing decision 6.

Two Python artifacts in one namespace therefore lose one runtime silently.
Trap identity collapses first: an embedder catching one artifact's trap catches the other's.
Worse, the bundle is a closure over the units each module reaches ([decision 6](6-runtime-units.md)).
Suppose the *first* artifact has the larger closure.
Then the second `class Rt` removes helpers the first still calls.
The result is an `AttributeError` at some later call site, nowhere near the cause.
Nothing warns at conversion time, which decision 0 says is where such failures belong.

`EMBEDDED_COEXIST` has covered exactly this since decision 27.
It is in `crates/dewasm-test-helper/src/multimodule.rs`.
The four flat-runtime backends carried REASON comments for not calling the macro.
Those comments described the name conflict as a property of the language.

## Decision

**`RuntimeLinkage::Embedded` output must be self-isolating.**
Consider two artifacts generated independently and placed in one namespace.
Each reaches its own runtime, with distinct trap, exit and link-error types.
The criterion: *an `Embedded` artifact names nothing another artifact also names.*
`Alias` is the only way to share a runtime.
Sharing is then a choice made at generation time.
It is not an accident of the target language's namespace.

The mechanism is per-backend, whatever isolation that language makes cheapest:

| Backend | Mechanism |
| --- | --- |
| Ruby | `module Rt` nested in the generated class; lexical constant lookup resolves it with no rewriting. |
| Perl | `Rt::` → `<Package>::Rt::` prefix added to the text at bundle time (decision 55). |
| Python | the runtime class is `<Class>Rt`, and the bundle's `Rt.` references are rewritten once at bundle time. |
| Java | `static` nested classes under the generated class (landed). Java resolves simple names through outer class scopes, so nothing inside the artifact is rewritten; only outside references gain the `<Class>.` qualifier ([decision 30](30-java-backend-lowering.md) revision). |
| Bash | `rt_`/`mem_`/`tab_`/`wasi_` function-name prefixing at bundle time (landed): the artifact's own prefix goes on every runtime name, `rt_trap` -> `<p>rt_trap`, and the backend emits its call sites already prefixed. `TRAP_MSG`/`EXIT_CODE`/`R0..` and `IMPORTS`/`PROVIDERS` stay global: they are the cross-module calling protocol two artifacts must share to link at all ([decision 35](35-bash-cross-module-linking.md)). Only the names change: each `Embedded` artifact already carried its own copy of the runtime text, so no text is copied that was not copied before, and parse cost is unchanged. |
| Go | one package per artifact: library output declares `package <module name>` ([decision 63](63-module-name-policy.md), #155), and a Go package *is* a namespace, so two artifacts share no identifier at all (landed). |

A backend is done when it calls `embedded_coexist_e2e!`.
Until then its REASON comment is a to-do, not a capability declaration.

`Alias` output is untouched by all of this: the shared runtime keeps the plain name `Rt`.
So the specification harness's generated text is byte-identical.

## Rejected alternatives

- **Nest the runtime for real on Python** (`class Rt` inside the generated class, Ruby's shape).
  Python method scopes cannot see the outer class scope.
  So every helper reference would have to spell the class (`Prog.Rt.trap`).
  That is a global lookup plus two attribute lookups instead of one plus one.
  It is paid on every masked-integer operation, every load and every store.
  That is in the slowest backend's hottest path.
  The rename costs nothing at run time.
  The generated code still resolves one module-level global by one name.
- **Accept flat runtimes as a permanent limitation** and keep the REASON comments.
  The failure is silent and is not only about trap identity.
  The smaller of the two bundles removes helpers the other artifact calls.
  Decision 6 rejected this shape before any of these backends existed.
  The deviation was drift, not a decision.
- **Split one artifact across files and lean on the target's module system** (Python `import`).
  Fixes Python only, and contradicts the single-file self-contained output contract ([decision 0](0-foundation.md)).
  It does nothing for Bash, Go or Java, where the namespace really is flat.
  Go's landed mechanism is not this: its `package` clause is a line *inside* the one generated file.
  So the artifact stays a single file, and the isolation costs nothing.
- **Detect the conflict at run time** (a guard that raises when a second runtime redefines the first).
  Converts a silent bug into a loud one but still refuses a program that has every right to run.

## Consequences

- Positive: decision 6's promise that artifacts can share a process holds where it is claimed.
  An embedder can hold two converted libraries in one namespace and catch each one's traps by name.
  This includes two versions of the same library.
  Python's hidden helper loss, where the smaller bundle wins, disappears.
- Negative: the runtime name is no longer the fixed literal `Rt` for `Embedded` output.
  So glue, documents and embedder code must derive it (Python: `<Class>Rt`).
  Some backends' mechanism is a rewrite of the text (Perl, Python).
  They require that no runtime unit mentions the runtime prefix inside a string literal.
  On Python a units-lint test checks that.
- On Bash the per-module WASI import wrapper had to be renamed `<p>imp_wasi_<name>`.
  That is a [decision 12](12-bash-wasi.md) revision.
  Its old name, `<p>wasi_<name>`, is what the prefixed unit itself is now called.
  So the wrapper would have called itself.
- The Go mechanism planned here was never built, and should not be.
  It was package-level identifier prefixing plus a header-less compose entry point.
  It also honored `GenOptions.runtime`.
  While it waited, decision 63/#155 made library output declare `package <module name>`.
  It did so for unrelated reasons.
  A package is a real namespace.
  Two artifacts now isolate with no renaming, no new entry point and no new linkage code.
  What was going to be the largest of the three took none of the work estimated for it.
  The observable in `embedded_coexist_e2e!` shifts accordingly.
  An importer cannot name the unexported `rtTrap`.
  So the driver recovers each panic and compares the values' dynamic types.
  `%T` prints them `*alpha.rtTrap` / `*beta.rtTrap`.
