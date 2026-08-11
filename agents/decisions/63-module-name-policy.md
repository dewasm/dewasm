# ADR-63 — `--module-name`: Fixed in Standalone, Validated Verbatim in Library Mode

Status: **Accepted, 2026-08-05.**
Landed for every backend and the CLI: the per-backend PascalCase sanitizers (and Bash's snake analogue) are deleted, a standalone artifact's internal name is fixed, and a library-mode name is used exactly as given or refused.
Go is included (`validate_library_module_name`, `crates/dewasm-backend-go/src/lib.rs`).
The kebab-case derivation the test suites use lives in `crates/dewasm-test-helper/src/backend.rs`, test infrastructure rather than a product surface.

## Context

`--module-name` (default: the input file stem) named the generated class/package/prefix, and each backend ran its own lossy sanitizer over it: uppercase every alphanumeric run, drop everything else, prepend `Wasm` if what remained was empty or digit-initial.
Nothing was ever rejected, so `sqlite3-shell` became `Sqlite3Shell`, `--` became `Wasm`, and `hello_world` and `HelloWorld` became the same class; the caller got a name they never wrote, predictable only by reading five backends' source, and any future namespacing feature (a Ruby constant path, a Java package) had to squeeze through a transformation that had already discarded `:` and `.`.
The two modes are not even the same problem: a standalone artifact's internal name appears in no interface, so asking the caller to supply one is a question with no right answer.

## Decision

**1. A standalone artifact's internal name is fixed.**
Ruby/Python `class Program`, Perl `package Program`, Java module class `Program` (the driver stays `public class Main`), Bash prefix `program_`, Go `package main` with type `Program`.
Passing `--module-name` together with `--mode standalone` is a conversion-time error naming the mode; the CLI hands the backend the fixed placeholder `program`, which labels only the output *file*, so converting any `.wasm` standalone needs no name at all.

**2. A library-mode name is required, and used verbatim or refused, never transformed and never defaulted.**
The flag is mandatory in library mode: the input file's name on disk says nothing about what the caller wants the embedded API to be called, so there is no default to derive.
The discriminating criterion, and the reusable rule: *a specification whose correct behaviour cannot be predicted from the input must not exist.*
An implicit transformation is a permanent maintenance burden, preserved for compatibility long after anyone remembers why it maps what it maps.
Validation is the opposite: the grammar is one line of documentation, and the failure is at conversion time with both the offending value and the grammar in the message (`dewasm_backend::check_module_name`, [ADR-0](0-foundation.md)).

| Backend | Library-mode grammar | Emitted as |
| --- | --- | --- |
| Ruby | `::`-separated segments, each `[A-Z][A-Za-z0-9_]*` | `class A::B::C`, preceded by guarded ancestor definitions |
| Perl | `::`-separated segments, each `[A-Za-z_][A-Za-z0-9_]*` | `package A::B::C`, runtime under `A::B::C::Rt` |
| Python | one identifier `[A-Za-z_][A-Za-z0-9_]*` | `class Name`, runtime `class NameRt` ([ADR-62](62-embedded-runtime-isolation.md)) |
| Java | `.`-separated segments, each `[A-Za-z_$][A-Za-z0-9_$]*` | leading segments → `package a.b;` first line, last segment → the class name |
| Bash | one identifier `[A-Za-z_][A-Za-z0-9_]*` | prefix = the name **lowercased** + `_` |
| Go | one identifier `[A-Za-z_][A-Za-z0-9_]*` | `package <lowercased>`, type = first letter capitalized |

Four details the grammars imply:

- **Ruby's guarded ancestors.**
  `class A::B::C` requires `A` and `A::B` to exist, and the file must load both standalone and beside a program that already defined them, so each ancestor gets `unless defined?(A)` / `module A; end`, outermost first.
  An existing ancestor is skipped by the guard and kept intact whatever it is; one bound to a non-module constant fails loudly at load with `TypeError`, the right outcome.
  A single-segment name emits no guards.
- **Java's split rule.**
  The last dot-separated segment is the class name and anything before it is the package declaration, emitted as the file's first statement ahead of the bundled runtime classes ([ADR-30](30-java-backend-lowering.md)), so `com.github.dewasm.Ruby` yields both a conventional package and a conventional class name (which is why the class segment is not forced to be capitalized).
  The grammar is character-level only: a segment that happens to be a Java keyword (`int`) or a restricted type name (`var`, JLS 8.1) passes validation and fails in `javac`, because a maintained keyword list is exactly the continuously-updated language-specific table this ADR exists to avoid and the compiler already errors at the same stage of the workflow.
- **Bash's lowercase exception.**
  Every generated Bash name is a global shell identifier, so the prefix is the name lowercased.
  This is the one deliberate mapping the policy keeps, admissible precisely because it is total and fully specified (`Sqlite3Shell` → `sqlite3shell_`, always) with no information the caller needs back.
- **Go's `--module-name main` is accepted.**
  `main` is a Go identifier and the grammar is the whole rule, so blocking one word would mean the name is not used verbatim after all; it also has a real use, dropping the artifact beside a hand-written `func main`.
  A caller who meant something else gets a Go compile error, not a silent wrong artifact.

**3. Validation happens in `Backend::generate` only, never in the `*_with_units` APIs.**
Those take an already-final internal handle rather than a caller's request (the spec harness hands Bash the prefix `m0_` and Ruby the class `WastMod0`), and the grammar is a rule about what a caller may *ask for*, so it belongs at the one place a request enters.

**4. Kebab-case derivation is test infrastructure.**
Test and tooling names are kebab/stem case (`sqlite3-shell`, `prog`) and each backend must be handed a name in its own grammar, so `BackendUnderTest::module_name` (defaulting through `derive_module_name`, `crates/dewasm-test-helper/src/backend.rs`) converts `[a-z0-9]+(-[a-z0-9]+)*` to PascalCase and panics outside that domain.
Bash alone takes snake_case, because its prefix is the name lowercased and `sqlite3_shell_` is what its glue spells; Go takes Pascal despite its lowercase package clause, because it builds *both* names out of the one string and only a Pascal input yields a conventional pair (`Sqlite3Shell` → `package sqlite3shell`, `type Sqlite3Shell`).
It lives in the test helper on purpose: the product performs no name transformation at all, and a convenience that only test tables need must not become a CLI behaviour nobody can remove later.

## Rejected alternatives

- **Keep sanitizing, but write the rule down.**
  The rule is unwritable as one rule (five variants), and documenting a lossy mapping does not make its inverse exist: the caller still cannot ask for `hello_world` and get it.
  It also blocks the namespacing the grammars now allow, since a sanitizer that deletes `:` and `.` can never grow a Ruby constant path or a Java package.
- **Sanitize, but warn on stderr.**
  A warning is not an error: CI pipelines swallow it, and the artifact still has a name the caller did not ask for.
  ADR-0's rule is that a request the converter cannot honour fails at conversion time.
- **Let `--module-name` work in standalone mode too** (naming the internal class).
  It answers a question no caller has, and it forced the Java dotted-name question, a `package` for a program launched as `Main`, into a decision with no good answer.
- **Default the library-mode name from the input file stem.**
  It couples the generated API's public name to how the input file happens to be stored (rename the file and the class changes) and resurrects a mode-and-backend-dependent surprise: a stem valid for Python but not for Ruby converts on one target and errors on the other.
- **Do the kebab derivation in the CLI** (accept kebab-case and convert).
  That is the sanitizer again, with a smaller input domain.
  The test suites need it because their case tables are keyed by cache stem; callers type the name they want.

## Consequences

- What the caller writes is what the artifact is called.
  Ruby gains constant paths and Java gains packages with no new flags, straight out of the grammars, and the generated output is byte-identical for every name that was already valid.
- Every library-mode conversion must now pass `--module-name` (`dewasm add.wasm --mode library` alone is an error).
  `examples/rails/build.sh` and the library walkthroughs in `README.md` and `docs/getting-started.md` spell it out.
- The module name's meaning is mode-dependent: an internal name in library mode, only an output filename in standalone.
  The CLI's standalone rejection is what keeps that from being discovered by surprise.
