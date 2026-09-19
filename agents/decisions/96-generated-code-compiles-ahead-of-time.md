# Decision 96: Generated Output Must Survive Ahead-of-Time Compilation

Status: **Accepted, 2026-09-19.**
Landed: the Ruby provider dispatch (`Rt::WASI#import`) and the export accessors are literal dispatches ([`crates/dewasm-backend-ruby/src/lib.rs`](../../crates/dewasm-backend-ruby/src/lib.rs), over the bundler's `ScopeMember` hook in [`crates/dewasm-backend/src/lib.rs`](../../crates/dewasm-backend/src/lib.rs)), the filesystem errno mapping is a `rescue` dispatch ([`crates/dewasm-backend-ruby/units/wasi/errno_fs.rb`](../../crates/dewasm-backend-ruby/units/wasi/errno_fs.rb)), and `--mode standalone` output carries no main guard in Ruby, Python, Codon, and Bash.
The other backends keep their reflective provider dispatch until a compiler for their language needs otherwise.

## Context

Generated code was written for an interpreter: one that resolves a method, an instance variable, or a constant under a name the program computes, and that knows which file it was started from.
Four shapes relied on that, measured against Spinel (matz/spinel), a Ruby ahead-of-time compiler, while compiling dewasm's Ruby output:

- `Rt::WASI#import` resolved `method(:"wasi_#{name}")` behind a `respond_to?`, though the bundled syscalls are chosen at conversion time.
- `global_get`, `global_export`, `table_export` and `tag_export` resolved `instance_variable_get` over a name-to-ivar hash, though the export table is written into the same file.
- `FS_ERRNO` keyed a hash by `Errno` *class objects*, which raised `NameError` at class-definition time under Spinel, killing the program before it ran.
- Every standalone program ran its main behind `__FILE__ == $PROGRAM_NAME` (`__name__ == "__main__"`, `${BASH_SOURCE[0]} == "$0"`), though `--mode standalone` already says the artifact is a program, and a compiled program's `$PROGRAM_NAME` is the binary rather than the source file the guard names, so the main never ran.

Each of these was a hand patch on the way to compiling the output, and none of them buys anything under an interpreter either.

## Decision

Generated code is source we hand to a toolchain we do not choose, so it uses constructs that survive compilation, under two rules.

**Resolve at conversion time what conversion time already knows.**
A name-to-member step whose table is fixed when the artifact is written is emitted as a literal dispatch, never as a run-time lookup under a computed name.
What bounds the rule is where the table comes from: a lookup keyed by host-supplied data (the `imports` table, `@exports`) stays a lookup, because nothing at conversion time knows its contents.

**Where the language spells one thing several ways, take the spelling that compiles.**
Naming an `Errno` class as a value (a hash key, a `case/when` operand) and naming it in a `rescue` clause are the same test to an interpreter; only the `rescue` clause is dependable once the source is compiled, so the errno mapping is a dispatch over `rescue` clauses.
This costs one re-raise on a path that already raised, and it keeps the single shared mapping the syscall units call.

A `--mode standalone` artifact is a program: it runs on load, in every backend.
The guard defended against loading a program as a library, which is what `--mode library` produces, so it protected nothing the mode system did not already express.

The bundler grew one mechanism for the first rule: a scope may register a [`ScopeMember`](../../crates/dewasm-backend/src/lib.rs), a function of the bundle's unit ids emitted after that scope's units.
A member whose body must name the units that ended up in the bundle cannot be a fixed unit source (decision 6).

## Rejected alternatives

- **A flag for the main guard**: a switch deciding which of two shapes standalone output takes makes the mode mean two things, and every backend would have to answer it separately.
  The mode already distinguishes "run it" from "load it", and the standalone interface is deliberately uniform across backends (decision 31).
- **Reflective dispatch behind an AOT-only code path**: there is no such path.
  dewasm emits source; which compiler or interpreter consumes it is the user's choice, unknown at conversion time.
- **A method table built at class-definition time** (`IMPORTS["fd_write"] = instance_method(:wasi_fd_write)`): the literal names come back, but the lookup becomes a method object bound per call, paid at every instantiation, for no gain over a `case`.
- **`case e when Errno::ENOENT`** for the errno mapping: the same class-as-a-value shape in another spelling, and the failure is worse than the hash's.
  Under Spinel it compiles and then silently takes the `else` branch, turning every host error into `EIO` with nothing to see; the hash at least raised.
- **Reading `e.errno` and comparing numbers**: the numbers are the platform's, so the mapping would have to carry an errno table per host, which the `Errno` classes already are.
- **Name-to-ivar hashes kept beside the literal accessors**: with the accessors resolving the ivar themselves, the symbols in those hashes are dead data.
  `GLOBAL_EXPORTS`, `TABLE_EXPORTS` and `TAG_EXPORTS` are now frozen name arrays, tested with `include?`, matching `MEMORY_EXPORTS`.

## Consequences

- Positive: the local patches that compiling the Ruby output under Spinel required are gone from the product; the compile-and-run check itself lives in that project, not here.
- Positive: `Rt::WASI#import` now answers for the WASI preview 1 surface only.
  The `respond_to?` form also matched the class's own helpers, so `import("filetype")` used to hand out the internal `wasi_filetype`.
- Negative: a standalone artifact can no longer be loaded from other code without running the guest.
  That is what `--mode library` is for, and [`docs/standalone-interface.md`](../../docs/standalone-interface.md) states it.
- Negative: the errno mapping is now ordered, where a hash was keyed.
  Two `Errno` constants that name one class (`EWOULDBLOCK`/`EAGAIN` on Linux) would resolve to the first clause rather than the last entry; none of the twelve mapped errors alias each other, checked by mapping every `Errno` constant through both forms.
- Carry-over: Python, Go, Java and Perl still resolve provider imports reflectively (`getattr` in [`crates/dewasm-backend-python/units/wasi/_class.py`](../../crates/dewasm-backend-python/units/wasi/_class.py)); Codon, the one backend already compiled ahead of time, has no `import` on its WASI class at all.

See also: [decision 6](6-runtime-units.md) (the runtime units this generates a member into), [decision 7](7-import-providers.md) (the provider protocol whose dispatch this changes), [decision 31](31-standalone-runtime-interface.md) (the standalone interface the guard removal belongs to).
