# Decision 96: Generated Code Resolves at Conversion Time What Conversion Time Knows

Status: **Accepted, 2026-09-19.**
Landed: the Ruby provider dispatch (`Rt::WASI#import`) and the export accessors are generated as literal dispatches ([`crates/dewasm-backend-ruby/src/lib.rs`](../../crates/dewasm-backend-ruby/src/lib.rs), over the bundler's `ScopeMember` hook in [`crates/dewasm-backend/src/lib.rs`](../../crates/dewasm-backend/src/lib.rs)), and `--mode standalone` output carries no main guard in Ruby, Python, Codon, and Bash.
The other backends keep their reflective provider dispatch until a compiler for their language needs otherwise.

## Context

Generated code was written for an interpreter, one that can look a method or an instance variable up under a name computed at run time.
Three shapes did exactly that, and all three are the *generated* side of a table that is already fixed when the artifact is written:

- `Rt::WASI#import` resolved `method(:"wasi_#{name}")` behind a `respond_to?`, though the bundled syscalls are chosen at conversion time.
- `global_get`, `global_export`, `table_export` and `tag_export` resolved `instance_variable_get(GLOBAL_EXPORTS.fetch(name))`, though the export table is written into the same file.
- Every standalone program ran its main behind `__FILE__ == $PROGRAM_NAME` (`__name__ == "__main__"`, `${BASH_SOURCE[0]} == "$0"`), though `--mode standalone` already says the artifact is a program.

An ahead-of-time compiler for the target language has none of that at run time.
Spinel (matz/spinel), a Ruby AOT compiler, carries no method table for a computed symbol to consult, and in a compiled program `$PROGRAM_NAME` is the binary rather than the source file the guard names, so a guarded main never runs and the program does nothing.
These three shapes were the whole gap: patching them locally was all that stood between dewasm's Ruby output and compiling under it.

## Decision

Generated code resolves at conversion time what conversion time already knows.
A name-to-member step whose table is fixed when the artifact is written is emitted as a literal dispatch, never as a run-time lookup under a computed name.
The criterion is where the table comes from, not which language is being emitted: a lookup keyed by host-supplied data (the `imports` table, `@exports`) stays a lookup, because nothing at conversion time knows its contents.

A `--mode standalone` artifact is a program: it runs on load, in every backend.
The guard defended against loading a program as a library, which is what `--mode library` produces, so it protected nothing the mode system did not already express, while quietly reducing a compiled program to a no-op.

The bundler grew one mechanism for the first half: a scope may register a [`ScopeMember`](../../crates/dewasm-backend/src/lib.rs), a function of the bundle's unit ids emitted after that scope's units.
A member whose body must name the units that ended up in the bundle cannot be a fixed unit source (decision 6).

## Rejected alternatives

- **A flag for the main guard**: a switch deciding which of two shapes standalone output takes makes the mode mean two things, and every backend would have to answer it separately.
  The mode already distinguishes "run it" from "load it", and the standalone interface is deliberately uniform across backends (decision 31).
- **Reflective dispatch behind an AOT-only code path**: there is no such path.
  dewasm emits source; which compiler or interpreter consumes it is the user's choice, unknown at conversion time.
- **A method table built at class-definition time** (`IMPORTS["fd_write"] = instance_method(:wasi_fd_write)`): the literal names come back, but the lookup becomes a method object bound per call, paid at every instantiation, for no gain over a `case`.
- **Name-to-ivar hashes kept beside the literal accessors**: with the accessors resolving the ivar themselves, the symbols in those hashes are dead data.
  `GLOBAL_EXPORTS`, `TABLE_EXPORTS` and `TAG_EXPORTS` are now frozen name arrays, tested with `include?`, matching `MEMORY_EXPORTS`.

## Consequences

- Positive: the three local patches that compiling the Ruby output under Spinel required are gone from the product; the compile-and-run check itself lives in that project, not here.
- Positive: `Rt::WASI#import` now answers for the WASI preview 1 surface only.
  The `respond_to?` form also matched the class's own helpers, so `import("filetype")` used to hand out the internal `wasi_filetype`.
- Negative: a standalone artifact can no longer be loaded from other code without running the guest.
  That is what `--mode library` is for, and [`docs/standalone-interface.md`](../../docs/standalone-interface.md) states it.
- Carry-over: Python, Go, Java and Perl still resolve provider imports reflectively (`getattr` in [`crates/dewasm-backend-python/units/wasi/_class.py`](../../crates/dewasm-backend-python/units/wasi/_class.py)); Codon, the one backend already compiled ahead of time, has no `import` on its WASI class at all.

See also: [decision 6](6-runtime-units.md) (the runtime units this generates a member into), [decision 7](7-import-providers.md) (the provider protocol whose dispatch this changes), [decision 31](31-standalone-runtime-interface.md) (the standalone interface the guard removal belongs to).
