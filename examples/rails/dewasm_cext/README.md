# `dewasm_cext`

A gem's C extension, compiled to wasm against a `ruby.h` of this directory's own.
dewasm converts it to Ruby, and the result is loaded in place of the `.so` file.
The gem's own Ruby and C code run unmodified.
This is a proof of concept: it covers what five gems use, about 200 functions of the Ruby C API.

| Path | What it is |
| --- | --- |
| `include/ruby.h`, `include/support.c` | the Ruby C API as wasm imports, and the part of it that needs no host |
| `lib/dewasm_cext.rb` | the host: the imports, implemented in Ruby over a handle table |
| `lib/dewasm_cext/recipes/<gem>.rb` | how a gem's extension is built: sources, flags, the `Init_*` symbol, the path the gem requires |
| `lib/dewasm_cext/builder.rb` | runs a recipe: fetch with a checksum, compile, convert, write the loader, stamp the result |
| `lib/dewasm_cext/gemfile.rb` | `dewasm_gem`, the Gemfile method |
| `bin/build-gem <gem> <version>` | the command-line form of `dewasm_gem`, into `vendor/dewasm/` under the working directory |
| `bin/compile-ext <name> <Module> <Init_symbol> <out dir> <c sources...>` | an extension that is no gem, compiled and converted the same way |
| `test/run.sh` | the checks against a small extension (`test/demo_ext.c`): strings, exceptions, `rb_protect`, blocks, typed data, handle collection |
| `test/smoke/<gem>.rb <gem directory>` | each gem's own API on its converted extension, callbacks included |

## In a Gemfile

```ruby
require_relative "dewasm_cext/lib/dewasm_cext/gemfile"

gem "sqlite3", ">= 2.1"

group :dewasm do
  dewasm_gem "sqlite3", "2.10.1"
end
```

`dewasm_gem` builds the gem at that version under `bundle install`, as a `path:` gem for Bundler.
The line replaces the `gem` line that names the same gem, or adds a gem no line names.
Outside `bundle install`, an out-of-date build is an error that names the command.
With the group excluded (`BUNDLE_WITHOUT=dewasm`), the line does nothing, and the native gem is used.

The build needs `wasi-sdk` with `WASI_SDK_PATH` set, and `make` and `patch` for `libxml2`.
`pygments.rb` also needs Wizer and the CPython library of `examples/apps/scripts/cpython-sdk.sh`.
`mise install` at the repository root provides `wasi-sdk` and Wizer.
The first `bundle install` takes a few minutes, most of it `libxml2`.

## How it works

`ruby.h` declares each `rb_*` function as an import from module `ruby`.
A `VALUE` is a 32-bit tagged word: a fixnum, one of the four constants, or a handle index.
The host keeps the handle table and defines the Ruby methods a `rb_define_method` call asks for.
It calls the C function through the module's exported function table.
A pointer into a Ruby String is a copy in linear memory.
The copy is written back when the C code returns or calls into Ruby.
A Ruby exception raised inside an import unwinds through the generated frames on its own.
The host saves `__stack_pointer` at each entry and restores it, since no epilogue runs.
Handles made during a call are collected once the outermost call returns.
One that a registered address or a wrapped struct's `dmark` marks stays strong.
Any other turns weak and stays valid while its object lives anywhere in Ruby, as in CRuby.

`pygments.rb` has no C extension; its boundary is the `python3` process it talks to over a pipe.
Its recipe builds a reactor around CPython (`recipes/pygments_rb/mentos_host.c`) with two exports.
`pyg_init` starts CPython and imports the gem's `mentos.py`; `pyg_mentos` answers one request.
A converted CPython takes more than 30 seconds to boot, so Wizer runs `pyg_init` at build time.
It writes the memory `pyg_init` leaves into the wasm module, and the converted Ruby starts from it.
The recipe writes `lib_dewasm/pygments/popen.rb` ahead of the gem's `lib` on its require paths.
That file loads the gem's `popen.rb` and points `Pygments::Popen#mentos` at the reactor.

## Adding a gem

A gem needs a recipe; `Builder.new` names the file to write.
A C API function the gem uses and `ruby.h` lacks fails at load with the missing import's name.
It is added as a declaration in `ruby.h` and a method in `lib/dewasm_cext.rb`.

## Limits

- Under YJIT the converted code runs 50 to 130 times slower than native.
  `bcrypt` at its default cost of 12 takes 19 seconds per hash.
- A callback into Ruby holds the instance's lock.
  Ruby code that waits for another thread inside such a callback deadlocks.
- WASI has no `iconv`, threads, `dlopen`, or shared memory.
  `libxml2` decodes UTF-8, UTF-16, ISO-8859-1, and ASCII documents itself.
  SQLite runs without loadable extensions.
  A database left in WAL mode by a native SQLite does not open.
