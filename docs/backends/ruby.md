# Ruby backend

`--target ruby` (the default). The most complete backend and the host for the
SQLite and CRuby north-star demos.

## Output shape

A single `.rb` file: the generated module as a Ruby **class** named after the
input file stem (override with `--module-name`), with the lightweight runtime
bundled inside it under the relative name `Rt`. Nesting the runtime in the
class lets several generated files coexist in one process without collision.
See [ADR-4](../adr/4-ruby-backend-lowering.md) for the lowering conventions and
[ADR-6](../adr/6-runtime-units.md) for the runtime-unit model.

## Requirements

`ruby` on `PATH`. No specific version is required and no gems are needed — the
output is self-contained.

## Running it

Standalone wires up WASI and runs `_start`:

```console
$ dewasm prog.wasm --target ruby --mode standalone -o prog.rb
$ ruby prog.rb arg1 arg2
```

Library mode exposes the exports:

```ruby
require_relative "add"
inst = Add.new                 # Add.new(imports, preopens: {...}) if needed
inst.invoke("add", 2, 3)       # => 5
inst.memory                    # linear memory (bytes is a binary String)
```

`proc_exit` raises `<Class>::Rt::Exit` (with `#code`); rescue it around
`invoke("_start")` in library mode.

## Capabilities

Full wasm core 1.0 plus the universal baseline, and **full WASI preview 1
including the filesystem** — the only backend with filesystem support besides
none. Non-function imports, multiple tables, and table bulk ops are all
supported. The authoritative matrix is [docs/support.md](../support.md); the
filesystem model (the `preopens:` provider kwarg, the fd-table, the accepted
TOCTOU/symlink sandboxing caveat) is [ADR-14](../adr/14-ruby-wasi-filesystem.md).

## Providers and library usage

Any WASI import the embedder does not supply falls back to a bundled WASI
implementation (built only if used; disable with `--no-default-wasi`). Override
a single import with a callable, or replace a whole namespace with a *provider
object* — `import(name)` resolves functions and `attach(instance)` binds the
memory before wasm runs ([ADR-7](../adr/7-import-providers.md)):

```ruby
class MyWasi
  def import(name) = respond_to?(name) ? method(name) : nil
  def attach(instance) = @memory = instance.memory
  def fd_write(fd, iovs, iovs_len, out_ptr) = ...   # @memory.bytes is binary
end

inst = Prog.new({ "wasi_snapshot_preview1" => MyWasi.new })
inst.invoke("_start")
```

Filesystem access is granted by preopening host directories:

```ruby
inst = Prog.new({}, preopens: { "/work" => "/path/on/host" })
```

A single-import override (capturing `fd_write`, everything else falling back to
the bundled WASI) is walked through in
[docs/getting-started.md](../getting-started.md#4-overriding-an-import-provider).

## Caveats

- Maturity: this is the most exercised backend, but the whole project is early
  development — treat generated output as tested, not battle-hardened.
- Numeric conventions ([ADR-2](../adr/2-numeric-semantics.md)): i32/i64 are
  masked-unsigned `Integer`s; signed views appear only where an instruction
  needs them, so reading the output requires knowing this.
- Large modules produce large files (CRuby is ~335 MB of Ruby); loading and
  running them costs proportional time and memory.
