# Decision 63: `--module-name` Fixed in Standalone, Validated Verbatim in Library Mode

Status: **Accepted, 2026-08-05.**
Landed for every backend and the CLI.
The per-backend PascalCase sanitizers (and Bash's snake analogue) are deleted.
A standalone artifact's internal name is fixed.
A library-mode name is used exactly as given or refused.
Go is included (`validate_library_module_name`, `crates/dewasm-backend-go/src/lib.rs`).
The kebab-case derivation the test suites use lives in `crates/dewasm-test-helper/src/backend.rs`.
It is test infrastructure rather than a product surface.

## Context

`--module-name` (default: the input file stem) named the generated class/package/prefix.
Each backend ran its own lossy sanitizer over it:

- uppercase every alphanumeric run;
- drop everything else;
- prepend `Wasm` if what remained was empty or digit-initial.

Nothing was ever rejected.
So `sqlite3-shell` became `Sqlite3Shell` and `--` became `Wasm`.
`hello_world` and `HelloWorld` became the same class.
The caller got a name they never wrote, predictable only by reading five backends' source.
Any future namespacing feature (a Ruby constant path, a Java package) had to squeeze through it.
That transformation had already discarded `:` and `.`.
The two modes are not even the same problem.
A standalone artifact's internal name appears in no interface.
So asking the caller to supply one is a question with no right answer.

## Decision

**1. A standalone artifact's internal name is fixed.**
The fixed names are Ruby/Python `class Program`, Perl `package Program`, and Bash prefix `program_`.
Go uses `package main` with type `Program`.
Java uses module class `Program` (the driver stays `public class Main`).
Passing `--module-name` together with `--mode standalone` is a conversion-time error naming the mode.
The CLI hands the backend the fixed placeholder `program`, which labels only the output *file*.
So converting any `.wasm` standalone needs no name at all.

**2. A library-mode name is required, used verbatim or refused, never transformed or defaulted.**
The flag is mandatory in library mode.
The input file's name on disk says nothing about what the caller wants the embedded API called.
So there is no default to derive.
The discriminating criterion is also the reusable rule.
*A specification whose correct behaviour cannot be predicted from the input must not exist.*
An implicit transformation is a permanent maintenance burden.
It is preserved for compatibility long after anyone remembers why it maps what it maps.
Validation is the opposite: the grammar is one line of documentation.
The failure is at conversion time with both the offending value and the grammar in the message.
The check is `dewasm_backend::check_module_name` ([decision 0](0-foundation.md)).

| Backend | Library-mode grammar | Emitted as |
| --- | --- | --- |
| Ruby | `::`-separated segments, each `[A-Z][A-Za-z0-9_]*` | `class A::B::C`, preceded by guarded ancestor definitions |
| Perl | `::`-separated segments, each `[A-Za-z_][A-Za-z0-9_]*` | `package A::B::C`, runtime under `A::B::C::Rt` |
| Python | one identifier `[A-Za-z_][A-Za-z0-9_]*` | `class Name`, runtime `class NameRt` ([decision 62](62-embedded-runtime-isolation.md)) |
| Java | `.`-separated segments, each `[A-Za-z_$][A-Za-z0-9_$]*` | leading segments → `package a.b;` first line, last segment → the class name |
| Bash | one identifier `[A-Za-z_][A-Za-z0-9_]*` | prefix = the name **lowercased** + `_` |
| Go | one identifier `[A-Za-z_][A-Za-z0-9_]*` | `package <lowercased>`, type = first letter capitalized |

Four details the grammars imply:

- **Ruby's guarded ancestors.**
  `class A::B::C` requires `A` and `A::B` to exist.
  The file must load both standalone and beside a program that already defined them.
  So each ancestor gets `unless defined?(A)` / `module A; end`, outermost first.
  An existing ancestor is skipped by the guard and kept intact whatever it is.
  One bound to a non-module constant fails loudly at load with `TypeError`, the right outcome.
  A single-segment name emits no guards.
- **Java's split rule.**
  The last dot-separated segment is the class name.
  Anything before it is the package declaration, emitted as the file's first statement.
  It comes ahead of the bundled runtime classes ([decision 30](30-java-backend-lowering.md)).
  So `com.github.dewasm.Ruby` yields both a conventional package and a conventional class name.
  That is why the class segment is not forced to be capitalized.
  The grammar is character-level only.
  A Java keyword (`int`) or restricted type name (`var`, JLS 8.1) as a segment passes validation.
  It then fails in `javac`.
  The validator keeps no keyword list, for two reasons.
  Such a list is the continuously-updated language-specific table this decision exists to avoid.
  The compiler already errors at the same stage of the workflow.
- **Bash's lowercase exception.**
  Every generated Bash name is a global shell identifier, so the prefix is the name lowercased.
  This is the one deliberate mapping the policy keeps.
  It is admissible precisely because it is total and fully specified.
  `Sqlite3Shell` always becomes `sqlite3shell_`, with no information the caller needs back.
- **Go's `--module-name main` is accepted.**
  `main` is a Go identifier and the grammar is the whole rule.
  So blocking one word would mean the name is not used verbatim after all.
  It also has a real use: dropping the artifact beside a hand-written `func main`.
  A caller who meant something else gets a Go compile error, not a silent wrong artifact.

**3. Validation happens in `Backend::generate` only, never in the `*_with_units` APIs.**
Those take an already-final internal handle rather than a caller's request.
The spec harness hands Bash the prefix `m0_` and Ruby the class `WastMod0`.
The grammar is a rule about what a caller may *ask for*.
So it belongs at the one place a request enters.

**4. Kebab-case derivation is test infrastructure.**
Test and tooling names are kebab/stem case (`sqlite3-shell`, `prog`).
Each backend must be handed a name in its own grammar.
So `BackendUnderTest::module_name` converts `[a-z0-9]+(-[a-z0-9]+)*` to PascalCase.
It panics outside that domain.
It defaults through `derive_module_name` (`crates/dewasm-test-helper/src/backend.rs`).
Bash alone takes snake_case, because its prefix is the name lowercased.
`sqlite3_shell_` is what its glue spells.
Go takes Pascal despite its lowercase package clause.
The reason is that it builds *both* names out of the one string.
Only a Pascal input yields a conventional pair.
`Sqlite3Shell` gives `package sqlite3shell` and `type Sqlite3Shell`.
It lives in the test helper on purpose: the product performs no name transformation at all.
A convenience that only test tables need must not become a CLI behaviour nobody can remove later.

## Rejected alternatives

- **Keep sanitizing, but write the rule down.**
  The rule is unwritable as one rule (five variants).
  Documenting a lossy mapping does not make its inverse exist.
  The caller still cannot ask for `hello_world` and get it.
  It also blocks the namespacing the grammars now allow.
  A sanitizer that deletes `:` and `.` can never grow a Ruby constant path or a Java package.
- **Sanitize, but warn on stderr.**
  A warning is not an error: CI pipelines swallow it.
  The artifact still has a name the caller did not ask for.
  Decision 0's rule is that a request the converter cannot honour fails at conversion time.
- **Let `--module-name` work in standalone mode too** (naming the internal class).
  It answers a question no caller has.
  It forced the Java dotted-name question into a decision with no good answer.
  That question is a `package` for a program launched as `Main`.
- **Default the library-mode name from the input file stem.**
  It couples the generated API's public name to how the input file happens to be stored.
  Rename the file and the class changes.
  It also resurrects a mode-and-backend-dependent surprise.
  A stem valid for Python but not for Ruby converts on one target and errors on the other.
- **Do the kebab derivation in the CLI** (accept kebab-case and convert).
  That is the sanitizer again, with a smaller input domain.
  The test suites need it because their case tables are keyed by cache stem.
  Callers type the name they want.

## Consequences

- What the caller writes is what the artifact is called.
  Ruby gains constant paths and Java gains packages with no new flags, straight out of the grammars.
  The generated output is byte-identical for every name that was already valid.
- Every library-mode conversion must now pass `--module-name`.
  `dewasm add.wasm --mode library` alone is an error.
  `examples/rails/build.sh` spells it out.
  So do the library walkthroughs in `README.md` and `docs/getting-started.md`.
- The module name's meaning is mode-dependent.
  It is an internal name in library mode, only an output filename in standalone.
  The CLI's standalone rejection is what keeps that from being discovered by surprise.
