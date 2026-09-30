# Ruby backend

`--target ruby` (the default).
The most complete backend, and the host for the SQLite and CRuby goal examples.
Those go up to and including Rails running on the converted SQLite ([`examples/rails`](../../examples/rails/README.md)).

## Output shape

A single `.rb` file holds the generated module as a Ruby **class**.
The small runtime is bundled inside it under the relative name `Rt`.
Nesting the runtime in the class lets generated files load into one process without a name conflict.

In **library** mode, `--module-name` is required and is taken unchanged as the class name.
It is a Ruby constant path: `::`-separated segments, each matching `[A-Z][A-Za-z0-9_]*`.
Anything else is a conversion-time error, nothing is rewritten.
Ruby is the only backend whose grammar demands the leading capital.
A nested name (`Dewasm::Sqlite3`) defines its ancestors under an `unless defined?` guard.
So the file loads both on its own and beside code that already defined them.
A valid name can still match a constant MRI defines.
For example, `--module-name Ruby` conflicts with Ruby 4.0's built-in `Ruby` module.
It fails at load with "Ruby is not a class (TypeError)", so pick a free constant.
In **standalone** mode the class is always `Program` and `--module-name` is rejected.

## Requirements

`ruby` **3.4 or newer** on `PATH`: the runtime's linear memory is an `IO::Buffer`.
No gems are needed, so the output is self-contained.

## Running it

Standalone sets up WASI and runs `_start`:

```console
$ dewasm prog.wasm --target ruby --mode standalone -o prog.rb
$ ruby prog.rb --dir ./data::/data arg1 arg2
```

Standalone programs share one runtime interface: [`docs/standalone-interface.md`](../standalone-interface.md).
It covers `argv`, `--dir` preopens, environment, and exit/trap.

Library mode exposes the exports:

```ruby
require_relative "add"
inst = Add.new                 # Add.new(imports, preopens: {...}) if needed
inst.invoke("add", 2, 3)       # => 5
inst.memory                    # linear memory (read_string, iwl = i32 load, iws = i32 store, over #buffer)
```

`proc_exit` raises `<Class>::Rt::Exit` (with `#code`); rescue it around `invoke("_start")` in library mode.

## Capabilities

Full wasm core 1.0 plus the universal baseline, and **full WASI Preview 1 including the file system**.
Non-function imports, multiple tables, and table bulk operations are all supported.
The final exception-handling proposal is supported:

- A thrown wasm exception is a native exception carrying its tag.
- `catch_all` cannot observe traps.
- C programs based on `setjmp`/`longjmp` convert and run; `mruby` is the covered app case.

The official support table is [`docs/support.md`](../support.md).
The file system model is the `preopens:` provider argument over an `fd` table.
It has an accepted TOCTOU sandboxing limit with symbolic links.

## Providers and library usage

Any WASI import the embedder does not supply falls back to a bundled WASI implementation.
That implementation is built only if used; `--no-default-wasi` turns it off.
Override a single import with a callable, or replace a whole namespace with a *provider object*.
Its `import(name)` resolves functions, and its `attach(instance)` binds the memory before wasm runs:

```ruby
class MyWasi
  def import(name) = respond_to?(name) ? method(name) : nil
  def attach(instance) = @memory = instance.memory
  def fd_write(fd, iovs, iovs_len, out_ptr) = ...   # @memory.read_string(ptr, len) returns binary
end

inst = Prog.new({ "wasi_snapshot_preview1" => MyWasi.new })
inst.invoke("_start")
```

File system access is granted by preopening host directories:

```ruby
inst = Prog.new({}, preopens: { "/work" => "/path/on/host" })
```

[`docs/getting-started.md`](../getting-started.md#4-overriding-an-import-provider) walks through a single-import override.
It captures `fd_write`, and everything else falls back to the bundled WASI.

## Limits

- Maturity: this is the most exercised backend, but the whole project is early development.
  Treat generated output as tested, not battle-hardened.
- Numeric conventions: i32/i64 are masked-unsigned `Integer`s.
  Signed views appear only where an instruction needs them.
  So reading the output requires knowing this.
- `path_link` on a Ruby without Fiddle: a hard link to a *symbolic link itself* needs `linkat(2)`.
  Only Fiddle can reach `linkat(2)`.
  On macOS, `link(2)` follows symbolic links, so there that one request answers `ENOTSUP` instead.
  Every other hard link, and everything on Linux, is unaffected.
  CRuby has Fiddle, so this is about runtimes that do not, such as an ahead-of-time compiler's.
- Large modules produce large files.
  A large interpreter converts to hundreds of MB of Ruby (measured in [`docs/sizes/results.md`](../sizes/results.md)).
  Loading and running one costs proportional time and memory.
