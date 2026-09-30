# Perl backend

`--target perl`.
A plain `require`-able package covering wasm core 1.0 plus full WASI Preview 1.
Its WASI follows the model of an `fd` table plus preopens, with `sock_*` excepted.
See [`docs/support.md`](../support.md).

## Output shape

A single `.pl` file holds the generated module as a `package`.
Instances are blessed hash references (`Package->new(\%imports)`).
The runtime is bundled in package namespaces under the generated package.
Those are `Package::Rt`, `Package::Rt::Memory`, and so on.
Perl package names are absolute, so the embedded runtime is prefixed rather than lexically nested.
Control flow lowers to Perl's native labeled blocks and loops, with no flag-variable scheme.
Every `br` is a direct `last Ln`/`next Ln`.

In **library** mode, `--module-name` is required and is taken unchanged as the package name.
It is `::`-separated segments, each matching `[A-Za-z_][A-Za-z0-9_]*`.
So `Dewasm::Sqlite3` nests as you would expect and carries the runtime with it (`Dewasm::Sqlite3::Rt`).
Anything else is a conversion-time error, nothing is rewritten.
In **standalone** mode the package is always `Program` and `--module-name` is rejected.

## Requirements

`perl` **5.26 or newer** on `PATH`, built with 64-bit integers and IEEE doubles (`ivsize=8`, `nvsize=8`).
Any stock `perl` on a 64-bit OS has both.
No CPAN modules: the output uses only core modules.
Those are `POSIX`, `Config`, `Cwd`, `Errno`, `Fcntl`, `Time::HiRes`, `File::Basename`, and `IO::Handle`.
The generated file verifies the integer/double sizes at load time and dies with an error otherwise.

## Running it

```console
$ dewasm prog.wasm --target perl --mode standalone -o prog.pl
$ perl prog.pl --dir .::/ input.txt
```

Standalone programs follow the shared runtime interface ([`docs/standalone-interface.md`](../standalone-interface.md)):

- A leading run of `--dir HOST::GUEST` flags mounts host directories at guest paths.
- The rest of the command line becomes the guest's `argv`.
- `proc_exit` maps to the process exit code.
- A trap prints `trap: <message>` and exits 134.

Library mode (the file ends in a true value, so plain `require` works):

```perl
require './add.pl';
my $inst = Add->new({});
print $inst->invoke('add', 2, 3), "\n";   # 5
$inst->{memory};                          # linear memory (Add::Rt::Memory)
```

A WASI module's constructor additionally takes `args`, `env`, and `preopens` options:

```perl
my $inst = Prog->new({}, args => ['prog', 'input.txt'], env => { LANG => 'C' }, preopens => { '/work' => './scratch' });
$inst->invoke('_start');   # proc_exit dies a Prog::Rt::Exit carrying {code}
```

Imports are a hash reference from module name to an import source.
A source is a hash reference (name to value) or a provider object.
A provider object has a `wasm_import($name)` method.
Its optional `attach($instance)` method is called after construction.
Another generated instance qualifies directly.
Function imports are code references.
Globals/tables/memories are the runtime's boxed objects, which `global_export`/`table_export` hand out.
An explicit WASI import overrides the bundled runtime per function.
Unhandled ones fall back to the bundled runtime, which is constructed lazily.

## Capabilities

Full wasm core 1.0 is supported, plus the universal baseline.
That baseline is non-function imports, multiple tables, and table bulk operations.
The final exception-handling proposal is supported:

- A thrown wasm exception is a native exception carrying its tag.
- `catch_all` cannot observe traps.
- C programs based on `setjmp`/`longjmp` convert and run; `mruby` is the covered app case.

Full WASI Preview 1 (`sock_*` out of scope), verified against the official `wasi-testsuite`.
The official support table: [`docs/support.md`](../support.md).

## Limits

- **Call depth is bounded by a counter.**
  Perl recursion grows on the heap and only stops at the OOM killer.
  So generated functions maintain an explicit depth counter (`$Rt::DEPTH`, limit `$Rt::LIMIT` = 100000).
  They trap with `call stack exhausted` past it.
  Raise `$Package::Rt::LIMIT` for legitimately deeper recursion (each frame costs heap).
- Float arithmetic corrects Perl's own operators.
  Those take an integer fast path that exceeds double precision and drops `-0.0`.
  Perl also dies on `x / 0.0` and `sqrt(-1)`.
  `+`, `-` and `*` are inlined as the native operation plus a `pack` round-trip.
  Rare results the inline form cannot finish fall back to the runtime helpers, such as `Rt::fadd`.
  Division and the rest stay helper calls.
- Numeric conventions are the shared masked-unsigned model.
  `use integer` appears only inside tightly-scoped runtime helpers.
- **File times are capped below nanosecond precision.**
  Core Perl's only sub-second time APIs (`Time::HiRes` `utime`/`stat`) pass NV seconds.
  That is ~400ns resolution for present-day times.
  Core Perl also has no `lutimes`/`utimensat`, so a symbolic link cannot carry its own times.
  So `path_filestat_set_times` without SYMLINK_FOLLOW sets the target's times.
  Both are attributed in the `wasi-testsuite` list (`crates/dewasm-backend-perl/tests/wasi_testsuite.rs`).
