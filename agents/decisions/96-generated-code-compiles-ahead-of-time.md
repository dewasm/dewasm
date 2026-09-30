# Decision 96: Generated Output Must Survive Ahead-of-Time Compilation

Status: **Accepted, 2026-09-19.**
Landed:
- The Ruby provider dispatch (`Rt::WASI#import`) and the export accessors are literal dispatches.
  They are in [`crates/dewasm-backend-ruby/src/lib.rs`](../../crates/dewasm-backend-ruby/src/lib.rs).
  They use the bundler's `ScopeMember` hook in [`crates/dewasm-backend/src/lib.rs`](../../crates/dewasm-backend/src/lib.rs).
- The filesystem errno mapping is a `rescue` dispatch.
  It is in [`crates/dewasm-backend-ruby/units/wasi/errno_fs.rb`](../../crates/dewasm-backend-ruby/units/wasi/errno_fs.rb).
- `--mode standalone` output carries no main guard in Ruby, Python, Codon, and Bash.

The other backends keep their reflective provider dispatch.
They keep it until a compiler for their language needs otherwise.

## Context

Generated code was written for an interpreter.
Such an interpreter resolves a method, an instance variable, or a constant under a computed name.
It also knows which file it was started from.
Four shapes relied on that.
Spinel (matz/spinel) is a Ruby ahead-of-time compiler.
The four shapes were measured while compiling dewasm's Ruby output with it:

- `Rt::WASI#import` resolved `method(:"wasi_#{name}")` behind a `respond_to?`.
  Yet the bundled syscalls are chosen at conversion time.
- `global_get`, `global_export`, `table_export` and `tag_export` resolved `instance_variable_get` over a hash.
  The hash mapped names to ivars, though the export table is written into the same file.
- `FS_ERRNO` keyed a hash by `Errno` *class objects*.
  That raised `NameError` at class-definition time under Spinel, killing the program before it ran.
- Every standalone program ran its main behind a guard.
  The guard was `__FILE__ == $PROGRAM_NAME` (`__name__ == "__main__"`, `${BASH_SOURCE[0]} == "$0"`).
  Yet `--mode standalone` already says the artifact is a program.
  A compiled program's `$PROGRAM_NAME` is the binary rather than the source file the guard names.
  So the main never ran.

Each of these was a hand patch on the way to compiling the output.
None of them buys anything under an interpreter either.

## Decision

Generated code is source we hand to a toolchain we do not choose.
So it uses constructs that survive compilation, under two rules.

**Resolve at conversion time what conversion time already knows.**
Take a name-to-member step whose table is fixed when the artifact is written.
It is emitted as a literal dispatch, never as a run-time lookup under a computed name.
What bounds the rule is where the table comes from.
A lookup keyed by host-supplied data (the `imports` table, `@exports`) stays a lookup.
That is because nothing at conversion time knows its contents.

**Where the language spells one thing several ways, take the spelling that compiles.**
Naming an `Errno` class as a value (a hash key, a `case/when` operand) is one spelling.
Naming it in a `rescue` clause is another, and the two are the same test to an interpreter.
Only the `rescue` clause is dependable once the source is compiled.
So the errno mapping is a dispatch over `rescue` clauses.
This costs one re-raise on a path that already raised.
It keeps the single shared mapping the syscall units call.

A `--mode standalone` artifact is a program: it runs on load, in every backend.
The guard defended against loading a program as a library.
That is what `--mode library` produces.
So the guard protected nothing the mode system did not already express.

The bundler grew one mechanism for the first rule.
A scope may register a [`ScopeMember`](../../crates/dewasm-backend/src/lib.rs).
That is a function of the bundle's unit ids, emitted after that scope's units.
Some members must name the units that ended up in the bundle.
Such a member cannot be a fixed unit source (decision 6).

## Rejected alternatives

- **A flag for the main guard.**
  A switch would decide which of two shapes standalone output takes.
  That makes the mode mean two things, and every backend would have to answer it separately.
  The mode already distinguishes "run it" from "load it".
  The standalone interface is deliberately uniform across backends (decision 31).
- **Reflective dispatch behind an AOT-only code path.**
  There is no such path.
  dewasm emits source.
  Which compiler or interpreter consumes it is the user's choice, unknown at conversion time.
- **A method table built at class-definition time.**
  An example entry is `IMPORTS["fd_write"] = instance_method(:wasi_fd_write)`.
  The literal names come back, but the lookup becomes a method object bound per call.
  That is paid at every instantiation, for no gain over a `case`.
- **`case e when Errno::ENOENT` for the errno mapping.**
  It is the same class-as-a-value shape in another spelling.
  Its failure is also worse than the hash's.
  Under Spinel it compiles and then silently takes the `else` branch.
  That turns every host error into `EIO` with nothing to see; the hash at least raised.
- **Reading `e.errno` and comparing numbers.**
  The numbers are the platform's, so the mapping would have to carry an errno table per host.
  The `Errno` classes already are that table.
- **Name-to-ivar hashes kept beside the literal accessors.**
  The accessors resolve the ivar themselves, so the symbols in those hashes are dead data.
  `GLOBAL_EXPORTS`, `TABLE_EXPORTS` and `TAG_EXPORTS` are now frozen name arrays, tested with `include?`.
  That matches `MEMORY_EXPORTS`.

## Consequences

- Positive: compiling the Ruby output under Spinel required local patches.
  Those patches are gone from the product.
  The compile-and-run check itself lives in that project, not here.
- Positive: `Rt::WASI#import` now answers for the WASI preview 1 surface only.
  The `respond_to?` form also matched the class's own helpers.
  So `import("filetype")` used to hand out the internal `wasi_filetype`.
- Negative: a standalone artifact can no longer be loaded from other code without running the guest.
  That is what `--mode library` is for.
  [`docs/standalone-interface.md`](../../docs/standalone-interface.md) states it.
- Negative: the errno mapping is now ordered, where a hash was keyed.
  Two `Errno` constants can name one class (`EWOULDBLOCK`/`EAGAIN` on Linux).
  They would resolve to the first clause rather than the last entry.
  None of the twelve mapped errors alias each other.
  That was checked by mapping every `Errno` constant through both forms.
- Carry-over: Python, Go, Java and Perl still resolve provider imports reflectively.
  In Python that is `getattr` in [`crates/dewasm-backend-python/units/wasi/_class.py`](../../crates/dewasm-backend-python/units/wasi/_class.py).
  Codon is the one backend already compiled ahead of time.
  It has no `import` on its WASI class at all.

See also:
- [decision 6](6-runtime-units.md) (the runtime units this generates a member into);
- [decision 7](7-import-providers.md) (the provider protocol whose dispatch this changes);
- [decision 31](31-standalone-runtime-interface.md) (the standalone interface the guard removal belongs to).
