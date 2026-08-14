# Perl backend

`--target perl`.
A plain `require`-able package covering wasm core 1.0 plus full WASI preview 1 (the fd-table/preopens model, `sock_*` excepted; see [docs/support.md](../support.md)).

## Output shape

A single `.pl` file: the generated module as a `package`, instances as blessed hashrefs (`Package->new(\%imports)`), with the runtime bundled in package namespaces under the generated package (`Package::Rt`, `Package::Rt::Memory`, and so on: perl package names are absolute, so the embedded runtime is prefixed rather than lexically nested).
Control flow lowers to perl's native labeled blocks and loops: every `br` is a direct `last Ln`/`next Ln`, with no flag-variable scheme.

In **library** mode the package name is `--module-name` (required in library mode) taken verbatim: `::`-separated segments each matching `[A-Za-z_][A-Za-z0-9_]*`, so `Dewasm::Sqlite3` nests as you would expect (and carries the runtime with it, `Dewasm::Sqlite3::Rt`).
Anything else is a conversion-time error, nothing is sanitized.
In **standalone** mode the package is always `Program` and `--module-name` is rejected.

## Requirements

`perl` **5.26 or newer** on `PATH`, built with 64-bit integers and IEEE doubles (`ivsize=8`, `nvsize=8`, which any stock perl on a 64-bit OS has).
No CPAN modules: the output uses only core modules (`POSIX`, `Config`, `Cwd`, `Errno`, `Fcntl`, `Time::HiRes`, `File::Basename`, `IO::Handle`).
The generated prelude verifies the integer/double sizes at load time and dies loudly otherwise.

## Running it

```console
$ dewasm prog.wasm --target perl --mode standalone -o prog.pl
$ perl prog.pl --dir .::/ input.txt
```

Standalone programs follow the shared runtime interface ([docs/standalone-interface.md](../standalone-interface.md)): a leading run of `--dir HOST::GUEST` flags mounts host directories at guest paths, the rest becomes the guest's argv, `proc_exit` maps to the process exit code, and a trap prints `trap: <message>` and exits 134.

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

Imports are a hashref of module name to either a hashref (name to value) or a provider object with a `wasm_import($name)` method (an optional `attach($instance)` is called after construction); another generated instance qualifies directly.
Function imports are coderefs; globals/tables/memories are the runtime's boxed objects (`global_export`/`table_export` hand them out).
An explicit WASI import overrides the bundled runtime per function; unhandled ones fall back to it, constructed lazily.

## Capabilities

Full wasm core 1.0 plus the universal baseline: non-function imports, multiple tables, and table bulk ops are supported.
The final exception-handling proposal is supported: a thrown wasm exception is a native exception carrying its tag, catch_all cannot observe traps, and setjmp/longjmp-based C programs (mruby is the covered app case) convert and run.
Full WASI preview 1 (`sock_*` out of scope), verified against the official wasi-testsuite.
Authoritative matrix: [docs/support.md](../support.md).

## Caveats

- **Call depth is bounded by a counter.**
  Perl recursion grows on the heap and only stops at the OOM killer, so generated functions maintain an explicit depth counter (`$Rt::DEPTH`, limit `$Rt::LIMIT` = 100000) and trap with `call stack exhausted` past it.
  Raise `$Package::Rt::LIMIT` for legitimately deeper recursion (each frame costs heap).
- Float arithmetic goes through helpers (`Rt::fadd` and friends): perl's own operators take an integer fast path that exceeds double precision and drops `-0.0`, and perl dies on `x / 0.0` and `sqrt(-1)`.
- Numeric conventions are the shared masked-unsigned model; `use integer` appears only inside tightly-scoped runtime helpers.
- **File timestamps are capped below nanosecond precision.**
  Core perl's only sub-second time APIs (`Time::HiRes` utime/stat) pass NV seconds (~400ns resolution at the current epoch), and there is no `lutimes`/`utimensat`, so a symlink cannot carry its own times (`path_filestat_set_times` without SYMLINK_FOLLOW sets the target's).
  Both are attributed in the wasi-testsuite list (`crates/dewasm-backend-perl/tests/wasi_testsuite.rs`).
