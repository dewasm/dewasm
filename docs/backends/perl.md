# Perl backend

`--target perl`. A plain `require`-able package covering wasm core 1.0; WASI preview 1 is follow-up work (issue #69), so `_start`-shaped programs convert but WASI syscalls stub to `ENOSYS` for now.

## Output shape

A single `.pl` file: the generated module as a `package` named after the input stem (override with `--module-name`), instances as blessed hashrefs (`Package->new(\%imports)`), with the runtime bundled in package namespaces under the generated package (`Package::Rt`, `Package::Rt::Memory`, ... — perl package names are absolute, so the embedded runtime is prefixed rather than lexically nested). Control flow lowers to perl's native labeled blocks and loops: every `br` is a direct `last Ln`/`next Ln`, with no flag-variable scheme. See [ADR-55](../adr/55-perl-backend-lowering.md).

## Requirements

`perl` **5.26 or newer** on `PATH` (or `$DEWASM_PERL`), built with 64-bit integers and IEEE doubles (`ivsize=8`, `nvsize=8` — any stock perl on a 64-bit OS). No CPAN modules — the output uses only core modules (`POSIX`, `Config`, `File::Basename`). The generated prelude verifies the integer/double sizes at load time and dies loudly otherwise (ADR-15).

## Running it

```console
$ dewasm prog.wasm --target perl --mode standalone -o prog.pl
$ perl prog.pl
```

Standalone programs follow the shared runtime interface for exit codes (`proc_exit` maps to the process exit code once WASI lands; a trap prints `trap: <message>` and exits 134): [docs/standalone-interface.md](../standalone-interface.md).

Library mode (the file ends in a true value, so plain `require` works):

```perl
require './add.pl';
my $inst = Add->new({});
print $inst->invoke('add', 2, 3), "\n";   # 5
$inst->{memory};                          # linear memory (Add::Rt::Memory)
```

Imports are a hashref of module name to either a hashref (name to value) or a provider object with a `wasm_import($name)` method — another generated instance qualifies directly (ADR-7). Function imports are coderefs; globals/tables/memories are the runtime's boxed objects (`global_export`/`table_export` hand them out).

## Capabilities

Full wasm core 1.0 plus the universal baseline: non-function imports, multiple tables, and table bulk ops are supported. **No WASI yet** (issue #69). Authoritative matrix: [docs/support.md](../support.md).

## Caveats

- **Call depth is bounded by a counter.** Perl recursion grows on the heap and only stops at the OOM killer, so generated functions maintain an explicit depth counter (`$Rt::DEPTH`, limit `$Rt::LIMIT` = 100000) and trap with `call stack exhausted` past it ([ADR-55](../adr/55-perl-backend-lowering.md)). Raise `$Package::Rt::LIMIT` for legitimately deeper recursion (each frame costs heap).
- Float arithmetic goes through helpers (`Rt::fadd` and friends): perl's own operators take an integer fast path that exceeds double precision and drops `-0.0`, and perl dies on `x / 0.0` and `sqrt(-1)` ([ADR-55](../adr/55-perl-backend-lowering.md)).
- Numeric conventions are the shared masked-unsigned model ([ADR-2](../adr/2-numeric-semantics.md)); `use integer` appears only inside tightly-scoped runtime helpers.
